import { ReferenceProvider } from '../documents/ReferenceProvider';
import { TemplateChooser } from '../templates/TemplateChooser';
import { previewProject, TemplateDiagnostics } from '../templates/TemplatePreview';
import { ProjectPicker, projectSignature, type ProjectChoice } from '../templates/ProjectPicker';
import { applyTemplate, templateBody, type TemplateField } from '../templates/model';
import type { TemplateDiagnostic, TemplatePreview, TemplateSelection } from '../templates/types';
import { DiscardUploadDraft } from '../attachments/DiscardUploadDraft';
import { AttachmentProvider } from '../attachments/context';
import type { AttachmentDraftState } from '../attachments/types';
import { useEffect, useRef, useState, type FormEvent } from 'react';
import { useUnsavedChanges } from '../router';
import { StructuredEditor } from '../documents/Editor';
import { EMPTY_BODY, hasContent, attachmentIds, type RichBody } from '../documents/model';
import { LoaderCircle } from 'lucide-react';
import { ApiError } from '../api';
import { ErrorBanner, Modal } from '../components';
import type { Bootstrap, Issue, IssueDraft, Session } from '../types';
import { useScopedRequest } from '../settings/shared';
import { useSessionScope } from '../desktop/context';
import { IssueFields } from './IssueFields';
import { useIssueWorkflow } from '../workflows/useIssueWorkflow';
import { defaultStatus, selectable } from '../workflows/model';
import { IssueOrderProgress, orderMaintenance, type OrderMaintenance } from './IssueOrdering';
import type { Workflow, WorkflowStatus } from '../workflows/types';

export function CreateIssue({ workspaceId, data, session, initialTeam, close, created, workflowChanged, parent }: {
  workspaceId: string; data: Bootstrap; session: Session; initialTeam?: string; parent?: Issue; workflowChanged?: (workflow: Workflow) => void; close: () => void; created: (issue: Issue) => void;
}) {
  const request = useScopedRequest();
  const scope = useSessionScope(session);
  const [parentIssue, setParentIssue] = useState(parent); const [parentReview, setParentReview] = useState(false); const [discarding, setDiscarding] = useState(false);
  initialTeam = parent?.team_id ?? initialTeam;
  const activeTeams = data.teams.filter(team => !team.archived_at);
  const [teamId, setTeam] = useState(activeTeams.some(team => team.id === initialTeam) ? initialTeam! : activeTeams[0]?.id || '');
  const seededDefault = (team: string) => defaultStatus(data.statuses.filter(status => status.team_id === team) as WorkflowStatus[])?.id ?? '';
  const [uploads, setUploads] = useState<AttachmentDraftState | null>(null);
  const [body, setBody] = useState<RichBody>(EMPTY_BODY);
  const [template, setTemplate] = useState<TemplateSelection | null>(null);
  const [templateName, setTemplateName] = useState(''); const [templateReview, setTemplateReview] = useState(false);
  const [templateDiagnostics, setTemplateDiagnostics] = useState<TemplateDiagnostic[]>([]);
  const [project, setProject] = useState<ProjectChoice | null>(null);
  const [projectEdited, setProjectEdited] = useState(false);
  const [projectDenied, setProjectDenied] = useState(false);
  const [verifiedProject, setVerifiedProject] = useState<string | null>(null);
  const [projectReview, setProjectReview] = useState(false); const [projectRefresh, setProjectRefresh] = useState(0);
  const editedFields = useRef(new Set<TemplateField>());
  const [composing, setComposing] = useState(false);
  const [useDefault, setUseDefault] = useState(true);
  const [draft, setDraft] = useState<IssueDraft>({ title: '', description: '', status_id: seededDefault(teamId), priority: 0, assignee_id: null, label_ids: [], estimate: null, due_date: null });
  const workflow = useIssueWorkflow(workspaceId, teamId, session, data, useDefault ? undefined : draft.status_id, workflowChanged);
  const currentDefault = defaultStatus(workflow.statuses);
  const validExplicitStatus = workflow.statuses.some(status => status.id === draft.status_id && workflow.currentIds.has(status.id) && selectable(status));
  const unavailable = workflow.error instanceof ApiError && [401, 403, 404].includes(workflow.error.status);
  const [scopeNotice, setScopeNotice] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<unknown>(null);
  const [uncertain, setUncertain] = useState(false);
  const [maintenance, setMaintenance] = useState<OrderMaintenance[] | null>(null);
  const fingerprint = JSON.stringify([draft, body, useDefault, teamId, template, project, parentIssue?.id, parentIssue?.etag]);
  const pending = useRef<{ key: string; fingerprint: string; body: Omit<IssueDraft, 'status_id' | 'description'> & { document: { schema_version: number; body: RichBody }; status_id?: string; team_id: string; template?: TemplateSelection; project_id?: string; project_etag?: string; parent_id?: string; parent_etag?: string } } | null>(null);
  useEffect(() => { if (maintenance && pending.current?.fingerprint !== fingerprint) setMaintenance(null); }, [fingerprint]);
  const unresolved = templateDiagnostics.filter(item => item.field === 'title' ? !draft.title.trim() || Array.from(draft.title.trim()).length > 256 : item.field === 'label_ids' ? !!item.reference_id && draft.label_ids.includes(item.reference_id) : item.field === 'status_id' ? !draft.status_id || draft.status_id === item.reference_id : item.field === 'project_id' ? !projectEdited : true);
  const projectValid = !project || verifiedProject === projectSignature(scope, workspaceId, teamId, project);
  const templateEdited = !!draft.title || hasContent(body) || editedFields.current.size > 0 || projectEdited || draft.assignee_id !== null || draft.estimate !== null || draft.due_date !== null || draft.label_ids.length > 0 || draft.priority !== 0;
  function changeFields(value: IssueDraft) {
    for (const field of ['status_id', 'priority', 'label_ids'] as const) if (JSON.stringify(value[field]) !== JSON.stringify(draft[field])) editedFields.current.add(field);
    setDraft(value);
  }
  function acceptTemplate(value: TemplatePreview, mode: 'merge' | 'replace'): string | null {
    if (busy || uncertain || uploads?.pending || value.template.team_id !== teamId || !value.template.can_use) return 'This template cannot be applied right now. Your draft is preserved.';
    const resolvedProject = previewProject(value, workspaceId);
    const projectDiagnostic = value.diagnostics.some(item => item.field === 'project_id');
    if (value.resolved_project && !resolvedProject || value.template.defaults.project_id && (!resolvedProject || resolvedProject.archived_at || !resolvedProject.can_edit) && !projectDiagnostic) return 'The project preview could not be verified. Refresh the template; your draft is preserved.';
    try {
      const next = applyTemplate({ title: draft.title, body, status_id: useDefault ? null : draft.status_id, priority: draft.priority, label_ids: draft.label_ids }, { title: value.materialized_title ?? '', body: templateBody(value.template.document.body), status_id: value.resolved_status?.id ?? value.template.defaults.status_id ?? '', priority: value.template.defaults.priority, label_ids: value.template.defaults.label_ids }, mode, editedFields.current);
      setDraft(old => ({ ...old, title: next.title, status_id: next.status_id ?? '', priority: next.priority, label_ids: next.label_ids })); setBody(next.body); setUseDefault(next.status_id === null);
      if (!projectEdited) setProject(resolvedProject && !projectDiagnostic ? { id: resolvedProject.id, etag: resolvedProject.etag } : null);
      setTemplate({ id: value.template.id, version: value.template.version }); setTemplateName(value.template.name); setTemplateDiagnostics(value.diagnostics); setTemplateReview(false); setError(null);
      return null;
    } catch (error) { return error instanceof Error ? error.message : 'The template could not be combined with this draft.'; }
  }
  function reviewTemplate(value: TemplatePreview) {
    if (busy || uncertain || uploads?.pending || !value.template.can_use || value.template.team_id !== teamId) return;
    setTemplate({ id: value.template.id, version: value.template.version }); setTemplateName(value.template.name); setTemplateDiagnostics(value.diagnostics); setTemplateReview(false); setError(null);
  }
  const committed = useRef(false);
  const releaseOwnedGuard = useUnsavedChanges(!committed.current && (templateEdited || !!template || uncertain || !!uploads?.pending));
  function dismiss() { if (busy || uncertain) return; if (templateEdited || template || uploads?.pending) setDiscarding(true); else close(); }
  async function refreshParent() {
    if (!parentIssue || busy) return; setBusy(true); setError(null);
    try { const current = await request<Issue>(`/workspaces/${workspaceId}/issues/${parentIssue.id}`); if (current.team_id !== teamId || current.archived_at) throw new ApiError(409, 'parent_unavailable', 'The parent moved or was archived. Close this draft and choose an active parent.'); setParentIssue(current); setParentReview(false); }
    catch (error) { setError(error); } finally { setBusy(false); }
  }
  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault(); if (committed.current || busy || uploads?.pending || composing || unavailable || (!uncertain && (projectDenied || !projectValid || projectReview)) || parentReview || templateReview || unresolved.length > 0 || (!uncertain && !useDefault && !validExplicitStatus)) return;
    if (!pending.current || !uncertain && (!maintenance || pending.current.fingerprint !== fingerprint)) { const { status_id, description: _description, ...fields } = draft; pending.current = { key: crypto.randomUUID(), fingerprint, body: { ...fields, document: { schema_version: 1, body }, ...(template ? { template, status_id: useDefault ? currentDefault?.id ?? '' : status_id } : !useDefault ? { status_id } : {}), title: draft.title.trim(), team_id: teamId, ...(project ? { project_id: project.id, project_etag: project.etag } : {}), ...(parentIssue ? { parent_id: parentIssue.id, parent_etag: parentIssue.etag } : {}) } }; }
    setBusy(true); setError(null); setMaintenance(null);
    let issue: Issue;
    try {
      issue = await request<Issue>(`/workspaces/${workspaceId}/issues`, { method: 'POST', body: pending.current.body, csrf: session.csrf_token, headers: { 'Idempotency-Key': pending.current.key } });
    } catch (error) {
      setError(error);
      const jobs = orderMaintenance(error, pending.current.body.team_id);
      if (jobs) { setMaintenance(jobs); setUncertain(false); return; }
      if (error instanceof ApiError && error.code === 'issue_order_maintenance') { setUncertain(true); return; }
      const currentId = error instanceof ApiError ? (error.details?.current as { id?: string } | undefined)?.id : undefined;
      const changedTemplate = template && error instanceof ApiError && error.status === 412 && currentId === template.id;
      const ambiguousProject = project && error instanceof ApiError && ([404, 409].includes(error.status) || error.status === 412 && (!template || currentId !== template.id) && (!parentIssue || currentId !== parentIssue.id));
      if (changedTemplate) setTemplateReview(true);
      else if (ambiguousProject) { setProjectReview(true); setVerifiedProject(null); setProjectRefresh(value => value + 1); }
      else if (template && error instanceof ApiError && [404, 409].includes(error.status)) setTemplateReview(true);
      else if (parentIssue && error instanceof ApiError && [409, 412].includes(error.status)) setParentReview(true);
      setUncertain(error instanceof ApiError && (error.status === 0 || error.status === 408 || error.status >= 500));
      return;
    } finally { setBusy(false); }
    committed.current = true;
    releaseOwnedGuard();
    created(issue);
  }
  return <AttachmentProvider workspaceId={workspaceId} session={session}><ReferenceProvider workspaceId={workspaceId} session={session} target={{ team_id: teamId }}><Modal title={parentIssue ? "New subissue" : "New issue"} close={dismiss} canClose={!busy && !uncertain}>
    <form className="create-form" onSubmit={submit} onKeyDown={e => { if (e.target instanceof Element && e.target.closest('dialog') !== e.currentTarget.closest('dialog')) return; if (!e.defaultPrevented && !e.nativeEvent.isComposing && e.keyCode !== 229 && !composing && !e.altKey && !e.getModifierState('AltGraph') && !e.repeat && (e.ctrlKey || e.metaKey) && e.key === 'Enter') { e.preventDefault(); e.currentTarget.requestSubmit(); } }} aria-busy={busy}>
      <ErrorBanner error={error} /><ErrorBanner error={workflow.error} retry={workflow.refresh} />
      {parentIssue && <div className="parent-context">Parent: <strong>{parentIssue.key}</strong> · {parentIssue.title}</div>}
      {parentReview && <div className="workflow-conflict" role="alert"><strong>Review the parent issue.</strong><p>Your draft is preserved. Refresh the parent before trying again.</p><button type="button" className="button compact" disabled={busy} onClick={refreshParent}>Refresh parent</button></div>}
      {projectReview && <div className="workflow-conflict" role="alert"><strong>Review your selections.</strong><p>The project, template or parent may have changed. Your draft is preserved. Review the project and any applied template before retrying.</p></div>}
      {scopeNotice && <p className="notice" role="status">{scopeNotice}</p>}
      {uncertain && <div className="notice" role="status">The response was interrupted, so this issue may already exist. Retry safely to recover the result without creating a duplicate. Keep this dialog open until the request is resolved.</div>}
      {maintenance && <><div className="notice" role="status">No issue was created by this request. Your draft and attachments are preserved. Retry the unchanged draft with the same request after order preparation, or edit or cancel this draft.</div><IssueOrderProgress workspaceId={workspaceId} session={session} initial={maintenance}/></>}
      <TemplateChooser workspaceId={workspaceId} teamId={teamId} session={session} selection={template} selectedName={templateName} disabled={busy || uncertain || unavailable || projectDenied} conceal={unavailable || projectDenied} pendingUploads={!!uploads?.pending} edited={templateEdited} needsReview={templateReview} apply={acceptTemplate} review={reviewTemplate} clear={() => { setTemplate(null); setTemplateName(''); setTemplateDiagnostics([]); setTemplateReview(false); setError(null); }}/>
      <TemplateDiagnostics diagnostics={unresolved}/>
      <fieldset disabled={busy || uncertain || unavailable}>
        <label className="field team-field">Team<select disabled={!!parentIssue || !!uploads?.pending || attachmentIds(body).length > 0} title={uploads?.pending || attachmentIds(body).length ? "Remove attachments before changing team." : undefined} value={teamId} onChange={e => { setTeam(e.target.value); setUseDefault(true); setTemplate(null); setTemplateName(''); setTemplateReview(false); setTemplateDiagnostics([]); setProject(null); setProjectEdited(false); setProjectReview(false); editedFields.current.delete('status_id'); if (draft.assignee_id || draft.label_ids.length || project) setScopeNotice('Team changed. Choose an assignee, labels and project for this team.'); setDraft({ ...draft, status_id: seededDefault(e.target.value), assignee_id: null, label_ids: [] }); }} required>{activeTeams.map(team => <option key={team.id} value={team.id}>{team.name} · {team.key}</option>)}</select></label>
        <label className="field">Title<input data-autofocus value={draft.title} onChange={e => { editedFields.current.add('title'); setDraft({ ...draft, title: e.target.value }); }} placeholder="What needs to change?" required /></label>
        <section className="create-document"><h3>Description</h3><StructuredEditor attachmentTarget={{ team_id: teamId }} attachmentsChanged={setUploads} value={body} onChange={value => { editedFields.current.add('body'); setBody(value); }} label="Description" disabled={busy || uncertain || unavailable} onCompositionChange={setComposing} compact /></section>
        <IssueFields workspaceId={workspaceId} session={session} disabled={busy || uncertain || unavailable} draft={draft} change={changeFields} data={data} teamId={teamId} statuses={workflow.statuses} currentStatusIds={workflow.currentIds} defaultMode={useDefault} defaultId={currentDefault?.id} chooseStatus={() => setUseDefault(false)} />
        <ProjectPicker workspaceId={workspaceId} teamId={teamId} session={session} value={project} label="Project" disabled={busy || uncertain || unavailable || projectDenied} conceal={unavailable || projectDenied} clearNeeded={unresolved.some(item => item.field === 'project_id')} reviewNeeded={projectReview} refreshKey={projectRefresh} accessChanged={setProjectDenied} verified={setVerifiedProject} change={value => { setProjectEdited(true); setProjectReview(false); setProject(value); }}/>
        {project && !projectValid && !uncertain && <p role="status">Review the selected project's current version before creating.</p>}
      </fieldset>
      <footer className="modal-footer"><span className="keyboard-hint">⌘ / Ctrl + Enter to create</span><button type="button" className="button" onClick={dismiss} disabled={busy || uncertain}>Cancel</button><button type="submit" className="button primary" disabled={busy || !!uploads?.pending || composing || unavailable || (!uncertain && (projectDenied || !projectValid || projectReview)) || parentReview || templateReview || unresolved.length > 0 || !teamId || !draft.title.trim() || (!uncertain && !useDefault && !validExplicitStatus)}>{busy && <LoaderCircle className="spinner" aria-hidden="true" />}{busy ? 'Creating…' : uncertain ? 'Retry safely' : maintenance ? 'Retry create' : parentIssue ? 'Create subissue' : 'Create issue'}</button></footer>
    </form>
  </Modal>{discarding && <DiscardUploadDraft title="Discard this issue draft?" description="Your unsaved title and description will be lost." action="Discard draft" uploads={uploads} close={() => setDiscarding(false)} discard={close} />}</ReferenceProvider></AttachmentProvider>;
}
