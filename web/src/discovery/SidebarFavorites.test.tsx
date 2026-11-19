// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { SidebarFavorites } from './SidebarFavorites';
import type { FavoriteDiscoveries, FavoriteGroup } from './useFavoriteDiscoveries';
import { useUnsavedChanges } from '../router';
const workspaceId = '00000000-0000-4000-8000-000000000001';
const id = '00000000-0000-4000-8000-000000000002';
let container: HTMLDivElement; let root: Root;
const group = (items: FavoriteGroup['items'] = []): FavoriteGroup => ({ items, busy: false, error: null, hasMore: false, canPrevious: false, next: vi.fn(), previous: vi.fn(), retry: vi.fn() });
const state = (): FavoriteDiscoveries => ({ enabled: true, views: group([{ id, workspace_id: workspaceId, kind: 'view', name: '<script>plain text</script> Long favorite name', repair_required: 'target unavailable' }]), searches: group([{ id, workspace_id: workspaceId, kind: 'search', name: 'Private saved search', repair_required: null }]), refresh: vi.fn(), unavailable: vi.fn() });
beforeEach(() => { vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true); history.replaceState(null, '', '/'); container = document.createElement('div'); document.body.append(container); root = createRoot(container); });
afterEach(async () => { await act(async () => root.unmount()); container.remove(); vi.unstubAllGlobals(); vi.restoreAllMocks(); });
it('renders typed stable links and plain names with accessible kind and repair labels', async () => {
 const favorites = state(); await act(async () => root.render(<SidebarFavorites workspaceId={workspaceId} favorites={favorites}/>));
 expect(container.querySelector('nav[aria-label="Favorites"]')).not.toBe(null); expect(container.querySelector('h2')?.textContent).toBe('Favorites');
 const links = [...container.querySelectorAll('a')]; expect(links.map(link => link.getAttribute('href'))).toEqual([`/w/${workspaceId}/issues?open_saved=view:${id}`, `/w/${workspaceId}/issues?open_saved=search:${id}`]);
 expect(links[0].textContent).toContain('<script>plain text</script>'); expect(container.querySelector('script')).toBe(null); expect(links[0].textContent).toContain('View'); expect(links[1].textContent).toContain('Search'); expect(links[0].textContent).toContain('Needs repair');
 favorites.views.items[0].name = 'Renamed view'; await act(async () => root.render(<SidebarFavorites workspaceId={workspaceId} favorites={favorites}/>)); expect(container.querySelector('a')?.getAttribute('href')).toBe(`/w/${workspaceId}/issues?open_saved=view:${id}`);
});
it('presents independent empty, busy and retry states and exposes bounded page controls', async () => {
 const favorites = state(); favorites.views = { ...group(), error: new Error('Offline'), hasMore: true, canPrevious: true }; favorites.searches = { ...group(), busy: true };
 await act(async () => root.render(<SidebarFavorites workspaceId={workspaceId} favorites={favorites}/>));
 expect(container.querySelector('[role="alert"]')?.textContent).toContain('Offline'); expect(container.textContent).toContain('Loading favorite searches');
 for (const [label, action] of [['Retry favorite views', favorites.views.retry], ['Next favorite views', favorites.views.next], ['Previous favorite views', favorites.views.previous], ['Refresh favorites', favorites.refresh]] as const) {
  const button = [...container.querySelectorAll('button')].find(item => item.getAttribute('aria-label') === label || item.textContent === label)!; await act(async () => button.click()); expect(action).toHaveBeenCalledOnce();
 }
 favorites.views = group(); favorites.searches = group(); await act(async () => root.render(<SidebarFavorites workspaceId={workspaceId} favorites={favorites}/>)); expect(container.textContent).toContain('No favorite views'); expect(container.textContent).toContain('No favorite searches');
});
it('uses ordinary guarded navigation and never prefetches on click', async () => {
 const favorites = state(), fetch = vi.fn(); vi.stubGlobal('fetch', fetch); const confirm = vi.spyOn(window, 'confirm').mockReturnValue(false);
 function Guarded() { useUnsavedChanges(true); return <SidebarFavorites workspaceId={workspaceId} favorites={favorites}/>; }
 await act(async () => root.render(<Guarded/>)); await act(async () => container.querySelector('a')!.click()); expect(location.pathname).toBe('/'); expect(confirm).toHaveBeenCalledOnce(); expect(fetch).not.toHaveBeenCalled();
 confirm.mockReturnValue(true); await act(async () => container.querySelector('a')!.click()); expect(location.search).toBe(`?open_saved=view:${id}`); expect(fetch).not.toHaveBeenCalled();
});
it('uses distinct accessible heading IDs when desktop and drawer render together', async () => {
 const favorites = state(); await act(async () => root.render(<><SidebarFavorites workspaceId={workspaceId} favorites={favorites}/><SidebarFavorites workspaceId={workspaceId} favorites={favorites}/></>));
 const ids = [...container.querySelectorAll('[id]')].map(item => item.id); expect(ids.length).toBeGreaterThan(0); expect(new Set(ids).size).toBe(ids.length);
});
