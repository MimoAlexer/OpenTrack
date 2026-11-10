import { useEffect, useRef, useState } from 'react';
import { ApiError, request } from '../api';
import type { components } from '../generated/api';
import { projectMetadataPath, type ProjectSummary } from '../projects/types';
import type { Issue } from '../types';
import type { PaletteIssueSource } from './usePaletteIssues';

export type PaletteSearchHit = components['schemas']['SearchHit'];
type SearchPage = components['schemas']['SearchPage'];
type Result = { scope: string; items: PaletteSearchHit[]; busy: boolean; error: unknown; fuzzyDegraded: boolean; notice: string | null };

export function usePaletteSearch(source: PaletteIssueSource | undefined, query: string) {
  const enabled = !!source && !!query.trim();
  const [attempt, setAttempt] = useState(0);
  // The exact input and retry generation fence both searches and detail checks.
  // Trimming is only used to decide whether this is a blank recent-items query.
  const key = JSON.stringify([source?.workspaceId, source?.scope, query, attempt]);
  const current = useRef(key); current.current = key;
  const active = useRef<{ key: string; abort: AbortController } | null>(null);
  const [result, setResult] = useState<Result>({ scope: '', items: [], busy: false, error: null, fuzzyDegraded: false, notice: null });
  useEffect(() => {
    if (!enabled || !source) return;
    const context = { key, abort: new AbortController() }; active.current = context;
    const valid = () => !context.abort.signal.aborted && active.current === context && current.current === key;
    setResult({ scope: key, items: [], busy: true, error: null, fuzzyDegraded: false, notice: null });
    const timer = setTimeout(async () => {
      try {
        const page = await request<SearchPage>(`/workspaces/${encodeURIComponent(source.workspaceId)}/search?${new URLSearchParams({ q: query, limit: '20' })}`, { signal: context.abort.signal });
        const seen = new Set<string>();
        const items = page.data.filter(item => {
          const identity = `${item.kind}:${item.id}`;
          if (seen.has(identity)) return false;
          seen.add(identity); return true;
        }).slice(0, 20);
        if (valid()) setResult({ scope: key, items, busy: false, error: null, fuzzyDegraded: page.fuzzy_degraded, notice: page.notice ?? null });
      } catch (error) {
        if (valid()) setResult({ scope: key, items: [], busy: false, error, fuzzyDegraded: false, notice: null });
      }
    }, 150);
    return () => { clearTimeout(timer); context.abort.abort(); if (active.current === context) active.current = null; };
  }, [key, enabled]);

  async function resolve(item: PaletteSearchHit) {
    const context = active.current;
    if (!enabled || !source || !context || context.key !== key || context.abort.signal.aborted) return false;
    const valid = () => !context.abort.signal.aborted && active.current === context && current.current === key;
    setResult(previous => previous.scope === key ? { ...previous, busy: true, error: null } : previous);
    try {
      if (item.kind === 'issue') {
        const detail = await request<Issue>(`/workspaces/${encodeURIComponent(source.workspaceId)}/issues/${encodeURIComponent(item.id)}`, { signal: context.abort.signal });
        if (!valid()) return false;
        if (detail.id !== item.id || detail.workspace_id !== source.workspaceId || detail.archived_at) throw new ApiError(404, 'not_found', 'This issue is unavailable.');
      } else {
        const detail = await request<ProjectSummary>(projectMetadataPath(source.workspaceId, item.team_id, item.id), { signal: context.abort.signal });
        if (!valid()) return false;
        if (detail.id !== item.id || detail.workspace_id !== source.workspaceId || detail.team_id !== item.team_id || detail.archived_at) throw new ApiError(404, 'not_found', 'This project is unavailable.');
      }
      return true;
    } catch (error) {
      if (valid()) setResult(previous => previous.scope === key ? { ...previous, items: previous.items.filter(value => value.kind !== item.kind || value.id !== item.id), busy: false, error } : previous);
      return false;
    }
  }
  return {
    generation: key,
    items: enabled && result.scope === key ? result.items : [],
    busy: enabled && (result.scope !== key || result.busy),
    error: enabled && result.scope === key ? result.error : null,
    fuzzyDegraded: enabled && result.scope === key && result.fuzzyDegraded,
    notice: enabled && result.scope === key ? result.notice : null,
    retry: () => setAttempt(value => value + 1),
    resolve,
  };
}
