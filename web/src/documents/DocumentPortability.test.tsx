// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { DocumentPortability } from './DocumentPortability';
import { fromPlainText, MAX_DOCUMENT_BYTES } from './model';
import { exportNativeDocument } from './portability';
import { importMarkdownDocument } from './portability-markdown';
import { reviewMarkdownInWorker } from './portability-worker';

vi.mock('./portability-worker', () => ({ reviewMarkdownInWorker: vi.fn() }));

let container: HTMLDivElement; let root: Root;
beforeEach(() => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true);
  vi.mocked(reviewMarkdownInWorker).mockReset();
  HTMLDialogElement.prototype.showModal = function () { this.setAttribute('open', ''); };
  HTMLDialogElement.prototype.close = function () { this.removeAttribute('open'); };
  container = document.createElement('div'); document.body.append(container); root = createRoot(container);
});
afterEach(async () => { await act(async () => root.unmount()); container.remove(); vi.unstubAllGlobals(); vi.restoreAllMocks(); });
const button = (label: string) => [...container.querySelectorAll('button')].find(item => item.textContent === label)!;
const click = async (label: string) => { await act(async () => button(label).click()); };
async function action(value: string) { await act(async () => { const select = container.querySelector('select')!; select.value = value; select.dispatchEvent(new Event('change', { bubbles: true })); }); }
async function importFormat(value: string) { await act(async () => { const select = container.querySelector<HTMLSelectElement>('select[id$="-import-format"]')!; select.value = value; select.dispatchEvent(new Event('change', { bubbles: true })); }); }
async function input(value: string) { await act(async () => { const input = container.querySelector('textarea')!; Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value')!.set!.call(input, value); input.dispatchEvent(new Event('input', { bubbles: true })); }); }
async function render(onImport?: (body: ReturnType<typeof fromPlainText>) => void, disabled = false) { await act(async () => root.render(<DocumentPortability value={fromPlainText('Existing draft')} onImport={onImport} disabled={disabled} />)); }

it('requires review and an explicit replace action, and never writes the document itself', async () => {
  const apply = vi.fn(); const fetch = vi.fn(); vi.stubGlobal('fetch', fetch);
  await render(apply); await click('Import / export text'); await action('import');
  await input(exportNativeDocument(fromPlainText('Imported draft')).text);
  expect(button('Replace current draft')).toBeUndefined(); expect(apply).not.toHaveBeenCalled();
  await click('Review import'); expect(container.textContent).toContain('Imported draft'); expect(apply).not.toHaveBeenCalled();
  await click('Replace current draft'); expect(apply).toHaveBeenCalledExactlyOnceWith(fromPlainText('Imported draft')); expect(fetch).not.toHaveBeenCalled(); expect(container.querySelector('dialog')).toBeNull();
});

it('appends reviewed blocks only after confirmation, preserving the current draft without saving', async () => {
  const apply = vi.fn(); const fetch = vi.fn(); vi.stubGlobal('fetch', fetch);
  await render(apply); await click('Import / export text'); await action('import'); await input(exportNativeDocument(fromPlainText('Imported draft')).text);
  expect(button('Append to draft')).toBeUndefined(); await click('Review import'); expect(apply).not.toHaveBeenCalled();
  await click('Append to draft'); expect(apply).toHaveBeenCalledExactlyOnceWith({ type: 'doc', content: [...fromPlainText('Existing draft').content, ...fromPlainText('Imported draft').content] }); expect(fetch).not.toHaveBeenCalled();
});

it('retains the review and draft when their combined append exceeds the unchanged document limit', async () => {
  const apply = vi.fn(); await act(async () => root.render(<DocumentPortability value={fromPlainText('a'.repeat(MAX_DOCUMENT_BYTES / 2))} onImport={apply}/>));
  await click('Import / export text'); await action('import'); await input(exportNativeDocument(fromPlainText('b'.repeat(MAX_DOCUMENT_BYTES / 2))).text); await click('Review import'); await click('Append to draft');
  expect(apply).not.toHaveBeenCalled(); expect(container.querySelector('[aria-label="Import preview"]')).not.toBeNull(); expect(container.querySelector('[role="alert"]')?.textContent).toContain('Your current draft is preserved');
});

it('invalidates the reviewed import after edits and preserves the draft on failure or cancellation', async () => {
  const apply = vi.fn(); await render(apply); await click('Import / export text'); await action('import');
  await input(exportNativeDocument(fromPlainText('Ready')).text); await click('Review import');
  await input('{unknown'); expect(button('Replace current draft')).toBeUndefined();
  await click('Review import'); expect(container.querySelector('[role="alert"]')?.textContent).toContain('not valid');
  await click('Cancel'); expect(apply).not.toHaveBeenCalled();
});

it('keeps the review available when the editor rejects applying a replacement', async () => {
  const apply = vi.fn(() => { throw new Error('Unsupported editor structure'); });
  await render(apply); await click('Import / export text'); await action('import');
  await input(exportNativeDocument(fromPlainText('Reviewed')).text); await click('Review import'); await click('Replace current draft');
  expect(container.querySelector('[role="alert"]')?.textContent).toContain('could not be applied');
  expect(container.querySelector('[aria-label="Import preview"]')).not.toBeNull();
  expect(container.querySelector('dialog')).not.toBeNull();
});

it('supports read-only export without offering import and keeps copy failure recoverable', async () => {
  Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { writeText: vi.fn().mockRejectedValue(new Error('denied')) } });
  await render(); await click('Import / export text');
  expect([...container.querySelectorAll('option')].map(option => option.value)).not.toContain('import');
  expect(container.querySelector('textarea')?.value).toContain('Existing draft');
  await click('Copy text'); expect(container.querySelector('[role="alert"]')?.textContent).toContain('Select and copy'); expect(container.querySelector('textarea')?.readOnly).toBe(true);
});

it('blocks import after write permission changes while the review is open', async () => {
  const apply = vi.fn(); await render(apply); await click('Import / export text'); await action('import');
  await input(exportNativeDocument(fromPlainText('Ready')).text); await click('Review import');
  await render(apply, true); expect(button('Replace current draft').disabled).toBe(true); expect(button('Append to draft').disabled).toBe(true); await click('Replace current draft'); await click('Append to draft'); expect(apply).not.toHaveBeenCalled();
});

it('renders a safe Markdown review with conversion warnings before replacing the draft', async () => {
  const apply = vi.fn(); vi.mocked(reviewMarkdownInWorker).mockImplementation(async source => importMarkdownDocument(source));
  await render(apply); await click('Import / export text'); await action('import'); await importFormat('markdown');
  await input('# Reviewed\n\n**Strong text**\n\n<img src=x onerror=alert(1)>'); await click('Review import');
  expect(container.querySelector('[aria-label="Import preview"] h1')?.textContent).toBe('Reviewed'); expect(container.querySelector('strong')?.textContent).toBe('Strong text');
  expect(container.querySelector('img')).toBeNull(); expect(container.querySelector('script')).toBeNull(); expect(container.textContent).toContain('never rendered or executed'); expect(apply).not.toHaveBeenCalled();
  await click('Replace current draft'); expect(apply).toHaveBeenCalledOnce();
});

it('cancels stale Markdown reviews when input or format changes and never applies their late results', async () => {
  let resolve!: (value: ReturnType<typeof importMarkdownDocument>) => void;
  vi.mocked(reviewMarkdownInWorker).mockImplementation(() => new Promise(done => { resolve = done; }));
  const apply = vi.fn(); await render(apply); await click('Import / export text'); await action('import'); await importFormat('markdown');
  await input('Old text'); await click('Review import'); expect(container.textContent).toContain('Checking Markdown');
  const signal = vi.mocked(reviewMarkdownInWorker).mock.calls[0][1]; await input('New text'); expect(signal.aborted).toBe(true);
  await act(async () => resolve(importMarkdownDocument('Old result'))); expect(container.querySelector('[aria-label="Import preview"]')).toBeNull();
  await click('Review import'); const secondSignal = vi.mocked(reviewMarkdownInWorker).mock.calls[1][1]; await importFormat('native'); expect(secondSignal.aborted).toBe(true); expect(apply).not.toHaveBeenCalled();
});

it('keeps unsafe Markdown rejection visible and aborts an outstanding review when canceled', async () => {
  vi.mocked(reviewMarkdownInWorker).mockRejectedValue(new Error('Markdown contains an unsafe or unsupported link.'));
  const apply = vi.fn(); await render(apply); await click('Import / export text'); await action('import'); await importFormat('markdown');
  await input('[Unsafe](javascript:alert(1))'); await click('Review import'); expect(container.querySelector('[role="alert"]')?.textContent).toContain('unsafe'); expect(button('Replace current draft')).toBeUndefined();
  vi.mocked(reviewMarkdownInWorker).mockImplementation(() => new Promise(() => {})); await input('Waiting'); await click('Review import');
  const signal = vi.mocked(reviewMarkdownInWorker).mock.calls[1][1]; await click('Cancel'); expect(signal.aborted).toBe(true); expect(apply).not.toHaveBeenCalled();
});
