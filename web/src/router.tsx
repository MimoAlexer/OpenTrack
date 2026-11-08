import { useCallback, useEffect, useRef, useSyncExternalStore, type AnchorHTMLAttributes, type MouseEvent } from 'react';
import { parseStructuredHash, structuredMarker, viewHash, viewParams, withBaseParams } from './filters/model';

const dirtyRegistrations = new Set<object>();
let currentIndex = Number(history.state?.otIndex ?? 0);
let reverting = false;
const currentPath = () => `${location.pathname}${location.search}${location.hash}`;
let currentUrl = currentPath();
history.replaceState({ ...history.state, otIndex: currentIndex }, '', location.href);
const listeners = new Set<() => void>();
function notify() { listeners.forEach(fn => fn()); }
window.addEventListener('popstate', () => {
  const next = Number(history.state?.otIndex ?? currentIndex);
  if (reverting) { reverting = false; currentUrl = currentPath(); return; }
  if (!confirmNavigation()) {
    if (next !== currentIndex) { reverting = true; history.go(currentIndex - next); }
    else history.replaceState({ ...history.state, otIndex: currentIndex }, '', currentUrl);
    return;
  }
  currentIndex = next;
  currentUrl = currentPath();
  notify();
});
window.addEventListener('hashchange', () => {
  if (reverting || currentUrl === currentPath()) return;
  if (!confirmNavigation()) { history.replaceState({ ...history.state, otIndex: currentIndex }, '', currentUrl); return; }
  currentUrl = currentPath();
  notify();
});

export function hasUnsavedChanges() { return dirtyRegistrations.size > 0; }

export function confirmNavigation() { return !hasUnsavedChanges() || window.confirm('Discard your unsaved changes?'); }

export function navigate(path: string, replace = false): boolean {
  if (path === currentPath()) return true;
  if (!confirmNavigation()) return false;
  return navigateConfirmed(path, replace);
}

export function navigateConfirmed(path: string, replace = false): boolean {
  if (replace) history.replaceState({ otIndex: currentIndex }, '', path);
  else { currentIndex += 1; history.pushState({ otIndex: currentIndex }, '', path); }
  currentUrl = currentPath();
  notify();
  return true;
}

export function useLocation() {
  return useSyncExternalStore(fn => { listeners.add(fn); return () => listeners.delete(fn); }, currentPath);
}

export function useUnsavedChanges(dirty: boolean) {
  const release = useRef<() => void>(() => {});
  // A confirmed save can release this draft before its navigation callback runs.
  // Other drafts retain their own registrations until they are saved or closed.
  const releaseOwnedGuard = useCallback(() => release.current(), []);
  useEffect(() => {
    if (!dirty) return;
    const registration = {};
    dirtyRegistrations.add(registration);
    const beforeUnload = (event: BeforeUnloadEvent) => { event.preventDefault(); };
    window.addEventListener('beforeunload', beforeUnload);
    const cleanup = () => { dirtyRegistrations.delete(registration); window.removeEventListener('beforeunload', beforeUnload); };
    release.current = cleanup;
    return cleanup;
  }, [dirty]);
  return releaseOwnedGuard;
}

export function Link({ href = '/', onClick, ...props }: AnchorHTMLAttributes<HTMLAnchorElement>) {
  function click(event: MouseEvent<HTMLAnchorElement>) {
    onClick?.(event);
    if (event.defaultPrevented || event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey || props.target === '_blank') return;
    event.preventDefault();
    navigate(href);
  }
  return <a {...props} href={href} onClick={click} />;
}

export function parseRoute(url: string) {
  const parsed = new URL(url, location.origin);
  const path = parsed.pathname;
  const parts = path.split('/').filter(Boolean);
  const structured = parts[2] === 'issues' ? parseStructuredHash(parsed.hash) : { kind: 'absent' as const };
  const search = structured.kind === 'valid' ? viewParams(structured.view) : new URLSearchParams(parsed.search);
  if (structured.kind === 'invalid') search.set(structuredMarker, 'invalid');
  return {
    workspaceId: parts[0] === 'w' ? parts[1] : undefined,
    issueId: parts[2] === 'issues' && parts[3] !== 'by-key' ? parts[3] : undefined,
    projectId: parts[2] === 'projects' ? parts[3] : undefined,
    cycleId: parts[2] === 'cycles' ? parts[3] : undefined,
    issueKey: parts[2] === 'issues' && parts[3] === 'by-key' ? parts[4] : undefined,
    deletedIssueId: parts[2] === 'trash' ? parts[3] : undefined,
    notificationId: parts[2] === 'inbox' ? parts[3] : undefined,
    area: path === '/auth/native/approve' ? 'native-consent' : path === '/recover' ? 'recover' : path === '/signin' ? 'signin' : path === '/account/security' ? 'account-security' : parts[0] === 'join' ? 'join' : parts[2] === 'projects' ? 'projects' : parts[2] === 'cycles' ? 'cycles' : parts[2] === 'settings' ? 'settings' : parts[2] === 'trash' ? 'trash' : parts[2] === 'inbox' ? 'inbox' : 'issues',
    settingsSection: parts[2] === 'settings' ? (parts[3] ?? 'workspace') : undefined,
    search,
    hash: parsed.hash,
    structured,
    valid: path === '/auth/native/approve' || path === '/' || path === '/recover' || path === '/signin' || path === '/join' || path === '/account/security' || (parts[0] === 'w' && !!parts[1] && ['issues', 'settings', 'trash', 'projects', 'cycles', 'inbox'].includes(parts[2]) && (parts.length <= 4 || (parts[2] === 'issues' && parts[3] === 'by-key' && parts.length === 5))),
  };
}

export function settingsUrl(workspaceId: string, section = 'workspace'): string {
  return `/w/${encodeURIComponent(workspaceId)}/settings/${encodeURIComponent(section)}`;
}

export function issuesUrl(workspaceId: string, search = new URLSearchParams(), issueId?: string): string {
  const path = `/w/${encodeURIComponent(workspaceId)}/issues${issueId ? `/${encodeURIComponent(issueId)}` : ''}`;
  const current = location.pathname.split('/').filter(Boolean);
  if (search.has(structuredMarker) && current[0] === 'w' && current[1] === workspaceId && current[2] === 'issues') {
    const structured = parseStructuredHash(location.hash);
    if (structured.kind === 'valid') return `${path}${viewHash(withBaseParams(structured.view, search))}`;
    if (structured.kind === 'invalid') return `${path}${location.hash}`;
  }
  const clean = new URLSearchParams(search); clean.delete(structuredMarker);
  const query = clean.toString();
  return `${path}${query ? `?${query}` : ''}`;
}

export function trashUrl(workspaceId: string, search = new URLSearchParams(), issueId?: string): string {
  const query = search.toString();
  return `/w/${encodeURIComponent(workspaceId)}/trash${issueId ? `/${encodeURIComponent(issueId)}` : ''}${query ? `?${query}` : ''}`;
}

export function inboxUrl(workspaceId: string, search = new URLSearchParams(), notificationId?: string): string {
  const query = search.toString();
  return `/w/${encodeURIComponent(workspaceId)}/inbox${notificationId ? `/${encodeURIComponent(notificationId)}` : ''}${query ? `?${query}` : ''}`;
}
