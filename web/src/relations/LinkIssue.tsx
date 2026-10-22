import { useRef, useState, type FormEvent } from 'react';
import { ApiError } from '../api';
import { ErrorBanner, Modal } from '../components';
import { useScopedRequest, UncertainNotice } from '../settings/shared';
import type { Issue, Session, Status } from '../types';
import { IssueTargetPicker } from './IssueTargetPicker';
import { relationNames, type CreateRelationKind, type IssueGraphTarget, type RelationMutation } from './types';

export function LinkIssue({ workspaceId, issue, session, mode, close, saved, reviewed }: { workspaceId: string; issue: Issue; session: Session; mode: 'parent' | 'child' | 'relation'; close: () => void; saved: (issue: Issue) => void; reviewed: (issue: Issue) => void }) {
  const request = useScopedRequest(); const [source, setSource] = useState(issue); const [target, setTarget] = useState<IssueGraphTarget | null>(null); const [kind, setKind] = useState<CreateRelationKind>('related');
  const [busy, setBusy] = useState(false); const [error, setError] = useState<unknown>(null); const [review, setReview] = useState(false); const [uncertain, setUncertain] = useState(false);
  const pending = useRef<{ path: string; body: object; etag: string; key: string } | null>(null);
  async function refresh() {
    if (!target || busy) return; setBusy(true); setError(null);
    try { const [current, selected] = await Promise.all([request<Issue>(`/workspaces/${workspaceId}/issues/${source.id}`), request<Issue>(`/workspaces/${workspaceId}/issues/${target.id}`)]); if (current.team_id !== source.team_id || current.archived_at || selected.team_id !== current.team_id || selected.archived_at) throw new ApiError(409, 'unavailable', 'The selected issue is no longer available in this team. Choose another issue.'); const status = await request<Status>(`/workspaces/${workspaceId}/teams/${selected.team_id}/statuses/${selected.status_id}`); setSource(current); setTarget({ id: selected.id, key: selected.key, title: selected.title, status, archived_at: selected.archived_at, etag: selected.etag }); reviewed(current); setReview(false); }
    catch (error) { setError(error); } finally { setBusy(false); }
  }
  async function submit(event: FormEvent) {
    event.preventDefault(); if (!target || busy || (review && !uncertain)) return;
    if (!uncertain || !pending.current) pending.current = { key: crypto.randomUUID(), path: `/workspaces/${workspaceId}/issues/${mode === 'child' ? target.id : source.id}/${mode === 'relation' ? 'relations' : 'parent'}`, etag: mode === 'child' ? target.etag : source.etag, body: mode === 'relation' ? { kind, target_id: target.id, target_etag: target.etag } : { parent_id: mode === 'child' ? source.id : target.id, parent_etag: mode === 'child' ? source.etag : target.etag } };
    setBusy(true); setError(null);
    try { const intent = pending.current; const result = await request<Issue | RelationMutation>(intent.path, { method: mode === 'relation' ? 'POST' : 'PATCH', body: intent.body, csrf: session.csrf_token, headers: { 'If-Match': intent.etag, ...(mode === 'relation' ? { 'Idempotency-Key': intent.key } : {}) } }); const current = mode === 'child' ? await request<Issue>(`/workspaces/${workspaceId}/issues/${source.id}`) : 'issue' in result ? result.issue : result; saved(current); }
    catch (error) { setError(error); const ambiguous = error instanceof ApiError && ([0, 408].includes(error.status) || error.status >= 500); setUncertain(mode === 'relation' && ambiguous); if ((error instanceof ApiError && [409, 412].includes(error.status)) || (mode !== 'relation' && ambiguous)) setReview(true); }
    finally { setBusy(false); }
  }
  return <Modal title={mode === 'parent' ? 'Set parent issue' : mode === 'child' ? 'Add existing subissue' : 'Add relationship'} close={close} canClose={!busy && !uncertain}><form className="settings-modal-content" onSubmit={submit}>
    <p>{mode === 'child' ? `Choose an issue to put under ${source.key}. If it already has a parent, this changes its parent.` : mode === 'parent' ? `Choose the parent for ${source.key}.` : `Connect ${source.key} to work in the same team.`}</p><ErrorBanner error={error} /><UncertainNotice show={uncertain} />
    {review && !uncertain && <div className="workflow-conflict" role="alert"><strong>Review the latest issues.</strong><p>Your selection is preserved. Refresh both issues before retrying; the server will check their graph again.</p><button type="button" className="button compact" disabled={busy || !target} onClick={refresh}>Refresh selected issues</button></div>}
    {mode === 'relation' && <label className="field">Relationship<select value={kind} disabled={busy || uncertain} onChange={event => { setKind(event.target.value as CreateRelationKind); setReview(false); setError(null); }}>{(['blocked_by', 'blocks', 'related', 'duplicate_of'] as const).map(kind => <option key={kind} value={kind}>{relationNames[kind]}</option>)}</select></label>}
    <IssueTargetPicker workspaceId={workspaceId} teamId={source.team_id} excludeId={source.id} session={session} selected={target} change={target => { setTarget(target); setReview(false); setError(null); }} disabled={busy || uncertain} label={mode === 'parent' ? 'Parent issue' : mode === 'child' ? 'Subissue' : 'Related issue'} />
    {mode === 'relation' && kind === 'duplicate_of' && <p className="reference-note">This records a duplicate link. Neither issue’s status will change.</p>}
    <div className="modal-footer"><button type="button" className="button" disabled={busy || uncertain} onClick={close}>Cancel</button><button className="button primary" disabled={busy || !target || (review && !uncertain)}>{busy ? 'Saving…' : uncertain ? 'Retry safely' : mode === 'parent' ? 'Set parent' : mode === 'child' ? 'Add subissue' : 'Add relationship'}</button></div>
  </form></Modal>;
}
