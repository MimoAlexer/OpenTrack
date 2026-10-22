import { DocumentView } from './DocumentView';
import { useState } from 'react';
import { attachmentIds, canonicalBody, withoutAttachments, type RichBody } from './model';
export function DocumentConflict({ latest, local, choose }: { latest: RichBody; local: RichBody; choose: (body: RichBody, review: boolean) => void }) {
  const [error, setError] = useState('');
  function combine() {
    try { const localUnique = withoutAttachments(local, new Set(attachmentIds(latest))); choose(canonicalBody({ type: 'doc', content: [...latest.content, ...localUnique.content] }), true); }
    catch (error) { setError(error instanceof Error ? error.message : 'These versions cannot be combined. Keep one version and edit it.'); }
  }
  return <section className="document-compare" role="alert" aria-label="Document conflict"><h3>This text changed in another session.</h3><p>Your draft is preserved. Compare both versions, then choose what to keep. Nothing is overwritten automatically.</p><div className="document-compare-columns"><section><h4>Latest saved version</h4><DocumentView body={latest} /></section><section><h4>Your draft</h4><DocumentView body={local} /></section></div>{error && <p className="document-notice">{error}</p>}{attachmentIds(latest).some(id => attachmentIds(local).includes(id)) && <p>Files present in both versions will appear once in the combined draft.</p>}<div className="button-row"><button type="button" className="button compact" onClick={() => choose(latest, false)}>Use latest text</button><button type="button" className="button compact" onClick={() => choose(local, true)}>Keep my draft for review</button><button type="button" className="button compact" onClick={combine}>Combine both for review</button></div></section>;
}
