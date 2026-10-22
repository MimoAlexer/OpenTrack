// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { ApiError, request } from '../api';
import type { Session } from '../types';
import { CommandHelp } from './CommandPalette';
import { useShortcutPreference, type UserPreferences } from './useShortcutPreference';

vi.mock('../api', async original => ({ ...await original<typeof import('../api')>(), request: vi.fn() }));
const session: Session = { audience: 'web', user: { id: 'owner', display_name: 'Example Owner', email: 'owner@example.test' }, csrf_token: 'csrf', workspaces: [] };
const record = (shortcut_mode: 'enabled' | 'disabled', version = '0', workspace_id = 'workspace'): UserPreferences => ({ workspace_id, shortcut_mode, version, etag: `"preferences:${workspace_id}:owner:${version}"` });
function deferred<T>() { let resolve!: (value: T) => void; let reject!: (error: unknown) => void; const promise = new Promise<T>((a, b) => { resolve = a; reject = b; }); return { promise, resolve, reject }; }
function Harness({ workspace = 'workspace', account = session, revision = 0 }: { workspace?: string; account?: Session; revision?: number }) {
  const preference = useShortcutPreference(workspace, account, revision);
  return <><output data-testid="enabled">{String(preference.charactersEnabled)}</output><CommandHelp commands={[{ id: 'create', label: 'Create issue', section: 'Actions', shortcut: { key: 'c' }, run: vi.fn() }, { id: 'palette', label: 'Search commands', section: 'Help', shortcut: { key: 'k', primary: true }, run: vi.fn() }]} preference={preference} close={vi.fn()} /></>;
}
let container: HTMLDivElement; let root: Root;
beforeEach(() => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true); vi.mocked(request).mockReset();
  HTMLDialogElement.prototype.showModal = function () { this.open = true; };
  HTMLDialogElement.prototype.close = function () { this.open = false; };
  container = document.createElement('div'); document.body.append(container); root = createRoot(container);
});
afterEach(async () => { await act(async () => root.unmount()); container.remove(); vi.unstubAllGlobals(); });
const enabled = () => container.querySelector('output')!.textContent;
const toggle = () => container.querySelector<HTMLInputElement>('input[type="checkbox"]')!;
const button = (label: string) => [...container.querySelectorAll('button')].find(item => item.textContent === label)!;

it('loads the server preference, pauses during saving, and restores the disabled setting after remount', async () => {
  const load = deferred<UserPreferences>(); const save = deferred<UserPreferences>();
  vi.mocked(request).mockReturnValueOnce(load.promise).mockReturnValueOnce(save.promise).mockResolvedValue(record('disabled', '1'));
  await act(async () => root.render(<Harness />)); expect(enabled()).toBe('false'); expect(toggle().disabled).toBe(true);
  await act(async () => load.resolve(record('enabled'))); expect(enabled()).toBe('true'); expect(toggle().checked).toBe(true);
  await act(async () => { toggle().click(); toggle().click(); });
  expect(enabled()).toBe('false'); expect(request).toHaveBeenCalledTimes(2);
  expect(request).toHaveBeenLastCalledWith('/workspaces/workspace/preferences', expect.objectContaining({ method: 'PATCH', csrf: 'csrf', headers: { 'If-Match': '"preferences:workspace:owner:0"' }, body: { shortcut_mode: 'disabled' } }));
  await act(async () => save.resolve(record('disabled', '1')));
  expect(enabled()).toBe('false'); expect(container.querySelector('dl')!.textContent).not.toContain('Create issue'); expect(container.querySelector('dl')!.textContent).toContain('Search commands');
  await act(async () => root.render(null)); await act(async () => root.render(<Harness />));
  expect(enabled()).toBe('false'); expect(toggle().checked).toBe(false); expect(toggle().disabled).toBe(false);
});

it('requires explicit review before applying a stale change with the current server version', async () => {
  vi.mocked(request).mockResolvedValueOnce(record('enabled')).mockRejectedValueOnce(new ApiError(412, 'version_conflict', 'Changed', undefined, { current: record('enabled', '2') })).mockResolvedValueOnce(record('disabled', '3'));
  await act(async () => root.render(<Harness />)); await act(async () => toggle().click());
  expect(enabled()).toBe('false'); expect(toggle().checked).toBe(false); expect(container.textContent).toContain('saved setting is enabled; your choice is disabled'); expect(request).toHaveBeenCalledTimes(2);
  await act(async () => button('Apply my choice').click());
  expect(request).toHaveBeenLastCalledWith('/workspaces/workspace/preferences', expect.objectContaining({ headers: { 'If-Match': '"preferences:workspace:owner:2"' }, body: { shortcut_mode: 'disabled' } }));
  expect(container.textContent).toContain('Character shortcuts are disabled.');
});

it('can accept the saved version after a conflict without issuing another mutation', async () => {
  vi.mocked(request).mockResolvedValueOnce(record('enabled')).mockRejectedValueOnce(new ApiError(412, 'version_conflict', 'Changed', undefined, { current: record('enabled', '2') }));
  await act(async () => root.render(<Harness />)); await act(async () => toggle().click()); await act(async () => button('Use saved setting').click());
  expect(enabled()).toBe('true'); expect(toggle().checked).toBe(true); expect(request).toHaveBeenCalledTimes(2);
});

it('retains the proposed value on an uncertain write and reconciles only through an explicit reload', async () => {
  vi.mocked(request).mockResolvedValueOnce(record('enabled')).mockRejectedValueOnce(new ApiError(0, 'network_error', 'Connection lost')).mockResolvedValueOnce(record('disabled', '1'));
  await act(async () => root.render(<Harness />)); await act(async () => toggle().click());
  expect(enabled()).toBe('false'); expect(toggle().checked).toBe(false); expect(toggle().disabled).toBe(true); expect(request).toHaveBeenCalledTimes(2);
  await act(async () => button('Reload saved setting').click()); expect(enabled()).toBe('false'); expect(toggle().disabled).toBe(false);
  expect(vi.mocked(request).mock.calls.filter(([, options]) => options?.method === 'PATCH')).toHaveLength(1);
});

it.each(['workspace', 'account', 'access'] as const)('ignores delayed reads and writes after a %s change', async kind => {
  const oldRead = deferred<UserPreferences>(); const oldWrite = deferred<UserPreferences>(); const nextRead = deferred<UserPreferences>();
  vi.mocked(request).mockReturnValueOnce(oldRead.promise).mockReturnValueOnce(oldWrite.promise).mockReturnValueOnce(nextRead.promise);
  await act(async () => root.render(<Harness />)); await act(async () => oldRead.resolve(record('enabled'))); await act(async () => toggle().click());
  const account = kind === 'account' ? { ...session, user: { ...session.user, id: 'member' }, csrf_token: 'new-session' } : session;
  const workspace = kind === 'workspace' ? 'other' : 'workspace';
  await act(async () => root.render(<Harness account={account} workspace={workspace} revision={kind === 'access' ? 1 : 0} />));
  expect(enabled()).toBe('false'); expect(toggle().disabled).toBe(true);
  await act(async () => nextRead.resolve(record('enabled', '0', workspace))); expect(enabled()).toBe('true');
  await act(async () => oldWrite.resolve(record('disabled', '1'))); expect(enabled()).toBe('true'); expect(toggle().checked).toBe(true);
  const delayed = deferred<UserPreferences>(); vi.mocked(request).mockReturnValueOnce(delayed.promise).mockResolvedValueOnce(record('disabled', '1', 'third'));
  await act(async () => root.render(<Harness workspace="slow" />)); await act(async () => root.render(<Harness workspace="third" />));
  await act(async () => delayed.resolve(record('enabled', '0', 'slow'))); expect(enabled()).toBe('false'); expect(toggle().checked).toBe(false);
});
