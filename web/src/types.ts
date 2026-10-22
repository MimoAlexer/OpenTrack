import type { components } from './generated/api';

export type User = components['schemas']['User'];
export type Workspace = components['schemas']['Workspace'];
export type Session = components['schemas']['Session'];
export type RecoveryComplete = components['schemas']['RecoveryComplete'];
export type Team = components['schemas']['Team'];
export type Status = components['schemas']['Status'];
export type Issue = components['schemas']['Issue'];
export type Bootstrap = components['schemas']['Bootstrap'];
export type IssuePage = components['schemas']['Page_Issue'];
export type ActivityEntry = components['schemas']['ActivityEntry'];
export type IssueDraft = Pick<Issue, 'title' | 'description' | 'status_id' | 'priority' | 'assignee_id' | 'estimate' | 'due_date' | 'label_ids'>;
export type IssuePatch = Partial<Omit<IssueDraft, 'description'>>;
export const priorities = ['No priority', 'Urgent', 'High', 'Normal', 'Low'] as const;
export const categoryOrder = ['triage', 'backlog', 'unstarted', 'started', 'completed', 'canceled'];

export type NativeAuthConsent = components['schemas']['NativeAuthConsent'];
export type NativeAuthApproval = components['schemas']['NativeAuthApproval'];
