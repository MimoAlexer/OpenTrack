import { useEffect, useRef, useState, type FormEvent } from 'react';
import { ArrowDown, ArrowUp, Flag, Pencil, Plus, Trash2 } from 'lucide-react';
import { ApiError, request as read } from '../api';
import { ErrorBanner, Loading, Modal } from '../components';
import { isAccessError } from '../documents/model';
import { useScopedRequest } from '../settings/shared';
import { useSessionScope } from '../desktop/context';
import { useUnsavedChanges } from '../router';
import type { components } from '../generated/api';
import type { Session } from '../types';
import { Progress } from './Projects';
import { projectMetadata, projectMetadataPath, projectPath, type FullProject, type Project } from './types';

export type Milestone = components['schemas']['Milestone'];
export type MilestoneOverview = components['schemas']['MilestoneOverview'];
type MilestonePage = components['schemas']['MilestonePage'];
type Move = { id: string; anchor: string; delta: number; title: string; anchorTitle: string };
type MoveReview = { move: Move; current?: Project; items?: MilestoneOverview[]; order?: string[]; observed?: boolean };

async function currentOrder(request: typeof read, project: Project) {
  const path = projectPath(project.workspace_id, project.team_id, project.id);
  const signal = AbortSignal.timeout(10_000);
  const before = await request<FullProject>(path, { signal });
  if (before.id !== project.id || before.workspace_id !== project.workspace_id || before.team_id !== project.team_id || !before.etag || !before.metadata_etag) throw new Error('The project scope changed. Reload the project.');
  const items: MilestoneOverview[] = []; const cursors = new Set<string>(); let cursor: string | null = null;
  for (let pageIndex = 0; pageIndex < 5; pageIndex += 1) {
    const page: MilestonePage = await request(`${path}/milestones?${new URLSearchParams({ limit: '100', ...(cursor ? { cursor } : {}) })}`, { signal });
    if (page.project_etag !== before.etag || typeof page.has_more !== 'boolean' || !(page.next_cursor === null || typeof page.next_cursor === 'string') || !Array.isArray(page.data) || page.data.length > 100 || page.data.some(item => !item.milestone.id || item.milestone.workspace_id !== project.workspace_id || item.milestone.team_id !== project.team_id || item.milestone.project_id !== project.id || items.some(previous => previous.milestone.id === item.milestone.id)) || new Set(page.data.map(item => item.milestone.id)).size !== page.data.length) throw new Error('The complete current milestone order could not be verified. Reload current order.');
    items.push(...page.data);
    if (!page.has_more) {
      if (page.next_cursor) throw new Error('The milestone cursor is inconsistent. Reload current order.');
      const current = await request<Project>(projectMetadataPath(project.workspace_id, project.team_id, project.id), { signal });
      if (current.id !== before.id || current.workspace_id !== before.workspace_id || current.team_id !== before.team_id || current.etag !== before.metadata_etag) throw new Error('The project changed during review. Reload current order.');
      return { current, items };
    }
    if (!page.data.length || !page.next_cursor || cursors.has(page.next_cursor)) throw new Error('The milestone cursor is incomplete or repeated. Reload current order.');
    cursor = page.next_cursor; cursors.add(cursor);
  }
  throw new Error('The complete order exceeds the 500-milestone review limit. No retry was sent.');
}
export function useMilestones(project: Project | null, refresh: number) {
  const path = project ? `${projectPath(project.workspace_id, project.team_id, project.id)}/milestones` : '';
  const [items, setItems] = useState<MilestoneOverview[]>([]); const [loading, setLoading] = useState(true); const [error, setError] = useState<unknown>(null); const [attempt, setAttempt] = useState(0);
  useEffect(() => {
    if (!path) return;
    const abort = new AbortController(); setLoading(true); setError(null);
    void (async () => {
      const all: MilestoneOverview[] = []; let cursor: string | null = null;
      do {
        const page: MilestonePage = await read(`${path}?${new URLSearchParams({ limit: '100', ...(cursor ? { cursor } : {}) })}`, { signal: abort.signal });
        all.push(...page.data); cursor = page.has_more ? page.next_cursor : null;
      } while (cursor && all.length < 500);
      if (!abort.signal.aborted) setItems(all);
    })().catch(error => { if (!abort.signal.aborted) { setError(error); if (isAccessError(error)) setItems([]); } }).finally(() => { if (!abort.signal.aborted) setLoading(false); });
    return () => abort.abort();
  }, [path, refresh, attempt]);
  return { items, loading, error, retry: () => setAttempt(value => value + 1) };
}
interface Props { project: Project; session: Session; resource: ReturnType<typeof useMilestones>; readOnly: boolean; changed: (value: Project) => void; denied: (error: unknown) => void }
export function Milestones({ project, session, resource, readOnly, changed, denied }: Props) {
  const scope = useSessionScope(session);
  return <ScopedMilestones key={`${scope}:${project.workspace_id}:${project.team_id}:${project.id}`} {...{ project, session, resource, readOnly, changed, denied }}/>;
}
function ScopedMilestones({ project, session, resource, readOnly, changed, denied }: Props) {
  const request = useScopedRequest(); const path = projectPath(project.workspace_id, project.team_id, project.id);
  const [editor, setEditor] = useState<Milestone | 'new' | null>(null); const [deleting, setDeleting] = useState<Milestone | null>(null); const [busy, setBusy] = useState(false); const [error, setError] = useState<unknown>(null); const [limit, setLimit] = useState(50);
  const container = useRef<HTMLElement>(null); const moveFocus = useRef<{ id: string; index?: number; delta: number } | null>(null);
  const [review, setReview] = useState<MoveReview | null>(null); const [unavailable, setUnavailable] = useState(false); const flight = useRef(false);
  useUnsavedChanges(!!review || busy);
  const canManage = !readOnly && (project.can_manage || project.lead_id === session.user.id);
  useEffect(() => { if (isAccessError(resource.error)) { setReview(null); setUnavailable(true); denied(resource.error); } }, [resource.error]);
  useEffect(() => {
    const focus = moveFocus.current; if (!focus || resource.loading || review) return;
    if (resource.error) { container.current?.focus(); moveFocus.current = null; return; }
    if (focus.index !== undefined && resource.items.findIndex(item => item.milestone.id === focus.id) !== focus.index) return;
    const target = container.current?.querySelector<HTMLButtonElement>(`button[data-move-id="${focus.id}"][data-direction="${focus.delta}"]`);
    if (target && !target.disabled) target.focus(); else (container.current?.querySelector<HTMLButtonElement>(`button[data-edit-id="${focus.id}"]`) ?? container.current)?.focus();
    moveFocus.current = null;
  }, [resource.items, resource.loading, resource.error, review]);
  function closeReview() {
    if (busy || !review) return;
    moveFocus.current = { id: review.move.id, delta: review.move.delta };
    setReview(null);
  }
  async function refresh() { changed(await request<Project>(projectMetadataPath(project.workspace_id, project.team_id, project.id))); resource.retry(); }
  async function recover(move: Move) {
    moveFocus.current = null; setReview({ move });
    try {
      const { current, items } = await currentOrder(request, project);
      changed(current); resource.retry();
      const order = items.map(item => item.milestone.id).filter(id => id !== move.id);
      const anchor = order.indexOf(move.anchor);
      if (anchor < 0 || !items.some(item => item.milestone.id === move.id)) throw new Error('The moved milestone or its target is unavailable. Discard this move or reload current order.');
      order.splice(anchor + (move.delta > 0 ? 1 : 0), 0, move.id);
      setReview({ move, current, items, order, observed: items.every((item, index) => item.milestone.id === order[index]) }); setError(null);
    } catch (error) { setError(error); if (isAccessError(error)) { setReview(null); setUnavailable(true); denied(error); } }
  }
  async function sendMove(move: Move, order: string[], etag: string) {
    if (flight.current || !canManage) return; flight.current = true;
    moveFocus.current = { id: move.id, index: order.indexOf(move.id), delta: move.delta };
    setBusy(true); setError(null);
    try { changed(projectMetadata(await request<FullProject>(`${path}/milestones/order`, { method: 'PUT', csrf: session.csrf_token, headers: { 'If-Match': etag }, body: { milestone_ids: order } }))); setReview(null); resource.retry(); }
    catch (error) {
      moveFocus.current = null; setError(error);
      if (isAccessError(error)) { setReview(null); setUnavailable(true); denied(error); }
      else if (!(error instanceof ApiError) || [0, 408, 412].includes(error.status) || error.status >= 500) await recover(move);
    } finally { flight.current = false; setBusy(false); }
  }
  async function reorder(index: number, delta: number) {
    if (busy || review || !canManage || resource.loading || resource.error) return;
    const ids = resource.items.map(item => item.milestone.id); [ids[index], ids[index + delta]] = [ids[index + delta], ids[index]];
    const move = { id: resource.items[index].milestone.id, anchor: resource.items[index + delta].milestone.id, delta, title: resource.items[index].milestone.title, anchorTitle: resource.items[index + delta].milestone.title };
    moveFocus.current = { id: resource.items[index].milestone.id, index: index + delta, delta }; if (index + delta >= limit) setLimit(value => value + 50);
    await sendMove(move, ids, project.etag);
  }
  async function remove() {
    if (!deleting || busy) return; setBusy(true); setError(null);
    try { await request(`${path}/milestones/${deleting.id}`, { method: 'DELETE', csrf: session.csrf_token, headers: { 'If-Match': deleting.etag } }); setDeleting(null); await refresh(); }
    catch (error) { setError(error); if (isAccessError(error)) denied(error); }
    finally { setBusy(false); }
  }
  if (unavailable || isAccessError(resource.error)) return <section aria-label="Project milestones"><h3>Milestones unavailable</h3><ErrorBanner error={error || resource.error}/></section>;
  return <section ref={container} tabIndex={-1} className="milestone-section" aria-label="Project milestones"><header className="project-section-heading"><div><h3>Milestones</h3><p>Intermediate outcomes. Completion is a manual decision, separate from issue progress.</p></div>{canManage && <button className="button compact" disabled={busy || !!review} onClick={() => setEditor('new')}><Plus aria-hidden="true"/>New milestone</button>}</header>
    {!!resource.items.length && (resource.loading ? <p role="status">Updating milestone counts. Showing the last confirmed calculations.</p> : resource.error ? <p role="status">Milestone counts could not be refreshed. Showing the last confirmed calculations.</p> : null)}
    <ErrorBanner error={resource.error} retry={resource.retry}/><ErrorBanner error={error}/>{error instanceof ApiError && error.status === 412 && <p className="notice">The order changed. The latest order is loaded; review it before moving again.</p>}
    {resource.loading && !resource.items.length ? <Loading label="Loading milestones…"/> : <div aria-busy={resource.loading}>{resource.items.slice(0, limit).map(({ milestone, progress }, index) => <article className="milestone-group" key={milestone.id}><header><div><h4><Flag aria-hidden="true"/>{milestone.title}</h4><p>{milestone.state === 'completed' ? 'Manually completed' : 'Planned'} · {milestone.target_date ? `Target ${new Date(`${milestone.target_date}T12:00:00`).toLocaleDateString()}` : 'No target date'}</p></div>{canManage && milestone.can_manage && <div className="button-row"><button className="icon-button" data-move-id={milestone.id} data-direction={-1} aria-label={`Move ${milestone.title} up`} disabled={busy || resource.loading || index === 0} onClick={() => void reorder(index, -1)}><ArrowUp aria-hidden="true"/></button><button className="icon-button" data-move-id={milestone.id} data-direction={1} aria-label={`Move ${milestone.title} down`} disabled={busy || resource.loading || index === resource.items.length - 1} onClick={() => void reorder(index, 1)}><ArrowDown aria-hidden="true"/></button><button className="icon-button" data-edit-id={milestone.id} aria-label={`Edit ${milestone.title}`} onClick={() => setEditor(milestone)}><Pencil aria-hidden="true"/></button><button className="icon-button" aria-label={`Delete ${milestone.title}`} onClick={() => { setError(null); setDeleting(milestone); }}><Trash2 aria-hidden="true"/></button></div>}</header><Progress value={progress} label={`${milestone.title} issue completion`}/></article>)}{!resource.items.length && !resource.error && <p className="project-empty">No milestones yet. Group project issues into intermediate outcomes.</p>}</div>}
    {resource.items.length > limit && <button className="button compact" onClick={() => setLimit(value => value + 50)}>Show more milestones</button>}
    {review && <Modal title="Review milestone move" close={closeReview} canClose={!busy}><div className="project-form"><p>Move “{review.move.title}” {review.move.delta < 0 ? 'before' : 'after'} “{review.move.anchorTitle}”. Your intended move is preserved; review it before moving again.</p><ErrorBanner error={error}/>{busy ? <p role="status">Checking current order…</p> : review.observed ? <p role="status">The desired milestone position is present in the current order. This confirms the current state, not who applied it.</p> : <p>The current order is shown below. Applying keeps every currently loaded milestone and uses the reviewed project version.</p>}{review.items && <ol aria-label="Current milestone order">{review.items.map(item => <li key={item.milestone.id}>{item.milestone.title}</li>)}</ol>}<div className="modal-footer"><button type="button" className="button" disabled={busy} onClick={closeReview}>{review.observed ? 'Done' : 'Discard move'}</button><button type="button" className="button" disabled={busy} onClick={async () => { if (flight.current) return; flight.current = true; setBusy(true); try { await recover(review.move); } finally { flight.current = false; setBusy(false); } }}>Reload current order</button>{!review.observed && <button type="button" className="button primary" disabled={busy || !canManage || !review.order || !review.current?.can_edit || !(review.current.can_manage || review.current.lead_id === session.user.id)} onClick={() => { if (review.order && review.current) void sendMove(review.move, review.order, review.current.etag); }}>Apply reviewed move</button>}</div></div></Modal>}
    {editor && <MilestoneEditor project={project} session={session} milestone={editor === 'new' ? undefined : editor} close={() => setEditor(null)} saved={() => { setEditor(null); void refresh().catch(setError); }} denied={denied}/>}
    {deleting && <Modal title="Delete milestone?" close={() => { if (!busy) setDeleting(null); }} canClose={!busy}><div className="project-form"><p>Remove “{deleting.title}” and detach its issues. Issues remain in the project and retain their status.</p><ErrorBanner error={error}/><div className="modal-footer"><button className="button" disabled={busy} onClick={() => setDeleting(null)}>Cancel</button><button className="button danger" disabled={busy} onClick={() => void remove()}>Delete milestone</button></div></div></Modal>}
  </section>;
}
function MilestoneEditor({ project, session, milestone, close, saved, denied }: { project: Project; session: Session; milestone?: Milestone; close: () => void; saved: () => void; denied: (error: unknown) => void }) {
  const request = useScopedRequest(); const path = projectPath(project.workspace_id, project.team_id, project.id);
  const [base, setBase] = useState(milestone); const [projectEtag, setProjectEtag] = useState(project.etag); const [title, setTitle] = useState(milestone?.title ?? ''); const [target, setTarget] = useState(milestone?.target_date ?? ''); const [state, setState] = useState(milestone?.state ?? 'planned'); const [busy, setBusy] = useState(false); const [error, setError] = useState<unknown>(null); const [conflict, setConflict] = useState(false); const [latest, setLatest] = useState<Milestone | null>(null); const [uncertain, setUncertain] = useState(false); const sent = useRef<{ key: string; etag: string; body: object } | null>(null);
  const dirty = title !== (base?.title ?? '') || target !== (base?.target_date ?? '') || state !== (base?.state ?? 'planned'); const release = useUnsavedChanges(dirty || uncertain);
  async function submit(event?: FormEvent) {
    event?.preventDefault(); if (busy || conflict || !title.trim()) return;
    if (!uncertain || !sent.current) sent.current = { key: crypto.randomUUID(), etag: base?.etag ?? projectEtag, body: { title: title.trim(), target_date: target || null, ...(base ? { state } : {}) } };
    const pending = sent.current; setBusy(true); setError(null);
    try { await request(`${path}/milestones${base ? `/${base.id}` : ''}`, { method: base ? 'PATCH' : 'POST', csrf: session.csrf_token, headers: { 'If-Match': pending.etag, ...(!base ? { 'Idempotency-Key': pending.key } : {}) }, body: pending.body }); release(); saved(); }
    catch (error) { setError(error); if (isAccessError(error)) denied(error); setUncertain(error instanceof ApiError && (error.status === 0 || error.status >= 500)); if (error instanceof ApiError && error.status === 412) setConflict(true); }
    finally { setBusy(false); }
  }
  async function review() {
    setBusy(true);
    try { if (base) setLatest(await request<Milestone>(`${path}/milestones/${base.id}`)); else { setProjectEtag((await request<Project>(projectMetadataPath(project.workspace_id, project.team_id, project.id))).etag); setConflict(false); } setError(null); }
    catch (error) { setError(error); if (isAccessError(error)) denied(error); }
    finally { setBusy(false); }
  }
  return <Modal title={base ? 'Edit milestone' : 'New milestone'} close={() => { if (!busy && !uncertain && (!dirty || window.confirm('Discard milestone changes?'))) close(); }} canClose={!busy && !uncertain}><form className="project-form" onSubmit={submit}><label className="field">Title<input data-autofocus name="milestone-title" value={title} onChange={event => setTitle(event.target.value)} disabled={busy || uncertain}/></label><label className="field">Target date<input type="date" value={target} onChange={event => setTarget(event.target.value)} disabled={busy || uncertain}/></label>{base && <label className="field">Milestone state<select value={state} onChange={event => setState(event.target.value as Milestone['state'])} disabled={busy || uncertain}><option value="planned">Planned</option><option value="completed">Completed manually</option></select></label>}<p className="project-help">Completing a milestone never changes issue statuses.</p><ErrorBanner error={error}/>{conflict && <div className="project-conflict"><p>This milestone or project changed. Your draft is retained. Load the latest version and review your draft before saving.</p>{latest ? <><p>Saved: {latest.title} · {latest.target_date ?? 'No target date'} · {latest.state}.</p><button type="button" className="button" disabled={busy} onClick={() => { setBase(latest); setLatest(null); setConflict(false); sent.current = null; }}>Keep my draft for review</button></> : <button type="button" className="button" disabled={busy} onClick={() => void review()}>Load latest for review</button>}</div>}<div className="modal-footer"><button type="button" className="button" disabled={busy || uncertain} onClick={() => { if (!dirty || window.confirm('Discard milestone changes?')) close(); }}>Cancel</button>{uncertain ? <button type="button" className="button primary" disabled={busy} onClick={() => void submit()}>Retry safely</button> : <button className="button primary" disabled={busy || conflict || !title.trim()}>{base ? 'Save milestone' : 'Create milestone'}</button>}</div></form></Modal>;
}
