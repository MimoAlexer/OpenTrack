// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import RichTextEditor from '../documents/RichTextEditor';
import { canonicalBody, fromPlainText, issueReferenceIds, plainText, type RichBody } from '../documents/model';
import { IssueReferenceProvider } from './context';
import type { Session } from '../types';
import type { IssueReference } from './types';
let root: Root; let container: HTMLDivElement; let last: RichBody; const change = vi.fn((body: RichBody) => { last = body; });
const first = '11111111-1111-4111-8111-111111111111', second = '22222222-2222-4222-8222-222222222222';
const session: Session = { audience: 'web', user: { id: 'u', display_name: 'Example Owner', email: 'owner@example.test' }, csrf_token: 'csrf', workspaces: [] };
const json = (value: unknown) => new Response(JSON.stringify(value), { status: 200 });
beforeEach(() => { vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true); Range.prototype.getClientRects = () => document.createElement('span').getClientRects(); Range.prototype.getBoundingClientRect = () => new DOMRect(100,100,0,20); vi.stubGlobal('requestAnimationFrame', (fn: FrameRequestCallback) => setTimeout(() => fn(0),0)); container = document.createElement('div'); document.body.append(container); root = createRoot(container); change.mockClear(); vi.stubGlobal('fetch', vi.fn(() => Promise.resolve(json({ data: [{ id: first, key: 'ENG-1', title: 'First issue' }, { id: second, key: 'ENG-2', title: 'Second issue' }], next_cursor: null, has_more: false })))); });
afterEach(async () => { await act(async () => root.unmount()); container.remove(); vi.unstubAllGlobals(); });
const editable = () => container.querySelector<HTMLElement>('[contenteditable=true]')!;
async function render(body: RichBody, save = () => {}, references: IssueReference[] = [], account = session, projectionScope = 'u:csrf') { last = body; await act(async () => root.render(<IssueReferenceProvider workspaceId="w" session={account} target={{ issue_id: 'i' }} references={references} projectionScope={projectionScope}><RichTextEditor value={body} onChange={change} label="Comment" onSave={save}/></IssueReferenceProvider>)); }
async function focusEnd() { await act(async () => { const element = editable(); const text = element.querySelector('p, code')?.lastChild; element.focus(); if (text) { const range = document.createRange(); range.selectNodeContents(text); range.collapse(false); const selection = window.getSelection()!; selection.removeAllRanges(); selection.addRange(range); document.dispatchEvent(new Event('selectionchange')); } await new Promise(resolve => setTimeout(resolve,30)); }); await act(async () => new Promise(resolve => setTimeout(resolve,180))); }
async function key(key: string, extra: KeyboardEventInit = {}) { const event = new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true, ...extra }); await act(async () => editable().dispatchEvent(event)); return event; }
it('selects an ID-only issue reference with keyboard while retaining the editor and suppressing parent submit', async () => {
  const save = vi.fn(); await render(fromPlainText('#ENG'), save); await focusEnd(); const node = editable(); expect(container.querySelectorAll('[role=option]')).toHaveLength(2);
  await key('ArrowDown'); const event = await key('Enter', { ctrlKey: true }); expect(event.defaultPrevented).toBe(true); expect(save).not.toHaveBeenCalled(); expect(issueReferenceIds(last)).toEqual([second]); expect(last.content[0].content?.[0]).toEqual({ type: 'issueReference', attrs: { id: second } }); expect(editable()).toBe(node); expect(document.activeElement).toBe(node); expect(node.textContent).toContain('ENG-2');
});
it('Escape preserves literal text and neither closes a parent dialog nor reopens on an unrelated transaction', async () => {
  await render(fromPlainText('#ENG')); await focusEnd(); const outer = vi.fn(); container.addEventListener('keydown', outer); const event = await key('Escape'); expect(event.defaultPrevented).toBe(true); expect(outer).not.toHaveBeenCalled(); expect(editable().textContent).toBe('#ENG'); expect(container.querySelector('[role=listbox]')).toBeNull(); expect(change).not.toHaveBeenCalled();
});
it('leaves URL fragments, code, and punctuation as ordinary text', async () => {
  for (const body of [fromPlainText('https://example.test/#anchor'), fromPlainText('#ENG,'), canonicalBody({ type: 'doc', content: [{ type: 'codeBlock', content: [{ type: 'text', text: '#ENG' }] }] })]) { await render(body); await focusEnd(); expect(container.querySelector('[role=listbox]')).toBeNull(); expect(plainText(last)).toBe(plainText(body)); }
});
it('keeps unmatched text and suppresses popup Enter without accidentally posting the comment', async () => {
  vi.stubGlobal('fetch', vi.fn(() => Promise.resolve(json({ data: [], next_cursor: null, has_more: false })))); const save = vi.fn(); await render(fromPlainText('#Nobody'), save); await focusEnd(); expect(container.textContent).toContain('No issues found'); await key('Enter', { ctrlKey: true }); expect(editable().textContent).toBe('#Nobody'); expect(save).not.toHaveBeenCalled();
});
it('does not open or select suggestions during IME composition', async () => {
  const save = vi.fn(); await render(fromPlainText('#ENG'), save); await focusEnd(); await act(async () => editable().dispatchEvent(new CompositionEvent('compositionstart', { bubbles: true }))); expect(container.querySelector('[role=listbox]')).toBeNull(); await key('Enter', { isComposing: true, ctrlKey: true }); expect(save).not.toHaveBeenCalled(); expect(issueReferenceIds(last)).toEqual([]); expect(editable().textContent).toBe('#ENG');
});
it('refreshes projected references without rewriting content, dirtying the draft or replacing the caret surface', async () => {
  const body = canonicalBody({ type: 'doc', content: [{ type: 'paragraph', content: [{ type: 'issueReference', attrs: { id: first } }, { type: 'text', text: ' note' }] }] });
  await render(body, () => {}, [{ id: first, key: 'ENG-1', title: 'Visible title', available: true }]); const node = editable(); node.focus(); expect(node.textContent).toContain('ENG-1');
  await render(body, () => {}, [{ id: first, key: null, title: null, available: false }]); expect(editable()).toBe(node); expect(node.textContent).toBe('#issue note'); expect(change).not.toHaveBeenCalled(); expect(document.activeElement).toBe(node);
  await render(body, () => {}, [{ id: first, key: 'ENG-1', title: 'Old title', available: true }], { ...session, csrf_token: 'rotated' }); expect(node.textContent).toBe('#issue note'); expect(issueReferenceIds(body)).toEqual([first]);
});
