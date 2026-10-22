// @vitest-environment jsdom
import { act, useState } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import type { NativeAuthConsent, Session } from '../types';
import { NativeConsent } from './NativeConsent';

const id = '11111111-1111-4111-8111-111111111111';
const session: Session = { user: { id: 'owner', email: 'owner@example.test', display_name: 'Owner' }, csrf_token: 'csrf-one', audience: 'web', workspaces: [] };
const metadata: NativeAuthConsent = { id, device_name: 'Office desktop', confirmation_code: 'ABCD2345', expires_at: '2099-01-01T00:00:00Z', reauthentication: true };
const json = (body: unknown, status = 200) => new Response(JSON.stringify(body), { status });
let container: HTMLDivElement; let root: Root;
beforeEach(() => { vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true); HTMLDialogElement.prototype.showModal = function () { this.setAttribute('open', ''); }; HTMLDialogElement.prototype.close = function () { this.removeAttribute('open'); }; history.replaceState({}, '', `/auth/native/approve?request_id=${id}`); container = document.createElement('div'); document.body.append(container); root = createRoot(container); });
afterEach(async () => { await act(async () => root.unmount()); container.remove(); vi.restoreAllMocks(); vi.unstubAllGlobals(); });
function button(text: string) { return [...container.querySelectorAll('button')].find(button => button.textContent === text)!; }
async function click(text: string) { await act(async () => button(text).click()); }
async function match() { await act(async () => container.querySelector<HTMLInputElement>('input[type=checkbox]')!.click()); }

it('requires an explicit code match and makes a lost approval response terminal rather than replaying it', async () => {
  const fetch = vi.fn((_url: string, options: RequestInit) => options.method === 'POST' ? Promise.reject(new TypeError('Disconnected')) : Promise.resolve(json(metadata)));
  vi.stubGlobal('fetch', fetch);
  await act(async () => root.render(<NativeConsent requestId={id} session={session} authenticated={vi.fn()} />));
  expect(container.textContent).toContain('Office desktop'); expect(container.textContent).toContain('ABCD2345');
  expect(button('Approve this device').disabled).toBe(true);
  await match(); await click('Approve this device');
  expect(container.textContent).toContain('Approval may have succeeded.');
  expect(button('Approve this device')).toBeUndefined();
  expect(fetch.mock.calls.filter(([, options]) => options.method === 'POST')).toHaveLength(1);
});

it('reauthenticates in the same request, refreshes metadata and requires a new explicit code match', async () => {
  let approved = 0;
  const fetch = vi.fn((url: string, options: RequestInit) => {
    if (url.endsWith('/auth/login')) return Promise.resolve(json({ ...session, csrf_token: 'csrf-two' }));
    if (options.method === 'POST') { approved += 1; return Promise.resolve(json({ code: 'reauthentication_required', detail: 'Confirm your identity.' }, 403)); }
    return Promise.resolve(json(metadata));
  });
  vi.stubGlobal('fetch', fetch);
  function Review() { const [current, authenticated] = useState(session); return <NativeConsent requestId={id} session={current} authenticated={authenticated} />; }
  await act(async () => root.render(<Review />)); await match(); await click('Approve this device');
  expect(container.querySelector('dialog[open]')).not.toBeNull();
  await act(async () => { const input = container.querySelector<HTMLInputElement>('input[type=password]')!; input.value = 'correct password'; container.querySelector('form')!.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true })); });
  expect(approved).toBe(1); expect(location.pathname).toBe('/auth/native/approve');
  expect(location.search).toBe(`?request_id=${id}`);
  expect(container.querySelector<HTMLInputElement>('input[type=checkbox]')!.checked).toBe(false);
  expect(button('Approve this device').disabled).toBe(true);
  expect(fetch.mock.calls.filter(([url, options]) => url.endsWith(`/requests/${id}`) && !options.method)).toHaveLength(2);
});

it('does not load or approve a malformed request identifier', async () => {
  const fetch = vi.fn(); vi.stubGlobal('fetch', fetch);
  await act(async () => root.render(<NativeConsent requestId={null} session={session} authenticated={vi.fn()} />));
  expect(container.textContent).toContain('Desktop request unavailable.'); expect(fetch).not.toHaveBeenCalled();
});
