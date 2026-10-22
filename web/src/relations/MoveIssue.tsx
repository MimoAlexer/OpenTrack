import { useSessionScope } from '../desktop/context';
import { useEffect, useRef, useState, type FormEvent } from 'react';
import { LockKeyhole } from 'lucide-react';
import { ApiError } from '../api';
import { ErrorBanner, Loading, Modal } from '../components';
import { toDraft } from '../issue-model';
import { IssueFields } from '../issues/IssueFields';
import { useScopedRequest, UncertainNotice } from '../settings/shared';
import type { AssignableUser, LabelSummary } from '../taxonomy/types';
import { Picker } from '../taxonomy/Picker';
import { useSearchPage } from '../taxonomy/useSearchPage';
import type { Bootstrap, Issue, Session } from '../types';
import { selectable } from '../workflows/model';
import { useIssueWorkflow } from '../workflows/useIssueWorkflow';
import { relationNames, type IssueMovePreview, type MoveDestination, type MoveIssueBody, type Page } from './types';

const audienceText: Record<IssueMovePreview['audience_change'], string> = {
  none: 'This issue stays visible to the workspace.',
  public_to_private: 'This issue will only be visible to people with access to the destination private team.',
  private_to_public: 'This issue will become visible to the workspace. Its title, description, and current fields will be visible; earlier private-team activity remains restricted.',
  between_private_teams: 'Access changes to the destination private team. People who only belong to the current team will lose access.',
};

export function MoveIssue({ workspaceId, issue, data, session, close, moved, reviewed }: { workspaceId: string; issue: Issue; data: Bootstrap; session: Session; close: () => void; moved: (issue: Issue) => void; reviewed: (issue: Issue) => void }) {
  const sessionScope = useSessionScope(session);
  const request = useScopedRequest(); const [destination, setDestination] = useState<MoveDestination | null>(null); const [open, setOpen] = useState(false); const [query, setQuery] = useState('');
  const directory = useSearchPage<MoveDestination, Page<MoveDestination>>(`/workspaces/${workspaceId}/issues/${issue.id}/move-destinations`, sessionScope, query, open);
  const [source, setSource] = useState(issue); const [preview, setPreview] = useState<IssueMovePreview | null>(null); const [draft, setDraft] = useState(toDraft(issue)); const [chosenAssignee, setChosenAssignee] = useState<AssignableUser | null>(null); const [chosenLabels, setChosenLabels] = useState<LabelSummary[]>([]);
  const [audienceAccepted, setAudienceAccepted] = useState(false); const [labelsAccepted, setLabelsAccepted] = useState(false); const [revision, setRevision] = useState(0); const [loadedDestination, setLoadedDestination] = useState('');
  const [loading, setLoading] = useState(false); const [busy, setBusy] = useState(false); const [error, setError] = useState<unknown>(null); const [needsReview, setNeedsReview] = useState(false); const [uncertain, setUncertain] = useState(false); const [expired, setExpired] = useState(false);
  const pending = useRef<{ body: MoveIssueBody; etag: string; key: string } | null>(null);
  const workflow = useIssueWorkflow(workspaceId, destination?.id ?? '', session, data, destination ? draft.status_id : undefined);
  useEffect(() => {
    const abort = new AbortController(); setPreview(null); setError(null); setNeedsReview(false); setExpired(false);
    if (!destination) return () => abort.abort();
    setLoading(true); setAudienceAccepted(false); setLabelsAccepted(false);
    Promise.all([request<IssueMovePreview>(`/workspaces/${workspaceId}/issues/${issue.id}/move-preview?destination_team_id=${encodeURIComponent(destination.id)}`, { signal: abort.signal }), request<Issue>(`/workspaces/${workspaceId}/issues/${issue.id}`, { signal: abort.signal })]).then(([next, current]) => {
      if (abort.signal.aborted) return;
      if (next.issue_etag !== current.etag || next.source_team_id !== current.team_id) throw new ApiError(409, 'preview_changed', 'This issue changed during review. Refresh the move review.');
      if (current.team_id !== issue.team_id || current.archived_at) throw new ApiError(409, 'issue_changed', 'This issue moved or was archived. Close this dialog and reload the issue.');
      setSource(current); setPreview(next); reviewed(current);
      if (loadedDestination !== destination.id) { setDraft({ ...toDraft(current), status_id: next.destination_default_status_id, label_ids: next.retained_label_ids }); setChosenAssignee(null); setChosenLabels([]); setLoadedDestination(destination.id); }
    }).catch(error => { if (!abort.signal.aborted) { setError(error); setNeedsReview(true); } }).finally(() => { if (!abort.signal.aborted) setLoading(false); });
    return () => abort.abort();
    // A refresh preserves repairs. Choosing a different destination starts its own review.
  }, [destination?.id, revision, workspaceId, issue.id, sessionScope]);
  useEffect(() => {
    if (!preview) return; const delay = Date.parse(preview.expires_at) - Date.now();
    if (delay <= 0) { setExpired(true); return; }
    const timer = setTimeout(() => setExpired(true), Math.min(delay, 2147483647)); return () => clearTimeout(timer);
  }, [preview]);
  const staleWorkflow = !!preview && !!workflow.data && workflow.data.version !== preview.destination_workflow_version;
  const validStatus = !!workflow.data && workflow.statuses.some(status => status.id === draft.status_id && workflow.currentIds.has(status.id) && selectable(status));
  const validAssignee = !draft.assignee_id || (draft.assignee_id === preview?.current_assignee?.id && preview.current_assignee.assignable) || chosenAssignee?.id === draft.assignee_id;
  const removedSharedLabels = preview?.retained_label_ids.filter(id => !draft.label_ids.includes(id)) ?? [];
  const sourceLabelsSelected = !!preview && draft.label_ids.some(id => preview.removed_label_ids.includes(id));
  const ready = !!preview && !preview.blockers.length && validStatus && validAssignee && !sourceLabelsSelected && !needsReview && !expired && !staleWorkflow && (!preview.removed_label_ids.length || labelsAccepted) && (preview.audience_change === 'none' || audienceAccepted);
  async function submit(event: FormEvent) {
    event.preventDefault(); if (busy || loading || (!uncertain && !ready)) return;
    if (!uncertain) { if (!preview || !destination) return; pending.current = { key: crypto.randomUUID(), etag: preview.issue_etag, body: { destination_team_id: destination.id, status_id: draft.status_id, assignee_id: draft.assignee_id, label_ids: [...draft.label_ids].sort(), preview_token: preview.preview_token } }; }
    if (!pending.current) return; setBusy(true); setError(null);
    try { const intent = pending.current; moved(await request<Issue>(`/workspaces/${workspaceId}/issues/${issue.id}/move`, { method: 'POST', body: intent.body, csrf: session.csrf_token, headers: { 'If-Match': intent.etag, 'Idempotency-Key': intent.key } })); }
    catch (error) { setError(error); const ambiguous = error instanceof ApiError && ([0, 408].includes(error.status) || error.status >= 500); setUncertain(ambiguous); if (!ambiguous) setNeedsReview(true); }
    finally { setBusy(false); }
  }
  const referenceIssue = preview ? { ...source, assignee: chosenAssignee?.id === draft.assignee_id ? { ...chosenAssignee, assignable: true } : preview.current_assignee, labels: [...source.labels.filter(label => !label.team_id), ...chosenLabels.filter(label => !source.labels.some(original => original.id === label.id))] } : source;
  return <Modal title={`Move ${issue.key} to another team`} close={close} canClose={!busy && !uncertain}><form className="settings-modal-content move-issue" onSubmit={submit}>
    <p>The issue keeps its history and receives a new team key after the move. Review its destination fields first.</p><ErrorBanner error={error} /><UncertainNotice show={uncertain} />
    <Picker label="Destination team" value={destination ? <>{destination.private && <LockKeyhole aria-hidden="true" />}{destination.name} · {destination.key}</> : 'Choose a team'} open={open} setOpen={setOpen} query={query} setQuery={setQuery} choices={directory.items.map(team => ({ id: team.id, text: team.name, detail: `${team.key}${team.private ? ' · Private team' : ''}`, selected: team.id === destination?.id, disabled: !team.can_move_issue }))} select={id => { const next = directory.items.find(team => team.id === id); if (next && next.id !== destination?.id) { setDestination(next); setChosenAssignee(null); } }} loading={directory.loading} error={directory.error} refresh={directory.refresh} more={directory.more} hasMore={!!directory.page?.has_more} disabled={busy || uncertain || loading} />
    {loading && <Loading label="Reviewing destination…" />}
    {!uncertain && (needsReview || expired || staleWorkflow) && <div className="workflow-conflict" role="alert"><strong>Refresh the move review.</strong><p>{expired ? 'This review expired.' : staleWorkflow ? 'The destination workflow changed.' : 'The issue or its destination changed.'} Your selected field values are preserved. Review the updated access and label changes before confirming.</p><button type="button" className="button compact" disabled={busy || loading} onClick={() => { workflow.refresh(); setRevision(value => value + 1); }}>Refresh move review</button></div>}
    {preview && <>
      {preview.blockers.length > 0 ? <div className="move-blockers" role="status"><strong>Resolve connections before moving.</strong><p>This issue still has: {preview.blockers.map(kind => kind === 'parent' ? 'a parent' : kind === 'children' ? 'subissues' : relationNames[kind].toLowerCase()).join(', ')}. Detach or remove these links explicitly, then review the move again.</p><button type="button" className="button compact" disabled={busy || uncertain} onClick={close}>Review connections</button></div> : <>
        <ErrorBanner error={workflow.error} retry={workflow.refresh} />
        <IssueFields workspaceId={workspaceId} session={session} draft={draft} change={setDraft} data={data} teamId={preview.destination.id} issue={referenceIssue} statuses={workflow.statuses} currentStatusIds={workflow.currentIds} repairOnly rememberedAssignee={setChosenAssignee} rememberedLabels={labels => setChosenLabels(old => [...old.filter(item => !labels.some(label => label.id === item.id)), ...labels])} disabled={busy || uncertain || loading} />
        {!validAssignee && <p className="reference-note">The current assignee cannot be assigned in this team. Choose an eligible person or Unassigned.</p>}
        {preview.removed_label_ids.length > 0 && <label className="move-acknowledgement"><input type="checkbox" checked={labelsAccepted} onChange={event => setLabelsAccepted(event.target.checked)} disabled={busy || uncertain} /><span>Remove labels belonging to the current team: {preview.removed_label_ids.map(id => source.labels.find(label => label.id === id)?.name ?? 'Unavailable label').join(', ')}.</span></label>}
        {removedSharedLabels.length > 0 && <p className="reference-note">Also removing shared labels: {removedSharedLabels.map(id => source.labels.find(label => label.id === id)?.name ?? 'Unavailable label').join(', ')}.</p>}
        {sourceLabelsSelected && <p className="reference-note">Remove the current team’s labels from your selection before moving.</p>}
        <div className="move-audience"><strong>Who can see this issue</strong><p>{audienceText[preview.audience_change]}</p>{preview.audience_change !== 'none' && <label className="move-acknowledgement"><input type="checkbox" checked={audienceAccepted} onChange={event => setAudienceAccepted(event.target.checked)} disabled={busy || uncertain} /><span>I have reviewed this access change.</span></label>}</div>
      </>}
    </>}
    <div className="modal-footer"><button type="button" className="button" disabled={busy || uncertain} onClick={close}>Cancel</button><button className="button primary" disabled={busy || loading || (!uncertain && !ready)}>{busy ? 'Moving…' : uncertain ? 'Retry safely' : 'Confirm move'}</button></div>
  </form></Modal>;
}
