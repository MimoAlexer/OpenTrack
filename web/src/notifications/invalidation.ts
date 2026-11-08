import { useEffect, useState } from 'react';
import { ApiError } from '../api';
import type { Session } from '../types';
import { useNotificationScope } from './scope';
import { notificationPath, type NotificationPage } from './types';

const changeEvent = 'opentrack:notifications-changed';
export function invalidateNotifications(workspaceId: string) {
  window.dispatchEvent(new CustomEvent(changeEvent, { detail: { workspaceId } }));
  // This contains no count, title or recipient data. Other tabs reauthorize reads.
  try { localStorage.setItem('opentrack:inbox-refresh', JSON.stringify({ workspaceId, nonce: crypto.randomUUID() })); } catch { /* Storage is optional. */ }
}
export function useInboxInvalidation(workspaceId: string, fallbackPolling = true): number {
  const [revision, setRevision] = useState(0);
  useEffect(() => {
    const refresh = () => setRevision(value => value + 1);
    const changed = (event: Event) => { if (event instanceof CustomEvent && event.detail?.workspaceId === workspaceId) refresh(); };
    const visible = () => { if (document.visibilityState === 'visible') refresh(); };
    const stored = (event: StorageEvent) => { if (event.key !== 'opentrack:inbox-refresh' || !event.newValue) return; try { if (JSON.parse(event.newValue).workspaceId === workspaceId) refresh(); } catch { /* Ignore unrelated storage. */ } };
    window.addEventListener(changeEvent, changed); window.addEventListener('online', refresh); window.addEventListener('focus', refresh); window.addEventListener('storage', stored); document.addEventListener('visibilitychange', visible);
    // The integrated realtime owner supplies this fallback. Standalone callers
    // retain bounded reconciliation until that owner is mounted.
    const timer = fallbackPolling ? window.setInterval(visible, 15_000) : undefined;
    return () => { window.clearInterval(timer); window.removeEventListener(changeEvent, changed); window.removeEventListener('online', refresh); window.removeEventListener('focus', refresh); window.removeEventListener('storage', stored); document.removeEventListener('visibilitychange', visible); };
  }, [workspaceId, fallbackPolling]);
  return revision;
}

export function useSnoozeDeadline(workspaceId: string, scopeKey: string, page: { next_snoozed_at?: string | null } | null, current: () => boolean) {
  useEffect(() => {
    const deadline = page?.next_snoozed_at ? Date.parse(page.next_snoozed_at) : NaN;
    if (!Number.isFinite(deadline)) return;
    let timer: number | undefined; let stopped = false;
    const schedule = (initial: boolean) => {
      if (stopped || !current()) return;
      const remaining = deadline - Date.now();
      if (remaining > 0 || initial) {
        // Long snoozes never overflow setTimeout. A response from a slightly
        // slower server also cannot start an immediate refetch loop.
        timer = window.setTimeout(() => schedule(false), Math.min(3_600_000, Math.max(initial && remaining <= 0 ? 1_000 : 1, remaining)));
      } else if (document.visibilityState === 'visible') invalidateNotifications(workspaceId);
    };
    schedule(true);
    return () => { stopped = true; window.clearTimeout(timer); };
  }, [workspaceId, scopeKey, page, current]);
}

export function useUnreadNotifications(workspaceId: string, session: Session, accessScope: string, ready: boolean, refresh = 0, externallyRefreshed = false) {
  const scope = useNotificationScope(workspaceId, session, accessScope); const invalidation = useInboxInvalidation(workspaceId, !externallyRefreshed);
  const [result, setResult] = useState<{ key: string; count: number; next_snoozed_at?: string | null } | null>(null);
  useSnoozeDeadline(workspaceId, scope.key, ready && result?.key === scope.key ? result : null, scope.current);
  useEffect(() => {
    if (!ready) { setResult(null); return; }
    const abort = new AbortController();
    scope.read<NotificationPage>(`${notificationPath(workspaceId)}?filter=unread&limit=1`, { signal: abort.signal })
      .then(page => { if (!abort.signal.aborted && scope.current()) setResult({ key: scope.key, count: page.unread_count, next_snoozed_at: page.next_snoozed_at }); })
      .catch(error => { if (!abort.signal.aborted && scope.current() && error instanceof ApiError && [401, 403, 404].includes(error.status)) setResult(null); });
    return () => abort.abort();
  }, [scope.key, scope.read, scope.current, workspaceId, ready, refresh, invalidation]);
  return ready && result?.key === scope.key ? result.count : null;
}
