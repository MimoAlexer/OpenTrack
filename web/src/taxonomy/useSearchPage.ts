import { useEffect, useRef, useState } from 'react';
import { ApiError, request } from '../api';

function pageError(error: unknown) { return error instanceof ApiError && error.code === 'cursor_expired' ? new ApiError(error.status, error.code, 'This result page expired. Try again to reload the first page; your selections are preserved.', error.requestId, error.details) : error; }

export function useSearchPage<T extends { id: string }, P extends { data: T[]; next_cursor: string | null; has_more: boolean }>(path: string, sessionKey: string, query: string, enabled: boolean, delay = 240) {
  const [state, setState] = useState<{ key: string; items: T[]; page: P | null; error: unknown; loading: boolean }>({ key: '', items: [], page: null, error: null, loading: false });
  const [revision, setRevision] = useState(0);
  const active = useRef<{ key: string; abort: AbortController; loading: boolean } | null>(null);
  const key = `${sessionKey}:${path}:${query}`;
  useEffect(() => {
    const scope = { key, abort: new AbortController(), loading: false }; active.current = scope;
    setState({ key, items: [], page: null, error: null, loading: enabled });
    const timer = enabled ? setTimeout(() => { void load(scope); }, query ? delay : 0) : undefined;
    async function load(context: typeof scope) {
      context.loading = true;
      try {
        const page = await request<P>(`${path}${path.includes('?') ? '&' : '?'}limit=20&q=${encodeURIComponent(query.trim())}`, { signal: context.abort.signal });
        if (!context.abort.signal.aborted && active.current === context) setState({ key, items: page.data, page, error: null, loading: false });
      } catch (error) { if (!context.abort.signal.aborted && active.current === context) setState({ key, items: [], page: null, error: pageError(error), loading: false }); }
      finally { context.loading = false; }
    }
    return () => { clearTimeout(timer); scope.abort.abort(); if (active.current === scope) active.current = null; };
  }, [key, enabled, revision, delay]);
  async function more() {
    const scope = active.current;
    if (!scope || scope.key !== key || scope.loading || !state.page?.next_cursor) return;
    scope.loading = true; setState(old => ({ ...old, loading: true, error: null }));
    try {
      const page = await request<P>(`${path}${path.includes('?') ? '&' : '?'}limit=20&q=${encodeURIComponent(query.trim())}&cursor=${encodeURIComponent(state.page.next_cursor)}`, { signal: scope.abort.signal });
      if (!scope.abort.signal.aborted && active.current === scope) setState(old => ({ key, items: [...old.items, ...page.data.filter(item => !old.items.some(previous => previous.id === item.id))], page, error: null, loading: false }));
    } catch (error) { if (!scope.abort.signal.aborted && active.current === scope) setState(old => ({ ...old, ...(error instanceof ApiError && [401, 403, 404].includes(error.status) ? { items: [], page: null } : {}), loading: false, error: pageError(error) })); }
    finally { scope.loading = false; }
  }
  return { items: state.key === key ? state.items : [], page: state.key === key ? state.page : null, error: state.key === key ? state.error : null, loading: state.key !== key || state.loading, more, refresh: () => setRevision(value => value + 1) };
}
