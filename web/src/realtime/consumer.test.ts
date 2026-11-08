// @vitest-environment jsdom
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { startRealtime, reconnectDelay, type ScopedRequest } from './consumer';
import { canonicalEvent, EventDeduplicator } from './contract';
import { SseParser } from './sse';
import { request, currentSessionGeneration } from '../api';
import { workspaceWriteGuard } from './guard';

const event = (version = '1', id = '00000000-0000-0000-0000-000000000011', workspace = 'workspace') => ({ event_id: id, schema_version: 1, type: 'issue.updated', workspace_id: workspace, entity_type: 'issue', entity_id: '00000000-0000-0000-0000-000000000001', entity_version: version, actor_id: null, occurred_at: '2026-11-08T12:00:00Z', mutation_id: '00000000-0000-0000-0000-000000000002', changed_fields: ['title'] });
const wire = (value: unknown, cursor = 'cursor_next', name = 'invalidation') => `event: ${name}\nid: ${cursor}\ndata: ${JSON.stringify(value)}\n\n`;
function channel() {
  let writer!: ReadableStreamDefaultController<Uint8Array>; const cancel = vi.fn();
  const body = new ReadableStream<Uint8Array>({ start(value) { writer = value; }, cancel });
  return { response: new Response(body, { headers: { 'Content-Type': 'text/event-stream' } }), send: (value: string) => writer.enqueue(new TextEncoder().encode(value)), end: () => writer.close(), cancel };
}
const idleRequest: ScopedRequest = async () => { throw new Error('Native request used by browser.'); };
const consumers: ReturnType<typeof startRealtime>[] = [];
function start(overrides: Partial<Parameters<typeof startRealtime>[0]> = {}) {
  const callbacks = { invalidate: vi.fn(), reset: vi.fn(), status: vi.fn() };
  const control = startRealtime({ workspaceId: 'workspace', cursor: 'cursor_initial', native: false, request: idleRequest, current: () => true, ...callbacks, ...overrides });
  consumers.push(control); return { ...control, ...callbacks };
}
async function settle() { for (let i = 0; i < 8; i++) await Promise.resolve(); }
beforeEach(() => { vi.useFakeTimers(); vi.spyOn(navigator, 'onLine', 'get').mockReturnValue(true); vi.spyOn(Math, 'random').mockReturnValue(0.5); });
afterEach(() => { consumers.forEach(value => value.dispose()); consumers.length = 0; vi.useRealTimers(); vi.restoreAllMocks(); vi.unstubAllGlobals(); });

it('parses split CRLF frames and comments, bounds unfinished lines, and rejects NUL ids', () => {
  const emitted = vi.fn(); const parser = new SseParser(emitted);
  parser.push(': heartbeat\r'); parser.push('\n\r\nevent: checkpoint\rid: opaque\rdata: {}\r\r');
  expect(emitted).toHaveBeenCalledWith({ event: 'checkpoint', id: 'opaque', data: '{}' });
  expect(() => new SseParser(emitted).push('data: ' + 'x'.repeat(65_536))).toThrow('limit');
  expect(() => new SseParser(emitted).push('id: bad\0cursor\n')).toThrow('cursor');
});

it('deduplicates event IDs and stale versions without using timestamps as order', () => {
  const dedup = new EventDeduplicator();
  expect(dedup.accept(canonicalEvent(event('2'), 'workspace'))).toBe(true);
  expect(dedup.accept(canonicalEvent(event('3'), 'workspace'))).toBe(false);
  expect(dedup.accept(canonicalEvent(event('1', '00000000-0000-0000-0000-000000000012'), 'workspace'))).toBe(false);
  expect(dedup.accept(canonicalEvent(event('3', '00000000-0000-0000-0000-000000000013'), 'workspace'))).toBe(true);
  expect(() => canonicalEvent(event('03'), 'workspace')).toThrow();
  expect(() => canonicalEvent(event('9223372036854775808'), 'workspace')).toThrow();
  expect(() => canonicalEvent(event('1'), 'other')).toThrow();
});

it('coalesces invalidations and reconnects with the latest hidden checkpoint cursor', async () => {
  const first = channel(); const second = channel(); const fetch = vi.fn().mockResolvedValueOnce(first.response).mockResolvedValueOnce(second.response); vi.stubGlobal('fetch', fetch);
  const consumer = start(); await settle();
  expect(fetch.mock.calls[0][1]).toMatchObject({ credentials: 'same-origin', headers: { 'Last-Event-ID': 'cursor_initial' } });
  first.send(wire(event())); first.send(wire(event())); first.send(wire(event('2', '00000000-0000-0000-0000-000000000012'))); first.send(wire({}, 'checkpoint_only', 'checkpoint'));
  await settle(); await vi.advanceTimersByTimeAsync(50); expect(consumer.invalidate).toHaveBeenCalledTimes(1);
  first.end(); await settle(); expect(consumer.status).toHaveBeenLastCalledWith('reconnecting');
  await vi.advanceTimersByTimeAsync(1000); expect(fetch.mock.calls[1][1].headers['Last-Event-ID']).toBe('checkpoint_only');
  expect(consumer.status).toHaveBeenLastCalledWith('connected'); expect(consumer.reset).not.toHaveBeenCalled();
});

it('bounds jittered retries and uses a 45 second traffic watchdog', async () => {
  expect(reconnectDelay(0, () => 0)).toBe(750); expect(reconnectDelay(8, () => 1)).toBe(30_000);
  const first = channel(); const fetch = vi.fn().mockResolvedValueOnce(first.response).mockRejectedValue(new TypeError('offline')); vi.stubGlobal('fetch', fetch);
  const consumer = start(); await settle();
  await vi.advanceTimersByTimeAsync(40_000); first.send(': heartbeat\n\n'); await settle();
  await vi.advanceTimersByTimeAsync(44_999); expect(fetch.mock.calls[0][1].signal.aborted).toBe(false);
  await vi.advanceTimersByTimeAsync(1); expect(fetch.mock.calls[0][1].signal.aborted).toBe(true);
  await vi.advanceTimersByTimeAsync(1000); expect(fetch).toHaveBeenCalledTimes(2);
  expect(consumer.status).toHaveBeenLastCalledWith('reconnecting'); expect(consumer.reset).not.toHaveBeenCalled();
});

it('drops late pages after a session switch and disposes pending invalidations', async () => {
  let finish!: (value: unknown) => void;
  const native = vi.fn(() => new Promise(resolve => { finish = resolve; })); let current = true;
  const consumer = start({ native: true, request: native as ScopedRequest, current: () => current });
  current = false; consumer.dispose(); finish({ events: [event()], cursor: 'new', has_more: false }); await settle(); await vi.advanceTimersByTimeAsync(100);
  expect(consumer.invalidate).not.toHaveBeenCalled(); expect(consumer.reset).not.toHaveBeenCalled(); expect(native).toHaveBeenCalledTimes(1);
});

it('polls native bounded pages using scoped requests only, including checkpoint-only pages', async () => {
  const native = vi.fn().mockResolvedValueOnce({ events: [event()], cursor: 'next/+=opaque', has_more: true }).mockResolvedValue({ events: [], cursor: 'checkpoint', has_more: false });
  const fetch = vi.fn(); vi.stubGlobal('fetch', fetch); const consumer = start({ native: true, request: native as ScopedRequest }); await settle(); await vi.advanceTimersByTimeAsync(50);
  expect(native.mock.calls[0][0]).toBe('/workspaces/workspace/events/page?cursor=cursor_initial&limit=100');
  expect(native.mock.calls[1][0]).toContain('cursor=next%2F%2B%3Dopaque'); expect(consumer.invalidate).toHaveBeenCalledTimes(1); expect(fetch).not.toHaveBeenCalled();
  await vi.advanceTimersByTimeAsync(2000); expect(native.mock.calls[2][0]).toContain('cursor=checkpoint');
});

it('rejects oversized native pages and retries after 15 seconds without consuming their cursor', async () => {
  const native = vi.fn().mockResolvedValue({ events: Array.from({ length: 101 }, () => event()), cursor: 'must_not_advance', has_more: true });
  const consumer = start({ native: true, request: native as ScopedRequest }); await settle();
  expect(consumer.status).toHaveBeenLastCalledWith('reconnecting'); expect(consumer.invalidate).not.toHaveBeenCalled();
  await vi.advanceTimersByTimeAsync(15_000); expect(native).toHaveBeenCalledTimes(2); expect(native.mock.calls[1][0]).toContain('cursor=cursor_initial');
});

it.each(['cursor_expired', 'access_changed', 'scope_changed', 'reset_required'] as const)('handles pre-stream %s resets without reconnecting the stale cursor', async code => {
  const fetch = vi.fn().mockResolvedValue(new Response(JSON.stringify({ code }), { status: 409 })); vi.stubGlobal('fetch', fetch);
  const consumer = start(); await settle(); expect(consumer.reset).toHaveBeenCalledWith(code);
  await vi.advanceTimersByTimeAsync(60_000); expect(fetch).toHaveBeenCalledTimes(1);
});

it('accepts named reset frames and ignores late frames after disposal', async () => {
  const first = channel(); vi.stubGlobal('fetch', vi.fn().mockResolvedValue(first.response)); const consumer = start(); await settle();
  first.send(wire(event())); first.send(wire({ reason: 'access_changed' }, '', 'reset_required')); first.send(wire(event('2', '00000000-0000-0000-0000-000000000012'))); await settle(); await vi.advanceTimersByTimeAsync(100);
  expect(consumer.reset).toHaveBeenCalledTimes(1); expect(consumer.invalidate).not.toHaveBeenCalled();
});

it('guards only current workspace writes while allowing drafts, reads and structured queries', async () => {
  const guard = workspaceWriteGuard('workspace', currentSessionGeneration()); guard.set(true);
  const fetch = vi.fn().mockImplementation(() => Promise.resolve(new Response('{}'))); vi.stubGlobal('fetch', fetch);
  try {
    await expect(request('/workspaces/workspace/issues/issue', { method: 'PATCH', body: { title: 'draft' } })).rejects.toMatchObject({ status: 409, code: 'realtime_offline' });
    expect(fetch).not.toHaveBeenCalled();
    await request('/workspaces/workspace/issues'); await request('/workspaces/workspace/issues/query', { method: 'POST', body: {} }); await request('/workspaces/another/issues', { method: 'POST', body: {} });
    expect(fetch).toHaveBeenCalledTimes(3);
    guard.release(); await request('/workspaces/workspace/issues', { method: 'POST', body: {} }); expect(fetch).toHaveBeenCalledTimes(4);
  } finally { guard.release(); }
});

it('uses the degraded 15 second refresh fallback during an outage', async () => {
  const fetch = vi.fn().mockRejectedValue(new TypeError('Network unavailable.')); vi.stubGlobal('fetch', fetch); const consumer = start(); await settle();
  expect(consumer.status).toHaveBeenLastCalledWith('reconnecting');
  await vi.advanceTimersByTimeAsync(15_050); expect(consumer.invalidate).toHaveBeenCalledTimes(1);
  expect(consumer.status.mock.calls.some(([status]) => status === 'degraded')).toBe(true);
});

it('rejects unsupported event schemas without advancing replay, and drops late browser frames', async () => {
  const first = channel(); const second = channel(); const fetch = vi.fn().mockResolvedValueOnce(first.response).mockResolvedValueOnce(second.response); vi.stubGlobal('fetch', fetch); let active = true;
  const consumer = start({ current: () => active }); await settle(); first.send(wire({ ...event(), schema_version: 2 }, 'invalid_cursor')); await settle();
  await vi.advanceTimersByTimeAsync(1000); expect(fetch.mock.calls[1][1].headers['Last-Event-ID']).toBe('cursor_initial');
  active = false; second.send(wire(event())); await settle(); await vi.advanceTimersByTimeAsync(100); expect(consumer.invalidate).not.toHaveBeenCalled(); expect(consumer.reset).not.toHaveBeenCalled();
});
