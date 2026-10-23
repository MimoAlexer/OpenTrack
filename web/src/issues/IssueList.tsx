import { useSessionScope } from '../desktop/context';
import { useEffect, useLayoutEffect, useRef, useState } from 'react';
import { Archive, ArrowDownWideNarrow, ListFilter, Plus, Search, X } from 'lucide-react';
import { ApiError, request } from '../api';
import { LabelDot, LabelPicker } from '../taxonomy/LabelPicker';
import { Avatar, ErrorBanner, Loading, StatusMark } from '../components';
import { formatDate } from '../issue-model';
import { restoreIssueRowFocus } from './focus';
import { IssueBoard } from './IssueBoard';
import { issuesUrl, Link, navigate } from '../router';
import { categoryOrder, priorities, type Bootstrap, type Issue, type IssuePage, type Session, type Status } from '../types';
import type { Workflow } from '../workflows/types';

export function IssueList({ workspaceId, data, session, params, selectedId, refresh, create, searchRef, headingRef, updated, workflowChanged, workspaceArchived = false, charactersEnabled = true }: {
  charactersEnabled?: boolean; workspaceArchived?: boolean; workspaceId: string; data: Bootstrap; session: Session; params: URLSearchParams; selectedId?: string;
  refresh: number; create: () => void; searchRef: React.RefObject<HTMLInputElement | null>; headingRef?: React.RefObject<HTMLHeadingElement | null>;
  updated?: (issue: Issue) => void; workflowChanged?: (workflow: Workflow) => void;
}) {
  const sessionScope = useSessionScope(session);
  const historyScope = `${workspaceId}:${sessionScope}:${data.auth_epoch}`;
  const [history, setHistory] = useState<{ scope: string; statuses: Status[] }>({ scope: historyScope, statuses: [] });
  const [result, setResult] = useState<{ scope: string; authority: string; page: IssuePage } | null>(null);
  const [loading, setLoading] = useState<{ scope: string; busy: boolean; error: unknown }>({ scope: '', busy: true, error: null });
  const [moreBusy, setMoreBusy] = useState(false);
  const [retry, setRetry] = useState(0);
  const [boardPages, setBoardPages] = useState<{ scope: string; cursors: (string | null)[] }>({ scope: '', cursors: [null] });
  const [boardBlocking, setBoardBlocking] = useState(false);
  const [query, setQuery] = useState(params.get('q') ?? '');
  const sequence = useRef(0);
  const activeRead = useRef<AbortController | null>(null);
  const deniedBoard = useRef<{ scope: string; error: unknown; retrying: boolean } | null>(null);
  const listRef = useRef<HTMLElement>(null);
  const pendingFocus = useRef<{ scope: string; authority: string; page: IssuePage; load: number; signal: AbortSignal; row: HTMLAnchorElement; rowIds: string[] } | null>(null);
  const composing = useRef(false);
  const teamId = params.get('team_id') ?? '';
  const accessScope = JSON.stringify([historyScope, teamId]);
  const layout = params.get('layout') ?? 'list';
  const board = layout === 'board';
  const displayValid = ['list', 'board'].includes(layout) && (!params.has('display_version') || params.get('display_version') === '1');
  const boardTeam = data.teams.find(team => team.id === teamId);
  const mine = params.get('view') === 'mine';
  const archived = params.get('archived') === 'true';
  const labelIds = [...new Set((params.get('label_ids') ?? '').split(',').filter(Boolean))].sort();
  const priorityOrder = params.get('sort') === 'priority';
  const filters = new URLSearchParams();
  for (const key of ['team_id', 'status_id', 'priority', 'q', 'archived', 'label_ids', 'label_mode', 'sort']) if (params.has(key)) filters.set(key, params.get(key)!);
  if (mine) filters.set('assignee_id', session.user.id);
  filters.set('limit', '50');
  const filterKey = filters.toString();
  const listingScope = JSON.stringify([historyScope, filterKey, layout, params.get('display_version')]);
  const boardCursors = boardPages.scope === listingScope ? boardPages.cursors : [null];
  const boardCursor = board ? boardCursors.at(-1) : null;
  const queryFilters = new URLSearchParams(filterKey);
  if (boardCursor) queryFilters.set('cursor', boardCursor);
  const queryKey = queryFilters.toString();
  const resultScope = JSON.stringify([listingScope, boardCursor]);
  const currentFocusScope = useRef({ scope: resultScope, authority: historyScope });
  currentFocusScope.current = { scope: resultScope, authority: historyScope };
  const hasSnapshot = result?.scope === resultScope;
  const pendingQuery = !hasSnapshot && result?.authority === historyScope && (loading.scope !== resultScope || loading.busy);
  const hasDisplay = hasSnapshot || pendingQuery;
  const items = hasDisplay && result && (!board || hasSnapshot) ? result.page.data : [];
  const nextCursor = hasSnapshot ? result.page.next_cursor : null;
  const hasMore = hasSnapshot && result.page.has_more;
  const busy = loading.scope !== resultScope || loading.busy;
  const error = loading.scope === resultScope ? loading.error : null;
  const historicalStatuses = history.scope === historyScope ? history.statuses : [];
  const filterCount = ['status_id', 'priority'].filter(key => params.has(key)).length + Number(labelIds.length > 0);
  function setFilter(key: string, value: string) {
    const next = new URLSearchParams(params);
    if (value) next.set(key, value); else next.delete(key);
    if (key === 'team_id') next.delete('status_id');
    if (key === 'archived' && !value && data.teams.find(team => team.id === next.get('team_id'))?.archived_at) { next.delete('team_id'); next.delete('status_id'); }
    navigate(issuesUrl(workspaceId, next));
  }
  function clearFilters() { const next = new URLSearchParams(params); ['q', 'status_id', 'priority', 'label_ids', 'label_mode'].forEach(key => next.delete(key)); navigate(issuesUrl(workspaceId, next)); }
  function setLayout(value: string) { const next = new URLSearchParams(params); next.set('layout', value); next.set('display_version', '1'); navigate(issuesUrl(workspaceId, next)); }
  function retryPage() {
    if (deniedBoard.current?.scope === accessScope) deniedBoard.current = { ...deniedBoard.current, retrying: true };
    setRetry(value => value + 1);
  }
  function denyBoard(denial: unknown) {
    deniedBoard.current = { scope: accessScope, error: denial, retrying: false }; sequence.current += 1;
    activeRead.current?.abort(); activeRead.current = null; pendingFocus.current = null;
    setResult(null); setMoreBusy(false); setLoading({ scope: resultScope, busy: false, error: denial });
  }
  useEffect(() => { setBoardPages({ scope: listingScope, cursors: [null] }); }, [listingScope]);
  useEffect(() => { setQuery(params.get('q') ?? ''); }, [params.get('q')]);
  useEffect(() => {
    if (selectedId || query === (params.get('q') ?? '')) return;
    const timer = setTimeout(() => setFilter('q', query.trim()), 280);
    return () => clearTimeout(timer);
  }, [query, params.toString(), selectedId]);
  useEffect(() => {
    const abort = new AbortController(); const load = ++sequence.current;
    activeRead.current = abort;
    const cleanup = () => { abort.abort(); if (activeRead.current === abort) activeRead.current = null; };
    const denial = board && deniedBoard.current?.scope === accessScope ? deniedBoard.current : null;
    if (!displayValid || board && !boardTeam || denial && !denial.retrying) { setResult(null); setLoading({ scope: resultScope, busy: false, error: denial?.error ?? null }); return cleanup; }
    setLoading({ scope: resultScope, busy: true, error: null }); setMoreBusy(false);
    async function loadPage() {
      const page = await request<IssuePage>(`/workspaces/${workspaceId}/issues?${queryKey}`, { signal: abort.signal });
      if (denial && !abort.signal.aborted) {
        const workflow = await request<Workflow>(`/workspaces/${workspaceId}/teams/${teamId}/workflow`, { signal: abort.signal });
        if (workflow.team_id !== teamId) throw new Error('Board access could not be confirmed.');
      }
      return page;
    }
    loadPage().then(page => {
      if (abort.signal.aborted || load !== sequence.current || currentFocusScope.current.scope !== resultScope || currentFocusScope.current.authority !== historyScope) return;
      if (denial && deniedBoard.current === denial) deniedBoard.current = null;
      const focused = document.activeElement;
      const group = focused instanceof HTMLAnchorElement && focused.classList.contains('issue-row') && listRef.current?.contains(focused) ? focused.closest('.issue-groups') : null;
      const rowIds = group ? [...group.querySelectorAll<HTMLAnchorElement>('a.issue-row')].map(row => row.id) : [];
      pendingFocus.current = group && focused instanceof HTMLAnchorElement ? { scope: resultScope, authority: historyScope, page, load, signal: abort.signal, row: focused, rowIds } : null;
      setResult({ scope: resultScope, authority: historyScope, page }); setLoading({ scope: resultScope, busy: false, error: null });
    }).catch(error => {
      if (abort.signal.aborted || load !== sequence.current || currentFocusScope.current.scope !== resultScope || currentFocusScope.current.authority !== historyScope) return;
      // Retain a previously authorized snapshot only for a transient failure in
      // this exact scope. Permission failures must immediately remove its rows.
      if (error instanceof ApiError && [401, 403, 404].includes(error.status)) {
        if (board) { denyBoard(error); return; }
        setResult(null);
      }
      if (denial && deniedBoard.current === denial) deniedBoard.current = { ...denial, error, retrying: false };
      setLoading({ scope: resultScope, busy: false, error });
    });
    return cleanup;
  }, [resultScope, refresh, retry, displayValid, boardTeam?.id]);
  useLayoutEffect(() => {
    const pending = pendingFocus.current;
    if (!pending) return;
    const current = () => !pending.signal.aborted && pending.load === sequence.current && currentFocusScope.current.scope === pending.scope && currentFocusScope.current.authority === pending.authority;
    if (!current()) { pendingFocus.current = null; return; }
    if (result?.page !== pending.page) return;
    pendingFocus.current = null;
    if (!pending.row.isConnected) restoreIssueRowFocus(pending.row.id.slice(4), pending.rowIds, headingRef?.current, current);
  }, [result, resultScope, historyScope, headingRef]);
  async function loadMore() {
    if (board || busy || moreBusy || !nextCursor) return;
    const load = sequence.current; const next = new URLSearchParams(filterKey); next.set('cursor', nextCursor);
    setMoreBusy(true); setLoading({ scope: resultScope, busy: false, error: null });
    try {
      const page = await request<IssuePage>(`/workspaces/${workspaceId}/issues?${next}`);
      if (load !== sequence.current) return;
      setResult(previous => previous?.scope === resultScope ? { scope: resultScope, authority: historyScope, page: { ...page, data: [...previous.page.data, ...page.data.filter(issue => !previous.page.data.some(old => old.id === issue.id))] } } : previous);
    } catch (error) { if (load === sequence.current) { if (error instanceof ApiError && [401, 403, 404].includes(error.status)) setResult(null); setLoading({ scope: resultScope, busy: false, error }); } }
    finally { if (load === sequence.current) setMoreBusy(false); }
  }
  const statusMap = new Map([...historicalStatuses, ...data.statuses].map(status => [status.id, status]));
  const missing = [...new Map(items.filter(issue => !statusMap.has(issue.status_id)).map(issue => [issue.status_id, { id: issue.status_id, team: issue.team_id }])).values()];
  const missingKey = JSON.stringify(missing);
  useEffect(() => {
    const abort = new AbortController();
    const ids = JSON.parse(missingKey) as { id: string; team: string }[];
    async function resolve() {
      for (let index = 0; index < ids.length && !abort.signal.aborted; index += 4) {
        const values = await Promise.all(ids.slice(index, index + 4).map(async item => {
          try { return await request<Status>(`/workspaces/${workspaceId}/teams/${item.team}/statuses/${item.id}`, { signal: abort.signal }); } catch { return null; }
        }));
        if (!abort.signal.aborted) setHistory(previous => { const old = previous.scope === historyScope ? previous.statuses : []; return { scope: historyScope, statuses: [...old, ...values.filter((value): value is Status => !!value && !old.some(status => status.id === value.id))] }; });
      }
    }
    void resolve(); return () => abort.abort();
  }, [historyScope, missingKey]);
  const statuses = [...statusMap.values()].filter(s => pendingQuery || !teamId || s.team_id === teamId).sort((a, b) => categoryOrder.indexOf(a.category) - categoryOrder.indexOf(b.category) || a.position - b.position);
  const groups = priorityOrder ? [{ id: 'priority-order', name: 'Priority order', teamId: '', status: undefined }] : [...statuses.map(status => ({ id: status.id, name: status.name, teamId: status.team_id, status })), ...missing.map(item => ({ id: item.id, name: 'Status unavailable', teamId: item.team, status: undefined }))];
  const readOnly = session.workspaces.find(w => w.id === workspaceId)?.role === 'viewer' || workspaceArchived;
  const filtered = !!query || !!filterCount;
  function moveRow(event: React.KeyboardEvent<HTMLDivElement>) {
    if (event.defaultPrevented || composing.current || event.nativeEvent.isComposing || event.keyCode === 229 || event.altKey || event.ctrlKey || event.metaKey || event.shiftKey || event.getModifierState('AltGraph') || !['ArrowUp', 'ArrowDown', 'Home', 'End'].includes(event.key)) return;
    const row = event.target;
    if (!(row instanceof HTMLAnchorElement) || row !== document.activeElement || !row.classList.contains('issue-row')) return;
    const rows = [...event.currentTarget.querySelectorAll<HTMLAnchorElement>('a.issue-row')];
    const index = rows.indexOf(row);
    if (index < 0) return;
    const next = event.key === 'Home' ? 0 : event.key === 'End' ? rows.length - 1 : Math.max(0, Math.min(rows.length - 1, index + (event.key === 'ArrowDown' ? 1 : -1)));
    event.preventDefault(); rows[next]?.focus();
  }
  if (!displayValid) return <section className="issue-list empty-state"><h2>These display settings are unavailable.</h2><p>Choose a supported layout to keep using the current filters.</p><button type="button" className="button" onClick={() => setLayout('list')}>Use list layout</button></section>;
  if (board && deniedBoard.current?.scope === accessScope) return <section className="issue-list empty-state" aria-busy={busy}><h2>Board unavailable.</h2>{busy ? <Loading label="Checking board access…" /> : <ErrorBanner error={deniedBoard.current.error} retry={retryPage} />}<button type="button" className="button" onClick={() => { const next = new URLSearchParams(params); next.delete('team_id'); next.delete('status_id'); next.set('layout', 'list'); navigate(issuesUrl(workspaceId, next)); }}>Open all issues</button></section>;
  return <section ref={listRef} className={`issue-list${board ? ' board-list' : ''}`} aria-label="Issue list" aria-busy={busy}>
    <div className="issue-display-controls"><label className="list-sort">Display<select aria-label="Issue layout" value={layout} onChange={event => setLayout(event.target.value)}><option value="list">List</option><option value="board">Board</option></select></label>{board && <label className="list-sort">Board team<select aria-label="Board team" value={teamId} onChange={event => setFilter('team_id', event.target.value)}><option value="">Choose a team</option>{data.teams.filter(team => archived || !team.archived_at || team.id === teamId).map(team => <option key={team.id} value={team.id}>{team.name}{team.archived_at ? ' · Archived' : ''}</option>)}</select></label>}</div>
    <div className="list-toolbar"><label className="search-field"><Search aria-hidden="true" /><input ref={searchRef} type="search" aria-label="Search issues by title or key" placeholder="Search issues…" value={query} onChange={e => setQuery(e.target.value)} maxLength={200} />{charactersEnabled && <kbd>/</kbd>}</label><details className="filter-menu"><summary className="button compact"><ListFilter aria-hidden="true" />Filters{filterCount + Number(archived) > 0 && <span className="count-badge">{filterCount + Number(archived)}</span>}</summary><div className="filter-popover">{!!filterCount && <button type="button" className="quiet" onClick={clearFilters}>Clear filters</button>}<label className="field">Show<select value={archived ? 'true' : ''} onChange={e => setFilter('archived', e.target.value)}><option value="">Active issues</option><option value="true">Archived issues</option></select></label><label className="field">Team<select value={teamId} onChange={e => setFilter('team_id', e.target.value)}><option value="">All teams</option>{data.teams.filter(team => archived || !team.archived_at).map(team => <option key={team.id} value={team.id}>{team.name}{team.archived_at ? ' · Archived' : ''}</option>)}</select></label><label className="field">Status<select value={params.get('status_id') ?? ''} onChange={e => setFilter('status_id', e.target.value)}><option value="">All statuses</option>{statuses.map(status => <option key={status.id} value={status.id}>{!teamId ? `${data.teams.find(t => t.id === status.team_id)?.key} · ` : ''}{status.name}</option>)}</select></label><label className="field">Priority<select value={params.get('priority') ?? ''} onChange={e => setFilter('priority', e.target.value)}><option value="">Any priority</option>{priorities.map((name, i) => <option value={i} key={i}>{name}</option>)}</select></label><LabelPicker filter workspaceId={workspaceId} teamId={teamId || undefined} session={session} value={labelIds} teams={data.teams} change={ids => setFilter('label_ids', ids.join(','))} />{!!labelIds.length && <label className="field">Match labels<select value={params.get('label_mode') ?? 'any'} onChange={event => setFilter('label_mode', event.target.value)}><option value="any">Any selected label</option><option value="all">All selected labels</option></select></label>}</div></details></div>
    {archived && <div className="archive-view-note"><Archive aria-hidden="true" /><span>Archived issues</span><button type="button" className="quiet" onClick={() => setFilter('archived', '')}>Back to active issues</button></div>}
    <div className="list-caption"><span>{busy && !hasDisplay ? 'Loading issues…' : `${items.length}${!board && hasMore ? '+' : ''} issue${items.length === 1 ? '' : 's'}${board ? ' on this page' : ''}`}{busy && hasDisplay && <small className="list-refreshing" role="status">{pendingQuery ? 'Updating results…' : 'Updating…'}</small>}</span><label className="list-sort"><ArrowDownWideNarrow aria-hidden="true" /><select aria-label="Issue order" value={priorityOrder ? 'priority' : 'created'} onChange={event => setFilter('sort', event.target.value === 'created' ? '' : event.target.value)}><option value="created">Newest first, grouped by status</option><option value="priority">Priority, unset last</option></select></label></div>
    <ErrorBanner error={error} retry={retryPage} />
    {board && (!boardTeam ? <div className="empty-state"><h2>{teamId ? 'Team unavailable.' : 'Choose a team for this board.'}</h2><p>Use Board team to show its status columns with the current filters.</p></div> : !hasSnapshot ? !error && <Loading label="Loading board…" /> : <>
      <IssueBoard key={listingScope} workspaceId={workspaceId} teamId={boardTeam.id} data={data} session={session} items={items} statuses={statuses} params={params} selectedId={selectedId} disabled={busy} readOnly={readOnly || archived || !!boardTeam.archived_at} headingRef={headingRef} moved={issue => { if (currentFocusScope.current.scope !== resultScope) return; updated?.(issue); setRetry(value => value + 1); }} reload={() => setRetry(value => value + 1)} denied={denyBoard} blockingChanged={setBoardBlocking} workflowChanged={workflowChanged} />
    </>)}
    {board && boardTeam && <div className="board-pagination" aria-label="Board pages"><button type="button" className="button compact" aria-disabled={busy || boardBlocking || boardCursors.length <= 1} onClick={() => { if (!busy && !boardBlocking && boardCursors.length > 1) setBoardPages({ scope: listingScope, cursors: boardCursors.slice(0, -1) }); }}>Previous board page</button><span>Page {boardCursors.length} · up to 50 issues</span><button type="button" className="button compact" aria-disabled={busy || boardBlocking || !hasMore || !nextCursor} onClick={() => { if (!busy && !boardBlocking && hasMore && nextCursor) setBoardPages({ scope: listingScope, cursors: [...boardCursors, nextCursor] }); }}>Next board page</button></div>}
    {board && boardCursor && !!error && <button type="button" className="button" onClick={() => setBoardPages({ scope: listingScope, cursors: [null] })}>First board page</button>}
    {!board && (busy && !hasDisplay ? <Loading label="Loading issues…" /> : !items.length && !error && !pendingQuery ? <div className="empty-state"><div className="empty-icon">{archived ? <Archive aria-hidden="true" /> : <Search aria-hidden="true" />}</div><h2>{filtered ? 'No issues match these filters.' : archived ? 'The archive is empty.' : mine ? 'Nothing assigned to you yet.' : 'Your next piece of work starts here.'}</h2><p>{filtered ? 'Try a different title, key, status, priority, or label.' : archived ? 'Archived issues stay available here when they are no longer part of active work.' : mine ? 'Assign an issue to yourself, or create something to work on.' : 'Capture a change, give it an owner, and move it forward.'}</p>{filtered ? <button className="button" onClick={clearFilters}><X aria-hidden="true" />Clear filters</button> : !archived && !readOnly && data.teams.some(team => !team.archived_at) && <button className="button primary" onClick={create}><Plus aria-hidden="true" />Create an issue</button>}</div> :
      <div className="issue-groups" onCompositionStart={() => { composing.current = true; }} onCompositionEnd={() => { composing.current = false; }} onKeyDown={moveRow}>{groups.map(({ id, name, teamId: groupTeam, status }) => {
        const group = priorityOrder ? items : items.filter(issue => issue.status_id === id);
        if (!group.length) return null;
        return <section className="issue-group" key={id} aria-label={name}><h2 className="group-heading">{!priorityOrder && <StatusMark status={status} />}{name}<span>{group.length}</span>{!teamId && data.teams.length > 1 && <small>{data.teams.find(t => t.id === groupTeam)?.name}</small>}</h2>{group.map(issue => <Link className={`issue-row${selectedId === issue.id ? ' selected' : ''}`} id={`row-${issue.id}`} href={issuesUrl(workspaceId, params, issue.id)} key={issue.id} aria-current={selectedId === issue.id ? 'page' : undefined}><span className="issue-key">{issue.key}</span><span className="issue-title">{issue.title}</span>{priorityOrder && <span title={statusMap.get(issue.status_id)?.name ?? 'Status unavailable'}><StatusMark status={statusMap.get(issue.status_id)} /></span>}{!!issue.labels.length && <span className="row-labels" aria-label={`Labels: ${issue.labels.map(label => `${label.name}${label.archived_at ? ' (archived)' : ''}`).join(', ')}`} title={issue.labels.map(label => label.name).join(', ')}>{issue.labels.slice(0, 2).map(label => <span className="row-label" key={label.id}><LabelDot color={label.color} />{label.name}</span>)}{issue.labels.length > 2 && <small>+{issue.labels.length - 2}</small>}</span>}{issue.priority > 0 && <span className={`row-priority priority-${issue.priority}`} title={`${priorities[issue.priority]} priority`}>{priorities[issue.priority]}</span>}{issue.due_date && <time className="row-due" dateTime={issue.due_date}>{formatDate(issue.due_date)}</time>}<Avatar small user={issue.assignee ?? (issue.assignee_id ? { display_name: 'Assignee unavailable' } : undefined)} /></Link>)}</section>;
      })}</div>)}
    {!board && hasMore && !busy && <div className="load-more"><button className="button" onClick={loadMore} disabled={moreBusy}>{moreBusy ? 'Loading…' : 'Load more issues'}</button></div>}
  </section>;
}
