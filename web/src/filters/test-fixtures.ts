import type { Bootstrap, Issue, Session, Status } from '../types';
export const id = (number: number) => `00000000-0000-4000-8000-${number.toString(16).padStart(12, '0')}`;
export const workspaceId = id(1); export const teamId = id(2);
export const session: Session = { audience: 'web', user: { id: id(3), display_name: 'Example Owner', email: 'owner@example.test' }, csrf_token: 'filter-test-csrf', workspaces: [{ id: workspaceId, name: 'Filter workspace', slug: 'filters', role: 'owner' }] };
export const status: Status = { id: id(4), team_id: teamId, name: 'Ready', category: 'unstarted', color: 'slate', position: 0, is_default: true, archived_at: null, retiring_at: null, replacement_status_id: null, replacement_job_id: null };
export const issue: Issue = { id: id(5), workspace_id: workspaceId, team_id: teamId, key: 'ENG-1', title: 'A matching issue', description: '', status_id: status.id, priority: 1, assignee_id: null, assignee: null, label_ids: [], labels: [], estimate: null, due_date: null, version: '1', etag: '"issue:filter:1"', created_at: '2026-11-08T10:00:00Z', updated_at: '2026-11-08T10:00:00Z', archived_at: null };
export const data: Bootstrap = { auth_epoch: '1', teams: [{ id: teamId, key: 'ENG', name: 'Engineering', private: false, archived_at: null }, { id: id(6), key: 'OPS', name: 'Operations', private: false, archived_at: null }], statuses: [status], members: [session.user], issues: [] };
export const json = (body: unknown, status = 200) => new Response(JSON.stringify(body), { status });
