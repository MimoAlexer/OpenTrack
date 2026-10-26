import { mkdirSync, writeFileSync } from 'node:fs';
import { basename, resolve } from 'node:path';
import type { FullResult, Reporter, TestCase, TestResult } from '@playwright/test/reporter';

export type SyntheticAuthFailure = { operation: 'invitation_accept' | 'owner_authentication'; status: number };

export function parseSyntheticAuthFailure(description: unknown): SyntheticAuthFailure | undefined {
  if (typeof description !== 'string' || description.length > 128) return;
  let data: unknown;
  try { data = JSON.parse(description); } catch { return; }
  if (!data || typeof data !== 'object' || Array.isArray(data) || Object.keys(data).length !== 2
      || !Object.hasOwn(data, 'operation') || !Object.hasOwn(data, 'status')) return;
  const value = data as Record<string, unknown>;
  if (!['invitation_accept', 'owner_authentication'].includes(value.operation as string)
      || typeof value.status !== 'number' || !Number.isInteger(value.status)
      || value.status < 100 || value.status > 599) return;
  return { operation: value.operation as SyntheticAuthFailure['operation'], status: value.status };
}

function ownerAuthFailure(message: unknown): SyntheticAuthFailure | undefined {
  const match = typeof message === 'string' && message.length <= 128
    ? message.match(/^(?:Error: )?Synthetic owner authentication failed: HTTP ([1-5][0-9]{2})$/) : null;
  if (match && match[0] === message) return { operation: 'owner_authentication', status: Number(match[1]) };
}

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

export type ProjectLeadReadiness = {
  request_state: 'unobserved' | 'pending' | 'completed' | 'failed';
  status: number | null; elapsed_ms: number; q_scalar_length: number | null;
  returned_count: number | null; has_more: boolean | null; self_assignable: boolean | null;
  fixture_owner_in_page: boolean | null; expected_display_name_match: boolean | null;
  current_user_id_match: boolean | null;
  option_observation: 'unobserved' | 'before_click' | 'after_failed_click';
  owner_option_match_count: number | null; you_marker_option_count: number | null;
  owner_option_visible: boolean | null; owner_option_enabled: boolean | null;
  click_failure: 'pointer_intercepted' | 'outside_viewport' | 'not_stable' | 'detached'
    | 'not_visible' | 'not_enabled' | 'strict_match' | 'unknown' | null;
  center_hit_self_or_descendant: boolean | null; center_inside_dialog: boolean | null;
  center_hit_another_option: boolean | null;
  center_inside_viewport: boolean | null; matching_dialog_count: number | null;
  option_rect: ProjectLeadRect | null; popover_rect: ProjectLeadRect | null;
  wrapper_rect: ProjectLeadRect | null; trigger_rect: ProjectLeadRect | null; dialog_rect: ProjectLeadRect | null;
  owner_option_content_rect: ProjectLeadRect | null; hit_option_rect: ProjectLeadRect | null;
  hit_option_content_rect: ProjectLeadRect | null;
};
type ProjectLeadRect = { x: number; y: number; width: number; height: number };

export function parseProjectLeadReadiness(description: unknown): ProjectLeadReadiness | undefined {
  if (typeof description !== 'string' || description.length > 2048) return;
  let data: unknown;
  try { data = JSON.parse(description); } catch { return; }
  const keys = ['request_state', 'status', 'elapsed_ms', 'q_scalar_length', 'returned_count',
    'has_more', 'self_assignable', 'fixture_owner_in_page', 'expected_display_name_match',
    'current_user_id_match', 'option_observation', 'owner_option_match_count',
    'you_marker_option_count', 'owner_option_visible', 'owner_option_enabled', 'click_failure',
    'center_hit_self_or_descendant', 'center_inside_dialog', 'center_inside_viewport',
    'matching_dialog_count', 'option_rect', 'popover_rect', 'wrapper_rect', 'trigger_rect', 'dialog_rect',
    'center_hit_another_option', 'owner_option_content_rect', 'hit_option_rect', 'hit_option_content_rect'];
  if (!data || typeof data !== 'object' || Array.isArray(data)
      || Object.keys(data).length !== keys.length || !keys.every(key => Object.hasOwn(data, key))) return;
  const value = data as Record<string, unknown>;
  const bounded = (item: unknown, max: number) => typeof item === 'number'
    && Number.isInteger(item) && item >= 0 && item <= max;
  if (!['unobserved', 'pending', 'completed', 'failed'].includes(value.request_state as string)
      || !['unobserved', 'before_click', 'after_failed_click'].includes(value.option_observation as string)
      || !bounded(value.elapsed_ms, 120000)
      || !(value.status === null || bounded(value.status, 599) && (value.status as number) >= 100)
      || !(value.q_scalar_length === null || bounded(value.q_scalar_length, 100))
      || !(value.returned_count === null || bounded(value.returned_count, 20))
      || !(value.owner_option_match_count === null || bounded(value.owner_option_match_count, 22))
      || !(value.you_marker_option_count === null || bounded(value.you_marker_option_count, 1))
      || !(value.matching_dialog_count === null || bounded(value.matching_dialog_count, 4))
      || !(value.click_failure === null || ['pointer_intercepted', 'outside_viewport', 'not_stable',
        'detached', 'not_visible', 'not_enabled', 'strict_match', 'unknown'].includes(value.click_failure as string))) return;
  for (const key of ['has_more', 'self_assignable', 'fixture_owner_in_page',
    'expected_display_name_match', 'current_user_id_match', 'owner_option_visible', 'owner_option_enabled',
    'center_hit_self_or_descendant', 'center_hit_another_option', 'center_inside_dialog', 'center_inside_viewport']) {
    if (value[key] !== null && typeof value[key] !== 'boolean') return;
  }
  const rects = ['option_rect', 'popover_rect', 'wrapper_rect', 'trigger_rect', 'dialog_rect',
    'owner_option_content_rect', 'hit_option_rect', 'hit_option_content_rect'];
  for (const key of rects) {
    const rect = value[key];
    if (rect === null) continue;
    if (!rect || typeof rect !== 'object' || Array.isArray(rect)
        || Object.keys(rect).length !== 4 || !['x', 'y', 'width', 'height'].every(field => Object.hasOwn(rect, field))) return;
    const fields = rect as Record<string, unknown>;
    for (const field of ['x', 'y', 'width', 'height']) {
      const number = fields[field];
      if (typeof number !== 'number' || !Number.isInteger(number) || number > 16384
          || number < (field === 'x' || field === 'y' ? -16384 : 0)) return;
    }
  }
  const page = ['returned_count', 'has_more', 'self_assignable', 'fixture_owner_in_page'];
  if (value.request_state === 'unobserved' && (value.status !== null || value.elapsed_ms !== 0
        || value.q_scalar_length !== null || page.some(key => value[key] !== null))
      || value.request_state !== 'unobserved' && value.q_scalar_length === null
      || value.request_state === 'completed' && value.status === null
      || page.some(key => value[key] !== null) && (value.status !== 200 || page.some(key => value[key] === null))
      || value.option_observation === 'unobserved' && ['owner_option_match_count', 'you_marker_option_count',
        'owner_option_visible', 'owner_option_enabled', 'matching_dialog_count', 'center_hit_self_or_descendant',
        'center_hit_another_option', 'center_inside_dialog', 'center_inside_viewport', ...rects].some(key => value[key] !== null)
      || value.option_observation !== 'unobserved' && (value.owner_option_match_count === null || value.you_marker_option_count === null)
      || value.owner_option_match_count !== 1 && ['owner_option_visible', 'owner_option_enabled',
        'center_hit_self_or_descendant', 'center_hit_another_option', 'center_inside_dialog', 'center_inside_viewport', ...rects].some(key => value[key] !== null)) return;
  return data as ProjectLeadReadiness;
}

export default class ReceiptReporter implements Reporter {
  private checks: { browser: string; check: string; status: string; duration_ms: number; failure?: string; saved_readiness?: SavedReadiness; project_lead_readiness?: ProjectLeadReadiness; auth_failure?: SyntheticAuthFailure }[] = [];
  onTestEnd(test: TestCase, result: TestResult) {
    // Deliberately exclude steps, request/response data, stdout, attachments and
    // raw errors: those can contain session headers or credential-bearing calls.
    const frame = result.error?.stack?.match(/(?:^|[/\\])(collaboration\.browser\.ts|projects\.browser\.ts|fixtures\.ts):(\d+):\d+/m);
    const failure = frame ? `${frame[1]}:${frame[2]}` : `${basename(result.error?.location?.file ?? test.location.file)}:${result.error?.location?.line ?? test.location.line}`;
    const annotations = result.annotations.filter(item => item.type === 'saved-readiness');
    const readiness = result.status !== 'passed' && annotations.length === 1 ? parseSavedReadiness(annotations[0].description) : undefined;
    const projectAnnotations = result.annotations.filter(item => item.type === 'project-lead-readiness');
    const projectLead = projectAnnotations.length === 1 ? parseProjectLeadReadiness(projectAnnotations[0].description) : undefined;
    const authAnnotations = result.annotations.filter(item => item.type === 'synthetic-auth-failure');
    let authFailure: SyntheticAuthFailure | undefined;
    if (result.status !== 'passed' && result.status !== 'skipped') {
      if (authAnnotations.length === 1) authFailure = parseSyntheticAuthFailure(authAnnotations[0].description);
      else if (authAnnotations.length === 0 && failure.startsWith('fixtures.ts:')) authFailure = ownerAuthFailure(result.error?.message);
    }
    const check = { browser: test.parent.project()?.name ?? 'unknown', check: test.title,
      status: result.status, duration_ms: result.duration,
      ...(result.status !== 'passed' ? { failure } : {}), ...(readiness ? { saved_readiness: readiness } : {}),
      ...(projectLead ? { project_lead_readiness: projectLead } : {}), ...(authFailure ? { auth_failure: authFailure } : {}) };
    this.checks.push(check);
    console.log(`${check.status.toUpperCase()} [${check.browser}] ${check.check}${check.failure ? ` (${check.failure})` : ''}`);
    if (projectLead) console.log(`PROJECT_LEAD_READINESS [${check.browser}] ${JSON.stringify(projectLead)}`);
    if (authFailure) console.log(`SYNTHETIC_AUTH_FAILURE [${check.browser}] ${JSON.stringify(authFailure)}`);
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
