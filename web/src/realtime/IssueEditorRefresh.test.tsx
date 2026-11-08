// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { IssueEditor } from '../issues/IssueEditor';
import { fromPlainText } from '../documents/model';
import type { StructuredDocument } from '../documents/types';
import type { Bootstrap, Issue, Session } from '../types';

vi.mock('../documents/Editor', async () => ({ StructuredEditor: (await import('../documents/RichTextEditor')).default }));

const issue: Issue = { id: 'i', workspace_id: 'w', team_id: 't', key: 'ENG-1', title: 'Saved title', description: 'Saved description', body_version: '1', status_id: 'todo', priority: 0, assignee_id: null, assignee: null, label_ids: [], labels: [], etag: '"issue:i:1:projection"', estimate: null, due_date: null, version: '1', created_at: '2026-10-20T12:00:00Z', updated_at: '2026-10-20T12:00:00Z', archived_at: null };
const session: Session = { audience: 'web', user: { id: 'u', display_name: 'Example Owner', email: 'owner@example.test' }, csrf_token: 'csrf', workspaces: [{ id: 'w', name: 'Workspace', slug: 'workspace', role: 'owner' }] };
const data: Bootstrap = { auth_epoch: '1', teams: [{ id: 't', name: 'Engineering', key: 'ENG', private: false, archived_at: null }], members: [session.user], statuses: [{ id: 'todo', team_id: 't', name: 'Todo', category: 'unstarted', color: 'slate', position: 0, is_default: true, retiring_at: null, archived_at: null, replacement_status_id: null, replacement_job_id: null }], issues: [issue] };
const documentRecord: StructuredDocument = { id: 'd', owner_id: 'i', owner_type: 'issue', schema_version: 1, schema_supported: true, body: fromPlainText('Saved description'), plain_text: 'Saved description', version: '1', etag: '"document:d:1"', can_edit: true, mentions: [], issue_references: [], created_at: issue.created_at, updated_at: issue.updated_at };
const json = (value: unknown) => new Response(JSON.stringify(value), { headers: { 'Content-Type': 'application/json' } });
let container: HTMLDivElement; let root: Root;

beforeEach(() => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true);
  Range.prototype.getClientRects = () => document.createElement('span').getClientRects();
  Range.prototype.getBoundingClientRect = () => new DOMRect(0, 0, 0, 0);
  HTMLDialogElement.prototype.showModal = function () { this.setAttribute('open', ''); };
  HTMLDialogElement.prototype.close = function () { this.removeAttribute('open'); };
  history.replaceState({ otIndex: 0 }, '', '/w/w/issues/i');
  container = document.createElement('div'); document.body.append(container); root = createRoot(container);
});
afterEach(async () => { await act(async () => root.unmount()); container.remove(); vi.unstubAllGlobals(); });


it('refreshes metadata, document and comments while retaining unsaved drafts and surfacing conflicts', async () => {
  let remote = false;
  const fetch = vi.fn(async (url: string, _options: RequestInit) => {
    if (url.endsWith('/capabilities')) return json({ features: { attachments: false }, limits: { max_upload_bytes: 1024 } });
    if (url.endsWith('/workflow')) return json({ team_id: 't', version: '1', etag: '"workflow:t:1"', can_manage: true, default_status_id: 'todo', statuses: data.statuses });
    if (url.endsWith('/document')) return json(remote ? { ...documentRecord, version: '2', etag: '"document:d:2"', body: fromPlainText('Remote description'), plain_text: 'Remote description' } : documentRecord);
    if (url.endsWith('/parent')) return json({ issue_id: issue.id, issue_etag: issue.etag, graph_version: '1', parent: null, can_change: true });
    if (/\/(children|relations)\?/.test(url)) return json({ data: [], next_cursor: null, has_more: false, issue_etag: issue.etag, graph_version: '1' });
    if (/\/(comments|activity-page)\?/.test(url)) return json({ data: [], next_cursor: null, has_more: false });
    if (url.endsWith('/subscription')) return json({ issue_id: 'i', preference: 'automatic', subscribed: false, sources: [], can_change: true, version: '0', etag: '"subscription:i:u:0"' });
    if (url.endsWith('/issues/i')) return json(remote ? { ...issue, title: 'Remote title', version: '2', etag: '"issue:i:2:projection"', body_version: '2' } : issue);
    throw new Error(`Unexpected fixture request: ${url}`);
  }); vi.stubGlobal('fetch', fetch);
  const props = { workspaceId: 'w', issueId: 'i', data, session, close: vi.fn(), updated: vi.fn() };
  await act(async () => root.render(<IssueEditor {...props} refresh={0} />));
  const title = container.querySelector<HTMLTextAreaElement>('#issue-title')!;
  await act(async () => { Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value')!.set!.call(title, 'Unsaved title'); title.dispatchEvent(new Event('input', { bubbles: true })); });
  const description = container.querySelector<HTMLElement>('.issue-document [contenteditable="true"]')!;
  expect(description).toBeTruthy();
  await act(async () => { description.innerHTML = '<p>Unsaved description</p>'; description.dispatchEvent(new Event('input', { bubbles: true })); });
  const compose = container.querySelector<HTMLElement>('.issue-comments [contenteditable="true"]') ?? [...container.querySelectorAll<HTMLElement>('[contenteditable="true"]')].at(-1)!;
  expect(compose).toBeTruthy();
  await act(async () => { compose.innerHTML = '<p>Unsent comment</p>'; compose.dispatchEvent(new Event('input', { bubbles: true })); });
  const commentReads = () => fetch.mock.calls.filter(([url]) => /\/comments\?/.test(url)).length;
  const reads = commentReads(); remote = true;
  await act(async () => root.render(<IssueEditor {...props} refresh={1} />));
  expect(container.querySelector('#issue-title')).toBe(title); expect(title.value).toBe('Unsaved title');
  expect(container.textContent).toContain('This issue changed while you were editing.'); expect(container.textContent).toContain('Remote title');
  expect(container.querySelector('.issue-document [contenteditable="true"]')?.textContent).toContain('Unsaved description');
  expect(container.textContent).toContain('Remote description');
  expect(compose.textContent).toContain('Unsent comment'); expect(commentReads()).toBeGreaterThan(reads);
  expect(fetch.mock.calls.every(([, options]) => !(options as RequestInit | undefined)?.method)).toBe(true);
});

it('keeps title and description drafts editable offline while pausing save and autosave', async () => {
  vi.useFakeTimers();
  const fetch = vi.fn(async (url: string, _options: RequestInit) => {
    if (url.endsWith('/capabilities')) return json({ features: { attachments: false }, limits: { max_upload_bytes: 1024 } });
    if (url.endsWith('/workflow')) return json({ team_id: 't', version: '1', etag: '"workflow:t:1"', can_manage: true, default_status_id: 'todo', statuses: data.statuses });
    if (url.endsWith('/document')) return json(documentRecord);
    if (url.endsWith('/parent')) return json({ issue_id: issue.id, issue_etag: issue.etag, graph_version: '1', parent: null, can_change: true });
    if (/\/(children|relations)\?/.test(url)) return json({ data: [], next_cursor: null, has_more: false, issue_etag: issue.etag, graph_version: '1' });
    if (/\/(comments|activity-page)\?/.test(url)) return json({ data: [], next_cursor: null, has_more: false });
    if (url.endsWith('/subscription')) return json({ issue_id: 'i', preference: 'automatic', subscribed: false, sources: [], can_change: true, version: '0', etag: '"subscription:i:u:0"' });
    return json(issue);
  }); vi.stubGlobal('fetch', fetch);
  try {
    await act(async () => root.render(<IssueEditor workspaceId="w" issueId="i" data={data} session={session} close={vi.fn()} updated={vi.fn()} connectionReadOnly />));
    const title = container.querySelector<HTMLTextAreaElement>('#issue-title')!;
    expect(title.matches(':disabled')).toBe(false);
    await act(async () => { Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value')!.set!.call(title, 'Offline title draft'); title.dispatchEvent(new Event('input', { bubbles: true })); });
    const description = container.querySelector<HTMLElement>('.issue-document [contenteditable="true"]')!;
    await act(async () => { description.innerHTML = '<p>Offline description draft</p>'; description.dispatchEvent(new Event('input', { bubbles: true })); });
    await act(async () => vi.advanceTimersByTimeAsync(2000));
    expect(title.value).toBe('Offline title draft'); expect(description.textContent).toContain('Offline description draft');
    expect([...container.querySelectorAll<HTMLButtonElement>('button')].find(button => button.textContent === 'Save changes')?.disabled).toBe(true);
    expect([...container.querySelectorAll<HTMLButtonElement>('button')].find(button => button.textContent === 'Save text now')?.disabled).toBe(true);
    expect(fetch.mock.calls.every(([, options]) => !options.method)).toBe(true);
  } finally { vi.useRealTimers(); }
});
