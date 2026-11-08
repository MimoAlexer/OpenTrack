import { useEffect, useRef, useState } from 'react';
import { ApiError, request } from '../api';
import { ErrorBanner, Loading } from '../components';
import { Link } from '../router';
import type { Session, Team } from '../types';
import type { components } from '../generated/api';
import { queryBody, queryFragment, validateView, type StructuredView } from '../filters/model';
import './discovery.css';

type Saved = components['schemas']['SavedDiscovery'];
type Definition = components['schemas']['DiscoveryDefinition'];
type SearchPage = components['schemas']['SearchPage'];
type Directory = { data: Saved[]; has_more: boolean; next_cursor?: string | null };
type Kind = 'view' | 'search';
const resource = (kind: Kind) => kind === 'search' ? 'saved-searches' : 'views';
const columns: Definition['columns'] = ['key', 'title', 'status', 'priority', 'assignee'];
function definition(view: StructuredView): Definition { return { query: queryBody(view), layout: view.layout, group_by: view.sort === 'priority' ? 'priority' : 'status', columns }; }
function asView(saved: Definition): StructuredView {
  const { cursor: _cursor, ...query } = saved.query;
  return { ...query, version: 1, filter: query.filter ?? null, archived: query.archived ?? false, limit: query.limit ?? 50, timezone: query.timezone ?? 'UTC', sort: query.sort ?? 'created', team_id: query.team_id ?? undefined, q: query.q ?? undefined, layout: saved.layout };
}
interface Props { workspaceId: string; scope: string; session: Session; view: StructuredView; teams: Team[]; apply: (view: StructuredView) => boolean }
export function Discovery(props: Props) {
  // Keying by authority closes all drafts, names, results and pending requests.
  return <ScopedDiscovery key={props.scope} {...props}/>;
}
function signature(value:unknown):string {
  return JSON.stringify(value,(_key,item)=>item && typeof item==='object' && !Array.isArray(item) ? Object.fromEntries(Object.keys(item).sort().map(key=>[key,item[key]])) : item);
}
function normalized(value:Definition):Definition { return {...value,query:queryBody(asView(value))}; }
interface Lease { signal:AbortSignal; current:()=>boolean }
interface Repair { id:string; etag:string; definition:Definition; acknowledged:boolean }
function ScopedDiscovery({workspaceId,scope,session,view,teams,apply}:Props) {
  const base=`/workspaces/${encodeURIComponent(workspaceId)}`;
  const [activated,setActivated]=useState(false);
  const [views,setViews]=useState<Directory|null>(null);
  const [searches,setSearches]=useState<Directory|null>(null);
  const [selected,setSelected]=useState<Saved|null>(null);
  const [name,setName]=useState('');
  const [audience,setAudience]=useState('personal');
  const [error,setError]=useState<unknown>(null);
  const [busy,setBusy]=useState(false);
  const [opening,setOpening]=useState(false);
  const [selectionBlocked,setSelectionBlocked]=useState(false);
  const [reconciliation,setReconciliation]=useState<Saved|null>(null);
  const [repair,setRepair]=useState<Repair|null>(null);
  const [awaitingView,setAwaitingView]=useState<string|null>(null);
  const [reload,setReload]=useState(0);
  const [term,setTerm]=useState('');
  const [kind,setKind]=useState<'issue'|'project'|'all'>('issue');
  const [search,setSearch]=useState<SearchPage|null>(null);
  const [searchError,setSearchError]=useState<unknown>(null);
  const [searching,setSearching]=useState(false);
  const [searchRetry,setSearchRetry]=useState(0);
  const mounted=useRef(true);
  const generation=useRef(0);
  const authorityController=useRef(new AbortController());
  const selectionSequence=useRef(0);
  const selectionRead=useRef<AbortController|null>(null);
  const searchRead=useRef<AbortController|null>(null);
  const activeSaved=useRef<{id:string;fingerprint:string}|null>(null);
  const pending=useRef<{path:string;options:Parameters<typeof request>[1];lease:Lease;selection:number;target?:string}|null>(null);
  const authority=`${scope}:${workspaceId}`;
  const snapshot=signature(definition(view));
  function lease(local?:AbortController):Lease {
    const ticket=generation.current;
    const signal=local?AbortSignal.any([authorityController.current.signal,local.signal]):authorityController.current.signal;
    return {signal,current:()=>mounted.current && ticket===generation.current && !signal.aborted};
  }
  useEffect(()=>{
    mounted.current=true;
    if(authorityController.current.signal.aborted)authorityController.current=new AbortController();
    return()=>{mounted.current=false;generation.current++;authorityController.current.abort();selectionRead.current?.abort();searchRead.current?.abort();pending.current=null;};
  },[]);
  useEffect(()=>{if(awaitingView===snapshot)setAwaitingView(null);},[awaitingView,snapshot]);
  function denial(reason:unknown,operation:Lease):boolean {
    if(!operation.current() || !(reason instanceof ApiError) || ![401,403,404].includes(reason.status))return false;
    // Invalidate every captured page, write/replay and read before clearing UI.
    generation.current++;authorityController.current.abort();authorityController.current=new AbortController();
    selectionSequence.current++;selectionRead.current?.abort();searchRead.current?.abort();pending.current=null;activeSaved.current=null;
    setViews(null);setSearches(null);setSelected(null);setSearch(null);setName('');setTerm('');setAudience('personal');
    setBusy(false);setOpening(false);setSearching(false);setSearchError(null);setSelectionBlocked(false);setReconciliation(null);setRepair(null);setAwaitingView(null);setError(reason);
    return true;
  }
  function effectiveDefinition(saveKind:Kind,inherited?:Definition|null):Definition {
    const value=definition(view);
    if(inherited){value.columns=inherited.columns;value.group_by=inherited.group_by;}
    if(saveKind==='search')value.query={...value.query,q:term,sort:'created',archived:false};
    return value;
  }
  const unsaved=selected && activeSaved.current?.id===selected.id && activeSaved.current.fingerprint!==signature(effectiveDefinition(selected.kind,selected.definition));
  const needsRepair=!!selected && (!selected.definition || !!selected.repair_required || selectionBlocked);
  const awaitingAppliedView=!!awaitingView && awaitingView!==snapshot;
  const updateBlocked=needsRepair || !!reconciliation || awaitingAppliedView || opening;
  useEffect(()=>{
    if(!activated)return;
    const abort=new AbortController(),operation=lease(abort);setError(null);
    Promise.all([request<Directory>(`${base}/views`,{signal:operation.signal}),request<Directory>(`${base}/saved-searches`,{signal:operation.signal})])
      .then(([v,s])=>{if(operation.current()){setViews(v);setSearches(s);}})
      .catch(reason=>{if(operation.current() && !denial(reason,operation))setError(reason);});
    return()=>abort.abort();
  },[authority,base,reload,activated]);
  function replace(item:Saved) {
    const update=(page:Directory|null):Directory=>({data:page?.data.some(d=>d.id===item.id)?page.data.map(d=>d.id===item.id?item:d):[...(page?.data??[]),item],has_more:page?.has_more??false,next_cursor:page?.next_cursor});
    (item.kind==='view'?setViews:setSearches)(update);
  }
  async function write(path:string,method:string,body?:unknown,etag?:string) {
    if(busy)return;
    const operation=lease();
    const options={method,body,csrf:session.csrf_token,signal:operation.signal,headers:{'Idempotency-Key':crypto.randomUUID(),...(etag?{'If-Match':etag}:{})}};
    pending.current={path,options,lease:operation,selection:selectionSequence.current,target:selected?.id};await dispatchWrite();
  }
  async function dispatchWrite() {
    const operation=pending.current;if(!operation?.lease.current())return;
    setBusy(true);setError(null);
    try {
      const item=await request<Saved|undefined>(operation.path,operation.options);
      if(!operation.lease.current())return;
      const sameSelection=operation.selection===selectionSequence.current;
      if(item){
        replace(item);
        const savedBody=operation.options?.body as {definition?:Definition}|undefined;
        if(sameSelection && savedBody?.definition){setSelected(item);setSelectionBlocked(!item.definition||!!item.repair_required);activeSaved.current=item.definition?{id:item.id,fingerprint:signature(normalized(item.definition))}:null;setRepair(null);setReconciliation(null);}
        else if(sameSelection && operation.target===item.id)setSelected(item);
      } else {
        const id=operation.target;setViews(p=>p&&{...p,data:p.data.filter(d=>d.id!==id)});setSearches(p=>p&&{...p,data:p.data.filter(d=>d.id!==id)});
        if(sameSelection){setSelected(null);activeSaved.current=null;setRepair(null);setReconciliation(null);}
      }
      pending.current=null;
    }catch(reason){
      if(operation.lease.current() && !denial(reason,operation.lease)){setError(reason);if(reason instanceof ApiError&&reason.status>0)pending.current=null;}
    }finally{if(operation.lease.current())setBusy(false);}
  }
  function save(saveKind:Kind,updating=false,reviewed?:Definition) {
    if(updating && (!selected || !selected.can_edit || (!reviewed && updateBlocked)))return;
    const current=reviewed??effectiveDefinition(saveKind,updating?selected?.definition:null);
    const visibility=audience==='personal'||audience==='workspace'?audience:'team';
    const body={name,kind:saveKind,visibility:updating?selected!.visibility:saveKind==='search'?'personal':visibility,team_id:updating?selected!.team_id:saveKind==='view'&&visibility==='team'?audience:null,definition:current};
    void write(`${base}/${resource(saveKind)}${updating?`/${selected!.id}`:''}`,updating?'PATCH':'POST',body,updating?selected!.etag:undefined);
  }
  function accept(item:Saved,keepDraftName=false):boolean {
    setRepair(null);setReconciliation(null);setSelected(item);if(!keepDraftName)setName(item.name);
    if(!item.definition || item.repair_required){setSelectionBlocked(true);activeSaved.current=null;setAwaitingView(null);setError(new Error(item.repair_required??'Repair this saved definition before running it.'));return false;}
    const next=asView(item.definition),invalid=validateView(next);
    if(invalid || !apply(next)){setSelectionBlocked(true);activeSaved.current=null;setAwaitingView(null);setError(new Error(`These saved filters could not be opened. ${invalid??'Finish the current draft or review the filter settings before trying again.'}`));return false;}
    activeSaved.current={id:item.id,fingerprint:signature(normalized(item.definition))};setAwaitingView(signature(definition(next)));setSelectionBlocked(false);setError(null);
    if(item.kind==='search'){setTerm(next.q??'');setKind('issue');}
    return true;
  }
  async function open(item:Saved) {
    selectionRead.current?.abort();const abort=new AbortController();selectionRead.current=abort;
    const operation=lease(abort),selection=++selectionSequence.current;setOpening(true);setRepair(null);setReconciliation(null);setError(null);
    try{const current=await request<Saved>(`${base}/${resource(item.kind)}/${item.id}`,{signal:operation.signal});if(operation.current()&&selection===selectionSequence.current){replace(current);accept(current);}}
    catch(reason){if(operation.current()&&selection===selectionSequence.current&&!denial(reason,operation))setError(reason);}
    finally{if(operation.current()&&selection===selectionSequence.current)setOpening(false);}
  }
  async function refreshSelected() {
    if(!selected)return;
    selectionRead.current?.abort();const abort=new AbortController();selectionRead.current=abort;
    const operation=lease(abort),selection=++selectionSequence.current;setOpening(true);setRepair(null);setError(null);
    try{const item=await request<Saved>(`${base}/${resource(selected.kind)}/${selected.id}`,{signal:operation.signal});if(operation.current()&&selection===selectionSequence.current){replace(item);setReconciliation(item);}}
    catch(reason){if(operation.current()&&selection===selectionSequence.current&&!denial(reason,operation))setError(reason);}
    finally{if(operation.current()&&selection===selectionSequence.current)setOpening(false);}
  }
  function reviewRepair(){if(selected?.can_edit)setRepair({id:selected.id,etag:selected.etag,definition:effectiveDefinition(selected.kind),acknowledged:false});}
  function confirmRepair(){if(selected && repair?.acknowledged && repair.id===selected.id && repair.etag===selected.etag)save(selected.kind,true,repair.definition);}
  async function more(saveKind:Kind) {
    const page=saveKind==='view'?views:searches;if(!page?.next_cursor)return;
    const operation=lease();
    try{const next=await request<Directory>(`${base}/${resource(saveKind)}?cursor=${encodeURIComponent(page.next_cursor)}`,{signal:operation.signal});if(operation.current())(saveKind==='view'?setViews:setSearches)({...next,data:[...page.data,...next.data]});}
    catch(reason){if(operation.current()&&!denial(reason,operation))setError(reason);}
  }
  const searchBody=()=>({query:{...queryBody(view),q:term,sort:'created',archived:false,...(kind!=='issue'?{filter:null,team_id:null}:{})},types:kind==='all'?['issue','project']:[kind]});
  useEffect(()=>{
    if(!activated)return;
    searchRead.current?.abort();const abort=new AbortController();searchRead.current=abort;const operation=lease(abort);
    if(!term.trim()){setSearch(null);setSearchError(null);setSearching(false);return()=>abort.abort();}
    setSearching(true);setSearchError(null);
    const timer=setTimeout(()=>{if(!operation.current())return;void request<SearchPage>(`${base}/search/query`,{method:'POST',csrf:session.csrf_token,body:searchBody(),signal:operation.signal})
      .then(page=>{if(operation.current())setSearch(page);})
      .catch(reason=>{if(operation.current()&&!denial(reason,operation)){setSearch(null);setSearchError(reason);}})
      .finally(()=>{if(operation.current())setSearching(false);});},150);
    return()=>{clearTimeout(timer);abort.abort();};
  },[authority,base,snapshot,term,kind,session.csrf_token,searchRetry,activated]);
  async function moreSearch() {
    if(!search?.next_cursor||searching||!searchRead.current)return;
    const operation=lease(searchRead.current);setSearching(true);
    try{const next=await request<SearchPage>(`${base}/search/query`,{method:'POST',csrf:session.csrf_token,body:{...searchBody(),query:{...searchBody().query,cursor:search.next_cursor}},signal:operation.signal});if(operation.current())setSearch({...next,data:[...search.data,...next.data]});}
    catch(reason){if(operation.current()&&!denial(reason,operation)){setSearch(null);setSearchError(reason);}}
    finally{if(operation.current())setSearching(false);}
  }
  const directory = (page: Directory | null, label: string, saveKind: Kind) => <section aria-label={label}><h3>{label}</h3>{page ? page.data.length ? <ul>{[...page.data].sort((a, b) => Number(b.favorite) - Number(a.favorite)).map(item => <li key={item.id}><button type="button" className="quiet" onClick={() => void open(item)}>{item.name}</button><span>{item.visibility}{item.repair_required ? ' · Repair required' : ''}</span><button type="button" className="quiet" disabled={busy} aria-label={`${item.favorite ? 'Unfavorite' : 'Favorite'} ${item.name}`} onClick={() => void write(`${base}/${resource(item.kind)}/${item.id}/favorite`, 'PUT', { favorite: !item.favorite })}>{item.favorite ? '★' : '☆'}</button></li>)}</ul> : <p>No {label.toLowerCase()} yet.</p> : !error && <Loading label={`Loading ${label.toLowerCase()}…`}/>} {page?.has_more && <button type="button" className="button compact" onClick={() => void more(saveKind)}>Load more {label.toLowerCase()}</button>}</section>;
  return <details className="discovery" onToggle={event=>{if(event.currentTarget.open)setActivated(true);}}><summary>Saved views and full-text search</summary>{activated && <div className="discovery-content">
    <div className="discovery-save"><label className="field">Definition name<input value={name} disabled={opening} onChange={e => setName(e.target.value)}/></label><label className="field">View audience<select value={audience} disabled={opening} onChange={e => setAudience(e.target.value)}><option value="personal">Only me</option><option value="workspace">Workspace</option>{teams.filter(t => !t.archived_at).map(t => <option key={t.id} value={t.id}>Team · {t.name}</option>)}</select></label><button type="button" className="button compact" disabled={busy || opening || !name.trim()} onClick={() => save('view')}>Save new view</button></div>
    {selected && <div className="notice"><span>{selected.name} · Version {selected.version}{unsaved ? ' · Unsaved overrides' : ''}{!selected.can_edit ? ' · Read only' : ''}</span>{selected.can_edit && <><button type="button" className="button compact" disabled={busy || updateBlocked || !name.trim() || selected.kind === 'search' && !term.trim()} onClick={() => save(selected.kind, true)}>Save changes</button><button type="button" className="quiet" disabled={busy || opening} onClick={() => void write(`${base}/${resource(selected.kind)}/${selected.id}`, 'DELETE', undefined, selected.etag)}>Delete saved definition</button></>}<button type="button" className="quiet" disabled={opening} onClick={() => void refreshSelected()}>Refresh saved version</button></div>}
    {opening && <p role="status">Checking the current saved definition…</p>}
    {needsRepair && selected?.can_edit && <div className="notice"><p>Ordinary Save changes is unavailable for these saved filters. Review an explicit replacement before changing the definition.</p><button type="button" className="button compact" disabled={busy || opening || !!reconciliation} onClick={reviewRepair}>Review replacement filters</button></div>}
    {repair && <section className="notice" aria-label="Review replacement filters"><h3>Review replacement filters</h3><p>This will replace the unavailable saved definition with the locally visible filters captured below. Renaming alone does not repair or preserve the unavailable predicate.</p><pre>{JSON.stringify(repair.definition,null,2)}</pre><label><input type="checkbox" checked={repair.acknowledged} onChange={event=>setRepair({...repair,acknowledged:event.target.checked})}/> I reviewed these replacement filters and intend to replace the saved definition.</label><button type="button" className="button compact" disabled={busy || !repair.acknowledged} onClick={confirmRepair}>Replace saved definition with reviewed filters</button><button type="button" className="quiet" onClick={()=>setRepair(null)}>Cancel replacement</button></section>}
    {reconciliation && <section className="notice" aria-label="Review latest saved filters"><h3>Review latest saved filters</h3><p>The current remote definition is shown below. Its version is not a write precondition until you deliberately use its filters. Your draft name is retained.</p>{reconciliation.definition?<pre>{JSON.stringify(reconciliation.definition,null,2)}</pre>:<p>{reconciliation.repair_required??'This version requires explicit repair.'}</p>}<button type="button" className="button compact" disabled={busy} onClick={()=>accept(reconciliation,true)}>{reconciliation.definition?'Use latest filters and keep draft name':'Use latest saved version and keep draft name'}</button><button type="button" className="quiet" onClick={()=>setReconciliation(null)}>Keep reviewing current draft</button></section>}
    <ErrorBanner error={error} retry={() => pending.current ? void dispatchWrite() : setReload(v => v + 1)}/>{busy && <p role="status">Saving definition…</p>}
    <div className="discovery-directories">{directory(views, 'Saved views', 'view')}{directory(searches, 'Saved searches', 'search')}</div>
    <section aria-label="Full-text search"><h3>Search issues and projects</h3><div className="discovery-search"><label className="field">Search text<input aria-label="Search issues and projects" type="search" disabled={opening} value={term} onChange={e => setTerm(e.target.value)}/></label><label className="field">Result type<select value={kind} disabled={opening} onChange={e => setKind(e.target.value as typeof kind)}><option value="issue">Issues with current filters</option><option value="project">Projects</option><option value="all">Issues and projects</option></select></label><button type="button" className="button compact" disabled={busy || opening || !name.trim() || !term.trim() || kind !== 'issue'} onClick={() => save('search')}>Save search</button></div>
      <p>Exact keys first, then weighted title and body matches. Saved searches are private.</p>{searching && <p role="status">Searching…</p>}<ErrorBanner error={searchError} retry={() => setSearchRetry(v => v + 1)}/>
      {search && <div className={searching ? 'discovery-results pending' : 'discovery-results'} aria-busy={searching}>{search.notice && <p className="notice">{search.notice}</p>}{!search.data.length && <p>No matching work. Try another word or a current issue key.</p>}<ul>{search.data.map(hit => <li key={`${hit.kind}:${hit.id}`}><Link href={hit.kind === 'issue' ? `/w/${workspaceId}/issues/${hit.id}${queryFragment(location.hash)}` : `/w/${workspaceId}/projects/${hit.id}?team_id=${hit.team_id}`}>{hit.key ? `${hit.key} · ` : ''}{hit.title}</Link><p>{hit.snippet}</p></li>)}</ul>{search.has_more && <button type="button" className="button compact" disabled={searching} onClick={() => void moreSearch()}>More search results</button>}</div>}
    </section>
  </div>}</details>;
}
