import { useRef, useState } from 'react';
import { Modal } from '../components';
import type { AttachmentDraftState } from './types';

export function DiscardUploadDraft({ title, description, action, uploads, close, discard }: {
  title: string; description: string; action: string; uploads: AttachmentDraftState | null; close: () => void; discard: () => void;
}) {
  const [canceling, setCanceling] = useState(false); const [error, setError] = useState(''); const flight = useRef(false);
  async function confirm() {
    if (flight.current) return;
    if (!uploads?.pending) { discard(); return; }
    flight.current = true; setCanceling(true); setError('');
    try {
      if (await uploads.cancelAll()) discard();
      else setError('Some uploads could not be canceled. Your draft is still here. Retry, or keep editing to review the files.');
    } catch { setError('Upload cancellation could not be confirmed. Your draft is still here. Retry, or keep editing.'); }
    finally { flight.current = false; setCanceling(false); }
  }
  return <Modal title={title} close={close} canClose={!canceling}><div className="settings-modal-content"><p>{description}</p>{error && <p className="document-notice" role="alert">{error}</p>}{canceling && <p className="document-notice" role="status">Canceling uploads…</p>}<div className="modal-footer"><button type="button" className="button" data-autofocus disabled={canceling} onClick={close}>Keep editing</button><button type="button" className="button primary" disabled={canceling} onClick={() => { void confirm(); }}>{canceling ? 'Canceling uploads…' : error ? 'Retry cancellation' : action}</button></div></div></Modal>;
}
