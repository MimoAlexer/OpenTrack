import { useRef, useState, type FormEvent } from 'react';
import { ApiError } from '../api';
import { ErrorBanner, Modal } from '../components';
import { useUnsavedChanges } from '../router';
import type { Session } from '../types';
import { useScopedRequest, UncertainNotice } from '../settings/shared';
import { categoryNames, selectable, statusColors, workflowETag } from './model';
import type { Workflow, WorkflowStatus } from './types';

export function StatusEditor({ workspaceId, workflow, status, session, canManage, close, saved, accept }: {
  workspaceId: string; workflow: Workflow; status?: WorkflowStatus; session: Session; canManage: boolean;
  close: () => void; saved: (workflow: Workflow) => void; accept: (workflow: Workflow) => void;
}) {
  const request = useScopedRequest(); const [base, setBase] = useState(workflow);
  const [name, setName] = useState(status?.name ?? ''); const [color, setColor] = useState(status?.color ?? 'slate'); const [category, setCategory] = useState(status?.category ?? 'unstarted');
  const [busy, setBusy] = useState(false); const [uncertain, setUncertain] = useState(false); const [error, setError] = useState<unknown>(null);
  const pending = useRef<{ body: object; key: string; version: string } | null>(null);
  const dirty = name !== (status?.name ?? '') || color !== (status?.color ?? 'slate') || category !== (status?.category ?? 'unstarted');
  useUnsavedChanges(dirty);
  const conflict = base.version !== workflow.version;
  const current = status && workflow.statuses.find(item => item.id === status.id);
  const unavailable = !!status && (!current || !selectable(current));
  function dismiss() { if (!busy && !uncertain && (!dirty || window.confirm('Discard this status draft?'))) close(); }
  async function submit(event: FormEvent) {
    event.preventDefault(); if (busy || !canManage || unavailable || (!uncertain && conflict) || !name.trim()) return;
    setBusy(true); setError(null);
    if (!uncertain || !pending.current) pending.current = { key: crypto.randomUUID(), version: base.version, body: status ? { name: name.trim(), color } : { name: name.trim(), category, color } };
    const intent = pending.current;
    try {
      const result = await request<Workflow>(`/workspaces/${workspaceId}/teams/${workflow.team_id}/statuses${status ? `/${status.id}` : ''}`, { method: status ? 'PATCH' : 'POST', body: intent.body, csrf: session.csrf_token, headers: { 'If-Match': workflowETag({ team_id: workflow.team_id, version: intent.version, can_manage: true }), ...(!status ? { 'Idempotency-Key': intent.key } : {}) } });
      saved(result);
    } catch (error) {
      setError(error); const ambiguous = error instanceof ApiError && (error.status === 0 || error.status === 408 || error.status >= 500);
      // Only creation has an idempotent recovery key. A versioned edit can be
      // reviewed against the latest aggregate after an uncertain response.
      setUncertain(!status && ambiguous);
      if (error instanceof ApiError && error.status === 412) { const latest = error.details?.current as Workflow | undefined; if (latest?.team_id === workflow.team_id) accept(latest); }
    } finally { setBusy(false); }
  }
  return <Modal title={status ? 'Edit status' : 'Add status'} close={dismiss} canClose={!busy && !uncertain}><form className="settings-modal-content" onSubmit={submit}>
    <ErrorBanner error={error} /><UncertainNotice show={uncertain} />
    {conflict && !uncertain && <div className="workflow-conflict" role="alert"><strong>This workflow changed.</strong><p>Your draft is preserved. {current ? `The latest name is “${current.name}”.` : status ? 'This status is no longer active.' : 'Review the latest workflow before adding a status.'}</p><div className="button-row"><button type="button" className="button compact" disabled={busy || unavailable} onClick={() => { if (current) { setName(current.name); setColor(current.color); } setBase(workflow); setError(null); }}>Use latest</button><button type="button" className="button compact" disabled={busy || unavailable} onClick={() => { setBase(workflow); setError(null); }}>Keep my draft for review</button></div></div>}
    {unavailable && <p className="settings-info">This status is being replaced or archived. Your draft cannot be applied to it.</p>}
    <fieldset disabled={busy || uncertain || !canManage || unavailable}><label className="field">Status name<input data-autofocus value={name} onChange={event => setName(event.target.value)} maxLength={100} required /></label><label className="field">Category{status ? <span className="workflow-fixed-category">{categoryNames[category]}<small>To change what this status means, replace it with a status in another category.</small></span> : <select value={category} onChange={event => setCategory(event.target.value as WorkflowStatus['category'])}>{Object.entries(categoryNames).map(([value, label]) => <option key={value} value={value}>{label}</option>)}</select>}</label><label className="field">Color<select value={color} onChange={event => setColor(event.target.value as WorkflowStatus['color'])}>{statusColors.map(value => <option key={value} value={value}>{value[0].toUpperCase() + value.slice(1)}</option>)}</select></label></fieldset>
    <div className="modal-footer"><button type="button" className="button" disabled={busy || uncertain} onClick={dismiss}>Cancel</button><button className="button primary" disabled={busy || !canManage || unavailable || (!uncertain && (conflict || !name.trim()))}>{busy ? 'Saving…' : uncertain ? 'Retry safely' : status ? 'Save status' : 'Add status'}</button></div>
  </form></Modal>;
}
