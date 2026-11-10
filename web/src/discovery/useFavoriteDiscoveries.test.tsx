// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { advanceSessionGeneration } from '../api';
import { useFavoriteDiscoveries } from './useFavoriteDiscoveries';
const workspaceId = '00000000-0000-4000-8000-000000000001';
const id = (n: number) => `00000000-0000-4000-8000-${n.toString(16).padStart(12, '0')}`;
const saved = (n: number, kind: 'view' | 'search' = 'view', extra: Record<string, unknown> = {}) => ({ id: id(n), workspace_id: workspaceId, kind, name: `Favorite ${n}`, favorite: true, repair_required: null, definition: { query: { q: 'private query' } }, owner_id: id(99), team_id: null, can_edit: true, visibility: 'personal', version: '1', etag: '"one"', ...extra });
const page = (data: unknown[], next_cursor: string | null = null) => ({ data, has_more: next_cursor !== null, next_cursor });
const json = (value: unknown, status = 200) => new Response(JSON.stringify(value), { status });
function deferred() { let resolve!: (response: Response) => void; const promise = new Promise<Response>(done => { resolve = done; }); return { promise, resolve }; }
let container: HTMLDivElement; let root: Root;
let favorites: ReturnType<typeof useFavoriteDiscoveries>;
let options: Parameters<typeof useFavoriteDiscoveries>[0];
function Harness({ value }: { value: typeof options }) { favorites = useFavoriteDiscoveries(value); return <>{favorites.views.items.map(item => <p key={`v:${item.id}`}>{item.name}</p>)}{favorites.searches.items.map(item => <p key={`s:${item.id}`}>{item.name}</p>)}</>; }
async function render(patch: Partial<typeof options> = {}) { options = { ...options, ...patch }; await act(async () => root.render(<Harness value={options} />)); }
beforeEach(() => { vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true); container = document.createElement('div'); document.body.append(container); root = createRoot(container); options = { workspaceId, scope: 'account:workspace:epoch:session', enabled: true, refresh: 0 }; });
afterEach(async () => { await act(async () => root.unmount()); container.remove(); vi.unstubAllGlobals(); vi.restoreAllMocks(); });
it('requests bounded favorite pages and retains only safe summaries', async () => {
 const fetch = vi.fn(async (url: string) => json(page(url.includes('/views?') ? [saved(2), saved(3, 'search'), saved(4, 'view', { favorite: false }), saved(5, 'view', { workspace_id: id(100) }), ...Array.from({ length: 20 }, (_, n) => saved(n + 10))] : [saved(6, 'search')]))); vi.stubGlobal('fetch', fetch);
 await render();
 expect(fetch.mock.calls.map(([url]) => String(url))).toEqual([`/api/v1/workspaces/${workspaceId}/views?favorites=true&limit=10`, `/api/v1/workspaces/${workspaceId}/saved-searches?favorites=true&limit=10`]);
 expect(favorites.views.items).toHaveLength(10); expect(favorites.views.items[0]).toEqual({ id: id(2), workspace_id: workspaceId, kind: 'view', name: 'Favorite 2', repair_required: null });
 expect(favorites.views.items.some(item => [id(3), id(4), id(5)].includes(item.id))).toBe(false); expect(favorites.searches.items.map(item => item.id)).toEqual([id(6)]);
});
it('replaces pages, retains independent cursors on refresh, and supports previous', async () => {
 const urls: string[] = []; vi.stubGlobal('fetch', vi.fn(async (url: string) => { urls.push(url); return json(page([saved(url.includes('cursor=next') ? 3 : 2, url.includes('/views?') ? 'view' : 'search')], url.includes('cursor=next') ? null : 'next')); }));
 await render(); await act(async () => favorites.views.next());
 expect(favorites.views.items.map(item => item.name)).toEqual(['Favorite 3']); expect(favorites.searches.items.map(item => item.name)).toEqual(['Favorite 2']); expect(favorites.views.canPrevious).toBe(true);
 await render({ refresh: 1 }); expect(urls.slice(-2)).toEqual([`/api/v1/workspaces/${workspaceId}/views?favorites=true&limit=10&cursor=next`, `/api/v1/workspaces/${workspaceId}/saved-searches?favorites=true&limit=10`]);
 await act(async () => favorites.views.previous()); expect(favorites.views.items.map(item => item.name)).toEqual(['Favorite 2']); expect(favorites.views.canPrevious).toBe(false);
});
it.each([401, 403, 404])('purges both groups and aborts siblings on a current %s denial', async status => {
 let deny = false; const sibling = deferred(); let siblingSignal: AbortSignal | undefined; const onDenied = vi.fn();
 vi.stubGlobal('fetch', vi.fn((url: string, init: RequestInit) => { if (!deny) return Promise.resolve(json(page([saved(2, url.includes('/views?') ? 'view' : 'search')]))); if (url.includes('/views?')) return Promise.resolve(json({ code: 'unavailable', detail: 'Unavailable' }, status)); siblingSignal = init.signal as AbortSignal; return sibling.promise; }));
 await render({ onDenied }); expect(container.textContent).toContain('Favorite 2'); deny = true; await act(async () => favorites.refresh());
 expect(container.textContent).toBe(''); expect(favorites.enabled).toBe(false); expect(siblingSignal?.aborted).toBe(true); expect(onDenied).toHaveBeenCalledOnce();
 await act(async () => sibling.resolve(json(page([saved(9, 'search')])))); expect(container.textContent).toBe(''); await act(async () => favorites.refresh()); expect(onDenied).toHaveBeenCalledOnce();
});
it('recovers a transient failure without hiding the successful sibling', async () => {
 let fail = true; vi.stubGlobal('fetch', vi.fn(async (url: string) => url.includes('/views?') && fail ? json({ detail: 'Temporary failure' }, 500) : json(page([saved(2, url.includes('/views?') ? 'view' : 'search')]))));
 await render(); expect(favorites.views.error).toBeInstanceOf(Error); expect(favorites.views.busy).toBe(false); expect(favorites.searches.items).toHaveLength(1);
 fail = false; await act(async () => favorites.views.retry()); expect(favorites.views.error).toBe(null); expect(favorites.views.items).toHaveLength(1); expect(favorites.views.busy).toBe(false);
});
it('fences old scopes and hides names when disabled', async () => {
 const old = deferred(); let pending = false; let oldSignal: AbortSignal | undefined;
 vi.stubGlobal('fetch', vi.fn((url: string, init: RequestInit) => { if (pending && url.includes('/views?')) { oldSignal = init.signal as AbortSignal; return old.promise; } return Promise.resolve(json(page([saved(2, url.includes('/views?') ? 'view' : 'search')]))); }));
 await render(); pending = true; await act(async () => favorites.refresh()); await render({ enabled: false }); expect(container.textContent).toBe(''); expect(oldSignal?.aborted).toBe(true);
 pending = false; await render({ scope: 'another-account:workspace:epoch:session', enabled: true }); await act(async () => old.resolve(json(page([saved(3)])))); expect(container.textContent).not.toContain('Favorite 3'); expect(favorites.views.busy).toBe(false);
});
it('ignores obsolete completions without stealing newer busy state', async () => {
 const first = deferred(), second = deferred(); let reads = 0;
 vi.stubGlobal('fetch', vi.fn((url: string) => { if (!url.includes('/views?')) return Promise.resolve(json(page([]))); reads++; return reads === 1 ? Promise.resolve(json(page([saved(2)], 'next'))) : reads === 2 ? first.promise : second.promise; }));
 await render(); await act(async () => favorites.views.next()); await act(async () => favorites.refresh()); await act(async () => first.resolve(json(page([saved(3)])))); expect(favorites.views.busy).toBe(true); expect(container.textContent).not.toContain('Favorite 3');
 await act(async () => second.resolve(json(page([saved(4)])))); expect(favorites.views.busy).toBe(false); expect(container.textContent).toBe('Favorite 4');
});
it('retains typed tombstones across late reads and refresh, then clears them for a new grant', async () => {
 const older = deferred(); let pending = false; vi.stubGlobal('fetch', vi.fn((url: string) => url.includes('/views?') && pending ? older.promise : Promise.resolve(json(page([saved(2, url.includes('/views?') ? 'view' : 'search')])))));
 await render(); pending = true; await act(async () => favorites.refresh()); await act(async () => favorites.unavailable('view', id(2))); expect(favorites.views.items).toHaveLength(0); expect(favorites.searches.items).toHaveLength(1);
 await act(async () => older.resolve(json(page([saved(2)])))); expect(favorites.views.items).toHaveLength(0); pending = false; await act(async () => favorites.refresh()); expect(favorites.views.items).toHaveLength(0);
 await render({ scope: 'account:workspace:new-epoch:session' }); expect(favorites.views.items).toHaveLength(1);
});
it('checks API session generation before accepting successful responses', async () => {
 const pending = deferred(); vi.stubGlobal('fetch', vi.fn(() => pending.promise)); await render(); advanceSessionGeneration(); await act(async () => pending.resolve(json(page([saved(2)])))); expect(container.textContent).toBe('');
});
it('does not restart reads when the denial callback identity changes', async () => {
 const fetch = vi.fn(async () => json(page([]))); vi.stubGlobal('fetch', fetch); await render({ onDenied: () => undefined }); await render({ onDenied: () => undefined }); expect(fetch).toHaveBeenCalledTimes(2);
});
it('does not let a denied request from an earlier identical scope invalidate its replacement', async () => {
 const old = deferred(); let initial = true; const denied = vi.fn();
 vi.stubGlobal('fetch', vi.fn((url: string) => initial && url.includes('/views?') ? old.promise : Promise.resolve(json(page([saved(2, url.includes('/views?') ? 'view' : 'search')])))));
 await render({ onDenied: denied }); initial = false; await render({ scope: 'different-principal' }); await render({ scope: 'account:workspace:epoch:session' });
 await act(async () => old.resolve(json({ detail: 'Old denial' }, 403)));
 expect(favorites.enabled).toBe(true); expect(favorites.views.items).toHaveLength(1); expect(denied).not.toHaveBeenCalled();
});
it('retries an expired cursor from the first page without broadening favorite scope', async () => {
 const urls: string[] = [];
 vi.stubGlobal('fetch', vi.fn(async (url: string) => { urls.push(url); return url.includes('cursor=expired') ? json({ detail: 'Cursor expired' }, 422) : json(page([saved(2, url.includes('/views?') ? 'view' : 'search')], 'expired')); }));
 await render(); await act(async () => favorites.views.next()); expect(favorites.views.error).toBeInstanceOf(Error);
 await act(async () => favorites.views.retry()); expect(favorites.views.error).toBe(null); expect(favorites.views.canPrevious).toBe(false); expect(favorites.views.items).toHaveLength(1); expect(urls.at(-1)).toBe(`/api/v1/workspaces/${workspaceId}/views?favorites=true&limit=10`);
});
it('does not fetch or display cached labels for an invalid workspace or empty authority scope', async () => {
 const fetch = vi.fn(async (url: string) => json(page([saved(2, url.includes('/views?') ? 'view' : 'search')]))); vi.stubGlobal('fetch', fetch);
 await render(); expect(container.textContent).toContain('Favorite 2'); await render({ workspaceId: 'invalid' }); expect(container.textContent).toBe(''); expect(favorites.enabled).toBe(false);
 await render({ workspaceId, scope: '' }); expect(favorites.enabled).toBe(false); expect(fetch).toHaveBeenCalledTimes(2);
});
