// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { IssueEditor } from './IssueEditor';
import type { Bootstrap, Issue, Session } from '../types';

const issue: Issue = { id: 'i', workspace_id: 'w', team_id: 't', key: 'ENG-1', title: 'A real issue', description: 'Details', status_id: 'todo', priority: 0, assignee_id: null, assignee: null, label_ids: [], labels: [], etag: '"issue:fixture:1:projection"', estimate: null, due_date: null, version: '1', created_at: '2026-10-17T10:00:00Z', updated_at: '2026-10-17T10:00:00Z', archived_at: null };
const session: Session = { audience: 'web', user: { id: 'u', display_name: 'Owner', email: 'owner@example.test' }, csrf_token: 'csrf', workspaces: [{ id: 'w', name: 'Workspace', slug: 'workspace', role: 'owner' }] };
const data: Bootstrap = { auth_epoch: '1', teams: [{ id: 't', name: 'Engineering', key: 'ENG', private: false, archived_at: null }], members: [session.user], statuses: [{ id: 'todo', team_id: 't', name: 'Todo', category: 'unstarted', color: 'slate', position: 0, is_default: true, retiring_at: null, archived_at: null, replacement_status_id: null, replacement_job_id: null }], issues: [issue] };
let container: HTMLDivElement; let root: Root;
const json = (value: unknown, status = 200) => new Response(JSON.stringify(value), { status });
beforeEach(() => { HTMLDialogElement.prototype.showModal = function () { this.setAttribute('open', ''); }; HTMLDialogElement.prototype.close = function () { this.removeAttribute('open'); }; vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true); container = document.createElement('div'); document.body.append(container); root = createRoot(container); });
afterEach(async () => { await act(async () => root.unmount()); container.remove(); vi.unstubAllGlobals(); });
function button(label: string) { return [...container.querySelectorAll<HTMLButtonElement>('button')].find(button => button.textContent === label)!; }

describe('archive and restore', () => {
  it('sends versioned archive/restore commands and makes archived fields read-only', async () => {
    let current = issue;
    const fetch = vi.fn((url: string, options: RequestInit) => {
      if (url.endsWith('/parent')) return Promise.resolve(json({ issue_id: issue.id, issue_etag: issue.etag, graph_version: '1', parent: null, can_change: true }));
      if (/\/(children|relations)\?/.test(url)) return Promise.resolve(json({ data: [], next_cursor: null, has_more: false, issue_etag: issue.etag, graph_version: '1' }));
      if (url.endsWith('/activity')) return Promise.resolve(json({ data: [] }));
      if (options.method === 'PATCH') {
        const body = JSON.parse(options.body as string);
        current = { ...current, etag: `"issue:i:${Number(current.version) + 1}:projection"`, version: String(Number(current.version) + 1), archived_at: body.archived ? '2026-10-17T12:00:00Z' : null };
      }
      return Promise.resolve(json(current));
    });
    vi.stubGlobal('fetch', fetch);
    const updated = vi.fn();
    await act(async () => root.render(<IssueEditor workspaceId="w" issueId="i" data={data} session={session} close={() => {}} updated={updated} />));
    await act(async () => button('Archive').click());
    expect(container.querySelector<HTMLInputElement>('#issue-title')?.matches(':disabled')).toBe(true);
    expect(container.textContent).toContain('This issue is archived');
    await act(async () => button('Restore').click());
    await act(async () => button('Restore issue').click());
    expect(container.querySelector<HTMLInputElement>('#issue-title')?.matches(':disabled')).toBe(false);
    const writes = fetch.mock.calls.filter(([, options]) => options.method === 'PATCH');
    expect(writes.map(([, options]) => JSON.parse(options.body as string))).toEqual([{ archived: true }, { archived: false }]);
    expect(writes.map(([, options]) => new Headers(options.headers).get('If-Match'))).toEqual(['"issue:fixture:1:projection"', '"issue:i:2:projection"']);
    expect(updated).toHaveBeenCalledTimes(2);
  });
  it('does not expose restore to a viewer', async () => {
    vi.stubGlobal('fetch', vi.fn((url: string) => Promise.resolve(json(url.endsWith('/parent') ? { issue_id: issue.id, issue_etag: issue.etag, graph_version: '1', parent: null, can_change: false } : /\/(children|relations)\?/.test(url) ? { data: [], next_cursor: null, has_more: false, issue_etag: issue.etag, graph_version: '1' } : url.endsWith('/activity') ? { data: [] } : { ...issue, archived_at: '2026-10-17T12:00:00Z' }))));
    const viewer = { ...session, workspaces: [{ ...session.workspaces[0], role: 'viewer' }] };
    await act(async () => root.render(<IssueEditor workspaceId="w" issueId="i" data={data} session={viewer} close={() => {}} updated={() => {}} />));
    expect(button('Restore')).toBeUndefined();
    expect(button('Archive')).toBeUndefined();
    expect(container.querySelector<HTMLInputElement>('#issue-title')?.matches(':disabled')).toBe(true);
  });
  it('loads a newer version on an archive conflict without silently retrying', async () => {
    const latest = { ...issue, title: 'Title changed by another person', version: '2' };
    const fetch = vi.fn((url: string, options: RequestInit) => {
      if (url.endsWith('/parent')) return Promise.resolve(json({ issue_id: issue.id, issue_etag: issue.etag, graph_version: '1', parent: null, can_change: true }));
      if (/\/(children|relations)\?/.test(url)) return Promise.resolve(json({ data: [], next_cursor: null, has_more: false, issue_etag: issue.etag, graph_version: '1' }));
      if (url.endsWith('/activity')) return Promise.resolve(json({ data: [] }));
      if (options.method === 'PATCH') return Promise.resolve(json({ type: 'about:blank', title: 'Conflict', status: 412, code: 'conflict', detail: 'Changed', details: { current: latest } }, 412));
      return Promise.resolve(json(issue));
    });
    vi.stubGlobal('fetch', fetch);
    await act(async () => root.render(<IssueEditor workspaceId="w" issueId="i" data={data} session={session} close={() => {}} updated={() => {}} />));
    await act(async () => button('Archive').click());
    expect(container.querySelector<HTMLInputElement>('#issue-title')?.value).toBe(latest.title);
    expect(container.textContent).toContain('Review it before trying again.');
    expect(fetch.mock.calls.filter(([, options]) => options.method === 'PATCH')).toHaveLength(1);
  });
  it('makes work inside an archived team read-only without offering issue restore', async () => {
    vi.stubGlobal('fetch', vi.fn((url: string) => Promise.resolve(json(url.endsWith('/parent') ? { issue_id: issue.id, issue_etag: issue.etag, graph_version: '1', parent: null, can_change: false } : /\/(children|relations)\?/.test(url) ? { data: [], next_cursor: null, has_more: false, issue_etag: issue.etag, graph_version: '1' } : url.endsWith('/activity') ? { data: [] } : { ...issue, archived_at: '2026-10-17T12:00:00Z' }))));
    const archivedTeam = { ...data, teams: data.teams.map(team => ({ ...team, archived_at: '2026-10-17T12:00:00Z' })) };
    await act(async () => root.render(<IssueEditor workspaceId="w" issueId="i" data={archivedTeam} session={session} close={() => {}} updated={() => {}} />));
    expect(container.querySelector<HTMLInputElement>('#issue-title')?.matches(':disabled')).toBe(true);
    expect(button('Restore')).toBeUndefined(); expect(button('Archive')).toBeUndefined();
    expect(container.textContent).toContain('Restore it before editing this issue.');
  });

});
