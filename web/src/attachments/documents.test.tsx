// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { hasUnsavedChanges, navigate, useUnsavedChanges } from '../router';
import { CreateIssue } from '../issues/CreateIssue';
import { IssueDocument, type DocumentDraftState } from '../documents/IssueDocument';
import type { RichTextEditorProps } from '../documents/RichTextEditor';
import type { Bootstrap, Session } from '../types';
import { EMPTY_BODY, type RichBody } from '../documents/model';
let editor: RichTextEditorProps;
vi.mock('../documents/Editor', () => ({ StructuredEditor: (props: RichTextEditorProps) => { editor = props; return <div aria-label={props.label} />; } }));
vi.mock('../issues/IssueFields', () => ({ IssueFields: () => null }));
vi.mock('../workflows/useIssueWorkflow', () => ({ useIssueWorkflow: () => ({ statuses: [{ id: 's', name: 'Ready', category: 'unstarted', is_default: true, archived_at: null, retiring_at: null }], currentIds: new Set(['s']), error: null, refresh: () => {} }) }));
const id = '11111111-1111-4111-8111-111111111111'; const body: RichBody = { type: 'doc', content: [{ type: 'attachment', attrs: { id } }] };
const session: Session = { audience: 'web', user: { id: 'u', display_name: 'Example Owner', email: 'owner@example.test' }, csrf_token: 'csrf', workspaces: [] };
const data: Bootstrap = { auth_epoch: '1', teams: [{ id: 't', name: 'Team', key: 'ENG', private: false, archived_at: null }, { id: 'other', name: 'Other', key: 'OPS', private: false, archived_at: null }], statuses: [], members: [], issues: [] };
const record = { id: 'd', owner_id: 'i', owner_type: 'issue', schema_version: 1, schema_supported: true, body: EMPTY_BODY, plain_text: '', version: '1', etag: '"d1"', can_edit: true, created_at: '2026-10-18T12:00:00Z', updated_at: '2026-10-18T12:00:00Z' };
const json = (value: unknown) => new Response(JSON.stringify(value), { status: 200 });
let container: HTMLDivElement; let root: Root;
beforeEach(() => { vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true); vi.useFakeTimers(); HTMLDialogElement.prototype.showModal = function () { this.setAttribute('open', ''); }; HTMLDialogElement.prototype.close = function () { this.removeAttribute('open'); }; container = document.createElement('div'); document.body.append(container); root = createRoot(container); });
afterEach(async () => { await act(async () => root.unmount()); container.remove(); vi.useRealTimers(); vi.restoreAllMocks(); vi.unstubAllGlobals(); });
it('creates an issue with its ready references atomically and prevents cross-team reuse or submit while files are pending', async () => {
  const fetch = vi.fn((url: string) => Promise.resolve(json(url.endsWith('/capabilities') ? { features: { attachments: true }, limits: { max_upload_bytes: 25 * 1024 * 1024 } } : { id: 'i' }))); vi.stubGlobal('fetch', fetch); const created = vi.fn();
  await act(async () => root.render(<CreateIssue workspaceId="w" data={data} session={session} close={() => {}} created={created} />)); expect(editor.attachmentTarget).toEqual({ team_id: 't' });
  const title = container.querySelector<HTMLInputElement>('input[maxlength="256"]')!; await act(async () => { Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(title, 'Files issue'); title.dispatchEvent(new Event('input', { bubbles: true })); editor.attachmentsChanged?.({ pending: true, busy: true, cancelAll: async () => true }); });
  const create = () => container.querySelector<HTMLButtonElement>('button[type=submit]')!; expect(create().disabled).toBe(true); expect(container.querySelector<HTMLSelectElement>('.team-field select')!.disabled).toBe(true);
  await act(async () => { editor.onChange(body); editor.attachmentsChanged?.({ pending: false, busy: false, cancelAll: async () => true }); }); expect(create().disabled).toBe(false); expect(container.querySelector<HTMLSelectElement>('.team-field select')!.disabled).toBe(true);
  await act(async () => container.querySelector('form')!.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }))); const sent = fetch.mock.calls.find(([url]) => url.endsWith('/issues'))!; expect(JSON.parse((sent as unknown as [string, RequestInit])[1].body as string).document).toEqual({ schema_version: 1, body }); expect(created).toHaveBeenCalledOnce();
});
it('autosaves a ready file reference using the document ETag while keeping pending files in the wider issue guard', async () => {
  let state!: DocumentDraftState; const fetch = vi.fn((_url: string, options: RequestInit) => Promise.resolve(json(options.method === 'PATCH' ? { ...record, body, version: '2', etag: '"d2"' } : record))); vi.stubGlobal('fetch', fetch);
  await act(async () => root.render(<IssueDocument workspaceId="w" issueId="i" bodyVersion="1" session={session} readOnly={false} saved={() => {}} stateChanged={value => { state = value; }} />)); expect(editor.attachmentTarget).toEqual({ document_id: 'd' });
  await act(async () => editor.attachmentsChanged?.({ pending: true, busy: true, cancelAll: async () => true })); expect(state.dirty).toBe(true); expect(state.blocked).toBe(true); expect(await state.flush()).toBe(false);
  await act(async () => { editor.onChange(body); editor.attachmentsChanged?.({ pending: false, busy: false, cancelAll: async () => true }); }); await act(async () => vi.advanceTimersByTime(750));
  const write = fetch.mock.calls.find(([, options]) => options.method === 'PATCH')!; expect(write[0]).toContain('/documents/d'); expect(new Headers(write[1].headers).get('If-Match')).toBe('"d1"'); expect(JSON.parse(write[1].body as string)).toEqual({ schema_version: 1, body }); expect(state.dirty).toBe(false);
});

function OtherDraft({ dirty }: { dirty: boolean }) { useUnsavedChanges(dirty); return null; }
async function readyIssueDraft() {
  const title = container.querySelector<HTMLInputElement>('input[maxlength="256"]')!;
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(title, 'Files issue');
    title.dispatchEvent(new Event('input', { bubbles: true }));
    editor.onChange(body);
    editor.attachmentsChanged?.({ pending: false, busy: false, cancelAll: async () => true });
  });
}
it('releases only the confirmed create composer guard before its callback navigates', async () => {
  vi.stubGlobal('fetch', vi.fn(() => Promise.resolve(json({ id: 'created' }))));
  const confirm = vi.spyOn(window, 'confirm').mockReturnValue(false);
  history.replaceState({}, '', '/w/w/issues/old');
  let navigated: boolean | undefined;
  const created = () => { navigated = navigate('/w/w/issues/created'); };
  const render = (otherDirty: boolean) => <><OtherDraft dirty={otherDirty}/><CreateIssue workspaceId="w" data={data} session={session} close={() => {}} created={created}/></>;
  await act(async () => root.render(render(false))); await readyIssueDraft();
  expect(hasUnsavedChanges()).toBe(true);
  await act(async () => container.querySelector('form')!.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true })));
  expect(navigated).toBe(true); expect(confirm).not.toHaveBeenCalled(); expect(location.pathname).toBe('/w/w/issues/created'); expect(hasUnsavedChanges()).toBe(false);
  // A separately mounted draft still owns its guard after this composer finishes.
  await act(async () => root.render(render(true))); expect(hasUnsavedChanges()).toBe(true);
  expect(navigate('/w/w/issues/other')).toBe(false); expect(confirm).toHaveBeenCalledOnce();
  await act(async () => root.render(render(false))); expect(hasUnsavedChanges()).toBe(false);
});
it('preserves every guard on an unknown create result and clears only its own after a same-key retry', async () => {
  let attempts = 0;
  const fetch = vi.fn((url: string) => url.endsWith('/capabilities') ? Promise.resolve(json({ features: { attachments: true }, limits: { max_upload_bytes: 25 * 1024 * 1024 } })) : ++attempts === 1 ? Promise.reject(new TypeError('lost response')) : Promise.resolve(json({ id: 'created' })));
  vi.stubGlobal('fetch', fetch); const confirm = vi.spyOn(window, 'confirm').mockReturnValue(false);
  history.replaceState({}, '', '/w/w/issues/old'); let navigated: boolean | undefined;
  const created = vi.fn(() => { navigated = navigate('/w/w/issues/created'); });
  const render = (otherDirty: boolean) => <><OtherDraft dirty={otherDirty}/><CreateIssue workspaceId="w" data={data} session={session} close={() => {}} created={created}/></>;
  await act(async () => root.render(render(true))); await readyIssueDraft();
  const submit = () => act(async () => container.querySelector('form')!.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true })));
  await submit(); expect(created).not.toHaveBeenCalled(); expect(hasUnsavedChanges()).toBe(true); expect(container.textContent).toContain('Retry safely');
  await submit(); expect(created).toHaveBeenCalledOnce(); expect(navigated).toBe(false); expect(confirm).toHaveBeenCalledOnce(); expect(hasUnsavedChanges()).toBe(true);
  const writes = fetch.mock.calls.filter(([url]) => url.endsWith('/issues')) as unknown as [string, RequestInit][];
  expect(new Headers(writes[0][1].headers).get('Idempotency-Key')).toBe(new Headers(writes[1][1].headers).get('Idempotency-Key'));
  expect(writes[0][1].body).toBe(writes[1][1].body);
  await act(async () => root.render(render(false))); expect(hasUnsavedChanges()).toBe(false);
});
