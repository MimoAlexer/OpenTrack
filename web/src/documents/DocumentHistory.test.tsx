// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { DocumentHistory, type DocumentHistoryProps } from './DocumentHistory';
import { ReferenceProvider } from './ReferenceProvider';
import { fromPlainText } from './model';
import type { Session } from '../types';

const session: Session = { audience: 'web', user: { id: 'u', display_name: 'Example Owner', email: 'owner@example.test' }, csrf_token: 'csrf', workspaces: [{ id: 'w', name: 'Workspace', slug: 'workspace', role: 'member' }] };
const props: DocumentHistoryProps = { workspaceId: 'w', documentId: 'd', session, refreshVersion: '1' };
const summary = (revision = '2', migrated = false, author: { id: string; display_name: string } | null = { id: 'author', display_name: 'Example Editor' }) => ({ revision, migrated, author, created_at: '2026-10-22T10:00:00Z', expires_at: '2026-11-21T10:00:00Z', schema_version: 1 });
const revision = (version = '2', text = 'Historical brief') => ({ ...summary(version), body: fromPlainText(text), plain_text: text, mentions: [], issue_references: [] });
const page = (data = [summary()], next_cursor: string | null = null) => ({ data, has_more: !!next_cursor, next_cursor });
const json = (data: unknown, status = 200) => new Response(JSON.stringify(data), { status });
function deferred<T>() { let resolve!: (value: T) => void; const promise = new Promise<T>(done => { resolve = done; }); return { promise, resolve }; }
let container: HTMLDivElement; let root: Root;
beforeEach(() => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true);
  HTMLDialogElement.prototype.showModal = function () { this.setAttribute('open', ''); };
  HTMLDialogElement.prototype.close = function () { this.removeAttribute('open'); };
  container = document.createElement('div'); document.body.append(container); root = createRoot(container);
});
afterEach(async () => { await act(async () => root.unmount()); container.remove(); vi.unstubAllGlobals(); vi.restoreAllMocks(); });
async function render(overrides: Partial<DocumentHistoryProps> = {}) { await act(async () => root.render(<DocumentHistory {...props} {...overrides} />)); }
function button(label: string) { const value = [...container.querySelectorAll('button')].find(item => item.textContent === label || item.getAttribute('aria-label') === label); if (!value) throw new Error(`Missing button: ${label}`); return value; }
async function click(label: string) { await act(async () => button(label).click()); }

it('loads metadata lazily, distinguishes explicit migrated provenance and leaves the live draft untouched', async () => {
  const fetch = vi.fn((url: string, _options: RequestInit) => Promise.resolve(json(url.includes('?') ? page([summary('3'), summary('2', false, null), summary('1', true, null)]) : { ...revision('1'), ...summary('1', true, null) })));
  vi.stubGlobal('fetch', fetch);
  await act(async () => root.render(<><textarea aria-label="Current draft" defaultValue="Unsaved current draft" /><DocumentHistory {...props} /></>));
  const draft = container.querySelector('textarea')!; expect(fetch).not.toHaveBeenCalled();
  await click('Brief history'); expect(fetch).toHaveBeenCalledTimes(1);
  expect(fetch.mock.calls[0][0]).toBe('/api/v1/workspaces/w/documents/d/revisions?limit=20');
  expect(button('View revision 3').textContent).toContain('Example Editor');
  expect(button('View revision 2').textContent).toContain('Unknown author'); expect(button('View revision 2').textContent).not.toContain('Migrated snapshot');
  expect(button('View revision 1').textContent).toContain('Migrated snapshot · Unknown author · Captured');
  expect(button('View revision 1').querySelector('time')?.dateTime).toBe(summary().created_at);
  await click('View revision 1'); expect(container.textContent).toContain('Historical brief'); expect(container.textContent).toContain('earlier author and editing time are unknown');
  expect(draft.value).toBe('Unsaved current draft'); expect(container.querySelector('[contenteditable="true"]')).toBeNull();
  expect(fetch.mock.calls.every(([, options]) => !options.method || options.method === 'GET')).toBe(true);
  await click('Done'); expect(container.querySelector('dialog')).toBeNull(); expect(container.querySelector('textarea')).toBe(draft); expect(draft.value).toBe('Unsaved current draft');
});

it('appends cursor pages, fetches only the selected revision and suppresses an earlier selection arriving late', async () => {
  const old = deferred<Response>(); let oldSignal!: AbortSignal;
  const fetch = vi.fn((url: string, options: RequestInit) => {
    if (url.endsWith('/2')) { oldSignal = options.signal!; return old.promise; }
    if (url.endsWith('/1')) return Promise.resolve(json(revision('1', 'Selected older text')));
    return Promise.resolve(json(url.includes('cursor=') ? page([summary('1')]) : page([summary('2')], 'older/+=token')));
  }); vi.stubGlobal('fetch', fetch); await render(); await click('Brief history'); await click('View revision 2');
  expect(container.textContent).toContain('Loading revision'); await click('Load older revisions');
  expect(fetch.mock.calls.some(([url]) => url.endsWith('&cursor=older%2F%2B%3Dtoken'))).toBe(true);
  expect(container.querySelectorAll('[aria-label="Saved brief revisions"] li')).toHaveLength(2);
  await click('View revision 1'); expect(oldSignal.aborted).toBe(true); expect(container.textContent).toContain('Selected older text');
  expect(button('View revision 1').getAttribute('aria-pressed')).toBe('true'); expect(button('View revision 2').getAttribute('aria-pressed')).toBe('false');
  await act(async () => old.resolve(json(revision('2', 'Late stale text'))));
  expect(container.textContent).not.toContain('Late stale text'); expect(container.textContent).toContain('Selected older text');
  expect(fetch.mock.calls.filter(([url]) => /\/revisions\/\d+$/.test(url))).toHaveLength(2);
  expect(container.textContent).not.toContain('Load older revisions');
});

it('renders historical mentions and issue references only from the selected revision, without inheriting live labels', async () => {
  const person = '11111111-1111-4111-8111-111111111111'; const issue = '22222222-2222-4222-8222-222222222222';
  const body = { type: 'doc', content: [{ type: 'paragraph', content: [{ type: 'userMention', attrs: { id: person } }, { type: 'text', text: ' ' }, { type: 'issueReference', attrs: { id: issue } }] }] };
  vi.stubGlobal('fetch', vi.fn((url: string) => Promise.resolve(json(url.includes('?') ? page([summary('2'), summary('1')]) : {
    ...revision(url.endsWith('/2') ? '2' : '1'), body,
    mentions: [{ id: person, available: url.endsWith('/2'), display_name: url.endsWith('/2') ? 'Example Historical Member' : null }],
    issue_references: [{ id: issue, available: url.endsWith('/2'), key: url.endsWith('/2') ? 'OLD-2' : null, title: url.endsWith('/2') ? 'Historical issue title' : null }],
  }))));
  await act(async () => root.render(<ReferenceProvider workspaceId="w" session={session} target={{ document_id: 'd' }} projectionScope="u:csrf" references={[{ id: person, display_name: 'Example Current Member', available: true }]} issueReferences={[{ id: issue, key: 'NOW-1', title: 'Current issue title', available: true }]}><DocumentHistory {...props} /></ReferenceProvider>));
  await click('Brief history'); await click('View revision 2');
  expect(container.textContent).toContain('@Example Historical Member'); expect(container.querySelector('a[aria-label="OLD-2: Historical issue title"]')).not.toBeNull();
  expect(container.textContent).not.toContain('Example Current Member'); expect(container.textContent).not.toContain('NOW-1');
  await click('View revision 1'); expect(container.textContent).toContain('@someone'); expect(container.textContent).toContain('#issue');
  expect(container.textContent).not.toContain('Example Historical Member'); expect(container.querySelector('a')).toBeNull();
});

it('keeps an unknown schema read-only and displays its plain text as inert fallback', async () => {
  const fallback = '<img src=x onerror=alert(1)> Preserved future text';
  vi.stubGlobal('fetch', vi.fn((url: string) => Promise.resolve(json(url.includes('?') ? page([{ ...summary(), schema_version: 2 }]) : { ...revision(), schema_version: 2, body: fromPlainText('Do not interpret the newer body'), plain_text: fallback }))));
  await render(); await click('Brief history'); await click('View revision 2');
  expect(container.textContent).toContain('newer format'); expect(container.querySelector('pre')?.textContent).toBe(fallback);
  expect(container.querySelector('img')).toBeNull(); expect(container.querySelector('script')).toBeNull(); expect(container.textContent).not.toContain('Do not interpret the newer body');
});

it('shows loading, recovers a failed list request and reports an empty history', async () => {
  const first = deferred<Response>(); const fetch = vi.fn().mockReturnValueOnce(first.promise).mockResolvedValueOnce(json(page([]))); vi.stubGlobal('fetch', fetch);
  await render(); await click('Brief history'); expect(container.querySelector('[role="status"]')?.textContent).toContain('Loading brief history');
  await act(async () => first.resolve(json({ code: 'unavailable', detail: 'History temporarily unavailable' }, 503)));
  expect(container.querySelector('[role="alert"]')?.textContent).toContain('History temporarily unavailable');
  await click('Try again'); expect(container.querySelector('[role="alert"]')).toBeNull(); expect(container.textContent).toContain('No saved revisions are available');
  expect(fetch.mock.calls[0][0]).toBe(fetch.mock.calls[1][0]);
});

it('retries a failed selected body without refetching the history metadata', async () => {
  const fetch = vi.fn().mockResolvedValueOnce(json(page())).mockResolvedValueOnce(json({ code: 'unavailable', detail: 'Revision temporarily unavailable' }, 503)).mockResolvedValueOnce(json(revision()));
  vi.stubGlobal('fetch', fetch); await render(); await click('Brief history'); await click('View revision 2');
  expect(container.querySelector('[role="alert"]')?.textContent).toContain('Revision temporarily unavailable'); expect(container.textContent).not.toContain('Historical brief');
  await click('Try again'); expect(container.querySelector('[role="alert"]')).toBeNull(); expect(container.textContent).toContain('Historical brief');
  expect(fetch.mock.calls.map(([url]) => url)).toEqual(['/api/v1/workspaces/w/documents/d/revisions?limit=20', '/api/v1/workspaces/w/documents/d/revisions/2', '/api/v1/workspaces/w/documents/d/revisions/2']);
});

it('preserves the current draft when a listed revision expires while its document remains accessible', async () => {
  const onDenied = vi.fn();
  const fetch = vi.fn((url: string) => Promise.resolve(json(url.includes('?') ? page() : url.endsWith('/2') ? { code: 'not_found', detail: 'Not found' } : { id: 'd' }, url.endsWith('/2') ? 404 : 200)));
  vi.stubGlobal('fetch', fetch);
  await act(async () => root.render(<><textarea aria-label="Current draft" defaultValue="Unsaved current draft" /><DocumentHistory {...props} onDenied={onDenied} /></>));
  const draft = container.querySelector('textarea')!;
  await click('Brief history'); await click('View revision 2');
  expect(fetch.mock.calls.at(-1)?.[0]).toBe('/api/v1/workspaces/w/documents/d');
  expect(container.querySelector('[role="alert"]')?.textContent).toContain('This revision is no longer available.');
  expect(container.textContent).not.toContain('hidden because access could not be confirmed'); expect(onDenied).not.toHaveBeenCalled();
  await click('Done'); expect(container.querySelector('textarea')).toBe(draft); expect(draft.value).toBe('Unsaved current draft');
});

it('discards a late owner-denial check after closing an expired revision', async () => {
  const owner = deferred<Response>(); const onDenied = vi.fn(); let signal!: AbortSignal;
  vi.stubGlobal('fetch', vi.fn((url: string, options: RequestInit) => {
    if (url.endsWith('/d')) { signal = options.signal!; return owner.promise; }
    return Promise.resolve(url.endsWith('/2') ? json({ code: 'not_found', detail: 'Not found' }, 404) : json(page()));
  }));
  await render({ onDenied }); await click('Brief history'); await click('View revision 2'); await click('Done');
  expect(signal.aborted).toBe(true);
  await act(async () => owner.resolve(json({ code: 'not_found', detail: 'Not found' }, 404)));
  expect(onDenied).not.toHaveBeenCalled(); expect(container.querySelector('dialog')).toBeNull();
});

const scopeChanges: [string, Partial<DocumentHistoryProps>][] = [
  ['document', { documentId: 'other-document' }],
  ['workspace', { workspaceId: 'other-workspace' }],
  ['credentials', { session: { ...session, csrf_token: 'rotated' } }],
  ['account', { session: { ...session, user: { id: 'other-user', display_name: 'Example Member', email: 'member@example.test' } } }],
];
it.each(scopeChanges)('closes and clears history before a %s change, aborting and discarding a late body', async (_name, overrides) => {
  const pending = deferred<Response>(); let signal!: AbortSignal;
  const fetch = vi.fn((url: string, options: RequestInit) => {
    if (url.endsWith('/2')) { signal = options.signal!; return pending.promise; }
    return Promise.resolve(json(page()));
  }); vi.stubGlobal('fetch', fetch); await render(); await click('Brief history'); await click('View revision 2');
  await render(overrides); expect(signal.aborted).toBe(true); expect(container.querySelector('dialog')).toBeNull(); expect(container.textContent).not.toContain('Example Editor');
  await act(async () => pending.resolve(json(revision('2', 'Previous scope private text'))));
  expect(container.textContent).not.toContain('Previous scope private text'); expect(container.querySelector('dialog')).toBeNull();
  await click('Brief history'); expect(fetch.mock.calls.at(-1)?.[0]).toBe(`/api/v1/workspaces/${overrides.workspaceId ?? 'w'}/documents/${overrides.documentId ?? 'd'}/revisions?limit=20`);
  expect(container.textContent).not.toContain('Previous scope private text');
});

it('revalidates an open history on refresh and discards the former pending revision', async () => {
  const pending = deferred<Response>(); let signal!: AbortSignal;
  const fetch = vi.fn((url: string, options: RequestInit) => {
    if (url.endsWith('/2')) { signal = options.signal!; return pending.promise; }
    return Promise.resolve(json(page()));
  }); vi.stubGlobal('fetch', fetch); await render(); await click('Brief history'); await click('View revision 2');
  fetch.mockImplementation(() => Promise.resolve(json(page([summary('3')])))); await render({ refreshVersion: '2' });
  expect(signal.aborted).toBe(true); expect(container.querySelector('dialog')).not.toBeNull(); expect(button('View revision 3')).toBeDefined(); expect(container.querySelector('[aria-label="Revision 2"]')).toBeNull();
  await act(async () => pending.resolve(json(revision('2', 'Pre-refresh text')))); expect(container.textContent).not.toContain('Pre-refresh text');
});

it.each([401, 403, 404])('purges all content on revision access denial (%s), aborts concurrent paging and ignores its late result', async status => {
  const denied = deferred<Response>(); const older = deferred<Response>(); let olderSignal!: AbortSignal; const onDenied = vi.fn();
  const fetch = vi.fn((url: string, options: RequestInit) => {
    if (url.endsWith('/d')) return Promise.resolve(json({ code: 'not_available', detail: 'History not available' }, status));
    if (url.endsWith('/2')) return denied.promise;
    if (url.includes('cursor=')) { olderSignal = options.signal!; return older.promise; }
    return Promise.resolve(json(page([summary()], 'older')));
  }); vi.stubGlobal('fetch', fetch); await render({ onDenied }); await click('Brief history'); await click('View revision 2'); await click('Load older revisions');
  await act(async () => denied.resolve(json({ code: 'not_available', detail: 'History not available' }, status)));
  expect(onDenied).toHaveBeenCalledOnce(); expect(onDenied.mock.calls[0][0]).toMatchObject({ status }); expect(olderSignal.aborted).toBe(true);
  expect(container.textContent).toContain('hidden because access could not be confirmed'); expect(container.textContent).not.toContain('Example Editor'); expect(container.querySelector('[aria-label="Revision 2"]')).toBeNull();
  await act(async () => older.resolve(json(page([summary('1', false, { id: 'old', display_name: 'Late concealed author' })]))));
  expect(container.textContent).not.toContain('Late concealed author'); expect(onDenied).toHaveBeenCalledOnce();
});

it('conceals an already displayed historical body when paging discovers revoked access', async () => {
  const onDenied = vi.fn();
  vi.stubGlobal('fetch', vi.fn((url: string) => Promise.resolve(url.includes('cursor=') ? json({ code: 'not_found', detail: 'Not found' }, 404) : json(url.endsWith('/2') ? revision('2', 'Previously visible private text') : page([summary()], 'older')))));
  await render({ onDenied }); await click('Brief history'); await click('View revision 2'); expect(container.textContent).toContain('Previously visible private text');
  await click('Load older revisions'); expect(container.textContent).not.toContain('Previously visible private text'); expect(container.textContent).not.toContain('Example Editor'); expect(onDenied).toHaveBeenCalledOnce();
});

it('supports modal cancellation, restores focus and aborts the outstanding history request', async () => {
  const pending = deferred<Response>(); let signal!: AbortSignal;
  vi.stubGlobal('fetch', vi.fn((_url: string, options: RequestInit) => { signal = options.signal!; return pending.promise; }));
  await render(); const trigger = button('Brief history'); trigger.focus(); await click('Brief history'); expect(document.activeElement).toBe(button('Done'));
  await act(async () => container.querySelector('dialog')!.dispatchEvent(new Event('cancel', { bubbles: true, cancelable: true })));
  expect(container.querySelector('dialog')).toBeNull(); expect(document.activeElement).toBe(trigger); expect(signal.aborted).toBe(true);
  await act(async () => pending.resolve(json(page()))); expect(container.querySelector('dialog')).toBeNull(); expect(container.textContent).not.toContain('Example Editor');
});
