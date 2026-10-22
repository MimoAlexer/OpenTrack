import { useEffect, useRef, useState } from 'react';
import { request } from '../api';
import { isAccessError } from '../documents/model';
import { templatePath, type TemplatePage } from './types';

export function useTemplates(workspace: string, team: string, scope: string, archived: boolean, enabled = true) {
  const path = `${templatePath(workspace, team)}?archived=${archived}&limit=100`;
  const key = `${scope}:${path}`; const [revision, setRevision] = useState(0);
  const [state, setState] = useState<{ key: string; page: TemplatePage | null; loading: boolean; error: unknown }>({ key, page: null, loading: true, error: null });
  const active = useRef<{ key: string; abort: AbortController; busy: boolean } | null>(null);
  useEffect(() => {
    const context = { key, abort: new AbortController(), busy: true }; active.current = context;
    setState(old => ({ key, page: old.key === key ? old.page : null, loading: enabled, error: null }));
    if (enabled && team) request<TemplatePage>(path, { signal: context.abort.signal }).then(page => {
      if (!context.abort.signal.aborted) setState({ key, page, loading: false, error: null });
    }).catch(error => { if (!context.abort.signal.aborted) setState(old => ({ key, page: isAccessError(error) ? null : old.page, loading: false, error })); }).finally(() => { context.busy = false; });
    else context.busy = false;
    return () => { context.abort.abort(); };
  }, [key, enabled, revision]);
  async function more() {
    const context = active.current; const cursor = state.key === key ? state.page?.next_cursor : null;
    if (!context || context.key !== key || context.busy || !cursor) return;
    context.busy = true; setState(old => ({ ...old, loading: true, error: null }));
    try {
      const next = await request<TemplatePage>(`${path}&cursor=${encodeURIComponent(cursor)}`, { signal: context.abort.signal });
      if (!context.abort.signal.aborted) setState(old => ({ key, page: { ...next, data: [...(old.page?.data ?? []), ...next.data.filter(item => !old.page?.data.some(old => old.id === item.id))] }, loading: false, error: null }));
    } catch (error) { if (!context.abort.signal.aborted) setState(old => ({ ...old, page: isAccessError(error) ? null : old.page, loading: false, error })); }
    finally { context.busy = false; }
  }
  return { page: state.key === key && enabled ? state.page : null, loading: enabled && (state.key !== key || state.loading), error: state.key === key ? state.error : null, refresh: () => setRevision(value => value + 1), more };
}
