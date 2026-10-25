// @vitest-environment jsdom
import { act, createRef, useState, type ReactNode } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { ApiError } from '../api';
import type { Bootstrap, Issue, Session } from '../types';
import { CreateIssue } from '../issues/CreateIssue';
import { IssueEditor } from '../issues/IssueEditor';
import { IssueList } from '../issues/IssueList';
import { TeamWorkflow } from './TeamWorkflow';
import { StatusEditor } from './StatusEditor';
import { ReplaceStatus } from './ReplaceStatus';
import { ReplacementProgress } from './ReplacementProgress';
import { useWorkflow, readWorkflow } from './useWorkflow';
import type { Workflow, WorkflowReplacement, WorkflowStatus } from './types';

const session: Session = { audience: 'web', user: { id: 'u', display_name: 'Owner', email: 'owner@example.test' }, csrf_token: 'csrf', workspaces: [{ id: 'w', name: 'Workspace', slug: 'workspace', role: 'owner' }] };
const team = { id: 't', workspace_id: 'w', name: 'Engineering', key: 'ENG', private: false, version: '1', can_manage: true, archived_at: null };
function status(id: string, name: string, position: number, isDefault = false): WorkflowStatus { return { id, team_id: 't', name, category: 'unstarted', color: 'slate', position, is_default: isDefault, retiring_at: null, archived_at: null, replacement_status_id: null, replacement_job_id: null }; }
const first = status('s1', 'Ready', 0); const second = status('s2', 'Next', 1, true);
const initial: Workflow = { team_id: 't', version: '1', can_manage: true, statuses: [first, second] };
const issue: Issue = { id: 'i', workspace_id: 'w', team_id: 't', key: 'ENG-1', title: 'An existing issue', description: 'Keep the context', status_id: first.id, priority: 0, assignee_id: null, assignee: null, label_ids: [], labels: [], etag: '"issue:fixture:1:projection"', estimate: null, due_date: null, version: '1', created_at: '2026-10-17T10:00:00Z', updated_at: '2026-10-17T10:00:00Z', archived_at: null };
const data: Bootstrap = { auth_epoch: '1', teams: [team], statuses: initial.statuses, members: [session.user], issues: [issue] };
const job: WorkflowReplacement = { id: 'j', team_id: 't', source_status_id: 's1', replacement_status_id: 's2', state: 'queued', matched_count: '253', processed_count: '0', remaining_count: '253', version: '1', error_code: null };
const json = (value: unknown, status = 200) => new Response(JSON.stringify(value), { status });
const page = (values: unknown[]) => ({ data: values, next_cursor: null, has_more: false });
let container: HTMLDivElement; let root: Root;
beforeEach(() => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true); vi.spyOn(window, 'confirm').mockReturnValue(true);
  HTMLDialogElement.prototype.showModal = function () { this.setAttribute('open', ''); };
  HTMLDialogElement.prototype.close = function () { this.removeAttribute('open'); };
  container = document.createElement('div'); document.body.append(container); root = createRoot(container);
});
afterEach(async () => { await act(async () => root.unmount()); container.remove(); vi.restoreAllMocks(); vi.unstubAllGlobals(); });
async function render(node: ReactNode) { await act(async () => root.render(node)); }
function button(label: string, scope: ParentNode = container) { return [...scope.querySelectorAll<HTMLButtonElement>('button')].find(element => element.textContent === label || element.getAttribute('aria-label') === label)!; }
async function click(label: string, scope?: ParentNode) { await act(async () => button(label, scope).click()); }
async function input(element: HTMLInputElement | HTMLTextAreaElement | HTMLSelectElement, value: string) {
  await act(async () => { const prototype = element instanceof HTMLSelectElement ? HTMLSelectElement.prototype : element instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype; Object.getOwnPropertyDescriptor(prototype, 'value')!.set!.call(element, value); element.dispatchEvent(new Event(element instanceof HTMLSelectElement ? 'change' : 'input', { bubbles: true })); });
}
async function submit(form: HTMLFormElement) { await act(async () => form.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }))); }
async function focusRefresh() { await act(async () => window.dispatchEvent(new Event('focus'))); }
function field(label: string) { return [...container.querySelectorAll('label')].find(element => element.firstChild?.textContent === label)?.querySelector<HTMLInputElement | HTMLTextAreaElement | HTMLSelectElement>('input,textarea,select')!; }

it.each(['create', 'edit'])('preserves a rejected supplementary status %s draft and repairs it under the same workflow precondition', async mode => {
  const name = '\u{1F680}'.repeat(100); const writes: RequestInit[] = []; const saved = vi.fn();
  const accepted = { ...initial, version: '2', statuses: mode === 'create' ? [...initial.statuses, status('added', name, 2)] : [{ ...first, name }, second] };
  vi.stubGlobal('fetch', vi.fn((_url: string, options: RequestInit) => {
    writes.push(options);
    return Promise.resolve(writes.length === 1 ? json({ code: 'validation_failed', detail: 'Status name must contain 1–100 characters without control characters' }, 422) : json(accepted, mode === 'create' ? 201 : 200));
  }));
  await render(<StatusEditor workspaceId="w" workflow={initial} status={mode === 'edit' ? first : undefined} session={session} canManage close={() => {}} saved={saved} accept={() => {}} />);
  await input(field('Status name'), name + '\u{1F680}'); await submit(container.querySelector('form')!);
  expect(field('Status name').value).toBe(name + '\u{1F680}'); expect(container.textContent).toContain('Status name must contain'); expect(saved).not.toHaveBeenCalled();
  expect(field('Status name').matches(':disabled')).toBe(false);
  await input(field('Status name'), name); await submit(container.querySelector('form')!);
  expect(writes).toHaveLength(2); expect(saved).toHaveBeenCalledExactlyOnceWith(accepted);
  for (const [index, write] of writes.entries()) {
    expect(write.method).toBe(mode === 'create' ? 'POST' : 'PATCH');
    expect(new Headers(write.headers).get('If-Match')).toBe('"workflow:t:1:true"');
    expect(JSON.parse(write.body as string)).toEqual({ name: name + (index === 0 ? '\u{1F680}' : ''), color: 'slate', ...(mode === 'create' ? { category: 'unstarted' } : {}) });
  }
});

it('shows the explicit default regardless of order and omits it until the person chooses a status', async () => {
  let workflow = initial; let body: Record<string, unknown> | undefined;
  vi.stubGlobal('fetch', vi.fn((url: string, options: RequestInit) => {
    if (url.endsWith('/workflow')) return Promise.resolve(json(workflow));
    body = JSON.parse(options.body as string); return Promise.resolve(json(issue, 201));
  }));
  const created = vi.fn();
  await render(<CreateIssue workspaceId="w" data={data} session={session} created={created} close={() => {}} />);
  expect(field('Status').value).toBe('s2');
  await input(field('Title'), 'Preserved draft');
  workflow = { ...initial, version: '2', statuses: [{ ...first, is_default: true }, { ...second, is_default: false }] };
  await focusRefresh();
  expect(field('Title').value).toBe('Preserved draft'); expect(field('Status').value).toBe('s1');
  await submit(container.querySelector('form')!);
  expect(body).not.toHaveProperty('status_id'); expect(body?.title).toBe('Preserved draft'); expect(created).toHaveBeenCalledOnce();
});

it('preserves explicit draft status through rename and blocks a retired choice without changing the title', async () => {
  let workflow = initial;
  vi.stubGlobal('fetch', vi.fn(() => Promise.resolve(json(workflow))));
  await render(<CreateIssue workspaceId="w" data={data} session={session} created={() => {}} close={() => {}} />);
  await input(field('Title'), 'Keep my title'); await input(field('Status'), 's1');
  workflow = { ...initial, version: '2', statuses: [{ ...first, name: 'Ready for work' }, second] };
  await focusRefresh(); expect(field('Status').value).toBe('s1'); expect(container.textContent).toContain('Ready for work');
  workflow = { ...workflow, version: '3', statuses: [{ ...workflow.statuses[0], retiring_at: '2026-10-17T11:00:00Z', replacement_status_id: 's2', replacement_job_id: 'j' }, second] };
  await focusRefresh(); expect(field('Title').value).toBe('Keep my title'); expect(field('Status').value).toBe('s1');
  expect(button('Create issue').disabled).toBe(true); expect(container.textContent).toContain('Being replaced');
});

it('allows unrelated issue edits while its unchanged status is retiring', async () => {
  const retiring = { ...initial, version: '2', statuses: [{ ...first, retiring_at: '2026-10-17T11:00:00Z', replacement_status_id: 's2', replacement_job_id: 'j' }, second] };
  let patch: unknown;
  vi.stubGlobal('fetch', vi.fn((url: string, options: RequestInit) => {
    if (url.endsWith('/workflow')) return Promise.resolve(json(retiring));
    if (url.endsWith('/parent')) return Promise.resolve(json({ issue_id: issue.id, issue_etag: issue.etag, graph_version: '1', parent: null, can_change: true }));
    if (/\/(children|relations)\?/.test(url)) return Promise.resolve(json({ data: [], next_cursor: null, has_more: false, issue_etag: issue.etag, graph_version: '1' }));
    if (url.endsWith('/activity')) return Promise.resolve(json({ data: [] }));
    if (options.method === 'PATCH') { patch = JSON.parse(options.body as string); return Promise.resolve(json({ ...issue, title: 'Updated context', version: '2' })); }
    return Promise.resolve(json(issue));
  }));
  await render(<IssueEditor workspaceId="w" issueId="i" data={data} session={session} close={() => {}} updated={() => {}} />);
  expect(field('Status').value).toBe('s1'); expect(container.textContent).toContain('Being replaced');
  await input(container.querySelector<HTMLInputElement>('#issue-title')!, 'Updated context');
  await submit(container.querySelector('form')!);
  expect(patch).toEqual({ title: 'Updated context' }); expect(container.textContent).toContain('Changes saved.');
});

it('preserves a status edit on412 and only reapplies it after explicit review', async () => {
  const latest = { ...initial, version: '2', statuses: [{ ...first, name: 'Changed elsewhere' }, second] };
  const fetch = vi.fn().mockResolvedValueOnce(json({ code: 'version_conflict', detail: 'Workflow changed.', details: { current: latest } }, 412)).mockResolvedValueOnce(json({ ...latest, version: '3' }));
  vi.stubGlobal('fetch', fetch); const saved = vi.fn();
  function Editor() { const [workflow, setWorkflow] = useState(initial); return <StatusEditor workspaceId="w" workflow={workflow} status={first} session={session} canManage close={() => {}} saved={saved} accept={setWorkflow} />; }
  await render(<Editor />); await input(field('Status name'), 'My proposed name'); await submit(container.querySelector('form')!);
  expect(field('Status name').value).toBe('My proposed name'); expect(container.textContent).toContain('Changed elsewhere'); expect(button('Save status').disabled).toBe(true);
  await click('Keep my draft for review'); await submit(container.querySelector('form')!);
  expect(fetch).toHaveBeenCalledTimes(2);
  expect(new Headers(fetch.mock.calls[1][1].headers).get('If-Match')).toBe('"workflow:t:2:true"');
  expect(JSON.parse(fetch.mock.calls[1][1].body)).toEqual({ name: 'My proposed name', color: 'slate' }); expect(saved).toHaveBeenCalledOnce();
});

it('keeps a reordered draft when a new status appears and includes that status only after review', async () => {
  let workflow = initial; let order: unknown;
  vi.stubGlobal('fetch', vi.fn((_url: string, options: RequestInit) => {
    if (options.method === 'PUT') { order = JSON.parse(options.body as string); return Promise.resolve(json({ ...workflow, version: '3' })); }
    return Promise.resolve(json(workflow));
  }));
  await render(<TeamWorkflow workspaceId="w" team={team} session={session} />);
  await click('Move Next up');
  await act(async () => new Promise<void>(resolve => requestAnimationFrame(() => resolve())));
  expect(document.activeElement?.id).toBe('workflow-row-s2');
  workflow = { ...initial, version: '2', statuses: [...initial.statuses, status('s3', 'Later', 2)] };
  await focusRefresh(); expect(container.textContent).toContain('Your order is preserved'); expect(button('Save order').disabled).toBe(true);
  await click('Keep my order for review'); await click('Save order'); expect(order).toEqual({ status_ids: ['s2', 's1', 's3'] });
});

it('recovers status creation with the original idempotency key after an interrupted response', async () => {
  const writes: RequestInit[] = []; const saved = vi.fn();
  vi.stubGlobal('fetch', vi.fn((_url: string, options: RequestInit) => { writes.push(options); return writes.length === 1 ? Promise.reject(new TypeError('Interrupted')) : Promise.resolve(json({ ...initial, version: '2', statuses: [...initial.statuses, status('s3', 'Review', 2)] }, 201)); }));
  await render(<StatusEditor workspaceId="w" workflow={initial} session={session} canManage close={() => {}} saved={saved} accept={() => {}} />);
  await input(field('Status name'), 'Review'); await submit(container.querySelector('form')!);
  expect(field('Status name').matches(':disabled')).toBe(true); expect(button('Cancel').disabled).toBe(true);
  await submit(container.querySelector('form')!);
  expect(writes).toHaveLength(2); expect(writes[0].body).toBe(writes[1].body);
  expect(new Headers(writes[0].headers).get('Idempotency-Key')).toBe(new Headers(writes[1].headers).get('Idempotency-Key'));
  expect(saved).toHaveBeenCalledOnce();
});

it('uses current server permission instead of owner role for workflow actions', async () => {
  vi.stubGlobal('fetch', vi.fn(() => Promise.resolve(json({ ...initial, can_manage: false }))));
  await render(<TeamWorkflow workspaceId="w" team={team} session={session} />);
  expect(container.textContent).toContain('New issues start in'); expect(button('Add status')).toBeUndefined(); expect(button('Edit Ready')).toBeUndefined();
});

it('reviews archive in an accessible dialog, cancels without a write, and focuses the workflow after success', async () => {
  let finish!: (value: Response) => void;
  const fetch = vi.fn((_url: string, options: RequestInit) => options.method === 'POST' ? new Promise<Response>(resolve => { finish = resolve; }) : Promise.resolve(json(initial)));
  vi.stubGlobal('fetch', fetch);
  await render(<TeamWorkflow workspaceId="w" team={team} session={session} />);
  const row = container.querySelector('#workflow-row-s1')!;
  const trigger = button('Archive', row);
  (row.querySelector('details') as HTMLDetailsElement).open = true;
  trigger.focus(); await click('Archive', row);
  let dialog = container.querySelector('dialog')!;
  expect(dialog.getAttribute('open')).not.toBeNull();
  expect(dialog.querySelector('h2')?.id).toBe(dialog.getAttribute('aria-labelledby'));
  expect(dialog.textContent).toContain('Ready'); expect(dialog.textContent).toContain('No issues will be moved');
  expect(document.activeElement).toBe(button('Cancel', dialog));
  await click('Cancel', dialog);
  expect(container.querySelector('dialog')).toBeNull(); expect(document.activeElement).toBe(trigger);
  await click('Archive', row);
  await act(async () => container.querySelector('dialog')!.dispatchEvent(new Event('cancel', { cancelable: true })));
  expect(container.querySelector('dialog')).toBeNull(); expect(document.activeElement).toBe(trigger);
  expect(fetch.mock.calls.filter(([, options]) => options.method === 'POST')).toHaveLength(0);
  await click('Archive', row); dialog = container.querySelector('dialog')!;
  await click('Archive', dialog);
  expect(button('Cancel', dialog).disabled).toBe(true); expect(button('Close dialog', dialog).disabled).toBe(true);
  await act(async () => dialog.dispatchEvent(new Event('cancel', { cancelable: true })));
  expect(container.querySelector('dialog')).toBe(dialog);
  const write = fetch.mock.calls.find(([, options]) => options.method === 'POST')!;
  expect(write[0]).toBe('/api/v1/workspaces/w/teams/t/statuses/s1/archive');
  expect(new Headers(write[1].headers).get('If-Match')).toBe('"workflow:t:1:true"');
  await act(async () => finish(json({ ...initial, version: '2', statuses: [second] })));
  await act(async () => new Promise<void>(resolve => requestAnimationFrame(() => resolve())));
  expect(container.querySelector('dialog')).toBeNull(); expect(container.querySelector('#workflow-row-s1')).toBeNull();
  expect(document.activeElement?.tagName).toBe('H3'); expect(document.activeElement?.textContent).toBe('Workflow');
  expect(container.textContent).toContain('Ready archived.'); expect(window.confirm).not.toHaveBeenCalled();
});

it('keeps archive review open on a conflict and requires review of the current name and version', async () => {
  const latest = { ...initial, version: '2', statuses: [{ ...first, name: 'Ready for QA' }, second] };
  const writes: RequestInit[] = [];
  vi.stubGlobal('fetch', vi.fn((_url: string, options: RequestInit) => {
    if (options.method !== 'POST') return Promise.resolve(json(initial));
    writes.push(options);
    return Promise.resolve(writes.length === 1 ? json({ code: 'version_conflict', detail: 'Workflow changed.', details: { current: latest } }, 412) : json({ ...latest, version: '3', statuses: [second] }));
  }));
  await render(<TeamWorkflow workspaceId="w" team={team} session={session} />);
  await click('Archive', container.querySelector('#workflow-row-s1')!);
  const dialog = container.querySelector('dialog')!;
  await click('Archive', dialog);
  expect(container.querySelector('dialog')).toBe(dialog); expect(dialog.textContent).toContain('Ready for QA');
  expect(button('Archive', dialog).disabled).toBe(true); expect(writes).toHaveLength(1);
  await click('Review latest status', dialog);
  expect(button('Archive', dialog).disabled).toBe(false);
  await click('Archive', dialog);
  expect(new Headers(writes[1].headers).get('If-Match')).toBe('"workflow:t:2:true"');
  expect(container.querySelector('dialog')).toBeNull(); expect(container.textContent).toContain('Ready for QA archived.');
});

it('requires category-change acknowledgement and retries replacement with the original key and preview version', async () => {
  const workflow: Workflow = { ...initial, statuses: [first, { ...second, category: 'completed' }] };
  const writes: RequestInit[] = [];
  vi.stubGlobal('fetch', vi.fn((url: string, options: RequestInit) => {
    if (url.includes('replacement-preview')) return Promise.resolve(json({ team_id: 't', source_status_id: 's1', replacement_status_id: 's2', issue_count: '253', source_is_default: false, workflow_version: '1' }));
    writes.push(options); return writes.length === 1 ? Promise.reject(new TypeError('Interrupted')) : Promise.resolve(json(job, 202));
  }));
  const accepted = vi.fn();
  await render(<ReplaceStatus workspaceId="w" workflow={workflow} source={first} session={session} canManage close={() => {}} accepted={accepted} accept={() => {}} />);
  await input(field('Replacement status'), 's2'); expect(button('Replace status').disabled).toBe(true);
  await act(async () => container.querySelector<HTMLInputElement>('input[type=checkbox]')!.click()); await submit(container.querySelector('form')!);
  expect(button('Retry safely')).toBeDefined(); expect(field('Replacement status').matches(':disabled')).toBe(true);
  await submit(container.querySelector('form')!);
  expect(writes).toHaveLength(2); expect(writes[0].body).toBe(writes[1].body);
  expect(new Headers(writes[1].headers).get('Idempotency-Key')).toBe(new Headers(writes[0].headers).get('Idempotency-Key'));
  expect(new Headers(writes[1].headers).get('If-Match')).toBe('"workflow:t:1:true"'); expect(accepted).toHaveBeenCalledWith(job);
});

it('shows durable queued/progress state without invented percent and resumes only with the job version', async () => {
  let current = { ...job, state: 'blocked', version: '4', processed_count: '100', remaining_count: '140', error_code: 'requester_forbidden' };
  const fetch = vi.fn((_url: string, options: RequestInit) => { if (options.method === 'POST') current = { ...current, state: 'queued', version: '5', error_code: null as unknown as string }; return Promise.resolve(json(current)); });
  vi.stubGlobal('fetch', fetch);
  await render(<ReplacementProgress workspaceId="w" teamId="t" jobId="j" source={{ ...first, replacement_status_id: 's2' }} workflow={initial} session={session} canManage changed={() => {}} />);
  expect(container.textContent).toContain('100 moved · 140 remaining'); expect(container.textContent).not.toContain('%');
  await click('Resume replacement');
  const write = fetch.mock.calls.find(([, options]) => options.method === 'POST')!;
  expect(new Headers(write[1].headers).get('If-Match')).toBe('"workflow-replacement:j:4"');
  expect(container.textContent).toContain('when it is available'); expect(button('Resume replacement')).toBeUndefined();
});

it('handles a conditional304 without discarding data and still expires protected401', async () => {
  const expired = vi.fn(); window.addEventListener('opentrack:session-expired', expired);
  const fetch = vi.fn().mockResolvedValueOnce(new Response(null, { status: 304 })).mockResolvedValueOnce(json({ code: 'unauthenticated', detail: 'Sign in.' }, 401)); vi.stubGlobal('fetch', fetch);
  expect(await readWorkflow('/workspaces/w/teams/t/workflow', initial, new AbortController().signal)).toBeNull();
  expect(new Headers(fetch.mock.calls[0][1].headers).get('If-None-Match')).toBe('"workflow:t:1:true"');
  await expect(readWorkflow('/workspaces/w/teams/t/workflow', initial, new AbortController().signal)).rejects.toBeInstanceOf(ApiError);
  expect(expired).toHaveBeenCalledOnce(); window.removeEventListener('opentrack:session-expired', expired);
});

it('aborts old team polling and never repopulates its private status after a switch', async () => {
  let oldResolve!: (value: Response) => void; let oldSignal!: AbortSignal;
  vi.stubGlobal('fetch', vi.fn((url: string, options: RequestInit) => url.includes('/teams/t/') ? new Promise<Response>(resolve => { oldResolve = resolve; oldSignal = options.signal!; }) : Promise.resolve(json({ ...initial, team_id: 'other', statuses: [status('other', 'Other team status', 0, true)] }))));
  function Reader({ teamId }: { teamId: string }) { const workflow = useWorkflow('w', teamId, 'user-session'); return <p>{workflow.data?.statuses.map(status => status.name).join(',')}</p>; }
  await render(<Reader teamId="t" />); await render(<Reader teamId="other" />); expect(oldSignal.aborted).toBe(true);
  await act(async () => oldResolve(json({ ...initial, statuses: [{ ...first, name: 'Private old name' }] })));
  expect(container.textContent).toContain('Other team status'); expect(container.textContent).not.toContain('Private old name');
});

it('removes cached workflow content when a fresh read denies its scope', async () => {
  vi.stubGlobal('fetch', vi.fn().mockResolvedValueOnce(json(initial)).mockResolvedValueOnce(json({ code: 'not_found', detail: 'Unavailable.' }, 404)));
  await render(<TeamWorkflow workspaceId="w" team={team} session={session} />);
  expect(container.textContent).toContain('Ready'); await focusRefresh();
  expect(container.textContent).not.toContain('Ready'); expect(container.textContent).toContain('Unavailable.');
});

it('keeps returned issues visible when their historical status metadata is unavailable', async () => {
  vi.stubGlobal('fetch', vi.fn((url: string) => Promise.resolve(url.includes('/statuses/') ? json({ code: 'not_found', detail: 'Unavailable.' }, 404) : json(page([{ ...issue, status_id: 'old' }])))));
  await render(<IssueList workspaceId="w" data={{ ...data, statuses: [] }} session={session} params={new URLSearchParams()} refresh={0} create={() => {}} searchRef={createRef<HTMLInputElement>()} />);
  expect(container.textContent).toContain('An existing issue'); expect(container.textContent).toContain('Status unavailable');
});

it('does not refresh the parent list for initial workflow hydration matching visible bootstrap metadata', async () => {
  let workflow = initial; const changed = vi.fn();
  vi.stubGlobal('fetch', vi.fn(async () => json(workflow)));
  await render(<CreateIssue workspaceId="w" data={data} session={session} created={() => {}} close={() => {}} workflowChanged={changed} />);
  expect(field('Status').value).toBe(second.id); expect(changed).not.toHaveBeenCalled();
  workflow = { ...initial, version: '2', statuses: [{ ...first, name: 'Ready for review' }, second] };
  await focusRefresh();
  expect(container.textContent).toContain('Ready for review'); expect(changed).toHaveBeenCalledExactlyOnceWith(workflow);
});

it('propagates a genuine first-fetch metadata change instead of silently trusting old bootstrap statuses', async () => {
  const workflow = { ...initial, version: '2', statuses: [{ ...first, is_default: true }, { ...second, is_default: false }] };
  const changed = vi.fn(); vi.stubGlobal('fetch', vi.fn(async () => json(workflow)));
  await render(<CreateIssue workspaceId="w" data={data} session={session} created={() => {}} close={() => {}} workflowChanged={changed} />);
  expect(field('Status').value).toBe(first.id); expect(changed).toHaveBeenCalledExactlyOnceWith(workflow);
});
