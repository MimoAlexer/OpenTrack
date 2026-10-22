import { useRef, useState, type FormEvent } from 'react';
import { ApiError } from '../api';
import { ErrorBanner, Loading, Modal } from '../components';
import type { Session } from '../types';
import { useResource } from '../settings/useResource';
import { UncertainNotice, useScopedRequest } from '../settings/shared';
import { categoryNames, selectable, workflowETag } from './model';
import type { ReplacementPreview, Workflow, WorkflowReplacement, WorkflowStatus } from './types';

export function ReplaceStatus({ workspaceId, workflow, source, session, canManage, close, accepted, accept }: {
  workspaceId: string; workflow: Workflow; source: WorkflowStatus; session: Session; canManage: boolean; close: () => void;
  accepted: (job: WorkflowReplacement) => void; accept: (workflow: Workflow) => void;
}) {
  const request = useScopedRequest(); const [target, setTarget] = useState(''); const [acknowledged, setAcknowledged] = useState(false);
  const [busy, setBusy] = useState(false); const [uncertain, setUncertain] = useState(false); const [error, setError] = useState<unknown>(null);
  const path = `/workspaces/${workspaceId}/teams/${workflow.team_id}/statuses/${source.id}`;
  const preview = useResource<ReplacementPreview>(target ? `${path}/replacement-preview?replacement_status_id=${encodeURIComponent(target)}` : null);
  const targets = workflow.statuses.filter(status => status.id !== source.id && selectable(status));
  const destination = targets.find(status => status.id === target); const categoryChange = !!destination && source.category !== destination.category;
  const validSource = workflow.statuses.some(status => status.id === source.id && selectable(status));
  const pending = useRef<{ key: string; target: string; version: string } | null>(null);
  const stalePreview = !!preview.data && preview.data.workflow_version !== workflow.version;
  async function submit(event: FormEvent) {
    event.preventDefault(); if (busy || !canManage || (!uncertain && (!destination || !validSource || !preview.data || stalePreview || (categoryChange && !acknowledged)))) return;
    if (!uncertain || !pending.current) pending.current = { key: crypto.randomUUID(), target, version: preview.data!.workflow_version };
    setBusy(true); setError(null); const intent = pending.current;
    try {
      const job = await request<WorkflowReplacement>(`${path}/replace`, { method: 'POST', body: { replacement_status_id: intent.target }, csrf: session.csrf_token, headers: { 'Idempotency-Key': intent.key, 'If-Match': workflowETag({ team_id: workflow.team_id, version: intent.version, can_manage: true }) } });
      accepted(job);
    } catch (error) {
      setError(error); setUncertain(error instanceof ApiError && (error.status === 0 || error.status === 408 || error.status >= 500));
      if (error instanceof ApiError && error.status === 412) { const current = error.details?.current as Workflow | undefined; if (current?.team_id === workflow.team_id) accept(current); preview.reload(); }
    } finally { setBusy(false); }
  }
  return <Modal title={`Replace ${source.name}`} close={close} canClose={!busy && !uncertain}><form className="settings-modal-content" onSubmit={submit}>
    <p>Move issues to another status, then archive <strong>{source.name}</strong>. The work continues if you leave this page.</p><ErrorBanner error={error} /><UncertainNotice show={uncertain} />
    <fieldset disabled={busy || uncertain || !canManage}><label className="field">Replacement status<select data-autofocus value={target} required onChange={event => { setTarget(event.target.value); setAcknowledged(false); setError(null); }}><option value="">Choose a status</option>{targets.map(status => <option key={status.id} value={status.id}>{status.name} · {categoryNames[status.category]}</option>)}</select></label>{!targets.length && <p className="settings-info">Add another active status before replacing this one.</p>}
      {target && <><ErrorBanner error={preview.error} retry={preview.reload} />{preview.loading ? <Loading label="Checking affected issues…" /> : preview.data && <div className="replacement-preview"><strong>{preview.data.issue_count} issue{preview.data.issue_count === '1' ? '' : 's'} currently use {source.name}.</strong><p>Archived issues and retained history are included. The final count is checked when replacement starts.</p>{preview.data.source_is_default && <p><strong>{destination?.name}</strong> will become the default immediately.</p>}</div>}</>}
      {categoryChange && <label className="settings-checkbox"><input type="checkbox" checked={acknowledged} onChange={event => setAcknowledged(event.target.checked)} /><span>Change these issues from {categoryNames[source.category]} to {categoryNames[destination!.category]}.<small>This can change which work is counted as open or completed.</small></span></label>}
    </fieldset>{stalePreview && !uncertain && <div className="notice">The workflow changed. <button type="button" className="button compact" onClick={preview.reload}>Refresh preview</button></div>}{!validSource && !uncertain && <div className="notice">This status is already being replaced or archived. Close this form to see its current state.</div>}
    <div className="modal-footer"><button type="button" className="button" disabled={busy || uncertain} onClick={close}>Cancel</button><button className="button primary" disabled={busy || !canManage || (!uncertain && (!destination || !validSource || preview.loading || !preview.data || !!preview.error || stalePreview || (categoryChange && !acknowledged)))}>{busy ? 'Starting…' : uncertain ? 'Retry safely' : 'Replace status'}</button></div>
  </form></Modal>;
}
