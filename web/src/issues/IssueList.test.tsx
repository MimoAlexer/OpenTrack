// @vitest-environment jsdom
import { act, createRef } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import type { Bootstrap, Issue, Session, Status } from '../types';
import { IssueList } from './IssueList';

const session: Session = { audience: 'web', user: { id: 'u', display_name: 'Owner', email: 'owner@example.test' }, csrf_token: 'csrf-one', workspaces: [{ id: 'w', name: 'Workspace', slug: 'workspace', role: 'owner' }] };
const status: Status = { id: 's', team_id: 't', name: 'Ready', category: 'unstarted', color: 'slate', position: 0, is_default: true, archived_at: null, retiring_at: null, replacement_status_id: null, replacement_job_id: null };
const issue: Issue = { id: 'i', workspace_id: 'w', team_id: 't', key: 'ENG-1', title: 'Keep this issue visible', description: '', status_id: 's', priority: 0, assignee_id: null, assignee: null, label_ids: [], labels: [], etag: '"issue:i:1:projection"', estimate: null, due_date: null, version: '1', created_at: '2026-10-17T10:00:00Z', updated_at: '2026-10-17T10:00:00Z', archived_at: null };
const data: Bootstrap = { auth_epoch: '1', teams: [{ id: 't', key: 'ENG', name: 'Engineering', private: false, archived_at: null }], statuses: [status], members: [session.user], issues: [] };
const json = (body: unknown, status = 200) => new Response(JSON.stringify(body), { status });
const page = (issues = [issue]) => json({ data: issues, next_cursor: null, has_more: false });
let container: HTMLDivElement; let root: Root;
beforeEach(() => { vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true); container = document.createElement('div'); document.body.append(container); root = createRoot(container); });
afterEach(async () => { await act(async () => root.unmount()); container.remove(); vi.restoreAllMocks(); vi.unstubAllGlobals(); });
async function render(refresh = 0, params = new URLSearchParams(), account = session, workspaceId = 'w') { await act(async () => root.render(<IssueList workspaceId={workspaceId} data={data} session={account} params={params} selectedId="i" refresh={refresh} create={() => {}} searchRef={createRef()} />)); }

it('keeps the same row and scroll container mounted while a same-scope refresh is pending', async () => {
  let finish!: (response: Response) => void;
  vi.stubGlobal('fetch', vi.fn().mockResolvedValueOnce(page()).mockImplementationOnce(() => new Promise<Response>(resolve => { finish = resolve; })));
  await render(); const row = container.querySelector('#row-i'); const list = container.querySelector('.issue-list');
  await render(1);
  expect(container.querySelector('#row-i')).toBe(row); expect(container.querySelector('.issue-list')).toBe(list);
  expect(container.querySelector('.loading')).toBeNull(); expect(container.textContent).toContain('Updating…'); expect(row?.getAttribute('aria-current')).toBe('page');
  await act(async () => finish(page([{ ...issue, title: 'Confirmed updated title' }])));
  expect(container.querySelector('#row-i')).toBe(row); expect(container.textContent).toContain('Confirmed updated title'); expect(container.textContent).not.toContain('Updating…');
});

it.each(['workspace', 'auth'] as const)('hides old rows while a changed %s scope loads', async change => {
  let finish!: (response: Response) => void;
  vi.stubGlobal('fetch', vi.fn().mockResolvedValueOnce(page()).mockImplementationOnce(() => new Promise<Response>(resolve => { finish = resolve; })));
  await render(); expect(container.querySelector('#row-i')).not.toBeNull();
  await render(0, new URLSearchParams(), change === 'auth' ? { ...session, csrf_token: 'csrf-two' } : session, change === 'workspace' ? 'other-workspace' : 'w');
  expect(container.querySelector('#row-i')).toBeNull(); expect(container.textContent).not.toContain(issue.title); expect(container.querySelector('.loading')).not.toBeNull();
  await act(async () => finish(page([])));
  expect(container.querySelector('.loading')).toBeNull(); expect(container.querySelector('#row-i')).toBeNull();
});

it('does not restore old rows from a late previous-filter response', async () => {
  let previous!: (response: Response) => void; let current!: (response: Response) => void;
  vi.stubGlobal('fetch', vi.fn().mockResolvedValueOnce(page()).mockImplementationOnce(() => new Promise<Response>(resolve => { previous = resolve; })).mockImplementationOnce(() => new Promise<Response>(resolve => { current = resolve; })));
  await render(); await render(1); await render(1, new URLSearchParams('priority=1'));
  await act(async () => previous(page()));
  expect(container.querySelector('#row-i')).not.toBeNull(); expect(container.querySelector('.loading')).toBeNull(); expect(container.textContent).toContain('Updating results…');
  await act(async () => current(page([{ ...issue, id: 'new', title: 'Current result' }])));
  expect(container.querySelector('#row-i')).toBeNull(); expect(container.textContent).toContain('Current result');
});

it('keeps same-scope rows on a transient refresh failure and offers a retry', async () => {
  vi.stubGlobal('fetch', vi.fn().mockResolvedValueOnce(page()).mockRejectedValueOnce(new TypeError('Offline')));
  await render(); await render(1);
  expect(container.querySelector('#row-i')).not.toBeNull(); expect(container.textContent).toContain('Could not reach OpenTrack');
  expect([...container.querySelectorAll('button')].some(button => button.textContent === 'Try again')).toBe(true);
});

it.each([401, 403, 404])('removes a retained snapshot when a refresh denies access with %s', async status => {
  vi.stubGlobal('fetch', vi.fn().mockResolvedValueOnce(page()).mockResolvedValueOnce(json({ code: 'not_available', detail: 'This issue list is unavailable.' }, status)));
  await render(); await render(1);
  expect(container.querySelector('#row-i')).toBeNull(); expect(container.textContent).not.toContain(issue.title); expect(container.textContent).toContain('This issue list is unavailable.');
});


it('keeps rows and the search input mounted during a new query, then replaces the result together', async () => {
  let finish!: (response: Response) => void;
  vi.stubGlobal('fetch', vi.fn().mockResolvedValueOnce(page()).mockImplementationOnce(() => new Promise<Response>(resolve => { finish = resolve; })));
  await render(); const row = container.querySelector('#row-i'); const input = container.querySelector('input[type=search]') as HTMLInputElement;
  input.focus();
  await render(0, new URLSearchParams('q=different'));
  expect(container.querySelector('#row-i')).toBe(row); expect(document.activeElement).toBe(input);
  expect(container.querySelector('.loading')).toBeNull(); expect(container.textContent).toContain('Updating results…');
  await act(async () => finish(page([])));
  expect(container.querySelector('#row-i')).toBeNull(); expect(container.textContent).toContain('No issues match these filters.');
});

it('clears previous-query rows when the new query denies access', async () => {
  vi.stubGlobal('fetch', vi.fn().mockResolvedValueOnce(page()).mockResolvedValueOnce(json({ code: 'forbidden', detail: 'Access changed.' }, 403)));
  await render(); await render(0, new URLSearchParams('q=new'));
  expect(container.querySelector('#row-i')).toBeNull(); expect(container.textContent).toContain('Access changed.');
});

it('does not retain rows when the workspace authorization epoch changes', async () => {
  let finish!: (response: Response) => void;
  vi.stubGlobal('fetch', vi.fn().mockResolvedValueOnce(page()).mockImplementationOnce(() => new Promise<Response>(resolve => { finish = resolve; })));
  await render();
  await act(async () => root.render(<IssueList workspaceId="w" data={{ ...data, auth_epoch: '2' }} session={session} params={new URLSearchParams()} refresh={0} create={() => {}} searchRef={createRef()} />));
  expect(container.querySelector('#row-i')).toBeNull();
  await act(async () => finish(page([])));
});
