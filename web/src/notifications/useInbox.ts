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
  const listDenials = useRef(new Map<string, number>());
  const entityDenials = useRef(new Map<string, number>());
  const previewDenials = useRef(new Map<string, number>());
  const denialRevision = useRef(0); const readRevision = useRef(0);
  const loadedWindow = useRef({ key: queryKey, pages: 1 });
  if (loadedWindow.current.key !== queryKey) loadedWindow.current = { key: queryKey, pages: 1 };
  const entityContext = (source: NotificationItem['source']) => JSON.stringify([scope.key, source.kind, source.id]);
  const previewContext = (source: NotificationItem['source']) => JSON.stringify([entityContext(source), source.document_id, source.comment_id]);
  const notificationContext = (item: NotificationItem) => JSON.stringify([entityContext(item.source), item.id]);
  function sanitize(item: NotificationItem) {
    return !item.source.available || listDenials.current.has(notificationContext(item)) || entityDenials.current.has(entityContext(item.source)) || previewDenials.current.has(previewContext(item.source)) ? safeSource(item) : item;
  }
  const latestPage = useRef(page); latestPage.current = page;
  const [retry, setRetry] = useState(0);
  const current = useCallback(() => scope.current() && latestQuery.current === queryKey, [scope.current, queryKey]);
  const load = useCallback(async (cursor?: string, signal?: AbortSignal) => {
    const token = ++sequence.current; const startedAfterDenial = denialRevision.current; setLoading(true); setError(null);
    try {
      const params = new URLSearchParams({ filter, limit: '50', ...(cursor ? { cursor } : {}) });
      const observeAuthority = (part: NotificationPage) => {
        if (signal?.aborted || token !== sequence.current || latestQuery.current !== queryKey || !scope.current()) return false;
        readRevision.current += 1;
        for (const item of part.items) {
          if (!item.source.available) listDenials.current.set(notificationContext(item), ++denialRevision.current);
          else for (const [decisions, key] of [
            [listDenials.current, notificationContext(item)],
            [entityDenials.current, entityContext(item.source)],
            [previewDenials.current, previewContext(item.source)],
          ] as const) {
            const deniedAt = decisions.get(key);
            if (deniedAt !== undefined && deniedAt <= startedAfterDenial) decisions.delete(key);
          }
        }
        // Privacy decisions take effect as soon as a fenced page succeeds. A
        // later page failure cannot discard a delivered denial. The refreshed
        // window and selection remain staged until all bounded pages succeed.
        setPage(previous => previous?.key === queryKey ? { ...previous, value: { ...previous.value, items: previous.value.items.map(sanitize) } } : previous);
        return true;
      };
      let value = await scope.read<NotificationPage>(`${notificationPath(workspaceId)}?${params}`, { signal });
      if (!observeAuthority(value)) return;
      // Revalidate at most four previously loaded pages with fresh signed
      // cursors. A selection beyond this bounded window becomes non-actionable.
      if (!cursor) for (let index = 1; index < Math.min(loadedWindow.current.pages, 4) && value.next_cursor; index++) {
        const next = new URLSearchParams({ filter, limit: '50', cursor: value.next_cursor });
        const part = await scope.read<NotificationPage>(`${notificationPath(workspaceId)}?${next}`, { signal });
        if (!observeAuthority(part)) return;
        value = { ...value, items: [...value.items, ...part.items], next_cursor: part.next_cursor };
      }
      if (signal?.aborted || token !== sequence.current || latestQuery.current !== queryKey) return;
      if (cursor) loadedWindow.current.pages += 1;
      const safeItems = value.items.map(sanitize);
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
    setPage(previous => previous?.key === queryKey ? { ...previous, value: { ...previous.value, items: update(previous.value.items).map(sanitize) } } : previous);
  }
  async function mutate(item: NotificationItem, action: NotificationAction, until?: string) {
    const readAtStart = readRevision.current;
    const result = await scope.read<NotificationItem>(notificationPath(workspaceId, item.id), { method: 'PATCH', body: { action, ...(until ? { until } : {}) }, csrf: session.csrf_token, headers: { 'If-Match': item.etag } });
    if (latestQuery.current === queryKey && readRevision.current === readAtStart) replaceItems(items => items.map(old => old.id === item.id && BigInt(old.version) <= BigInt(result.version) ? result : old));
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
    reconciliationToken: () => readRevision.current,
    rollback: (item: NotificationItem, readAtStart: number) => {
      // A completed later list is authoritative even when the personal version
      // stayed equal or the item moved outside the loaded/filter window.
      if (readRevision.current !== readAtStart) { setRetry(value => value + 1); return; }
      replaceItems(items => {
        const existing = items.find(value => value.id === item.id);
        if (existing && BigInt(existing.version) > BigInt(item.version)) return items;
        return [...items.filter(value => value.id !== item.id), item].sort((a, b) => BigInt(a.seq) > BigInt(b.seq) ? -1 : BigInt(a.seq) < BigInt(b.seq) ? 1 : 0);
      });
    },
    unavailable: (source: NotificationItem['source'], kind: 'entity' | 'preview' = 'entity') => {
      const decisions = kind === 'entity' ? entityDenials.current : previewDenials.current;
      decisions.set(kind === 'entity' ? entityContext(source) : previewContext(source), ++denialRevision.current);
      replaceItems(items => items);
    },
  };
}
