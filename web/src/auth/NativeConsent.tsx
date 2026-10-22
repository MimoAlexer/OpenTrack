import { useEffect, useRef, useState } from 'react';
import { Monitor } from 'lucide-react';
import { ApiError } from '../api';
import { Brand, ErrorBanner, Loading } from '../components';
import { Link } from '../router';
import { Reauthenticate, useScopedRequest } from '../settings/shared';
import type { Session, NativeAuthConsent, NativeAuthApproval } from '../types';
import { AuthScreen } from './AuthScreen';
import { nativeCallbackUri } from './native-callback';

type Outcome = 'review' | 'uncertain' | 'unavailable' | 'handoff';

export function NativeConsent({ requestId, session, authenticated }: { requestId: string | null; session: Session | null; authenticated: (session: Session) => void }) {
  if (!requestId) return <main className="auth-page"><header className="auth-brand"><Brand /></header><section className="auth-panel"><h1>Desktop request unavailable.</h1><p>Return to the OpenTrack app and start sign-in again.</p><Link className="button" href="/">Back to OpenTrack</Link></section></main>;
  if (!session) return <AuthScreen setup={false} notice="Sign in to review the request from your OpenTrack desktop app." onAuthenticated={authenticated} />;
  return <ConsentReview key={`${requestId}:${session.user.id}`} requestId={requestId} session={session} authenticated={authenticated} />;
}

function ConsentReview({ requestId, session, authenticated }: { requestId: string; session: Session; authenticated: (session: Session) => void }) {
  const request = useScopedRequest();
  const [metadata, setMetadata] = useState<NativeAuthConsent | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<unknown>(null);
  const [matched, setMatched] = useState(false);
  const [busy, setBusy] = useState(false);
  const inFlight = useRef(false);
  const [outcome, setOutcome] = useState<Outcome>('review');
  const [reauth, setReauth] = useState(false);
  const [revision, setRevision] = useState(0);
  const [now, setNow] = useState(Date.now());
  const heading = useRef<HTMLHeadingElement>(null);
  const scope = `${requestId}:${session.user.id}:${session.csrf_token}`;
  const currentScope = useRef(scope); currentScope.current = scope;
  const path = `/auth/native/requests/${requestId}`;
  useEffect(() => {
    document.title = 'Approve desktop sign-in · OpenTrack';
    const timer = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(timer);
  }, []);
  useEffect(() => {
    const abort = new AbortController();
    setLoading(true); setMetadata(null); setError(null); setMatched(false); setOutcome('review');
    request<NativeAuthConsent>(path, { signal: abort.signal }).then(value => {
      if (abort.signal.aborted || currentScope.current !== scope) return;
      if (value.id !== requestId) throw new Error('The server returned an unexpected desktop request.');
      setMetadata(value);
    }).catch(error => {
      if (abort.signal.aborted || currentScope.current !== scope) return;
      setError(error);
      if (error instanceof ApiError && [404, 422].includes(error.status)) setOutcome('unavailable');
    }).finally(() => { if (!abort.signal.aborted && currentScope.current === scope) setLoading(false); });
    return () => abort.abort();
  }, [scope, revision]);
  const expired = metadata ? !Number.isFinite(Date.parse(metadata.expires_at)) || Date.parse(metadata.expires_at) <= now : false;
  async function approve() {
    if (inFlight.current || !matched || !metadata || expired || outcome !== 'review' || loading) return;
    inFlight.current = true; setBusy(true); setError(null);
    try {
      const result = await request<NativeAuthApproval>(`${path}/approve`, { method: 'POST', csrf: session.csrf_token, body: { confirmation_code: metadata.confirmation_code } });
      if (currentScope.current !== scope) return;
      const callback = nativeCallbackUri(result.callback_uri, requestId);
      if (!callback) { setOutcome('uncertain'); throw new Error('The desktop handoff could not be opened. Return to the app and start sign-in again.'); }
      setOutcome('handoff');
      window.location.replace(callback);
    } catch (error) {
      if (currentScope.current !== scope || error instanceof DOMException && error.name === 'AbortError') return;
      setError(error);
      if (error instanceof ApiError && error.code === 'reauthentication_required') setReauth(true);
      else if (error instanceof ApiError && [404, 422].includes(error.status)) setOutcome('unavailable');
      else if (!(error instanceof ApiError && error.status === 429)) setOutcome('uncertain');
    } finally { inFlight.current = false; setBusy(false); }
  }
  useEffect(() => { if (outcome !== 'review') heading.current?.focus(); }, [outcome]);
  const unavailable = outcome === 'unavailable' || expired;
  return <main className="auth-page"><header className="auth-brand"><Brand /><span>DESKTOP SIGN-IN</span></header><section className="auth-panel native-consent">
    <div className="eyebrow"><Monitor aria-hidden="true" /> OPENTRACK DESKTOP</div>
    <h1 ref={heading} tabIndex={-1}>{outcome === 'handoff' ? 'Return to the app.' : outcome === 'uncertain' ? 'Check the desktop app.' : unavailable ? 'This request has ended.' : 'Approve desktop sign-in.'}</h1>
    {outcome === 'handoff' ? <p role="status">The browser handoff is opening. The app will confirm when sign-in finishes.</p> : outcome === 'uncertain' ? <><p>The response was interrupted or the handoff could not be opened. Approval may have succeeded.</p><p>Return to the app. If sign-in did not finish, start a new request there. You can review active devices in Account security.</p><Link className="button" href="/account/security">Account security</Link></> : unavailable ? <p>Return to the OpenTrack app and start sign-in again.</p> : <>
      <p>Signed in as <strong>{session.user.email}</strong> on <span className="native-origin">{window.location.origin}</span>.</p>
      {loading ? <Loading label="Loading desktop request…" /> : metadata && <>
        <p>{metadata.reauthentication ? 'Reconnect' : 'Sign in to'} <strong>{metadata.device_name}</strong>.</p>
        <div className="native-code" aria-label={`Confirmation code ${metadata.confirmation_code}`}><span aria-hidden="true">{metadata.confirmation_code}</span></div>
        <p>Only approve if this code matches the code shown in the OpenTrack app.</p>
        <p className="muted">Expires at {new Date(metadata.expires_at).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}.</p>
        <label className="native-match"><input type="checkbox" checked={matched} onChange={event => setMatched(event.target.checked)} disabled={busy} />The codes match.</label>
        <div className="button-row"><button className="button primary" onClick={() => void approve()} disabled={busy || !matched}>{busy ? 'Approving…' : 'Approve this device'}</button><Link className="button" href="/">Cancel</Link></div>
      </>}
    </>}
    <ErrorBanner error={error} retry={!metadata && outcome === 'review' && !loading ? () => setRevision(value => value + 1) : undefined} />
    {error instanceof ApiError && error.code === 'reauthentication_required' && !reauth && <button className="button" onClick={() => setReauth(true)}>Confirm your password</button>}
    {outcome !== 'review' || unavailable ? <Link className="quiet" href="/">Back to OpenTrack</Link> : null}
    {reauth && <Reauthenticate session={session} authenticated={authenticated} close={() => setReauth(false)} />}
  </section></main>;
}
