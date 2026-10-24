import { GripVertical } from 'lucide-react';
import { useEffect, useLayoutEffect, useRef, useState, type PointerEvent, type RefObject } from 'react';
import { ApiError } from '../api';
import { Avatar, ErrorBanner, Loading, Modal, StatusMark } from '../components';
import { useSessionScope } from '../desktop/context';
import { issuesUrl, Link, useUnsavedChanges } from '../router';
import { useScopedRequest } from '../settings/shared';
import type { Bootstrap, Issue, Session, Status } from '../types';
import { ordered, selectable, statusLabel } from '../workflows/model';
import type { Workflow } from '../workflows/types';
import { useIssueWorkflow } from '../workflows/useIssueWorkflow';
import { readWorkflow } from '../workflows/useWorkflow';
import { restoreIssueRowFocus } from './focus';

interface Props {
  workspaceId: string; teamId: string; data: Bootstrap; session: Session; items: Issue[]; statuses: Status[];
  params: URLSearchParams; selectedId?: string; disabled: boolean; readOnly: boolean;
  headingRef?: RefObject<HTMLHeadingElement | null>;
  moved: (issue: Issue) => void; reload: () => void; denied: (error: unknown) => void;
  blockingChanged: (value: boolean) => void; workflowChanged?: (workflow: Workflow) => void;
}
type Phase = 'choose' | 'saving' | 'conflict' | 'review' | 'uncertain' | 'checking';
type Move = { issue: Issue; target: string; targetName: string; phase: Phase; origin: 'menu' | 'pointer'; error: unknown };
type Overlay = { issue: Issue; page: Issue[] };
type Drag = { pointer: number; issue: Issue; handle: HTMLButtonElement; x: number; y: number; touch: boolean; active: boolean; target: string | null; timer?: number };
const accessFailure = (error: unknown) => error instanceof ApiError && [401, 403, 404].includes(error.status);
const inFlight = (move: Move | null) => move?.phase === 'saving' || move?.phase === 'checking';

export function IssueBoard(props: Props) {
  const account = useSessionScope(props.session);
  const filters = ['team_id', 'status_id', 'assignee_id', 'priority', 'q', 'archived', 'label_ids', 'label_mode', 'sort', 'view', 'layout', 'display_version'].map(key => [key, props.params.get(key)]);
  const scope = JSON.stringify([props.workspaceId, props.teamId, account, props.data.auth_epoch, filters]);
  return <Board key={scope} {...props} />;
}

function Board(props: Props) {
  const { workspaceId, teamId, data, session, items, statuses, params, selectedId, disabled, readOnly, headingRef, workflowChanged } = props;
  const request = useScopedRequest();
  const mounted = useRef(true); const revoked = useRef(false);
  const workflow = useIssueWorkflow(workspaceId, teamId, session, data, undefined, value => { if (mounted.current && !revoked.current) workflowChanged?.(value); });
  const [move, setMove] = useState<Move | null>(null); const moveRef = useRef(move);
  const [overlays, setOverlays] = useState<Record<string, Overlay>>({});
  const [drag, setDrag] = useState<{ issue: string; active: boolean; target: string | null } | null>(null);
  const [columnStart, setColumnStart] = useState(0); const [announcement, setAnnouncement] = useState('');
  const [accessError, setAccessError] = useState<unknown>(null);
  const root = useRef<HTMLDivElement>(null); const flight = useRef(false);
  const pending = useRef<AbortController | null>(null); const dragging = useRef<Drag | null>(null);
  const focus = useRef<{ issue: string; rows: string[] } | null>(null);
  const focusedCard = useRef<{ issue: string; rows: string[]; element: HTMLElement } | null>(null);
  const current = useRef({ props, workflow }); current.current = { props, workflow };
  const blocking = !!move || !!drag;
  useUnsavedChanges(blocking);
  function changeMove(value: Move | null) { moveRef.current = value; setMove(value); }
  function stopDrag() {
    const active = dragging.current; dragging.current = null;
    if (active?.timer !== undefined) window.clearTimeout(active.timer);
    if (active?.handle.hasPointerCapture?.(active.pointer)) active.handle.releasePointerCapture(active.pointer);
    if (mounted.current) setDrag(null);
    return active;
  }
  function deny(error: unknown) {
    if (revoked.current || !mounted.current) return;
    revoked.current = true; pending.current?.abort(); stopDrag(); changeMove(null); setOverlays({}); setAccessError(error);
    current.current.props.denied(error);
  }
  useLayoutEffect(() => {
    mounted.current = true;
    return () => { mounted.current = false; pending.current?.abort(); stopDrag(); current.current.props.blockingChanged(false); };
  }, []);
  useEffect(() => { current.current.props.blockingChanged(blocking); }, [blocking]);
  useLayoutEffect(() => { if (accessFailure(workflow.error)) deny(workflow.error); }, [workflow.error]);
  useEffect(() => {
    setOverlays(previous => {
      const entries = Object.entries(previous).filter(([id, overlay]) => {
        const incoming = items.find(issue => issue.id === id);
        return incoming ? BigInt(incoming.version) < BigInt(overlay.issue.version) : items === overlay.page;
      });
      return entries.length === Object.keys(previous).length ? previous : Object.fromEntries(entries);
    });
  }, [items]);
  const live = workflow.data ? ordered(workflow.data.statuses.filter(status => status.team_id === teamId && !status.archived_at)) : [];
  const target = (id: string) => current.current.workflow.data?.statuses.find(status => status.id === id && status.team_id === teamId && selectable(status));
  function movable(issue: Issue) {
    const now = current.current;
    return !revoked.current && !now.props.disabled && !now.props.readOnly && !issue.archived_at
      && now.props.session.workspaces.find(workspace => workspace.id === workspaceId)?.role !== 'viewer'
      && !now.props.data.teams.find(team => team.id === teamId)?.archived_at
      && issue.workspace_id === workspaceId && issue.team_id === teamId && issue.id !== now.props.selectedId
      && !!now.workflow.data && !now.workflow.error && !now.workflow.loading;
  }
  function rememberFocus(id: string) {
    focus.current = { issue: id, rows: [...root.current?.querySelectorAll<HTMLAnchorElement>('a.issue-row') ?? []].map(row => row.id) };
  }
  useLayoutEffect(() => {
    const valid = () => mounted.current && !revoked.current && !accessFailure(current.current.workflow.error);
    if (focus.current && (!move || move.origin === 'pointer' && move.phase === 'saving')) {
      const destination = focus.current; focus.current = null;
      restoreIssueRowFocus(destination.issue, destination.rows, headingRef?.current, valid);
    }
    const previous = focusedCard.current;
    if (previous && !previous.element.isConnected) {
      focusedCard.current = null;
      restoreIssueRowFocus(previous.issue, previous.rows, headingRef?.current, valid);
    }
  }, [move, drag, overlays, columnStart, items, workflow.data, headingRef]);
  function cancelMove() {
    if (flight.current) return;
    const active = moveRef.current;
    if (active) { rememberFocus(active.issue.id); setAnnouncement(`Move of ${active.issue.key} canceled.`); }
    changeMove(null);
  }
  function openMove(issue: Issue) {
    if (!movable(issue) || moveRef.current || dragging.current || flight.current) return;
    changeMove({ issue, target: issue.status_id, targetName: live.find(status => status.id === issue.status_id)?.name ?? 'Status unavailable', phase: 'choose', origin: 'menu', error: null });
  }
  function validateIssue(issue: Issue, expected: string) {
    if (issue.id !== expected || issue.workspace_id !== workspaceId || issue.team_id !== teamId) {
      throw new ApiError(404, 'not_found', 'This issue is no longer available in this board.');
    }
    return issue;
  }
  async function refreshWorkflow(signal: AbortSignal) {
    const snapshot = current.current.workflow.data;
    const fresh = await readWorkflow(`/workspaces/${workspaceId}/teams/${teamId}/workflow`, snapshot, signal);
    if (!mounted.current || revoked.current || signal.aborted || accessFailure(current.current.workflow.error)) throw new DOMException('The board closed.', 'AbortError');
    const value = fresh ?? snapshot;
    if (!value || value.team_id !== teamId) throw new ApiError(404, 'not_found', 'This workflow is unavailable.');
    current.current.workflow.accept(value);
  }
  async function readCurrent(active: Move, signal: AbortSignal) {
    const [issue] = await Promise.all([
      request<Issue>(`/workspaces/${workspaceId}/issues/${active.issue.id}`, { signal }),
      refreshWorkflow(signal),
    ]);
    return validateIssue(issue, active.issue.id);
  }
  function confirm(issue: Issue, active: Move) {
    setOverlays(previous => ({ ...previous, [issue.id]: { issue, page: current.current.props.items } }));
    const index = ordered(current.current.workflow.data?.statuses.filter(status => !status.archived_at && status.team_id === teamId) ?? []).findIndex(status => status.id === issue.status_id);
    if (index >= 0) setColumnStart(start => index < start || index >= start + 5 ? Math.floor(index / 5) * 5 : start);
    rememberFocus(issue.id); changeMove(null);
    setAnnouncement(`${issue.key} is now in ${target(issue.status_id)?.name ?? active.targetName}.`);
    current.current.props.moved(issue);
  }
  async function submit(active: Move) {
    if (flight.current || !movable(active.issue) || !target(active.target)) return;
    if (active.issue.status_id === active.target) {
      rememberFocus(active.issue.id); setAnnouncement(`${active.issue.key} is already in ${active.targetName}.`); changeMove(null); return;
    }
    const abort = new AbortController(); pending.current = abort; flight.current = true;
    changeMove({ ...active, phase: 'saving', error: null });
    setAnnouncement(`Moving ${active.issue.key} to ${active.targetName}.`);
    try {
      const result = validateIssue(await request<Issue>(`/workspaces/${workspaceId}/issues/${active.issue.id}`, {
        method: 'PATCH', csrf: current.current.props.session.csrf_token, signal: abort.signal,
        body: { status_id: active.target }, headers: { 'If-Match': active.issue.etag },
      }), active.issue.id);
      if (!mounted.current || revoked.current || abort.signal.aborted || accessFailure(current.current.workflow.error)) return;
      confirm(result, active);
    } catch (error) {
      if (!mounted.current || revoked.current || abort.signal.aborted) return;
      if (accessFailure(error)) { deny(error); return; }
      if (error instanceof ApiError && [409, 412, 422].includes(error.status)) {
        try {
          const issue = await readCurrent(active, abort.signal);
          if (!mounted.current || revoked.current || abort.signal.aborted || accessFailure(current.current.workflow.error)) return;
          changeMove({ ...active, issue, phase: 'conflict', error }); current.current.props.reload();
        } catch (readError) {
          if (!mounted.current || revoked.current || abort.signal.aborted) return;
          if (accessFailure(readError)) deny(readError); else changeMove({ ...active, phase: 'uncertain', error: readError });
        }
      } else changeMove({ ...active, phase: 'uncertain', error });
    } finally {
      flight.current = false; if (pending.current === abort) pending.current = null;
    }
  }
  async function checkResult() {
    const active = moveRef.current; if (!active || flight.current) return;
    const abort = new AbortController(); pending.current = abort; flight.current = true;
    changeMove({ ...active, phase: 'checking', error: null });
    try {
      const issue = await readCurrent(active, abort.signal);
      if (!mounted.current || revoked.current || abort.signal.aborted || accessFailure(current.current.workflow.error)) return;
      if (issue.status_id === active.target) confirm(issue, active);
      else { changeMove({ ...active, issue, phase: 'conflict', error: null }); current.current.props.reload(); }
    } catch (error) {
      if (!mounted.current || revoked.current || abort.signal.aborted) return;
      if (accessFailure(error)) deny(error); else changeMove({ ...active, phase: 'uncertain', error });
    } finally {
      flight.current = false; if (pending.current === abort) pending.current = null;
    }
  }
  function cancelDrag() {
    const active = stopDrag();
    if (active) { rememberFocus(active.issue.id); setAnnouncement(`Move of ${active.issue.key} canceled.`); }
  }
  useEffect(() => {
    const escape = (event: KeyboardEvent) => {
      if (event.key === 'Escape' && dragging.current) { event.preventDefault(); event.stopPropagation(); cancelDrag(); }
    };
    window.addEventListener('keydown', escape, true);
    return () => window.removeEventListener('keydown', escape, true);
  }, []);
  function activate(active: Drag) {
    if (dragging.current !== active || !mounted.current || !movable(active.issue) || moveRef.current) { cancelDrag(); return; }
    active.active = true; setDrag({ issue: active.issue.id, active: true, target: active.target });
    setAnnouncement(`Lifted ${active.issue.key}. Choose a status column; Escape cancels.`);
  }
  function pointerDown(event: PointerEvent<HTMLButtonElement>, issue: Issue) {
    if (event.button !== 0 || event.isPrimary === false || !movable(issue) || moveRef.current || dragging.current || flight.current) return;
    event.stopPropagation(); event.currentTarget.focus();
    const active: Drag = { pointer: event.pointerId, issue, handle: event.currentTarget, x: event.clientX, y: event.clientY, touch: event.pointerType === 'touch', active: false, target: null };
    dragging.current = active; setDrag({ issue: issue.id, active: false, target: null });
    active.handle.setPointerCapture?.(active.pointer);
    if (active.touch) active.timer = window.setTimeout(() => activate(active), 200);
  }
  function destination(x: number, y: number) {
    const element = typeof document.elementFromPoint === 'function' ? document.elementFromPoint(x, y) : null;
    const column = element?.closest<HTMLElement>('[data-board-status]');
    const id = column && root.current?.contains(column) ? column.dataset.boardStatus : undefined;
    return id && target(id) ? id : null;
  }
  function pointerMove(event: PointerEvent<HTMLButtonElement>) {
    const active = dragging.current; if (!active || active.pointer !== event.pointerId) return;
    if (!active.active && Math.hypot(event.clientX - active.x, event.clientY - active.y) >= 6) {
      if (active.touch) { cancelDrag(); return; }
      activate(active);
    }
    if (!active.active || dragging.current !== active) return;
    event.preventDefault();
    const id = destination(event.clientX, event.clientY);
    if (id !== active.target) {
      active.target = id; setDrag({ issue: active.issue.id, active: true, target: id });
      setAnnouncement(id ? `Move ${active.issue.key} to ${target(id)?.name}.` : 'Outside a status column. Release to cancel.');
    }
  }
  function pointerUp(event: PointerEvent<HTMLButtonElement>) {
    const active = dragging.current; if (!active || active.pointer !== event.pointerId) return;
    const id = active.active ? destination(event.clientX, event.clientY) : null;
    stopDrag();
    if (!id || !movable(active.issue)) { rememberFocus(active.issue.id); setAnnouncement(`Move of ${active.issue.key} canceled.`); return; }
    const selected = target(id); if (!selected) return;
    rememberFocus(active.issue.id);
    void submit({ issue: active.issue, target: id, targetName: selected.name, phase: 'choose', origin: 'pointer', error: null });
  }
  const displayed = items.slice(0, 50).filter(issue => issue.workspace_id === workspaceId && issue.team_id === teamId).map(issue => {
    const saved = overlays[issue.id]?.issue;
    const base = saved && BigInt(saved.version) > BigInt(issue.version) ? saved : issue;
    return move?.phase === 'saving' && move.issue.id === issue.id && BigInt(base.version) <= BigInt(move.issue.version)
      ? { ...base, status_id: move.target } : base;
  }).filter(issue => !params.get('status_id') || issue.status_id === params.get('status_id'));
  const missing = [...new Set(displayed.map(issue => issue.status_id))].filter(id => !live.some(status => status.id === id));
  const columns = [...live.map(status => ({ id: status.id, name: statusLabel(status), status, available: selectable(status) })), ...missing.map(id => ({ id, name: statuses.find(status => status.id === id && status.team_id === teamId)?.name ?? 'Status unavailable', status: statuses.find(status => status.id === id && status.team_id === teamId), available: false }))];
  const start = Math.min(columnStart, Math.max(0, columns.length - 1));
  if (accessError || accessFailure(workflow.error)) return <div className="issue-board-shell"><ErrorBanner error={accessError ?? workflow.error}/><p>Board unavailable. Current access could not be confirmed.</p></div>;
  if (!workflow.data) return <div className="issue-board-shell"><ErrorBanner error={workflow.error} retry={workflow.refresh}/>{!workflow.error && <Loading label="Loading board workflow…"/>}</div>;
  const busy = inFlight(move);
  const canApply = !!move && movable(move.issue) && !!target(move.target) && !busy;
  const currentStatus = move && (live.find(status => status.id === move.issue.status_id)?.name ?? statuses.find(status => status.id === move.issue.status_id && status.team_id === teamId)?.name ?? 'Status unavailable');
  return <div ref={root} className="issue-board-shell" role="region" aria-label="Issue board" onFocusCapture={event => {
    const element = event.target;
    const row = element instanceof HTMLElement ? element.closest('.issue-board-card')?.querySelector<HTMLAnchorElement>('a.issue-row') : null;
    focusedCard.current = row && element instanceof HTMLElement ? { issue: row.id.slice(4), element, rows: [...root.current?.querySelectorAll<HTMLAnchorElement>('a.issue-row') ?? []].map(item => item.id) } : null;
  }}>
    <ErrorBanner error={workflow.error} retry={workflow.refresh}/>
    <nav className="issue-board-controls" aria-label="Board columns">
      <button type="button" className="button compact" disabled={disabled || blocking || start === 0} onClick={() => setColumnStart(Math.max(0, start - 5))}>Previous columns</button>
      <select aria-label="Board columns" value={columns[start]?.id ?? ''} disabled={disabled || blocking || !columns.length} onChange={event => setColumnStart(columns.findIndex(column => column.id === event.target.value))}>{columns.map(column => <option value={column.id} key={column.id}>{column.name}</option>)}</select>
      <span>Columns {columns.length ? start + 1 : 0}–{Math.min(start + 5, columns.length)} of {columns.length}</span>
      <button type="button" className="button compact" disabled={disabled || blocking || start + 5 >= columns.length} onClick={() => setColumnStart(start + 5)}>Next columns</button>
    </nav>
    <p className="project-help">Drag to change status. Cards follow the selected sort order.</p>
    <div className="issue-groups issue-board">{columns.slice(start, start + 5).map(column => {
      const cards = displayed.filter(issue => issue.status_id === column.id);
      return <section key={column.id} className={`issue-board-column${drag?.active && drag.target === column.id ? ' is-drop-target' : ''}`} aria-label={column.name} data-board-status={column.available ? column.id : undefined}>
        <h2 className="issue-board-column-heading"><StatusMark status={column.status}/>{column.name}<span>{cards.length} on this page</span></h2>
        {!cards.length && <p className="issue-board-empty">No issues on this page</p>}
        {cards.map(issue => <article key={issue.id} className={`issue-board-card${drag?.active && drag.issue === issue.id ? ' is-lifted' : ''}${move?.phase === 'saving' && move.issue.id === issue.id ? ' is-moving' : ''}`}>
          <Link id={`row-${issue.id}`} className={`issue-row${selectedId === issue.id ? ' selected' : ''}`} href={issuesUrl(workspaceId, params, issue.id)} aria-current={selectedId === issue.id ? 'page' : undefined}><span className="issue-key">{issue.key}</span><span className="issue-title">{issue.title}</span><Avatar small user={issue.assignee ?? (issue.assignee_id ? { display_name: 'Assignee unavailable' } : undefined)}/></Link>
          <div className="issue-board-card-actions"><button type="button" className="button compact" aria-label={`Move ${issue.key}`} disabled={!movable(issue) || blocking} onClick={() => openMove(issue)}>Move</button><button type="button" className="issue-board-drag icon-button" aria-label={`Drag ${issue.key}`} disabled={!movable(issue) || !!move} onPointerDown={event => pointerDown(event, issue)} onPointerMove={pointerMove} onPointerUp={pointerUp} onPointerCancel={cancelDrag} onLostPointerCapture={() => { if (dragging.current?.issue.id === issue.id) cancelDrag(); }}><GripVertical aria-hidden="true" /></button></div>
        </article>)}
      </section>;
    })}</div>
    <div className="issue-board-announcement" role="status" aria-live="polite">{announcement}</div>
    {move && (move.origin === 'menu' || move.phase !== 'saving') && <Modal title={`Move ${move.issue.key}`} close={cancelMove} canClose={!busy}><form className="issue-board-move" onSubmit={event => { event.preventDefault(); if (move.phase === 'choose' || move.phase === 'review') void submit(move); }}>
      <ErrorBanner error={move.error}/>
      <p className="issue-board-move-status">Current status: {currentStatus}. Requested status: {move.targetName}.</p>
      {move.phase === 'uncertain' && <p>The outcome could not be confirmed. Check the saved issue before attempting another move.</p>}
      {move.phase === 'conflict' && <p>The latest saved issue is loaded. Review the requested move before applying it. Status and assignee rules are checked by the server.</p>}
      {!target(move.target) && <p>The requested status is unavailable. Choose an available status; no replacement is selected automatically.</p>}
      {move.issue.id === selectedId && <p>Close the issue detail or use its status control before moving this card.</p>}
      <label className="field">Status<select data-autofocus aria-label={`Move ${move.issue.key} to status`} value={move.target} disabled={busy || move.phase === 'uncertain' || move.phase === 'conflict'} onChange={event => { const status = target(event.target.value); if (status) changeMove({ ...move, target: status.id, targetName: status.name }); }}>
        {!live.some(status => status.id === move.target) && <option value={move.target} disabled>{move.targetName} · Unavailable</option>}
        {live.map(status => <option value={status.id} key={status.id} disabled={!selectable(status)}>{statusLabel(status)}</option>)}
      </select></label>
      <div className="modal-footer"><button type="button" className="button" disabled={busy} onClick={cancelMove}>Cancel move</button>
        {move.phase === 'uncertain' ? <button type="button" className="button primary" onClick={() => void checkResult()}>Check move result</button>
          : move.phase === 'conflict' ? <button type="button" className="button primary" disabled={disabled || readOnly || !workflow.data || !!workflow.error} onClick={event => { event.preventDefault(); changeMove({ ...move, phase: 'review' }); }}>Review move</button>
            : <button type="submit" className="button primary" disabled={!canApply}>{busy ? move.phase === 'checking' ? 'Checking…' : 'Moving…' : move.phase === 'review' ? 'Apply move' : `Move ${move.issue.key}`}</button>}
      </div>
    </form></Modal>}
  </div>;
}
