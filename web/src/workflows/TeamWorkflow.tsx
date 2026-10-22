import { useSessionScope } from '../desktop/context';
import { useRef, useState } from 'react';
import { Archive, ArchiveRestore, ArrowDown, ArrowUp, Check, Pencil, Plus, RefreshCw } from 'lucide-react';
import { ApiError } from '../api';
import { ErrorBanner, Loading, Modal, StatusMark } from '../components';
import { useUnsavedChanges } from '../router';
import type { Session } from '../types';
import type { TeamDetail } from '../settings/types';
import { DirectoryFooter, useDirectory, useScopedRequest } from '../settings/shared';
import { categoryNames, defaultStatus, moveStatus, ordered, rebaseOrder, selectable, workflowETag } from './model';
import { useWorkflow } from './useWorkflow';
import { StatusEditor } from './StatusEditor';
import { ReplaceStatus } from './ReplaceStatus';
import { ReplacementProgress } from './ReplacementProgress';
import type { Workflow, WorkflowStatus } from './types';
import './workflows.css';

interface Props { workspaceId: string; team: TeamDetail; session: Session; workspaceArchived?: boolean; changed?: (workflow: Workflow) => void }
export function TeamWorkflow({ workspaceId, team, session, workspaceArchived, changed }: Props) {
  const sessionScope = useSessionScope(session);
  const workflow = useWorkflow(workspaceId, team.id, sessionScope, changed);
  const request = useScopedRequest();
  const [editor, setEditor] = useState<'new' | WorkflowStatus | null>(null);
  const [replacement, setReplacement] = useState<WorkflowStatus | null>(null);
  const [archiving, setArchiving] = useState<{ status: WorkflowStatus; base: Workflow } | null>(null);
  const [order, setOrder] = useState<{ base: Workflow; ids: string[] } | null>(null);
  const [busy, setBusy] = useState(false); const [error, setError] = useState<unknown>(null); const [message, setMessage] = useState('');
  const [history, setHistory] = useState(false); const [historyRevision, setHistoryRevision] = useState(0);
  const heading = useRef<HTMLHeadingElement>(null);
  useUnsavedChanges(!!order);
  const data = workflow.data;
  const canManage = !!data?.can_manage && !workspaceArchived && !team.archived_at;
  const conflict = !!order && !!data && order.base.version !== data.version;
  const path = `/workspaces/${workspaceId}/teams/${team.id}`;
  async function mutate(suffix: string, method: 'POST' | 'PUT', body?: object, base = data) {
    if (!base || busy || !canManage) return false;
    setBusy(true); setError(null); setMessage('');
    try {
      const result = await request<Workflow>(`${path}${suffix}`, { method, body, csrf: session.csrf_token, headers: { 'If-Match': workflowETag(base) } });
      workflow.accept(result); setOrder(null); setHistoryRevision(value => value + 1); setMessage('Workflow saved.');
      return true;
    } catch (error) {
      setError(error);
      if (error instanceof ApiError && error.status === 412) { const current = error.details?.current as Workflow | undefined; if (current?.team_id === team.id) workflow.accept(current); else workflow.refresh(); }
      return false;
    } finally { setBusy(false); }
  }
  function saved(result: Workflow) { workflow.accept(result); setEditor(null); setMessage('Status saved.'); }
  function move(id: string, delta: -1 | 1) {
    if (!data || busy || !canManage) return;
    const base = order?.base ?? data; const ids = order?.ids ?? ordered(data.statuses).map(status => status.id);
    const next = moveStatus(ids, base.statuses, id, delta); if (next === ids) return;
    setOrder({ base, ids: next });
    const status = base.statuses.find(status => status.id === id)!;
    const members = next.filter(value => base.statuses.find(item => item.id === value)?.category === status.category);
    setMessage(`${status.name} moved to position ${members.indexOf(id) + 1} in ${categoryNames[status.category]}. Save order to apply.`);
    requestAnimationFrame(() => { const row = document.getElementById(`workflow-row-${id}`); (row?.querySelector<HTMLButtonElement>(`[data-move="${delta}"]:not(:disabled)`) ?? row)?.focus(); });
  }
  if (!data) return <section className="team-workflow"><h3>Workflow</h3><ErrorBanner error={workflow.error} retry={workflow.refresh} />{workflow.loading && <Loading label="Loading workflow…" />}</section>;
  const statuses = order ? order.ids.map(id => order.base.statuses.find(status => status.id === id)!).filter(Boolean) : ordered(data.statuses);
  const currentDefault = defaultStatus(data.statuses);
  const jobs = data.statuses.filter(status => status.replacement_job_id && status.retiring_at);
  const archiveCurrent = archiving && data.statuses.find(status => status.id === archiving.status.id);
  const archiveAvailable = !!archiveCurrent && selectable(archiveCurrent) && !archiveCurrent.is_default && canManage;
  const archiveChanged = !!archiving && workflowETag(archiving.base) !== workflowETag(data);
  const mutationLocked = busy || !!order || !!editor || !!replacement || !!archiving || !canManage;
  return <section className="team-workflow" aria-label={`${team.name} workflow`}>
    <header className="workflow-heading"><div><h3 ref={heading} tabIndex={-1}>Workflow</h3><p>New issues start in <strong>{currentDefault?.name ?? 'an unavailable default'}</strong>.</p></div><div className="button-row"><button className="icon-button" aria-label="Refresh workflow" onClick={workflow.refresh}><RefreshCw aria-hidden="true" /></button>{canManage && <button className="button compact" disabled={mutationLocked || data.statuses.length >= 100} onClick={() => setEditor('new')}><Plus aria-hidden="true" />Add status</button>}</div></header>
    {!canManage && <div className="settings-info">{team.archived_at || workspaceArchived ? 'Restore the team and workspace before changing this workflow.' : 'A team maintainer or an administrator with team access can change this workflow.'}</div>}
    {data.statuses.length >= 100 && <p className="muted">This workflow has reached its limit of 100 current statuses. Archive an unused status before adding another.</p>}
    <ErrorBanner error={workflow.error} retry={workflow.refresh} />{!archiving && <ErrorBanner error={error} />}
    {conflict && <div className="workflow-conflict" role="alert"><strong>The workflow changed while you reordered it.</strong><p>Your order is preserved below. Review it against the latest workflow before saving.</p><div className="button-row"><button className="button compact" disabled={busy} onClick={() => { setOrder(null); setError(null); }}>Use latest order</button><button className="button compact" disabled={busy} onClick={() => { setOrder({ base: data, ids: rebaseOrder(order!.ids, data.statuses) }); setError(null); setMessage('Your order is applied to the latest statuses for review.'); }}>Keep my order for review</button></div></div>}
    {Object.keys(categoryNames).map(category => {
      const group = statuses.filter(status => status.category === category);
      return <section className="workflow-category" key={category} aria-label={categoryNames[category]}><h4>{categoryNames[category]}<span>{group.length}</span></h4>{!group.length ? <p className="workflow-empty-category">No statuses</p> : <ol>{group.map((status, index) => <li className="workflow-row" id={`workflow-row-${status.id}`} tabIndex={-1} key={status.id}>
        <StatusMark status={status} /><span className="workflow-status-name"><strong>{status.name}</strong>{status.is_default && <small className="workflow-badge">Default</small>}{status.retiring_at && <small className="workflow-badge">Being replaced</small>}</span>
        {canManage && <div className="workflow-actions"><button className="icon-button" data-move="-1" aria-label={`Move ${status.name} up`} disabled={busy || index === 0 || !!editor || !!replacement || !!archiving} onClick={() => move(status.id, -1)}><ArrowUp aria-hidden="true" /></button><button className="icon-button" data-move="1" aria-label={`Move ${status.name} down`} disabled={busy || index === group.length - 1 || !!editor || !!replacement || !!archiving} onClick={() => move(status.id, 1)}><ArrowDown aria-hidden="true" /></button><button className="icon-button" aria-label={`Edit ${status.name}`} disabled={mutationLocked || !!status.retiring_at} onClick={() => setEditor(status)}><Pencil aria-hidden="true" /></button><details className="workflow-row-menu"><summary aria-label={`Actions for ${status.name}`} aria-disabled={mutationLocked || !!status.retiring_at}>•••</summary><div><button disabled={mutationLocked || !selectable(status) || status.is_default} onClick={() => void mutate('/workflow/default', 'PUT', { status_id: status.id })}><Check aria-hidden="true" />Make default</button><button disabled={mutationLocked || !selectable(status)} onClick={() => setReplacement(status)}>Replace status…</button><button disabled={mutationLocked || !selectable(status) || status.is_default} onClick={() => { setError(null); setArchiving({ status, base: data }); }}><Archive aria-hidden="true" />Archive</button></div></details></div>}
      </li>)}</ol>}</section>;
    })}
    <div className="workflow-save"><span role="status">{message}</span>{order && <div className="button-row"><button className="button" disabled={busy} onClick={() => { setOrder(null); setError(null); setMessage('Order changes discarded.'); }}>Discard order</button><button className="button primary" disabled={busy || conflict || !canManage} onClick={() => void mutate('/workflow/order', 'PUT', { status_ids: order.ids }, order.base)}>{busy ? 'Saving…' : 'Save order'}</button></div>}</div>
    {jobs.map(status => <ReplacementProgress key={status.replacement_job_id} workspaceId={workspaceId} teamId={team.id} jobId={status.replacement_job_id!} source={status} workflow={data} session={session} canManage={canManage && !order} changed={completed => { if (completed) setMessage(`Replacement of ${status.name} completed.`); workflow.refresh(); }} />)}
    <button className="quiet workflow-history-toggle" aria-expanded={history} onClick={() => setHistory(value => !value)}><Archive aria-hidden="true" />Archived statuses</button>
    {history && <ArchivedStatuses key={historyRevision} workspaceId={workspaceId} teamId={team.id} canManage={!mutationLocked} workflow={data} session={session} restore={id => void mutate(`/statuses/${id}/restore`, 'POST')} />}
    {archiving && <Modal title="Archive status" close={() => { setArchiving(null); setError(null); }} canClose={!busy}><div className="settings-modal-content">
      <p>Archive <strong>{archiving.status.name}</strong> from {team.name}? You can restore it from Archived statuses.</p>
      <p className="muted">A status used by issues must be replaced first. No issues will be moved by this action.</p>
      <ErrorBanner error={error} />
      {!archiveAvailable ? <p className="settings-info">This status can no longer be archived. It may be the default, be undergoing replacement, or no longer be available to you.</p> : archiveChanged && <div className="workflow-conflict" role="alert"><strong>The workflow changed.</strong><p>The latest name is “{archiveCurrent.name}”. Review the current status before archiving it.</p><button type="button" className="button compact" disabled={busy} onClick={() => { setArchiving({ status: archiveCurrent, base: data }); setError(null); }}>Review latest status</button></div>}
      <div className="modal-footer"><button type="button" className="button" data-autofocus disabled={busy} onClick={() => { setArchiving(null); setError(null); }}>Cancel</button><button type="button" className="button primary" disabled={busy || !archiveAvailable || archiveChanged} onClick={async () => { if (await mutate(`/statuses/${archiving.status.id}/archive`, 'POST', undefined, archiving.base)) { setMessage(`${archiving.status.name} archived. You can restore it from Archived statuses.`); setArchiving(null); requestAnimationFrame(() => heading.current?.focus()); } }}>{busy ? 'Archiving…' : 'Archive'}</button></div>
    </div></Modal>}
    {editor && <StatusEditor workspaceId={workspaceId} workflow={data} status={editor === 'new' ? undefined : editor} session={session} canManage={canManage} close={() => { setEditor(null); }} saved={saved} accept={workflow.accept} />}
    {replacement && <ReplaceStatus workspaceId={workspaceId} workflow={data} source={replacement} session={session} canManage={canManage} close={() => setReplacement(null)} accepted={() => { setReplacement(null); workflow.refresh(); setMessage('Replacement queued. Progress is saved when you leave this page.'); requestAnimationFrame(() => heading.current?.focus()); }} accept={workflow.accept} />}
  </section>;
}

function ArchivedStatuses({ workspaceId, teamId, canManage, restore, workflow, session }: { workspaceId: string; teamId: string; canManage: boolean; restore: (id: string) => void; workflow: Workflow; session: Session }) {
  const [selected, setSelected] = useState<WorkflowStatus | null>(null);
  const directory = useDirectory<WorkflowStatus>(`/workspaces/${workspaceId}/teams/${teamId}/statuses?state=archived`);
  return <section className="workflow-history" aria-label="Archived statuses"><ErrorBanner error={directory.error} retry={directory.reload} />{directory.loading && !directory.items.length ? <Loading label="Loading archived statuses…" /> : <>{!directory.items.length && !directory.error && <p className="muted">No archived statuses.</p>}{directory.items.map(status => <div className="workflow-row" key={status.id}><StatusMark status={status} /><span className="workflow-status-name"><strong>{status.name}</strong><small>{categoryNames[status.category]}</small></span>{status.replacement_job_id && <button className="quiet" onClick={() => setSelected(selected?.id === status.id ? null : status)}>{selected?.id === status.id ? 'Hide replacement' : 'View replacement'}</button>}{canManage && <button className="button compact" onClick={() => restore(status.id)}><ArchiveRestore aria-hidden="true" />Restore</button>}</div>)}<DirectoryFooter directory={directory} />{selected?.replacement_job_id && <ReplacementProgress key={selected.replacement_job_id} workspaceId={workspaceId} teamId={teamId} jobId={selected.replacement_job_id} source={selected} workflow={workflow} session={session} canManage={false} changed={() => {}} />}</>}</section>;
}
