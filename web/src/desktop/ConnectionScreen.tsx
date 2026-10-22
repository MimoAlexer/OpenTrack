import { useEffect, useRef, useState, type FormEvent } from 'react';
import { ArrowRight, Server } from 'lucide-react';
import { Brand, ErrorBanner } from '../components';

interface Props {
  connect: (origin: string, developmentHttp: boolean) => Promise<void>;
  initialOrigin?: string;
  cancel?: () => void;
}

export function ConnectionScreen({ connect, initialOrigin = '', cancel }: Props) {
  const [origin, setOrigin] = useState(initialOrigin);
  const [developmentHttp, setDevelopmentHttp] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<unknown>(null);
  const inFlight = useRef(false);
  useEffect(() => { document.title = 'Connect to a server · OpenTrack'; }, []);
  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault(); if (inFlight.current) return;
    inFlight.current = true; setBusy(true); setError(null);
    try { await connect(origin.trim(), developmentHttp); }
    catch (error) { setError(error); }
    finally { inFlight.current = false; setBusy(false); }
  }
  return <main className="auth-page"><header className="auth-brand"><Brand /><span>DESKTOP</span></header><section className="auth-panel native-connection">
    <div className="eyebrow"><Server aria-hidden="true" /> YOUR SERVER</div>
    <h1>Connect to OpenTrack.</h1>
    <p>Enter the address of the OpenTrack server you use in your browser.</p>
    <form onSubmit={submit} aria-busy={busy}><fieldset disabled={busy}>
      <label className="field">Server address<input autoFocus type="url" inputMode="url" autoCapitalize="none" autoCorrect="off" spellCheck={false} value={origin} onChange={event => setOrigin(event.target.value)} placeholder="https://work.example.com" maxLength={2048} required aria-describedby="native-server-help" /><small id="native-server-help">Use the server origin without a page path. Your account and work belong to this server.</small></label>
      <label className="native-match"><input type="checkbox" checked={developmentHttp} onChange={event => setDevelopmentHttp(event.target.checked)} />Allow HTTP for a local development server</label>
      {developmentHttp && <p className="muted">Only a loopback address on this computer can use HTTP.</p>}
      <ErrorBanner error={error} />
      <div className="button-row"><button className="button primary" type="submit"><ArrowRight aria-hidden="true" />{busy ? 'Connecting…' : 'Connect to this server'}</button>{cancel && <button type="button" className="button" onClick={cancel}>Cancel</button>}</div>
    </fieldset></form>
  </section></main>;
}
