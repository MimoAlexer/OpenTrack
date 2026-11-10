import { useEffect, useRef, useState } from 'react';
import { ApiError, currentSessionGeneration, request } from '../api';
import type { components } from '../generated/api';

export type FavoriteKind = 'view' | 'search';
export type FavoriteSummary = Pick<components['schemas']['SavedDiscovery'], 'id' | 'workspace_id' | 'kind' | 'name' | 'repair_required'>;
export interface FavoriteGroup {
  items: FavoriteSummary[];
  busy: boolean;
  error: unknown;
  hasMore: boolean;
  canPrevious: boolean;
  next: () => void;
  previous: () => void;
  retry: () => void;
}
export interface FavoriteDiscoveries {
  views: FavoriteGroup;
  searches: FavoriteGroup;
  enabled: boolean;
  refresh: () => void;
  unavailable: (kind: FavoriteKind, id: string) => void;
}
interface Options { workspaceId: string; scope: string; enabled: boolean; refresh: number; onDenied?: () => void }
interface Page { data: components['schemas']['SavedDiscovery'][]; has_more: boolean; next_cursor?: string | null }
interface GroupState {
  items: FavoriteSummary[]; busy: boolean; error: unknown;
  cursor: string | null; previous: (string | null)[]; next: string | null;
  controller?: AbortController;
}
interface Context {
  key: string; workspaceId: string; session: number; disposed: boolean; denied: boolean; refresh: number;
  groups: Record<FavoriteKind, GroupState>;
  unavailable: Set<string>;
}
const uuid = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
const empty = (): GroupState => ({ items: [], busy: false, error: null, cursor: null, previous: [], next: null });
const kinds: FavoriteKind[] = ['view', 'search'];
const typedId = (kind: FavoriteKind, id: string) => `${kind}:${id.toLowerCase()}`;

export function useFavoriteDiscoveries({ workspaceId, scope, enabled, refresh, onDenied }: Options): FavoriteDiscoveries {
  const session = currentSessionGeneration();
  const active = enabled && uuid.test(workspaceId) && !!scope;
  const key = JSON.stringify([workspaceId, scope, session, active]);
  const latestKey = useRef(key); latestKey.current = key;
  const callback = useRef(onDenied); callback.current = onDenied;
  const context = useRef<Context | null>(null);
  const [, redraw] = useState(0);
  const changed = () => redraw(value => value + 1);
  const current = (value: Context) => context.current === value && !value.disposed && !value.denied && latestKey.current === value.key && currentSessionGeneration() === value.session;
  function retire(value: Context) {
    value.disposed = true;
    for (const kind of kinds) { value.groups[kind].controller?.abort(); value.groups[kind].items = []; }
  }
  function deny(value: Context) {
    if (!current(value)) return;
    value.denied = true;
    for (const kind of kinds) { value.groups[kind].controller?.abort(); value.groups[kind].items = []; value.groups[kind] = empty(); }
    changed();
    callback.current?.();
  }
  function read(value: Context, kind: FavoriteKind, clear = false) {
    if (!current(value)) return;
    const group = value.groups[kind];
    group.controller?.abort();
    const controller = new AbortController(); group.controller = controller;
    group.busy = true; group.error = null; group.next = null;
    if (clear) group.items = [];
    changed();
    const valid = () => current(value) && group.controller === controller && !controller.signal.aborted;
    const params = new URLSearchParams({ favorites: 'true', limit: '10' });
    if (group.cursor) params.set('cursor', group.cursor);
    const resource = kind === 'view' ? 'views' : 'saved-searches';
    void request<Page>(`/workspaces/${encodeURIComponent(value.workspaceId)}/${resource}?${params}`, { signal: controller.signal }).then(page => {
      if (!valid()) return;
      const seen = new Set<string>();
      group.items = page.data.filter(item => {
        if (item.workspace_id !== value.workspaceId || item.kind !== kind || item.favorite !== true || !uuid.test(item.id) || typeof item.name !== 'string' || value.unavailable.has(typedId(kind, item.id)) || seen.has(item.id.toLowerCase())) return false;
        seen.add(item.id.toLowerCase()); return true;
      }).slice(0, 10).map(item => ({ id: item.id, workspace_id: item.workspace_id, kind, name: item.name, repair_required: item.repair_required ?? null }));
      group.next = page.has_more && typeof page.next_cursor === 'string' && page.next_cursor && page.next_cursor !== group.cursor ? page.next_cursor : null;
      group.busy = false; changed();
    }).catch(error => {
      if (!valid()) return;
      if (error instanceof ApiError && [401, 403, 404].includes(error.status)) { deny(value); return; }
      group.error = error; group.busy = false; changed();
    });
  }
  useEffect(() => {
    if (!active) return;
    const value: Context = { key, workspaceId, session, disposed: false, denied: false, refresh, groups: { view: empty(), search: empty() }, unavailable: new Set() };
    context.current = value;
    read(value, 'view'); read(value, 'search');
    const expired = () => deny(value);
    window.addEventListener('opentrack:session-expired', expired);
    return () => { retire(value); window.removeEventListener('opentrack:session-expired', expired); };
  }, [key]);
  useEffect(() => {
    const value = context.current;
    if (!value || !current(value) || value.refresh === refresh) return;
    value.refresh = refresh;
    read(value, 'view'); read(value, 'search');
  }, [key, refresh]);
  function group(kind: FavoriteKind): FavoriteGroup {
    const value = context.current;
    const visible = active && value?.key === key && !value.disposed && !value.denied && value.session === currentSessionGeneration();
    const state = visible ? value.groups[kind] : empty();
    const operate = (action: 'next' | 'previous' | 'retry') => {
      if (!value || !current(value)) return;
      const state = value.groups[kind];
      if (action === 'next') {
        if (state.busy || !state.next) return;
        // Store cursors only; page data is always replaced. The history is
        // capped too, so navigation cannot grow client state without bound.
        state.previous = [...state.previous, state.cursor].slice(-100);
        state.cursor = state.next;
      } else if (action === 'previous') {
        if (state.busy || !state.previous.length) return;
        state.cursor = state.previous.pop()!;
      } else if (state.error instanceof ApiError && state.error.status === 422 && state.cursor) {
        state.cursor = null; state.previous = [];
      }
      read(value, kind, action !== 'retry');
    };
    return { items: state.items, busy: !!active && !value?.denied && (!visible || state.busy), error: state.error, hasMore: !!state.next, canPrevious: state.previous.length > 0, next: () => operate('next'), previous: () => operate('previous'), retry: () => operate('retry') };
  }
  const value = context.current;
  return {
    views: group('view'), searches: group('search'), enabled: active && !(value?.key === key && value.denied),
    refresh: () => { const value = context.current; if (!value || !current(value)) return; read(value, 'view'); read(value, 'search'); },
    unavailable: (kind, id) => {
      const value = context.current;
      if (!value || !current(value)) return;
      value.unavailable.add(typedId(kind, id));
      value.groups[kind].items = value.groups[kind].items.filter(item => item.id.toLowerCase() !== id.toLowerCase()); changed();
    },
  };
}
