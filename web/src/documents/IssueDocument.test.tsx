// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { IssueDocument, type DocumentDraftState } from './IssueDocument';
import type { Session } from '../types';
import type { StructuredDocument } from './types';
import { fromPlainText, plainText } from './model';

vi.mock('./Editor', async () => { const { MentionText } = await import('../mentions/MentionText'); return { StructuredEditor: ({ value, onChange, label, disabled, onCompositionChange }: any) => <><textarea aria-label={label} value={plainText(value)} disabled={disabled} onChange={e => onChange(fromPlainText(e.target.value))} onCompositionStart={() => onCompositionChange?.(true)} onCompositionEnd={() => onCompositionChange?.(false)} /><MentionText id="11111111-1111-4111-8111-111111111111"/></> }; });
const session: Session = { audience: 'web', user: { id: 'u', display_name: 'Example Owner', email: 'owner@example.test' }, csrf_token: 'csrf', workspaces: [{ id: 'w', name: 'Workspace', slug: 'workspace', role: 'member' }] };
const documentRecord = (text = 'Saved body', version = '1'): StructuredDocument => ({ mentions: [], issue_references: [], id: 'd', owner_id: 'i', owner_type: 'issue', body: fromPlainText(text), plain_text: text, version, etag: `"document:d:${version}"`, can_edit: true, schema_supported: true, schema_version: 1, created_at: '2026-10-18T12:00:00Z', updated_at: '2026-10-18T12:00:00Z' });
const json = (data: unknown, status = 200) => new Response(JSON.stringify(data), { status });
let container: HTMLDivElement; let root: Root; let state: DocumentDraftState; let saved: () => void;
beforeEach(() => { vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true); vi.useFakeTimers(); container = document.createElement('div'); document.body.append(container); root = createRoot(container); saved = vi.fn(); });
afterEach(async () => { await act(async () => root.unmount()); container.remove(); vi.useRealTimers(); vi.unstubAllGlobals(); });
async function render(bodyVersion = '1', token = 'csrf') { await act(async () => root.render(<IssueDocument workspaceId="w" issueId="i" bodyVersion={bodyVersion} session={{ ...session, csrf_token: token }} readOnly={false} stateChanged={value => { state = value; }} saved={saved} />)); }
function editor() { return container.querySelector('textarea')!; }
async function type(value: string) { await act(async () => { Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value')!.set!.call(editor(), value); editor().dispatchEvent(new Event('input', { bubbles: true })); }); }
function button(label: string) { const button = [...container.querySelectorAll('button')].find(item => item.textContent === label); if (!button) throw new Error(`Missing ${label}`); return button; }
async function click(label: string) { await act(async () => button(label).click()); }

describe('independent description saves', () => {
  it('autosaves after750ms, keeps typing during a pending save, and never disables the focused editor for its acknowledgement', async () => {
    let finish!: (value: Response) => void; const fetch = vi.fn((_url: string, options: RequestInit) => options.method === 'PATCH' ? new Promise<Response>(resolve => { finish = resolve; }) : Promise.resolve(json(documentRecord()))); vi.stubGlobal('fetch', fetch);
    await render(); editor().focus(); await type('First change'); await act(async () => vi.advanceTimersByTime(749)); expect(fetch.mock.calls.filter(([,o]) => o.method === 'PATCH')).toHaveLength(0);
    await act(async () => vi.advanceTimersByTime(1)); expect(state.busy).toBe(true); expect(editor().disabled).toBe(false); await type('Typed while saving');
    await act(async () => finish(json(documentRecord('First change','2')))); expect(editor().value).toBe('Typed while saving'); expect(state.dirty).toBe(true); expect(document.activeElement).toBe(editor());
    let refresh!: (value:Response)=>void; fetch.mockImplementation(() => new Promise<Response>(resolve => { refresh=resolve; })); await render('2'); expect(editor().disabled).toBe(false); expect(document.activeElement).toBe(editor());
    await act(async () => refresh(json(documentRecord('First change','2')))); expect(editor().value).toBe('Typed while saving');
  });
  it('keeps composition out of autosave and flushes only after composition ends', async () => {
    const fetch=vi.fn((_url:string,options:RequestInit)=>Promise.resolve(json(options.method==='PATCH'?documentRecord('Composed','2'):documentRecord()))); vi.stubGlobal('fetch',fetch); await render();
    await act(async()=>editor().dispatchEvent(new CompositionEvent('compositionstart',{bubbles:true}))); await type('Composed'); await act(async()=>vi.advanceTimersByTime(1000)); expect(fetch).toHaveBeenCalledTimes(1);
    await act(async()=>editor().dispatchEvent(new CompositionEvent('compositionend',{bubbles:true}))); await act(async()=>vi.advanceTimersByTime(750)); expect(fetch).toHaveBeenCalledTimes(2);
  });
  it('requires explicit conflict review and never auto-overwrites the latest text', async () => {
    const remote=documentRecord('Other author','2'); const fetch=vi.fn((_url:string,options:RequestInit)=>Promise.resolve(options.method==='PATCH'?json({code:'version_conflict',detail:'Changed',details:{current:remote}},412):json(documentRecord()))); vi.stubGlobal('fetch',fetch); await render(); await type('My text'); await click('Save text now'); expect(container.textContent).toContain('Other author'); expect(editor().value).toBe('My text'); await click('Keep my draft for review'); await act(async()=>vi.advanceTimersByTime(2000)); expect(fetch).toHaveBeenCalledTimes(2); expect(editor().value).toBe('My text');
    fetch.mockImplementation((_url,options)=>{ expect(new Headers(options.headers).get('If-Match')).toBe(remote.etag); return Promise.resolve(json(documentRecord('My text','3'))); }); await click('Save text now'); expect(state.dirty).toBe(false);
  });
  it('preserves a local draft on an unsupported remote refresh and pauses all writes', async () => {
    const fetch=vi.fn().mockResolvedValue(json(documentRecord())); vi.stubGlobal('fetch',fetch); await render(); await type('Local draft'); const element=editor(); element.focus();
    fetch.mockResolvedValue(json({...documentRecord('Future content','2'),schema_supported:false,schema_version:2,body:{type:'future'}})); await render('2','rotated'); expect(editor()).toBe(element); expect(editor().value).toBe('Local draft'); expect(container.textContent).toContain('saving is paused'); expect(state.blocked).toBe(true); await act(async()=>vi.advanceTimersByTime(2000)); expect(fetch).toHaveBeenCalledTimes(2); expect(button('Save text now').disabled).toBe(true);
    await click('Discard text'); expect(container.querySelector('textarea')).toBeNull(); expect(container.textContent).toContain('Future content'); expect(container.textContent).not.toContain('Local draft'); expect(state.dirty).toBe(false); await act(async () => vi.advanceTimersByTime(2000)); expect(fetch).toHaveBeenCalledTimes(2);
  });
  it('does not roll a completed save back when an earlier refresh arrives late', async () => {
    let refresh!: (value:Response)=>void; const fetch=vi.fn((_url:string,options:RequestInit)=>Promise.resolve(json(options.method==='PATCH'?documentRecord('New body','3'):documentRecord()))); vi.stubGlobal('fetch',fetch); await render(); await type('New body');
    fetch.mockImplementation((_url,options)=>options.method==='PATCH'?Promise.resolve(json({...documentRecord('New body','3'),mentions:[{id:'11111111-1111-4111-8111-111111111111',display_name:'Fresh projection',available:true}]})):new Promise(resolve=>{refresh=resolve;})); await render('2'); await click('Save text now'); await act(async()=>refresh(json({...documentRecord('Earlier body','2'),can_edit:false,mentions:[{id:'11111111-1111-4111-8111-111111111111',display_name:'Old projection',available:true}]}))); expect(editor().value).toBe('New body'); expect(state.dirty).toBe(false); expect(container.textContent).toContain('@Fresh projection'); expect(container.textContent).not.toContain('@Old projection');
  });
  it('reconciles an unknown write without losing text typed after the request', async () => {
    const fetch=vi.fn((_url:string,options:RequestInit)=>options.method==='PATCH'?Promise.reject(new TypeError('Offline')):Promise.resolve(json(documentRecord()))); vi.stubGlobal('fetch',fetch); await render(); await type('Submitted'); await click('Save text now'); await type('Newer local text'); expect(container.textContent).toContain('Save not confirmed');
    fetch.mockResolvedValue(json(documentRecord('Submitted','2'))); await click('Check save result'); expect(editor().value).toBe('Newer local text'); expect(state.dirty).toBe(true); expect(state.blocked).toBe(false);
  });
  it('hides denied content immediately and preserves the draft for a confirmed same-user refresh', async () => {
    const fetch=vi.fn().mockResolvedValue(json(documentRecord())); vi.stubGlobal('fetch',fetch); await render(); await type('Private draft'); fetch.mockResolvedValue(json({code:'not_found',detail:'Not found'},404)); await render('2','rotated'); expect(container.textContent).not.toContain('Saved body'); expect(container.querySelector('textarea')).toBeNull();
    fetch.mockResolvedValue(json(documentRecord())); await render('2','rotated-again'); expect(editor().value).toBe('Private draft');
  });
});
