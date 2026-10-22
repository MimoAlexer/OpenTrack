import { useEffect, useRef, useState } from 'react';
import { ApiError, request } from '../api';
import type { Issue, IssuePage } from '../types';

export interface PaletteIssueSource {
  workspaceId: string;
  scope: string;
  recentIds: readonly string[];
  open: (id: string) => void;
}
type Item = Pick<Issue, 'id' | 'workspace_id' | 'key' | 'title'>;
type Result = { scope: string; items: Item[]; busy: boolean; error: unknown };

// Keep only references, for this authenticated workspace lifetime. Titles always
// come from a new authorized request; logout/access changes destroy the history.
export function useRecentIssues(scope: string, issueId?: string) {
  const [recent, setRecent] = useState<{ scope: string; ids: string[] }>({ scope, ids: [] });
  useEffect(() => {
    setRecent(previous => {
      const ids = previous.scope === scope ? previous.ids : [];
      return { scope, ids: issueId ? [issueId, ...ids.filter(id => id !== issueId)].slice(0, 10) : ids };
    });
  }, [scope, issueId]);
  return recent.scope === scope ? recent.ids : [];
}

export function usePaletteIssues(source: PaletteIssueSource | undefined, query: string) {
  const text = query.trim();
  const recentIds = [...new Set(source?.recentIds ?? [])].slice(0, 10);
  const key = JSON.stringify([source?.workspaceId, source?.scope, text, text ? [] : recentIds]);
  const current = useRef(key); current.current = key;
  const controller = useRef<AbortController | null>(null);
  const [result, setResult] = useState<Result>({ scope: '', items: [], busy: false, error: null });
  const [attempt, setAttempt] = useState(0);
  useEffect(() => {
    if (!source) return;
    const abort = new AbortController(); controller.current = abort;
    setResult({ scope: key, items: [], busy: true, error: null });
    const valid = () => !abort.signal.aborted && current.current === key;
    const timer = setTimeout(() => {
      async function load() {
        try {
          const path = `/workspaces/${encodeURIComponent(source!.workspaceId)}/issues`;
          const items = text
            ? (await request<IssuePage>(`${path}?${new URLSearchParams({ q: text, limit: '20' })}`, { signal: abort.signal })).data.slice(0, 20)
            : (await Promise.all(recentIds.map(async id => {
              try { return await request<Issue>(`${path}/${encodeURIComponent(id)}`, { signal: abort.signal }); }
              catch (error) { if (error instanceof ApiError && error.status === 404) return null; throw error; }
            }))).filter((item): item is Issue => item !== null && !item.archived_at);
          if (valid()) setResult({ scope: key, items: items.filter(item => item.workspace_id === source!.workspaceId).map(({ id, workspace_id, key, title }) => ({ id, workspace_id, key, title })), busy: false, error: null });
        } catch (error) {
          if (valid()) setResult({ scope: key, items: [], busy: false, error });
        }
      }
      void load();
    }, text ? 150 : 0);
    return () => { clearTimeout(timer); abort.abort(); };
  }, [key, attempt]);

  async function resolve(id: string) {
    const abort = controller.current;
    if (!source || !abort || abort.signal.aborted) return false;
    const valid = () => !abort.signal.aborted && current.current === key;
    setResult(previous => previous.scope === key ? { ...previous, busy: true, error: null } : previous);
    try {
      const issue = await request<Issue>(`/workspaces/${encodeURIComponent(source.workspaceId)}/issues/${encodeURIComponent(id)}`, { signal: abort.signal });
      if (!valid()) return false;
      if (issue.workspace_id !== source.workspaceId) throw new ApiError(404, 'not_found', 'This issue is unavailable.');
      return true;
    } catch (error) {
      if (valid()) setResult(previous => previous.scope === key ? { ...previous, items: previous.items.filter(item => item.id !== id), busy: false, error } : previous);
      return false;
    }
  }
  return {
    items: result.scope === key ? result.items : [],
    busy: !!source && (result.scope !== key || result.busy),
    error: result.scope === key ? result.error : null,
    retry: () => setAttempt(value => value + 1),
    resolve,
  };
}
