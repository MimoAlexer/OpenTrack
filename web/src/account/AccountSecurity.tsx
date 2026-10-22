import { useDesktop } from '../desktop/context';
import { useNativeConfirmation } from '../desktop/ConfirmAction';
import { useEffect, useRef, useState } from 'react';
import { ArrowLeft, LogOut, RefreshCw, ShieldCheck } from 'lucide-react';
import { ApiError } from '../api';
import { Avatar, Brand, Loading } from '../components';
import { issuesUrl, Link } from '../router';
import { DirectoryFooter, Reauthenticate, SettingsError, useDirectory, useScopedRequest } from '../settings/shared';
import type { Session } from '../types';
import type { SessionInfo } from './types';
import './security.css';

function timestamp(value: string) {
  const date = new Date(value);
  return Number.isNaN(date.valueOf()) ? 'Unavailable' : new Intl.DateTimeFormat(undefined, { dateStyle: 'medium', timeStyle: 'short' }).format(date);
}
function sessionLabel(item: SessionInfo) { return item.audience === 'desktop' ? item.device_name || 'Desktop session' : item.current ? 'This session' : `Session ${item.id.slice(0, 8)}`; }

export function AccountSecurity({ session, authenticated, signedOut }: { session: Session; authenticated: (session: Session) => void; signedOut: () => void }) {
  const desktop = useDesktop();
  const nativeConfirm = useNativeConfirmation(true);
  const request = useScopedRequest();
  const resource = useDirectory<SessionInfo>('/auth/sessions');
  const [pending, setPending] = useState<string | null>(null);
  const [error, setError] = useState<unknown>(null);
  const [notice, setNotice] = useState('');
  const [reauth, setReauth] = useState(false);
  const refreshButton = useRef<HTMLButtonElement>(null);
  const focusAfterReauth = useRef(false);
  const entries = [...resource.items].sort((a, b) => Number(b.current) - Number(a.current) || Date.parse(b.auth_time) - Date.parse(a.auth_time));
  const others = entries.filter(item => !item.current);
  const mayHaveOtherSessions = !!others.length || resource.hasMore;
  const back = session.workspaces[0] ? issuesUrl(session.workspaces[0].id) : '/';
  useEffect(() => { document.title = 'Account security · OpenTrack'; }, []);
  useEffect(() => {
    if (!focusAfterReauth.current || reauth || resource.loading || pending) return;
    focusAfterReauth.current = false;
    refreshButton.current?.focus();
  }, [reauth, resource.loading, pending]);

  async function revoke(item: SessionInfo) {
    if (pending) return;
    if (item.current && !(desktop ? await nativeConfirm!({ title: 'Sign out this session?', description: 'Unsaved changes will be discarded.', action: 'Sign out' }) : window.confirm('Sign out this session? Unsaved edits in this tab will be discarded.'))) return;
    setPending(item.id); setError(null); setNotice('');
    try {
      if (desktop && item.current) await desktop.store.logout();
      else await request(`/auth/sessions/${encodeURIComponent(item.id)}/revoke`, { method: 'POST', csrf: session.csrf_token });
      if (item.current) signedOut();
      else { setNotice(`${sessionLabel(item)} signed out.`); resource.reload(); }
    } catch (error) {
      setError(error instanceof ApiError && error.status === 404 ? new ApiError(404, error.code, 'That session is no longer available. Refresh the list before trying again.', error.requestId) : error);
    } finally { setPending(null); }
  }
  async function revokeOthers() {
    if (pending || resource.loading || !mayHaveOtherSessions) return;
    if (!(desktop ? await nativeConfirm!({ title: 'Sign out other sessions?', description: 'Every other active sign-in will end. This session stays signed in.', action: 'Sign out other sessions' }) : window.confirm('Sign out every other active session? This session will stay signed in.'))) return;
    setPending('others'); setError(null); setNotice('');
    try {
      await request('/auth/sessions/revoke-others', { method: 'POST', csrf: session.csrf_token });
      setNotice('All other sessions signed out.'); resource.reload();
    } catch (error) { setError(error); } finally { setPending(null); }
  }
  function reauthenticated(updated: Session) {
    // The successful confirmation removes its trigger; hand off focus after reload.
    focusAfterReauth.current = true;
    authenticated(updated);
    // Login rotates the current session, so replace its marker before another action.
    resource.reload();
  }

  return <div className="account-security"><a className="skip-link" href="#account-content">Skip to content</a><header className="account-topbar"><Brand /><Link className="quiet" href={back}><ArrowLeft aria-hidden="true" />{session.workspaces.length ? 'Back to workspace' : 'Back to account'}</Link></header><main id="account-content" className="account-content"><header className="account-heading"><div><div className="account-eyebrow"><ShieldCheck aria-hidden="true" />Your account</div><h1>Security</h1><p>Review your active sign-ins and end sessions you no longer need.</p></div><div className="account-identity"><Avatar user={session.user} /><div><strong>{session.user.display_name}</strong><span>{session.user.email}</span></div></div></header><section className="account-sessions" aria-labelledby="sessions-heading"><header className="account-section-heading"><div><h2 id="sessions-heading">Active sessions</h2><p>These sessions belong to your account across all workspaces.</p></div><div className="button-row"><button ref={refreshButton} type="button" className="icon-button" aria-label="Refresh sessions" disabled={!!pending || resource.loading} onClick={() => { setError(null); setNotice(''); resource.reload(); }}><RefreshCw aria-hidden="true" /></button><button type="button" className="button" disabled={!!pending || resource.loading || !mayHaveOtherSessions} onClick={() => void revokeOthers()}><LogOut aria-hidden="true" />{pending === 'others' ? 'Signing out…' : 'Sign out other sessions'}</button></div></header><SettingsError error={resource.error} retry={resource.reload} reauthenticate={() => setReauth(true)} /><SettingsError error={error} reauthenticate={() => setReauth(true)} />{notice && <div className="account-notice" role="status">{notice}</div>}{resource.loading && !entries.length ? <Loading label="Loading active sessions…" /> : entries.length ? <><p className="session-scroll-hint">Scroll horizontally for session times and actions.</p><div className="session-table-wrap" role="region" aria-label="Session details" tabIndex={0}><table className="session-table"><caption className="sr-only">Your active sign-in sessions. Times use this device’s timezone.</caption><thead><tr><th scope="col">Session</th><th scope="col">Signed in</th><th scope="col">Last active</th><th scope="col">Expires</th><th scope="col"><span className="sr-only">Actions</span></th></tr></thead><tbody>{entries.map(item => <tr key={item.id}><th scope="row"><strong>{sessionLabel(item)}{item.current && <span className="session-current">Current</span>}</strong><small>{item.audience === 'desktop' ? 'Desktop app' : 'Browser'} · Created <time dateTime={item.created_at}>{timestamp(item.created_at)}</time></small></th><td><time dateTime={item.auth_time}>{timestamp(item.auth_time)}</time></td><td><time dateTime={item.last_seen_at}>{timestamp(item.last_seen_at)}</time></td><td><time dateTime={item.expires_at}>{timestamp(item.expires_at)}</time><small>Idle: <time dateTime={item.idle_expires_at}>{timestamp(item.idle_expires_at)}</time></small></td><td className="session-action"><button type="button" className="button compact" disabled={!!pending} aria-label={item.current ? 'Sign out this session' : `Revoke session ${item.id}`} onClick={() => void revoke(item)}>{pending === item.id ? 'Signing out…' : item.current ? 'Sign out' : 'Revoke'}</button></td></tr>)}</tbody></table></div><footer className="session-table-footer"><span>Times use this device’s timezone. A session ends at its idle or absolute expiry, whichever comes first.</span>{resource.hasMore ? <span>{entries.length} sessions loaded. More are available.</span> : !others.length && <span>Only this session is signed in.</span>}</footer><DirectoryFooter directory={resource} /></> : !resource.error && <div className="account-empty"><h3>No active sessions were returned.</h3><p>Refresh the list to check your current sign-in.</p><button type="button" className="button" onClick={resource.reload}>Refresh sessions</button></div>}</section></main>{reauth && <Reauthenticate session={session} close={() => setReauth(false)} authenticated={reauthenticated} />}</div>;
}
