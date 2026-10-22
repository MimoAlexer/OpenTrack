import { priorities, type ActivityEntry } from '../types';

const labels: Record<string, string> = { title: 'title', description: 'description', status_id: 'status', priority: 'priority', assignee_id: 'assignee', label_ids: 'labels', estimate: 'estimate', due_date: 'due date', archived: 'archive state' };
const lowValueFields = new Set(['priority', 'label_ids', 'estimate', 'due_date']);

export function activityFields(entry: ActivityEntry): string[] {
  if (Array.isArray(entry.changes.fields)) return [...new Set(entry.changes.fields.filter(field => typeof field === 'string' && (Object.hasOwn(labels, field) || ['parent', 'relations'].includes(field))))];
  return ['status_id', 'priority', 'assignee_id', 'label_ids', 'estimate', 'due_date', 'archived'].filter(field => {
    const value = entry.changes[field as keyof ActivityEntry['changes']];
    return value && typeof value === 'object' && 'before' in value && 'after' in value && JSON.stringify(value.before) !== JSON.stringify(value.after);
  });
}

export function activitySummary(entry: ActivityEntry): string {
  if (entry.event_kind === 'document.updated') return 'updated the description';
  if (entry.event_kind === 'comment.created') return 'posted a comment';
  if (entry.event_kind === 'comment.updated') return 'edited a comment';
  if (entry.event_kind === 'comment.deleted') return 'deleted a comment';
  if (entry.event_kind === 'issue.created') return 'created this issue';
  if (entry.event_kind === 'issue.deleted') return 'moved this issue to Trash';
  if (entry.event_kind === 'issue.restored') return 'restored this issue from Trash';
  if (entry.event_kind === 'issue.parent_changed' && entry.changes.action === 'parent_detached_after_deletion') return activityFields(entry).includes('relations') ? 'removed hierarchy and relationship connections after a linked issue was deleted' : 'detached a parent or subissue connection after a linked issue was deleted';
  if (entry.event_kind === 'issue.relation_removed' && entry.changes.action === 'relation_removed_after_deletion') return 'removed a relationship after a linked issue was deleted';
  if (entry.event_kind === 'issue.updated') {
    if (entry.changes.source === 'access_cleanup') return 'cleared the assignee because they no longer have access';
    if (entry.changes.source === 'workflow_replacement') return 'changed status through workflow replacement';
    const fields = activityFields(entry);
    if (fields.length === 1 && fields[0] === 'archived' && entry.changes.archived) return entry.changes.archived.after ? 'archived this issue' : 'unarchived this issue';
    if (!fields.length) return 'updated this issue';
    return `changed ${new Intl.ListFormat(undefined, { style: 'long', type: 'conjunction' }).format(fields.map(field => labels[field] ?? field.replaceAll('_', ' ')))}`;
  }
  return entry.event_kind.replaceAll('.', ' · ').replaceAll('_', ' ');
}

export function activityFacts(entry: ActivityEntry): { label: string; value: string }[] {
  if (entry.event_kind !== 'issue.updated' || entry.changes.source === 'access_cleanup') return [];
  const changes = entry.changes; const fields = new Set(activityFields(entry));
  const facts: { label: string; value: string }[] = [];
  if (fields.has('status_id') && changes.status_names) facts.push({ label: 'Status', value: `${changes.status_names.before} → ${changes.status_names.after}` });
  if (fields.has('priority') && changes.priority) facts.push({ label: 'Priority', value: `${priorities[changes.priority.before] ?? 'Unknown priority'} → ${priorities[changes.priority.after] ?? 'Unknown priority'}` });
  if (fields.has('estimate') && changes.estimate) facts.push({ label: 'Estimate', value: `${changes.estimate.before ?? 'Not set'} → ${changes.estimate.after ?? 'Not set'}` });
  if (fields.has('due_date') && changes.due_date) facts.push({ label: 'Due date', value: `${changes.due_date.before ?? 'Not set'} → ${changes.due_date.after ?? 'Not set'}` });
  if (fields.has('archived') && changes.archived) facts.push({ label: 'Archive state', value: `${changes.archived.before ? 'Archived' : 'Active'} → ${changes.archived.after ? 'Archived' : 'Active'}` });
  if (fields.has('assignee_id') && changes.assignee_id) {
    const { before, after } = changes.assignee_id;
    facts.push({ label: 'Assignee', value: before && after ? 'Changed to another assignee' : `${before ? 'Assigned' : 'Unassigned'} → ${after ? 'Assigned' : 'Unassigned'}` });
  }
  if (fields.has('label_ids') && changes.label_ids) {
    const before = new Set(changes.label_ids.before); const after = new Set(changes.label_ids.after);
    const added = [...after].filter(id => !before.has(id)).length; const removed = [...before].filter(id => !after.has(id)).length;
    facts.push({ label: 'Labels', value: `${added} added, ${removed} removed` });
  }
  return facts;
}

export type ActivityGroup = { id: string; entries: ActivityEntry[] };
export function groupActivity(entries: ActivityEntry[]): ActivityGroup[] {
  const eligible = (entry: ActivityEntry) => {
    const fields = entry.changes.fields ?? activityFields(entry);
    return entry.event_kind === 'issue.updated' && !!entry.actor_id && entry.changes.source === 'manual' && fields.length > 0 && fields.every(field => lowValueFields.has(field));
  };
  const groups: ActivityGroup[] = [];
  for (const entry of entries) {
    const group = groups.at(-1); const first = group?.entries[0]; const previous = group?.entries.at(-1);
    const time = Date.parse(entry.created_at);
    if (group && first && previous && group.entries.length < 5 && eligible(entry) && eligible(first) && entry.actor_id === first.actor_id && entry.actor === first.actor && Number.isFinite(time) && time >= Date.parse(previous.created_at) && time - Date.parse(first.created_at) <= 120_000) group.entries.push(entry);
    else groups.push({ id: entry.id, entries: [entry] });
  }
  return groups;
}
