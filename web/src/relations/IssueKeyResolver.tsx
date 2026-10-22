import { useEffect, useState } from 'react';
import { ApiError, request } from '../api';
import { ErrorBanner, Loading } from '../components';
import { issuesUrl, Link, navigate } from '../router';
import type { Issue } from '../types';

export function IssueKeyResolver({ workspaceId, issueKey, search }: { workspaceId: string; issueKey: string; search: URLSearchParams }) {
  const [error, setError] = useState<unknown>(null); const [retry, setRetry] = useState(0); const query = search.toString();
  useEffect(() => {
    const abort = new AbortController(); setError(null);
    request<Issue>(`/workspaces/${workspaceId}/issues/by-key/${encodeURIComponent(issueKey)}`, { signal: abort.signal }).then(issue => {
      if (abort.signal.aborted) return;
      if (issue.workspace_id !== workspaceId) throw new ApiError(404, 'not_found', 'This issue is unavailable.');
      navigate(issuesUrl(workspaceId, new URLSearchParams(query), issue.id), true);
    }).catch(error => { if (!abort.signal.aborted) setError(error); });
    return () => abort.abort();
  }, [workspaceId, issueKey, query, retry]);
  return <section className="issue-detail" aria-label="Opening issue key"><div className="settings-modal-content">{error ? <><ErrorBanner error={error} retry={() => setRetry(value => value + 1)} /><Link className="button" href={issuesUrl(workspaceId, search)}>Back to issues</Link></> : <Loading label="Opening issue…" />}</div></section>;
}
