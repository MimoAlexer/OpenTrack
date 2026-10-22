import { useSessionScope, useSessionBoundary } from '../desktop/context';
import { useState } from 'react';
import type { Session } from '../types';
import { Avatar } from '../components';
import { Picker, type Choice } from './Picker';
import { useSearchPage } from './useSearchPage';
import type { AssignableUser, AssigneePage, IssueAssignee } from './types';

interface Props { workspaceId: string; teamId: string; session: Session; value: string | null; current?: IssueAssignee | null; disabled?: boolean; change: (id: string | null) => void; remembered?: (person: AssignableUser) => void }
export function AssigneePicker(props: Props) { const boundary = useSessionBoundary(props.session); return <ScopedAssigneePicker key={`${props.workspaceId}:${props.teamId}:${boundary}`} {...props} />; }
function ScopedAssigneePicker({ workspaceId, teamId, session, value, current, disabled, change, remembered }: Props) {
  const sessionScope = useSessionScope(session);
  const [open, setOpen] = useState(false); const [query, setQuery] = useState(''); const [selected, setSelected] = useState<AssignableUser | null>(null);
  const directory = useSearchPage<AssignableUser, AssigneePage>(`/workspaces/${workspaceId}/teams/${teamId}/assignees`, sessionScope, query, open && !!teamId && !disabled);
  const person = value ? current?.id === value ? current : selected?.id === value ? selected : directory.items.find(item => item.id === value) : null;
  const choices: Choice[] = [{ id: '', text: 'Unassigned', selected: value === null }];
  if (directory.page?.self_assignable) choices.push({ id: session.user.id, text: 'Assign to me', detail: session.user.display_name, selected: value === session.user.id, icon: <Avatar small user={session.user} /> });
  for (const item of directory.items) if (!choices.some(choice => choice.id === item.id)) choices.push({ id: item.id, text: item.display_name, selected: value === item.id, icon: <Avatar small user={item} /> });
  if (value && person && !choices.some(choice => choice.id === value)) choices.push({ id: value, text: person.display_name, detail: current?.id === value && !current.assignable ? 'Current assignee · no longer assignable' : 'Current selection', disabled: true, selected: true, icon: <Avatar small user={person} /> });
  return <Picker label="Assignee" value={<><Avatar small user={value ? person ?? { display_name: 'Assignee unavailable' } : undefined} />{value ? person?.display_name ?? 'Assignee unavailable' : 'Unassigned'}</>} open={open} setOpen={setOpen} query={query} setQuery={setQuery} choices={choices} select={id => { const next = id === session.user.id ? session.user : directory.items.find(item => item.id === id); if (next) { setSelected(next); remembered?.(next); } change(id || null); }} loading={directory.loading} error={directory.error} refresh={directory.refresh} more={directory.more} hasMore={!!directory.page?.has_more} disabled={disabled}>
    {value && current?.id === value && !current.assignable && <small className="reference-note">This person is no longer assignable. Their existing assignment is retained until you change it.</small>}
  </Picker>;
}
