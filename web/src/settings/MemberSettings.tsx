import { useState } from 'react';
import { UserPlus } from 'lucide-react';
import { Avatar, Loading } from '../components';
import { Link, settingsUrl } from '../router';
import type { Session, Workspace } from '../types';
import type { WorkspaceMember, WorkspaceRole } from './types';
import { useScopedRequest, DirectoryFooter, SettingsError, SettingsHeading, tag, useDirectory } from './shared';

export function MemberSettings({ workspace, session, reauthenticate, changed, workspaceArchived = false }: { workspace: Workspace; session: Session; workspaceArchived?: boolean; reauthenticate: () => void; changed: () => Promise<void> }) {
  const [state, setState] = useState('active');
  return <><SettingsHeading title="Members" description="Control workspace access. Promoting or reactivating a member ends their existing sign-ins.">{['owner', 'admin'].includes(workspace.role) && <Link className="button" href={settingsUrl(workspace.id, 'invitations')}><UserPlus aria-hidden="true" />Invite people</Link>}</SettingsHeading><label className="field settings-state-filter">Show<select value={state} onChange={e => setState(e.target.value)}><option value="active">Active members</option><option value="inactive">Inactive members</option><option value="all">Everyone</option></select></label><MemberDirectory workspaceArchived={workspaceArchived} key={state} workspace={workspace} session={session} state={state} reauthenticate={reauthenticate} changed={changed} /></>;
}
function MemberDirectory({ workspace, session, state, reauthenticate, changed, workspaceArchived }: { workspace: Workspace; session: Session; workspaceArchived?: boolean; state: string; reauthenticate: () => void; changed: () => Promise<void> }) {
  const directory = useDirectory<WorkspaceMember>(`/workspaces/${workspace.id}/members?state=${state}`);
  async function refresh() { await changed(); directory.reload(); }
  return <><SettingsError error={directory.error} retry={directory.reload} reauthenticate={reauthenticate} />{directory.loading && !directory.items.length ? <Loading label="Loading members…" /> : <div className="settings-list">{directory.items.map(member => <MemberRow workspaceArchived={workspaceArchived} key={`${member.user.id}:${member.version}`} member={member} workspace={workspace} session={session} reauthenticate={reauthenticate} changed={refresh} />)}{!directory.items.length && !directory.error && <p className="settings-empty">No {state === 'all' ? '' : state} members.</p>}</div>}<DirectoryFooter directory={directory} /></>;
}
function MemberRow({ member, workspace, session, reauthenticate, changed, workspaceArchived }: { member: WorkspaceMember; workspace: Workspace; session: Session; workspaceArchived?: boolean; reauthenticate: () => void; changed: () => Promise<void> }) {
  const request = useScopedRequest();
  const [role, setRole] = useState<WorkspaceRole>(member.role); const [busy, setBusy] = useState(false); const [error, setError] = useState<unknown>(null);
  const canEdit = !workspaceArchived && (workspace.role === 'owner' || workspace.role === 'admin' && member.role !== 'owner');
  const roles: WorkspaceRole[] = workspace.role === 'owner' ? ['owner', 'admin', 'member', 'viewer'] : ['admin', 'member', 'viewer'];
  async function update(patch: { role?: WorkspaceRole; active?: boolean }) {
    if (busy || !canEdit) return;
    if (patch.active === false && !window.confirm(`Deactivate ${member.user.display_name} in this workspace? Their access will stop. Their existing work will remain.`)) return;
    if (patch.role && member.role === 'owner' && patch.role !== 'owner' && !window.confirm(`Change ${member.user.display_name} from owner to ${patch.role}? At least one active owner must remain.`)) return;
    setBusy(true); setError(null);
    try { await request(`/workspaces/${workspace.id}/members/${member.user.id}`, { method: 'PATCH', body: patch, csrf: session.csrf_token, headers: { 'If-Match': tag('member', workspace.id, member.user.id, member.version) } }); await changed(); }
    catch (error) { setError(error); } finally { setBusy(false); }
  }
  return <div><div className={`settings-row${member.active ? '' : ' inactive'}`}><Avatar user={member.user} /><div className="settings-row-main"><strong>{member.user.display_name}{member.user.id === session.user.id ? ' (you)' : ''}</strong><small>{member.user.email}{!member.active && ' · Inactive'}</small></div>{canEdit ? <div className="row-actions"><label><span className="sr-only">Role for {member.user.display_name}</span><select value={role} onChange={e => setRole(e.target.value as WorkspaceRole)} disabled={busy}>{roles.map(role => <option key={role} value={role}>{role}</option>)}</select></label>{role !== member.role && <button className="button compact" disabled={busy} onClick={() => update({ role })}>Save role</button>}<button className="quiet" disabled={busy} onClick={() => update({ active: !member.active })}>{member.active ? 'Deactivate' : 'Reactivate'}</button></div> : <span className="row-role">{member.role}</span>}</div><SettingsError error={error} reload={() => void changed()} reauthenticate={reauthenticate} /></div>;
}
