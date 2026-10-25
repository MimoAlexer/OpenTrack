import { useEffect, useRef, useState, type PointerEvent } from 'react';
import { GripVertical } from 'lucide-react';
import { ApiError, request } from '../api';
import { ErrorBanner, Modal } from '../components';
import { useSessionScope } from '../desktop/context';
import { useUnsavedChanges } from '../router';
import { selectable, statusLabel } from '../workflows/model';
import type { Workflow } from '../workflows/types';
import type { components } from '../generated/api';
import type { Issue, IssuePage, Session } from '../types';
import type { TeamDetail } from '../settings/types';
import './IssueOrdering.css';

type Schemas = components['schemas'];
export type OrderMaintenance = Schemas['IssueOrderMaintenance'];
type PositionBody = Schemas['PositionIssue'];
type PositionAck = Schemas['IssuePosition'];
type Placement = 'append' | 'before' | 'after';
type Choice = { statusId: string; placement: Placement; anchorId?: string };
type FrozenRequest = { key: string; body: PositionBody; etag: string };
type Facts = { authority: string; issue: Issue; workflow: Workflow; candidates: Issue[]; orderVersion?: string; anchor?: Issue; anchorUnavailable: boolean };
type Intent = Choice & { id: string; sourceId: string; teamId: string; outcome: 'unsent' | 'unknown' | 'not-applied' | 'acknowledged'; phase: 'choose' | 'sending' | 'uncertain' | 'review' | 'maintenance' | 'acknowledged' | 'denied'; sent?: FrozenRequest; facts?: Facts; jobs: OrderMaintenance[]; error: unknown };
type Options = { workspaceId: string; session: Session; authority: string; ready: boolean; readOnly: boolean; updated: (issue: Issue) => void; reload: () => void; sourceDenied: (id: string) => void; teamDenied: (team: string, error: unknown) => void };
const denied = (error: unknown) => error instanceof ApiError && [401, 403, 404].includes(error.status);
const text = (value: unknown): value is string => typeof value === 'string' && !!value.trim();
const count = (value: unknown): value is string => typeof value === 'string' && /^(0|[1-9][0-9]*)$/.test(value);
const jobStates = ['queued', 'running', 'retry_wait', 'blocked', 'dead_letter', 'succeeded'];

export function validOrderJobs(value: unknown): value is OrderMaintenance[] {
  return Array.isArray(value) && value.every(job => job && text(job.id) && text(job.team_id) && text(job.status_id) && jobStates.includes(job.state) && ['staging', 'applying', 'completed'].includes(job.phase) && count(job.staged_count) && count(job.applied_count) && (job.total_count === null || count(job.total_count)) && count(job.version) && text(job.etag) && typeof job.can_resume === 'boolean' && (job.error_code === null || typeof job.error_code === 'string'));
}
export function orderMaintenance(error: unknown, teamId?: string): OrderMaintenance[] | null {
  if (!(error instanceof ApiError) || error.status !== 409 || error.code !== 'issue_order_maintenance' || error.details?.applied !== false) return null;
  return validOrderJobs(error.details.maintenance) && error.details.maintenance.length && (!teamId || error.details.maintenance.every(job => job.team_id === teamId)) ? error.details.maintenance : null;
}
function currentIssue(value: Issue, id: string, workspace: string) {
  if (!value || value.id !== id || value.workspace_id !== workspace || !text(value.team_id) || !text(value.status_id) || !text(value.etag) || !text(value.key) || typeof value.title !== 'string' || !count(value.version) || !Array.isArray(value.labels)) throw new ApiError(0, 'invalid_response', 'The current issue could not be verified. Refresh its access before continuing.');
  return value;
}
async function readTeam(workspace: string, team: string, signal: AbortSignal) {
  const value = await request<TeamDetail>(`/workspaces/${encodeURIComponent(workspace)}/teams/${encodeURIComponent(team)}`, { signal });
  if (value.id !== team || value.workspace_id !== workspace) throw new ApiError(0, 'invalid_response', 'Team access could not be verified.');
  return value;
}

export function useIssueOrdering(options: Options) {
  const [state, setState] = useState<Intent | null>(null); const stateRef = useRef(state);
  const [busy, setBusy] = useState(false); const [announcement, setAnnouncement] = useState('');
  const [focus, setFocus] = useState<{ id: string } | null>(null);
  const latest = useRef(options); latest.current = options;
  const mounted = useRef(true);
  const flight = useRef<{ abort: AbortController; authority: string; id: string } | null>(null);
  function change(value: Intent | null) { stateRef.current = value; setState(value); }
  function start(active: Intent) {
    if (flight.current || !latest.current.ready) return null;
    const operation = { abort: new AbortController(), authority: latest.current.authority, id: active.id }; flight.current = operation; setBusy(true);
    return operation;
  }
  const valid = (operation: NonNullable<typeof flight.current>) => mounted.current && !operation.abort.signal.aborted && latest.current.ready && latest.current.authority === operation.authority && stateRef.current?.id === operation.id && flight.current === operation;
  function end(operation: NonNullable<typeof flight.current>) { if (flight.current === operation) { flight.current = null; if (mounted.current) setBusy(false); } }
  async function accessFailure(active: Intent, operation: NonNullable<typeof flight.current>, error: unknown, sourceReadDenied = false) {
    if (!valid(operation)) return;
    change({ ...active, facts: undefined, phase: 'denied', error });
    if (sourceReadDenied) latest.current.sourceDenied(active.sourceId);
    else {
      try { await request<Issue>(`/workspaces/${options.workspaceId}/issues/${active.sourceId}`, { signal: operation.abort.signal }); }
      catch (readError) { if (valid(operation) && denied(readError)) latest.current.sourceDenied(active.sourceId); }
    }
    try { await readTeam(options.workspaceId, active.teamId, operation.abort.signal); }
    catch (teamError) { if (valid(operation) && denied(teamError)) latest.current.teamDenied(active.teamId, teamError); }
    if (valid(operation)) latest.current.reload();
  }
  async function refresh(active: Intent, operation: NonNullable<typeof flight.current>) {
    let sourceRead = true;
    try {
      const issue = currentIssue(await request<Issue>(`/workspaces/${options.workspaceId}/issues/${active.sourceId}`, { signal: operation.abort.signal }), active.sourceId, options.workspaceId);
      sourceRead = false;
      const workflow = await request<Workflow>(`/workspaces/${options.workspaceId}/teams/${active.teamId}/workflow`, { signal: operation.abort.signal });
      if (workflow.team_id !== active.teamId || !Array.isArray(workflow.statuses)) throw new ApiError(0, 'invalid_response', 'The current workflow could not be verified.');
      let page: IssuePage;
      try { page = await request<IssuePage>(`/workspaces/${options.workspaceId}/issues?${new URLSearchParams({ team_id: active.teamId, status_id: active.statusId, sort: 'manual', limit: '50' })}`, { signal: operation.abort.signal }); }
      catch (error) {
        const jobs = orderMaintenance(error, active.teamId);
        if (!jobs) throw error;
        const combined = [...new Map([...active.jobs, ...jobs].map(job => [job.id, job])).values()];
        if (valid(operation)) change({ ...active, jobs: combined, facts: { authority: operation.authority, issue, workflow, candidates: [], anchorUnavailable: !!active.anchorId }, phase: active.outcome === 'unknown' ? 'uncertain' : active.outcome === 'acknowledged' ? 'acknowledged' : 'maintenance', error });
        return;
      }
      if (!text(page.order_version) || !Array.isArray(page.data) || page.data.length > 50 || page.data.some(item => item.workspace_id !== options.workspaceId || item.team_id !== active.teamId || item.status_id !== active.statusId || item.archived_at)) throw new ApiError(0, 'invalid_response', 'The current issue order could not be verified.');
      let anchor: Issue | undefined; let anchorUnavailable = false;
      if (active.anchorId && active.outcome !== 'unknown' && active.outcome !== 'acknowledged') {
        try {
          anchor = currentIssue(await request<Issue>(`/workspaces/${options.workspaceId}/issues/${active.anchorId}`, { signal: operation.abort.signal }), active.anchorId, options.workspaceId);
          anchorUnavailable = anchor.team_id !== active.teamId || anchor.status_id !== active.statusId || !!anchor.archived_at || anchor.id === issue.id;
        } catch (error) {
          if (!denied(error)) throw error;
          await readTeam(options.workspaceId, active.teamId, operation.abort.signal);
          anchorUnavailable = true;
        }
      }
      if (!valid(operation)) return;
      if (active.outcome === 'acknowledged') {
        latest.current.updated(issue); latest.current.reload(); change(null); setFocus({ id: active.sourceId }); setAnnouncement('Position refreshed from the current saved issue and order.'); return;
      }
      change({ ...active, facts: { authority: operation.authority, issue, workflow, candidates: page.data, orderVersion: page.order_version, anchor, anchorUnavailable }, phase: active.outcome === 'unknown' ? 'uncertain' : active.outcome === 'unsent' ? 'choose' : 'review', jobs: [], error: anchorUnavailable ? new Error('The reference issue is unavailable. Choose another reference or the end of the status.') : issue.team_id !== active.teamId ? new Error('This issue moved to another team. Start a new move from its current view.') : null });
      if (active.outcome === 'not-applied') latest.current.reload();
    } catch (error) {
      if (!valid(operation)) return;
      if (denied(error)) await accessFailure(active, operation, error, sourceRead);
      else change({ ...active, facts: undefined, error, phase: active.outcome === 'unknown' ? 'uncertain' : active.outcome === 'acknowledged' ? 'acknowledged' : 'review' });
    }
  }
  async function review() {
    const active = stateRef.current; if (!active) return;
    const operation = start(active); if (!operation) return;
    try { await refresh(active, operation); } finally { end(operation); }
  }
  async function send(active: Intent, sent: FrozenRequest) {
    const operation = start(active); if (!operation) return;
    const pending: Intent = { ...active, sent, outcome: 'unknown', phase: 'sending', error: null };
    change(pending);
    try {
      const ack = await request<PositionAck>(`/workspaces/${options.workspaceId}/issues/${active.sourceId}/position`, { method: 'POST', csrf: latest.current.session.csrf_token, signal: operation.abort.signal, body: sent.body, headers: { 'If-Match': sent.etag, 'Idempotency-Key': sent.key } });
      if (!ack || ack.issue?.id !== active.sourceId || ack.issue.workspace_id !== options.workspaceId || !text(ack.order_version)) throw new ApiError(0, 'invalid_response', 'The position response could not be verified. Check the same request.');
      if (!valid(operation)) return;
      const acknowledged: Intent = { ...pending, outcome: 'acknowledged', phase: 'acknowledged' }; change(acknowledged);
      await refresh(acknowledged, operation);
    } catch (error) {
      if (!valid(operation)) return;
      const jobs = orderMaintenance(error, active.teamId);
      if (jobs) {
        const maintenance: Intent = { ...pending, outcome: 'not-applied', phase: 'maintenance', jobs, error }; change(maintenance); latest.current.reload();
        await refresh(maintenance, operation);
      }
      else if (denied(error)) await accessFailure(pending, operation, error);
      else if (error instanceof ApiError && [409, 412, 422].includes(error.status)) {
        const conflict: Intent = { ...pending, outcome: 'not-applied', phase: 'review', facts: undefined, error }; change(conflict);
        await refresh(conflict, operation);
      } else change({ ...pending, phase: 'uncertain', error });
    } finally { end(operation); }
  }
  function begin(issue: Issue, teamId: string, orderVersion: string, choice?: Partial<Choice>, pointer = false) {
    if (stateRef.current || !latest.current.ready || latest.current.readOnly || !text(orderVersion) || issue.workspace_id !== options.workspaceId || issue.team_id !== teamId || issue.archived_at) return;
    const active: Intent = { id: crypto.randomUUID(), sourceId: issue.id, teamId, statusId: choice?.statusId ?? issue.status_id, placement: choice?.placement ?? 'append', anchorId: choice?.anchorId, outcome: 'unsent', phase: 'choose', jobs: [], error: null };
    change(active);
    if (pointer) {
      const body: PositionBody = { order_version: orderVersion, status_id: active.statusId, ...(active.placement === 'before' && active.anchorId ? { before_id: active.anchorId } : active.placement === 'after' && active.anchorId ? { after_id: active.anchorId } : {}) };
      void send(active, Object.freeze({ key: crypto.randomUUID(), etag: issue.etag, body: Object.freeze(body) }));
    } else void review();
  }
  function choose(choice: Partial<Choice>) {
    const active = stateRef.current; if (!active || flight.current || active.outcome === 'unknown' || active.outcome === 'acknowledged') return;
    const next = { ...active, ...choice, phase: 'choose' as const, error: null };
    if (choice.statusId && choice.statusId !== active.statusId) { next.anchorId = undefined; next.facts = undefined; change(next); void review(); }
    else {
      if (next.placement === 'append') next.anchorId = undefined;
      const facts = next.facts;
      const anchor = facts?.candidates.find(item => item.id === next.anchorId) ?? (facts?.anchor?.id === next.anchorId ? facts?.anchor : undefined);
      if (next.facts) next.facts = { ...next.facts, anchor, anchorUnavailable: next.placement !== 'append' && !anchor };
      change(next);
    }
  }
  function apply() {
    const active = stateRef.current; const facts = active?.facts;
    if (!active || !facts || facts.authority !== latest.current.authority || latest.current.readOnly || active.outcome === 'unknown' || active.outcome === 'acknowledged' || !facts.orderVersion || facts.issue.team_id !== active.teamId || facts.issue.archived_at || !facts.workflow.statuses.some(status => status.id === active.statusId && selectable(status)) || active.placement !== 'append' && (!active.anchorId || facts.anchorUnavailable)) return;
    const body: PositionBody = { order_version: facts.orderVersion, status_id: active.statusId, ...(active.placement === 'before' ? { before_id: active.anchorId } : active.placement === 'after' ? { after_id: active.anchorId } : {}) };
    void send(active, Object.freeze({ key: crypto.randomUUID(), body: Object.freeze(body), etag: facts.issue.etag }));
  }
  function check() { const active = stateRef.current; if (active?.outcome === 'unknown' && active.sent) void send(active, active.sent); }
  function cancel() { const active = stateRef.current; if (!flight.current && active && (active.outcome !== 'unknown' || active.phase === 'denied')) { if (active.outcome === 'acknowledged') latest.current.reload(); change(null); setFocus({ id: active.sourceId }); } }
  useUnsavedChanges(!!state);
  useEffect(() => {
    flight.current?.abort.abort(); flight.current = null; setBusy(false); setAnnouncement(''); setFocus(null);
    const active = stateRef.current;
    if (active) { change({ ...active, facts: undefined, phase: active.outcome === 'unknown' ? 'uncertain' : active.phase }); if (options.ready) void review(); }
  }, [options.authority, options.ready]);
  useEffect(() => { mounted.current = true; return () => { mounted.current = false; flight.current?.abort.abort(); }; }, []);
  const facts = options.ready && state?.facts?.authority === options.authority ? state.facts : null;
  return { state, facts, busy, announcement, focus, ready: options.ready, readOnly: options.readOnly, begin, choose, apply, check, review, cancel };
}
export type IssueOrderingController = ReturnType<typeof useIssueOrdering>;

export function IssueOrderProgress({ workspaceId, session, initial, changed, teamDenied }: { workspaceId: string; session: Session; initial: OrderMaintenance[]; changed?: () => void; teamDenied?: (team: string, error: unknown) => void }) {
  const scope = useSessionScope(session); const key = JSON.stringify([workspaceId, scope, initial.map(job => [job.id, job.team_id])]);
  const [result, setResult] = useState<{ key: string; jobs: OrderMaintenance[]; error: unknown }>({ key: '', jobs: [], error: null });
  const [busy, setBusy] = useState(false); const [attempt, setAttempt] = useState(0); const flight = useRef<{ abort: AbortController } | null>(null);
  const current = useRef(key); current.current = key;
  const operation = useRef<AbortController | null>(null);
  const complete = useRef('');
  async function unavailable(job: OrderMaintenance, abort: AbortController, error: unknown) {
    if (denied(error)) {
      try { await readTeam(workspaceId, job.team_id, abort.signal); }
      catch (teamError) { if (!abort.signal.aborted && current.current === key && denied(teamError)) teamDenied?.(job.team_id, teamError); }
    }
  }
  async function readJob(job: OrderMaintenance, abort: AbortController) {
    try {
      const value = await request<OrderMaintenance>(`/workspaces/${workspaceId}/teams/${job.team_id}/issue-order-maintenance/${job.id}`, { signal: abort.signal });
      if (!validOrderJobs([value]) || value.id !== job.id || value.team_id !== job.team_id) throw new ApiError(0, 'invalid_response', 'Ordering progress could not be verified.');
      return value;
    } catch (error) { await unavailable(job, abort, error); throw error; }
  }
  useEffect(() => {
    const abort = new AbortController(); operation.current = abort; let timer: ReturnType<typeof setTimeout>;
    async function poll() {
      if (flight.current && !flight.current.abort.signal.aborted) { timer = setTimeout(poll, 2000); return; }
      const token = { abort }; flight.current = token; setBusy(true);
      try {
        const jobs: OrderMaintenance[] = [];
        for (const job of initial) jobs.push(await readJob(job, abort));
        if (abort.signal.aborted || current.current !== key) return;
        setResult({ key, jobs, error: null });
        if (jobs.length && jobs.every(job => job.phase === 'completed' || job.state === 'succeeded')) {
          if (complete.current !== key) { complete.current = key; changed?.(); }
          return;
        }
      } catch (error) { if (!abort.signal.aborted && current.current === key) setResult({ key, jobs: [], error }); }
      finally { if (flight.current === token) flight.current = null; if (!abort.signal.aborted && current.current === key) setBusy(false); }
      if (!abort.signal.aborted) timer = setTimeout(poll, 2000);
    }
    void poll(); return () => { abort.abort(); clearTimeout(timer); };
  }, [key, attempt]);
  async function resume(job: OrderMaintenance) {
    if (flight.current && !flight.current.abort.signal.aborted || !operation.current || operation.current.signal.aborted) return;
    const abort = operation.current; const token = { abort }; flight.current = token; setBusy(true);
    try {
      const fresh = await readJob(job, abort);
      if (!fresh.can_resume) throw new Error('Ordering maintenance cannot currently be resumed with your access. Refresh its progress.');
      const value = await request<OrderMaintenance>(`/workspaces/${workspaceId}/teams/${fresh.team_id}/issue-order-maintenance/${fresh.id}/resume`, { method: 'POST', csrf: session.csrf_token, signal: abort.signal, headers: { 'If-Match': fresh.etag } });
      if (!validOrderJobs([value]) || value.id !== fresh.id || value.team_id !== fresh.team_id) throw new ApiError(0, 'invalid_response', 'The resumed ordering progress could not be verified.');
      if (!abort.signal.aborted && current.current === key) { setResult(previous => ({ key, jobs: previous.jobs.map(item => item.id === value.id ? value : item), error: null })); setAttempt(value => value + 1); }
    } catch (error) { await unavailable(job, abort, error); if (!abort.signal.aborted && current.current === key) setResult({ key, jobs: [], error }); }
    finally { if (flight.current === token) flight.current = null; if (!abort.signal.aborted && current.current === key) setBusy(false); }
  }
  const jobs = result.key === key ? result.jobs : [];
  const error = result.key === key ? result.error : null;
  return <section className="issue-order-progress" aria-label="Issue order maintenance" aria-busy={busy}><h3>Preparing issue order</h3><ErrorBanner error={error} retry={() => setAttempt(value => value + 1)}/>{!jobs.length && !error && <p role="status">Checking current progress…</p>}{jobs.map(job => <div key={job.id}><p role="status">{job.phase === 'completed' ? 'Order preparation complete.' : job.phase === 'staging' ? `Preparing ${job.staged_count} issues; ${job.total_count === null ? 'total not known yet' : `${job.total_count} in total`}.` : `Applied ${job.applied_count} of ${job.total_count ?? 'an unknown total'} issues.`}</p><dl className="issue-order-counts"><div><dt>Staged</dt><dd>{job.staged_count}</dd></div><div><dt>Applied</dt><dd>{job.applied_count}</dd></div><div><dt>Total</dt><dd>{job.total_count ?? 'Unknown'}</dd></div></dl><p>{job.state.replaceAll('_', ' ')}</p>{job.can_resume && <button type="button" className="button compact" disabled={busy || !!error} onClick={() => void resume(job)}>Resume order preparation</button>}</div>)}<p>Moves are never applied automatically after maintenance.</p></section>;
}

export function IssueOrderingDialog({ controller, workspaceId, session, teamDenied }: { controller: IssueOrderingController; workspaceId: string; session: Session; teamDenied: (team: string, error: unknown) => void }) {
  const { state, facts, busy } = controller;
  if (!state || !controller.ready) return <p className="sr-only" role="status">{controller.announcement}</p>;
  const fixed = state.outcome === 'unknown' || state.outcome === 'acknowledged';
  const statuses = facts?.workflow.statuses.filter(status => !status.archived_at) ?? [];
  const candidates = facts ? [...facts.candidates, ...(facts.anchor && !facts.candidates.some(item => item.id === facts.anchor!.id) ? [facts.anchor] : [])].filter(issue => issue.id !== state.sourceId && issue.status_id === state.statusId) : [];
  const canApply = !!facts?.orderVersion && !controller.readOnly && facts.issue.team_id === state.teamId && !facts.issue.archived_at && statuses.some(status => status.id === state.statusId && selectable(status)) && (state.placement === 'append' || !!state.anchorId && !facts.anchorUnavailable);
  return <Modal title={facts ? `Position ${facts.issue.key}` : 'Issue position'} close={controller.cancel} canClose={!busy && (!fixed || state.phase === 'denied')}><form className="issue-order-form" onSubmit={event => { event.preventDefault(); if (!fixed) controller.apply(); }} onKeyDown={event => { if (event.key === 'Enter' && (event.nativeEvent.isComposing || event.keyCode === 229 || event.repeat || event.altKey || event.getModifierState('AltGraph'))) event.preventDefault(); }}>
    <ErrorBanner error={state.error}/>
    {state.phase === 'denied' ? <p>The issue or its original team is unavailable with your current access. Any previous outcome remains unconfirmed.</p> : <>
      {!facts && <p role="status">Current issue access and order must be checked before a new move.</p>}
      {facts && <p>{facts.issue.title}</p>}
      {state.outcome === 'unknown' && <p>The outcome is uncertain. Check by sending the identical position request; no new move will be substituted.</p>}
      {state.outcome === 'acknowledged' && <p>The position was acknowledged. Refresh the current saved issue and order before continuing.</p>}
      {state.outcome === 'not-applied' && <p>The requested move was not applied. Review the current issue and order before making a new request.</p>}
      {!!state.jobs.length && facts && <IssueOrderProgress workspaceId={workspaceId} session={session} initial={state.jobs} teamDenied={teamDenied}/>}
      <fieldset disabled={busy || fixed || !facts || state.phase === 'maintenance'}><label className="field">Status<select data-autofocus aria-label="Position status" value={state.statusId} onChange={event => controller.choose({ statusId: event.target.value })}>{!statuses.some(status => status.id === state.statusId) && <option value={state.statusId}>Requested status unavailable</option>}{statuses.map(status => <option value={status.id} key={status.id} disabled={!selectable(status)}>{statusLabel(status)}</option>)}</select></label><label className="field">Position<select aria-label="Position placement" value={state.placement} onChange={event => controller.choose({ placement: event.target.value as Placement })}><option value="append">End of this status</option><option value="before">Before a reference issue</option><option value="after">After a reference issue</option></select></label>{state.placement !== 'append' && <label className="field">Reference issue<select aria-label="Position reference issue" value={state.anchorId ?? ''} onChange={event => controller.choose({ anchorId: event.target.value || undefined })}><option value="">Choose a reference issue</option>{state.anchorId && !candidates.some(issue => issue.id === state.anchorId) && <option value={state.anchorId} disabled>Original reference unavailable</option>}{candidates.map(issue => <option value={issue.id} key={issue.id}>{issue.key} · {issue.title}</option>)}</select></label>}</fieldset>
      <p>Reference choices are bounded loaded results. One selected reference is resolved against the complete authorized status group by the server.</p>
    </>}
    <div className="modal-footer"><button type="button" className="button" disabled={busy || fixed && state.phase !== 'denied'} onClick={controller.cancel}>Discard local move</button>{state.phase !== 'denied' && (state.outcome === 'unknown' ? <button type="button" className="button primary" disabled={busy} onClick={controller.check}>Check same position request</button> : <><button type="button" className="button" disabled={busy} onClick={() => void controller.review()}>Refresh current issue and order</button>{state.outcome !== 'acknowledged' && <button type="submit" className="button primary" disabled={busy || !canApply || state.phase === 'maintenance'}>Apply reviewed position</button>}</>)}</div>
  </form></Modal>;
}

export function useIssueOrderDrag({ enabled, controller, teamId, orderVersion, items }: { enabled: boolean; controller?: IssueOrderingController; teamId: string; orderVersion?: string | null; items: Issue[] }) {
  type Drag = { pointer: number; issue: Issue; teamId: string; orderVersion: string; handle: HTMLButtonElement; x: number; y: number; active: boolean; timer?: number; target?: Choice };
  const active = useRef<Drag | null>(null); const [drag, setDrag] = useState<{ issue: string; target?: string; placement?: Placement } | null>(null);
  function stop() { const value = active.current; active.current = null; if (value?.timer !== undefined) window.clearTimeout(value.timer); if (value?.handle.hasPointerCapture?.(value.pointer)) value.handle.releasePointerCapture(value.pointer); setDrag(null); return value; }
  function lift(value: Drag) { if (active.current !== value || !enabled) return; value.active = true; setDrag({ issue: value.issue.id }); }
  function pointerDown(event: PointerEvent<HTMLButtonElement>, issue: Issue) {
    if (!enabled || !controller || controller.state || !orderVersion || event.button !== 0 || event.isPrimary === false) return;
    const value: Drag = { pointer: event.pointerId, issue, teamId, orderVersion, handle: event.currentTarget, x: event.clientX, y: event.clientY, active: false }; active.current = value;
    value.handle.setPointerCapture?.(value.pointer);
    if (event.pointerType === 'touch') value.timer = window.setTimeout(() => lift(value), 180);
  }
  function pointerMove(event: PointerEvent<HTMLButtonElement>) {
    const value = active.current; if (!value || value.pointer !== event.pointerId) return;
    if (!value.active && Math.hypot(event.clientX - value.x, event.clientY - value.y) >= 5) lift(value);
    if (!value.active) return; event.preventDefault();
    const element = document.elementFromPoint?.(event.clientX, event.clientY);
    const row = element?.closest<HTMLElement>('[data-order-issue]'); const column = element?.closest<HTMLElement>('[data-order-status]');
    const issue = row && items.find(item => item.id === row.dataset.orderIssue);
    value.target = issue && issue.id !== value.issue.id && column?.dataset.orderStatus ? { statusId: column.dataset.orderStatus, placement: event.clientY < row!.getBoundingClientRect().top + row!.getBoundingClientRect().height / 2 ? 'before' : 'after', anchorId: issue.id } : !row && column?.dataset.orderStatus ? { statusId: column.dataset.orderStatus, placement: 'append' } : undefined;
    setDrag({ issue: value.issue.id, target: value.target?.anchorId ?? value.target?.statusId, placement: value.target?.placement });
  }
  function pointerUp(event: PointerEvent<HTMLButtonElement>) { const value = active.current; if (!value || value.pointer !== event.pointerId) return; stop(); if (value.active && value.target && controller) controller.begin(value.issue, value.teamId, value.orderVersion, value.target, true); }
  useEffect(() => { if (!enabled || active.current?.teamId !== teamId) stop(); }, [enabled, teamId]);
  useEffect(() => { const escape = (event: KeyboardEvent) => { if (event.key === 'Escape' && active.current) { event.preventDefault(); stop(); } }; window.addEventListener('keydown', escape, true); return () => { window.removeEventListener('keydown', escape, true); stop(); }; }, []);
  return { drag, pointerDown, pointerMove, pointerUp, cancel: stop };
}
export type IssueOrderDrag = ReturnType<typeof useIssueOrderDrag>;
export function IssueOrderActions({ issue, teamId, orderVersion, controller, drag, disabled }: { issue: Issue; teamId: string; orderVersion?: string | null; controller?: IssueOrderingController; drag: IssueOrderDrag; disabled: boolean }) {
  return <div className="issue-order-actions"><button type="button" className="button compact" aria-label={`Position ${issue.key}`} disabled={disabled || !controller || !!controller.state || !orderVersion} onClick={() => { if (!disabled && controller && orderVersion) controller.begin(issue, teamId, orderVersion); }}>Position</button><button type="button" className="icon-button issue-board-drag" aria-label={`Drag position ${issue.key}`} disabled={disabled || !controller || !!controller.state || !orderVersion} onPointerDown={event => { if (!disabled) drag.pointerDown(event, issue); }} onPointerMove={drag.pointerMove} onPointerUp={drag.pointerUp} onPointerCancel={drag.cancel} onLostPointerCapture={() => drag.cancel()}><GripVertical aria-hidden="true"/></button></div>;
}
