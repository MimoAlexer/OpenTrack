import { useRef, useState } from 'react';

import { ApiError } from '../api';

import { ErrorBanner, Loading, Modal } from '../components';

import { useSessionScope } from '../desktop/context';

import { Link, useUnsavedChanges } from '../router';
import { isAccessError } from '../documents/model';

import { useProjectData, useProjectPage } from '../projects/useProjectData';

import { useScopedRequest } from '../settings/shared';

import { useSearchPage } from '../taxonomy/useSearchPage';

import type { Issue, IssuePage, Session } from '../types';

import { IssueLink, ScopeSummary, ScopeTable } from './CycleScope';

import { cyclePath, cycleUrl, type CarryOperation, type ClosePreview, type CloseReport, type Cycle } from './types';



export function AddCycleIssue({ cycle, session, included, close, changed, denied }: { cycle: Cycle; session: Session; included: string[]; close: () => void; changed: (cycle: Cycle) => void; denied: (error: unknown) => void }) {

  const request = useScopedRequest(); const [query, setQuery] = useState(''); const [busy, setBusy] = useState(false); const [error, setError] = useState<unknown>(null);

  const results = useSearchPage<Issue, IssuePage>(`/workspaces/${cycle.workspace_id}/issues?team_id=${encodeURIComponent(cycle.team_id)}`, useSessionScope(session), query, true);

  async function add(issue: Issue) {

    if (busy) return; setBusy(true); setError(null);

    try { changed(await request<Cycle>(`${cyclePath(cycle.workspace_id, cycle.team_id, cycle.id)}/issues/${issue.id}`, { method: 'PUT', csrf: session.csrf_token, headers: { 'If-Match': cycle.etag }, body: { included: true, issue_etag: issue.etag } })); close(); }

    catch (error) { setError(error); if (isAccessError(error)) denied(error); }

    finally { setBusy(false); }

  }

  return <Modal title="Add an issue to this cycle" close={close} canClose={!busy}><div className="project-form"><p className="project-help">Choose an issue from this team. Remove it from any other cycle first. Issue status is preserved.</p><label className="field">Find an issue<input data-autofocus value={query} onChange={event => setQuery(event.target.value)} disabled={busy}/></label><ErrorBanner error={error}/>{error instanceof ApiError && error.status === 412 && <p className="notice">The cycle or issue changed. Close this picker and refresh before reviewing another assignment.</p>}<ErrorBanner error={results.error} retry={results.refresh}/>{results.loading && <Loading label="Finding issues…"/>}<div className="project-candidates">{results.items.map(issue => <button className="project-candidate" key={issue.id} disabled={busy || included.includes(issue.id) || error instanceof ApiError && error.status === 412} onClick={() => void add(issue)}><span className="project-issue-key">{issue.key}</span><span>{issue.title}</span><small>{included.includes(issue.id) ? 'In cycle' : 'Add'}</small></button>)}</div>{!results.loading && !results.items.length && <p className="project-help">No matching issues.</p>}{results.page?.has_more && <button className="button compact" disabled={busy || results.loading} onClick={results.more}>Load more results</button>}<div className="modal-footer"><button className="button" disabled={busy} onClick={close}>Done</button></div></div></Modal>;

}

export function CloseCycle({ cycle, session, close, closed, denied }: { cycle: Cycle; session: Session; close: () => void; closed: (report: CloseReport) => void; denied: (error: unknown) => void }) {

  const path = cyclePath(cycle.workspace_id, cycle.team_id, cycle.id); const preview = useProjectData<ClosePreview>(`${path}/close-preview`); const request = useScopedRequest();

  const [ack, setAck] = useState(false); const [busy, setBusy] = useState(false); const [error, setError] = useState<unknown>(null); const [uncertain, setUncertain] = useState(false); const sent = useRef<{ key: string; preview: ClosePreview } | null>(null);

  async function submit() {

    if (!preview.value || !ack || busy) return;

    if (!uncertain || !sent.current) sent.current = { key: crypto.randomUUID(), preview: preview.value };

    const pending = sent.current; setBusy(true); setError(null);

    try { closed(await request<CloseReport>(`${path}/close`, { method: 'POST', csrf: session.csrf_token, headers: { 'If-Match': pending.preview.cycle.etag, 'Idempotency-Key': pending.key }, body: { review_token: pending.preview.review_token } })); }

    catch (error) { setError(error); if (isAccessError(error)) denied(error); setUncertain(error instanceof ApiError && (error.status === 0 || error.status >= 500)); }

    finally { setBusy(false); }

  }

  const conflict = error instanceof ApiError && [409, 412].includes(error.status);
  useUnsavedChanges(uncertain);

  return <Modal title="Review cycle closure" close={close} canClose={!busy && !uncertain}><div className="project-form"><ErrorBanner error={preview.error} retry={preview.retry}/><ErrorBanner error={error}/>{!preview.value ? !preview.error && <Loading label="Loading close preview…"/> : <><ScopeSummary scope={preview.value.scope}/><ScopeTable workspace={cycle.workspace_id} team={cycle.team_id} scope={preview.value.scope} caption="Reviewed cycle closure scope"/><label className="planning-ack"><input type="checkbox" checked={ack} disabled={busy || uncertain} onChange={event => setAck(event.target.checked)}/>Close this cycle and retain every issue and status. Carry-forward is a separate explicit action.</label></>}{conflict && <div className="notice"><p>The scope changed after this review. Load a new preview and review it again.</p><button className="button" disabled={busy} onClick={() => { setAck(false); setError(null); sent.current = null; preview.retry(); }}>Reload close preview</button></div>}{uncertain && <p role="status">Closure may have committed. Retry the identical command to reconcile its report.</p>}<div className="modal-footer"><button className="button" disabled={busy || uncertain} onClick={close}>Cancel</button><button className="button primary" disabled={busy || !ack || !preview.value || conflict || preview.loading} onClick={() => void submit()}>{uncertain ? 'Retry safely' : 'Close cycle'}</button></div></div></Modal>;

}

export function CarryForward({ cycle, report, session, close, changed, denied, initialOperation, operationSaved }: { initialOperation?: CarryOperation; operationSaved?: (operation: CarryOperation) => void; cycle: Cycle; report: CloseReport; session: Session; close: () => void; changed: () => void; denied: (error: unknown) => void }) {

  const path = cyclePath(cycle.workspace_id, cycle.team_id, cycle.id); const request = useScopedRequest();

  const destinations = useProjectPage<Cycle>(cyclePath(cycle.workspace_id, cycle.team_id), 0, 50);

  const [destination, setDestination] = useState(''); const [selected, setSelected] = useState(new Set<string>()); const [operation, setOperation] = useState<CarryOperation | null>(initialOperation ?? null); const [busy, setBusy] = useState(false); const [error, setError] = useState<unknown>(null); const [uncertain, setUncertain] = useState(false);

  const sent = useRef<{ key: string; etag: string; body: object } | null>(null);

  const [review, setReview] = useState<{ target: Cycle; issues: Issue[] } | null>(null);

  const [resumeSelected, setResumeSelected] = useState(new Set<string>());

  const resumeSent = useRef<{ key: string; etag: string; body: object } | null>(null);

  const release = useUnsavedChanges(uncertain);
  const available = destinations.value?.data.filter(item => item.id !== cycle.id && item.state !== 'closed' && item.can_plan && item.start_date >= cycle.end_date_exclusive) ?? [];

  async function submit() {

    const target = available.find(item => item.id === destination); if (!target || !selected.size || busy) return;

    if (!uncertain || !sent.current) sent.current = { key: crypto.randomUUID(), etag: cycle.etag, body: { report_id: report.id, destination_cycle_id: target.id, destination_etag: target.etag, items: report.scope.items.filter(item => selected.has(item.issue_id)).map(({ issue_id, issue_version }) => ({ issue_id, issue_version })) } };

    const pending = sent.current; setBusy(true); setError(null);

    try { const result = await request<CarryOperation>(`${path}/carry-forward`, { method: 'POST', csrf: session.csrf_token, headers: { 'If-Match': pending.etag, 'Idempotency-Key': pending.key }, body: pending.body }); setOperation(result); setUncertain(false); release(); operationSaved?.(result); changed(); }

    catch (error) { setError(error); if (isAccessError(error)) denied(error); setUncertain(error instanceof ApiError && (error.status === 0 || error.status >= 500)); }

    finally { setBusy(false); }

  }

  async function reviewUnresolved() {

    if (!operation || busy) return; setBusy(true); setError(null);

    try {

      const current = await request<CarryOperation>(`${path}/carry-forward/${operation.id}`);

      const target = await request<Cycle>(cyclePath(cycle.workspace_id, cycle.team_id, current.destination_cycle_id));

      const ids = current.results.filter(result => result.outcome !== 'moved').map(result => result.issue_id).slice(0, 100);

      const issues: Issue[] = [];

      for (const id of ids) {

        try { const issue = await request<Issue>(`/workspaces/${cycle.workspace_id}/issues/${id}`); if (!issue.archived_at && issue.team_id === cycle.team_id) issues.push(issue); }

        catch (error) { if (!(error instanceof ApiError && error.status === 404)) throw error; }

      }

      setOperation(current); setReview({ target, issues }); setResumeSelected(new Set()); resumeSent.current = null;

    } catch (error) { setError(error); if (isAccessError(error)) denied(error); }

    finally { setBusy(false); }

  }

  async function resume() {

    if (!operation || !review || !resumeSelected.size || busy) return;

    if (!uncertain || !resumeSent.current) resumeSent.current = { key: crypto.randomUUID(), etag: operation.etag, body: { destination_etag: review.target.etag, items: review.issues.filter(issue => resumeSelected.has(issue.id)).map(issue => ({ issue_id: issue.id, issue_version: issue.version })) } };

    const pending = resumeSent.current; setBusy(true); setError(null);

    try { const result = await request<CarryOperation>(`${path}/carry-forward/${operation.id}/resume`, { method: 'POST', csrf: session.csrf_token, headers: { 'If-Match': pending.etag, 'Idempotency-Key': pending.key }, body: pending.body }); setOperation(result); setReview(null); setUncertain(false); release(); operationSaved?.(result); changed(); }

    catch (error) { setError(error); if (isAccessError(error)) denied(error); setUncertain(error instanceof ApiError && (error.status === 0 || error.status >= 500)); }

    finally { setBusy(false); }

  }

  return <Modal title="Carry unfinished work forward" close={close} canClose={!busy && !uncertain}><div className="project-form">{operation ? <><h3>{operation.state === 'complete' ? 'Carry-forward complete' : operation.state === 'restricted' ? 'Some results are unavailable' : 'Some issues need review'}</h3><p>Only the explicitly selected eligible issues were attempted. Other closed-cycle history remains in place.</p>{operation.redacted && <p>Results are filtered to your current access.</p>}<ul className="carry-results">{operation.results.map(result => <li key={result.issue_id}><IssueLink workspace={cycle.workspace_id} team={cycle.team_id} id={result.issue_id}/><span>{result.outcome.replaceAll('_', ' ')}</span></li>)}</ul><ErrorBanner error={error}/>{operation.state !== 'complete' && !review && <button className="button" disabled={busy} onClick={() => void reviewUnresolved()}>Review unresolved issues</button>}{review && <section aria-label="Review carry-forward retry"><h4>Review current versions</h4><p>Destination: {review.target.name}. Only unresolved accessible issues appear. Select the exact items to retry.</p>{review.issues.map(issue => <label className="planning-ack" key={issue.id}><input type="checkbox" disabled={busy || uncertain} checked={resumeSelected.has(issue.id)} onChange={() => setResumeSelected(current => { const next = new Set(current); if (next.has(issue.id)) next.delete(issue.id); else next.add(issue.id); return next; })}/>{issue.key} · {issue.title} · version {issue.version}</label>)}{!review.issues.length && <p>No unresolved issues are currently accessible.</p>}<button className="button primary" disabled={busy || !resumeSelected.size} onClick={() => void resume()}>{uncertain ? 'Retry safely' : 'Retry selected unresolved issues'}</button></section>}<button className="button" disabled={busy || uncertain} onClick={close}>Done</button></> : <><p className="project-help">Issue labels reflect current access; categories and estimates are captured in the close report. Nothing is selected automatically. Select at most 100 issues per operation. Choose a destination and the exact unfinished issues to attempt; version conflicts never overwrite later edits.</p><ErrorBanner error={destinations.error} retry={destinations.retry}/><label className="field">Destination cycle<select value={destination} disabled={busy || uncertain} onChange={event => setDestination(event.target.value)}><option value="">Choose a cycle</option>{available.map(item => <option key={item.id} value={item.id}>{item.name} · {item.start_date}</option>)}</select></label>{!destinations.loading && !destinations.error && !available.length && <p className="project-help">No later planned or active cycle is available. <Link href={cycleUrl(cycle.workspace_id, cycle.team_id)}>Open cycles</Link> to create a destination, then return to this closure report.</p>}{destinations.value?.has_more && <button className="button compact" disabled={destinations.loading || uncertain} onClick={destinations.more}>Load more destination cycles</button>}<ScopeTable workspace={cycle.workspace_id} team={cycle.team_id} scope={report.scope} selectable selected={selected} busy={busy || uncertain} select={id => setSelected(current => { const next = new Set(current); if (next.has(id)) next.delete(id); else if (next.size < 100) next.add(id); return next; })}/><ErrorBanner error={error}/>{uncertain && <p role="status">The result is uncertain. Retry the identical operation before leaving.</p>}<div className="modal-footer"><button className="button" disabled={busy || uncertain} onClick={close}>Cancel</button><button className="button primary" disabled={busy || !destination || !selected.size} onClick={() => void submit()}>{uncertain ? 'Retry safely' : `Move ${selected.size} selected issues`}</button></div></>}</div></Modal>;

}
