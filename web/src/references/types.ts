import type { components } from '../generated/api';
import type { MentionTarget } from '../mentions/types';
export type IssueReference = components['schemas']['IssueReference'];
export type IssueReferenceCandidate = components['schemas']['IssueReferenceCandidate'];
export type ReferenceTarget = MentionTarget;
export type ReferencePage = { data: IssueReferenceCandidate[]; has_more: boolean; next_cursor: string | null };
