// @vitest-environment jsdom
import { act, useState } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import type { Editor } from '@tiptap/core';
import { Plugin } from '@tiptap/pm/state';
import RichTextEditor from './RichTextEditor';
import { canonicalBody, fromPlainText, MAX_DOCUMENT_BYTES, type RichBody } from './model';
import { exportNativeDocument } from './portability';

let root: Root; let container: HTMLDivElement;
beforeEach(() => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true);
  Range.prototype.getClientRects = () => document.createElement('span').getClientRects();
  Range.prototype.getBoundingClientRect = () => new DOMRect(0, 0, 0, 0);
  HTMLDialogElement.prototype.showModal = function () { this.setAttribute('open', ''); };
  HTMLDialogElement.prototype.close = function () { this.removeAttribute('open'); };
  container = document.createElement('div'); document.body.append(container); root = createRoot(container);
});
afterEach(async () => { await act(async () => root.unmount()); container.remove(); vi.unstubAllGlobals(); });

async function click(label: string) {
  await act(async () => [...container.querySelectorAll('button')].find(button => button.textContent === label || button.getAttribute('aria-label') === label)!.click());
}
async function prepareImport(body: RichBody) {
  await click('Import / export text');
  await act(async () => {
    const action = container.querySelector('dialog select')! as HTMLSelectElement;
    action.value = 'import'; action.dispatchEvent(new Event('change', { bubbles: true }));
  });
  await act(async () => {
    const input = container.querySelector('dialog textarea')!;
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value')!.set!.call(input, exportNativeDocument(body).text);
    input.dispatchEvent(new Event('input', { bubbles: true }));
  });
  await click('Review import');
}

it('applies a reviewed JSON document through the actual editor with formatting intact and retains undo', async () => {
  const changed = vi.fn(); const saved = vi.fn();
  function Harness() {
    const [body, setBody] = useState(fromPlainText('Original draft'));
    return <RichTextEditor value={body} label="Description" onChange={next => { changed(next); setBody(next); }} onSave={saved} />;
  }
  const imported = canonicalBody({ type: 'doc', content: [
    { type: 'heading', attrs: { level: 2 }, content: [{ type: 'text', text: 'Imported title' }] },
    { type: 'paragraph', content: [{ type: 'text', text: 'Technical text', marks: [{ type: 'bold' }, { type: 'code' }] }] },
    { type: 'taskList', content: [{ type: 'taskItem', attrs: { checked: true }, content: [{ type: 'paragraph', content: [{ type: 'text', text: 'Done' }] }] }] },
    { type: 'codeBlock', attrs: { language: 'rust' }, content: [{ type: 'text', text: 'fn main() {}\n' }] },
  ] }, true);
  await act(async () => root.render(<Harness />));
  await prepareImport(imported);
  expect(changed).not.toHaveBeenCalled(); expect(container.querySelector('[role="textbox"]')?.textContent).toBe('Original draft');
  await click('Replace current draft');
  expect(changed).toHaveBeenLastCalledWith(imported); expect(saved).not.toHaveBeenCalled();
  expect(container.querySelector('[role="textbox"] h2')?.textContent).toBe('Imported title');
  expect(container.querySelector('[role="textbox"] strong code')?.textContent).toBe('Technical text');
  await click('Undo');
  expect(changed).toHaveBeenLastCalledWith(fromPlainText('Original draft'));
});

it('appends through the actual editor as one undoable draft change without requesting a save', async () => {
  const changed = vi.fn(); const saved = vi.fn();
  function Harness() { const [body, setBody] = useState(fromPlainText('Unsaved original')); return <RichTextEditor value={body} label="Description" onChange={next => { changed(next); setBody(next); }} onSave={saved}/>; }
  const imported = canonicalBody({ type: 'doc', content: [{ type: 'heading', attrs: { level: 3 }, content: [{ type: 'text', text: 'Imported heading' }] }, ...fromPlainText('Imported paragraph').content] }, true);
  await act(async () => root.render(<Harness/>)); await prepareImport(imported); expect(changed).not.toHaveBeenCalled(); await click('Append to draft');
  expect(changed).toHaveBeenCalledExactlyOnceWith({ type: 'doc', content: [...fromPlainText('Unsaved original').content, ...imported.content] }); expect(saved).not.toHaveBeenCalled();
  await click('Undo'); expect(changed).toHaveBeenLastCalledWith(fromPlainText('Unsaved original'));
});

it('keeps the draft and review when editor defaults push a valid imported body over its size limit', async () => {
  const changed = vi.fn(); const saved = vi.fn();
  await act(async () => root.render(<RichTextEditor value={fromPlainText('Original draft')} label="Description" onChange={changed} onSave={saved} />));
  const body: RichBody = { type: 'doc', content: [{ type: 'paragraph', content: [{ type: 'text', text: 'x', marks: [{ type: 'link', attrs: { href: 'https://example.test' } }] }] }] };
  body.content[0].content![0].text = 'x'.repeat(MAX_DOCUMENT_BYTES - JSON.stringify(body).length - 10 + 1);
  const imported = canonicalBody(body, true);
  expect(new TextEncoder().encode(JSON.stringify(imported)).length).toBe(262_134);
  await prepareImport(imported);
  expect(container.querySelector('[aria-label="Import preview"]')).not.toBeNull();
  await click('Replace current draft');
  expect(changed).not.toHaveBeenCalled(); expect(saved).not.toHaveBeenCalled();
  expect(container.querySelector('[role="textbox"]')?.textContent).toBe('Original draft');
  expect(container.querySelector('dialog')).not.toBeNull();
  expect(container.querySelector('[aria-label="Import preview"]')).not.toBeNull();
  expect(container.querySelector('dialog [role="alert"]')?.textContent).toContain('could not be applied');
  expect(container.querySelector('dialog [role="alert"]')?.textContent).toContain('Your current draft is preserved');
  await click('Cancel');
  await prepareImport(fromPlainText('Smaller replacement'));
  await click('Replace current draft');
  expect(changed).toHaveBeenCalledExactlyOnceWith(fromPlainText('Smaller replacement'));
  expect(container.querySelector('dialog')).toBeNull();
});

it('detects a transaction filter rejecting replacement even when the editor command reports success', async () => {
  const changed = vi.fn();
  await act(async () => root.render(<RichTextEditor value={fromPlainText('Original draft')} label="Description" onChange={changed} />));
  const editor = (container.querySelector('[role="textbox"]') as HTMLElement & { editor: Editor }).editor;
  editor.registerPlugin(new Plugin({ filterTransaction: transaction => !transaction.docChanged }));
  await act(async () => { expect(editor.commands.setContent(fromPlainText('Rejected replacement'), { emitUpdate: true, errorOnInvalidContent: true })).toBe(true); });
  await prepareImport(fromPlainText('Rejected replacement'));
  await click('Replace current draft');
  expect(changed).not.toHaveBeenCalled();
  expect(container.querySelector('[role="textbox"]')?.textContent).toBe('Original draft');
  expect(container.querySelector('dialog')).not.toBeNull();
  expect(container.querySelector('[aria-label="Import preview"]')).not.toBeNull();
  expect(container.querySelector('dialog [role="alert"]')?.textContent).toContain('Your current draft is preserved');
});
