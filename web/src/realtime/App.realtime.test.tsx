// @vitest-environment jsdom
import { act, createElement, useState } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { App } from '../App';
import type { Bootstrap, Session } from '../types';

vi.mock('../issues/IssueList', () => ({ IssueList: ({ refresh, data }: { refresh: number; data: Bootstrap }) => createElement('div', { 'data-refresh': refresh, 'data-status-labels': data.statuses.map(status => status.name).join(',') }, 'Issue list') }));
vi.mock('../issues/IssueEditor', () => ({ IssueEditor: () => { const [draft, setDraft] = useState('Saved title'); return createElement('input', { 'aria-label': 'Issue title', value: draft, onChange: (event: { target: HTMLInputElement }) => setDraft(event.target.value) }); } }));
vi.mock('../useWorkspaceAccess', () => ({ useWorkspaceAccess: () => {} }));
const session: Session = { audience: 'web', user: { id: 'owner', display_name: 'Example Owner', email: 'owner@example.test' }, csrf_token: 'synthetic-csrf', workspaces: [{ id: 'workspace', name: 'Workspace', slug: 'workspace', role: 'owner' }] };
const snapshot: Bootstrap & { realtime_cursor?: string } = { auth_epoch: '1', realtime_cursor: 'snapshot_cursor', teams: [{ id: 'team', name: 'Example team', key: 'ENG', private: false, archived_at: null }], statuses: [], members: [], issues: [] };
const event = { event_id: '00000000-0000-0000-0000-000000000011', schema_version: 1, type: 'issue.updated', workspace_id: 'workspace', entity_type: 'issue', entity_id: '00000000-0000-0000-0000-000000000001', entity_version: '2', actor_id: null, occurred_at: '2026-11-08T12:00:00Z', mutation_id: '00000000-0000-0000-0000-000000000002', changed_fields: ['title'] };
const wire = (name: string, payload: unknown, cursor = 'stream_cursor') => `event: ${name}\nid: ${cursor}\ndata: ${JSON.stringify(payload)}\n\n`;
const json = (value: unknown) => new Response(JSON.stringify(value));
let container: HTMLDivElement; let root: Root; let channels: ReadableStreamDefaultController<Uint8Array>[]; let fetch: ReturnType<typeof vi.fn>; let bootstrapCount: number;
let currentSnapshot: typeof snapshot;
let notificationTitle: string;
let notificationCount: number;
const notificationId = '00000000-0000-0000-0000-000000000099';
const notification = () => ({ id: notificationId, seq: '1', reason: 'assignment', created_at: '2026-11-08T12:00:00Z', read_at: null, archived_at: null, snoozed_until: null, version: '1', etag: '"notification:1"', source: { kind: 'issue', id: event.entity_id, team_id: 'team', available: true, title: notificationTitle, key: 'ENG-1' } });
beforeEach(() => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true); vi.useFakeTimers(); localStorage.clear(); history.replaceState({ otIndex: 0 }, '', '/w/workspace/issues/issue'); channels = []; bootstrapCount = 0; currentSnapshot = snapshot; notificationTitle = 'Initial private source'; notificationCount = 1;
  fetch = vi.fn(async (url: string) => {
    if (url.endsWith('/setup/status')) return json({ setup_required: false });
    if (url.endsWith('/auth/session')) return json(session);
    if (url.endsWith('/bootstrap')) { bootstrapCount++; return json(currentSnapshot); }
    if (url.endsWith('/access')) return json({ auth_epoch: currentSnapshot.auth_epoch, role: 'owner', archived: false });
    if (url.includes('/notifications?')) return json({ items: notificationCount ? [notification()] : [], next_cursor: null, next_snoozed_at: null, snapshot: 'signed-snapshot', unread_count: notificationCount });
    if (url.endsWith(`/issues/${event.entity_id}/document`)) return json({ id: 'document', plain_text: notificationTitle });
    if (url.endsWith(`/issues/${event.entity_id}`)) return json({ id: event.entity_id, key: 'ENG-1', title: notificationTitle });
    if (url.endsWith('/preferences')) return json({ shortcut_mode: 'standard' });
    if (url.endsWith('/events')) return new Response(new ReadableStream<Uint8Array>({ start(controller) { channels.push(controller); } }), { headers: { 'Content-Type': 'text/event-stream' } });
    throw new Error('Unexpected fixture request.');
  }); vi.stubGlobal('fetch', fetch);
  container = document.createElement('div'); document.body.append(container); root = createRoot(container);
});
afterEach(async () => { await act(async () => root.unmount()); container.remove(); vi.useRealTimers(); vi.unstubAllGlobals(); vi.restoreAllMocks(); });
async function render() { await act(async () => root.render(createElement(App))); }
async function send(value: string) { await act(async () => { channels.at(-1)!.enqueue(new TextEncoder().encode(value)); for (let index = 0; index < 10; index++) await Promise.resolve(); }); }
async function draft() {
  const input = container.querySelector<HTMLInputElement>('[aria-label="Issue title"]')!;
  await act(async () => { Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(input, 'Unsubmitted title draft'); input.dispatchEvent(new Event('input', { bubbles: true })); });
  return input;
}
it('coalesces list refresh while leaving the current draft editor mounted', async () => {
  await render(); const original = await draft(); await send(wire('invalidation', event)); await send(wire('invalidation', event)); await act(async () => vi.advanceTimersByTimeAsync(50));
  expect(container.querySelector('[data-refresh]')?.getAttribute('data-refresh')).toBe('1'); expect(container.querySelector('[aria-label="Issue title"]')).toBe(original); expect(original.value).toBe('Unsubmitted title draft'); expect(bootstrapCount).toBe(1);
});
it('recovers an expired replay cursor without discarding a draft or remounting its editor', async () => {
  await render(); const original = await draft(); currentSnapshot = { ...snapshot, realtime_cursor: 'fresh_snapshot_cursor' };
  await send(wire('reset_required', { reason: 'cursor_expired' }));
  expect(bootstrapCount).toBe(2); expect(channels).toHaveLength(2); expect(container.querySelector('[aria-label="Issue title"]')).toBe(original); expect(original.value).toBe('Unsubmitted title draft');
  const streamCalls = fetch.mock.calls.filter(([url]) => url.endsWith('/events')); expect(streamCalls).toHaveLength(2);
});
it('purges the editor when authority changes during replay recovery', async () => {
  await render(); const original = await draft(); currentSnapshot = { ...snapshot, auth_epoch: '2', realtime_cursor: 'new_authority_cursor' };
  await send(wire('reset_required', { reason: 'cursor_expired' }));
  expect(container.querySelector('[aria-label="Issue title"]')).not.toBe(original); expect(container.querySelector<HTMLInputElement>('[aria-label="Issue title"]')?.value).toBe('Saved title');
});
it('honestly stays refresh-only when the bootstrap has no realtime cursor', async () => {
  currentSnapshot = { ...snapshot, realtime_cursor: undefined }; await render(); expect(channels).toHaveLength(0); expect(container.textContent).toContain('Live updates are unavailable.');
});

it('uses one canonical stream for inbox, badge and preview refresh without duplicate fallback timers', async () => {
  history.replaceState({ otIndex: 0 }, '', '/w/workspace/inbox');
  const intervals = vi.spyOn(window, 'setInterval'); const cancelled = vi.spyOn(window, 'clearInterval');
  await render();
  expect(container.querySelector('.inbox-badge')?.textContent).toBe('1');
  expect(container.querySelector('.inbox-source')?.textContent).toContain('Initial private source');
  expect(intervals.mock.calls.filter(([, delay], index) => delay === 15_000 && !cancelled.mock.calls.some(([id]) => id === intervals.mock.results[index].value))).toHaveLength(0);
  notificationTitle = 'Current private source'; notificationCount = 2;
  await send(wire('invalidation', { ...event, event_id: notificationId, type: 'notification.created', entity_type: 'notification', entity_id: notificationId, entity_version: '1', changed_fields: [] }));
  await act(async () => vi.advanceTimersByTimeAsync(50));
  expect(container.querySelector('.inbox-badge')?.textContent).toBe('2');
  expect(container.querySelector('.inbox-list')?.textContent).toContain('Current private source');
  expect(container.querySelector('.inbox-source')?.textContent).toContain('Current private source');
});

it('refreshes renamed bootstrap metadata while retaining the current stream and dirty editor', async () => {
  await render(); const original = await draft();
  currentSnapshot = { ...snapshot, realtime_cursor: 'newer_snapshot_cursor', teams: [{ ...snapshot.teams[0], name: 'Renamed remote team' }], statuses: [{ id: 'status', team_id: 'team', name: 'Renamed remote status', category: 'started', color: 'blue', position: 1, is_default: true, archived_at: null, retiring_at: null, replacement_status_id: null, replacement_job_id: null }] };
  await send(wire('invalidation', { ...event, type: 'team.updated', entity_type: 'team' }, 'retained_stream_cursor'));
  await send(wire('invalidation', { ...event, event_id: notificationId, type: 'workflow.status_updated', entity_type: 'workflow_status' }, 'retained_stream_cursor'));
  await act(async () => vi.advanceTimersByTimeAsync(50));
  expect(container.querySelector('[aria-label="Workspace navigation"]')?.textContent).toContain('Renamed remote team');
  expect(container.querySelector('[data-status-labels]')?.getAttribute('data-status-labels')).toBe('Renamed remote status');
  expect(container.querySelector('[aria-label="Issue title"]')).toBe(original); expect(original.value).toBe('Unsubmitted title draft');
  expect(channels).toHaveLength(1); expect(bootstrapCount).toBe(2);
});

it('canonical remote-read invalidation clears row and badge despite delayed older list responses', async () => {
  history.replaceState({ otIndex: 0 }, '', '/w/workspace/inbox');
  const intervals = vi.spyOn(window, 'setInterval'); const cancelled = vi.spyOn(window, 'clearInterval');
  await render();
  expect(container.querySelector('.inbox-badge')?.textContent).toBe('1');
  expect(container.querySelector('[data-notification-id]')).not.toBeNull();
  const original = fetch.getMockImplementation() as (url: string, options: RequestInit) => Promise<Response>;
  const delayed: Array<(response: Response) => void> = [];
  let hold = true;
  fetch.mockImplementation((url: string, options: RequestInit) => url.includes('/notifications?') && hold ? new Promise<Response>(resolve => delayed.push(resolve)) : original(url, options));
  await send(wire('invalidation', { ...event, event_id: '00000000-0000-0000-0000-000000000021', type: 'notification.created', entity_type: 'notification', entity_id: notificationId, entity_version: '1', changed_fields: [] }));
  await act(async () => vi.advanceTimersByTimeAsync(50));
  expect(delayed).toHaveLength(2);
  hold = false; notificationCount = 0;
  await send(wire('invalidation', { ...event, event_id: '00000000-0000-0000-0000-000000000022', type: 'notification.updated', entity_type: 'notification', entity_id: notificationId, entity_version: '2', changed_fields: [] }));
  await act(async () => vi.advanceTimersByTimeAsync(50));
  expect(container.querySelector('[data-notification-id]')).toBeNull();
  expect(container.querySelector('.inbox-badge')).toBeNull();
  await act(async () => { for (const finish of delayed) finish(json({ items: [notification()], next_cursor: null, next_snoozed_at: null, snapshot: 'older-snapshot', unread_count: 1 })); });
  expect(container.querySelector('[data-notification-id]')).toBeNull();
  expect(container.querySelector('.inbox-badge')).toBeNull();
  expect(channels).toHaveLength(1);
  expect(intervals.mock.calls.filter(([, delay], index) => delay === 15_000 && !cancelled.mock.calls.some(([id]) => id === intervals.mock.results[index].value))).toHaveLength(0);
});
