// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { CreateIssue } from '../issues/CreateIssue';
import type { RichTextEditorProps } from '../documents/RichTextEditor';
import { attachmentIds, fromPlainText, plainText } from '../documents/model';
import { hasUnsavedChanges } from '../router';
import type { Bootstrap, Issue, IssueDraft, Session } from '../types';
import type { TemplatePreview } from './types';
import type { ProjectSummary } from '../projects/types';
let editor: RichTextEditorProps;
vi.mock('../documents/Editor', () => ({ StructuredEditor: (props: RichTextEditorProps) => { editor = props; return <div aria-label={props.label}/>; } }));
vi.mock('../issues/IssueFields', () => ({ IssueFields: ({ draft, change, chooseStatus }: { draft: IssueDraft; change: (draft: IssueDraft) => void; chooseStatus: () => void }) => <><select aria-label="Priority" value={draft.priority} onChange={e => change({ ...draft, priority: Number(e.target.value) })}>{[0,1,2,3,4].map(value => <option key={value} value={value}>{value}</option>)}</select><select aria-label="Status" value={draft.status_id} onChange={e => { chooseStatus(); change({ ...draft, status_id: e.target.value }); }}><option value="ready">Ready</option><option value="started">Started</option><option value="stale">Unavailable</option></select><button type="button" onClick={() => change({ ...draft, label_ids: [] })}>Clear labels</button></> }));
vi.mock('../workflows/useIssueWorkflow', () => ({ useIssueWorkflow: () => ({ statuses: [{ id: 'ready', name: 'Ready', category: 'unstarted', is_default: true, archived_at: null, retiring_at: null }, { id: 'started', name: 'Started', category: 'started', is_default: false, archived_at: null, retiring_at: null }], currentIds: new Set(['ready', 'started']), error: null, refresh: () => {} }) }));
const session: Session = { audience: 'web', user: { id: 'u', display_name: 'Example Owner', email: 'owner@example.test' }, csrf_token: 'csrf', workspaces: [] };
const data: Bootstrap = { auth_epoch: '1', teams: [{ id: 't', name: 'Team', key: 'ENG', private: false, archived_at: null }, { id: 'other', name: 'Other', key: 'OPS', private: false, archived_at: null }], statuses: [], members: [], issues: [] };
const fileId = '11111111-1111-4111-8111-111111111111';
const project: ProjectSummary = { id: 'project', workspace_id: 'w', team_id: 't', name: 'Example project', summary: '', state: 'active', health: 'on_track', lead_id: null, lead: null, start_date: null, target_date: null, archived_at: null, version: '1', etag: '"project:1"', can_edit: true, can_manage: true, created_at: '2026-10-20T12:00:00Z', updated_at: '2026-10-20T12:00:00Z' };
const fixture = (version = '1', team = 't'): TemplatePreview => ({ template: { id: `template-${team}`, team_id: team, name: 'Bug report', version, etag: `"template:${version}"`, archived_at: null, updated_at: '2026-10-18T12:00:00Z', can_manage: true, can_use: true, title_mode: 'literal', title: 'Investigate failure', defaults: { status_id: null, priority: 2, label_ids: ['bug'] }, document: { schema_version: 1, schema_supported: true, body: fromPlainText('Steps to reproduce'), plain_text: 'Steps to reproduce' } }, materialized_title: 'Investigate failure', title_context_at: '2026-10-20T12:00:00Z', resolved_project: null, resolved_status: { id: 'ready', team_id: team, name: 'Ready', category: 'unstarted', color: 'gray', position: 0, is_default: true, archived_at: null, retiring_at: null, replacement_status_id: null, replacement_job_id: null }, labels: [], diagnostics: [], can_create: true });
const json = (value: unknown, status = 200) => new Response(JSON.stringify(value), { status });
let container: HTMLDivElement; let root: Root; let preview: TemplatePreview; let create: (options: RequestInit) => Promise<Response>; let fetchMock: ReturnType<typeof vi.fn>; let created = vi.fn((_issue: Issue) => {});
beforeEach(() => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true); vi.spyOn(window, 'confirm').mockReturnValue(false);
  HTMLDialogElement.prototype.showModal = function () { this.setAttribute('open', ''); }; HTMLDialogElement.prototype.close = function () { this.removeAttribute('open'); };
  container = document.createElement('div'); document.body.append(container); root = createRoot(container); preview = fixture(); created = vi.fn((_issue: Issue) => {}); create = async () => json({ id: 'i' });
  fetchMock = vi.fn((url: string, options: RequestInit) => {
    if (url.endsWith('/capabilities')) return Promise.resolve(json({ features: { attachments: true }, limits: { max_upload_bytes: 25 * 1024 * 1024 } }));
    if (url.includes('/templates?')) return Promise.resolve(json({ data: [preview.template], next_cursor: null, has_more: false }));
    if (url.includes('/projects?')) return Promise.resolve(json({ data: [project], next_cursor: null, has_more: false }));
    if (url.endsWith('/preview')) return Promise.resolve(json(preview));
    if (url.endsWith('/issues')) return create(options);
    throw new Error(`Unexpected request: ${url}`);
  }); vi.stubGlobal('fetch', fetchMock);
});
afterEach(async () => { await act(async () => root.unmount()); container.remove(); vi.useRealTimers(); vi.restoreAllMocks(); vi.unstubAllGlobals(); });
const buttons = (text: string) => [...container.querySelectorAll<HTMLButtonElement>('button')].filter(button => button.textContent?.trim() === text);
async function click(text: string) { const button = buttons(text).at(-1); expect(button, text).toBeTruthy(); await act(async () => button!.click()); }
async function render(close: () => void = () => {}) { await act(async () => root.render(<CreateIssue workspaceId="w" data={data} session={session} close={close} created={created}/>)); }
async function select(label: string, value: string) { const element = container.querySelector<HTMLSelectElement>(`select[aria-label="${label}"]`)!; await act(async () => { element.value = value; element.dispatchEvent(new Event('change', { bubbles: true })); }); }
const titleInput = () => [...container.querySelectorAll('label')].find(label => label.firstChild?.textContent === 'Title')!.querySelector<HTMLInputElement>('input')!;
async function title(value: string) { const input = titleInput(); await act(async () => { Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(input, value); input.dispatchEvent(new Event('input', { bubbles: true })); }); }
async function choose() { await click('Use a template'); expect(fetchMock.mock.calls.filter(([url]) => String(url).endsWith('/preview'))).toHaveLength(0); await click('Bug reportv1'); }
async function submit() { await act(async () => container.querySelector('form')!.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }))); }
const submitButton = () => container.querySelector<HTMLButtonElement>('button[type=submit]')!;

it('applies a project default and sends its reviewed version atomically with the issue', async () => {
  preview = { ...fixture(), template: { ...fixture().template, defaults: { ...fixture().template.defaults, project_id: project.id } }, resolved_project: project };
  await render(); await choose(); expect(container.textContent).toContain('Example project'); await click('Use template'); await submit();
  const write = fetchMock.mock.calls.find(([url]) => String(url).endsWith('/issues'))!;
  expect(JSON.parse((write[1] as RequestInit).body as string)).toMatchObject({ project_id: project.id, project_etag: project.etag });
});

it.each(['Merge into draft', 'Replace template fields'])('preserves an explicit project clear when applying a template: %s', async choice => {
  preview = { ...fixture(), template: { ...fixture().template, defaults: { ...fixture().template.defaults, project_id: project.id } }, resolved_project: project };
  await render(); await select('Project', project.id); await select('Project', ''); await choose(); await click(choice); await submit();
  const write = fetchMock.mock.calls.find(([url]) => String(url).endsWith('/issues'))!;
  const body = JSON.parse((write[1] as RequestInit).body as string);
  expect(body).not.toHaveProperty('project_id'); expect(body).not.toHaveProperty('project_etag');
});

it('requires an explicit choice when a template project is unavailable', async () => {
  preview = { ...fixture(), template: { ...fixture().template, defaults: { ...fixture().template.defaults, project_id: 'unavailable' } }, resolved_project: null, diagnostics: [{ field: 'project_id', code: 'unavailable', reference_id: null }], can_create: false };
  await render(); await choose(); await click('Use template'); expect(submitButton().disabled).toBe(true);
  await click('Continue without a project'); expect(submitButton().disabled).toBe(false); await submit();
  const write = fetchMock.mock.calls.find(([url]) => String(url).endsWith('/issues'))!;
  expect(JSON.parse((write[1] as RequestInit).body as string)).not.toHaveProperty('project_id');
});

it('requires an explicit refresh of a stale project version before creating', async () => {
  preview = { ...fixture(), template: { ...fixture().template, defaults: { ...fixture().template.defaults, project_id: project.id } }, resolved_project: { ...project, etag: '"project:old"' } };
  await render(); await choose(); await click('Use template'); expect(submitButton().disabled).toBe(true);
  await click('Use current project version'); expect(submitButton().disabled).toBe(false); await submit();
  const write = fetchMock.mock.calls.find(([url]) => String(url).endsWith('/issues'))!;
  expect(JSON.parse((write[1] as RequestInit).body as string).project_etag).toBe(project.etag);
});

it('preserves the draft and offers project review after an ambiguous create conflict', async () => {
  preview = { ...fixture(), template: { ...fixture().template, defaults: { ...fixture().template.defaults, project_id: project.id } }, resolved_project: project };
  await render(); await choose(); await click('Use template');
  let attempts = 0; create = async () => ++attempts === 1 ? json({ code: 'conflict', detail: 'The selection changed.' }, 409) : json({ id: 'i' });
  await submit(); expect(container.textContent).toContain('The project, template or parent may have changed');
  expect(container.textContent).not.toContain('The template changed or is unavailable'); expect(submitButton().disabled).toBe(true);
  await click('Confirm selected project'); expect(submitButton().disabled).toBe(false); await submit(); expect(created).toHaveBeenCalledOnce();
});

it('loads bodies only on selection and offers explicit choices for a metadata-only edited draft', async () => {
  await render(); await select('Priority', '3'); await choose();
  expect(container.textContent).toContain('Steps to reproduce'); expect(buttons('Use template')).toHaveLength(0);
  expect(buttons('Replace template fields')).toHaveLength(1); await click('Cancel'); expect(container.querySelector<HTMLSelectElement>('[aria-label="Priority"]')!.value).toBe('3');
  await click('Use a template'); await click('Bug reportv1'); await click('Merge into draft');
  expect(container.querySelector<HTMLSelectElement>('[aria-label="Priority"]')!.value).toBe('3'); expect(plainText(editor.value)).toBe('Steps to reproduce');
  await submit(); const write = fetchMock.mock.calls.find(([url]) => String(url).endsWith('/issues'))!; const sent = JSON.parse((write[1] as RequestInit).body as string);
  expect(sent.template).toEqual({ id: 'template-t', version: '1' }); expect(sent.status_id).toBe('ready'); expect(sent.priority).toBe(3); expect(created).toHaveBeenCalledOnce(); expect(hasUnsavedChanges()).toBe(false);
});
it('waits for pending uploads and preserves ready files when explicitly replacing template fields', async () => {
  await render(); await title('Local title'); await act(async () => { editor.onChange({ type: 'doc', content: [...fromPlainText('Local body').content, { type: 'attachment', attrs: { id: fileId } }] }); editor.attachmentsChanged?.({ pending: true, busy: true, cancelAll: async () => true }); });
  await choose(); expect(buttons('Replace template fields')[0].disabled).toBe(true); expect(container.textContent).toContain('Finish or cancel pending uploads');
  await act(async () => editor.attachmentsChanged?.({ pending: false, busy: false, cancelAll: async () => true })); await click('Replace template fields');
  expect(attachmentIds(editor.value)).toEqual([fileId]); expect(plainText(editor.value).trim()).toBe('Steps to reproduce'); expect(titleInput().value).toBe('Investigate failure');
});
it('preserves draft and pinned version after a template conflict until an explicit preview review', async () => {
  await render(); await choose(); await click('Use template'); await title('My corrected title');
  create = async () => json({ code: 'version_conflict', detail: 'Template changed', details: { current: fixture('2').template } }, 412);
  submitButton().focus(); await submit(); expect(submitButton().disabled).toBe(true); expect(document.activeElement).toBe(buttons('Bug report')[0]); expect(titleInput().value).toBe('My corrected title');
  preview = fixture('2'); await click('Bug report'); expect(container.textContent).toContain('Version 2'); await click('Keep draft with reviewed version');
  expect(titleInput().value).toBe('My corrected title'); expect(submitButton().disabled).toBe(false);
  create = async () => json({ id: 'i' }); await submit(); const writes = fetchMock.mock.calls.filter(([url]) => String(url).endsWith('/issues')); expect(JSON.parse((writes[0][1] as RequestInit).body as string).template.version).toBe('1'); expect(JSON.parse((writes[1][1] as RequestInit).body as string).template.version).toBe('2');
});
it('requires field-by-field stale-default repair and blocks unsupported content without changing the draft', async () => {
  preview = { ...fixture(), can_create: false, resolved_project: null, resolved_status: null, diagnostics: [{ field: 'status_id', code: 'archived', reference_id: 'stale' }, { field: 'label_ids', code: 'unavailable', reference_id: 'missing' }] }; preview.template.defaults = { status_id: 'stale', priority: 2, label_ids: ['missing'] };
  await render(); await choose(); await click('Use template'); expect(submitButton().disabled).toBe(true); expect(container.textContent).toContain('Status is archived'); expect(container.textContent).toContain('A label is no longer available');
  await select('Status', 'started'); expect(submitButton().disabled).toBe(true); await click('Clear labels'); expect(submitButton().disabled).toBe(false);
  preview = fixture('2'); preview.template.document = { ...preview.template.document, schema_supported: false, body: { type: 'doc', content: [{ type: 'futureBlock' }] } };
  await click('Bug report'); expect(buttons('Replace template fields')[0].disabled).toBe(true); expect(container.textContent).toContain('unsupported content'); await click('Cancel'); expect(plainText(editor.value)).toBe('Steps to reproduce');
});
it('retries an unknown issue response with exactly the original template version, body and key', async () => {
  await render(); await choose(); await click('Use template'); let count = 0; create = async () => ++count === 1 ? Promise.reject(new TypeError('lost response')) : json({ id: 'i' });
  await submit(); expect(buttons('Bug report')[0].disabled).toBe(true); expect(hasUnsavedChanges()).toBe(true); preview = fixture('2'); await submit();
  const writes = fetchMock.mock.calls.filter(([url]) => String(url).endsWith('/issues')).map(([, options]) => options as RequestInit);
  expect(writes[0].body).toBe(writes[1].body); expect(new Headers(writes[0].headers).get('Idempotency-Key')).toBe(new Headers(writes[1].headers).get('Idempotency-Key')); expect(created).toHaveBeenCalledOnce();
});

it('does not submit the underlying issue from Ctrl+Enter inside the template chooser', async () => {
  await render(); await title('Ready draft'); await choose();
  await act(async () => buttons('Merge into draft')[0].dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', ctrlKey: true, bubbles: true, cancelable: true })));
  expect(fetchMock.mock.calls.filter(([url]) => String(url).endsWith('/issues'))).toHaveLength(0); expect(created).not.toHaveBeenCalled();
  expect(titleInput().value).toBe('Ready draft'); expect(container.querySelector('.template-picker')).not.toBeNull();
});

it.each([false, true])('Escape closes only the nested chooser and preserves the outer issue dialog (dirty: %s)', async dirty => {
  const close = vi.fn(); await render(close); if (dirty) await title('Local draft');
  const trigger = buttons('Use a template')[0]; trigger.focus(); await choose();
  const dialogs = [...container.querySelectorAll<HTMLDialogElement>('dialog[open]')]; expect(dialogs).toHaveLength(2);
  const cancel = new Event('cancel', { bubbles: false, cancelable: true });
  await act(async () => dialogs[1].dispatchEvent(cancel));
  expect(cancel.defaultPrevented).toBe(true); expect(close).not.toHaveBeenCalled(); expect(container.querySelectorAll('dialog[open]')).toHaveLength(1);
  expect(container.textContent).not.toContain('Discard this issue draft?'); expect(titleInput().value).toBe(dirty ? 'Local draft' : ''); expect(document.activeElement).toBe(trigger);
});

it('applies the reviewed server title and keeps it across UTC midnight and an interrupted create', async () => {
  vi.useFakeTimers({ toFake: ['Date'] }); vi.setSystemTime(new Date('2028-02-29T23:59:59Z'));
  preview = { ...fixture(), materialized_title: 'ENG review 2028-02-29', title_context_at: '2028-02-29T23:59:58Z' };
  preview.template = { ...preview.template, title_mode: 'pattern', title: '{{team.key}} review {{date}}' };
  await render(); await choose(); expect(container.textContent).toContain('ENG review 2028-02-29'); expect(container.textContent).toContain('2028-02-29T23:59:58Z'); await click('Use template');
  vi.setSystemTime(new Date('2028-03-01T00:00:01Z'));
  let attempts = 0; create = async () => ++attempts === 1 ? Promise.reject(new TypeError('lost response')) : json({ id: 'i' });
  await submit(); vi.setSystemTime(new Date('2028-03-02T00:00:01Z')); await submit();
  const writes = fetchMock.mock.calls.filter(([url]) => String(url).endsWith('/issues')).map(([, options]) => options as RequestInit);
  expect(JSON.parse(writes[0].body as string).title).toBe('ENG review 2028-02-29'); expect(writes[0].body).toBe(writes[1].body); expect(created).toHaveBeenCalledOnce();
});

it('requires an explicit valid title repair when the resolved pattern is too long', async () => {
  preview = { ...fixture(), materialized_title: null, can_create: false, diagnostics: [{ field: 'title', code: 'title_too_long', reference_id: null }] };
  preview.template = { ...preview.template, title_mode: 'pattern', title: '{{team.name}}{{team.name}}{{team.name}}' };
  await render(); await choose(); expect(container.textContent).toContain('Title needs attention'); expect(container.textContent).toContain('exceeds 256 characters'); await click('Use template');
  expect(titleInput().value).toBe(''); expect(submitButton().disabled).toBe(true);
  await title('界'.repeat(257)); await submit(); expect(fetchMock.mock.calls.filter(([url]) => String(url).endsWith('/issues'))).toHaveLength(0);
  await title('Reviewed replacement'); expect(submitButton().disabled).toBe(false); await submit();
  const write = fetchMock.mock.calls.find(([url]) => String(url).endsWith('/issues'))![1] as RequestInit;
  expect(JSON.parse(write.body as string).title).toBe('Reviewed replacement'); expect(created).toHaveBeenCalledOnce();
});

it.each(['Merge into draft', 'Replace template fields'])('keeps explicit draft choice for pattern titles: %s', async choice => {
  preview = { ...fixture(), materialized_title: 'ENG 2028-02-29' }; preview.template = { ...preview.template, title_mode: 'pattern', title: '{{team.key}} {{date}}' };
  await render(); await title('My edited title'); await choose(); await click('Cancel'); expect(titleInput().value).toBe('My edited title');
  await click('Use a template'); await click('Bug reportv1'); await click(choice);
  expect(titleInput().value).toBe(choice === 'Merge into draft' ? 'My edited title' : 'ENG 2028-02-29');
});

it('preserves literal brace titles without evaluating placeholders', async () => {
  preview = { ...fixture(), materialized_title: '{{unknown}} {literal}' }; preview.template.title = '{{unknown}} {literal}';
  await render(); await choose(); expect(container.textContent).not.toContain('Resolved at'); await click('Use template'); await submit();
  const write = fetchMock.mock.calls.find(([url]) => String(url).endsWith('/issues'))![1] as RequestInit;
  expect(JSON.parse(write.body as string).title).toBe('{{unknown}} {literal}');
});

it.each(['Merge into draft', 'Keep draft with reviewed version'])('accepts an existing reviewed title as repair through %s', async choice => {
  await render(); await choose(); await click('Use template');
  create = async () => json({ code: 'version_conflict', detail: 'Template changed', details: { current: fixture('2').template } }, 412);
  await submit();
  preview = { ...fixture('2'), materialized_title: null, can_create: false, diagnostics: [{ field: 'title', code: 'title_too_long', reference_id: null }] }; preview.template = { ...preview.template, title_mode: 'pattern', title: '{{team.name}}'.repeat(19) };
  await click('Bug report'); await click(choice);
  expect(titleInput().value).toBe('Investigate failure'); expect(submitButton().disabled).toBe(false);
  create = async () => json({ id: 'i' }); await submit(); expect(created).toHaveBeenCalledOnce();
});
