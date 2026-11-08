import { useEffect, useRef, useState } from 'react';
import { ListFilter, Plus, X } from 'lucide-react';
import { ApiError, request } from '../api';
import { useSessionScope } from '../desktop/context';
import { Picker, type Choice } from '../taxonomy/Picker';
import { ErrorBanner } from '../components';
import { priorities, type Bootstrap, type Session } from '../types';
import { defaultPredicate, fieldLabels, fields, isDateField, operatorLabels, operators, predicateCount, validateView, type FilterField, type FilterNode, type FilterValue, type Predicate, type StructuredView } from './model';
import './filters.css';

interface Props { workspaceId: string; data: Bootstrap; session: Session; initial: StructuredView; active: boolean; repair?: boolean; apply: (view: StructuredView) => boolean; clear: () => void }
interface Option { id: string; name?: string; display_name?: string; archived_at?: string | null }
interface OptionPage { data: Option[]; has_more: boolean; next_cursor: string | null }

export function FilterBuilder({ workspaceId, data, session, initial, active, repair = false, apply, clear }: Props) {
  const [open, setOpen] = useState(repair); const [draft, setDraft] = useState(initial);
  const error = validateView(draft); const count = predicateCount(draft.filter);
  const update = (filter: FilterNode | null) => setDraft(value => ({ ...value, filter }));
  function add() { update(!draft.filter ? defaultPredicate() : { kind: 'all', children: [draft.filter, defaultPredicate()] }); }
  return <details className="structured-filters" open={open} onToggle={event => setOpen(event.currentTarget.open)}>
    <summary className="button compact"><ListFilter aria-hidden="true"/>{repair ? 'Repair filters' : 'Advanced filters'}{active && <span className="count-badge">{count}</span>}</summary>
    {open && <form className="structured-filter-panel" aria-label="Advanced issue filters" onSubmit={event => { event.preventDefault(); if (!error && apply(draft)) setOpen(false); }}>
      <header><h3>{repair ? 'Build replacement filters' : 'Match the work you need'}</h3><p>{repair ? 'The current settings stay blocked until you apply a valid replacement or clear them.' : 'Combine conditions without changing issues. Each group matches all or any of its conditions.'}</p></header>
      <div className="structured-filter-scope"><label className="field">Query team<select value={draft.team_id ?? ''} onChange={event => setDraft(value => { const next = { ...value }; if (event.target.value) next.team_id = event.target.value; else delete next.team_id; return next; })}><option value="">All accessible teams</option>{data.teams.map(team => <option key={team.id} value={team.id}>{team.name}{team.archived_at ? ' · Archived' : ''}</option>)}</select></label>
        <label className="field">Show<select value={draft.archived ? 'archived' : 'active'} onChange={event => setDraft(value => ({ ...value, archived: event.target.value === 'archived' }))}><option value="active">Active issues</option><option value="archived">Archived issues</option></select></label>
        <label className="field">Query timezone<input value={draft.timezone} list="query-timezones" onChange={event => setDraft(value => ({ ...value, timezone: event.target.value }))} placeholder="UTC or Europe/Zurich" spellCheck={false}/><datalist id="query-timezones">{['UTC', 'Europe/Zurich', 'Europe/London', 'America/New_York', 'America/Los_Angeles', 'Asia/Tokyo', 'Australia/Sydney'].map(zone => <option key={zone} value={zone}/>)}</datalist></label></div>
      <p className="filter-help">Local dates and relative days use this query timezone and the server time recorded for the first page.</p>
      {draft.filter ? <NodeEditor node={draft.filter} path="1" depth={1} total={count} workspaceId={workspaceId} data={data} session={session} team={draft.team_id} change={update} remove={() => update(null)}/> : <p>No conditions. The selected team, archive and text search still apply.</p>}
      {(!draft.filter || draft.filter.kind === 'predicate') && <button type="button" className="button compact" disabled={count >= 50} onClick={add}><Plus aria-hidden="true"/>Add condition</button>}
      <div className="structured-filter-display"><label className="field">Layout<select value={draft.layout} onChange={event => setDraft(value => ({ ...value, layout: event.target.value as StructuredView['layout'], limit: event.target.value === 'board' ? Math.min(value.limit, 50) : value.limit }))}><option value="list">List</option><option value="board">Board</option></select></label><label className="field">Order<select value={draft.sort} onChange={event => setDraft(value => ({ ...value, sort: event.target.value as StructuredView['sort'] }))}><option value="created">Newest first</option><option value="priority">Priority, unset last</option><option value="manual" disabled={!draft.team_id || draft.archived || !!data.teams.find(team => team.id === draft.team_id)?.archived_at}>Manual order within status</option></select></label></div>
      {error && <p className="filter-validation" role="alert">{error}</p>}
      <footer><span>{count} / 50 conditions</span><button type="button" className="quiet" onClick={() => update(null)}>Remove all conditions</button>{active && <button type="button" className="quiet" onClick={clear}>Clear advanced settings</button>}<button type="submit" className="button primary compact" disabled={!!error}>Apply filters</button></footer>
    </form>}
  </details>;
}

interface NodeProps { node: FilterNode; path: string; depth: number; total: number; workspaceId: string; data: Bootstrap; session: Session; team?: string; change: (node: FilterNode | null) => void; remove: () => void }
function NodeEditor(props: NodeProps) {
  const { node, path, depth, total, change, remove } = props;
  if (node.kind === 'predicate') return <PredicateEditor {...props} node={node} change={value => change(value)}/>;
  const group = node;
  function child(index: number, value: FilterNode | null) {
    const children = value ? group.children.map((old, at) => at === index ? value : old) : group.children.filter((_, at) => at !== index);
    change(children.length ? { ...group, children } : null);
  }
  return <fieldset className="filter-group"><legend>Filter group {path}</legend><div className="filter-group-heading"><label>Match<select aria-label={`Group ${path} match`} value={node.kind} onChange={event => change({ ...node, kind: event.target.value as 'all' | 'any' })}><option value="all">All conditions</option><option value="any">Any condition</option></select></label><button type="button" className="icon-button" aria-label={`Remove group ${path}`} onClick={remove}><X aria-hidden="true"/></button></div>
    {node.children.map((value, index) => <NodeEditor {...props} key={`${index}:${value.kind}`} node={value} path={`${path}.${index + 1}`} depth={depth + 1} change={next => child(index, next)} remove={() => child(index, null)}/>)}
    <div className="filter-group-actions"><button type="button" className="quiet" disabled={total >= 50 || depth >= 4} onClick={() => change({ ...node, children: [...node.children, defaultPredicate()] })}>Add condition</button><button type="button" className="quiet" disabled={total >= 50 || depth >= 3} onClick={() => change({ ...node, children: [...node.children, { kind: 'any', children: [defaultPredicate()] }] })}>Add nested group</button></div>
  </fieldset>;
}

function fieldDefault(field: FilterField): FilterValue { return field === 'priority' ? 3 : isDateField(field) ? { relative_days: 0 } : ''; }
function PredicateEditor({ node, path, data, session, workspaceId, team, change, remove }: Omit<NodeProps, 'node' | 'change'> & { node: Predicate; change: (value: Predicate) => void }) {
  const multiple = node.op === 'in' || node.op === 'not_in';
  const values = multiple ? node.values ?? [] : node.value === undefined ? [] : [node.value];
  function setValues(values: FilterValue[]) { change(multiple ? { kind: 'predicate', field: node.field, op: node.op, values } : { kind: 'predicate', field: node.field, op: node.op, value: values[0] ?? fieldDefault(node.field) }); }
  function setOperator(op: Predicate['op']) {
    const valid = node.op === 'is_null' ? fieldDefault(node.field) : values[0] ?? fieldDefault(node.field);
    change(op === 'is_null' ? { kind: 'predicate', field: node.field, op, value: true } : op === 'in' || op === 'not_in' ? { kind: 'predicate', field: node.field, op, values: node.op === 'is_null' ? [valid] : values.length ? values : [valid] } : { kind: 'predicate', field: node.field, op, value: valid });
  }
  return <fieldset className="filter-condition"><legend>Condition {path}</legend><div className="filter-condition-controls"><label className="field">Field<select aria-label={`Condition ${path} field`} value={node.field} onChange={event => { const field = event.target.value as FilterField; change({ kind: 'predicate', field, op: 'eq', value: fieldDefault(field) }); }}>{fields.map(field => <option key={field} value={field}>{fieldLabels[field]}</option>)}</select></label><label className="field">Operator<select aria-label={`Condition ${path} operator`} value={node.op} onChange={event => setOperator(event.target.value as Predicate['op'])}>{operators(node.field).map(op => <option value={op} key={op}>{operatorLabels[op]}</option>)}</select></label><button type="button" className="icon-button" aria-label={`Remove condition ${path}`} onClick={remove}><X aria-hidden="true"/></button></div>
    {node.op === 'is_null' ? <label className="field">Empty value<select aria-label={`Condition ${path} empty value`} value={node.value === false ? 'false' : 'true'} onChange={event => setValues([event.target.value === 'true'])}><option value="true">Must be empty</option><option value="false">Must have a value</option></select></label> : isDateField(node.field) ? <DateValues field={node.field} path={path} values={values} multiple={multiple} change={setValues}/> : <ValuePicker key={node.field} field={node.field} path={path} values={values} multiple={multiple} data={data} session={session} workspaceId={workspaceId} team={team} change={setValues}/>}
    {node.op === 'not_in' && ['assignee', 'project', 'cycle', 'due_date'].includes(node.field) && <p className="filter-help">Empty values are excluded. Add an “Is empty” condition in an Any group to include them.</p>}
    {node.op === 'not_in' && node.field === 'label' && <p className="filter-help">Matches issues with none of these labels, including issues without labels.</p>}
    {isDateField(node.field) && <p className="filter-help">A local date matches the whole day. Before excludes that day; After starts at the next local day.</p>}
  </fieldset>;
}

function DateValues({ field, path, values, multiple, change }: { field: FilterField; path: string; values: FilterValue[]; multiple: boolean; change: (values: FilterValue[]) => void }) {
  function set(index: number, value: FilterValue) { change(values.map((old, at) => at === index ? value : old)); }
  return <div className="filter-date-values">{values.map((value, index) => <div className="filter-date-value" key={index}><DateValue field={field} label={`Condition ${path} date ${index + 1}`} value={value} change={value => set(index, value)}/>{multiple && <button type="button" className="icon-button" aria-label={`Remove date ${index + 1} from condition ${path}`} onClick={() => change(values.filter((_, at) => at !== index))}><X aria-hidden="true"/></button>}</div>)}{multiple && <button type="button" className="quiet" disabled={values.length >= 100} onClick={() => change([...values, { relative_days: 0 }])}>Add date value</button>}</div>;
}

function DateValue({ field, label, value, change }: { field: FilterField; label: string; value: FilterValue; change: (value: FilterValue) => void }) {
  const meaning = () => typeof value === 'object' ? 'relative' : typeof value === 'string' && /[Tt]/.test(value) ? 'instant' : 'date';
  const [mode, setMode] = useState(meaning);
  useEffect(() => { if (value !== '') setMode(meaning()); }, [value]);
  const relative = typeof value === 'object' ? value.relative_days : 0;
  return <><label className="field">Date meaning<select aria-label={`${label} meaning`} value={mode} onChange={event => { setMode(event.target.value); change(event.target.value === 'relative' ? { relative_days: 0 } : ''); }}><option value="relative">Relative day</option><option value="date">Local date</option>{field !== 'due_date' && <option value="instant">Exact instant</option>}</select></label>{mode === 'relative' ? <label className="field">Days from today<input aria-label={`${label} days`} type="number" min={-36500} max={36500} value={Number.isFinite(relative) ? relative : ''} onChange={event => change({ relative_days: event.target.value === '' ? Number.NaN : Number(event.target.value) })}/><small>0 is today in the query timezone.</small></label> : <label className="field">{mode === 'instant' ? 'Instant with UTC offset' : 'Local date'}<input aria-label={`${label} value`} type={mode === 'instant' ? 'text' : 'date'} value={typeof value === 'string' ? value : ''} placeholder={mode === 'instant' ? '2026-03-29T12:00:00Z' : undefined} onChange={event => change(event.target.value)}/>{mode === 'instant' && <small>Include Z for UTC or an offset such as +02:00. Precision is preserved.</small>}</label>}</>;
}

function ValuePicker({ field, path, values, multiple, data, session, workspaceId, team, change }: { field: FilterField; path: string; values: FilterValue[]; multiple: boolean; data: Bootstrap; session: Session; workspaceId: string; team?: string; change: (values: FilterValue[]) => void }) {
  const [open, setOpen] = useState(false); const [query, setQuery] = useState(''); const [browseTeam, setBrowseTeam] = useState(team ?? data.teams[0]?.id ?? ''); const [archived, setArchived] = useState(false);
  const [remembered, setRemembered] = useState<Choice[]>([]); const scope = useSessionScope(session);
  const remote = !['priority', 'team'].includes(field);
  const remotePath = field === 'label' ? `/workspaces/${workspaceId}/labels?scope=visible&state=all` : !browseTeam ? '' : `/workspaces/${workspaceId}/teams/${browseTeam}/${field === 'status' ? 'statuses?state=all' : field === 'assignee' ? 'assignees' : field === 'project' ? `projects?archived=${archived}` : 'cycles'}`;
  const searchOnServer = !['status', 'cycle'].includes(field);
  const directory = useOptions(remotePath, scope, searchOnServer ? query : '', open && remote && !!remotePath);
  const seeds: Choice[] = field === 'priority' ? priorities.map((text, index) => ({ id: String(index), text })) : field === 'team' ? data.teams.map(team => ({ id: team.id, text: team.name, detail: team.archived_at ? 'Archived' : undefined })) : field === 'status' ? data.statuses.filter(status => status.team_id === browseTeam).map(status => ({ id: status.id, text: status.name })) : field === 'assignee' ? data.members.map(person => ({ id: person.id, text: person.display_name })) : [];
  const selected = values.filter(value => typeof value === 'string' || typeof value === 'number').map(String);
  const known = new Map([...seeds, ...remembered, ...directory.items.map(item => ({ id: item.id, text: item.name ?? item.display_name ?? 'Unnamed value', detail: item.archived_at ? 'Archived' : undefined }))].map(item => [item.id, item]));
  const available = remote && directory.loaded ? directory.items.map(item => known.get(item.id)!) : seeds;
  const filtered = available.filter(item => searchOnServer && remote || item.text.toLocaleLowerCase().includes(query.toLocaleLowerCase()));
  const choices: Choice[] = filtered.map(item => ({ ...item, selected: selected.includes(item.id), disabled: multiple && !selected.includes(item.id) && selected.length >= 100 }));
  function choose(id: string) {
    const option = known.get(id); if (option) setRemembered(old => [...old.filter(item => item.id !== id), option]);
    const next = multiple ? selected.includes(id) ? selected.filter(value => value !== id) : [...selected, id] : [id];
    if (next.length > 100) return;
    change(field === 'priority' ? next.map(Number) : next);
  }
  return <div className="filter-value-picker">{remote && field !== 'label' && <div className="filter-value-scope"><label className="field">Browse team<select aria-label={`Condition ${path} browse team`} value={browseTeam} onChange={event => { setBrowseTeam(event.target.value); setQuery(''); }}><option value="">Choose a team</option>{data.teams.map(team => <option key={team.id} value={team.id}>{team.name}</option>)}</select></label>{field === 'project' && <label className="field">Project state<select value={archived ? 'archived' : 'active'} onChange={event => setArchived(event.target.value === 'archived')}><option value="active">Active projects</option><option value="archived">Archived projects</option></select></label>}</div>}
    <Picker label={`Condition ${path} values`} value={selected.length ? multiple ? `${selected.length} selected` : known.get(selected[0])?.text ?? 'Selection unavailable' : 'Choose a value'} open={open} setOpen={setOpen} query={query} setQuery={setQuery} choices={choices} select={choose} multiple={multiple} loading={directory.loading} error={directory.error} refresh={directory.retry} more={directory.more} hasMore={directory.hasMore} disabled={remote && field !== 'label' && !browseTeam}>
      {multiple && !!selected.length && <div className="filter-selected-values">{selected.map(id => <span key={id}>{known.get(id)?.text ?? 'Selection unavailable'}<button type="button" className="icon-button" aria-label={`Remove ${known.get(id)?.text ?? 'unavailable value'} from condition ${path}`} onClick={() => choose(id)}><X aria-hidden="true"/></button></span>)}</div>}
      {!searchOnServer && directory.hasMore && <p className="filter-help">Search filters the loaded values. Load more to inspect the next page.</p>}
      {remote && !!directory.error && <ErrorBanner error={directory.error} retry={directory.retry}/>}
    </Picker>
  </div>;
}

function useOptions(path: string, scope: string, query: string, enabled: boolean) {
  const key = `${scope}:${path}:${query}`;
  const [state, setState] = useState<{ key: string; items: Option[]; next: string | null; hasMore: boolean; error: unknown; loading: boolean; loaded: boolean }>({ key: '', items: [], next: null, hasMore: false, error: null, loading: false, loaded: false });
  const [revision, setRevision] = useState(0); const active = useRef<{ key: string; abort: AbortController; pending: boolean } | null>(null);
  useEffect(() => {
    const context = { key, abort: new AbortController(), pending: false }; active.current = context;
    setState({ key, items: [], next: null, hasMore: false, error: null, loading: enabled, loaded: false });
    const timer = enabled ? setTimeout(() => { void load(context); }, query ? 150 : 0) : undefined;
    async function load(current: typeof context) {
      current.pending = true;
      try {
        const page = await request<OptionPage>(`${path}${path.includes('?') ? '&' : '?'}limit=20${query ? `&q=${encodeURIComponent(query)}` : ''}`, { signal: current.abort.signal });
        if (!current.abort.signal.aborted && active.current === current) setState({ key, items: page.data, next: page.next_cursor, hasMore: page.has_more, error: null, loading: false, loaded: true });
      } catch (error) { if (!current.abort.signal.aborted && active.current === current) setState({ key, items: [], next: null, hasMore: false, error, loading: false, loaded: true }); }
      finally { current.pending = false; }
    }
    return () => { clearTimeout(timer); context.abort.abort(); if (active.current === context) active.current = null; };
  }, [key, enabled, revision]);
  async function more() {
    const current = active.current; if (!current || current.key !== key || current.pending || !state.next) return;
    current.pending = true; setState(old => ({ ...old, loading: true, error: null }));
    try {
      const page = await request<OptionPage>(`${path}${path.includes('?') ? '&' : '?'}limit=20${query ? `&q=${encodeURIComponent(query)}` : ''}&cursor=${encodeURIComponent(state.next)}`, { signal: current.abort.signal });
      if (!current.abort.signal.aborted && active.current === current) setState(old => ({ key, items: [...old.items, ...page.data.filter(item => !old.items.some(value => value.id === item.id))], next: page.next_cursor, hasMore: page.has_more, error: null, loading: false, loaded: true }));
    } catch (error) { if (!current.abort.signal.aborted && active.current === current) setState(old => ({ ...old, ...(error instanceof ApiError && [401, 403, 404].includes(error.status) ? { items: [], next: null, hasMore: false } : {}), error, loading: false })); }
    finally { current.pending = false; }
  }
  const current = state.key === key;
  return { items: current ? state.items : [], error: current ? state.error : null, loading: current && state.loading, loaded: current && state.loaded, hasMore: current && state.hasMore, more, retry: () => setRevision(value => value + 1) };
}
