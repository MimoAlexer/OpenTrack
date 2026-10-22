import { attachmentIds, canonicalBody, hasContent, mentionIds, type RichBody, type RichNode } from '../documents/model';

export type TemplateFields = { title: string; body: RichBody; status_id: string | null; priority: number; label_ids: string[] };
export type TemplateField = keyof TemplateFields;

export function templateBody(value: unknown): RichBody {
  const body = canonicalBody(value, true);
  if (attachmentIds(body).length) throw new Error('Reusable templates cannot contain attachments yet. Keep files on the issue draft.');
  if (mentionIds(body).length) throw new Error('Reusable templates cannot contain mentions. Mention people on the issue draft.');
  return body;
}

function retainedFiles(body: RichNode): RichNode[] {
  return body.type === 'attachment' ? [{ type: 'attachment', attrs: { id: body.attrs!.id } }] : (body.content ?? []).flatMap(retainedFiles);
}

/** Only template-owned fields change; the caller keeps assignee, estimate and due date. */
export function applyTemplate(current: TemplateFields, defaults: TemplateFields, mode: 'merge' | 'replace', edited: ReadonlySet<TemplateField>): TemplateFields {
  const incoming = templateBody(defaults.body);
  const merge = mode === 'merge';
  const labels = [...new Set(merge ? [...current.label_ids, ...defaults.label_ids] : defaults.label_ids)];
  if (labels.length > 50) throw new Error('The combined draft would have more than 50 labels. Remove some labels or replace the defaults.');
  const content = merge && hasContent(current.body)
    ? [...current.body.content, ...(hasContent(incoming) ? incoming.content : [])]
    : [...incoming.content, ...retainedFiles(current.body)];
  return {
    title: merge && (edited.has('title') || current.title.trim()) ? current.title : defaults.title,
    body: canonicalBody({ type: 'doc', content }),
    status_id: merge && edited.has('status_id') ? current.status_id : defaults.status_id,
    priority: merge && edited.has('priority') ? current.priority : defaults.priority,
    label_ids: labels,
  };
}
