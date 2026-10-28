import { DocumentView } from '../documents/DocumentView';
import { priorities } from '../types';
import { templateBody } from './model';
import type { IssueTemplate, TemplateDiagnostic, TemplatePreview as Preview } from './types';

export function diagnosticMessage(item: TemplateDiagnostic) {
  if (item.field === 'title') return ({
    malformed_pattern: 'The title pattern has unpaired braces. Edit the template or enter an issue title.',
    unknown_placeholder: 'The title pattern uses an unsupported placeholder. Only {{team.name}}, {{team.key}} and {{date}} are supported.',
    title_too_long: 'The resolved title exceeds 256 characters. Shorten the pattern or enter an issue title.',
    invalid_title: 'The resolved title contains unsupported characters. Edit the template or enter an issue title.',
  } as Record<string, string>)[item.code] ?? 'The title could not be resolved. Enter an issue title before creating.';
  const field = item.field === 'status_id' ? 'Status' : item.field === 'label_ids' ? 'A label' : item.field === 'project_id' ? 'Project' : 'Description';
  return item.field === 'document' ? 'This description uses a format this version cannot apply.' : `${field} is ${item.code === 'unavailable' ? 'no longer available' : item.code}. Choose a replacement${item.field === 'label_ids' ? ' or remove it' : ''} before creating the issue.`;
}
export function TemplateDiagnostics({ diagnostics }: { diagnostics: TemplateDiagnostic[] }) {
  return diagnostics.length ? <ul className="template-diagnostics" aria-label="Template fields needing attention">{diagnostics.map((item, index) => <li key={`${item.field}:${item.reference_id ?? index}`}>{diagnosticMessage(item)}</li>)}</ul> : null;
}
export function supportedDefinition(template: IssueTemplate) {
  if (!template.document.schema_supported) return false;
  try { templateBody(template.document.body); return true; } catch { return false; }
}
export function supportedTemplate(preview: Preview) { return supportedDefinition(preview.template); }
export function previewProject(value: Preview, workspaceId: string) {
  const project = value.resolved_project;
  return project && project.workspace_id === workspaceId && project.team_id === value.template.team_id && project.id === value.template.defaults.project_id ? project : null;
}
export function TemplatePreviewContent({ value, workspaceId }: { value: Preview; workspaceId: string }) {
  const item = value.template; const supported = supportedTemplate(value);
  return <section className="template-preview" aria-label="Template preview"><header><h3>{item.name}</h3><span className="muted">Version {item.version}{item.archived_at ? ' · Archived' : ''}</span></header><h4>{value.materialized_title === null ? 'Title needs attention' : value.materialized_title || 'No title provided'}</h4>{item.title_mode === 'pattern' && <p className="muted">Pattern: <code>{item.title}</code><br/>Resolved at {value.title_context_at} (UTC). Applying keeps this reviewed title; you can edit it before creating.</p>}<dl className="template-defaults"><div><dt>Status</dt><dd>{value.resolved_status?.name ?? 'Status unavailable'}{!item.defaults.status_id && ' · Team default'}</dd></div><div><dt>Priority</dt><dd>{priorities[item.defaults.priority] ?? 'Unavailable'}</dd></div><div><dt>Labels</dt><dd>{value.labels.length ? value.labels.map(label => label.name).join(', ') : 'None'}</dd></div><div><dt>Project</dt><dd>{previewProject(value, workspaceId)?.name ?? (item.defaults.project_id || value.diagnostics.some(diagnostic => diagnostic.field === 'project_id') ? 'Project unavailable' : 'None')}</dd></div></dl><TemplateDiagnostics diagnostics={value.diagnostics}/>{!supported && <p role="alert" className="settings-info">This template contains unsupported content and cannot be applied or edited here. Its saved content has not changed.</p>}<DocumentView body={item.document.body} fallback={item.document.plain_text} supported={supported}/></section>;
}
