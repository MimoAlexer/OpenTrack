// @vitest-environment jsdom
import { act, useState, type ReactNode } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { Session } from '../types';
import { MemberSettings } from './MemberSettings';
import { TeamSettings } from './TeamSettings';
import { InvitationSettings } from './InvitationSettings';
import { JoinWorkspace } from './JoinWorkspace';
import { WorkspaceSettings } from './WorkspaceSettings';
import { SettingsScreen } from './SettingsScreen';
import { ApiError } from '../api';
import { hasUnsavedChanges, navigate } from '../router';
import { Reauthenticate, SettingsError, useDirectory } from './shared';

const workspace = { id: 'w', name: 'Workspace', slug: 'workspace', role: 'owner' };
const session: Session = { audience: 'web', user: { id: 'owner', display_name: 'Owner', email: 'owner@example.test' }, csrf_token: 'csrf', workspaces: [workspace] };
const invited: Session = { ...session, user: { id: 'new', display_name: 'New member', email: 'new@example.test' }, csrf_token: 'new-csrf' };
const preview = { workspace_id: 'w', workspace_name: 'Workspace', email: invited.user.email, role: 'member', expires_at: '2099-10-24T12:00:00Z' };
const json = (value: unknown, status = 200) => new Response(JSON.stringify(value), { status });
const page = (data: unknown[], next_cursor: string | null = null) => ({ data, next_cursor, has_more: !!next_cursor });
let container: HTMLDivElement; let root: Root;
beforeEach(() => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true);
  vi.spyOn(window, 'confirm').mockReturnValue(true);
  HTMLDialogElement.prototype.showModal = function () { this.setAttribute('open', ''); };
  HTMLDialogElement.prototype.close = function () { this.removeAttribute('open'); };
  container = document.createElement('div'); document.body.append(container); root = createRoot(container);
});
afterEach(async () => { await act(async () => root.unmount()); container.remove(); vi.restoreAllMocks(); vi.unstubAllGlobals(); });
async function render(node: ReactNode) { await act(async () => root.render(node)); }
function button(label: string) { return [...container.querySelectorAll<HTMLButtonElement>('button')].find(element => element.textContent === label)!; }
async function click(label: string) { await act(async () => button(label).click()); }
async function input(element: HTMLInputElement | HTMLTextAreaElement | HTMLSelectElement, value: string) {
  await act(async () => {
    const prototype = element instanceof HTMLSelectElement ? HTMLSelectElement.prototype : element instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(prototype, 'value')!.set!.call(element, value);
    element.dispatchEvent(new Event(element instanceof HTMLSelectElement ? 'change' : 'input', { bubbles: true }));
  });
}
async function submit(form: HTMLFormElement) { await act(async () => form.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }))); }

it.each(['workspace', 'team'] as const)('repairs a rejected supplementary %s rename while preserving its draft and version precondition', async kind => {
  let current = { ...workspace, organization_id: 'organization', version: '1', archived_at: null, ...(kind === 'team' ? { id: 't', workspace_id: 'w', key: 'NEW', private: true, can_manage: true } : {}) }; let reject = true; const changed = vi.fn(async () => {});
  const fetch = vi.fn((url: string, options: RequestInit) => {
    if (options.method === 'PATCH') { if (reject) return Promise.resolve(json({ code: 'invalid', detail: 'Name exceeds its scalar limit' }, 422)); current = { ...current, name: JSON.parse(options.body as string).name, version: '2' }; return Promise.resolve(json(current)); }
    return Promise.resolve(json(url.includes('/members') ? page([]) : kind === 'team' && url.includes('/teams?') ? page([current]) : current));
  }); vi.stubGlobal('fetch', fetch);
  await render(kind === 'workspace' ? <WorkspaceSettings workspaceId="w" session={session} reauthenticate={() => {}} changed={changed}/> : <TeamSettings workspace={workspace} session={session} reauthenticate={() => {}} changed={changed}/>);
  const field = container.querySelector<HTMLInputElement>(kind === 'workspace' ? '.settings-form input' : '.settings-team-detail input')!; const name = '🧭'.repeat(100); await input(field, name + '🧭'); await submit(field.closest('form')!);
  const writes = () => fetch.mock.calls.filter(([, options]) => options.method === 'PATCH'); expect(JSON.parse(writes()[0][1].body as string)).toMatchObject({ name: name + '🧭' }); expect(field.value).toBe(name + '🧭'); expect(changed).not.toHaveBeenCalled(); expect(hasUnsavedChanges()).toBe(true); expect(current.version).toBe('1');
  reject = false; await input(field, ` ${name} `); await submit(field.closest('form')!); expect(JSON.parse(writes()[1][1].body as string)).toMatchObject({ name }); expect(new Headers(writes()[1][1].headers).get('If-Match')).toBe(new Headers(writes()[0][1].headers).get('If-Match')); expect(writes()).toHaveLength(2); expect(changed).toHaveBeenCalledTimes(1); expect(current).toMatchObject({ name, version: '2' });
});

it('repairs a rejected supplementary team creation without changing the prefix or issuing another successful callback', async () => {
  let posts = 0; const changed = vi.fn(async () => {}); const bodies: unknown[] = [];
  vi.stubGlobal('fetch', vi.fn((_url: string, options: RequestInit = {}) => { if (options.method !== 'POST') return Promise.resolve(json(page([]))); bodies.push(JSON.parse(options.body as string)); return Promise.resolve(++posts === 1 ? json({ code: 'invalid', detail: 'Name exceeds its scalar limit' }, 422) : json({ id: 'created-team' }, 201)); }));
  await render(<TeamSettings workspace={workspace} session={session} reauthenticate={() => {}} changed={changed}/>); await click('New team'); const form = container.querySelector<HTMLFormElement>('dialog form')!; const name = '😀'.repeat(100); const field = form.querySelector<HTMLInputElement>('input[name="name"]')!;
  await input(field, name + '😀'); await input(form.querySelector<HTMLInputElement>('input[name="key"]')!, 'NEW'); await submit(form); expect(bodies[0]).toEqual({ name: name + '😀', key: 'NEW', private: false }); expect(field.value).toBe(name + '😀'); expect(changed).not.toHaveBeenCalled();
  await input(field, name); await submit(form); expect(bodies[1]).toEqual({ name, key: 'NEW', private: false }); expect(posts).toBe(2); expect(changed).toHaveBeenCalledTimes(1); expect(container.querySelector('dialog')).toBeNull();
});

it('keeps drafts through incorrect password confirmation and replaces only the stale reauthentication error after success', async () => {
  const authenticated = vi.fn();
  const conflict = new ApiError(412, 'version_conflict', 'A separate record changed.');
  function DraftWithReauthentication() {
    const [reauth, setReauth] = useState(true);
    const [error, setError] = useState(new ApiError(403, 'reauthentication_required', 'Sign in again.'));
    return <><input aria-label="Workspace draft" defaultValue="Keep this draft" /><SettingsError error={error} reauthenticate={() => setReauth(true)} /><SettingsError error={conflict} reauthenticate={() => setReauth(true)} /><button onClick={() => setError(new ApiError(403, 'reauthentication_required', 'A new password confirmation is required.'))}>New protected failure</button>{reauth && <Reauthenticate session={session} authenticated={authenticated} close={() => setReauth(false)} />}</>;
  }
  vi.stubGlobal('fetch', vi.fn()
    .mockResolvedValueOnce(json({ code: 'unauthenticated', detail: 'Sign in to continue.', request_id: 'login-request' }, 401))
    .mockResolvedValueOnce(json(session)));
  await render(<DraftWithReauthentication />);
  await input(container.querySelector<HTMLInputElement>('input[name=password]')!, 'incorrect password');
  await submit(container.querySelector('form')!);
  expect(container.textContent).toContain('That password was not accepted. Try again.');
  expect(container.textContent).toContain('Request login-request');
  expect(container.textContent).not.toContain('Password confirmed.');
  expect(authenticated).not.toHaveBeenCalled();
  await input(container.querySelector<HTMLInputElement>('input[name=password]')!, 'correct password');
  await submit(container.querySelector('form')!);
  expect(container.querySelector('dialog')).toBeNull();
  expect(container.querySelector<HTMLInputElement>('[aria-label="Workspace draft"]')?.value).toBe('Keep this draft');
  expect(container.textContent).toContain('Password confirmed. Retry your action.');
  expect(container.textContent).not.toContain('Sign in again.');
  expect(button('Confirm your password')).toBeUndefined();
  expect(container.textContent).toContain('A separate record changed.');
  expect(authenticated).toHaveBeenCalledWith(session);
  await click('New protected failure');
  expect(container.textContent).toContain('A new password confirmation is required.');
  expect(button('Confirm your password')).toBeDefined();
});

it('keeps a rejected member role change and requires a deliberate conflict reload', async () => {
  const member = { user: invited.user, role: 'member', active: true, version: '7' };
  const fetch = vi.fn((_url: string, options: RequestInit) => Promise.resolve(options.method === 'PATCH' ? json({ type: 'about:blank', title: 'Conflict', status: 412, code: 'version_conflict', detail: 'Membership changed.' }, 412) : json(page([member]))));
  vi.stubGlobal('fetch', fetch);
  const changed = vi.fn(async () => {});
  await render(<MemberSettings workspace={workspace} session={session} changed={changed} reauthenticate={() => {}} />);
  await input(container.querySelector('select[aria-label]') ?? container.querySelectorAll('select')[1], 'viewer');
  await click('Save role');
  const writes = fetch.mock.calls.filter(([, options]) => options.method === 'PATCH');
  expect(writes).toHaveLength(1);
  expect(new Headers(writes[0][1].headers).get('If-Match')).toBe('"member:w:new:7"');
  expect(JSON.parse(writes[0][1].body as string)).toEqual({ role: 'viewer' });
  expect(container.textContent).toContain('Your proposed changes have not been applied');
  expect(container.querySelectorAll('select')[1].value).toBe('viewer');
  expect(changed).not.toHaveBeenCalled();
});

it.each([403, 503])('preserves a workspace name draft after a %s mutation failure without offering a destructive reload', async status => {
  const detail = { ...workspace, organization_id: 'org', version: '1', archived_at: null };
  const fetch = vi.fn((_url: string, options: RequestInit) => Promise.resolve(options.method === 'PATCH' ? json({ code: status === 403 ? 'reauthentication_required' : 'unavailable', detail: 'The change was not saved.' }, status) : json(detail)));
  vi.stubGlobal('fetch', fetch);
  await render(<WorkspaceSettings workspaceId="w" session={session} changed={async () => {}} reauthenticate={() => {}} />);
  await input(container.querySelector<HTMLInputElement>('input')!, 'Unsubmitted new name');
  await submit(container.querySelector('form')!);
  expect(container.querySelector<HTMLInputElement>('input')?.value).toBe('Unsubmitted new name');
  expect(container.textContent).toContain('The change was not saved.');
  expect(button('Try again')).toBeUndefined();
  expect(button('Save changes').disabled).toBe(false);
  expect(fetch.mock.calls.filter(([, options]) => options.method === 'PATCH')).toHaveLength(1);
});

it('retains a safe retry for an initial workspace load failure', async () => {
  vi.stubGlobal('fetch', vi.fn().mockResolvedValue(json({ code: 'unavailable', detail: 'Could not load workspace.' }, 503)));
  await render(<WorkspaceSettings workspaceId="w" session={session} changed={async () => {}} reauthenticate={() => {}} />);
  expect(button('Try again')).toBeDefined();
});

it('keeps archived workspace settings readable while disabling team, member, and invitation writes', async () => {
  const team = { id: 't', workspace_id: 'w', name: 'Engineering', key: 'ENG', private: false, version: '1', archived_at: null, can_manage: true };
  const member = { user: invited.user, role: 'member', active: true, version: '1' };
  const invitation = { id: 'inv', workspace_id: 'w', email: invited.user.email, role: 'member', team_ids: [], expires_at: preview.expires_at, accepted_at: null, revoked_at: null, version: '1' };
  vi.stubGlobal('fetch', vi.fn((url: string) => Promise.resolve(json(page(url.includes('/invitations') ? [invitation] : url.includes('/members') ? [member] : [team])))));
  const props = { workspace, session, workspaceArchived: true, changed: async () => {}, authenticated: () => {} };
  await render(<SettingsScreen {...props} section="teams" />);
  expect(container.textContent).toContain('Restore it in Workspace settings');
  expect(button('New team').disabled).toBe(true);
  expect(container.querySelector<HTMLInputElement>('.settings-team-detail input')?.matches(':disabled')).toBe(true);
  expect(button('Archive team').disabled).toBe(true);
  expect(button('Add member')).toBeUndefined();
  expect(button('Remove')).toBeUndefined();
  await render(<SettingsScreen {...props} section="members" />);
  expect(container.textContent).toContain(invited.user.email);
  expect(button('Deactivate')).toBeUndefined();
  expect(container.querySelectorAll('select')).toHaveLength(1);
  await render(<SettingsScreen {...props} section="invitations" />);
  expect(container.textContent).toContain(invitation.email);
  expect(button('Create invitation').disabled).toBe(true);
  expect(button('Revoke').disabled).toBe(true);
});

it('uses the server team management capability, even for a workspace owner', async () => {
  const team = { id: 't', workspace_id: 'w', name: 'Restricted team', key: 'SEC', private: true, version: '1', archived_at: null, can_manage: false };
  vi.stubGlobal('fetch', vi.fn((url: string) => Promise.resolve(json(page(url.includes('/members') ? [] : [team])))));
  await render(<TeamSettings workspace={workspace} session={session} changed={async () => {}} reauthenticate={() => {}} />);
  expect(container.querySelector<HTMLInputElement>('.settings-team-detail input')?.matches(':disabled')).toBe(true);
  expect(button('Add member')).toBeUndefined();
  expect(button('Archive team')).toBeUndefined();
  expect(container.textContent).toContain('A team maintainer manages its settings');
});

it('locks membership writes while team settings are dirty and unlocks them after discard', async () => {
  const team = { id: 't', workspace_id: 'w', name: 'Engineering', key: 'ENG', private: false, version: '1', archived_at: null, can_manage: true };
  const member = { user: invited.user, role: 'contributor', version: '1' };
  const fetch = vi.fn((url: string) => Promise.resolve(json(page(url.includes('/members') ? [member] : [team]))));
  vi.stubGlobal('fetch', fetch);
  await render(<TeamSettings workspace={workspace} session={session} changed={async () => {}} reauthenticate={() => {}} />);
  await input(container.querySelector<HTMLInputElement>('.settings-team-detail input')!, 'Unsubmitted team name');
  expect(button('Add member').disabled).toBe(true);
  expect(button('Remove').disabled).toBe(true);
  expect(container.querySelector<HTMLSelectElement>('select[aria-label="Team role for New member"]')?.disabled).toBe(true);
  expect(container.textContent).toContain('Save or discard team settings before changing membership.');
  await click('Remove');
  expect(fetch.mock.calls).toHaveLength(2);
  expect(container.querySelector<HTMLInputElement>('.settings-team-detail input')?.value).toBe('Unsubmitted team name');
  await click('Discard changes');
  expect(container.querySelector<HTMLInputElement>('.settings-team-detail input')?.value).toBe('Engineering');
  expect(button('Add member').disabled).toBe(false);
  expect(button('Remove').disabled).toBe(false);
});

it('retries an uncertain invitation with the same key and explains an unrecoverable one-time token', async () => {
  let writes = 0;
  const invitation = { id: 'inv', workspace_id: 'w', email: invited.user.email, role: 'member', team_ids: [], expires_at: preview.expires_at, accepted_at: null, revoked_at: null, version: '1' };
  const fetch = vi.fn((_url: string, options: RequestInit) => {
    if (options.method === 'POST') { writes += 1; return writes === 1 ? Promise.reject(new TypeError('Connection lost')) : Promise.resolve(json({ invitation, token: null })); }
    return Promise.resolve(json(page([])));
  });
  vi.stubGlobal('fetch', fetch);
  await render(<InvitationSettings workspace={workspace} session={session} reauthenticate={() => {}} />);
  await click('Create invitation');
  await input(container.querySelector<HTMLInputElement>('input[name=email]')!, invited.user.email);
  await submit(container.querySelector('form')!);
  expect(container.textContent).toContain('The response was interrupted');
  await click('Retry safely');
  const requests = fetch.mock.calls.filter(([, options]) => options.method === 'POST');
  expect(requests).toHaveLength(2);
  expect(new Headers(requests[0][1].headers).get('Idempotency-Key')).toBe(new Headers(requests[1][1].headers).get('Idempotency-Key'));
  expect(requests[0][1].body).toBe(requests[1][1].body);
  expect(container.textContent).toContain('cannot be recovered');
  expect(button('Copy token')).toBeUndefined();
});

describe('manual invitation acceptance', () => {
  it('signs in the invited account before acceptance and keeps the token out of URLs and storage', async () => {
    const secret = 'single-use-secret';
    const storage = vi.spyOn(Storage.prototype, 'setItem');
    const fetch = vi.fn((url: string) => Promise.resolve(json(url.endsWith('/preview') ? preview : invited)));
    vi.stubGlobal('fetch', fetch);
    const authenticated = vi.fn();
    await render(<JoinWorkspace session={null} authenticated={authenticated} />);
    await input(container.querySelector('textarea')!, secret);
    await submit(container.querySelector('form')!);
    await input(container.querySelector<HTMLInputElement>('input[name=password]')!, 'correct password');
    await submit(container.querySelector('form')!);
    const calls = fetch.mock.calls as unknown as [string, RequestInit][];
    expect(calls.map(([url]) => url)).toEqual(['/api/v1/invitations/preview', '/api/v1/auth/login', '/api/v1/invitations/accept']);
    expect(JSON.parse(calls[1][1].body as string)).toEqual({ email: invited.user.email, password: 'correct password' });
    expect(JSON.parse(calls[2][1].body as string)).toEqual({ token: secret });
    expect(new Headers(calls[2][1].headers).get('X-CSRF-Token')).toBe('new-csrf');
    expect(calls.every(([url]) => !url.includes(secret))).toBe(true);
    expect(storage).not.toHaveBeenCalled();
    expect(location.href).not.toContain(secret);
    expect(authenticated).toHaveBeenLastCalledWith(invited);
  });
  it('does not apply a late acceptance after the invitation view is left', async () => {
    let finish!: (response: Response) => void; let acceptanceSignal: AbortSignal | undefined;
    vi.stubGlobal('fetch', vi.fn((url: string, options: RequestInit) => url.endsWith('/accept') ? new Promise<Response>(resolve => { finish = resolve; acceptanceSignal = options.signal as AbortSignal; }) : Promise.resolve(json(preview))));
    const authenticated = vi.fn();
    await render(<JoinWorkspace session={invited} authenticated={authenticated} />);
    await input(container.querySelector('textarea')!, 'secret');
    await submit(container.querySelector('form')!);
    await submit(container.querySelector('form')!);
    await render(<div>Another workspace</div>);
    expect(acceptanceSignal?.aborted).toBe(true);
    await act(async () => finish(json(invited)));
    expect(authenticated).not.toHaveBeenCalled();
  });
});

it('does not carry a directory cursor or rows into another workspace', async () => {
  function Directory({ path }: { path: string }) { const directory = useDirectory<{ name: string }>(path); return <><p>{directory.items.map(item => item.name).join(',')}</p><button onClick={directory.more}>More</button></>; }
  const fetch = vi.fn((url: string) => Promise.resolve(json(page([{ name: url.includes('/a/') ? 'Private A' : 'Workspace B' }], url.includes('cursor=') ? null : 'next-a'))));
  vi.stubGlobal('fetch', fetch);
  await render(<Directory path="/workspaces/a/teams" />); await click('More');
  await render(<Directory path="/workspaces/b/teams" />);
  expect(fetch.mock.calls.at(-1)?.[0]).toBe('/api/v1/workspaces/b/teams?limit=50');
  expect(container.textContent).not.toContain('Private A');
  expect(container.textContent).toContain('Workspace B');
});


it.each([403, 503])('does not describe an initial team load failure as an empty directory: %s', async status => {
  vi.stubGlobal('fetch', vi.fn().mockResolvedValue(json({ code: status === 403 ? 'forbidden' : 'unavailable', detail: 'Teams are unavailable.' }, status)));
  await render(<TeamSettings workspace={workspace} session={session} changed={async () => {}} reauthenticate={() => {}} />);
  expect(container.textContent).toContain('Teams are unavailable.');
  expect(container.textContent).not.toContain('No active teams.');
  expect(button('Try again')).toBeDefined();
});

it('retries invitation team discovery without losing the email draft', async () => {
  let failed = true;
  vi.stubGlobal('fetch', vi.fn((url: string) => Promise.resolve(url.includes('/teams') && failed ? json({ code: 'unavailable', detail: 'Teams could not be loaded.' }, 503) : json(page([])))));
  await render(<InvitationSettings workspace={workspace} session={session} reauthenticate={() => {}} />);
  await click('Create invitation');
  const email = container.querySelector<HTMLInputElement>('input[name=email]')!;
  await input(email, 'member@example.test');
  expect(container.textContent).toContain('Teams could not be loaded.');
  failed = false; await click('Try again');
  expect(email.value).toBe('member@example.test');
  expect(container.textContent).toContain('No teams are available.');
});

it('preserves a rejected supplementary invited name and repairs it with one authentication callback', async () => {
  const previous = { url: `${location.pathname}${location.search}`, state: history.state }; const authenticated = vi.fn(); const names: string[] = []; const name = '😀'.repeat(100);
  vi.stubGlobal('fetch', vi.fn((url: string, options: RequestInit) => { if (url.endsWith('/preview')) return Promise.resolve(json(preview)); const displayName = JSON.parse(options.body as string).display_name as string; names.push(displayName); return Promise.resolve(names.length === 1 ? json({ code: 'invalid', detail: 'Name must contain 1–100 characters' }, 422) : json({ ...invited, user: { ...invited.user, display_name: displayName } })); }));
  try {
    await render(<JoinWorkspace session={null} authenticated={authenticated}/>); await input(container.querySelector('textarea')!, 'synthetic-invitation'); await submit(container.querySelector('form')!); await click('Create an account'); const field = container.querySelector<HTMLInputElement>('input[name="display_name"]')!;
    await input(field, name + '😀'); await input(container.querySelector<HTMLInputElement>('input[name="password"]')!, 'synthetic-new-account-password'); await submit(container.querySelector('form')!); expect(names).toEqual([name + '😀']); expect(field.value).toBe(name + '😀'); expect(authenticated).not.toHaveBeenCalled(); expect(field.disabled).toBe(false);
    await input(field, name); await submit(container.querySelector('form')!); expect(names).toEqual([name + '😀', name]); expect(authenticated).toHaveBeenCalledTimes(1); expect((authenticated.mock.calls[0][0] as Session).user.display_name).toBe(name);
  } finally { history.replaceState(previous.state, '', previous.url); }
});

it('focuses invitation review and rejected credentials without discarding the token', async () => {
  const fetch = vi.fn((url: string) => Promise.resolve(url.endsWith('/preview') ? json(preview) : json({ code: 'unauthenticated', detail: 'Sign in to continue.' }, 401)));
  vi.stubGlobal('fetch', fetch);
  await render(<JoinWorkspace session={null} authenticated={() => {}} />);
  await input(container.querySelector('textarea')!, 'synthetic-invitation');
  await submit(container.querySelector('form')!);
  expect(document.activeElement?.tagName).toBe('H1');
  const password = container.querySelector<HTMLInputElement>('input[name=password]')!;
  await input(password, 'synthetic rejected password');
  await submit(container.querySelector('form')!);
  expect(document.activeElement).toBe(password);
  expect(password.getAttribute('aria-invalid')).toBe('true');
  expect(document.getElementById(password.getAttribute('aria-describedby')!)?.getAttribute('tabindex')).toBe('-1');
  await click('Use a different token');
  expect(document.activeElement).toBe(container.querySelector('textarea'));
  expect(container.querySelector('textarea')?.value).toBe('synthetic-invitation');
});


describe('selected team settings outside the loaded directory page', () => {
  const target = { id: 'selected', workspace_id: 'w', name: 'Selected team', key: 'SEL', private: false, version: '1', archived_at: null, can_manage: true };
  beforeEach(() => { history.replaceState({ otIndex: 0 }, '', '/w/w/settings/teams?team_id=selected'); });
  afterEach(() => { history.replaceState({ otIndex: 0 }, '', '/'); });

  it('loads a deep link independently of the first 50 rows, preserves reauthentication drafts and refreshes the saved version', async () => {
    let current = { ...target };
    const firstPage = Array.from({ length: 50 }, (_, index) => ({ ...target, id: `listed-${index}`, name: `Listed team ${index}` }));
    const writes: string[] = [];
    const fetch = vi.fn((url: string, options: RequestInit = {}) => {
      if (url.includes('/teams?')) return Promise.resolve(json(page(firstPage, 'next-page')));
      if (url.endsWith('/members?limit=50')) return Promise.resolve(json(page([])));
      if (options.method === 'PATCH') {
        writes.push(new Headers(options.headers).get('If-Match')!);
        current = { ...current, ...JSON.parse(options.body as string), version: String(Number(current.version) + 1) };
      }
      return Promise.resolve(json(current));
    });
    vi.stubGlobal('fetch', fetch);
    const props = { workspace, session, changed: async () => {}, reauthenticate: () => {} };
    await render(<TeamSettings {...props} />);
    expect(container.querySelectorAll('.settings-team-list > button')).toHaveLength(50);
    expect(container.querySelector<HTMLInputElement>('.settings-team-detail input')?.value).toBe(target.name);
    expect(button('Load more')).toBeDefined();
    await input(container.querySelector<HTMLInputElement>('.settings-team-detail input')!, 'First rename');
    await render(<TeamSettings {...props} session={{ ...session, csrf_token: 'rotated-csrf' }} />);
    expect(container.querySelector<HTMLInputElement>('.settings-team-detail input')?.value).toBe('First rename');
    await submit(container.querySelector('.settings-team-detail form')!);
    expect(container.querySelector<HTMLInputElement>('.settings-team-detail input')?.value).toBe('First rename');
    await input(container.querySelector<HTMLInputElement>('.settings-team-detail input')!, 'Second rename');
    await submit(container.querySelector('.settings-team-detail form')!);
    expect(writes).toEqual(['"team:selected:1"', '"team:selected:2"']);
    expect(fetch.mock.calls.some(([url]) => url.includes('cursor='))).toBe(false);
  });

  it.each([403, 404, 503])('conceals denied private team metadata while keeping %s recovery separate from the directory', async status => {
    const privateTarget = { ...target, name: 'Private cached team', key: 'HIDDENTEAM', private: true };
    let finish!: (response: Response) => void; let retried = false; let directoryReads = 0;
    vi.stubGlobal('fetch', vi.fn((url: string) => {
      if (url.includes('/teams?')) { directoryReads++; return Promise.resolve(json(page([privateTarget]))); }
      if (url.includes('/members?')) return Promise.resolve(json(page([])));
      if (!retried) return new Promise<Response>(resolve => { finish = resolve; });
      return Promise.resolve(json({ ...privateTarget, archived_at: '2026-10-22T12:00:00Z' }));
    }));
    await render(<TeamSettings workspace={workspace} session={session} changed={async () => {}} reauthenticate={() => {}} />);
    expect(container.textContent).toContain('Loading selected team');
    expect(container.querySelector('.settings-team-detail')).toBeNull();
    await act(async () => finish(json({ code: status === 403 ? 'forbidden' : status === 404 ? 'not_found' : 'unavailable', detail: 'Selected team unavailable.' }, status)));
    expect(container.querySelector('.settings-team-panel')?.textContent).toContain('Selected team unavailable.');
    expect(container.querySelector('.settings-team-detail')).toBeNull();
    expect(container.textContent).not.toContain('No active teams.');
    if (status === 503) {
      expect(container.textContent).toContain(privateTarget.name); expect(container.textContent).toContain(privateTarget.key);
      expect(directoryReads).toBe(1);
    } else {
      expect(container.textContent).not.toContain(privateTarget.name); expect(container.textContent).not.toContain(privateTarget.key);
      expect(directoryReads).toBe(2);
      expect(button('Add member')).toBeUndefined(); expect(button('Archive team')).toBeUndefined(); expect(button('Save team')).toBeUndefined();
    }
    retried = true;
    await act(async () => container.querySelector<HTMLButtonElement>('.settings-team-panel button')!.click());
    expect(container.querySelector<HTMLInputElement>('.settings-team-detail input')?.value).toBe(privateTarget.name);
    expect(container.querySelector<HTMLInputElement>('.settings-team-detail input')?.matches(':disabled')).toBe(true);
    expect(button('Restore team')).toBeDefined();
  });

  it('opens an authorized selected team while the separate directory request fails', async () => {
    vi.stubGlobal('fetch', vi.fn((url: string) => Promise.resolve(url.includes('/teams?')
      ? json({ code: 'unavailable', detail: 'Team directory unavailable.' }, 503)
      : url.includes('/members?') ? json(page([])) : json(target))));
    await render(<TeamSettings workspace={workspace} session={session} changed={async () => {}} reauthenticate={() => {}} />);
    expect(container.textContent).toContain('Team directory unavailable.');
    expect(container.querySelector<HTMLInputElement>('.settings-team-detail input')?.value).toBe(target.name);
    expect(container.textContent).not.toContain('No active teams.');
  });

  it.each(['selection', 'workspace', 'account'] as const)('discards a late selected-team response after changing %s scope', async change => {
    let finishOld!: (response: Response) => void; let finishCurrent!: (response: Response) => void; let changed = false;
    vi.stubGlobal('fetch', vi.fn((url: string) => {
      if (url.includes('/teams?')) return Promise.resolve(json(page([{ ...target, id: changed ? 'current-list' : 'old-list', name: changed ? 'Current list row' : 'Previous private list row' }])));
      if (url.includes('/members?')) return Promise.resolve(json(page([])));
      return new Promise<Response>(resolve => { if (changed) finishCurrent = resolve; else finishOld = resolve; });
    }));
    const props = { workspace, session, changed: async () => {}, reauthenticate: () => {} };
    await render(<TeamSettings {...props} />);
    changed = true;
    if (change === 'selection') await act(async () => { navigate('/w/w/settings/teams?team_id=current'); });
    else await render(<TeamSettings {...props} workspace={change === 'workspace' ? { ...workspace, id: 'other' } : workspace} session={change === 'account' ? { ...session, user: { ...session.user, id: 'other-owner' } } : session} />);
    await act(async () => finishOld(json({ ...target, name: 'Previous private team' })));
    expect(container.querySelector('.settings-team-detail')).toBeNull();
    expect(container.textContent).not.toContain('Previous private team');
    if (change !== 'selection') expect(container.textContent).not.toContain('Previous private list row');
    await act(async () => finishCurrent(json({ ...target, id: change === 'selection' ? 'current' : target.id, workspace_id: change === 'workspace' ? 'other' : 'w', name: 'Current selected team' })));
    expect(container.querySelector<HTMLInputElement>('.settings-team-detail input')?.value).toBe('Current selected team');
  });
});
