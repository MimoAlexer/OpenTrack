import { useState } from 'react';
import { ExternalLink } from 'lucide-react';
import { ErrorBanner, Modal } from '../components';
import type { NativeIssueLink } from './types';

interface Props {
  link: NativeIssueLink;
  selectedOrigin: string | null;
  signedIn: boolean;
  reviewConnection: (origin: string) => void;
  open: (link: NativeIssueLink) => Promise<boolean>;
  dismiss: (id: string) => Promise<void>;
}

export function IncomingIssueLink({ link, selectedOrigin, signedIn, reviewConnection, open, dismiss }: Props) {
  const [reviewing, setReviewing] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<unknown>(null);
  const currentServer = link.server === selectedOrigin;
  async function perform(callback: () => Promise<void>) {
    if (busy) return; setBusy(true); setError(null);
    try { await callback(); } catch (error) { setError(error); } finally { setBusy(false); }
  }
  return <>
    <aside className="native-incoming-link" aria-label="Received issue link"><ExternalLink aria-hidden="true" /><span>Issue link received from <strong>{link.server}</strong></span><button type="button" className="button compact" onClick={() => setReviewing(true)}>Review link</button></aside>
    {reviewing && <Modal title="Open this issue link?" close={() => setReviewing(false)} canClose={!busy}><div className="settings-modal-content">
      <p>Server <strong className="native-origin">{link.server}</strong></p>
      <p>{currentServer ? signedIn ? 'The issue will open with your current account if you have access.' : 'Sign in to this server, then review the link again.' : 'This link points to a different server. Review its connection before signing in or opening the issue.'}</p>
      <ErrorBanner error={error} />
      <div className="modal-footer"><button type="button" className="button" data-autofocus disabled={busy} onClick={() => setReviewing(false)}>Not now</button><button type="button" className="button" disabled={busy} onClick={() => void perform(() => dismiss(link.id))}>Dismiss link</button>{currentServer && signedIn ? <button type="button" className="button primary" disabled={busy} onClick={() => void perform(async () => { if (await open(link)) { await dismiss(link.id); setReviewing(false); } })}>Open issue</button> : !currentServer ? <button type="button" className="button primary" disabled={busy} onClick={() => { setReviewing(false); reviewConnection(link.server); }}>Review connection</button> : null}</div>
    </div></Modal>}
  </>;
}
