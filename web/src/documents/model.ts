export type RichMark = { type: 'bold' | 'italic' | 'strike' | 'code' | 'link'; attrs?: { href: string } };
export type RichNode = { type: string; text?: string; attrs?: Record<string, string | number | boolean | null>; marks?: RichMark[]; content?: RichNode[] };
export type RichBody = RichNode & { type: 'doc'; content: RichNode[] };
export const EMPTY_BODY: RichBody = { type: 'doc', content: [{ type: 'paragraph' }] };
export const MAX_DOCUMENT_BYTES = 256 * 1024;
const nodeTypes = new Set(['doc', 'paragraph', 'heading', 'text', 'hardBreak', 'bulletList', 'orderedList', 'listItem', 'taskList', 'taskItem', 'blockquote', 'codeBlock', 'attachment', 'userMention', 'issueReference']);
const markTypes = new Set(['bold', 'italic', 'strike', 'code', 'link']);
const encoder = new TextEncoder();

export function safeHref(value: unknown): value is string {
  if (typeof value !== 'string' || !value || encoder.encode(value).length > 2048 || /[\s\u0000-\u001f\u007f-\u009f\\]/u.test(value)) return false;
  try {
    const url = new URL(value);
    return !url.username && !url.password && ((['http:', 'https:'].includes(url.protocol) && !!url.hostname) || (url.protocol === 'mailto:' && !!url.pathname && !value.includes('?') && !value.includes('#')));
  } catch { return false; }
}

// Incoming documents are checked before Tiptap sees them. Outgoing editor JSON
// differs only in rendering defaults (link target/rel/class and ordered-list type).
export function canonicalBody(value: unknown, strict = false): RichBody {
  let count = 0; const attachments = new Set<string>(); const mentions = new Set<string>(); const references = new Set<string>();
  const invalid = () => new Error('This document has an unsupported structure.');
  const object = (value: unknown): Record<string, unknown> => {
    if (!value || typeof value !== 'object' || Array.isArray(value)) throw invalid();
    return value as Record<string, unknown>;
  };
  function node(raw: unknown, depth: number, parent: string): RichNode {
    if (++count > 10000 || depth > 32) throw new Error('This document is too complex. Split it into smaller sections.');
    const source = object(raw); const kind = source.type;
    if (Object.keys(source).some(key => !['type', 'text', 'attrs', 'marks', 'content'].includes(key))) throw invalid();
    if (typeof kind !== 'string' || !nodeTypes.has(kind)) throw new Error('This document uses formatting this version cannot edit.');
    const blocks = ['paragraph', 'heading', 'bulletList', 'orderedList', 'taskList', 'blockquote', 'codeBlock', 'attachment'];
    const childrenAllowed = parent === 'root' ? kind === 'doc' : ['paragraph', 'heading'].includes(parent) ? ['text', 'hardBreak', 'userMention', 'issueReference'].includes(kind) : parent === 'codeBlock' ? kind === 'text' : ['bulletList', 'orderedList'].includes(parent) ? kind === 'listItem' : parent === 'taskList' ? kind === 'taskItem' : ['doc', 'blockquote', 'listItem', 'taskItem'].includes(parent) && blocks.includes(kind);
    if (!childrenAllowed) throw invalid();
    const result: RichNode = { type: kind };
    const attrs = 'attrs' in source ? object(source.attrs) : {};
    const allowed = kind === 'heading' ? ['level'] : kind === 'orderedList' ? ['start', ...(!strict ? ['type'] : [])] : kind === 'taskItem' ? ['checked'] : kind === 'codeBlock' ? ['language'] : ['attachment', 'userMention', 'issueReference'].includes(kind) ? ['id'] : [];
    if (Object.keys(attrs).some(key => !allowed.includes(key))) throw new Error('This document has unsupported formatting attributes.');
    if (kind === 'heading') { if (typeof attrs.level !== 'number' || ![1, 2, 3].includes(attrs.level)) throw invalid(); result.attrs = { level: attrs.level }; }
    if (kind === 'orderedList') { const start = 'start' in attrs ? attrs.start : 1; if (typeof start !== 'number' || !Number.isInteger(start) || start < 1 || start > 2147483647) throw invalid(); result.attrs = { start }; }
    if (kind === 'taskItem') { if (typeof attrs.checked !== 'boolean') throw invalid(); result.attrs = { checked: attrs.checked }; }
    if (kind === 'codeBlock') { const language = attrs.language ?? null; if (language !== null && (typeof language !== 'string' || !/^[a-zA-Z0-9_+#-]{0,64}$/.test(language))) throw invalid(); result.attrs = { language }; }
    if (kind === 'attachment') { if (typeof attrs.id !== 'string' || !/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(attrs.id) || attachments.has(attrs.id.toLowerCase()) || attachments.size >= 100) throw new Error('A document can contain up to 100 different attachments, each only once.'); attachments.add(attrs.id.toLowerCase()); result.attrs = { id: attrs.id }; }
    if (kind === 'userMention') { if (typeof attrs.id !== 'string' || !/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(attrs.id)) throw invalid(); mentions.add(attrs.id.toLowerCase()); if (mentions.size > 50) throw new Error('A document can mention up to 50 different people.'); result.attrs = { id: attrs.id }; }
    if (kind === 'issueReference') { if (typeof attrs.id !== 'string' || !/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(attrs.id)) throw invalid(); references.add(attrs.id.toLowerCase()); if (references.size > 50) throw new Error('A document can reference up to 50 different issues.'); result.attrs = { id: attrs.id.toLowerCase() }; }
    if (kind === 'text') {
      if (typeof source.text !== 'string' || !source.text || source.text.includes('\0') || 'content' in source) throw invalid(); result.text = source.text;
    } else if ('text' in source || 'marks' in source) throw invalid();
    if ('marks' in source) {
      if (!Array.isArray(source.marks) || source.marks.length > 5 || parent === 'codeBlock') throw invalid();
      const seen = new Set<string>();
      const marks = source.marks.map(rawMark => {
        const mark = object(rawMark);
        if (Object.keys(mark).some(key => !['type', 'attrs'].includes(key)) || typeof mark.type !== 'string' || !markTypes.has(mark.type) || seen.has(mark.type)) throw invalid();
        seen.add(mark.type); const markAttrs = 'attrs' in mark ? object(mark.attrs) : {};
        if (mark.type === 'link') {
          const allowed = strict ? ['href'] : ['href', 'target', 'rel', 'class', 'title'];
          if (!safeHref(markAttrs.href) || Object.keys(markAttrs).some(key => !allowed.includes(key))) throw new Error('This document contains an unsupported link.');
          return { type: 'link' as const, attrs: { href: markAttrs.href } };
        }
        if (Object.keys(markAttrs).length) throw invalid();
        return { type: mark.type as RichMark['type'] };
      });
      if (marks.length) result.marks = marks;
    }
    if ('content' in source) {
      if (!Array.isArray(source.content) || ['text', 'hardBreak', 'attachment', 'userMention', 'issueReference'].includes(kind)) throw invalid();
      if (source.content.length) result.content = source.content.map(child => node(child, depth + 1, kind));
    }
    if (['bulletList', 'orderedList', 'taskList', 'listItem', 'taskItem', 'blockquote'].includes(kind) && !result.content?.length) throw invalid();
    if (['listItem', 'taskItem'].includes(kind) && result.content?.[0]?.type !== 'paragraph') throw invalid();
    return result;
  }
  const result = node(value, 1, 'root');
  // Both wire representations mean an empty document; ProseMirror needs a block.
  if (!result.content?.length) result.content = [{ type: 'paragraph' }];
  if (encoder.encode(JSON.stringify(value)).length > MAX_DOCUMENT_BYTES || encoder.encode(JSON.stringify(result)).length > MAX_DOCUMENT_BYTES) throw new Error('The document is too large. Keep it under 256 KiB.');
  return result as RichBody;
}

export function bodyKey(value: RichBody): string { return JSON.stringify(canonicalBody(value)); }
export function sameBody(a: RichBody, b: RichBody): boolean { return bodyKey(a) === bodyKey(b); }
export function plainText(body: RichNode): string {
  if (body.type === 'text') return body.text ?? '';
  if (body.type === 'hardBreak') return '\n';
  if (body.type === 'userMention') return '@someone';
  if (body.type === 'issueReference') return '#issue';
  const children = body.content ?? [];
  return children.map(plainText).join(['doc', 'bulletList', 'orderedList', 'taskList', 'listItem', 'taskItem', 'blockquote'].includes(body.type) ? '\n' : '');
}
export function attachmentIds(body: RichNode): string[] { return body.type === 'attachment' ? [String(body.attrs?.id)] : (body.content ?? []).flatMap(attachmentIds); }
export function mentionIds(body: RichNode): string[] { return body.type === 'userMention' ? [String(body.attrs?.id)] : (body.content ?? []).flatMap(mentionIds); }
export function issueReferenceIds(body: RichNode): string[] { return body.type === 'issueReference' ? [String(body.attrs?.id)] : (body.content ?? []).flatMap(issueReferenceIds); }
export function hasText(body: RichBody): boolean { return !!plainText(body).trim(); }
export function hasContent(body: RichBody): boolean { return hasText(body) || attachmentIds(body).length > 0; }
export function withoutAttachments(body: RichBody, ids: Set<string>): RichBody {
  const visit = (node: RichNode): RichNode => {
    if (!node.content) return { ...node };
    const content = node.content.filter(child => child.type !== 'attachment' || !ids.has(String(child.attrs?.id))).map(visit);
    if (!content.length && ['blockquote', 'listItem', 'taskItem'].includes(node.type)) content.push({ type: 'paragraph' });
    return { ...node, content };
  };
  return canonicalBody(visit(body));
}
export function fromPlainText(text: string): RichBody { return { type: 'doc', content: text.split(/\r?\n/).map(line => ({ type: 'paragraph', ...(line ? { content: [{ type: 'text', text: line }] } : {}) })) }; }
export function isAccessError(error: unknown): boolean { return !!error && typeof error === 'object' && 'status' in error && [401, 403, 404].includes(Number(error.status)); }

export function markdown(body: RichNode): string {
  if (body.type === 'userMention') return '@someone';
  if (body.type === 'issueReference') return '#issue';
  const children = (body.content ?? []).map(markdown);
  if (body.type === 'text') {
    let text = (body.text ?? '').replace(/[\\`*_{}\[\]<>]/g, '\\$&');
    for (const mark of body.marks ?? []) { if (mark.type === 'bold') text = `**${text}**`; else if (mark.type === 'italic') text = `*${text}*`; else if (mark.type === 'strike') text = `~~${text}~~`; else if (mark.type === 'code') text = `\`${(body.text ?? '').replaceAll('`', '\\`')}\``; else if (mark.type === 'link') text = `[${text}](${mark.attrs?.href?.replaceAll(')', '%29')})`; }
    return text;
  }
  if (body.type === 'hardBreak') return '  \n';
  if (body.type === 'heading') return `${'#'.repeat(Number(body.attrs?.level ?? 1))} ${children.join('')}`;
  if (body.type === 'codeBlock') { const text = plainText(body); const fence = '`'.repeat(Math.max(3, ...[...text.matchAll(/`+/g)].map(match => match[0].length + 1))); return `${fence}${body.attrs?.language ?? ''}\n${text}\n${fence}`; }
  if (body.type === 'blockquote') return children.join('\n\n').split('\n').map(line => `> ${line}`).join('\n');
  if (['bulletList', 'orderedList', 'taskList'].includes(body.type)) return (body.content ?? []).map((child, index) => { const prefix = body.type === 'orderedList' ? `${Number(body.attrs?.start ?? 1) + index}. ` : body.type === 'taskList' ? `- [${child.attrs?.checked ? 'x' : ' '}] ` : '- '; return prefix + markdown(child).split('\n').join(`\n${' '.repeat(prefix.length)}`); }).join('\n');
  return children.join(body.type === 'doc' || body.type === 'listItem' || body.type === 'taskItem' ? '\n\n' : '');
}
