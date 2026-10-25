// @vitest-environment jsdom
import { act, createElement } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import type { Session } from '../types';
import type { WorkspaceDetail } from './types';
import { CreateWorkspace } from './WorkspaceSettings';

const session: Session = { audience: 'web', user: { id: 'owner', display_name: 'Example Owner', email: 'owner@example.test' }, csrf_token: 'original-csrf', workspaces: [] };
const workspace: WorkspaceDetail = { id: 'created-workspace', organization_id: 'organization', name: 'New workspace', slug: 'created-workspace', role: 'owner', version: '1', archived_at: null };
const fields = { name: workspace.name, team_name: 'Example team', team_key: 'NEW' };
const json = (value: unknown, status = 200) => new Response(JSON.stringify(value), { status });
const dialogMethods = ['showModal', 'close'].map(name => [name, Object.getOwnPropertyDescriptor(HTMLDialogElement.prototype, name)] as const);
let container: HTMLDivElement; let root: Root;
let previous: { url: string; state: unknown };
let completions: (() => void)[];
let pending: Promise<unknown>[];
beforeEach(() => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true);
  HTMLDialogElement.prototype.showModal = function () { this.setAttribute('open', ''); };
  HTMLDialogElement.prototype.close = function () { this.removeAttribute('open'); };
  previous = { url: `${location.pathname}${location.search}`, state: history.state };
  history.replaceState({ otIndex: 0 }, '', '/');
  completions = []; pending = [];
  container = document.createElement('div'); document.body.append(container); root = createRoot(container);
});
afterEach(async () => {
  await act(async () => { completions.forEach(finish => finish()); await Promise.allSettled(pending); });
  await act(async () => root.unmount()); container.remove();
  history.replaceState(previous.state, '', previous.url);
  for (const [name, descriptor] of dialogMethods) {
    if (descriptor) Object.defineProperty(HTMLDialogElement.prototype, name, descriptor);
    else Reflect.deleteProperty(HTMLDialogElement.prototype, name);
  }
  vi.useRealTimers(); vi.restoreAllMocks(); vi.unstubAllGlobals();
});
function deferred<T>(fallback: T) {
  let resolve!: (value: T) => void; let reject!: (error: unknown) => void;
  const promise = new Promise<T>((finish, fail) => { resolve = finish; reject = fail; });
  completions.push(() => resolve(fallback)); pending.push(promise); return { promise, resolve, reject };
}
function mockCreation(respond: (count: number) => Promise<Response> = () => Promise.resolve(json(workspace, 201))) {
  const posts: { body: string; key: string | null; csrf: string | null }[] = [];
  vi.stubGlobal('fetch', vi.fn((url: string, options: RequestInit = {}) => {
    if (url.endsWith('/organizations')) return Promise.resolve(json([{ id: 'organization', name: 'Example organization', role: 'owner' }]));
    if (url.endsWith('/workspaces') && options.method === 'POST') {
      const headers = new Headers(options.headers);
      posts.push({ body: options.body as string, key: headers.get('Idempotency-Key'), csrf: headers.get('X-CSRF-Token') });
      return respond(posts.length);
    }
    throw new Error(`Unexpected request: ${url}`);
  }));
  return posts;
}
function button(text: string) { return [...container.querySelectorAll<HTMLButtonElement>('button')].find(item => item.textContent?.trim() === text)!; }
function closeIcon() { return container.querySelector<HTMLButtonElement>('[aria-label="Close dialog"]')!; }
async function cancelDialog() { await act(async () => container.querySelector('dialog')!.dispatchEvent(new Event('cancel', { bubbles: false, cancelable: true }))); }
async function render(current: Session, close: () => void, created: (item: WorkspaceDetail) => Promise<void>) {
  await act(async () => root.render(createElement(CreateWorkspace, { session: current, close, created, reauthenticate: vi.fn() })));
}
async function fill() {
  const form = container.querySelector<HTMLFormElement>('form')!;
  for (const [name, value] of Object.entries(fields)) {
    const field = form.elements.namedItem(name) as HTMLInputElement;
    await act(async () => { Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(field, value); field.dispatchEvent(new Event('input', { bubbles: true })); });
  }
}
function expectFieldsLocked() {
  const inputs = [...container.querySelectorAll<HTMLInputElement | HTMLSelectElement>('input, select')];
  expect(inputs).toHaveLength(4); expect(inputs.every(item => item.matches(':disabled'))).toBe(true);
}

it.each(['name', 'team_name'])('preserves a rejected supplementary %s draft and repairs without an extra workspace handoff', async field => {
  const created = vi.fn(async (_item: WorkspaceDetail) => {});
  const posts = mockCreation(count => Promise.resolve(count === 1 ? json({ code: 'invalid', detail: 'Name exceeds its scalar limit' }, 422) : json(workspace, 201)));
  await render(session, vi.fn(), created); await fill();
  const input = container.querySelector<HTMLInputElement>(`input[name="${field}"]`)!; const name = '😀'.repeat(100);
  await act(async () => { input.value = name + '😀'; input.dispatchEvent(new Event('input', { bubbles: true })); button('Create workspace').click(); });
  expect(JSON.parse(posts[0].body)).toEqual({ ...fields, [field]: name + '😀', organization_id: 'organization' }); expect(input.value).toBe(name + '😀'); expect(created).not.toHaveBeenCalled(); expect(input.matches(':disabled')).toBe(false);
  await act(async () => { input.value = name; input.dispatchEvent(new Event('input', { bubbles: true })); button('Create workspace').click(); });
  expect(JSON.parse(posts[1].body)).toEqual({ ...fields, [field]: name, organization_id: 'organization' }); expect(posts).toHaveLength(2); expect(created).toHaveBeenCalledExactlyOnceWith(workspace); expectFieldsLocked();
});

it.each([
  ['network', false], ['408', false], ['500', false], ['network', true],
] as const)('retries a %s workspace outcome with the exact request (CSRF rotated: %s)', async (failure, rotate) => {
  const close = vi.fn(); const created = vi.fn(async (_item: WorkspaceDetail) => {});
  const posts = mockCreation(count => count > 1 ? Promise.resolve(json(workspace, 201)) : failure === 'network'
    ? Promise.reject(new TypeError('Synthetic disconnected response'))
    : Promise.resolve(json({ code: 'request_interrupted', detail: 'Synthetic interrupted response.' }, Number(failure))));
  await render(session, close, created); await fill();
  await act(async () => button('Create workspace').click());
  expect(created).not.toHaveBeenCalled(); expect(posts).toHaveLength(1);
  expect(posts[0].key).toBeTruthy(); expect(posts[0].csrf).toBe(session.csrf_token);
  expect(JSON.parse(posts[0].body)).toEqual({ ...fields, organization_id: 'organization' });
  expect(container.textContent).toContain('The response was interrupted.'); expectFieldsLocked();
  expect(closeIcon().disabled).toBe(true); expect(button('Cancel').disabled).toBe(true);
  await act(async () => { closeIcon().click(); button('Cancel').click(); }); await cancelDialog();
  expect(close).not.toHaveBeenCalled();
  const current = rotate ? { ...session, csrf_token: 'rotated-csrf' } : session;
  if (rotate) {
    await render(current, close, created);
    expectFieldsLocked();
    for (const [name, value] of Object.entries(fields)) expect(container.querySelector<HTMLInputElement>(`input[name="${name}"]`)?.value).toBe(value);
  }
  await act(async () => button('Retry safely').click());
  expect(posts).toHaveLength(2); expect(posts[1]).toEqual({ ...posts[0], csrf: current.csrf_token });
  expect(created).toHaveBeenCalledExactlyOnceWith(workspace);
  expect(container.textContent).not.toContain('The response was interrupted.'); expectFieldsLocked();
});

it('blocks dismissal during create and handoff, then permits close and dialog cancellation after a handoff error', async () => {
  const response = deferred(json(workspace, 201)); const handoff = deferred<void>(undefined);
  const close = vi.fn(); const created = vi.fn((_item: WorkspaceDetail) => handoff.promise);
  const posts = mockCreation(() => response.promise);
  await render(session, close, created); await fill();
  expect(closeIcon().disabled).toBe(false); expect(button('Cancel').disabled).toBe(false);
  await act(async () => button('Create workspace').click());
  expect(posts).toHaveLength(1); expect(created).not.toHaveBeenCalled(); expectFieldsLocked();
  expect(closeIcon().disabled).toBe(true); expect(button('Cancel').disabled).toBe(true);
  await act(async () => { closeIcon().click(); button('Cancel').click(); }); await cancelDialog();
  expect(close).not.toHaveBeenCalled();
  await act(async () => response.resolve(json(workspace, 201)));
  expect(created).toHaveBeenCalledExactlyOnceWith(workspace); expectFieldsLocked();
  expect(button('Opening…').disabled).toBe(true); expect(closeIcon().disabled).toBe(true); expect(button('Close').disabled).toBe(true);
  await act(async () => { closeIcon().click(); button('Close').click(); }); await cancelDialog();
  expect(close).not.toHaveBeenCalled();
  await act(async () => handoff.reject(new Error('Synthetic access refresh failure.')));
  expect(container.querySelector('[role="alert"]')?.textContent).toContain('Synthetic access refresh failure.');
  expectFieldsLocked(); expect(closeIcon().disabled).toBe(false); expect(button('Close').disabled).toBe(false);
  await cancelDialog(); expect(close).toHaveBeenCalledTimes(1);
  await act(async () => button('Close').click()); expect(close).toHaveBeenCalledTimes(2);
  await act(async () => closeIcon().click()); expect(close).toHaveBeenCalledTimes(3);
  expect(posts).toHaveLength(1); expect(created).toHaveBeenCalledTimes(1);
});

it('does not create another workspace while the committed workspace handoff is still pending', async () => {
  const posts = mockCreation(); const handoff = deferred<void>(undefined);
  const created = vi.fn((_item: WorkspaceDetail) => handoff.promise);
  await render(session, vi.fn(), created); await fill();
  await act(async () => button('Create workspace').click());
  expect(created).toHaveBeenCalledExactlyOnceWith(workspace);
  expect(posts).toHaveLength(1); expect(posts[0].key).toBeTruthy();
  expect(JSON.parse(posts[0].body)).toEqual({ ...fields, organization_id: 'organization' });
  await act(async () => button('Opening…').click());
  expect({ posts: posts.length, distinctKeys: new Set(posts.map(post => post.key)).size, handoffs: created.mock.calls.length },
    'An acknowledged workspace must not be created again while its access refresh or navigation is pending.')
    .toEqual({ posts: 1, distinctKeys: 1, handoffs: 1 });
});

it('keeps the committed workspace locked after a handoff failure and retries opening that same result without another create', async () => {
  const posts = mockCreation(); const first = deferred<void>(undefined); const second = deferred<void>(undefined);
  const created = vi.fn<(item: WorkspaceDetail) => Promise<void>>()
    .mockImplementationOnce(() => first.promise).mockImplementationOnce(() => second.promise);
  await render(session, vi.fn(), created); await fill();
  await act(async () => button('Create workspace').click());
  expect(created).toHaveBeenCalledExactlyOnceWith(workspace);
  const committed = created.mock.calls[0][0]; expectFieldsLocked();
  const failure = new Error('Synthetic workspace access refresh failed.');
  await act(async () => first.reject(failure));
  expect(container.querySelector('[role="alert"]')?.textContent).toContain(failure.message);
  expect(container.textContent).toContain(workspace.name); expectFieldsLocked();
  const retry = button('Retry opening workspace'); expect(retry).toBeDefined(); expect(retry.disabled).toBe(false);
  await act(async () => retry.click());
  expect(created).toHaveBeenCalledTimes(2); expect(created.mock.calls[1][0]).toBe(committed); expectFieldsLocked();
  expect({ posts: posts.length, distinctKeys: new Set(posts.map(post => post.key)).size }).toEqual({ posts: 1, distinctKeys: 1 });
  await act(async () => second.resolve());
  expect(container.querySelector('[role="alert"]')).toBeNull(); expect(container.textContent).not.toContain(failure.message);
  expectFieldsLocked(); expect(created.mock.calls[1][0]).toBe(committed); expect(posts).toHaveLength(1);
});
