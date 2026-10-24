// @vitest-environment jsdom
import { act, useLayoutEffect, useRef, useState } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { App } from './App';
import { advanceSessionGeneration, ApiError, currentSessionGeneration } from './api';
import { Modal } from './components';
import { hasUnsavedChanges, issuesUrl, navigate, useUnsavedChanges } from './router';
import type { Bootstrap, Issue, Session, Status } from './types';

type BoardProps = { items: Issue[]; session: Session; disabled: boolean; denied: (error: unknown) => void; reload: () => void };
type CreatorWitness = { session: Session; node: HTMLDivElement };
let boardRenders: BoardProps[], mountedCreator: CreatorWitness | null;
vi.mock('./issues/IssueBoard', () => ({ IssueBoard: (props: BoardProps) => {
  boardRenders.push(props);
  return <section aria-label="Controlled board renderer"><p>{props.items.map(item => item.title).join(', ')}</p>
    <button disabled={props.disabled} onClick={() => props.denied(new ApiError(404, 'issue_unavailable', 'Moved issue unavailable.'))}>Reject moved issue A</button>
    <button disabled={props.disabled} onClick={props.reload}>Refresh exact board</button></section>;
} }));
vi.mock('./issues/IssueEditor', () => ({ IssueEditor: ({ issueId }: { issueId: string }) => {
  const [value, setValue] = useState('Saved other issue');
  useUnsavedChanges(value !== 'Saved other issue');
  return <label>Other issue draft<input data-testid="other-draft" data-issue={issueId} value={value} onChange={event => setValue(event.target.value)} /></label>;
} }));
vi.mock('./settings/WorkspaceSettings', async original => ({ ...await original<typeof import('./settings/WorkspaceSettings')>(),
  CreateWorkspace: ({ close, reauthenticate, session }: { close: () => void; reauthenticate: () => void; session: Session }) => {
    const node = useRef<HTMLDivElement>(null);
    useLayoutEffect(() => {
      const witness = { session, node: node.current! }; mountedCreator = witness;
      return () => { if (mountedCreator === witness) mountedCreator = null; };
    }, [session]);
    return <Modal title="Create workspace" close={close}><div ref={node} data-testid="committed-creator">
      <button onClick={reauthenticate}>Launch real password confirmation</button></div></Modal>;
  },
}));
vi.mock('./commands/CommandPalette', () => ({
  CommandPalette: () => null, CommandHelp: () => null,
}));
vi.mock('./useWorkspaceAccess', () => ({ useWorkspaceAccess: () => {} }));

const workspace = { id: 'w', name: 'Workspace A', slug: 'workspace-a', role: 'owner' };
const initialSession: Session = { audience: 'web', user: { id: 'owner', display_name: 'Example Owner', email: 'owner@example.test' },
  csrf_token: 'synthetic-csrf', workspaces: [workspace] };
const privateTeam = { id: 'private-team', name: 'Private board team', key: 'PRV', private: true, archived_at: null };
const publicTeam = { id: 'public-team', name: 'Public team', key: 'PUB', private: false, archived_at: null };
const privateStatus: Status = { id: 'private-status', team_id: privateTeam.id, name: 'Private review stage', category: 'unstarted', color: 'slate', position: 0,
  is_default: true, archived_at: null, retiring_at: null, replacement_status_id: null, replacement_job_id: null };
const publicStatus: Status = { ...privateStatus, id: 'public-status', team_id: publicTeam.id, name: 'Public review stage' };
const issueA: Issue = { id: 'moved-issue', workspace_id: 'w', team_id: privateTeam.id, key: 'PRV-1', title: 'Saved private issue A',
  description: '', status_id: privateStatus.id, priority: 2, assignee_id: null, assignee: null, label_ids: [], labels: [],
  etag: '"issue:moved-issue:1:projection"', estimate: null, due_date: null, version: '1', created_at: '2026-10-24T10:00:00Z',
  updated_at: '2026-10-24T10:00:00Z', archived_at: null };
const issueB: Issue = { ...issueA, id: 'other-issue', team_id: publicTeam.id, key: 'PUB-1', title: 'Saved public issue B',
  status_id: publicStatus.id, etag: '"issue:other-issue:1:projection"' };
const privateData: Bootstrap = { auth_epoch: '1', teams: [privateTeam, publicTeam], statuses: [privateStatus, publicStatus],
  members: [initialSession.user], issues: [] };
const publicData: Bootstrap = { ...privateData, auth_epoch: '2', teams: [publicTeam], statuses: [publicStatus] };
const json = (value: unknown, status = 200) => new Response(JSON.stringify(value), { status });
const page = (items: Issue[]) => ({ data: items, next_cursor: null, has_more: false });
const query = 'team_id=private-team&status_id=private-status&layout=board&display_version=1&q=Saved&priority=2&sort=priority';
const initialUrl = (detail = false) => '/w/w/issues' + (detail ? '/' + issueB.id : '') + '?' + query;
const exitUrl = () => { const params = new URLSearchParams(query); params.delete('team_id'); params.delete('status_id'); params.set('layout', 'list'); return issuesUrl('w', params); };
type ParseWitness = { creator: CreatorWitness | null; generation: number; connected: boolean };
type Pending = { promise: Promise<Response>; resolve: (value: Response) => void; reject: (error: unknown) => void;
  settled: boolean; signal?: AbortSignal | null; parsed?: ParseWitness };
type Call = { path: string; method: string; creator: CreatorWitness | null; generation: number };
let root: Root, container: HTMLDivElement, calls: Call[], pending: Pending[];
let proofs: Pending[], logins: Pending[], sessions: Pending[], bootstraps: Pending[], accesses: Pending[], pages: Pending[];
let holdAuthority: boolean, holdNextPage: boolean, serverData: Bootstrap, unexpected: string[];
let previous: { url: string; state: unknown; title: string; theme: string | null; storage: [string, string][] };
const dialogMethods = ['showModal', 'close'].map(name => [name, Object.getOwnPropertyDescriptor(HTMLDialogElement.prototype, name)] as const);
function gate(list: Pending[], signal?: AbortSignal | null) {
  let finish!: (value: Response) => void, fail!: (error: unknown) => void;
  const promise = new Promise<Response>((resolve, reject) => { finish = resolve; fail = reject; });
  const item: Pending = { promise, signal, settled: false,
    resolve(value) { item.settled = true; finish(value); }, reject(error) { item.settled = true; fail(error); } };
  pending.push(item); list.push(item); return promise;
}
function watchedProblem(item: Pending, status: number) {
  const response = json({ code: 'held_team_denial', detail: 'Team unavailable.' }, status);
  const parse = response.json.bind(response);
  let parsed!: () => void;
  const parsedBody = new Promise<void>(resolve => { parsed = resolve; });
  response.json = async () => {
    try {
      const value: unknown = await parse();
      item.parsed = { creator: mountedCreator, generation: currentSessionGeneration(),
        connected: mountedCreator !== null && mountedCreator.node.isConnected && container.contains(mountedCreator.node) };
      return value;
    } finally { parsed(); }
  };
  return { response, parsedBody };
}
beforeEach(() => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true);
  vi.spyOn(window, 'confirm').mockReturnValue(false);
  HTMLDialogElement.prototype.showModal = function () { this.setAttribute('open', ''); };
  HTMLDialogElement.prototype.close = function () { this.removeAttribute('open'); };
  previous = { url: location.pathname + location.search, state: history.state, title: document.title, theme: document.documentElement.getAttribute('data-theme'),
    storage: Array.from({ length: localStorage.length }, (_, index) => {
      const key = localStorage.key(index)!; return [key, localStorage.getItem(key)!] as [string, string];
    }) };
  history.replaceState({ otIndex: 0 }, '', initialUrl()); localStorage.clear();
  boardRenders = []; mountedCreator = null; calls = []; pending = []; proofs = []; logins = []; sessions = [];
  bootstraps = []; accesses = []; pages = []; unexpected = []; holdAuthority = false; holdNextPage = false; serverData = privateData;
  vi.stubGlobal('fetch', vi.fn((input: string, options: RequestInit = {}) => {
    const url = new URL(input, 'https://example.test'), path = url.pathname.replace(/^\/api\/v1/, '');
    const method = options.method ?? 'GET';
    calls.push({ path, method, creator: mountedCreator, generation: currentSessionGeneration() });
    if (path === '/setup/status') return Promise.resolve(json({ setup_required: false }));
    if (path === '/auth/session') return holdAuthority ? gate(sessions, options.signal) : Promise.resolve(json(initialSession));
    if (path === '/auth/login' && method === 'POST') return gate(logins, options.signal);
    if (path.endsWith('/preferences')) return Promise.resolve(json({ workspace_id: 'w', shortcut_mode: 'disabled', version: '1', etag: 'synthetic-preference-etag' }));
    if (path === '/workspaces/w/bootstrap') return holdAuthority ? gate(bootstraps, options.signal) : Promise.resolve(json(serverData));
    if (path === '/workspaces/w/access') return holdAuthority ? gate(accesses, options.signal)
      : Promise.resolve(json({ auth_epoch: serverData.auth_epoch, role: 'owner', archived: false }));
    if (path === '/workspaces/w/teams/private-team/workflow' && method === 'GET') return gate(proofs, options.signal);
    if (path === '/workspaces/w/issues' && method === 'GET') {
      if (holdNextPage) { holdNextPage = false; return gate(pages, options.signal); }
      const team = url.searchParams.get('team_id');
      return Promise.resolve(json(page(team === privateTeam.id ? [issueA] : team === publicTeam.id ? [issueB]
        : serverData === publicData ? [issueB] : [issueA, issueB])));
    }
    unexpected.push(path); throw new Error('Unexpected request in connected authority fixture.');
  }));
  container = document.createElement('div'); document.body.append(container); root = createRoot(container);
});
afterEach(async () => {
  await act(async () => root.unmount());
  await act(async () => { for (const item of pending) if (!item.settled) item.resolve(json({})); await Promise.allSettled(pending.map(item => item.promise)); });
  container.remove(); localStorage.clear();
  for (const [key, value] of previous.storage) localStorage.setItem(key, value);
  history.replaceState(previous.state, '', previous.url); document.title = previous.title;
  if (previous.theme === null) document.documentElement.removeAttribute('data-theme'); else document.documentElement.setAttribute('data-theme', previous.theme);
  for (const [name, descriptor] of dialogMethods) {
    if (descriptor) Object.defineProperty(HTMLDialogElement.prototype, name, descriptor); else Reflect.deleteProperty(HTMLDialogElement.prototype, name);
  }
  vi.restoreAllMocks(); vi.unstubAllGlobals();
  expect(unexpected).toEqual([]);
});
function button(name: string) {
  const result = [...container.querySelectorAll<HTMLButtonElement>('button')].find(item => item.textContent?.trim() === name);
  expect(result, 'Required integration control: ' + name).toBeDefined(); return result!;
}
async function click(name: string) { await act(async () => button(name).click()); }
async function settle(item: Pending, value: unknown, status = 200) {
  expect(item).toBeDefined(); await act(async () => item.resolve(json(value, status)));
}
async function start(detail = false) {
  history.replaceState({ otIndex: 0 }, '', initialUrl(detail));
  await act(async () => root.render(<App />));
  expect(container.querySelector('.issue-list')?.getAttribute('aria-busy')).toBe('false');
  expect(container.textContent).toContain(issueA.title);
  expect(boardRenders.at(-1)?.items.some(item => item.id === issueA.id)).toBe(true);
}
async function ambiguousDenial() {
  await click('Reject moved issue A'); expect(proofs.length).toBe(1);
  expect(proofs[0].signal?.aborted).toBe(false);
  await act(async () => proofs[0].reject(new TypeError('Synthetic authority network failure.')));
  expect(container.querySelector('.issue-list')?.textContent).toContain('Board unavailable.');
  expect(button('Try again').disabled).toBe(false);
  expect(button('Open all issues').getAttribute('aria-disabled')).toBe('false');
  expect(sessions.length + bootstraps.length + accesses.length).toBe(0);
}
async function holdExitProof() {
  await ambiguousDenial(); await click('Open all issues');
  expect(proofs.length).toBe(2); expect(proofs[1].signal?.aborted).toBe(false);
  return proofs[1];
}
function committedCreator() {
  const witness = mountedCreator;
  expect(witness !== null && witness.node.isConnected && container.contains(witness.node), 'Setup: creator must be committed and connected.').toBe(true);
  expect(container.querySelector('[data-testid="committed-creator"]') === witness?.node).toBe(true);
  return witness!;
}
function sameCommittedCreator(witness: CreatorWitness) {
  expect(committedCreator() === witness, 'Setup: credential props must remain deliberately withheld throughout the controlled counter phase.').toBe(true);
}
async function openWorkspaceCreator() {
  await act(async () => container.querySelector<HTMLElement>('.workspace-menu > summary')!.click());
  await click('Create workspace'); return committedCreator();
}
async function submitPasswordConfirmation() {
  await click('Launch real password confirmation');
  const password = container.querySelector<HTMLInputElement>('input[name="password"]'); expect(password).toBeTruthy();
  password!.value = 'synthetic account password';
  await act(async () => password!.closest('form')!.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true })));
  expect(logins.length).toBe(1);
}
async function preparePasswordConfirmation() {
  await openWorkspaceCreator(); await submitPasswordConfirmation();
}
async function closeWorkspaceCreator() {
  const creation = [...container.querySelectorAll<HTMLDialogElement>('dialog')].find(dialog => dialog.querySelector('h2')?.textContent === 'Create workspace');
  expect(creation).toBeDefined();
  await act(async () => creation!.querySelector<HTMLButtonElement>('[aria-label="Close dialog"]')!.click());
  expect(mountedCreator).toBeNull();
}
async function dirtyOtherIssue() {
  const field = container.querySelector<HTMLInputElement>('[data-testid="other-draft"]')!;
  expect(field.dataset.issue).toBe(issueB.id); expect(issueB.id === issueA.id).toBe(false);
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(field, 'Unsubmitted distinct issue B draft');
    field.dispatchEvent(new Event('input', { bubbles: true }));
  });
  expect(hasUnsavedChanges()).toBe(true);
  const path = location.pathname + location.search;
  let moved = true;
  await act(async () => { moved = navigate('/w/w/issues?team_id=public-team&layout=list'); });
  expect(moved).toBe(false); expect(window.confirm).toHaveBeenCalledOnce();
  expect(location.pathname + location.search).toBe(path);
  expect(container.querySelector('[data-testid="other-draft"]') === field).toBe(true);
  expect(field.value).toBe('Unsubmitted distinct issue B draft');
  vi.mocked(window.confirm).mockClear();
  return field;
}
function concealed() {
  for (const value of [privateTeam.name, privateTeam.key, privateStatus.name, issueA.title]) {
    expect(container.textContent).not.toContain(value); expect(document.title).not.toContain(value);
  }
  expect(container.querySelector('[data-testid="other-draft"]')).toBeNull(); expect(hasUnsavedChanges()).toBe(false);
}
function onlyLoginWrites() { expect(calls.filter(item => !['GET', 'POST'].includes(item.method) || item.method === 'POST' && item.path !== '/auth/login').length).toBe(0); }

it.each([403, 404])('rejects old exit proof %s in a controlled real-counter phase before new credential props render', async status => {
  await start(); const oldProof = await holdExitProof();
  const creator = await openWorkspaceCreator();
  const generation = currentSessionGeneration(), before = calls.length, oldRoute = location.pathname + location.search;
  const observed = watchedProblem(oldProof, status);
  const exits: { creator: CreatorWitness | null; generation: number; parsed: boolean }[] = [];
  const push = history.pushState.bind(history);
  vi.spyOn(history, 'pushState').mockImplementation((...args) => {
    exits.push({ creator: mountedCreator, generation: currentSessionGeneration(), parsed: !!oldProof.parsed });
    return push(...args);
  });
  let credentialEvents = 0;
  const authenticated = () => { credentialEvents += 1; };
  window.addEventListener('opentrack:reauthenticated', authenticated);
  try {
    holdAuthority = true;
    expect(logins.length).toBe(0); sameCommittedCreator(creator);
    await act(async () => {
      advanceSessionGeneration();
      sameCommittedCreator(creator);
      oldProof.resolve(observed.response);
      await oldProof.promise;
      await observed.parsedBody;
      sameCommittedCreator(creator);
    });
    expect(oldProof.settled).toBe(true);
    expect(oldProof.parsed?.generation, 'Setup: the old error body must complete after the real counter advances.').toBe(generation + 1);
    expect(oldProof.parsed !== undefined && oldProof.parsed.creator === creator && oldProof.parsed.connected,
      'Setup: the old error must parse with the original committed session and connected DOM.').toBe(true);
    expect(calls.slice(before).filter(item => item.path === '/auth/session').length,
      'Old Team denial must not start parent authority reconstruction in the held old-props/new-counter state.').toBe(0);
    expect(exits.length, 'The settled stale child chain must not navigate before current credential props are delivered.').toBe(0);
    expect(location.pathname + location.search).toBe(oldRoute);
    expect(currentSessionGeneration()).toBe(generation + 1);
    expect(credentialEvents).toBe(0); expect(logins.length).toBe(0);
    expect(sessions.length + bootstraps.length + accesses.length).toBe(0);
    sameCommittedCreator(creator);

    await submitPasswordConfirmation();
    sameCommittedCreator(creator);
    expect(credentialEvents).toBe(0); expect(currentSessionGeneration()).toBe(generation + 1);
    expect(calls.slice(before).filter(item => item.path === '/auth/session').length).toBe(0);
    expect(exits.length).toBe(0); expect(location.pathname + location.search).toBe(oldRoute);
    expect(logins[0].settled).toBe(false);
    holdAuthority = false;
    const currentSession: Session = { ...initialSession, csrf_token: 'synthetic-current-csrf' };
    await act(async () => logins[0].resolve(json(currentSession)));
    expect(credentialEvents).toBe(1); expect(currentSessionGeneration() > generation + 1).toBe(true);
    const currentCreator = committedCreator();
    expect(currentCreator === creator).toBe(false);
    expect(currentCreator.session.csrf_token === currentSession.csrf_token).toBe(true);
    await closeWorkspaceCreator();
    expect(location.pathname + location.search).toBe(oldRoute);
    expect(container.querySelector('.issue-list')?.getAttribute('aria-busy')).toBe('false');
    expect(button('Refresh exact board').disabled).toBe(false);
    expect(container.textContent).toContain(issueA.title); onlyLoginWrites();
  } finally { window.removeEventListener('opentrack:reauthenticated', authenticated); }
});

it('settles a pending authorized list read through ordinary same-scope reauthentication without a busy dead end', async () => {
  await start(); holdNextPage = true; await click('Refresh exact board'); expect(pages.length).toBe(1);
  expect(container.querySelector('.issue-list')?.getAttribute('aria-busy')).toBe('true');
  const generation = currentSessionGeneration(), sessionsBefore = calls.filter(item => item.path === '/auth/session').length;
  await preparePasswordConfirmation();
  await act(async () => logins[0].resolve(json({ ...initialSession })));
  expect(currentSessionGeneration() > generation).toBe(true);
  await closeWorkspaceCreator(); await settle(pages[0], page([issueA]));
  expect(container.querySelector('.issue-list')?.getAttribute('aria-busy')).toBe('false');
  expect(button('Refresh exact board').disabled).toBe(false); expect(container.textContent).toContain(issueA.title);
  expect(calls.filter(item => item.path === '/auth/session').length).toBe(sessionsBefore);
  expect(calls.filter(item => item.path === '/workspaces/w/issues').length).toBeLessThanOrEqual(3);
  expect(proofs.length).toBe(0); onlyLoginWrites();
});

it.each([403, 404])('current Team denial %s discards a distinct dirty editor and honors Open all issues without a dirty prompt', async status => {
  await start(true); const field = await dirtyOtherIssue(); const proof = await holdExitProof();
  expect(container.querySelector('[data-testid="other-draft"]') === field).toBe(true); expect(hasUnsavedChanges()).toBe(true);
  holdAuthority = true;
  await settle(proof, { code: 'team_unavailable', detail: 'Team unavailable.' }, status);
  concealed(); expect(window.confirm).not.toHaveBeenCalled();
  expect(sessions.length).toBe(1); expect(bootstraps.length + accesses.length).toBe(0);
  serverData = publicData; await settle(sessions[0], initialSession);
  expect(bootstraps.length).toBe(1); expect(accesses.length).toBe(1);
  await settle(bootstraps[0], publicData); concealed();
  await settle(accesses[0], { auth_epoch: publicData.auth_epoch, role: 'owner', archived: false });
  concealed(); expect(container.textContent).toContain(issueB.title);
  expect(location.pathname + location.search).toBe(exitUrl());
  expect(window.confirm).not.toHaveBeenCalled(); onlyLoginWrites();
});

it.each([403, 404])('newer real navigation fences pending Open all issues proof %s and preserves the current dirty editor', async status => {
  await start(true); const field = await dirtyOtherIssue(); const proof = await holdExitProof();
  const next = '/w/w/issues/' + issueB.id + '?team_id=public-team&layout=list&q=Saved&priority=2';
  vi.mocked(window.confirm).mockReturnValueOnce(true);
  let moved = false; await act(async () => { moved = navigate(next); });
  expect(moved).toBe(true); expect(window.confirm).toHaveBeenCalledOnce();
  expect(proof.signal?.aborted).toBe(true); vi.mocked(window.confirm).mockClear();
  const before = calls.length; holdAuthority = true;
  await settle(proof, { code: 'obsolete_team_denial', detail: 'Obsolete team result.' }, status);
  expect(location.pathname + location.search).toBe(next);
  expect(calls.slice(before).filter(item => item.path === '/auth/session' || item.path.endsWith('/bootstrap')).length).toBe(0);
  expect(container.querySelector('[data-testid="other-draft"]') === field).toBe(true);
  expect(field.value).toBe('Unsubmitted distinct issue B draft'); expect(hasUnsavedChanges()).toBe(true);
  expect(window.confirm).not.toHaveBeenCalled(); onlyLoginWrites();
});
