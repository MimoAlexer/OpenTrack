import { AttachmentProvider } from '../attachments/context';
import { useEffect, useLayoutEffect, useRef, useState, type FormEvent, type MouseEvent } from 'react';
import { Archive, ArchiveRestore, ArrowLeft, Check, GitCompareArrows, LoaderCircle, MonitorUp, X } from 'lucide-react';
import { ApiError } from '../api';
import { useDesktop, useSessionScope } from '../desktop/context';
import { ErrorBanner, Loading, Modal } from '../components';
import { changes, formatDate, rebaseDraft, toDraft } from '../issue-model';
import { navigate, useUnsavedChanges } from '../router';
import type { Bootstrap, Issue, IssueDraft, Session } from '../types';
import { useScopedRequest } from '../settings/shared';
import { IssueFields } from './IssueFields';
import { IssueRelationships } from '../relations/IssueRelationships';
import { RestoreIssue } from './RestoreIssue';
import { IssueDeleteAction } from './IssueDelete';
import type { AssignableUser, LabelSummary } from '../taxonomy/types';
import { IssueDocument, type DocumentDraftState } from '../documents/IssueDocument';
import { IssueComments, type CommentDraftState } from '../documents/IssueComments';
import { IssueActivity } from './IssueActivity';
import { useIssueWorkflow } from '../workflows/useIssueWorkflow';
import { selectable } from '../workflows/model';
import type { Workflow } from '../workflows/types';

const fieldNames: Record<keyof IssueDraft, string> = { title: 'Title', description: 'Description', status_id: 'Status', priority: 'Priority', assignee_id: 'Assignee', estimate: 'Estimate', due_date: 'Due date', label_ids: 'Labels' };

export function IssueEditor({ workspaceId, issueId, data, session, close, updated, deleted, workflowChanged, workspaceArchived = false }: {
  workflowChanged?: (workflow: Workflow) => void; workspaceArchived?: boolean; workspaceId: string; issueId: string; data: Bootstrap; session: Session; close: () => void; updated: (issue: Issue, options?: { refreshList?: boolean }) => void; deleted?: (issue: Issue) => void;
}) {
  const request = useScopedRequest();
  const desktop = useDesktop();
  const authScope = useSessionScope(session);
  const desktopLink = `opentrack://open?${new URLSearchParams({ server: window.location.origin, workspace: workspaceId, issue: issueId })}`;
  const [base, setBase] = useState<Issue | null>(null);
  const [draft, setDraft] = useState<IssueDraft | null>(null);
  const [latest, setLatest] = useState<Issue | null>(null);
  const [error, setError] = useState<unknown>(null);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState('');
  const [documentState, setDocumentState] = useState<DocumentDraftState | null>(null);
  const [commentState, setCommentState] = useState<CommentDraftState | null>(null);
  const [discussionVersion, setDiscussionVersion] = useState(0);
  const [restoring, setRestoring] = useState(false);
  const [pendingLink, setPendingLink] = useState<string | null>(null); const [discarding, setDiscarding] = useState(false);
  const [people, setPeople] = useState<AssignableUser[]>([]); const [labelNames, setLabelNames] = useState<LabelSummary[]>([]);
  const [retry, setRetry] = useState(0);
  const pendingFrame = useRef<number | null>(null);
  useEffect(() => () => { if (pendingFrame.current !== null) cancelAnimationFrame(pendingFrame.current); }, []);
  const titleRef = useRef<HTMLTextAreaElement>(null);
  const savedFocus = useRef<HTMLElement | null>(null);
  useLayoutEffect(() => {
    if (busy || !savedFocus.current) return;
    const target = savedFocus.current; savedFocus.current = null;
    if (target.isConnected && !target.matches(':disabled') && document.activeElement === document.body) target.focus({ preventScroll: true });
  }, [busy]);
  const headingRef = useRef<HTMLElement>(null);
  const editorScope = `${workspaceId}:${issueId}`;
  const currentEdit = useRef({ base, draft, latest, scope: editorScope });
  currentEdit.current = { base, draft, latest, scope: editorScope };
  const mounted = useRef(true);
  useEffect(() => { mounted.current = true; return () => { mounted.current = false; }; }, []);
  const workflow = useIssueWorkflow(workspaceId, base?.team_id ?? '', session, data, draft?.status_id, workflowChanged);
  const invalidStatusChange = !!base && !!draft && draft.status_id !== base.status_id && !workflow.statuses.some(status => status.id === draft.status_id && workflow.currentIds.has(status.id) && selectable(status));
  const workflowUnavailable = workflow.error instanceof ApiError && [401, 403, 404].includes(workflow.error.status);
  const dirty = !!base && !!draft && Object.keys(changes(toDraft(base), draft)).length > 0;
  const allDirty = dirty || !!documentState?.dirty || !!commentState?.dirty;
  const allBusy = busy || !!documentState?.busy || !!commentState?.busy;
  useUnsavedChanges(dirty);
  useEffect(() => {
    const abort = new AbortController(); setBase(null); setDraft(null); setError(null); setLatest(null); setRestoring(false); setPeople([]); setLabelNames([]);
    request<Issue>(`/workspaces/${workspaceId}/issues/${issueId}`, { signal: abort.signal }).then(issue => {
      if (issue.workspace_id !== workspaceId) throw new ApiError(404, 'not_found', 'This issue is not available in this workspace.');
      if (!abort.signal.aborted) { setBase(issue); setDraft(toDraft(issue)); }
    }).catch(error => { if (!abort.signal.aborted) setError(error); });
    return () => abort.abort();
  }, [workspaceId, issueId, retry]);
  useEffect(() => { if (base) headingRef.current?.focus({ preventScroll: true }); }, [base?.id]);
  useLayoutEffect(() => {
    const title = titleRef.current;
    if (!title) return;
    const resize = () => { title.style.height = 'auto'; title.style.height = `${title.scrollHeight}px`; };
    resize();
    if (typeof ResizeObserver === 'undefined') return;
    const observer = new ResizeObserver(resize); observer.observe(title);
    return () => observer.disconnect();
  }, [draft?.title]);
  function display(field: keyof IssueDraft, value: unknown): string {
    if (field === 'status_id') return workflow.statuses.find(s => s.id === value)?.name ?? String(value);
    if (field === 'assignee_id') return value ? [latest?.assignee, base?.assignee, ...people].find(person => person?.id === value)?.display_name ?? 'Assignee unavailable' : 'Unassigned';
    if (field === 'label_ids') return (value as string[]).map(id => [...(latest?.labels ?? []), ...(base?.labels ?? []), ...labelNames].find(label => label.id === id)?.name ?? 'Label unavailable').join(', ') || 'No labels';
    return value === null || value === '' ? 'Not set' : String(value);
  }
  async function save(event: FormEvent<HTMLFormElement>) { event.preventDefault(); await saveChanges(); }
  async function saveChanges(): Promise<boolean> {
    const current = currentEdit.current; const source = current.base; const local = current.draft;
    if (!source || !local || busy || documentState?.busy || current.latest || readOnly || invalidStatusChange || !local.title.trim()) return false;
    if (!Object.keys(changes(toDraft(source), local)).length) return true;
    const active = document.activeElement;
    savedFocus.current = active instanceof HTMLElement && active.closest('.detail-form') ? active : null;
    setBusy(true); setError(null); setMessage('');
    const patch = changes(toDraft(source), { ...local, title: local.title.trim() });
    try {
      const result = await request<Issue>(`/workspaces/${workspaceId}/issues/${source.id}`, { method: 'PATCH', body: patch, csrf: session.csrf_token, headers: { 'If-Match': source.etag } });
      currentEdit.current = { ...currentEdit.current, base: result, draft: toDraft(result), latest: null }; setBase(result); setDraft(toDraft(result)); setMessage('Changes saved.'); updated(result); return true;
    } catch (error) {
      if (error instanceof ApiError && error.status === 412) {
        try {
          const current = error.details?.current as Issue | undefined;
          setLatest(current?.id === source.id && current.workspace_id === workspaceId ? current : await request<Issue>(`/workspaces/${workspaceId}/issues/${source.id}`));
        } catch (loadError) { setError(loadError); }
      } else setError(error);
    } finally { setBusy(false); }
    return false;
  }
  function discard() { if (dirty) setDiscarding(true); }
  function openLink(event: MouseEvent<HTMLAnchorElement>, href: string) {
    if (event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
    if (allDirty || allBusy) { event.preventDefault(); setPendingLink(href); }
  }
  function followPending() { const href = pendingLink; setPendingLink(null); if (href) pendingFrame.current = requestAnimationFrame(() => navigate(href)); }
  function graphUpdated(result: Issue) {
    const current = currentEdit.current;
    if (!mounted.current || current.scope !== editorScope || result.workspace_id !== workspaceId || result.id !== issueId || !current.base || !current.draft) return;
    // A late graph refresh must not roll back a newer save or discard typing that
    // began after the graph operation's dialog closed.
    if (BigInt(result.version) < BigInt((current.latest ?? current.base).version)) return;
    const localEdits = changes(toDraft(current.base), current.draft);
    const remoteEdits = changes(toDraft(current.base), toDraft(result));
    if (Object.keys(localEdits).length && (current.latest || Object.keys(remoteEdits).length)) {
      setLatest(result); setMessage('Connections refreshed. Review the latest issue before saving your edits.');
    } else {
      setDraft(rebaseDraft(current.base, current.draft, result)); setBase(result); setLatest(null);
      if (Object.keys(localEdits).length) setMessage('Connections refreshed. Your unsaved edits are preserved.');
    }
    updated(result);
  }

  async function bodySaved() {
    setDiscussionVersion(value => value + 1);
    try {
      const result = await request<Issue>(`/workspaces/${workspaceId}/issues/${issueId}`);
      if (!mounted.current) return;
      const current = currentEdit.current;
      if (!current.base || !current.draft || current.scope !== editorScope) return;
      if (BigInt(result.version) < BigInt(current.base.version)) return;
      const local = changes(toDraft(current.base), current.draft);
      const remote = changes(toDraft(current.base), toDraft(result));
      if (Object.keys(local).length && Object.keys(remote).length) { setLatest(result); return; }
      const next = rebaseDraft(current.base, current.draft, result);
      currentEdit.current = { ...current, base: result, draft: next };
      setBase(result); setDraft(next); updated(result, { refreshList: false });
    } catch (error) { if (mounted.current) setError(error); }
  }
  async function saveAll() {
    if (commentState?.dirty || allBusy || documentState?.blocked) return false;
    if (documentState?.dirty && !await documentState.flush()) return false;
    return saveChanges();
  }
  function discardAll() {
    if (base) setDraft(toDraft(base)); setLatest(null); documentState?.discard(); commentState?.discard();
  }

  async function changeArchive() {
    if (!base || allBusy || allDirty || latest || parentArchived || roleReadOnly) return;
    if (base.archived_at) { setRestoring(true); return; }
    setBusy(true); setError(null); setMessage('');
    try {
      const result = await request<Issue>(`/workspaces/${workspaceId}/issues/${base.id}`, { method: 'PATCH', body: { archived: !base.archived_at }, csrf: session.csrf_token, headers: { 'If-Match': base.etag } });
      setBase(result); setDraft(toDraft(result)); setMessage(result.archived_at ? 'Issue archived. It remains available in the archive.' : 'Issue restored to active work.'); updated(result);
    } catch (error) {
      if (error instanceof ApiError && error.status === 412) {
        try {
          const current = error.details?.current as Issue | undefined;
          const result = current?.id === base.id && current.workspace_id === workspaceId ? current : await request<Issue>(`/workspaces/${workspaceId}/issues/${base.id}`);
          setBase(result); setDraft(toDraft(result));
          setError(new ApiError(412, error.code, 'This issue changed before your request was saved. The latest version is now shown. Review it before trying again.', error.requestId));
        } catch (loadError) { setError(loadError); }
      } else setError(error);
    } finally { setBusy(false); }
  }
  const roleReadOnly = session.workspaces.find(w => w.id === workspaceId)?.role === 'viewer';
  const parentArchived = workspaceArchived || !!data.teams.find(team => team.id === base?.team_id)?.archived_at;
  const readOnly = roleReadOnly || parentArchived || !!base?.archived_at || workflowUnavailable;
  return <AttachmentProvider workspaceId={workspaceId} session={session}><section className="issue-detail" aria-label="Issue detail">
    <header className="detail-header" ref={headingRef} tabIndex={-1} aria-label={base ? `${base.key} · ${base.title}` : 'Issue'}><button className="quiet mobile-back" onClick={close}><ArrowLeft aria-hidden="true" />Issues</button><span className="issue-key">{base?.key ?? 'Issue'}</span><span className="detail-team">{data.teams.find(t => t.id === base?.team_id)?.name}</span>{base && !desktop && <a className="icon-button" href={desktopLink} aria-label="Open issue in desktop app" title="Open in desktop app"><MonitorUp aria-hidden="true" /></a>}{base && !roleReadOnly && !parentArchived && <button type="button" className="quiet archive-action" onClick={changeArchive} disabled={allBusy || allDirty || !!latest} title={allDirty ? 'Save or discard your edits before archiving.' : undefined}>{base.archived_at ? <ArchiveRestore aria-hidden="true" /> : <Archive aria-hidden="true" />}{base.archived_at ? 'Restore' : 'Archive'}</button>}{base && !readOnly && deleted && <IssueDeleteAction key={base.id} issue={base} session={session} dirty={allDirty} disabled={allBusy || !!latest} deleted={deleted} />}<button className="icon-button detail-close" aria-label="Close issue" onClick={close}><X aria-hidden="true" /></button></header>
    {!base || !draft ? <><ErrorBanner error={error} retry={() => setRetry(n => n + 1)} />{!error && <Loading label="Opening issue…" />}</> :
      workflowUnavailable ? <ErrorBanner error={workflow.error} retry={workflow.refresh} /> : <form onSubmit={save} className="detail-form" aria-busy={busy} onKeyDown={event => { if (event.target instanceof Element && event.target.closest('dialog') !== event.currentTarget.closest('dialog')) return; if (!event.defaultPrevented && !event.nativeEvent.isComposing && event.keyCode !== 229 && !event.altKey && !event.getModifierState('AltGraph') && !event.repeat && (event.ctrlKey || event.metaKey) && event.key === 'Enter') { event.preventDefault(); event.currentTarget.requestSubmit(); } }}>
        <ErrorBanner error={error} /><ErrorBanner error={workflow.error} retry={workflow.refresh} />
        {readOnly && <div className="notice">{parentArchived ? 'The workspace or team is archived. Restore it before editing this issue.' : base.archived_at ? 'This issue is archived and cannot be edited.' : 'Your workspace role is read-only.'}</div>}
        {latest && <section className="conflict" aria-labelledby="conflict-title" role="alert"><h3 id="conflict-title"><GitCompareArrows aria-hidden="true" />This issue changed while you were editing.</h3><p>Your draft is still here. Compare the latest version before saving again.</p><div className="conflict-table"><div className="conflict-row conflict-heading"><span>Field</span><span>Latest version</span><span>Your edit</span></div>{(Object.keys(changes(toDraft(base), draft)) as (keyof IssueDraft)[]).map(field => <div key={field} className="conflict-row"><strong>{fieldNames[field]}</strong><span>{display(field, latest[field])}</span><span>{display(field, draft[field])}</span></div>)}</div><div className="button-row"><button type="button" className="button" onClick={() => { setDraft(toDraft(latest)); setBase(latest); setLatest(null); setMessage('Loaded the latest version. Your previous edits were discarded.'); }}>Use latest version</button><button type="button" className="button primary" onClick={() => { setDraft(rebaseDraft(base, draft, latest)); setBase(latest); setLatest(null); setMessage('Your edits are applied to the latest version. Review them, then save.'); }}>Keep my edits for review</button></div></section>}
        <fieldset disabled={busy || readOnly}>
          <label className="sr-only" htmlFor="issue-title">Issue title</label><textarea id="issue-title" rows={1} ref={titleRef} className="title-input" value={draft.title} onChange={e => { setDraft({ ...draft, title: e.target.value.replace(/[\r\n]+/g, ' ') }); setMessage(''); }} required />
          <div className="detail-grid"><aside className="detail-properties" aria-label="Issue properties"><IssueFields workspaceId={workspaceId} session={session} issue={base} disabled={busy || readOnly} rememberedAssignee={person => setPeople(old => [...old.filter(item => item.id !== person.id), person])} rememberedLabels={labels => setLabelNames(old => [...old.filter(item => !labels.some(label => label.id === item.id)), ...labels])} draft={draft} change={value => { setDraft(value); setMessage(''); }} data={data} teamId={base.team_id} statuses={workflow.statuses} currentStatusIds={workflow.currentIds} /></aside></div></fieldset>
        <footer className="detail-footer" style={{ visibility: dirty || busy || message ? 'visible' : 'hidden' }}><span className={dirty ? 'unsaved' : 'save-message'} role="status">{message || (dirty ? 'Unsaved changes' : '')}{message === 'Changes saved.' && <Check aria-hidden="true" />}</span>{!readOnly && <><button type="button" className="button" onClick={discard} disabled={!dirty || busy}>Discard</button><button className="button primary" type="submit" disabled={!dirty || busy || !!documentState?.busy || !!latest || !draft.title.trim() || invalidStatusChange}>{busy && <LoaderCircle className="spinner" aria-hidden="true" />}{busy ? 'Saving…' : 'Save changes'}</button></>}</footer>
        <IssueDocument key={`${workspaceId}:${issueId}`} workspaceId={workspaceId} issueId={issueId} bodyVersion={base.body_version ?? '0'} session={session} readOnly={readOnly} paused={busy} stateChanged={setDocumentState} saved={bodySaved} />
        <div className="issue-metadata"><span>Created {formatDate(base.created_at)}</span><span>Updated {formatDate(base.updated_at)}</span></div>
      </form>}
    {base && draft && !workflowUnavailable && <><IssueRelationships key={`${workspaceId}:${base.id}:${base.team_id}`} workspaceId={workspaceId} issue={base} session={session} data={data} disabled={readOnly} pending={allBusy || !!latest} dirty={allDirty} updated={graphUpdated} openLink={openLink} /><IssueComments key={`${workspaceId}:${issueId}`} workspaceId={workspaceId} issueId={issueId} teamId={base.team_id} session={session} readOnly={readOnly} refreshVersion={`${base.version}:${base.body_version ?? 0}`} stateChanged={setCommentState} changed={() => setDiscussionVersion(value => value + 1)} /><IssueActivity authScope={authScope} workspaceId={workspaceId} issueId={base.id} version={`${base.version}:${base.body_version ?? 0}:${discussionVersion}`} /></>}
    {pendingLink && <Modal title="Save changes before opening another issue?" close={() => setPendingLink(null)} canClose={!allBusy}><div className="settings-modal-content"><p>Your current issue has unsaved changes.</p>{commentState?.dirty && <p>Post or finish editing your comment before saving and opening another issue.</p>}<div className="modal-footer"><button type="button" className="button" data-autofocus disabled={allBusy} onClick={() => setPendingLink(null)}>Keep editing</button><button type="button" className="button" disabled={allBusy || !!documentState?.blocked || !!commentState?.blocked} onClick={() => { discardAll(); followPending(); }}>Discard and open</button><button type="button" className="button primary" disabled={allBusy || !!commentState?.dirty || !!documentState?.blocked || !!latest || readOnly || invalidStatusChange || !draft?.title.trim()} onClick={async () => { if (await saveAll()) followPending(); else setPendingLink(null); }}>Save and open</button></div></div></Modal>}
    {discarding && <Modal title="Discard your unsaved changes?" close={() => setDiscarding(false)}><div className="settings-modal-content"><p>The issue will return to its last loaded version.</p><div className="modal-footer"><button className="button" type="button" data-autofocus onClick={() => setDiscarding(false)}>Keep editing</button><button className="button primary" type="button" onClick={() => { if (base) setDraft(toDraft(base)); setLatest(null); setMessage(''); setError(null); setDiscarding(false); }}>Discard changes</button></div></div></Modal>}
    {restoring && base && !workflowUnavailable && !parentArchived && !roleReadOnly && <RestoreIssue issue={base} workspaceId={workspaceId} session={session} data={data} statuses={workflow.statuses} currentStatusIds={workflow.currentIds} close={() => setRestoring(false)} restored={result => { setRestoring(false); setBase(result); setDraft(toDraft(result)); setMessage('Issue restored to active work.'); updated(result); requestAnimationFrame(() => titleRef.current?.focus()); }} />}
  </section></AttachmentProvider>;
}
