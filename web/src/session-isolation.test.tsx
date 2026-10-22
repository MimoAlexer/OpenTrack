// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { App } from './App';
import { request, SessionInvalidatedError } from './api';
import { navigate } from './router';
import type { Bootstrap, Issue, Session } from './types';

const user = { id: 'user-a', email: 'a@example.test', display_name: 'Person A' };
const session: Session = { audience: 'web', user, csrf_token: 'test-csrf', workspaces: [{ id: 'a', name: 'Workspace A', slug: 'a', role: 'owner' }, { id: 'b', name: 'Workspace B', slug: 'b', role: 'owner' }] };
const issue: Issue = { id: 'issue-a', workspace_id: 'a', team_id: 'team-a', key: 'AAA-1', title: 'Private title from A', description: 'Description belonging to A', status_id: 'status-a', priority: 0, assignee_id: null, assignee: null, label_ids: [], labels: [], etag: '"issue:fixture:1:projection"', estimate: null, due_date: null, version: '1', created_at: '2026-10-17T10:00:00Z', updated_at: '2026-10-17T10:00:00Z', archived_at: null };
function bootstrap(workspace: string): Bootstrap { return { auth_epoch: '1', teams: [{ id: `team-${workspace}`, name: `Team ${workspace}`, key: workspace.toUpperCase(), private: false, archived_at: null }], statuses: [{ id: `status-${workspace}`, team_id: `team-${workspace}`, name: 'Todo', category: 'unstarted', color: 'slate', position: 0, is_default: true, retiring_at: null, archived_at: null, replacement_status_id: null, replacement_job_id: null }], members: [user], issues: workspace === 'a' ? [issue] : [] }; }
function json(value: unknown) { return new Response(JSON.stringify(value), { status: 200, headers: { 'Content-Type': 'application/json' } }); }
let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true);
  vi.spyOn(window, 'confirm').mockReturnValue(true);
  vi.spyOn(document, 'visibilityState', 'get').mockReturnValue('visible');
  history.replaceState({ otIndex: 0 }, '', '/w/a/issues/issue-a');
  container = document.createElement('div'); document.body.append(container); root = createRoot(container);
});
afterEach(async () => {
  await act(async () => root.unmount()); container.remove(); vi.restoreAllMocks(); vi.unstubAllGlobals();
});
async function flush() { await act(async () => { await new Promise(resolve => setTimeout(resolve, 0)); }); }
function transport(url: string): Promise<Response> {
  if (url.endsWith('/setup/status')) return Promise.resolve(json({ setup_required: false }));
  if (url.endsWith('/auth/session')) return Promise.resolve(json(session));
  if (url.endsWith('/a/bootstrap')) return Promise.resolve(json(bootstrap('a')));
  if (url.endsWith('/b/bootstrap')) return Promise.resolve(json(bootstrap('b')));
  if (url.endsWith('/access')) return Promise.resolve(json({ auth_epoch: '1', role: 'owner', archived: false }));
  if (url.endsWith('/activity')) return Promise.resolve(json({ data: [] }));
  if (url.endsWith('/issues/issue-a')) return Promise.resolve(json(issue));
  if (url.includes('/a/issues?')) return Promise.resolve(json({ data: [issue], next_cursor: null, has_more: false }));
  if (url.includes('/b/issues?')) return Promise.resolve(json({ data: [], next_cursor: null, has_more: false }));
  throw new Error(`Unexpected request: ${url}`);
}

describe('tenant and session UI boundaries', () => {
  it('removes the previous workspace subtree before a new bootstrap finishes', async () => {
    let completeBootstrap!: (response: Response) => void;
    vi.stubGlobal('fetch', vi.fn((url: string) => url.endsWith('/b/bootstrap') ? new Promise<Response>(resolve => { completeBootstrap = resolve; }) : transport(url)));
    await act(async () => root.render(<App />)); await flush();
    expect(container.querySelector<HTMLSelectElement>('select[aria-label="Workspace"]')?.value).toBe('a');
    expect(container.querySelector<HTMLInputElement>('#issue-title')?.value).toBe(issue.title);
    await act(async () => { navigate('/w/b/issues'); });
    expect(container.textContent).not.toContain(issue.title);
    expect(container.querySelector('#issue-title')).toBeNull();
    expect(container.textContent).not.toContain('Team a');
    await act(async () => { completeBootstrap(json(bootstrap('b'))); }); await flush();
    expect(container.textContent).toContain('Team b');
    expect(container.textContent).not.toContain(issue.title);
  });
  it('clears issue details when the session expires', async () => {
    vi.stubGlobal('fetch', vi.fn(transport));
    await act(async () => root.render(<App />)); await flush();
    expect(container.querySelector<HTMLInputElement>('#issue-title')?.value).toBe(issue.title);
    await act(async () => { window.dispatchEvent(new CustomEvent('opentrack:session-expired')); });
    expect(container.textContent).toContain('Sign in to OpenTrack.');
    expect(container.textContent).not.toContain('Workspace A');
    expect(container.querySelector('#issue-title')).toBeNull();
    expect(container.querySelector<HTMLTextAreaElement>('textarea')?.value).not.toBe(issue.description);
  });
  it('shows a committed-access notice and clears authenticated views without refreshing after self-elevation', async () => {
    const fetch = vi.fn((url: string) => url.endsWith('/members/user-a')
      ? Promise.resolve(new Response(JSON.stringify({ role: 'maintainer' }), { headers: { 'X-OpenTrack-Session-Invalidated': 'true' } }))
      : transport(url));
    vi.stubGlobal('fetch', fetch);
    await act(async () => root.render(<App />)); await flush();
    expect(container.querySelector('#issue-title')).not.toBeNull();
    const initialReads = fetch.mock.calls.length;
    const refresh = vi.fn();
    await act(async () => {
      await expect(request('/workspaces/a/teams/team-a/members/user-a', { method: 'PUT' }).then(refresh)).rejects.toBeInstanceOf(SessionInvalidatedError);
    });
    expect(container.textContent).toContain('Access updated. Sign in again.');
    expect(container.textContent).toContain('Sign in to OpenTrack.');
    expect(container.textContent).not.toContain('Workspace A');
    expect(container.querySelector('#issue-title')).toBeNull();
    expect(refresh).not.toHaveBeenCalled();
    expect(fetch.mock.calls).toHaveLength(initialReads + 1);
  });
  it('rejects a detail response belonging to a different workspace', async () => {
    vi.stubGlobal('fetch', vi.fn((url: string) => url.endsWith('/issues/issue-a') ? Promise.resolve(json({ ...issue, workspace_id: 'b' })) : transport(url)));
    await act(async () => root.render(<App />)); await flush();
    expect(container.textContent).toContain('This issue is not available in this workspace.');
    expect(container.querySelector('#issue-title')).toBeNull();
  });
  it('destroys cached issue editors immediately when access changes, before the new bootstrap', async () => {
    let epoch = '1'; let completeBootstrap!: (response: Response) => void;
    const fetch = vi.fn((url: string) => {
      if (url.endsWith('/access')) return Promise.resolve(json({ auth_epoch: epoch, role: 'viewer', archived: false }));
      if (epoch === '2' && url.endsWith('/a/bootstrap')) return new Promise<Response>(resolve => { completeBootstrap = resolve; });
      return transport(url);
    });
    vi.stubGlobal('fetch', fetch);
    await act(async () => root.render(<App />)); await flush();
    expect(container.querySelector<HTMLInputElement>('#issue-title')?.value).toBe(issue.title);
    epoch = '2';
    await act(async () => window.dispatchEvent(new Event('focus'))); await flush();
    expect(container.querySelector('#issue-title')).toBeNull();
    expect(container.textContent).not.toContain(issue.title);
    expect(container.textContent).toContain('Refreshing workspace access');
    await act(async () => completeBootstrap(json({ ...bootstrap('a'), auth_epoch: '2' }))); await flush();
    expect(container.querySelector<HTMLInputElement>('#issue-title')?.value).toBe(issue.title);
  });

  it('aborts an open assignee search immediately on access epoch change and excludes its late results after remount', async () => {
    let epoch = '1'; let completeBootstrap!: (response: Response) => void; let completeCandidates!: (response: Response) => void; let candidateSignal!: AbortSignal;
    vi.stubGlobal('fetch', vi.fn((url: string, options: RequestInit) => {
      if (url.endsWith('/workflow')) return Promise.resolve(json({ team_id: 'team-a', version: '1', can_manage: true, statuses: bootstrap('a').statuses }));
      if (url.includes('/assignees?')) return new Promise<Response>(resolve => { completeCandidates = resolve; candidateSignal = options.signal!; });
      if (url.endsWith('/access')) return Promise.resolve(json({ auth_epoch: epoch, role: 'owner', archived: false }));
      if (epoch === '2' && url.endsWith('/a/bootstrap')) return new Promise<Response>(resolve => { completeBootstrap = resolve; });
      return transport(url);
    }));
    await act(async () => root.render(<App />)); await flush();
    const picker = [...container.querySelectorAll('.reference-picker')].find(element => element.querySelector('.field-label')?.textContent === 'Assignee')!;
    await act(async () => picker.querySelector<HTMLButtonElement>('button')!.click()); await flush();
    expect(container.querySelector('[role=combobox]')).not.toBeNull();
    epoch = '2'; await act(async () => window.dispatchEvent(new Event('focus'))); await flush();
    expect(candidateSignal.aborted).toBe(true); expect(container.querySelector('.reference-picker')).toBeNull();
    await act(async () => completeCandidates(json({ data: [{ id: 'private-user', display_name: 'Private candidate before revocation' }], next_cursor: null, has_more: false, self_assignable: true })));
    expect(container.textContent).not.toContain('Private candidate before revocation');
    await act(async () => completeBootstrap(json({ ...bootstrap('a'), auth_epoch: '2' }))); await flush();
    expect(container.querySelector('[role=combobox]')).toBeNull(); expect(container.textContent).not.toContain('Private candidate before revocation');
  });

  it('withholds cached settings after a failed bootstrap refresh and continues checking access', async () => {
    history.replaceState({ otIndex: 0 }, '', '/w/a/settings/workspace');
    let epoch = '1'; let bootstrapCount = 0; let completeBootstrap!: (response: Response) => void;
    let detail = { ...session.workspaces[0], organization_id: 'org', version: '1', archived_at: null };
    const fetch = vi.fn((url: string, options: RequestInit) => {
      if (url.endsWith('/access')) return Promise.resolve(json({ auth_epoch: epoch, role: 'owner', archived: false }));
      if (url.endsWith('/a/bootstrap')) {
        bootstrapCount += 1;
        if (bootstrapCount === 2) return Promise.resolve(new Response(JSON.stringify({ code: 'unavailable', detail: 'Refresh failed.' }), { status: 503 }));
        if (bootstrapCount === 3) return new Promise<Response>(resolve => { completeBootstrap = resolve; });
      }
      if (url.endsWith('/workspaces/a')) {
        if (options.method === 'PATCH') detail = { ...detail, name: 'Updated workspace', version: '2' };
        return Promise.resolve(json(detail));
      }
      return transport(url);
    });
    vi.stubGlobal('fetch', fetch);
    await act(async () => root.render(<App />)); await flush();
    const name = container.querySelector<HTMLInputElement>('.settings-form input')!;
    await act(async () => { Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(name, 'Updated workspace'); name.dispatchEvent(new Event('input', { bubbles: true })); });
    await act(async () => container.querySelector('form')!.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }))); await flush();
    expect(bootstrapCount).toBe(2);
    expect(container.querySelector('.settings-page')).toBeNull();
    expect(container.textContent).toContain('Refresh failed.');
    const accessRequests = fetch.mock.calls.filter(([url]) => url.endsWith('/access')).length;
    epoch = '2';
    await act(async () => window.dispatchEvent(new Event('focus'))); await flush();
    expect(fetch.mock.calls.filter(([url]) => url.endsWith('/access')).length).toBeGreaterThan(accessRequests);
    expect(bootstrapCount).toBe(3);
    expect(container.querySelector('.settings-page')).toBeNull();
    await act(async () => completeBootstrap(json({ ...bootstrap('a'), auth_epoch: '2' }))); await flush();
    expect(container.querySelector<HTMLInputElement>('.settings-form input')?.value).toBe('Updated workspace');
  });

});
