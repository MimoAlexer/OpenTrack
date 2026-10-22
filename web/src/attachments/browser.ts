import { ApiError } from '../api';
import { MAX_ATTACHMENT_BYTES, HARD_MAX_ATTACHMENT_BYTES, formatBytes, type Attachment } from './types';

export function validateFile(file: Pick<File, 'name' | 'size'>, maximum = MAX_ATTACHMENT_BYTES) {
  if (!file.name || /[\u0000-\u001f\u007f]/u.test(file.name)) throw new Error('Choose a file with a valid name.');
  if (!Number.isSafeInteger(file.size) || file.size <= 0) throw new Error('Empty files cannot be attached.');
  if (file.size > Math.min(maximum, HARD_MAX_ATTACHMENT_BYTES)) throw new Error(`Each file must be ${formatBytes(maximum)} or smaller.`);
}
export async function hashFile(file: File, signal: AbortSignal, maximum = MAX_ATTACHMENT_BYTES): Promise<string> {
  validateFile(file, maximum); signal.throwIfAborted();
  // Web Crypto has no streaming digest. Bound the file before allocating it;
  // the upload path sends the File directly and does not allocate a second copy.
  const bytes = await file.arrayBuffer(); signal.throwIfAborted();
  const digest = await crypto.subtle.digest('SHA-256', bytes); signal.throwIfAborted();
  return [...new Uint8Array(digest)].map(byte => byte.toString(16).padStart(2, '0')).join('');
}
export function uploadBrowserFile(workspaceId: string, attachment: Attachment, file: File, csrf: string, signal: AbortSignal, progress: (sent: number, total: number) => void): Promise<Attachment> {
  validateFile(file, HARD_MAX_ATTACHMENT_BYTES);
  return new Promise((resolve, reject) => {
    const xhr = new XMLHttpRequest(); let settled = false;
    const abort = () => xhr.abort();
    function finish(error: unknown, value?: Attachment) {
      if (settled) return; settled = true; signal.removeEventListener('abort', abort);
      if (error) reject(error); else resolve(value!);
    }
    // Bind upload listeners before open/send for consistent browser behavior.
    xhr.upload.onprogress = event => { if (event.lengthComputable) progress(event.loaded, event.total); };
    xhr.onload = () => {
      let body: unknown;
      try { body = JSON.parse(xhr.responseText); } catch { finish(new ApiError(0, 'invalid_response', 'The upload response was interrupted. Check its status before retrying.')); return; }
      if (xhr.status >= 200 && xhr.status < 300 && body && typeof body === 'object' && 'id' in body) { finish(null, body as Attachment); return; }
      const problem = body as { code?: string; detail?: string; request_id?: string; details?: Record<string, unknown> };
      finish(new ApiError(xhr.status, problem?.code ?? 'upload_failed', problem?.detail ?? 'The upload could not be completed.', problem?.request_id, problem?.details));
    };
    xhr.onerror = () => finish(new ApiError(0, 'upload_interrupted', 'The connection was interrupted. Check the file status before retrying.'));
    xhr.ontimeout = () => finish(new ApiError(408, 'upload_timeout', 'The upload timed out. Check the file status before retrying.'));
    xhr.onabort = () => finish(new DOMException('Upload canceled.', 'AbortError'));
    xhr.open('PUT', `/api/v1/workspaces/${encodeURIComponent(workspaceId)}/attachments/${encodeURIComponent(attachment.id)}/content`);
    xhr.withCredentials = true; xhr.timeout = 180_000;
    xhr.setRequestHeader('Accept', 'application/json, application/problem+json'); xhr.setRequestHeader('Content-Type', 'application/octet-stream'); xhr.setRequestHeader('X-CSRF-Token', csrf); xhr.setRequestHeader('If-Match', attachment.etag);
    // Content-Length is a forbidden browser header; XHR derives it from File.
    signal.addEventListener('abort', abort, { once: true });
    if (signal.aborted) { finish(new DOMException('Upload canceled.', 'AbortError')); return; }
    xhr.send(file);
  });
}
export async function downloadBrowserAttachment(workspaceId: string, attachment: Attachment, signal: AbortSignal): Promise<void> {
  const response = await fetch(`/api/v1/workspaces/${encodeURIComponent(workspaceId)}/attachments/${encodeURIComponent(attachment.id)}/content`, { signal, credentials: 'same-origin', headers: { Accept: 'application/octet-stream' }, redirect: 'error' });
  if (!response.ok) { const problem = await response.json().catch(() => null); throw new ApiError(response.status, problem?.code ?? 'download_failed', problem?.detail ?? 'This file could not be downloaded.'); }
  // The same configured per-file bound applies to downloads; never trust MIME or
  // render active content. The browser receives only a download Blob URL.
  const rawLength = response.headers.get('Content-Length'); const length = rawLength === null ? NaN : Number(rawLength);
  if (!Number.isSafeInteger(length) || length <= 0 || length !== attachment.size_bytes || length > HARD_MAX_ATTACHMENT_BYTES) throw new Error('The download size could not be verified.');
  const reader = response.body?.getReader(); if (!reader) throw new Error('The download stream is unavailable.');
  const chunks: Uint8Array<ArrayBuffer>[] = []; let size = 0;
  try { for (;;) { const { done, value } = await reader.read(); if (done) break; size += value.byteLength; if (size > HARD_MAX_ATTACHMENT_BYTES || size > attachment.size_bytes) { await reader.cancel(); throw new Error('The downloaded file exceeds its recorded size.'); } chunks.push(new Uint8Array(value)); } }
  finally { reader.releaseLock(); }
  signal.throwIfAborted(); if (size !== attachment.size_bytes) throw new Error('The download was incomplete. Try again.');
  const blob = new Blob(chunks, { type: 'application/octet-stream' });
  const digest = await crypto.subtle.digest('SHA-256', await blob.arrayBuffer()); signal.throwIfAborted();
  const hash = [...new Uint8Array(digest)].map(byte => byte.toString(16).padStart(2, '0')).join('');
  if (hash !== attachment.sha256.toLowerCase()) throw new Error('The downloaded file failed its integrity check. Try again.');
  const url = URL.createObjectURL(blob); const link = document.createElement('a'); link.href = url; link.download = attachment.original_name; document.body.append(link); link.click(); link.remove(); window.setTimeout(() => URL.revokeObjectURL(url), 30_000);
}
