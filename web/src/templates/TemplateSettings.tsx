import { useEffect, useRef, useState, type FormEvent } from 'react';
import { Archive, ArchiveRestore, Eye, Pencil, Plus, RefreshCw } from 'lucide-react';
import { ApiError, request } from '../api';
import { ErrorBanner, Modal } from '../components';
import { useDesktop, useSessionScope } from '../desktop/context';
import { DocumentView } from '../documents/DocumentView';
import { StructuredEditor } from '../documents/Editor';
import { EMPTY_BODY, hasContent, isAccessError, sameBody, type RichBody } from '../documents/model';
import { useUnsavedChanges } from '../router';
import { LabelPicker } from '../taxonomy/LabelPicker';
import { priorities, type Session } from '../types';
import type { TeamDetail } from '../settings/types';
import { useWorkflow } from '../workflows/useWorkflow';
import { selectable } from '../workflows/model';
import { supportedDefinition, TemplatePreviewContent } from './TemplatePreview';
import { ProjectPicker, projectSignature, type ProjectChoice } from './ProjectPicker';
import { templateBody } from './model';
import { templatePath, type IssueTemplate, type TemplatePreview } from './types';
import { useTemplates } from './useTemplates';
import './templates.css';

type Mode = 'create' | 'preview' | 'edit' | 'archive' | 'restore';
interface Props { workspaceId: string; team: TeamDetail; session: Session; workspaceArchived?: boolean }
export function TemplateSettings(props: Props) {
  const desktop = useDesktop(); const boundary = `${props.session.user.id}:${desktop?.snapshot.connection?.scope_generation ?? 'browser'}`;
  return <ScopedTemplateSettings key={`${props.workspaceId}:${props.team.id}:${boundary}`} {...props}/>;
}
function ScopedTemplateSettings({ workspaceId, team, session, workspaceArchived = false }: Props) {
  const scope = useSessionScope(session); const [archived, setArchived] = useState(false);
  const [denied, setDenied] = useState<unknown>(null); const [accessAttempt, setAccessAttempt] = useState(0);
  const [selected, setSelected] = useState<{ id?: string; mode: Mode } | null>(null); const [notice, setNotice] = useState('');
  const heading = useRef<HTMLHeadingElement>(null); const focusSaved = useRef(false);
  useEffect(() => { if (!selected && focusSaved.current) { focusSaved.current = false; heading.current?.focus(); } }, [selected]);
  const directory = useTemplates(workspaceId, team.id, `${scope}:${accessAttempt}`, archived);
  const writable = team.can_manage && !team.archived_at && !workspaceArchived;
  if (denied || isAccessError(directory.error)) return <section className="template-settings"><h3>Templates unavailable</h3><ErrorBanner error={denied || directory.error} retry={() => { setAccessAttempt(value => value + 1); setDenied(null); }}/></section>;
  return <section className="template-settings" aria-label={`${team.name} templates`}><header className="workflow-heading"><div><h3 ref={heading} tabIndex={-1}>Issue templates</h3><p>Reusable titles, descriptions and defaults for {team.name}.</p></div><div className="button-row"><button type="button" className="icon-button" aria-label="Refresh templates" onClick={directory.refresh}><RefreshCw aria-hidden="true"/></button>{writable && <button type="button" className="button compact" onClick={() => setSelected({ mode: 'create' })}><Plus aria-hidden="true"/>New template</button>}</div></header><label className="field settings-state-filter">Show<select value={archived ? 'archived' : 'active'} onChange={event => setArchived(event.target.value === 'archived')}><option value="active">Active templates</option><option value="archived">Archived templates</option></select></label><ErrorBanner error={directory.error} retry={directory.refresh}/><div className="settings-list" aria-busy={directory.loading}>{directory.page?.data.map(item => <div className="settings-row" key={item.id}><div className="settings-row-main"><strong>{item.name}</strong><small>Version {item.version}{item.archived_at ? ' · Archived' : ''}</small></div><div className="row-actions"><button type="button" className="icon-button" aria-label={`Preview ${item.name}`} onClick={() => setSelected({ id: item.id, mode: 'preview' })}><Eye aria-hidden="true"/></button>{item.can_manage && writable && (item.archived_at ? <button type="button" className="button compact" onClick={() => setSelected({ id: item.id, mode: 'restore' })}><ArchiveRestore aria-hidden="true"/>Restore</button> : <><button type="button" className="icon-button" aria-label={`Edit ${item.name}`} onClick={() => setSelected({ id: item.id, mode: 'edit' })}><Pencil aria-hidden="true"/></button><button type="button" className="icon-button" aria-label={`Archive ${item.name}`} onClick={() => setSelected({ id: item.id, mode: 'archive' })}><Archive aria-hidden="true"/></button></>)}</div></div>)}</div>{directory.loading && <p className="muted" role="status">Loading templates…</p>}{!directory.loading && !directory.error && !directory.page?.data.length && <p className="settings-empty">No {archived ? 'archived' : 'active'} templates.{writable && !archived && ' Create one for work your team repeats.'}</p>}{directory.page?.has_more && <button type="button" className="button compact" disabled={directory.loading} onClick={directory.more}>Load more templates</button>}<p className="save-message" role="status">{notice}</p>{selected && <TemplateModal key={selected.id ?? 'new'} workspaceId={workspaceId} team={team} session={session} id={selected.id} mode={selected.mode} writable={writable} denied={error => { setSelected(null); setDenied(error); }} close={() => setSelected(null)} saved={value => { focusSaved.current = true; setSelected(null); setNotice(`${value.name} ${selected.mode === 'archive' ? 'archived' : selected.mode === 'restore' ? 'restored' : 'saved'}.`); directory.refresh(); }}/>}</section>;
}
function TemplateModal({ workspaceId, team, session, id, mode, writable, denied, close, saved }: Props & { id?: string; mode: Mode; writable: boolean; denied: (error: unknown) => void; close: () => void; saved: (value: IssueTemplate) => void }) {
  const scope = useSessionScope(session); const [preview, setPreview] = useState<TemplatePreview | null>(null); const [verified, setVerified] = useState(id ? '' : scope); const [error, setError] = useState<unknown>(null); const [retry, setRetry] = useState(0);
  useEffect(() => {
    const abort = new AbortController(); setError(null);
    if (id) request<TemplatePreview>(`${templatePath(workspaceId, team.id)}/${id}/preview`, { signal: abort.signal }).then(value => { if (!abort.signal.aborted && value.template.id === id && value.template.team_id === team.id) { setPreview(value); setVerified(scope); } }).catch(error => { if (!abort.signal.aborted) { setError(error); if (isAccessError(error)) { setPreview(null); denied(error); } } });
    else setVerified(scope);
    return () => abort.abort();
  }, [workspaceId, team.id, id, scope, retry]);
  const title = mode === 'create' ? 'New issue template' : mode === 'archive' ? 'Archive template' : mode === 'restore' ? 'Restore template' : mode === 'preview' ? 'Template preview' : 'Edit issue template';
  if (id && !preview) return <Modal title={title} close={close}><div className="settings-modal-content"><ErrorBanner error={error} retry={() => setRetry(value => value + 1)}/>{!error && <p role="status">Loading template…</p>}</div></Modal>;
  if (mode === 'preview') return <Modal title={title} close={close}><div className="settings-modal-content"><ErrorBanner error={error}/>{preview && <TemplatePreviewContent value={preview} workspaceId={workspaceId}/>}<div className="modal-footer"><button type="button" className="button" onClick={close}>Close</button></div></div></Modal>;
  return <TemplateEditor workspaceId={workspaceId} team={team} session={session} preview={preview} mode={mode} title={title} writable={writable && verified === scope && !error} close={close} saved={saved} denied={denied} refreshError={error} refresh={() => setRetry(value => value + 1)}/>;
}
function TemplateEditor({ workspaceId, team, session, preview, mode, title, writable, close, saved, denied: deny, refreshError, refresh }: Props & { preview: TemplatePreview | null; mode: Mode; title: string; writable: boolean; close: () => void; saved: (value: IssueTemplate) => void; denied: (error: unknown) => void; refreshError: unknown; refresh: () => void }) {
  const scope = useSessionScope(session); const workflow = useWorkflow(workspaceId, team.id, scope);
  const initial = preview?.template; const [base, setBase] = useState(initial);
  const [name, setName] = useState(initial?.name ?? ''); const [issueTitle, setIssueTitle] = useState(initial?.title ?? '');
  const [titleMode, setTitleMode] = useState<'literal' | 'pattern'>(initial?.title_mode ?? 'literal');
  const [body, setBody] = useState<RichBody>(() => { try { return initial ? templateBody(initial.document.body) : EMPTY_BODY; } catch { return EMPTY_BODY; } });
  const [status, setStatus] = useState<string | null>(initial?.defaults.status_id ?? null); const [priority, setPriority] = useState(initial?.defaults.priority ?? 0); const [labels, setLabels] = useState<string[]>(initial?.defaults.label_ids ?? []);
  const [project, setProject] = useState<ProjectChoice | null>(() => initial?.defaults.project_id ? { id: initial.defaults.project_id, etag: preview?.resolved_project?.etag ?? '' } : null);
  const [projectRepaired, setProjectRepaired] = useState(false);
  const [projectDenied, setProjectDenied] = useState(false); const [verifiedProject, setVerifiedProject] = useState<string | null>(null);
  const [denied, setDenied] = useState(false); const [busy, setBusy] = useState(false); const [uncertain, setUncertain] = useState(false); const [error, setError] = useState<unknown>(null); const [latest, setLatest] = useState<IssueTemplate | null>(null); const [discard, setDiscard] = useState(false);
  const pending = useRef<{ key: string; body: object; etag?: string } | null>(null); const committed = useRef(false);
  const bodySupported = !base || supportedDefinition(base);
  const projectNeedsReview = !!preview?.diagnostics.some(item => item.field === 'project_id') && !projectRepaired;
  const dirty = !!(base ? name !== base.name || issueTitle !== base.title || titleMode !== base.title_mode || status !== base.defaults.status_id || projectRepaired || (project?.id ?? null) !== (base.defaults.project_id ?? null) || priority !== base.defaults.priority || JSON.stringify([...labels].sort()) !== JSON.stringify([...base.defaults.label_ids].sort()) || bodySupported && !sameBody(body, templateBody(base.document.body)) : name || issueTitle || titleMode !== 'literal' || status !== null || project || priority !== 0 || labels.length > 0 || hasContent(body));
  const release = useUnsavedChanges(!committed.current && (dirty || uncertain));
  useEffect(() => { if (preview && base && preview.template.etag !== base.etag) setLatest(preview.template); }, [preview?.template.etag]);
  const canManage = writable && (preview?.template.can_manage ?? true) && (base?.can_manage ?? true);
  const projectValid = !project || verifiedProject === projectSignature(scope, workspaceId, team.id, project);
  const canWrite = canManage && !latest && ((mode === 'archive' || mode === 'restore') || bodySupported && !projectNeedsReview && !projectDenied && projectValid) && (mode === 'restore' ? !!base?.archived_at : !base?.archived_at);
  function dismiss() { if (busy || uncertain) return; if (dirty) setDiscard(true); else close(); }
  function useLatest(value: IssueTemplate, keep: boolean) {
    setBase(value); setLatest(null); setError(null); setUncertain(false);
    if (!keep) { setName(value.name); setIssueTitle(value.title); setTitleMode(value.title_mode); setStatus(value.defaults.status_id); setProject(value.defaults.project_id ? { id: value.defaults.project_id, etag: '' } : null); setProjectRepaired(false); setPriority(value.defaults.priority); setLabels(value.defaults.label_ids); try { setBody(templateBody(value.document.body)); } catch { /* Unsupported content stays read-only. */ } }
  }
  async function submit(event: FormEvent) {
    event.preventDefault(); if (busy || !canWrite || committed.current) return;
    if (!uncertain || !pending.current) {
      let fields: object;
      try { fields = mode === 'archive' || mode === 'restore' ? { archived: mode === 'archive' } : { name: name.trim(), title: issueTitle.trim(), title_mode: titleMode, defaults: { status_id: status, priority, label_ids: labels, ...(project ? { project_id: project.id } : {}) }, document: { schema_version: 1, body: templateBody(body) } }; }
      catch (error) { setError(error); return; }
      pending.current = { key: crypto.randomUUID(), body: fields, etag: base?.etag };
    }
    const sent = pending.current; setBusy(true); setError(null);
    try {
      const value = await request<IssueTemplate>(`${templatePath(workspaceId, team.id)}${base ? `/${base.id}` : ''}`, { method: base ? 'PATCH' : 'POST', csrf: session.csrf_token, body: sent.body, headers: base ? { 'If-Match': sent.etag! } : { 'Idempotency-Key': sent.key } });
      committed.current = true; release(); saved(value);
    } catch (error) {
      setError(error); if (isAccessError(error)) { setDenied(true); deny(error); } setUncertain(error instanceof ApiError && (error.status === 0 || error.status === 408 || error.status >= 500));
      if (error instanceof ApiError && error.status === 412) { const value = error.details?.current as IssueTemplate | undefined; if (value?.id === base?.id && value?.team_id === team.id) {
          const fields = sent.body as { archived?: boolean; name?: string; title?: string; title_mode?: 'literal' | 'pattern'; defaults?: unknown; document?: { body: unknown } };
          const reconciled = uncertain && (fields.archived !== undefined ? !!value.archived_at === fields.archived : value.name === fields.name && value.title === fields.title && value.title_mode === fields.title_mode && JSON.stringify(value.defaults) === JSON.stringify(fields.defaults) && JSON.stringify(value.document.body) === JSON.stringify(fields.document?.body));
          if (reconciled) { committed.current = true; release(); saved(value); } else setLatest(value);
        } }
    } finally { setBusy(false); }
  }
  const statuses = workflow.data?.statuses.filter(selectable) ?? [];
  if (denied) return <Modal title="Template unavailable" close={close}><div className="settings-modal-content"><ErrorBanner error={error}/><p>The template can no longer be shown with your current access.</p><button type="button" className="button" onClick={close}>Close</button></div></Modal>;
  return <><Modal title={title} close={dismiss} canClose={!busy && !uncertain}><form className="settings-modal-content template-editor" onSubmit={submit}><ErrorBanner error={error || refreshError} retry={refreshError ? refresh : undefined}/>{uncertain && <p className="notice" role="status">The response was interrupted. Retry the same request safely before changing or closing this draft.</p>}{latest && <section className="template-version-conflict" role="alert"><h3>This template changed.</h3><p>Your edits are preserved. Latest: {latest.name}, version {latest.version}{latest.archived_at ? ' (archived)' : ''}.</p><div className="template-compare"><div><h4>Latest saved title</h4><p>{latest.title}</p><p>Title mode: {latest.title_mode}</p><p>Priority: {priorities[latest.defaults.priority]}; {latest.defaults.label_ids.length} labels.</p><DocumentView body={latest.document.body} supported={supportedDefinition(latest)} fallback={latest.document.plain_text}/></div><div><h4>Your title</h4><p>{issueTitle}</p><p>Title mode: {titleMode}</p><p>Priority: {priorities[priority]}; {labels.length} labels.</p><DocumentView body={body}/></div></div><div className="button-row"><button type="button" className="button compact" onClick={() => useLatest(latest, false)}>Use latest</button><button type="button" className="button compact" disabled={!supportedDefinition(latest)} onClick={() => useLatest(latest, true)}>Keep my draft for review</button></div></section>}{!canManage && <p className="settings-info">Editing is unavailable while permissions refresh or this team is read-only. Your draft is preserved.</p>}{!bodySupported && preview && !projectDenied && <TemplatePreviewContent value={{ ...preview, template: base ?? preview.template }} workspaceId={workspaceId}/>}{mode === 'archive' || mode === 'restore' ? <p>{mode === 'archive' ? `Archive ${base?.name}? Existing issues stay unchanged. It will no longer appear in new issue choices.` : `Restore ${base?.name}? Review its defaults before using it for new issues.`}</p> : <fieldset disabled={busy || uncertain || !canManage || !bodySupported}><label className="field">Template name<input data-autofocus value={name} onChange={event => setName(event.target.value)} required/></label><label className="field">Title mode<select aria-label="Title mode" value={titleMode} onChange={event => setTitleMode(event.target.value as 'literal' | 'pattern')}><option value="literal">Literal title</option><option value="pattern">Pattern substitutions</option></select></label><label className="field">Issue title<input value={issueTitle} onChange={event => setIssueTitle(event.target.value)}/><small>{titleMode === 'pattern' ? 'Use {{team.name}}, {{team.key}} or {{date}} (UTC YYYY-MM-DD). Double braces are required; expressions are not supported. Preview the saved template before using it.' : 'Leave blank when each issue needs its own title. Braces stay literal.'}</small></label><div className="template-editor-defaults"><label className="field">Default status<select value={status ?? ''} onChange={event => setStatus(event.target.value || null)}><option value="">Follow team default</option>{status && !statuses.some(item => item.id === status) && <option value={status} disabled>Status unavailable — choose a replacement</option>}{statuses.map(item => <option key={item.id} value={item.id}>{item.name}</option>)}</select></label><label className="field">Priority<select value={priority} onChange={event => setPriority(Number(event.target.value))}>{priorities.map((name, value) => <option key={value} value={value}>{name}</option>)}</select></label></div>{projectNeedsReview && <p role="alert">The default project is unavailable. Choose a project or explicitly continue without one before saving.</p>}<ProjectPicker workspaceId={workspaceId} teamId={team.id} session={session} value={project} label="Default project" disabled={busy || uncertain || !canManage} versionRequired={false} conceal={projectDenied} accessChanged={setProjectDenied} verified={setVerifiedProject} clearNeeded={projectNeedsReview} change={value => { setProject(value); setProjectRepaired(true); }}/><LabelPicker workspaceId={workspaceId} teamId={team.id} session={session} value={labels} summaries={preview?.labels} teams={[team]} disabled={busy || uncertain || !canManage} change={setLabels}/><section><h3>Description</h3>{bodySupported && <StructuredEditor value={body} onChange={setBody} label="Template description" disabled={busy || uncertain || !canManage} compact/>}</section></fieldset>}<div className="modal-footer"><button type="button" className="button" disabled={busy || uncertain} onClick={dismiss}>Cancel</button><button className="button primary" disabled={busy || !canWrite || ((mode === 'create' || mode === 'edit') && !name.trim())}>{busy ? 'Saving…' : uncertain ? 'Retry safely' : mode === 'archive' ? 'Archive template' : mode === 'restore' ? 'Restore template' : base ? 'Save template' : 'Create template'}</button></div></form></Modal>{discard && <Modal title="Discard template changes?" close={() => setDiscard(false)}><div className="settings-modal-content"><p>Your unsaved template changes will be lost.</p><div className="modal-footer"><button type="button" className="button" onClick={() => setDiscard(false)}>Keep editing</button><button type="button" className="button" onClick={() => { release(); close(); }}>Discard changes</button></div></div></Modal>}</>;
}
