import { useCallback, useEffect, useRef, useState } from 'react';
import { ApiError, request } from '../api';
import { workflowETag } from './model';
import type { Workflow, WorkflowStatus } from './types';

export async function readWorkflow(path: string, current: Workflow | null, signal: AbortSignal): Promise<Workflow | null> {
  try {
    return await request<Workflow>(path, { signal, headers: current ? { 'If-None-Match': workflowETag(current) } : {} });
  } catch (error) {
    // The shared transport still owns protected401 and committed invalidation.
    // A304 is meaningful only with a current snapshot of this exact scope.
    if (current && error instanceof ApiError && error.status === 304) return null;
    throw error;
  }
}

function statusSnapshot(statuses: WorkflowStatus[]) {
  return JSON.stringify([...statuses].sort((a, b) => a.id.localeCompare(b.id)).map(status => [status.id, status.team_id, status.name, status.category, status.color, status.position, status.is_default, status.archived_at, status.retiring_at, status.replacement_status_id, status.replacement_job_id]));
}

export function useWorkflow(workspaceId: string, teamId: string, sessionKey: string, changed?: (workflow: Workflow) => void, initialStatuses?: WorkflowStatus[]) {
  const path = `/workspaces/${workspaceId}/teams/${teamId}/workflow`;
  const [state, setState] = useState<{ path: string; data: Workflow | null }>({ path, data: null });
  const [loading, setLoading] = useState(true); const [error, setError] = useState<unknown>(null);
  const current = useRef<Workflow | null>(null); const scope = useRef(path); const callback = useRef(changed); callback.current = changed;
  const baseline = useRef(initialStatuses); baseline.current = initialStatuses;
  const refreshRef = useRef<() => void>(() => {});
  const accept = useCallback((workflow: Workflow) => {
    if (workflow.team_id !== teamId || scope.current !== path) return;
    if (current.current && BigInt(workflow.version) < BigInt(current.current.version)) return;
    // Bootstrap metadata is already visible. First hydration should propagate
    // only a real difference, rather than refreshing the issue list on opening.
    const different = current.current
      ? current.current.version !== workflow.version || current.current.can_manage !== workflow.can_manage
      : !baseline.current || statusSnapshot(baseline.current) !== statusSnapshot(workflow.statuses);
    current.current = workflow; setState({ path, data: workflow }); setLoading(false); setError(null);
    if (different) callback.current?.(workflow);
  }, [path, teamId]);
  useEffect(() => {
    if (scope.current !== path) { current.current = null; setState({ path, data: null }); }
    scope.current = path; let active = true; let inFlight = false; let timer: ReturnType<typeof setTimeout> | undefined;
    const abort = new AbortController(); let failures = 0;
    setLoading(!current.current); setError(null);
    async function refresh() {
      if (!active || inFlight) return; clearTimeout(timer); inFlight = true;
      try {
        const result = await readWorkflow(path, current.current, abort.signal);
        if (active && !abort.signal.aborted) { if (result) accept(result); setError(null); failures = 0; }
      } catch (error) {
        if (active && !abort.signal.aborted) {
          setError(error); failures += 1;
          if (error instanceof ApiError && [401, 403, 404].includes(error.status)) { current.current = null; setState({ path, data: null }); }
        }
      } finally {
        inFlight = false;
        if (active) { setLoading(false); if (document.visibilityState !== 'hidden') timer = setTimeout(() => void refresh(), Math.min(15_000, 2_000 * 2 ** failures)); }
      }
    }
    const visible = () => { clearTimeout(timer); if (teamId && document.visibilityState !== 'hidden') void refresh(); };
    refreshRef.current = () => { if (teamId) void refresh(); }; if (teamId) void refresh(); else setLoading(false);
    window.addEventListener('focus', visible); document.addEventListener('visibilitychange', visible);
    return () => { active = false; abort.abort(); clearTimeout(timer); window.removeEventListener('focus', visible); document.removeEventListener('visibilitychange', visible); };
  }, [path, teamId, sessionKey, accept]);
  return { data: state.path === path ? state.data : null, loading: state.path !== path || loading, error, accept, refresh: () => refreshRef.current() };
}
