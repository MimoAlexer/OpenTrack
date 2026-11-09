import { useEffect, useRef, useState, type FormEvent } from 'react';
import { ApiError, retryWindowExpired } from '../api';
import { ErrorBanner, Loading } from '../components';
import { useNativeRequestRevision, useSessionBoundary } from '../desktop/context';
import { isAccessError } from '../documents/model';
import { Link, useUnsavedChanges } from '../router';
import { useScopedRequest } from '../settings/shared';
import type { Session } from '../types';
import { projectPath, type Project } from './types';
import './ProjectScopeHistory.css';

export interface ScopeIssue { id: string; key: string; title: string; reason?: string }
export interface ScopeBaseline { id: string; label: string; reason: string; author_id: string | null; author_name: string | null; created_at: string }
export interface ScopeEvent { id: string; action: 'added' | 'removed' | 'postponed'; reason: string; issue: ScopeIssue; actor_id: string | null; actor_name: string | null; created_at: string }
export interface ProjectScopeSnapshot {
  scope_etag: string;
  history_started_at: string;
  legacy_history_unknown: boolean;
  can_agree: boolean;
  capture_limit: number;
  items_has_more: boolean;
  items_next_cursor: string | null;
  baselines_has_more: boolean;
  baselines_next_cursor: string | null;
  current: ScopeIssue[];
  baselines: ScopeBaseline[];
  baseline: ScopeBaseline | null;
  comparison: { unchanged: number; added: ScopeIssue[]; removed: ScopeIssue[]; postponed: ScopeIssue[] } | null;
  events: { data: ScopeEvent[]; has_more: boolean; next_cursor: string | null };
}
interface Props { project: Project; session: Session; refresh: number; readOnly: boolean; denied: (error: unknown) => void; changed?: () => void }
interface PendingWrite { kind: 'baseline' | 'change'; path: string; body: Record<string, string>; key: string; etag: string }
type MoreKind = 'events' | 'items' | 'baselines';
const when = (value: string) => new Date(value).toLocaleString(undefined, { dateStyle: 'medium', timeStyle: 'short' });
const uncertainError = (error: unknown) => error instanceof ApiError && (error.status === 0 || error.status === 408 || error.status >= 500);
const withinLimit = (value: string, maximum: number) => value.trim().length > 0 && Array.from(value.trim()).length <= maximum;
const mergeById = <T extends { id: string }>(before: T[], next: T[]) => [...before, ...next.filter(item => !before.some(existing => existing.id === item.id))];

export function ProjectScopeHistory(props: Props) {
  const boundary = useSessionBoundary(props.session);
  return <ScopeHistory key={`${boundary}:${props.project.workspace_id}:${props.project.team_id}:${props.project.id}`} {...props}/>;
}

function ScopeHistory({ project, session, refresh, readOnly, denied, changed }: Props) {
  const request = useScopedRequest();
  const nativeRevision = useNativeRequestRevision();
  const base = projectPath(project.workspace_id, project.team_id, project.id);
  const [selected, setSelected] = useState('');
  const viewKey = `${base}:${selected}`;
  const [snapshot, setSnapshot] = useState<{ key: string; value: ProjectScopeSnapshot } | null>(null);
  const value = snapshot?.key === viewKey ? snapshot.value : null;
  const knownBaselines = snapshot?.value.baselines ?? [];
  const knownBaseline = snapshot?.value.baseline;
  const baselineOptions = knownBaseline && !knownBaselines.some(baseline => baseline.id === knownBaseline.id) ? [knownBaseline, ...knownBaselines] : knownBaselines;
  const [loading, setLoading] = useState(true);
  const [loadingMore, setLoadingMore] = useState<MoreKind | null>(null);
  const [readError, setReadError] = useState<unknown>(null);
  const [writeError, setWriteError] = useState<unknown>(null);
  const [accessError, setAccessError] = useState<unknown>(null);
  const [attempt, setAttempt] = useState(0);
  const [busy, setBusy] = useState(false);
  const [uncertain, setUncertain] = useState(false);
  const [expired, setExpired] = useState<PendingWrite['kind'] | null>(null);
  const [review, setReview] = useState(false);
  const [notice, setNotice] = useState('');
  const [label, setLabel] = useState('');
  const [baselineReason, setBaselineReason] = useState('');
  const [agreed, setAgreed] = useState(false);
  const [issueId, setIssueId] = useState('');
  const [action, setAction] = useState<'removed' | 'postponed'>('postponed');
  const [changeReason, setChangeReason] = useState('');
  const pending = useRef<PendingWrite | null>(null);
  const mounted = useRef(true);
  const reading = useRef<AbortController | null>(null);
  const generation = useRef(0);
  const writing = useRef(false);
  const dirty = !!(label.trim() || baselineReason.trim() || changeReason.trim() || uncertain);
  useUnsavedChanges(dirty);
  useEffect(() => { mounted.current = true; return () => { mounted.current = false; reading.current?.abort(); }; }, []);

  function deny(error: unknown) {
    setAccessError(error); setSnapshot(null); setLabel(''); setBaselineReason(''); setChangeReason('');
    setAgreed(false); pending.current = null; setUncertain(false); denied(error);
  }
  function url(kind?: MoreKind, cursor?: string) {
    const cursorName = kind === 'items' ? 'items_cursor' : kind === 'baselines' ? 'baseline_cursor' : 'cursor';
    return `${base}/scope-history?${new URLSearchParams({ limit: '25', ...(selected ? { baseline_id: selected } : {}), ...(cursor ? { [cursorName]: cursor } : {}) })}`;
  }
  useEffect(() => {
    if (writing.current) return;
    const abort = new AbortController(); reading.current?.abort(); reading.current = abort;
    const token = ++generation.current;
    setLoading(true); setLoadingMore(null); setReadError(null);
    request<ProjectScopeSnapshot>(url(), { signal: abort.signal }).then(next => {
      if (!mounted.current || abort.signal.aborted || token !== generation.current) return;
      setSnapshot({ key: viewKey, value: next });
      // Agreement belongs to the reviewed scope, not the transport refresh.
      // Keep it only while the same view and authorized validator still hold.
      if (!value || value.scope_etag !== next.scope_etag || !next.can_agree) setAgreed(false);
      setIssueId(previous => next.current.some(issue => issue.id === previous) ? previous : '');
    }).catch(error => {
      if (!mounted.current || abort.signal.aborted || token !== generation.current) return;
      setReadError(error); if (isAccessError(error)) deny(error);
    }).finally(() => { if (mounted.current && !abort.signal.aborted && token === generation.current) setLoading(false); });
    return () => abort.abort();
  }, [viewKey, refresh, attempt, nativeRevision]);

  function reloadScope() {
    setSnapshot(null);
    setSelected('');
    setIssueId('');
    setAgreed(false);
    setAttempt(count => count + 1);
  }

  async function more(kind: MoreKind) {
    const cursor = kind === 'events' ? value?.events.next_cursor : kind === 'items' ? value?.items_next_cursor : value?.baselines_next_cursor;
    if (!value || !cursor || loading || loadingMore || busy || uncertain) return;
    const before = value; const key = viewKey;
    const abort = new AbortController(); reading.current?.abort(); reading.current = abort;
    const token = ++generation.current; setLoadingMore(kind); setReadError(null);
    try {
      const next = await request<ProjectScopeSnapshot>(url(kind, cursor), { signal: abort.signal });
      if (!mounted.current || abort.signal.aborted || token !== generation.current) return;
      if (next.scope_etag !== before.scope_etag || next.baseline?.id !== before.baseline?.id) {
        setNotice('The scope changed while loading history. The previous pages were cleared.');
        reloadScope();
        return;
      }
      const merged: ProjectScopeSnapshot = kind === 'events'
        ? { ...before, events: { ...next.events, data: mergeById(before.events.data, next.events.data) } }
        : kind === 'baselines'
          ? { ...before, baselines: mergeById(before.baselines, next.baselines), baselines_has_more: next.baselines_has_more, baselines_next_cursor: next.baselines_next_cursor }
          : { ...before, current: mergeById(before.current, next.current), items_has_more: next.items_has_more, items_next_cursor: next.items_next_cursor, comparison: before.comparison && next.comparison ? { unchanged: next.comparison.unchanged, added: mergeById(before.comparison.added, next.comparison.added), removed: mergeById(before.comparison.removed, next.comparison.removed), postponed: mergeById(before.comparison.postponed, next.comparison.postponed) } : before.comparison };
      setSnapshot({ key, value: merged });
    } catch (error) {
      if (!mounted.current || abort.signal.aborted || token !== generation.current) return;
      if (error instanceof ApiError && [410, 422].includes(error.status)) {
        setNotice('The saved page is no longer current. The previous pages were cleared.');
        reloadScope();
      } else {
        setReadError(error); if (isAccessError(error)) deny(error);
      }
    } finally { if (mounted.current && !abort.signal.aborted && token === generation.current) setLoadingMore(null); }
  }

  const canWrite = !readOnly && project.can_edit && !project.archived_at;
  const frozen = busy || uncertain || !!expired || review || loading || !!loadingMore || !!readError;
  async function write(operation: PendingWrite) {
    if (writing.current || !canWrite) return;
    writing.current = true; setBusy(true); setWriteError(null); setNotice('');
    reading.current?.abort(); generation.current += 1; setLoading(false); setLoadingMore(null);
    pending.current = operation;
    let committed = false;
    try {
      const next = await request<ProjectScopeSnapshot>(operation.path, { method: 'POST', csrf: session.csrf_token, headers: { 'If-Match': operation.etag, 'Idempotency-Key': operation.key }, body: operation.body });
      if (!mounted.current) return;
      const baseline = next.baseline?.id ?? '';
      setSelected(baseline); setSnapshot({ key: `${base}:${baseline}`, value: next });
      pending.current = null; setUncertain(false); setReview(false); setReadError(null);
      if (operation.kind === 'baseline') { setLabel(''); setBaselineReason(''); setAgreed(false); setNotice('Baseline set. Future scope changes will be compared with this plan.'); }
      else { setChangeReason(''); setIssueId(''); setNotice(operation.body.action === 'postponed' ? 'Issue postponed and its reason recorded.' : 'Issue removed and its reason recorded.'); }
      committed = true;
    } catch (error) {
      if (!mounted.current) return;
      setWriteError(error);
      if (isAccessError(error)) deny(error);
      else if (retryWindowExpired(error)) {
        pending.current = null;
        setUncertain(false);
        setExpired(operation.kind);
        reloadScope();
      } else if (uncertainError(error) || uncertain && error instanceof ApiError && error.status === 429) setUncertain(true);
      else {
        pending.current = null; setUncertain(false);
        if (error instanceof ApiError && [409, 412].includes(error.status)) {
          setReview(true); setAgreed(false); setAttempt(count => count + 1);
        }
      }
    } finally {
      writing.current = false;
      if (mounted.current) setBusy(false);
    }
    if (committed) changed?.();
  }
  function agreeBaseline(event: FormEvent) {
    event.preventDefault();
    if (!value?.can_agree || value.items_has_more || !canWrite || frozen || !agreed || !withinLimit(label, 160) || !withinLimit(baselineReason, 1000)) return;
    void write({ kind: 'baseline', path: `${base}/scope-baselines`, body: { label: label.trim(), reason: baselineReason.trim() }, key: crypto.randomUUID(), etag: value.scope_etag });
  }
  function changeScope(event: FormEvent) {
    event.preventDefault();
    if (!value || !canWrite || frozen || !value.current.some(issue => issue.id === issueId) || !withinLimit(changeReason, 1000)) return;
    void write({ kind: 'change', path: `${base}/scope-changes`, body: { issue_id: issueId, action, reason: changeReason.trim() }, key: crypto.randomUUID(), etag: value.scope_etag });
  }
  if (accessError) return <section className="project-scope-history" aria-label="Project scope history"><h3>Scope history unavailable</h3><ErrorBanner error={accessError}/></section>;
  return <section className="project-scope-history" aria-label="Project scope history">
    <header className="project-section-heading"><div><h3>Scope history</h3><p>Compare agreed work with what changed, and keep the reasons with the plan.</p></div><button className="button compact" type="button" disabled={busy || uncertain || loading || !!loadingMore} onClick={() => setAttempt(count => count + 1)}>Refresh scope</button></header>
    <p className="scope-visibility">Comparison shows currently accessible, non-deleted issues; earlier work may be unavailable. Deleted issues and inaccessible work are omitted from lists and counts.</p>
    <ErrorBanner error={readError} retry={() => setAttempt(count => count + 1)}/>
    {loading && !value && <Loading label="Loading scope history…"/>}
    {loading && value && <p role="status">Refreshing scope. Showing the last confirmed comparison.</p>}
    {!!readError && value && <p role="status">The scope could not be refreshed. Changes are unavailable until a fresh comparison is loaded.</p>}
    <ErrorBanner error={writeError}/>
    {uncertain && <div className="notice scope-recovery"><p>The response was interrupted. Retry the same request to confirm its outcome without recording a duplicate.</p><button className="button compact" type="button" disabled={busy || !canWrite} onClick={() => { if (pending.current) void write(pending.current); }}>{busy ? 'Confirming…' : 'Retry safely'}</button></div>}
    {expired && <div className="notice scope-recovery">
      <p>The retry window ended. Inspect the refreshed baselines and scope changes to check whether your attempt was recorded.</p>
      <button className="button compact" type="button" disabled={loading || !!readError || !value} onClick={() => {
        if (expired === 'baseline') { setLabel(''); setBaselineReason(''); setAgreed(false); }
        else { setChangeReason(''); setIssueId(''); }
        setExpired(null); setWriteError(null);
      }}>Close attempted draft</button>
    </div>}
    {review && <div className="notice scope-recovery"><p>The scope changed. Review the refreshed comparison before submitting this draft again.</p><button className="button compact" type="button" disabled={loading || !!readError || !value} onClick={() => { setReview(false); setWriteError(null); }}>I reviewed the refreshed scope</button></div>}
    <p role="status" className="project-save-notice">{notice}</p>
    {!!baselineOptions.length && <div className="scope-baseline-selector"><label className="field">Compare with baseline<select aria-label="Compare with baseline" name="scope-baseline" value={selected || value?.baseline?.id || ''} disabled={busy || uncertain || !!loadingMore} onChange={event => setSelected(event.target.value)}>{baselineOptions.map(baseline => <option key={baseline.id} value={baseline.id}>{baseline.label} · {when(baseline.created_at)}</option>)}</select></label></div>}
    {value && <>
      {value.baselines_has_more && <button className="button compact scope-more" type="button" disabled={busy || uncertain || loading || !!loadingMore} onClick={() => void more('baselines')}>{loadingMore === 'baselines' ? 'Loading…' : 'Load more baselines'}</button>}
      {value.items_has_more && <div className="notice scope-recovery"><p>This comparison is partly loaded. Load more items to review the remaining currently accessible work before agreeing a baseline.</p><button className="button compact" type="button" disabled={busy || uncertain || loading || !!loadingMore} onClick={() => void more('items')}>{loadingMore === 'items' ? 'Loading…' : 'Load more scope items'}</button></div>}
      {canWrite && value.can_agree && <p className="scope-visibility">A baseline records the entire current project scope, up to {value.capture_limit.toLocaleString()} issues.</p>}
      {value.legacy_history_unknown && <p className="notice">Earlier scope changes were not recorded. History starts on <time dateTime={value.history_started_at}>{when(value.history_started_at)}</time>; an agreed baseline records the current plan without inventing its past.</p>}
      {value.baseline && value.comparison ? <section className="scope-comparison" aria-label="Baseline comparison"><h4>{value.baseline.label}</h4><p>Recorded by {value.baseline.author_name ?? 'an unavailable member'} on <time dateTime={value.baseline.created_at}>{when(value.baseline.created_at)}</time>.</p><p className="scope-reason">{value.baseline.reason || 'No reason recorded.'}</p><p>{value.comparison.unchanged} currently accessible {value.comparison.unchanged === 1 ? 'issue remains' : 'issues remain'} in the plan.</p>{!value.items_has_more && !value.comparison.added.length && !value.comparison.removed.length && !value.comparison.postponed.length && <p>No changes among currently accessible issues.</p>}<div className="scope-comparison-columns"><ScopeList label="Added since agreement" issues={value.comparison.added} workspace={project.workspace_id} partial={value.items_has_more}/><ScopeList label="Removed since agreement" issues={value.comparison.removed} workspace={project.workspace_id} partial={value.items_has_more}/><ScopeList label="Postponed since agreement" issues={value.comparison.postponed} workspace={project.workspace_id} partial={value.items_has_more}/></div></section> : <div className="project-empty"><h4>No baseline yet</h4><p>Set a baseline of the current scope to compare future additions, removals and postponements.</p></div>}
      <details className="scope-current"><summary>Current accessible scope ({value.current.length}{value.items_has_more ? ' shown' : ''})</summary><ScopeList label="Current project issues" issues={value.current} workspace={project.workspace_id} partial={value.items_has_more}/></details>
      {canWrite && <div className="scope-forms">{value.can_agree && <form aria-label="Set scope baseline" className="scope-form" onSubmit={agreeBaseline}><h4>Set baseline</h4><p>This records the current project membership. It does not change issue states or replace earlier baselines.</p><label className="field">Baseline name<input name="scope-baseline-name" value={label} required disabled={frozen} aria-invalid={Array.from(label.trim()).length > 160 || undefined} onChange={event => setLabel(event.target.value)}/><small>Up to 160 characters.</small></label><label className="field">Baseline reason<textarea name="scope-baseline-reason" value={baselineReason} rows={3} required disabled={frozen} aria-invalid={Array.from(baselineReason.trim()).length > 1000 || undefined} onChange={event => setBaselineReason(event.target.value)}/><small>Up to 1,000 characters.</small></label><label className="scope-agreement"><input type="checkbox" checked={agreed} disabled={frozen || value.items_has_more} onChange={event => setAgreed(event.target.checked)}/>I agree the current project scope as a baseline.</label><button className="button primary compact" title="Project leads and team managers can set a baseline." disabled={frozen || value.items_has_more || !agreed || !withinLimit(label, 160) || !withinLimit(baselineReason, 1000)}>{busy && pending.current?.kind === 'baseline' ? 'Setting…' : 'Set baseline'}</button></form>}
        <form aria-label="Record scope change" className="scope-form" onSubmit={changeScope}><h4>Explain a scope change</h4><p>Postponing or removing an issue detaches it from this project and keeps its workflow state. Add work through the Issues tab.</p><label className="field">Project issue<select aria-label="Project issue" name="scope-issue" value={issueId} required disabled={frozen || !value.current.length} onChange={event => setIssueId(event.target.value)}><option value="">Choose an issue</option>{value.current.map(issue => <option key={issue.id} value={issue.id}>{issue.key} · {issue.title}</option>)}</select></label><label className="field">Change<select aria-label="Change" name="scope-action" value={action} disabled={frozen} onChange={event => setAction(event.target.value as 'removed' | 'postponed')}><option value="postponed">Postpone from this plan</option><option value="removed">Remove from this plan</option></select></label><label className="field">Change reason<textarea name="scope-change-reason" value={changeReason} rows={3} required disabled={frozen} aria-invalid={Array.from(changeReason.trim()).length > 1000 || undefined} onChange={event => setChangeReason(event.target.value)}/><small>Up to 1,000 characters.</small></label><button className="button compact" disabled={frozen || !issueId || !withinLimit(changeReason, 1000) || !value.current.some(issue => issue.id === issueId)}>{busy && pending.current?.kind === 'change' ? 'Recording…' : 'Record scope change'}</button></form></div>}
      <section className="scope-history-feed" aria-label="Recorded scope changes"><h4>Recorded changes</h4>{value.events.data.length ? <ol>{value.events.data.map(event => <li key={event.id}><header><strong>{event.actor_name ?? (event.actor_id ? 'Unavailable member' : 'System')}</strong><time dateTime={event.created_at}>{when(event.created_at)}</time></header><p><span className={`scope-action scope-action-${event.action}`}>{event.action === 'added' ? 'Added' : event.action === 'removed' ? 'Removed' : 'Postponed'}</span> <IssueLink issue={event.issue} workspace={project.workspace_id}/></p><p className="scope-reason">{event.reason || 'No reason recorded.'}</p></li>)}</ol> : <p>No recorded changes in this view.</p>}{value.events.has_more && <button className="button compact" type="button" disabled={busy || uncertain || loading || !!loadingMore} onClick={() => void more('events')}>{loadingMore === 'events' ? 'Loading…' : 'Load older scope changes'}</button>}</section>
    </>}
  </section>;
}

function IssueLink({ issue, workspace }: { issue: ScopeIssue; workspace: string }) {
  return <Link href={`/w/${encodeURIComponent(workspace)}/issues/${encodeURIComponent(issue.id)}`}><span>{issue.key}</span> {issue.title}</Link>;
}
function ScopeList({ label, issues, workspace, partial = false }: { label: string; issues: ScopeIssue[]; workspace: string; partial?: boolean }) {
  return <section className="scope-issue-list" aria-label={label}><h5>{label} ({issues.length}{partial ? ' shown' : ''})</h5>{issues.length ? <ul>{issues.map(issue => <li key={issue.id}><IssueLink issue={issue} workspace={workspace}/>{issue.reason && <p className="scope-reason">{issue.reason}</p>}</li>)}</ul> : <p>{partial ? 'None on the loaded pages.' : 'None among currently accessible issues.'}</p>}</section>;
}
