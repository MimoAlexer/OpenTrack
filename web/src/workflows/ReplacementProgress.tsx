import { useSessionScope } from '../desktop/context';
import { useEffect, useRef, useState } from 'react';
import { ApiError, request } from '../api';
import { ErrorBanner } from '../components';
import type { Session } from '../types';
import { useScopedRequest } from '../settings/shared';
import type { Workflow, WorkflowReplacement, WorkflowStatus } from './types';

const stateLabels: Record<WorkflowReplacement['state'], string> = { queued: 'Queued', running: 'Moving issues', retry_wait: 'Waiting to retry', blocked: 'Paused', dead_letter: 'Needs attention', succeeded: 'Completed' };
export function ReplacementProgress({ workspaceId, teamId, jobId, source, workflow, session, canManage, changed }: {
  workspaceId: string; teamId: string; jobId: string; source: WorkflowStatus; workflow: Workflow; session: Session; canManage: boolean; changed: (completed?: boolean) => void;
}) {
  const sessionScope = useSessionScope(session);
  const scoped = useScopedRequest(); const [job, setJob] = useState<WorkflowReplacement | null>(null); const [error, setError] = useState<unknown>(null); const [busy, setBusy] = useState(false);
  const [revision, setRevision] = useState(0); const current = useRef<WorkflowReplacement | null>(null); const callback = useRef(changed); callback.current = changed;
  const path = `/workspaces/${workspaceId}/teams/${teamId}/workflow-replacements/${jobId}`;
  useEffect(() => {
    let active = true; let inFlight = false; let timer: ReturnType<typeof setTimeout> | undefined; const abort = new AbortController(); let failures = 0;
    async function refresh() {
      if (!active || inFlight) return; inFlight = true; clearTimeout(timer);
      try {
        const result = await request<WorkflowReplacement>(path, { signal: abort.signal });
        if (active && !abort.signal.aborted && (!current.current || BigInt(result.version) >= BigInt(current.current.version))) { const completed = result.state === 'succeeded' && current.current?.state !== 'succeeded'; current.current = result; setJob(result); setError(null); failures = 0; if (completed) callback.current(true); }
      } catch (error) { if (active && !abort.signal.aborted) { setError(error); failures += 1; if (error instanceof ApiError && [401, 403, 404].includes(error.status)) { current.current = null; setJob(null); } } }
      finally { inFlight = false; if (active && document.visibilityState !== 'hidden' && current.current?.state !== 'succeeded') timer = setTimeout(() => void refresh(), Math.min(15_000, 2_000 * 2 ** failures)); }
    }
    const visible = () => { clearTimeout(timer); if (document.visibilityState !== 'hidden') void refresh(); };
    void refresh(); window.addEventListener('focus', visible); document.addEventListener('visibilitychange', visible);
    return () => { active = false; abort.abort(); clearTimeout(timer); window.removeEventListener('focus', visible); document.removeEventListener('visibilitychange', visible); };
  }, [path, revision, sessionScope]);
  async function resume() {
    if (!job || busy || !canManage) return; setBusy(true); setError(null);
    try {
      const result = await scoped<WorkflowReplacement>(`${path}/resume`, { method: 'POST', csrf: session.csrf_token, headers: { 'If-Match': `"workflow-replacement:${job.id}:${job.version}"` } });
      current.current = result; setJob(result); setRevision(value => value + 1); callback.current();
    } catch (error) { setError(error); if (error instanceof ApiError && error.status === 412) setRevision(value => value + 1); }
    finally { setBusy(false); }
  }
  const target = workflow.statuses.find(status => status.id === source.replacement_status_id);
  return <section className="replacement-progress" aria-label={`Replacement of ${source.name}`}><div className="replacement-title"><strong>{source.name} → {target?.name ?? 'Replacement status'}</strong><span>{job ? stateLabels[job.state] : 'Loading progress…'}</span></div><ErrorBanner error={error} retry={() => setRevision(value => value + 1)} />{job && <><p role="status">{job.processed_count} moved · {job.remaining_count} remaining{error ? ' · Last confirmed progress' : ''}</p>{job.state === 'queued' && <p className="muted">The worker will continue this saved replacement when it is available.</p>}{job.state === 'retry_wait' && <p className="muted">The worker will retry automatically. Completed changes are already saved.</p>}{['blocked', 'dead_letter'].includes(job.state) && <><p className="muted">Replacement stopped before finishing. A current team manager can resume the remaining work after checking access and team availability.</p>{canManage && <button className="button compact" disabled={busy} onClick={resume}>{busy ? 'Resuming…' : 'Resume replacement'}</button>}</>}</>}</section>;
}
