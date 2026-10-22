import { useSessionScope } from '../desktop/context';
import { useState } from 'react';
import { Circle } from 'lucide-react';
import { Picker } from '../taxonomy/Picker';
import { useSearchPage } from '../taxonomy/useSearchPage';
import type { Session } from '../types';
import type { IssueGraphTarget, Page } from './types';

export function GraphMark({ target }: { target: IssueGraphTarget }) { return <Circle className={`status-color-${target.status.color}`} aria-label={target.status.name} />; }
export function IssueTargetPicker({ workspaceId, teamId, excludeId, session, selected, change, disabled = false, label = 'Issue' }: { workspaceId: string; teamId: string; excludeId: string; session: Session; selected: IssueGraphTarget | null; change: (target: IssueGraphTarget) => void; disabled?: boolean; label?: string }) {
  const sessionScope = useSessionScope(session);
  const [open, setOpen] = useState(false); const [query, setQuery] = useState('');
  const directory = useSearchPage<IssueGraphTarget, Page<IssueGraphTarget>>(`/workspaces/${workspaceId}/issues/lookup?team_id=${teamId}&exclude_issue_id=${excludeId}`, sessionScope, query, open && !disabled);
  return <Picker label={label} value={selected ? <><span className="issue-key">{selected.key}</span>{selected.title}</> : 'Search by title or key'} open={open} setOpen={setOpen} query={query} setQuery={setQuery} choices={directory.items.map(target => ({ id: target.id, text: `${target.key} · ${target.title}`, detail: target.status.name, selected: selected?.id === target.id, icon: <GraphMark target={target} /> }))} select={id => { const target = directory.items.find(item => item.id === id); if (target) change(target); }} loading={directory.loading} error={directory.error} refresh={directory.refresh} more={directory.more} hasMore={!!directory.page?.has_more} disabled={disabled} />;
}
