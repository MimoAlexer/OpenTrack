import { useSessionScope } from '../desktop/context';
import { useEffect, useRef, useState } from 'react';
import { ApiError, request } from '../api';
import type { Issue, Session } from '../types';
import type { GraphPage, IssueGraphTarget, IssueParent, IssueRelation } from './types';

interface Graph { parent: IssueParent; children: GraphPage<IssueGraphTarget>; relations: GraphPage<IssueRelation> }
export function useIssueGraph(workspaceId: string, issue: Issue, session: Session) {
  const sessionScope = useSessionScope(session);
  // Body-only saves change the representation ETag, not the connection graph.
  // Commands still use the current Issue.etag supplied by the detail view.
  const path = `/workspaces/${workspaceId}/issues/${issue.id}`; const key = `${path}:${issue.team_id}:${issue.version}:${sessionScope}`;
  const [state, setState] = useState<{ key: string; data: Graph | null; error: unknown; loading: boolean }>({ key, data: null, error: null, loading: true });
  const [revision, setRevision] = useState(0); const [moreBusy, setMoreBusy] = useState(false); const active = useRef<AbortController | null>(null);
  useEffect(() => {
    const abort = new AbortController(); active.current = abort; setState({ key, data: null, error: null, loading: true }); setMoreBusy(false);
    Promise.all([request<IssueParent>(`${path}/parent`, { signal: abort.signal }), request<GraphPage<IssueGraphTarget>>(`${path}/children?limit=20`, { signal: abort.signal }), request<GraphPage<IssueRelation>>(`${path}/relations?limit=20`, { signal: abort.signal })]).then(([parent, children, relations]) => {
      if (abort.signal.aborted) return;
      if (parent.graph_version !== children.graph_version || parent.graph_version !== relations.graph_version) throw new ApiError(409, 'graph_changed', 'Relationships changed while loading. Refresh to load a consistent view.');
      setState({ key, data: { parent, children, relations }, error: null, loading: false });
    }).catch(error => { if (!abort.signal.aborted) setState({ key, data: null, error, loading: false }); });
    return () => { abort.abort(); if (active.current === abort) active.current = null; };
  }, [key, revision]);
  async function more(kind: 'children' | 'relations') {
    const abort = active.current; const graph = state.key === key ? state.data : null; const page = graph?.[kind];
    if (!abort || !page?.next_cursor || moreBusy) return; setMoreBusy(true);
    try {
      const next = await request<GraphPage<IssueGraphTarget | IssueRelation>>(`${path}/${kind}?limit=20&cursor=${encodeURIComponent(page.next_cursor)}`, { signal: abort.signal });
      if (abort.signal.aborted) return;
      if (next.graph_version !== page.graph_version) throw new ApiError(409, 'graph_changed', 'Relationships changed. Refresh to reload the first page.');
      setState(old => old.key !== key || !old.data ? old : { ...old, error: null, data: { ...old.data, [kind]: { ...next, data: [...old.data[kind].data, ...next.data.filter(item => !old.data![kind].data.some(previous => previous.id === item.id))] } } as Graph });
    } catch (error) { if (!abort.signal.aborted) setState(old => ({ ...old, ...(error instanceof ApiError && [401, 403, 404].includes(error.status) ? { data: null } : {}), error })); }
    finally { if (!abort.signal.aborted) setMoreBusy(false); }
  }
  return { data: state.key === key ? state.data : null, loading: state.key !== key || state.loading, error: state.key === key ? state.error : null, moreBusy, more, refresh: () => setRevision(value => value + 1) };
}
