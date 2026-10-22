import type { components } from '../generated/api';
type Schemas = components['schemas'];
export type IssueTemplateSummary = Schemas['IssueTemplateSummary'];
export type IssueTemplate = Schemas['IssueTemplate'];
export type TemplatePreview = Schemas['TemplatePreview'];
export type TemplateDiagnostic = Schemas['TemplateDiagnostic'];
export type TemplateSelection = Schemas['TemplateSelection'];
export type TemplatePage = { data: IssueTemplateSummary[]; next_cursor: string | null; has_more: boolean };
export const templatePath = (workspace: string, team: string) => `/workspaces/${workspace}/teams/${team}/templates`;
