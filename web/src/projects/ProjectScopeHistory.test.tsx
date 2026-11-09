// @vitest-environment jsdom
import { act, StrictMode, type ReactNode } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { hasUnsavedChanges } from '../router';
import type { Session } from '../types';
import { ProjectScopeHistory, type ProjectScopeSnapshot, type ScopeBaseline, type ScopeEvent } from './ProjectScopeHistory';
import type { Project } from './types';

const session: Session = { audience: 'web', user: { id: 'owner', email: 'owner@example.test', display_name: 'Example Owner' }, csrf_token: 'csrf', workspaces: [{ id: 'workspace', name: 'Example Workspace', slug: 'example', role: 'owner' }] };
const project: Project = { id: 'project', workspace_id: 'workspace', team_id: 'team', name: 'Planning', summary: '', state: 'active', health: 'on_track', lead_id: null, lead: null, start_date: null, target_date: null, version: '1', etag: '"project:1"', created_at: '2026-11-08T10:00:00Z', updated_at: '2026-11-08T10:00:00Z', archived_at: null, can_edit: true, can_manage: true };
const baseline: ScopeBaseline = { id: 'baseline-one', label: 'Agreed launch', reason: 'Scope approved at planning.', author_id: 'owner', author_name: 'Example Owner', created_at: '2026-11-08T10:00:00Z' };
const issue = { id: 'issue-one', key: 'ENG-1', title: 'Build the editor' };
const event: ScopeEvent = { id: 'event-one', action: 'added', reason: 'Needed for launch.', issue, actor_id: 'owner', actor_name: 'Example Owner', created_at: '2026-11-08T11:00:00Z' };
function snapshot(overrides: Partial<ProjectScopeSnapshot> = {}): ProjectScopeSnapshot {
  return { scope_etag: '"scope-one"', history_started_at: '2026-11-08T09:00:00Z', legacy_history_unknown: false, can_agree: true, capture_limit: 1000, items_has_more: false, items_next_cursor: null, baselines_has_more: false, baselines_next_cursor: null, current: [issue], baselines: [baseline], baseline, comparison: { unchanged: 1, added: [], removed: [], postponed: [] }, events: { data: [event], has_more: false, next_cursor: null }, ...overrides };
}
const json = (value: unknown, status = 200) => new Response(JSON.stringify(value), { status });
const problem = (status: number, detail = 'Request failed.') => json({ code: status === 409 ? 'version_conflict' : 'request_failed', detail }, status);
let container: HTMLDivElement; let root: Root;
beforeEach(() => { vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true); container = document.createElement('div'); document.body.append(container); root = createRoot(container); });
afterEach(async () => { await act(async () => root.unmount()); container.remove(); vi.restoreAllMocks(); vi.unstubAllGlobals(); });
async function render(node: ReactNode) { await act(async () => root.render(node)); }
function view(overrides: Partial<Parameters<typeof ProjectScopeHistory>[0]> = {}) { return <ProjectScopeHistory project={project} session={session} refresh={0} readOnly={false} denied={() => {}} {...overrides}/>; }
function button(label: string) { const found = [...container.querySelectorAll<HTMLButtonElement>('button')].find(item => item.textContent === label); if (!found) throw new Error(`Missing button: ${label}`); return found; }
async function click(label: string) { await act(async () => button(label).click()); }
async function input(name: string, value: string) {
  const element = container.querySelector<HTMLInputElement | HTMLTextAreaElement | HTMLSelectElement>(`[name="${name}"]`)!;
  const prototype = element instanceof HTMLSelectElement ? HTMLSelectElement.prototype : element instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
  await act(async () => { Object.getOwnPropertyDescriptor(prototype, 'value')!.set!.call(element, value); element.dispatchEvent(new Event(element instanceof HTMLSelectElement ? 'change' : 'input', { bubbles: true })); });
}
async function agree() { await act(async () => container.querySelector<HTMLInputElement>('.scope-agreement input')!.click()); }
async function submit(label: string) { await act(async () => container.querySelector(`form[aria-label="${label}"]`)!.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }))); }
async function baselineDraft() { await input('scope-baseline-name', ' Release plan '); await input('scope-baseline-reason', ' Reviewed with the team. '); await agree(); }

describe('project scope history', () => {
  it('shows attributed changes, comparison reasons and an explicit unknown legacy boundary', async () => {
    vi.stubGlobal('fetch', vi.fn(() => Promise.resolve(json(snapshot({ legacy_history_unknown: true, comparison: { unchanged: 0, added: [issue], removed: [{ id: 'removed', key: 'ENG-2', title: 'Old import', reason: 'No longer required.' }], postponed: [{ id: 'later', key: 'ENG-3', title: 'Mobile view', reason: 'Waiting for research.' }] } })))));
    await render(view());
    expect(container.textContent).toContain('Earlier scope changes were not recorded.');
    expect(container.textContent).toContain('Scope approved at planning.');
    expect(container.textContent).toContain('Added since agreement (1)');
    expect(container.textContent).toContain('No longer required.');
    expect(container.textContent).toContain('Waiting for research.');
    expect(container.textContent).toContain('Needed for launch.');
    expect(container.textContent).toContain('Deleted issues and inaccessible work are omitted from lists and counts.');
    expect(container.querySelector('a[href="/w/workspace/issues/issue-one"]')).not.toBeNull();
  });

  it('renders loading and retryable empty/read failures without inventing a baseline', async () => {
    let finish!: (response: Response) => void;
    const fetch = vi.fn(() => new Promise<Response>(resolve => { finish = resolve; })); vi.stubGlobal('fetch', fetch);
    await render(view()); expect(container.textContent).toContain('Loading scope history');
    await act(async () => finish(problem(503, 'Temporarily unavailable.')));
    expect(container.textContent).toContain('Temporarily unavailable.'); expect(container.querySelector('form')).toBeNull();
    fetch.mockImplementation(() => Promise.resolve(json(snapshot({ baseline: null, baselines: [], comparison: null, current: [], events: { data: [], has_more: false, next_cursor: null } }))));
    await click('Try again'); expect(container.textContent).toContain('No baseline yet'); expect(container.textContent).toContain('No recorded changes in this view.');
  });

  it.each([{ readOnly: true }, { project: { ...project, can_edit: false } }, { project: { ...project, archived_at: '2026-11-08T12:00:00Z' } }])('keeps readonly history available without mutation forms (%j)', async overrides => {
    vi.stubGlobal('fetch', vi.fn(() => Promise.resolve(json(snapshot())))); await render(view(overrides));
    expect(container.textContent).toContain('Agreed launch'); expect(container.querySelector('form')).toBeNull();
  });

  it('uses server baseline authority separately from contributor editing', async () => {
    vi.stubGlobal('fetch', vi.fn(() => Promise.resolve(json(snapshot({ can_agree: false }))))); await render(view());
    expect(container.querySelector('form[aria-label="Set scope baseline"]')).toBeNull();
    expect(container.querySelector('form[aria-label="Record scope change"]')).not.toBeNull();
  });

  it('records a baseline only after explicit agreement and clears the submitted draft', async () => {
    const changed = vi.fn();
    const fetch = vi.fn((_url: string, options: RequestInit) => Promise.resolve(json(options.method === 'POST' ? snapshot({ baseline: { ...baseline, id: 'new-plan', label: 'Release plan' }, baselines: [{ ...baseline, id: 'new-plan', label: 'Release plan' }] }) : snapshot()))); vi.stubGlobal('fetch', fetch);
    await render(view({ changed })); await input('scope-baseline-name', ' Release plan '); await input('scope-baseline-reason', ' Reviewed with the team. ');
    await submit('Set scope baseline'); expect(fetch.mock.calls.filter(([, options]) => options.method === 'POST')).toHaveLength(0);
    await agree(); await submit('Set scope baseline');
    const [url, options] = fetch.mock.calls.find(([, options]) => options.method === 'POST')!;
    expect(url).toBe('/api/v1/workspaces/workspace/teams/team/projects/project/scope-baselines');
    expect(JSON.parse(options.body as string)).toEqual({ label: 'Release plan', reason: 'Reviewed with the team.' });
    expect(new Headers(options.headers).get('If-Match')).toBe('"scope-one"'); expect(new Headers(options.headers).get('Idempotency-Key')).toBeTruthy();
    expect(container.textContent).toContain('Baseline set.'); expect(container.querySelector<HTMLInputElement>('[name="scope-baseline-name"]')!.value).toBe(''); expect(hasUnsavedChanges()).toBe(false);
    expect(changed).toHaveBeenCalledTimes(1);
  });

  it('freezes exact body, version and idempotency key during uncertain replay', async () => {
    let writes = 0;
    const fetch = vi.fn((_url: string, options: RequestInit) => {
      if (options.method === 'POST' && ++writes === 1) return Promise.reject(new TypeError('Connection lost'));
      return Promise.resolve(json(snapshot()));
    }); vi.stubGlobal('fetch', fetch);
    await render(view()); await baselineDraft(); await submit('Set scope baseline');
    expect(container.textContent).toContain('The response was interrupted.'); expect(container.querySelector<HTMLInputElement>('[name="scope-baseline-name"]')!.disabled).toBe(true);
    expect(container.querySelector<HTMLSelectElement>('[name="scope-baseline"]')!.disabled).toBe(true);
    expect(hasUnsavedChanges()).toBe(true); await click('Retry safely');
    const posts = fetch.mock.calls.filter(([, options]) => options.method === 'POST'); expect(posts).toHaveLength(2);
    expect(posts[1][1].body).toBe(posts[0][1].body);
    expect(new Headers(posts[1][1].headers).get('Idempotency-Key')).toBe(new Headers(posts[0][1].headers).get('Idempotency-Key'));
    expect(new Headers(posts[1][1].headers).get('If-Match')).toBe(new Headers(posts[0][1].headers).get('If-Match'));
  });

  it('reloads stale scope, preserves the draft and requires review before a new submission', async () => {
    let writes = 0; let reads = 0;
    const fetch = vi.fn((_url: string, options: RequestInit) => options.method === 'POST' ? Promise.resolve(++writes === 1 ? problem(409, 'Scope changed.') : json(snapshot())) : Promise.resolve(json(snapshot({ scope_etag: ++reads === 1 ? '"scope-one"' : '"scope-two"' })))); vi.stubGlobal('fetch', fetch);
    await render(view()); await baselineDraft(); await submit('Set scope baseline');
    expect(container.textContent).toContain('Review the refreshed comparison');
    expect(container.querySelector<HTMLInputElement>('[name="scope-baseline-name"]')!.value).toBe(' Release plan ');
    await submit('Set scope baseline'); expect(writes).toBe(1);
    await click('I reviewed the refreshed scope'); await agree(); await submit('Set scope baseline');
    const posts = fetch.mock.calls.filter(([, options]) => options.method === 'POST'); expect(posts).toHaveLength(2);
    expect(new Headers(posts[1][1].headers).get('If-Match')).toBe('"scope-two"');
    expect(new Headers(posts[1][1].headers).get('Idempotency-Key')).not.toBe(new Headers(posts[0][1].headers).get('Idempotency-Key'));
  });

  it.each(['unchanged', 'changed', 'denied'] as const)('binds checked baseline agreement to the confirmed scope through realtime refresh: %s', async outcome => {
    let finish!: (response: Response) => void; let refreshing = false;
    const denied = vi.fn();
    const fetch = vi.fn((_url: string, options: RequestInit) => options.method === 'POST'
      ? Promise.resolve(json(snapshot()))
      : refreshing ? new Promise<Response>(resolve => { finish = resolve; }) : Promise.resolve(json(snapshot())));
    vi.stubGlobal('fetch', fetch);
    await render(view({ denied })); await baselineDraft();
    refreshing = true; await render(view({ denied, refresh: 1 }));
    expect(button('Set baseline').disabled).toBe(true);
    await act(async () => finish(outcome === 'denied' ? problem(403, 'Access changed.') : json(snapshot({ scope_etag: outcome === 'changed' ? '"scope-two"' : '"scope-one"' }))));
    if (outcome === 'denied') {
      expect(denied).toHaveBeenCalledTimes(1); expect(container.querySelector('form')).toBeNull();
    } else {
      expect(container.querySelector<HTMLInputElement>('.scope-agreement input')!.checked).toBe(outcome === 'unchanged');
      expect(button('Set baseline').disabled).toBe(outcome !== 'unchanged');
      if (outcome === 'unchanged') {
        await submit('Set scope baseline');
        const posts = fetch.mock.calls.filter(([, options]) => options.method === 'POST');
        expect(posts).toHaveLength(1);
        expect(new Headers(posts[0][1].headers).get('If-Match')).toBe('"scope-one"');
      }
    }
  });

  it('requires fresh agreement when the comparison context changes even with the same scope validator', async () => {
    const alternative = { ...baseline, id: 'baseline-two', label: 'Another comparison' };
    vi.stubGlobal('fetch', vi.fn(() => Promise.resolve(json(snapshot({ baselines: [baseline, alternative] })))));
    await render(view()); await baselineDraft();
    await act(async () => {
      const selector = container.querySelector<HTMLSelectElement>('[name="scope-baseline"]')!;
      selector.value = alternative.id; selector.dispatchEvent(new Event('change', { bubbles: true }));
    });
    expect(container.querySelector<HTMLInputElement>('.scope-agreement input')!.checked).toBe(false);
    expect(button('Set baseline').disabled).toBe(true);
    expect(container.querySelector<HTMLInputElement>('[name="scope-baseline-name"]')!.value).toBe(' Release plan ');
  });

  it('requires fresh agreement after a refresh loads changed scope without discarding the draft', async () => {
    let reads = 0;
    const fetch = vi.fn((_url: string, options: RequestInit) => Promise.resolve(json(snapshot({ scope_etag: options.method === 'POST' || ++reads > 1 ? '"scope-two"' : '"scope-one"' })))); vi.stubGlobal('fetch', fetch);
    await render(view()); await baselineDraft();
    expect(container.querySelector<HTMLInputElement>('.scope-agreement input')!.checked).toBe(true);
    await click('Refresh scope');
    expect(container.querySelector<HTMLInputElement>('.scope-agreement input')!.checked).toBe(false);
    expect(container.querySelector<HTMLInputElement>('[name="scope-baseline-name"]')!.value).toBe(' Release plan ');
    expect(container.querySelector<HTMLTextAreaElement>('[name="scope-baseline-reason"]')!.value).toBe(' Reviewed with the team. ');
    await submit('Set scope baseline');
    expect(fetch.mock.calls.filter(([, options]) => options.method === 'POST')).toHaveLength(0);
    await agree(); await submit('Set scope baseline');
    const posts = fetch.mock.calls.filter(([, options]) => options.method === 'POST');
    expect(posts).toHaveLength(1);
    expect(new Headers(posts[0][1].headers).get('If-Match')).toBe('"scope-two"');
  });

  it('preserves the original uncertain request even when a refresh clears agreement', async () => {
    let writes = 0; let reads = 0;
    const fetch = vi.fn((_url: string, options: RequestInit) => {
      if (options.method === 'POST') return ++writes === 1 ? Promise.reject(new TypeError('Connection lost')) : Promise.resolve(json(snapshot()));
      return Promise.resolve(json(snapshot({ scope_etag: ++reads === 1 ? '"scope-one"' : '"scope-two"' })));
    }); vi.stubGlobal('fetch', fetch);
    await render(view()); await baselineDraft(); await submit('Set scope baseline');
    await render(view({ refresh: 1 }));
    expect(container.querySelector<HTMLInputElement>('.scope-agreement input')!.checked).toBe(false);
    expect(container.querySelector<HTMLInputElement>('[name="scope-baseline-name"]')!.disabled).toBe(true);
    await click('Retry safely');
    const posts = fetch.mock.calls.filter(([, options]) => options.method === 'POST');
    expect(posts).toHaveLength(2); expect(posts[1][1].body).toBe(posts[0][1].body);
    expect(new Headers(posts[1][1].headers).get('If-Match')).toBe('"scope-one"');
    expect(new Headers(posts[1][1].headers).get('Idempotency-Key')).toBe(new Headers(posts[0][1].headers).get('Idempotency-Key'));
  });

  it('records explicit postponement with its reason and retains ordinary rejected drafts', async () => {
    let reject = true;
    const fetch = vi.fn((_url: string, options: RequestInit) => Promise.resolve(options.method === 'POST' && reject ? problem(422, 'Please revise the reason.') : json(snapshot()))); vi.stubGlobal('fetch', fetch);
    await render(view()); await input('scope-issue', issue.id); await input('scope-change-reason', ' Waiting for design. '); await submit('Record scope change');
    expect(container.querySelector<HTMLTextAreaElement>('[name="scope-change-reason"]')!.value).toBe(' Waiting for design. '); expect(container.textContent).toContain('Please revise the reason.');
    reject = false; await submit('Record scope change');
    const [url, options] = fetch.mock.calls.filter(([, options]) => options.method === 'POST').at(-1)!;
    expect(url).toContain('/scope-changes'); expect(JSON.parse(options.body as string)).toEqual({ issue_id: issue.id, action: 'postponed', reason: 'Waiting for design.' }); expect(container.textContent).toContain('Issue postponed and its reason recorded.');
  });

  it('conceals data and drafts immediately after denied reads', async () => {
    const denied = vi.fn(); let lost = false;
    vi.stubGlobal('fetch', vi.fn(() => Promise.resolve(lost ? problem(403, 'Access changed.') : json(snapshot()))));
    await render(view({ denied })); await baselineDraft(); lost = true; await click('Refresh scope');
    expect(denied).toHaveBeenCalledTimes(1); expect(container.textContent).toContain('Scope history unavailable'); expect(container.textContent).not.toContain('Agreed launch'); expect(container.textContent).not.toContain(issue.title); expect(container.querySelector('form')).toBeNull(); expect(hasUnsavedChanges()).toBe(false);
  });

  it('conceals data after denied writes rather than leaving the old scope in view', async () => {
    const denied = vi.fn(); vi.stubGlobal('fetch', vi.fn((_url: string, options: RequestInit) => Promise.resolve(options.method === 'POST' ? problem(404, 'Not found.') : json(snapshot()))));
    await render(view({ denied })); await baselineDraft(); await submit('Set scope baseline');
    expect(denied).toHaveBeenCalledTimes(1); expect(container.textContent).not.toContain(issue.title); expect(container.querySelector('form')).toBeNull();
  });

  it('adds older pages once and rejects mixed-version history', async () => {
    let mismatch = false;
    const fetch = vi.fn((url: string) => Promise.resolve(json(url.includes('cursor=') ? snapshot({ scope_etag: mismatch ? '"changed"' : '"scope-one"', events: { data: [event, { ...event, id: 'older', reason: 'Earlier change.' }], has_more: false, next_cursor: null } }) : snapshot({ events: { data: [event], has_more: true, next_cursor: 'older-cursor' } })))); vi.stubGlobal('fetch', fetch);
    await render(view()); await click('Load older scope changes'); expect(container.querySelectorAll('.scope-history-feed li')).toHaveLength(2); expect(container.textContent).toContain('Earlier change.');
    await click('Refresh scope'); mismatch = true; await click('Load older scope changes'); expect(container.textContent).toContain('The scope changed while loading history'); expect(container.querySelectorAll('.scope-history-feed li')).toHaveLength(1);
  });

  it('isolates pending data and drafts at a changed session boundary', async () => {
    let finish!: (response: Response) => void; let pending = false;
    vi.stubGlobal('fetch', vi.fn(() => pending ? new Promise<Response>(resolve => { finish = resolve; }) : Promise.resolve(json(snapshot()))));
    await render(view()); await baselineDraft(); pending = true;
    await render(view({ session: { ...session, csrf_token: 'replacement' } })); expect(container.textContent).not.toContain('Agreed launch'); expect(hasUnsavedChanges()).toBe(false);
    await act(async () => finish(json(snapshot()))); expect(container.querySelector<HTMLInputElement>('[name="scope-baseline-name"]')!.value).toBe('');
  });

  it('remains live after StrictMode effect cleanup and setup', async () => {
    vi.stubGlobal('fetch', vi.fn(() => Promise.resolve(json(snapshot())))); await render(<StrictMode>{view()}</StrictMode>); expect(container.textContent).toContain('Agreed launch');
  });

  it('does not let a late baseline comparison overwrite the selected baseline', async () => {
    const other = { ...baseline, id: 'baseline-two', label: 'Later plan' }; let late!: (response: Response) => void;
    vi.stubGlobal('fetch', vi.fn((url: string) => url.includes('baseline_id=baseline-two') ? new Promise<Response>(resolve => { late = resolve; }) : Promise.resolve(json(snapshot({ baselines: [baseline, other] })))));
    await render(view()); await input('scope-baseline', other.id);
    expect(container.textContent).not.toContain('Scope approved at planning.');
    await input('scope-baseline', baseline.id);
    await act(async () => late(json(snapshot({ baseline: other, baselines: [baseline, other], comparison: { unchanged: 99, added: [], removed: [], postponed: [] } }))));
    expect(container.querySelector('.scope-comparison h4')!.textContent).toBe(baseline.label);
    expect(container.textContent).not.toContain('99 currently accessible');
  });

  it('counts Unicode characters without silently truncating a valid name', async () => {
    const fetch = vi.fn((_url: string, _options: RequestInit) => Promise.resolve(json(snapshot()))); vi.stubGlobal('fetch', fetch);
    await render(view()); await input('scope-baseline-name', '🧭'.repeat(161)); await input('scope-baseline-reason', 'Reason'); await agree(); await submit('Set scope baseline');
    expect(fetch.mock.calls.filter(([, options]) => options.method === 'POST')).toHaveLength(0);
    expect(container.querySelector<HTMLInputElement>('[name="scope-baseline-name"]')!.value).toBe('🧭'.repeat(161));
    await input('scope-baseline-name', '🧭'.repeat(160)); await submit('Set scope baseline');
    const write = fetch.mock.calls.find(([, options]) => options.method === 'POST')!;
    expect(JSON.parse(write[1].body as string).label).toBe('🧭'.repeat(160));
  });

  it('does not present unavailable authors as system actions or an empty visible comparison as complete', async () => {
    vi.stubGlobal('fetch', vi.fn(() => Promise.resolve(json(snapshot({ events: { data: [{ ...event, actor_name: null }], has_more: false, next_cursor: null } }))))); await render(view());
    expect(container.querySelector('.scope-history-feed header strong')!.textContent).toBe('Unavailable member');
    expect(container.textContent).toContain('No changes among currently accessible issues.');
    expect(container.textContent).toContain('earlier work may be unavailable');
  });

  it('loads paginated scope references without duplicating total counts or agreeing an unseen scope', async () => {
    const nextIssue = { id: 'issue-two', key: 'ENG-2', title: 'Review new workflow' };
    const removed = { id: 'issue-three', key: 'ENG-3', title: 'Old workflow', reason: 'Superseded.' };
    const fetch = vi.fn((url: string, options: RequestInit) => Promise.resolve(json(options.method === 'POST' ? snapshot() : url.includes('items_cursor=') ? snapshot({ current: [issue, nextIssue], comparison: { unchanged: 2, added: [], removed: [removed], postponed: [] } }) : snapshot({ items_has_more: true, items_next_cursor: 'items-two', comparison: { unchanged: 2, added: [issue], removed: [], postponed: [] } })))); vi.stubGlobal('fetch', fetch);
    await render(view()); expect(container.textContent).toContain('This comparison is partly loaded.'); expect(container.textContent).not.toContain('No changes among currently accessible issues.');
    await baselineDraft(); await submit('Set scope baseline'); expect(fetch.mock.calls.filter(([, options]) => options.method === 'POST')).toHaveLength(0);
    await click('Load more scope items');
    expect(fetch.mock.calls.some(([url]) => url.includes('items_cursor=items-two'))).toBe(true);
    expect(container.querySelector('.scope-current summary')!.textContent).toBe('Current accessible scope (2)');
    expect(container.textContent).toContain('2 currently accessible issues remain'); expect(container.textContent).not.toContain('4 currently accessible issues remain');
    expect(container.textContent).toContain('Superseded.'); expect(container.textContent).not.toContain('This comparison is partly loaded.');
    await agree(); await submit('Set scope baseline'); expect(fetch.mock.calls.filter(([, options]) => options.method === 'POST')).toHaveLength(1);
  });

  it('pages baseline choices and keeps an older selected baseline available after refresh', async () => {
    const older = { ...baseline, id: 'older-baseline', label: 'Original plan' };
    const fetch = vi.fn((url: string) => Promise.resolve(json(url.includes('baseline_id=older-baseline') ? snapshot({ baseline: older }) : url.includes('baseline_cursor=') ? snapshot({ baselines: [older] }) : snapshot({ baselines_has_more: true, baselines_next_cursor: 'older-baselines' })))); vi.stubGlobal('fetch', fetch);
    await render(view()); await click('Load more baselines');
    expect(fetch.mock.calls.some(([url]) => url.includes('baseline_cursor=older-baselines'))).toBe(true);
    expect(container.querySelectorAll('[name="scope-baseline"] option')).toHaveLength(2);
    await input('scope-baseline', older.id);
    expect(container.querySelector('.scope-comparison h4')!.textContent).toBe('Original plan');
    expect(container.querySelector<HTMLSelectElement>('[name="scope-baseline"]')!.value).toBe(older.id);
  });
});

it.each(['invalid', 'expired', 'validator'] as const)('drops all stale scope references before reloading a %s continuation', async failure => {
  let reload!: (response: Response) => void;
  let stale = false;
  const fetch = vi.fn((url: string) => {
    if (url.includes('cursor=')) {
      stale = true;
      return Promise.resolve(failure === 'validator' ? json(snapshot({ scope_etag: '"changed"' }))
        : json({ code: failure === 'expired' ? 'cursor_expired' : 'validation_failed' }, 422));
    }
    if (stale) return new Promise<Response>(resolve => { reload = resolve; });
    return Promise.resolve(json(snapshot({ events: { data: [event], has_more: true, next_cursor: 'next' } })));
  });
  vi.stubGlobal('fetch', fetch);
  await render(view()); await input('scope-baseline', baseline.id); await input('scope-issue', issue.id); await agree();
  expect(container.textContent).toContain(issue.title);
  await click('Load older scope changes');
  expect(container.querySelector('a[href$="/issue-one"]')).toBeNull();
  expect(container.textContent).not.toContain(issue.title);
  expect(container.textContent).not.toContain(event.reason);
  expect(container.textContent).not.toContain('1 currently accessible issue remains');
  expect(container.querySelector('[name="scope-baseline"]')).toBeNull();
  expect(fetch.mock.calls.at(-1)![0]).not.toMatch(/cursor=|baseline_id=/);
  await act(async () => reload(json(snapshot({ current: [], comparison: { unchanged: 0, added: [], removed: [], postponed: [] }, events: { data: [], next_cursor: null, has_more: false } }))));
  expect(container.textContent).not.toContain(issue.title);
  expect(container.querySelector<HTMLSelectElement>('[name="scope-issue"]')!.value).toBe('');
  expect(container.querySelector<HTMLInputElement>('.scope-agreement input')!.checked).toBe(false);
});

it('reconciles an expired baseline against refreshed history and closes the attempted draft', async () => {
  let posts = 0;
  vi.stubGlobal('fetch', vi.fn((_url: string, options: RequestInit) => {
    if (options.method !== 'POST') return Promise.resolve(json(snapshot({ baseline: { ...baseline, label: posts ? 'Release plan' : 'Previous plan' } })));
    if (++posts === 1) return Promise.reject(new TypeError('Response lost'));
    return Promise.resolve(json({ code: 'conflict', details: { retry_window_expired: true } }, 409));
  }));
  await render(view()); await baselineDraft(); await submit('Set scope baseline'); await click('Retry safely');
  expect(container.querySelector('.scope-comparison h4')!.textContent).toBe('Release plan');
  expect(button('Set baseline').disabled).toBe(true);
  await click('Close attempted draft');
  expect(container.querySelector<HTMLInputElement>('[name="scope-baseline-name"]')!.value).toBe('');
  expect(posts).toBe(2);
});
