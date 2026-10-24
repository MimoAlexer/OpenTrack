// @vitest-environment jsdom
import { act, createRef } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import type { Bootstrap, Issue, Session, Status } from '../types';
import { IssueList } from './IssueList';
import { restoreIssueRowFocus } from './focus';
import { issuesUrl, navigate, parseRoute, useLocation, useUnsavedChanges } from '../router';

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
async function renderPrivateBoard(refresh = 0, params = boardParams(), teamAccessLost?: (team: string) => void) { await act(async () => root.render(<IssueList workspaceId="w" data={privateData} session={session} params={params} selectedId="i" refresh={refresh} create={() => {}} searchRef={createRef()} teamAccessLost={teamAccessLost} />)); }
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

it.each([403, 404])('notifies only after fresh known-team workflow proof returns %s', async status => {
  let reads = 0; let workflows = 0; let finish!: (response: Response) => void;
  const lost = vi.fn();
  vi.stubGlobal('fetch', vi.fn((url: string) => {
    if (url.includes('/workflow')) return ++workflows === 1 ? Promise.resolve(privateWorkflow()) : new Promise<Response>(resolve => { finish = resolve; });
    return Promise.resolve(++reads === 1 ? page() : json({ code: 'not_found', detail: 'Issue unavailable.' }, 404));
  }));
  await renderPrivateBoard(0, boardParams(), lost); await renderPrivateBoard(1, boardParams(), lost);
  concealedPrivateBoard(); expect(lost).not.toHaveBeenCalled();
  await act(async () => finish(json({ code: 'not_available', detail: 'Team unavailable.' }, status)));
  expect(lost).toHaveBeenCalledTimes(1); expect(lost).toHaveBeenCalledWith('t'); concealedPrivateBoard();
});

it.each(['authorized', 'unauthenticated', 'network', 'wrong team'] as const)('does not report an ambiguous resource denial as team loss when proof is %s', async outcome => {
  let reads = 0; let workflows = 0;
  const lost = vi.fn();
  vi.stubGlobal('fetch', vi.fn((url: string) => {
    if (url.includes('/workflow')) {
      if (++workflows === 1 || outcome === 'authorized') return Promise.resolve(privateWorkflow());
      if (outcome === 'network') return Promise.reject(new TypeError('Offline'));
      return Promise.resolve(outcome === 'unauthenticated' ? json({ code: 'unauthorized' }, 401) : json({ team_id: 'another-team', version: '1', can_manage: true, statuses: [] }));
    }
    return Promise.resolve(++reads === 1 ? page() : json({ code: 'not_found', detail: 'Issue unavailable.' }, 404));
  }));
  await renderPrivateBoard(0, boardParams(), lost); await renderPrivateBoard(1, boardParams(), lost);
  expect(workflows).toBe(2); expect(lost).not.toHaveBeenCalled(); concealedPrivateBoard();
});

it('does not classify an authorized empty page as team loss', async () => {
  const lost = vi.fn();
  vi.stubGlobal('fetch', vi.fn((url: string) => Promise.resolve(url.includes('/workflow') ? privateWorkflow() : page([]))));
  await renderPrivateBoard(0, boardParams(), lost);
  expect(lost).not.toHaveBeenCalled(); expect(container.textContent).toContain('No issues on this page');
});

it('retains the originating callback when a same-scope proof outlives a callback update', async () => {
  let reads = 0; let workflows = 0; let finish!: (response: Response) => void;
  const original = vi.fn(); const replacement = vi.fn();
  vi.stubGlobal('fetch', vi.fn((url: string) => {
    if (url.includes('/workflow')) return ++workflows === 1 ? Promise.resolve(privateWorkflow()) : new Promise<Response>(resolve => { finish = resolve; });
    return Promise.resolve(++reads === 1 ? page() : json({ code: 'not_found' }, 404));
  }));
  await renderPrivateBoard(0, boardParams(), original); await renderPrivateBoard(1, boardParams(), original);
  await renderPrivateBoard(1, boardParams(), replacement);
  await act(async () => finish(json({ code: 'not_found' }, 404)));
  expect(original).toHaveBeenCalledTimes(1); expect(original).toHaveBeenCalledWith('t'); expect(replacement).not.toHaveBeenCalled();
});

it.each(['principal', 'workspace', 'team', 'filter', 'epoch', 'URL', 'unmount'] as const)('ignores team proof after a changed %s', async change => {
  let reads = 0; let workflows = 0; let finish!: (response: Response) => void;
  const lost = vi.fn(); const previous = { url: `${location.pathname}${location.search}`, state: history.state };
  vi.stubGlobal('fetch', vi.fn((url: string) => {
    if (url.includes('/workflow')) return ++workflows === 2 ? new Promise<Response>(resolve => { finish = resolve; }) : Promise.resolve(privateWorkflow());
    return Promise.resolve(++reads === 2 ? json({ code: 'not_found' }, 404) : page([]));
  }));
  const draw = (props: Partial<Parameters<typeof IssueList>[0]> = {}) => act(async () => root.render(<IssueList workspaceId="w" data={privateData} session={session} params={boardParams()} refresh={reads ? 1 : 0} create={() => {}} searchRef={createRef()} teamAccessLost={lost} {...props}/>));
  try {
    await draw(); await draw();
    expect(workflows).toBe(2); expect(lost).not.toHaveBeenCalled();
    if (change === 'unmount') await act(async () => root.render(null));
    else if (change === 'URL') history.pushState({}, '', '/w/w/issues?team_id=another-team');
    else if (change === 'principal') await draw({ session: { ...session, csrf_token: 'another-session' } });
    else if (change === 'workspace') await draw({ workspaceId: 'another-workspace' });
    else if (change === 'epoch') await draw({ data: { ...privateData, auth_epoch: '2' } });
    else await draw({ params: new URLSearchParams(change === 'team' ? 'team_id=another-team&layout=board&display_version=1' : 'team_id=t&layout=board&display_version=1&q=changed') });
    await act(async () => finish(json({ code: 'not_found' }, 404)));
    expect(lost).not.toHaveBeenCalled();
  } finally { history.replaceState(previous.state, '', previous.url); }
});

it.each([false, true])('keeps exit concealed until authority proof, including a missing parent callback (%s)', async callback => {
  let reads = 0; let workflows = 0; let finish!: (response: Response) => void;
  const lost = vi.fn(); const previous = { url: `${location.pathname}${location.search}`, state: history.state };
  history.replaceState({ otIndex: 0 }, '', '/w/w/issues?team_id=t&status_id=s&layout=board&display_version=1&q=review&priority=2&sort=priority');
  vi.stubGlobal('fetch', vi.fn((url: string) => {
    if (url.includes('/workflow')) {
      workflows += 1;
      return workflows === (callback ? 3 : 2) ? new Promise<Response>(resolve => { finish = resolve; }) : Promise.resolve(privateWorkflow());
    }
    return Promise.resolve(++reads === 1 ? page() : json({ code: 'not_found' }, 404));
  }));
  function Routed({ refresh }: { refresh: number }) {
    const route = parseRoute(useLocation());
    return <IssueList workspaceId="w" data={privateData} session={session} params={route.search} refresh={refresh} create={() => {}} searchRef={createRef()} teamAccessLost={callback ? lost : undefined}/>;
  }
  try {
    await act(async () => root.render(<Routed refresh={0}/>)); await act(async () => root.render(<Routed refresh={1}/>));
    const exit = [...container.querySelectorAll('button')].find(button => button.textContent === 'Open all issues')!; exit.focus();
    await act(async () => exit.click());
    expect(document.activeElement).toBe(exit); expect(exit.disabled).toBe(false); expect(exit.getAttribute('aria-disabled')).toBe('true');
    concealedPrivateBoard(); expect(location.search).toContain('team_id=t'); expect(lost).not.toHaveBeenCalled();
    await act(async () => finish(json({ code: 'not_found' }, 404)));
    concealedPrivateBoard();
    const query = new URLSearchParams(location.search);
    expect(query.get('q')).toBe('review'); expect(query.get('priority')).toBe('2'); expect(query.get('sort')).toBe('priority');
    expect(query.get('team_id')).toBe(callback ? null : 't'); expect(query.get('layout')).toBe(callback ? 'list' : 'board');
    expect(lost).toHaveBeenCalledTimes(callback ? 1 : 0);
    expect(reads).toBe(2);
  } finally { history.replaceState(previous.state, '', previous.url); }
});

it('does not invalidate team authority when a moved issue was deleted but its team remains readable', async () => {
  HTMLDialogElement.prototype.showModal = function () { this.setAttribute('open', ''); };
  HTMLDialogElement.prototype.close = function () { this.removeAttribute('open'); };
  const target = { ...privateStatus, id: 'started', name: 'Started', is_default: false };
  const lost = vi.fn(); let workflows = 0;
  const fetch = vi.fn((url: string, options: RequestInit) => {
    if (url.includes('/workflow')) { workflows += 1; return Promise.resolve(json({ team_id: 't', version: '1', can_manage: true, statuses: [privateStatus, target] })); }
    return Promise.resolve(options.method === 'PATCH' ? json({ code: 'not_found', detail: 'Issue deleted.' }, 404) : page());
  });
  vi.stubGlobal('fetch', fetch);
  await act(async () => root.render(<IssueList workspaceId="w" data={{ ...privateData, statuses: [privateStatus, target] }} session={session} params={boardParams()} refresh={0} create={() => {}} searchRef={createRef()} teamAccessLost={lost}/>));
  await act(async () => container.querySelector<HTMLButtonElement>('[aria-label="Move ENG-1"]')!.click());
  const select = container.querySelector<HTMLSelectElement>('[aria-label="Move ENG-1 to status"]')!;
  await act(async () => { select.value = target.id; select.dispatchEvent(new Event('change', { bubbles: true })); });
  await act(async () => container.querySelector<HTMLButtonElement>('dialog button[type="submit"]')!.click());
  expect(fetch.mock.calls.filter(([, options]) => options.method === 'PATCH')).toHaveLength(1);
  expect(workflows).toBe(2); expect(lost).not.toHaveBeenCalled(); concealedPrivateBoard();
});

it.each([false, true])('uses authorized exit proof while respecting an independent draft navigation guard (%s)', async draft => {
  let reads = 0; let workflows = 0; let finish!: (response: Response) => void;
  const previous = { url: `${location.pathname}${location.search}`, state: history.state };
  history.replaceState({ otIndex: 0 }, '', '/w/w/issues?team_id=t&status_id=s&layout=board&display_version=1&q=review&priority=2&sort=priority');
  const confirm = vi.spyOn(window, 'confirm').mockReturnValue(false);
  vi.stubGlobal('fetch', vi.fn((url: string) => {
    if (url.includes('/workflow')) return ++workflows === 1 ? Promise.resolve(privateWorkflow()) : new Promise<Response>(resolve => { finish = resolve; });
    return Promise.resolve(++reads === 2 ? json({ code: 'not_found' }, 404) : page());
  }));
  function Routed({ refresh }: { refresh: number }) {
    const route = parseRoute(useLocation()); useUnsavedChanges(draft);
    return <><input aria-label="Independent draft" defaultValue="Keep this draft"/><IssueList workspaceId="w" data={privateData} session={session} params={route.search} refresh={refresh} create={() => {}} searchRef={createRef()}/></>;
  }
  try {
    await act(async () => root.render(<Routed refresh={0}/>)); await act(async () => root.render(<Routed refresh={1}/>));
    const input = container.querySelector<HTMLInputElement>('[aria-label="Independent draft"]')!;
    await act(async () => [...container.querySelectorAll('button')].find(button => button.textContent === 'Open all issues')!.click());
    concealedPrivateBoard();
    await act(async () => finish(privateWorkflow()));
    expect(container.querySelector('[aria-label="Independent draft"]')).toBe(input); expect(input.value).toBe('Keep this draft');
    const query = new URLSearchParams(location.search);
    expect(query.get('q')).toBe('review'); expect(query.get('priority')).toBe('2'); expect(query.get('sort')).toBe('priority');
    expect(query.get('team_id')).toBe(draft ? 't' : null); expect(query.get('layout')).toBe(draft ? 'board' : 'list');
    expect(confirm).toHaveBeenCalledTimes(draft ? 1 : 0);
    if (draft) concealedPrivateBoard(); else expect(container.querySelector('#row-i')).not.toBeNull();
  } finally { history.replaceState(previous.state, '', previous.url); }
});

it('retains the original denied team and reports only its fresh recovery proofs after another team is selected', async () => {
  const otherTeam = { ...privateData.teams[0], id: 'other-team', key: 'OTHER', name: 'Other team', private: false };
  const otherStatus = { ...status, id: 'other-status', team_id: otherTeam.id };
  const bootstrap = { ...privateData, teams: [...privateData.teams, otherTeam], statuses: [...privateData.statuses, otherStatus] };
  let reads = 0; let workflows = 0; const proofPaths: string[] = []; const lost = vi.fn();
  vi.stubGlobal('fetch', vi.fn((url: string) => {
    if (url.includes('/workflow')) {
      proofPaths.push(url);
      return Promise.resolve(++workflows === 1 ? privateWorkflow() : json({ code: 'not_found' }, 404));
    }
    reads += 1;
    return Promise.resolve(reads === 2 || reads === 3 ? json({ code: 'not_found' }, 404) : page());
  }));
  const draw = (refresh: number, params = boardParams()) => act(async () => root.render(<IssueList workspaceId="w" data={bootstrap} session={session} params={params} refresh={refresh} create={() => {}} searchRef={createRef()} teamAccessLost={lost}/>));
  await draw(0); await draw(1); expect(lost).toHaveBeenCalledWith('t'); lost.mockClear();
  await draw(1, new URLSearchParams('team_id=other-team&layout=board&display_version=1'));
  concealedPrivateBoard(); expect(reads).toBe(2);
  for (let attempt = 0; attempt < 2; attempt++) {
    await act(async () => [...container.querySelectorAll('button')].find(button => button.textContent === 'Try again')!.click());
    concealedPrivateBoard();
  }
  expect(reads).toBe(4); expect(proofPaths.every(path => path.endsWith('/teams/t/workflow'))).toBe(true);
  expect(lost).toHaveBeenCalledTimes(2); expect(lost).toHaveBeenNthCalledWith(1, 't'); expect(lost).toHaveBeenNthCalledWith(2, 't');
});

it.each([['Try again', 403], ['Try again', 404], ['Open all issues', 403], ['Open all issues', 404]] as const)('reports only a new current retained-source proof from %s after fresh %s', async (recovery, denialStatus) => {
  const previous = { url: location.pathname + location.search, state: history.state };
  const originalParams = new URLSearchParams('team_id=t&layout=board&display_version=1&q=work&priority=2&sort=priority');
  history.replaceState({ otIndex: 0 }, '', issuesUrl('w', originalParams));
  const otherTeam = { ...privateData.teams[0], id: 'other-team', key: 'OTHER', name: 'Accessible team', private: false };
  const otherStatus = { ...status, id: 'other-status', team_id: otherTeam.id };
  const bootstrap = { ...privateData, teams: [...privateData.teams, otherTeam], statuses: [...privateData.statuses, otherStatus] };
  const source = { ...issue, title: 'Retained private work', priority: 2 };
  const control = { ...issue, id: 'public-control', key: 'OTHER-1', team_id: otherTeam.id, status_id: otherStatus.id, title: 'Accessible control work', priority: 2 };
  let reads = 0; let workflows = 0;
  let finishOld: (response: Response) => void = () => {}; let finishCurrent: (response: Response) => void = () => {};
  const obsoleteLost = vi.fn(); const lost = vi.fn(); const proofPaths: string[] = []; const issueQueries: URLSearchParams[] = [];
  const fetch = vi.fn((url: string, _options: RequestInit) => {
    if (url.endsWith('/workflow')) {
      proofPaths.push(url); workflows += 1;
      if (workflows === 1) return Promise.resolve(privateWorkflow());
      if (workflows === 2) return new Promise<Response>(resolve => { finishOld = resolve; });
      return new Promise<Response>(resolve => { finishCurrent = resolve; });
    }
    issueQueries.push(new URLSearchParams(url.split('?')[1])); reads += 1;
    return Promise.resolve(reads === 1 ? page([source]) : reads === 2 ? json({ code: 'not_found', detail: 'Issue unavailable.' }, 404) : page([control]));
  });
  vi.stubGlobal('fetch', fetch);
  function Routed({ refresh }: { refresh: number }) {
    const route = parseRoute(useLocation());
    return <IssueList workspaceId="w" data={bootstrap} session={session} params={route.search} selectedId={route.issueId} refresh={refresh} create={() => {}} searchRef={createRef()} teamAccessLost={route.search.get('team_id') === 't' ? obsoleteLost : lost}/>;
  }
  const noWrites = () => expect(fetch.mock.calls.filter(([, options]) => options.method && options.method !== 'GET')).toEqual([]);
  try {
    await act(async () => root.render(<Routed refresh={0}/>));
    expect(container.querySelector('#row-i')?.textContent).toContain(source.title);
    expect(container.textContent).toContain('Private board team');
    expect(container.textContent).toContain('Private review stage');
    await act(async () => root.render(<Routed refresh={1}/>));
    expect(workflows).toBe(2); concealedPrivateBoard();
    expect(obsoleteLost).not.toHaveBeenCalled(); expect(lost).not.toHaveBeenCalled(); noWrites();
    const next = new URLSearchParams(originalParams); next.set('team_id', otherTeam.id); next.set('layout', 'list');
    await act(async () => { navigate(issuesUrl('w', next)); });
    expect(new URLSearchParams(location.search).get('team_id')).toBe(otherTeam.id);
    expect(new URLSearchParams(location.search).get('layout')).toBe('list');
    await act(async () => finishOld(json({ code: 'not_available', detail: 'Team unavailable.' }, denialStatus)));
    concealedPrivateBoard(); expect(reads).toBe(2);
    expect(obsoleteLost).not.toHaveBeenCalled(); expect(lost).not.toHaveBeenCalled(); noWrites();
    await act(async () => [...container.querySelectorAll('button')].find(button => button.textContent === recovery)!.click());
    expect(workflows).toBe(3); expect(reads).toBe(recovery === 'Try again' ? 3 : 2);
    expect(proofPaths).toEqual(Array(3).fill('/api/v1/workspaces/w/teams/t/workflow'));
    if (recovery === 'Try again') {
      const query = issueQueries.at(-1)!;
      expect(query.get('team_id')).toBe(otherTeam.id); expect(query.get('limit')).toBe('50');
      expect(query.get('q')).toBe('work'); expect(query.get('priority')).toBe('2'); expect(query.get('sort')).toBe('priority');
      expect(query.has('layout')).toBe(false); expect(query.has('display_version')).toBe(false);
    }
    concealedPrivateBoard(); expect(container.querySelector('#row-public-control')).toBeNull();
    expect(obsoleteLost).not.toHaveBeenCalled(); expect(lost).not.toHaveBeenCalled(); noWrites();
    await act(async () => finishCurrent(json({ code: 'not_available', detail: 'Team unavailable.' }, denialStatus)));
    concealedPrivateBoard(); expect(container.textContent).not.toContain(source.title); noWrites();
    expect(obsoleteLost).not.toHaveBeenCalled();
    expect(lost).toHaveBeenCalledTimes(1); expect(lost).toHaveBeenCalledWith('t');
  } finally {
    finishOld(json({ code: 'not_available' }, denialStatus)); finishCurrent(json({ code: 'not_available' }, denialStatus));
    history.replaceState(previous.state, '', previous.url);
  }
});

it.each(['list', 'board'])('settles pending %s refreshes across opening and closing the same-query detail', async layout => {
  const previous = { url: `${location.pathname}${location.search}`, state: history.state };
  history.replaceState({ otIndex: 0 }, '', `/w/w/issues?team_id=t&layout=${layout}&display_version=1`);
  const other = { ...issue, id: 'other', key: 'ENG-2', title: 'Other saved work' };
  const snapshot = (title: string) => json({ data: [{ ...issue, title }, other], next_cursor: 'next-page', has_more: true });
  let reads = 0; let finish!: (response: Response) => void;
  vi.stubGlobal('fetch', vi.fn((url: string) => {
    if (url.includes('/workflow')) return Promise.resolve(privateWorkflow());
    return ++reads === 1 ? Promise.resolve(snapshot(issue.title)) : new Promise<Response>(resolve => { finish = resolve; });
  }));
  function Routed({ refresh }: { refresh: number }) {
    const route = parseRoute(useLocation());
    return <><IssueList workspaceId="w" data={privateData} session={session} params={route.search} selectedId={route.issueId} refresh={refresh} create={() => {}} searchRef={createRef()}/>{route.issueId && <button onClick={() => navigate(issuesUrl('w', route.search))}>Close detail</button>}</>;
  }
  try {
    await act(async () => root.render(<Routed refresh={0}/>)); await act(async () => root.render(<Routed refresh={1}/>));
    await act(async () => container.querySelector<HTMLAnchorElement>('#row-i')!.click());
    expect(location.pathname).toBe('/w/w/issues/i');
    await act(async () => finish(snapshot('Saved while detail opened')));
    expect(container.querySelector('#row-i')?.textContent).toContain('Saved while detail opened');
    expect(container.querySelector('.issue-list')?.getAttribute('aria-busy')).toBe('false');
    if (layout === 'board') {
      expect(container.querySelector<HTMLButtonElement>('[aria-label="Move ENG-2"]')?.disabled).toBe(false);
      expect([...container.querySelectorAll('button')].find(button => button.textContent === 'Next board page')?.getAttribute('aria-disabled')).toBe('false');
    }
    await act(async () => root.render(<Routed refresh={2}/>));
    await act(async () => [...container.querySelectorAll('button')].find(button => button.textContent === 'Close detail')!.click());
    expect(location.pathname).toBe('/w/w/issues');
    await act(async () => finish(snapshot('Saved while detail closed')));
    expect(container.querySelector('#row-i')?.textContent).toContain('Saved while detail closed');
    expect(container.querySelector('.issue-list')?.getAttribute('aria-busy')).toBe('false');
    if (layout === 'board') expect(container.querySelector<HTMLButtonElement>('[aria-label="Move ENG-1"]')?.disabled).toBe(false);
    expect(reads).toBe(3);
  } finally { history.replaceState(previous.state, '', previous.url); }
});

it.each([403, 404])('processes current-team %s after detail changes during both pending reads', async status => {
  const previous = { url: `${location.pathname}${location.search}`, state: history.state };
  history.replaceState({ otIndex: 0 }, '', '/w/w/issues?team_id=t&layout=board&display_version=1');
  let reads = 0; let workflows = 0; let finishPage!: (response: Response) => void; let finishProof!: (response: Response) => void;
  const lost = vi.fn();
  vi.stubGlobal('fetch', vi.fn((url: string) => {
    if (url.includes('/workflow')) return ++workflows === 1 ? Promise.resolve(privateWorkflow()) : new Promise<Response>(resolve => { finishProof = resolve; });
    return ++reads === 1 ? Promise.resolve(page()) : new Promise<Response>(resolve => { finishPage = resolve; });
  }));
  function Routed({ refresh }: { refresh: number }) {
    const route = parseRoute(useLocation());
    return <><IssueList workspaceId="w" data={privateData} session={session} params={route.search} selectedId={route.issueId} refresh={refresh} create={() => {}} searchRef={createRef()} teamAccessLost={lost}/>{route.issueId && <button onClick={() => navigate(issuesUrl('w', route.search))}>Close detail</button>}</>;
  }
  try {
    await act(async () => root.render(<Routed refresh={0}/>)); await act(async () => root.render(<Routed refresh={1}/>));
    await act(async () => container.querySelector<HTMLAnchorElement>('#row-i')!.click());
    await act(async () => finishPage(json({ code: 'not_found', detail: 'Issue unavailable.' }, 404)));
    expect(workflows).toBe(2); expect(lost).not.toHaveBeenCalled(); concealedPrivateBoard();
    await act(async () => [...container.querySelectorAll('button')].find(button => button.textContent === 'Close detail')!.click());
    await act(async () => finishProof(json({ code: 'not_available', detail: 'Team unavailable.' }, status)));
    expect(lost).toHaveBeenCalledTimes(1); expect(lost).toHaveBeenCalledWith('t'); concealedPrivateBoard();
    expect(container.querySelector('.issue-list')?.getAttribute('aria-busy')).toBe('false');
  } finally { history.replaceState(previous.state, '', previous.url); }
});

it.each([200, 404])('does not replay an obsolete exit intent after detail navigation when proof returns %s', async status => {
  const previous = { url: `${location.pathname}${location.search}`, state: history.state };
  const query = '?team_id=t&layout=board&display_version=1';
  history.replaceState({ otIndex: 0 }, '', `/w/w/issues${query}`);
  let reads = 0; let workflows = 0; let finish!: (response: Response) => void; const lost = vi.fn();
  vi.stubGlobal('fetch', vi.fn((url: string) => {
    if (url.includes('/workflow')) return ++workflows === 3 ? new Promise<Response>(resolve => { finish = resolve; }) : Promise.resolve(privateWorkflow());
    return Promise.resolve(++reads === 1 ? page() : json({ code: 'not_found' }, 404));
  }));
  try {
    await renderPrivateBoard(0, boardParams(), lost); await renderPrivateBoard(1, boardParams(), lost);
    await act(async () => [...container.querySelectorAll('button')].find(button => button.textContent === 'Open all issues')!.click());
    history.pushState({}, '', `/w/w/issues/another-issue${query}`);
    await act(async () => finish(status === 200 ? privateWorkflow() : json({ code: 'not_found' }, 404)));
    expect(location.pathname).toBe('/w/w/issues/another-issue'); expect(location.search).toBe(query);
    expect(lost).toHaveBeenCalledTimes(status === 404 ? 1 : 0); concealedPrivateBoard();
    expect(container.querySelector('.issue-list')?.getAttribute('aria-busy')).toBe('false');
  } finally { history.replaceState(previous.state, '', previous.url); }
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
