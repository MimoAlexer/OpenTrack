import { useSessionScope } from '../desktop/context';
import { useEffect, useState } from 'react';
import { request } from '../api';
import type { Bootstrap, Session } from '../types';
import { mergeStatuses, ordered } from './model';
import { useWorkflow } from './useWorkflow';
import type { Workflow, WorkflowStatus } from './types';

export function useIssueWorkflow(workspaceId: string, teamId: string, session: Session, data: Bootstrap, statusId?: string, changed?: (workflow: Workflow) => void) {
  const sessionScope = useSessionScope(session);
  const seeded = data.statuses.filter(status => status.team_id === teamId) as WorkflowStatus[];
  const resource = useWorkflow(workspaceId, teamId, sessionScope, changed, seeded);
  const [history, setHistory] = useState<{ scope: string; status: WorkflowStatus } | null>(null);
  const scope = `${workspaceId}:${teamId}:${sessionScope}`;
  const current = resource.data?.statuses ?? seeded;
  const missing = statusId && !current.some(status => status.id === statusId) ? statusId : null;
  useEffect(() => {
    const abort = new AbortController(); setHistory(null);
    if (missing && teamId) request<WorkflowStatus>(`/workspaces/${workspaceId}/teams/${teamId}/statuses/${missing}`, { signal: abort.signal }).then(status => { if (!abort.signal.aborted && status.id === missing && status.team_id === teamId) setHistory({ scope, status }); }).catch(() => { /* The picker retains a neutral unavailable label. Shared401 handling remains active. */ });
    return () => abort.abort();
  }, [workspaceId, teamId, missing, scope]);
  const historical = history?.scope === scope ? history.status : seeded.find(status => status.id === missing);
  return { ...resource, statuses: ordered(historical ? mergeStatuses(current, [historical]) : current), currentIds: new Set(current.map(status => status.id)) };
}
