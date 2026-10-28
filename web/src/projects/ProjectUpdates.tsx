import { useEffect, useRef, useState, type FormEvent } from 'react';
import { ApiError } from '../api';
import { Avatar, ErrorBanner, Loading } from '../components';
import { isAccessError } from '../documents/model';
import { useUnsavedChanges } from '../router';
import { useScopedRequest } from '../settings/shared';
import type { Session } from '../types';
import { healthLabels, projectPath, states, type Project, type ProjectUpdate } from './types';
import { useProjectPage } from './useProjectData';

function UpdateComparison({ update, next }: { update: ProjectUpdate; next: ProjectUpdate }) {
  const progress = (item: ProjectUpdate) => item.eligible ? `${item.completed} of ${item.eligible} eligible issues completed` : 'no eligible issue scope';
  const changes = [
    update.state !== next.state && `State: this update ${states[update.state]}; next shown update ${states[next.state]}`,
    update.health !== next.health && `Health: this update ${healthLabels[update.health]}; next shown update ${healthLabels[next.health]}`,
    (update.completed !== next.completed || update.eligible !== next.eligible) && `Progress: this update ${progress(update)}; next shown update ${progress(next)}`,
  ].filter((change): change is string => Boolean(change));
  return <div><p>Compared with the next shown update:</p>{changes.length ? <ul>{changes.map(change => <li key={change}>{change}</li>)}</ul> : <p>State, health and progress match.</p>}</div>;
}

export function ProjectUpdates({ project, session, refresh, readOnly, denied }: { project: Project; session: Session; refresh: number; readOnly: boolean; denied: (error: unknown) => void }) {
  const path = `${projectPath(project.workspace_id, project.team_id, project.id)}/updates`; const request = useScopedRequest();
  const [revision, setRevision] = useState(0); const directory = useProjectPage<ProjectUpdate>(path, refresh + revision);
  const [body, setBody] = useState(''); const [error, setError] = useState<unknown>(null); const [busy, setBusy] = useState(false); const [uncertain, setUncertain] = useState(false); const [notice, setNotice] = useState('');
  const pending = useRef<{ body: string; key: string } | null>(null);
  useUnsavedChanges(!!body.trim() || uncertain);
  useEffect(() => { if (isAccessError(directory.error)) denied(directory.error); }, [directory.error]);
  async function submit(event: FormEvent) {
    event.preventDefault(); if (busy || !body.trim() || readOnly) return;
    if (!uncertain || !pending.current) pending.current = { body: body.trim(), key: crypto.randomUUID() };
    setBusy(true); setError(null); setNotice('');
    try { await request<ProjectUpdate>(path, { method: 'POST', csrf: session.csrf_token, headers: { 'Idempotency-Key': pending.current.key }, body: { body: pending.current.body } }); setBody(''); setUncertain(false); pending.current = null; setRevision(value => value + 1); setNotice('Update published.'); }
    catch (error) { setError(error); if (isAccessError(error)) denied(error); setUncertain(error instanceof ApiError && (error.status === 0 || error.status === 408 || error.status >= 500)); }
    finally { setBusy(false); }
  }
  return <section className="project-updates" aria-label="Project updates"><header className="project-section-heading"><div><h3>Updates</h3><p>Written progress, with state and health captured when published.</p></div></header>
    {!readOnly && <form className="project-update-composer" onSubmit={submit}><label className="field">Write an update<textarea name="project-update" value={body} onChange={event => setBody(event.target.value)} rows={4} placeholder="What changed? What needs attention?" disabled={busy || uncertain}/></label><ErrorBanner error={error}/>{uncertain && <p className="notice">The response was interrupted. Retry safely to confirm this update without publishing a duplicate.</p>}<div className="project-update-actions"><span>{Array.from(body).length.toLocaleString()} / 8,000</span><button className="button primary compact" disabled={busy || !body.trim()}>{busy ? 'Publishing…' : uncertain ? 'Retry safely' : 'Publish update'}</button></div></form>}
    <p className="project-save-notice" role="status">{notice}</p><ErrorBanner error={directory.error} retry={directory.retry}/>
    {!directory.value && directory.loading ? <Loading label="Loading project updates…"/> : <div className="project-update-feed" aria-busy={directory.loading}>{directory.value?.data.map((update, index, updates) => <article className="project-update" key={update.id}><header><Avatar small user={{ display_name: update.author_name }}/><strong>{update.author_name}</strong><time dateTime={update.created_at}>{new Date(update.created_at).toLocaleString(undefined, { dateStyle: 'medium', timeStyle: 'short' })}</time></header><p className="project-update-body">{update.body}</p><footer><span className={`project-health health-${update.health}`}>{healthLabels[update.health]}</span><span>{states[update.state]}</span><span>{update.eligible ? `${update.completed} of ${update.eligible} completed` : 'No eligible issue scope'}</span></footer>{updates[index + 1] ? <UpdateComparison update={update} next={updates[index + 1]}/> : <p>{directory.value?.has_more ? 'Load more updates to compare this update.' : 'No next update shown.'}</p>}</article>)}{!directory.value?.data.length && !directory.error && <div className="project-empty"><h4>No updates yet</h4><p>Publish a short update when plans change or work reaches a milestone.</p></div>}</div>}
    {directory.value?.has_more && <button className="button compact project-more" disabled={directory.loading} onClick={directory.more}>{directory.loading ? 'Loading…' : 'Load more updates'}</button>}
  </section>;
}
