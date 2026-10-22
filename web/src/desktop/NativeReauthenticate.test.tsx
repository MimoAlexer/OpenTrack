// @vitest-environment jsdom
import { act, type ReactNode } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import type { Session } from '../types';
import { DesktopContext } from './context';
import { NativeReauthenticate } from './NativeReauthenticate';
import { NativeBridgeError } from './http';
import type { DesktopSnapshot, DesktopStore } from './store';

const session: Session = { user: { id: 'owner', email: 'owner@example.test', display_name: 'Owner' }, workspaces: [], csrf_token: '', audience: 'desktop' };
const snapshot: DesktopSnapshot = { connection: { generation: '1', scope_generation: '1', selected: { origin: 'https://work.example.test', development_http: false }, credential_present: true, pending: { request_id: 'request', confirmation_code: 'ABCD2345', expires_at: '2030-01-01T00:00:00Z' } }, session, issueLink: null, authEvent: null, loading: false, error: null };
let container: HTMLDivElement; let root: Root;
beforeEach(() => { vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true); HTMLDialogElement.prototype.showModal = function () { this.setAttribute('open', ''); }; HTMLDialogElement.prototype.close = function () { this.removeAttribute('open'); }; container = document.createElement('div'); document.body.append(container); root = createRoot(container); });
afterEach(async () => { await act(async () => root.unmount()); container.remove(); vi.restoreAllMocks(); vi.unstubAllGlobals(); });
async function render(node: ReactNode) { await act(async () => root.render(node)); }
async function click(text: string) { await act(async () => [...container.querySelectorAll('button')].find(button => button.textContent === text)!.click()); }

it('keeps the matching code and dialog open when native cancellation says authentication is finishing', async () => {
  const close = vi.fn(); const cancel = vi.fn().mockRejectedValue(new NativeBridgeError('authentication_finishing', 'Authentication is finishing. Wait for its result.', true));
  const store = { cancel } as unknown as DesktopStore;
  await render(<DesktopContext.Provider value={{ store, snapshot }}><NativeReauthenticate session={session} close={close} authenticated={vi.fn()} /></DesktopContext.Provider>);
  expect(container.querySelector('input[type=password]')).toBeNull();
  await click('Cancel');
  expect(close).not.toHaveBeenCalled(); expect(cancel).toHaveBeenCalledOnce();
  expect(container.querySelector('dialog[open]')).not.toBeNull();
  expect(container.textContent).toContain('ABCD2345'); expect(container.textContent).toContain('Authentication is finishing.');
});

it('completes only a new matching-account native authentication without restarting or replaying work', async () => {
  const close = vi.fn(); const authenticated = vi.fn(); const begin = vi.fn(); const store = { begin } as unknown as DesktopStore;
  const view = (value: DesktopSnapshot) => <DesktopContext.Provider value={{ store, snapshot: value }}><NativeReauthenticate session={session} close={close} authenticated={authenticated} /></DesktopContext.Provider>;
  await render(view(snapshot));
  const event = { generation: '2', scope_generation: '1', kind: 'authenticated' as const, request_id: 'request', session, error: null, remote_revocation_confirmed: null };
  await render(view({ ...snapshot, connection: { ...snapshot.connection!, generation: '2', pending: null }, authEvent: event }));
  expect(authenticated).toHaveBeenCalledExactlyOnceWith(session); expect(close).toHaveBeenCalledOnce(); expect(begin).not.toHaveBeenCalled();
  await render(view({ ...snapshot, authEvent: event }));
  expect(authenticated).toHaveBeenCalledOnce();
});
