import { describe, expect, it } from 'vitest';
import type { ActivityEntry } from '../types';
import { activityFacts, activityFields, activitySummary, groupActivity } from './activity-model';

const entry = (id: number, extra: Partial<ActivityEntry> = {}): ActivityEntry => ({
  id: `event-${id}`, actor: 'Example Member', actor_id: 'member-1', event_kind: 'issue.updated',
  created_at: new Date(Date.UTC(2026, 9, 20, 12, 0, id * 10)).toISOString(),
  changes: { source: 'manual', fields: ['priority'], priority: { before: 0, after: 2 } }, ...extra,
});

describe('activity facts', () => {
  it('renders known before/after facts and nullable values without content or identifiers', () => {
    const item = entry(0, { changes: {
      source: 'manual', fields: ['title', 'description', 'status_id', 'priority', 'estimate', 'due_date', 'archived', 'assignee_id', 'label_ids'],
      status_names: { before: 'Backlog', after: 'In progress' }, priority: { before: 0, after: 2 },
      estimate: { before: null, after: 5 }, due_date: { before: '2026-11-01', after: null }, archived: { before: false, after: true },
      assignee_id: { before: 'former-account-id', after: 'new-account-id' }, label_ids: { before: ['old-label', 'kept-label'], after: ['new-label', 'kept-label'] },
    } });
    expect(activityFacts(item)).toEqual([
      { label: 'Status', value: 'Backlog → In progress' }, { label: 'Priority', value: 'No priority → High' },
      { label: 'Estimate', value: 'Not set → 5' }, { label: 'Due date', value: '2026-11-01 → Not set' },
      { label: 'Archive state', value: 'Active → Archived' }, { label: 'Assignee', value: 'Changed to another assignee' },
      { label: 'Labels', value: '1 added, 1 removed' },
    ]);
  });
  it('keeps old records understandable without inventing before/after facts', () => {
    const legacy = entry(0, { changes: { fields: ['title', 'title', 'status_id', 'unknown'] } });
    expect(activityFields(legacy)).toEqual(['title', 'status_id']);
    expect(activitySummary(legacy)).toBe('changed title and status'); expect(activityFacts(legacy)).toEqual([]);
    const replaced = entry(0, { changes: { source: 'workflow_replacement', status_id: { before: 's1', after: 's2' } } });
    expect(activityFields(replaced)).toEqual(['status_id']);
    expect(activitySummary(replaced)).toBe('changed status through workflow replacement');
  });
  it('uses the recorded archive direction and separates system cleanup', () => {
    expect(activitySummary(entry(0, { changes: { fields: ['archived'], archived: { before: true, after: false } } }))).toBe('unarchived this issue');
    const cleanup = entry(0, { actor: 'OpenTrack', actor_id: null, changes: { source: 'access_cleanup', assignee_id: { before: 'former-account-id', after: null } } });
    expect(activitySummary(cleanup)).toContain('no longer have access'); expect(activityFacts(cleanup)).toEqual([]);
  });
  it('describes a manual move from the projected source without inventing position facts', () => {
    for (const fields of [[], ['position']]) {
      const move = entry(1, { changes: { source: 'manual_order', fields } });
      expect(activityFields(move)).toEqual(['position']); expect(activitySummary(move)).toBe('changed position'); expect(activityFacts(move)).toEqual([]);
      expect(groupActivity([entry(0), move, entry(2)]).map(group => group.entries.length)).toEqual([1, 1, 1]);
    }
  });
  it('retains captured status facts for a manual move while keeping it separate from low-value edits', () => {
    const move = entry(1, { changes: { source: 'manual_order', fields: ['status_id'], status_id: { before: 'old-status-id', after: 'new-status-id' }, status_names: { before: 'Backlog', after: 'In progress' } } });
    expect(activityFields(move)).toEqual(['status_id', 'position']); expect(activitySummary(move)).toBe('changed status and position');
    expect(activityFacts(move)).toEqual([{ label: 'Status', value: 'Backlog → In progress' }]);
    expect(groupActivity([entry(0), move, entry(2)]).map(group => group.entries.length)).toEqual([1, 1, 1]);
  });
});

describe('bounded activity groups', () => {
  it('groups up to five adjacent low-value manual changes without dropping any event', () => {
    const entries = Array.from({ length: 7 }, (_, id) => entry(id)); const groups = groupActivity(entries);
    expect(groups.map(group => group.entries.length)).toEqual([5, 2]);
    expect(groups.flatMap(group => group.entries)).toEqual(entries);
  });
  it('bounds the entire group to two minutes rather than extending with every event', () => {
    expect(groupActivity([entry(0), entry(12), entry(13)]).map(group => group.entries.length)).toEqual([2, 1]);
  });
  it.each([
    { actor_id: 'member-2' }, { actor_id: null }, { actor_id: undefined },
    { event_kind: 'comment.created' }, { changes: { source: 'workflow_replacement', fields: ['priority'] } },
    { changes: { source: 'access_cleanup', fields: ['priority'] } }, { changes: { fields: ['priority'] } },
    { changes: { source: 'manual', fields: ['status_id'] } }, { changes: { source: 'manual', fields: ['priority', 'title'] } },
    { changes: { source: 'manual', fields: ['priority', 'unknown'] } },
    { changes: { source: 'manual', fields: [] } }, { created_at: 'invalid' },
  ] as Partial<ActivityEntry>[])('keeps important, unattributed or differently attributed events separate: %j', barrier => {
    expect(groupActivity([entry(0), entry(1, barrier), entry(2)]).map(group => group.entries.length)).toEqual([1, 1, 1]);
  });
  it('does not merge reversed timestamps or changed actor labels', () => {
    expect(groupActivity([entry(2), entry(1)]).length).toBe(2);
    expect(groupActivity([entry(0), entry(1, { actor: 'Example Owner' })]).length).toBe(2);
  });
});
