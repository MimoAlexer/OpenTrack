import { useState } from 'react';
import { Avatar } from '../components';
import { useSessionScope } from '../desktop/context';
import { Picker, type Choice } from '../taxonomy/Picker';
import { useSearchPage } from '../taxonomy/useSearchPage';
import type { AssignableUser, AssigneePage, IssueAssignee } from '../taxonomy/types';
import type { Session } from '../types';

export function ProjectLead({ workspaceId, teamId, session, value, current, disabled, change }: { workspaceId: string; teamId: string; session: Session; value: string | null; current: IssueAssignee | null; disabled: boolean; change: (value: string | null) => void }) {
  const scope = useSessionScope(session); const [open, setOpen] = useState(false); const [query, setQuery] = useState(''); const [selected, setSelected] = useState<AssignableUser | null>(null);
  const directory = useSearchPage<AssignableUser, AssigneePage>(`/workspaces/${workspaceId}/teams/${teamId}/assignees`, scope, query, open && !disabled);
  const person = current?.id === value ? current : selected?.id === value ? selected : directory.items.find(item => item.id === value);
  const choices: Choice[] = [{ id: '', text: 'No lead', selected: !value }];
  if (directory.page?.self_assignable) choices.push({ id: session.user.id, text: session.user.display_name, detail: 'You', selected: value === session.user.id });
  for (const item of directory.items) if (!choices.some(choice => choice.id === item.id)) choices.push({ id: item.id, text: item.display_name, selected: value === item.id });
  if (value && person && !choices.some(choice => choice.id === value)) choices.push({ id: value, text: person.display_name, detail: 'Current lead', selected: true, disabled: true });
  return <Picker label="Project lead" value={<><Avatar small user={value ? person ?? { display_name: 'Lead unavailable' } : undefined}/>{value ? person?.display_name ?? 'Lead unavailable' : 'No lead'}</>} open={open} setOpen={setOpen} query={query} setQuery={setQuery} choices={choices} select={id => { const next = id === session.user.id ? session.user : directory.items.find(item => item.id === id); if (next) setSelected(next); change(id || null); }} loading={directory.loading} error={directory.error} refresh={directory.refresh} more={directory.more} hasMore={!!directory.page?.has_more} disabled={disabled}/>;
}
