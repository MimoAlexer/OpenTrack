import { categoryOrder } from '../types';
import type { Workflow, WorkflowStatus } from './types';

export const categoryNames: Record<string, string> = { triage: 'Triage', backlog: 'Backlog', unstarted: 'Not started', started: 'In progress', completed: 'Completed', canceled: 'Canceled' };
export const statusColors = ['gray', 'slate', 'red', 'orange', 'amber', 'green', 'teal', 'blue', 'indigo', 'purple', 'pink'] as const;
export function workflowETag(workflow: Pick<Workflow, 'team_id' | 'version' | 'can_manage'>) { return `"workflow:${workflow.team_id}:${workflow.version}:${workflow.can_manage}"`; }
export function selectable(status: Pick<WorkflowStatus, 'archived_at' | 'retiring_at'>) { return !status.archived_at && !status.retiring_at; }
export function ordered(statuses: WorkflowStatus[]) {
  return [...statuses].sort((a, b) => categoryOrder.indexOf(a.category) - categoryOrder.indexOf(b.category) || a.position - b.position || a.id.localeCompare(b.id));
}
export function defaultStatus(statuses: WorkflowStatus[]) { return statuses.find(status => status.is_default && selectable(status)); }
export function statusLabel(status: WorkflowStatus) { return `${status.name}${status.archived_at ? ' · Archived' : status.retiring_at ? ' · Being replaced' : ''}`; }
export function moveStatus(ids: string[], statuses: WorkflowStatus[], id: string, delta: -1 | 1): string[] {
  const index = ids.indexOf(id); const next = index + delta;
  if (index < 0 || next < 0 || next >= ids.length) return ids;
  if (statuses.find(status => status.id === id)?.category !== statuses.find(status => status.id === ids[next])?.category) return ids;
  const result = [...ids]; [result[index], result[next]] = [result[next], result[index]]; return result;
}
export function rebaseOrder(ids: string[], latest: WorkflowStatus[]): string[] {
  // Explicit review keeps the order of surviving IDs, adding new rows at the
  // end of their fixed category rather than silently omitting them on save.
  return categoryOrder.flatMap(category => {
    const members = ordered(latest.filter(status => status.category === category));
    return [...ids.filter(id => members.some(status => status.id === id)), ...members.filter(status => !ids.includes(status.id)).map(status => status.id)];
  });
}
export function mergeStatuses(previous: WorkflowStatus[], incoming: WorkflowStatus[]): WorkflowStatus[] {
  const changed = new Map(incoming.map(status => [status.id, status]));
  return [...previous.map(status => changed.get(status.id) ?? status), ...incoming.filter(status => !previous.some(old => old.id === status.id))];
}
