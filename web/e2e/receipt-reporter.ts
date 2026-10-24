import { mkdirSync, writeFileSync } from 'node:fs';
import { basename, resolve } from 'node:path';
import type { FullResult, Reporter, TestCase, TestResult } from '@playwright/test/reporter';

export type SavedReadiness = Record<'setup_status' | 'session' | 'bootstrap' | 'access', {
  state: 'unobserved' | 'pending' | 'completed' | 'failed'; status: number | null; elapsed_ms: number;
}>;

export function parseSavedReadiness(description: unknown): SavedReadiness | undefined {
  if (typeof description !== 'string' || description.length > 2048) return;
  let data: unknown;
  try { data = JSON.parse(description); } catch { return; }
  const exact = (value: unknown, keys: string[]): value is Record<string, unknown> =>
    !!value && typeof value === 'object' && !Array.isArray(value)
      && Object.keys(value).length === keys.length && keys.every(key => Object.hasOwn(value, key));
  const routes = ['setup_status', 'session', 'bootstrap', 'access'];
  if (!exact(data, routes)) return;
  for (const route of routes) {
    const item = data[route];
    if (!exact(item, ['state', 'status', 'elapsed_ms'])
        || typeof item.state !== 'string' || !['unobserved', 'pending', 'completed', 'failed'].includes(item.state)
        || typeof item.elapsed_ms !== 'number' || !Number.isInteger(item.elapsed_ms)
        || item.elapsed_ms < 0 || item.elapsed_ms > 120000
        || !(item.status === null || typeof item.status === 'number'
          && Number.isInteger(item.status) && item.status >= 100 && item.status <= 599)
        || item.state === 'completed' && item.status === null
        || item.state === 'unobserved' && (item.status !== null || item.elapsed_ms !== 0)) return;
  }
  return data as SavedReadiness;
}

export default class ReceiptReporter implements Reporter {
  private checks: { browser: string; check: string; status: string; duration_ms: number; failure?: string; saved_readiness?: SavedReadiness }[] = [];
  onTestEnd(test: TestCase, result: TestResult) {
    // Deliberately exclude steps, request/response data, stdout, attachments and
    // raw errors: those can contain session headers or credential-bearing calls.
    const frame = result.error?.stack?.match(/(?:^|[/\\])(collaboration\.browser\.ts|projects\.browser\.ts|fixtures\.ts):(\d+):\d+/m);
    const failure = frame ? `${frame[1]}:${frame[2]}` : `${basename(result.error?.location?.file ?? test.location.file)}:${result.error?.location?.line ?? test.location.line}`;
    const annotations = result.annotations.filter(item => item.type === 'saved-readiness');
    const readiness = result.status !== 'passed' && annotations.length === 1 ? parseSavedReadiness(annotations[0].description) : undefined;
    const check = { browser: test.parent.project()?.name ?? 'unknown', check: test.title,
      status: result.status, duration_ms: result.duration,
      ...(result.status !== 'passed' ? { failure } : {}), ...(readiness ? { saved_readiness: readiness } : {}) };
    this.checks.push(check);
    console.log(`${check.status.toUpperCase()} [${check.browser}] ${check.check}${check.failure ? ` (${check.failure})` : ''}`);
  }
  onError() { console.error('Browser runner error; raw diagnostic details withheld to protect authentication state.'); }
  onEnd(result: FullResult) {
    const directory = resolve('../work/browser-acceptance');
    mkdirSync(directory, { recursive: true });
    const receipt = { status: result.status, checks: this.checks,
      limitations: ['Synthetic composition events do not qualify a real OS IME.',
        'Native desktop, assistive technology, physical keyboard layouts and screenshot review remain separate gates.'],
      artifacts: 'No auth state, trace, video, screenshot, request headers or HTTP bodies are published.' };
    writeFileSync(resolve(directory, 'receipt.json'), `${JSON.stringify(receipt, null, 2)}\n`);
    console.log(`Browser acceptance: ${result.status}; ${this.checks.filter(item => item.status === 'passed').length}/${this.checks.length} checks passed.`);
  }
}
