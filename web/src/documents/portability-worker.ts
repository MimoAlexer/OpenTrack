import { canonicalBody, MAX_DOCUMENT_BYTES } from './model';
import type { NativeDocumentImport } from './portability';

export const MARKDOWN_REVIEW_TIMEOUT_MS = 5000;

export async function reviewMarkdownInWorker(source: string, signal: AbortSignal): Promise<NativeDocumentImport> {
  if (new TextEncoder().encode(source).length > MAX_DOCUMENT_BYTES) throw new Error('Markdown is too large. The limit is 256 KiB.');
  if (signal.aborted) throw new DOMException('Import canceled.', 'AbortError');
  let worker: Worker;
  try { worker = new Worker(new URL('./portability.worker.ts', import.meta.url), { type: 'module' }); }
  catch { throw new Error('Markdown import is unavailable in this browser. You can still import document JSON.'); }
  return new Promise((resolve, reject) => {
    let settled = false;
    const cleanup = () => { window.clearTimeout(timer); signal.removeEventListener('abort', abort); worker.terminate(); };
    const finish = (error: Error | null, result?: NativeDocumentImport) => { if (settled) return; settled = true; cleanup(); if (error) reject(error); else resolve(result!); };
    const abort = () => finish(new DOMException('Import canceled.', 'AbortError'));
    const timer = window.setTimeout(() => finish(new Error('Markdown took too long to process. Split it into smaller sections and try again.')), MARKDOWN_REVIEW_TIMEOUT_MS);
    signal.addEventListener('abort', abort, { once: true });
    worker.onmessage = (event: MessageEvent<{ ok: boolean; result?: NativeDocumentImport; error?: string }>) => {
      if (!event.data || event.data.ok !== true || !event.data.result) { finish(new Error(typeof event.data?.error === 'string' ? event.data.error : 'Markdown could not be imported.')); return; }
      try {
        const result = event.data.result;
        if (!Array.isArray(result.warnings) || result.warnings.length > 30 || result.warnings.some(warning => !warning || typeof warning.code !== 'string' || typeof warning.message !== 'string' || warning.code.length > 100 || warning.message.length > 1000)) throw new Error('Invalid warnings');
        finish(null, { body: canonicalBody(result.body, true), warnings: result.warnings });
      }
      catch { finish(new Error('The imported Markdown has an unsupported structure.')); }
    };
    worker.onerror = event => { event.preventDefault(); finish(new Error('Markdown could not be imported. Try a smaller document or document JSON.')); };
    try { worker.postMessage(source); }
    catch { finish(new Error('Markdown import could not start. Try again.')); }
  });
}
