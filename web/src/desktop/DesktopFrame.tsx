import { createContext, useContext, useRef, useState, type FormEvent, type ReactNode } from 'react';
import { Server } from 'lucide-react';
import { ErrorBanner, Modal } from '../components';
import { hasUnsavedChanges, issuesUrl, navigateConfirmed } from '../router';
import { useNativeConfirmation } from './ConfirmAction';
import { useDesktop } from './context';
import { IncomingIssueLink } from './IncomingIssueLink';

const ServerReviewContext = createContext<(() => void) | null>(null);
export function useServerReview() { return useContext(ServerReviewContext); }

export function DesktopFrame({ children }: { children: ReactNode }) {
  const desktop = useDesktop()!; const { store, snapshot } = desktop;
  const connection = snapshot.connection!;
  const [editing, setEditing] = useState<{ origin: string; development: boolean } | null>(null);
  const confirm = useNativeConfirmation();
  function review(origin = connection.selected?.origin ?? '') { setEditing({ origin, development: !!connection.selected && origin === connection.selected.origin && connection.selected.development_http }); }
  return <div className="desktop-frame"><header className="desktop-server-bar"><Server aria-hidden="true" /><span className="native-origin">{connection.selected?.origin ?? 'No server selected'}</span><button type="button" className="quiet" onClick={() => review()}>Change server</button></header>
    {snapshot.issueLink && <IncomingIssueLink key={snapshot.issueLink.id} link={snapshot.issueLink} selectedOrigin={connection.selected?.origin ?? null} signedIn={connection.credential_present} reviewConnection={review} dismiss={id => store.dismissIssueLink(id)} open={async link => {
      if (hasUnsavedChanges() && !await confirm({ title: 'Open another issue?', description: 'Your unsaved changes will be discarded.', action: 'Discard and open' })) return false;
      navigateConfirmed(issuesUrl(link.workspace, new URLSearchParams(), link.issue)); return true;
    }} />}
    <ServerReviewContext.Provider value={() => review()}><div className="desktop-client">{children}</div></ServerReviewContext.Provider>
    {editing && <ChangeServer initial={editing} close={() => setEditing(null)} select={async (origin, http) => {
      if (hasUnsavedChanges() && !await confirm({ title: 'Switch servers?', description: 'Your unsaved changes will be discarded after the new connection succeeds.', action: 'Discard and switch' })) return false;
      await store.select(origin, http); navigateConfirmed('/', true); setEditing(null); return true;
    }} forget={connection.selected ? async () => {
      if (!await confirm({ title: 'Forget this server?', description: 'The saved connection and sign-in will be removed from this computer. This does not confirm remote session revocation. Unsaved changes will be discarded.', action: 'Forget server' })) return;
      await store.forget(); navigateConfirmed('/', true); setEditing(null);
    } : undefined} />}
  </div>;
}

function ChangeServer({ initial, close, select, forget }: { initial: { origin: string; development: boolean }; close: () => void; select: (origin: string, http: boolean) => Promise<boolean>; forget?: () => Promise<void> }) {
  const [origin, setOrigin] = useState(initial.origin); const [http, setHttp] = useState(initial.development);
  const [busy, setBusy] = useState(false); const [error, setError] = useState<unknown>(null); const inFlight = useRef(false);
  async function submit(event: FormEvent) { event.preventDefault(); if (inFlight.current) return; inFlight.current = true; setBusy(true); setError(null); try { await select(origin.trim(), http); } catch (error) { setError(error); } finally { setBusy(false); inFlight.current = false; } }
  async function forgetCurrent() { if (!forget || inFlight.current) return; inFlight.current = true; setBusy(true); setError(null); try { await forget(); } catch (error) { setError(error); } finally { setBusy(false); inFlight.current = false; } }
  return <Modal title="Server connection" close={close} canClose={!busy}><form className="settings-modal-content" onSubmit={submit}><p>Choose the server that holds your OpenTrack account and work.</p><ErrorBanner error={error} /><fieldset disabled={busy}><label className="field">Server address<input data-autofocus type="url" value={origin} onChange={event => setOrigin(event.target.value)} maxLength={2048} required autoCapitalize="none" autoCorrect="off" spellCheck={false} /></label><label className="native-match"><input type="checkbox" checked={http} onChange={event => setHttp(event.target.checked)} />Allow HTTP for a loopback development server</label></fieldset><div className="modal-footer">{forget && <button type="button" className="quiet" disabled={busy} onClick={() => void forgetCurrent()}>Forget current server</button>}<button type="button" className="button" disabled={busy} onClick={close}>Cancel</button><button type="submit" className="button primary" disabled={busy || !origin.trim()}>{busy ? 'Connecting…' : 'Connect to this server'}</button></div></form></Modal>;
}
