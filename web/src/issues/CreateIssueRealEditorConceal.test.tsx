// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { CreateIssue } from './CreateIssue';
import type { Bootstrap, Session } from '../types';

let reportAccess: (denied: boolean) => void;
vi.mock('../documents/Editor', async () => ({ StructuredEditor: (await import('../documents/RichTextEditor')).default }));
vi.mock('../templates/ProjectPicker', () => ({ ProjectPicker: ({ accessChanged }: { accessChanged: (denied: boolean) => void }) => { reportAccess = accessChanged; return null; }, projectSignature: () => '' }));
vi.mock('../templates/TemplateChooser', () => ({ TemplateChooser: () => null }));
vi.mock('./IssueFields', () => ({ IssueFields: () => null }));
vi.mock('../workflows/useIssueWorkflow', () => ({ useIssueWorkflow: () => ({ statuses: [{ id: 'ready', name: 'Ready', category: 'unstarted', is_default: true, archived_at: null, retiring_at: null }], currentIds: new Set(['ready']), error: null, refresh: () => {} }) }));
const session: Session = { audience: 'web', user: { id: 'u', display_name: 'Example Owner', email: 'owner@example.test' }, csrf_token: 'csrf', workspaces: [] };
const data: Bootstrap = { auth_epoch: '1', teams: [{ id: 't', name: 'Team', key: 'EX', private: false, archived_at: null }], statuses: [], members: [], issues: [] };
let container: HTMLDivElement; let root: Root;
beforeEach(() => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true); vi.stubGlobal('requestAnimationFrame', (fn: FrameRequestCallback) => setTimeout(() => fn(0), 0));
  Range.prototype.getClientRects = () => document.createElement('span').getClientRects(); Range.prototype.getBoundingClientRect = () => new DOMRect(0, 0, 0, 0);
  HTMLDialogElement.prototype.showModal = function () { this.open = true; }; HTMLDialogElement.prototype.close = function () { this.open = false; };
  vi.stubGlobal('fetch', vi.fn(() => Promise.resolve(new Response(JSON.stringify({ features: { attachments: true }, limits: { max_upload_bytes: 25 * 1024 * 1024 } }), { status: 200 }))));
  container = document.createElement('div'); document.body.append(container); root = createRoot(container);
});
afterEach(async () => { await act(async () => root.unmount()); container.remove(); vi.restoreAllMocks(); vi.unstubAllGlobals(); });

it('closes the real nested link dialog and focuses the denial notice without remounting the editor', async () => {
  await act(async () => root.render(<CreateIssue workspaceId="w" data={data} session={session} close={() => {}} created={() => {}}/>));
  const editor = container.querySelector('[role="textbox"]')!; expect(editor).toBeTruthy();
  await act(async () => container.querySelector<HTMLButtonElement>('button[aria-label="Insert or edit link"]')!.click());
  expect(container.querySelectorAll('dialog[open]')).toHaveLength(2);
  await act(async () => reportAccess(true));
  expect(container.querySelectorAll('dialog[open]')).toHaveLength(1); expect(container.querySelector('input[placeholder="https://example.com"]')).toBeNull();
  expect(editor.closest('section')?.hidden).toBe(true); expect(document.activeElement?.getAttribute('role')).toBe('alert');
  await act(async () => reportAccess(false)); expect(container.querySelector('[role="textbox"]')).toBe(editor);
});
