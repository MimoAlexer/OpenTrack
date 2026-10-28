// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { CreateIssue } from './CreateIssue';
import { useAttachments, type AttachmentContextValue } from '../attachments/context';
import type { Attachment } from '../attachments/types';
import type { RichTextEditorProps } from '../documents/RichTextEditor';
import { attachmentIds, fromPlainText, plainText, type RichBody } from '../documents/model';
import { hasUnsavedChanges } from '../router';
import type { TemplatePreview } from '../templates/types';
import type { Bootstrap, Issue, Session, Status } from '../types';
import type { OrderMaintenance } from './IssueOrdering';

let editor: RichTextEditorProps; let attachmentContext: AttachmentContextValue | null;
vi.mock('../documents/Editor', () => ({ StructuredEditor: (props: RichTextEditorProps) => { editor = props; attachmentContext = useAttachments(); return <div aria-label={props.label} />; } }));
vi.mock('./IssueFields', () => ({ IssueFields: () => null }));
vi.mock('../workflows/useIssueWorkflow', () => ({ useIssueWorkflow: () => ({ statuses: [status], currentIds: new Set([status.id]), error: null, refresh: vi.fn() }) }));
vi.mock('../templates/TemplateChooser', () => ({ TemplateChooser: ({ disabled, needsReview, apply }: { disabled: boolean; needsReview: boolean; apply: (value: TemplatePreview, mode: 'merge' | 'replace') => string | null }) => <><button type="button" disabled={disabled} onClick={() => apply(template, 'replace')}>Apply fixture template</button>{needsReview && <p role="alert">Template needs review</p>}</> }));
const session: Session = { audience: 'web', user: { id: 'owner', display_name: 'Example Owner', email: 'owner@example.test' }, csrf_token: 'csrf', workspaces: [{ id: 'workspace', name: 'Example Workspace', slug: 'example', role: 'owner' }] };
const status: Status = { id: 'status', team_id: 'team', name: 'Ready', category: 'unstarted', color: 'slate', position: 0, is_default: true, archived_at: null, retiring_at: null, replacement_status_id: null, replacement_job_id: null };
const data: Bootstrap = { auth_epoch: '1', teams: [{ id: 'team', name: 'Example Team', key: 'EX', private: false, archived_at: null }, { id: 'other-team', name: 'Other Team', key: 'OTHER', private: false, archived_at: null }], statuses: [status], members: [session.user], issues: [] };
const saved: Issue = { id: 'created', workspace_id: 'workspace', team_id: 'team', key: 'EX-2', title: 'Retained draft', description: 'Retained details', body_version: '1', status_id: status.id, priority: 0, assignee_id: null, assignee: null, label_ids: [], labels: [], etag: '"issue:created:1"', estimate: null, due_date: null, version: '1', created_at: '2026-10-25T10:00:00Z', updated_at: '2026-10-25T10:00:00Z', archived_at: null };
const parent: Issue = { ...saved, id: 'parent', key: 'EX-1', title: 'Parent issue', etag: '"issue:parent:1"' };
const fileId = '11111111-1111-4111-8111-111111111111';
const file: Attachment = { id: fileId, document_id: null, original_name: 'evidence.txt', size_bytes: 4, sha256: 'a'.repeat(64), state: 'ready', detected_mime: 'text/plain', scan_status: 'not_scanned', version: '1', etag: '"attachment:1"', expires_at: null, created_at: saved.created_at, can_delete: true };
const readyBody: RichBody = { type: 'doc', content: [{ type: 'paragraph', content: [{ type: 'text', text: 'Retained details' }] }, { type: 'attachment', attrs: { id: fileId } }] };
const template: TemplatePreview = { template: { id: 'template', team_id: 'team', name: 'Example template', version: '1', etag: '"template:1"', archived_at: null, updated_at: saved.created_at, can_manage: true, can_use: true, title_mode: 'literal', title: 'Template draft', defaults: { status_id: null, priority: 0, label_ids: [] }, document: { schema_version: 1, schema_supported: true, body: fromPlainText('Template details'), plain_text: 'Template details' } }, materialized_title: 'Template draft', title_context_at: saved.created_at, resolved_project: null, resolved_status: status, labels: [], diagnostics: [], can_create: true };
const job: OrderMaintenance = { id: 'job', team_id: 'team', status_id: status.id, state: 'queued', phase: 'staging', staged_count: '0', applied_count: '0', total_count: null, version: '1', error_code: null, can_resume: false, etag: '"issue-order-maintenance:job:1:false:authority"' };
const json = (value: unknown, code = 200) => new Response(JSON.stringify(value), { status: code, headers: { 'Content-Type': 'application/json' } });
const maintenance = (details: unknown = { applied: false, maintenance: [job] }) => json({ code: 'issue_order_maintenance', detail: 'Issue order maintenance is active; this change was not applied', details }, 409);
let container: HTMLDivElement; let root: Root; let completed: boolean;
let create: (init: RequestInit) => Promise<Response>; let fetch: ReturnType<typeof vi.fn>;
beforeEach(() => {
  vi.useFakeTimers(); vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true);
  HTMLDialogElement.prototype.showModal = function () { this.setAttribute('open', ''); }; HTMLDialogElement.prototype.close = function () { this.removeAttribute('open'); };
  container = document.createElement('div'); document.body.append(container); root = createRoot(container); completed = false; attachmentContext = null;
  create = () => Promise.resolve(maintenance());
  fetch = vi.fn((url: string, init: RequestInit) => {
    if (url.endsWith('/capabilities')) return Promise.resolve(json({ features: { attachments: true }, limits: { max_upload_bytes: 25 * 1024 * 1024 } }));
    if (url.includes('/projects?')) return Promise.resolve(json({ data: [], next_cursor: null, has_more: false }));
    if (url.endsWith('/issues') && init.method === 'POST') return create(init);
    if (url.endsWith('/issue-order-maintenance/job')) return Promise.resolve(json(completed ? { ...job, phase: 'completed', state: 'succeeded', staged_count: '501', applied_count: '501', total_count: '501', version: '3', etag: '"completed-job"' } : job));
    throw new Error(`Unexpected scoped request ${url}`);
  }); vi.stubGlobal('fetch', fetch);
});
afterEach(async () => { await act(async () => root.unmount()); container.remove(); vi.useRealTimers(); vi.restoreAllMocks(); vi.unstubAllGlobals(); });
async function render(close = vi.fn(), created = vi.fn(), parentIssue?: Issue) { await act(async () => root.render(<CreateIssue workspaceId="workspace" data={data} session={session} close={close} created={created} parent={parentIssue} />)); }
function button(label: string) { const value = [...container.querySelectorAll<HTMLButtonElement>('button')].find(item => item.textContent === label); if (!value) throw new Error(`Missing button ${label}`); return value; }
const titleInput = () => container.querySelector<HTMLInputElement>('input[placeholder="What needs to change?"]')!;
async function title(value: string) { const input = titleInput(); await act(async () => { Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(input, value); input.dispatchEvent(new Event('input', { bubbles: true })); }); }
async function click(label: string) { await act(async () => button(label).click()); }
async function submit() { await act(async () => container.querySelector('form.create-form')!.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }))); }
const writes = () => fetch.mock.calls.filter(([url, init]) => String(url).endsWith('/issues') && (init as RequestInit).method === 'POST') as [string, RequestInit][];
async function readyDraft() {
  await title('Retained draft');
  await act(async () => { editor.onChange(readyBody); editor.attachmentsChanged?.({ pending: false, busy: false, cancelAll: async () => true }); attachmentContext!.rememberReady(file); });
}

it('preserves the draft and ready attachment context while maintenance leaves editing and cancellation available', async () => {
  const close = vi.fn(); const created = vi.fn(); await render(close, created); await readyDraft();
  const binding = attachmentContext!.binding; await submit();
  expect(container.textContent).toContain('No issue was created by this request'); expect(titleInput().value).toBe('Retained draft'); expect(editor.value).toEqual(readyBody); expect(editor.disabled).toBe(false);
  expect(attachmentContext!.binding).toBe(binding); expect(attachmentContext!.readySeed(fileId)).toEqual(file); expect(editor.attachmentTarget).toEqual({ team_id: 'team' });
  expect(container.querySelector<HTMLFieldSetElement>('fieldset')!.disabled).toBe(false); expect(button('Cancel').disabled).toBe(false); expect(button('Retry create').disabled).toBe(false);
  expect(container.querySelector<HTMLSelectElement>('.team-field select')!.disabled).toBe(true); expect(hasUnsavedChanges()).toBe(true); expect(created).not.toHaveBeenCalled();
  await click('Cancel'); expect(container.textContent).toContain('Discard this issue draft?'); expect(close).not.toHaveBeenCalled(); await click('Keep editing');
  expect(titleInput().value).toBe('Retained draft'); expect(attachmentIds(editor.value)).toEqual([fileId]); expect(attachmentContext!.readySeed(fileId)).toEqual(file);
});

it('never creates automatically after maintenance completes and explicitly retries an unchanged ready draft with the same key and body', async () => {
  const created = vi.fn(); await render(vi.fn(), created); await readyDraft(); await submit();
  completed = true; await act(async () => { await vi.advanceTimersByTimeAsync(2000); });
  expect(container.textContent).toContain('Order preparation complete.'); expect(writes()).toHaveLength(1); expect(created).not.toHaveBeenCalled();
  create = () => Promise.resolve(json(saved)); await click('Retry create'); const requests = writes(); expect(requests).toHaveLength(2);
  expect(requests[1][1].body).toBe(requests[0][1].body); expect(new Headers(requests[1][1].headers).get('Idempotency-Key')).toBe(new Headers(requests[0][1].headers).get('Idempotency-Key'));
  expect(JSON.parse(requests[1][1].body as string).document).toEqual({ schema_version: 1, body: readyBody }); expect(created).toHaveBeenCalledExactlyOnceWith(saved);
});

it('withdraws prior noncreation confirmation before retry and freezes an unchanged attached draft after response loss', async () => {
  const close = vi.fn(); const created = vi.fn(); await render(close, created); await readyDraft(); await submit();
  const binding = attachmentContext!.binding; expect(container.textContent).toContain('No issue was created by this request');
  let interrupt!: (error: Error) => void; create = () => new Promise((_resolve, reject) => { interrupt = reject; });
  await click('Retry create'); expect(writes()).toHaveLength(2); expect(container.textContent).not.toContain('No issue was created by this request');
  expect(button('Cancel').disabled).toBe(true); expect(container.querySelector<HTMLFieldSetElement>('fieldset')!.disabled).toBe(true);
  await act(async () => interrupt(new TypeError('Response lost')));
  expect(container.textContent).toContain('this issue may already exist'); expect(container.textContent).not.toContain('No issue was created by this request');
  expect(button('Cancel').disabled).toBe(true); expect(button('Retry safely').disabled).toBe(false); expect(editor.disabled).toBe(true);
  expect(container.querySelector<HTMLFieldSetElement>('fieldset')!.disabled).toBe(true); expect(titleInput().value).toBe('Retained draft');
  expect(editor.value).toEqual(readyBody); expect(attachmentContext!.binding).toBe(binding); expect(attachmentContext!.readySeed(fileId)).toEqual(file);
  expect(close).not.toHaveBeenCalled(); expect(created).not.toHaveBeenCalled(); expect(hasUnsavedChanges()).toBe(true);
  create = () => Promise.resolve(json(saved)); await click('Retry safely'); const requests = writes(); expect(requests).toHaveLength(3);
  for (const [, request] of requests.slice(1)) {
    expect(request.body).toBe(requests[0][1].body); expect(new Headers(request.headers).get('Idempotency-Key')).toBe(new Headers(requests[0][1].headers).get('Idempotency-Key'));
  }
  expect(JSON.parse(requests[2][1].body as string).document).toEqual({ schema_version: 1, body: readyBody }); expect(created).toHaveBeenCalledExactlyOnceWith(saved);
});

it('uses a new request key and body only after the user edits a confirmed unapplied draft', async () => {
  const created = vi.fn(); await render(vi.fn(), created); await readyDraft(); await submit(); await title('Explicitly revised draft');
  expect(writes()).toHaveLength(1); expect(container.textContent).not.toContain('No issue was created by this request'); expect(editor.value).toEqual(readyBody);
  create = () => Promise.resolve(json({ ...saved, title: 'Explicitly revised draft' })); await submit(); const requests = writes(); expect(requests).toHaveLength(2);
  expect(new Headers(requests[1][1].headers).get('Idempotency-Key')).not.toBe(new Headers(requests[0][1].headers).get('Idempotency-Key'));
  expect(JSON.parse(requests[0][1].body as string).title).toBe('Retained draft'); expect(JSON.parse(requests[1][1].body as string).title).toBe('Explicitly revised draft');
  expect(JSON.parse(requests[1][1].body as string).document).toEqual({ schema_version: 1, body: readyBody }); expect(created).toHaveBeenCalledOnce();
});

it.each(['template', 'parent'] as const)('keeps maintenance distinct from a real %s conflict without suppressing later reference review', async context => {
  await render(vi.fn(), vi.fn(), context === 'parent' ? parent : undefined);
  if (context === 'template') await click('Apply fixture template'); else await title('Subissue draft');
  await submit(); expect(container.textContent).toContain('No issue was created by this request'); expect(container.textContent).not.toContain('Review the parent issue.'); expect(container.textContent).not.toContain('Template needs review');
  expect(button('Retry create').disabled).toBe(false); create = () => Promise.resolve(json({ code: 'conflict', detail: 'Reference changed' }, 409)); await click('Retry create');
  expect(container.textContent).toContain(context === 'template' ? 'Template needs review' : 'Review the parent issue.'); expect(writes()).toHaveLength(2);
  expect(new Headers(writes()[1][1].headers).get('Idempotency-Key')).toBe(new Headers(writes()[0][1].headers).get('Idempotency-Key'));
  expect(JSON.parse(writes()[0][1].body as string)).toMatchObject(context === 'template' ? { template: { id: 'template', version: '1' } } : { parent_id: parent.id, parent_etag: parent.etag });
});

it.each(['applied true', 'wrong team', 'missing counts'] as const)('keeps malformed maintenance %s uncertain and frozen until an identical safe retry resolves it', async malformed => {
  create = () => Promise.resolve(maintenance(malformed === 'applied true' ? { applied: true, maintenance: [job] } : { applied: false, maintenance: [malformed === 'wrong team' ? { ...job, team_id: 'other-team' } : { id: job.id, team_id: job.team_id }] }));
  const created = vi.fn(); await render(vi.fn(), created, parent); await readyDraft(); await submit();
  expect(container.textContent).toContain('this issue may already exist'); expect(container.textContent).not.toContain('No issue was created by this request'); expect(container.textContent).not.toContain('Review the parent issue.');
  expect(container.querySelector<HTMLFieldSetElement>('fieldset')!.disabled).toBe(true); expect(editor.disabled).toBe(true); expect(button('Cancel').disabled).toBe(true); expect(button('Retry safely').disabled).toBe(false);
  expect(titleInput().value).toBe('Retained draft'); expect(plainText(editor.value)).toContain('Retained details'); expect(attachmentIds(editor.value)).toEqual([fileId]); expect(hasUnsavedChanges()).toBe(true);
  create = () => Promise.resolve(json(saved)); await click('Retry safely'); const requests = writes(); expect(requests).toHaveLength(2);
  expect(requests[1][1].body).toBe(requests[0][1].body); expect(new Headers(requests[1][1].headers).get('Idempotency-Key')).toBe(new Headers(requests[0][1].headers).get('Idempotency-Key')); expect(created).toHaveBeenCalledExactlyOnceWith(saved);
});
