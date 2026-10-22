import { useSessionScope, useSessionBoundary } from '../desktop/context';
import { useEffect, useState } from 'react';
import { Tag, X } from 'lucide-react';
import { request } from '../api';
import { ErrorBanner } from '../components';
import type { Session, Team } from '../types';
import { Picker, type Choice } from './Picker';
import { useSearchPage } from './useSearchPage';
import type { Label, LabelPage, LabelSummary } from './types';

export function LabelDot({ color }: { color: LabelSummary['color'] }) { return <span className={`label-dot status-color-${color}`} aria-hidden="true" />; }
interface Props { workspaceId: string; teamId?: string; session: Session; value: string[]; summaries?: LabelSummary[]; teams: Pick<Team, 'id' | 'name'>[]; disabled?: boolean; filter?: boolean; change: (ids: string[]) => void; remembered?: (labels: LabelSummary[]) => void }
export function LabelPicker(props: Props) { const boundary = useSessionBoundary(props.session); return <ScopedLabelPicker key={`${props.workspaceId}:${props.teamId ?? 'visible'}:${boundary}`} {...props} />; }
function ScopedLabelPicker({ workspaceId, teamId, session, value, summaries = [], teams, disabled, filter = false, change, remembered }: Props) {
  const sessionScope = useSessionScope(session);
  const [open, setOpen] = useState(false); const [query, setQuery] = useState(''); const [cache, setCache] = useState<LabelSummary[]>([]); const [lookupError, setLookupError] = useState<unknown>(null); const [lookupRetry, setLookupRetry] = useState(0);
  const path = `/workspaces/${workspaceId}/labels?scope=${teamId ? 'applicable' : 'visible'}${teamId ? `&team_id=${encodeURIComponent(teamId)}` : ''}&state=${filter ? 'all' : 'active'}`;
  const directory = useSearchPage<Label, LabelPage>(path, sessionScope, query, open && !disabled);
  const known = new Map([...cache, ...summaries, ...directory.items].map(label => [label.id, label]));
  const missingKey = value.slice(0, 50).filter(id => !known.has(id)).sort().join(',');
  useEffect(() => {
    const abort = new AbortController(); setLookupError(null);
    async function resolve() {
      const ids = missingKey.split(',').filter(Boolean);
      for (let index = 0; index < ids.length && !abort.signal.aborted; index += 4) {
        await Promise.all(ids.slice(index, index + 4).map(async id => {
          try { const item = await request<Label>(`/workspaces/${workspaceId}/labels/${encodeURIComponent(id)}`, { signal: abort.signal }); if (!abort.signal.aborted) setCache(old => [...old.filter(label => label.id !== id), item]); }
          catch (error) { if (!abort.signal.aborted) setLookupError(error); }
        }));
      }
    }
    void resolve(); return () => abort.abort();
  }, [workspaceId, missingKey, lookupRetry, sessionScope]);
  function scope(label: LabelSummary) { return label.team_id ? teams.find(team => team.id === label.team_id)?.name ?? 'Team label' : 'Shared'; }
  function toggle(id: string) {
    const next = value.includes(id) ? value.filter(value => value !== id) : [...value, id].sort();
    if (next.length > 50) return;
    const label = known.get(id); if (label) setCache(old => [...old.filter(item => item.id !== id), label]);
    const selected = next.map(id => known.get(id)).filter((label): label is LabelSummary => !!label); remembered?.(selected); change(next);
  }
  const choices: Choice[] = directory.items.map(label => ({ id: label.id, text: label.name, detail: `${scope(label)}${label.archived_at ? ' · Archived' : ''}`, selected: value.includes(label.id), disabled: !value.includes(label.id) && value.length >= 50, icon: <LabelDot color={label.color} /> }));
  const removedArchived = !filter ? summaries.filter(label => label.archived_at && !value.includes(label.id)) : [];
  return <Picker label={filter ? 'Filter labels' : 'Labels'} value={<><Tag aria-hidden="true" />{value.length ? `${value.length} selected` : filter ? 'Any label' : 'Add labels'}</>} open={open} setOpen={setOpen} query={query} setQuery={setQuery} choices={choices} select={toggle} multiple loading={directory.loading} error={directory.error} refresh={directory.refresh} more={directory.more} hasMore={!!directory.page?.has_more} disabled={disabled}>
    {!!value.length && <div className="label-chips">{value.slice(0, 50).map(id => { const label = known.get(id); return <span className="label-chip" key={id}>{label && <LabelDot color={label.color} />}<span>{label?.name ?? 'Label unavailable'}<small>{label ? scope(label) : ''}{label?.archived_at ? ' · Archived' : ''}</small></span>{!disabled && <button type="button" aria-label={`Remove ${label?.name ?? 'unavailable label'}`} onClick={() => toggle(id)}><X aria-hidden="true" /></button>}</span>; })}</div>}
    {removedArchived.map(label => <small className="reference-note" key={label.id}>{label.name} removed. <button type="button" className="quiet" disabled={disabled || value.length >= 50} onClick={() => toggle(label.id)}>Undo removal</button></small>)}
    {value.length >= 50 && <small className="reference-note" role="status">{value.length > 50 ? 'This selection exceeds the 50-label limit. Clear the label filter and choose fewer labels.' : 'Maximum 50 labels selected.'}</small>}
    <ErrorBanner error={lookupError} retry={() => setLookupRetry(value => value + 1)} />
  </Picker>;
}
