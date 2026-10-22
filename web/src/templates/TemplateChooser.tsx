import { useEffect, useRef, useState } from 'react';
import { FileText, RefreshCw } from 'lucide-react';
import { isAccessError } from '../documents/model';
import { request } from '../api';
import { ErrorBanner, Modal } from '../components';
import { useSessionScope } from '../desktop/context';
import type { Session } from '../types';
import { supportedTemplate, TemplatePreviewContent } from './TemplatePreview';
import { templatePath, type TemplatePreview, type TemplateSelection } from './types';
import { useTemplates } from './useTemplates';
import './templates.css';

export function TemplateChooser({ workspaceId, teamId, session, selection, selectedName, disabled, pendingUploads, edited, needsReview, apply, review, clear }: {
  workspaceId: string; teamId: string; session: Session; selection: TemplateSelection | null; selectedName: string; disabled: boolean; pendingUploads: boolean; edited: boolean; needsReview: boolean;
  apply: (preview: TemplatePreview, mode: 'merge' | 'replace') => string | null; review: (preview: TemplatePreview) => void; clear: () => void;
}) {
  const [open, setOpen] = useState(false);
  const trigger = useRef<HTMLButtonElement>(null); const reviewFocused = useRef(false);
  useEffect(() => { if (!needsReview) reviewFocused.current = false; else if (!disabled && !reviewFocused.current) { trigger.current?.focus(); reviewFocused.current = true; } }, [needsReview, disabled]);
  return <div className="template-choice"><button ref={trigger} type="button" className="quiet" disabled={disabled || !teamId} onClick={() => setOpen(true)}><FileText aria-hidden="true"/>{selection ? selectedName || 'Applied template' : 'Use a template'}</button>{selection && <><span className="muted">v{selection.version}</span><button type="button" className="quiet" disabled={disabled} onClick={clear}>Use draft without template</button></>}{needsReview && <p className="template-warning" role="alert">The template changed or is unavailable. Your draft is preserved. Review the template or continue without it before creating.</p>}{open && <Chooser key={`${workspaceId}:${teamId}`} workspaceId={workspaceId} teamId={teamId} session={session} initialId={selection?.id} pendingUploads={pendingUploads} edited={edited} needsReview={needsReview} close={() => setOpen(false)} apply={(value, mode) => { const error = apply(value, mode); if (!error) setOpen(false); return error; }} review={value => { review(value); setOpen(false); }}/>}</div>;
}
function Chooser({ workspaceId, teamId, session, initialId, pendingUploads, edited, needsReview, close, apply, review }: {
  workspaceId: string; teamId: string; session: Session; initialId?: string; pendingUploads: boolean; edited: boolean; needsReview: boolean; close: () => void;
  apply: (preview: TemplatePreview, mode: 'merge' | 'replace') => string | null; review: (preview: TemplatePreview) => void;
}) {
  const scope = useSessionScope(session); const [selected, setSelected] = useState(initialId ?? ''); const [retry, setRetry] = useState(0);
  const directory = useTemplates(workspaceId, teamId, scope, false);
  const directoryDenied = isAccessError(directory.error);
  const key = `${workspaceId}:${teamId}:${scope}:${selected}`;
  const [state, setState] = useState<{ key: string; value: TemplatePreview | null; loading: boolean; error: unknown }>({ key, value: null, loading: false, error: null });
  const [applyError, setApplyError] = useState<string | null>(null);
  useEffect(() => {
    const abort = new AbortController(); setState(old => ({ key, value: old.key === key && !directoryDenied ? old.value : null, loading: !!selected, error: null })); setApplyError(null);
    if (directoryDenied) { setState({ key, value: null, loading: false, error: directory.error }); return () => abort.abort(); }
    if (selected) request<TemplatePreview>(`${templatePath(workspaceId, teamId)}/${selected}/preview`, { signal: abort.signal }).then(value => { if (!abort.signal.aborted && value.template.id === selected && value.template.team_id === teamId) setState({ key, value, loading: false, error: null }); }).catch(error => { if (!abort.signal.aborted) setState(old => ({ key, value: old.key === key && !isAccessError(error) ? old.value : null, loading: false, error })); });
    return () => abort.abort();
  }, [key, retry, directoryDenied]);
  const preview = !directoryDenied && state.key === key ? state.value : null; const allowed = !!preview && !state.loading && !state.error && preview.template.can_use && supportedTemplate(preview) && !pendingUploads;
  return <Modal title="Choose an issue template" close={close}><div className="template-picker"><div className="template-picker-list"><div className="template-picker-heading"><strong>Team templates</strong><button type="button" className="icon-button" aria-label="Refresh templates" onClick={() => { directory.refresh(); setRetry(value => value + 1); }}><RefreshCw aria-hidden="true"/></button></div><ErrorBanner error={directory.error} retry={() => { directory.refresh(); setRetry(value => value + 1); }}/>{directory.loading && <p role="status" className="muted">Loading templates…</p>}{directory.page?.data.map(item => <button type="button" key={item.id} aria-pressed={selected === item.id} onClick={() => setSelected(item.id)}><span>{item.name}</span><small>v{item.version}</small></button>)}{!directory.loading && !directory.error && !directory.page?.data.length && <p className="settings-empty">No active templates in this team.</p>}{directory.page?.has_more && <button type="button" className="button compact" disabled={directory.loading} onClick={directory.more}>Load more</button>}</div><div className="template-picker-preview"><ErrorBanner error={state.key === key ? state.error : null} retry={() => setRetry(value => value + 1)}/><p role="status" className="template-preview-state muted">{state.loading || state.key !== key ? preview ? 'Refreshing preview…' : 'Loading preview…' : ''}</p>{preview ? <TemplatePreviewContent value={preview}/> : !selected && <p className="settings-empty">Choose a template to preview its fields and description.</p>}</div></div><div className="template-apply"><ErrorBanner error={applyError}/>{pendingUploads && <p role="status">Finish or cancel pending uploads before applying a template. Your files and insertion positions are preserved.</p>}{preview && !preview.template.can_use && <p>This template is not available for new issues.</p>}{preview && edited && <p>Merge keeps your edits and combines descriptions and labels. Replace uses the template fields. Existing attachments, assignee, estimate, and due date stay in either case.</p>}<div className="button-row"><button type="button" className="button" onClick={close}>Cancel</button>{needsReview && selected === initialId && <button type="button" className="button" disabled={!allowed} onClick={() => preview && review(preview)}>Keep draft with reviewed version</button>}{edited ? <><button type="button" className="button" disabled={!allowed} onClick={() => preview && setApplyError(apply(preview, 'replace'))}>Replace template fields</button><button type="button" className="button primary" disabled={!allowed} onClick={() => preview && setApplyError(apply(preview, 'merge'))}>Merge into draft</button></> : <button type="button" className="button primary" disabled={!allowed} onClick={() => preview && setApplyError(apply(preview, 'replace'))}>Use template</button>}</div></div></Modal>;
}
