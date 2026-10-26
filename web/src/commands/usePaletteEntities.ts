import { useEffect, useRef, useState } from 'react';
import { ApiError, request } from '../api';
import { projectMetadataPath, projectPath, type Page, type ProjectSummary } from '../projects/types';
import type { TeamDetail } from '../settings/types';
import type { PaletteIssueSource } from './usePaletteIssues';

export type PaletteEntity = { kind: 'team' | 'project'; id: string; workspaceId: string; teamId?: string; name: string; key?: string };
type Result = { scope: string; items: PaletteEntity[]; busy: boolean; error: unknown };

function useEntitySearch(source: PaletteIssueSource | undefined, query: string, teamId: string | undefined, kind: PaletteEntity['kind']) {
  const text = query.trim();
  const enabled = !!source && !!text && (kind === 'team' || !!teamId);
  const key = JSON.stringify([source?.workspaceId, source?.scope, teamId, text, kind]);
  const current = useRef(key); current.current = key;
  const controller = useRef<AbortController | null>(null);
  const [result, setResult] = useState<Result>({ scope: '', items: [], busy: false, error: null });
  const [attempt, setAttempt] = useState(0);
  useEffect(() => {
    if (!enabled || !source) return;
    const abort = new AbortController(); controller.current = abort;
    const valid = () => !abort.signal.aborted && current.current === key;
    setResult({ scope: key, items: [], busy: true, error: null });
    const timer = setTimeout(async () => {
      try {
        const path = kind === 'team' ? `/workspaces/${encodeURIComponent(source.workspaceId)}/teams` : projectPath(source.workspaceId, teamId!);
        const search = new URLSearchParams({ q: text, limit: '20', ...(kind === 'team' ? { state: 'active' } : { archived: 'false' }) });
        const page = await request<Page<TeamDetail | ProjectSummary>>(`${path}?${search}`, { signal: abort.signal });
        const items = page.data.slice(0, 20).filter(item => item.workspace_id === source.workspaceId && !item.archived_at && (kind === 'team' || 'team_id' in item && item.team_id === teamId))
          .map(item => ({ kind, id: item.id, workspaceId: item.workspace_id, teamId: kind === 'project' ? teamId : undefined, name: item.name, key: 'key' in item ? item.key : undefined }));
        if (valid()) setResult({ scope: key, items, busy: false, error: null });
      } catch (error) {
        if (valid()) setResult({ scope: key, items: [], busy: false, error });
      }
    }, 150);
    return () => { clearTimeout(timer); abort.abort(); };
  }, [key, attempt, enabled]);

  async function resolve(item: PaletteEntity) {
    const abort = controller.current;
    if (!enabled || !source || !abort || abort.signal.aborted || item.workspaceId !== source.workspaceId || item.kind !== kind || kind === 'project' && item.teamId !== teamId) return false;
    const valid = () => !abort.signal.aborted && current.current === key;
    setResult(previous => previous.scope === key ? { ...previous, busy: true, error: null } : previous);
    try {
      const path = kind === 'team' ? `/workspaces/${encodeURIComponent(source.workspaceId)}/teams/${encodeURIComponent(item.id)}` : projectMetadataPath(source.workspaceId, teamId!, item.id);
      const detail = await request<TeamDetail | ProjectSummary>(path, { signal: abort.signal });
      if (!valid()) return false;
      if (detail.id !== item.id || detail.workspace_id !== source.workspaceId || detail.archived_at || kind === 'project' && (!('team_id' in detail) || detail.team_id !== teamId)) throw new ApiError(404, 'not_found', `This ${kind} is unavailable.`);
      return true;
    } catch (error) {
      if (valid()) setResult(previous => previous.scope === key ? { ...previous, items: previous.items.filter(value => value.id !== item.id), busy: false, error } : previous);
      return false;
    }
  }
  return {
    items: enabled && result.scope === key ? result.items : [],
    busy: enabled && (result.scope !== key || result.busy),
    error: enabled && result.scope === key ? result.error : null,
    retry: () => setAttempt(value => value + 1),
    resolve,
  };
}

export function usePaletteEntities(source: PaletteIssueSource | undefined, query: string, teamId?: string) {
  return { teams: useEntitySearch(source, query, teamId, 'team'), projects: useEntitySearch(source, query, teamId, 'project') };
}
