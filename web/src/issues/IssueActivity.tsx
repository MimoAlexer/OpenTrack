import { useNativeRequestRevision } from '../desktop/context';
import { useEffect, useRef, useState } from 'react';
import { History, LoaderCircle } from 'lucide-react';
import { ApiError, request } from '../api';
import { Avatar, ErrorBanner } from '../components';
import type { ActivityEntry } from '../types';

import { activityFacts, activitySummary, groupActivity } from './activity-model';
export { activitySummary } from './activity-model';
import './activity.css';

function ActivityTime({ entry }: { entry: ActivityEntry }) {
  const date = new Date(entry.created_at); const valid = !Number.isNaN(date.getTime());
  return <time dateTime={entry.created_at} title={valid ? date.toLocaleString() : entry.created_at}>{valid ? date.toLocaleString(undefined, { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' }) : entry.created_at}</time>;
}

function ActivityContent({ entry }: { entry: ActivityEntry }) {
  const facts = activityFacts(entry);
  return <><p><strong>{entry.actor}</strong> {activitySummary(entry)}.</p>{facts.length > 0 && <dl className='activity-facts'>{facts.map(fact => <div key={fact.label}><dt>{fact.label}</dt><dd>{fact.value}</dd></div>)}</dl>}<ActivityTime entry={entry} /></>;
}

export function IssueActivity({ workspaceId, issueId, version, authScope }: { workspaceId: string; issueId: string; version: string; authScope?: string }) {
  const nativeRevision = useNativeRequestRevision();
  const scope = JSON.stringify([workspaceId, issueId, authScope ?? nativeRevision]);
  const [snapshot, setSnapshot] = useState<{ scope: string; entries: ActivityEntry[]; cursor: string | null } | null>(null);
  const [state, setState] = useState<{ scope: string; loading: boolean; error: unknown }>({ scope, loading: true, error: null });
  const hasSnapshot = snapshot?.scope === scope;
  const entries = hasSnapshot ? snapshot.entries : [];
  const loading = state.scope !== scope || state.loading;
  const error = state.scope === scope ? state.error : null;
  const [retry, setRetry] = useState(0); const [moreBusy, setMoreBusy] = useState(false);
  const generation = useRef(0); const olderRequest = useRef<AbortController | null>(null);
  const currentScope = useRef(scope); currentScope.current = scope; const alive = useRef(true);
  useEffect(() => { alive.current = true; return () => { alive.current = false; }; }, []);
  const ordered = (rows: ActivityEntry[]) => [...new Map(rows.map(row => [row.id, row])).values()].sort((a, b) => a.created_at.localeCompare(b.created_at) || a.id.localeCompare(b.id));
  const pagePath = `/workspaces/${workspaceId}/issues/${issueId}/activity-page?limit=25`;
  async function older() {
    if (!snapshot?.cursor || moreBusy || loading) return;
    const abort = new AbortController(); const started = generation.current; olderRequest.current = abort;
    const current = () => alive.current && !abort.signal.aborted && currentScope.current === scope && generation.current === started;
    setMoreBusy(true); setState({ scope, loading: false, error: null });
    try { const result = await request<{ data: ActivityEntry[]; has_more?: boolean; next_cursor?: string | null }>(`${pagePath}&cursor=${encodeURIComponent(snapshot.cursor)}`, { signal: abort.signal }); if (current()) setSnapshot(previous => previous?.scope === scope ? { scope, entries: ordered([...previous.entries, ...result.data]), cursor: result.has_more ? result.next_cursor ?? null : null } : previous); }
    catch (error) { if (current()) { if (error instanceof ApiError && [401,403,404].includes(error.status)) setSnapshot(null); setState({scope, loading:false,error}); } }
    finally { if (current()) { olderRequest.current = null; setMoreBusy(false); } }
  }
  useEffect(() => {
    const abort = new AbortController(); let active = true;
    generation.current += 1; olderRequest.current?.abort(); olderRequest.current = null; setMoreBusy(false);
    setState({ scope, loading: true, error: null });
    request<{ data: ActivityEntry[]; has_more?: boolean; next_cursor?: string | null }>(pagePath, { signal: abort.signal }).then(result => {
      if (active && !abort.signal.aborted) { setSnapshot({ scope, entries: ordered(result.data), cursor: result.has_more ? result.next_cursor ?? null : null }); setState({ scope, loading: false, error: null }); }
    }).catch(error => {
      if (!active || abort.signal.aborted) return;
      if (error instanceof ApiError && [401, 403, 404].includes(error.status)) setSnapshot(null);
      setState({ scope, loading: false, error });
    });
    return () => { active = false; abort.abort(); olderRequest.current?.abort(); };
  }, [scope, version, retry]);
  return <section className="activity" aria-labelledby="activity-heading" aria-busy={loading}>
    <header><h2 id="activity-heading"><History aria-hidden="true" />Activity</h2>{loading && hasSnapshot && <small role="status">Updating…</small>}</header>
    <ErrorBanner error={error} retry={() => setRetry(count => count + 1)} />
    {hasSnapshot && snapshot.cursor && <button type="button" className="quiet comments-older" disabled={moreBusy || loading} onClick={older}>{moreBusy ? 'Loading older activity…' : 'Load older activity'}</button>}
    {loading && !hasSnapshot ? <p className="activity-loading" role="status"><LoaderCircle className="spinner" aria-hidden="true" />Loading activity…</p> : !error && !entries.length ? <p className="activity-empty">No activity has been recorded for this issue.</p> : <ol className="activity-list">{groupActivity(entries).map(group => {
      const entry = group.entries[0];
      return <li key={group.id}><Avatar small user={{ display_name: entry.actor }} /><div>{group.entries.length === 1 ? <ActivityContent entry={entry} /> : <details className="activity-group"><summary><strong>{entry.actor}</strong> changed properties in {group.entries.length} updates.<ActivityTime entry={group.entries.at(-1)!} /></summary><div className="activity-group-events" role="list" aria-label={`${group.entries.length} updates by ${entry.actor}`}>{group.entries.map(item => <div key={item.id} role="listitem"><ActivityContent entry={item} /></div>)}</div></details>}</div></li>;
    })}</ol>}
  </section>;
}
