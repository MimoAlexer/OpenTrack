import { useState } from 'react';
import { Building2, Mail, Tag, Users, UsersRound } from 'lucide-react';
import { Link, settingsUrl } from '../router';
import type { Session, Workspace } from '../types';
import { WorkspaceSettings } from './WorkspaceSettings';
import { TeamSettings } from './TeamSettings';
import { MemberSettings } from './MemberSettings';
import { InvitationSettings } from './InvitationSettings';
import { Reauthenticate } from './shared';
import './settings.css';
import { LabelSettings } from '../taxonomy/LabelSettings';
import type { Workflow } from '../workflows/types';

export function SettingsScreen({ workspace, session, section = 'workspace', changed, workflowChanged, authenticated, workspaceArchived = false }: { workspace: Workspace; session: Session; section?: string; workflowChanged?: (workflow: Workflow) => void; workspaceArchived?: boolean; changed: () => Promise<void>; authenticated: (session: Session) => void }) {
  const [reauth, setReauth] = useState(false);
  const tabs = [['workspace', 'Workspace', Building2], ['teams', 'Teams', UsersRound], ['labels', 'Labels', Tag], ['members', 'Members', Users], ['invitations', 'Invitations', Mail]] as const;
  const selected = tabs.some(([key]) => key === section) ? section : 'workspace';
  const props = { workspace, session, changed, workflowChanged, workspaceArchived, reauthenticate: () => setReauth(true) };
  return <section className="settings-page" aria-label="Workspace settings"><nav className="settings-tabs" aria-label="Settings sections">{tabs.map(([key, name, Icon]) => <Link key={key} href={settingsUrl(workspace.id, key)} aria-current={selected === key ? 'page' : undefined}><Icon aria-hidden="true" />{name}</Link>)}</nav><div className="settings-content" key={selected}>{workspaceArchived && selected !== 'workspace' && <div className="settings-info">This workspace is archived. <Link href={settingsUrl(workspace.id)}>Restore it in Workspace settings</Link> before making changes.</div>}{selected === 'workspace' ? <WorkspaceSettings workspaceId={workspace.id} session={session} changed={changed} reauthenticate={props.reauthenticate} /> : selected === 'teams' ? <TeamSettings {...props} /> : selected === 'labels' ? <LabelSettings workspaceId={workspace.id} scopeName={workspace.name} session={session} canCreate={['owner', 'admin'].includes(workspace.role)} archived={workspaceArchived} /> : selected === 'members' ? <MemberSettings {...props} /> : <InvitationSettings {...props} />}</div>{reauth && <Reauthenticate session={session} close={() => setReauth(false)} authenticated={authenticated} />}</section>;
}
