// @vitest-environment jsdom
import { act, useState } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { App } from './App';
import { request } from './api';
import { Modal } from './components';
import { hasUnsavedChanges, navigate, useUnsavedChanges } from './router';
import type { Bootstrap, Session, Status } from './types';

type ListProps = { workspaceId: string; data: Bootstrap; session: Session; teamAccessLost?: (teamId: string) => boolean };
let lists: ListProps[];
let creatorSessions: Session[];
vi.mock('./issues/IssueList', () => ({ IssueList: (props: ListProps) => {
  lists.push(props);
  const [notice, setNotice] = useState('');
  async function ordinary(kind: string) {
    const suffix = kind === 'empty' ? '?probe=empty' : `/${kind}`;
    try { await request(`/workspaces/${props.workspaceId}/issues${suffix}`); setNotice(`${kind} completed`); }
    catch { setNotice(`${kind} rejected`); }
  }
  return <section aria-label="Issue list"><output data-testid="known-bootstrap">{JSON.stringify(props.data)}</output><output data-testid="list-role">{props.session.workspaces.find(item => item.id === props.workspaceId)?.role}</output><button onClick={() => props.teamAccessLost?.('private-team')}>Confirm private-team authority loss</button>{['unavailable', 'empty', 'network'].map(kind => <button key={kind} onClick={() => void ordinary(kind)}>{kind} issue response</button>)}<p role="status">{notice}</p></section>;
} }));
vi.mock('./issues/IssueEditor', () => ({ IssueEditor: () => {
  const [title, setTitle] = useState('Saved issue title');
  useUnsavedChanges(title !== 'Saved issue title');
  return <label>Issue title<input value={title} onChange={event => setTitle(event.target.value)} /></label>;
} }));
vi.mock('./issues/CreateIssue', () => ({ CreateIssue: ({ data, close }: { data: Bootstrap; close: () => void }) => <Modal title="New issue" close={close}><p>{data.teams.map(team => team.name).join(', ')}</p></Modal> }));
vi.mock('./settings/WorkspaceSettings', async original => ({ ...await original<typeof import('./settings/WorkspaceSettings')>(), CreateWorkspace: ({ close, reauthenticate, session }: { close: () => void; reauthenticate: () => void; session: Session }) => {
  creatorSessions.push(session);
  return <Modal title="Create workspace" close={close}><p>Workspace creation overlay</p><button onClick={reauthenticate}>Confirm password</button></Modal>;
} }));
vi.mock('./commands/CommandPalette', () => ({
  CommandPalette: ({ commands, close }: { commands: { label: string }[]; close: () => void }) => <Modal title="Commands" close={close}><p>{commands.map(command => command.label).join(', ')}</p></Modal>,
  CommandHelp: ({ close }: { close: () => void }) => <Modal title="Keyboard shortcuts" close={close}><p>Shortcut help overlay</p></Modal>,
}));
vi.mock('./useWorkspaceAccess', () => ({ useWorkspaceAccess: () => {} }));

const workspace = { id: 'w', name: 'Workspace A', slug: 'workspace-a', role: 'owner' };
const otherWorkspace = { id: 'other', name: 'Workspace B', slug: 'workspace-b', role: 'owner' };
const initialSession: Session = { audience: 'web', user: { id: 'owner', display_name: 'Example Owner', email: 'owner@example.test' }, csrf_token: 'synthetic-csrf', workspaces: [workspace, otherWorkspace] };
const privateTeam = { id: 'private-team', name: 'Private board team', key: 'PRV', private: true, archived_at: null };
const publicTeam = { id: 'public-team', name: 'Public team', key: 'PUB', private: false, archived_at: null };
const privateStatus: Status = { id: 'private-status', team_id: privateTeam.id, name: 'Private review stage', category: 'unstarted', color: 'slate', position: 0, is_default: true, archived_at: null, retiring_at: null, replacement_status_id: null, replacement_job_id: null };
const privateData: Bootstrap = { auth_epoch: '1', teams: [privateTeam, publicTeam], statuses: [privateStatus], members: [initialSession.user], issues: [] };
const publicData = (epoch = '2'): Bootstrap => ({ ...privateData, auth_epoch: epoch, teams: [publicTeam], statuses: [] });
const json = (value: unknown, status = 200) => new Response(JSON.stringify(value), { status });
type Pending = { promise: Promise<Response>; resolve: (value: Response) => void; reject: (error: unknown) => void; settled: boolean };
const dialogMethods = ['showModal', 'close'].map(name => [name, Object.getOwnPropertyDescriptor(HTMLDialogElement.prototype, name)] as const);
let container: HTMLDivElement; let root: Root;
let previous: { url: string; state: unknown; title: string; theme: string | null; storage: [string, string][] };
let calls: { path: string; method: string }[];
let pending: Pending[]; let sessions: Pending[]; let bootstraps: Pending[]; let accesses: Pending[]; let logins: Pending[];
let holdAuthority: boolean; let holdBootstrapPairs: boolean; let holdLogin: boolean; let serverSession: Session;

function gate(list: Pending[]) {
  let resolve!: (value: Response) => void; let reject!: (error: unknown) => void;
  const promise = new Promise<Response>((finish, fail) => { resolve = finish; reject = fail; });
  const item: Pending = { promise, settled: false, resolve(value) { item.settled = true; resolve(value); }, reject(error) { item.settled = true; reject(error); } };
  pending.push(item); list.push(item); return promise;
}
beforeEach(() => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true);
  vi.spyOn(window, 'confirm').mockReturnValue(false);
  HTMLDialogElement.prototype.showModal = function () { this.setAttribute('open', ''); };
  HTMLDialogElement.prototype.close = function () { this.removeAttribute('open'); };
  previous = { url: `${location.pathname}${location.search}`, state: history.state, title: document.title, theme: document.documentElement.getAttribute('data-theme'),
    storage: Array.from({ length: localStorage.length }, (_, index) => { const key = localStorage.key(index)!; return [key, localStorage.getItem(key)!] as [string, string]; }) };
  history.replaceState({ otIndex: 0 }, '', '/w/w/issues/issue?team_id=private-team'); localStorage.clear();
  calls = []; lists = []; creatorSessions = []; pending = []; sessions = []; bootstraps = []; accesses = []; logins = []; holdAuthority = false; holdBootstrapPairs = false; holdLogin = false;
  serverSession = initialSession;
  vi.stubGlobal('fetch', vi.fn((input: string, options: RequestInit = {}) => {
    const url = new URL(input, 'https://example.test'); const path = url.pathname.replace(/^\/api\/v1/, '');
    calls.push({ path, method: options.method ?? 'GET' });
    if (path === '/setup/status') return Promise.resolve(json({ setup_required: false }));
    if (path === '/auth/session') return holdAuthority ? gate(sessions) : Promise.resolve(json(serverSession));
    if (path === '/auth/login') return holdLogin ? gate(logins) : Promise.resolve(json(serverSession));
    if (path.endsWith('/preferences')) return Promise.resolve(json({ workspace_id: path.split('/')[2], shortcut_mode: 'disabled', version: '1', etag: 'synthetic-preference-etag' }));
    if (path.endsWith('/bootstrap')) {
      if ((holdAuthority || holdBootstrapPairs) && path === '/workspaces/w/bootstrap') return gate(bootstraps);
      return Promise.resolve(json(path.includes('/other/') ? { ...publicData('20'), teams: [{ ...publicTeam, id: 'other-team', name: 'Other authorized team' }] } : serverSession.user.id === initialSession.user.id ? privateData : publicData('20')));
    }
    if (path.endsWith('/access')) {
      if ((holdAuthority || holdBootstrapPairs) && path === '/workspaces/w/access') return gate(accesses);
      const id = path.split('/')[2]; const role = serverSession.workspaces.find(item => item.id === id)?.role;
      return Promise.resolve(json({ auth_epoch: id === 'other' || serverSession.user.id !== initialSession.user.id ? '20' : '1', role, archived: false }));
    }
    if (path === '/workspaces/w/issues/unavailable') return Promise.resolve(json({ code: 'not_found', detail: 'Issue unavailable.' }, 404));
    if (path === '/workspaces/w/issues/network') return Promise.reject(new Error('Synthetic network failure.'));
    if (path === '/workspaces/w/issues' && url.searchParams.get('probe') === 'empty') return Promise.resolve(json({ data: [], next_cursor: null, has_more: false }));
    throw new Error('Unexpected request in authority fixture.');
  }));
  container = document.createElement('div'); document.body.append(container); root = createRoot(container);
});
afterEach(async () => {
  await act(async () => root.unmount());
  await act(async () => { pending.filter(item => !item.settled).forEach(item => item.resolve(json({}))); await Promise.allSettled(pending.map(item => item.promise)); });
  container.remove(); localStorage.clear(); for (const [key, value] of previous.storage) localStorage.setItem(key, value);
  history.replaceState(previous.state, '', previous.url); document.title = previous.title;
  if (previous.theme === null) document.documentElement.removeAttribute('data-theme'); else document.documentElement.setAttribute('data-theme', previous.theme);
  for (const [name, descriptor] of dialogMethods) {
    if (descriptor) Object.defineProperty(HTMLDialogElement.prototype, name, descriptor); else Reflect.deleteProperty(HTMLDialogElement.prototype, name);
  }
  vi.restoreAllMocks(); vi.unstubAllGlobals();
});
function button(name: string) { return [...container.querySelectorAll<HTMLButtonElement>('button')].find(item => item.textContent?.trim().replace(/\s+/g, ' ') === name)!; }
function report() {
  const callback = lists.at(-1)?.teamAccessLost;
  expect(typeof callback, 'The actual App must provide the optional child authority-notification contract.').toBe('function');
  return callback!;
}
async function start() {
  await act(async () => root.render(<App />));
  expect(container.querySelector('[data-testid="known-bootstrap"]')?.textContent).toContain(privateTeam.name);
  expect(container.querySelector('[data-testid="known-bootstrap"]')?.textContent).toContain(privateStatus.name);
}
async function dirty() {
  const field = container.querySelector<HTMLInputElement>('label > input')!;
  await act(async () => { Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(field, 'Unsubmitted authorized draft'); field.dispatchEvent(new Event('input', { bubbles: true })); });
  expect(hasUnsavedChanges()).toBe(true); return field;
}
function concealed() {
  expect(container.querySelector('[data-testid="known-bootstrap"]')).toBeNull();
  for (const value of [privateTeam.name, privateTeam.key, privateStatus.name]) { expect(container.textContent).not.toContain(value); expect(document.title).not.toContain(value); }
  expect(container.querySelector('label > input')).toBeNull(); expect(container.querySelectorAll('dialog')).toHaveLength(0);
  expect(hasUnsavedChanges()).toBe(false);
  const create = [...container.querySelectorAll<HTMLButtonElement>('button')].find(item => item.textContent?.startsWith('New issue'));
  expect(!create || create.disabled).toBe(true);
}
async function settle(item: Pending, value: unknown) { expect(item).toBeDefined(); await act(async () => item.resolve(json(value))); }
async function allowFresh(epoch = '2', role = 'owner') {
  expect(bootstraps).toHaveLength(1); expect(accesses).toHaveLength(1);
  await settle(bootstraps[0], publicData(epoch)); concealed();
  await settle(accesses[0], { auth_epoch: epoch, role, archived: false });
  expect(container.querySelector('[data-testid="known-bootstrap"]')?.textContent).toContain(publicTeam.name);
  expect(container.textContent).not.toContain(privateTeam.name);
}

it.each(['issue', 'workspace', 'palette', 'help'] as const)('confirmed team loss conceals cached authority and the %s overlay before refresh responses arrive', async overlay => {
  await start(); await dirty(); const callback = report();
  if (overlay === 'issue') await act(async () => button('New issue').click());
  if (overlay === 'workspace') {
    await act(async () => container.querySelector<HTMLElement>('.workspace-menu > summary')!.click());
    await act(async () => button('Create workspace').click());
  }
  if (overlay === 'palette') await act(async () => container.querySelector<HTMLButtonElement>('[aria-label="Search commands"]')!.click());
  if (overlay === 'help') await act(async () => button('Keyboard shortcuts').click());
  expect(container.querySelector('dialog')).toBeTruthy();
  holdAuthority = true;
  await act(async () => { expect(callback(privateTeam.id)).toBe(true); });
  expect(callback(privateTeam.id), 'An accepted generation cannot start another authority refresh.').toBe(false);
  concealed(); expect(sessions).toHaveLength(1); expect(bootstraps).toHaveLength(0); expect(accesses).toHaveLength(0);
  await settle(sessions[0], initialSession); concealed();
  await allowFresh(); expect(window.confirm).not.toHaveBeenCalled();
});

it('ignores an unknown team notification and keeps ordinary dirty navigation guarded', async () => {
  await start(); const field = await dirty(); const callback = report();
  const before = calls.filter(item => item.path === '/auth/session').length;
  await act(async () => { expect(callback('unknown-team')).toBe(false); });
  expect(calls.filter(item => item.path === '/auth/session')).toHaveLength(before);
  expect(container.querySelector('label > input')).toBe(field); expect(field.value).toBe('Unsubmitted authorized draft');
  expect(container.textContent).toContain(privateTeam.name); expect(hasUnsavedChanges()).toBe(true);
  await act(async () => [...container.querySelectorAll<HTMLAnchorElement>('nav a')].find(item => item.textContent === 'All issues')!.click());
  expect(window.confirm).toHaveBeenCalledOnce(); expect(location.pathname).toBe('/w/w/issues/issue');
  expect(container.querySelector('label > input')).toBe(field); expect(hasUnsavedChanges()).toBe(true);
});

it.each(['unavailable', 'empty', 'network'])('an ordinary %s issue response without a confirmed-team notification preserves the editor and dirty guard', async kind => {
  await start(); const field = await dirty();
  const before = calls.filter(item => item.path === '/auth/session').length;
  await act(async () => button(`${kind} issue response`).click());
  expect(container.querySelector('label > input')).toBe(field); expect(field.value).toBe('Unsubmitted authorized draft');
  expect(hasUnsavedChanges()).toBe(true); expect(container.textContent).toContain(privateTeam.name);
  expect(calls.filter(item => item.path === '/auth/session')).toHaveLength(before);
});

it.each([
  ['workspace', 'success'], ['workspace', 'failure'], ['account', 'success'], ['account', 'failure'],
] as const)('an obsolete callback and session %s scope completion (%s) cannot replace the current scope', async (change, outcome) => {
  await start(); const callback = report(); holdAuthority = true;
  await act(async () => { expect(callback(privateTeam.id)).toBe(true); }); concealed(); expect(sessions).toHaveLength(1);
  holdAuthority = false;
  if (change === 'workspace') {
    const select = container.querySelector<HTMLSelectElement>('.workspace-menu select')!;
    await act(async () => { select.value = 'other'; select.dispatchEvent(new Event('change', { bubbles: true })); });
    expect(location.pathname).toBe('/w/other/issues'); expect(container.textContent).toContain('Other authorized team');
  } else {
    serverSession = { ...initialSession, user: { id: 'member', display_name: 'Example Member', email: 'member@example.test' }, csrf_token: 'synthetic-next-csrf', workspaces: [{ ...workspace, role: 'member' }] };
    await act(async () => window.dispatchEvent(new CustomEvent('opentrack:session-expired')));
    const email = container.querySelector<HTMLInputElement>('input[name="email"]')!;
    const password = container.querySelector<HTMLInputElement>('input[name="password"]')!;
    email.value = serverSession.user.email; password.value = 'synthetic account password';
    await act(async () => container.querySelector('form')!.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true })));
    expect(container.querySelector('.account-menu > summary')?.getAttribute('aria-label')).toBe('Account menu: Example Member');
    expect(container.textContent).toContain(publicTeam.name); expect(container.textContent).not.toContain(privateTeam.name);
  }
  const before = calls.filter(item => item.path === '/auth/session').length;
  const bootstrapReads = calls.filter(item => item.path.endsWith('/bootstrap')).length;
  await act(async () => { expect(callback(publicTeam.id)).toBe(false); });
  expect(calls.filter(item => item.path === '/auth/session')).toHaveLength(before);
  if (outcome === 'success') await settle(sessions[0], { ...initialSession, workspaces: [workspace] });
  else await act(async () => sessions[0].reject(new Error('Obsolete authority failure.')));
  expect(calls.filter(item => item.path === '/auth/session')).toHaveLength(before);
  expect(calls.filter(item => item.path.endsWith('/bootstrap'))).toHaveLength(bootstrapReads);
  expect(container.querySelector('[role="alert"]')).toBeNull();
  expect(container.textContent).not.toContain(privateTeam.name);
  if (change === 'workspace') { expect(location.pathname).toBe('/w/other/issues'); expect(container.textContent).toContain('Other authorized team'); }
  else expect(container.querySelector('.account-menu > summary')?.getAttribute('aria-label')).toBe('Account menu: Example Member');
});

it('retries a failed authority refresh through canonical session before any bootstrap can unblock it', async () => {
  await start(); const callback = report(); holdAuthority = true;
  await act(async () => { expect(callback(privateTeam.id)).toBe(true); }); concealed();
  await act(async () => sessions[0].reject(new Error('Synthetic authority connection failure.')));
  concealed(); expect(container.querySelector('[role="alert"]')).toBeTruthy(); expect(bootstraps).toHaveLength(0);
  await act(async () => { expect(callback(publicTeam.id)).toBe(false); }); expect(sessions).toHaveLength(1);
  await act(async () => button('Try again').click());
  expect(sessions).toHaveLength(2); expect(bootstraps).toHaveLength(0); expect(accesses).toHaveLength(0); concealed();
  await settle(sessions[1], initialSession); concealed(); await allowFresh();
});

it('withholds bootstrap data across epoch and canonical-role mismatches and rejects the former rendered callback', async () => {
  await start(); const callback = report(); holdAuthority = true;
  await act(async () => { expect(callback(privateTeam.id)).toBe(true); }); await settle(sessions[0], initialSession);
  await settle(bootstraps[0], publicData('2')); await settle(accesses[0], { auth_epoch: '3', role: 'owner', archived: false });
  concealed(); expect(bootstraps).toHaveLength(2); expect(accesses).toHaveLength(2);
  await settle(bootstraps[1], publicData('3')); await settle(accesses[1], { auth_epoch: '3', role: 'viewer', archived: false });
  concealed(); expect(sessions).toHaveLength(2);
  const viewer: Session = { ...initialSession, workspaces: [{ ...workspace, role: 'viewer' }, otherWorkspace] };
  serverSession = viewer; await settle(sessions[1], viewer); concealed();
  expect(bootstraps).toHaveLength(3); expect(accesses).toHaveLength(3);
  await settle(bootstraps[2], publicData('4')); concealed();
  await settle(accesses[2], { auth_epoch: '4', role: 'viewer', archived: false });
  expect(container.querySelector('[data-testid="known-bootstrap"]')?.textContent).toContain(publicTeam.name);
  expect(container.querySelector('[data-testid="list-role"]')?.textContent).toBe('viewer');
  expect(container.textContent).not.toContain(privateTeam.name);
  expect([...container.querySelectorAll('button')].some(item => item.textContent?.startsWith('New issue'))).toBe(false);
  await act(async () => { expect(callback(publicTeam.id)).toBe(false); }); expect(sessions).toHaveLength(2);
});

async function confirmCurrentPassword() {
  await act(async () => container.querySelector<HTMLElement>('.workspace-menu > summary')!.click());
  await act(async () => button('Create workspace').click());
  await act(async () => button('Confirm password').click());
  const password = container.querySelector<HTMLInputElement>('input[name="password"]'); expect(password).toBeTruthy();
  password!.value = 'synthetic account password';
  await act(async () => password!.closest('form')!.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true })));
  expect(calls.filter(item => item.path === '/auth/login' && item.method === 'POST')).toHaveLength(1);
  expect(container.querySelector('input[name="password"]')).toBeNull();
  const creation = [...container.querySelectorAll<HTMLDialogElement>('dialog')].find(dialog => dialog.querySelector('h2')?.textContent === 'Create workspace');
  if (creation) await act(async () => creation.querySelector<HTMLButtonElement>('[aria-label="Close dialog"]')!.click());
}

it('rejects the held bootstrap and access pair after same-account credentials rotate and requires a fresh current-scope pair', async () => {
  holdBootstrapPairs = true;
  await act(async () => root.render(<App />));
  expect(bootstraps).toHaveLength(1); expect(accesses).toHaveLength(1); concealed();
  const previousBootstrap = bootstraps[0]; const previousAccess = accesses[0];
  const rotated: Session = { ...initialSession, csrf_token: 'synthetic-rotated-csrf' };
  serverSession = rotated;
  await confirmCurrentPassword(); concealed();
  await settle(previousBootstrap, privateData);
  await settle(previousAccess, { auth_epoch: '1', role: 'owner', archived: false });
  concealed();
  expect(bootstraps.length).toBeGreaterThanOrEqual(2); expect(accesses.length).toBeGreaterThanOrEqual(2);
  const currentBootstrap = bootstraps.at(-1)!; const currentAccess = accesses.at(-1)!;
  expect(currentBootstrap).not.toBe(previousBootstrap); expect(currentAccess).not.toBe(previousAccess);
  await settle(currentBootstrap, publicData('2')); concealed();
  await settle(currentAccess, { auth_epoch: '2', role: 'owner', archived: false });
  expect(container.querySelector('[data-testid="known-bootstrap"]')?.textContent).toContain(publicTeam.name);
  expect(container.textContent).not.toContain(privateTeam.name);
  expect(lists.at(-1)?.session.csrf_token).toBe(rotated.csrf_token);
  expect(lists.at(-1)?.session.user.id).toBe(initialSession.user.id);
});

it('preserves a ready authorized dirty editor through ordinary same-account password confirmation', async () => {
  await start(); const field = await dirty();
  const path = `${location.pathname}${location.search}`;
  const rotated: Session = { ...initialSession, csrf_token: 'synthetic-rotated-csrf' };
  serverSession = rotated;
  await confirmCurrentPassword();
  expect(container.querySelector('label > input')).toBe(field); expect(field.isConnected).toBe(true);
  expect(field.value).toBe('Unsubmitted authorized draft'); expect(hasUnsavedChanges()).toBe(true);
  expect(`${location.pathname}${location.search}`).toBe(path); expect(window.confirm).not.toHaveBeenCalled();
  expect(container.querySelector('[data-testid="known-bootstrap"]')?.textContent).toContain(privateTeam.name);
  expect(lists.at(-1)?.session.csrf_token).toBe(rotated.csrf_token);
  expect(lists.at(-1)?.session.user.id).toBe(initialSession.user.id);
});

it('does not publish the old pair when same-account reauthentication and old reads complete in one React batch', async () => {
  holdBootstrapPairs = true; holdLogin = true;
  await act(async () => root.render(<App />));
  expect(bootstraps).toHaveLength(1); expect(accesses).toHaveLength(1); concealed();
  const previousBootstrap = bootstraps[0]; const previousAccess = accesses[0];
  await act(async () => container.querySelector<HTMLElement>('.workspace-menu > summary')!.click());
  await act(async () => button('Create workspace').click());
  await act(async () => button('Confirm password').click());
  const password = container.querySelector<HTMLInputElement>('input[name="password"]')!;
  password.value = 'synthetic account password';
  await act(async () => password.closest('form')!.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true })));
  expect(logins).toHaveLength(1);
  expect(previousBootstrap.settled || previousAccess.settled).toBe(false);
  const rotated: Session = { ...initialSession, csrf_token: 'synthetic-batched-csrf' };
  serverSession = rotated;
  let confirmations = 0; let renderedKeyAtConfirmation: string | undefined; let firstPublication = -1;
  let confirmed!: () => void;
  const confirmation = new Promise<void>(resolve => { confirmed = resolve; });
  const releaseAtConfirmation = () => {
    confirmations++;
    renderedKeyAtConfirmation = creatorSessions.at(-1)?.csrf_token;
    firstPublication = lists.length;
    previousBootstrap.resolve(json(privateData));
    previousAccess.resolve(json({ auth_epoch: '1', role: 'owner', archived: false }));
    confirmed();
  };
  window.addEventListener('opentrack:reauthenticated', releaseAtConfirmation, { once: true });
  try {
    await act(async () => {
      logins[0].resolve(json(rotated));
      await confirmation;
      // Drain the controlled transport's promise chain inside this same React batch.
      for (let turn = 0; turn < 12; turn++) await Promise.resolve();
    });
  } finally { window.removeEventListener('opentrack:reauthenticated', releaseAtConfirmation); }
  expect(confirmations).toBe(1);
  expect(renderedKeyAtConfirmation, 'The real authenticated callback queued the new session before its creator props rendered.').toBe(initialSession.csrf_token);
  expect(previousBootstrap.settled && previousAccess.settled).toBe(true);
  expect(firstPublication).toBeGreaterThanOrEqual(0);
  expect(lists.slice(firstPublication), 'Old reads must never publish Bootstrap into a child, even if a later effect conceals it again.').toHaveLength(0);
  const creation = [...container.querySelectorAll<HTMLDialogElement>('dialog')].find(dialog => dialog.querySelector('h2')?.textContent === 'Create workspace');
  if (creation) await act(async () => creation.querySelector<HTMLButtonElement>('[aria-label="Close dialog"]')!.click());
  concealed();
  expect(bootstraps.length).toBeGreaterThanOrEqual(2); expect(accesses.length).toBeGreaterThanOrEqual(2);
  const currentBootstrap = bootstraps.at(-1)!; const currentAccess = accesses.at(-1)!;
  expect(currentBootstrap).not.toBe(previousBootstrap); expect(currentAccess).not.toBe(previousAccess);
  await settle(currentBootstrap, publicData('2')); concealed();
  await settle(currentAccess, { auth_epoch: '2', role: 'owner', archived: false });
  expect(container.querySelector('[data-testid="known-bootstrap"]')?.textContent).toContain(publicTeam.name);
  expect(container.textContent).not.toContain(privateTeam.name);
  expect(lists.at(-1)?.session.csrf_token).toBe(rotated.csrf_token);
});


it('accepts a fresh retained source loss while a different team list is routed', async () => {
  await start();
  await act(async () => { expect(navigate('/w/w/issues?team_id=public-team&layout=list')).toBe(true); });
  const callback = report(); holdAuthority = true;
  await act(async () => { expect(callback(privateTeam.id)).toBe(true); });
  concealed(); expect(sessions).toHaveLength(1);
  expect(location.search).toBe('?team_id=public-team&layout=list');
  await settle(sessions[0], initialSession); concealed(); await allowFresh();
  expect(location.search).toBe('?team_id=public-team&layout=list');
  expect(window.confirm).not.toHaveBeenCalled();
});
