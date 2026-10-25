// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { CommandPalette } from './CommandPalette';
import { useRecentIssues, type PaletteIssueSource } from './usePaletteIssues';

let container: HTMLDivElement; let root: Root;
beforeEach(() => {
  vi.useFakeTimers(); vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true);
  HTMLDialogElement.prototype.showModal = function () { this.setAttribute('open', ''); };
  HTMLDialogElement.prototype.close = function () { this.removeAttribute('open'); };
  container = document.createElement('div'); document.body.append(container); root = createRoot(container);
});
afterEach(async () => { await act(async () => root.unmount()); container.remove(); vi.useRealTimers(); vi.unstubAllGlobals(); });
const json = (value: unknown, status = 200) => new Response(JSON.stringify(value), { status, headers: { 'Content-Type': 'application/json' } });
const issue = (id: string, title = id, workspace_id = 'workspace') => ({ id, title, workspace_id, key: `EX-${id}`, archived_at: null });
const page = (...items: ReturnType<typeof issue>[]) => ({ data: items, has_more: false, next_cursor: null });
const source = (overrides: Partial<PaletteIssueSource> = {}): PaletteIssueSource => ({ workspaceId: 'workspace', scope: 'account:workspace:epoch', recentIds: [], open: vi.fn(), ...overrides });
function deferred() { let resolve!: (response: Response) => void; const promise = new Promise<Response>(done => { resolve = done; }); return { promise, resolve }; }
async function render(value = source(), close = vi.fn()) { await act(async () => root.render(<CommandPalette commands={[]} close={close} issueSource={value} />)); }
async function tick(ms = 150) { await act(async () => { await vi.advanceTimersByTimeAsync(ms); }); }
async function query(value: string) {
  const input = container.querySelector('input')!;
  await act(async () => { Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(input, value); input.dispatchEvent(new Event('input', { bubbles: true })); });
}
async function enter() { await act(async () => container.querySelector('input')!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true }))); }

it('sends full supplementary queries, preserves rejected text without fallback and resolves only the repaired result', async () => {
  const text = '\u{1F680}'.repeat(256); const overlong = text + '\u{1F680}'; const queries: string[] = [];
  const options = source(); const close = vi.fn(); const frames: FrameRequestCallback[] = [];
  vi.stubGlobal('requestAnimationFrame', (frame: FrameRequestCallback) => { frames.push(frame); return frames.length; });
  const fetch = vi.fn((url: string) => {
    if (url.endsWith('/issues/full')) return Promise.resolve(json(issue('full', text)));
    const request = new URL(url, 'https://example.test'); const value = request.searchParams.get('q')!;
    expect(request.searchParams.get('limit')).toBe('20'); queries.push(value);
    if (value === overlong) return Promise.resolve(json({ code: 'validation_failed', detail: 'Invalid issue filter or page size' }, 422));
    expect(value).toBe(text); return Promise.resolve(json(page(issue('full', text))));
  });
  vi.stubGlobal('fetch', fetch);
  await render(options, close); await query(text); await tick(149); expect(fetch).not.toHaveBeenCalled(); await tick(1);
  expect(queries).toEqual([text]); expect(container.querySelector('[role="option"]')?.textContent).toContain(text);
  await query(overlong); expect(container.querySelectorAll('[role="option"]')).toHaveLength(0); await tick();
  expect(container.querySelector('input')!.value).toBe(overlong); expect(container.textContent).toContain('Invalid issue filter or page size');
  expect(queries).toEqual([text, overlong]); await enter(); expect(close).not.toHaveBeenCalled(); expect(options.open).not.toHaveBeenCalled();
  await query(text); await tick(); expect(container.querySelectorAll('[role="option"]')).toHaveLength(1);
  await enter(); expect(fetch.mock.calls.at(-1)![0]).toBe('/api/v1/workspaces/workspace/issues/full');
  expect(close).toHaveBeenCalledOnce(); expect(options.open).not.toHaveBeenCalled(); expect(frames).toHaveLength(1);
  frames[0](0); expect(options.open).toHaveBeenCalledExactlyOnceWith('full'); expect(queries).toEqual([text, overlong, text]);
});

it('debounces remote search, bounds the request, and rejects out-of-order responses even when transport ignores abort', async () => {
  const older = deferred(); const newer = deferred();
  const fetch = vi.fn().mockReturnValueOnce(older.promise).mockReturnValueOnce(newer.promise); vi.stubGlobal('fetch', fetch);
  await render(); await query('old'); await tick(149); expect(fetch).not.toHaveBeenCalled(); await tick(1);
  await query('new & exact'); await tick();
  expect(fetch).toHaveBeenCalledTimes(2);
  const url = new URL(String(fetch.mock.calls[1][0]), 'https://example.test');
  expect(url.searchParams.get('q')).toBe('new & exact'); expect(url.searchParams.get('limit')).toBe('20');
  expect(fetch.mock.calls[0][1].signal.aborted).toBe(true);
  await act(async () => newer.resolve(json(page(issue('new', 'Latest accessible issue')))));
  expect(container.textContent).toContain('Latest accessible issue');
  await act(async () => older.resolve(json(page(issue('old', 'Obsolete result')))));
  expect(container.textContent).not.toContain('Obsolete result'); expect(container.textContent).toContain('Latest accessible issue');
});

it('removes the previous query rows immediately, including during debounce and failed replacement requests', async () => {
  const pending = deferred(); const fetch = vi.fn().mockResolvedValueOnce(json(page(issue('a', 'Previous title')))).mockReturnValueOnce(pending.promise); vi.stubGlobal('fetch', fetch);
  await render(); await query('first'); await tick(); expect(container.textContent).toContain('Previous title');
  await query('second'); expect(container.textContent).not.toContain('Previous title'); await tick();
  await act(async () => pending.resolve(json({ code: 'unavailable', detail: 'Search unavailable' }, 503)));
  expect(container.textContent).not.toContain('Previous title'); expect(container.textContent).toContain('Search unavailable');
});

it('reauthorizes reference-only recents on every open and excludes forbidden, archived and cross-workspace rows', async () => {
  const fetch = vi.fn((url: string) => Promise.resolve(url.endsWith('/denied') ? json({ code: 'not_found', detail: 'Unavailable' }, 404) : json(issue(url.split('/').pop()!, 'Current title', url.endsWith('/foreign') ? 'different-workspace' : 'workspace'))));
  vi.stubGlobal('fetch', fetch);
  const options = source({ recentIds: ['live', 'denied', 'foreign', 'live'] });
  await render(options); await tick(0);
  expect(fetch).toHaveBeenCalledTimes(3); expect(container.querySelectorAll('[role="option"]')).toHaveLength(1); expect(container.textContent).toContain('Recent issues');
  expect(localStorage.length).toBe(0); expect(sessionStorage.length).toBe(0);
  await act(async () => root.render(null)); await render(options); await tick(0); expect(fetch).toHaveBeenCalledTimes(6);
});

it('never reuses result titles after account, workspace or authority changes', async () => {
  const old = deferred(); const newer = deferred(); const fetch = vi.fn().mockReturnValueOnce(old.promise).mockReturnValueOnce(newer.promise); vi.stubGlobal('fetch', fetch);
  await render(source({ recentIds: ['old'] })); await tick(0);
  await render(source({ workspaceId: 'next', scope: 'next-account:next:epoch', recentIds: ['new'] }));
  expect(container.querySelectorAll('[role="option"]')).toHaveLength(0); await tick(0);
  await act(async () => old.resolve(json(issue('old', 'Forbidden former title'))));
  expect(container.textContent).not.toContain('Forbidden former title');
  await act(async () => newer.resolve(json(issue('new', 'Authorized new title', 'next'))));
  expect(container.textContent).toContain('Authorized new title');
  await render(source({ workspaceId: 'next', scope: 'next-account:next:new-epoch' }));
  expect(container.textContent).not.toContain('Authorized new title');
});

it('keeps the palette open and removes an issue that becomes unavailable before invocation', async () => {
  const options = source(); const close = vi.fn();
  vi.stubGlobal('fetch', vi.fn().mockResolvedValueOnce(json(page(issue('gone', 'Previously accessible')))).mockResolvedValueOnce(json({ code: 'not_found', detail: 'This issue is unavailable.' }, 404)));
  await render(options, close); await query('match'); await tick(); await enter();
  expect(close).not.toHaveBeenCalled(); expect(options.open).not.toHaveBeenCalled();
  expect(container.querySelectorAll('[role="option"]')).toHaveLength(0); expect(container.textContent).toContain('This issue is unavailable.'); expect(document.activeElement).toBe(container.querySelector('input'));
});

it('reauthorizes a selected issue before closing, restores focus, and opens once on the next frame', async () => {
  const frames: FrameRequestCallback[] = []; vi.stubGlobal('requestAnimationFrame', (frame: FrameRequestCallback) => { frames.push(frame); return frames.length; });
  const pending = deferred(); const options = source(); const close = vi.fn(); const fetch = vi.fn().mockResolvedValueOnce(json(page(issue('live')))).mockReturnValueOnce(pending.promise); vi.stubGlobal('fetch', fetch);
  await render(options, close); await query('live'); await tick(); await enter(); await enter();
  expect(fetch).toHaveBeenCalledTimes(2); expect(close).not.toHaveBeenCalled();
  await act(async () => pending.resolve(json(issue('live')))); expect(close).toHaveBeenCalledOnce(); expect(options.open).not.toHaveBeenCalled();
  expect(frames).toHaveLength(1); frames[0](0); expect(options.open).toHaveBeenCalledExactlyOnceWith('live');
});

it('removes a suggestion archived after search without navigating', async () => {
  const options = source(); const close = vi.fn();
  vi.stubGlobal('fetch', vi.fn().mockResolvedValueOnce(json(page(issue('archived'))))
    .mockResolvedValueOnce(json({ ...issue('archived'), archived_at: '2026-10-22T00:00:00Z' })));
  await render(options, close); await query('archived'); await tick(); await enter();
  expect(close).not.toHaveBeenCalled(); expect(options.open).not.toHaveBeenCalled();
  expect(container.querySelectorAll('[role="option"]')).toHaveLength(0);
  expect(container.textContent).toContain('This issue is unavailable.');
  expect(document.activeElement).toBe(container.querySelector('input'));
});

it('does not navigate when a selected issue resolves after the palette closes', async () => {
  const pending = deferred(); const options = source(); const close = vi.fn(); vi.stubGlobal('fetch', vi.fn().mockResolvedValueOnce(json(page(issue('live')))).mockReturnValueOnce(pending.promise));
  await render(options, close); await query('live'); await tick(); await enter(); await act(async () => root.render(null));
  await act(async () => pending.resolve(json(issue('live')))); expect(close).not.toHaveBeenCalled(); expect(options.open).not.toHaveBeenCalled();
});

it('keeps at most ten recent references and clears them synchronously across authority scopes', async () => {
  function History({ scope, id }: { scope: string; id?: string }) { return <output>{useRecentIssues(scope, id).join(',')}</output>; }
  for (let id = 0; id < 12; id += 1) await act(async () => root.render(<History scope="first-account" id={String(id)}/>));
  expect(container.textContent).toBe('11,10,9,8,7,6,5,4,3,2');
  await act(async () => root.render(<History scope="first-account" id="6"/>)); expect(container.textContent).toBe('6,11,10,9,8,7,5,4,3,2');
  await act(async () => root.render(<History scope="new-authority"/>)); expect(container.textContent).toBe('');
});
