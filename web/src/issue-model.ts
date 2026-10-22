import type { Issue, IssueDraft, IssuePatch } from './types';

export function toDraft(issue: Issue): IssueDraft {
  return { title: issue.title, description: issue.description, status_id: issue.status_id, priority: issue.priority,
    assignee_id: issue.assignee_id, label_ids: [...issue.label_ids].sort(), estimate: issue.estimate, due_date: issue.due_date };
}

export function sameIds(a: string[], b: string[]): boolean { return [...new Set(a)].sort().join(',') === [...new Set(b)].sort().join(','); }

export function changes(base: IssueDraft, draft: IssueDraft): IssuePatch {
  return Object.fromEntries(Object.entries(draft).filter(([key, value]) => key !== 'description' && (key === 'label_ids' ? !sameIds(base.label_ids, value as string[]) : base[key as keyof IssueDraft] !== value))) as IssuePatch;
}

// Preserve only the user's changed fields. Untouched fields take the latest server value.
export function rebaseDraft(original: Issue, draft: IssueDraft, latest: Issue): IssueDraft {
  return { ...toDraft(latest), ...changes(toDraft(original), draft) };
}

export function initials(name: string): string {
  return name.trim().split(/\s+/).slice(0, 2).map(s => s[0] ?? '').join('').toUpperCase() || '?';
}

export function formatDate(value: string): string {
  const date = new Date(value.includes('T') ? value : `${value}T12:00:00`);
  return Number.isNaN(date.getTime()) ? value : date.toLocaleDateString(undefined, { month: 'short', day: 'numeric' });
}
