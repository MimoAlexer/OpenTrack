// @vitest-environment jsdom
import { act, useEffect } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { CreateIssue } from './CreateIssue';
import { hashFile, uploadBrowserFile } from '../attachments/browser';
import { useUploads } from '../attachments/useUploads';
import type { Attachment } from '../attachments/types';
import type { RichTextEditorProps } from '../documents/RichTextEditor';
import type { Bootstrap, Session } from '../types';

vi.mock('@tauri-apps/api/core', () => ({ isTauri: () => false }));
vi.mock('../attachments/browser', async original => ({ ...await original<typeof import('../attachments/browser')>(), hashFile: vi.fn(), uploadBrowserFile: vi.fn() }));
vi.mock('../templates/TemplateChooser', () => ({ TemplateChooser: () => null }));
vi.mock('../templates/ProjectPicker', () => ({ ProjectPicker: ({ accessChanged }: { accessChanged: (denied: boolean) => void }) => <><button type="button" onClick={() => accessChanged(true)}>Deny project access</button><button type="button" onClick={() => accessChanged(false)}>Restore project access</button></>, projectSignature: () => '' }));
vi.mock('./IssueFields', () => ({ IssueFields: () => null }));
vi.mock('../workflows/useIssueWorkflow', () => ({ useIssueWorkflow: () => ({ statuses: [{ id: 'ready', name: 'Ready', category: 'unstarted', is_default: true, archived_at: null, retiring_at: null }], currentIds: new Set(['ready']), error: null, refresh: () => {} }) }));
let uploads: ReturnType<typeof useUploads>; let unmounts = 0;
vi.mock('../documents/Editor', () => ({ StructuredEditor: (props: RichTextEditorProps) => {
  useEffect(() => () => { unmounts += 1; }, []);
  uploads = useUploads({ target: props.attachmentTarget, enabled: !props.disabled, prepareAnchor: () => 'anchor', forgetAnchor: () => {}, ready: () => true, changed: props.attachmentsChanged });
  return <div data-testid="editor"><span>Private description</span><button type="button" disabled={props.disabled} onClick={() => uploads.addBrowserFiles([new File(['body'], 'private-file.txt', { type: 'text/plain' })])}>Add file</button>{uploads.rows.map(row => <button type="button" key={row.key} disabled={props.disabled} onClick={() => void uploads.cancel(row.key)}>Cancel file</button>)}</div>;
} }));

const session: Session = { audience: 'web', user: { id: 'u', display_name: 'Example Owner', email: 'owner@example.test' }, csrf_token: 'csrf', workspaces: [] };
const data: Bootstrap = { auth_epoch: '1', teams: [{ id: 't', name: 'Private team', key: 'EX', private: true, archived_at: null }], statuses: [], members: [], issues: [] };
const id = '11111111-1111-4111-8111-111111111111'; const hash = 'a'.repeat(64);
const attachment: Attachment = { id, document_id: null, original_name: 'private-file.txt', size_bytes: 4, sha256: hash, state: 'pending', detected_mime: 'text/plain', scan_status: 'pending', version: '1', etag: '"attachment:1"', expires_at: null, created_at: '2026-10-18T12:00:00Z', can_delete: true };
const json = (value: unknown) => new Response(JSON.stringify(value), { status: 200 });
let container: HTMLDivElement; let root: Root; let fetchMock: ReturnType<typeof vi.fn>; let finishReservation!: (value: Response) => void;
beforeEach(() => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true); HTMLDialogElement.prototype.showModal = function () { this.open = true; }; HTMLDialogElement.prototype.close = function () { this.open = false; };
  unmounts = 0; vi.mocked(hashFile).mockResolvedValue(hash); vi.mocked(uploadBrowserFile).mockReset(); let reservations = 0;
  fetchMock = vi.fn((url: string, options: RequestInit = {}) => {
    if (url.endsWith('/capabilities')) return Promise.resolve(json({ features: { attachments: true }, limits: { max_upload_bytes: 25 * 1024 * 1024 } }));
    if (url.endsWith('/attachments') && options.method === 'POST') return ++reservations === 1 ? new Promise<Response>(resolve => { finishReservation = resolve; }) : Promise.resolve(json(attachment));
    if (url.endsWith(`/attachments/${id}`) && options.method === 'DELETE') return Promise.resolve(new Response(null, { status: 204 }));
    if (url.endsWith(`/attachments/${id}`)) return Promise.resolve(json(attachment));
    if (url.endsWith('/issues')) return Promise.resolve(json({ id: 'created' }));
    throw new Error(`Unexpected request: ${url}`);
  }); vi.stubGlobal('fetch', fetchMock);
  container = document.createElement('div'); document.body.append(container); root = createRoot(container);
});
afterEach(async () => { await act(async () => root.unmount()); container.remove(); vi.restoreAllMocks(); vi.unstubAllGlobals(); });
const button = (name: string) => [...container.querySelectorAll<HTMLButtonElement>('button')].find(item => item.textContent?.trim() === name)!;
async function click(name: string) { await act(async () => button(name).click()); }

it('retains and reconciles a pending reservation through denied project access', async () => {
  const created = vi.fn(); await act(async () => root.render(<CreateIssue workspaceId="w" data={data} session={session} close={() => {}} created={created}/>));
  const title = container.querySelector<HTMLInputElement>('input[placeholder="What needs to change?"]')!;
  await act(async () => { Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(title, 'Private title'); title.dispatchEvent(new Event('input', { bubbles: true })); });
  await click('Add file'); expect(uploads.rows).toHaveLength(1); expect(uploads.rows[0].phase).toBe('reserving');
  const editor = container.querySelector('[data-testid="editor"]')!; const firstPost = fetchMock.mock.calls.find(([url, options]) => String(url).endsWith('/attachments') && options.method === 'POST')!;
  await click('Deny project access');
  const concealed = editor.closest('section')!; expect(concealed.hidden).toBe(true); expect(concealed.hasAttribute('inert')).toBe(true); expect(concealed.getAttribute('aria-hidden')).toBe('true');
  expect(container.querySelector('input[placeholder="What needs to change?"]')).toBeNull(); expect(container.textContent).not.toContain('Private title'); expect(container.textContent).not.toContain('Private team');
  expect(button('Add file').disabled).toBe(true); expect(document.activeElement?.getAttribute('role')).toBe('alert'); expect(unmounts).toBe(0); expect((firstPost[1] as RequestInit).signal?.aborted).toBe(false);
  await act(async () => container.querySelector('form')!.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }))); expect(created).not.toHaveBeenCalled();
  await click('Restore project access'); expect(container.querySelector('[data-testid="editor"]')).toBe(editor); expect(uploads.rows).toHaveLength(1); expect(container.querySelector<HTMLButtonElement>('button[type="submit"]')!.disabled).toBe(true);
  await click('Cancel file'); expect(uploads.rows).toHaveLength(0);
  const posts = fetchMock.mock.calls.filter(([url, options]) => String(url).endsWith('/attachments') && options.method === 'POST'); expect(posts).toHaveLength(2);
  expect(posts[0][1].body).toBe(posts[1][1].body); expect(new Headers(posts[0][1].headers).get('Idempotency-Key')).toBe(new Headers(posts[1][1].headers).get('Idempotency-Key'));
  expect(fetchMock.mock.calls.filter(([, options]) => options.method === 'DELETE')).toHaveLength(1); expect(container.querySelector<HTMLButtonElement>('button[type="submit"]')!.disabled).toBe(false);
  await act(async () => finishReservation(json(attachment))); expect(uploadBrowserFile).not.toHaveBeenCalled(); expect(created).not.toHaveBeenCalled();
});
