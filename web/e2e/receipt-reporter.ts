import { mkdirSync, writeFileSync } from 'node:fs';
import { basename, resolve } from 'node:path';
import type { FullResult, Reporter, TestCase, TestResult } from '@playwright/test/reporter';

export default class ReceiptReporter implements Reporter {
  private checks: { browser: string; check: string; status: string; duration_ms: number; failure?: string }[] = [];
  onTestEnd(test: TestCase, result: TestResult) {
    // Deliberately exclude steps, request/response data, stdout, attachments and
    // raw errors: those can contain session headers or credential-bearing calls.
    const frame = result.error?.stack?.match(/(?:^|[/\\])(collaboration\.browser\.ts|fixtures\.ts):(\d+):\d+/m);
    const failure = frame ? `${frame[1]}:${frame[2]}` : `${basename(result.error?.location?.file ?? test.location.file)}:${result.error?.location?.line ?? test.location.line}`;
    const check = { browser: test.parent.project()?.name ?? 'unknown', check: test.title,
      status: result.status, duration_ms: result.duration,
      ...(result.status !== 'passed' ? { failure } : {}) };
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
