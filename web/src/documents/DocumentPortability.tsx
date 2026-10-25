import { useEffect, useId, useRef, useState } from 'react';
import { Modal } from '../components';
import { canonicalBody, plainText, type RichBody } from './model';
import { exportMarkdownDocument, exportNativeDocument, importNativeDocument, MAX_PORTABLE_DOCUMENT_BYTES, type NativeDocumentImport, type PortabilityWarning } from './portability';
import { reviewMarkdownInWorker } from './portability-worker';
import { DocumentView } from './DocumentView';
import './portability.css';

export type DocumentPortabilityProps = {
  value: RichBody;
  onImport?: (body: RichBody) => void;
  disabled?: boolean;
};

function Warnings({ warnings }: { warnings: PortabilityWarning[] }) {
  return warnings.length ? <ul aria-label="Transfer limitations">{warnings.map(warning => <li key={warning.code}>{warning.message}</li>)}</ul> : null;
}

export function DocumentPortability({ value, onImport, disabled = false }: DocumentPortabilityProps) {
  const [open, setOpen] = useState(false);
  return <><button type="button" className="quiet" disabled={disabled} onClick={() => setOpen(true)}>Import / export text</button>{open && <TransferDialog value={value} onImport={onImport} disabled={disabled} close={() => setOpen(false)} />}</>;
}

function TransferDialog({ value, onImport, disabled, close }: DocumentPortabilityProps & { close: () => void }) {
  const [mode, setMode] = useState<'export' | 'import'>('export');
  const [format, setFormat] = useState<'native' | 'markdown'>('native');
  const [importFormat, setImportFormat] = useState<'native' | 'markdown'>('native');
  const [input, setInput] = useState('');
  const [preview, setPreview] = useState<NativeDocumentImport | null>(null);
  const [error, setError] = useState('');
  const [copied, setCopied] = useState(false);
  const [reviewing, setReviewing] = useState(false);
  const reviewRequest = useRef<AbortController | null>(null);
  useEffect(() => () => { reviewRequest.current?.abort(); }, []);
  function resetReview() { reviewRequest.current?.abort(); reviewRequest.current = null; setReviewing(false); setPreview(null); setError(''); }
  const id = useId();
  let exported = { text: '', warnings: [] as PortabilityWarning[] };
  let exportError = '';
  try { exported = format === 'native' ? exportNativeDocument(value) : exportMarkdownDocument(value); }
  catch (cause) { exportError = cause instanceof Error ? cause.message : 'This document cannot be exported.'; }
  async function copy() {
    try { await navigator.clipboard.writeText(exported.text); setCopied(true); setError(''); }
    catch { setError('Clipboard access is unavailable. Select and copy the text below.'); }
  }
  async function review() {
    resetReview();
    const controller = new AbortController(); reviewRequest.current = controller; setReviewing(true);
    try {
      const result = importFormat === 'native' ? importNativeDocument(input) : await reviewMarkdownInWorker(input, controller.signal);
      if (reviewRequest.current === controller && !controller.signal.aborted) setPreview(result);
    } catch (cause) {
      if (reviewRequest.current === controller && !controller.signal.aborted) { setPreview(null); setError(cause instanceof Error ? cause.message : 'This document cannot be imported.'); }
    } finally { if (reviewRequest.current === controller) { reviewRequest.current = null; setReviewing(false); } }
  }
  function apply(mode: 'replace' | 'append') {
    if (disabled || !onImport || !preview) return;
    try { onImport(mode === 'append' ? canonicalBody({ type: 'doc', content: [...value.content, ...preview.body.content] }, true) : preview.body); close(); }
    catch { setError('This document could not be applied to the editor. Your current draft is preserved; review the imported structure and try again.'); }
  }
  return <Modal title="Import and export text" close={close}>
    <div className="settings-modal-content">
      <p>This transfers the current document text. It does not export a workspace, discussion history or attachment files.</p>
      <label className="field" htmlFor={`${id}-mode`}>Action<select id={`${id}-mode`} data-autofocus value={mode} onChange={event => { resetReview(); setMode(event.target.value as 'export' | 'import'); setCopied(false); }}><option value="export">Export current draft</option>{onImport && <option value="import">Import text</option>}</select></label>
      {mode === 'export' ? <>
        <label className="field" htmlFor={`${id}-format`}>Format<select id={`${id}-format`} value={format} onChange={event => { setFormat(event.target.value as 'native' | 'markdown'); setCopied(false); setError(''); }}><option value="native">OpenTrack document JSON</option><option value="markdown">Markdown</option></select></label>
        <p>{format === 'native' ? 'Document JSON preserves supported text structure and reference IDs. Keep it with the same installation and permitted reference scope.' : 'Markdown preserves supported formatting, lists and code. References and some document details need conversion; review the limitations below. Use document JSON for exact structure.'}</p>
        <Warnings warnings={exported.warnings} />
        {exportError && <p role="alert">{exportError}</p>}
        <label className="field" htmlFor={`${id}-output`}>Exported {format === 'native' ? 'document JSON' : 'Markdown'}<textarea id={`${id}-output`} value={exported.text} readOnly rows={10} spellCheck={false} /></label>
        {copied && <p role="status">Copied.</p>}
        <div className="modal-footer"><button type="button" className="button" disabled={disabled || !!exportError} onClick={copy}>Copy text</button><button type="button" className="button" onClick={close}>Done</button></div>
      </> : <>
        <p>Paste document JSON or Markdown. Review the converted content, then choose whether to replace your draft or append imported blocks. Your document is saved through its normal save controls.</p>
        <label className="field" htmlFor={`${id}-import-format`}>Import format<select id={`${id}-import-format`} value={importFormat} disabled={disabled} onChange={event => { resetReview(); setImportFormat(event.target.value as 'native' | 'markdown'); }}><option value="native">OpenTrack document JSON</option><option value="markdown">Markdown</option></select></label>
        {importFormat === 'markdown' && <p>HTML and unsupported blocks remain inert source text. Unsafe links are rejected. Plain @names and issue keys stay text and do not create mentions or references.</p>}
        <label className="field" htmlFor={`${id}-input`}>{importFormat === 'native' ? 'Document JSON' : 'Markdown'}<textarea id={`${id}-input`} value={input} rows={8} spellCheck={false} maxLength={MAX_PORTABLE_DOCUMENT_BYTES} disabled={disabled} onChange={event => { resetReview(); setInput(event.target.value); }} /></label>
        <button type="button" className="button" disabled={disabled || reviewing || !input.trim()} onClick={review}>{reviewing ? 'Reviewing…' : 'Review import'}</button>
        {reviewing && <p role="status">Checking Markdown…</p>}
        {preview && <section aria-label="Import preview"><h3>Import preview</h3><p>{preview.body.content.length} imported top-level blocks. Replace removes your current text and reference nodes. Append keeps your draft and adds these blocks at the end.</p><Warnings warnings={preview.warnings} /><div className="document-import-preview">{importFormat === 'markdown' ? <DocumentView body={preview.body} /> : <pre>{plainText(preview.body) || 'This document contains no plain text.'}</pre>}</div><div className="button-row"><button type="button" className="button primary" disabled={disabled || !onImport} onClick={() => apply('replace')}>Replace current draft</button><button type="button" className="button" disabled={disabled || !onImport} onClick={() => apply('append')}>Append to draft</button></div></section>}
        <div className="modal-footer"><button type="button" className="button" onClick={close}>Cancel</button></div>
      </>}
      {error && <p role="alert">{error}</p>}
    </div>
  </Modal>;
}
