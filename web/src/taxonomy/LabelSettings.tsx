import { useSessionScope } from '../desktop/context';
import { useRef, useState, type FormEvent } from 'react';
import { Archive, ArchiveRestore, Pencil, Plus, RefreshCw } from 'lucide-react';
import { ApiError } from '../api';
import { ErrorBanner, Modal } from '../components';
import { useCreateRequest, useScopedRequest, UncertainNotice } from '../settings/shared';
import type { Session } from '../types';
import { statusColors } from '../workflows/model';
import { useSearchPage } from './useSearchPage';
import { LabelDot } from './LabelPicker';
import { labelETag, type Label, type LabelPage } from './types';
import './taxonomy.css';

export function LabelSettings({ workspaceId, teamId, scopeName, session, canCreate, archived = false }: { workspaceId: string; teamId?: string; scopeName: string; session: Session; canCreate: boolean; archived?: boolean }) {
  const sessionScope = useSessionScope(session);
  const [query, setQuery] = useState(''); const [state, setState] = useState('active');
  const [edit, setEdit] = useState<{ label?: Label; mode: 'edit' | 'archive' | 'restore' } | null>(null); const [message, setMessage] = useState(''); const heading = useRef<HTMLHeadingElement>(null);
  const directory = useSearchPage<Label, LabelPage>(`/workspaces/${workspaceId}/labels?scope=${teamId ? 'team' : 'workspace'}${teamId ? `&team_id=${teamId}` : ''}&state=${state}`, sessionScope, query, true);
  return <section className="label-settings" aria-label={`${scopeName} labels`}>
    <header className="workflow-heading"><div><h3 tabIndex={-1} ref={heading}>{teamId ? 'Team labels' : 'Shared labels'}</h3><p>{teamId ? `Available to issues in ${scopeName}.` : 'Available to all teams in this workspace.'}</p></div><div className="button-row"><button type="button" className="icon-button" aria-label="Refresh labels" onClick={directory.refresh}><RefreshCw aria-hidden="true" /></button>{canCreate && !archived && <button type="button" className="button compact" onClick={() => setEdit({ mode: 'edit' })}><Plus aria-hidden="true" />New label</button>}</div></header>
    {archived && <p className="settings-info">Restore the workspace and team before changing labels.</p>}
    <div className="labels-toolbar"><input type="search" aria-label="Search labels" placeholder="Search labels…" value={query} onChange={event => setQuery(event.target.value)} /><select aria-label="Label state" value={state} onChange={event => setState(event.target.value)}><option value="active">Active labels</option><option value="archived">Archived labels</option></select></div>
    <ErrorBanner error={directory.error} retry={directory.refresh} />
    <div className="settings-list" aria-busy={directory.loading}>{directory.items.map(label => <div className="settings-row" key={label.id}><LabelDot color={label.color} /><div className="settings-row-main"><strong>{label.name}</strong><small>{scopeName}{label.archived_at ? ' · Archived' : ''}</small></div>{label.can_manage && !archived && <div className="row-actions">{label.archived_at ? <button type="button" className="button compact" onClick={() => setEdit({ label, mode: 'restore' })}><ArchiveRestore aria-hidden="true" />Restore</button> : <><button type="button" className="icon-button" aria-label={`Edit ${label.name}`} onClick={() => setEdit({ label, mode: 'edit' })}><Pencil aria-hidden="true" /></button><button type="button" className="icon-button" aria-label={`Archive ${label.name}`} onClick={() => setEdit({ label, mode: 'archive' })}><Archive aria-hidden="true" /></button></>}</div>}</div>)}</div>
    {directory.loading && <p className="muted" role="status">Loading labels…</p>}{!directory.loading && !directory.items.length && !directory.error && <p className="settings-empty">{query ? 'No labels match this search.' : `No ${state} labels.`}</p>}
    {directory.page?.has_more && <div className="load-more"><button type="button" className="button" disabled={directory.loading} onClick={directory.more}>Load more labels</button></div>}
    <p className="save-message" role="status">{message}</p>
    {edit && <LabelEditor workspaceId={workspaceId} teamId={teamId} session={session} label={edit.label} mode={edit.mode} canManage={!archived && (edit.label?.can_manage ?? canCreate)} close={() => setEdit(null)} saved={label => { setEdit(null); setMessage(`${label.name} ${edit.mode === 'archive' ? 'archived' : edit.mode === 'restore' ? 'restored' : 'saved'}.`); directory.refresh(); requestAnimationFrame(() => heading.current?.focus()); }} />}
  </section>;
}

function LabelEditor({ workspaceId, teamId, session, label, mode, canManage, close, saved }: { workspaceId: string; teamId?: string; session: Session; label?: Label; mode: 'edit' | 'archive' | 'restore'; canManage: boolean; close: () => void; saved: (label: Label) => void }) {
  const request = useScopedRequest(); const create = useCreateRequest<Label>(`/workspaces/${workspaceId}/labels`, session.csrf_token);
  const [base, setBase] = useState(label); const [name, setName] = useState(label?.name ?? ''); const [color, setColor] = useState<Label['color']>(label?.color ?? 'gray');
  const [busy, setBusy] = useState(false); const [error, setError] = useState<unknown>(null); const [latest, setLatest] = useState<Label | null>(null);
  const pending = busy || create.busy; const locked = pending || create.uncertain;
  const eligible = canManage && (base?.can_manage ?? true) && (!base || (mode === 'restore' ? !!base.archived_at : !base.archived_at));
  async function submit(event: FormEvent) {
    event.preventDefault(); if (pending || latest || !eligible) return;
    const fields = { name: name.trim(), color };
    if (!base) { const result = await create.submit({ ...fields, team_id: teamId ?? null }); if (result) saved(result); return; }
    setBusy(true); setError(null);
    try { saved(await request<Label>(`/workspaces/${workspaceId}/labels/${base.id}`, { method: 'PATCH', body: mode === 'archive' ? { archived: true } : { ...fields, ...(mode === 'restore' ? { archived: false } : {}) }, csrf: session.csrf_token, headers: { 'If-Match': labelETag(base) } })); }
    catch (error) { setError(error); if (error instanceof ApiError && error.status === 412) { const current = error.details?.current as Label | undefined; if (current?.id === base.id) setLatest(current); } }
    finally { setBusy(false); }
  }
  return <Modal title={mode === 'archive' ? 'Archive label' : mode === 'restore' ? 'Restore label' : base ? 'Edit label' : 'New label'} close={close} canClose={!locked}><form className="settings-modal-content" onSubmit={submit}>
    <ErrorBanner error={error || create.error} /><UncertainNotice show={create.uncertain} />
    {latest && <div className="workflow-conflict" role="alert"><strong>This label changed.</strong><p>The latest name is “{latest.name}”{latest.archived_at ? ' and it is archived' : ''}. Your proposed changes are preserved.</p><div className="button-row"><button type="button" className="button compact" onClick={() => { setName(latest.name); setColor(latest.color); setBase(latest); setLatest(null); setError(null); }}>Use latest</button><button type="button" className="button compact" onClick={() => { setBase(latest); setLatest(null); setError(null); }}>Keep my draft for review</button></div></div>}
    {!eligible && <p className="settings-info">This action is no longer available. Close this dialog and refresh labels to review the current state.</p>}
    {mode === 'archive' ? <p>Archive <strong>{base?.name}</strong>? It will stay on existing issues and remain available in filters. It cannot be newly applied until restored.</p> : <fieldset disabled={locked || !eligible}><label className="field">Label name<input data-autofocus value={name} required onChange={event => setName(event.target.value)} /></label><label className="field">Color<select value={color} onChange={event => setColor(event.target.value as Label['color'])}>{statusColors.map(color => <option key={color} value={color}>{color[0].toUpperCase() + color.slice(1)}</option>)}</select></label>{mode === 'restore' && <small className="muted">You can change the name if an active label already uses it.</small>}</fieldset>}
    <div className="modal-footer"><button type="button" className="button" data-autofocus={mode === 'archive' || undefined} disabled={locked} onClick={close}>Cancel</button><button className="button primary" disabled={pending || !!latest || !eligible || (mode !== 'archive' && !name.trim())}>{pending ? 'Saving…' : create.uncertain ? 'Retry safely' : mode === 'archive' ? 'Archive label' : mode === 'restore' ? 'Restore label' : base ? 'Save label' : 'Create label'}</button></div>
  </form></Modal>;
}
