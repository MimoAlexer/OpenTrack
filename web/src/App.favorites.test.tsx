// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { App } from './App';
import { request } from './api';
import { navigateConfirmed } from './router';
import { viewHash } from './filters/model';
import type { Bootstrap, Session } from './types';

vi.mock('./api', async () => ({ ...await vi.importActual('./api'), request: vi.fn() }));
vi.mock('./useWorkspaceAccess', () => ({ useWorkspaceAccess: () => {} }));
vi.mock('./realtime/useWorkspaceRealtime', () => ({ useWorkspaceRealtime: () => ({ status: 'live', readOnly: false, retry: () => {} }), RealtimeBanner: () => null }));
const id = '11111111-1111-4111-8111-111111111111';
const workspace = { id: 'w', name: 'Example Workspace', slug: 'example', role: 'owner' };
const session: Session = { audience: 'web', user: { id: 'owner', display_name: 'Example Owner', email: 'owner@example.test' }, csrf_token: 'csrf', workspaces: [workspace] };
const data: Bootstrap = { auth_epoch: '1', teams: [{ id: 'team', key: 'EX', name: 'Example Team', private: false, archived_at: null }], statuses: [], members: [session.user], issues: [] };
let root: Root; let host: HTMLDivElement; let previous: { url: string; title: string; theme: string | null; storage: [string, string][] };
beforeEach(() => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true); vi.useFakeTimers(); vi.mocked(request).mockReset();
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
  vi.useRealTimers(); vi.restoreAllMocks(); vi.unstubAllGlobals();
});
it.each([
  '/w/w/projects', '/w/w/projects/project', '/w/w/cycles', '/w/w/settings/workspace', '/w/w/inbox', '/w/w/trash',
  '/w/w/issues/issue', '/w/w/issues/by-key/EX-1',
])('raw favorite marker on %s mounts only the invalid-link boundary', async path => {
  navigateConfirmed(`${path}?open_saved=view:${id}`);
  await act(async () => root.render(<App/>)); await act(async () => { await vi.advanceTimersByTimeAsync(200); });
  expect(host.textContent).toContain('This saved discovery link is invalid');
  const reads = vi.mocked(request).mock.calls.map(([path]) => path);
  expect(reads).toContain('/workspaces/w/bootstrap');
  const allowed = (path: string) => ['/setup/status', '/auth/session', '/workspaces/w/bootstrap', '/workspaces/w/access', '/workspaces/w/preferences'].includes(path)
    || path.includes('/notifications?') && path.includes('limit=1') || /\/(views|saved-searches)\?/.test(path);
  expect(reads.filter(path => !allowed(path))).toEqual([]);
  expect(host.querySelector('.issue-workspace, .issue-detail, .issue-row, .discovery-results')).toBeNull();
});
it.each(['valid', 'malformed'] as const)('raw favorite marker plus %s structured hash cannot become an ordinary query', async variant => {
  const hash = variant === 'valid' ? viewHash({ version: 1, layout: 'list', filter: null, sort: 'created', archived: false, limit: 50, timezone: 'UTC' }) : '#filters=malformed';
  navigateConfirmed(`/w/w/issues?open_saved=view:${id}${hash}`);
  await act(async () => root.render(<App/>)); await act(async () => { await vi.advanceTimersByTimeAsync(200); });
  expect(host.textContent).toContain('This saved discovery link is invalid');
  const reads = vi.mocked(request).mock.calls.map(([path]) => path);
  expect(reads.filter(path => /\/(issues(?:\/|\?|$)|search\/query)/.test(path))).toEqual([]);
});
