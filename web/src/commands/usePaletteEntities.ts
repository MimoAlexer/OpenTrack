import { useEffect, useRef, useState } from 'react';
import { ApiError, request } from '../api';
import { projectMetadataPath, projectPath, type Page, type ProjectSummary } from '../projects/types';
import type { TeamDetail } from '../settings/types';
import type { PaletteIssueSource } from './usePaletteIssues';

export type PaletteEntity = { kind: 'team' | 'project'; id: string; workspaceId: string; teamId?: string; name: string; key?: string };
type Result = { scope: string; items: PaletteEntity[]; busy: boolean; error: unknown };

function useEntitySearch(source: PaletteIssueSource | undefined, query: string, teamId: string | undefined, kind: PaletteEntity['kind'], searchProjects: boolean) {
  const text = query.trim();
  const [attempt, setAttempt] = useState(0);
  const enabled = !!source && !!text && (kind === 'team' || searchProjects && !!teamId);
  const key = JSON.stringify([source?.workspaceId, source?.scope, teamId, query, kind, enabled, attempt]);
  const current = useRef(key); current.current = key;
  const controller = useRef<AbortController | null>(null);
  const detailTicket = useRef(0);
  const detailRows = useRef(new Map<string, number>());
  const [result, setResult] = useState<Result>({ scope: '', items: [], busy: false, error: null });
  useEffect(() => {
    if (!enabled || !source) return;
    const abort = new AbortController(); controller.current = abort;
    const rows = new Map<string, number>(); detailRows.current = rows;
    const valid = () => !abort.signal.aborted && current.current === key;
    setResult({ scope: key, items: [], busy: true, error: null });
    const timer = setTimeout(async () => {
      try {
        const path = kind === 'team' ? `/workspaces/${encodeURIComponent(source.workspaceId)}/teams` : projectPath(source.workspaceId, teamId!);
        const search = new URLSearchParams({ q: text, limit: '20', ...(kind === 'team' ? { state: 'active' } : { archived: 'false' }) });
        const page = await request<Page<TeamDetail | ProjectSummary>>(`${path}?${search}`, { signal: abort.signal });
        const items = page.data.slice(0, 20).filter(item => item.workspace_id === source.workspaceId && !item.archived_at && (kind === 'team' || 'team_id' in item && item.team_id === teamId))
          .map(item => ({ kind, id: item.id, workspaceId: item.workspace_id, teamId: kind === 'project' ? teamId : undefined, name: item.name, key: 'key' in item ? item.key : undefined }));
        if (valid()) {
          for (const item of items) rows.set(item.id, 0);
          setResult({ scope: key, items, busy: false, error: null });
        }
      } catch (error) {
        if (valid()) setResult({ scope: key, items: [], busy: false, error });
      }
    }, 150);
    return () => { clearTimeout(timer); abort.abort(); };
  }, [key, attempt, enabled]);

  async function resolve(item: PaletteEntity) {
    const abort = controller.current;
    const rows = detailRows.current;
    if (!enabled || !source || !abort || current.current !== key || abort.signal.aborted || !rows.has(item.id) || item.workspaceId !== source.workspaceId || item.kind !== kind || kind === 'project' && item.teamId !== teamId) return false;
    const ticket = ++detailTicket.current;
    rows.set(item.id, ticket);
    const contextValid = () => !abort.signal.aborted && current.current === key && detailRows.current === rows;
    const latest = () => contextValid() && detailTicket.current === ticket;
    setResult(previous => previous.scope === key ? { ...previous, busy: true, error: null } : previous);
    try {
      const path = kind === 'team' ? `/workspaces/${encodeURIComponent(source.workspaceId)}/teams/${encodeURIComponent(item.id)}` : projectMetadataPath(source.workspaceId, teamId!, item.id);
      const detail = await request<TeamDetail | ProjectSummary>(path, { signal: abort.signal });
      if (!contextValid()) return false;
      if (detail.id !== item.id || detail.workspace_id !== source.workspaceId || detail.archived_at || kind === 'project' && (!('team_id' in detail) || detail.team_id !== teamId)) throw new ApiError(404, 'not_found', `This ${kind} is unavailable.`);
      return latest();
    } catch (error) {
      if (contextValid() && rows.get(item.id) === ticket) {
        const selected = latest();
        const remove = selected || error instanceof ApiError && [401, 403, 404].includes(error.status);
        if (remove) rows.delete(item.id);
        setResult(previous => previous.scope === key ? { ...previous, items: remove ? previous.items.filter(value => value.id !== item.id) : previous.items, ...(selected ? { error } : {}) } : previous);
      }
      return false;
    } finally {
      if (latest()) setResult(previous => previous.scope === key ? { ...previous, busy: false } : previous);
    }
  }
  return {
    generation: key,
    items: enabled && result.scope === key ? result.items : [],
    busy: enabled && (result.scope !== key || result.busy),
    error: enabled && result.scope === key ? result.error : null,
    retry: () => setAttempt(value => value + 1),
    resolve,
  };
}

export function usePaletteEntities(source: PaletteIssueSource | undefined, query: string, teamId?: string, searchProjects = true) {
  return { teams: useEntitySearch(source, query, teamId, 'team', searchProjects), projects: useEntitySearch(source, query, teamId, 'project', searchProjects) };
}
