import type { components } from '../generated/api';
export type LabelSummary = components['schemas']['LabelSummary'];
export type Label = components['schemas']['Label'];
export type AssignableUser = components['schemas']['AssignableUser'];
export type IssueAssignee = components['schemas']['IssueAssignee'];
export type AssigneePage = components['schemas']['AssigneePage'];
export type LabelPage = components['schemas']['Page_Label'];
export const labelETag = (label: Label) => `"label:${label.id}:${label.version}:${label.can_manage}"`;
