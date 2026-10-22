// @vitest-environment jsdom
import { act, useState, type ReactNode } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { App } from '../App';
import type { Session } from '../types';
import { AccountSecurity } from './AccountSecurity';
import type { SessionInfo } from './types';

const account: Session = { audience: 'web', user: { id: 'owner', email: 'owner@example.test', display_name: 'Owner' }, csrf_token: 'initial-csrf', workspaces: [] };
const current: SessionInfo = { audience: 'web', device_name: null, id: '11111111-1111-4111-8111-111111111111', created_at: '2026-10-17T10:00:00Z', auth_time: '2026-10-17T10:00:00Z', last_seen_at: '2026-10-17T12:00:00Z', expires_at: '2026-10-24T10:00:00Z', idle_expires_at: '2026-10-18T00:00:00Z', current: true };
const other: SessionInfo = { ...current, id: '22222222-2222-4222-8222-222222222222', current: false };
const json = (body: unknown, status = 200) => new Response(JSON.stringify(body), { status });
const page = (data: SessionInfo[], next_cursor: string | null = null) => ({ data, next_cursor, has_more: !!next_cursor });
let container: HTMLDivElement; let root: Root;
beforeEach(() => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true);
  vi.spyOn(window, 'confirm').mockReturnValue(true);
  HTMLDialogElement.prototype.showModal = function () { this.setAttribute('open', ''); };
  HTMLDialogElement.prototype.close = function () { this.removeAttribute('open'); };
  history.replaceState({ otIndex: 0 }, '', '/account/security');
  container = document.createElement('div'); document.body.append(container); root = createRoot(container);
});
afterEach(async () => { await act(async () => root.unmount()); container.remove(); vi.restoreAllMocks(); vi.unstubAllGlobals(); });
async function render(node: ReactNode) { await act(async () => root.render(node)); }
function button(label: string) { return [...container.querySelectorAll<HTMLButtonElement>('button')].find(item => item.textContent === label || item.getAttribute('aria-label') === label)!; }
async function click(label: string) { await act(async () => button(label).click()); }
async function password(value: string) { await act(async () => { const field = container.querySelector<HTMLInputElement>('input[name=password]')!; Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(field, value); field.dispatchEvent(new Event('input', { bubbles: true })); }); }
async function submit() { await act(async () => container.querySelector('form')!.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }))); }
function initialAppResponse(url: string) {
  if (url.endsWith('/setup/status')) return json({ setup_required: false });
  if (url.endsWith('/auth/session')) return json(account);
  throw new Error(`Unexpected request: ${url}`);
}

it('revokes another session using CSRF and refreshes the list without signing out this tab', async () => {
  let entries = [current, other];
  const fetch = vi.fn((_url: string, options: RequestInit) => {
    if (options.method === 'POST') { entries = [current]; return Promise.resolve(new Response(null, { status: 204 })); }
    return Promise.resolve(json(page(entries)));
  });
  vi.stubGlobal('fetch', fetch);
  const signedOut = vi.fn();
  await render(<AccountSecurity session={account} authenticated={() => {}} signedOut={signedOut} />);
  await click(`Revoke session ${other.id}`);
  const write = fetch.mock.calls.find(([, options]) => options.method === 'POST')!;
  expect(write[0]).toBe(`/api/v1/auth/sessions/${other.id}/revoke`);
  expect(new Headers(write[1].headers).get('X-CSRF-Token')).toBe('initial-csrf');
  expect(write[1].credentials).toBe('same-origin');
  expect(signedOut).not.toHaveBeenCalled();
  expect(button(`Revoke session ${other.id}`)).toBeUndefined();
  expect(container.textContent).toContain('Session 22222222 signed out.');
  expect(button('Sign out this session')).toBeDefined();
});

it('opens security without workspace membership and clears the account UI after current-session revocation', async () => {
  const fetch = vi.fn((url: string, _options: RequestInit) => {
    if (url.endsWith('/revoke')) return Promise.resolve(new Response(null, { status: 204 }));
    if (url.includes('/auth/sessions?')) return Promise.resolve(json(page([current])));
    return Promise.resolve(initialAppResponse(url));
  });
  vi.stubGlobal('fetch', fetch);
  await render(<App />);
  expect(container.textContent).toContain('Active sessions');
  expect(container.textContent).not.toContain('No workspace access.');
  vi.mocked(window.confirm).mockReturnValueOnce(false);
  await click('Sign out this session');
  expect(fetch.mock.calls.filter(([, options]) => options.method === 'POST')).toHaveLength(0);
  await click('Sign out this session');
  expect(container.textContent).toContain('Sign in to OpenTrack.');
  expect(container.querySelector('.session-table')).toBeNull();
  expect(container.textContent).not.toContain(account.user.email);
});

it('reauthenticates without silently retrying bulk revocation and uses the refreshed CSRF token on explicit retry', async () => {
  let entries = [current, other]; let bulkCalls = 0;
  let refreshPending = false; let finishRefresh!: (response: Response) => void;
  const rotated = { ...current, id: '33333333-3333-4333-8333-333333333333' };
  const fetch = vi.fn((url: string, _options: RequestInit) => {
    if (url.endsWith('/auth/login')) { entries = [rotated, other]; refreshPending = true; return Promise.resolve(json({ ...account, csrf_token: 'refreshed-csrf' })); }
    if (url.endsWith('/revoke-others')) { bulkCalls += 1; if (bulkCalls === 1) return Promise.resolve(json({ code: 'reauthentication_required', detail: 'Confirm your identity.' }, 403)); entries = [rotated]; return Promise.resolve(new Response(null, { status: 204 })); }
    if (refreshPending) { refreshPending = false; return new Promise<Response>(resolve => { finishRefresh = resolve; }); }
    return Promise.resolve(json(page(entries)));
  });
  vi.stubGlobal('fetch', fetch);
  function SignedInSecurity() { const [session, setSession] = useState(account); return <AccountSecurity session={session} authenticated={setSession} signedOut={() => {}} />; }
  await render(<SignedInSecurity />);
  await click('Sign out other sessions');
  const confirmationTrigger = button('Confirm your password');
  confirmationTrigger.focus();
  await click('Confirm your password');
  expect(document.activeElement).toBe(container.querySelector('input[name=password]'));
  await click('Cancel');
  expect(document.activeElement).toBe(confirmationTrigger);
  await click('Confirm your password');
  expect(container.textContent).toContain('Confirm your password to continue.');
  await password('correct password'); await submit();
  expect(bulkCalls).toBe(1);
  expect(container.textContent).toContain('Password confirmed. Retry your action.');
  expect(container.querySelector('tbody')).toBeNull();
  expect(button('Sign out other sessions').disabled).toBe(true);
  expect(button('Sign out this session')).toBeUndefined();
  expect(container.textContent).toContain('Loading active sessions…');
  expect(confirmationTrigger.isConnected).toBe(false);
  expect(button('Refresh sessions').disabled).toBe(true);
  expect(document.activeElement).not.toBe(button('Refresh sessions'));
  await act(async () => finishRefresh(json(page(entries))));
  expect(button(`Revoke session ${other.id}`)).toBeDefined();
  expect(document.activeElement).toBe(button('Refresh sessions'));
  await click('Sign out other sessions');
  const writes = fetch.mock.calls.filter(([url]) => url.endsWith('/revoke-others'));
  expect(writes).toHaveLength(2);
  expect(new Headers(writes[1][1].headers).get('X-CSRF-Token')).toBe('refreshed-csrf');
  expect(container.textContent).toContain('All other sessions signed out.');
  expect(container.textContent).toContain('Only this session is signed in.');
});

it('allows revoking unloaded sessions when the loaded page contains only the current session', async () => {
  let revoked = false;
  const fetch = vi.fn((url: string) => {
    if (url.endsWith('/revoke-others')) { revoked = true; return Promise.resolve(new Response(null, { status: 204 })); }
    return Promise.resolve(json(page([current], revoked ? null : 'next-page')));
  });
  vi.stubGlobal('fetch', fetch);
  await render(<AccountSecurity session={account} authenticated={() => {}} signedOut={() => {}} />);
  expect(button('Sign out other sessions').disabled).toBe(false);
  expect(container.textContent).not.toContain('Only this session is signed in.');
  await click('Sign out other sessions');
  expect(revoked).toBe(true);
  expect(button('Sign out other sessions').disabled).toBe(true);
  expect(container.textContent).toContain('Only this session is signed in.');
});

it('clears cached session rows on a protected 401', async () => {
  let expired = false;
  vi.stubGlobal('fetch', vi.fn((url: string) => Promise.resolve(url.includes('/auth/sessions?') ? expired ? json({ code: 'unauthenticated', detail: 'Session expired.' }, 401) : json(page([current, other])) : initialAppResponse(url))));
  await render(<App />);
  expect(button(`Revoke session ${other.id}`)).toBeDefined();
  expired = true;
  await click('Refresh sessions');
  expect(container.textContent).toContain('Your session has ended.');
  expect(container.querySelector('.session-table')).toBeNull();
  expect(container.textContent).not.toContain('Session 22222222');
});

it('ignores a late current-session revoke response after the security view closes', async () => {
  let finish!: (value: Response) => void; let signal: AbortSignal | undefined;
  vi.stubGlobal('fetch', vi.fn((_url: string, options: RequestInit) => options.method === 'POST' ? new Promise<Response>(resolve => { finish = resolve; signal = options.signal as AbortSignal; }) : Promise.resolve(json(page([current])))));
  const signedOut = vi.fn();
  await render(<AccountSecurity session={account} authenticated={() => {}} signedOut={signedOut} />);
  await click('Sign out this session');
  await render(<div>Another account view</div>);
  expect(signal?.aborted).toBe(true);
  await act(async () => finish(new Response(null, { status: 204 })));
  expect(signedOut).not.toHaveBeenCalled();
});

it('loads the next opaque session cursor without replacing existing rows', async () => {
  const cursor = 'actor-bound:page/2';
  const fetch = vi.fn((url: string) => Promise.resolve(json(url.includes('cursor=') ? page([other]) : page([current], cursor))));
  vi.stubGlobal('fetch', fetch);
  await render(<AccountSecurity session={account} authenticated={() => {}} signedOut={() => {}} />);
  await click('Load more');
  expect(fetch.mock.calls.at(-1)?.[0]).toBe(`/api/v1/auth/sessions?limit=50&cursor=${encodeURIComponent(cursor)}`);
  expect(container.querySelectorAll('tbody tr')).toHaveLength(2);
  expect(button('Sign out this session')).toBeDefined();
  expect(button(`Revoke session ${other.id}`)).toBeDefined();
  expect(button('Load more')).toBeUndefined();
  const scrollRegion = container.querySelector<HTMLElement>('[role="region"][aria-label="Session details"]')!;
  scrollRegion.focus();
  expect(document.activeElement).toBe(scrollRegion);
});

it('retries a failed next page from Load more while retaining the loaded session rows', async () => {
  let pageAttempts = 0;
  const fetch = vi.fn((url: string) => {
    if (!url.includes('cursor=')) return Promise.resolve(json(page([current], 'next-page')));
    pageAttempts += 1;
    return Promise.resolve(pageAttempts === 1 ? json({ code: 'unavailable', detail: 'Temporary failure.' }, 503) : json(page([other])));
  });
  vi.stubGlobal('fetch', fetch);
  await render(<AccountSecurity session={account} authenticated={() => {}} signedOut={() => {}} />);
  await click('Load more');
  expect(container.querySelectorAll('tbody tr')).toHaveLength(1);
  expect(container.textContent).toContain('Temporary failure.');
  await click('Load more');
  expect(pageAttempts).toBe(2);
  expect(container.querySelectorAll('tbody tr')).toHaveLength(2);
  expect(container.textContent).not.toContain('Temporary failure.');
});
