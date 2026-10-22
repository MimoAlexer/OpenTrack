import { canonicalBody, MAX_DOCUMENT_BYTES, type RichBody, type RichMark, type RichNode } from './model';

export const MAX_PORTABLE_DOCUMENT_BYTES = MAX_DOCUMENT_BYTES + 1024;
export type PortabilityWarning = { code: string; message: string };
export type PortableExport = { text: string; warnings: PortabilityWarning[] };
export type NativeDocumentImport = { body: RichBody; warnings: PortabilityWarning[] };

const encoder = new TextEncoder();
const nativeKeys = ['format', 'format_version', 'schema_version', 'body'];

function referenceWarnings(body: RichBody): PortabilityWarning[] {
  const kinds = new Set<string>();
  const visit = (node: RichNode) => { kinds.add(node.type); node.content?.forEach(visit); };
  visit(body);
  const warnings: PortabilityWarning[] = [];
  if (kinds.has('userMention') || kinds.has('issueReference') || kinds.has('attachment')) warnings.push({ code: 'reference-scope', message: 'References retain their original IDs. Use them only in the same installation and permitted destination; importing IDs does not grant access. Saving checks current access again.' });
  if (kinds.has('attachment')) warnings.push({ code: 'attachment-bytes', message: 'Attachment files are not included. Their IDs remain tied to the original document; this file cannot copy attachments to another document or recover missing files.' });
  return warnings;
}

export function exportNativeDocument(value: unknown): PortableExport {
  const body = canonicalBody(value, true);
  return { text: JSON.stringify({ format: 'opentrack.document', format_version: 1, schema_version: 1, body }), warnings: referenceWarnings(body) };
}

export function importNativeDocument(text: string): NativeDocumentImport {
  if (encoder.encode(text).length > MAX_PORTABLE_DOCUMENT_BYTES) throw new Error('This document file is too large. The limit is 257 KiB.');
  let value: unknown;
  try { value = JSON.parse(text); } catch { throw new Error('This is not valid OpenTrack document JSON.'); }
  if (!value || typeof value !== 'object' || Array.isArray(value)) throw new Error('An OpenTrack document file must contain a document envelope.');
  const envelope = value as Record<string, unknown>;
  if (Object.keys(envelope).length !== nativeKeys.length || Object.keys(envelope).some(key => !nativeKeys.includes(key))) throw new Error('This document file has missing or unsupported envelope fields.');
  if (envelope.format !== 'opentrack.document' || envelope.format_version !== 1 || envelope.schema_version !== 1) throw new Error('This document format or schema version is not supported.');
  const body = canonicalBody(envelope.body, true);
  return { body, warnings: referenceWarnings(body) };
}

// Escape every ASCII punctuation character. This also protects line-start list,
// heading, quote and HTML syntax in text that happens to resemble Markdown.
function escapeText(text: string) {
  return text.replace(/[!-/:-@[-`{-~]/g, '\\$&').replace(/[\r\n]/g, character => `&#${character.codePointAt(0)};`);
}

function backtickFence(text: string, minimum: number) {
  let length = minimum;
  for (const match of text.matchAll(/`+/g)) length = Math.max(length, match[0].length + 1);
  return '`'.repeat(length);
}

function codeSpan(text: string) {
  const fence = backtickFence(text, 1);
  const pad = /^`|`$/.test(text) || (/^ | $/.test(text) && !/^ +$/.test(text)) ? ' ' : '';
  return `${fence}${pad}${text}${pad}${fence}`;
}

export function exportMarkdownDocument(value: unknown): PortableExport {
  const body = canonicalBody(value, true);
  const warnings = new Map<string, PortabilityWarning>();
  const warn = (code: string, message: string) => warnings.set(code, { code, message });
  function markdownMarks(node: RichNode): RichNode {
    let marks = node.marks;
    if (marks?.some(mark => mark.type === 'code') && marks.some(mark => ['bold', 'italic', 'strike'].includes(mark.type))) {
      warn('code-formatting', 'This Markdown conversion retains inline code and its link but removes simultaneous bold, italic or strikethrough. Document JSON preserves those combined marks.');
      marks = marks.filter(mark => mark.type === 'code' || mark.type === 'link');
    }
    return { ...node, ...(marks ? { marks } : {}), ...(node.content ? { content: node.content.map(markdownMarks) } : {}) };
  }
  const portable = markdownMarks(body);
  const firstBoundary = new WeakSet<RichNode>(); const lastBoundary = new WeakSet<RichNode>();
  function preserveBoundary(text: string, node: RichNode) {
    if (firstBoundary.has(node)) text = text.replace(/^[\p{L}\p{N}]/u, character => `&#${character.codePointAt(0)};`);
    if (lastBoundary.has(node)) text = text.replace(/[\p{L}\p{N}]$/u, character => `&#${character.codePointAt(0)};`);
    return text;
  }
  function leaf(node: RichNode): string {
    if (node.type === 'hardBreak') return '\\\n';
    if (node.type === 'userMention') { warn('user-mentions', 'User mentions become the text @someone; identities and notifications are not preserved.'); return preserveBoundary('@someone', node); }
    if (node.type === 'issueReference') { warn('issue-references', 'Issue references become the text #issue; issue identities and links are not preserved.'); return preserveBoundary('\\#issue', node); }
    if (node.type !== 'text') throw new Error('This inline node cannot be exported to Markdown.');
    let text = escapeText(node.text ?? '');
    if (node.marks?.some(mark => ['bold', 'italic', 'strike'].includes(mark.type))) text = text.replace(/^\s+|\s+$/gu, spaces => [...spaces].map(character => `&#${character.codePointAt(0)};`).join(''));
    return preserveBoundary(text, node);
  }
  const order = ['link', 'bold', 'italic', 'strike', 'code'] as const;
  function inline(nodes: RichNode[], level = 0): string {
    if (level === order.length) return nodes.map(leaf).join('');
    const kind = order[level];
    for (const [index, node] of nodes.entries()) {
      if (node.marks?.some(mark => mark.type === kind)) continue;
      if (node.marks?.some(mark => order.indexOf(mark.type) > level)) continue;
      const prior = nodes[index - 1]?.marks; const next = nodes[index + 1]?.marks;
      if (prior?.some(mark => mark.type === kind)) firstBoundary.add(node);
      if (next?.some(mark => mark.type === kind)) lastBoundary.add(node);
    }
    const groups: { mark: RichMark | undefined; nodes: RichNode[] }[] = [];
    for (const node of nodes) {
      const mark = node.marks?.find(item => item.type === kind);
      const prior = groups.at(-1);
      if (prior && JSON.stringify(prior.mark) === JSON.stringify(mark)) prior.nodes.push(node);
      else groups.push({ mark, nodes: [node] });
    }
    return groups.map(({ mark, nodes: children }) => {
      if (!mark) return inline(children, level + 1);
      if (kind === 'code') {
        const raw = children.map(node => node.text ?? '').join('');
        if (/[\r\n]/.test(raw)) warn('code-line-breaks', 'Markdown inline code converts line breaks to spaces. Use the native document file to preserve those characters.');
        return codeSpan(raw.replace(/\r\n?|\n/g, ' '));
      }
      const text = inline(children, level + 1);
      if (kind === 'bold') return `**${text}**`;
      if (kind === 'italic') return `*${text}*`;
      if (kind === 'strike') { warn('github-markdown', 'Task lists and strikethrough require a Markdown reader with GitHub-style extensions.'); return `~~${text}~~`; }
      const href = mark.attrs!.href.replaceAll('\\', '%5C').replaceAll('<', '%3C').replaceAll('>', '%3E').replaceAll('&', '&amp;');
      return `[${text}](<${href}>)`;
    }).join('');
  }
  function blocks(nodes: RichNode[]): string {
    let previousList: string | undefined; let alternate = false;
    return nodes.map(node => {
      const list = node.type === 'orderedList' ? 'ordered' : ['bulletList', 'taskList'].includes(node.type) ? 'bullet' : undefined;
      alternate = list && previousList === list ? !alternate : false; previousList = list;
      return block(node, alternate);
    }).join('\n\n');
  }
  function block(node: RichNode, alternateListMarker = false): string {
    if (node.type === 'attachment') { warn('attachments', 'Attachment files and IDs are omitted from Markdown. Each attachment is replaced with an explicit placeholder.'); return '\\[Attachment omitted\\]'; }
    if (node.type === 'paragraph' || node.type === 'heading') {
      const content = [...node.content ?? []];
      while (content.at(-1)?.type === 'hardBreak') {
        content.pop(); warn('trailing-hard-breaks', 'Markdown cannot preserve hard breaks at the end of a paragraph or heading. These are omitted; document JSON preserves them.');
      }
      const text = inline(content).replace(/^[ \t]+|[ \t]+$/gm, spaces => [...spaces].map(character => character === '\t' ? '&#9;' : '&#32;').join(''));
      if (!text && node.type === 'paragraph') warn('empty-paragraphs', 'Markdown readers may collapse empty paragraphs and extra blank lines.');
      if (node.type === 'heading') {
        if (text.includes('\n')) warn('heading-line-breaks', 'Markdown headings cannot contain line breaks. These become spaces; document JSON preserves the original heading.');
        return `${'#'.repeat(Number(node.attrs!.level))} ${text.replace(/\\\n|\n/g, ' ')}`;
      }
      return text;
    }
    if (node.type === 'codeBlock') {
      const text = (node.content ?? []).map(child => child.text ?? '').join('');
      if (text.includes('\r')) warn('code-carriage-returns', 'Markdown readers normalize carriage returns in code blocks to line feeds. Document JSON preserves the original line endings.');
      const fence = backtickFence(text, 3);
      return `${fence}${node.attrs?.language ?? ''}\n${text}\n${fence}`;
    }
    if (node.type === 'blockquote') return blocks(node.content ?? []).split('\n').map(line => `> ${line}`).join('\n');
    if (['bulletList', 'orderedList', 'taskList'].includes(node.type)) {
      if (node.type === 'taskList') warn('github-markdown', 'Task lists and strikethrough require a Markdown reader with GitHub-style extensions.');
      const originalStart = Number(node.attrs?.start ?? 1);
      const start = originalStart <= 999_999_999 ? originalStart : 1;
      if (node.type === 'orderedList' && start !== originalStart) warn('large-list-start', 'Markdown supports list markers of at most nine digits. This list starts at 1 in Markdown; document JSON preserves its original start.');
      return (node.content ?? []).map((item, index) => {
        const marker = node.type === 'orderedList' ? `${start + index <= 999_999_999 ? start + index : start}${alternateListMarker ? ')' : '.'} ` : alternateListMarker ? '+ ' : '- ';
        const task = node.type === 'taskList' ? `[${item.attrs?.checked ? 'x' : ' '}] ` : '';
        const content = blocks(item.content ?? []);
        return marker + task + content.split('\n').join(`\n${' '.repeat(marker.length)}`);
      }).join('\n');
    }
    throw new Error('This document node cannot be exported to Markdown.');
  }
  const text = blocks(portable.content!);
  if (encoder.encode(text).length > MAX_DOCUMENT_BYTES) warn('markdown-size', 'Escaping makes this Markdown larger than the 256 KiB import limit. Split it before importing, or use document JSON for this round trip.');
  return { text, warnings: [...warnings.values()] };
}
