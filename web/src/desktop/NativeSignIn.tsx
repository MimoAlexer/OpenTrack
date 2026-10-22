import { useEffect, useRef, useState } from 'react';
import { ExternalLink, Monitor } from 'lucide-react';
import { Brand, ErrorBanner } from '../components';
import type { BrowserAction, ConnectionState } from './types';

interface Props {
  connection: ConnectionState;
  begin: (deviceName: string) => Promise<void>;
  cancel: () => Promise<void>;
  openBrowser: (action: BrowserAction) => Promise<void>;
  changeServer: () => void;
  error?: unknown;
  notice?: string;
}

export function NativeSignIn({ connection, begin, cancel, openBrowser, changeServer, error: authError, notice }: Props) {
  const [deviceName, setDeviceName] = useState('OpenTrack desktop');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<unknown>(null);
  const inFlight = useRef(false);
  const [now, setNow] = useState(Date.now());
  const pending = connection.pending;
  const expired = pending ? !Number.isFinite(Date.parse(pending.expires_at)) || Date.parse(pending.expires_at) <= now : false;
  useEffect(() => { document.title = 'Sign in on desktop · OpenTrack'; }, []);
  useEffect(() => {
    if (!pending) return;
    const timer = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(timer);
  }, [pending]);
  async function action(callback: () => Promise<void>) {
    if (inFlight.current) return;
    inFlight.current = true; setBusy(true); setError(null);
    try { await callback(); } catch (error) { setError(error); }
    finally { inFlight.current = false; setBusy(false); }
  }
  return <main className="auth-page"><header className="auth-brand"><Brand /><span>DESKTOP</span></header><section className="auth-panel native-sign-in">
    <div className="eyebrow"><Monitor aria-hidden="true" /> OPENTRACK DESKTOP</div>
    <h1>{pending ? expired ? 'Start sign-in again.' : 'Continue in your browser.' : 'Sign in to OpenTrack.'}</h1>
    <p className="native-origin">{connection.selected?.origin}</p>
    {notice && <div className="notice" role="status">{notice}</div>}
    <ErrorBanner error={error ?? authError} />
    {pending ? <>
      <div className="native-code" aria-label={`Confirmation code ${pending.confirmation_code}`}><span aria-hidden="true">{pending.confirmation_code}</span></div>
      <p>{expired ? 'This request has expired. Cancel it, then start a new sign-in.' : 'Check that the browser shows this same code before approving this device.'}</p>
      {!expired && <p className="muted" role="status">Waiting for browser approval…</p>}
      <button className="button" onClick={() => void action(cancel)} disabled={busy}>{busy ? 'Cancelling…' : 'Cancel sign-in'}</button>
    </> : <>
      <p>Your browser will ask you to approve this app. Your password stays in the browser.</p>
      <form onSubmit={event => { event.preventDefault(); void action(() => begin(deviceName.trim())); }} aria-busy={busy}><fieldset disabled={busy}>
        <label className="field">Device name<input value={deviceName} onChange={event => setDeviceName(event.target.value)} required maxLength={80} autoComplete="off" aria-describedby="native-device-help" /><small id="native-device-help">Shown in Account security so you can identify this session.</small></label>
        <button className="button primary" type="submit"><ExternalLink aria-hidden="true" />{busy ? 'Opening browser…' : 'Continue in browser'}</button>
      </fieldset></form>
      <div className="native-account-actions"><button className="quiet" disabled={busy} onClick={() => void action(() => openBrowser('setup'))}>Set up this server</button><button className="quiet" disabled={busy} onClick={() => void action(() => openBrowser('join'))}>Join with an invitation</button><button className="quiet" disabled={busy} onClick={() => void action(() => openBrowser('recover'))}>Reset password</button></div>
    </>}
    <button className="quiet native-change-server" disabled={busy} onClick={changeServer}>Change server</button>
  </section></main>;
}
