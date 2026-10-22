import { useEffect, useState } from 'react';
import { Plus, X } from 'lucide-react';
import { ApiError } from '../api';
import { ErrorBanner, Loading, Modal } from '../components';
import { useSessionScope } from '../desktop/context';
import { isAccessError } from '../documents/model';
import { issuesUrl, Link } from '../router';
import { useScopedRequest } from '../settings/shared';
import { useSearchPage } from '../taxonomy/useSearchPage';
import type { Issue, IssuePage, Session } from '../types';
import { projectPath, type Project, type ProjectIssue } from './types';
import { useProjectPage } from './useProjectData';

export function ProjectIssues({ project, session, refresh, readOnly, changed, denied }: { project: Project; session: Session; refresh: number; readOnly: boolean; changed: (value: Project) => void; denied: (error: unknown) => void }) {
  const path = projectPath(project.workspace_id, project.team_id, project.id); const request = useScopedRequest();
  const directory = useProjectPage<ProjectIssue>(`${path}/issues`, refresh, 50);
  const [adding, setAdding] = useState(false); const [busy, setBusy] = useState<string | null>(null); const [error, setError] = useState<unknown>(null);
  useEffect(() => { if (isAccessError(directory.error)) denied(directory.error); }, [directory.error]);
  async function membership(issue: { id: string }, included: boolean) {
    if (busy || readOnly) return; setBusy(issue.id); setError(null);
    try { const value = await request<Project>(`${path}/issues/${issue.id}`, { method: 'PUT', csrf: session.csrf_token, headers: { 'If-Match': project.etag }, body: { included } }); changed(value); if (included) setAdding(false); }
    catch (error) {
      setError(error); if (isAccessError(error)) denied(error);
      if (error instanceof ApiError && error.status === 412) {
        try { changed(await request<Project>(path)); }
        catch (readError) { setError(readError); if (isAccessError(readError)) denied(readError); }
      }
    } finally { setBusy(null); }
  }
  return <section className="project-issues" aria-label="Project issues"><header className="project-section-heading"><div><h3>Issues</h3><p>The work included in this project.</p></div>{!readOnly && <button className="button compact" onClick={() => { setError(null); setAdding(true); }}><Plus aria-hidden="true"/>Add issues</button>}</header>
    {!adding && <ErrorBanner error={error}/>}{!adding && error instanceof ApiError && error.status === 412 && <p className="notice">The project changed. The latest version is loaded; review its issues before trying again.</p>}
    <ErrorBanner error={directory.error} retry={directory.retry}/>
    {!directory.value && directory.loading ? <Loading label="Loading project issues…"/> : <div className="project-issue-list" aria-busy={directory.loading}>{directory.value?.data.map(issue => <div className="project-issue-row" key={issue.id}><Link href={issuesUrl(project.workspace_id, new URLSearchParams({ team_id: project.team_id }), issue.id)}><span className="project-issue-key">{issue.key}</span><span>{issue.title}</span></Link><span className={`project-issue-status status-${issue.status_category}`}>{issue.status_name}</span>{!readOnly && <button className="icon-button" aria-label={`Remove ${issue.key} from project`} disabled={!!busy} onClick={() => void membership(issue, false)}><X aria-hidden="true"/></button>}</div>)}{!directory.value?.data.length && !directory.error && <div className="project-empty"><h4>No issues yet</h4><p>Add existing issues from this team to define the project scope.</p></div>}</div>}
    {directory.value?.has_more && <button className="button compact project-more" disabled={directory.loading} onClick={directory.more}>{directory.loading ? 'Loading…' : 'Load more issues'}</button>}
    {adding && <AddProjectIssues project={project} session={session} included={directory.value?.data.map(item => item.id) ?? []} busy={!!busy} error={error} close={() => { if (!busy) setAdding(false); }} add={issue => void membership(issue, true)}/>}
  </section>;
}
function AddProjectIssues({ project, session, included, busy, error, close, add }: { project: Project; session: Session; included: string[]; busy: boolean; error: unknown; close: () => void; add: (issue: Issue) => void }) {
  const [query, setQuery] = useState(''); const scope = useSessionScope(session);
  const results = useSearchPage<Issue, IssuePage>(`/workspaces/${project.workspace_id}/issues?team_id=${encodeURIComponent(project.team_id)}`, scope, query, true);
  return <Modal title="Add an issue to this project" close={close} canClose={!busy}><div className="project-form"><p className="project-help">Choose an issue from this team. Issues already belonging to another project must be removed there first.</p><label className="field">Find an issue<input data-autofocus value={query} onChange={event => setQuery(event.target.value)} placeholder="Search by title or issue key" disabled={busy}/></label><ErrorBanner error={error}/>{error instanceof ApiError && error.status === 412 && <p className="notice">Project scope changed. The latest version is loaded. Review your selection and try again.</p>}<ErrorBanner error={results.error} retry={results.refresh}/><div className="project-candidates" aria-busy={results.loading}>{results.items.map(issue => <button className="project-candidate" key={issue.id} onClick={() => add(issue)} disabled={busy || included.includes(issue.id)}><span className="project-issue-key">{issue.key}</span><span>{issue.title}</span><small>{included.includes(issue.id) ? 'In project' : 'Add'}</small></button>)}</div>{results.loading && <p role="status" className="project-help">Finding issues…</p>}{!results.loading && !results.error && !results.items.length && <p className="project-help">No matching issues in this team.</p>}{results.page?.has_more && <button className="button compact project-more" disabled={results.loading || busy} onClick={results.more}>Load more results</button>}<div className="modal-footer"><button className="button" disabled={busy} onClick={close}>Done</button></div></div></Modal>;
}
