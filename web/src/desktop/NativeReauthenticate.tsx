import { useEffect, useRef, useState } from 'react';
import { ErrorBanner, Modal } from '../components';
import type { Session } from '../types';
import { useDesktop } from './context';

export function NativeReauthenticate({ session, close, authenticated }: { session: Session; close: () => void; authenticated: (session: Session) => void }) {
  const desktop = useDesktop();
  const connection = desktop?.snapshot.connection;
  if (!desktop || !connection) throw new Error('Native reauthentication requires an active desktop connection.');
  const { store, snapshot } = desktop;
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<unknown>(null);
  const initialGeneration = useRef(connection.generation);
  const initialEvent = useRef(snapshot.authEvent);
  const completed = useRef(false);
  const active = useRef(true);
  useEffect(() => { active.current = true; return () => { active.current = false; }; }, []);
  useEffect(() => {
    const event = snapshot.authEvent;
    if (completed.current || !event || event === initialEvent.current || event.kind !== 'authenticated' || !event.session || event.session.user.id !== session.user.id || BigInt(event.generation) <= BigInt(initialGeneration.current)) return;
    completed.current = true;
    authenticated(event.session);
    window.dispatchEvent(new CustomEvent('opentrack:reauthenticated'));
    close();
  }, [snapshot.authEvent, session.user.id]);
  async function start() {
    if (busy) return; setBusy(true); setError(null);
    try { await store.begin('OpenTrack desktop'); }
    catch (error) { if (active.current) setError(error); }
    finally { if (active.current) setBusy(false); }
  }
  async function cancel() {
    if (busy) return; setBusy(true); setError(null);
    try { if (connection?.pending) await store.cancel(); close(); }
    catch (error) { if (active.current) setError(error); }
    finally { if (active.current) setBusy(false); }
  }
  return <Modal title="Confirm it’s you" close={() => void cancel()} canClose={!busy}><div className="settings-modal-content">
    <p>Continue in your browser to confirm {session.user.email}. Your unsaved changes stay here.</p>
    <p className="native-origin">{connection.selected?.origin}</p>
    <ErrorBanner error={error ?? (snapshot.authEvent !== initialEvent.current ? snapshot.error : null)} />
    {connection.pending ? <><div className="native-code" aria-label={`Confirmation code ${connection.pending.confirmation_code}`}><span aria-hidden="true">{connection.pending.confirmation_code}</span></div><p>Only approve if the browser shows this same code.</p><p role="status">Waiting for browser confirmation…</p></> : <p>The browser will confirm your account before reconnecting this session.</p>}
    <div className="modal-footer"><button type="button" className="button" data-autofocus disabled={busy} onClick={() => void cancel()}>Cancel</button>{!connection.pending && <button type="button" className="button primary" disabled={busy} onClick={() => void start()}>{busy ? 'Opening browser…' : 'Continue in browser'}</button>}</div>
  </div></Modal>;
}
