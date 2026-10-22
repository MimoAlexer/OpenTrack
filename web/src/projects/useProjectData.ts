import { useEffect, useRef, useState } from 'react';
import { ApiError, request } from '../api';
import { isAccessError } from '../documents/model';
import type { Page } from './types';

// Refreshes keep the last confirmed rendering in place. A changed scope never
// reuses it, and denied reads remove it before the error is displayed.
export function useProjectData<T>(path: string, refresh = 0) {
  const [result, setResult] = useState<{ path: string; value: T } | null>(null);
  const [error, setError] = useState<unknown>(null);
  const [loading, setLoading] = useState(true); const [attempt, setAttempt] = useState(0);
  const generation = useRef(0);
  useEffect(() => {
    const abort = new AbortController(); const token = ++generation.current;
    setLoading(true); setError(null);
    request<T>(path, { signal: abort.signal }).then(value => {
      if (!abort.signal.aborted && generation.current === token) setResult({ path, value });
    }).catch(error => { if (!abort.signal.aborted && generation.current === token) { setError(error); if (isAccessError(error)) setResult(null); } })
      .finally(() => { if (!abort.signal.aborted && generation.current === token) setLoading(false); });
    return () => abort.abort();
  }, [path, refresh, attempt]);
  return { value: result?.path === path ? result.value : null, loading, error, retry: () => setAttempt(value => value + 1), replace: (value: T) => { generation.current += 1; setResult({ path, value }); setLoading(false); setError(null); } };
}

export function useProjectPage<T extends { id: string }>(path: string, refresh = 0, limit = 25) {
  const [page, setPage] = useState<{ path: string; value: Page<T> } | null>(null);
  const [cursor, setCursor] = useState<{ path: string; value: string | null }>({ path, value: null });
  const [loading, setLoading] = useState(true); const [error, setError] = useState<unknown>(null);
  const [attempt, setAttempt] = useState(0); const generation = useRef(0);
  const previousRefresh = useRef(refresh);
  const nextCursor = cursor.path === path && previousRefresh.current === refresh ? cursor.value : null;
  useEffect(() => {
    if (previousRefresh.current !== refresh) { previousRefresh.current = refresh; setCursor({ path, value: null }); }
    const abort = new AbortController(); const token = ++generation.current;
    setLoading(true); setError(null);
    const suffix = new URLSearchParams({ limit: String(limit), ...(nextCursor ? { cursor: nextCursor } : {}) });
    request<Page<T>>(`${path}${path.includes('?') ? '&' : '?'}${suffix}`, { signal: abort.signal }).then(value => {
      if (!abort.signal.aborted && generation.current === token) setPage(previous => ({ path, value: { ...value, data: nextCursor && previous?.path === path ? [...previous.value.data, ...value.data.filter(item => !previous.value.data.some(existing => existing.id === item.id))] : value.data } }));
    }).catch(error => { if (!abort.signal.aborted && generation.current === token) { setError(error); if (isAccessError(error)) setPage(null); } })
      .finally(() => { if (!abort.signal.aborted && generation.current === token) setLoading(false); });
    return () => abort.abort();
  }, [path, nextCursor, refresh, attempt, limit]);
  const value = page?.path === path ? page.value : null;
  return { value, error, loading, retry: () => { if (error instanceof ApiError && error.status === 410) setCursor({ path, value: null }); setAttempt(value => value + 1); }, more: () => { if (!loading && value?.has_more && value.next_cursor) setCursor({ path, value: value.next_cursor }); } };
}
