// @vitest-environment jsdom
import { act, useState } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { App } from './App';
import { hasUnsavedChanges, useUnsavedChanges } from './router';
import type { Bootstrap, Session } from './types';

vi.mock('./issues/IssueList', () => ({ IssueList: () => <div>Issue list</div> }));
vi.mock('./issues/IssueEditor', () => ({ IssueEditor: () => {
  const [title, setTitle] = useState('Saved title');
  useUnsavedChanges(title !== 'Saved title');
  return <input aria-label="Issue title" value={title} onChange={event => setTitle(event.target.value)} />;
} }));
vi.mock('./useWorkspaceAccess', () => ({ useWorkspaceAccess: () => {} }));

const session: Session = { audience: 'web', user: { id: 'u', display_name: 'Example Owner', email: 'owner@example.test' }, csrf_token: 'csrf', workspaces: [{ id: 'w', name: 'Workspace', slug: 'workspace', role: 'owner' }] };
const bootstrap: Bootstrap = { auth_epoch: '1', teams: [{ id: 't', name: 'Team', key: 'ENG', private: false, archived_at: null }], statuses: [], members: [], issues: [] };
const json = (value: unknown) => new Response(JSON.stringify(value), { status: 200 });
let container: HTMLDivElement; let root: Root;
beforeEach(() => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true);
  vi.spyOn(window, 'confirm').mockReturnValue(false);
  localStorage.clear(); history.replaceState({ otIndex: 0 }, '', '/w/w/issues/i');
  vi.stubGlobal('fetch', vi.fn((url: string) => {
    if (url.endsWith('/setup/status')) return Promise.resolve(json({ setup_required: false }));
    if (url.endsWith('/auth/session')) return Promise.resolve(json(session));
    if (url.endsWith('/bootstrap')) return Promise.resolve(json(bootstrap));
    if (url.endsWith('/access')) return Promise.resolve(json({ auth_epoch: '1', role: 'owner', archived: false }));
    throw new Error(`Unexpected request: ${url}`);
  }));
  container = document.createElement('div'); document.body.append(container); root = createRoot(container);
});
afterEach(async () => { await act(async () => root.unmount()); container.remove(); localStorage.clear(); vi.restoreAllMocks(); vi.unstubAllGlobals(); });

it.each([false, true])('Escape closes only the account popover while an issue has unsaved changes: %s', async dirty => {
  await act(async () => root.render(<App />));
  const title = container.querySelector<HTMLInputElement>('[aria-label="Issue title"]')!;
  expect(title).toBeTruthy();
  if (dirty) await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(title, 'Local draft');
    title.dispatchEvent(new Event('input', { bubbles: true }));
  });
  const menu = container.querySelector<HTMLDetailsElement>('.account-menu')!;
  const summary = menu.querySelector('summary')!;
  await act(async () => summary.click()); expect(menu.open).toBe(true);
  const appearance = [...menu.querySelectorAll<HTMLButtonElement>('.appearance-options button')];
  await act(async () => { appearance[1].focus(); appearance[1].click(); });
  expect(document.documentElement.dataset.theme).toBe('light');
  await act(async () => { appearance[0].focus(); appearance[0].click(); });
  expect(document.documentElement.dataset.theme).toBe('system');
  const escape = new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true });
  await act(async () => appearance[0].dispatchEvent(escape));
  expect(escape.defaultPrevented).toBe(true); expect(menu.open).toBe(false);
  expect(appearance.map(button => button.getAttribute('aria-label'))).toEqual(['Use system appearance', 'Use light appearance', 'Use dark appearance']);
  expect(document.activeElement).toBe(summary); expect(location.pathname).toBe('/w/w/issues/i');
  expect(container.querySelector('[aria-label="Issue title"]')).toBe(title);
  expect(title.value).toBe(dirty ? 'Local draft' : 'Saved title');
  expect(hasUnsavedChanges()).toBe(dirty); expect(window.confirm).not.toHaveBeenCalled();
  // Once the popover is closed, the surrounding issue shortcut still works.
  await act(async () => summary.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true })));
  if (dirty) { expect(window.confirm).toHaveBeenCalledOnce(); expect(location.pathname).toBe('/w/w/issues/i'); expect(title.value).toBe('Local draft'); }
  else { expect(location.pathname).toBe('/w/w/issues'); expect(container.querySelector('[aria-label="Issue title"]')).toBeNull(); }
});


it('keeps a canceled workspace switch operable and focuses the new scope after a successful switch', async () => {
  session.workspaces.push({ id: 'other', name: 'Other workspace', slug: 'other', role: 'owner' });
  try {
    await act(async () => root.render(<App />));
    const title = container.querySelector<HTMLInputElement>('[aria-label="Issue title"]')!;
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(title, 'Local draft');
      title.dispatchEvent(new Event('input', { bubbles: true }));
    });
    const menu = container.querySelector<HTMLDetailsElement>('.workspace-menu')!;
    await act(async () => menu.querySelector('summary')!.click());
    const select = menu.querySelector('select')!;
    async function choose() { await act(async () => { select.focus(); select.value = 'other'; select.dispatchEvent(new Event('change', { bubbles: true })); }); }
    await choose();
    expect(menu.open).toBe(true); expect(document.activeElement).toBe(select);
    expect(select.value).toBe('w'); expect(location.pathname).toBe('/w/w/issues/i');
    expect(title.value).toBe('Local draft');
    vi.mocked(window.confirm).mockReturnValue(true);
    await choose();
    expect(location.pathname).toBe('/w/other/issues');
    expect(container.querySelector('[aria-label="Issue title"]')).toBeNull();
    expect(document.activeElement).toBe(container.querySelector('h1'));
    expect(container.querySelector('.workspace-identity strong')?.textContent).toBe('Other workspace');
  } finally { session.workspaces.pop(); }
});


it('clears the prior account draft when another account signs in to the same workspace', async () => {
  const nextSession: Session = { ...session, user: { id: 'other-user', display_name: 'Example Member', email: 'member@example.test' }, csrf_token: 'next-csrf' };
  const previous = vi.mocked(fetch).getMockImplementation()!;
  vi.stubGlobal('fetch', vi.fn((url: string, options: RequestInit) => url.endsWith('/auth/login') ? Promise.resolve(json(nextSession)) : previous(url, options)));
  await act(async () => root.render(<App />));
  const prior = container.querySelector<HTMLInputElement>('[aria-label="Issue title"]')!;
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(prior, 'Prior account private draft');
    prior.dispatchEvent(new Event('input', { bubbles: true }));
  });
  await act(async () => window.dispatchEvent(new CustomEvent('opentrack:session-expired')));
  expect(container.querySelector('[aria-label="Issue title"]')).toBeNull();
  expect(container.textContent).toContain('Your session has ended.');
  const email = container.querySelector<HTMLInputElement>('input[name=email]')!;
  const password = container.querySelector<HTMLInputElement>('input[name=password]')!;
  email.value = 'member@example.test'; password.value = 'synthetic account password';
  await act(async () => container.querySelector('form')!.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true })));
  const current = container.querySelector<HTMLInputElement>('[aria-label="Issue title"]')!;
  expect(current).not.toBe(prior); expect(current.value).toBe('Saved title');
  expect(container.querySelector('.account-menu summary')?.getAttribute('aria-label')).toBe('Account menu: Example Member');
  expect(container.textContent).not.toContain('owner@example.test');
});
