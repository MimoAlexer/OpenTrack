import { useEffect, useRef, useState } from 'react';
import { Plus, X } from 'lucide-react';
import { ApiError } from '../api';
import { ErrorBanner, Loading, Modal } from '../components';
import { useSessionScope } from '../desktop/context';
import { isAccessError } from '../documents/model';
import { issuesUrl, Link, useUnsavedChanges } from '../router';
import { useScopedRequest } from '../settings/shared';
import { useSearchPage } from '../taxonomy/useSearchPage';
import type { Issue, IssuePage, Session } from '../types';
import { projectMetadata, projectMetadataPath, projectPath, type FullProject, type Page, type Project, type ProjectIssue } from './types';
import type { Milestone, MilestoneOverview } from './Milestones';
import { useProjectPage } from './useProjectData';

type Props = { milestones?: MilestoneOverview[]; project: Project; session: Session; refresh: number; readOnly: boolean; changed: (value: Project) => void; denied: (error: unknown) => void };
type Assignment = { issue: { id: string; key?: string; title?: string }; included: boolean; milestone?: string };
type AssignmentReview = { intent: Assignment; current?: Project; observed?: boolean; milestoneTitle?: string; currentMilestoneTitle?: string; included?: boolean };

export function ProjectIssues(props: Props) {
  const scope = useSessionScope(props.session); const project = props.project;
  return <ScopedProjectIssues key={`${scope}:${project.workspace_id}:${project.team_id}:${project.id}`} {...props}/>;
}
function ScopedProjectIssues({ project, session, refresh, readOnly, changed, denied, milestones = [] }: Props) {
  const path = projectPath(project.workspace_id, project.team_id, project.id); const request = useScopedRequest();
  const directory = useProjectPage<ProjectIssue>(`${path}/issues`, refresh, 50);
  const [grouped, setGrouped] = useState(false);
  const [adding, setAdding] = useState(false); const [busy, setBusy] = useState<string | null>(null); const [error, setError] = useState<unknown>(null);
  const [review, setReview] = useState<AssignmentReview | null>(null); const [unavailable, setUnavailable] = useState(false); const flight = useRef(false);
  const container = useRef<HTMLElement>(null); const returnFocus = useRef(false);
  useUnsavedChanges(!!busy || !!review);
  useEffect(() => {
    if (returnFocus.current && !review && !busy && !adding && !directory.loading) { container.current?.focus(); returnFocus.current = false; }
  }, [review, busy, adding, directory.loading, directory.value]);
  function closeReview() {
    if (busy) return; returnFocus.current = !adding || !!review?.observed; setReview(null); if (review?.observed) setAdding(false);
  }
  useEffect(() => { if (isAccessError(directory.error)) { setReview(null); setUnavailable(true); denied(directory.error); } }, [directory.error]);
  async function recover(intent: Assignment) {
    setReview({ intent });
    try {
      const signal = AbortSignal.timeout(10_000);
      const metadataPath = projectMetadataPath(project.workspace_id, project.team_id, project.id);
      const issuePath = `/workspaces/${encodeURIComponent(project.workspace_id)}/issues/${encodeURIComponent(intent.issue.id)}`;
      const current = await request<Project>(metadataPath, { signal });
      if (current.id !== project.id || current.workspace_id !== project.workspace_id || current.team_id !== project.team_id || !current.etag) throw new Error('The project scope changed. Reload the project.');
      const issue = await request<Issue>(issuePath, { signal });
      if (issue.id !== intent.issue.id || issue.workspace_id !== project.workspace_id || issue.team_id !== project.team_id || !issue.etag || issue.archived_at) throw new Error('This issue is no longer eligible for assignment review. No retry was sent.');
      const items: ProjectIssue[] = []; const cursors = new Set<string>(); let cursor: string | null = null; let complete = false;
      for (let pageIndex = 0; pageIndex < 10; pageIndex += 1) {
        const page: Page<ProjectIssue> = await request(`${path}/issues?${new URLSearchParams({ limit: '100', ...(cursor ? { cursor } : {}) })}`, { signal });
        if (typeof page.has_more !== 'boolean' || !(page.next_cursor === null || typeof page.next_cursor === 'string') || !Array.isArray(page.data) || page.data.length > 100 || page.data.some(item => typeof item?.id !== 'string' || !item.id.trim() || !Object.hasOwn(item, 'milestone_id') || !(item.milestone_id === null || typeof item.milestone_id === 'string' && !!item.milestone_id.trim()) || items.some(previous => previous.id === item.id)) || new Set(page.data.map(item => item.id)).size !== page.data.length) throw new Error('The current project scope could not be verified. Reload current assignment.');
        items.push(...page.data);
        if (!page.has_more) {
          if (page.next_cursor) throw new Error('The project issue cursor is inconsistent. Reload current assignment.');
          complete = true; break;
        }
        if (!page.data.length || !page.next_cursor || cursors.has(page.next_cursor)) throw new Error('The project issue cursor is incomplete or repeated. Reload current assignment.');
        cursor = page.next_cursor; cursors.add(cursor);
      }
      if (!complete) throw new Error('Assignment remains unresolved beyond the 1,000-issue review limit. No retry was sent.');
      let target: Milestone | null = null;
      if (intent.milestone) {
        try { target = await request<Milestone>(`${path}/milestones/${encodeURIComponent(intent.milestone)}`, { signal }); }
        catch (error) {
          if (error instanceof ApiError && error.status === 404) {
            const access = await request<Project>(metadataPath, { signal });
            if (access.id !== project.id || access.workspace_id !== project.workspace_id || access.team_id !== project.team_id || !access.etag) throw new Error('The project scope could not be verified. Reload current assignment.');
            throw new Error('The selected milestone is no longer available. Discard this assignment and choose another.');
          }
          throw error;
        }
        if (target.id !== intent.milestone || target.workspace_id !== project.workspace_id || target.team_id !== project.team_id || target.project_id !== project.id || !target.can_assign) throw new Error('The selected milestone is unavailable for this project. No retry was sent.');
      }
      const member = items.find(item => item.id === issue.id);
      let currentMilestone: Milestone | null = null;
      if (member?.milestone_id) {
        currentMilestone = target?.id === member.milestone_id ? target : await request<Milestone>(`${path}/milestones/${encodeURIComponent(member.milestone_id)}`, { signal });
        if (currentMilestone.id !== member.milestone_id || currentMilestone.workspace_id !== project.workspace_id || currentMilestone.team_id !== project.team_id || currentMilestone.project_id !== project.id) throw new Error('The current milestone could not be verified. Reload current assignment.');
      }
      const afterIssue = await request<Issue>(issuePath, { signal });
      const after = await request<Project>(metadataPath, { signal });
      if (after.id !== current.id || after.workspace_id !== current.workspace_id || after.team_id !== current.team_id || after.etag !== current.etag || afterIssue.id !== issue.id || afterIssue.workspace_id !== issue.workspace_id || afterIssue.team_id !== issue.team_id || afterIssue.etag !== issue.etag || afterIssue.archived_at) throw new Error('The project or issue changed during review. Reload current assignment.');
      const observed = intent.milestone !== undefined ? !!member && member.milestone_id === (intent.milestone || null) : !!member === intent.included;
      setReview({ intent: { ...intent, issue }, current: after, observed, included: !!member, currentMilestoneTitle: currentMilestone?.title, milestoneTitle: target?.title });
      changed(after); setError(null);
    } catch (error) { setError(error); if (isAccessError(error)) { setReview(null); setAdding(false); setUnavailable(true); denied(error); } }
  }
  async function membership(issue: Assignment['issue'], included: boolean, milestone?: string, etag = project.etag) {
    if (flight.current || readOnly) return; flight.current = true; setBusy(issue.id); setError(null);
    try { const value = await request<FullProject>(`${path}/issues/${issue.id}${milestone !== undefined ? '/milestone' : ''}`, { method: 'PUT', csrf: session.csrf_token, headers: { 'If-Match': etag }, body: milestone !== undefined ? { milestone_id: milestone || null } : { included } }); changed(projectMetadata(value)); if (review) returnFocus.current = true; setReview(null); if (included && milestone === undefined) setAdding(false); }
    catch (error) {
      setError(error); if (isAccessError(error)) { setReview(null); setAdding(false); setUnavailable(true); denied(error); }
      else if (!(error instanceof ApiError) || [0, 408, 412].includes(error.status) || error.status >= 500) await recover({ issue, included, milestone });
    } finally { flight.current = false; setBusy(null); }
  }
  if (unavailable || isAccessError(directory.error)) return <section aria-label="Project issues"><h3>Project issues unavailable</h3><ErrorBanner error={error || directory.error}/></section>;
  return <section ref={container} tabIndex={-1} className="project-issues" aria-label="Project issues"><header className="project-section-heading"><div><h3>Issues</h3><p>The work included in this project.</p></div>{!readOnly && <button className="button compact" disabled={!!busy || !!review} onClick={() => { setError(null); setAdding(true); }}><Plus aria-hidden="true"/>Add issues</button>}</header>
    {!adding && <ErrorBanner error={error}/>}{!adding && error instanceof ApiError && error.status === 412 && <p className="notice">The project changed. The latest version is loaded; review its issues before trying again.</p>}
    {milestones.length > 0 && <label className="planning-group-toggle"><input type="checkbox" checked={grouped} onChange={event => setGrouped(event.target.checked)}/>Group by milestone</label>}
    <ErrorBanner error={directory.error} retry={directory.retry}/>
    {!directory.value && directory.loading ? <Loading label="Loading project issues…"/> : <div className="project-issue-list" aria-busy={directory.loading}>{(grouped ? [...milestones.map(item => ({ id: item.milestone.id, title: item.milestone.title })), { id: null, title: 'No milestone' }] : [{ id: undefined, title: '' }]).map(group => <div className="project-issue-group" key={group.id ?? 'ungrouped'}>{grouped && <h4>{group.title}</h4>}{directory.value?.data.filter(issue => !grouped || (issue.milestone_id ?? null) === group.id).map(issue => <div className="project-issue-row" key={issue.id}><Link href={issuesUrl(project.workspace_id, new URLSearchParams({ team_id: project.team_id }), issue.id)}><span className="project-issue-key">{issue.key}</span><span>{issue.title}</span></Link><span className={`project-issue-status status-${issue.status_category}`}>{issue.status_name}</span>{!readOnly && <select aria-label={`Milestone for ${issue.key}`} value={issue.milestone_id ?? ''} disabled={!!busy || !!review} onChange={event => void membership(issue, true, event.target.value)}><option value="">No milestone</option>{milestones.map(({ milestone }) => <option value={milestone.id} key={milestone.id}>{milestone.title}</option>)}</select>}{!readOnly && <button className="icon-button" aria-label={`Remove ${issue.key} from project`} disabled={!!busy || !!review} onClick={() => void membership(issue, false)}><X aria-hidden="true"/></button>}</div>)}</div>)}{!directory.value?.data.length && !directory.error && <div className="project-empty"><h4>No issues yet</h4><p>Add existing issues from this team to define the project scope.</p></div>}</div>}
    {grouped && directory.value?.has_more && <p className="project-help">Groups show the issues loaded so far. Load more to see the remaining project scope.</p>}{directory.value?.has_more && <button className="button compact project-more" disabled={directory.loading} onClick={directory.more}>{directory.loading ? 'Loading…' : 'Load more issues'}</button>}
    {adding && <AddProjectIssues project={project} session={session} included={directory.value?.data.map(item => item.id) ?? []} busy={!!busy || !!review} error={error} close={() => { if (!busy && !review) setAdding(false); }} add={issue => void membership(issue, true)}/>}
    {review && <Modal title="Review project assignment" close={closeReview} canClose={!busy}><div className="project-form"><p>{review.intent.milestone !== undefined ? `Assign ${review.intent.issue.key ?? 'this issue'} to ${review.milestoneTitle ?? (review.intent.milestone ? 'the selected milestone' : 'no milestone')}.` : `${review.intent.included ? 'Include' : 'Remove'} ${review.intent.issue.key ?? 'this issue'} ${review.intent.included ? 'in' : 'from'} this project.`} Your selection is preserved for review.</p><ErrorBanner error={error}/>{busy ? <p role="status">Checking current assignment…</p> : review.observed ? <p role="status">The requested assignment is present in the current state. This does not identify which request or person applied it.</p> : review.current ? <p>Current state: {review.included ? 'in this project' : 'not in this project'}.{review.included && ` Current milestone: ${review.currentMilestoneTitle ?? 'No milestone'}.`} Apply only after reviewing this current project version.</p> : <p>The outcome remains unresolved. Reload current assignment before any retry.</p>}<div className="modal-footer"><button type="button" className="button" disabled={!!busy} onClick={closeReview}>{review.observed ? 'Done' : 'Discard assignment'}</button><button type="button" className="button" disabled={!!busy} onClick={async () => { if (flight.current) return; flight.current = true; setBusy(review.intent.issue.id); try { await recover(review.intent); } finally { flight.current = false; setBusy(null); } }}>Reload current assignment</button>{!review.observed && <button type="button" className="button primary" disabled={!!busy || readOnly || !review.current?.can_edit} onClick={() => { if (review.current) void membership(review.intent.issue, review.intent.included, review.intent.milestone, review.current.etag); }}>Apply reviewed assignment</button>}</div></div></Modal>}
  </section>;
}
function AddProjectIssues({ project, session, included, busy, error, close, add }: { project: Project; session: Session; included: string[]; busy: boolean; error: unknown; close: () => void; add: (issue: Issue) => void }) {
  const [query, setQuery] = useState(''); const scope = useSessionScope(session);
  const results = useSearchPage<Issue, IssuePage>(`/workspaces/${project.workspace_id}/issues?team_id=${encodeURIComponent(project.team_id)}`, scope, query, true);
  return <Modal title="Add an issue to this project" close={close} canClose={!busy}><div className="project-form"><p className="project-help">Choose an issue from this team. Issues already belonging to another project must be removed there first.</p><label className="field">Find an issue<input data-autofocus value={query} onChange={event => setQuery(event.target.value)} placeholder="Search by title or issue key" disabled={busy}/></label><ErrorBanner error={error}/>{error instanceof ApiError && error.status === 412 && <p className="notice">Project scope changed. The latest version is loaded. Review your selection and try again.</p>}<ErrorBanner error={results.error} retry={results.refresh}/><div className="project-candidates" aria-busy={results.loading}>{results.items.map(issue => <button className="project-candidate" key={issue.id} onClick={() => add(issue)} disabled={busy || included.includes(issue.id)}><span className="project-issue-key">{issue.key}</span><span>{issue.title}</span><small>{included.includes(issue.id) ? 'In project' : 'Add'}</small></button>)}</div>{results.loading && <p role="status" className="project-help">Finding issues…</p>}{!results.loading && !results.error && !results.items.length && <p className="project-help">No matching issues in this team.</p>}{results.page?.has_more && <button className="button compact project-more" disabled={results.loading || busy} onClick={results.more}>Load more results</button>}<div className="modal-footer"><button className="button" disabled={busy} onClick={close}>Done</button></div></div></Modal>;
}
