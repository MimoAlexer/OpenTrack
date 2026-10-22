// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { IssueEditor } from './IssueEditor';
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

it.each([{ ctrlKey: true }, { metaKey: true }])('keeps the transfer dialog outside issue metadata save shortcuts (%j)', async modifier => {
  const fetch = vi.fn(async (url: string, options: RequestInit) => {
    if (url.endsWith('/capabilities')) return json({ features: { attachments: false }, limits: { max_upload_bytes: 1024 } });
    if (url.endsWith('/workflow')) return json({ team_id: 't', version: '1', etag: '"workflow:t:1"', can_manage: true, default_status_id: 'todo', statuses: data.statuses });
    if (url.endsWith('/document')) return json(documentRecord);
    if (url.endsWith('/parent')) return json({ issue_id: issue.id, issue_etag: issue.etag, graph_version: '1', parent: null, can_change: true });
    if (/\/(children|relations)\?/.test(url)) return json({ data: [], next_cursor: null, has_more: false, issue_etag: issue.etag, graph_version: '1' });
    if (/\/(comments|activity-page)\?/.test(url)) return json({ data: [], next_cursor: null, has_more: false });
    if (url.endsWith('/subscription')) return json({ issue_id: 'i', preference: 'automatic', subscribed: false, sources: [], can_change: true, version: '0', etag: '"subscription:i:u:0"' });
    if (url.endsWith('/issues/i')) return json(options.method === 'PATCH' ? { ...issue, ...JSON.parse(String(options.body)), version: '2', etag: '"issue:i:2:projection"' } : issue);
    throw new Error(`Unexpected fixture request: ${url}`);
  });
  vi.stubGlobal('fetch', fetch); const updated = vi.fn();
  await act(async () => root.render(<IssueEditor workspaceId="w" issueId="i" data={data} session={session} close={vi.fn()} updated={updated} />));
  const title = container.querySelector<HTMLTextAreaElement>('#issue-title')!;
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value')!.set!.call(title, 'Unsaved title');
    title.dispatchEvent(new Event('input', { bubbles: true }));
  });
  const transfer = container.querySelector('.issue-document .document-transfer-actions button') as HTMLButtonElement;
  expect(transfer?.textContent).toBe('Import / export text');
  await act(async () => transfer.click());
  const dialog = container.querySelector('dialog')!;
  const writes = () => fetch.mock.calls.filter(([, options]) => options.method === 'PATCH');
  async function saveKey(target: HTMLElement) {
    await act(async () => { target.focus(); target.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', ...modifier, bubbles: true, cancelable: true })); });
  }
  await saveKey(dialog.querySelector('textarea')!);
  expect(writes()).toHaveLength(0); expect(updated).not.toHaveBeenCalled();
  expect(title.value).toBe('Unsaved title'); expect(dialog.open).toBe(true);
  await act(async () => { const action = dialog.querySelector('select')!; action.value = 'import'; action.dispatchEvent(new Event('change', { bubbles: true })); });
  await saveKey(dialog.querySelector('textarea')!);
  expect(writes()).toHaveLength(0); expect(updated).not.toHaveBeenCalled();
  expect(title.value).toBe('Unsaved title'); expect(dialog.open).toBe(true);
  await act(async () => [...dialog.querySelectorAll('button')].find(button => button.textContent === 'Cancel')!.click());
  await saveKey(title);
  expect(writes()).toHaveLength(1);
  expect(JSON.parse(String(writes()[0][1].body))).toEqual({ title: 'Unsaved title' });
  expect(new Headers(writes()[0][1].headers).get('If-Match')).toBe(issue.etag);
  expect(updated).toHaveBeenCalledOnce(); expect(container.textContent).toContain('Changes saved.');
});
