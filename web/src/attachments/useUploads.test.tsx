// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useUploads } from './useUploads';
import { AttachmentProvider } from './context';
import { hashFile, uploadBrowserFile } from './browser';
import { pickAttachments, uploadAttachment, releasePickedAttachment } from '../desktop/bridge';
import { isTauri } from '@tauri-apps/api/core';
import type { Attachment, AttachmentDraftState } from './types';
import type { Session } from '../types';
vi.mock('@tauri-apps/api/core', () => ({ isTauri: vi.fn(() => false) }));
vi.mock('../desktop/bridge', () => ({ pickAttachments: vi.fn(), uploadAttachment: vi.fn(), releasePickedAttachment: vi.fn().mockResolvedValue(undefined) }));
vi.mock('./browser', async original => ({ ...await original<typeof import('./browser')>(), hashFile: vi.fn(), uploadBrowserFile: vi.fn() }));
const id = '11111111-1111-4111-8111-111111111111'; const hash = 'a'.repeat(64);
const record = (state: Attachment['state'] = 'pending'): Attachment => ({ id, document_id: null, original_name: 'notes.txt', size_bytes: 4, sha256: hash, state, detected_mime: 'text/plain', scan_status: state === 'ready' ? 'not_scanned' : 'pending', version: '1', etag: '"a1"', expires_at: null, created_at: '2026-10-18T12:00:00Z', can_delete: true });
const session: Session = { audience: 'web', user: { id: 'u', display_name: 'Example Owner', email: 'owner@example.test' }, csrf_token: 'csrf', workspaces: [] };
const json = (data: unknown, status = 200) => new Response(JSON.stringify(data), { status });
const capabilities = { features: { attachments: true }, limits: { max_upload_bytes: 25 * 1024 * 1024 } };
let container: HTMLDivElement; let root: Root; let uploads: ReturnType<typeof useUploads>; let draftState: AttachmentDraftState; let ready: ReturnType<typeof vi.fn<(value: Attachment, anchor: string) => boolean>>; let fetchMock: ReturnType<typeof vi.fn>; let forget: ReturnType<typeof vi.fn>;
function Surface({team='t', enabled=true}:{team?:string; enabled?:boolean}) { uploads = useUploads({ target: { team_id: team }, enabled, prepareAnchor: () => 'anchor', forgetAnchor: forget, ready, changed: value => { draftState = value; } }); return <span>{uploads.rows.map(row => `${row.name}:${row.phase}:${row.error ?? ''}`).join('|')}</span>; }
async function render(token = 'csrf', team = 't', enabled = true, user = 'u') { await act(async () => root.render(<AttachmentProvider workspaceId="w" session={{ ...session, user: { ...session.user, id: user }, csrf_token: token }}><Surface team={team} enabled={enabled} /></AttachmentProvider>)); }
async function add() { await act(async () => uploads.addBrowserFiles([new File(['body'], 'notes.txt', { type: 'text/plain' })])); }
beforeEach(() => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true); container = document.createElement('div'); document.body.append(container); root = createRoot(container); ready = vi.fn(() => true); forget = vi.fn(); vi.mocked(isTauri).mockReturnValue(false); vi.mocked(hashFile).mockResolvedValue(hash); vi.mocked(uploadBrowserFile).mockResolvedValue(record('ready')); vi.mocked(uploadAttachment).mockResolvedValue(record('ready'));
  fetchMock = vi.fn((url: string, options: RequestInit) => Promise.resolve(url.endsWith('/capabilities') ? json(capabilities) : options.method === 'DELETE' ? new Response(null, { status: 204 }) : json(record()))); vi.stubGlobal('fetch', fetchMock);
});
afterEach(async () => { await act(async () => root.unmount()); container.remove(); vi.clearAllMocks(); vi.unstubAllGlobals(); });

describe('attachment draft lifecycle', () => {
  it('releases an initial native selection when its target changes while the picker is open', async () => {
    vi.mocked(isTauri).mockReturnValue(true); let finish!: (value: Awaited<ReturnType<typeof pickAttachments>>) => void;
    vi.mocked(pickAttachments).mockImplementationOnce(() => new Promise(resolve => { finish = resolve; })); await render();
    let selecting!: Promise<void>; await act(async () => { selecting = uploads.pickNative('anchor'); }); await render('csrf', 'other'); await render('csrf', 't');
    await act(async () => { finish([{ handle: 'picked', original_name: 'notes.txt', size_bytes: 4, sha256: hash, mime_hint: 'text/plain' }]); await selecting; });
    expect(releasePickedAttachment).toHaveBeenCalledWith('picked'); expect(forget).toHaveBeenCalledWith('anchor'); expect(uploadAttachment).not.toHaveBeenCalled(); expect(ready).not.toHaveBeenCalled(); expect(uploads.rows).toHaveLength(0); expect(uploads.notice).toContain('Choose the file again');
    expect(fetchMock.mock.calls.some(([url, options]) => url.endsWith('/attachments') && options.method === 'POST')).toBe(false);
  });
  it('keeps an initial native selection valid across same-user reauthentication', async () => {
    vi.mocked(isTauri).mockReturnValue(true); let finish!: (value: Awaited<ReturnType<typeof pickAttachments>>) => void;
    vi.mocked(pickAttachments).mockImplementationOnce(() => new Promise(resolve => { finish = resolve; })); await render();
    let selecting!: Promise<void>; await act(async () => { selecting = uploads.pickNative('anchor'); }); await render('new-csrf');
    await act(async () => { finish([{ handle: 'picked', original_name: 'notes.txt', size_bytes: 4, sha256: hash, mime_hint: 'text/plain' }]); await selecting; });
    expect(uploadAttachment).toHaveBeenCalledTimes(1); expect(ready).toHaveBeenCalledTimes(1);
  });
  it('does not adopt a picker selection after an account change', async () => {
    vi.mocked(isTauri).mockReturnValue(true); let finish!: (value: Awaited<ReturnType<typeof pickAttachments>>) => void;
    vi.mocked(pickAttachments).mockImplementationOnce(() => new Promise(resolve => { finish = resolve; })); await render();
    let selecting!: Promise<void>; await act(async () => { selecting = uploads.pickNative('anchor'); }); await render('csrf', 't', true, 'other');
    await act(async () => { finish([{ handle: 'picked', original_name: 'notes.txt', size_bytes: 4, sha256: hash, mime_hint: 'text/plain' }]); await selecting; });
    expect(releasePickedAttachment).toHaveBeenCalledWith('picked'); expect(uploadAttachment).not.toHaveBeenCalled(); expect(ready).not.toHaveBeenCalled();
  });
  it('discards a browser picker selection after its scope changes, even if it changes back', async () => {
    await render(); const revision = uploads.pickerRevision(); await render('csrf', 'other'); await render();
    await act(async () => uploads.addBrowserFiles([new File(['body'], 'notes.txt')], 'anchor', revision));
    expect(forget).toHaveBeenCalledWith('anchor'); expect(hashFile).not.toHaveBeenCalled(); expect(uploads.rows).toHaveLength(0); expect(ready).not.toHaveBeenCalled(); expect(uploads.notice).toContain('Choose the file again');
  });
  it('does not reserve after editing is paused while hashing, and retries when editing resumes', async () => {
    let finish!: (value: string) => void; vi.mocked(hashFile).mockImplementationOnce(() => new Promise(resolve => { finish = resolve; }));
    await render(); await add(); const key = uploads.rows[0].key; await render('csrf', 't', false);
    await act(async () => finish(hash)); expect(uploads.rows[0].phase).toBe('failed'); expect(uploads.rows[0].error).toContain('Editing is paused');
    expect(fetchMock.mock.calls.some(([url, options]) => url.endsWith('/attachments') && options.method === 'POST')).toBe(false); expect(uploadBrowserFile).not.toHaveBeenCalled();
    await render(); await act(async () => uploads.retry(key)); expect(ready).toHaveBeenCalledTimes(1); expect(uploads.rows).toHaveLength(0);
  });
  it('does not reserve after hashing across a target change, even when the original target returns', async () => {
    let finish!: (value: string) => void; vi.mocked(hashFile).mockImplementationOnce(() => new Promise(resolve => { finish = resolve; }));
    await render(); await add(); const key = uploads.rows[0].key; await render('csrf', 'other'); await render();
    await act(async () => finish(hash)); expect(uploads.rows[0].error).toContain('document changed');
    expect(fetchMock.mock.calls.some(([url, options]) => url.endsWith('/attachments') && options.method === 'POST')).toBe(false); expect(ready).not.toHaveBeenCalled();
    await act(async () => { expect(await uploads.cancel(key)).toBe(true); }); expect(uploads.rows).toHaveLength(0);
  });
  it('does not insert a transfer that completes after a target changes and returns, but allows cleanup', async () => {
    let finish!: (value: Attachment) => void; vi.mocked(uploadBrowserFile).mockImplementationOnce(() => new Promise(resolve => { finish = resolve; }));
    await render(); await add(); const key = uploads.rows[0].key; await render('csrf', 'other'); await render();
    await act(async () => finish(record('ready'))); expect(ready).not.toHaveBeenCalled(); expect(uploads.rows[0].error).toContain('document changed');
    await act(async () => { expect(await uploads.cancel(key)).toBe(true); }); expect(fetchMock.mock.calls.some(([, options]) => options.method === 'DELETE')).toBe(true); expect(uploads.rows).toHaveLength(0);
  });
  it('conceals another target’s pending upload while preserving its unsaved guard and cleanup', async () => {
    let finish!: (value: Attachment) => void; vi.mocked(uploadBrowserFile).mockImplementationOnce(() => new Promise(resolve => { finish = resolve; }));
    await render(); await add(); const key = uploads.rows[0].key; expect(container.textContent).toContain('notes.txt');
    await render('csrf', 'other'); expect(container.textContent).not.toContain('notes.txt'); expect(uploads.rows).toHaveLength(0); expect(draftState.pending).toBe(true);
    await render(); expect(container.textContent).toContain('notes.txt'); await act(async () => finish(record('ready')));
    expect(ready).not.toHaveBeenCalled(); await act(async () => { expect(await uploads.cancel(key)).toBe(true); }); expect(draftState.pending).toBe(false);
  });
  it('locally cancels an unreserved hash after a target change without contacting the server', async () => {
    let finish!: (value: string) => void; vi.mocked(hashFile).mockImplementationOnce(() => new Promise(resolve => { finish = resolve; }));
    await render(); await add(); await render('csrf', 'other'); await act(async () => { expect(await draftState.cancelAll()).toBe(true); });
    expect(draftState.pending).toBe(false); await act(async () => finish(hash)); expect(ready).not.toHaveBeenCalled();
    expect(fetchMock.mock.calls.some(([url, options]) => url.endsWith('/attachments') && (options.method === 'POST' || options.method === 'DELETE'))).toBe(false);
  });
  it('requires the original target to reconcile a reserved upload after a target change', async () => {
    let finish!: (value: Attachment) => void; vi.mocked(uploadBrowserFile).mockImplementationOnce(() => new Promise(resolve => { finish = resolve; }));
    await render(); await add(); const key = uploads.rows[0].key; await render('csrf', 'other');
    await act(async () => { expect(await draftState.cancelAll()).toBe(false); }); expect(uploads.notice).toContain('Return to it'); expect(draftState.pending).toBe(true);
    expect(fetchMock.mock.calls.some(([, options]) => options.method === 'DELETE')).toBe(false); await render();
    await act(async () => { expect(await uploads.cancel(key)).toBe(true); }); await act(async () => finish(record('ready'))); expect(ready).not.toHaveBeenCalled(); expect(draftState.pending).toBe(false);
  });
  it('shares cancellation when row cancel and discard overlap', async () => {
    let finishTransfer!: (value: Attachment) => void; vi.mocked(uploadBrowserFile).mockImplementationOnce(() => new Promise(resolve => { finishTransfer = resolve; }));
    let finishGet!: (value: Response) => void;
    fetchMock.mockImplementation((url: string, options: RequestInit) => {
      if (url.endsWith('/capabilities')) return Promise.resolve(json(capabilities));
      if (options.method === 'DELETE') return Promise.resolve(new Response(null, { status: 204 }));
      if (url.endsWith(`/attachments/${id}`)) return new Promise((resolve, reject) => { finishGet = resolve; options.signal?.addEventListener('abort', () => reject(new DOMException('Canceled', 'AbortError'))); });
      return Promise.resolve(json(record()));
    });
    await render(); await add(); const key = uploads.rows[0].key; let first!: Promise<boolean>; let second!: Promise<boolean>;
    await act(async () => { first = uploads.cancel(key); second = draftState.cancelAll(); });
    await act(async () => { finishGet(json(record())); expect(await first).toBe(true); expect(await second).toBe(true); });
    expect(fetchMock.mock.calls.filter(([, options]) => options.method === 'DELETE')).toHaveLength(1); expect(draftState.pending).toBe(false);
    await act(async () => finishTransfer(record('ready'))); expect(ready).not.toHaveBeenCalled();
  });
  it('allows a failed cancellation to be retried', async () => {
    let finishTransfer!: (value: Attachment) => void; vi.mocked(uploadBrowserFile).mockImplementationOnce(() => new Promise(resolve => { finishTransfer = resolve; })); let deletes = 0;
    fetchMock.mockImplementation((url: string, options: RequestInit) => url.endsWith('/capabilities') ? Promise.resolve(json(capabilities)) : options.method === 'DELETE' ? (++deletes === 1 ? Promise.reject(new TypeError('Offline')) : Promise.resolve(new Response(null, { status: 204 }))) : Promise.resolve(json(record())));
    await render(); await add(); const key = uploads.rows[0].key;
    await act(async () => { expect(await uploads.cancel(key)).toBe(false); }); expect(uploads.rows[0].error).toContain('Cancellation could not be confirmed');
    await act(async () => { expect(await uploads.cancel(key)).toBe(true); }); expect(deletes).toBe(2); expect(draftState.pending).toBe(false);
    await act(async () => finishTransfer(record('ready'))); expect(ready).not.toHaveBeenCalled();
  });
  it('releases a newly reselected native handle when the document scope changes while its picker is open', async () => {
    vi.mocked(isTauri).mockReturnValue(true); vi.mocked(pickAttachments).mockResolvedValue([{ handle: 'old', original_name: 'notes.txt', size_bytes: 4, sha256: hash, mime_hint: 'text/plain' }]); vi.mocked(uploadAttachment).mockRejectedValueOnce(new Error('Handle expired')); await render(); await act(async () => uploads.pickNative('anchor')); const key = uploads.rows[0].key;
    let finish!: (value: Awaited<ReturnType<typeof pickAttachments>>) => void; vi.mocked(pickAttachments).mockImplementationOnce(() => new Promise(resolve => { finish = resolve; }));
    let selecting!: Promise<void>; await act(async () => { selecting = uploads.reselect(key); }); await render('csrf', 'other');
    await act(async () => { finish([{ handle: 'new', original_name: 'notes.txt', size_bytes: 4, sha256: hash, mime_hint: 'text/plain' }]); await selecting; }); expect(releasePickedAttachment).toHaveBeenCalledWith('new'); expect(uploadAttachment).toHaveBeenCalledTimes(1);
  });

  it('inserts only a ready response and keeps raw files out of JSON reservation', async () => {
    await render(); await add(); expect(ready).toHaveBeenCalledWith(record('ready'), 'anchor'); expect(uploads.rows).toHaveLength(0); expect(draftState.pending).toBe(false);
    const [, options] = fetchMock.mock.calls.find(([url, options]) => url.endsWith('/attachments') && options.method === 'POST')!;
    expect(JSON.parse(options.body)).toEqual({ team_id: 't', original_name: 'notes.txt', size_bytes: 4, sha256: hash, mime_hint: 'text/plain' }); expect(new Headers(options.headers).get('Idempotency-Key')).toBeTruthy();
  });
  it('checks an unknown transfer result before retrying bytes and inserts a committed ready file once', async () => {
    vi.mocked(uploadBrowserFile).mockRejectedValue(new Error('Connection lost')); await render(); await add(); expect(draftState.pending).toBe(true); expect(ready).not.toHaveBeenCalled(); const key = uploads.rows[0].key;
    fetchMock.mockImplementation((url: string) => Promise.resolve(json(url.endsWith('/capabilities') ? capabilities : record('ready'))));
    await act(async () => uploads.retry(key)); expect(uploadBrowserFile).toHaveBeenCalledTimes(1); expect(ready).toHaveBeenCalledTimes(1); expect(uploads.rows).toHaveLength(0);
  });
  it('replays an ambiguous reservation with exactly the same key and immutable body', async () => {
    let posts = 0; fetchMock.mockImplementation((url: string, options: RequestInit) => url.endsWith('/capabilities') ? Promise.resolve(json(capabilities)) : options.method === 'POST' && posts++ === 0 ? Promise.reject(new TypeError('Offline')) : Promise.resolve(json(record())));
    await render(); await add(); const key = uploads.rows[0].key; await act(async () => uploads.retry(key));
    const postsSeen = fetchMock.mock.calls.filter(([, options]) => options.method === 'POST'); expect(postsSeen).toHaveLength(2); expect(postsSeen[0][1].body).toBe(postsSeen[1][1].body); expect(new Headers(postsSeen[0][1].headers).get('Idempotency-Key')).toBe(new Headers(postsSeen[1][1].headers).get('Idempotency-Key')); expect(ready).toHaveBeenCalledTimes(1);
  });
  it('keeps quarantine and rejected bytes out of the document, and retries only the scanner', async () => {
    vi.mocked(uploadBrowserFile).mockResolvedValue({ ...record('quarantined'), scan_status: 'unavailable' }); await render(); await add(); expect(ready).not.toHaveBeenCalled(); expect(uploads.rows[0].phase).toBe('waiting'); const key = uploads.rows[0].key;
    fetchMock.mockImplementation((_url: string, options: RequestInit) => Promise.resolve(json(options.method === 'POST' ? record('ready') : record('quarantined'))));
    await act(async () => uploads.retry(key)); expect(uploadBrowserFile).toHaveBeenCalledTimes(1); expect(fetchMock.mock.calls.some(([url]) => url.endsWith('/finalize'))).toBe(true); expect(ready).toHaveBeenCalledTimes(1);
    ready.mockClear(); vi.mocked(uploadBrowserFile).mockResolvedValue({ ...record('rejected'), scan_status: 'infected' }); fetchMock.mockImplementation(() => Promise.resolve(json(record()))); await add(); expect(ready).not.toHaveBeenCalled(); expect(uploads.rows[0].error).toContain('rejected');
  });
  it('aborts a pending transfer, reconciles current ETag, and never inserts a late ready response', async () => {
    let finish!: (value: Attachment) => void; vi.mocked(uploadBrowserFile).mockImplementation(() => new Promise(resolve => { finish = resolve; })); await render(); await add(); const key = uploads.rows[0].key;
    expect(draftState.pending).toBe(true); expect(draftState.busy).toBe(true); await act(async () => { expect(await uploads.cancel(key)).toBe(true); });
    expect(vi.mocked(uploadBrowserFile).mock.calls[0][4].aborted).toBe(true); await act(async () => finish(record('ready'))); expect(ready).not.toHaveBeenCalled(); expect(uploads.rows).toHaveLength(0); expect(fetchMock.mock.calls.some(([, options]) => options.method === 'DELETE')).toBe(true);
  });
  it('does not restart a canceled reservation with its cleanup controller', async () => {
    let late!: (value: Response) => void; let posts = 0;
    fetchMock.mockImplementation((url: string, options: RequestInit) => url.endsWith('/capabilities') ? Promise.resolve(json(capabilities)) : options.method === 'POST' && posts++ === 0 ? new Promise(resolve => { late = resolve; }) : Promise.resolve(options.method === 'DELETE' ? new Response(null, { status: 204 }) : json(record())));
    await render(); await add(); await act(async () => { expect(await uploads.cancel(uploads.rows[0].key)).toBe(true); }); await act(async () => late(json(record()))); expect(uploadBrowserFile).not.toHaveBeenCalled(); expect(ready).not.toHaveBeenCalled();
  });
  it('preserves pending uploads across same-user reauthentication and uses the current CSRF on retry', async () => {
    vi.mocked(uploadBrowserFile).mockRejectedValueOnce(new Error('Sign in again')); await render(); await add(); const key = uploads.rows[0].key; await render('new-csrf'); expect(uploads.rows[0].key).toBe(key); await act(async () => uploads.retry(key)); expect(vi.mocked(uploadBrowserFile).mock.calls[1][3]).toBe('new-csrf'); expect(ready).toHaveBeenCalledTimes(1);
  });
  it('enforces the configured limit before allocating or reserving and caps selection batches', async () => {
    fetchMock.mockImplementation(() => Promise.resolve(json({ ...capabilities, limits: { max_upload_bytes: 2 } }))); await render(); await add(); expect(hashFile).not.toHaveBeenCalled(); expect(uploadBrowserFile).not.toHaveBeenCalled(); expect(uploads.notice).toContain('2 B');
    await act(async () => uploads.addBrowserFiles(Array.from({ length: 11 }, () => new File(['x'], 'x.txt')))); expect(uploads.notice).toContain('up to 10');
  });
  it('lets expired native access be reselected only with matching bytes and preserves the reservation', async () => {
    vi.mocked(isTauri).mockReturnValue(true); vi.mocked(pickAttachments).mockResolvedValue([{ handle: 'old', original_name: 'notes.txt', size_bytes: 4, sha256: hash, mime_hint: 'text/plain' }]); vi.mocked(uploadAttachment).mockRejectedValueOnce(new Error('Handle expired')); await render(); await act(async () => uploads.pickNative('anchor')); const key = uploads.rows[0].key;
    expect(uploads.rows[0].error).toContain('30 minutes'); vi.mocked(pickAttachments).mockResolvedValueOnce([{ handle: 'wrong', original_name: 'notes.txt', size_bytes: 4, sha256: 'b'.repeat(64), mime_hint: 'text/plain' }]); await act(async () => uploads.reselect(key)); expect(uploadAttachment).toHaveBeenCalledTimes(1); expect(releasePickedAttachment).toHaveBeenCalledWith('wrong');
    vi.mocked(pickAttachments).mockResolvedValueOnce([{ handle: 'new', original_name: 'notes.txt', size_bytes: 4, sha256: hash, mime_hint: 'text/plain' }]); await act(async () => uploads.reselect(key)); expect(uploadAttachment).toHaveBeenCalledTimes(2); expect(vi.mocked(uploadAttachment).mock.calls[1][0]).toMatchObject({ attachment_id: id, handle: 'new' }); expect(ready).toHaveBeenCalledTimes(1);
  });
});
