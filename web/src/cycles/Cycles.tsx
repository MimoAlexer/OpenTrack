import { useEffect, useState } from 'react';
import { ArrowLeft, CalendarRange, Pencil, Plus } from 'lucide-react';
import { ApiError } from '../api';
import { ErrorBanner, Loading, Modal } from '../components';
import { useSessionBoundary } from '../desktop/context';
import { isAccessError } from '../documents/model';
import { Link, navigate } from '../router';
import { useScopedRequest } from '../settings/shared';
import { useProjectData, useProjectPage } from '../projects/useProjectData';
import type { Bootstrap, Session, Team } from '../types';
import { AddCycleIssue, CarryForward, CloseCycle } from './CycleActions';
import { CycleEditor } from './CycleEditor';
import { CurrentCycleIssues, CycleHistory, ScopeSummary, ScopeTable } from './CycleScope';
import { cyclePath, cycleUrl, date, timestamp, type CarryOperation, type CloseReport, type Cycle, type CycleIssue, type Snapshot } from './types';
import './cycles.css';

interface Props { workspaceId: string; session: Session; data: Bootstrap; cycleId?: string; params: URLSearchParams; refresh: number; readOnly: boolean }
export function Cycles(props: Props) {
  const boundary = useSessionBoundary(props.session); const selected = props.params.get('team_id'); const team = selected ? props.data.teams.find(item => item.id === selected) : props.data.teams.find(item => !item.archived_at) ?? props.data.teams[0];
  if (!team) return <div className="empty-state"><CalendarRange aria-hidden="true"/><h2>{selected ? 'Team unavailable' : 'No teams available'}</h2><p>{selected ? 'This team may have been removed, or your access changed.' : 'Join a team to plan cycles.'}</p>{selected && <Link className="button" href={`/w/${encodeURIComponent(props.workspaceId)}/cycles`}>Choose another team</Link>}</div>;
  return <section className="projects-screen cycles-screen" aria-label="Cycles" key={`${boundary}:${team.id}:${props.cycleId ?? 'list'}`}><div className="projects-team-bar"><label>Team<select aria-label="Cycle team" value={team.id} onChange={event => navigate(cycleUrl(props.workspaceId, event.target.value))}>{props.data.teams.map(item => <option key={item.id} value={item.id}>{item.name}{item.archived_at ? ' (archived)' : ''}</option>)}</select></label><span>{team.private ? 'Private team' : 'Workspace team'}</span>{team.archived_at && <span>Archived team · read only</span>}</div>{props.cycleId ? <CycleDetail {...props} team={team} cycleId={props.cycleId} readOnly={props.readOnly || !!team.archived_at}/> : <CycleDirectory {...props} team={team} readOnly={props.readOnly || !!team.archived_at}/>}</section>;
}
function CycleDirectory({ workspaceId, session, team, refresh, readOnly }: Props & { team: Team }) {
  const directory = useProjectPage<Cycle>(cyclePath(workspaceId, team.id), refresh, 50); const [creating, setCreating] = useState(false); const [denied, setDenied] = useState<unknown>(null);
  if (denied || isAccessError(directory.error)) return <><h2>Cycles unavailable</h2><ErrorBanner error={denied || directory.error}/></>;
  return <><header className="projects-heading"><div><h2>Cycles</h2><p className="project-help">Bounded team work, with explicit closure and carry-forward.</p></div>{!readOnly && <button className="button primary compact" onClick={() => setCreating(true)}><Plus aria-hidden="true"/>New cycle</button>}</header><ErrorBanner error={directory.error} retry={directory.retry}/>{directory.loading && !directory.value ? <Loading label="Loading cycles…"/> : <div className="cycle-directory" aria-busy={directory.loading}>{directory.value?.data.map(cycle => <Link className="cycle-row" key={cycle.id} href={cycleUrl(workspaceId, team.id, cycle.id)}><CalendarRange aria-hidden="true"/><div><strong>{cycle.name}</strong><span>{date(cycle.start_date)} — {date(cycle.end_date_exclusive)} exclusive · {cycle.timezone}</span></div><span className={`cycle-state cycle-${cycle.state}`}>{cycle.state}</span></Link>)}{!directory.value?.data.length && !directory.error && <div className="project-empty project-empty-large"><CalendarRange aria-hidden="true"/><h3>No cycles yet</h3><p>Create an interval, then select the work to include.</p></div>}</div>}{directory.value?.has_more && <button className="button compact project-more" disabled={directory.loading} onClick={directory.more}>Load more cycles</button>}{creating && <CycleEditor workspace={workspaceId} team={team.id} session={session} close={() => setCreating(false)} saved={cycle => navigate(cycleUrl(workspaceId, team.id, cycle.id))} denied={setDenied}/>}</>;
}
function CycleDetail({ workspaceId, session, team, cycleId, refresh, readOnly, params }: Props & { team: Team; cycleId: string }) {
  const path = cyclePath(workspaceId, team.id, cycleId); const request = useScopedRequest(); const [revision, setRevision] = useState(0); const resource = useProjectData<Cycle>(path, refresh + revision); const scope = useProjectData<Snapshot>(`${path}/scope`, refresh + revision);
  const [editing, setEditing] = useState(false); const [adding, setAdding] = useState(false); const [closing, setClosing] = useState(false); const [carrying, setCarrying] = useState(false); const [viewingReport, setViewingReport] = useState(false); const [reopening, setReopening] = useState(false); const [reason, setReason] = useState(''); const [carryOperation, setCarryOperation] = useState<CarryOperation | null>(null);
  const [report, setReport] = useState<CloseReport | null>(null);
  const [error, setError] = useState<unknown>(null); const [denied, setDenied] = useState<unknown>(null); const [busy, setBusy] = useState(false);
  const cycle = resource.value; const operationId = params.get('carry');
  useEffect(() => {
    if (!operationId || !cycle) return; let active = true;
    void (async () => { const operation = await request<CarryOperation>(`${path}/carry-forward/${encodeURIComponent(operationId)}`); const report = await request<CloseReport>(`${path}/reports/${operation.report_id}`); if (active) { setCarryOperation(operation); setReport(report); setCarrying(true); } })().catch(error => { if (active) { setError(error); if (isAccessError(error)) setDenied(error); } });
    return () => { active = false; };
  }, [operationId, cycle?.id]);
  useEffect(() => { if (cycle) document.title = `${cycle.name} · Cycles · OpenTrack`; }, [cycle?.name]);
  useEffect(() => { if (isAccessError(scope.error)) setDenied(scope.error); }, [scope.error]);
  function changed(value: Cycle) { resource.replace(value); setRevision(value => value + 1); }
  async function command(action: 'open' | 'snapshots' | 'reopen') {
    if (!cycle || busy) return; setBusy(true); setError(null);
    try { const value = await request<Cycle | Snapshot>(`${path}/${action}`, { method: 'POST', csrf: session.csrf_token, headers: { 'If-Match': cycle.etag }, ...(action === 'reopen' ? { body: { reason: reason.trim() } } : {}) }); if (action === 'snapshots') setRevision(value => value + 1); else { changed(value as Cycle); setReopening(false); setReason(''); } }
    catch (error) { setError(error); if (isAccessError(error)) setDenied(error); }
    finally { setBusy(false); }
  }
  async function remove(issue: CycleIssue) {
    if (!cycle || busy) return; setBusy(true); setError(null);
    try { changed(await request<Cycle>(`${path}/issues/${issue.id}`, { method: 'PUT', csrf: session.csrf_token, headers: { 'If-Match': cycle.etag }, body: { included: false, issue_etag: issue.etag } })); }
    catch (error) { setError(error); if (isAccessError(error)) setDenied(error); }
    finally { setBusy(false); }
  }
  async function carry(view = false) {
    if (!cycle?.latest_report_id || busy) return; setBusy(true); setError(null);
    try { setReport(await request<CloseReport>(`${path}/reports/${cycle.latest_report_id}`)); if (view) setViewingReport(true); else { setCarryOperation(null); setCarrying(true); } }
    catch (error) { setError(error); if (isAccessError(error)) setDenied(error); }
    finally { setBusy(false); }
  }
  if (denied || isAccessError(resource.error)) return <><Link className="quiet" href={cycleUrl(workspaceId, team.id)}><ArrowLeft aria-hidden="true"/>Cycles</Link><h2>Cycle unavailable</h2><ErrorBanner error={denied || resource.error}/><p>This cycle may have been removed, or your access changed.</p></>;
  if (!cycle) return <><ErrorBanner error={resource.error} retry={resource.retry}/>{!resource.error && <Loading label="Loading cycle…"/>}</>;
  const canManage = !readOnly && cycle.can_manage; const canPlan = !readOnly && cycle.can_plan && cycle.state !== 'closed';
  return <><header className="project-detail-heading"><Link className="quiet project-back" href={cycleUrl(workspaceId, team.id)}><ArrowLeft aria-hidden="true"/>Cycles</Link><div className="project-title-line"><div className="project-title"><CalendarRange aria-hidden="true"/><h2>{cycle.name}</h2><span className={`cycle-state cycle-${cycle.state}`}>{cycle.state}</span></div><div className="button-row">{cycle.latest_report_id && <button className="button compact" disabled={busy} onClick={() => void carry(true)}>View closure report</button>}{canManage && cycle.state !== 'closed' && <button className="button compact" onClick={() => setEditing(true)}><Pencil aria-hidden="true"/>Edit cycle</button>}{canManage && cycle.state === 'planned' && <button className="button compact" disabled={busy} onClick={() => void command('open')}>Start cycle</button>}{canManage && cycle.state === 'active' && <button className="button compact" onClick={() => setClosing(true)}>Review closure</button>}{canManage && cycle.state === 'closed' && <><button className="button compact" onClick={() => setReopening(true)}>Reopen cycle</button><button className="button compact" disabled={busy || !cycle.latest_report_id} onClick={() => void carry()}>Carry unfinished work</button></>}</div></div><p className="cycle-dates">{date(cycle.start_date)} to {date(cycle.end_date_exclusive)} exclusive · {cycle.timezone}</p><details className="cycle-boundaries"><summary>Calendar boundaries</summary><p>Start: <time dateTime={cycle.starts_at}>{timestamp(cycle.starts_at)}</time> · {cycle.starts_at}<br/>Exclusive end: <time dateTime={cycle.ends_at}>{timestamp(cycle.ends_at)}</time> · {cycle.ends_at}</p></details></header>
    <ErrorBanner error={resource.error} retry={resource.retry}/><ErrorBanner error={error}/>{error instanceof ApiError && [409, 412].includes(error.status) && <div className="notice">The cycle or issue changed. Refresh, then review the latest state before trying again. <button className="button compact" onClick={() => { setError(null); setRevision(value => value + 1); }}>Load latest cycle</button></div>}
    <section className="cycle-scope" aria-label="Cycle scope" aria-busy={scope.loading}>{cycle.state === 'closed' && <p className="project-help">This is the current scope after explicit corrections or carry-forward. View the closure report for the original snapshot.</p>}<header className="project-section-heading"><h3>Scope and progress</h3><div className="button-row"><button className="button compact" disabled={scope.loading || busy} onClick={scope.retry}>Refresh cycle counts</button>{canPlan && <button className="button compact" onClick={() => setAdding(true)}><Plus aria-hidden="true"/>Add issues</button>}{canManage && cycle.state !== 'closed' && <button className="button compact" disabled={busy} onClick={() => void command('snapshots')}>Capture snapshot</button>}</div></header>{scope.value && (scope.loading ? <p role="status">Updating cycle counts. Showing the last confirmed calculation.</p> : scope.error ? <p role="status">Cycle counts could not be refreshed. Showing the last confirmed calculation.</p> : null)}<ErrorBanner error={scope.error} retry={scope.retry}/>{scope.value ? <><ScopeSummary scope={scope.value}/><CurrentCycleIssues denied={setDenied} key={cycle.id} workspace={workspaceId} team={team.id} path={path} refresh={refresh + revision} remove={canPlan ? issue => void remove(issue) : undefined} busy={busy}/></> : !scope.error && <Loading label="Loading cycle scope…"/>}</section>
    <CycleHistory denied={setDenied} path={path} refresh={refresh + revision}/>
    {editing && <CycleEditor workspace={workspaceId} team={team.id} session={session} cycle={cycle} close={() => setEditing(false)} saved={value => { changed(value); setEditing(false); }} denied={setDenied}/>}
    {adding && <AddCycleIssue cycle={cycle} session={session} included={scope.value?.items.map(item => item.issue_id) ?? []} close={() => setAdding(false)} changed={changed} denied={setDenied}/>}
    {closing && <CloseCycle cycle={cycle} session={session} close={() => setClosing(false)} closed={value => { setReport(value); changed(value.cycle); setClosing(false); }} denied={setDenied}/>}
    {viewingReport && report && <Modal title="Cycle closure report" close={() => setViewingReport(false)}><div className="project-form"><p>Closed {timestamp(report.closed_at)}. Counts, categories and estimates reflect that reviewed closure; issue labels use current access.</p><ScopeSummary scope={report.scope}/><ScopeTable workspace={workspaceId} team={team.id} scope={report.scope} caption="Original cycle closure scope"/><div className="modal-footer"><button className="button" onClick={() => setViewingReport(false)}>Done</button></div></div></Modal>}
    {carrying && report && <CarryForward readOnly={!canManage} key={carryOperation?.id ?? 'new'} initialOperation={carryOperation ?? undefined} operationSaved={operation => { setCarryOperation(operation); navigate(`${cycleUrl(workspaceId, team.id, cycleId)}&carry=${encodeURIComponent(operation.id)}`, true); }} cycle={cycle} report={report} session={session} close={() => { setCarrying(false); setCarryOperation(null); navigate(cycleUrl(workspaceId, team.id, cycleId), true); setRevision(value => value + 1); }} changed={() => setRevision(value => value + 1)} denied={setDenied}/>}
    {reopening && <Modal title="Reopen cycle" close={() => setReopening(false)} canClose={!busy}><div className="project-form"><p>Reopening permits explicit corrections. Original closure reports remain unchanged.</p><label className="field">Reason<textarea data-autofocus value={reason} onChange={event => setReason(event.target.value)} disabled={busy}/></label><ErrorBanner error={error}/><div className="modal-footer"><button className="button" disabled={busy} onClick={() => setReopening(false)}>Cancel</button><button className="button primary" disabled={busy || !reason.trim()} onClick={() => void command('reopen')}>Reopen cycle</button></div></div></Modal>}
  </>;
}
