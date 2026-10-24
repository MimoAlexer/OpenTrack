import { Archive, ArchiveRestore } from 'lucide-react';
import { useRef, useState, type FormEvent } from 'react';
import { Loading, Modal } from '../components';
import { useUnsavedChanges } from '../router';
import type { Session } from '../types';
import type { OrganizationSummary, WorkspaceDetail } from './types';
import { useScopedRequest, SettingsError, SettingsHeading, tag, UncertainNotice, useCreateRequest } from './shared';
import { useResource } from './useResource';

export function WorkspaceSettings({ workspaceId, session, reauthenticate, changed }: { workspaceId: string; session: Session; reauthenticate: () => void; changed: () => Promise<void> }) {
  const resource = useResource<WorkspaceDetail>(`/workspaces/${workspaceId}`);
  return <><SettingsHeading title="Workspace" description="A shared home for your teams and their work." /><SettingsError error={resource.error} retry={resource.reload} reauthenticate={reauthenticate} />{resource.loading ? <Loading /> : resource.data && <WorkspaceForm key={`${resource.data.id}:${resource.data.version}`} item={resource.data} session={session} reauthenticate={reauthenticate} reload={resource.reload} changed={changed} />}</>;
}
function WorkspaceForm({ item, session, reauthenticate, reload, changed }: { item: WorkspaceDetail; session: Session; reauthenticate: () => void; reload: () => void; changed: () => Promise<void> }) {
  const request = useScopedRequest();
  const [name, setName] = useState(item.name); const [busy, setBusy] = useState(false); const [error, setError] = useState<unknown>(null); const [saved, setSaved] = useState('');
  const canManage = ['owner', 'admin'].includes(item.role); const canEdit = canManage && !item.archived_at; const dirty = name !== item.name;
  useUnsavedChanges(dirty);
  async function update(body: { name?: string; archived?: boolean }) {
    if (busy) return; setBusy(true); setError(null);
    try { await request(`/workspaces/${item.id}`, { method: 'PATCH', body, csrf: session.csrf_token, headers: { 'If-Match': tag('workspace', item.id, item.version) } }); setSaved('Workspace updated.'); await changed(); reload(); }
    catch (error) { setError(error); } finally { setBusy(false); }
  }
  async function submit(event: FormEvent) { event.preventDefault(); if (dirty && canEdit) await update({ name: name.trim() }); }

  return <form className="settings-form" onSubmit={submit}><SettingsError error={error} reload={reload} reauthenticate={reauthenticate} />{!canEdit && <div className="settings-info">{item.archived_at ? 'This workspace is archived.' : 'Only workspace owners and administrators can change these settings.'}</div>}<label className="field">Workspace name<input value={name} onChange={e => setName(e.target.value)} maxLength={100} required disabled={!canEdit || busy} /><small>Renaming keeps the workspace identity and existing issue links intact.</small></label><div className="settings-meta"><dl><div><dt>Workspace ID</dt><dd>{item.id}</dd></div><div><dt>URL name</dt><dd>{item.slug}</dd></div><div><dt>Your role</dt><dd>{item.role}</dd></div></dl></div>{canEdit && <footer className="settings-form-footer"><span role="status">{saved}</span><button className="button primary" disabled={busy || !dirty || !name.trim()}>{busy ? 'Saving…' : 'Save changes'}</button></footer>}{canManage && <div className="settings-archive-section"><h3>{item.archived_at ? 'Restore workspace' : 'Archive workspace'}</h3><p>{item.archived_at ? 'Restore this workspace to allow changes to its teams and work again.' : 'Keep the workspace and its history while preventing new changes. You can restore it here later.'}</p><button type="button" className="button" disabled={busy || dirty} onClick={() => { if (item.archived_at || window.confirm(`Archive ${item.name}? Its teams and issues will become read-only until it is restored.`)) void update({ archived: !item.archived_at }); }}>{item.archived_at ? <ArchiveRestore aria-hidden="true" /> : <Archive aria-hidden="true" />}{item.archived_at ? 'Restore workspace' : 'Archive workspace'}</button></div>}</form>;
}
export function CreateWorkspace({ session, close, created, reauthenticate }: { session: Session; close: () => void; created: (workspace: WorkspaceDetail) => Promise<void>; reauthenticate: () => void }) {
  const organizations = useResource<OrganizationSummary[]>('/organizations');
  const [organization, setOrganization] = useState('');
  const command = useCreateRequest<WorkspaceDetail>('/workspaces', session.csrf_token);
  const committed = useRef<WorkspaceDetail | null>(null);
  const [opening, setOpening] = useState(false); const [handoffError, setHandoffError] = useState<unknown>(null);
  const busy = command.busy || opening;
  const allowed = organizations.data?.filter(item => ['owner', 'admin'].includes(item.role)) ?? [];
  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault(); if (busy) return;
    if (!committed.current) {
      const data = Object.fromEntries(new FormData(event.currentTarget));
      const result = await command.submit({ ...data, organization_id: organization || allowed[0]?.id });
      if (!result) return;
      committed.current = result;
    }
    setOpening(true); setHandoffError(null);
    try { await created(committed.current); } catch (error) { setHandoffError(error); } finally { setOpening(false); }
  }
  return <Modal title="Create workspace" close={close} canClose={!busy && !command.uncertain}><form className="settings-modal-content" onSubmit={submit}><p>Create a separate workspace within an organization you manage. Teams, members, and issues stay scoped to that workspace.</p><SettingsError error={handoffError || organizations.error || command.error} retry={organizations.error ? organizations.reload : undefined} reauthenticate={reauthenticate} /><UncertainNotice show={command.uncertain} />{committed.current && <p className="settings-info" role="status">{committed.current.name} was created.</p>}{organizations.loading ? <Loading label="Checking organization access…" /> : !allowed.length ? <div className="settings-info">You do not have organization permission to create a workspace. A workspace owner role alone does not grant this permission.</div> : <><fieldset disabled={busy || command.uncertain || !!committed.current}><label className="field">Organization<select value={organization || allowed[0].id} onChange={e => setOrganization(e.target.value)}>{allowed.map(item => <option key={item.id} value={item.id}>{item.name}</option>)}</select></label><label className="field">Workspace name<input data-autofocus name="name" required maxLength={100} /></label><div className="form-grid"><label className="field">First team<input name="team_name" required maxLength={100} /></label><label className="field">Issue prefix<input name="team_key" required pattern="[A-Za-z][A-Za-z0-9]{1,9}" maxLength={10} placeholder="ENG" className="uppercase" /></label></div></fieldset><footer className="modal-footer"><button type="button" className="button" onClick={close} disabled={busy || command.uncertain}>{committed.current ? 'Close' : 'Cancel'}</button><button className="button primary" disabled={busy}>{opening ? 'Opening…' : command.busy ? 'Creating…' : committed.current ? 'Retry opening workspace' : command.uncertain ? 'Retry safely' : 'Create workspace'}</button></footer></>}</form></Modal>;
}
