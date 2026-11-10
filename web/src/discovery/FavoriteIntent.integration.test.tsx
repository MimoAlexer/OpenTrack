// @vitest-environment jsdom
import { act, createRef, useLayoutEffect } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { advanceSessionGeneration, ApiError, request } from '../api';
import { IssueList } from '../issues/IssueList';
import { navigate, navigateConfirmed, parseRoute, useLocation, useUnsavedChanges } from '../router';
import { parseStructuredHash, viewHash, type StructuredView } from '../filters/model';
import type { Bootstrap, Issue, Session, Status } from '../types';
import type { components } from '../generated/api';

vi.mock('../api', async () => ({ ...await vi.importActual('../api'), request: vi.fn() }));
type Saved = components['schemas']['SavedDiscovery'];
const id = '11111111-1111-4111-8111-111111111111';
const otherId = '22222222-2222-4222-8222-222222222222';
const teamId = '33333333-3333-4333-8333-333333333333';
const session: Session = { audience: 'web', user: { id: 'owner', display_name: 'Example Owner', email: 'owner@example.test' }, csrf_token: 'csrf', workspaces: [{ id: 'w', name: 'Example Workspace', slug: 'example', role: 'owner' }] };
const status: Status = { id: 'ready', team_id: teamId, name: 'Ready', category: 'unstarted', color: 'slate', position: 0, is_default: true, archived_at: null, retiring_at: null, replacement_status_id: null, replacement_job_id: null };
const data: Bootstrap = { auth_epoch: '1', teams: [{ id: teamId, key: 'EX', name: 'Example Team', private: false, archived_at: null }], statuses: [status], members: [session.user], issues: [] };
const issue: Issue = { id: 'issue', workspace_id: 'w', team_id: teamId, key: 'EX-1', title: 'Previously visible issue', description: '', status_id: status.id, priority: 1, assignee_id: null, assignee: null, labels: [], label_ids: [], etag: '"issue:1"', estimate: null, due_date: null, version: '1', created_at: '2026-11-09T10:00:00Z', updated_at: '2026-11-09T10:00:00Z', archived_at: null };
const view: StructuredView = { version: 1, layout: 'list', filter: { kind: 'predicate', field: 'priority', op: 'eq', value: 1 }, sort: 'priority', archived: false, limit: 50, timezone: 'UTC' };
function saved(kind: 'view' | 'search' = 'view', overrides: Partial<Saved> = {}): Saved {
  const { layout, ...query } = view;
  return { id, workspace_id: 'w', owner_id: session.user.id, name: 'Current renamed favorite', kind, favorite: true, visibility: 'personal', team_id: null, definition: { query: { ...query, ...(kind === 'search' ? { q: 'nebula', sort: 'created' } : {}) }, layout, group_by: 'priority', columns: ['key', 'title'] }, can_edit: true, repair_required: null, version: '2', etag: '"favorite:2"', ...overrides };
}
const directory = { data: [], has_more: false, next_cursor: null };
const page = (title = issue.title) => ({ data: [{ ...issue, title }], has_more: false, next_cursor: null, query_context: { timezone: 'UTC', as_of: '2026-11-09T10:00:00Z' } });
const searchPage = (title = 'Current full-text result') => ({ ...directory, data: [{ id: 'hit', team_id: teamId, kind: 'issue', key: 'EX-2', title, snippet: 'Current authorized match' }], fuzzy_degraded: false, query_context: { timezone: 'UTC', as_of: '2026-11-09T10:00:00Z' } });
function deferred<T>() { let resolve!: (value: T) => void; let reject!: (reason: unknown) => void; const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; }); return { promise, resolve, reject }; }
const favoriteUrl = (kind = 'view', target = id) => `/w/w/issues?open_saved=${kind}:${target}`;
let host: HTMLDivElement; let root: Root;
const unavailable = vi.fn();
const rowObservations: boolean[] = [];
let initialUrl: string;
function Routed({ account = session, epoch = '1', dirty = false }: { account?: Session; epoch?: string; dirty?: boolean }) {
  const route = parseRoute(useLocation());
  useUnsavedChanges(dirty);
  useLayoutEffect(() => { if (route.search.has('open_saved')) rowObservations.push(!!host.querySelector('.issue-row')); });
  const props = { workspaceId: route.workspaceId ?? 'w', data: { ...data, auth_epoch: epoch }, session: account, params: route.search, refresh: 0, create: () => {}, searchRef: createRef<HTMLInputElement>(), favoriteUnavailable: unavailable };
  return <IssueList {...props}/>;
}
async function render(props: Parameters<typeof Routed>[0] = {}) { await act(async () => root.render(<Routed {...props}/>)); }
async function go(url: string) { await act(async () => { navigateConfirmed(url); }); }
async function tick() { await act(async () => { await vi.advanceTimersByTimeAsync(200); }); }
async function click(text: string) { const button = [...host.querySelectorAll('button')].find(item => item.textContent === text); expect(button, text).toBeDefined(); await act(async () => button!.click()); }
function executionCalls() { return vi.mocked(request).mock.calls.filter(([path]) => /\/(?:issues(?:\/query|\?)|search\/query)/.test(path)); }
function detailCalls() { return vi.mocked(request).mock.calls.filter(([path]) => /\/(?:views|saved-searches)\//.test(path)); }
function conceal() { expect(host.textContent).not.toContain(issue.title); expect(host.querySelector('.issue-row')).toBeNull(); expect(host.querySelector('.discovery-results')).toBeNull(); }
beforeEach(() => {
  initialUrl = `${location.pathname}${location.search}${location.hash}`;
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true); vi.useFakeTimers(); unavailable.mockReset(); rowObservations.length = 0; vi.mocked(request).mockReset();
  host = document.createElement('div'); document.body.append(host); root = createRoot(host);
  vi.mocked(request).mockImplementation(async (path) => {
    if (path.endsWith('/workflow')) return { team_id: teamId, version: '1', can_manage: true, statuses: [status] } as never;
    if (path.endsWith('/search/query')) return searchPage() as never;
    if (path.endsWith('/issues/query') || path.includes('/issues?')) return page() as never;
    return directory as never;
  });
});
afterEach(async () => { await act(async () => root.unmount()); host.remove(); navigateConfirmed(initialUrl, true); vi.useRealTimers(); vi.restoreAllMocks(); vi.unstubAllGlobals(); });

it.each(['view', 'search'] as const)('fresh %s favorite waits for current typed GET before any execution', async kind => {
  const detail = deferred<Saved>(); const fallback = vi.mocked(request).getMockImplementation()!;
  vi.mocked(request).mockImplementation((path, options) => path.endsWith(`/${kind === 'view' ? 'views' : 'saved-searches'}/${id}`) ? detail.promise as never : fallback(path, options));
  await go(favoriteUrl(kind)); await render(); await tick();
  expect(detailCalls().map(([path]) => path)).toEqual([`/workspaces/w/${kind === 'view' ? 'views' : 'saved-searches'}/${id}`]);
  expect(executionCalls()).toHaveLength(0); conceal(); expect(host.querySelector<HTMLDetailsElement>('.discovery')?.open).toBe(true);
  await act(async () => detail.resolve(saved(kind))); await tick();
  expect(location.search).not.toContain('open_saved');
  const canonical = parseStructuredHash(location.hash); expect(canonical.kind).toBe('valid');
  if (canonical.kind === 'valid') expect(canonical.view.filter).toEqual(view.filter);
  expect(host.textContent).toContain('Current renamed favorite · Version 2');
  expect(executionCalls().length).toBeGreaterThan(0);
  for (const [path, options] of executionCalls()) {
    expect(path).not.toContain('/issues?');
    const body = options?.body as { query?: { filter: unknown }; filter?: unknown };
    expect(body.query?.filter ?? body.filter).toEqual(view.filter);
  }
  if (kind === 'search') {
    expect(host.querySelector<HTMLInputElement>('[aria-label="Search issues and projects"]')?.value).toBe('nebula');
    expect(executionCalls().find(([path]) => path.endsWith('/search/query'))?.[1]?.body).toMatchObject({ query: { q: 'nebula', filter: view.filter }, types: ['issue'] });
    expect(host.textContent).toContain('Current full-text result');
  } else expect(host.querySelector('.issue-row')).not.toBeNull();
});

it('a view favorite restores its board layout through the existing issue board', async () => {
  const favorite = saved('view'); favorite.definition = { ...favorite.definition!, layout: 'board', query: { ...favorite.definition!.query, team_id: teamId } };
  const fallback = vi.mocked(request).getMockImplementation()!;
  vi.mocked(request).mockImplementation((path, options) => path.endsWith(`/views/${id}`) ? Promise.resolve(favorite) as never : fallback(path, options));
  await go(favoriteUrl()); await render(); await tick();
  const canonical = parseStructuredHash(location.hash); expect(canonical.kind).toBe('valid');
  if (canonical.kind === 'valid') expect(canonical.view).toMatchObject({ layout: 'board', team_id: teamId, filter: view.filter });
  expect(host.querySelector('.issue-board')).not.toBeNull(); expect(host.textContent).toContain(issue.title);
  expect(executionCalls()).toHaveLength(1); expect(executionCalls()[0][0]).toBe('/workspaces/w/issues/query');
  expect(executionCalls()[0][1]?.body).toMatchObject({ team_id: teamId, filter: view.filter });
});

it.each([
  '?open_saved=view:not-a-uuid', '?open_saved=unknown:11111111-1111-4111-8111-111111111111',
  `?open_saved=view:${id}&open_saved=view:${otherId}`, `?open_saved=view:${id}&q=broaden`,
  `?open_saved=view:${id}#comment-11111111-1111-4111-8111-111111111111`,
  `?open_saved=view:${id}${viewHash(view)}`, `?open_saved=view:${id}#filters=malformed`,
])('malformed intent %s conceals existing rows without issuing a fallback request', async suffix => {
  await go('/w/w/issues'); await render(); expect(host.textContent).toContain(issue.title); vi.mocked(request).mockClear();
  await go(`/w/w/issues${suffix}`); await tick();
  conceal(); expect(executionCalls()).toHaveLength(0); expect(detailCalls()).toHaveLength(0); expect(rowObservations).not.toContain(true);
});

it.each([403, 404])('denied favorite GET (%s) signals a typed tombstone and never falls back', async code => {
  const fallback = vi.mocked(request).getMockImplementation()!;
  vi.mocked(request).mockImplementation((path, options) => path.endsWith(`/views/${id}`) ? Promise.reject(new ApiError(code, 'not_available', 'Favorite is unavailable')) : fallback(path, options));
  await go('/w/w/issues'); await render(); vi.mocked(request).mockClear(); await go(favoriteUrl()); await tick();
  conceal(); expect(executionCalls()).toHaveLength(0); expect(unavailable).toHaveBeenCalledWith('view', id); expect(host.textContent).toContain('Favorite is unavailable'); expect(rowObservations).not.toContain(true);
});

it.each(['id', 'kind', 'workspace_id'] as const)('rejects a successful GET whose %s does not match the intent', async field => {
  const fallback = vi.mocked(request).getMockImplementation()!;
  const wrong = saved('view', { [field]: field === 'kind' ? 'search' : 'different' });
  vi.mocked(request).mockImplementation((path, options) => path.endsWith(`/views/${id}`) ? Promise.resolve(wrong) as never : fallback(path, options));
  await go(favoriteUrl()); await render(); await tick();
  expect(detailCalls()).toHaveLength(1); expect(executionCalls()).toHaveLength(0); conceal(); expect(location.search).toContain('open_saved');
});

it('retries a failed detail lookup explicitly, then applies only the verified filters', async () => {
  let reads = 0; const fallback = vi.mocked(request).getMockImplementation()!;
  vi.mocked(request).mockImplementation((path, options) => path.endsWith(`/views/${id}`) ? (++reads === 1 ? Promise.reject(new ApiError(0, 'network_error', 'Connection interrupted')) : Promise.resolve(saved())) as never : fallback(path, options));
  await go(favoriteUrl()); await render(); await tick(); expect(executionCalls()).toHaveLength(0);
  await click('Try again'); await tick(); expect(reads).toBe(2); expect(location.search).not.toContain('open_saved'); expect(host.querySelector('.issue-row')).not.toBeNull();
  expect(executionCalls().every(([path]) => !path.includes('/issues?'))).toBe(true);
});

it.each(['definition missing', 'repair required'] as const)('holds a %s favorite for explicit replacement review', async reason => {
  const fallback = vi.mocked(request).getMockImplementation()!;
  const broken = saved('view', reason === 'definition missing' ? { definition: null } : { repair_required: 'A filter target is no longer available' });
  vi.mocked(request).mockImplementation((path, options) => path.endsWith(`/views/${id}`) ? Promise.resolve(broken) as never : fallback(path, options));
  await go(favoriteUrl()); await render(); await tick();
  conceal(); expect(executionCalls()).toHaveLength(0); expect(location.search).toContain('open_saved');
  await click('Review replacement filters'); await tick(); expect(executionCalls()).toHaveLength(0);
  expect(host.querySelector('section[aria-label="Review replacement filters"]')).not.toBeNull();
  expect([...host.querySelectorAll('button')].find(button => button.textContent === 'Replace saved definition with reviewed filters')?.disabled).toBe(true);
  await click('Cancel replacement'); expect(executionCalls()).toHaveLength(0);
});

it('only executes repaired filters after the explicit replacement acknowledgement and successful PATCH', async () => {
  const patch = deferred<Saved>(); const fallback = vi.mocked(request).getMockImplementation()!;
  vi.mocked(request).mockImplementation((path, options) => path.endsWith(`/views/${id}`)
    ? (options?.method === 'PATCH' ? patch.promise : Promise.resolve(saved('view', { definition: null, repair_required: 'Deleted filter target' }))) as never : fallback(path, options));
  await go(favoriteUrl()); await render(); await tick(); await click('Review replacement filters');
  const replacement = host.querySelector('section[aria-label="Review replacement filters"]')!;
  const reviewed = JSON.parse(replacement.querySelector('pre')!.textContent!);
  expect(executionCalls()).toHaveLength(0);
  await act(async () => replacement.querySelector<HTMLInputElement>('input[type="checkbox"]')!.click());
  await click('Replace saved definition with reviewed filters'); expect(executionCalls()).toHaveLength(0);
  const write = detailCalls().find(([, options]) => options?.method === 'PATCH');
  expect(write?.[1]?.headers).toMatchObject({ 'If-Match': '"favorite:2"' }); expect(write?.[1]?.body).toMatchObject({ definition: reviewed });
  await act(async () => patch.resolve(saved('view', { definition: reviewed, version: '3', etag: '"favorite:3"' }))); await tick();
  expect(location.search).not.toContain('open_saved'); expect(executionCalls()).toHaveLength(1);
  expect(executionCalls()[0][0]).toBe('/workspaces/w/issues/query');
  expect(executionCalls()[0][1]?.body).toMatchObject(reviewed.query);
});

it.each(['selection', 'principal', 'epoch', 'workspace'] as const)('discards a delayed favorite response after a changed %s', async change => {
  const late = deferred<Saved>(); const fallback = vi.mocked(request).getMockImplementation()!; let reads = 0;
  vi.mocked(request).mockImplementation((path, options) => path.endsWith(`/views/${id}`) ? (++reads === 1 ? late.promise : new Promise(() => {})) as never : path.endsWith(`/views/${otherId}`) ? new Promise(() => {}) : fallback(path, options));
  await go(favoriteUrl()); await render();
  if (change === 'selection') await go(favoriteUrl('view', otherId));
  else if (change === 'principal') await render({ account: { ...session, user: { ...session.user, id: 'other-owner' }, csrf_token: 'other-csrf' } });
  else if (change === 'epoch') await render({ epoch: '2' });
  else await go(`/w/other-workspace/issues?open_saved=view:${otherId}`);
  await act(async () => late.resolve(saved())); await tick();
  expect(host.textContent).not.toContain('Current renamed favorite · Version'); expect(executionCalls()).toHaveLength(0); conceal(); expect(location.search).toContain('open_saved');
});

it('an in-flight ordinary issue page cannot reappear while a new favorite is pending', async () => {
  const latePage = deferred<ReturnType<typeof page>>(); const fallback = vi.mocked(request).getMockImplementation()!;
  vi.mocked(request).mockImplementation((path, options) => path.includes('/issues?') ? latePage.promise as never : path.endsWith(`/views/${id}`) ? new Promise(() => {}) : fallback(path, options));
  await go('/w/w/issues'); await render(); expect(executionCalls()).toHaveLength(1);
  await go(favoriteUrl()); await act(async () => latePage.resolve(page('Obsolete delayed issue'))); await tick();
  expect(host.textContent).not.toContain('Obsolete delayed issue'); conceal(); expect(executionCalls()).toHaveLength(1); expect(rowObservations).not.toContain(true);
});

it('a delayed previous full-text result cannot reappear while another favorite is pending', async () => {
  const lateSearch = deferred<ReturnType<typeof searchPage>>(); const fallback = vi.mocked(request).getMockImplementation()!;
  vi.mocked(request).mockImplementation((path, options) => path.endsWith(`/saved-searches/${id}`) ? Promise.resolve(saved('search')) as never : path.endsWith(`/views/${otherId}`) ? new Promise(() => {}) : path.endsWith('/search/query') ? lateSearch.promise as never : fallback(path, options));
  await go(favoriteUrl('search')); await render(); await tick(); expect(executionCalls().filter(([path]) => path.endsWith('/search/query'))).toHaveLength(1);
  await go(favoriteUrl('view', otherId)); await act(async () => lateSearch.resolve(searchPage('Obsolete delayed search hit'))); await tick();
  expect(host.textContent).not.toContain('Obsolete delayed search hit'); conceal(); expect(executionCalls().filter(([path]) => path.endsWith('/search/query'))).toHaveLength(1);
});

it('explicit All issues exits a held intent and fences its delayed response', async () => {
  const detail = deferred<Saved>(); const fallback = vi.mocked(request).getMockImplementation()!;
  vi.mocked(request).mockImplementation((path, options) => path.endsWith(`/views/${id}`) ? detail.promise as never : fallback(path, options));
  await go(favoriteUrl()); await render(); expect(executionCalls()).toHaveLength(0);
  await click('Open all issues'); expect(location.search).not.toContain('open_saved'); expect(executionCalls()).toHaveLength(1);
  expect(host.textContent).not.toContain('Checking the current saved definition');
  expect(host.querySelector<HTMLInputElement>('.discovery-save input')?.disabled).toBe(false);
  await act(async () => detail.resolve(saved())); await tick(); expect(location.hash).toBe(''); expect(host.textContent).not.toContain('Current renamed favorite · Version'); expect(host.textContent).toContain(issue.title);
});

it.each(['success', 'denial'] as const)('session generation fences a deferred favorite %s before any parent rerender', async outcome => {
  const detail = deferred<Saved>(); const fallback = vi.mocked(request).getMockImplementation()!;
  vi.mocked(request).mockImplementation((path, options) => path.endsWith(`/views/${id}`) ? detail.promise as never : fallback(path, options));
  await go(favoriteUrl()); await render();
  advanceSessionGeneration();
  await act(async () => { if (outcome === 'success') detail.resolve(saved()); else detail.reject(new ApiError(404, 'not_available', 'Obsolete denial')); }); await tick();
  expect(location.search).toContain('open_saved'); expect(location.hash).toBe(''); expect(executionCalls()).toHaveLength(0); expect(unavailable).not.toHaveBeenCalled(); conceal();
});

it.each(['success', 'denial'] as const)('a changed live URL fences a deferred favorite %s before effect cleanup', async outcome => {
  const detail = deferred<Saved>(); const fallback = vi.mocked(request).getMockImplementation()!;
  vi.mocked(request).mockImplementation((path, options) => path.endsWith(`/views/${id}`) ? detail.promise as never : fallback(path, options));
  await go(favoriteUrl()); await render();
  history.pushState({}, '', '/w/w/projects');
  await act(async () => { if (outcome === 'success') detail.resolve(saved()); else detail.reject(new ApiError(404, 'not_available', 'Obsolete denial')); });
  expect(location.pathname).toBe('/w/w/projects'); expect(location.hash).toBe(''); expect(executionCalls()).toHaveLength(0); expect(unavailable).not.toHaveBeenCalled();
});

it('keeps the global unsaved-draft guard when leaving a held favorite', async () => {
  vi.mocked(request).mockImplementation(async () => { throw new ApiError(0, 'network_error', 'Connection interrupted'); });
  const confirm = vi.spyOn(window, 'confirm').mockReturnValue(false);
  await go(favoriteUrl()); await render({ dirty: true }); await click('Open all issues');
  expect(confirm).toHaveBeenCalled(); expect(location.search).toContain('open_saved'); expect(executionCalls()).toHaveLength(0); conceal();
  await act(async () => { expect(navigate('/w/w/projects')).toBe(false); }); expect(location.search).toContain('open_saved');
});
