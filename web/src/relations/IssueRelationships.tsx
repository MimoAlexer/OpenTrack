import { useEffect, useId, useRef, useState, type MouseEvent } from 'react';
import { ArrowRightLeft, ChevronDown, CornerDownRight, Ellipsis, LoaderCircle, Plus, RefreshCw, Unlink } from 'lucide-react';
import { ApiError } from '../api';
import { ErrorBanner, Modal } from '../components';
import { issuesUrl, Link } from '../router';
import type { Bootstrap, Issue, Session } from '../types';
import { useScopedRequest } from '../settings/shared';
import { CreateIssue } from '../issues/CreateIssue';
import { GraphMark } from './IssueTargetPicker';
import { LinkIssue } from './LinkIssue';
import { MoveIssue } from './MoveIssue';
import { relationNames, type IssueGraphTarget, type IssueRelation, type RelationKind } from './types';
import { useIssueGraph } from './useIssueGraph';
import './relations.css';

export function IssueRelationships({ workspaceId, issue, session, data, disabled, pending = false, dirty, updated, openLink }: { workspaceId: string; issue: Issue; session: Session; data: Bootstrap; disabled: boolean; pending?: boolean; dirty: boolean; updated: (issue: Issue) => void; openLink: (event: MouseEvent<HTMLAnchorElement>, href: string) => void }) {
  const request = useScopedRequest(); const graph = useIssueGraph(workspaceId, issue, session);
  const [linkMode, setLinkMode] = useState<'parent' | 'child' | 'relation' | null>(null); const [creating, setCreating] = useState(false); const [moving, setMoving] = useState(false);
  const [remove, setRemove] = useState<'parent' | IssueRelation | null>(null); const [busy, setBusy] = useState(false); const [error, setError] = useState<unknown>(null); const [review, setReview] = useState(false);
  const [menu, setMenu] = useState<'add' | 'more' | null>(null);
  const actions = useRef<HTMLDivElement>(null); const addTrigger = useRef<HTMLButtonElement>(null); const moreTrigger = useRef<HTMLButtonElement>(null);
  const actionId = useId();
  const locked = disabled || pending || dirty || busy; const current = graph.data;
  useEffect(() => {
    if (!menu) return;
    actions.current?.querySelector<HTMLButtonElement>('.graph-action-popover button:not(:disabled)')?.focus();
    const outside = (event: PointerEvent) => { if (event.target instanceof Node && !actions.current?.contains(event.target)) setMenu(null); };
    document.addEventListener('pointerdown', outside);
    return () => document.removeEventListener('pointerdown', outside);
  }, [menu]);
  function closeMenu() { (menu === 'add' ? addTrigger : moreTrigger).current?.focus(); setMenu(null); }
  function runAction(action: () => void) { closeMenu(); action(); }
  function saved(result: Issue) { setLinkMode(null); setRemove(null); setMoving(false); setError(null); updated(result); }
  async function removeLink() {
    if (!remove || locked || review) return; setBusy(true); setError(null);
    try { const result = await request<Issue>(`/workspaces/${workspaceId}/issues/${issue.id}/${remove === 'parent' ? 'parent' : `relations/${remove.id}`}`, { method: remove === 'parent' ? 'PATCH' : 'DELETE', ...(remove === 'parent' ? { body: { parent_id: null } } : {}), csrf: session.csrf_token, headers: { 'If-Match': issue.etag } }); saved(result); }
    catch (error) { setError(error); if (error instanceof ApiError && ([0, 408, 409, 412].includes(error.status) || error.status >= 500)) setReview(true); }
    finally { setBusy(false); }
  }
  async function refreshSource() {
    setBusy(true); setError(null);
    try { const result = await request<Issue>(`/workspaces/${workspaceId}/issues/${issue.id}`); setRemove(null); setReview(false); updated(result); graph.refresh(); }
    catch (error) { setError(error); } finally { setBusy(false); }
  }
  function target(value: IssueGraphTarget | null) {
    if (!value) return <span className="unavailable-target">Issue unavailable</span>;
    const href = issuesUrl(workspaceId, new URLSearchParams(location.search), value.id);
    return <Link className="graph-target" href={href} onClick={event => openLink(event, href)}><span className="issue-key">{value.key}</span><span className="graph-title">{value.title}</span><span className="graph-status"><GraphMark target={value} />{value.status.name}</span>{value.archived_at && <small>Archived</small>}</Link>;
  }
  return <section className="issue-relationships" aria-label="Issue relationships">
    <header className="graph-heading"><h2>Connections</h2><div className="graph-actions" ref={actions} onBlur={event => { if (!(event.relatedTarget instanceof Node) || !event.currentTarget.contains(event.relatedTarget)) setMenu(null); }} onKeyDown={event => { if (event.key === 'Escape' && menu) { event.preventDefault(); event.stopPropagation(); closeMenu(); } }}>
      {!disabled && <button ref={addTrigger} className="quiet graph-add" type="button" disabled={locked || !current} title={dirty ? 'Save or discard your edits before changing connections.' : undefined} aria-describedby={dirty ? `${actionId}-blocked` : undefined} aria-expanded={menu === 'add'} aria-controls={`${actionId}-add`} onClick={() => setMenu(menu === 'add' ? null : 'add')}><Plus aria-hidden="true" />Add connection<ChevronDown aria-hidden="true" /></button>}
      <button ref={moreTrigger} className="icon-button" type="button" aria-label="More connection actions" aria-expanded={menu === 'more'} aria-controls={`${actionId}-more`} onClick={() => setMenu(menu === 'more' ? null : 'more')}><Ellipsis aria-hidden="true" /></button>
      {menu === 'add' && !disabled && <div className="graph-action-popover" id={`${actionId}-add`} role="group" aria-label="Add connection">
        {current?.parent.can_change && <button type="button" disabled={locked} onClick={() => runAction(() => setLinkMode('parent'))}>{current.parent.parent ? 'Change parent' : 'Set parent'}</button>}
        <button type="button" disabled={locked} onClick={() => runAction(() => setLinkMode('child'))}>Add existing subissue</button>
        <button type="button" disabled={locked} onClick={() => runAction(() => setCreating(true))}>Create subissue</button>
        <button type="button" disabled={locked} onClick={() => runAction(() => setLinkMode('relation'))}>Add relationship</button>
      </div>}
      {menu === 'more' && <div className="graph-action-popover" id={`${actionId}-more`} role="group" aria-label="Connection actions">
        <button type="button" disabled={busy} onClick={() => runAction(graph.refresh)}><RefreshCw aria-hidden="true" />Refresh relationships</button>
        {!disabled && <button type="button" disabled={locked} onClick={() => runAction(() => setMoving(true))}><ArrowRightLeft aria-hidden="true" />Move to team</button>}
      </div>}
    </div></header>
    <p className="sr-only" id={`${actionId}-blocked`}>{dirty ? 'Save or discard your edits before changing connections or moving this issue.' : ''}</p>
    <ErrorBanner error={graph.error} retry={graph.refresh} /><ErrorBanner error={remove ? null : error} retry={refreshSource} />
    {graph.loading && <p className="graph-loading" role="status"><LoaderCircle className="spinner" aria-hidden="true" />Loading connections…</p>}
    {current && <>
      {!current.parent.parent && !current.children.data.length && !current.relations.data.length && !current.children.has_more && !current.relations.has_more && <p className="graph-empty" role="status">No connections yet.</p>}
      {current.parent.parent && <section className="graph-group" aria-labelledby={`parent-${issue.id}`}><header><h3 id={`parent-${issue.id}`}>Parent</h3></header><div className="graph-row">{target(current.parent.parent.target)}{!disabled && current.parent.parent.can_detach && <button type="button" className="icon-button" aria-label="Detach parent" disabled={locked} onClick={() => { setRemove('parent'); setError(null); setReview(false); }}><Unlink aria-hidden="true" /></button>}</div></section>}
      {(current.children.data.length > 0 || current.children.has_more) && <section className="graph-group" aria-labelledby={`children-${issue.id}`}><header><h3 id={`children-${issue.id}`}><CornerDownRight aria-hidden="true" />Subissues<span className="graph-count">{current.children.data.length}{current.children.has_more ? '+' : ''}</span></h3></header>{current.children.data.map(child => <div className="graph-row" key={child.id}>{target(child)}</div>)}{current.children.has_more && <button type="button" className="button compact" disabled={graph.moreBusy} onClick={() => graph.more('children')}>Load more subissues</button>}</section>}
      {(current.relations.data.length > 0 || current.relations.has_more) && <section className="graph-group" aria-labelledby={`relations-${issue.id}`}><header><h3 id={`relations-${issue.id}`}>Relationships<span className="graph-count">{current.relations.data.length}{current.relations.has_more ? '+' : ''}</span></h3></header>{(Object.keys(relationNames) as RelationKind[]).map(kind => { const links = current.relations.data.filter(link => link.kind === kind); return links.length > 0 && <div className="graph-relation-group" key={kind}><h4>{relationNames[kind]}</h4>{links.map(link => <div className="graph-row" key={link.id}>{target(link.target)}{!disabled && link.can_remove && <button className="icon-button" type="button" aria-label={`Remove ${relationNames[kind].toLowerCase()} ${link.target?.key ?? 'unavailable issue'}`} disabled={locked} onClick={() => { setRemove(link); setError(null); setReview(false); }}><Unlink aria-hidden="true" /></button>}</div>)}</div>; })}{current.relations.has_more && <button type="button" className="button compact" disabled={graph.moreBusy} onClick={() => graph.more('relations')}>Load more relationships</button>}</section>}
    </>}
    {linkMode && <LinkIssue workspaceId={workspaceId} issue={issue} session={session} mode={linkMode} close={() => setLinkMode(null)} saved={saved} reviewed={updated} />}
    {creating && <CreateIssue workspaceId={workspaceId} data={data} session={session} initialTeam={issue.team_id} parent={issue} close={() => setCreating(false)} created={async () => { setCreating(false); await refreshSource(); }} />}
    {moving && <MoveIssue workspaceId={workspaceId} issue={issue} data={data} session={session} close={() => setMoving(false)} moved={saved} reviewed={updated} />}
    {remove && <Modal title={remove === 'parent' ? 'Detach parent?' : 'Remove relationship?'} close={() => setRemove(null)} canClose={!busy}><div className="settings-modal-content"><p>{remove === 'parent' ? 'This issue will become a top-level issue. Its own subissues stay attached.' : 'The link is removed from both issues. Their statuses stay unchanged.'}</p><ErrorBanner error={error} />{review && <div className="notice">The result could not be confirmed or the issue changed. Reload the issue and review its links before trying again.<button type="button" className="button compact" disabled={busy} onClick={refreshSource}>Reload issue and links</button></div>}<div className="modal-footer"><button type="button" className="button" data-autofocus disabled={busy} onClick={() => setRemove(null)}>Cancel</button><button type="button" className="button primary" disabled={busy || review} onClick={removeLink}>{busy ? 'Removing…' : remove === 'parent' ? 'Detach parent' : 'Remove link'}</button></div></div></Modal>}
  </section>;
}
