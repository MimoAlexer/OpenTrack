import { useRef, useState, type FormEvent } from 'react';
import { ApiError } from '../api';
import { ErrorBanner, Modal } from '../components';
import { isAccessError } from '../documents/model';
import { useUnsavedChanges } from '../router';
import { useScopedRequest } from '../settings/shared';
import type { Session } from '../types';
import { cyclePath, type Cycle } from './types';

export function CycleEditor({ workspace, team, session, cycle, close, saved, denied }: { workspace: string; team: string; session: Session; cycle?: Cycle; close: () => void; saved: (cycle: Cycle) => void; denied: (error: unknown) => void }) {
  const request = useScopedRequest(); const path = cyclePath(workspace, team, cycle?.id);
  const [base, setBase] = useState(cycle); const [name, setName] = useState(cycle?.name ?? ''); const [start, setStart] = useState(cycle?.start_date ?? ''); const [end, setEnd] = useState(cycle?.end_date_exclusive ?? ''); const [zone, setZone] = useState(cycle?.timezone ?? 'UTC');
  const [busy, setBusy] = useState(false); const [error, setError] = useState<unknown>(null); const [latest, setLatest] = useState<Cycle | null>(null); const [uncertain, setUncertain] = useState(false);
  const sent = useRef<{ body: object; key: string; etag?: string } | null>(null);
  const dirty = name !== (base?.name ?? '') || start !== (base?.start_date ?? '') || end !== (base?.end_date_exclusive ?? '') || zone !== (base?.timezone ?? 'UTC'); const release = useUnsavedChanges(dirty || uncertain);
  const validDates = !!start && !!end && start < end && (new Date(`${end}T12:00:00Z`).getTime() - new Date(`${start}T12:00:00Z`).getTime()) / 86400000 <= 366;
  async function submit(event?: FormEvent) {
    event?.preventDefault(); if (busy || latest || !name.trim() || !validDates) return;
    if (!uncertain || !sent.current) sent.current = { key: crypto.randomUUID(), etag: base?.etag, body: { name: name.trim(), start_date: start, end_date_exclusive: end, ...(!base ? { timezone: zone.trim() } : {}) } };
    const pending = sent.current; setBusy(true); setError(null);
    try { const result = await request<Cycle>(path, { method: base ? 'PATCH' : 'POST', csrf: session.csrf_token, body: pending.body, headers: base ? { 'If-Match': pending.etag! } : { 'Idempotency-Key': pending.key } }); release(); saved(result); }
    catch (error) { setError(error); if (isAccessError(error) && !(error instanceof ApiError && error.status === 403)) denied(error); setUncertain(error instanceof ApiError && (error.status === 0 || error.status >= 500)); if (error instanceof ApiError && error.status === 412 && base) { try { setLatest(await request<Cycle>(path)); } catch (readError) { setError(readError); if (isAccessError(readError)) denied(readError); } } }
    finally { setBusy(false); }
  }
  function dismiss() { if (!busy && !uncertain && (!dirty || window.confirm('Discard cycle changes?'))) close(); }
  return <Modal title={cycle ? 'Edit cycle' : 'New cycle'} close={dismiss} canClose={!busy && !uncertain}><form className="project-form" onSubmit={submit}><fieldset disabled={busy || uncertain} className="planning-fieldset"><label className="field">Cycle name<input data-autofocus value={name} onChange={event => setName(event.target.value)}/></label><div className="form-grid"><label className="field">Start date<input type="date" value={start} onChange={event => setStart(event.target.value)}/></label><label className="field">End date (exclusive)<input type="date" value={end} onChange={event => setEnd(event.target.value)}/></label></div><label className="field">Timezone<input value={zone} readOnly={!!base} onChange={event => setZone(event.target.value)} placeholder="UTC or an IANA timezone"/></label><p className="project-help">Calendar dates use this timezone. The end date is the first day outside the cycle. A cycle spans at most 366 days; team cycles cannot overlap.</p>{start && end && !validDates && <p role="alert" className="project-validation">Choose an exclusive end after the start, within 366 days.</p>}</fieldset><ErrorBanner error={error}/>{latest && <div className="project-conflict"><h3>Cycle changed</h3><p>Saved: {latest.name}, {latest.start_date} to {latest.end_date_exclusive} exclusive. Your draft is retained.</p><button type="button" className="button" onClick={() => { setBase(latest); setLatest(null); setError(null); sent.current = null; }}>Keep my draft for review</button></div>}{uncertain && <p role="status">The result is uncertain. Retry the identical command before editing or leaving.</p>}<div className="modal-footer"><button type="button" className="button" disabled={busy || uncertain} onClick={dismiss}>Cancel</button>{uncertain ? <button type="button" className="button primary" disabled={busy} onClick={() => void submit()}>Retry safely</button> : <button className="button primary" disabled={busy || !!latest || !name.trim() || !zone.trim() || !validDates}>{cycle ? 'Save cycle' : 'Create cycle'}</button>}</div></form></Modal>;
}
