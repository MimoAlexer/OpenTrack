// @vitest-environment jsdom
import { act, createRef } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import canonical from '../../../crates/opentrack-domain/tests/fixtures/issue-query-v1.json';
import { IssueList } from '../issues/IssueList';
import { hasUnsavedChanges, issuesUrl, navigate, navigateConfirmed, parseRoute, useLocation, useUnsavedChanges } from '../router';
import { data, id, issue, json, session, teamId, workspaceId } from './test-fixtures';
import { parseStructuredHash, structuredMarker, viewHash, type StructuredView } from './model';

let container: HTMLDivElement; let root: Root;
const basePath = `/w/${workspaceId}/issues`;
const view = (): StructuredView => ({ ...structuredClone(canonical), layout: 'list' }) as StructuredView;
const context = { timezone: 'UTC', as_of: '2026-11-08T12:00:00Z' };
const page = (rows = [issue], more = false) => json({ data: rows, has_more: more, next_cursor: more ? 'opaque-page' : null, query_context: context });
beforeEach(() => { vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true); navigateConfirmed(basePath, true); container = document.createElement('div'); document.body.append(container); root = createRoot(container); });
afterEach(async () => { await act(async () => root.unmount()); container.remove(); navigateConfirmed(basePath, true); vi.restoreAllMocks(); vi.unstubAllGlobals(); });
function Harness({ dirty = false }: { dirty?: boolean }) {
  const route = parseRoute(useLocation()); useUnsavedChanges(dirty);
  return <><IssueList workspaceId={workspaceId} data={data} session={session} params={route.search} selectedId={route.issueId} refresh={0} create={() => {}} searchRef={createRef()}/>{route.issueId && <button onClick={() => navigate(issuesUrl(workspaceId, route.search))}>Close detail</button>}</>;
}
async function flush() { await act(async () => { await new Promise(resolve => setTimeout(resolve, 5)); }); }
async function render(hash = viewHash(view()), dirty = false) { await act(async () => { navigateConfirmed(`${basePath}${hash}`, true); root.render(<Harness dirty={dirty}/>); }); await flush(); }
async function click(element: Element) { await act(async () => (element as HTMLElement).click()); await flush(); }
function button(text: string) { const value = [...container.querySelectorAll('button')].find(button => button.textContent === text); if (!value) throw new Error(`Missing button ${text}`); return value; }

it('posts the canonical query, paginates with the same context and keeps filters through detail navigation', async () => {
  const requests: { url: string; init: RequestInit }[] = [];
  vi.stubGlobal('fetch', vi.fn(async (url: string, init: RequestInit) => { requests.push({ url, init }); return requests.length === 1 ? page([issue], true) : page([issue, { ...issue, id: id(8), key: 'ENG-2', title: 'Second matching issue' }]); }));
  const state = view(); await render(viewHash(state));
  expect(requests[0].url).toBe(`/api/v1/workspaces/${workspaceId}/issues/query`);
  expect(requests[0].init.method).toBe('POST');
  expect(new Headers(requests[0].init.headers).get('X-CSRF-Token')).toBe(session.csrf_token);
  expect(JSON.parse(requests[0].init.body as string)).toEqual(canonical);
  expect(container.querySelector(`#row-${issue.id}`)).not.toBeNull();
  await click(button('Load more issues'));
  expect(JSON.parse(requests[1].init.body as string)).toEqual({ ...canonical, cursor: 'opaque-page' });
  expect(container.querySelectorAll('.issue-row')).toHaveLength(2);
  await click(container.querySelector(`#row-${issue.id}`)!);
  expect(location.pathname).toBe(`${basePath}/${issue.id}`);
  expect(parseStructuredHash(location.hash)).toEqual({ kind: 'valid', view: state });
  await click(button('Close detail'));
  expect(location.pathname).toBe(basePath);
  expect(location.search).toBe('');
  expect(parseStructuredHash(location.hash)).toEqual({ kind: 'valid', view: state });
  expect(requests).toHaveLength(2);
});

it('keeps exactly the same AST when switching from list to board', async () => {
  const bodies: unknown[] = [];
  vi.stubGlobal('fetch', vi.fn(async (url: string, init: RequestInit) => { if (url.endsWith('/workflow')) return json({ team_id: teamId, version: '1', can_manage: true, statuses: data.statuses }); bodies.push(JSON.parse(init.body as string)); return page(); }));
  const state = { ...view(), team_id: teamId }; await render(viewHash(state));
  const layout = container.querySelector<HTMLSelectElement>('select[aria-label="Issue layout"]')!;
  await act(async () => { layout.value = 'board'; layout.dispatchEvent(new Event('change', { bubbles: true })); }); await flush();
  expect(bodies).toHaveLength(2);
  expect(bodies[1]).toEqual(bodies[0]);
  expect(parseStructuredHash(location.hash)).toEqual({ kind: 'valid', view: { ...state, layout: 'board' } });
  expect(container.querySelector('.issue-board')).not.toBeNull();
});

it.each(['#filters=%', '#unknown=state', `#filters=${encodeURIComponent(JSON.stringify({ ...view(), version: 8 }))}`, `#filters=${encodeURIComponent(JSON.stringify({ ...view(), filter: { kind: 'all', children: [] } }))}`])('blocks invalid fragment %s without sending GET or POST', async hash => {
  const fetch = vi.fn(async (_url: string, _init: RequestInit) => page()); vi.stubGlobal('fetch', fetch);
  await render(hash);
  expect(container.textContent).toContain('These filters are unavailable.');
  expect(container.querySelector('.issue-row')).toBeNull();
  expect(fetch).not.toHaveBeenCalled();
  await click(button('Clear filter settings'));
  expect(location.hash).toBe('');
  expect(fetch).toHaveBeenCalledOnce();
  expect(fetch.mock.calls[0][1]?.method).toBeUndefined();
});

it('rejects a late result after the structured filter changed, even when its base parameters match', async () => {
  let resolveOld!: (response: Response) => void; let oldSignal: AbortSignal | null | undefined;
  const calls: unknown[] = [];
  vi.stubGlobal('fetch', vi.fn((_url: string, init: RequestInit) => {
    calls.push(JSON.parse(init.body as string));
    if (calls.length === 1) { oldSignal = init.signal; return new Promise<Response>(resolve => { resolveOld = resolve; }); }
    return Promise.resolve(page([{ ...issue, title: 'New filter result' }]));
  }));
  await render();
  const next: StructuredView = { ...view(), filter: { kind: 'predicate', field: 'priority', op: 'eq', value: 4 } };
  await act(async () => { navigate(`${basePath}${viewHash(next)}`); }); await flush();
  expect(oldSignal?.aborted).toBe(true);
  await act(async () => resolveOld(page([{ ...issue, title: 'Late result from prior filter' }])));
  expect(container.textContent).toContain('New filter result');
  expect(container.textContent).not.toContain('Late result from prior filter');
  expect(calls).toHaveLength(2);
});

it('does not fall back to a flat query when the structured endpoint rejects an inaccessible target', async () => {
  const fetch = vi.fn(async () => json({ detail: 'A filter target is unavailable', code: 'validation_failed' }, 422)); vi.stubGlobal('fetch', fetch);
  await render();
  expect(container.textContent).toContain('A filter target is unavailable');
  expect(container.querySelector('.issue-row')).toBeNull();
  expect(fetch).toHaveBeenCalledOnce();
  expect(location.hash).not.toBe('');
});

it('accepts a pending query after a comment-anchor-only change without replacing its AST or refetching', async () => {
  let resolve!: (response: Response) => void;
  const fetch = vi.fn((_url: string, _init: RequestInit) => new Promise<Response>(done => { resolve = done; })); vi.stubGlobal('fetch', fetch);
  const state = view(); await render(viewHash(state));
  await act(async () => { navigateConfirmed(`${basePath}/${issue.id}${viewHash(state)}&comment=${id(70)}`, true); });
  await act(async () => resolve(page()));
  expect(container.querySelector(`#row-${issue.id}`)).not.toBeNull();
  expect(fetch).toHaveBeenCalledOnce();
  await click(button('Close detail'));
  expect(parseStructuredHash(location.hash)).toEqual({ kind: 'valid', view: state });
  expect(fetch).toHaveBeenCalledOnce();
});

it('round-trips Back and Forward and retains the complete fragment on reload', async () => {
  vi.stubGlobal('fetch', vi.fn(async (_url: string, init: RequestInit) => json({ data: [issue], next_cursor: null, has_more: false, query_context: { ...context, timezone: JSON.parse(init.body as string).timezone } })));
  const first = view(); await render(viewHash(first));
  const second: StructuredView = { ...first, timezone: 'Europe/Zurich', filter: { kind: 'predicate', field: 'due_date', op: 'before', value: '2026-03-29' } };
  await act(async () => { navigate(`${basePath}${viewHash(second)}`); }); await flush();
  await act(async () => { history.back(); await new Promise(resolve => setTimeout(resolve, 30)); });
  expect(parseStructuredHash(location.hash)).toEqual({ kind: 'valid', view: first });
  await act(async () => { history.forward(); await new Promise(resolve => setTimeout(resolve, 30)); });
  expect(parseStructuredHash(location.hash)).toEqual({ kind: 'valid', view: second });
  await act(async () => { root.unmount(); root = createRoot(container); root.render(<Harness/>); }); await flush();
  expect(parseRoute(`${location.pathname}${location.search}${location.hash}`).structured).toEqual({ kind: 'valid', view: second });
});

it('guards hash-only filter changes while an issue draft is dirty and does not propagate fragments across workspaces', async () => {
  vi.stubGlobal('fetch', vi.fn(async () => page())); const confirm = vi.spyOn(window, 'confirm').mockReturnValue(false);
  const state = view(); await render(viewHash(state), true);
  expect(hasUnsavedChanges()).toBe(true);
  const next = { ...state, sort: 'priority' as const };
  await act(async () => { expect(navigate(`${basePath}${viewHash(next)}`)).toBe(false); });
  expect(confirm).toHaveBeenCalledOnce();
  expect(parseStructuredHash(location.hash)).toEqual({ kind: 'valid', view: state });
  const params = parseRoute(`${location.pathname}${location.hash}`).search;
  expect(params.has(structuredMarker)).toBe(true);
  expect(issuesUrl(id(90), params)).not.toContain('#');
  expect(issuesUrl(id(90), params)).not.toContain(structuredMarker);
  expect(issuesUrl(workspaceId, new URLSearchParams({ view: 'mine' }))).not.toContain('#');
});

it.each([
  { search: `?priority=4&team_id=${teamId}`, filter: { kind: 'predicate', field: 'priority', op: 'eq', value: 4 }, team: teamId },
  { search: '?view=mine', filter: { kind: 'predicate', field: 'assignee', op: 'eq', value: session.user.id }, team: undefined },
])('opens Advanced with the current flat intent after navigation: $search', async ({ search, filter, team }) => {
  const bodies: Record<string, unknown>[] = [];
  vi.stubGlobal('fetch', vi.fn(async (_url: string, init: RequestInit) => { if (init.method === 'POST') bodies.push(JSON.parse(init.body as string)); return page(); }));
  await render('');
  await act(async () => { navigate(`${basePath}${search}`); }); await flush();
  await click([...container.querySelectorAll('summary')].find(summary => summary.textContent?.includes('Advanced filters'))!);
  await click(button('Apply filters'));
  expect(bodies).toHaveLength(1);
  expect(bodies[0].filter).toEqual(filter);
  expect(bodies[0].team_id).toBe(team);
});
