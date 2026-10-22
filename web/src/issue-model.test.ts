import { describe, expect, it } from 'vitest';
import { changes, rebaseDraft, toDraft } from './issue-model';
import type { Issue } from './types';

const original: Issue = { id: 'issue-1', workspace_id: 'workspace-1', team_id: 'team-1', key: 'ENG-1', title: 'Original title', description: 'Original description', status_id: 'todo', priority: 0, assignee_id: null, assignee: null, label_ids: [], labels: [], etag: '"issue:fixture:1:projection"', estimate: null, due_date: null, version: '1', created_at: '2026-10-17T10:00:00Z', updated_at: '2026-10-17T10:00:00Z', archived_at: null };

describe('versioned issue edits', () => {
  it('sends only fields changed by the user, including an explicit clear', () => {
    const base = toDraft({ ...original, assignee_id: 'person-1' });
    expect(changes(base, { ...base, title: 'Changed', assignee_id: null })).toEqual({ title: 'Changed', assignee_id: null });
  });
  it('retains server changes to fields that the user did not edit during conflict resolution', () => {
    const latest = { ...original, title: 'Another editor’s title', priority: 2, version: '2' };
    const draft = { ...toDraft(original), estimate: 5 };
    expect(rebaseDraft(original, draft, latest)).toEqual({ ...toDraft(latest), estimate: 5 });
  });
  it('keeps the selected user edit when both editors changed the same field', () => {
    const latest = { ...original, title: 'Server title', version: '2' };
    const reviewed = rebaseDraft(original, { ...toDraft(original), title: 'My title' }, latest);
    expect(changes(toDraft(latest), reviewed)).toEqual({ title: 'My title' });
  });
  it('treats label IDs as a set so reordered references do not create spurious edits', () => {
    const base = toDraft({ ...original, label_ids: ['a', 'b'] });
    expect(changes(base, { ...base, label_ids: ['b', 'a'] })).toEqual({});
    expect(changes(base, { ...base, label_ids: ['b'] })).toEqual({ label_ids: ['b'] });
  });
  it('keeps the selected label set while accepting untouched remote fields during rebase', () => {
    const base = { ...original, label_ids: ['a'] };
    const latest = { ...base, title: 'Remote title', label_ids: ['a', 'remote'] };
    const rebased = rebaseDraft(base, { ...toDraft(base), label_ids: ['mine'] }, latest);
    expect(rebased.label_ids).toEqual(['mine']); expect(rebased.title).toBe('Remote title');
  });
  it('does not produce a mutation for an unchanged draft', () => {
    expect(changes(toDraft(original), toDraft(original))).toEqual({});
  });
  it('never flattens the structured document through an issue metadata patch', () => {
    expect(changes(toDraft(original), { ...toDraft(original), description: 'Stale plain text' })).toEqual({});
  });
});
