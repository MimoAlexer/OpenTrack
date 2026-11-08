import { ApiError } from '../api';
import { canonicalEvent, cursorValue, EventDeduplicator, resetReason, type ResetReason } from './contract';
import { SseParser, type Frame } from './sse';

export type RealtimeStatus = 'unavailable' | 'connecting' | 'connected' | 'reconnecting' | 'degraded' | 'offline' | 'resetting';
export type ScopedRequest = <T>(path: string, options?: { signal?: AbortSignal }) => Promise<T>;
type Options = {
  workspaceId: string; cursor: string; native: boolean; request: ScopedRequest;
  current: () => boolean; invalidate: () => void; reset: (reason: ResetReason) => void;
  status: (status: RealtimeStatus) => void;
};

export function reconnectDelay(attempt: number, random = Math.random): number {
  const base = [1000, 2000, 4000, 8000, 15000, 30000][Math.min(attempt, 5)];
  return Math.min(30_000, Math.round(base * (0.75 + random() * 0.5)));
}

/** One bounded consumer per mounted workspace. All callbacks are fenced on disposal. */
export function startRealtime(options: Options) {
  const lifetime = new AbortController(); const dedup = new EventDeduplicator();
  let cursor = options.cursor; let attempt = 0; let stopped = false; let transport: AbortController | null = null;
  let retryTimer: ReturnType<typeof setTimeout> | undefined; let flushTimer: ReturnType<typeof setTimeout> | undefined;
  let trafficTimer: ReturnType<typeof setTimeout> | undefined; let degradedTimer: ReturnType<typeof setInterval> | undefined;
  let activeReader: ReadableStreamDefaultReader<Uint8Array> | null = null;
  const current = () => !stopped && !lifetime.signal.aborted && options.current();
  const setStatus = (status: RealtimeStatus) => { if (current()) options.status(status); };
  function flush() {
    if (flushTimer !== undefined || !current()) return;
    flushTimer = setTimeout(() => { flushTimer = undefined; if (current()) options.invalidate(); }, 50);
  }
  function dispose() {
    stopped = true; lifetime.abort(); abortTransport(); clearTimeout(retryTimer); clearTimeout(flushTimer); clearTimeout(trafficTimer); clearInterval(degradedTimer);
    window.removeEventListener('offline', offline); window.removeEventListener('online', online);
  }
  function reset(reason: ResetReason) {
    if (!current()) return;
    setStatus('resetting'); dispose(); options.reset(reason);
  }
  function event(value: unknown) {
    const record = canonicalEvent(value, options.workspaceId);
    if (record.type === 'access.changed') { reset('access_changed'); return; }
    if (dedup.accept(record)) flush();
  }
  function frame(value: Frame) {
    if (!current()) return;
    if (value.event === 'reset_required') {
      const payload = JSON.parse(value.data); if (!resetReason(payload?.reason)) throw new Error('Invalid realtime reset.');
      reset(payload.reason); return;
    }
    if (!['invalidation', 'checkpoint'].includes(value.event)) throw new Error('Unsupported realtime frame.');
    if (!cursorValue(value.id)) throw new Error('Invalid realtime cursor.');
    const payload: unknown = JSON.parse(value.data);
    if (value.event === 'invalidation') event(payload);
    else if (!payload || typeof payload !== 'object' || Array.isArray(payload) || Object.keys(payload).length) throw new Error('Invalid realtime checkpoint.');
    if (current()) cursor = value.id;
  }
  function disconnected() {
    if (!current()) return;
    setStatus(navigator.onLine === false ? 'offline' : 'reconnecting');
    if (degradedTimer === undefined) degradedTimer = setInterval(() => {
      if (!current()) return;
      setStatus(navigator.onLine === false ? 'offline' : 'degraded');
      if (navigator.onLine !== false) flush();
    }, 15_000);
  }
  function connected() {
    attempt = 0; clearInterval(degradedTimer); degradedTimer = undefined; setStatus('connected');
  }
  function schedule(delay: number) { clearTimeout(retryTimer); if (current()) retryTimer = setTimeout(() => { void connect(); }, delay); }
  function abortTransport() { const previous = transport; transport = null; previous?.abort(); void activeReader?.cancel().catch(() => {}); activeReader = null; }
  function offline() { if (!current()) return; abortTransport(); disconnected(); schedule(15_000); }
  function online() { if (!current()) return; clearTimeout(retryTimer); abortTransport(); schedule(0); }
  function touch(abort: AbortController) {
    clearTimeout(trafficTimer); trafficTimer = setTimeout(() => { abort.abort(); void activeReader?.cancel().catch(() => {}); }, 45_000);
  }
  async function browser(abort: AbortController) {
    touch(abort);
    const response = await fetch(`/api/v1/workspaces/${encodeURIComponent(options.workspaceId)}/events`, {
      method: 'GET', credentials: 'same-origin', headers: { Accept: 'text/event-stream', 'Last-Event-ID': cursor }, signal: abort.signal,
    });
    if (!current() || abort.signal.aborted) {
      await response.body?.cancel();
      if (!current()) return;
      throw new DOMException('Realtime request was interrupted.', 'AbortError');
    }
    if (response.status === 409) {
      // Error responses are bounded too: do not parse an arbitrary proxy body.
      const reader = response.body?.getReader(); activeReader = reader ?? null; let bytes = 0; let body = ''; const decoder = new TextDecoder();
      try {
        if (!reader) throw new Error('Missing reset response.');
        for (;;) { const chunk = await reader.read(); if (!current()) return; if (abort.signal.aborted) throw new DOMException('Realtime request was interrupted.', 'AbortError'); if (chunk.done) break; bytes += chunk.value.byteLength; if (bytes > 16_384) throw new Error('Oversized reset response.'); body += decoder.decode(chunk.value, { stream: true }); }
        const payload = JSON.parse(body + decoder.decode()); if (!resetReason(payload?.code)) throw new Error('Invalid realtime reset.'); reset(payload.code);
      } finally { await reader?.cancel().catch(() => {}); if (activeReader === reader) activeReader = null; }
      return;
    }
    if ([401, 403, 404].includes(response.status)) { await response.body?.cancel(); reset('access_changed'); return; }
    if (!response.ok || response.headers.get('Content-Type')?.split(';')[0].trim() !== 'text/event-stream' || !response.body) { await response.body?.cancel(); throw new Error('Realtime stream is unavailable.'); }
    const reader = response.body.getReader(); const decoder = new TextDecoder('utf-8', { fatal: true }); const parser = new SseParser(frame);
    activeReader = reader;
    connected(); touch(abort);
    try {
      for (;;) {
        const chunk = await reader.read(); if (!current()) return; if (abort.signal.aborted) throw new DOMException('Realtime request was interrupted.', 'AbortError');
        if (chunk.done) throw new Error('Realtime stream ended.');
        touch(abort);
        // Bound work per chunk even when a custom transport emits a large buffer.
        for (let offset = 0; offset < chunk.value.byteLength && current(); offset += 16_384) parser.push(decoder.decode(chunk.value.subarray(offset, offset + 16_384), { stream: true }));
      }
    } finally { await reader.cancel().catch(() => {}); if (activeReader === reader) activeReader = null; }
  }
  async function native(abort: AbortController) {
    const page = await options.request<{ events: unknown[]; cursor: string; has_more: boolean }>(`/workspaces/${options.workspaceId}/events/page?cursor=${encodeURIComponent(cursor)}&limit=100`, { signal: abort.signal });
    if (!current()) return; if (abort.signal.aborted) throw new DOMException('Realtime request was interrupted.', 'AbortError');
    if (!page || !Array.isArray(page.events) || page.events.length > 100 || !cursorValue(page.cursor) || typeof page.has_more !== 'boolean'
      || ((page.has_more || page.events.length > 0) && page.cursor === cursor) || JSON.stringify(page).length > 1024 * 1024) throw new Error('Invalid realtime page.');
    const records = page.events.map(value => { if (JSON.stringify(value).length > 64 * 1024) throw new Error('Oversized realtime event.'); return canonicalEvent(value, options.workspaceId); });
    for (const record of records) { if (!current()) return; event(record); }
    if (!current()) return;
    cursor = page.cursor; connected(); schedule(page.has_more ? 0 : 2000);
  }
  async function connect() {
    if (!current()) return;
    if (navigator.onLine === false) { disconnected(); schedule(15_000); return; }
    const abort = new AbortController(); transport?.abort(); transport = abort;
    try { if (options.native) await native(abort); else await browser(abort); }
    catch (error) {
      if (!current() || transport !== abort) return;
      if (error instanceof ApiError && (resetReason(error.code) || [401, 403, 404].includes(error.status))) { reset(resetReason(error.code) ? error.code : 'access_changed'); return; }
      disconnected(); schedule(options.native ? 15_000 : reconnectDelay(attempt++));
    } finally { if (transport === abort) clearTimeout(trafficTimer); }
  }
  window.addEventListener('offline', offline); window.addEventListener('online', online);
  setStatus('connecting'); void connect();
  return { dispose, retry: () => { if (!current()) return; abortTransport(); schedule(0); } };
}
