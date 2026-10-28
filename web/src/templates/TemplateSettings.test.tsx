// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { TemplateSettings } from './TemplateSettings';
import type { RichTextEditorProps } from '../documents/RichTextEditor';
import { fromPlainText, plainText } from '../documents/model';
import { hasUnsavedChanges } from '../router';
import type { Session } from '../types';
import type { TeamDetail } from '../settings/types';
import type { IssueTemplate, TemplatePreview } from './types';
import type { ProjectSummary } from '../projects/types';
let editor: RichTextEditorProps;
vi.mock('../documents/Editor', () => ({ StructuredEditor: (props: RichTextEditorProps) => { editor = props; return <div aria-label={props.label}/>; } }));
vi.mock('../taxonomy/LabelPicker', () => ({ LabelPicker: () => null }));
vi.mock('../workflows/useWorkflow', () => ({ useWorkflow: () => ({ data: { statuses: [{ id: 'ready', name: 'Ready', category: 'unstarted', archived_at: null, retiring_at: null }] } }) }));
const session: Session = { audience: 'web', user: { id: 'u', display_name: 'Example Owner', email: 'owner@example.test' }, csrf_token: 'csrf', workspaces: [] };
const team = { id: 't', name: 'Team', key: 'ENG', private: false, archived_at: null, can_manage: true } as TeamDetail;
const project: ProjectSummary = { id: 'project', workspace_id: 'w', team_id: 't', name: 'Example project', summary: '', state: 'active', health: 'on_track', lead_id: null, lead: null, start_date: null, target_date: null, archived_at: null, version: '1', etag: '"project:1"', can_edit: true, can_manage: true, created_at: '2026-10-20T12:00:00Z', updated_at: '2026-10-20T12:00:00Z' };
const record = (version = '1'): IssueTemplate => ({ id: 'template', team_id: 't', name: 'Bug report', version, etag: `"template:${version}"`, archived_at: null, updated_at: '2026-10-18T12:00:00Z', can_manage: true, can_use: true, title_mode: 'literal', title: 'Original title', defaults: { status_id: null, priority: 0, label_ids: [] }, document: { schema_version: 1, schema_supported: true, body: fromPlainText('Original body'), plain_text: 'Original body' } });
const json = (value: unknown, status = 200) => new Response(JSON.stringify(value), { status });
let container: HTMLDivElement; let root: Root; let current: IssueTemplate; let command: (options: RequestInit) => Promise<Response>; let fetchMock: ReturnType<typeof vi.fn>;
beforeEach(() => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true); HTMLDialogElement.prototype.showModal = function () { this.setAttribute('open', ''); }; HTMLDialogElement.prototype.close = function () { this.removeAttribute('open'); };
  container = document.createElement('div'); document.body.append(container); root = createRoot(container); current = record(); command = async () => json(current);
  fetchMock = vi.fn((url: string, options: RequestInit) => {
    if (options.method === 'PATCH' || options.method === 'POST') return command(options);
    if (url.includes('/projects?')) return Promise.resolve(json({ data: [project], next_cursor: null, has_more: false }));
    if (url.endsWith('/preview')) return Promise.resolve(json({ template: current, materialized_title: current.title, title_context_at: '2026-10-20T12:00:00Z', resolved_project: null, resolved_status: null, labels: [], diagnostics: [], can_create: true } satisfies TemplatePreview));
    return Promise.resolve(json({ data: [current], next_cursor: null, has_more: false }));
  }); vi.stubGlobal('fetch', fetchMock);
});
afterEach(async () => { await act(async () => root.unmount()); container.remove(); vi.restoreAllMocks(); vi.unstubAllGlobals(); });
async function render(manage = true, account = session) { await act(async () => root.render(<TemplateSettings workspaceId="w" team={{ ...team, can_manage: manage }} session={account}/>)); }
const button = (label: string) => [...container.querySelectorAll<HTMLButtonElement>('button')].find(item => item.getAttribute('aria-label') === label || item.textContent?.trim() === label)!;
async function click(label: string) { expect(button(label), label).toBeTruthy(); await act(async () => button(label).click()); }
function input(label: string) { return [...container.querySelectorAll('label')].find(item => item.textContent?.startsWith(label))!.querySelector<HTMLInputElement>('input')!; }
async function fill(label: string, value: string) { const element = input(label); await act(async () => { Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(element, value); element.dispatchEvent(new Event('input', { bubbles: true })); }); }
async function submit() { await act(async () => container.querySelector('form')!.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }))); }

it('preserves the selected default project after a rejected template save', async () => {
  await render(); await click('New template'); await fill('Template name', 'Project template');
  const select = container.querySelector<HTMLSelectElement>('select[aria-label="Default project"]')!;
  await act(async () => { select.value = project.id; select.dispatchEvent(new Event('change', { bubbles: true })); });
  command = async options => { expect(JSON.parse(options.body as string).defaults.project_id).toBe(project.id); return json({ code: 'validation_failed', detail: 'Please retry.' }, 422); };
  await submit(); expect(select.value).toBe(project.id); expect(container.textContent).toContain('Please retry.'); expect(hasUnsavedChanges()).toBe(true);
});

it('requires an explicit clear for a redacted unavailable default before saving', async () => {
  fetchMock.mockImplementation((url: string, options: RequestInit) => {
    if (options.method === 'PATCH') return command(options);
    if (url.endsWith('/preview')) return Promise.resolve(json({ template: current, materialized_title: current.title, title_context_at: '2026-10-20T12:00:00Z', resolved_project: null, resolved_status: null, labels: [], diagnostics: [{ field: 'project_id', code: 'unavailable', reference_id: null }], can_create: false }));
    return Promise.resolve(json({ data: url.includes('/projects?') ? [project] : [current], next_cursor: null, has_more: false }));
  });
  await render(); await click('Edit Bug report'); expect(button('Save template').disabled).toBe(true);
  expect(container.textContent).toContain('default project is unavailable'); await click('Continue without a project');
  command = async options => { expect(JSON.parse(options.body as string).defaults).not.toHaveProperty('project_id'); return json(current); };
  expect(button('Save template').disabled).toBe(false); await submit();
});

it('conceals the editor on project directory denial while preserving its draft for recovery', async () => {
  const normal = fetchMock.getMockImplementation()!; let denied = false;
  fetchMock.mockImplementation((url: string, options: RequestInit) => denied && url.includes('/projects?') ? Promise.resolve(json({ code: 'forbidden', detail: 'Unavailable' }, 403)) : normal(url, options));
  await render(); await click('Edit Bug report'); await fill('Issue title', 'Unsaved private title');
  const select = container.querySelector<HTMLSelectElement>('select[aria-label="Default project"]')!;
  await act(async () => { select.value = project.id; select.dispatchEvent(new Event('change', { bubbles: true })); });
  denied = true; await click('Refresh projects');
  const dialog = container.querySelector('dialog[open]')!; expect(dialog.textContent).not.toContain('Unsaved private title'); expect(dialog.textContent).not.toContain('Original body'); expect(dialog.querySelector('input')).toBeNull();
  denied = false; await click('Try again'); expect(input('Issue title').value).toBe('Unsaved private title'); expect(hasUnsavedChanges()).toBe(true);
});

it('retries the original uncertain template request after project data becomes unavailable', async () => {
  await render(); await click('New template'); await fill('Template name', 'Pending template');
  const select = container.querySelector<HTMLSelectElement>('select[aria-label="Default project"]')!;
  await act(async () => { select.value = project.id; select.dispatchEvent(new Event('change', { bubbles: true })); });
  const attempts: { body: string; key: string | null }[] = [];
  command = async options => { attempts.push({ body: options.body as string, key: new Headers(options.headers).get('Idempotency-Key') }); if (attempts.length === 1) throw new TypeError('lost response'); return json(current); };
  await submit(); expect(button('Retry safely')).toBeTruthy();
  const normal = fetchMock.getMockImplementation()!;
  fetchMock.mockImplementation((url: string, options: RequestInit) => url.includes('/projects?') ? Promise.resolve(json({ code: 'unavailable', detail: 'Unavailable' }, 503)) : normal(url, options));
  await render(true, { ...session, csrf_token: 'new' }); expect(button('Retry safely').disabled).toBe(false); await submit();
  expect(attempts).toHaveLength(2); expect(attempts[1]).toEqual(attempts[0]);
});

it.each(['create', 'edit'])('preserves supplementary template %s drafts after server rejection and submits repaired scalar boundaries once', async mode => {
  const name = '\u{1F680}'.repeat(100); const title = '\u{1F30D}'.repeat(256);
  await render(); await click(mode === 'create' ? 'New template' : 'Edit Bug report');
  await fill('Template name', name); await fill('Issue title', title);
  const original = structuredClone(current);
  for (const field of ['name', 'title'] as const) {
    const label = field === 'name' ? 'Template name' : 'Issue title';
    const value = (field === 'name' ? name : title) + '\u{1F31F}';
    await fill(label, value);
    command = async options => {
      expect(JSON.parse(options.body as string)).toMatchObject({ name, title, [field]: value, title_mode: 'literal' });
      return json({ code: 'validation_failed', detail: `${label} must contain fewer characters` }, 422);
    };
    await submit();
    expect(input(label).value).toBe(value); expect(input(field === 'name' ? 'Issue title' : 'Template name').value).toBe(field === 'name' ? title : name);
    expect(container.textContent).toContain(`${label} must contain fewer characters`);
    expect(hasUnsavedChanges()).toBe(true); expect(container.querySelector('form')).not.toBeNull(); expect(current).toEqual(original);
    await fill(label, field === 'name' ? name : title);
  }
  command = async options => {
    expect(JSON.parse(options.body as string)).toMatchObject({ name, title, title_mode: 'literal' });
    return json({ ...current, name, title, version: mode === 'create' ? '1' : '2' });
  };
  await submit(); expect(container.querySelector('form')).toBeNull(); expect(hasUnsavedChanges()).toBe(false);
  const writes = fetchMock.mock.calls.filter(([, options]) => (options as RequestInit).method === (mode === 'create' ? 'POST' : 'PATCH'));
  expect(writes).toHaveLength(3);
  if (mode === 'edit') for (const [, options] of writes) expect(new Headers((options as RequestInit).headers).get('If-Match')).toBe(original.etag);
});

it('keeps whitespace-only names unsaveable and trims a valid name while allowing an empty literal title', async () => {
  await render(); await click('New template'); await fill('Template name', '   ');
  expect(button('Create template').disabled).toBe(true);
  await fill('Template name', '  Checklist  '); expect(button('Create template').disabled).toBe(false);
  command = async options => { expect(JSON.parse(options.body as string)).toMatchObject({ name: 'Checklist', title: '', title_mode: 'literal' }); return json(current); };
  await submit(); expect(container.querySelector('form')).toBeNull(); expect(hasUnsavedChanges()).toBe(false);
});

it('uses server management flags, loads the body lazily, and exposes only preview to a reader', async () => {
  current = { ...current, can_manage: false }; await render(false); expect(button('New template')).toBeUndefined(); expect(button('Edit Bug report')).toBeUndefined(); expect(fetchMock.mock.calls.every(([url]) => !String(url).endsWith('/preview'))).toBe(true);
  await click('Preview Bug report'); expect(container.textContent).toContain('Original body'); expect(container.querySelector('form')).toBeNull();
});
it('preserves a failed new-template draft and retries unknown creation with the same key and body', async () => {
  let attempts = 0; command = async () => ++attempts === 1 ? Promise.reject(new TypeError('lost response')) : json(current);
  await render(); await click('New template'); expect(document.activeElement).toBe(input('Template name')); await fill('Template name', 'Checklist'); await act(async () => editor.onChange(fromPlainText('Review the change')));
  expect(editor.attachmentTarget).toBeUndefined(); await submit(); expect(container.textContent).toContain('Retry safely'); expect(hasUnsavedChanges()).toBe(true); expect(input('Template name').value).toBe('Checklist');
  await submit(); const writes = fetchMock.mock.calls.filter(([, options]) => (options as RequestInit).method === 'POST').map(([, options]) => options as RequestInit);
  expect(writes[0].body).toBe(writes[1].body); expect(new Headers(writes[0].headers).get('Idempotency-Key')).toBe(new Headers(writes[1].headers).get('Idempotency-Key')); expect(hasUnsavedChanges()).toBe(false); expect(container.querySelector('form')).toBeNull();
});
it('shows latest versus local text after 412 and preserves edits until explicitly reviewed', async () => {
  await render(); await click('Edit Bug report'); await fill('Issue title', 'Local title'); await act(async () => editor.onChange(fromPlainText('Local body')));
  current = { ...record('2'), title: 'Remote title', document: { ...current.document, body: fromPlainText('Remote body'), plain_text: 'Remote body' } }; command = async () => json({ code: 'version_conflict', detail: 'Changed', details: { current } }, 412);
  await submit(); expect(container.textContent).toContain('Remote body'); expect(container.textContent).toContain('Local body'); expect(button('Save template').disabled).toBe(true); await click('Keep my draft for review');
  expect(input('Issue title').value).toBe('Local title'); expect(plainText(editor.value)).toBe('Local body'); command = async () => json({ ...current, version: '3' }); await submit();
  const writes = fetchMock.mock.calls.filter(([, options]) => (options as RequestInit).method === 'PATCH').map(([, options]) => options as RequestInit); expect(new Headers(writes[1].headers).get('If-Match')).toBe('"template:2"');
});
it('archives and restores with only the lifecycle field and current ETag', async () => {
  await render(); await click('Archive Bug report'); await submit(); let write = fetchMock.mock.calls.find(([, options]) => (options as RequestInit).method === 'PATCH')![1] as RequestInit; expect(JSON.parse(write.body as string)).toEqual({ archived: true }); expect(new Headers(write.headers).get('If-Match')).toBe('"template:1"');
  current = { ...record('2'), archived_at: '2026-10-18T13:00:00Z', can_use: false }; const state = container.querySelector<HTMLSelectElement>('.settings-state-filter select')!; await act(async () => { state.value = 'archived'; state.dispatchEvent(new Event('change', { bubbles: true })); }); await click('Restore'); await submit();
  write = fetchMock.mock.calls.filter(([, options]) => (options as RequestInit).method === 'PATCH').at(-1)![1] as RequestInit; expect(JSON.parse(write.body as string)).toEqual({ archived: false }); expect(new Headers(write.headers).get('If-Match')).toBe('"template:2"');
});
it('keeps a draft through same-user refresh but withholds private content after access denial', async () => {
  await render(); await click('Edit Bug report'); await fill('Issue title', 'Local title'); await act(async () => editor.onChange(fromPlainText('Local body')));
  await render(true, { ...session, csrf_token: 'new' }); expect(input('Issue title').value).toBe('Local title'); expect(plainText(editor.value)).toBe('Local body');
  command = async () => json({ code: 'not_found', detail: 'Unavailable.' }, 404); await submit(); expect(container.querySelector('form')).toBeNull(); expect(container.textContent).not.toContain('Local body'); expect(container.textContent).not.toContain('Original body'); expect(container.textContent).not.toContain('Bug report'); expect(container.textContent).toContain('Templates unavailable');
});

it('can retry a transient permission refresh without discarding the edited template', async () => {
  await render(); await click('Edit Bug report'); await fill('Issue title', 'Local title'); await act(async () => editor.onChange(fromPlainText('Local body')));
  let failed = true;
  fetchMock.mockImplementation((url: string) => Promise.resolve(url.endsWith('/preview') && failed ? json({ code: 'unavailable', detail: 'Refresh unavailable.' }, 503) : json(url.endsWith('/preview') ? { template: current, materialized_title: current.title, title_context_at: '2026-10-20T12:00:00Z', resolved_project: null, resolved_status: null, labels: [], diagnostics: [], can_create: true } : { data: [current], next_cursor: null, has_more: false })));
  await render(true, { ...session, csrf_token: 'new' }); expect(button('Save template').disabled).toBe(true); expect(input('Issue title').value).toBe('Local title');
  failed = false; await click('Try again'); expect(button('Save template').disabled).toBe(false); expect(input('Issue title').value).toBe('Local title'); expect(plainText(editor.value)).toBe('Local body');
});
it('hides an open editor when directory access is denied even if its separate detail refresh succeeds', async () => {
  await render(); await click('Edit Bug report'); await fill('Issue title', 'Private local draft');
  fetchMock.mockImplementation((url: string) => Promise.resolve(url.endsWith('/preview') ? json({ template: current, materialized_title: current.title, title_context_at: '2026-10-20T12:00:00Z', resolved_project: null, resolved_status: null, labels: [], diagnostics: [], can_create: true }) : json({ code: 'forbidden', detail: 'Access changed.' }, 403)));
  await render(true, { ...session, csrf_token: 'rotated' }); expect(container.textContent).toContain('Templates unavailable'); expect(container.textContent).not.toContain('Bug report'); expect(container.querySelector('form')).toBeNull(); expect(container.querySelector('input')).toBeNull();
});
it.each(['archive', 'restore'])('moves focus to a stable heading when %s removes the selected row', async mode => {
  current = { ...record(), archived_at: mode === 'restore' ? '2026-10-18T13:00:00Z' : null };
  fetchMock.mockImplementation((url: string, options: RequestInit) => {
    if (options.method === 'PATCH') { current = { ...current, archived_at: JSON.parse(options.body as string).archived ? '2026-10-18T13:00:00Z' : null, version: '2' }; return Promise.resolve(json(current)); }
    return Promise.resolve(json(url.endsWith('/preview') ? { template: current, materialized_title: current.title, title_context_at: '2026-10-20T12:00:00Z', resolved_project: null, resolved_status: null, labels: [], diagnostics: [], can_create: true } : { data: url.includes('archived=true') === !!current.archived_at ? [current] : [], next_cursor: null, has_more: false }));
  });
  await render();
  if (mode === 'restore') { const state = container.querySelector<HTMLSelectElement>('.settings-state-filter select')!; await act(async () => { state.value = 'archived'; state.dispatchEvent(new Event('change', { bubbles: true })); }); }
  const trigger = button(mode === 'restore' ? 'Restore' : 'Archive Bug report'); trigger.focus(); await click(mode === 'restore' ? 'Restore' : 'Archive Bug report'); await submit();
  expect(trigger.isConnected).toBe(false); expect(document.activeElement).toBe(container.querySelector('h3[tabindex="-1"]')); expect(container.querySelector('dialog')).toBeNull();
});

it('requires opting into pattern mode and preserves rejected pattern text for correction', async () => {
  await render(); await click('New template'); await fill('Template name', 'Daily review');
  const mode = container.querySelector<HTMLSelectElement>('[aria-label="Title mode"]')!; expect(mode.value).toBe('literal');
  await act(async () => { mode.value = 'pattern'; mode.dispatchEvent(new Event('change', { bubbles: true })); });
  expect(container.textContent).toContain('UTC YYYY-MM-DD'); await fill('Issue title', '{{unknown}}');
  command = async () => json({ code: 'validation_failed', detail: 'Title patterns support only {{team.name}}, {{team.key}} and {{date}}' }, 422);
  await submit(); expect(input('Issue title').value).toBe('{{unknown}}'); expect(mode.value).toBe('pattern'); expect(hasUnsavedChanges()).toBe(true);
  await fill('Issue title', '{{team.key}} {{date}}'); command = async () => json({ ...current, title_mode: 'pattern', title: '{{team.key}} {{date}}' }); await submit();
  const writes = fetchMock.mock.calls.filter(([, options]) => (options as RequestInit).method === 'POST').map(([, options]) => JSON.parse((options as RequestInit).body as string));
  expect(writes[0].title_mode).toBe('pattern'); expect(writes[1].title).toBe('{{team.key}} {{date}}'); expect(writes[1].title_mode).toBe('pattern'); expect(hasUnsavedChanges()).toBe(false);
});

it('treats changing only title mode as an unsaved edit and restores latest mode after conflict', async () => {
  await render(); await click('Edit Bug report');
  const mode = container.querySelector<HTMLSelectElement>('[aria-label="Title mode"]')!;
  await act(async () => { mode.value = 'pattern'; mode.dispatchEvent(new Event('change', { bubbles: true })); }); expect(hasUnsavedChanges()).toBe(true);
  current = record('2'); command = async () => json({ code: 'version_conflict', detail: 'Changed', details: { current } }, 412);
  await submit(); expect(container.textContent).toContain('Title mode: pattern'); expect(container.textContent).toContain('Title mode: literal'); await click('Use latest'); expect(mode.value).toBe('literal'); expect(hasUnsavedChanges()).toBe(false);
});
