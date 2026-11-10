// @vitest-environment jsdom
import { act, useState } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { CommandPalette } from './CommandPalette';
import type { PaletteIssueSource } from './usePaletteIssues';
import type { components } from '../generated/api';
import { navigate } from '../router';

type SearchHit = components['schemas']['SearchHit'];
let container: HTMLDivElement; let root: Root;
beforeEach(() => {
  vi.useFakeTimers(); vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true);
  HTMLDialogElement.prototype.showModal = function () { this.setAttribute('open', ''); };
  HTMLDialogElement.prototype.close = function () { this.removeAttribute('open'); };
  history.replaceState(null, '', '/w/workspace/issues?team_id=current-team');
  container = document.createElement('div'); document.body.append(container); root = createRoot(container);
});
afterEach(async () => { await act(async () => root.unmount()); container.remove(); vi.useRealTimers(); vi.unstubAllGlobals(); vi.restoreAllMocks(); });
const json = (value: unknown, status = 200) => new Response(JSON.stringify(value), { status, headers: { 'Content-Type': 'application/json' } });
const hit = (kind: SearchHit['kind'], id: string, title = id, team_id = 'other-team'): SearchHit => ({ kind, id, title, team_id, key: kind === 'issue' ? `EX-${id}` : null, snippet: `${title} body snippet` });
const page = (...data: SearchHit[]) => ({ data, has_more: false, next_cursor: null, fuzzy_degraded: false, notice: null, query_context: { as_of: '2026-11-10T12:00:00Z', timezone: 'UTC' } });
const source = (overrides: Partial<PaletteIssueSource> = {}): PaletteIssueSource => ({ workspaceId: 'workspace', scope: 'account:workspace:epoch', recentIds: [], open: vi.fn(), ...overrides });
const detail = (item: SearchHit) => ({ id: item.id, workspace_id: 'workspace', team_id: item.team_id, archived_at: null, key: item.key, title: item.title, name: item.title });
function deferred() { let resolve!: (response: Response) => void; const promise = new Promise<Response>(done => { resolve = done; }); return { promise, resolve }; }
function installFetch(work: (url: URL, options?: RequestInit) => Promise<Response>) {
  const fetch = vi.fn((path: string, options?: RequestInit) => {
    const url = new URL(path, 'https://example.test');
    return url.pathname.endsWith('/teams') ? Promise.resolve(json({ data: [], has_more: false, next_cursor: null })) : work(url, options);
  }); vi.stubGlobal('fetch', fetch); return fetch;
}
async function render(value = source(), close = vi.fn()) { await act(async () => root.render(<CommandPalette commands={[]} close={close} issueSource={value} />)); }
async function tick(ms = 150) { await act(async () => { await vi.advanceTimersByTimeAsync(ms); }); }
async function query(value: string) {
  const input = container.querySelector('input')!;
  await act(async () => { Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(input, value); input.dispatchEvent(new Event('input', { bubbles: true })); });
}
const rows = () => Array.from(container.querySelectorAll<HTMLElement>('[role="option"]'));
async function enter() { await act(async () => container.querySelector('input')!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true }))); }

it('uses bounded workspace full-text search and retains mixed server rank, body-only matches and typed identities', async () => {
  const text = '  nebula & %_ 🚀  ';
  const ranked = [hit('project', 'shared', 'First project'), hit('issue', 'body', 'Title without the query'), hit('issue', 'shared', 'Third issue')];
  const fetch = vi.fn((url: string) => Promise.resolve(json(new URL(url, 'https://example.test').pathname.endsWith('/search') ? page(...ranked) : { data: [], has_more: false, next_cursor: null })));
  vi.stubGlobal('fetch', fetch);
  await render(); await query(text); await tick(149); expect(fetch).not.toHaveBeenCalled(); await tick(1);
  const requests = fetch.mock.calls.map(([url]) => new URL(url, 'https://example.test'));
  const searches = requests.filter(url => url.pathname.endsWith('/search'));
  expect(searches).toHaveLength(1);
  expect(searches[0].pathname).toBe('/api/v1/workspaces/workspace/search');
  expect(Object.fromEntries(searches[0].searchParams)).toEqual({ q: text, limit: '20' });
  expect(requests.some(url => url.pathname.endsWith('/issues') || url.pathname.endsWith('/projects'))).toBe(false);
  expect(rows().map(row => row.querySelector('strong')?.textContent)).toEqual(['First project', 'EX-body · Title without the query', 'EX-shared · Third issue']);
  expect(new Set(rows().map(row => row.id)).size).toBe(3);
});

it('renders full-text snippets as text and announces degraded fuzzy search without losing exact results', async () => {
  const item = { ...hit('issue', 'body', 'Only the description matches'), snippet: '<img src=x onerror="alert(1)"> nebula in the description' };
  installFetch(() => Promise.resolve(json({ ...page(item), fuzzy_degraded: true, notice: 'Fuzzy title matching is unavailable; exact and full-text matches remain available.' })));
  await render(); await query('nebula'); await tick();
  expect(rows()).toHaveLength(1); expect(rows()[0].textContent).toContain(item.snippet);
  expect(container.querySelector('img')).toBeNull();
  expect(container.textContent).toContain('Fuzzy title matching is unavailable; exact and full-text matches remain available.');
});

it.each(['query', 'authority', 'workspace', 'route'] as const)('clears mixed rows immediately on %s change and ignores an earlier search even when abort is ignored', async change => {
  const stale = deferred(); const latest = deferred(); let count = 0;
  const fetch = installFetch(() => ++count === 1 ? Promise.resolve(json(page(hit('project', 'previous', 'Previous project')))) : count === 2 ? stale.promise : latest.promise);
  const original = source(); await render(original); await query('first'); await tick();
  expect(container.textContent).toContain('Previous project');
  await query('pending'); expect(rows()).toHaveLength(0); await tick();
  let next = original;
  if (change === 'query') await query('newest');
  if (change === 'authority') { next = source({ scope: 'account:workspace:next-epoch' }); await render(next); }
  if (change === 'workspace') { next = source({ workspaceId: 'next', scope: 'account:next:epoch' }); await act(async () => { navigate('/w/next/issues'); }); await render(next); }
  if (change === 'route') await act(async () => { navigate('/w/workspace/projects?team_id=current-team'); });
  expect(rows()).toHaveLength(0);
  const searches = fetch.mock.calls.filter(([url]) => url.includes('/search?'));
  expect(searches[1][1]?.signal?.aborted).toBe(true);
  await act(async () => stale.resolve(json(page(hit('issue', 'stale', 'Forbidden earlier title')))));
  expect(container.textContent).not.toContain('Forbidden earlier title'); await tick();
  await act(async () => latest.resolve(json(page(hit('issue', 'current', 'Current accessible title')))));
  expect(container.textContent).toContain('Current accessible title');
});

it.each(['/w/workspace/issues', '/w/workspace/issues?team_id=current-team'])('reauthorizes a cross-team project from %s with its own metadata path before dispatching once', async route => {
  history.replaceState(null, '', route);
  const project = hit('project', 'shared', 'Workspace project', 'owning-team'); const pending = deferred(); const close = vi.fn(); const options = source();
  const frames: FrameRequestCallback[] = []; vi.stubGlobal('requestAnimationFrame', (frame: FrameRequestCallback) => { frames.push(frame); return frames.length; });
  const fetch = installFetch(url => url.pathname.endsWith('/search') ? Promise.resolve(json(page(project))) : pending.promise);
  await render(options, close); await query('Workspace'); await tick(); await enter(); await enter();
  expect(fetch.mock.calls.filter(([url]) => url.endsWith('/metadata')).map(([url]) => url)).toEqual(['/api/v1/workspaces/workspace/teams/owning-team/projects/shared/metadata']);
  expect(close).not.toHaveBeenCalled(); expect(frames).toHaveLength(0);
  await act(async () => pending.resolve(json(detail(project))));
  expect(close).toHaveBeenCalledOnce(); expect(frames).toHaveLength(1); expect(options.open).not.toHaveBeenCalled();
  await act(async () => frames[0](0));
  expect(location.pathname + location.search).toBe('/w/workspace/projects/shared?team_id=owning-team');
});

it('dispatches an authorized issue after successful close unmounts the palette and restores the invoking focus', async () => {
  const item = hit('issue', 'live', 'Current issue'); const options = { ...source(), isCurrent: () => true };
  const frames: FrameRequestCallback[] = []; vi.stubGlobal('requestAnimationFrame', (frame: FrameRequestCallback) => { frames.push(frame); return frames.length; });
  installFetch(url => Promise.resolve(json(url.pathname.endsWith('/search') ? page(item) : detail(item))));
  function Harness() {
    const [open, setOpen] = useState(false);
    return <><button onClick={() => setOpen(true)}>Commands</button>{open && <CommandPalette commands={[]} issueSource={options} close={() => setOpen(false)} />}</>;
  }
  await act(async () => root.render(<Harness />));
  const trigger = container.querySelector('button')!; trigger.focus(); await act(async () => trigger.click());
  await query('match'); await tick(); await enter();
  expect(container.querySelector('dialog')).toBeNull(); expect(document.activeElement).toBe(trigger); expect(options.open).not.toHaveBeenCalled(); expect(frames).toHaveLength(1);
  await act(async () => frames[0](0)); expect(options.open).toHaveBeenCalledExactlyOnceWith('live');
});

it('does not dispatch after the source authority expires between successful close and the animation frame', async () => {
  const item = hit('issue', 'live'); let current = true; const options = { ...source(), isCurrent: () => current }; const close = vi.fn();
  const frames: FrameRequestCallback[] = []; vi.stubGlobal('requestAnimationFrame', (frame: FrameRequestCallback) => { frames.push(frame); return frames.length; });
  installFetch(url => Promise.resolve(json(url.pathname.endsWith('/search') ? page(item) : detail(item))));
  await render(options, close); await query('match'); await tick(); await enter(); expect(close).toHaveBeenCalledOnce(); expect(frames).toHaveLength(1);
  await act(async () => root.render(null)); current = false;
  await act(async () => frames[0](0)); expect(options.open).not.toHaveBeenCalled();
});

it.each(['query', 'route'] as const)('does not run an authorized selection when %s changes before the post-close animation frame', async change => {
  const item = hit('issue', 'live'); const options = source(); const close = vi.fn();
  const frames: FrameRequestCallback[] = []; vi.stubGlobal('requestAnimationFrame', (frame: FrameRequestCallback) => { frames.push(frame); return frames.length; });
  installFetch(url => Promise.resolve(json(url.pathname.endsWith('/search') ? page(item) : detail(item))));
  await render(options, close); await query('match'); await tick(); await enter(); expect(close).toHaveBeenCalledOnce(); expect(frames).toHaveLength(1);
  if (change === 'query') await query('replacement');
  else await act(async () => { navigate('/w/workspace/issues/another?team_id=current-team'); });
  await act(async () => frames[0](0)); expect(options.open).not.toHaveBeenCalled();
});

it.each(['query', 'query round trip', 'authority', 'authority callback', 'route area', 'route issue', 'route team', 'workspace', 'unmount'] as const)('cannot dispatch a project detail response after %s changes its selection lifetime', async change => {
  const project = hit('project', 'selected', 'Previous project'); const pending = deferred(); let current = true; const options = { ...source(), isCurrent: () => current }; const close = vi.fn();
  const frames: FrameRequestCallback[] = []; vi.stubGlobal('requestAnimationFrame', (frame: FrameRequestCallback) => { frames.push(frame); return frames.length; });
  const fetch = installFetch(url => url.pathname.endsWith('/search') ? Promise.resolve(json(page(project))) : pending.promise);
  await render(options, close); await query('match'); await tick(); await enter();
  if (change === 'query' || change === 'query round trip') await query('replacement');
  if (change === 'query round trip') { await tick(); await query('match'); await tick(); }
  if (change === 'authority') await render(source({ scope: 'account:workspace:new-epoch' }), close);
  if (change === 'authority callback') current = false;
  if (change === 'route area') await act(async () => { navigate('/w/workspace/projects?team_id=current-team'); });
  if (change === 'route issue') await act(async () => { navigate('/w/workspace/issues/another?team_id=current-team'); });
  if (change === 'route team') await act(async () => { navigate('/w/workspace/issues?team_id=next-team'); });
  if (change === 'workspace') { await act(async () => { navigate('/w/next/issues'); }); await render(source({ workspaceId: 'next', scope: 'account:next:epoch' }), close); }
  if (change === 'unmount') await act(async () => root.render(null));
  if (change !== 'authority callback') expect(fetch.mock.calls.find(([url]) => url.endsWith('/metadata'))?.[1]?.signal?.aborted).toBe(true);
  await act(async () => pending.resolve(json(detail(project))));
  expect(close).not.toHaveBeenCalled(); expect(options.open).not.toHaveBeenCalled(); expect(frames).toHaveLength(0);
});

it.each([
  ['issue', '404'], ['issue', 'id'], ['issue', 'workspace'], ['issue', 'archived'],
  ['project', '404'], ['project', 'id'], ['project', 'workspace'], ['project', 'team'], ['project', 'archived'],
] as const)('conceals a selected %s after fresh detail reports %s, preserving other typed results', async (kind, failure) => {
  const selected = hit(kind, 'shared', 'Selected result'); const other = hit(kind === 'issue' ? 'project' : 'issue', 'shared', 'Other typed result');
  const options = source(); const close = vi.fn(); const frames = vi.fn(); vi.stubGlobal('requestAnimationFrame', frames);
  const fetch = installFetch(url => {
    if (url.pathname.endsWith('/search')) return Promise.resolve(json(page(selected, other)));
    if (failure === '404') return Promise.resolve(json({ code: 'not_found', detail: 'This item is unavailable.' }, 404));
    return Promise.resolve(json({ ...detail(selected), ...(failure === 'id' ? { id: 'different' } : failure === 'workspace' ? { workspace_id: 'different' } : failure === 'team' ? { team_id: 'different' } : { archived_at: '2026-11-10T12:00:00Z' }) }));
  });
  await render(options, close); await query('match'); await tick(); expect(rows()).toHaveLength(2); await enter();
  expect(close).not.toHaveBeenCalled(); expect(frames).not.toHaveBeenCalled(); expect(options.open).not.toHaveBeenCalled();
  expect(rows()).toHaveLength(1); expect(rows()[0].textContent).toContain('Other typed result'); expect(container.textContent).not.toContain('Selected result');
  expect(container.querySelector('[role="alert"]')).not.toBeNull(); expect(document.activeElement).toBe(container.querySelector('input'));
  expect(fetch.mock.calls.at(-1)?.[0]).toBe(kind === 'issue' ? '/api/v1/workspaces/workspace/issues/shared' : '/api/v1/workspaces/workspace/teams/other-team/projects/shared/metadata');
});

it('keeps a search error recoverable and retries the exact query without broadening it', async () => {
  const text = 'exact & query'; let attempts = 0;
  const fetch = installFetch(() => Promise.resolve(++attempts === 1 ? json({ code: 'unavailable', detail: 'Search temporarily unavailable' }, 503) : json(page(hit('issue', 'live', 'Recovered body match')))));
  const close = vi.fn(); await render(source(), close); await query(text); await tick();
  expect(container.querySelector('[role="alert"]')?.textContent).toContain('Search temporarily unavailable'); expect(rows()).toHaveLength(0);
  expect(container.textContent).not.toContain('No commands match'); await enter(); expect(close).not.toHaveBeenCalled();
  await act(async () => Array.from(container.querySelectorAll('button')).find(button => button.textContent === 'Try again')!.click()); await tick();
  expect(container.querySelector('[role="alert"]')).toBeNull(); expect(rows()[0].textContent).toContain('Recovered body match');
  expect(fetch.mock.calls.filter(([url]) => url.includes('/search?')).map(([url]) => Object.fromEntries(new URL(url, 'https://example.test').searchParams))).toEqual([{ q: text, limit: '20' }, { q: text, limit: '20' }]);
});

it.each(['Teams', 'Search'] as const)('settles a cancelled selection after retrying %s while its sibling detail is pending', async retryGroup => {
  const item = hit('project', 'selected', 'Work result'); const team = { id: 'destination', workspace_id: 'workspace', key: 'EX', name: 'Team result', archived_at: null };
  const pending = deferred(); let attempts = 0; const options = source(); const close = vi.fn();
  const frames = vi.fn(); vi.stubGlobal('requestAnimationFrame', frames);
  const fetch = vi.fn((path: string) => {
    const url = new URL(path, 'https://example.test');
    if (url.pathname.endsWith('/search') || url.pathname.endsWith('/teams')) {
      const failedGroup = retryGroup === 'Search' ? url.pathname.endsWith('/search') : url.pathname.endsWith('/teams');
      if (failedGroup && ++attempts === 1) return Promise.resolve(json({ code: 'unavailable', detail: `${retryGroup} temporarily unavailable` }, 503));
      return Promise.resolve(json(url.pathname.endsWith('/search') ? page(item) : { data: [team], has_more: false, next_cursor: null }));
    }
    return pending.promise;
  }); vi.stubGlobal('fetch', fetch);
  await render(options, close); await query('match'); await tick();
  expect(rows()).toHaveLength(1); expect(container.querySelector('[role="listbox"]')?.getAttribute('aria-busy')).toBe('false'); await enter();
  expect(container.querySelector('[role="listbox"]')?.getAttribute('aria-busy')).toBe('true');
  const retry = Array.from(container.querySelectorAll('.command-error')).find(group => group.querySelector('strong')?.textContent === retryGroup)!.querySelector('button')!;
  await act(async () => retry.click()); await tick();
  expect(container.querySelector('[role="alert"]')).toBeNull(); expect(rows()).toHaveLength(2);
  await act(async () => pending.resolve(json(retryGroup === 'Teams' ? detail(item) : team)));
  expect(close).not.toHaveBeenCalled(); expect(options.open).not.toHaveBeenCalled(); expect(frames).not.toHaveBeenCalled();
  expect(location.pathname + location.search).toBe('/w/workspace/issues?team_id=current-team');
  expect(container.querySelector('[role="listbox"]')?.getAttribute('aria-busy')).toBe('false');
  expect(container.querySelector('[role="status"][aria-live="polite"]')?.textContent).toBe('0 issue results, 1 teams and 1 projects in this workspace.');
});

it.each([
  ['Teams', 'success'], ['Search', 'success'], ['Teams', '404'], ['Search', '404'],
] as const)('keeps a newer detail busy after retrying %s when an older selection returns %s', async (retryGroup, earlierResult) => {
  const item = hit('project', 'selected', 'Work result'); const team = { id: 'destination', workspace_id: 'workspace', key: 'EX', name: 'Team result', archived_at: null };
  const older = deferred(); const newer = deferred(); let attempts = 0; let selections = 0; const options = source(); const close = vi.fn();
  const frames: FrameRequestCallback[] = []; vi.stubGlobal('requestAnimationFrame', (frame: FrameRequestCallback) => { frames.push(frame); return frames.length; });
  const fetch = vi.fn((path: string) => {
    const url = new URL(path, 'https://example.test');
    if (url.pathname.endsWith('/search') || url.pathname.endsWith('/teams')) {
      const failedGroup = retryGroup === 'Search' ? url.pathname.endsWith('/search') : url.pathname.endsWith('/teams');
      if (failedGroup && ++attempts === 1) return Promise.resolve(json({ code: 'unavailable', detail: `${retryGroup} temporarily unavailable` }, 503));
      return Promise.resolve(json(url.pathname.endsWith('/search') ? page(item) : { data: [team], has_more: false, next_cursor: null }));
    }
    return ++selections === 1 ? older.promise : newer.promise;
  }); vi.stubGlobal('fetch', fetch);
  await render(options, close); await query('match'); await tick();
  await act(async () => container.querySelector('input')!.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true, cancelable: true }))); await enter();
  const retry = Array.from(container.querySelectorAll('.command-error')).find(group => group.querySelector('strong')?.textContent === retryGroup)!.querySelector('button')!;
  await act(async () => retry.click()); await tick(); await enter(); await enter();
  expect(selections).toBe(2); expect(container.querySelector('[role="listbox"]')?.getAttribute('aria-busy')).toBe('true');
  await act(async () => older.resolve(earlierResult === '404' ? json({ code: 'not_found', detail: 'Earlier selection is unavailable' }, 404) : json(retryGroup === 'Teams' ? detail(item) : team)));
  expect(close).not.toHaveBeenCalled(); expect(frames).toHaveLength(0);
  expect(rows()).toHaveLength(2); expect(container.querySelector('[role="alert"]')).toBeNull();
  expect(container.querySelector('[role="listbox"]')?.getAttribute('aria-busy')).toBe('true');
  expect(container.querySelector('[role="status"][aria-live="polite"]')?.textContent).toBe('Searching accessible results…');
  await act(async () => newer.resolve(json(retryGroup === 'Teams' ? detail(item) : team)));
  expect(close).toHaveBeenCalledOnce(); expect(frames).toHaveLength(1);
  expect(container.querySelector('[role="listbox"]')?.getAttribute('aria-busy')).toBe('false');
  expect(container.querySelector('[role="status"][aria-live="polite"]')?.textContent).toBe('0 issue results, 1 teams and 1 projects in this workspace.');
});

it.each([
  { label: '257 supplementary scalars', text: '🚀'.repeat(257) },
  { label: '33 words', text: Array.from({ length: 33 }, (_, index) => `term${index}`).join(' ') },
])('retains rejected $label unchanged and sends no broader fallback', async ({ text }) => {
  const fetch = installFetch(() => Promise.resolve(json({ code: 'validation_failed', detail: 'Search query exceeds its safe limits' }, 422)));
  const close = vi.fn(); const options = source(); await render(options, close); await query(text); await tick(); await tick(1000);
  expect(container.querySelector('input')!.value).toBe(text); expect(container.querySelector('[role="alert"]')?.textContent).toContain('Search query exceeds its safe limits');
  expect(rows()).toHaveLength(0); await enter(); expect(close).not.toHaveBeenCalled(); expect(options.open).not.toHaveBeenCalled();
  const workRequests = fetch.mock.calls.map(([url]) => new URL(url, 'https://example.test')).filter(url => !url.pathname.endsWith('/teams'));
  expect(workRequests).toHaveLength(1); expect(workRequests[0].pathname).toBe('/api/v1/workspaces/workspace/search');
  expect(Object.fromEntries(workRequests[0].searchParams)).toEqual({ q: text, limit: '20' });
});

it.each(['/signin', '/w/other/issues', '/w/workspace/unknown'])('makes no source requests outside its authorized workspace route %s', async route => {
  history.replaceState(null, '', route); const fetch = vi.fn(); vi.stubGlobal('fetch', fetch);
  const cached = vi.fn(); const close = vi.fn();
  await act(async () => root.render(<CommandPalette commands={[{ id: 'team-private', label: 'Match cached private team', section: 'Navigation', run: cached }]} close={close} issueSource={source({ recentIds: ['private-reference'] })} />)); await query('match'); await tick(); await enter();
  expect(fetch).not.toHaveBeenCalled(); expect(rows()).toHaveLength(0);
  expect(container.textContent).not.toContain('Match cached private team'); expect(cached).not.toHaveBeenCalled(); expect(close).not.toHaveBeenCalled();
});
