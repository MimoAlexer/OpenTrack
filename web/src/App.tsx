import { useDesktop, useSessionScope } from './desktop/context';
import { useServerReview } from './desktop/DesktopFrame';
import { useNativeConfirmation } from './desktop/ConfirmAction';
import { NativeSignIn } from './desktop/NativeSignIn';
import { BrowserAccountAction } from './desktop/BrowserAccountAction';
import { NativeConsent } from './auth/NativeConsent';
import { nativeRequestId } from './auth/native-callback';
import { useCallback, useEffect, useRef, useState, type ReactNode } from 'react';
import { CalendarRange, ChevronsUpDown, CircleUserRound, FolderKanban, Hash, HelpCircle, ListChecks, LockKeyhole, LogOut, Menu, Monitor, Moon, Plus, RefreshCw, Search, Settings, Sun, Trash2, X } from 'lucide-react';
import { advanceSessionGeneration, ApiError, request } from './api';
import { AccountSecurity } from './account/AccountSecurity';
import { AuthScreen } from './auth/AuthScreen';
import { RecoveryScreen } from './auth/RecoveryScreen';
import { Avatar, Brand, ErrorBanner, Loading } from './components';
import { CreateIssue } from './issues/CreateIssue';
import { IssueEditor } from './issues/IssueEditor';
import { IssueKeyResolver } from './relations/IssueKeyResolver';
import { Cycles } from './cycles/Cycles';
import { Projects } from './projects/Projects';
import { IssueList } from './issues/IssueList';
import { restoreIssueRowFocus } from './issues/focus';
import { Trash } from './issues/Trash';
import { CommandPalette, CommandHelp } from './commands/CommandPalette';
import { useCommandShortcuts, type Command } from './commands/registry';
import { useShortcutPreference } from './commands/useShortcutPreference';
import { useRecentIssues } from './commands/usePaletteIssues';
import { issuesUrl, settingsUrl, trashUrl, Link, navigate, parseRoute, useLocation } from './router';
import { SettingsScreen } from './settings/SettingsScreen';
import { CreateWorkspace } from './settings/WorkspaceSettings';
import { JoinWorkspace } from './settings/JoinWorkspace';
import { Reauthenticate, useScopedRequest } from './settings/shared';
import { useWorkspaceAccess } from './useWorkspaceAccess';
import type { Bootstrap, Issue, Session, Workspace } from './types';
import type { Workflow } from './workflows/types';

type Entry = { kind: 'loading' } | { kind: 'error'; error: unknown } | { kind: 'auth'; setup: boolean; notice?: string } | { kind: 'session'; session: Session };
type Theme = 'system' | 'light' | 'dark';

export function App() {
  const desktop = useDesktop(); const changeServer = useServerReview();
  const location = useLocation(); const route = parseRoute(location);
  const recovering = route.area === 'recover';
  const signingIn = route.area === 'signin';
  const consenting = !desktop && route.area === 'native-consent';
  const publicAuthentication = !desktop && (recovering || signingIn);
  const verifiedSignIn = useRef(false);
  const [entry, setEntry] = useState<Entry>({ kind: 'loading' });
  const [attempt, setAttempt] = useState(0);
  const entryRef = useRef(entry); entryRef.current = entry;
  function authenticated(session: Session) { advanceSessionGeneration(); setEntry({ kind: 'session', session }); }
  function signedInWithCredentials(session: Session) {
    // Use this credential response directly, rather than restoring an older cookie.
    authenticated(session); verifiedSignIn.current = true;
    navigate(session.workspaces[0] ? issuesUrl(session.workspaces[0].id) : '/');
  }
  function signedOut() { advanceSessionGeneration(); setEntry({ kind: 'auth', setup: false }); }
  const [theme, setTheme] = useState<Theme>(() => { try { const value = localStorage.getItem('opentrack:theme'); return value === 'light' || value === 'dark' ? value : 'system'; } catch { return 'system'; } });
  useEffect(() => { document.documentElement.dataset.theme = theme; try { localStorage.setItem('opentrack:theme', theme); } catch { /* Appearance is optional when storage is unavailable. */ } }, [theme]);
  useEffect(() => {
    // Public credential forms never substitute an existing session for a password check.
    if (publicAuthentication) return;
    if (verifiedSignIn.current) { verifiedSignIn.current = false; return; }
    const abort = new AbortController(); setEntry({ kind: 'loading' });
    async function start() {
      try {
        if (desktop) {
          if (desktop.snapshot.session) { authenticated(desktop.snapshot.session); return; }
          if (!desktop.snapshot.connection?.credential_present) { setEntry({ kind: 'auth', setup: false }); return; }
          const session = await request<Session>('/auth/session', { signal: abort.signal });
          if (!abort.signal.aborted) authenticated(session);
          return;
        }
        const status = await request<{ setup_required: boolean }>('/setup/status', { signal: abort.signal });
        if (abort.signal.aborted) return;
        if (status.setup_required) { setEntry({ kind: 'auth', setup: true }); return; }
        const session = await request<Session>('/auth/session', { signal: abort.signal });
        if (!abort.signal.aborted) authenticated(session);
      } catch (error) {
        if (!abort.signal.aborted && !(error instanceof DOMException && error.name === 'AbortError')) setEntry(error instanceof ApiError && error.status === 401 ? { kind: 'auth', setup: false } : { kind: 'error', error });
      }
    }
    void start(); return () => abort.abort();
  }, [attempt, publicAuthentication, !!desktop]);
  useEffect(() => {
    const event = desktop?.snapshot.authEvent;
    if (!event) return;
    if (event.kind === 'authenticated' && event.session) {
      if (entryRef.current.kind !== 'session' || entryRef.current.session !== event.session) authenticated(event.session);
      if (signingIn) navigate(event.session.workspaces[0] ? issuesUrl(event.session.workspaces[0].id) : '/');
    } else if (event.kind === 'signed_out') {
      advanceSessionGeneration();
      setEntry({ kind: 'auth', setup: false, notice: event.error?.message ?? (event.remote_revocation_confirmed === false ? 'Signed out on this computer. Remote session revocation could not be confirmed.' : 'This desktop session has ended. Sign in to continue.') });
    }
  }, [desktop?.snapshot.authEvent]);
  useEffect(() => {
    const expired = (event: Event) => {
      if (entryRef.current.kind !== 'session') return;
      advanceSessionGeneration();
      const accessChanged = event instanceof CustomEvent && event.detail?.reason === 'access_changed';
      setEntry({ kind: 'auth', setup: false, notice: accessChanged ? 'Access updated. Sign in again.' : 'Your session has ended. Sign in to continue.' });
    };
    window.addEventListener('opentrack:session-expired', expired);
    return () => window.removeEventListener('opentrack:session-expired', expired);
  }, []);
  if (desktop && (recovering || route.area === 'join')) return <BrowserAccountAction action={recovering ? 'recover' : 'join'} session={entry.kind === 'session' ? entry.session : null} authenticated={authenticated} />;
  if (desktop && (signingIn || entry.kind === 'auth')) return <NativeSignIn connection={desktop.snapshot.connection!} begin={name => desktop.store.begin(name)} cancel={() => desktop.store.cancel()} openBrowser={action => desktop.store.bridge.openBrowser(action).then(() => undefined)} changeServer={() => changeServer?.()} error={desktop.snapshot.authEvent?.kind === 'failed' ? desktop.snapshot.error : null} notice={entry.kind === 'auth' && entry.notice ? entry.notice : desktop.snapshot.authEvent?.kind === 'signed_out' ? desktop.snapshot.authEvent.error?.message ?? (desktop.snapshot.authEvent.remote_revocation_confirmed === false ? 'Signed out on this computer. Remote session revocation could not be confirmed.' : undefined) : undefined} />;
  if (recovering) return <RecoveryScreen onCompleted={signedOut} />;
  if (signingIn) return <AuthScreen key="explicit-sign-in" setup={false} onAuthenticated={signedInWithCredentials} />;
  if (entry.kind === 'loading') return <main className="startup"><Brand /><Loading label="Connecting to your workspace…" /></main>;
  if (entry.kind === 'error') return <main className="startup"><Brand /><ErrorBanner error={entry.error} retry={() => setAttempt(n => n + 1)} /></main>;
  if (consenting) return <NativeConsent requestId={nativeRequestId(location.split('?')[1] ?? '')} session={entry.kind === 'session' ? entry.session : null} authenticated={authenticated} />;
  if (route.area === 'join' && !(entry.kind === 'auth' && entry.setup)) return <JoinWorkspace session={entry.kind === 'session' ? entry.session : null} authenticated={authenticated} />;
  if (entry.kind === 'auth') return <AuthScreen setup={entry.setup} notice={entry.notice} onAuthenticated={authenticated} />;
  if (route.area === 'account-security') return <AccountSecurity key={entry.session.user.id} session={entry.session} authenticated={authenticated} signedOut={signedOut} />;
  if (!entry.session.workspaces.length) return <NoWorkspace session={entry.session} authenticated={authenticated} signedOut={signedOut} />;
  return <WorkspaceApp session={entry.session} theme={theme} setTheme={setTheme} signedOut={signedOut} authenticated={authenticated} />;
}

function NoWorkspace({ session, authenticated, signedOut }: { session: Session; authenticated: (session: Session) => void; signedOut: () => void }) {
  const desktop = useDesktop();
  const request = useScopedRequest();
  const [creating, setCreating] = useState(false); const [reauth, setReauth] = useState(false); const [error, setError] = useState<unknown>(null);
  return <main className="startup"><Brand /><h1>No workspace access.</h1><p>Join with an invitation, or create a workspace in an organization you manage.</p><ErrorBanner error={error} /><div className="button-row"><Link className="button primary" href="/join">Join workspace</Link><button className="button" onClick={() => setCreating(true)}>Create workspace</button><Link className="button" href="/account/security">Account security</Link><button className="quiet" onClick={async () => { try { if (desktop) await desktop.store.logout(); else await request('/auth/logout', { method: 'POST', csrf: session.csrf_token }); signedOut(); } catch (error) { setError(error); } }}>Sign out</button></div>{creating && <CreateWorkspace session={session} close={() => setCreating(false)} reauthenticate={() => setReauth(true)} created={async item => { authenticated(await request<Session>('/auth/session')); navigate(issuesUrl(item.id)); }} />}{reauth && <Reauthenticate session={session} close={() => setReauth(false)} authenticated={authenticated} />}</main>;
}

function WorkspaceApp({ session, theme, setTheme, signedOut, authenticated }: { session: Session; theme: Theme; setTheme: (theme: Theme) => void; signedOut: () => void; authenticated: (session: Session) => void }) {
  const location = useLocation(); const route = parseRoute(location);
  const workspace = session.workspaces.find(workspace => workspace.id === route.workspaceId) ?? session.workspaces[0];
  // A workspace switch gets a new subtree: no issue, form, or member state is reused across tenants.
  return <WorkspaceScreen key={`${session.user.id}:${workspace.id}`} workspace={workspace} session={session} theme={theme} setTheme={setTheme} signedOut={signedOut} authenticated={authenticated} />;
}

function WorkspaceScreen({ workspace, session, theme, setTheme, signedOut, authenticated }: { workspace: Workspace; session: Session; theme: Theme; setTheme: (theme: Theme) => void; signedOut: () => void; authenticated: (session: Session) => void }) {
  const desktop = useDesktop(); const nativeConfirm = useNativeConfirmation(true);
  const sessionScope = useSessionScope(session);
  const request = useScopedRequest();
  const location = useLocation(); const route = parseRoute(location);
  const [data, setData] = useState<Bootstrap | null>(null);
  const [error, setError] = useState<unknown>(null);
  const [retry, setRetry] = useState(0);
  const [refresh, setRefresh] = useState(0);
  const [creating, setCreating] = useState(false);
  const [creatingWorkspace, setCreatingWorkspace] = useState(false);
  const [reauth, setReauth] = useState(false);
  const [mobileOpen, setMobileOpen] = useState(false);
  const [shortcuts, setShortcuts] = useState(false);
  const [palette, setPalette] = useState(false);
  const [notice, setNotice] = useState('');
  const [logoutBusy, setLogoutBusy] = useState(false);
  const searchRef = useRef<HTMLInputElement>(null);
  const previousIssue = useRef<string | undefined>(undefined);
  const bootstrapGeneration = useRef(0);
  const [accessRevision, setAccessRevision] = useState(0);
  const shortcutPreference = useShortcutPreference(workspace.id, session, accessRevision);
  const [accessBlocked, setAccessBlocked] = useState(true);
  const [lastKnownEpoch, setLastKnownEpoch] = useState<string | null>(null);
  const [workspaceArchived, setWorkspaceArchived] = useState(false);
  const paletteScope = accessBlocked ? '' : `${sessionScope}:${workspace.id}:${accessRevision}:${lastKnownEpoch}`;
  const recentIssues = useRecentIssues(paletteScope, accessBlocked ? undefined : route.issueId);
  const readOnly = workspace.role === 'viewer' || workspaceArchived;
  const activeTeams = data?.teams.filter(team => !team.archived_at) ?? [];
  const team = data?.teams.find(team => team.id === route.search.get('team_id'));
  const mine = route.search.get('view') === 'mine';
  const cycles = route.area === 'cycles'; const projects = route.area === 'projects'; const settings = route.area === 'settings'; const trash = route.area === 'trash'; const issueArea = route.area === 'issues';
  const title = cycles ? 'Cycles' : projects ? 'Projects' : settings ? 'Settings' : trash ? 'Trash' : team ? team.name : mine ? 'My work' : 'All issues';
  useWorkspaceAccess(workspace.id, sessionScope, lastKnownEpoch, async revoked => {
    // Destroy every scoped editor before refreshing authority, including forms in settings.
    bootstrapGeneration.current += 1; setData(null); setAccessBlocked(true); setAccessRevision(value => value + 1);
    setCreating(false); setCreatingWorkspace(false); setReauth(false); setPalette(false); setShortcuts(false); setNotice('');
    try { authenticated(await request<Session>('/auth/session')); if (!revoked) setRetry(value => value + 1); }
    catch (error) { setError(error); }
  });
  useEffect(() => {
    if (!route.workspaceId || !route.valid) navigate(issuesUrl(workspace.id), true);
  }, [route.workspaceId, route.valid, workspace.id]);
  useEffect(() => {
    const abort = new AbortController(); const generation = ++bootstrapGeneration.current; setData(null); setAccessBlocked(true); setError(null); setNotice(''); setCreating(false); setPalette(false); setShortcuts(false);
    Promise.all([request<Bootstrap>(`/workspaces/${workspace.id}/bootstrap`, { signal: abort.signal }), request<{ archived: boolean; auth_epoch: string }>(`/workspaces/${workspace.id}/access`, { signal: abort.signal })]).then(([result, access]) => { if (!abort.signal.aborted && generation === bootstrapGeneration.current) { if (result.auth_epoch !== access.auth_epoch) { setRetry(value => value + 1); return; } setData(result); setLastKnownEpoch(result.auth_epoch); setWorkspaceArchived(access.archived); setAccessBlocked(false); } }).catch(error => { if (!abort.signal.aborted && generation === bootstrapGeneration.current) setError(error); });
    return () => abort.abort();
  }, [workspace.id, retry]);
  useEffect(() => { document.title = `${title} · ${workspace.name} · OpenTrack`; }, [title, workspace.name]);
  useEffect(() => {
    setMobileOpen(false);
    if (!route.issueId && previousIssue.current) {
      restoreIssueRowFocus(previousIssue.current);
    }
    previousIssue.current = route.issueId;
  }, [location]);
  function update(issue: Issue, options?: { refreshList?: boolean }) {
    setData(previous => previous ? { ...previous, issues: previous.issues.some(old => old.id === issue.id) ? previous.issues.map(old => old.id === issue.id ? issue : old) : [issue, ...previous.issues] } : previous);
    if (options?.refreshList !== false) setRefresh(value => value + 1);
  }
  function deleted(issue: Issue) {
    setData(previous => previous ? { ...previous, issues: previous.issues.filter(item => item.id !== issue.id) } : previous);
    setRefresh(value => value + 1); setNotice(`${issue.key} moved to Trash. It can be restored for 30 days.`); navigate(trashUrl(workspace.id));
  }
  function created(issue: Issue) { update(issue); setCreating(false); setNotice(`${issue.key} created.`); navigate(issuesUrl(workspace.id, route.search, issue.id)); }
  async function logout() {
    if (logoutBusy) return;
    if (!(desktop ? await nativeConfirm!({ title: 'Sign out of this server?', description: 'Unsaved changes will be discarded.', action: 'Sign out' }) : window.confirm('Sign out of this installation? Unsaved edits will be discarded.'))) return;
    setLogoutBusy(true); setError(null);
    try { if (desktop) await desktop.store.logout(); else await request('/auth/logout', { method: 'POST', csrf: session.csrf_token }); signedOut(); }
    catch (error) { setError(error); }
    finally { setLogoutBusy(false); }
  }
  const workflowChanged = useCallback((workflow: Workflow) => {
    setData(previous => previous ? { ...previous, statuses: [...previous.statuses.filter(status => status.team_id !== workflow.team_id), ...workflow.statuses] } : previous);
    setRefresh(value => value + 1);
  }, []);
  async function settingsChanged() {
    const updatedSession = await request<Session>('/auth/session');
    authenticated(updatedSession); setRetry(value => value + 1);
  }
  function teamHref(id?: string) { return issuesUrl(workspace.id, id ? new URLSearchParams({ team_id: id }) : new URLSearchParams()); }
  const commands: Command[] = [
    { id: 'new-issue', label: 'Create issue', section: 'Actions', keywords: 'new task', shortcut: { key: 'c' }, available: issueArea && !readOnly && activeTeams.length > 0 && !accessBlocked, run: () => setCreating(true) },
    { id: 'search-issues', label: 'Search issues', section: 'Actions', shortcut: { key: '/' }, available: issueArea && !accessBlocked, run: () => { if (route.issueId || route.issueKey) { if (!navigate(issuesUrl(workspace.id, route.search))) return; } requestAnimationFrame(() => searchRef.current?.focus()); } },
    { id: 'close-issue', label: 'Close issue', section: 'Actions', shortcut: { key: 'Escape' }, available: !!route.issueId || !!route.issueKey, run: () => { navigate(issuesUrl(workspace.id, route.search)); } },
    { id: 'refresh', label: cycles ? 'Refresh cycles' : projects ? 'Refresh projects' : trash ? 'Refresh Trash' : 'Refresh issues', section: 'Actions', available: !settings && !accessBlocked, run: () => setRefresh(value => value + 1) },
    { id: 'cycles', label: 'Cycles', section: 'Navigation', keywords: 'planning iterations scope', run: () => { navigate(`/w/${workspace.id}/cycles`); } },
    { id: 'projects', label: 'Projects', section: 'Navigation', keywords: 'planning delivery outcomes', run: () => { navigate(`/w/${workspace.id}/projects`); } },
    { id: 'all-issues', label: 'All issues', section: 'Navigation', run: () => { navigate(teamHref()); } },
    { id: 'my-work', label: 'My work', section: 'Navigation', keywords: 'assigned to me', shortcut: { key: 'g', then: 'm' }, available: !accessBlocked, run: () => { navigate(issuesUrl(workspace.id, new URLSearchParams({ view: 'mine' }))); } },
    ...activeTeams.map(item => ({ id: `team-${item.id}`, label: `${item.name} issues`, section: 'Navigation' as const, keywords: item.key, run: () => { navigate(teamHref(item.id)); } })),
    { id: 'trash', label: 'Trash', section: 'Navigation', keywords: 'deleted restore', run: () => { navigate(trashUrl(workspace.id)); } },
    { id: 'settings', label: 'Workspace settings', section: 'Navigation', run: () => { navigate(settingsUrl(workspace.id)); } },
    { id: 'account-security', label: 'Account security', section: 'Navigation', keywords: 'sessions sign in', run: () => { navigate('/account/security'); } },
    { id: 'shortcut-help', label: 'Keyboard shortcuts', section: 'Help', shortcut: { key: '?' }, run: () => setShortcuts(true) },
    { id: 'commands', label: 'Search commands', section: 'Help', available: !accessBlocked, shortcut: { key: 'k', primary: true }, run: () => { setMobileOpen(false); setPalette(true); } },
  ];
  useCommandShortcuts(shortcuts ? [{ id: 'close-help', label: 'Close keyboard shortcuts', section: 'Help', shortcut: { key: 'Escape' }, run: () => setShortcuts(false) }] : commands, creating || creatingWorkspace || reauth || mobileOpen || palette || accessBlocked, shortcutPreference.charactersEnabled, `${sessionScope}:${workspace.id}:${accessRevision}:${location}`);
  const sidebar = <>
    <div className="workspace-heading">
      <details className="workspace-menu" onKeyDown={event => { if (event.key === 'Escape' && event.currentTarget.open) { event.preventDefault(); event.stopPropagation(); event.currentTarget.open = false; event.currentTarget.querySelector('summary')?.focus(); } }} onBlur={event => { if (!event.currentTarget.contains(event.relatedTarget as Node | null)) event.currentTarget.open = false; }}>
        <summary aria-label={`Workspace menu: ${workspace.name}`}><span className="workspace-monogram" aria-hidden="true">{workspace.name[0]?.toUpperCase()}</span><span className="workspace-identity"><strong>{workspace.name}</strong><small>OpenTrack</small></span><ChevronsUpDown aria-hidden="true" /></summary>
        <div className="workspace-popover">
          <label className="field">Switch workspace<select aria-label="Workspace" value={workspace.id} onChange={event => { event.currentTarget.closest('details')?.removeAttribute('open'); navigate(issuesUrl(event.target.value)); }}>{session.workspaces.map(item => <option key={item.id} value={item.id}>{item.name}</option>)}</select></label>
          <button className="nav-item" onClick={event => { const menu = event.currentTarget.closest('details'); menu?.querySelector('summary')?.focus(); menu?.removeAttribute('open'); setMobileOpen(false); setCreatingWorkspace(true); }}><Plus aria-hidden="true" />Create workspace</button>
          <Link className="nav-item" href="/join">Join workspace</Link>
        </div>
      </details>
      <button className="icon-button mobile-only" aria-label="Close navigation" onClick={() => setMobileOpen(false)}><X aria-hidden="true" /></button>
    </div>
    <nav aria-label="Workspace navigation"><button type="button" className="nav-item command-trigger" aria-label="Search commands" disabled={accessBlocked} onClick={() => { setMobileOpen(false); setPalette(true); }}><Search aria-hidden="true" /><span>Commands</span><kbd>Ctrl / ⌘ K</kbd></button><div className="nav-section">Your work</div><Link href={issuesUrl(workspace.id, new URLSearchParams({ view: 'mine' }))} className="nav-item" aria-current={issueArea && mine && !team ? 'page' : undefined}><CircleUserRound aria-hidden="true" />My work</Link><Link href={teamHref()} className="nav-item" aria-current={issueArea && !mine && !team ? 'page' : undefined}><ListChecks aria-hidden="true" />All issues</Link><div className="nav-section team-section">Teams<span>{data ? activeTeams.length : ''}</span></div>{activeTeams.map(item => <Link className="nav-item" href={teamHref(item.id)} key={item.id} aria-current={issueArea && team?.id === item.id ? 'page' : undefined}>{item.private ? <LockKeyhole aria-hidden="true" /> : <Hash aria-hidden="true" />}<span>{item.name}</span><small>{item.key}</small></Link>)}<div className="nav-section">Workspace</div><Link href={`/w/${workspace.id}/projects`} className="nav-item" aria-current={projects ? 'page' : undefined}><FolderKanban aria-hidden="true"/>Projects</Link><Link href={`/w/${workspace.id}/cycles`} className="nav-item" aria-current={cycles ? 'page' : undefined}><CalendarRange aria-hidden="true"/>Cycles</Link><Link href={trashUrl(workspace.id)} className="nav-item" aria-current={trash ? 'page' : undefined}><Trash2 aria-hidden="true" />Trash</Link><Link href={settingsUrl(workspace.id)} className="nav-item" aria-current={settings ? 'page' : undefined}><Settings aria-hidden="true" />Settings</Link></nav>
    <div className="sidebar-bottom"><button className="nav-item" onClick={() => { setMobileOpen(false); setShortcuts(value => !value); }}><HelpCircle aria-hidden="true" />Keyboard shortcuts{shortcutPreference.charactersEnabled && <kbd>?</kbd>}</button><details className="account-menu" onKeyDown={event => { if (event.key === 'Escape' && !event.nativeEvent.isComposing && event.currentTarget.open) { event.preventDefault(); event.stopPropagation(); event.currentTarget.open = false; event.currentTarget.querySelector('summary')?.focus(); } }}><summary aria-label={`Account menu: ${session.user.display_name}`}><Avatar user={session.user} /><span>{session.user.display_name}<small>{workspace.role}</small></span></summary><div className="account-popover"><div className="account-email">{session.user.email}</div><div className="appearance-options" role="group" aria-label="Appearance">{([['system', Monitor], ['light', Sun], ['dark', Moon]] as const).map(([value, Icon]) => <button type="button" key={value} aria-label={`Use ${value} appearance`} aria-pressed={theme === value} onClick={() => setTheme(value)} title={`${value} appearance`}><Icon aria-hidden="true" />{value}</button>)}</div><Link className="nav-item" href="/account/security"><LockKeyhole aria-hidden="true" />Account security</Link><button className="nav-item" onClick={logout} disabled={logoutBusy}><LogOut aria-hidden="true" />{logoutBusy ? 'Signing out…' : 'Sign out'}</button></div></details></div>
  </>;
  const inaccessibleWorkspace = !!route.workspaceId && !session.workspaces.some(w => w.id === route.workspaceId);
  return <div className="app-shell"><a className="skip-link" href="#main-content">Skip to content</a><aside className="sidebar">{sidebar}</aside>
    {mobileOpen && <NavigationDrawer close={() => setMobileOpen(false)}>{sidebar}</NavigationDrawer>}
    <div className="workspace-content"><header className="topbar"><button className="icon-button mobile-only" aria-label="Open navigation" aria-expanded={mobileOpen} onClick={() => setMobileOpen(true)}><Menu aria-hidden="true" /></button><div className="breadcrumbs"><span>{workspace.name}</span><span className="breadcrumb-divider">/</span><h1>{title}</h1></div><div className="topbar-actions">{!settings && <><button className="icon-button" aria-label={cycles ? 'Refresh cycles' : projects ? 'Refresh projects' : trash ? 'Refresh Trash' : 'Refresh issues'} onClick={() => setRefresh(n => n + 1)}><RefreshCw aria-hidden="true" /></button>{issueArea && !readOnly && <button className="button primary compact" onClick={() => setCreating(true)} disabled={!activeTeams.length}><Plus aria-hidden="true" />New issue{shortcutPreference.charactersEnabled && <kbd>C</kbd>}</button>}</>}</div></header>
      <main id="main-content" className={`main-content${route.issueId || route.issueKey ? ' with-detail' : ''}`}>
        {workspaceArchived && !settings && <div className="notice">This workspace is archived. Restore it in <Link href={settingsUrl(workspace.id)}>workspace settings</Link> before making changes.</div>}
        {notice && <div className="workspace-notice" role="status">{notice}<button className="icon-button" aria-label="Dismiss message" onClick={() => setNotice('')}><X aria-hidden="true" /></button></div>}
        <ErrorBanner error={error} retry={() => setRetry(n => n + 1)} />
        {inaccessibleWorkspace ? <div className="empty-state"><h2>Workspace unavailable.</h2><p>This workspace may have been removed, or you may no longer have access.</p><Link className="button" href={issuesUrl(workspace.id)}>Open {workspace.name}</Link></div> : accessBlocked ? <Loading label="Refreshing workspace access…" /> : settings ? <SettingsScreen workspaceArchived={workspaceArchived} key={accessRevision} workspace={workspace} session={session} section={route.settingsSection} changed={settingsChanged} workflowChanged={workflowChanged} authenticated={authenticated} /> : !data ? !error && <Loading /> : cycles ? <Cycles key={accessRevision} workspaceId={workspace.id} session={session} data={data} cycleId={route.cycleId} params={route.search} refresh={refresh} readOnly={readOnly}/> : projects ? <Projects key={accessRevision} workspaceId={workspace.id} session={session} data={data} projectId={route.projectId} params={route.search} refresh={refresh} readOnly={readOnly}/> : trash ? <Trash workspaceId={workspace.id} data={data} session={session} params={route.search} selectedId={route.deletedIssueId} refresh={refresh} restored={issue => { update(issue); setNotice(`${issue.key} restored. Its connections remain detached.`); }} workflowChanged={workflowChanged} /> : !data.teams.length ? <div className="empty-state"><h2>No teams are available.</h2><p>Ask a workspace owner to add you to a team.</p></div> : <div className="issue-workspace"><IssueList charactersEnabled={shortcutPreference.charactersEnabled} workspaceArchived={workspaceArchived} key={workspace.id} workspaceId={workspace.id} data={data} session={session} params={route.search} selectedId={route.issueId} refresh={refresh} create={() => setCreating(true)} searchRef={searchRef} />{route.issueKey && <IssueKeyResolver key={`${workspace.id}:${route.issueKey}`} workspaceId={workspace.id} issueKey={route.issueKey} search={route.search} />}{route.issueId && <IssueEditor workspaceArchived={workspaceArchived} key={`${workspace.id}:${route.issueId}`} workspaceId={workspace.id} issueId={route.issueId} data={data} session={session} close={() => navigate(issuesUrl(workspace.id, route.search))} updated={update} deleted={deleted} workflowChanged={workflowChanged} />}</div>}
      </main>
    </div>
    {creating && data && <CreateIssue workspaceId={workspace.id} data={data} session={session} initialTeam={team?.id} close={() => setCreating(false)} created={created} workflowChanged={workflowChanged} />}
    {creatingWorkspace && <CreateWorkspace session={session} close={() => setCreatingWorkspace(false)} reauthenticate={() => setReauth(true)} created={async item => { await settingsChanged(); setCreatingWorkspace(false); navigate(issuesUrl(item.id)); }} />}
    {reauth && <Reauthenticate session={session} close={() => setReauth(false)} authenticated={authenticated} />}
    {palette && !accessBlocked && <CommandPalette key={paletteScope} charactersEnabled={shortcutPreference.charactersEnabled} commands={commands.filter(command => command.id !== 'commands')} issueSource={{ workspaceId: workspace.id, scope: paletteScope, recentIds: recentIssues, open: id => navigate(issuesUrl(workspace.id, new URLSearchParams(), id)) }} close={() => setPalette(false)} />}
    {shortcuts && <CommandHelp preference={shortcutPreference} commands={commands} close={() => setShortcuts(false)} />}
  </div>;
}

function NavigationDrawer({ children, close }: { children: ReactNode; close: () => void }) {
  const ref = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const dialog = ref.current!;
    const trigger = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    dialog.showModal();
    return () => { dialog.close(); trigger?.focus(); };
  }, []);
  return <dialog ref={ref} className="navigation-dialog" aria-label="Workspace navigation" onCancel={event => { event.preventDefault(); close(); }} onClick={event => { if (event.target === event.currentTarget) close(); }}><aside className="sidebar mobile-sidebar">{children}</aside></dialog>;
}
