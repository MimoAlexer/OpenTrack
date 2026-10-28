// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import RichTextEditor from './RichTextEditor';
import { fromPlainText } from './model';

let container: HTMLDivElement; let root: Root;
beforeEach(() => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true); vi.stubGlobal('requestAnimationFrame', (fn: FrameRequestCallback) => setTimeout(() => fn(0), 0));
  Range.prototype.getClientRects = () => document.createElement('span').getClientRects(); Range.prototype.getBoundingClientRect = () => new DOMRect(0, 0, 0, 0);
  HTMLDialogElement.prototype.showModal = function () { this.open = true; }; HTMLDialogElement.prototype.close = function () { this.open = false; };
  container = document.createElement('div'); document.body.append(container); root = createRoot(container);
});
afterEach(async () => { await act(async () => root.unmount()); container.remove(); vi.restoreAllMocks(); vi.unstubAllGlobals(); });
const body = fromPlainText('Private document');
async function render(disabled = false, concealed = false) { await act(async () => root.render(<RichTextEditor value={body} onChange={() => {}} label="Description" disabled={disabled} concealed={concealed}/>)); }

it('closes the nested link modal only for concealment and retains the actual editor', async () => {
  await render(); const editor = container.querySelector('[role="textbox"]')!;
  await act(async () => container.querySelector<HTMLButtonElement>('button[aria-label="Insert or edit link"]')!.click());
  expect(container.querySelector('dialog[open] input[placeholder="https://example.com"]')).not.toBeNull();
  await render(true); expect(container.querySelector('dialog[open]')).not.toBeNull();
  await render(true, true); expect(container.querySelector('dialog')).toBeNull(); expect(container.querySelector('[role="textbox"]')).toBe(editor);
  await render(); expect(container.querySelector('dialog')).toBeNull(); expect(container.querySelector('[role="textbox"]')).toBe(editor);
});

it('closes the document-transfer modal during concealment without replacing the editor', async () => {
  await render(); const editor = container.querySelector('[role="textbox"]')!;
  await act(async () => [...container.querySelectorAll<HTMLButtonElement>('button')].find(button => button.textContent === 'Import / export text')!.click());
  expect(container.querySelector('dialog[open] textarea')).not.toBeNull();
  await render(true, true); expect(container.querySelector('dialog')).toBeNull(); expect(container.querySelector('[role="textbox"]')).toBe(editor);
});
