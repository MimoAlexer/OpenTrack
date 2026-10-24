// @vitest-environment jsdom
import { act, createElement, useState } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { App } from './App';
import { hasUnsavedChanges, useUnsavedChanges } from './router';
import type { Bootstrap, Session } from './types';
import type { WorkspaceDetail } from './settings/types';

vi.mock('./issues/IssueList', () => ({ IssueList: () => createElement('div', null, 'Issue list') }));
vi.mock('./issues/IssueEditor', () => ({ IssueEditor: () => {
  const [title, setTitle] = useState('Saved title');
  useUnsavedChanges(title !== 'Saved title');
  return createElement('input', { 'aria-label': 'Issue title', value: title, onChange: (event: { target: HTMLInputElement }) => setTitle(event.target.value) });
} }));
vi.mock('./useWorkspaceAccess', () => ({ useWorkspaceAccess: () => {} }));

const session: Session = { audience: 'web', user: { id: 'owner', display_name: 'Example Owner', email: 'owner@example.test' }, csrf_token: 'synthetic-csrf', workspaces: [{ id: 'existing', name: 'Existing workspace', slug: 'existing', role: 'owner' }] };
const workspace: WorkspaceDetail = { id: 'created-workspace', organization_id: 'organization', name: 'New workspace', slug: 'created-workspace', role: 'owner', version: '1', archived_at: null };
const bootstrap: Bootstrap = { auth_epoch: '1', teams: [{ id: 'team', name: 'Example team', key: 'ENG', private: false, archived_at: null }], statuses: [], members: [], issues: [] };
const json = (value: unknown, status = 200) => new Response(JSON.stringify(value), { status });
const dialogMethods = ['showModal', 'close'].map(name => [name, Object.getOwnPropertyDescriptor(HTMLDialogElement.prototype, name)] as const);
let container: HTMLDivElement; let root: Root;
let previous: { url: string; state: unknown };
let requests: { url: string; method: string }[];
let created: boolean;

beforeEach(() => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true);
  vi.spyOn(window, 'confirm').mockReturnValue(false);
  HTMLDialogElement.prototype.showModal = function () { this.setAttribute('open', ''); };
  HTMLDialogElement.prototype.close = function () { this.removeAttribute('open'); };
  previous = { url: `${location.pathname}${location.search}`, state: history.state };
  localStorage.clear(); history.replaceState({ otIndex: 0 }, '', '/w/existing/issues/issue');
  requests = []; created = false;
  vi.stubGlobal('fetch', vi.fn((url: string, options: RequestInit = {}) => {
    requests.push({ url, method: options.method ?? 'GET' });
    if (url.endsWith('/setup/status')) return Promise.resolve(json({ setup_required: false }));
    if (url.endsWith('/auth/session')) return Promise.resolve(json(created ? { ...session, workspaces: [...session.workspaces, workspace] } : session));
    if (url.endsWith('/bootstrap')) return Promise.resolve(json(bootstrap));
    if (url.endsWith('/access')) return Promise.resolve(json({ auth_epoch: '1', role: 'owner', archived: false }));
    if (url.endsWith('/organizations')) return Promise.resolve(json([{ id: 'organization', name: 'Example organization', role: 'owner' }]));
    if (url.endsWith('/workspaces') && options.method === 'POST') { created = true; return Promise.resolve(json(workspace, 201)); }
    throw new Error('Unexpected request in workspace handoff test.');
  }));
  container = document.createElement('div'); document.body.append(container); root = createRoot(container);
});

afterEach(async () => {
  await act(async () => root.unmount()); container.remove(); localStorage.clear();
  history.replaceState(previous.state, '', previous.url);
  for (const [name, descriptor] of dialogMethods) {
    if (descriptor) Object.defineProperty(HTMLDialogElement.prototype, name, descriptor);
    else Reflect.deleteProperty(HTMLDialogElement.prototype, name);
  }
  vi.restoreAllMocks(); vi.unstubAllGlobals();
});

async function setInput(field: HTMLInputElement, value: string) {
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(field, value);
    field.dispatchEvent(new Event('input', { bubbles: true }));
  });
}

it('keeps a dirty issue mounted when opening a newly created workspace is declined', async () => {
  await act(async () => root.render(createElement(App)));
  const originalTitle = container.querySelector<HTMLInputElement>('[aria-label="Issue title"]')!;
  expect(originalTitle).toBeTruthy();
  await setInput(originalTitle, 'Unsubmitted issue draft');
  expect(hasUnsavedChanges()).toBe(true);
  const menu = container.querySelector<HTMLDetailsElement>('.workspace-menu')!;
  await act(async () => menu.querySelector('summary')!.click());
  const create = [...menu.querySelectorAll<HTMLButtonElement>('button')].find(button => button.textContent?.trim() === 'Create workspace')!;
  await act(async () => create.click());
  const form = container.querySelector<HTMLFormElement>('dialog form')!;
  expect(form).toBeTruthy();
  for (const [name, value] of Object.entries({ name: 'New workspace', team_name: 'Example team', team_key: 'NEW' })) await setInput(form.elements.namedItem(name) as HTMLInputElement, value);
  const submit = [...form.querySelectorAll<HTMLButtonElement>('button')].find(button => button.type === 'submit')!;
  await act(async () => submit.click());
  const currentTitle = container.querySelector<HTMLInputElement>('[aria-label="Issue title"]');
  expect({
    creationPosts: requests.filter(item => item.url.endsWith('/workspaces') && item.method === 'POST').length,
    sessionReads: requests.filter(item => item.url.endsWith('/auth/session')).length,
    oldWorkspaceBootstrapReads: requests.filter(item => item.url.endsWith('/workspaces/existing/bootstrap')).length,
    confirmationCalls: vi.mocked(window.confirm).mock.calls.length,
    path: location.pathname,
    sameEditor: currentTitle === originalTitle,
    draft: currentTitle?.value,
    guarded: hasUnsavedChanges(),
  }).toEqual({
    creationPosts: 1, sessionReads: 2, oldWorkspaceBootstrapReads: 1, confirmationCalls: 1,
    path: '/w/existing/issues/issue', sameEditor: true, draft: 'Unsubmitted issue draft', guarded: true,
  });
});
