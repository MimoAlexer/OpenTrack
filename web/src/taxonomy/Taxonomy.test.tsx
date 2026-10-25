// @vitest-environment jsdom
import { act, createRef, useState, type ReactNode } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import type { Bootstrap, Issue, Session, Status } from '../types';
import { AssigneePicker } from './AssigneePicker';
import { LabelPicker } from './LabelPicker';
import { LabelSettings } from './LabelSettings';
import { IssueEditor } from '../issues/IssueEditor';
import { IssueList } from '../issues/IssueList';
import { CreateIssue } from '../issues/CreateIssue';
import { RestoreIssue } from '../issues/RestoreIssue';
import type { Label, LabelSummary } from './types';

const session: Session = { audience: 'web', user: { id: 'me', display_name: 'Zoe Owner', email: 'private@example.test' }, csrf_token: 'csrf', workspaces: [{ id: 'w', name: 'Workspace', slug: 'workspace', role: 'owner' }] };
const status: Status = { id: 'ready', team_id: 't', name: 'Ready', category: 'unstarted', color: 'gray', position: 0, is_default: true, retiring_at: null, archived_at: null, replacement_status_id: null, replacement_job_id: null };
const label: Label = { id: 'l1', team_id: null, name: 'Customer', color: 'blue', archived_at: null, version: '1', can_manage: true };
const oldLabel: Label = { ...label, id: 'old', name: 'Legacy', archived_at: '2026-10-18T00:00:00Z' };
const issue: Issue = { id: 'i', workspace_id: 'w', team_id: 't', key: 'ENG-1', title: 'Existing issue', description: 'Context', status_id: 'ready', priority: 0, assignee_id: 'former', assignee: { id: 'former', display_name: 'Former colleague', assignable: false }, label_ids: ['old'], labels: [oldLabel], etag: '"issue:i:1:before"', estimate: null, due_date: null, version: '1', created_at: '2026-10-17T10:00:00Z', updated_at: '2026-10-17T10:00:00Z', archived_at: null };
const data: Bootstrap = { auth_epoch: '1', teams: [{ id: 't', name: 'Engineering', key: 'ENG', private: false, archived_at: null }, { id: 'other', name: 'Design', key: 'DES', private: true, archived_at: null }], statuses: [status, { ...status, id: 'other-ready', team_id: 'other' }], members: [], issues: [issue] };
const json = (value: unknown, status = 200) => new Response(JSON.stringify(value), { status });
const page = (values: unknown[], next: string | null = null) => ({ data: values, next_cursor: next, has_more: !!next });
let container: HTMLDivElement; let root: Root;
beforeEach(() => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true); vi.spyOn(window, 'confirm').mockReturnValue(true);
  HTMLDialogElement.prototype.showModal = function () { this.setAttribute('open', ''); };
  HTMLDialogElement.prototype.close = function () { this.removeAttribute('open'); };
  container = document.createElement('div'); document.body.append(container); root = createRoot(container);
});
afterEach(async () => { await act(async () => root.unmount()); container.remove(); vi.restoreAllMocks(); vi.unstubAllGlobals(); });
async function render(node: ReactNode) { await act(async () => root.render(node)); }
async function settle(ms = 10) { await act(async () => new Promise(resolve => setTimeout(resolve, ms))); }
function button(label: string, scope: ParentNode = container) { return [...scope.querySelectorAll<HTMLButtonElement>('button')].find(element => element.textContent === label || element.getAttribute('aria-label') === label)!; }
async function click(label: string, scope?: ParentNode) { await act(async () => button(label, scope).click()); }
async function input(element: HTMLInputElement | HTMLTextAreaElement | HTMLSelectElement, value: string) { await act(async () => { Object.getOwnPropertyDescriptor(element instanceof HTMLSelectElement ? HTMLSelectElement.prototype : element instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype, 'value')!.set!.call(element, value); element.dispatchEvent(new Event(element instanceof HTMLSelectElement ? 'change' : 'input', { bubbles: true })); }); }
async function submit(form: HTMLFormElement) { await act(async () => form.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }))); }
function picker(label: string) { return [...container.querySelectorAll<HTMLElement>('.reference-picker')].find(element => element.querySelector('.field-label')?.textContent === label)!; }
async function open(label: string) { await act(async () => picker(label).querySelector<HTMLButtonElement>('.reference-trigger')!.click()); await settle(); }
async function choose(name: string) { const option = [...container.querySelectorAll<HTMLElement>('[role=option]')].find(option => option.textContent?.includes(name))!; await act(async () => option.click()); }

it.each(['create', 'edit'])('retains a rejected supplementary label %s draft and submits its repaired scalar boundary once', async mode => {
  const name = '\u{1F308}'.repeat(100); const writes: RequestInit[] = [];
  const original = { ...label, team_id: 't' }; let current: Label | null = mode === 'edit' ? original : null;
  const fetch = vi.fn((_url: string, options: RequestInit) => {
    if (options.method !== 'POST' && options.method !== 'PATCH') return Promise.resolve(json(page(current ? [current] : [])));
    writes.push(options);
    if (writes.length === 1) return Promise.resolve(json({ code: 'validation_failed', detail: 'Label name must contain 1–100 characters without control characters' }, 422));
    current = { ...original, name, color: mode === 'create' ? 'gray' : 'blue', version: mode === 'create' ? '1' : '2' };
    return Promise.resolve(json(current, mode === 'create' ? 201 : 200));
  });
  vi.stubGlobal('fetch', fetch);
  await render(<LabelSettings workspaceId="w" teamId="t" scopeName="Engineering" session={session} canCreate />); await settle();
  await click(mode === 'create' ? 'New label' : 'Edit Customer');
  const nameInput = container.querySelector<HTMLInputElement>('dialog input')!;
  await input(nameInput, name + '\u{1F308}'); await submit(container.querySelector('dialog form')!);
  expect(nameInput.value).toBe(name + '\u{1F308}'); expect(nameInput.matches(':disabled')).toBe(false);
  expect(container.textContent).toContain('Label name must contain'); expect(current).toEqual(mode === 'edit' ? original : null);
  await input(nameInput, name); await submit(container.querySelector('dialog form')!);
  expect(container.querySelector('dialog')).toBeNull(); expect(writes).toHaveLength(2);
  for (const [index, write] of writes.entries()) {
    expect(write.method).toBe(mode === 'create' ? 'POST' : 'PATCH');
    expect(JSON.parse(write.body as string)).toEqual({ name: name + (index === 0 ? '\u{1F308}' : ''), color: mode === 'create' ? 'gray' : 'blue', ...(mode === 'create' ? { team_id: 't' } : {}) });
    if (mode === 'edit') expect(new Headers(write.headers).get('If-Match')).toBe('"label:l1:1:true"');
  }
});

it('sends complete supplementary label queries and preserves 422 text without retrying an unfiltered directory', async () => {
  const name = '\u{1F31F}'.repeat(100); const overlong = name + '\u{1F31F}'; const queries: string[] = [];
  vi.stubGlobal('fetch', vi.fn((url: string) => {
    const query = new URL(url, 'http://example.test').searchParams.get('q') ?? ''; queries.push(query);
    return Promise.resolve(query === overlong ? json({ code: 'validation_failed', detail: 'Search must contain at most 100 characters' }, 422) : json(page(query === name ? [{ ...label, name }] : [])));
  }));
  await render(<LabelSettings workspaceId="w" scopeName="Workspace" session={session} canCreate />); await settle();
  const search = container.querySelector<HTMLInputElement>('[aria-label="Search labels"]')!;
  await input(search, name); await settle(260); expect(queries.at(-1)).toBe(name); expect(button(`Edit ${name}`)).toBeTruthy();
  const beforeRejected = queries.length;
  await input(search, overlong); await settle(260);
  expect(search.value).toBe(overlong); expect(container.textContent).toContain('Search must contain at most 100 characters');
  expect(container.querySelectorAll('.settings-row')).toHaveLength(0); expect(queries.slice(beforeRejected)).toEqual([overlong]);
  await input(search, name); await settle(260); expect(button(`Edit ${name}`)).toBeTruthy(); expect(queries.at(-1)).toBe(name);
});

it('offers Assign to me independently of candidate pages and supports keyboard selection with focus restoration', async () => {
  const fetch = vi.fn((_url: string) => Promise.resolve(json({ ...page(Array.from({ length: 20 }, (_, i) => ({ id: `u${i}`, display_name: `Candidate ${i}` })), 'page2'), self_assignable: true })));
  vi.stubGlobal('fetch', fetch);
  function Field() { const [value, change] = useState<string | null>(null); return <AssigneePicker workspaceId="w" teamId="t" session={session} value={value} change={change} />; }
  await render(<Field />); await open('Assignee');
  const search = container.querySelector<HTMLInputElement>('[role=combobox]')!;
  expect(document.activeElement).toBe(search); expect(container.textContent).toContain('Assign to me'); expect(fetch.mock.calls[0][0]).toContain('limit=20');
  await act(async () => { search.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true })); });
  await act(async () => { search.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true })); });
  expect(search.getAttribute('aria-activedescendant')).toBe(container.querySelectorAll('[role=option]')[1].id);
  await act(async () => search.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true })));
  expect(container.querySelector('[role=combobox]')).toBeNull(); expect(picker('Assignee').textContent).toContain('Zoe Owner'); expect(document.activeElement).toBe(picker('Assignee').querySelector('button'));
  expect(container.textContent).not.toContain(session.user.email);
});

it('loads more eligible candidates and retains the chosen name outside later search pages', async () => {
  vi.stubGlobal('fetch', vi.fn((url: string) => Promise.resolve(json({ ...page(url.includes('cursor=') ? [{ id: 'last', display_name: 'Last candidate' }] : url.includes('q=none') ? [] : [{ id: 'first', display_name: 'First candidate' }], url.includes('cursor=') || url.includes('q=none') ? null : 'opaque'), self_assignable: false }))));
  function Field() { const [value, change] = useState<string | null>(null); return <AssigneePicker workspaceId="w" teamId="t" session={session} value={value} change={change} />; }
  await render(<Field />); await open('Assignee'); await click('Load more'); await choose('Last candidate');
  expect(picker('Assignee').textContent).toContain('Last candidate'); await open('Assignee');
  await input(container.querySelector<HTMLInputElement>('[role=combobox]')!, 'none'); await settle(260);
  expect(picker('Assignee').querySelector('.reference-trigger')?.textContent).toContain('Last candidate');
  expect(container.textContent).not.toContain('Assign to me');
});

it('aborts old private-team candidate responses and removes their selection display after switching scope', async () => {
  let resolveOld!: (value: Response) => void; let oldSignal!: AbortSignal;
  vi.stubGlobal('fetch', vi.fn((url: string, options: RequestInit) => url.includes('/teams/t/') ? new Promise<Response>(resolve => { resolveOld = resolve; oldSignal = options.signal!; }) : Promise.resolve(json({ ...page([{ id: 'new', display_name: 'New team person' }]), self_assignable: false }))));
  await render(<AssigneePicker workspaceId="w" teamId="t" session={session} value={null} change={() => {}} />); await open('Assignee');
  await render(<AssigneePicker workspaceId="w" teamId="other" session={session} value={null} change={() => {}} />); expect(oldSignal.aborted).toBe(true);
  await open('Assignee'); await act(async () => resolveOld(json({ ...page([{ id: 'secret', display_name: 'Private person' }]), self_assignable: true })));
  expect(container.textContent).toContain('New team person'); expect(container.textContent).not.toContain('Private person');
});

it('preserves archived attached labels, supports undo, and distinguishes shared and team names', async () => {
  vi.stubGlobal('fetch', vi.fn(() => Promise.resolve(json(page([label, { ...label, id: 'team-label', team_id: 't' }])))));
  function Field() { const [value, change] = useState(['old']); return <LabelPicker workspaceId="w" teamId="t" session={session} value={value} summaries={[oldLabel]} teams={data.teams} change={change} />; }
  await render(<Field />); expect(container.textContent).toContain('Legacy'); expect(container.textContent).toContain('Archived');
  await click('Remove Legacy'); expect(container.textContent).toContain('Legacy removed.'); await click('Undo removal'); expect(button('Remove Legacy')).toBeDefined();
  await open('Labels'); const options = [...container.querySelectorAll('[role=option]')];
  expect(options.map(option => option.textContent)).toEqual(['CustomerShared', 'CustomerEngineering']);
  await act(async () => (options[1] as HTMLElement).click()); expect(button('Remove Customer')).toBeDefined();
  await act(async () => container.querySelector<HTMLInputElement>('[role=combobox]')!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true })));
  expect(container.querySelector('[role=listbox]')).toBeNull(); expect(document.activeElement).toBe(picker('Labels').querySelector('button'));
});

it('prevents selecting a fifty-first label while leaving current selections removable', async () => {
  const labels: LabelSummary[] = Array.from({ length: 50 }, (_, i) => ({ ...label, id: `label-${i}`, name: `Selected ${i}` }));
  const change = vi.fn(); vi.stubGlobal('fetch', vi.fn(() => Promise.resolve(json(page([{ ...label, id: 'extra' }])))));
  await render(<LabelPicker workspaceId="w" teamId="t" session={session} value={labels.map(label => label.id)} summaries={labels} teams={data.teams} change={change} />); await open('Labels');
  expect(container.querySelector('[role=option]')?.getAttribute('aria-disabled')).toBe('true'); await choose('Customer'); expect(change).not.toHaveBeenCalled();
  await click('Remove Selected 0'); expect(change.mock.calls[0][0]).toHaveLength(49);
});

it('keeps label edits on412 and uses the reviewed permission-sensitive tag', async () => {
  const latest = { ...label, name: 'Changed elsewhere', version: '2' }; const writes: RequestInit[] = [];
  vi.stubGlobal('fetch', vi.fn((_url: string, options: RequestInit) => { if (options.method !== 'PATCH') return Promise.resolve(json(page([label]))); writes.push(options); return Promise.resolve(writes.length === 1 ? json({ code: 'version_conflict', detail: 'Changed.', details: { current: latest } }, 412) : json({ ...latest, name: 'My draft', version: '3' })); }));
  await render(<LabelSettings workspaceId="w" scopeName="Workspace" session={session} canCreate />); await settle(); await click('Edit Customer');
  const field = container.querySelector<HTMLInputElement>('dialog input')!; await input(field, 'My draft'); await submit(container.querySelector('dialog form')!);
  expect(field.value).toBe('My draft'); expect(container.textContent).toContain('Changed elsewhere'); expect(button('Save label').disabled).toBe(true);
  await click('Keep my draft for review'); await submit(container.querySelector('dialog form')!);
  expect(new Headers(writes[1].headers).get('If-Match')).toBe('"label:l1:2:true"'); expect(JSON.parse(writes[1].body as string)).toEqual({ name: 'My draft', color: 'blue' });
});

it('retries an uncertain label creation with the original body and key', async () => {
  const writes: RequestInit[] = []; vi.stubGlobal('fetch', vi.fn((_url: string, options: RequestInit) => { if (options.method !== 'POST') return Promise.resolve(json(page([]))); writes.push(options); return writes.length === 1 ? Promise.reject(new TypeError('Lost response')) : Promise.resolve(json(label, 201)); }));
  await render(<LabelSettings workspaceId="w" scopeName="Workspace" session={session} canCreate />); await settle(); await click('New label');
  await input(container.querySelector<HTMLInputElement>('dialog input')!, 'Customer'); await submit(container.querySelector('dialog form')!);
  expect(button('Cancel').disabled).toBe(true); expect(container.querySelector<HTMLInputElement>('dialog input')?.matches(':disabled')).toBe(true);
  await submit(container.querySelector('dialog form')!); expect(writes).toHaveLength(2); expect(writes[1].body).toBe(writes[0].body);
  expect(new Headers(writes[1].headers).get('Idempotency-Key')).toBe(new Headers(writes[0].headers).get('Idempotency-Key'));
});

it('offers no label mutation controls without current authority', async () => {
  vi.stubGlobal('fetch', vi.fn(() => Promise.resolve(json(page([{ ...label, can_manage: false }])))));
  await render(<LabelSettings workspaceId="w" teamId="t" scopeName="Engineering" session={session} canCreate={false} />); await settle();
  expect(container.textContent).toContain('Customer'); expect(button('New label')).toBeUndefined(); expect(button('Edit Customer')).toBeUndefined(); expect(button('Archive Customer')).toBeUndefined();
});

it('uses the server projection tag, preserves a title draft on metadata-only conflict, and retains historical names', async () => {
  const latest = { ...issue, etag: '"issue:i:1:renamed"', labels: [{ ...oldLabel, name: 'Historical label' }] }; const writes: RequestInit[] = [];
  vi.stubGlobal('fetch', vi.fn((url: string, options: RequestInit) => {
    if (url.endsWith('/workflow')) return Promise.resolve(json({ team_id: 't', version: '1', can_manage: true, statuses: [status] }));
    if (url.endsWith('/parent')) return Promise.resolve(json({ issue_id: issue.id, issue_etag: issue.etag, graph_version: '1', parent: null, can_change: true }));
    if (/\/(children|relations)\?/.test(url)) return Promise.resolve(json({ data: [], next_cursor: null, has_more: false, issue_etag: issue.etag, graph_version: '1' }));
    if (url.endsWith('/activity')) return Promise.resolve(json(page([])));
    if (options.method === 'PATCH') { writes.push(options); return Promise.resolve(writes.length === 1 ? json({ code: 'version_conflict', detail: 'References changed.', details: { current: latest } }, 412) : json({ ...latest, title: 'My title', version: '2', etag: '"issue:i:2:renamed"' })); }
    return Promise.resolve(json(issue));
  }));
  await render(<IssueEditor workspaceId="w" issueId="i" data={data} session={session} close={() => {}} updated={() => {}} />);
  expect(picker('Assignee').textContent).toContain('Former colleague'); expect(picker('Assignee').textContent).not.toContain('Unassigned');
  await input(container.querySelector<HTMLTextAreaElement>('#issue-title')!, 'My title'); await submit(container.querySelector('form')!);
  expect(container.querySelector<HTMLTextAreaElement>('#issue-title')?.value).toBe('My title'); await click('Keep my edits for review');
  expect(container.textContent).toContain('Historical label'); await submit(container.querySelector('form')!);
  expect(writes.map(write => new Headers(write.headers).get('If-Match'))).toEqual([issue.etag, latest.etag]);
  expect(JSON.parse(writes[1].body as string)).toEqual({ title: 'My title' });
});

it('restores with explicit assignment repair and keeps an existing archived label in the same atomic command', async () => {
  let write: RequestInit | undefined;
  vi.stubGlobal('fetch', vi.fn((_url: string, options: RequestInit) => { if (options.method === 'PATCH') { write = options; return Promise.resolve(json({ ...issue, archived_at: null, assignee_id: null, assignee: null })); } return Promise.resolve(json({ ...page([]), self_assignable: false })); }));
  const restored = vi.fn(); await render(<RestoreIssue issue={{ ...issue, archived_at: '2026-10-18T00:00:00Z' }} workspaceId="w" session={session} data={data} statuses={[status]} currentStatusIds={new Set(['ready'])} close={() => {}} restored={restored} />);
  expect(button('Restore issue').disabled).toBe(true); expect(container.textContent).toContain('Legacy'); expect(container.querySelector('input[type=number]')).toBeNull();
  await open('Assignee'); await choose('Unassigned'); expect(button('Restore issue').disabled).toBe(false); await submit(container.querySelector('form')!);
  expect(JSON.parse(write!.body as string)).toEqual({ archived: false, assignee_id: null }); expect(new Headers(write!.headers).get('If-Match')).toBe(issue.etag); expect(restored).toHaveBeenCalledOnce();
});

it('passes label any/all and priority sort to the server and preserves global returned order across status groups', async () => {
  const urgent = { ...issue, id: 'urgent', title: 'Urgent complete', priority: 1, status_id: 'done' }; const unset = { ...issue, id: 'unset', title: 'Unset ready', priority: 0 };
  const fetch = vi.fn((url: string) => Promise.resolve(json(url.includes('/labels/') ? url.endsWith('/old') ? oldLabel : label : page([urgent, unset])))); vi.stubGlobal('fetch', fetch);
  const params = new URLSearchParams({ label_ids: 'old,l1', label_mode: 'all', sort: 'priority' });
  await render(<IssueList workspaceId="w" data={{ ...data, statuses: [...data.statuses, { ...status, id: 'done', category: 'completed' }] }} session={session} params={params} refresh={0} create={() => {}} searchRef={createRef<HTMLInputElement>()} />);
  const listRequest = fetch.mock.calls.find(([url]) => url.includes('/issues?'))![0];
  expect(listRequest).toContain('label_mode=all'); expect(listRequest).toContain('sort=priority');
  expect([...container.querySelectorAll('.issue-title')].map(item => item.textContent)).toEqual(['Urgent complete', 'Unset ready']);
  expect(container.querySelector('[aria-label="Former colleague"]')).not.toBeNull(); expect(container.textContent).toContain('Legacy');
});

it('clears team-bound create selections visibly when the person chooses another team', async () => {
  vi.stubGlobal('fetch', vi.fn((url: string) => Promise.resolve(json(url.includes('/workflow') ? { team_id: url.includes('/other/') ? 'other' : 't', version: '1', can_manage: true, statuses: data.statuses.filter(status => status.team_id === (url.includes('/other/') ? 'other' : 't')) } : { ...page([]), self_assignable: true }))));
  await render(<CreateIssue workspaceId="w" data={data} session={session} close={() => {}} created={() => {}} />);
  await open('Assignee'); await choose('Assign to me'); expect(picker('Assignee').textContent).toContain('Zoe Owner');
  await input(container.querySelector<HTMLSelectElement>('.team-field select')!, 'other');
  expect(picker('Assignee').textContent).toContain('Unassigned'); expect(container.textContent).toContain('Team changed. Choose an assignee and labels for this team.');
});

it('reloads the first candidate page after an expired cursor without discarding the selected assignee', async () => {
  const fetch = vi.fn((url: string) => Promise.resolve(url.includes('cursor=') ? json({ code: 'cursor_expired', detail: 'Expired.' }, 410) : json({ ...page([{ id: 'first', display_name: 'First page' }], 'expired'), self_assignable: false })));
  vi.stubGlobal('fetch', fetch);
  await render(<AssigneePicker workspaceId="w" teamId="t" session={session} value="former" current={issue.assignee} change={() => {}} />); await open('Assignee'); await click('Load more');
  expect(container.textContent).toContain('reload the first page; your selections are preserved'); expect(picker('Assignee').querySelector('.reference-trigger')?.textContent).toContain('Former colleague');
  await click('Try again'); await settle();
  expect(fetch.mock.calls.at(-1)![0]).not.toContain('cursor='); expect(container.textContent).not.toContain('This result page expired');
});

it('Escape closes the label picker without dismissing the surrounding create dialog', async () => {
  vi.stubGlobal('fetch', vi.fn((url: string) => Promise.resolve(json(url.endsWith('/workflow') ? { team_id: 't', version: '1', can_manage: true, statuses: [status] } : page([label])))));
  const close = vi.fn(); await render(<CreateIssue workspaceId="w" data={data} session={session} close={close} created={() => {}} />); await open('Labels');
  const dialog = container.querySelector('dialog')!; const search = dialog.querySelector<HTMLInputElement>('[role=combobox]')!;
  await act(async () => search.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true })));
  expect(dialog.hasAttribute('open')).toBe(true); expect(dialog.querySelector('[role=listbox]')).toBeNull(); expect(close).not.toHaveBeenCalled(); expect(document.activeElement).toBe(picker('Labels').querySelector('button'));
});

it('restores an archived label with an explicitly repaired name in one versioned patch', async () => {
  let write: RequestInit | undefined;
  vi.stubGlobal('fetch', vi.fn((_url: string, options: RequestInit) => { if (options.method === 'PATCH') { write = options; return Promise.resolve(json({ ...oldLabel, name: 'Legacy customer', archived_at: null, version: '2' })); } return Promise.resolve(json(page([oldLabel]))); }));
  await render(<LabelSettings workspaceId="w" scopeName="Workspace" session={session} canCreate />); await settle(); await click('Restore');
  await input(container.querySelector<HTMLInputElement>('dialog input')!, 'Legacy customer'); await submit(container.querySelector('dialog form')!);
  expect(JSON.parse(write!.body as string)).toEqual({ archived: false, name: 'Legacy customer', color: 'blue' }); expect(new Headers(write!.headers).get('If-Match')).toBe('"label:old:1:true"');
});

it('removes visible candidates when loading another page explicitly denies team access', async () => {
  vi.stubGlobal('fetch', vi.fn((url: string) => Promise.resolve(url.includes('cursor=') ? json({ code: 'not_found', detail: 'Unavailable.' }, 404) : json({ ...page([{ id: 'hidden', display_name: 'Private candidate' }], 'page2'), self_assignable: false }))));
  await render(<AssigneePicker workspaceId="w" teamId="t" session={session} value={null} change={() => {}} />); await open('Assignee'); expect(container.textContent).toContain('Private candidate'); await click('Load more');
  expect(container.textContent).not.toContain('Private candidate'); expect(container.textContent).toContain('Unavailable.');
});
