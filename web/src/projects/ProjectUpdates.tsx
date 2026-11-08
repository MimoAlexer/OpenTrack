import { useEffect, useId, useRef, useState, type FormEvent } from 'react';
import { ApiError } from '../api';
import { Avatar, ErrorBanner, Loading } from '../components';
import { isAccessError } from '../documents/model';
import { useUnsavedChanges } from '../router';
import { useScopedRequest } from '../settings/shared';
import type { Session } from '../types';
import type { components } from '../generated/api';
import { ProjectUpdateComparison, ProjectUpdateContext } from './ProjectUpdateContext';
import { ProjectUpdateNotesEditor } from './ProjectUpdateNotesEditor';
import { copyNotes, emptyNotes, notesProblem, updateDate, updateTime, type UpdateNotes } from './updateContext';
import { healthLabels, projectMetadataPath, projectPath, states, type Project, type ProjectUpdate } from './types';
import { useProjectPage } from './useProjectData';

interface Props { project: Project; session: Session; refresh: number; readOnly: boolean; denied: (error: unknown) => void; changed?: (project: Project) => void; published?: (update: ProjectUpdate) => void }
interface PendingUpdate { body: components['schemas']['CreateProjectUpdate']; key: string; etag: string }

export function ProjectUpdates({ project, session, refresh, readOnly, denied, changed, published }: Props) {
  const path = `${projectPath(project.workspace_id, project.team_id, project.id)}/updates`; const request = useScopedRequest();
  const prefix = useId(); const updateId = (id: string) => `${prefix}-update-${id}`;
  const [revision, setRevision] = useState(0); const directory = useProjectPage<ProjectUpdate>(path, refresh + revision);
  const [body, setBody] = useState(''); const [includeNotes, setIncludeNotes] = useState(false); const [notes, setNotes] = useState<UpdateNotes>(emptyNotes);
  const [error, setError] = useState<unknown>(null); const [accessError, setAccessError] = useState<unknown>(null); const [busy, setBusy] = useState(false); const [uncertain, setUncertain] = useState(false); const [notice, setNotice] = useState('');
  const [currentProject, setCurrentProject] = useState(project); const [reviewRequired, setReviewRequired] = useState(false); const [review, setReview] = useState<Project | null>(null);
  const pending = useRef<PendingUpdate | null>(null); const flight = useRef(false); const notesInitialized = useRef(false);
  const problem = includeNotes ? notesProblem(notes) : null;
  useUnsavedChanges(!!body.trim() || includeNotes || uncertain);
  useEffect(() => { if (!uncertain && !reviewRequired) setCurrentProject(project); }, [project.id, project.etag]);
  useEffect(() => { if (isAccessError(directory.error)) { setAccessError(directory.error); denied(directory.error); } }, [directory.error]);
  function toggleNotes(value: boolean) {
    if (value && !notesInitialized.current) {
      setNotes(copyNotes(directory.value?.data[0]?.context?.notes ?? emptyNotes())); notesInitialized.current = true;
    }
    setIncludeNotes(value);
  }
  async function loadCurrentProject() {
    try { setReview(await request<Project>(projectMetadataPath(project.workspace_id, project.team_id, project.id))); setError(null); }
    catch (error) { setError(error); if (isAccessError(error)) { setAccessError(error); denied(error); } }
  }
  async function submit(event: FormEvent) {
    event.preventDefault(); if (flight.current || busy || !body.trim() || readOnly || !currentProject.can_edit || currentProject.archived_at || reviewRequired || problem) return;
    flight.current = true;
    if (!uncertain || !pending.current) pending.current = { body: { body: body.trim(), ...(includeNotes ? { notes: copyNotes(notes) } : {}) }, key: crypto.randomUUID(), etag: currentProject.etag };
    setBusy(true); setError(null); setNotice('');
    try {
      const update = await request<ProjectUpdate>(path, { method: 'POST', csrf: session.csrf_token, headers: { 'Idempotency-Key': pending.current.key, 'If-Match': pending.current.etag }, body: pending.current.body });
      setBody(''); setIncludeNotes(false); setNotes(emptyNotes()); notesInitialized.current = false; setUncertain(false); pending.current = null; setRevision(value => value + 1); setNotice('Update published.'); published?.(update);
    } catch (error) {
      setError(error);
      if (isAccessError(error)) { setAccessError(error); denied(error); }
      setUncertain(error instanceof ApiError && (error.status === 0 || error.status === 408 || error.status >= 500));
      if (error instanceof ApiError && error.status === 412) { pending.current = null; setReviewRequired(true); setReview(null); await loadCurrentProject(); }
    } finally { setBusy(false); flight.current = false; }
  }
  if (accessError || isAccessError(directory.error)) return <section aria-label="Project updates"><ErrorBanner error={accessError || directory.error}/></section>;
  const writable = !readOnly && currentProject.can_edit && !currentProject.archived_at;
  return <section className="project-updates" aria-label="Project updates"><header className="project-section-heading"><div><h3>Updates</h3><p>Written progress, with state, health and project dates captured when published.</p></div></header>
    {writable && <form className="project-update-composer" onSubmit={submit}>
      <fieldset disabled={busy || uncertain || !!review && (!review.can_edit || !!review.archived_at)}><label className="field">Write an update<textarea name="project-update" value={body} onChange={event => setBody(event.target.value)} rows={4} placeholder="What changed? What needs attention?" disabled={busy || uncertain || !!review && (!review.can_edit || !!review.archived_at)}/></label>
        <p className="muted">Dates to capture: {updateDate(currentProject.start_date)} → {updateDate(currentProject.target_date)}.</p>
        <label className="project-update-include-notes"><input type="checkbox" checked={includeNotes} onChange={event => toggleNotes(event.target.checked)}/>Record reviewed risks and decisions in this update</label>
        {includeNotes ? <><p>Review these lists before publishing. Empty lists explicitly record no risks or decisions.</p><ProjectUpdateNotesEditor notes={notes} change={setNotes}/></> : <p className="muted">Risks and decisions will be left unspecified.</p>}
      </fieldset>
      <ErrorBanner error={error}/>{problem && <p role="alert" className="project-validation">{problem}</p>}
      {uncertain && <p className="notice">The response was interrupted. Retry safely to confirm this update without publishing a duplicate.</p>}
      {reviewRequired && <section className="project-update-review" aria-label="Review current project context"><h4>The project changed before publication</h4><p>Your update and notes are preserved. Review the current project context before trying again.</p>{review && <><p>{states[review.state]} · {healthLabels[review.health]}</p><p>Start: {updateDate(review.start_date)} · Target: {updateDate(review.target_date)}</p><button type="button" className="button compact" disabled={busy || !review.can_edit || !!review.archived_at} onClick={() => { setCurrentProject(review); changed?.(review); setReviewRequired(false); setReview(null); setError(null); }}>Use current project context</button>{(!review.can_edit || review.archived_at) && <p>This project is currently read-only. Your draft remains preserved.</p>}</>}<button type="button" className="button compact" disabled={busy} onClick={async () => { setReview(null); setBusy(true); try { await loadCurrentProject(); } finally { setBusy(false); } }}>Reload current project context</button></section>}
      <div className="project-update-actions"><span>{Array.from(body).length.toLocaleString()} / 8,000</span><button className="button primary compact" disabled={busy || !body.trim() || reviewRequired || !!problem}>{busy ? 'Publishing…' : uncertain ? 'Retry safely' : 'Publish update'}</button></div>
    </form>}
    <p className="project-save-notice" role="status">{notice}</p><ErrorBanner error={directory.error} retry={directory.retry}/>
    {!directory.value && directory.loading ? <Loading label="Loading project updates…"/> : <div className="project-update-feed" aria-busy={directory.loading}>{directory.value?.data.map((update, index, updates) => {
      const previous = 'previous' in update ? update.previous : updates[index + 1];
      const comparedTarget = previous && updates.some(item => item.id === previous.id) ? updateId(previous.id) : undefined;
      return <article id={updateId(update.id)} tabIndex={-1} className="project-update" key={update.id}><header><Avatar small user={{ display_name: update.author_name }}/><strong>{update.author_name}</strong><time dateTime={update.created_at}>{updateTime(update.created_at)}</time></header><p className="project-update-body">{update.body}</p><footer><span className={`project-health health-${update.health}`}>{healthLabels[update.health]}</span><span>{states[update.state]}</span><span>{update.eligible ? `${update.completed} of ${update.eligible} completed` : 'No eligible issue scope'}</span></footer><ProjectUpdateContext context={update.context}/>{previous ? <ProjectUpdateComparison update={update} previous={previous} targetId={comparedTarget}/> : <p>{!('previous' in update) && directory.value?.has_more ? 'Load more updates to compare this update.' : 'No next update shown.'}</p>}</article>;
    })}{!directory.value?.data.length && !directory.error && <div className="project-empty"><h4>No updates yet</h4><p>Publish a short update when plans change or work reaches a milestone.</p></div>}</div>}
    {directory.value?.has_more && <button className="button compact project-more" disabled={directory.loading} onClick={directory.more}>{directory.loading ? 'Loading…' : 'Load more updates'}</button>}
  </section>;
}
