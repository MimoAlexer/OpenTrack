import { useCallback, useEffect, useRef, useState } from 'react';
import { ApiError } from '../api';
import type { Session } from '../types';
import { invalidateNotifications, useInboxInvalidation, useSnoozeDeadline } from './invalidation';
import { isAborted, useNotificationScope } from './scope';
import { notificationPath, safeSource, type InboxFilter, type NotificationAction, type NotificationItem, type NotificationPage } from './types';

export function useInbox(workspaceId: string, session: Session, accessScope: string, filter: InboxFilter, refresh: number, externallyRefreshed = false) {
  const scope = useNotificationScope(workspaceId, session, accessScope);
  const invalidation = useInboxInvalidation(workspaceId, !externallyRefreshed); const queryKey = `${scope.key}:${filter}`;
  const [page, setPage] = useState<{ key: string; value: NotificationPage } | null>(null);
  const [loading, setLoading] = useState(true); const [error, setError] = useState<unknown>(null);
  const sequence = useRef(0); const latestQuery = useRef(queryKey); latestQuery.current = queryKey;
  const deniedSources = useRef(new Map<string, number>()); const denialRevision = useRef(0);
  const completedRead = useRef(0); const loadedWindow = useRef({ key: queryKey, pages: 1 });
  if (loadedWindow.current.key !== queryKey) loadedWindow.current = { key: queryKey, pages: 1 };
  const sourceContext = (source: NotificationItem['source']) => JSON.stringify([scope.key, source.kind, source.id]);
  function deny(source: NotificationItem['source']) {
    deniedSources.current.set(sourceContext(source), ++denialRevision.current);
  }
  const latestPage = useRef(page); latestPage.current = page;
  const [retry, setRetry] = useState(0);
  const current = useCallback(() => scope.current() && latestQuery.current === queryKey, [scope.current, queryKey]);
  const load = useCallback(async (cursor?: string, signal?: AbortSignal) => {
    const token = ++sequence.current; const startedAfterDenial = denialRevision.current; setLoading(true); setError(null);
    try {
      const params = new URLSearchParams({ filter, limit: '50', ...(cursor ? { cursor } : {}) });
      let value = await scope.read<NotificationPage>(`${notificationPath(workspaceId)}?${params}`, { signal });
      // Revalidate at most four previously loaded pages with fresh signed
      // cursors. A selection beyond this bounded window becomes non-actionable.
      if (!cursor) for (let index = 1; index < Math.min(loadedWindow.current.pages, 4) && value.next_cursor; index++) {
        const next = new URLSearchParams({ filter, limit: '50', cursor: value.next_cursor });
        const part = await scope.read<NotificationPage>(`${notificationPath(workspaceId)}?${next}`, { signal });
        value = { ...value, items: [...value.items, ...part.items], next_cursor: part.next_cursor };
      }
      if (signal?.aborted || token !== sequence.current || latestQuery.current !== queryKey) return;
      completedRead.current = token;
      if (cursor) loadedWindow.current.pages += 1;
      // List denials carry the same authority as a denied source preview.
      for (const item of value.items) if (!item.source.available) deny(item.source);
      const safeItems = value.items.map(item => {
        const sourceKey = sourceContext(item.source); const deniedAt = deniedSources.current.get(sourceKey);
        // Only an authorized read begun after a denial may restore its labels.
        // A request already in flight when the preview was denied stays scrubbed.
        if (!item.source.available || deniedAt !== undefined && deniedAt > startedAfterDenial) return safeSource(item);
        deniedSources.current.delete(sourceKey); return item;
      });
      setPage(previous => ({ key: queryKey, value: { ...value, items: cursor && previous?.key === queryKey ? [...previous.value.items, ...safeItems.filter(item => !previous.value.items.some(old => old.id === item.id))] : safeItems } }));
    } catch (error) {
      if (signal?.aborted || !scope.current() || token !== sequence.current || latestQuery.current !== queryKey || isAborted(error)) return;
      setError(error); if (error instanceof ApiError && [401, 403, 404, 410].includes(error.status)) setPage(null);
    } finally { if (!signal?.aborted && scope.current() && token === sequence.current && latestQuery.current === queryKey) setLoading(false); }
  }, [filter, workspaceId, queryKey, scope.read, scope.current]);
  useEffect(() => {
    const abort = new AbortController(); void load(undefined, abort.signal);
    return () => abort.abort();
  }, [load, refresh, retry, invalidation]);
  const value = page?.key === queryKey ? page.value : null;
  useSnoozeDeadline(workspaceId, queryKey, value, current);
  function replaceItems(update: (items: NotificationItem[]) => NotificationItem[]) {
    setPage(previous => previous?.key === queryKey ? { ...previous, value: { ...previous.value, items: update(previous.value.items).map(item => deniedSources.current.has(sourceContext(item.source)) || !item.source.available ? safeSource(item) : item) } } : previous);
  }
  async function mutate(item: NotificationItem, action: NotificationAction, until?: string) {
    const readAtStart = completedRead.current;
    const result = await scope.read<NotificationItem>(notificationPath(workspaceId, item.id), { method: 'PATCH', body: { action, ...(until ? { until } : {}) }, csrf: session.csrf_token, headers: { 'If-Match': item.etag } });
    if (latestQuery.current === queryKey && completedRead.current === readAtStart) replaceItems(items => items.map(old => old.id === item.id && BigInt(old.version) <= BigInt(result.version) ? result : old));
    invalidateNotifications(workspaceId);
    return result;
  }
  async function markRead(snapshot: string) {
    // Every batch retains the originally reviewed snapshot, including retries.
    for (let batch = 0; batch < 100; batch += 1) {
      const result = await scope.read<{ updated_count: number; unread_count: number; has_more: boolean }>(`${notificationPath(workspaceId)}/mark-read`, { method: 'POST', body: { snapshot }, csrf: session.csrf_token });
      if (!result.has_more) { invalidateNotifications(workspaceId); return; }
    }
    invalidateNotifications(workspaceId);
    throw new Error('There are more earlier notifications to mark read. Try again to continue.');
  }
  return {
    value, loading: !value && latestPage.current?.key !== queryKey || loading, error, current, scopeKey: queryKey,
    reload: () => setRetry(value => value + 1), more: () => { if (!loading && value?.next_cursor) void load(value.next_cursor); },
    replaceItems, mutate, markRead,
    reconciliationToken: () => completedRead.current,
    rollback: (item: NotificationItem, readAtStart: number) => {
      // A completed later list is authoritative even when the personal version
      // stayed equal or the item moved outside the loaded/filter window.
      if (completedRead.current !== readAtStart) { setRetry(value => value + 1); return; }
      replaceItems(items => {
        const existing = items.find(value => value.id === item.id);
        if (existing && BigInt(existing.version) > BigInt(item.version)) return items;
        return [...items.filter(value => value.id !== item.id), item].sort((a, b) => BigInt(a.seq) > BigInt(b.seq) ? -1 : BigInt(a.seq) < BigInt(b.seq) ? 1 : 0);
      });
    },
    unavailable: (source: NotificationItem['source']) => { deny(source); replaceItems(items => items); },
  };
}
