// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { AuthScreen } from './AuthScreen';
import type { Session } from '../types';

let container: HTMLDivElement; let root: Root;
beforeEach(() => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true);
  container = document.createElement('div'); document.body.append(container); root = createRoot(container);
});
afterEach(async () => { await act(async () => root.unmount()); container.remove(); vi.restoreAllMocks(); vi.unstubAllGlobals(); });

it.each(['display_name', 'workspace_name', 'team_name'] as const)('retains a rejected supplementary setup %s and repairs with one owner callback', async field => {
  const names = { display_name: '😀'.repeat(100), workspace_name: '🧭'.repeat(100), team_name: '🚀'.repeat(100) }; const requests: typeof names[] = []; const authenticated = vi.fn();
  const session: Session = { audience: 'web', user: { id: 'owner', email: 'owner@example.test', display_name: names.display_name }, csrf_token: 'synthetic-csrf', workspaces: [{ id: 'workspace', name: names.workspace_name, slug: 'workspace', role: 'owner' }] };
  vi.stubGlobal('fetch', vi.fn((_url: string, options: RequestInit) => { const body = JSON.parse(options.body as string); requests.push({ display_name: body.display_name, workspace_name: body.workspace_name, team_name: body.team_name }); return Promise.resolve(new Response(JSON.stringify(requests.length === 1 ? { code: 'invalid', detail: 'Setup name exceeds its scalar limit' } : session), { status: requests.length === 1 ? 422 : 201 })); }));
  await act(async () => root.render(<AuthScreen setup onAuthenticated={authenticated}/>));
  for (const [key, value] of Object.entries({ ...names, setup_token: 'synthetic-installation-token', email: 'owner@example.test', password: 'synthetic-setup-password', team_key: 'SET' })) container.querySelector<HTMLInputElement>(`input[name="${key}"]`)!.value = value;
  const input = container.querySelector<HTMLInputElement>(`input[name="${field}"]`)!; input.value = names[field] + '🌍'; await act(async () => container.querySelector('form')!.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true })));
  expect(requests).toEqual([{ ...names, [field]: names[field] + '🌍' }]); expect(authenticated).not.toHaveBeenCalled(); for (const key of Object.keys(names) as (keyof typeof names)[]) expect(container.querySelector<HTMLInputElement>(`input[name="${key}"]`)!.value).toBe(key === field ? names[key] + '🌍' : names[key]);
  input.value = names[field]; await act(async () => container.querySelector('form')!.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }))); expect(requests[1]).toEqual(names); expect(requests.length).toBe(2); expect(authenticated).toHaveBeenCalledTimes(1); expect((authenticated.mock.calls[0][0] as Session).user.display_name).toBe(names.display_name);
});

it.each([401, 503])('preserves password-manager values and focuses actionable feedback after HTTP %s', async status => {
  vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response(JSON.stringify({ code: status === 401 ? 'unauthenticated' : 'unavailable', detail: 'Could not sign in.' }), { status })));
  const authenticated = vi.fn();
  await act(async () => root.render(<AuthScreen setup={false} onAuthenticated={authenticated} />));
  const email = container.querySelector<HTMLInputElement>('input[name=email]')!;
  const password = container.querySelector<HTMLInputElement>('input[name=password]')!;
  // Password managers may set native values without a React input event.
  email.value = 'member@example.test'; password.value = 'synthetic rejected password';
  await act(async () => container.querySelector('form')!.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true })));
  expect(authenticated).not.toHaveBeenCalled();
  expect(email.value).toBe('member@example.test'); expect(password.value).toBe('synthetic rejected password');
  expect(email.autocomplete).toBe('username'); expect(password.autocomplete).toBe('current-password');
  if (status === 401) {
    expect(document.activeElement).toBe(password);
    expect(email.getAttribute('aria-invalid')).toBe('true'); expect(password.getAttribute('aria-invalid')).toBe('true');
    expect(document.getElementById(password.getAttribute('aria-describedby')!)?.textContent).toContain('Email or password was not accepted.');
  } else {
    expect(document.activeElement?.textContent).toContain('Could not sign in.');
    expect(password.hasAttribute('aria-invalid')).toBe(false);
  }
});
