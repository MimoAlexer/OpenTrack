import { useEffect, useRef } from 'react';
import { ApiError, request } from './api';

/** Lightweight access checks replace cached permissions before another request can render them. */
export function useWorkspaceAccess(workspaceId: string, sessionKey: string, epoch: string | null, invalidate: (revoked: boolean) => Promise<void>) {
  const current = useRef({ epoch, invalidate }); current.current = { epoch, invalidate };
  useEffect(() => {
    const abort = new AbortController(); let inFlight = false; let revoked = false; let invalidatedEpoch: string | null = null;
    async function check() {
      if (document.visibilityState === 'hidden' || inFlight || revoked || abort.signal.aborted || !current.current.epoch) return;
      inFlight = true;
      try {
        const access = await request<{ auth_epoch: string; role: string; archived: boolean }>(`/workspaces/${workspaceId}/access`, { signal: abort.signal });
        if (!abort.signal.aborted && current.current.epoch && access.auth_epoch !== current.current.epoch && access.auth_epoch !== invalidatedEpoch) {
          // Clear authority immediately once for each observed change. Repeating
          // the same invalidation would abort a slow bootstrap every two seconds.
          // Keep polling so further changes and revocation still take effect.
          invalidatedEpoch = access.auth_epoch;
          await current.current.invalidate(false);
        }
      } catch (error) {
        if (!abort.signal.aborted && error instanceof ApiError && error.status === 404) { revoked = true; await current.current.invalidate(true); }
        // Network errors preserve the last view. A 401 follows the common session-expiry path.
      } finally { inFlight = false; }
    }
    const timer = window.setInterval(() => { void check(); }, 2000);
    const focused = () => { void check(); };
    window.addEventListener('focus', focused); document.addEventListener('visibilitychange', focused);
    void check();
    return () => { abort.abort(); window.clearInterval(timer); window.removeEventListener('focus', focused); document.removeEventListener('visibilitychange', focused); };
  }, [workspaceId, sessionKey]);
}
