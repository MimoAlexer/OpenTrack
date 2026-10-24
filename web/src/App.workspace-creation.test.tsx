// @vitest-environment jsdom
import { act, createElement } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { App } from './App';
import type { Session } from './types';

const accountA: Session = { audience: 'web', user: { id: 'account-a', display_name: 'Example Owner', email: 'owner@example.test' }, csrf_token: 'synthetic-csrf-a', workspaces: [] };
const accountB: Session = { audience: 'web', user: { id: 'account-b', display_name: 'Example Member', email: 'member@example.test' }, csrf_token: 'synthetic-csrf-b', workspaces: [] };
const workspace = { id: 'created-workspace', organization_id: 'organization-a', name: 'Account A workspace draft', slug: 'created-workspace', role: 'owner', version: '1', archived_at: null };
const json = (value: unknown, status = 200) => new Response(JSON.stringify(value), { status });
const dialogMethods = ['showModal', 'close'].map(name => [name, Object.getOwnPropertyDescriptor(HTMLDialogElement.prototype, name)] as const);
let container: HTMLDivElement; let root: Root; let previous: { url: string; state: unknown };

beforeEach(() => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true);
  HTMLDialogElement.prototype.showModal = function () { this.setAttribute('open', ''); };
  HTMLDialogElement.prototype.close = function () { this.removeAttribute('open'); };
  previous = { url: `${location.pathname}${location.search}`, state: history.state };
  history.replaceState({ otIndex: 0 }, '', '/');
  localStorage.clear();
  container = document.createElement('div'); document.body.append(container); root = createRoot(container);
});
afterEach(async () => {
  await act(async () => root.unmount()); container.remove();
  history.replaceState(previous.state, '', previous.url); localStorage.clear();
  for (const [name, descriptor] of dialogMethods) {
    if (descriptor) Object.defineProperty(HTMLDialogElement.prototype, name, descriptor);
    else Reflect.deleteProperty(HTMLDialogElement.prototype, name);
  }
  vi.restoreAllMocks(); vi.unstubAllGlobals();
});

async function openDraft(workspaceAccess = false) {
  if (workspaceAccess) history.replaceState({ otIndex: 0 }, '', '/w/existing-workspace/issues');
  await act(async () => root.render(createElement(App)));
  expect(container.querySelector('h1')?.textContent).toBe(workspaceAccess ? 'All issues' : 'No workspace access.');
  if (workspaceAccess) await act(async () => container.querySelector<HTMLElement>('.workspace-menu summary')!.click());
  const create = [...container.querySelectorAll<HTMLButtonElement>('button')].find(button => button.textContent === 'Create workspace')!;
  await act(async () => create.click());
  const form = container.querySelector<HTMLFormElement>('dialog form')!;
  expect(form).toBeTruthy();
  for (const [name, value] of Object.entries({ name: workspace.name, team_name: 'Account A team draft', team_key: 'DRAFT' })) {
    const field = form.elements.namedItem(name) as HTMLInputElement;
    await act(async () => { Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(field, value); field.dispatchEvent(new Event('input', { bubbles: true })); });
  }
  return form;
}
async function submit(form: HTMLFormElement) {
  await act(async () => form.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true })));
}

it.each([false, true])('does not continue the creator destination after its real session refresh installs another account, existing workspace: %s', async workspaceAccess => {
  let sessionReads = 0; const posts: unknown[] = [];
  vi.stubGlobal('fetch', vi.fn((url: string, options: RequestInit = {}) => {
    if (url.endsWith('/setup/status')) return Promise.resolve(json({ setup_required: false }));
    if (url.endsWith('/auth/session')) return Promise.resolve(json(++sessionReads === 1 ? { ...accountA, workspaces: workspaceAccess ? [{ id: 'existing-workspace', name: 'Existing workspace', slug: 'existing-workspace', role: 'owner' }] : [] } : accountB));
    if (url.endsWith('/bootstrap')) return Promise.resolve(json({ auth_epoch: '1', teams: [], statuses: [], members: [], issues: [] }));
    if (url.endsWith('/access')) return Promise.resolve(json({ auth_epoch: '1', role: 'owner', archived: false }));
    if (url.endsWith('/preferences')) return Promise.resolve(json({ workspace_id: 'existing-workspace', shortcut_mode: 'disabled', etag: 'synthetic-preferences' }));
    if (url.endsWith('/organizations')) return Promise.resolve(json([{ id: 'organization-a', name: 'Account A organization', role: 'owner' }]));
    if (url.endsWith('/workspaces') && options.method === 'POST') { posts.push(JSON.parse(options.body as string)); return Promise.resolve(json(workspace, 201)); }
    throw new Error(`Unexpected request: ${url}`);
  }));
  const form = await openDraft(workspaceAccess);
  await submit(form);
  expect(sessionReads).toBe(2); expect(posts).toHaveLength(1);
  expect(location.pathname, 'A creation callback must not navigate for the prior account after session refresh installs another account.').toBe(workspaceAccess ? '/w/existing-workspace/issues' : '/');
  expect(container.querySelector('h1')?.textContent).toBe('No workspace access.');
  expect({ creatorOpen: !!container.querySelector('dialog form'), draft: container.querySelector<HTMLInputElement>('input[name=name]')?.value ?? null, priorOrganization: container.textContent?.includes('Account A organization') },
    'The App authenticated callback must discard zero-workspace account A creator state when it installs account B.')
    .toEqual({ creatorOpen: false, draft: null, priorOrganization: false });
});

it('keeps the zero-workspace creator draft through real same-account password confirmation', async () => {
  const rotated: Session = { ...accountA, csrf_token: 'synthetic-csrf-a-rotated' };
  const logins: unknown[] = [];
  vi.stubGlobal('fetch', vi.fn((url: string, options: RequestInit = {}) => {
    if (url.endsWith('/setup/status')) return Promise.resolve(json({ setup_required: false }));
    if (url.endsWith('/auth/session')) return Promise.resolve(json(accountA));
    if (url.endsWith('/organizations')) return Promise.resolve(json([{ id: 'organization-a', name: 'Account A organization', role: 'owner' }]));
    if (url.endsWith('/workspaces') && options.method === 'POST') return Promise.resolve(json({ code: 'reauthentication_required', detail: 'Confirm your password before this action.' }, 403));
    if (url.endsWith('/auth/login') && options.method === 'POST') { logins.push(JSON.parse(options.body as string)); return Promise.resolve(json(rotated)); }
    throw new Error(`Unexpected request: ${url}`);
  }));
  const form = await openDraft();
  const name = form.elements.namedItem('name') as HTMLInputElement;
  await submit(form);
  const confirm = [...container.querySelectorAll<HTMLButtonElement>('button')].find(button => button.textContent === 'Confirm your password')!;
  expect(confirm).toBeTruthy(); await act(async () => confirm.click());
  const confirmation = container.querySelector<HTMLFormElement>('input[name=password]')!.closest('form')!;
  const password = confirmation.elements.namedItem('password') as HTMLInputElement;
  password.value = 'synthetic account password';
  await submit(confirmation);
  expect(logins).toEqual([{ email: 'owner@example.test', password: 'synthetic account password' }]);
  expect(container.querySelector('input[name=password]')).toBeNull();
  expect(container.querySelector('dialog form')).toBe(form);
  expect(container.querySelector('input[name=name]')).toBe(name);
  expect(name.value).toBe(workspace.name);
  expect((form.elements.namedItem('team_name') as HTMLInputElement).value).toBe('Account A team draft');
  expect(container.textContent).toContain('Password confirmed.');
});
