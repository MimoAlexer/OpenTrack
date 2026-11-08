import { useCallback, useEffect, useMemo, useRef } from 'react';
import { currentSessionGeneration, request } from '../api';
import { useSessionScope } from '../desktop/context';
import type { Session } from '../types';

export function useNotificationScope(workspace: string, session: Session, accessScope: string) {
  const sessionScope = useSessionScope(session); const generation = currentSessionGeneration();
  const key = `${sessionScope}:${generation}:${workspace}:${accessScope}`;
  const scope = useMemo(() => ({ key, generation, abort: new AbortController() }), [key]);
  const latest = useRef(scope); latest.current = scope;
  useEffect(() => {
    if (scope.abort.signal.aborted) scope.abort = new AbortController();
    return () => scope.abort.abort();
  }, [scope]);
  const current = useCallback(() => latest.current === scope && !scope.abort.signal.aborted && currentSessionGeneration() === scope.generation, [scope]);
  const read = useCallback(async <T,>(path: string, options: Parameters<typeof request>[1] = {}): Promise<T> => {
    const signal = options.signal ? AbortSignal.any([scope.abort.signal, options.signal]) : scope.abort.signal;
    const result = await request<T>(path, { ...options, signal });
    if (!current() || signal.aborted) throw new DOMException('This inbox is closed.', 'AbortError');
    return result;
  }, [scope, current]);
  return { key, current, read };
}

export function isAborted(error: unknown) { return error instanceof DOMException && error.name === 'AbortError'; }
