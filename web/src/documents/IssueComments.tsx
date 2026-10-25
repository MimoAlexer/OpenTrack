import { DiscardUploadDraft } from '../attachments/DiscardUploadDraft';
import type { AttachmentDraftState } from '../attachments/types';
import { useEffect, useRef, useState } from 'react';
import { LoaderCircle, MessageSquare, MoreHorizontal } from 'lucide-react';
import { ApiError } from '../api';
import { Avatar, ErrorBanner, Modal } from '../components';
import { useSessionScope } from '../desktop/context';
import { useUnsavedChanges } from '../router';
import { useScopedRequest } from '../settings/shared';
import type { Session } from '../types';
import { StructuredEditor } from './Editor';
import { DocumentView } from './DocumentView';
import { DocumentConflict } from './DocumentConflict';
import { canonicalBody, EMPTY_BODY, hasContent, attachmentIds, withoutAttachments, isAccessError, sameBody, type RichBody } from './model';
import type { IssueComment, Page } from './types';
import { ReferenceProvider } from '../documents/ReferenceProvider';
import { IssueSubscription } from '../issues/IssueSubscription';
import './documents.css';

export type CommentDraftState = { dirty: boolean; busy: boolean; blocked: boolean; discard: () => void };
const unknownOutcome = (error: unknown) => !(error instanceof ApiError) || error.status === 0 || error.status === 408 || error.status >= 500;
function chronological(items: IssueComment[]) {
  const records = new Map<string, IssueComment>();
  for (const item of items) {
    const previous = records.get(item.id);
    // Later inputs win equal versions, so a fresh authorized read can update
    // mention/permission projections without undoing a newer edit or tombstone.
    if (!previous || BigInt(item.version) >= BigInt(previous.version)) records.set(item.id, item);
  }
  return [...records.values()].sort((a, b) => a.created_at.localeCompare(b.created_at) || a.id.localeCompare(b.id));
}
function timestamp(value: string) { const date = new Date(value); return Number.isNaN(date.getTime()) ? value : date.toLocaleString(undefined, { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' }); }

export function IssueComments({ workspaceId, issueId, teamId, session, readOnly, refreshVersion = '', stateChanged, changed }: {
  workspaceId: string; issueId: string; teamId?: string; session: Session; readOnly: boolean; refreshVersion?: string; stateChanged: (state: CommentDraftState) => void; changed: () => void;
}) {
  const request = useScopedRequest(); const scope = useSessionScope(session); const path = `/workspaces/${workspaceId}/issues/${issueId}/comments`;
  const [projectionScope, setProjectionScope] = useState('');
  const [committed, setCommitted] = useState(0);
  const journalKey = `${scope}:${path}`;
  const journal = useRef({ key: journalKey, sequence: 0, values: new Map<string, { sequence: number; comment: IssueComment }>() });
  if (journal.current.key !== journalKey) journal.current = { key: journalKey, sequence: 0, values: new Map() };
  const [hash, setHash] = useState(location.hash); const [linkNotice, setLinkNotice] = useState('');
  useEffect(() => { const changed = () => setHash(location.hash); window.addEventListener('hashchange', changed); return () => window.removeEventListener('hashchange', changed); }, []);
  const linkedId = /^#comment-([0-9a-f-]{36})$/i.exec(hash)?.[1]; const focusedLink = useRef<string | null>(null);
  const [comments, setComments] = useState<IssueComment[]>([]); const [cursor, setCursor] = useState<string | null>(null); const [loading, setLoading] = useState(true); const [moreBusy, setMoreBusy] = useState(false); const [error, setError] = useState<unknown>(null); const [denied, setDenied] = useState(false); const [retry, setRetry] = useState(0);
  const [uploads, setUploads] = useState<AttachmentDraftState | null>(null); const [attachmentNotice, setAttachmentNotice] = useState('');
  const [draft, setDraft] = useState<RichBody>(EMPTY_BODY); const [sending, setSending] = useState(false); const [sendError, setSendError] = useState<unknown>(null); const [uncertain, setUncertain] = useState(false); const [composing, setComposing] = useState(false);
  const draftRef = useRef(draft); draftRef.current = draft;
  const [editing, setEditing] = useState<string | null>(null); const [removing, setRemoving] = useState<IssueComment | null>(null); const [editDirty, setEditDirty] = useState(false); const [editBusy, setEditBusy] = useState(false); const [editBlocked, setEditBlocked] = useState(false);
  const editingRef = useRef(editing); editingRef.current = editing;
  const pending = useRef<{ key: string; body: RichBody } | null>(null); const inFlight = useRef(false); const alive = useRef(true); const pageGeneration = useRef(0); const olderFlight = useRef<AbortController | null>(null);
  const callbacks = useRef({ stateChanged, changed }); callbacks.current = { stateChanged, changed };
  useEffect(() => { alive.current = true; return () => { alive.current = false; }; }, []);
  const dirty = hasContent(draft) || uncertain || editDirty || !!uploads?.pending;
  useUnsavedChanges(dirty || sending);
  useEffect(() => { callbacks.current.stateChanged({ dirty, busy: sending || editBusy || !!uploads?.busy, blocked: uncertain || editBlocked || !!uploads?.pending, discard: () => { if (!inFlight.current && !uncertain && !editBusy && !editBlocked && !uploads?.pending) { setDraft(EMPTY_BODY); setEditing(null); setEditDirty(false); setEditBusy(false); setEditBlocked(false); setSendError(null); } } }); }, [dirty, sending, uncertain, editBusy, editBlocked, uploads]);
  useEffect(() => {
    const abort = new AbortController(); const startedAt = journal.current.sequence; pageGeneration.current += 1; olderFlight.current?.abort(); olderFlight.current = null; setMoreBusy(false); setLoading(true); setError(null);
    request<Page<IssueComment>>(`${path}?limit=25`, { signal: abort.signal }).then(async result => {
      const editingId = editingRef.current;
      const pinned = editingId && !result.data.some(item => item.id === editingId) ? await request<IssueComment>(`/workspaces/${workspaceId}/comments/${editingId}`, { signal: abort.signal }) : null;
      let linked: IssueComment | null = null;
      if (linkedId && !result.data.some(item => item.id === linkedId) && pinned?.id !== linkedId) {
        try { const record = await request<IssueComment>(`/workspaces/${workspaceId}/comments/${linkedId}`, { signal: abort.signal }); if (record.issue_id === issueId) linked = record; else setLinkNotice('The linked comment is not available in this issue.'); }
        catch (error) { if (abort.signal.aborted) return; if (error instanceof ApiError && [403,404].includes(error.status)) setLinkNotice('The linked comment is no longer available.'); else throw error; }
      }
      if (!abort.signal.aborted && journal.current.key === journalKey) {
        const committedSince = [...journal.current.values.values()].filter(value => value.sequence > startedAt).map(value => value.comment);
        setComments(chronological([...committedSince, ...result.data, ...(pinned ? [pinned] : []), ...(linked ? [linked] : [])]));
        for (const [id, value] of journal.current.values) if (value.sequence <= startedAt) journal.current.values.delete(id);
        setCursor(result.has_more ? result.next_cursor : null); setDenied(false); setProjectionScope(scope); }
    }).catch(error => { if (!abort.signal.aborted) { setError(error); if (isAccessError(error)) { setComments([]); setDenied(true); } } }).finally(() => { if (!abort.signal.aborted) setLoading(false); });
    return () => { abort.abort(); olderFlight.current?.abort(); };
  }, [path, scope, retry, linkedId]);
  useEffect(() => { if (!linkedId) focusedLink.current = null; if (linkedId && focusedLink.current !== linkedId && !loading && !denied && comments.some(comment => comment.id === linkedId)) { focusedLink.current = linkedId; const target = document.getElementById(`comment-${linkedId}`); target?.scrollIntoView?.({ block: 'center' }); target?.focus({ preventScroll: true }); } }, [linkedId, loading, denied]);
  async function older() {
    if (!cursor || moreBusy || loading) return;
    const expected = journalKey; const generation = pageGeneration.current; const abort = new AbortController(); olderFlight.current = abort; setMoreBusy(true); setError(null);
    const current = () => alive.current && !abort.signal.aborted && expected === journal.current.key && generation === pageGeneration.current && olderFlight.current === abort;
    try {
      const result = await request<Page<IssueComment>>(`${path}?limit=25&cursor=${encodeURIComponent(cursor)}`, { signal: abort.signal });
      if (current()) { setComments(old => chronological([...old, ...result.data])); setCursor(result.has_more ? result.next_cursor : null); }
    } catch (error) { if (current()) { setError(error); if (isAccessError(error)) { setDenied(true); setComments([]); } } }
    finally { if (current()) { setMoreBusy(false); olderFlight.current = null; } }
  }
  async function update(comment: IssueComment) {
    const expected = journal.current.key;
    // Browser writes can finish after same-user reauthentication. Confirm the
    // comment's current access before mixing that old receipt into fresh rows.
    let record: IssueComment;
    try { record = expected === journalKey ? comment : await request<IssueComment>(`/workspaces/${workspaceId}/comments/${comment.id}`); }
    catch (error) { if (journal.current.key !== expected) throw new DOMException('Access changed while checking the comment.', 'AbortError'); throw error; }
    if (!alive.current || journal.current.key !== expected) throw new DOMException('The comment response belongs to an earlier access scope.', 'AbortError');
    journal.current.sequence += 1; journal.current.values.set(record.id, { sequence: journal.current.sequence, comment: record }); setCommitted(value => value + 1); setComments(old => chronological([...old, record])); callbacks.current.changed();
  }
  async function send() {
    if (inFlight.current || uploads?.pending || readOnly || denied || composing || !hasContent(draft)) return;
    if (!uncertain || !pending.current) pending.current = { key: crypto.randomUUID(), body: draft };
    const attempt = pending.current; inFlight.current = true; setSending(true); setSendError(null);
    try {
      const result = await request<IssueComment>(path, { method: 'POST', body: { schema_version: 1, body: attempt.body }, csrf: session.csrf_token, headers: { 'Idempotency-Key': attempt.key } });
      if (!alive.current) return; await update(result);
      if (attachmentIds(attempt.body).length && !sameBody(draftRef.current, attempt.body)) setAttachmentNotice('Files stayed with the posted comment. Your newer text is still here.');
      setDraft(current => {
        if (sameBody(current, attempt.body)) return EMPTY_BODY;
        const submittedFiles = new Set(attachmentIds(attempt.body));
        if (submittedFiles.size) return withoutAttachments(current, submittedFiles);
        return current;
      }); setUncertain(false); pending.current = null;
    } catch (error) { if (alive.current) { setSendError(error); setUncertain(unknownOutcome(error)); if (isAccessError(error)) { setDenied(true); setComments([]); } } }
    finally { inFlight.current = false; if (alive.current) setSending(false); }
  }
  return <ReferenceProvider workspaceId={workspaceId} session={session} target={{ issue_id: issueId }}><section className="issue-comments" aria-labelledby={`comments-${issueId}`}>
    <header className="discussion-heading"><h2 id={`comments-${issueId}`}><MessageSquare aria-hidden="true" />Comments</h2><div className="discussion-controls"><IssueSubscription workspaceId={workspaceId} issueId={issueId} session={session} refreshVersion={`${refreshVersion}:${retry}:${committed}`}/><button type="button" className="quiet" onClick={() => setRetry(n => n + 1)} disabled={loading}>Refresh</button></div></header>
    <ErrorBanner error={error} retry={() => setRetry(n => n + 1)} />{linkNotice && <p className="document-notice">{linkNotice}</p>}
    {denied ? <p className="document-notice">This discussion is hidden because access could not be confirmed. Your unsent draft has not been submitted.</p> : <>
      {cursor && <button type="button" className="quiet comments-older" disabled={moreBusy || loading} onClick={older}>{moreBusy ? 'Loading older comments…' : 'Load older comments'}</button>}
      {loading && !comments.length ? <p role="status" className="comments-empty">Loading comments…</p> : !loading && !comments.length && !error ? <p className="comments-empty">No comments yet.</p> : null}
      <ol className="comment-list">{comments.map(comment => <li tabIndex={-1} key={comment.id} id={`comment-${comment.id}`} className={comment.deleted_at ? 'comment tombstone' : 'comment'}><Avatar small user={comment.author} /><div className="comment-content"><header><strong>{comment.author.display_name}</strong><a className="comment-time" href={`#comment-${comment.id}`} aria-label={`Permalink to comment from ${timestamp(comment.created_at)}`} onClick={event => { event.preventDefault(); history.replaceState(history.state, '', `${location.pathname}${location.search}#comment-${comment.id}`); setHash(`#comment-${comment.id}`); document.getElementById(`comment-${comment.id}`)?.scrollIntoView({ block: 'nearest' }); }}><time dateTime={comment.created_at} title={new Date(comment.created_at).toLocaleString()}>{timestamp(comment.created_at)}</time></a>{comment.edited_at && !comment.deleted_at && <span className="comment-edited">edited</span>}{!readOnly && !comment.deleted_at && (comment.can_edit || comment.can_delete) && <details className="comment-menu" onKeyDown={event => { if (event.key === 'Escape' && event.currentTarget.open) { event.preventDefault(); event.stopPropagation(); event.currentTarget.open = false; event.currentTarget.querySelector('summary')?.focus({ preventScroll: true }); } }}><summary aria-label="Comment actions"><MoreHorizontal aria-hidden="true" /></summary><div>{comment.can_edit && <button type="button" disabled={loading || editDirty || editBusy || editBlocked} title={editDirty ? "Finish the current comment edit first." : undefined} onClick={event => { event.currentTarget.closest('details')?.removeAttribute('open'); setEditing(comment.id); }}>Edit comment</button>}{comment.can_delete && <button type="button" disabled={editDirty || editBusy || editBlocked} onClick={event => { event.currentTarget.closest('details')?.removeAttribute('open'); setRemoving(comment); }}>Delete comment</button>}</div></details>}</header>
        {editing === comment.id ? <EditComment key={comment.id} comment={comment} workspaceId={workspaceId} session={session} projectionScope={projectionScope} readOnly={readOnly} dirtyChanged={setEditDirty} busyChanged={setEditBusy} blockedChanged={setEditBlocked} close={() => { setEditing(null); setEditDirty(false); setEditBusy(false); setEditBlocked(false); }} saved={async value => { await update(value); setEditing(null); setEditDirty(false); setEditBusy(false); setEditBlocked(false); }} denied={() => { setDenied(true); setComments([]); }} /> : comment.deleted_at ? <p className="comment-tombstone">Comment deleted.</p> : comment.document ? <ReferenceProvider workspaceId={workspaceId} session={session} target={{ document_id: comment.document.id }} references={comment.document.mentions ?? []} issueReferences={comment.document.issue_references ?? []} projectionScope={projectionScope}><DocumentView body={comment.document.body} supported={comment.document.schema_supported} fallback={comment.document.plain_text} /></ReferenceProvider> : null}
      </div></li>)}</ol>
      {!readOnly && <div className="comment-composer"><Avatar small user={session.user} /><div><label className="comment-composer-label">Add a comment</label><ErrorBanner error={sendError} />{uncertain && <p className="document-notice" role="status">The response was interrupted. Retry safely to recover this comment without posting it twice.</p>}<StructuredEditor attachmentTarget={teamId ? { team_id: teamId } : undefined} attachmentsChanged={setUploads} value={draft} onChange={setDraft} label="Comment" compact disabled={uncertain} onCompositionChange={setComposing} onSave={() => { void send(); }} />{attachmentNotice && <p className="document-notice" role="status">{attachmentNotice}</p>}<footer><span>Ctrl / ⌘ Enter to post</span><button type="button" className="button primary compact" disabled={sending || !!uploads?.pending || composing || !hasContent(draft)} onClick={send}>{sending && <LoaderCircle className="spinner" aria-hidden="true" />}{sending ? 'Posting…' : uncertain ? 'Retry safely' : 'Post comment'}</button></footer></div></div>}
    </>}
    {removing && !denied && <DeleteComment comment={removing} workspaceId={workspaceId} session={session} close={() => setRemoving(null)} changed={async comment => { await update(comment); setRemoving(null); }} denied={() => { setDenied(true); setComments([]); }} />}
  </section></ReferenceProvider>;
}

function EditComment({ comment, workspaceId, session, projectionScope, readOnly, dirtyChanged, busyChanged, blockedChanged, close, saved, denied }: {
  comment: IssueComment; workspaceId: string; session: Session; projectionScope: string; readOnly: boolean; dirtyChanged: (value: boolean) => void; busyChanged: (value: boolean) => void; blockedChanged: (value: boolean) => void; close: () => void; saved: (comment: IssueComment) => Promise<void>; denied: () => void;
}) {
  const [uploads, setUploads] = useState<AttachmentDraftState | null>(null);
  const request = useScopedRequest(); const [base, setBase] = useState(comment); const [draft, setDraft] = useState<RichBody | null>(() => { try { return comment.document?.schema_supported ? canonicalBody(comment.document.body, true) : null; } catch { return null; } });
  const [latest, setLatest] = useState<IssueComment | null>(null); const [reason, setReason] = useState(''); const [error, setError] = useState<unknown>(null); const [busy, setBusy] = useState(false); const [uncertain, setUncertain] = useState(false); const [composing, setComposing] = useState(false); const [discarding, setDiscarding] = useState(false);
  const pending = useRef<{ body: RichBody; etag: string } | null>(null); const flight = useRef(false);
  const moderation = base.author.id !== session.user.id;
  const dirty = !!draft && !!base.document && !sameBody(draft, canonicalBody(base.document.body)); useUnsavedChanges(dirty || uncertain || busy || !!uploads?.pending);
  useEffect(() => { dirtyChanged(dirty || uncertain || !!uploads?.pending); busyChanged(busy || !!uploads?.busy); blockedChanged(uncertain || !!uploads?.pending); }, [dirty, uncertain, busy, dirtyChanged, busyChanged, blockedChanged, uploads]);
  useEffect(() => { if (comment.etag !== base.etag) { if (dirty || uncertain || busy) setLatest(comment); else { setBase(comment); try { setDraft(comment.document?.schema_supported ? canonicalBody(comment.document.body, true) : null); } catch { setDraft(null); } } } }, [comment.etag]);
  const writable = !readOnly && comment.can_edit && !comment.deleted_at && base.can_edit && !base.deleted_at;
  async function submit() {
    if (!draft || uploads?.pending || !writable || flight.current || composing || latest || uncertain || !hasContent(draft) || (moderation && !reason.trim())) return;
    const body = draft; pending.current = { body, etag: base.etag }; flight.current = true; setBusy(true); setError(null);
    try { await saved(await request<IssueComment>(`/workspaces/${workspaceId}/comments/${base.id}`, { method: 'PATCH', body: { schema_version: 1, body, ...(moderation ? { moderation_reason: reason.trim() } : {}) }, csrf: session.csrf_token, headers: { 'If-Match': base.etag } })); }
    catch (error) { setError(error); if (error instanceof ApiError && error.status === 412) { const current = error.details?.current as IssueComment | undefined; if (current?.id === base.id) setLatest(current); } else if (isAccessError(error)) denied(); else if (unknownOutcome(error)) setUncertain(true); }
    finally { flight.current = false; setBusy(false); }
  }
  async function check() {
    if (flight.current || !pending.current) return; flight.current = true; setBusy(true); setError(null);
    try { const current = await request<IssueComment>(`/workspaces/${workspaceId}/comments/${base.id}`); if (current.document && sameBody(canonicalBody(current.document.body), pending.current.body)) await saved(current); else { setUncertain(false); if (current.etag !== pending.current.etag) setLatest(current); } }
    catch (error) { setError(error); if (isAccessError(error)) denied(); } finally { flight.current = false; setBusy(false); }
  }
  let latestBody: RichBody | null = null; try { if (latest?.document?.schema_supported) latestBody = canonicalBody(latest.document.body, true); } catch { /* Keep the draft if the other version is newer than this editor. */ }
  return <ReferenceProvider workspaceId={workspaceId} session={session} target={base.document ? { document_id: base.document.id } : { issue_id: base.issue_id }} references={latest?.document?.mentions ?? comment.document?.mentions ?? []} issueReferences={latest?.document?.issue_references ?? comment.document?.issue_references ?? []} projectionScope={projectionScope}><div className="comment-edit"><ErrorBanner error={error} />{uncertain && <p className="document-notice">The edit may have been saved. <button type="button" className="quiet" disabled={busy} onClick={check}>Check edit result</button></p>}{latestBody && draft && <DocumentConflict latest={latestBody} local={draft} choose={(body, review) => { if (latest) { setBase(latest); setDraft(body); setLatest(null); setError(null); if (!review) close(); } }} />}{latest && !latestBody && <p className="document-notice">This comment was deleted or its new format is not supported. Your draft is preserved here.</p>}
    {draft ? <StructuredEditor attachmentTarget={base.document ? { document_id: base.document.id } : undefined} attachmentsChanged={setUploads} value={draft} onChange={setDraft} label="Edit comment" compact disabled={busy || uncertain || !writable} onCompositionChange={setComposing} onSave={() => { void submit(); }} /> : <><p className="notice">This comment cannot be edited by this version.</p><DocumentView body={base.document?.body} supported={base.document?.schema_supported} fallback={base.document?.plain_text} /></>}
    {moderation && <label className="field moderation-reason">Reason for editing another person’s comment<input value={reason} onChange={event => setReason(event.target.value)} disabled={busy || uncertain} required /></label>}
    <div className="comment-edit-actions"><button type="button" className="button compact" disabled={busy || uncertain} onClick={() => { if (dirty || uploads?.pending) setDiscarding(true); else close(); }}>Cancel edit</button><button type="button" className="button primary compact" disabled={!dirty || busy || !!uploads?.pending || uncertain || composing || !!latest || !writable || !draft || !hasContent(draft) || (moderation && !reason.trim())} onClick={submit}>Save comment</button></div>
    {discarding && <DiscardUploadDraft title="Discard comment changes?" description="Your unsaved changes to this comment will be discarded." action="Discard changes" uploads={uploads} close={() => setDiscarding(false)} discard={close} />}
  </div></ReferenceProvider>;
}

function DeleteComment({ comment, workspaceId, session, close, changed, denied }: {
  comment: IssueComment; workspaceId: string; session: Session; close: () => void; changed: (comment: IssueComment) => Promise<void>; denied: () => void;
}) {
  const request = useScopedRequest(); const scope = useSessionScope(session); const [referenceScope, setReferenceScope] = useState(scope); const [base, setBase] = useState(comment); const [reason, setReason] = useState(''); const [busy, setBusy] = useState(false); const [uncertain, setUncertain] = useState(false); const [review, setReview] = useState(false); const [error, setError] = useState<unknown>(null); const flight = useRef(false);
  const moderation = base.author.id !== session.user.id; const path = `/workspaces/${workspaceId}/comments/${base.id}`;
  async function remove() {
    if (flight.current || uncertain || !base.can_delete || (moderation && !reason.trim())) return; flight.current = true; setBusy(true); setError(null);
    try { await request<void>(path, { method: 'DELETE', csrf: session.csrf_token, headers: { 'If-Match': base.etag }, ...(moderation ? { body: { moderation_reason: reason.trim() } } : {}) }); await changed(await request<IssueComment>(path)); }
    catch (error) { setError(error); if (error instanceof ApiError && error.status === 412) { const latest = error.details?.current as IssueComment | undefined; if (latest?.id === base.id) { setBase(latest); setReferenceScope(scope); setReview(true); } } else if (isAccessError(error)) denied(); else if (unknownOutcome(error)) setUncertain(true); }
    finally { flight.current = false; setBusy(false); }
  }
  async function check() { if (flight.current) return; flight.current = true; setBusy(true); setError(null); try { const result = await request<IssueComment>(path); if (result.deleted_at) await changed(result); else { setBase(result); setReferenceScope(scope); setUncertain(false); setReview(true); } } catch (error) { setError(error); if (isAccessError(error)) denied(); } finally { flight.current = false; setBusy(false); } }
  return <ReferenceProvider workspaceId={workspaceId} session={session} target={base.document ? { document_id: base.document.id } : { issue_id: base.issue_id }} references={base.document?.mentions ?? []} issueReferences={base.document?.issue_references ?? []} projectionScope={referenceScope}><Modal title="Delete comment?" close={close} canClose={!busy && !uncertain}><div className="settings-modal-content"><ErrorBanner error={error} /><p>The comment text will be removed. A deleted-comment entry will remain in the discussion.</p>{review && <p className="notice">The latest comment is shown. Review it before trying again.</p>}{base.document && <DocumentView body={base.document.body} supported={base.document.schema_supported} fallback={base.document.plain_text} />}{moderation && <label className="field">Reason for deleting another person’s comment<input value={reason} onChange={event => setReason(event.target.value)} required disabled={busy || uncertain} /></label>}{uncertain && <p className="document-notice">The response was interrupted. Check whether the comment was deleted before retrying.</p>}<div className="modal-footer"><button type="button" className="button" onClick={close} disabled={busy || uncertain}>Cancel</button>{uncertain ? <button type="button" className="button" onClick={check} disabled={busy}>Check deletion result</button> : <button type="button" className="button danger" onClick={remove} disabled={busy || !base.can_delete || (moderation && !reason.trim())}>{busy ? 'Deleting…' : 'Delete comment'}</button>}</div></div></Modal></ReferenceProvider>;
}
