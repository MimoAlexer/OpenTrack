// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { App } from './App';
import type { Bootstrap, Session } from './types';

vi.mock('./issues/IssueList', () => ({ IssueList: () => <div>Issue list</div> }));
vi.mock('./issues/IssueEditor', () => ({ IssueEditor: () => <div>Created issue editor</div> }));
vi.mock('./useWorkspaceAccess', () => ({ useWorkspaceAccess: () => {} }));
vi.mock('./documents/Editor', () => ({ StructuredEditor: () => <div contentEditable aria-label="Description" /> }));
vi.mock('./issues/IssueFields', () => ({ IssueFields: () => null }));
vi.mock('./workflows/useIssueWorkflow', () => ({ useIssueWorkflow: () => ({ statuses: [{ id: 'ready', name: 'Ready', category: 'unstarted', is_default: true, archived_at: null, retiring_at: null }], currentIds: new Set(['ready']), error: null, refresh: () => {} }) }));

const session: Session = { audience: 'web', user: { id: 'u', display_name: 'Example Owner', email: 'owner@example.test' }, csrf_token: 'csrf', workspaces: [{ id: 'w', name: 'Workspace', slug: 'workspace', role: 'owner' }] };
const bootstrap: Bootstrap = { auth_epoch: '1', teams: [{ id: 't', name: 'Team', key: 'ENG', private: false, archived_at: null }], statuses: [], members: [], issues: [] };
const json = (value: unknown) => new Response(JSON.stringify(value), { status: 200 });
let container: HTMLDivElement; let root: Root; let fetchMock: ReturnType<typeof vi.fn>; let mode: 'enabled' | 'disabled'; let frames: FrameRequestCallback[];
beforeEach(() => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true); frames = []; mode = 'enabled';
  vi.stubGlobal('requestAnimationFrame', (callback: FrameRequestCallback) => { frames.push(callback); return frames.length; });
  HTMLDialogElement.prototype.showModal = function () { this.open = true; }; HTMLDialogElement.prototype.close = function () { this.open = false; };
  localStorage.clear(); history.replaceState({ otIndex: 0 }, '', '/w/w/issues');
  fetchMock = vi.fn((url: string, options: RequestInit = {}) => {
    if (url.endsWith('/setup/status')) return Promise.resolve(json({ setup_required: false }));
    if (url.endsWith('/auth/session')) return Promise.resolve(json(session));
    if (url.endsWith('/bootstrap')) return Promise.resolve(json(bootstrap));
    if (url.endsWith('/access')) return Promise.resolve(json({ auth_epoch: '1', role: 'owner', archived: false }));
    if (url.endsWith('/preferences')) { if (options.method === 'PATCH') mode = JSON.parse(options.body as string).shortcut_mode; return Promise.resolve(json({ workspace_id: 'w', shortcut_mode: mode, version: '1', etag: '"preferences:1"' })); }
    if (url.endsWith('/capabilities')) return Promise.resolve(json({ features: { attachments: false }, limits: { max_upload_bytes: 0 } }));
    if (url.endsWith('/issues') && options.method === 'POST') return Promise.resolve(json({ id: 'created', key: 'ENG-1', team_id: 't', title: JSON.parse(options.body as string).title }));
    throw new Error(`Unexpected request: ${url}`);
  }); vi.stubGlobal('fetch', fetchMock);
  container = document.createElement('div'); document.body.append(container); root = createRoot(container);
});
afterEach(async () => { await act(async () => root.unmount()); container.remove(); localStorage.clear(); vi.restoreAllMocks(); vi.unstubAllGlobals(); });
async function key(target: EventTarget, key: string, options: KeyboardEventInit = {}) { await act(async () => target.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true, ...options }))); }
async function click(text: string) { const button = [...container.querySelectorAll<HTMLButtonElement>('button')].find(item => item.textContent?.replace('?', '').trim() === text)!; expect(button).toBeTruthy(); await act(async () => button.click()); }
async function setTitle(value: string) { const input = container.querySelector<HTMLInputElement>('input[maxlength="256"]')!; expect(input).toBeTruthy(); await act(async () => { Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(input, value); input.dispatchEvent(new Event('input', { bubbles: true })); }); return input; }

it('creates from C and opens the returned issue through the ordinary create form', async () => {
  await act(async () => root.render(<App />)); await key(window, 'c');
  const input = await setTitle('Keyboard created issue'); expect(document.activeElement).toBe(input);
  await key(input, 'Enter', { ctrlKey: true });
  expect(location.pathname).toBe('/w/w/issues/created'); expect(container.textContent).toContain('Created issue editor');
  const writes = fetchMock.mock.calls.filter(([url, options]) => String(url).endsWith('/issues') && options?.method === 'POST');
  expect(writes).toHaveLength(1); expect(JSON.parse(writes[0][1].body).title).toBe('Keyboard created issue');
});

it('keeps palette creation, visible controls and Mod+Enter available with character shortcuts disabled', async () => {
  await act(async () => root.render(<App />)); await click('Keyboard shortcuts');
  await act(async () => container.querySelector<HTMLInputElement>('input[type="checkbox"]')!.click()); await click('Done');
  expect(mode).toBe('disabled'); await key(window, 'c'); await key(window, 'g'); await key(window, 'm');
  expect(container.querySelector('dialog')).toBeNull(); expect(location.search).toBe('');
  expect(container.querySelector('.topbar-actions')!.textContent).toContain('New issue'); expect(container.querySelector('.topbar-actions kbd')).toBeNull();
  await key(window, 'k', { ctrlKey: true });
  const search = container.querySelector<HTMLInputElement>('input[aria-label="Search commands"]')!;
  expect(document.activeElement).toBe(search); expect(container.textContent).not.toContain('Inbox');
  await key(search, 'Enter'); await act(async () => { for (const frame of frames.splice(0)) frame(0); });
  const input = await setTitle('Created with character shortcuts off'); await key(input, 'Enter', { ctrlKey: true });
  expect(location.pathname).toBe('/w/w/issues/created'); expect(fetchMock.mock.calls.filter(([, options]) => options?.method === 'POST')).toHaveLength(1);
});

it('navigates G then M only while enabled and keeps My work visible', async () => {
  await act(async () => root.render(<App />)); await key(window, 'g'); await key(window, 'm');
  expect(location.search).toBe('?view=mine'); expect(container.querySelector('a[aria-current="page"]')!.textContent).toBe('My work');
  await click('Keyboard shortcuts'); expect(container.textContent).toContain('G then M'); expect(container.textContent).not.toContain('Inbox');
});

it('does not submit a create draft for legacy IME, AltGraph or repeated Mod+Enter', async () => {
  await act(async () => root.render(<App />)); await key(window, 'c');
  const input = await setTitle('Reviewed keyboard issue');
  for (const options of [{ keyCode: 229 }, { isComposing: true }, { altKey: true }, { repeat: true }]) await key(input, 'Enter', { ctrlKey: true, ...options });
  const event = new KeyboardEvent('keydown', { key: 'Enter', ctrlKey: true, bubbles: true, cancelable: true });
  Object.defineProperty(event, 'getModifierState', { value: (name: string) => name === 'AltGraph' });
  await act(async () => input.dispatchEvent(event));
  expect(fetchMock.mock.calls.filter(([, options]) => options?.method === 'POST')).toHaveLength(0);
  expect(input.value).toBe('Reviewed keyboard issue');
  await key(input, 'Enter', { ctrlKey: true }); expect(location.pathname).toBe('/w/w/issues/created');
});
