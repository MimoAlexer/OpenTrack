import { useState } from 'react';
import { ExternalLink } from 'lucide-react';
import { useScopedRequest } from '../settings/shared';
import { Brand, ErrorBanner } from '../components';
import { issuesUrl, Link, navigate } from '../router';
import type { Session } from '../types';
import { useDesktop } from './context';
import type { BrowserAction } from './types';

export function BrowserAccountAction({ action, session, authenticated }: { action: BrowserAction; session: Session | null; authenticated: (session: Session) => void }) {
  const request = useScopedRequest();
  const desktop = useDesktop()!; const [busy, setBusy] = useState(false); const [error, setError] = useState<unknown>(null);
  const title = action === 'join' ? 'Join a workspace in your browser.' : action === 'recover' ? 'Reset your password in your browser.' : 'Set up OpenTrack in your browser.';
  async function open() { if (busy) return; setBusy(true); setError(null); try { await desktop.store.bridge.openBrowser(action); } catch (error) { if (!(error instanceof DOMException && error.name === 'AbortError')) setError(error); } finally { setBusy(false); } }
  async function refresh() { if (busy) return; setBusy(true); setError(null); try { const current = await request<Session>('/auth/session'); authenticated(current); navigate(current.workspaces[0] ? issuesUrl(current.workspaces[0].id) : '/'); } catch (error) { if (!(error instanceof DOMException && error.name === 'AbortError')) setError(error); } finally { setBusy(false); } }
  return <main className="startup"><Brand /><h1>{title}</h1><p className="native-origin">{desktop.snapshot.connection?.selected?.origin}</p><p>{action === 'join' ? 'Paste your invitation in the browser. When it is accepted, return here and refresh your workspace access.' : action === 'recover' ? 'Use the operator-issued recovery code in the browser. Resetting the password ends the account’s existing sessions; sign in to the app again afterward.' : 'Create the first account and workspace in the browser, then return here to sign in.'}</p><ErrorBanner error={error} /><div className="button-row"><button type="button" className="button primary" disabled={busy} onClick={() => void open()}><ExternalLink aria-hidden="true" />Open browser</button>{action === 'join' && session ? <button type="button" className="button" disabled={busy} onClick={() => void refresh()}>Refresh workspace access</button> : <Link className="button" href="/signin">Sign in to the app</Link>}<Link className="quiet" href="/">Back to OpenTrack</Link></div></main>;
}
