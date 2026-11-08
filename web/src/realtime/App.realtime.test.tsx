// @vitest-environment jsdom
import { act, createElement, useState } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { App } from '../App';
import type { Bootstrap, Session } from '../types';

vi.mock('../issues/IssueList', () => ({ IssueList: ({ refresh }: { refresh: number }) => createElement('div', { 'data-refresh': refresh }, 'Issue list') }));
vi.mock('../issues/IssueEditor', () => ({ IssueEditor: () => { const [draft, setDraft] = useState('Saved title'); return createElement('input', { 'aria-label': 'Issue title', value: draft, onChange: (event: { target: HTMLInputElement }) => setDraft(event.target.value) }); } }));
vi.mock('../useWorkspaceAccess', () => ({ useWorkspaceAccess: () => {} }));
const session: Session = { audience: 'web', user: { id: 'owner', display_name: 'Example Owner', email: 'owner@example.test' }, csrf_token: 'synthetic-csrf', workspaces: [{ id: 'workspace', name: 'Workspace', slug: 'workspace', role: 'owner' }] };
const snapshot: Bootstrap & { realtime_cursor?: string } = { auth_epoch: '1', realtime_cursor: 'snapshot_cursor', teams: [{ id: 'team', name: 'Example team', key: 'ENG', private: false, archived_at: null }], statuses: [], members: [], issues: [] };
const event = { event_id: '00000000-0000-0000-0000-000000000011', schema_version: 1, type: 'issue.updated', workspace_id: 'workspace', entity_type: 'issue', entity_id: '00000000-0000-0000-0000-000000000001', entity_version: '2', actor_id: null, occurred_at: '2026-11-08T12:00:00Z', mutation_id: '00000000-0000-0000-0000-000000000002', changed_fields: ['title'] };
const wire = (name: string, payload: unknown, cursor = 'stream_cursor') => `event: ${name}\nid: ${cursor}\ndata: ${JSON.stringify(payload)}\n\n`;
const json = (value: unknown) => new Response(JSON.stringify(value));
let container: HTMLDivElement; let root: Root; let channels: ReadableStreamDefaultController<Uint8Array>[]; let fetch: ReturnType<typeof vi.fn>; let bootstrapCount: number;
let currentSnapshot: typeof snapshot;
beforeEach(() => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true); vi.useFakeTimers(); localStorage.clear(); history.replaceState({ otIndex: 0 }, '', '/w/workspace/issues/issue'); channels = []; bootstrapCount = 0; currentSnapshot = snapshot;
  fetch = vi.fn(async (url: string) => {
    if (url.endsWith('/setup/status')) return json({ setup_required: false });
    if (url.endsWith('/auth/session')) return json(session);
    if (url.endsWith('/bootstrap')) { bootstrapCount++; return json(currentSnapshot); }
    if (url.endsWith('/access')) return json({ auth_epoch: currentSnapshot.auth_epoch, role: 'owner', archived: false });
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
