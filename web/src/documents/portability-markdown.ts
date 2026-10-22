import { Lexer, type Token, type Tokens } from 'marked';
import { decodeHTMLStrict } from 'entities';
import { canonicalBody, MAX_DOCUMENT_BYTES, safeHref, type RichBody, type RichMark, type RichNode } from './model';
import type { NativeDocumentImport, PortabilityWarning } from './portability';

const encoder = new TextEncoder();
const markOrder = ['bold', 'italic', 'strike', 'code', 'link'];

export function importMarkdownDocument(source: string): NativeDocumentImport {
  if (encoder.encode(source).length > MAX_DOCUMENT_BYTES) throw new Error('Markdown is too large. The limit is 256 KiB.');
  if (source.includes('\0')) throw new Error('Markdown cannot contain null characters.');
  const warnings = new Map<string, PortabilityWarning>();
  const warn = (code: string, message: string) => warnings.set(code, { code, message });
  let tokens: Token[];
  try { tokens = new Lexer({ gfm: true, breaks: false, pedantic: false }).lex(source); }
  catch { throw new Error('This Markdown is too complex to parse safely. Split it into smaller sections.'); }
  const pending = tokens.map(token => ({ token, depth: 1 }));
  let count = 0;
  while (pending.length) {
    const { token, depth } = pending.pop()!;
    if (++count > 10_000 || depth > 32) throw new Error('This Markdown is too complex. Split it into smaller sections.');
    const children = token.type === 'list' ? (token as Tokens.List).items : ('tokens' in token ? token.tokens ?? [] : []);
    for (const child of children) pending.push({ token: child, depth: depth + 1 });
  }
  function marksWith(marks: RichMark[], next: RichMark): RichMark[] {
    return [...marks.filter(mark => mark.type !== next.type), next].sort((left, right) => markOrder.indexOf(left.type) - markOrder.indexOf(right.type));
  }
  function text(value: string, marks: RichMark[] = []): RichNode[] { return value ? [{ type: 'text', text: value, ...(marks.length ? { marks } : {}) }] : []; }
  function merge(nodes: RichNode[]): RichNode[] {
    const result: RichNode[] = [];
    for (const node of nodes) {
      const prior = result.at(-1);
      if (node.type === 'text' && prior?.type === 'text' && JSON.stringify(node.marks ?? []) === JSON.stringify(prior.marks ?? [])) prior.text! += node.text;
      else result.push(node);
    }
    return result;
  }
  function href(token: Tokens.Link | Tokens.Image | Tokens.Def) {
    const value = 'autolink' in token && token.autolink ? token.href : decodeHTMLStrict(token.href);
    if (!safeHref(value)) throw new Error('Markdown contains an unsafe or unsupported link. Only safe http, https and mailto URLs are accepted.');
    return value;
  }
  function inline(items: Token[], marks: RichMark[] = [], literalEntities = false): RichNode[] {
    return merge(items.flatMap(token => {
      switch (token.type) {
        case 'checkbox': return [];
        case 'text': return token.tokens ? inline(token.tokens, marks, literalEntities) : text(literalEntities ? token.raw : decodeHTMLStrict(token.raw), marks);
        case 'escape': return text((token as Tokens.Escape).text, marks);
        case 'strong': return inline((token as Tokens.Strong).tokens, marksWith(marks, { type: 'bold' }), literalEntities);
        case 'em': return inline((token as Tokens.Em).tokens, marksWith(marks, { type: 'italic' }), literalEntities);
        case 'del': return inline((token as Tokens.Del).tokens, marksWith(marks, { type: 'strike' }), literalEntities);
        case 'codespan': return text((token as Tokens.Codespan).text, marksWith(marks, { type: 'code' }));
        case 'br': return [{ type: 'hardBreak' }];
        case 'link': {
          const link = token as Tokens.Link; const destination = href(link);
          if (link.title) warn('link-titles', 'Link title attributes are not supported and are omitted. Link text and safe destinations are retained.');
          return inline(link.tokens, marksWith(marks, { type: 'link', attrs: { href: destination } }), !!link.autolink);
        }
        case 'image':
          href(token as Tokens.Image);
          warn('images-literal', 'Markdown images are kept as literal source text. No remote images are fetched and no attachment is created.');
          return text(token.raw, marks);
        case 'html':
          warn('html-literal', 'HTML is kept as literal source text. Tags, scripts and attributes are never rendered or executed.');
          return text(token.raw, marks);
        default:
          warn('unsupported-literal', 'Unsupported Markdown is preserved as literal source text. Review the replacement before saving.');
          return text(token.raw, marks);
      }
    }));
  }
  const paragraph = (content: RichNode[]): RichNode => ({ type: 'paragraph', ...(content.length ? { content } : {}) });
  function literalBlock(token: Token, code: string, message: string): RichNode {
    warn(code, message);
    return { type: 'codeBlock', attrs: { language: null }, ...(token.raw ? { content: text(token.raw) } : {}) };
  }
  function list(token: Tokens.List): RichNode[] {
    if (token.ordered && Number(token.start) < 1) return [literalBlock(token, 'list-start-literal', 'Ordered lists starting below 1 are preserved as literal source text.')];
    if (token.ordered && token.items.some(item => item.task)) return [literalBlock(token, 'ordered-tasks-literal', 'Numbered task lists are not supported and are preserved as literal source text.')];
    const groups: RichNode[] = [];
    for (const item of token.items) {
      const type = item.task ? 'taskList' : token.ordered ? 'orderedList' : 'bulletList';
      let group = groups.at(-1);
      if (group?.type !== type) { group = { type, ...(token.ordered ? { attrs: { start: Number(token.start) } } : {}), content: [] }; groups.push(group); }
      const children = blocks(item.tokens.filter(child => child.type !== 'checkbox'));
      if (children[0]?.type !== 'paragraph') children.unshift(paragraph([]));
      group.content!.push({ type: item.task ? 'taskItem' : 'listItem', ...(item.task ? { attrs: { checked: !!item.checked } } : {}), content: children });
    }
    if (groups.length > 1) warn('mixed-lists', 'Mixed task and ordinary list items are split into adjacent lists while keeping their order and check states.');
    return groups;
  }
  function blocks(items: Token[]): RichNode[] {
    return items.flatMap(token => {
      switch (token.type) {
        case 'space': return [];
        case 'def':
          href(token as Tokens.Def);
          warn('reference-definitions', 'Reference-style links become inline links. Their source definitions are preserved as literal code for review.');
          return [literalBlock(token, 'reference-definitions', 'Reference-style links become inline links. Their source definitions are preserved as literal code for review.')];
        case 'paragraph': case 'text': return [paragraph(inline(token.tokens ?? [token]))];
        case 'heading': {
          const heading = token as Tokens.Heading;
          return [heading.depth <= 3 ? { type: 'heading', attrs: { level: heading.depth }, content: inline(heading.tokens) } : literalBlock(token, 'heading-level-literal', 'Headings deeper than level 3 are preserved as literal source text.')];
        }
        case 'blockquote': { const content = blocks((token as Tokens.Blockquote).tokens); return [{ type: 'blockquote', content: content.length ? content : [paragraph([])] }]; }
        case 'list': return list(token as Tokens.List);
        case 'code': {
          const code = token as Tokens.Code;
          if (code.lang && !/^[a-zA-Z0-9_+#-]{1,64}$/.test(code.lang)) return [literalBlock(token, 'code-info-literal', 'Unsupported code fence metadata is preserved with its source as literal code.')];
          return [{ type: 'codeBlock', attrs: { language: code.lang || null }, ...(code.text ? { content: text(code.text) } : {}) }];
        }
        case 'html': return [literalBlock(token, 'html-literal', 'HTML is kept as literal source text. Tags, scripts and attributes are never rendered or executed.')];
        default: return [literalBlock(token, 'unsupported-literal', 'Unsupported Markdown blocks, such as tables and horizontal rules, are preserved as literal source text.')];
      }
    });
  }
  const body: RichBody = canonicalBody({ type: 'doc', content: blocks(tokens) }, true);
  return { body, warnings: [...warnings.values()] };
}
