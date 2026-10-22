// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { activitySummary, IssueActivity } from './IssueActivity';
import type { ActivityEntry } from '../types';

const entry: ActivityEntry = { id: 'event-1', event_kind: 'issue.updated', actor: 'Example Member', created_at: '2026-10-17T10:30:00Z', changes: { fields: ['title', 'status_id'] } };
const json = (data: unknown) => new Response(JSON.stringify({ data }), { status: 200 });
let container: HTMLDivElement;
let root: Root;
beforeEach(() => { vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true); container = document.createElement('div'); document.body.append(container); root = createRoot(container); });
afterEach(async () => { await act(async () => root.unmount()); container.remove(); vi.unstubAllGlobals(); });

describe('issue activity', () => {
  it('describes recorded field changes without inventing archive direction or values', () => {
    expect(activitySummary(entry)).toBe('changed title and status');
    expect(activitySummary({ ...entry, changes: { fields: ['archived'] } })).toBe('changed archive state');
    expect(activitySummary({ ...entry, event_kind: 'issue.created', changes: {} })).toBe('created this issue');
  });
  it('explains deletion effects without naming a deleted or inaccessible target', () => {
    expect(activitySummary({ ...entry, event_kind: 'issue.deleted', changes: {} })).toBe('moved this issue to Trash');
    expect(activitySummary({ ...entry, event_kind: 'issue.restored', changes: {} })).toBe('restored this issue from Trash');
    expect(activitySummary({ ...entry, event_kind: 'issue.parent_changed', changes: { action: 'parent_detached_after_deletion', fields: ['parent'] } })).toBe('detached a parent or subissue connection after a linked issue was deleted');
    expect(activitySummary({ ...entry, event_kind: 'issue.parent_changed', changes: { action: 'parent_detached_after_deletion', fields: ['parent', 'relations'] } })).toBe('removed hierarchy and relationship connections after a linked issue was deleted');
    expect(activitySummary({ ...entry, event_kind: 'issue.relation_removed', changes: { action: 'relation_removed_after_deletion' } })).toBe('removed a relationship after a linked issue was deleted');
  });
  it('renders the actual actor and reloads when the issue version changes', async () => {
    const fetch = vi.fn().mockResolvedValue(json([entry])); vi.stubGlobal('fetch', fetch);
    await act(async () => root.render(<IssueActivity workspaceId="w" issueId="i" version="1" />));
    expect(container.textContent).toContain('Example Member changed title and status.');
    fetch.mockResolvedValue(json([{ ...entry, id: 'event-2', actor: 'Example Owner' }]));
    await act(async () => root.render(<IssueActivity workspaceId="w" issueId="i" version="2" />));
    expect(fetch).toHaveBeenCalledTimes(2);
    expect(container.textContent).toContain('Example Owner changed title and status.');
  });
  it('explains system assignment cleanup without exposing the former assignee', async () => {
    const cleanup = { ...entry, actor: 'OpenTrack', changes: { source: 'access_cleanup', fields: ['assignee_id'], assignee_id: { before: 'private-account-id', after: null } } };
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(json([cleanup])));
    await act(async () => root.render(<IssueActivity workspaceId="w" issueId="i" version="2" />));
    expect(container.textContent).toContain('OpenTrack cleared the assignee because they no longer have access.');
    expect(container.textContent).not.toContain('private-account-id');
  });
  it('aborts the previous workspace request and ignores a late response', async () => {
    let finishOld!: (response: Response) => void; let oldSignal!: AbortSignal;
    vi.stubGlobal('fetch', vi.fn((url: string, options: RequestInit) => {
      if (url.includes('/old/')) { oldSignal = options.signal!; return new Promise<Response>(resolve => { finishOld = resolve; }); }
      return Promise.resolve(json([{ ...entry, actor: 'Current workspace actor' }]));
    }));
    await act(async () => root.render(<IssueActivity workspaceId="old" issueId="i-old" version="1" />));
    await act(async () => root.render(<IssueActivity workspaceId="new" issueId="i-new" version="1" />));
    expect(oldSignal.aborted).toBe(true);
    await act(async () => { finishOld(json([{ ...entry, actor: 'Old workspace actor' }])); });
    expect(container.textContent).toContain('Current workspace actor');
    expect(container.textContent).not.toContain('Old workspace actor');
  });
  it('aborts activity loading when the session clears the mounted detail', async () => {
    let signal!: AbortSignal;
    vi.stubGlobal('fetch', vi.fn((_url: string, options: RequestInit) => { signal = options.signal!; return new Promise<Response>(() => {}); }));
    await act(async () => root.render(<IssueActivity workspaceId="w" issueId="i" version="1" />));
    await act(async () => root.render(<p>Sign in</p>));
    expect(signal.aborted).toBe(true);
    expect(container.textContent).toBe('Sign in');
  });
  it('renders expandable individual events with facts and timestamps in a property group', async () => {
    const updates = [0, 1].map(index => ({ ...entry, id: `event-${index}`, actor_id: 'member-1', created_at: `2026-10-17T10:3${index}:00Z`, changes: { source: 'manual', fields: ['priority'], priority: { before: index, after: index + 1 } } }));
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(json(updates)));
    await act(async () => root.render(<IssueActivity workspaceId="w" issueId="i" version="1" />));
    expect(container.querySelectorAll('.activity-list > li').length).toBe(1);
    const group = container.querySelector('details')!;
    expect(group.querySelector('summary')!.textContent).toContain('Example Member changed properties in 2 updates.');
    expect(group.querySelector('summary time')!.getAttribute('datetime')).toBe(updates[1].created_at);
    expect(group.open).toBe(false);
    await act(async () => group.querySelector('summary')!.click());
    expect(group.open).toBe(true);
    expect(group.querySelectorAll('[role="listitem"]').length).toBe(2);
    expect(group.querySelectorAll('[role="listitem"] time').length).toBe(2);
    expect(group.textContent).toContain('No priority → Urgent'); expect(group.textContent).toContain('Urgent → High');
  });
  it('loads older events in chronological order and groups across the page boundary', async () => {
    const update = (id: number): ActivityEntry => ({ ...entry, id: `event-${id}`, actor_id: 'member-1', created_at: `2026-10-17T10:3${id}:00Z`, changes: { source: 'manual', fields: ['estimate'], estimate: { before: id, after: id + 1 } } });
    const fetch = vi.fn().mockResolvedValueOnce(new Response(JSON.stringify({ data: [update(2)], has_more: true, next_cursor: 'older' }))).mockResolvedValueOnce(new Response(JSON.stringify({ data: [update(1), update(0)], has_more: false })));
    vi.stubGlobal('fetch', fetch);
    await act(async () => root.render(<IssueActivity workspaceId="w" issueId="i" version="1" />));
    await act(async () => container.querySelector<HTMLButtonElement>('.comments-older')!.click());
    expect(fetch.mock.calls[1][0]).toContain('cursor=older');
    expect(container.querySelectorAll('.activity-list > li').length).toBe(1);
    expect([...container.querySelectorAll<HTMLTimeElement>('[role="listitem"] time')].map(time => time.dateTime)).toEqual([0, 1, 2].map(id => `2026-10-17T10:3${id}:00Z`));
    expect(container.querySelector('.comments-older')).toBeNull();
  });
});
