// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { App } from './App';
import { ApiError, request } from './api';
import { navigateConfirmed } from './router';
import { viewHash } from './filters/model';
import type { Bootstrap, Session } from './types';

vi.mock('./api', async () => ({ ...await vi.importActual('./api'), request: vi.fn() }));
vi.mock('./useWorkspaceAccess', () => ({ useWorkspaceAccess: () => {} }));
vi.mock('./realtime/useWorkspaceRealtime', () => ({ useWorkspaceRealtime: () => ({ status: 'live', readOnly: false, retry: () => {} }), RealtimeBanner: () => null }));
const id = '11111111-1111-4111-8111-111111111111';
const workspaceId = '44444444-4444-4444-8444-444444444444';
const workspace = { id: workspaceId, name: 'Example Workspace', slug: 'example', role: 'owner' };
const session: Session = { audience: 'web', user: { id: 'owner', display_name: 'Example Owner', email: 'owner@example.test' }, csrf_token: 'csrf', workspaces: [workspace] };
const data: Bootstrap = { auth_epoch: '1', teams: [{ id: 'team', key: 'EX', name: 'Example Team', private: false, archived_at: null }], statuses: [], members: [session.user], issues: [] };
let root: Root; let host: HTMLDivElement; let previous: { url: string; title: string; theme: string | null; storage: [string, string][] };
const dialogMethods = ['showModal', 'close'].map(name => [name, Object.getOwnPropertyDescriptor(HTMLDialogElement.prototype, name)] as const);
beforeEach(() => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true); vi.useFakeTimers(); vi.mocked(request).mockReset();
  HTMLDialogElement.prototype.showModal = function () { this.setAttribute('open', ''); };
  HTMLDialogElement.prototype.close = function () { this.removeAttribute('open'); };
  previous = { url: location.pathname + location.search + location.hash, title: document.title, theme: document.documentElement.getAttribute('data-theme'), storage: Array.from({ length: localStorage.length }, (_, index) => { const key = localStorage.key(index)!; return [key, localStorage.getItem(key)!]; }) };
  localStorage.clear(); host = document.createElement('div'); document.body.append(host); root = createRoot(host);
  vi.mocked(request).mockImplementation(async path => {
    if (path === '/setup/status') return { setup_required: false } as never;
    if (path === '/auth/session') return session as never;
    if (path.endsWith('/bootstrap')) return data as never;
    if (path.endsWith('/access')) return { auth_epoch: '1', role: 'owner', archived: false } as never;
    if (path.endsWith('/preferences')) return { workspace_id: 'w', shortcut_mode: 'disabled', version: '1', etag: '"preferences:1"' } as never;
    if (path.includes('/notifications?')) return { data: [], has_more: false, next_cursor: null, unread_count: 0 } as never;
    if (/\/(views|saved-searches)\?/.test(path)) return { data: [], has_more: false, next_cursor: null } as never;
    // Keep unexpected area/detail reads pending so they cannot redirect the
    // route or conceal the fact that an invalid entry mounted its reader.
    return new Promise(() => {});
  });
});
afterEach(async () => {
  await act(async () => root.unmount()); host.remove(); navigateConfirmed(previous.url, true); document.title = previous.title;
  localStorage.clear(); previous.storage.forEach(([key, value]) => localStorage.setItem(key, value));
  if (previous.theme === null) document.documentElement.removeAttribute('data-theme'); else document.documentElement.setAttribute('data-theme', previous.theme);
  for (const [name, descriptor] of dialogMethods) { if (descriptor) Object.defineProperty(HTMLDialogElement.prototype, name, descriptor); else Reflect.deleteProperty(HTMLDialogElement.prototype, name); }
  vi.useRealTimers(); vi.restoreAllMocks(); vi.unstubAllGlobals();
});
it.each([
  `/w/${workspaceId}/projects`, `/w/${workspaceId}/projects/project`, `/w/${workspaceId}/cycles`, `/w/${workspaceId}/settings/workspace`, `/w/${workspaceId}/inbox`, `/w/${workspaceId}/trash`,
  `/w/${workspaceId}/issues/issue`, `/w/${workspaceId}/issues/by-key/EX-1`,
])('raw favorite marker on %s mounts only the invalid-link boundary', async path => {
  navigateConfirmed(`${path}?open_saved=view:${id}`);
  await act(async () => root.render(<App/>)); await act(async () => { await vi.advanceTimersByTimeAsync(200); });
  expect(host.textContent).toContain('This saved discovery link is invalid');
  const reads = vi.mocked(request).mock.calls.map(([path]) => path);
  expect(reads).toContain(`/workspaces/${workspaceId}/bootstrap`);
  const allowed = (path: string) => ['/setup/status', '/auth/session', `/workspaces/${workspaceId}/bootstrap`, `/workspaces/${workspaceId}/access`, `/workspaces/${workspaceId}/preferences`].includes(path)
    || path.includes('/notifications?') && path.includes('limit=1') || /\/(views|saved-searches)\?/.test(path);
  expect(reads.filter(path => !allowed(path))).toEqual([]);
  expect(host.querySelector('.issue-workspace, .issue-detail, .issue-row, .discovery-results')).toBeNull();
});
it.each(['valid', 'malformed'] as const)('raw favorite marker plus %s structured hash cannot become an ordinary query', async variant => {
  const hash = variant === 'valid' ? viewHash({ version: 1, layout: 'list', filter: null, sort: 'created', archived: false, limit: 50, timezone: 'UTC' }) : '#filters=malformed';
  navigateConfirmed(`/w/${workspaceId}/issues?open_saved=view:${id}${hash}`);
  await act(async () => root.render(<App/>)); await act(async () => { await vi.advanceTimersByTimeAsync(200); });
  expect(host.textContent).toContain('This saved discovery link is invalid');
  const reads = vi.mocked(request).mock.calls.map(([path]) => path);
  expect(reads.filter(path => /\/(issues(?:\/|\?|$)|search\/query)/.test(path))).toEqual([]);
});

it('opening the mobile navigation reuses the existing favorite directories', async () => {
  navigateConfirmed(`/w/${workspaceId}/issues`);
  await act(async () => root.render(<App/>));
  const collectionCalls = () => vi.mocked(request).mock.calls.filter(([path]) => /\/(views|saved-searches)\?/.test(path));
  expect(collectionCalls()).toHaveLength(2);
  const open = host.querySelector<HTMLButtonElement>('[aria-label="Open navigation"]'); expect(open).not.toBeNull();
  await act(async () => open!.click());
  expect(host.querySelector('dialog')).not.toBeNull(); expect(collectionCalls()).toHaveLength(2);
});

it.each([403, 404])('repeated collection %s with unchanged authority has only one automatic repair', async code => {
  const fallback = vi.mocked(request).getMockImplementation()!;
  vi.mocked(request).mockImplementation((path, options) => /\/(views|saved-searches)\?/.test(path)
    ? Promise.reject(new ApiError(code, 'not_available', 'Favorite directory unavailable')) : fallback(path, options));
  navigateConfirmed(`/w/${workspaceId}/issues`);
  await act(async () => root.render(<App/>));
  for (let round = 0; round < 4; round++) await act(async () => { await vi.advanceTimersByTimeAsync(200); });
  const reads = vi.mocked(request).mock.calls.map(([path]) => path);
  expect(reads.filter(path => path === '/auth/session')).toHaveLength(2);
  expect(reads.filter(path => path.endsWith('/bootstrap'))).toHaveLength(2);
  expect(reads.filter(path => path.endsWith('/access'))).toHaveLength(2);
  expect(host.textContent).toContain('Favorite directory unavailable');
  const retry = host.querySelector<HTMLButtonElement>('[aria-label="Retry favorite views"]'); expect(retry).not.toBeNull();
  await act(async () => retry!.click());
  expect(vi.mocked(request).mock.calls.filter(([path]) => path === '/auth/session')).toHaveLength(2);
  expect(vi.mocked(request).mock.calls.filter(([path]) => path.endsWith('/bootstrap'))).toHaveLength(2);
});
