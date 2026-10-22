import { expect, it, vi } from 'vitest';
import type { Session } from '../types';
import { DesktopStore } from './store';
import type { ConnectionState, NativeAuthEvent, NativeInvoke, NativeIssueLink, NativeListen } from './types';

const session: Session = { user: { id: 'user', display_name: 'User', email: 'user@example.test' }, workspaces: [], csrf_token: '', audience: 'desktop' };
const state: ConnectionState = { generation: '1', scope_generation: '1', selected: { origin: 'https://work.example.test', development_http: false }, credential_present: true, pending: null };
const link: NativeIssueLink = { id: 'link-one', server: 'https://other.example.test', workspace: 'workspace', issue: 'issue' };
function setup(invoke: NativeInvoke) {
  let auth: ((event: { payload: NativeAuthEvent }) => void) | undefined;
  let issue: ((event: { payload: NativeIssueLink }) => void) | undefined;
  const unlisten = vi.fn();
  const listen = vi.fn(async (name: string, handler: unknown) => {
    if (name === 'opentrack-native-auth') auth = handler as typeof auth;
    else issue = handler as typeof issue;
    return unlisten;
  }) as NativeListen;
  return { store: new DesktopStore(invoke, listen), auth: (payload: NativeAuthEvent) => auth!({ payload }), issue: (payload: NativeIssueLink) => issue!({ payload }), unlisten };
}
const authenticated = (generation = '2', scope = '1'): NativeAuthEvent => ({ generation, scope_generation: scope, kind: 'authenticated', request_id: 'request', session, error: null, remote_revocation_confirmed: null });

it('reconciles a successful authentication event arriving before initial connection read', async () => {
  let resolve!: (state: ConnectionState) => void;
  const invoke = vi.fn((name: string) => name === 'connection_read' ? new Promise<ConnectionState>(done => { resolve = done; }) : Promise.resolve(null));
  const subject = setup(invoke as NativeInvoke);
  const starting = subject.store.start();
  await vi.waitFor(() => expect(resolve).toBeTypeOf('function'));
  subject.auth(authenticated('2', '2'));
  resolve({ ...state, credential_present: false });
  await starting;
  expect(subject.store.getSnapshot()).toMatchObject({ loading: false, session, connection: { generation: '2', scope_generation: '2', credential_present: true, selected: state.selected } });
  subject.store.stop(); expect(subject.unlisten).toHaveBeenCalledTimes(2);
});

it('advances only the request revision for same-user reauthentication and ignores older reads', async () => {
  const invoke = vi.fn((name: string) => Promise.resolve(name === 'connection_read' ? state : null));
  const subject = setup(invoke as NativeInvoke); await subject.store.start();
  subject.auth(authenticated()); await subject.store.refresh();
  expect(subject.store.getSnapshot()).toMatchObject({ session, connection: { generation: '2', scope_generation: '1' } });
  subject.auth({ ...authenticated('1'), kind: 'signed_out', session: null });
  expect(subject.store.getSnapshot().session).toBe(session);
  subject.store.stop();
});

it('keeps an existing credential after cancelled reauthentication but clears it on authoritative sign-out', async () => {
  const invoke = vi.fn((name: string) => Promise.resolve(name === 'connection_read' ? state : null));
  const subject = setup(invoke as NativeInvoke); await subject.store.start(); subject.auth(authenticated());
  subject.auth({ ...authenticated('3'), kind: 'cancelled', session: null });
  expect(subject.store.getSnapshot()).toMatchObject({ session, connection: { generation: '3', scope_generation: '1', credential_present: true } });
  subject.auth({ ...authenticated('4', '2'), kind: 'signed_out', session: null, error: { code: 'access_changed', message: 'Access updated.', retryable: false } });
  expect(subject.store.getSnapshot()).toMatchObject({ session: null, connection: { generation: '4', scope_generation: '2', credential_present: false } });
  subject.store.stop();
});

it('does not restore a failed pending prompt from its late begin receipt or same-generation read', async () => {
  let finishBegin!: (value: unknown) => void;
  let finishRead!: (value: ConnectionState) => void;
  let delayed = false;
  const invoke = vi.fn((name: string) => {
    if (name === 'auth_begin') return new Promise(done => { finishBegin = done; });
    if (name === 'connection_read') return delayed ? new Promise<ConnectionState>(done => { finishRead = done; }) : Promise.resolve(state);
    return Promise.resolve(null);
  });
  const subject = setup(invoke as NativeInvoke); await subject.store.start();
  const beginning = subject.store.begin('This desktop');
  delayed = true; const reading = subject.store.refresh();
  subject.auth({ ...authenticated('1'), kind: 'failed', session: null, error: { code: 'callback_failed', message: 'Start sign-in again.', retryable: true } });
  const pending = { request_id: 'request', confirmation_code: 'ABCD2345', expires_at: '2030-01-01T00:00:00Z' };
  finishBegin({ generation: '1', ...pending }); finishRead({ ...state, pending });
  await Promise.all([beginning, reading]);
  expect(subject.store.getSnapshot().connection?.pending).toBeNull();
  expect(subject.store.getSnapshot().error).toMatchObject({ code: 'callback_failed' });
  subject.store.stop();
});

it('dismisses only the reviewed link when a new link arrives while dismissal is pending', async () => {
  let finish!: () => void;
  const invoke = vi.fn((name: string) => name === 'issue_link_dismiss' ? new Promise<void>(done => { finish = done; }) : Promise.resolve(name === 'connection_read' ? state : link));
  const subject = setup(invoke as NativeInvoke); await subject.store.start();
  const dismissing = subject.store.dismissIssueLink(link.id);
  subject.issue({ ...link, id: 'link-two' }); finish(); await dismissing;
  expect(subject.store.getSnapshot().issueLink?.id).toBe('link-two');
  expect(invoke).toHaveBeenCalledWith('issue_link_dismiss', { request: { id: 'link-one' } });
  subject.store.stop();
});

it('does not select a server or authenticate merely because an issue link arrived', async () => {
  const invoke = vi.fn((name: string) => Promise.resolve(name === 'connection_read' ? state : null));
  const subject = setup(invoke as NativeInvoke); await subject.store.start(); subject.issue(link);
  expect(subject.store.getSnapshot().issueLink).toEqual(link);
  expect(subject.store.getSnapshot().connection?.selected).toEqual(state.selected);
  expect(invoke.mock.calls.map(([name]) => name)).toEqual(['connection_read', 'issue_link_read']);
  subject.store.stop(); subject.auth(authenticated('9'));
  expect(subject.store.getSnapshot().connection?.generation).toBe('1');
});

it('retains an explicit remote-unconfirmed logout receipt before its native event arrives', async () => {
  const ended = { ...state, generation: '2', scope_generation: '2', credential_present: false, remote_revocation_confirmed: false };
  const invoke = vi.fn((name: string) => Promise.resolve(name === 'auth_logout' ? ended : name === 'connection_read' ? state : null));
  const subject = setup(invoke as NativeInvoke); await subject.store.start();
  await subject.store.logout();
  expect(subject.store.getSnapshot()).toMatchObject({ session: null, connection: { credential_present: false }, authEvent: { kind: 'signed_out', remote_revocation_confirmed: false } });
  subject.store.stop();
});

it('never applies a late logout receipt to a newer selected account scope', async () => {
  let finish!: (value: unknown) => void;
  const invoke = vi.fn((name: string) => name === 'auth_logout' ? new Promise(resolve => { finish = resolve; }) : Promise.resolve(name === 'connection_read' ? state : null));
  const subject = setup(invoke as NativeInvoke); await subject.store.start();
  const logout = subject.store.logout(); subject.auth(authenticated('4', '3'));
  finish({ ...state, generation: '2', scope_generation: '2', credential_present: false, remote_revocation_confirmed: true });
  await logout;
  expect(subject.store.getSnapshot()).toMatchObject({ session, connection: { generation: '4', scope_generation: '3', credential_present: true }, authEvent: { kind: 'authenticated' } });
  subject.store.stop();
});
