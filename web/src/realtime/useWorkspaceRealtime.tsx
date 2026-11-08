import { useEffect, useRef, useState } from 'react';
import { currentSessionGeneration } from '../api';
import { cursorValue, type ResetReason } from './contract';
import { startRealtime, type RealtimeStatus, type ScopedRequest } from './consumer';
import { workspaceWriteGuard } from './guard';

type Options = { workspaceId: string; sessionKey: string; cursor?: string | null; enabled: boolean; native: boolean; request: ScopedRequest; invalidate: () => void; reset: (reason: ResetReason) => void };
export function useWorkspaceRealtime(options: Options) {
  const callbacks = useRef(options); callbacks.current = options;
  const [state, setState] = useState<{ scope: string; status: RealtimeStatus }>({ scope: '', status: 'unavailable' });
  const control = useRef<ReturnType<typeof startRealtime> | null>(null);
  const scope = JSON.stringify([options.sessionKey, options.workspaceId, options.cursor ?? '']);
  const generation = currentSessionGeneration();
  useEffect(() => {
    if (!options.enabled || !cursorValue(options.cursor)) return;
    let active = true; const guard = workspaceWriteGuard(options.workspaceId, generation);
    const consumer = startRealtime({ ...options, cursor: options.cursor,
      current: () => active && callbacks.current.sessionKey === options.sessionKey && callbacks.current.workspaceId === options.workspaceId && callbacks.current.cursor === options.cursor && currentSessionGeneration() === generation,
      status: status => { guard.set(['offline', 'reconnecting', 'degraded', 'resetting'].includes(status)); setState(previous => previous.scope === scope && previous.status === status ? previous : { scope, status }); },
      invalidate: () => callbacks.current.invalidate(), reset: reason => callbacks.current.reset(reason),
      request: (path, init) => callbacks.current.request(path, init),
    });
    control.current = consumer;
    return () => { active = false; consumer.dispose(); guard.release(); if (control.current === consumer) control.current = null; };
  }, [scope, options.enabled, options.native, generation]);
  const status = options.enabled && state.scope === scope ? state.status : 'unavailable';
  return { status, readOnly: ['offline', 'reconnecting', 'degraded', 'resetting'].includes(status), retry: () => control.current?.retry() };
}

export function RealtimeBanner({ status, retry }: { status: RealtimeStatus; retry: () => void }) {
  if (status === 'connected' || status === 'connecting') return null;
  return <div className="notice" role="status">{status === 'unavailable' ? 'Live updates are unavailable. Refresh to check for changes.' : status === 'resetting' ? 'Refreshing live updates. Changes are read-only; your unsaved drafts are preserved.' : status === 'offline' ? 'You are offline. Changes are read-only; your unsaved drafts are preserved.' : 'Live updates are disconnected. Changes are read-only; your unsaved drafts are preserved. Retrying automatically.'}{status !== 'unavailable' && <button type="button" className="button compact" onClick={retry}>Retry connection</button>}</div>;
}
