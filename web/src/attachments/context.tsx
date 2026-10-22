import { createContext, useContext, useEffect, useMemo, useState, type ReactNode } from 'react';
import { request } from '../api';
import { useDesktop, useSessionScope } from '../desktop/context';
import type { Session } from '../types';
import { HARD_MAX_ATTACHMENT_BYTES, MAX_ATTACHMENT_BYTES, type Attachment } from './types';
export type AttachmentContextValue = { workspaceId: string; session: Session; scope: string; binding: string; enabled: boolean; maximum: number; rememberReady: (value: Attachment) => void; readySeed: (id: string) => Attachment | undefined };
const AttachmentContext = createContext<AttachmentContextValue | null>(null);
export function AttachmentProvider({ workspaceId, session, children }: { workspaceId: string; session: Session; children: ReactNode }) {
  const scope = useSessionScope(session); const desktop = useDesktop();
  const binding = `${session.user.id}:${desktop ? `native:${desktop.snapshot.connection?.scope_generation ?? '0'}` : 'browser'}`;
  // Only just-completed receipts are seeded, and only within this exact auth
  // scope. Cards still reauthorize in the background; no record crosses a
  // workspace/account/credential boundary.
  const seeds = useMemo(() => new Map<string, { value: Attachment; time: number }>(), [workspaceId, scope]);
  const metadata = useMemo(() => ({
    rememberReady(value: Attachment) { if (value.state !== 'ready') return; if (seeds.size >= 100) seeds.delete(seeds.keys().next().value!); seeds.set(value.id, { value, time: Date.now() }); },
    readySeed(id: string) { const entry = seeds.get(id); if (!entry || Date.now() - entry.time > 60_000) { seeds.delete(id); return undefined; } return entry.value; },
  }), [seeds]);
  const [capabilities, setCapabilities] = useState({ enabled: false, maximum: MAX_ATTACHMENT_BYTES });
  useEffect(() => {
    const abort = new AbortController(); setCapabilities({ enabled: false, maximum: MAX_ATTACHMENT_BYTES });
    request<{ features: { attachments: boolean }; limits: { max_upload_bytes: number } }>('/capabilities', { signal: abort.signal }).then(value => {
      const maximum = value.limits.max_upload_bytes;
      if (!abort.signal.aborted && Number.isSafeInteger(maximum) && maximum > 0 && maximum <= HARD_MAX_ATTACHMENT_BYTES) setCapabilities({ enabled: value.features.attachments, maximum });
    }).catch(() => { /* Existing documents remain readable when uploads are unavailable. */ });
    return () => abort.abort();
  }, [binding]);
  const value = useMemo(() => ({ workspaceId, session, scope, binding, ...capabilities, ...metadata }), [workspaceId, session, scope, binding, capabilities, metadata]);
  return <AttachmentContext.Provider value={value}>{children}</AttachmentContext.Provider>;
}
export function useAttachments() { return useContext(AttachmentContext); }
