// @vitest-environment jsdom
import { act, useRef, useState } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { App } from '../App';
import { hasUnsavedChanges, navigateConfirmed, parseRoute, useLocation } from '../router';
import type { Bootstrap, Issue, Session, Status } from '../types';
import type { Workflow } from '../workflows/types';
import { IssueList } from './IssueList';
import { IssueBoard } from './IssueBoard';
import { IssueOrderingDialog, useIssueOrdering } from './IssueOrdering';

const session: Session = { audience: 'web', user: { id: 'owner', display_name: 'Example Owner', email: 'owner@example.test' }, csrf_token: 'csrf-one', workspaces: [{ id: 'workspace', name: 'Example Workspace', slug: 'example', role: 'owner' }] };
const status: Status = { id: 'ready', team_id: 'team', name: 'Ready', category: 'unstarted', color: 'slate', position: 0, is_default: true, archived_at: null, retiring_at: null, replacement_status_id: null, replacement_job_id: null };
const target: Status = { ...status, id: 'started', name: 'In progress', category: 'started', position: 1, is_default: false };
const workflow: Workflow = { team_id: 'team', version: '1', can_manage: true, statuses: [status, target] };
const source: Issue = { id: 'source', workspace_id: 'workspace', team_id: 'team', key: 'EX-1', title: 'Visible source issue', description: '', body_version: '1', status_id: status.id, priority: 0, assignee_id: null, assignee: null, label_ids: [], labels: [], etag: '"issue:source:1"', estimate: null, due_date: null, version: '1', created_at: '2026-10-25T10:00:00Z', updated_at: '2026-10-25T10:00:00Z', archived_at: null };
const anchor: Issue = { ...source, id: 'anchor', key: 'EX-2', title: 'Visible reference issue', etag: '"issue:anchor:1"' };
const hidden: Issue = { ...source, id: 'hidden', key: 'EX-3', title: 'Filtered out issue', etag: '"issue:hidden:1"' };
const data: Bootstrap = { auth_epoch: '1', teams: [{ id: 'team', name: 'Example private team', key: 'EX', private: true, archived_at: null }], statuses: [status, target], members: [session.user], issues: [] };
const issuePath = '/workspaces/workspace/issues';
const json = (value: unknown, statusCode = 200) => new Response(JSON.stringify(value), { status: statusCode, headers: { 'Content-Type': 'application/json' } });
const initialUrl = (layout = 'list') => `/w/workspace/issues?team_id=team&sort=manual&layout=${layout}&display_version=1`;
type Call = { path: string; query: URLSearchParams; init: RequestInit };
function deferred() { let resolve!: (value: Response) => void; const promise = new Promise<Response>(done => { resolve = done; }); return { promise, resolve }; }
let container: HTMLDivElement; let root: Root; let calls: Call[]; let items: Issue[]; let generation: number; let epoch: string;
let serverSession: Session; let pagination: boolean; let omitToken: boolean; let hit: Element | null; let heldSession: ReturnType<typeof deferred> | null;
let positionReply: (body: Record<string, string>, init: RequestInit) => Promise<Response>;
let previous: { url: string; state: unknown; title: string; theme: string | null; storage: [string, string][] };
const originalHitTest = Object.getOwnPropertyDescriptor(document, 'elementFromPoint');
function currentSource() { return items.find(item => item.id === source.id)!; }
function changeSource(body: Record<string, string>) {
  const current = currentSource(); const next = { ...current, status_id: body.status_id ?? current.status_id, version: String(Number(current.version) + 1), etag: `"issue:source:${Number(current.version) + 1}"` };
  items = items.filter(item => item.id !== source.id); const reference = body.before_id ?? body.after_id;
  const index = reference ? items.findIndex(item => item.id === reference) + (body.after_id ? 1 : 0) : items.length;
  items.splice(index, 0, next); generation += 1; return next;
}
beforeEach(() => {
  vi.useFakeTimers(); vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true); vi.spyOn(window, 'confirm').mockReturnValue(false);
  vi.spyOn(document, 'visibilityState', 'get').mockReturnValue('visible');
  vi.stubGlobal('requestAnimationFrame', vi.fn(() => 1)); vi.stubGlobal('cancelAnimationFrame', vi.fn());
  HTMLDialogElement.prototype.showModal = function () { this.setAttribute('open', ''); }; HTMLDialogElement.prototype.close = function () { this.removeAttribute('open'); };
  previous = { url: location.pathname + location.search, state: history.state, title: document.title, theme: document.documentElement.getAttribute('data-theme'), storage: Array.from({ length: localStorage.length }, (_, index) => { const key = localStorage.key(index)!; return [key, localStorage.getItem(key)!] as [string, string]; }) };
  localStorage.clear(); history.replaceState({ otIndex: 0 }, '', initialUrl()); calls = []; items = [anchor, hidden, source]; generation = 1; epoch = '1'; serverSession = session; pagination = false; omitToken = false; heldSession = null; hit = null;
  Object.defineProperty(document, 'elementFromPoint', { configurable: true, value: () => hit });
  positionReply = body => Promise.resolve(json({ issue: changeSource(body), order_version: `${generation}:1` }));
  vi.stubGlobal('fetch', vi.fn((input: string, init: RequestInit = {}) => {
    const url = new URL(input, 'https://example.test'); const path = url.pathname.replace(/^\/api\/v1/, ''); const query = url.searchParams; calls.push({ path, query, init });
    if (path === '/setup/status') return Promise.resolve(json({ setup_required: false }));
    if (path === '/auth/session') return heldSession ? heldSession.promise : Promise.resolve(json(serverSession));
    if (path.endsWith('/preferences')) return Promise.resolve(json({ workspace_id: 'workspace', shortcut_mode: 'disabled', version: '1', etag: '"preference:1"' }));
    if (path === '/workspaces/workspace/bootstrap') return Promise.resolve(json({ ...data, auth_epoch: epoch, issues: items }));
    if (path === '/workspaces/workspace/access') return Promise.resolve(json({ auth_epoch: epoch, role: 'owner', archived: false }));
    if (path === '/workspaces/workspace/teams/team/workflow') return Promise.resolve(json(workflow));
    if (path === '/workspaces/workspace/teams/team') return Promise.resolve(json({ ...data.teams[0], workspace_id: 'workspace', version: '1', can_manage: true }));
    if (path === `${issuePath}/source/position` && init.method === 'POST') return positionReply(JSON.parse(init.body as string), init);
    if (path === `${issuePath}/source` && init.method === 'PATCH') return Promise.resolve(json(changeSource(JSON.parse(init.body as string))));
    if (path.startsWith(`${issuePath}/`)) return Promise.resolve(json(items.find(item => item.id === path.split('/').at(-1))));
    if (path === issuePath) {
      let selected = items.filter(item => (!query.get('status_id') || item.status_id === query.get('status_id')) && (!query.get('q') || item.title.includes(query.get('q')!)));
      let next: string | null = null;
      if (pagination) { if (query.get('cursor')) selected = selected.slice(50); else { next = selected.length > 50 ? 'second-page' : null; selected = selected.slice(0, 50); } }
      return Promise.resolve(json({ data: selected, next_cursor: next, has_more: !!next, ...(query.get('sort') === 'manual' && !omitToken ? { order_version: `${generation}:1` } : {}) }));
    }
    throw new Error(`Unexpected integrated view request ${path}`);
  }));
  container = document.createElement('div'); document.body.append(container); root = createRoot(container);
});
afterEach(async () => {
  await act(async () => { root.unmount(); heldSession?.resolve(json(serverSession)); }); container.remove();
  localStorage.clear(); for (const [key, value] of previous.storage) localStorage.setItem(key, value); history.replaceState(previous.state, '', previous.url); document.title = previous.title;
  if (previous.theme === null) document.documentElement.removeAttribute('data-theme'); else document.documentElement.setAttribute('data-theme', previous.theme);
  if (originalHitTest) Object.defineProperty(document, 'elementFromPoint', originalHitTest); else delete (document as Partial<Document>).elementFromPoint;
  vi.useRealTimers(); vi.restoreAllMocks(); vi.unstubAllGlobals();
});
function WorkspaceHarness() {
  const route = parseRoute(useLocation()); const [refresh, setRefresh] = useState(0); const searchRef = useRef<HTMLInputElement>(null); const headingRef = useRef<HTMLHeadingElement>(null);
  const ordering = useIssueOrdering({ workspaceId: 'workspace', session, authority: 'account:workspace:1', ready: true, readOnly: false, updated: () => setRefresh(value => value + 1), reload: () => setRefresh(value => value + 1), sourceDenied: vi.fn(), teamDenied: vi.fn() });
  return <><h1 ref={headingRef} tabIndex={-1}>Issue workspace</h1><IssueList workspaceId="workspace" data={data} session={session} params={route.search} refresh={refresh} create={vi.fn()} searchRef={searchRef} headingRef={headingRef} ordering={ordering} updated={() => setRefresh(value => value + 1)} /><IssueOrderingDialog controller={ordering} workspaceId="workspace" session={session} teamDenied={vi.fn()} /></>;
}
async function renderViews(layout = 'list') { history.replaceState({ otIndex: 0 }, '', initialUrl(layout)); await act(async () => root.render(<WorkspaceHarness />)); }
function button(name: string, scope: ParentNode = container) { const value = [...scope.querySelectorAll<HTMLButtonElement>('button')].find(item => item.getAttribute('aria-label') === name || item.textContent?.trim() === name); if (!value) throw new Error(`Missing control ${name}`); return value; }
async function click(name: string, scope?: ParentNode) { await act(async () => button(name, scope).click()); }
async function select(label: string, value: string) { const element = container.querySelector<HTMLSelectElement>(`select[aria-label="${label}"]`)!; expect(element).not.toBeNull(); await act(async () => { element.value = value; element.dispatchEvent(new Event('change', { bubbles: true })); }); }
async function submit(selector = '.issue-order-form') { await act(async () => container.querySelector(selector)!.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }))); }
const positionWrites = () => calls.filter(call => call.path.endsWith('/position') && call.init.method === 'POST');
async function pointer(element: Element, type: string, y: number) { const event = new MouseEvent(type, { bubbles: true, cancelable: true, button: 0, clientX: 20, clientY: y }); Object.defineProperties(event, { pointerId: { value: 1 }, pointerType: { value: 'mouse' }, isPrimary: { value: true } }); await act(async () => { element.dispatchEvent(event); }); }

it.each(['list', 'board'])('wires visible %s Position controls to reviewed same-status placement and cross-status append', async layout => {
  await renderViews(layout); expect(button('Position EX-1').disabled).toBe(false);
  await click('Position EX-1'); await select('Position placement', 'before'); await select('Position reference issue', anchor.id); await submit();
  expect(JSON.parse(positionWrites()[0].init.body as string)).toEqual({ order_version: '1:1', status_id: status.id, before_id: anchor.id }); expect(container.querySelector('dialog')).toBeNull();
  await click('Position EX-1'); await select('Position status', target.id); await submit();
  expect(JSON.parse(positionWrites()[1].init.body as string)).toEqual({ order_version: '2:1', status_id: target.id }); expect(positionWrites()).toHaveLength(2);
  expect(container.querySelector('#row-source')?.closest('[data-order-status]')?.getAttribute('data-order-status')).toBe(target.id);
  expect(calls.filter(call => call.init.method === 'PATCH')).toHaveLength(0);
});

it('uses the actual filtered board hit target as one anchor without inferring an adjacent hidden issue', async () => {
  await renderViews('board'); await act(async () => { navigateConfirmed(initialUrl('board') + '&q=Visible'); });
  expect(container.querySelector('#row-hidden')).toBeNull(); const row = container.querySelector('[data-order-issue="anchor"]')!; expect(row).not.toBeNull();
  vi.spyOn(row, 'getBoundingClientRect').mockReturnValue(new DOMRect(0, 100, 100, 100)); hit = row;
  const handle = button('Drag position EX-1'); await pointer(handle, 'pointerdown', 0); await pointer(handle, 'pointermove', 110); await pointer(handle, 'pointerup', 110);
  expect(positionWrites()).toHaveLength(1); expect(JSON.parse(positionWrites()[0].init.body as string)).toEqual({ order_version: '1:1', status_id: status.id, before_id: anchor.id });
  expect(items.map(item => item.id)).toEqual([source.id, anchor.id, hidden.id]); expect(container.querySelector('#row-hidden')).toBeNull();
});

it('resets the manual board cursor and reloads the first current page after a position acknowledgement', async () => {
  pagination = true; items = [...Array.from({ length: 50 }, (_, index) => ({ ...anchor, id: `first-${index}`, key: `FIRST-${index}`, title: `First page issue ${index}` })), source, anchor];
  await renderViews('board'); await click('Next board page'); expect(container.textContent).toContain('Page 2'); expect(button('Position EX-1').disabled).toBe(false);
  await click('Position EX-1'); const before = calls.length; await submit();
  expect(positionWrites()).toHaveLength(1); expect(container.textContent).toContain('Page 1'); expect(container.querySelector('#row-source')).toBeNull();
  const refreshed = calls.slice(before).filter(call => call.path === issuePath && !call.query.has('status_id'));
  expect(refreshed.length).toBeGreaterThan(0); expect(refreshed.every(call => !call.query.has('cursor'))).toBe(true);
});

it('blocks invalid manual scopes and a missing server order token without broadening their query', async () => {
  await renderViews(); const before = calls.length;
  for (const query of ['sort=manual&layout=list', 'team_id=team&sort=manual&layout=list&archived=true']) {
    await act(async () => { navigateConfirmed(`/w/workspace/issues?${query}`); });
    expect(container.textContent).toContain('Manual ordering requires an explicitly selected'); expect(container.querySelector('#row-source')).toBeNull();
  }
  expect(calls.slice(before).filter(call => call.path === issuePath)).toHaveLength(0);
  omitToken = true; await act(async () => { navigateConfirmed(initialUrl()); });
  expect(container.textContent).toContain('The manual order could not be verified'); expect(container.querySelector('#row-source')).toBeNull(); expect(positionWrites()).toHaveLength(0);
  expect(calls.at(-1)?.query.get('sort')).toBe('manual');
});

it('forwards a full 256-scalar search and keeps created/priority boards on their legacy status-update path', async () => {
  const query = '\u{1f680}'.repeat(256); items = [{ ...source, title: query }];
  history.replaceState({ otIndex: 0 }, '', '/w/workspace/issues?team_id=team'); await act(async () => root.render(<WorkspaceHarness />));
  expect(container.querySelector('[aria-label="Position EX-1"]')).toBeNull(); const input = container.querySelector<HTMLInputElement>('input[type="search"]')!; expect(input.hasAttribute('maxlength')).toBe(false);
  await act(async () => { Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(input, query); input.dispatchEvent(new Event('input', { bubbles: true })); await vi.advanceTimersByTimeAsync(280); });
  expect(calls.filter(call => call.path === issuePath).at(-1)?.query.get('q')).toBe(query);
  await select('Issue order', 'priority'); await select('Issue layout', 'board'); expect(container.querySelector('[aria-label="Position EX-1"]')).toBeNull();
  await click('Move EX-1'); await select('Move EX-1 to status', target.id); await submit('.issue-board-move');
  const patches = calls.filter(call => call.init.method === 'PATCH'); expect(patches).toHaveLength(1); expect(JSON.parse(patches[0].init.body as string)).toEqual({ status_id: target.id }); expect(positionWrites()).toHaveLength(0);
  expect(calls.some(call => call.path === issuePath && call.query.get('sort') === 'priority' && call.query.get('q') === query)).toBe(true);
});

it('retains an uncertain request in the actual App through auth-epoch reconstruction and same-user session/bootstrap refresh', async () => {
  let posts = 0;
  positionReply = () => {
    if (++posts === 1) return Promise.reject(new TypeError('Committed outcome unknown'));
    const canonical = { ...source, title: 'Canonical saved issue after refresh', version: '7', etag: '"issue:source:7"' }; items = [anchor, canonical]; generation = 7;
    return Promise.resolve(json({ issue: { ...source, title: 'Historical cached acknowledgement', version: '2', etag: '"issue:source:2"' }, order_version: '2:1' }));
  };
  await act(async () => root.render(<App />)); await click('Position EX-1'); await submit(); expect(container.textContent).toContain('outcome is uncertain'); expect(hasUnsavedChanges()).toBe(true);
  epoch = '2'; heldSession = deferred(); await act(async () => { window.dispatchEvent(new Event('focus')); });
  expect(container.textContent).toContain('Refreshing workspace access'); expect(container.querySelector('dialog')).toBeNull(); expect(container.textContent).not.toContain(source.title); expect(hasUnsavedChanges()).toBe(true);
  serverSession = { ...session, csrf_token: 'csrf-two' }; const gate = heldSession; heldSession = null; await act(async () => gate.resolve(json(serverSession)));
  expect(button('Check same position request').disabled).toBe(false); expect(posts).toBe(1);
  expect(calls.filter(call => call.path.endsWith('/bootstrap')).length).toBeGreaterThanOrEqual(2);
  await click('Check same position request'); const requests = positionWrites(); expect(requests).toHaveLength(2);
  expect(requests[1].init.body).toBe(requests[0].init.body); for (const name of ['If-Match', 'Idempotency-Key']) expect(new Headers(requests[1].init.headers).get(name)).toBe(new Headers(requests[0].init.headers).get(name));
  expect(new Headers(requests[1].init.headers).get('X-CSRF-Token')).toBe('csrf-two'); expect(container.querySelector('#row-source')?.textContent).toContain('Canonical saved issue after refresh'); expect(container.textContent).not.toContain('Historical cached acknowledgement'); expect(hasUnsavedChanges()).toBe(false);
});

it('fails closed when a manual board lacks its ordering controller instead of exposing legacy status moves', async () => {
  const moved = vi.fn(); const reload = vi.fn();
  await act(async () => root.render(<IssueBoard workspaceId="workspace" teamId="team" data={data} session={session} items={[source, anchor]} statuses={data.statuses} params={new URLSearchParams({ team_id: 'team', layout: 'board', sort: 'manual' })} orderVersion="1:1" disabled={false} readOnly={false} moved={moved} reload={reload} denied={vi.fn()} blockingChanged={vi.fn()} />));
  expect(container.querySelector('[role="alert"]')?.textContent).toContain('Manual ordering controls are unavailable');
  expect(container.querySelector('[aria-label="Move EX-1"], [aria-label="Drag EX-1"], [aria-label="Position EX-1"]')).toBeNull();
  expect(container.querySelector('dialog')).toBeNull(); expect(calls.filter(call => call.init.method === 'PATCH' || call.path.endsWith('/position'))).toHaveLength(0);
  expect(moved).not.toHaveBeenCalled(); expect(reload).not.toHaveBeenCalled();
});
