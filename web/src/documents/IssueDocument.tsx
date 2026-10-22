import type { AttachmentDraftState } from '../attachments/types';
import { useEffect, useRef, useState } from 'react';
import { Check, LoaderCircle } from 'lucide-react';
import { ApiError } from '../api';
import { ErrorBanner } from '../components';
import { useSessionScope } from '../desktop/context';
import { useUnsavedChanges } from '../router';
import { useScopedRequest } from '../settings/shared';
import type { Session } from '../types';
import { DocumentPortability } from './DocumentPortability';
import { StructuredEditor } from './Editor';
import { DocumentView } from './DocumentView';
import { DocumentConflict } from './DocumentConflict';
import { canonicalBody, isAccessError, sameBody, type RichBody } from './model';
import type { StructuredDocument } from './types';
import { ReferenceProvider } from '../documents/ReferenceProvider';
import type { IssueReference } from '../references/types';
import type { MentionReference } from '../mentions/types';

export type DocumentDraftState = { dirty: boolean; busy: boolean; blocked: boolean; flush: () => Promise<boolean>; discard: () => void };
function readable(record: StructuredDocument): { body: RichBody | null; notice: string } {
  if (!record.schema_supported) return { body: null, notice: 'This document uses a newer format. Its text is shown read-only.' };
  try { return { body: canonicalBody(record.body, true), notice: '' }; }
  catch (error) { return { body: null, notice: `${error instanceof Error ? error.message : 'This format is not supported.'} Its text is shown read-only.` }; }
}
export function IssueDocument({ workspaceId, issueId, bodyVersion, session, readOnly, paused = false, stateChanged, saved }: {
  workspaceId: string; issueId: string; bodyVersion: string; session: Session; readOnly: boolean; paused?: boolean;
  stateChanged: (state: DocumentDraftState) => void; saved: (document: StructuredDocument) => void | Promise<void>;
}) {
  const request = useScopedRequest(); const scope = useSessionScope(session);
  const [uploads, setUploads] = useState<AttachmentDraftState | null>(null);
  const [mentionProjection, setMentionProjection] = useState<{ scope: string; references: MentionReference[]; issues: IssueReference[] }>({ scope: '', references: [], issues: [] });
  const [base, setBase] = useState<StructuredDocument | null>(null); const [draft, setDraft] = useState<RichBody | null>(null);
  const [latest, setLatest] = useState<StructuredDocument | null>(null); const [error, setError] = useState<unknown>(null);
  const [permitted, setPermitted] = useState(true);
  const [busy, setBusy] = useState(false); const [loading, setLoading] = useState(true); const [denied, setDenied] = useState(false);
  const [composing, setComposing] = useState(false); const [uncertain, setUncertain] = useState(false); const [review, setReview] = useState(false); const [savedOnce, setSavedOnce] = useState(false); const [retry, setRetry] = useState(0);
  const attempt = useRef<{ body: RichBody; etag: string } | null>(null); const flight = useRef(false);
  const alive = useRef(true); useEffect(() => { alive.current = true; return () => { alive.current = false; }; }, []);
  const current = useRef({ base, draft, latest, denied, composing, uncertain, review, readOnly, session, permitted, paused }); current.current = { base, draft, latest, denied, composing, uncertain, review, readOnly, session, permitted, paused };
  const callbacks = useRef({ stateChanged, saved }); callbacks.current = { stateChanged, saved };
  const parsed = base ? readable(base) : null;
  const dirty = !!draft && !!parsed?.body && !sameBody(draft, parsed.body);
  useUnsavedChanges(dirty || uncertain || busy || !!uploads?.pending);
  useEffect(() => {
    const abort = new AbortController(); setLoading(true);
    request<StructuredDocument>(`/workspaces/${workspaceId}/issues/${issueId}/document`, { signal: abort.signal }).then(record => {
      if (abort.signal.aborted) return;
      const now = current.current; const body = readable(record).body; const oldBody = now.base ? readable(now.base).body : null;
      // A refresh begun before a save must never roll its body or projection back.
      if (now.base && BigInt(record.version) < BigInt(now.base.version)) return;
      setDenied(false); setPermitted(record.can_edit); setMentionProjection({ scope, references: record.mentions ?? [], issues: record.issue_references ?? [] });
      const pending = !!now.draft && !!oldBody && !sameBody(now.draft, oldBody);
      if (!now.base || (!pending && !now.uncertain && !flight.current)) { setBase(record); setDraft(body); setLatest(null); setError(null); }
      else if (record.etag !== now.base.etag && (!body || !now.draft || !sameBody(body, now.draft))) setLatest(record);
    }).catch(error => { if (!abort.signal.aborted) { setError(error); if (isAccessError(error)) setDenied(true); } }).finally(() => { if (!abort.signal.aborted) setLoading(false); });
    return () => abort.abort();
  }, [workspaceId, issueId, bodyVersion, scope, retry]);
  async function accept(record: StructuredDocument, submitted: RichBody) {
    setMentionProjection({ scope, references: record.mentions ?? [], issues: record.issue_references ?? [] });
    const now = current.current; const next = now.draft && sameBody(now.draft, submitted) ? readable(record).body : now.draft;
    current.current = { ...now, base: record, draft: next, latest: null, uncertain: false, review: false };
    setBase(record); setDraft(next); setLatest(null); setError(null); setUncertain(false); setReview(false); setSavedOnce(true); attempt.current = null;
    await callbacks.current.saved(record);
  }
  async function save(): Promise<boolean> {
    const now = current.current; const before = now.base ? readable(now.base).body : null;
    if (!now.base || !now.draft || !before || now.denied || now.readOnly || now.paused || !now.base.can_edit || !now.permitted || now.composing || now.latest || now.uncertain || flight.current) return false;
    if (sameBody(now.draft, before)) return true;
    const submitted = now.draft; attempt.current = { body: submitted, etag: now.base.etag }; flight.current = true; setBusy(true); setError(null);
    try {
      const result = await request<StructuredDocument>(`/workspaces/${workspaceId}/documents/${now.base.id}`, { method: 'PATCH', body: { schema_version: 1, body: submitted }, csrf: now.session.csrf_token, headers: { 'If-Match': now.base.etag } });
      if (!alive.current) return false; await accept(result, submitted);
      return !!current.current.draft && sameBody(current.current.draft, submitted);
    } catch (error) {
      if (!alive.current) return false;
      if (error instanceof ApiError && error.status === 412) {
        const remote = error.details?.current as StructuredDocument | undefined;
        if (remote?.id === now.base.id) { setLatest(remote); setMentionProjection({ scope, references: remote.mentions ?? [], issues: remote.issue_references ?? [] }); } else { try { const record = await request<StructuredDocument>(`/workspaces/${workspaceId}/documents/${now.base.id}`); if (!alive.current) return false; setLatest(record); setMentionProjection({ scope, references: record.mentions ?? [], issues: record.issue_references ?? [] }); } catch (loadError) { setError(loadError); if (isAccessError(loadError)) setDenied(true); } }
        setReview(true);
      } else { setError(error); setReview(true); if (isAccessError(error)) setDenied(true); else if (!(error instanceof ApiError) || error.status === 0 || error.status === 408 || error.status >= 500) setUncertain(true); }
      return false;
    } finally { flight.current = false; if (alive.current) setBusy(false); }
  }
  async function reconcile() {
    const pending = attempt.current; const now = current.current; if (!pending || !now.base || flight.current) return;
    flight.current = true; setBusy(true); setError(null);
    try {
      const record = await request<StructuredDocument>(`/workspaces/${workspaceId}/documents/${now.base.id}`);
      if (!alive.current) return; setDenied(false); setMentionProjection({ scope, references: record.mentions ?? [], issues: record.issue_references ?? [] }); const body = readable(record).body;
      if (body && sameBody(body, pending.body)) await accept(record, pending.body);
      else { setUncertain(false); setReview(true); if (record.etag === pending.etag) setBase(record); else setLatest(record); }
    } catch (error) { if (alive.current) { setError(error); if (isAccessError(error)) setDenied(true); } }
    finally { flight.current = false; if (alive.current) setBusy(false); }
  }
  function discard() { const now = current.current; const record = now.latest ?? now.base; if (!uploads?.pending && record && !now.uncertain && !flight.current) { setBase(record); setDraft(readable(record).body); setPermitted(record.can_edit); setLatest(null); setReview(false); setError(null); } }
  useEffect(() => {
    callbacks.current.stateChanged({ dirty: dirty || uncertain || !!uploads?.pending, busy: busy || !!uploads?.busy, blocked: !!uploads?.pending || !!latest || uncertain || denied || !permitted || composing, flush: () => uploads?.pending ? Promise.resolve(false) : save(), discard });
  }, [dirty, uncertain, busy, latest, denied, composing, base?.etag, draft, readOnly, permitted, paused, uploads]);
  useEffect(() => {
    if (!dirty || busy || composing || latest || uncertain || review || readOnly || paused || !base?.can_edit || !permitted || denied) return;
    const timer = window.setTimeout(() => { void save(); }, 750); return () => clearTimeout(timer);
  }, [dirty, draft, busy, composing, latest, uncertain, review, readOnly, paused, base?.can_edit, permitted, denied]);
  if (denied) return <section className="issue-document"><ErrorBanner error={error} retry={() => setRetry(n => n + 1)} /><p className="document-notice">The description is hidden because access could not be confirmed.</p></section>;
  const latestBody = latest ? readable(latest).body : null;
  return <ReferenceProvider workspaceId={workspaceId} session={session} target={base ? { document_id: base.id } : { issue_id: issueId }} references={mentionProjection.references} issueReferences={mentionProjection.issues} projectionScope={mentionProjection.scope}><section className="issue-document" aria-label="Description">
    <div className="document-section-title"><h2>Description</h2><span className="document-save-status" role="status" aria-live="polite">{busy ? <><LoaderCircle className="spinner" aria-hidden="true" />Saving…</> : uncertain ? 'Save not confirmed' : dirty ? review || latest ? 'Review before saving' : composing ? 'Editing…' : 'Unsaved' : savedOnce ? <><Check aria-hidden="true" />Saved</> : ''}</span></div>
    <ErrorBanner error={error} retry={!uncertain ? () => setRetry(n => n + 1) : undefined} />
    {uncertain && <p className="document-notice" role="status">The response was interrupted. Your draft is still here. <button type="button" className="quiet" disabled={busy} onClick={reconcile}>Check save result</button></p>}
    {latestBody && draft && <DocumentConflict latest={latestBody} local={draft} choose={(body, needsReview) => { setBase(latest); setDraft(body); setLatest(null); setReview(needsReview); setError(null); }} />}
    {latest && !latestBody && <p className="document-notice">The latest saved version uses unsupported formatting. Your draft is preserved; saving is paused.</p>}
    {!base ? loading ? <p role="status" className="document-loading">Loading description…</p> : null : !draft ? <div className="document-fallback"><p className="notice">{parsed?.notice}</p><pre>{base.plain_text}</pre></div> : readOnly || !base.can_edit || !permitted ? <><DocumentView body={draft} fallback={base.plain_text} /><DocumentPortability value={draft} /></> : <StructuredEditor value={draft} onChange={setDraft} label="Description" attachmentTarget={{ document_id: base.id }} attachmentsChanged={setUploads} onCompositionChange={setComposing} onSave={() => { void save(); }} />}
    {base && draft && !readOnly && base.can_edit && permitted && <div className="document-state"><span>Type / for blocks · @ for people · # for issues · Ctrl / ⌘ Enter to save</span><div className="button-row" style={{ visibility: dirty || review || error ? 'visible' : 'hidden' }}><button type="button" className="quiet" disabled={!dirty || busy || uncertain || !!uploads?.pending} onClick={discard}>Discard text</button><button type="button" className="button compact" disabled={!dirty || busy || paused || composing || !!latest || uncertain} onClick={() => { void save(); }}>Save text now</button></div></div>}
  </section></ReferenceProvider>;
}
