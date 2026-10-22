import { useEffect, useRef, useState } from 'react';
import { ApiError } from '../api';
import { ErrorBanner, Modal } from '../components';
import { useSessionScope } from '../desktop/context';
import type { components } from '../generated/api';
import { useScopedRequest } from '../settings/shared';
import type { Session } from '../types';
import { DocumentView } from './DocumentView';
import { isAccessError } from './model';
import { ReferenceProvider } from './ReferenceProvider';
import type { Page } from './types';

type RevisionSummary = components['schemas']['DocumentRevisionSummary'];
type Revision = components['schemas']['DocumentRevision'];
export type DocumentHistoryProps = { workspaceId: string; documentId: string; session: Session; refreshVersion: string; onDenied?: (error: unknown) => void };

function timestamp(value: string) { const date = new Date(value); return Number.isNaN(date.getTime()) ? value : date.toLocaleString(undefined, { dateStyle: 'medium', timeStyle: 'short' }); }

export function DocumentHistory(props: DocumentHistoryProps) {
  const scope = useSessionScope(props.session);
  return <HistoryEntry key={`${props.workspaceId}:${props.documentId}:${scope}`} {...props} scope={scope} />;
}

function HistoryEntry(props: DocumentHistoryProps & { scope: string }) {
  const [open, setOpen] = useState(false);
  return <><button type="button" className="quiet" onClick={() => setOpen(true)}>Brief history</button>{open && <HistoryDialog key={props.refreshVersion} {...props} close={() => setOpen(false)} />}</>;
}

function HistoryDialog({ workspaceId, documentId, session, scope, onDenied, close }: DocumentHistoryProps & { scope: string; close: () => void }) {
  const request = useScopedRequest(); const path = `/workspaces/${workspaceId}/documents/${documentId}/revisions`;
  const [summaries, setSummaries] = useState<RevisionSummary[]>([]); const [cursor, setCursor] = useState<string | null>(null); const [next, setNext] = useState<string | null>(null);
  const [listLoading, setListLoading] = useState(true); const [listError, setListError] = useState<unknown>(null); const [listRetry, setListRetry] = useState(0);
  const [selected, setSelected] = useState<string | null>(null); const [record, setRecord] = useState<Revision | null>(null); const [detailError, setDetailError] = useState<unknown>(null); const [detailRetry, setDetailRetry] = useState(0);
  const [denied, setDenied] = useState<unknown>(null); const revoked = useRef(false);
  const pending = useRef<{ list: AbortController | null; detail: AbortController | null }>({ list: null, detail: null });
  const deniedCallback = useRef(onDenied); deniedCallback.current = onDenied;
  function deny(error: unknown) {
    if (revoked.current) return;
    revoked.current = true; pending.current.list?.abort(); pending.current.detail?.abort();
    setSummaries([]); setNext(null); setSelected(null); setRecord(null); setDenied(error); deniedCallback.current?.(error);
  }
  useEffect(() => {
    const abort = new AbortController(); pending.current.list = abort; setListLoading(true); setListError(null);
    request<Page<RevisionSummary>>(`${path}?limit=20${cursor ? `&cursor=${encodeURIComponent(cursor)}` : ''}`, { signal: abort.signal }).then(page => {
      if (abort.signal.aborted || revoked.current) return;
      setSummaries(old => cursor ? [...old, ...page.data] : page.data); setNext(page.has_more ? page.next_cursor : null);
    }).catch(error => { if (!abort.signal.aborted && !revoked.current) { if (isAccessError(error)) deny(error); else setListError(error); } })
      .finally(() => { if (!abort.signal.aborted && !revoked.current) setListLoading(false); });
    return () => { abort.abort(); if (pending.current.list === abort) pending.current.list = null; };
  }, [path, cursor, listRetry]);
  useEffect(() => {
    const abort = new AbortController(); pending.current.detail = abort; setRecord(null); setDetailError(null);
    if (selected && !revoked.current) request<Revision>(`${path}/${encodeURIComponent(selected)}`, { signal: abort.signal }).then(value => {
      if (!abort.signal.aborted && !revoked.current) setRecord(value);
    }).catch(async error => {
      if (abort.signal.aborted || revoked.current) return;
      if (error instanceof ApiError && error.status === 404) {
        try {
          await request(`/workspaces/${workspaceId}/documents/${documentId}`, { signal: abort.signal });
          if (!abort.signal.aborted && !revoked.current) setDetailError(new ApiError(404, 'revision_unavailable', 'This revision is no longer available.'));
          return;
        }
        catch (ownerError) { error = ownerError; }
        if (abort.signal.aborted || revoked.current) return;
      }
      if (isAccessError(error)) deny(error); else setDetailError(error);
    });
    return () => { abort.abort(); if (pending.current.detail === abort) pending.current.detail = null; };
  }, [path, selected, detailRetry]);
  const revision = record?.revision === selected ? record : null;
  return <Modal title="Brief history" close={close}><div className="settings-modal-content">
    {denied ? <><ErrorBanner error={denied} /><p>The brief history is hidden because access could not be confirmed.</p></> : <>
      <p>Saved revisions are read-only. Viewing a revision preserves your current draft.</p>
      <ErrorBanner error={listError} retry={() => setListRetry(value => value + 1)} />
      {listLoading && <p role="status">Loading brief history…</p>}
      {!listLoading && !listError && !summaries.length && <p>No saved revisions are available.</p>}
      {!!summaries.length && <ol className="comment-list" aria-label="Saved brief revisions">{summaries.map(summary => <li key={summary.revision}>
        <button type="button" className="button compact" aria-label={`View revision ${summary.revision}`} aria-pressed={selected === summary.revision} onClick={() => setSelected(summary.revision)}>
          Revision {summary.revision} · {summary.migrated && 'Migrated snapshot · '}{summary.author?.display_name ?? 'Unknown author'} · {summary.migrated ? 'Captured ' : 'Saved '}<time dateTime={summary.created_at}>{timestamp(summary.created_at)}</time>
        </button>
      </li>)}</ol>}
      {next && <button type="button" className="button compact" disabled={listLoading} onClick={() => { setCursor(next); setNext(null); }}>Load older revisions</button>}
      {selected ? <section aria-label={`Revision ${selected}`} aria-busy={!revision && !detailError}>
        <h3>Revision {selected}</h3><ErrorBanner error={detailError} retry={() => setDetailRetry(value => value + 1)} />
        {!revision && !detailError && <p role="status">Loading revision…</p>}
        {revision && <>
          {revision.migrated && <p>This migrated snapshot captures the brief at migration time. Its earlier author and editing time are unknown.</p>}
          {revision.schema_version !== 1 && <p>This revision uses a newer format. Its text is shown read-only.</p>}
          <ReferenceProvider key={revision.revision} workspaceId={workspaceId} session={session} target={{ document_id: documentId }} references={revision.mentions} issueReferences={revision.issue_references} projectionScope={scope}>
            <DocumentView body={revision.body} fallback={revision.plain_text} supported={revision.schema_version === 1} className="document-readonly" />
          </ReferenceProvider>
        </>}
      </section> : !!summaries.length && <p>Choose a revision to read.</p>}
    </>}
    <div className="modal-footer"><button type="button" className="button" data-autofocus onClick={close}>Done</button></div>
  </div></Modal>;
}
