import { useEffect, useId, useRef, useState } from 'react';
import { MoreHorizontal, Trash2 } from 'lucide-react';
import { ApiError } from '../api';
import { ErrorBanner, Modal } from '../components';
import { useScopedRequest } from '../settings/shared';
import type { Issue, Session } from '../types';
import type { DeletedIssue, DeletionPreview } from './deletion-types';
import './deletion.css';

export function uncertainMutation(error: unknown) {
  return error instanceof ApiError && (error.status === 0 || error.status === 408 || error.status >= 500);
}

export function IssueDeleteAction({ issue, session, dirty, disabled, deleted }: { issue: Issue; session: Session; dirty: boolean; disabled: boolean; deleted: (issue: Issue) => void }) {
  const request = useScopedRequest(); const id = useId(); const wrapper = useRef<HTMLDivElement>(null); const trigger = useRef<HTMLButtonElement>(null);
  const [open, setOpen] = useState(false); const [review, setReview] = useState<DeletionPreview | null>(null);
  const [preview, setPreview] = useState<DeletionPreview | null>(null); const [error, setError] = useState<unknown>(null); const [busy, setBusy] = useState(false); const [retry, setRetry] = useState(0);
  useEffect(() => {
    if (!open) return;
    const abort = new AbortController(); setPreview(null); setError(null); setBusy(true);
    request<DeletionPreview>(`/workspaces/${issue.workspace_id}/issues/${issue.id}/deletion`, { signal: abort.signal }).then(value => { if (!abort.signal.aborted) setPreview(value); }).catch(error => { if (!abort.signal.aborted) setError(error); }).finally(() => { if (!abort.signal.aborted) setBusy(false); });
    return () => abort.abort();
  }, [open, issue.id, issue.workspace_id, issue.etag, retry]);
  useEffect(() => {
    if (!open) return;
    const outside = (event: PointerEvent) => { if (!wrapper.current?.contains(event.target as Node)) setOpen(false); };
    document.addEventListener('pointerdown', outside); return () => document.removeEventListener('pointerdown', outside);
  }, [open]);
  return <div className="issue-actions" ref={wrapper} onBlur={event => { if (event.relatedTarget && !event.currentTarget.contains(event.relatedTarget)) setOpen(false); }} onKeyDown={event => { if (open && event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); setOpen(false); trigger.current?.focus(); } }}>
    <button type="button" className="icon-button" ref={trigger} aria-label="More issue actions" aria-expanded={open} aria-controls={open ? id : undefined} onClick={() => setOpen(value => !value)}><MoreHorizontal aria-hidden="true" /></button>
    {open && <div className="issue-action-menu" id={id} role="group" aria-label="Issue actions">
      {busy && <p role="status">Checking available actions…</p>}<ErrorBanner error={error} retry={() => setRetry(value => value + 1)} />
      {preview?.can_delete ? <><button type="button" className="quiet delete-action" disabled={disabled || dirty} onClick={() => { trigger.current?.focus(); setOpen(false); setReview(preview); }}><Trash2 aria-hidden="true" />Delete issue…</button>{dirty && <p>Save or discard your edits before deleting.</p>}</> : !busy && !error && preview && <p>No additional actions are available for this issue.</p>}
    </div>}
    {review && <DeleteIssue preview={review} session={session} close={() => setReview(null)} deleted={result => { setReview(null); deleted(result); }} />}
  </div>;
}

export function DeleteIssue({ preview: initial, session, close, deleted }: { preview: DeletionPreview; session: Session; close: () => void; deleted: (issue: Issue) => void }) {
  const request = useScopedRequest(); const [preview, setPreview] = useState(initial); const [busy, setBusy] = useState(false); const [error, setError] = useState<unknown>(null);
  const [uncertain, setUncertain] = useState(false); const [needsReview, setNeedsReview] = useState(false); const [hidden, setHidden] = useState(false); const [notice, setNotice] = useState('');
  const path = `/workspaces/${preview.issue.workspace_id}/issues/${preview.issue.id}`;
  async function refresh() {
    if (busy) return; setBusy(true); setError(null);
    try {
      if (uncertain) {
        try { const value = await request<DeletedIssue>(`${path}/deleted`); deleted(value.issue); return; }
        catch (error) { if (!(error instanceof ApiError && error.status === 404)) throw error; }
      }
      const value = await request<DeletionPreview>(`${path}/deletion`);
      setPreview(value); setHidden(false); setUncertain(false); setNeedsReview(false); setNotice('The latest issue is shown below. Review it before deleting.');
    } catch (error) { setError(error); if (error instanceof ApiError && [403, 404].includes(error.status)) setHidden(true); }
    finally { setBusy(false); }
  }
  async function remove() {
    if (busy || uncertain || needsReview || !preview.can_delete) return;
    setBusy(true); setError(null); setNotice('');
    try { await request(path, { method: 'DELETE', csrf: session.csrf_token, headers: { 'If-Match': preview.issue.etag } }); deleted(preview.issue); }
    catch (error) { setError(error); if (error instanceof ApiError && [403, 404].includes(error.status)) setHidden(true); setUncertain(uncertainMutation(error)); setNeedsReview(error instanceof ApiError && [403, 404, 409, 412].includes(error.status)); }
    finally { setBusy(false); }
  }
  return <Modal title={hidden ? 'Issue unavailable' : `Delete ${preview.issue.key}?`} close={close} canClose={!busy}><div className="settings-modal-content issue-deletion-review">
    {hidden ? <p>This issue is no longer available. Its details have been hidden.</p> : <><div className="deletion-subject"><strong>{preview.issue.title}</strong>{preview.issue.description && <p>{preview.issue.description}</p>}</div>
    <p>The issue will leave normal views and move to Trash. It can be restored for 30 days while you still have the required access.</p>
    <ul className="deletion-effects">
      {preview.has_parent && <li>Its parent link will be removed.</li>}
      <li>{preview.child_count === 0 ? 'No subissues will be detached.' : `${preview.child_count} direct subissue${preview.child_count === 1 ? '' : 's'} will be detached. They will not be deleted.`}</li>
      <li>{preview.relationship_count === 0 ? 'No relationships will be removed.' : `${preview.relationship_count} relationship${preview.relationship_count === 1 ? '' : 's'} will be removed from both ends.`}</li>
    </ul>
    <p className="reference-note">Restoring this issue will not reconnect its parent, subissues or relationships. Archive instead if you want to keep those connections.</p></>}
    <ErrorBanner error={error} />{notice && <p className="notice" role="status">{notice}</p>}
    {uncertain && <p className="notice" role="status">The response was interrupted. Check the result before sending another deletion request.</p>}
    {needsReview && !uncertain && <p className="notice">This issue or your access changed. Load the latest deletion review before continuing.</p>}
    {!preview.can_delete && <p className="reference-note">You cannot delete this issue in its current state.</p>}
    <div className="modal-footer"><button type="button" className="button" data-autofocus disabled={busy} onClick={close}>Cancel</button>{uncertain || needsReview ? <button type="button" className="button" disabled={busy} onClick={refresh}>{busy ? 'Checking…' : uncertain ? 'Check deletion result' : 'Review latest issue'}</button> : <button type="button" className="button danger-button" disabled={busy || !preview.can_delete} onClick={remove}>{busy ? 'Deleting…' : 'Delete issue'}</button>}</div>
  </div></Modal>;
}
