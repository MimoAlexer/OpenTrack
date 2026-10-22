// @vitest-environment jsdom
import { act, type ReactNode } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { App } from '../App';
import { navigate, parseRoute } from '../router';
import type { Session } from '../types';
import { RecoveryScreen } from './RecoveryScreen';

const token = 'operator-issued-single-use-code';
const password = 'A new recovery passphrase 2026!';
const account: Session = { audience: 'web', user: { id: 'owner', email: 'owner@example.test', display_name: 'Owner' }, csrf_token: 'existing-csrf', workspaces: [] };
const json = (body: unknown, status = 200) => new Response(JSON.stringify(body), { status });
let container: HTMLDivElement; let root: Root;
beforeEach(() => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true);
  history.replaceState({ otIndex: 0 }, '', '/recover');
  container = document.createElement('div'); document.body.append(container); root = createRoot(container);
});
afterEach(async () => { await act(async () => root.unmount()); container.remove(); vi.restoreAllMocks(); vi.unstubAllGlobals(); });
async function render(node: ReactNode) { await act(async () => root.render(node)); }
function field(name: string) { return container.querySelector<HTMLInputElement>(`input[name="${name}"]`)!; }
function button(text: string) { return [...container.querySelectorAll<HTMLButtonElement>('button')].find(item => item.textContent === text)!; }
async function fill(name: string, value: string) {
  await act(async () => {
    const input = field(name);
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(input, value);
    input.dispatchEvent(new Event('input', { bubbles: true }));
  });
}
async function fillReset() { await fill('recovery_token', ` ${token} `); await fill('password', password); await fill('password_confirmation', password); }
async function submit() { await act(async () => container.querySelector('form')!.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }))); }
async function clickLink(text: string) { await act(async () => [...container.querySelectorAll<HTMLAnchorElement>('a')].find(item => item.textContent === text)!.click()); }

it('opens the public recovery route without waiting for setup or session APIs and supports pasted/password-manager values', async () => {
  const fetch = vi.fn(); vi.stubGlobal('fetch', fetch);
  const storageWrites = vi.spyOn(Storage.prototype, 'setItem');
  await render(<App />);
  expect(fetch).not.toHaveBeenCalled();
  expect(container.textContent).toContain('No recovery email is sent.');
  expect(container.textContent).toContain('The code identifies the account to reset.');
  expect(document.activeElement).toBe(field('recovery_token'));
  expect(field('recovery_token').autocomplete).toBe('one-time-code');
  expect(field('password').autocomplete).toBe('new-password');
  expect(field('password_confirmation').autocomplete).toBe('new-password');
  await fillReset();
  expect(field('password').value).toBe(password);
  expect(location.pathname).toBe('/recover');
  expect(location.search).toBe('');
  expect(storageWrites.mock.calls.every(([key, value]) => key === 'opentrack:theme' && ['system', 'light', 'dark'].includes(value))).toBe(true);
});

it('submits actual password-manager-filled controls even without input events', async () => {
  const fetch = vi.fn().mockResolvedValue(new Response(null, { status: 204 }));
  vi.stubGlobal('fetch', fetch);
  await render(<RecoveryScreen onCompleted={() => {}} />);
  for (const [name, value] of [['recovery_token', token], ['password', password], ['password_confirmation', password]]) {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(field(name), value);
  }
  await submit();
  expect(JSON.parse(fetch.mock.calls[0][1].body)).toEqual({ token, password });
  expect(container.textContent).toContain('Password reset.');
});

it('confirms a successful reset, clears the current App session and all secret fields, then requires a fresh sign-in', async () => {
  history.replaceState({ otIndex: 0 }, '', '/');
  let reset = false; let finish!: (response: Response) => void;
  const fetch = vi.fn((url: string, _options: RequestInit) => {
    if (url.endsWith('/auth/recovery/complete')) return new Promise<Response>(resolve => { finish = resolve; });
    if (url.endsWith('/setup/status')) return Promise.resolve(json({ setup_required: false }));
    if (url.endsWith('/auth/session')) return Promise.resolve(reset ? json({ code: 'unauthenticated', detail: 'Sign in.' }, 401) : json(account));
    throw new Error(`Unexpected request: ${url}`);
  });
  vi.stubGlobal('fetch', fetch);
  await render(<App />);
  expect(container.textContent).toContain('No workspace access.');
  await act(async () => { navigate('/recover'); });
  await fillReset(); await submit(); await submit();
  const writes = fetch.mock.calls.filter(([url]) => url.endsWith('/auth/recovery/complete'));
  expect(writes).toHaveLength(1);
  const [url, options] = writes[0];
  expect(url).toBe('/api/v1/auth/recovery/complete');
  expect(options.method).toBe('POST');
  expect(options.credentials).toBe('same-origin');
  expect(new Headers(options.headers).has('X-CSRF-Token')).toBe(false);
  expect(JSON.parse(options.body as string)).toEqual({ token, password });
  expect(container.querySelector('fieldset')?.disabled).toBe(true);
  reset = true;
  await act(async () => finish(new Response(null, { status: 204 })));
  expect(container.textContent).toContain('Password reset.');
  expect(container.textContent).toContain('This browser is signed out.');
  expect(container.querySelector('input')).toBeNull();
  expect(document.activeElement).toBe(container.querySelector('h1'));
  expect(fetch.mock.calls.some(([url]) => url.endsWith('/auth/login'))).toBe(false);
  await clickLink('Sign in');
  expect(container.textContent).toContain('Sign in to OpenTrack.');
  expect(container.textContent).not.toContain('No workspace access.');
  expect(container.querySelector<HTMLInputElement>('input[name=password]')?.value).toBe('');
  expect(location.pathname).toBe('/signin');
});

it.each([false, true])('requires password verification after an uncertain reset despite an existing session (workspace=%s)', async hasWorkspace => {
  history.replaceState({ otIndex: 0 }, '', '/');
  let loginAttempts = 0;
  const recovered: Session = {
    audience: 'web',
    user: { id: 'recovered-account', email: 'recovered@example.test', display_name: 'Recovered user' },
    csrf_token: 'fresh-verified-csrf',
    workspaces: hasWorkspace ? [{ id: 'recovered-workspace', name: 'Recovered workspace', slug: 'recovered', role: 'owner' }] : [],
  };
  const fetch = vi.fn((url: string, _options: RequestInit) => {
    if (url.endsWith('/setup/status')) return Promise.resolve(json({ setup_required: false }));
    // This cookie remains valid: session restoration must not count as password verification.
    if (url.endsWith('/auth/session')) return Promise.resolve(json(account));
    if (url.endsWith('/auth/recovery/complete')) return Promise.reject(new TypeError('Failed to fetch'));
    if (url.endsWith('/auth/login')) return Promise.resolve(++loginAttempts === 1
      ? json({ code: 'unauthenticated', detail: 'The password was not accepted.' }, 401)
      : json(recovered));
    if (url.endsWith('/bootstrap')) return Promise.resolve(json({ auth_epoch: '1', teams: [], statuses: [], members: [], issues: [] }));
    if (url.includes('/issues?')) return Promise.resolve(json({ data: [], next_cursor: null, has_more: false }));
    if (url.endsWith('/access')) return Promise.resolve(json({ auth_epoch: '1', role: 'owner', archived: false }));
    throw new Error(`Unexpected request: ${url}`);
  });
  vi.stubGlobal('fetch', fetch);
  await render(<App />);
  expect(container.textContent).toContain('No workspace access.');
  await act(async () => { navigate('/recover'); });
  expect([...container.querySelectorAll('a')].every(link => link.getAttribute('href') === '/signin')).toBe(true);
  await fillReset(); await submit();
  await clickLink('Try signing in');
  expect(location.pathname).toBe('/signin');
  expect(parseRoute(location.pathname).valid).toBe(true);
  expect(container.textContent).toContain('Sign in to OpenTrack.');
  expect(container.textContent).not.toContain('No workspace access.');
  expect(field('password').value).toBe('');
  await fill('email', recovered.user.email); await fill('password', password); await submit();
  expect(location.pathname).toBe('/signin');
  expect(container.textContent).toContain('The password was not accepted.');
  expect(container.textContent).toContain('Sign in to OpenTrack.');
  await submit();
  expect(container.textContent).not.toContain('Sign in to OpenTrack.');
  expect(location.pathname).toBe(hasWorkspace ? '/w/recovered-workspace/issues' : '/');
  expect(container.textContent).toContain(hasWorkspace ? 'Recovered workspace' : 'No workspace access.');
  const logins = fetch.mock.calls.filter(([url]) => url.endsWith('/auth/login'));
  expect(logins).toHaveLength(2);
  expect(JSON.parse(logins[1][1].body as string)).toEqual({ email: recovered.user.email, password });
  expect(fetch.mock.calls.filter(([url]) => url.endsWith('/setup/status'))).toHaveLength(1);
  expect(fetch.mock.calls.filter(([url]) => url.endsWith('/auth/session'))).toHaveLength(1);
});

it('keeps explicit sign-in available when automatic session startup has failed', async () => {
  history.replaceState({ otIndex: 0 }, '', '/');
  const fetch = vi.fn().mockRejectedValue(new TypeError('Failed to fetch'));
  vi.stubGlobal('fetch', fetch);
  await render(<App />);
  expect(container.textContent).toContain('Could not reach OpenTrack.');
  await act(async () => { navigate('/signin'); });
  expect(container.textContent).toContain('Sign in to OpenTrack.');
  expect(container.textContent).not.toContain('Could not reach OpenTrack.');
  expect(document.activeElement).toBe(field('email'));
  expect(fetch).toHaveBeenCalledTimes(1);
});

it('aborts pending automatic session restoration when explicit sign-in opens and ignores its late success', async () => {
  history.replaceState({ otIndex: 0 }, '', '/');
  let finish!: (response: Response) => void; let signal: AbortSignal | undefined;
  const fetch = vi.fn((url: string, options: RequestInit) => {
    if (url.endsWith('/setup/status')) return Promise.resolve(json({ setup_required: false }));
    if (url.endsWith('/auth/session')) return new Promise<Response>(resolve => { finish = resolve; signal = options.signal as AbortSignal; });
    throw new Error(`Unexpected request: ${url}`);
  });
  vi.stubGlobal('fetch', fetch);
  await render(<App />);
  await act(async () => { navigate('/signin'); });
  expect(signal?.aborted).toBe(true);
  await act(async () => finish(json(account)));
  expect(location.pathname).toBe('/signin');
  expect(container.textContent).toContain('Sign in to OpenTrack.');
  expect(container.textContent).not.toContain('No workspace access.');
});

it('requires matching passwords before sending and reports invalid recovery codes without account details', async () => {
  const onCompleted = vi.fn();
  const fetch = vi.fn().mockResolvedValue(json({ code: 'recovery_invalid', detail: 'An internal account-specific message.', request_id: 'recovery-request' }, 422));
  vi.stubGlobal('fetch', fetch);
  await render(<RecoveryScreen onCompleted={onCompleted} />);
  await fillReset(); await fill('password_confirmation', 'A different password'); await submit();
  expect(fetch).not.toHaveBeenCalled();
  expect(container.textContent).toContain('The passwords do not match.');
  expect(document.activeElement).toBe(field('password_confirmation'));
  await fill('password_confirmation', password); await submit();
  expect(fetch).toHaveBeenCalledTimes(1);
  expect(container.textContent).toContain('This recovery code is invalid, expired, or no longer available.');
  expect(container.textContent).not.toContain('An internal account-specific message.');
  expect(container.textContent).toContain('Request recovery-request');
  expect(document.activeElement).toBe(field('recovery_token'));
  expect(field('password').value).toBe(password);
  expect(onCompleted).not.toHaveBeenCalled();
});

it.each(['network', 'timeout', 'unavailable', 'unexpected-response'])('keeps a %s result uncertain and retries the exact body without claiming a used-code error means success', async failure => {
  const onCompleted = vi.fn();
  const fetch = vi.fn()
    .mockImplementationOnce(() => failure === 'network' ? Promise.reject(new TypeError('Failed to fetch')) : Promise.resolve(failure === 'unexpected-response' ? json({ status: 'unknown' }) : json({ code: 'unavailable', detail: 'Response unavailable.' }, failure === 'timeout' ? 408 : 503)))
    .mockResolvedValueOnce(json({ code: 'recovery_invalid', detail: 'Invalid code.' }, 422));
  vi.stubGlobal('fetch', fetch);
  await render(<RecoveryScreen onCompleted={onCompleted} />);
  await fillReset(); await submit();
  expect(container.textContent).toContain('The result is uncertain.');
  expect(container.textContent).toContain('Try signing in with the new password first.');
  expect(field('password').readOnly).toBe(true);
  expect(field('recovery_token').readOnly).toBe(true);
  expect(fetch).toHaveBeenCalledTimes(1);
  await submit();
  expect(fetch).toHaveBeenCalledTimes(2);
  expect(fetch.mock.calls[1][1].body).toBe(fetch.mock.calls[0][1].body);
  expect(container.textContent).toContain('The result is uncertain.');
  expect(container.textContent).not.toContain('Password reset.');
  expect(onCompleted).not.toHaveBeenCalled();
  await act(async () => button('Use a different code').click());
  expect(field('recovery_token').value).toBe('');
  expect(field('password').value).toBe('');
  expect(field('password_confirmation').value).toBe('');
  expect(field('password').readOnly).toBe(false);
  expect(document.activeElement).toBe(field('recovery_token'));
});

it('leaves rate-limited input editable and only retries when the person submits again', async () => {
  const onCompleted = vi.fn();
  const fetch = vi.fn().mockResolvedValueOnce(json({ code: 'rate_limited', detail: 'Too many attempts. Try again later.' }, 429)).mockResolvedValueOnce(new Response(null, { status: 204 }));
  vi.stubGlobal('fetch', fetch);
  await render(<RecoveryScreen onCompleted={onCompleted} />);
  await fillReset(); await submit();
  expect(container.textContent).toContain('Too many attempts.');
  expect(container.textContent).not.toContain('The result is uncertain.');
  expect(field('password').readOnly).toBe(false);
  expect(fetch).toHaveBeenCalledTimes(1);
  await submit();
  expect(onCompleted).toHaveBeenCalledTimes(1);
});

it('aborts and ignores late reset completion after the recovery view is closed', async () => {
  let finish!: (response: Response) => void; let signal: AbortSignal | undefined;
  const onCompleted = vi.fn();
  vi.stubGlobal('fetch', vi.fn((_url: string, options: RequestInit) => new Promise<Response>(resolve => { finish = resolve; signal = options.signal as AbortSignal; })));
  await render(<RecoveryScreen onCompleted={onCompleted} />);
  await fillReset(); await submit();
  await render(<main>Another account view</main>);
  expect(signal?.aborted).toBe(true);
  await act(async () => finish(new Response(null, { status: 204 })));
  expect(onCompleted).not.toHaveBeenCalled();
  expect(container.textContent).toBe('Another account view');
});

it('aborts an old startup session load when entering recovery and ignores its late success', async () => {
  history.replaceState({ otIndex: 0 }, '', '/');
  let finishSession!: (response: Response) => void; let signal: AbortSignal | undefined;
  const fetch = vi.fn((url: string, options: RequestInit) => {
    if (url.endsWith('/setup/status')) return Promise.resolve(json({ setup_required: false }));
    if (url.endsWith('/auth/session')) return new Promise<Response>(resolve => { finishSession = resolve; signal = options.signal as AbortSignal; });
    if (url.endsWith('/auth/recovery/complete')) return Promise.resolve(new Response(null, { status: 204 }));
    throw new Error(`Unexpected request: ${url}`);
  });
  vi.stubGlobal('fetch', fetch);
  await render(<App />);
  await act(async () => { navigate('/recover'); });
  expect(signal?.aborted).toBe(true);
  await fillReset(); await submit();
  await act(async () => finishSession(json(account)));
  expect(container.textContent).toContain('Password reset.');
  expect(container.textContent).not.toContain('No workspace access.');
  expect(container.querySelector('input')).toBeNull();
});

it.each(['/', '/signin'])('aborts an in-flight login from %s when the recovery link is opened', async path => {
  history.replaceState({ otIndex: 0 }, '', path);
  let finishLogin!: (response: Response) => void; let signal: AbortSignal | undefined;
  const fetch = vi.fn((url: string, options: RequestInit) => {
    if (url.endsWith('/setup/status')) return Promise.resolve(json({ setup_required: false }));
    if (url.endsWith('/auth/session')) return Promise.resolve(json({ code: 'unauthenticated', detail: 'Sign in.' }, 401));
    if (url.endsWith('/auth/login')) return new Promise<Response>(resolve => { finishLogin = resolve; signal = options.signal as AbortSignal; });
    throw new Error(`Unexpected request: ${url}`);
  });
  vi.stubGlobal('fetch', fetch);
  await render(<App />);
  if (path === '/signin') expect(fetch).not.toHaveBeenCalled();
  await fill('email', account.user.email); await fill('password', password); await submit();
  await clickLink('Reset your password');
  expect(signal?.aborted).toBe(true);
  await act(async () => finishLogin(json(account)));
  expect(container.textContent).toContain('Reset your password.');
  expect(container.textContent).not.toContain('No workspace access.');
});
