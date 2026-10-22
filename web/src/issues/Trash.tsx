import { useEffect, useRef, useState, type FormEvent } from 'react';
import { Search, Trash2 } from 'lucide-react';
import { ApiError } from '../api';
import { ErrorBanner, Loading, Modal } from '../components';
import { useSessionBoundary, useSessionScope } from '../desktop/context';
import { formatDate, rebaseDraft, toDraft } from '../issue-model';
import { issuesUrl, Link, navigate, trashUrl } from '../router';
import { useScopedRequest } from '../settings/shared';
import type { Bootstrap, Issue, Session } from '../types';
import { selectable } from '../workflows/model';
import { useIssueWorkflow } from '../workflows/useIssueWorkflow';
import type { Workflow } from '../workflows/types';
import type { DeletedIssue, DeletedIssuePage } from './deletion-types';
import { IssueFields } from './IssueFields';
import { uncertainMutation } from './IssueDelete';
import './deletion.css';

const expired = (item: DeletedIssue) => Date.parse(item.restore_until) <= Date.now();
const deadline = (item: DeletedIssue) => new Date(item.restore_until).toLocaleString(undefined, { dateStyle: 'medium', timeStyle: 'short' });

export function Trash({ workspaceId, data, session, params, selectedId, refresh, restored, workflowChanged }: { workspaceId: string; data: Bootstrap; session: Session; params: URLSearchParams; selectedId?: string; refresh: number; restored: (issue: Issue) => void; workflowChanged?: (workflow: Workflow) => void }) {
  const request = useScopedRequest(); const authScope = useSessionScope(session); const boundary = useSessionBoundary(session);
  const [result, setResult] = useState<{ scope: string; page: DeletedIssuePage } | null>(null);
  const [loading, setLoading] = useState<{ scope: string; busy: boolean; error: unknown }>({ scope: '', busy: true, error: null });
  const [moreBusy, setMoreBusy] = useState(false); const [retry, setRetry] = useState(0); const [query, setQuery] = useState(params.get('q') ?? '');
  const sequence = useRef(0);
  const filters = new URLSearchParams({ limit: '50' });
  for (const key of ['team_id', 'q']) if (params.get(key)) filters.set(key, params.get(key)!);
  const filterKey = filters.toString(); const scope = JSON.stringify([workspaceId, authScope, filterKey]);
  const snapshot = result?.scope === scope ? result.page : null;
  const busy = loading.scope !== scope || loading.busy; const error = loading.scope === scope ? loading.error : null;
  function setFilter(key: string, value: string) { const next = new URLSearchParams(params); if (value) next.set(key, value); else next.delete(key); navigate(trashUrl(workspaceId, next)); }
  useEffect(() => { setQuery(params.get('q') ?? ''); }, [params.get('q')]);
  useEffect(() => {
    // Opening a row must cancel pending list navigation and preserve its repair draft.
    if (selectedId || query === (params.get('q') ?? '')) return;
    const timer = setTimeout(() => setFilter('q', query.trim()), 280); return () => clearTimeout(timer);
  }, [query, params.toString(), selectedId]);
  useEffect(() => {
    const abort = new AbortController(); const generation = ++sequence.current; setLoading({ scope, busy: true, error: null }); setMoreBusy(false);
    request<DeletedIssuePage>(`/workspaces/${workspaceId}/issues/deleted?${filterKey}`, { signal: abort.signal }).then(page => {
      if (!abort.signal.aborted && sequence.current === generation) { setResult({ scope, page }); setLoading({ scope, busy: false, error: null }); }
    }).catch(error => {
      if (!abort.signal.aborted && sequence.current === generation) { if (error instanceof ApiError && [401, 403, 404].includes(error.status)) setResult(null); setLoading({ scope, busy: false, error }); }
    });
    return () => abort.abort();
  }, [scope, refresh, retry]);
  async function more() {
    if (busy || moreBusy || !snapshot?.next_cursor) return;
    const generation = sequence.current; const next = new URLSearchParams(filterKey); next.set('cursor', snapshot.next_cursor); setMoreBusy(true);
    try {
      const page = await request<DeletedIssuePage>(`/workspaces/${workspaceId}/issues/deleted?${next}`);
      if (generation === sequence.current) setResult(previous => previous?.scope === scope ? { scope, page: { ...page, data: [...previous.page.data, ...page.data.filter(item => !previous.page.data.some(old => old.issue.id === item.issue.id))] } } : previous);
    } catch (error) { if (generation === sequence.current) { if (error instanceof ApiError && [401, 403, 404].includes(error.status)) setResult(null); setLoading({ scope, busy: false, error }); } }
    finally { if (generation === sequence.current) setMoreBusy(false); }
  }
  return <section className="trash-screen" aria-label="Trash" aria-busy={busy}>
    <p className="trash-intro">Deleted issues can be restored for 30 days. Restoring does not reconnect their parent, subissues or relationships.</p>
    <div className="trash-toolbar"><label className="search-field"><Search aria-hidden="true" /><input type="search" aria-label="Search Trash by title or key" placeholder="Search Trash…" maxLength={200} value={query} onChange={event => setQuery(event.target.value)} /></label><select aria-label="Trash team" value={params.get('team_id') ?? ''} onChange={event => setFilter('team_id', event.target.value)}><option value="">All teams</option>{data.teams.map(team => <option key={team.id} value={team.id}>{team.name}{team.archived_at ? ' · Archived' : ''}</option>)}</select></div>
    <ErrorBanner error={error} retry={() => setRetry(value => value + 1)} />
    {busy && !snapshot ? <Loading label="Loading Trash…" /> : !snapshot?.data.length && !error ? <div className="empty-state trash-empty"><Trash2 aria-hidden="true" /><h2>{params.get('q') || params.get('team_id') ? 'No deleted issues match.' : 'Trash is empty.'}</h2><p>{params.get('q') || params.get('team_id') ? 'Try a different title, key or team.' : 'Issues you delete will appear here. Archived issues remain in the issue list’s archive view.'}</p><Link className="button" href={issuesUrl(workspaceId)}>Back to issues</Link></div> : <div className="trash-rows">
      <div className="list-caption"><span>{snapshot?.data.length ?? 0}{snapshot?.has_more ? '+' : ''} deleted issue{snapshot?.data.length === 1 ? '' : 's'}</span>{busy && snapshot && <span role="status">Updating…</span>}</div>
      {snapshot?.data.map(item => <Link key={item.issue.id} href={trashUrl(workspaceId, params, item.issue.id)} className="trash-row"><span className="issue-key" title={item.issue.key}>{item.issue.key}</span><span className="trash-row-title"><strong>{item.issue.title}</strong><small>{data.teams.find(team => team.id === item.issue.team_id)?.name ?? 'Team'} · Deleted {formatDate(item.deleted_at)} · {expired(item) ? 'Restore period ended' : `Restore until ${formatDate(item.restore_until)}`}</small></span><span className="trash-row-action">{item.can_restore && !expired(item) ? 'Review restore' : 'View details'}</span></Link>)}
      {snapshot?.has_more && <div className="load-more"><button type="button" className="button" disabled={busy || moreBusy} onClick={more}>{moreBusy ? 'Loading…' : 'Load more'}</button></div>}
    </div>}
    {selectedId && <DeletedIssueDetail key={`${workspaceId}:${selectedId}:${boundary}`} workspaceId={workspaceId} issueId={selectedId} session={session} data={data} workflowChanged={workflowChanged} close={() => navigate(trashUrl(workspaceId, params))} restored={issue => { restored(issue); navigate(issuesUrl(workspaceId, new URLSearchParams(), issue.id)); }} />}
  </section>;
}

function DeletedIssueDetail({ workspaceId, issueId, session, data, workflowChanged, close, restored }: { workspaceId: string; issueId: string; session: Session; data: Bootstrap; workflowChanged?: (workflow: Workflow) => void; close: () => void; restored: (issue: Issue) => void }) {
  const request = useScopedRequest(); const authScope = useSessionScope(session); const [verifying, setVerifying] = useState(true); const [item, setItem] = useState<DeletedIssue | null>(null); const [error, setError] = useState<unknown>(null); const [retry, setRetry] = useState(0);
  useEffect(() => {
    const abort = new AbortController(); setVerifying(true); setError(null);
    request<DeletedIssue>(`/workspaces/${workspaceId}/issues/${issueId}/deleted`, { signal: abort.signal }).then(value => { if (!abort.signal.aborted) setItem(value); }).catch(error => { if (!abort.signal.aborted) { setError(error); if (error instanceof ApiError && [401, 403, 404].includes(error.status)) setItem(null); } }).finally(() => { if (!abort.signal.aborted) setVerifying(false); });
    return () => abort.abort();
  }, [workspaceId, issueId, retry, authScope]);
  return item ? <RestoreDeletedIssue item={item} verifying={verifying || !!error} verificationError={error} verify={() => setRetry(value => value + 1)} session={session} data={data} workflowChanged={workflowChanged} close={close} restored={restored} /> : <Modal title="Deleted issue" close={close}><div className="settings-modal-content"><ErrorBanner error={error} retry={() => setRetry(value => value + 1)} />{!error && <Loading label="Opening deleted issue…" />}<div className="modal-footer"><button type="button" className="button" data-autofocus onClick={close}>Close</button></div></div></Modal>;
}

export function RestoreDeletedIssue({ item, session, data, close, restored, workflowChanged, verifying = false, verificationError, verify }: { verifying?: boolean; verificationError?: unknown; verify?: () => void; item: DeletedIssue; session: Session; data: Bootstrap; close: () => void; restored: (issue: Issue) => void; workflowChanged?: (workflow: Workflow) => void }) {
  const request = useScopedRequest(); const [base, setBase] = useState(item); const [draft, setDraft] = useState(toDraft(item.issue)); const [latest, setLatest] = useState<DeletedIssue | null>(null);
  const [hidden, setHidden] = useState(false); const [busy, setBusy] = useState(false); const [error, setError] = useState<unknown>(null); const [uncertain, setUncertain] = useState(false); const [needsReview, setNeedsReview] = useState(false); const [chosenAssignee, setChosenAssignee] = useState<string | null>(null);
  const workflow = useIssueWorkflow(base.issue.workspace_id, base.issue.team_id, session, data, draft.status_id, workflowChanged);
  const unavailable = workflow.error instanceof ApiError && [401, 403, 404].includes(workflow.error.status);
  const validStatus = workflow.statuses.some(status => status.id === draft.status_id && workflow.currentIds.has(status.id) && selectable(status));
  const validAssignee = !draft.assignee_id || draft.assignee_id !== base.issue.assignee_id || !!base.issue.assignee?.assignable || chosenAssignee === draft.assignee_id;
  const canRestore = base.can_restore && !expired(base);
  const path = `/workspaces/${base.issue.workspace_id}/issues/${base.issue.id}`;
  useEffect(() => {
    // Reauthentication may refresh permissions/references without remounting the repair draft.
    if (item.issue.etag === base.issue.etag) setBase(item);
    else setLatest(item);
  }, [item]);
  async function reviewLatest() {
    if (busy) return; setBusy(true); setError(null);
    try {
      if (uncertain) {
        try { const issue = await request<Issue>(path); restored(issue); return; }
        catch (error) { if (!(error instanceof ApiError && error.status === 404)) throw error; }
      }
      setLatest(await request<DeletedIssue>(`${path}/deleted`)); setHidden(false); setUncertain(false); setNeedsReview(false);
    } catch (error) { setError(error); if (error instanceof ApiError && [403, 404].includes(error.status)) setHidden(true); }
    finally { setBusy(false); }
  }
  async function submit(event: FormEvent) {
    event.preventDefault(); if (busy || verifying || hidden || latest || uncertain || needsReview || unavailable || !canRestore || !validStatus || !validAssignee) return;
    setBusy(true); setError(null);
    try { restored(await request<Issue>(`${path}/restore`, { method: 'POST', body: { status_id: draft.status_id, assignee_id: draft.assignee_id, label_ids: draft.label_ids }, csrf: session.csrf_token, headers: { 'If-Match': base.issue.etag } })); }
    catch (error) {
      setError(error); if (error instanceof ApiError && [403, 404].includes(error.status)) setHidden(true); setUncertain(uncertainMutation(error));
      if (error instanceof ApiError && [403, 404, 409, 412].includes(error.status)) {
        setNeedsReview(true);
        if (error.status === 412) {
          try { setLatest(await request<DeletedIssue>(`${path}/deleted`)); setNeedsReview(false); }
          catch (loadError) { setError(loadError); if (loadError instanceof ApiError && [403, 404].includes(loadError.status)) setHidden(true); }
        }
      }
    } finally { setBusy(false); }
  }
  function adopt(keep: boolean) {
    if (!latest) return;
    setDraft(keep ? rebaseDraft(base.issue, draft, latest.issue) : toDraft(latest.issue)); setBase(latest); setLatest(null); setChosenAssignee(null); setError(null);
  }
  return <Modal title={hidden ? 'Issue unavailable' : `${canRestore ? 'Restore' : 'Deleted issue'} ${base.issue.key}`} close={close} canClose={!busy}><form className="settings-modal-content trash-restore restore-repair" onSubmit={submit}>
    {hidden ? <p>This issue is no longer available. Its details have been hidden.</p> : <><div className="deletion-subject"><strong>{base.issue.title}</strong>{base.issue.description && <p>{base.issue.description}</p>}</div>
    <p>Deleted {new Date(base.deleted_at).toLocaleString()}. {expired(base) ? 'The 30-day restore period has ended.' : `Restore is available until ${deadline(base)} for people with the required access.`}</p>
    <p className="reference-note">Parent, subissue and relationship links removed by deletion will stay disconnected. Review the status, assignee and labels below. Existing archived labels may remain.</p></>}
    <ErrorBanner error={error} /><ErrorBanner error={verificationError} retry={verify} /><ErrorBanner error={workflow.error} retry={workflow.refresh} />
    {!canRestore && !expired(base) && <p className="notice">You cannot restore this issue with its current permissions or while its team or workspace is archived.</p>}
    {uncertain && <p className="notice" role="status">The response was interrupted. Your repairs are preserved. Check whether the issue was restored before sending another request.</p>}
    {needsReview && !uncertain && <p className="notice">This issue or your access changed. Review the latest deleted issue before continuing.</p>}
    {!hidden && latest && <section className="workflow-conflict" role="alert"><strong>This issue changed.</strong><p>Your proposed repairs are preserved. The latest issue is “{latest.issue.title}”. Review its references before restoring.</p><div className="button-row"><button type="button" className="button compact" onClick={() => adopt(false)}>Use latest version</button><button type="button" className="button compact" onClick={() => adopt(true)}>Keep repairs for review</button></div></section>}
    {!hidden && !unavailable && <IssueFields workspaceId={base.issue.workspace_id} session={session} draft={draft} change={setDraft} data={data} teamId={base.issue.team_id} issue={base.issue} statuses={workflow.statuses} currentStatusIds={workflow.currentIds} rememberedAssignee={person => setChosenAssignee(person.id)} repairOnly disabled={busy || verifying || uncertain || needsReview || !canRestore} />}
    {canRestore && !validStatus && <p className="reference-note">Choose an active status before restoring.</p>}{canRestore && !validAssignee && <p className="reference-note">Choose an eligible assignee or Unassigned before restoring.</p>}
    <div className="modal-footer"><button type="button" className="button" data-autofocus disabled={busy} onClick={close}>{canRestore ? 'Cancel' : 'Close'}</button>{uncertain || needsReview ? <button type="button" className="button" disabled={busy} onClick={reviewLatest}>{busy ? 'Checking…' : uncertain ? 'Check restore result' : 'Review latest issue'}</button> : canRestore && <button type="submit" className="button primary" disabled={busy || verifying || hidden || !!latest || unavailable || !validStatus || !validAssignee}>{busy ? 'Restoring…' : 'Restore issue'}</button>}</div>
  </form></Modal>;
}
