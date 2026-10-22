import { createContext, useContext } from 'react';
import type { Session } from '../types';
import type { DesktopSnapshot, DesktopStore } from './store';

export const DesktopContext = createContext<{ store: DesktopStore; snapshot: DesktopSnapshot } | null>(null);
export function useDesktop() { return useContext(DesktopContext); }

export function useSessionScope(session: Session): string {
  const desktop = useDesktop();
  return `${session.user.id}:${desktop ? `native:${desktop.snapshot.connection?.generation ?? '0'}` : session.csrf_token}`;
}

export function useSessionBoundary(session: Session): string {
  const desktop = useDesktop();
  return `${session.user.id}:${desktop ? `native:${desktop.snapshot.connection?.scope_generation ?? '0'}` : session.csrf_token}`;
}

// Read-only projections may refresh after native authentication rotates without
// making a browser render depend on its CSRF token.
export function useNativeRequestRevision(): string {
  return useDesktop()?.snapshot.connection?.generation ?? 'browser';
}
