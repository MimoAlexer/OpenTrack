import { useEffect, useRef, useState } from 'react';
import { ApiError, request } from '../api';
import { useSessionScope } from '../desktop/context';
import type { components } from '../generated/api';
import type { Session } from '../types';

export type UserPreferences = components['schemas']['UserPreferences'];
type ShortcutMode = UserPreferences['shortcut_mode'];
type State = { scope: string; record: UserPreferences | null; desired: ShortcutMode; status: 'loading' | 'ready' | 'saving' | 'conflict' | 'error'; error?: unknown };

export function useShortcutPreference(workspaceId: string, session: Session, accessRevision = 0) {
  const sessionScope = useSessionScope(session);
  const scope = `${sessionScope}:${workspaceId}:${accessRevision}`;
  const [revision, setRevision] = useState(0);
  const [state, setState] = useState<State>({ scope, record: null, desired: 'disabled', status: 'loading' });
  const boundary = useRef({ scope, generation: 0, abort: new AbortController() });
  const busy = useRef(false);
  const currentScope = useRef(scope); currentScope.current = scope;
  const path = `/workspaces/${workspaceId}/preferences`;
  useEffect(() => {
    const abort = new AbortController(); const generation = ++boundary.current.generation;
    boundary.current = { scope, generation, abort }; busy.current = false;
    setState({ scope, record: null, desired: 'disabled', status: 'loading' });
    request<UserPreferences>(path, { signal: abort.signal }).then(record => {
      if (!abort.signal.aborted && currentScope.current === scope && generation === boundary.current.generation) setState({ scope, record, desired: record.shortcut_mode, status: 'ready' });
    }).catch(error => {
      if (!abort.signal.aborted && currentScope.current === scope && generation === boundary.current.generation) setState({ scope, record: null, desired: 'disabled', status: 'error', error });
    });
    return () => abort.abort();
  }, [scope, path, revision]);

  async function change(desired: ShortcutMode) {
    if (busy.current || state.scope !== scope || !state.record || !['ready', 'conflict'].includes(state.status)) return;
    const { abort, generation } = boundary.current;
    busy.current = true; setState({ ...state, desired, status: 'saving', error: undefined });
    try {
      const record = await request<UserPreferences>(path, { method: 'PATCH', csrf: session.csrf_token, headers: { 'If-Match': state.record.etag }, body: { shortcut_mode: desired }, signal: abort.signal });
      if (!abort.signal.aborted && currentScope.current === scope && generation === boundary.current.generation) setState({ scope, record, desired: record.shortcut_mode, status: 'ready' });
    } catch (error) {
      if (!abort.signal.aborted && currentScope.current === scope && generation === boundary.current.generation) {
        const latest = error instanceof ApiError && error.status === 412 ? error.details?.current as UserPreferences | undefined : undefined;
        if (latest?.workspace_id === workspaceId && ['enabled', 'disabled'].includes(latest.shortcut_mode) && typeof latest.etag === 'string') setState({ scope, record: latest, desired, status: 'conflict' });
        else setState({ ...state, desired, status: 'error', error });
      }
    } finally {
      if (generation === boundary.current.generation) busy.current = false;
    }
  }
  const active = state.scope === scope;
  return {
    charactersEnabled: active && state.status === 'ready' && state.record?.shortcut_mode === 'enabled',
    desired: active ? state.desired : 'disabled' as ShortcutMode,
    saved: active ? state.record?.shortcut_mode : undefined,
    status: active ? state.status : 'loading' as State['status'],
    error: active ? state.error : undefined,
    change,
    reload: () => setRevision(value => value + 1),
    useSaved: () => { if (state.scope === scope && state.status === 'conflict' && state.record) setState({ ...state, desired: state.record.shortcut_mode, status: 'ready' }); },
  };
}

export type ShortcutPreference = ReturnType<typeof useShortcutPreference>;
