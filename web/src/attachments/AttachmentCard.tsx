import { useEffect, useRef, useState } from 'react';
import { isTauri } from '@tauri-apps/api/core';
import { Download, File, LoaderCircle, X } from 'lucide-react';
import { request } from '../api';
import { downloadAttachment } from '../desktop/bridge';
import { downloadBrowserAttachment } from './browser';
import { useAttachments } from './context';
import { formatBytes, scanLabel, type Attachment } from './types';
import './attachments.css';

export function AttachmentCard({ id, remove }: { id: string; remove?: () => void }) {
  const context = useAttachments();
  // Scope changes replace the inner state before paint, not in a later effect.
  // This also cancels any download owned by the previous credential scope.
  return <AttachmentCardContent key={`${context?.workspaceId ?? ''}:${context?.scope ?? ''}:${id}`} id={id} remove={remove} />;
}
function AttachmentCardContent({ id, remove }: { id: string; remove?: () => void }) {
  const context = useAttachments(); const [record, setRecord] = useState<Attachment | null>(() => context?.readySeed(id) ?? null); const [error, setError] = useState(''); const [busy, setBusy] = useState(false); const [retry, setRetry] = useState(0);
  const download = useRef<AbortController | null>(null);
  useEffect(() => {
    const abort = new AbortController(); download.current = null; setBusy(false); setError('');
    if (!context) { setRecord(null); return; }
    // Reauthorize on every account/request revision. Do not expose stale file
    // names while authority is being checked after a credential change.
    setRecord(context.readySeed(id) ?? null);
    request<Attachment>(`/workspaces/${context.workspaceId}/attachments/${id}`, { signal: abort.signal }).then(value => { if (!abort.signal.aborted) setRecord(value); }).catch(() => { if (!abort.signal.aborted) { setRecord(null); setError('This file is unavailable.'); } });
    return () => { abort.abort(); download.current?.abort(); };
  }, [context?.workspaceId, context?.scope, id, retry]);
  async function save() {
    if (!context || !record || record.state !== 'ready' || busy) return;
    const controller = new AbortController(); download.current = controller; setBusy(true); setError('');
    try {
      if (isTauri()) await downloadAttachment({ workspace_id: context.workspaceId, attachment_id: id, original_name: record.original_name });
      else await downloadBrowserAttachment(context.workspaceId, record, controller.signal);
    } catch (error) { if (!controller.signal.aborted) setError(error instanceof Error ? error.message : 'The download failed. Try again.'); }
    finally { if (download.current === controller) { download.current = null; if (!controller.signal.aborted) setBusy(false); } }
  }
  return <div className="attachment-card" contentEditable={false} data-attachment-id={id}>
    <File aria-hidden="true" className="attachment-file-icon" /><div className="attachment-information"><strong>{record?.original_name ?? (error ? 'File unavailable' : 'Loading file…')}</strong>{record && <span>{formatBytes(record.size_bytes)}{record.detected_mime ? ` · ${record.detected_mime}` : ''} · {scanLabel(record.scan_status)}</span>}{record && record.state !== 'ready' && <span>{record.state === 'deleted' ? 'File deleted' : 'This file is not ready to download.'}</span>}{error && <span role="alert" className="attachment-error">{error} <button type="button" className="quiet" onClick={() => setRetry(value => value + 1)}>Check again</button></span>}</div>
    {record?.state === 'ready' && <button type="button" className="icon-button" disabled={busy} aria-label={`Download ${record.original_name}`} title="Download file" onClick={save}>{busy ? <LoaderCircle className="spinner" aria-hidden="true" /> : <Download aria-hidden="true" />}</button>}
    {remove && <button type="button" className="icon-button" aria-label={`Remove ${record?.original_name ?? 'file'} from document`} title="Remove from document" onClick={remove}><X aria-hidden="true" /></button>}
  </div>;
}
