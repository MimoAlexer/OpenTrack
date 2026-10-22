import type { components } from '../generated/api';
type Schemas = components['schemas'];
export type MentionCandidate = Schemas['MentionCandidate'];
export type MentionReference = Schemas['MentionReference'];
export type IssueSubscription = Schemas['IssueSubscription'];
export type SubscriptionPreference = IssueSubscription['preference'];
export type MentionTarget = { team_id: string; issue_id?: never; document_id?: never } | { issue_id: string; team_id?: never; document_id?: never } | { document_id: string; issue_id?: never; team_id?: never };
export type MentionPage = Schemas['Page_MentionCandidate'];
