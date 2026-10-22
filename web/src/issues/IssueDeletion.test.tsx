// @vitest-environment jsdom
import { act, type ReactNode } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { DeleteIssue, IssueDeleteAction } from './IssueDelete';
import { RestoreDeletedIssue, Trash } from './Trash';
import { IssueEditor } from './IssueEditor';
import { App } from '../App';
import { DesktopContext } from '../desktop/context';
import type { DesktopSnapshot, DesktopStore } from '../desktop/store';
import { parseRoute, trashUrl, useLocation } from '../router';
import type { Bootstrap, Issue, Session } from '../types';
import type { DeletedIssue, DeletionPreview } from './deletion-types';

const issue: Issue = { id: 'i', workspace_id: 'w', team_id: 't', key: 'ENG-1', title: 'Review customer feedback', description: 'Keep the useful context.', status_id: 'todo', priority: 0, assignee_id: null, assignee: null, label_ids: [], labels: [], etag: '"issue:i:1:projection"', estimate: null, due_date: null, version: '1', created_at: '2026-10-17T10:00:00Z', updated_at: '2026-10-17T10:00:00Z', archived_at: null };
const session: Session = { audience: 'web', user: { id: 'u', display_name: 'Owner', email: 'owner@example.test' }, csrf_token: 'csrf', workspaces: [{ id: 'w', name: 'Workspace', slug: 'workspace', role: 'owner' }] };
const data: Bootstrap = { auth_epoch: '1', teams: [{ id: 't', name: 'Engineering', key: 'ENG', private: false, archived_at: null }], members: [], statuses: [{ id: 'todo', team_id: 't', name: 'Todo', category: 'unstarted', color: 'slate', position: 0, is_default: true, retiring_at: null, archived_at: null, replacement_status_id: null, replacement_job_id: null }, { id: 'doing', team_id: 't', name: 'In progress', category: 'started', color: 'blue', position: 1, is_default: false, retiring_at: null, archived_at: null, replacement_status_id: null, replacement_job_id: null }], issues: [issue] };
const preview: DeletionPreview = { issue, can_delete: true, child_count: 2, relationship_count: 3, has_parent: true };
const tombstone: DeletedIssue = { issue: { ...issue, version: '2', etag: '"issue:i:2:projection"' }, deleted_at: new Date(Date.now() - 60_000).toISOString(), restore_until: new Date(Date.now() + 30 * 86_400_000).toISOString(), can_restore: true };
const json = (value: unknown, status = 200) => new Response(JSON.stringify(value), { status, headers: { 'Content-Type': 'application/json' } });
const page = (items: DeletedIssue[], cursor: string | null = null) => ({ data: items, next_cursor: cursor, has_more: !!cursor });
const problem = (status: number, code = 'conflict', details?: unknown) => json({ code, detail: 'Request needs review.', details }, status);
let container: HTMLDivElement; let root: Root;
beforeEach(() => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true);
  HTMLDialogElement.prototype.showModal = function () { this.setAttribute('open', ''); };
  HTMLDialogElement.prototype.close = function () { this.removeAttribute('open'); };
  vi.spyOn(window, 'confirm').mockImplementation(() => { throw new Error('Deletion must use a review dialog.'); });
  history.replaceState({ otIndex: 0 }, '', '/w/w/issues/i');
  container = document.createElement('div'); document.body.append(container); root = createRoot(container);
});
afterEach(async () => { await act(async () => root.unmount()); container.remove(); vi.useRealTimers(); vi.restoreAllMocks(); vi.unstubAllGlobals(); });
async function render(node: ReactNode) { await act(async () => root.render(node)); }
function button(label: string) { return [...container.querySelectorAll<HTMLButtonElement>('button')].find(item => item.textContent === label || item.getAttribute('aria-label') === label)!; }
async function click(label: string) { const target = button(label); expect(target, label).toBeDefined(); await act(async () => target.click()); }
async function input(element: HTMLInputElement | HTMLTextAreaElement | HTMLSelectElement, value: string) {
  await act(async () => { Object.getOwnPropertyDescriptor(element instanceof HTMLSelectElement ? HTMLSelectElement.prototype : element instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype, 'value')!.set!.call(element, value); element.dispatchEvent(new Event(element instanceof HTMLSelectElement ? 'change' : 'input', { bubbles: true })); });
}
async function submit() { await act(async () => container.querySelector('form')!.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }))); }
function common(url: string) {
  if (url.endsWith('/workflow')) return json({ team_id: 't', version: '1', can_manage: true, default_status_id: 'todo', statuses: data.statuses });
  if (url.includes('/assignees?')) return json({ data: [], next_cursor: null, has_more: false, self_assignable: false });
  if (url.endsWith('/parent')) return json({ issue_id: 'i', issue_etag: issue.etag, graph_version: '1', parent: null, can_change: true });
  if (/\/(children|relations)\?/.test(url)) return json({ data: [], next_cursor: null, has_more: false, issue_etag: issue.etag, graph_version: '1' });
  if (url.endsWith('/activity')) return json({ data: [] });
  return json(issue);
}

it('shows deletion only after the server allows it and requires saved or discarded edits', async () => {
  const fetch = vi.fn(() => Promise.resolve(json({ ...preview, can_delete: false }))); vi.stubGlobal('fetch', fetch);
  await render(<IssueDeleteAction issue={issue} session={session} dirty={false} disabled={false} deleted={() => {}} />); await click('More issue actions');
  expect(button('Delete issue…')).toBeUndefined(); expect(container.textContent).toContain('No additional actions');
  await click('More issue actions'); fetch.mockImplementation(() => Promise.resolve(json(preview)));
  await render(<IssueDeleteAction issue={issue} session={session} dirty disabled={false} deleted={() => {}} />); await click('More issue actions');
  expect(button('Delete issue…').disabled).toBe(true); expect(container.textContent).toContain('Save or discard your edits');
  expect(fetch.mock.calls).toHaveLength(2);
});

it('preserves the editor draft when deletion preview fails', async () => {
  vi.stubGlobal('fetch', vi.fn((url: string) => Promise.resolve(url.endsWith('/deletion') ? problem(503, 'unavailable') : common(url))));
  await render(<IssueEditor workspaceId="w" issueId="i" data={data} session={session} close={() => {}} updated={() => {}} deleted={() => {}} />);
  await input(container.querySelector<HTMLTextAreaElement>('#issue-title')!, 'My unsaved title'); await click('More issue actions');
  expect(container.querySelector<HTMLTextAreaElement>('#issue-title')!.value).toBe('My unsaved title'); expect(button('Delete issue…')).toBeUndefined();
  expect(container.textContent).toContain('Request needs review.');
});

it('reviews exact detach counts and sends one conditional deletion without a browser confirm', async () => {
  const fetch = vi.fn(() => Promise.resolve(new Response(null, { status: 204 }))); vi.stubGlobal('fetch', fetch); const deleted = vi.fn();
  await render(<DeleteIssue preview={preview} session={session} close={() => {}} deleted={deleted} />);
  expect(container.textContent).toContain('2 direct subissues will be detached. They will not be deleted.');
  expect(container.textContent).toContain('3 relationships will be removed from both ends.'); expect(container.textContent).toContain('Its parent link will be removed.');
  expect(container.textContent).toContain('30 days'); expect(container.textContent).toContain('Archive instead');
  await click('Delete issue'); expect(fetch).toHaveBeenCalledOnce(); expect(deleted).toHaveBeenCalledWith(issue);
  const options = (fetch.mock.calls as unknown as [string, RequestInit][])[0][1]; expect(options.method).toBe('DELETE'); expect(new Headers(options.headers).get('If-Match')).toBe(issue.etag); expect(new Headers(options.headers).get('X-CSRF-Token')).toBe('csrf');
});

it('does not retry a stale deletion until the new issue and detach counts are reviewed', async () => {
  const current = { ...preview, issue: { ...issue, title: 'Changed title', etag: '"issue:i:4:new"' }, child_count: 5 };
  const fetch = vi.fn((url: string, options: RequestInit) => Promise.resolve(options.method === 'DELETE' ? problem(412, 'version_conflict', { current: current.issue }) : url.endsWith('/deletion') ? json(current) : common(url))); vi.stubGlobal('fetch', fetch);
  await render(<DeleteIssue preview={preview} session={session} close={() => {}} deleted={() => {}} />); await click('Delete issue');
  expect(button('Delete issue')).toBeUndefined(); await click('Review latest issue'); expect(container.textContent).toContain('Changed title'); expect(container.textContent).toContain('5 direct subissues');
  expect(fetch.mock.calls.filter(([, options]) => options.method === 'DELETE')).toHaveLength(1);
  await click('Delete issue'); const writes = fetch.mock.calls.filter(([, options]) => options.method === 'DELETE'); expect(new Headers(writes[1][1].headers).get('If-Match')).toBe(current.issue.etag);
});

it('reconciles an interrupted deletion from its tombstone without a second DELETE', async () => {
  const fetch = vi.fn((url: string, options: RequestInit) => options.method === 'DELETE' ? Promise.reject(new TypeError('Connection lost')) : Promise.resolve(url.endsWith('/deleted') ? json(tombstone) : common(url))); vi.stubGlobal('fetch', fetch); const deleted = vi.fn();
  await render(<DeleteIssue preview={preview} session={session} close={() => {}} deleted={deleted} />); await click('Delete issue');
  expect(button('Delete issue')).toBeUndefined(); expect(deleted).not.toHaveBeenCalled(); await click('Check deletion result');
  expect(deleted).toHaveBeenCalledWith(tombstone.issue); expect(fetch.mock.calls.filter(([, options]) => options.method === 'DELETE')).toHaveLength(1);
});

it('keeps deletion uncertain when neither its live nor deleted record can be read', async () => {
  const fetch = vi.fn((_url: string, options: RequestInit) => options.method === 'DELETE' ? Promise.reject(new TypeError('Connection lost')) : Promise.resolve(problem(404, 'not_found'))); vi.stubGlobal('fetch', fetch); const deleted = vi.fn();
  await render(<DeleteIssue preview={preview} session={session} close={() => {}} deleted={deleted} />); await click('Delete issue'); await click('Check deletion result');
  expect(deleted).not.toHaveBeenCalled(); expect(button('Delete issue')).toBeUndefined(); expect(button('Check deletion result')).toBeDefined(); expect(container.textContent).not.toContain(issue.title); expect(container.textContent).not.toContain(issue.description);
});

it('requires an eligible assignee repair and submits every restore reference explicitly', async () => {
  const item = { ...tombstone, issue: { ...tombstone.issue, assignee_id: 'former', assignee: { id: 'former', display_name: 'Former teammate', assignable: false } } };
  const fetch = vi.fn((url: string, options: RequestInit) => Promise.resolve(options.method === 'POST' ? json(issue) : common(url))); vi.stubGlobal('fetch', fetch); const restored = vi.fn();
  await render(<RestoreDeletedIssue item={item} session={session} data={data} close={() => {}} restored={restored} />);
  expect(button('Restore issue').disabled).toBe(true);
  const picker = [...container.querySelectorAll('.reference-picker')].find(item => item.querySelector('.field-label')?.textContent === 'Assignee')!;
  await act(async () => picker.querySelector<HTMLButtonElement>('button')!.click());
  await act(async () => ([...container.querySelectorAll<HTMLElement>('[role=option]')].find(item => item.textContent === 'Unassigned')!).click());
  await submit(); const writes = fetch.mock.calls.filter(([, options]) => options.method === 'POST');
  expect(writes).toHaveLength(1); expect(JSON.parse(writes[0][1].body as string)).toEqual({ status_id: 'todo', assignee_id: null, label_ids: [] });
  expect(new Headers(writes[0][1].headers).get('If-Match')).toBe(item.issue.etag); expect(restored).toHaveBeenCalledWith(issue);
});

it('preserves restore repairs on412 and uses a fresh deleted snapshot only after review', async () => {
  const current = { ...tombstone, issue: { ...tombstone.issue, title: 'Workflow moved this issue', version: '3', etag: '"issue:i:3:projection"' } };
  let posts = 0;
  const fetch = vi.fn((url: string, options: RequestInit) => Promise.resolve(options.method === 'POST' ? ++posts === 1 ? problem(412, 'version_conflict', { current: current.issue }) : json(issue) : url.endsWith('/deleted') ? json(current) : common(url))); vi.stubGlobal('fetch', fetch);
  await render(<RestoreDeletedIssue item={tombstone} session={session} data={data} close={() => {}} restored={() => {}} />);
  await input(container.querySelector<HTMLSelectElement>('.issue-fields select')!, 'doing'); await submit();
  expect(container.textContent).toContain('Your proposed repairs are preserved.'); expect(container.querySelector<HTMLSelectElement>('.issue-fields select')!.value).toBe('doing'); expect(button('Restore issue').disabled).toBe(true);
  await click('Keep repairs for review'); expect(container.querySelector<HTMLSelectElement>('.issue-fields select')!.value).toBe('doing'); await submit();
  const writes = fetch.mock.calls.filter(([, options]) => options.method === 'POST'); expect(writes).toHaveLength(2); expect(new Headers(writes[1][1].headers).get('If-Match')).toBe(current.issue.etag); expect(JSON.parse(writes[1][1].body as string).status_id).toBe('doing');
});

it('reconciles an interrupted restore without repeating its POST or losing repairs', async () => {
  const fetch = vi.fn((url: string, options: RequestInit) => options.method === 'POST' ? Promise.reject(new TypeError('Connection lost')) : Promise.resolve(common(url))); vi.stubGlobal('fetch', fetch); const restored = vi.fn();
  await render(<RestoreDeletedIssue item={tombstone} session={session} data={data} close={() => {}} restored={restored} />); await input(container.querySelector<HTMLSelectElement>('.issue-fields select')!, 'doing'); await submit();
  expect(button('Restore issue')).toBeUndefined(); expect(container.querySelector<HTMLSelectElement>('.issue-fields select')!.value).toBe('doing'); await click('Check restore result');
  expect(restored).toHaveBeenCalledWith(issue); expect(fetch.mock.calls.filter(([, options]) => options.method === 'POST')).toHaveLength(1);
});

it('shows read-only and expired Trash entries without a restore action or a purge promise', async () => {
  vi.stubGlobal('fetch', vi.fn((url: string) => Promise.resolve(common(url))));
  await render(<RestoreDeletedIssue item={{ ...tombstone, can_restore: false }} session={session} data={data} close={() => {}} restored={() => {}} />);
  expect(button('Restore issue')).toBeUndefined(); expect(container.querySelector<HTMLSelectElement>('.issue-fields select')!.matches(':disabled')).toBe(true); expect(container.textContent).toContain('current permissions');
  await render(<RestoreDeletedIssue key="expired" item={{ ...tombstone, restore_until: '2020-01-01T00:00:00Z', can_restore: false }} session={session} data={data} close={() => {}} restored={() => {}} />);
  expect(container.textContent).toContain('30-day restore period has ended'); expect(button('Restore issue')).toBeUndefined(); expect(container.textContent).not.toContain('permanently');
});

it('loads Trash pages, retains rows during refresh, and clears them on denied access', async () => {
  let deny = false; let completeRefresh: ((value: Response) => void) | null = null;
  const second = { ...tombstone, issue: { ...tombstone.issue, id: 'second', key: 'ENG-2', title: 'Another deleted issue' } };
  const fetch = vi.fn((url: string) => deny ? Promise.resolve(problem(403, 'forbidden')) : url.includes('cursor=') ? Promise.resolve(json(page([second]))) : completeRefresh ? new Promise<Response>(resolve => { completeRefresh = resolve; }) : Promise.resolve(json(page([tombstone], 'next')))); vi.stubGlobal('fetch', fetch);
  const view = (refresh: number) => <Trash workspaceId="w" data={data} session={session} params={new URLSearchParams()} refresh={refresh} restored={() => {}} />;
  await render(view(0)); await click('Load more'); expect(container.textContent).toContain(issue.title); expect(container.textContent).toContain(second.issue.title);
  completeRefresh = () => {}; await render(view(1)); expect(container.textContent).toContain(issue.title); expect(container.textContent).toContain('Updating…');
  await act(async () => completeRefresh!(json(page([tombstone])))); deny = true; await render(view(2)); expect(container.textContent).not.toContain(issue.title); expect(container.textContent).toContain('Request needs review.');
});

it('hides previous filter results before the next Trash page arrives', async () => {
  let resolve!: (response: Response) => void;
  vi.stubGlobal('fetch', vi.fn((url: string) => url.includes('q=unrelated') ? new Promise<Response>(done => { resolve = done; }) : Promise.resolve(json(page([tombstone])))));
  await render(<Trash workspaceId="w" data={data} session={session} params={new URLSearchParams()} refresh={0} restored={() => {}} />);
  await render(<Trash workspaceId="w" data={data} session={session} params={new URLSearchParams({ q: 'unrelated' })} refresh={0} restored={() => {}} />);
  expect(container.textContent).not.toContain(issue.title); await act(async () => resolve(json(page([])))); expect(container.textContent).toContain('No deleted issues match.');
});

it('pauses a pending Trash search while a restore detail is open without losing repairs', async () => {
  vi.useFakeTimers();
  history.replaceState({ otIndex: 0 }, '', '/w/w/trash');
  const fetch = vi.fn((url: string) => Promise.resolve(url.includes('/issues/deleted?') ? json(page([tombstone])) : url.endsWith('/deleted') ? json(tombstone) : common(url)));
  vi.stubGlobal('fetch', fetch);
  function RoutedTrash() {
    const route = parseRoute(useLocation());
    return <Trash workspaceId="w" data={data} session={session} params={route.search} selectedId={route.deletedIssueId} refresh={0} restored={() => {}} />;
  }
  await render(<RoutedTrash />);
  await input(container.querySelector<HTMLInputElement>('[aria-label="Search Trash by title or key"]')!, 'feedback');
  await act(async () => vi.advanceTimersByTime(100));
  await act(async () => container.querySelector<HTMLAnchorElement>('.trash-row')!.click());
  expect(location.pathname).toBe('/w/w/trash/i');
  const status = container.querySelector<HTMLSelectElement>('dialog .issue-fields select')!;
  await input(status, 'doing');
  await act(async () => vi.advanceTimersByTime(500));
  expect(location.pathname).toBe('/w/w/trash/i');
  expect(container.querySelector('dialog .issue-fields select')).toBe(status);
  expect(status.value).toBe('doing');
  expect(fetch.mock.calls.some(([url]) => url.includes('q=feedback'))).toBe(false);
  await click('Cancel');
  await act(async () => vi.advanceTimersByTime(280));
  expect(location.pathname).toBe('/w/w/trash');
  expect(new URLSearchParams(location.search).get('q')).toBe('feedback');
  expect(fetch.mock.calls.some(([url]) => url.includes('q=feedback'))).toBe(true);
});

it('routes Trash details separately from live issue editors', () => {
  const url = trashUrl('w', new URLSearchParams({ team_id: 't' }), 'i'); const route = parseRoute(url);
  expect(url).toBe('/w/w/trash/i?team_id=t'); expect(route.area).toBe('trash'); expect(route.deletedIssueId).toBe('i'); expect(route.issueId).toBeUndefined(); expect(route.valid).toBe(true);
});

it('connects workspace navigation and the central command palette to real Trash', async () => {
  history.replaceState({ otIndex: 0 }, '', '/w/w/trash');
  vi.stubGlobal('fetch', vi.fn((url: string) => Promise.resolve(url.endsWith('/setup/status') ? json({ setup_required: false }) : url.endsWith('/auth/session') ? json(session) : url.endsWith('/bootstrap') ? json(data) : url.endsWith('/access') ? json({ auth_epoch: '1', role: 'owner', archived: false }) : url.includes('/issues/deleted?') ? json(page([tombstone])) : common(url))));
  await render(<App />); expect(container.querySelector('.breadcrumbs h1')?.textContent).toBe('Trash'); expect(container.querySelector('a[aria-current=page]')?.textContent).toBe('Trash'); expect(container.textContent).toContain(issue.title); expect(button('New issueC')).toBeUndefined();
  await act(async () => document.body.dispatchEvent(new KeyboardEvent('keydown', { key: 'k', ctrlKey: true, bubbles: true, cancelable: true })));
  expect(container.querySelector('[role=combobox][aria-controls]')).not.toBeNull(); const options = [...container.querySelectorAll('[role=option]')].map(item => item.textContent); expect(options.some(value => value?.includes('Trash'))).toBe(true); expect(options.some(value => value?.includes('Create issue'))).toBe(false);
});

it('keeps native restore repairs and uncertain outcomes through same-account reauthentication', async () => {
  const nativeSession: Session = { ...session, audience: 'desktop', csrf_token: '' };
  const oldLabel = { id: 'old', team_id: null, name: 'Legacy label', color: 'blue' as const, archived_at: '2020-01-01T00:00:00Z' };
  const item: DeletedIssue = { ...tombstone, issue: { ...tombstone.issue, label_ids: ['old'], labels: [oldLabel], assignee_id: 'former', assignee: { id: 'former', display_name: 'Former teammate', assignable: false } } };
  const fetch = vi.fn((url: string, options: RequestInit) => options.method === 'POST' ? Promise.reject(new TypeError('Connection lost')) : Promise.resolve(url.endsWith('/deleted') ? json(item) : url.includes('/issues/deleted?') ? json(page([item])) : common(url))); vi.stubGlobal('fetch', fetch);
  const store = {} as DesktopStore;
  const view = (generation: string) => {
    const snapshot: DesktopSnapshot = { connection: { generation, scope_generation: '1', selected: { origin: 'https://work.example.test', development_http: false }, credential_present: true, pending: null }, session: nativeSession, issueLink: null, authEvent: null, loading: false, error: null };
    return <DesktopContext.Provider value={{ store, snapshot }}><Trash workspaceId="w" data={data} session={nativeSession} params={new URLSearchParams()} selectedId="i" refresh={0} restored={() => {}} /></DesktopContext.Provider>;
  };
  await render(view('1')); await input(container.querySelector<HTMLSelectElement>('dialog .issue-fields select')!, 'doing');
  const picker = [...container.querySelectorAll('.reference-picker')].find(value => value.querySelector('.field-label')?.textContent === 'Assignee')!;
  await act(async () => picker.querySelector<HTMLButtonElement>('button')!.click());
  await act(async () => ([...container.querySelectorAll<HTMLElement>('[role=option]')].find(value => value.textContent === 'Unassigned')!).click());
  await click('Remove Legacy label'); await render(view('2'));
  expect(container.querySelector<HTMLSelectElement>('dialog .issue-fields select')!.value).toBe('doing'); expect(button('Remove Legacy label')).toBeUndefined(); expect(container.querySelector('dialog')!.textContent).toContain('Unassigned');
  await submit(); expect(button('Check restore result')).toBeDefined(); await render(view('3'));
  expect(button('Check restore result')).toBeDefined(); expect(button('Restore issue')).toBeUndefined(); expect(container.querySelector<HTMLSelectElement>('dialog .issue-fields select')!.value).toBe('doing');
  const writes = fetch.mock.calls.filter(([, options]) => options.method === 'POST'); expect(writes).toHaveLength(1); expect(JSON.parse(writes[0][1].body as string)).toEqual({ status_id: 'doing', assignee_id: null, label_ids: [] });
});

it('hides deleted issue content and references immediately after a denied restore', async () => {
  vi.stubGlobal('fetch', vi.fn((url: string, options: RequestInit) => Promise.resolve(options.method === 'POST' ? problem(403, 'forbidden') : common(url))));
  await render(<RestoreDeletedIssue item={tombstone} session={session} data={data} close={() => {}} restored={() => {}} />); await submit();
  expect(container.textContent).not.toContain(issue.title); expect(container.textContent).not.toContain(issue.description); expect(container.querySelector('.reference-picker')).toBeNull(); expect(container.textContent).toContain('Its details have been hidden.');
});
