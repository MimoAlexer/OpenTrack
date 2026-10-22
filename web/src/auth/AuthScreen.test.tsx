// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { AuthScreen } from './AuthScreen';

let container: HTMLDivElement; let root: Root;
beforeEach(() => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true);
  container = document.createElement('div'); document.body.append(container); root = createRoot(container);
});
afterEach(async () => { await act(async () => root.unmount()); container.remove(); vi.restoreAllMocks(); vi.unstubAllGlobals(); });

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
