// @vitest-environment jsdom
import { act, createRef } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import type { Bootstrap, Issue, Session, Status } from '../types';
import { IssueList } from './IssueList';
import { restoreIssueRowFocus } from './focus';
import { issuesUrl, navigate, parseRoute, useLocation } from '../router';

const session: Session = { audience: 'web', user: { id: 'u', display_name: 'Owner', email: 'owner@example.test' }, csrf_token: 'csrf-one', workspaces: [{ id: 'w', name: 'Workspace', slug: 'workspace', role: 'owner' }] };
const status: Status = { id: 's', team_id: 't', name: 'Ready', category: 'unstarted', color: 'slate', position: 0, is_default: true, archived_at: null, retiring_at: null, replacement_status_id: null, replacement_job_id: null };
const issue: Issue = { id: 'i', workspace_id: 'w', team_id: 't', key: 'ENG-1', title: 'Keep this issue visible', description: '', status_id: 's', priority: 0, assignee_id: null, assignee: null, label_ids: [], labels: [], etag: '"issue:i:1:projection"', estimate: null, due_date: null, version: '1', created_at: '2026-10-17T10:00:00Z', updated_at: '2026-10-17T10:00:00Z', archived_at: null };
const data: Bootstrap = { auth_epoch: '1', teams: [{ id: 't', key: 'ENG', name: 'Engineering', private: false, archived_at: null }], statuses: [status], members: [session.user], issues: [] };
const json = (body: unknown, status = 200) => new Response(JSON.stringify(body), { status });
const page = (issues = [issue]) => json({ data: issues, next_cursor: null, has_more: false });
let container: HTMLDivElement; let root: Root;
beforeEach(() => { vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true); container = document.createElement('div'); document.body.append(container); root = createRoot(container); });
afterEach(async () => { await act(async () => root.unmount()); container.remove(); vi.useRealTimers(); vi.restoreAllMocks(); vi.unstubAllGlobals(); });
async function render(refresh = 0, params = new URLSearchParams(), account = session, workspaceId = 'w') { await act(async () => root.render(<IssueList workspaceId={workspaceId} data={data} session={account} params={params} selectedId="i" refresh={refresh} create={() => {}} searchRef={createRef()} />)); }

const boardParams = () => new URLSearchParams('team_id=t&layout=board&display_version=1');
const workflow = () => json({ team_id: 't', version: '1', can_manage: true, statuses: [status] });
const privateStatus = { ...status, name: 'Private review stage' };
const privateData: Bootstrap = { ...data, teams: [{ ...data.teams[0], name: 'Private board team', private: true }], statuses: [privateStatus] };
const privateWorkflow = () => json({ team_id: 't', version: '1', can_manage: true, statuses: [privateStatus] });
async function renderPrivateBoard(refresh = 0, params = boardParams()) { await act(async () => root.render(<IssueList workspaceId="w" data={privateData} session={session} params={params} selectedId="i" refresh={refresh} create={() => {}} searchRef={createRef()} />)); }
function concealedPrivateBoard() {
  expect(container.textContent).not.toContain('Private board team');
  expect(container.textContent).not.toContain('Private review stage');
  expect(container.querySelector('[aria-label="Board team"]')).toBeNull();
  expect(container.querySelector('option[value="t"], option[value="s"]')).toBeNull();
  expect(container.querySelector('#row-i')).toBeNull();
}

it.each([[403, 'refresh'], [404, 'refresh'], [403, 'retry'], [404, 'retry']] as const)('conceals denied private board metadata after %s during %s', async (statusCode, action) => {
  let denied = false; let holding = false; let holdWorkflow = false; let reads = 0; let workflowReads = 0;
  let finish!: (response: Response) => void; let finishWorkflow!: (response: Response) => void;
  vi.stubGlobal('fetch', vi.fn((url: string) => {
    if (url.includes('/workflow')) {
      workflowReads += 1;
      return holdWorkflow ? new Promise<Response>(resolve => { finishWorkflow = resolve; }) : Promise.resolve(privateWorkflow());
    }
    reads += 1;
    if (holding) return new Promise<Response>(resolve => { finish = resolve; });
    return Promise.resolve(denied ? json({ code: 'not_available', detail: 'Board access changed.' }, statusCode) : page());
  }));
  await renderPrivateBoard();
  expect(container.textContent).toContain('Private board team');
  denied = true; await renderPrivateBoard(1); concealedPrivateBoard();
  holding = true;
  if (action === 'refresh') {
    await renderPrivateBoard(2); concealedPrivateBoard(); expect(reads).toBe(2);
  }
  await act(async () => [...container.querySelectorAll('button')].find(button => button.textContent === 'Try again')!.click());
  concealedPrivateBoard(); expect(reads).toBe(3);
  denied = false; holding = false; holdWorkflow = true;
  await act(async () => finish(page()));
  concealedPrivateBoard(); expect(workflowReads).toBe(2);
  holdWorkflow = false;
  await act(async () => finishWorkflow(privateWorkflow()));
  expect(container.textContent).toContain('Private board team');
  expect(container.textContent).toContain('Private review stage');
  expect(container.querySelector('#row-i')).not.toBeNull();
});

it.each(['404', 'network', 'wrong team'] as const)('does not restore a denied board from an empty list when fresh workflow proof fails with %s', async failure => {
  let reads = 0; let workflowReads = 0;
  let finish!: (response: Response) => void; let reject!: (error: Error) => void;
  vi.stubGlobal('fetch', vi.fn((url: string) => {
    if (url.includes('/workflow')) {
      workflowReads += 1;
      return workflowReads === 1 ? Promise.resolve(privateWorkflow()) : new Promise<Response>((resolve, fail) => { finish = resolve; reject = fail; });
    }
    reads += 1;
    return Promise.resolve(reads === 1 ? page() : reads === 2 ? json({ code: 'not_found', detail: 'Board unavailable.' }, 404) : page([]));
  }));
  await renderPrivateBoard();
  await renderPrivateBoard(1); concealedPrivateBoard();
  await act(async () => [...container.querySelectorAll('button')].find(button => button.textContent === 'Try again')!.click());
  expect(reads).toBe(3); expect(workflowReads).toBe(2); concealedPrivateBoard();
  await act(async () => {
    if (failure === 'network') reject(new TypeError('Offline'));
    else finish(failure === '404' ? json({ code: 'not_found', detail: 'Board unavailable.' }, 404)
      : json({ team_id: 'another-team', version: '1', can_manage: true, statuses: [] }));
  });
  concealedPrivateBoard();
  const retry = [...container.querySelectorAll('button')].find(button => button.textContent === 'Try again');
  expect(retry).toBeDefined(); expect(retry?.disabled).toBe(false);
  expect(reads).toBe(3);
});

it('retains denial for the same team, principal and epoch across filter changes and back until explicit retry', async () => {
  let denied = false; let reads = 0;
  vi.stubGlobal('fetch', vi.fn((url: string) => {
    if (url.includes('/workflow')) return Promise.resolve(privateWorkflow());
    reads += 1;
    return Promise.resolve(denied ? json({ code: 'forbidden', detail: 'Board unavailable.' }, 403) : page());
  }));
  await renderPrivateBoard();
  denied = true; await renderPrivateBoard(1); concealedPrivateBoard();
  const changed = boardParams(); changed.set('q', 'changed query'); changed.set('sort', 'priority');
  await renderPrivateBoard(1, changed); concealedPrivateBoard();
  await renderPrivateBoard(1, boardParams()); concealedPrivateBoard();
  expect(reads).toBe(2);
  denied = false;
  await act(async () => [...container.querySelectorAll('button')].find(button => button.textContent === 'Try again')!.click());
  expect(reads).toBe(3);
  expect(container.textContent).toContain('Private board team');
  expect(container.textContent).toContain('Private review stage');
  expect(container.querySelector('#row-i')).not.toBeNull();
});

it('fences a superseded workflow proof and page until the latest refresh receives its own proof', async () => {
  let reads = 0; let workflowReads = 0;
  let finishOldProof!: (response: Response) => void; let finishLatestPage!: (response: Response) => void; let finishLatestProof!: (response: Response) => void;
  const old = { ...issue, title: 'Superseded reviewed page' }; const latest = { ...issue, title: 'Current reviewed page' };
  vi.stubGlobal('fetch', vi.fn((url: string) => {
    if (url.includes('/workflow')) {
      workflowReads += 1;
      if (workflowReads === 2) return new Promise<Response>(resolve => { finishOldProof = resolve; });
      if (workflowReads === 3) return new Promise<Response>(resolve => { finishLatestProof = resolve; });
      return Promise.resolve(privateWorkflow());
    }
    reads += 1;
    if (reads === 2) return Promise.resolve(json({ code: 'not_found', detail: 'Board unavailable.' }, 404));
    if (reads === 3) return Promise.resolve(page([old]));
    if (reads === 4) return new Promise<Response>(resolve => { finishLatestPage = resolve; });
    return Promise.resolve(page());
  }));
  await renderPrivateBoard(); await renderPrivateBoard(1); concealedPrivateBoard();
  await act(async () => [...container.querySelectorAll('button')].find(button => button.textContent === 'Try again')!.click());
  expect(workflowReads).toBe(2); concealedPrivateBoard();
  await renderPrivateBoard(2); expect(reads).toBe(4); concealedPrivateBoard();
  await act(async () => finishOldProof(privateWorkflow()));
  concealedPrivateBoard(); expect(container.textContent).not.toContain(old.title);
  await act(async () => finishLatestPage(page([latest])));
  expect(workflowReads).toBe(3); concealedPrivateBoard();
  await act(async () => finishLatestProof(privateWorkflow()));
  expect(container.textContent).toContain(latest.title);
  expect(container.textContent).not.toContain(old.title);
  expect(container.textContent).toContain('Private board team');
});

it('requires an explicit team and rejects unsupported board display versions without broadening the query', async () => {
  const fetch = vi.fn(); vi.stubGlobal('fetch', fetch);
  await render(0, new URLSearchParams('layout=board&display_version=1'));
  expect(container.textContent).toContain('Choose a team for this board.');
  expect(fetch).not.toHaveBeenCalled();
  await render(0, new URLSearchParams('team_id=t&layout=board&display_version=2&q=retained'));
  expect(container.textContent).toContain('These display settings are unavailable.');
  expect(fetch).not.toHaveBeenCalled();
});

it('refetches a real first page when switching from an appended list to a bounded board and replaces subsequent pages', async () => {
  const second = { ...issue, id: 'second', key: 'ENG-2', title: 'Second page issue' };
  const calls: URLSearchParams[] = [];
  vi.stubGlobal('fetch', vi.fn((url: string) => {
    if (url.includes('/workflow')) return Promise.resolve(workflow());
    const params = new URLSearchParams(url.split('?')[1]); calls.push(params);
    return Promise.resolve(params.has('cursor') ? json({ data: [second], next_cursor: null, has_more: false }) : json({ data: [issue], next_cursor: 'second-page', has_more: true }));
  }));
  await render(0, new URLSearchParams('team_id=t'));
  await act(async () => [...container.querySelectorAll('button')].find(button => button.textContent === 'Load more issues')!.click());
  expect(container.querySelectorAll('a.issue-row')).toHaveLength(2);
  await render(0, boardParams());
  expect(container.querySelectorAll('a.issue-row')).toHaveLength(1);
  expect(container.querySelector('#row-i')).not.toBeNull();
  expect(container.querySelector('#row-second')).toBeNull();
  expect(calls.at(-1)?.has('cursor')).toBe(false);
  expect(calls.every(params => !params.has('layout') && !params.has('display_version') && params.get('limit') === '50')).toBe(true);
  await act(async () => [...container.querySelectorAll('button')].find(button => button.textContent === 'Next board page')!.click());
  expect(container.querySelectorAll('a.issue-row')).toHaveLength(1);
  expect(container.querySelector('#row-i')).toBeNull();
  expect(container.querySelector('#row-second')).not.toBeNull();
  await act(async () => [...container.querySelectorAll('button')].find(button => button.textContent === 'Previous board page')!.click());
  expect(container.querySelector('#row-i')).not.toBeNull();
  expect(container.querySelector('#row-second')).toBeNull();
});

it('resets a board cursor on filter changes and hides the previous page until the new authorized scope arrives', async () => {
  let finish!: (response: Response) => void;
  const queries: URLSearchParams[] = [];
  vi.stubGlobal('fetch', vi.fn((url: string) => {
    if (url.includes('/workflow')) return Promise.resolve(workflow());
    const params = new URLSearchParams(url.split('?')[1]); queries.push(params);
    if (params.get('q') === 'changed') return new Promise<Response>(resolve => { finish = resolve; });
    return Promise.resolve(json({ data: [issue], next_cursor: 'next-page', has_more: true }));
  }));
  await render(0, boardParams());
  await act(async () => [...container.querySelectorAll('button')].find(button => button.textContent === 'Next board page')!.click());
  expect(queries.at(-1)?.get('cursor')).toBe('next-page');
  const changed = boardParams(); changed.set('q', 'changed');
  await render(0, changed);
  expect(queries.at(-1)?.has('cursor')).toBe(false);
  expect(container.querySelector('#row-i')).toBeNull();
  expect(container.textContent).not.toContain(issue.title);
  await act(async () => finish(page([])));
  expect(container.querySelector('#row-i')).toBeNull();
});

it('removes board cards and cached team names after a permission failure', async () => {
  let denied = false;
  vi.stubGlobal('fetch', vi.fn((url: string) => Promise.resolve(url.includes('/workflow') ? workflow() : denied ? json({ code: 'forbidden', detail: 'Access changed.' }, 403) : page())));
  await render(0, boardParams());
  expect(container.querySelector('#row-i')).not.toBeNull();
  denied = true; await render(1, boardParams());
  expect(container.textContent).toContain('Board unavailable.');
  expect(container.querySelector('#row-i')).toBeNull();
  expect(container.textContent).not.toContain('Engineering');
  expect(container.textContent).not.toContain(issue.title);
});

it('fences a late listing response after independent workflow revocation until an explicit retry', async () => {
  let finish!: (response: Response) => void;
  let denyWorkflow = false; let listReads = 0;
  vi.stubGlobal('fetch', vi.fn((url: string) => {
    if (url.includes('/workflow')) return Promise.resolve(denyWorkflow ? json({ code: 'not_found', detail: 'Board unavailable.' }, 404) : workflow());
    listReads += 1;
    return listReads === 1 ? Promise.resolve(page()) : new Promise<Response>(resolve => { finish = resolve; });
  }));
  await render(0, boardParams());
  expect(container.querySelector('#row-i')).not.toBeNull();
  await render(1, boardParams());
  denyWorkflow = true;
  await act(async () => window.dispatchEvent(new Event('focus')));
  expect(container.querySelector('#row-i')).toBeNull();
  expect(container.textContent).not.toContain('Engineering');
  await act(async () => finish(page()));
  expect(container.querySelector('#row-i')).toBeNull();
  expect(container.textContent).not.toContain(issue.title);
  await render(2, boardParams());
  expect(listReads).toBe(2);
  expect(container.querySelector('#row-i')).toBeNull();
  denyWorkflow = false;
  await act(async () => [...container.querySelectorAll('button')].find(button => button.textContent === 'Try again')!.click());
  expect(listReads).toBe(3);
  expect(container.querySelector('#row-i')).toBeNull();
  expect(container.querySelector('[aria-label="Board team"]')).toBeNull();
  expect(container.textContent).not.toContain('Engineering');
  expect(container.textContent).not.toContain('Ready');
  await act(async () => finish(page()));
  expect(container.querySelector('#row-i')).not.toBeNull();
});

it('keeps the activated board page control focused through pending and completed page replacement', async () => {
  let finish!: (response: Response) => void;
  vi.stubGlobal('fetch', vi.fn((url: string) => url.includes('/workflow') ? Promise.resolve(workflow()) : url.includes('cursor=') ? new Promise<Response>(resolve => { finish = resolve; }) : Promise.resolve(json({ data: [issue], next_cursor: 'next-page', has_more: true }))));
  await render(0, boardParams());
  const next = [...container.querySelectorAll('button')].find(button => button.textContent === 'Next board page')!;
  next.focus();
  await act(async () => next.click());
  expect(next.isConnected).toBe(true);
  expect(document.activeElement).toBe(next);
  expect(next.getAttribute('aria-disabled')).toBe('true');
  await act(async () => finish(page([])));
  expect([...container.querySelectorAll('button')].find(button => button.textContent === 'Next board page')).toBe(next);
  expect(document.activeElement).toBe(next);
});

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

it('pauses a pending list search while an issue is selected, then applies it after closing without losing filters', async () => {
  vi.useFakeTimers();
  const previous = { url: `${location.pathname}${location.search}`, state: history.state };
  history.replaceState({ otIndex: 0 }, '', '/w/w/issues?team_id=t&sort=priority&q=Keep');
  const fetch = vi.fn((_url: string) => Promise.resolve(page())); vi.stubGlobal('fetch', fetch);
  function RoutedIssueList() {
    const route = parseRoute(useLocation());
    return <><IssueList workspaceId="w" data={data} session={session} params={route.search} selectedId={route.issueId} refresh={0} create={() => {}} searchRef={createRef()} />{route.issueId && <button onClick={() => navigate(issuesUrl('w', route.search))}>Close issue</button>}</>;
  }
  try {
    await act(async () => root.render(<RoutedIssueList />));
    const input = container.querySelector<HTMLInputElement>('input[type=search]')!;
    await act(async () => { Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(input, 'visible'); input.dispatchEvent(new Event('input', { bubbles: true })); });
    await act(async () => vi.advanceTimersByTime(100));
    await act(async () => container.querySelector<HTMLAnchorElement>('#row-i')!.click());
    expect(location.pathname).toBe('/w/w/issues/i');
    expect(location.search).toBe('?team_id=t&sort=priority&q=Keep');
    await act(async () => vi.advanceTimersByTime(500));
    expect(location.pathname).toBe('/w/w/issues/i');
    expect(location.search).toBe('?team_id=t&sort=priority&q=Keep');
    expect(input.value).toBe('visible');
    expect(container.querySelector('#row-i')?.getAttribute('aria-current')).toBe('page');
    expect(fetch.mock.calls.some(([url]) => new URLSearchParams(url.split('?')[1]).get('q') === 'visible')).toBe(false);
    await act(async () => [...container.querySelectorAll<HTMLButtonElement>('button')].find(button => button.textContent === 'Close issue')!.click());
    await act(async () => vi.advanceTimersByTime(280));
    expect(location.pathname).toBe('/w/w/issues');
    expect(location.search).toBe('?team_id=t&sort=priority&q=visible');
    expect(fetch.mock.calls.some(([url]) => new URLSearchParams(url.split('?')[1]).get('q') === 'visible')).toBe(true);
  } finally { history.replaceState(previous.state, '', previous.url); }
});

async function rowKey(target: Element, key: string, options: KeyboardEventInit = {}, altGraph = false) {
  const event = new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true, ...options });
  if (altGraph) Object.defineProperty(event, 'getModifierState', { value: (name: string) => name === 'AltGraph' });
  await act(async () => target.dispatchEvent(event));
  return event;
}

it('moves across rendered status groups with bounded arrows and Home/End while leaving link keys native', async () => {
  const done: Status = { ...status, id: 'done', name: 'Done', category: 'completed' };
  vi.stubGlobal('fetch', vi.fn().mockResolvedValue(page([{ ...issue, id: 'last', status_id: done.id }, issue, { ...issue, id: 'next' }])));
  await act(async () => root.render(<IssueList workspaceId="w" data={{ ...data, statuses: [status, done] }} session={session} params={new URLSearchParams()} refresh={0} create={() => {}} searchRef={createRef()} />));
  const rows = [...container.querySelectorAll<HTMLAnchorElement>('a.issue-row')];
  expect(rows.map(row => row.id)).toEqual(['row-i', 'row-next', 'row-last']);
  rows[0].focus();
  for (const [key, index] of [['ArrowUp', 0], ['ArrowDown', 1], ['ArrowDown', 2], ['ArrowDown', 2], ['Home', 0], ['End', 2], ['ArrowUp', 1]] as const) {
    expect((await rowKey(document.activeElement!, key)).defaultPrevented).toBe(true);
    expect(document.activeElement).toBe(rows[index]);
  }
  for (const key of ['Enter', 'Tab', ' ', 'ArrowLeft', 'ArrowRight']) {
    expect((await rowKey(rows[1], key)).defaultPrevented).toBe(false);
    expect(document.activeElement).toBe(rows[1]);
  }
  expect(rows.every(row => !row.hasAttribute('tabindex'))).toBe(true);
  expect(rows[1].getAttribute('href')).toBe('/w/w/issues/next');
});

it('keeps composition, modifiers and non-row controls outside row navigation', async () => {
  vi.stubGlobal('fetch', vi.fn().mockResolvedValue(page([issue, { ...issue, id: 'next' }])));
  await render();
  const row = container.querySelector<HTMLAnchorElement>('#row-i')!;
  row.focus();
  for (const options of [{ isComposing: true }, { keyCode: 229 }, { altKey: true }, { ctrlKey: true }, { metaKey: true }, { shiftKey: true }]) {
    for (const key of ['ArrowUp', 'ArrowDown', 'Home', 'End']) {
      expect((await rowKey(row, key, options)).defaultPrevented).toBe(false);
      expect(document.activeElement).toBe(row);
    }
  }
  expect((await rowKey(row, 'End', {}, true)).defaultPrevented).toBe(false);
  await act(async () => row.dispatchEvent(new CompositionEvent('compositionstart', { bubbles: true })));
  expect((await rowKey(row, 'End')).defaultPrevented).toBe(false);
  expect(document.activeElement).toBe(row);
  await act(async () => row.dispatchEvent(new CompositionEvent('compositionend', { bubbles: true })));
  for (const control of [container.querySelector('input')!, container.querySelector('select')!]) {
    (control as HTMLElement).focus();
    expect((await rowKey(control, 'Home')).defaultPrevented).toBe(false);
    expect(document.activeElement).toBe(control);
  }
  row.focus();
  expect((await rowKey(row, 'End')).defaultPrevented).toBe(true);
  expect(document.activeElement?.id).toBe('row-next');
});

it.each(['neighbor', 'heading', 'search', 'account'] as const)('recovers a closed origin after the delayed list commit while respecting %s focus', async destination => {
  let finish!: (response: Response) => void;
  const frames: FrameRequestCallback[] = [];
  vi.stubGlobal('requestAnimationFrame', (callback: FrameRequestCallback) => { frames.push(callback); return frames.length; });
  const flushFrames = () => act(async () => { frames.splice(0).forEach(callback => callback(0)); });
  vi.stubGlobal('fetch', vi.fn().mockResolvedValueOnce(page([{ ...issue, id: 'before' }, issue, { ...issue, id: 'after' }]))
    .mockImplementationOnce(() => new Promise<Response>(resolve => { finish = resolve; }))
    .mockResolvedValueOnce(page([{ ...issue, id: 'other-account', title: 'Other account issue' }])));
  const headingRef = createRef<HTMLHeadingElement>();
  const draw = (refresh: number, account = session) => act(async () => root.render(<><header><h1 ref={headingRef} tabIndex={-1}>All issues</h1></header><main><IssueList workspaceId="w" data={data} session={account} params={new URLSearchParams()} selectedId="i" refresh={refresh} create={() => {}} searchRef={createRef()} headingRef={headingRef} /></main></>));
  await draw(0);
  const row = container.querySelector<HTMLAnchorElement>('#row-i')!;
  const rowIds = [...container.querySelectorAll<HTMLAnchorElement>('a.issue-row')].map(item => item.id);
  expect(container.querySelector('main h1')).toBeNull();
  await draw(1);
  const close = document.createElement('button'); document.body.append(close); close.focus(); close.remove();
  restoreIssueRowFocus('i', rowIds, headingRef.current); await flushFrames();
  expect(document.activeElement).toBe(row);
  await act(async () => {
    finish(page(destination === 'heading' ? [] : [{ ...issue, id: 'before' }, { ...issue, id: 'after' }]));
    await new Promise<void>(resolve => setImmediate(resolve));
    expect(row.isConnected).toBe(true);
    frames.splice(0).forEach(callback => callback(0));
  });
  expect(row.isConnected).toBe(false);
  const search = container.querySelector<HTMLInputElement>('input[type=search]')!;
  if (destination === 'search') search.focus();
  if (destination === 'account') await draw(1, { ...session, csrf_token: 'csrf-two' });
  await flushFrames();
  const expected = destination === 'neighbor' ? container.querySelector('#row-after') : destination === 'heading' ? headingRef.current : destination === 'search' ? search : document.body;
  expect(document.activeElement).toBe(expected);
});
