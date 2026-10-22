import { useRef, useState, type FormEvent } from 'react';
import { ApiError } from '../api';
import { ErrorBanner, Modal } from '../components';
import { StructuredEditor } from '../documents/Editor';
import { canonicalBody, EMPTY_BODY, hasContent, isAccessError, type RichBody } from '../documents/model';
import { useUnsavedChanges } from '../router';
import { useScopedRequest } from '../settings/shared';
import type { Session, Team } from '../types';
import { ProjectLead } from './ProjectLead';
import { healthLabels, projectMetadata, projectMetadataPath, projectPath, states, type FullProject, type Project, type ProjectHealth, type ProjectState } from './types';

type Mode = 'edit' | 'archive' | 'restore';
interface Props { workspaceId: string; team: Team; session: Session; project?: Project; mode?: Mode; pendingBrief?: boolean; close: () => void; saved: (project: Project) => void; denied: (error: unknown) => void }
export function ProjectEditor({ workspaceId, team, session, project, mode = 'edit', pendingBrief = false, close, saved, denied }: Props) {
  const request = useScopedRequest(); const [base, setBase] = useState(project);
  const [name, setName] = useState(project?.name ?? ''); const [summary, setSummary] = useState(project?.summary ?? ''); const [body, setBody] = useState<RichBody>(EMPTY_BODY);
  const [lead, setLead] = useState(project?.lead_id ?? null); const [state, setState] = useState<ProjectState>(project?.state ?? 'planned'); const [health, setHealth] = useState<ProjectHealth>(project?.health ?? 'on_track');
  const [start, setStart] = useState(project?.start_date ?? ''); const [target, setTarget] = useState(project?.target_date ?? '');
  const [acknowledged, setAcknowledged] = useState(false); const [busy, setBusy] = useState(false); const [uncertain, setUncertain] = useState(false); const [error, setError] = useState<unknown>(null);
  const [latest, setLatest] = useState<Project | null>(null); const [discard, setDiscard] = useState(false);
  const sent = useRef<{ key: string; body: object; etag?: string } | null>(null); const committed = useRef(false);
  const dirty = base ? name !== base.name || summary !== base.summary || lead !== base.lead_id || state !== base.state || health !== base.health || start !== (base.start_date ?? '') || target !== (base.target_date ?? '') : !!(name || summary || lead || start || target || hasContent(body));
  const release = useUnsavedChanges(!committed.current && (dirty || uncertain));
  const completing = !!base && base.state !== 'completed' && state === 'completed';
  const writable = !base || (mode === 'edit' ? base.can_edit && !base.archived_at : base.can_manage);
  const validDates = !start || !target || start <= target;
  function dismiss() { if (busy || uncertain) return; if (dirty) setDiscard(true); else close(); }
  function reconcile(value: Project, keep: boolean) {
    setBase(value); setLatest(null); setUncertain(false); setError(null); sent.current = null; setAcknowledged(false);
    if (!keep) { setName(value.name); setSummary(value.summary); setLead(value.lead_id); setState(value.state); setHealth(value.health); setStart(value.start_date ?? ''); setTarget(value.target_date ?? ''); }
  }
  async function readLatest() {
    if (!base) return;
    setBusy(true);
    try { setLatest(await request<Project>(projectMetadataPath(workspaceId, team.id, base.id))); setUncertain(false); }
    catch (error) { setError(error); if (isAccessError(error)) denied(error); }
    finally { setBusy(false); }
  }
  async function submit(event: FormEvent) {
    event.preventDefault(); if (busy || !writable || latest || committed.current || mode !== 'edit' && pendingBrief || mode === 'edit' && (!name.trim() || !validDates || completing && !acknowledged)) return;
    if (!uncertain || !sent.current) {
      let fields: object;
      if (mode !== 'edit') fields = { archived: mode === 'archive' };
      else {
        let narrative: RichBody | undefined;
        try { if (!base) narrative = canonicalBody(body); }
        catch (error) { setError(error); return; }
        fields = { name: name.trim(), summary: summary.trim(), ...(!base ? { narrative: { schema_version: 1, body: narrative } } : {}), ...(!base || lead !== base.lead_id ? { lead_id: lead } : {}), start_date: start || null, target_date: target || null, ...(base ? { state, health, acknowledge_open_issues: completing && acknowledged } : {}) };
      }
      sent.current = { key: crypto.randomUUID(), body: fields, etag: base?.etag };
    }
    const pending = sent.current; setBusy(true); setError(null);
    try {
      const value = base
        ? await request<Project>(projectMetadataPath(workspaceId, team.id, base.id), { method: 'PATCH', csrf: session.csrf_token, body: pending.body, headers: { 'If-Match': pending.etag! } })
        : projectMetadata(await request<FullProject>(projectPath(workspaceId, team.id), { method: 'POST', csrf: session.csrf_token, body: pending.body, headers: { 'Idempotency-Key': pending.key } }));
      committed.current = true; release(); saved(value);
    } catch (error) {
      setError(error);
      if (isAccessError(error)) { denied(error); return; }
      setUncertain(error instanceof ApiError && (error.status === 0 || error.status === 408 || error.status >= 500));
      if (error instanceof ApiError && error.status === 412 && base) {
        try { setLatest(await request<Project>(projectMetadataPath(workspaceId, team.id, base.id))); }
        catch (readError) { if (isAccessError(readError)) denied(readError); }
      }
    } finally { setBusy(false); }
  }
  const action = mode === 'archive' ? 'Archive project' : mode === 'restore' ? 'Restore project' : base ? 'Save project' : 'Create project';
  return <><Modal title={mode === 'edit' ? base ? 'Edit project' : 'New project' : action} close={dismiss} canClose={!busy && !uncertain}><form className="project-form" onSubmit={submit}>
    <ErrorBanner error={error}/>
    {uncertain && <div className="notice" role="status">The response was interrupted. Your draft is preserved. Retry the same request to confirm its outcome.{base && <button type="button" className="button compact" onClick={() => void readLatest()} disabled={busy}>Review saved project</button>}</div>}
    {latest && <section className="project-conflict" role="alert"><h3>This project changed.</h3><p>Your changes have not been overwritten. Compare them with the saved version before continuing.</p><div className="project-comparison"><div><h4>Latest saved version</h4><strong>{latest.name}</strong><p>{latest.summary || 'No summary'}</p><p>{states[latest.state]} · {healthLabels[latest.health]}{latest.archived_at ? ' · Archived' : ''}</p><p>{latest.start_date || 'No start'} → {latest.target_date || 'No target'}</p></div><div><h4>Your draft</h4><strong>{name}</strong><p>{summary || 'No summary'}</p><p>{states[state]} · {healthLabels[health]}</p><p>{start || 'No start'} → {target || 'No target'}</p></div></div><div className="button-row"><button type="button" className="button compact" onClick={() => reconcile(latest, false)}>Use latest version</button><button type="button" className="button compact" disabled={!latest.can_edit || !!latest.archived_at} onClick={() => reconcile(latest, true)}>Keep my draft for review</button></div></section>}
    {!writable && <p className="notice">This project is read-only with your current access. Your draft is preserved.</p>}
    {mode !== 'edit' && pendingBrief && <p className="notice">Close this dialog and save or discard the pending project brief before changing archive state.</p>}
    {mode !== 'edit' ? <p>{mode === 'archive' ? `Archive “${base?.name}”? It will move out of active projects. Its issues keep their current states.` : `Restore “${base?.name}” to active projects?`}</p> : <fieldset disabled={busy || uncertain || !writable}>
      <p className="project-form-team">Owning team <strong>{team.name}</strong></p>
      <label className="field">Name<input name="project-name" value={name} onChange={event => setName(event.target.value)} required maxLength={160} data-autofocus placeholder="Project name"/></label>
      <label className="field">Summary<input name="project-summary" value={summary} onChange={event => setSummary(event.target.value)} maxLength={500} placeholder="Short summary"/></label>
      {base && <div className="form-grid"><label className="field">State<select aria-label="Project state" value={state} onChange={event => { setState(event.target.value as ProjectState); setAcknowledged(false); }}>{Object.entries(states).map(([value, label]) => <option key={value} value={value}>{label}</option>)}</select></label><label className="field">Health<select aria-label="Project health" value={health} onChange={event => setHealth(event.target.value as ProjectHealth)}>{Object.entries(healthLabels).map(([value, label]) => <option key={value} value={value}>{label}</option>)}</select></label></div>}
      <div className="project-lead-field"><ProjectLead workspaceId={workspaceId} teamId={team.id} session={session} value={lead} current={base?.lead ?? null} disabled={busy || uncertain || !writable} change={setLead}/></div>
      <div className="form-grid"><label className="field">Start date<input name="project-start" aria-label="Start date" type="date" value={start} onChange={event => setStart(event.target.value)}/></label><label className="field">Target date<input name="project-target" aria-label="Target date" type="date" value={target} min={start || undefined} onChange={event => setTarget(event.target.value)}/></label></div>
      {!validDates && <p className="project-validation" role="alert">Target date must be on or after the start date.</p>}
      {!base && <section className="project-narrative-field"><h3>Project brief</h3><StructuredEditor value={body} onChange={setBody} label="Project brief" compact disabled={busy || uncertain || !writable}/></section>}
      {completing && <label className="project-acknowledgement"><input type="checkbox" checked={acknowledged} onChange={event => setAcknowledged(event.target.checked)}/>I understand that completing this project leaves its issues in their current states.</label>}
    </fieldset>}
    <div className="modal-footer"><button type="button" className="button" onClick={dismiss} disabled={busy || uncertain}>Cancel</button><button className="button primary" disabled={busy || !writable || !!latest || mode !== 'edit' && pendingBrief || mode === 'edit' && (!name.trim() || !validDates || completing && !acknowledged)}>{busy ? 'Saving…' : uncertain ? 'Retry safely' : action}</button></div>
  </form></Modal>{discard && <Modal title="Discard project changes?" close={() => setDiscard(false)}><div className="project-form"><p>Your unsaved project changes will be lost.</p><div className="modal-footer"><button className="button" onClick={() => setDiscard(false)}>Keep editing</button><button className="button" onClick={() => { release(); close(); }}>Discard changes</button></div></div></Modal>}</>;
}
