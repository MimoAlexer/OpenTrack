import { describe, expect, it } from 'vitest';
import { canonicalBody, fromPlainText, MAX_DOCUMENT_BYTES, type RichBody, type RichNode } from './model';
import { exportMarkdownDocument, exportNativeDocument, importNativeDocument, MAX_PORTABLE_DOCUMENT_BYTES } from './portability';

const id = '11111111-1111-4111-8111-111111111111';
const text = (value: string): RichNode => ({ type: 'text', text: value });
const paragraph = (...content: RichNode[]): RichNode => ({ type: 'paragraph', ...(content.length ? { content } : {}) });
const document = (...content: RichNode[]): RichBody => ({ type: 'doc', content });
const corpus = document(
  paragraph(text('Literal # heading, <script>, **bold**, [link] & Unicode: Grüezi 世界 👋')),
  { type: 'heading', attrs: { level: 3 }, content: [text('Overview')] },
  paragraph({ type: 'text', text: 'Many marks', marks: [{ type: 'bold' }, { type: 'italic' }, { type: 'strike' }, { type: 'link', attrs: { href: 'https://example.test/a?b=1&c=2' } }] }, { type: 'hardBreak' }, text('Next line')),
  { type: 'orderedList', attrs: { start: 9 }, content: [{ type: 'listItem', content: [paragraph(text('First')), { type: 'bulletList', content: [{ type: 'listItem', content: [paragraph(text('Nested'))] }] }] }, { type: 'listItem', content: [paragraph(text('Second'))] }] },
  { type: 'taskList', content: [{ type: 'taskItem', attrs: { checked: true }, content: [paragraph(text('Done'))] }, { type: 'taskItem', attrs: { checked: false }, content: [paragraph(text('Open'))] }] },
  { type: 'blockquote', content: [paragraph(text('Quote')), paragraph(text('More'))] },
  { type: 'codeBlock', attrs: { language: 'rust' }, content: [text('let x = "```";\n\n// trailing line\n')] },
  paragraph({ type: 'userMention', attrs: { id } }),
  { type: 'attachment', attrs: { id } },
  paragraph(),
);

describe('single-document native transfer', () => {
  it('round-trips all supported structure and IDs without copying display projections or inventing attachment bytes', () => {
    const original = canonicalBody(corpus, true);
    const exported = exportNativeDocument(original);
    const envelope = JSON.parse(exported.text);
    expect(envelope).toEqual({ format: 'opentrack.document', format_version: 1, schema_version: 1, body: original });
    expect(importNativeDocument(exported.text).body).toEqual(original);
    expect(exported.warnings.map(warning => warning.code)).toEqual(['reference-scope', 'attachment-bytes']);
    expect(exported.warnings[1].message).toContain('cannot copy attachments');
  });
  it('reports scope limits for reference-only documents, but does not label ordinary text as lossy', () => {
    expect(exportNativeDocument(fromPlainText('Hello')).warnings).toEqual([]);
    const exported = exportNativeDocument(document(paragraph({ type: 'userMention', attrs: { id } })));
    expect(exported.warnings.map(warning => warning.code)).toEqual(['reference-scope']);
  });
  it('round-trips ID-only issue references natively and never copies a projected title into Markdown', () => {
    const body = document(paragraph({ type: 'issueReference', attrs: { id } }));
    expect(importNativeDocument(exportNativeDocument(body).text).body).toEqual(body);
    expect(exportMarkdownDocument(body)).toEqual({ text: '\\#issue', warnings: [{ code: 'issue-references', message: 'Issue references become the text #issue; issue identities and links are not preserved.' }] });
  });
  it('rejects future formats, unknown envelope/body fields and foreign document shapes before returning a draft', () => {
    const valid = JSON.parse(exportNativeDocument(fromPlainText('Keep')).text);
    for (const invalid of [null, [], { ...valid, format: 'other' }, { ...valid, format_version: 2 }, { ...valid, schema_version: 2 }, { ...valid, extra: true }, { ...valid, body: { ...valid.body, extra: true } }, { ...valid, body: document({ type: 'iframe' }) }, { format: 'opentrack.document' }]) expect(() => importNativeDocument(JSON.stringify(invalid))).toThrow();
    expect(() => importNativeDocument('{broken')).toThrow('not valid');
  });
  it('bounds UTF-8 input before parsing and separately bounds the actual document', () => {
    expect(() => importNativeDocument('🧑'.repeat(Math.ceil(MAX_PORTABLE_DOCUMENT_BYTES / 4) + 1))).toThrow('too large');
    const body = fromPlainText('x'.repeat(MAX_DOCUMENT_BYTES));
    expect(() => importNativeDocument(JSON.stringify({ format: 'opentrack.document', format_version: 1, schema_version: 1, body }))).toThrow('too large');
  });
  it('rejects unsafe URLs, attachment duplicates and deep structures without silently dropping content', () => {
    const envelope = (body: RichBody) => JSON.stringify({ format: 'opentrack.document', format_version: 1, schema_version: 1, body });
    expect(() => importNativeDocument(envelope(document(paragraph({ ...text('Danger'), marks: [{ type: 'link', attrs: { href: 'javascript:alert(1)' } }] }))))).toThrow();
    expect(() => importNativeDocument(envelope(document({ type: 'attachment', attrs: { id } }, { type: 'attachment', attrs: { id } })))).toThrow();
    let node = paragraph(text('Deep')); for (let depth = 0; depth < 33; depth++) node = { type: 'blockquote', content: [node] };
    expect(() => importNativeDocument(envelope(document(node)))).toThrow('too complex');
  });
});

describe('explicitly lossy Markdown conversion', () => {
  it('escapes text syntax and prevents text from turning into links, HTML, headings or lists', () => {
    expect(exportMarkdownDocument(fromPlainText('# title\n- bullet\n<img src=x onerror=alert(1)>\n[link](javascript:x)')).text).toBe('\\# title\n\n\\- bullet\n\n\\<img src\\=x onerror\\=alert\\(1\\)\\>\n\n\\[link\\]\\(javascript\\:x\\)');
  });
  it('writes code fences longer than their content and pads literal backticks or edge spaces in inline code', () => {
    const exported = exportMarkdownDocument(document({ type: 'codeBlock', attrs: { language: 'rust' }, content: [text('```\n````')] }, paragraph({ ...text('`a`'), marks: [{ type: 'code' }] }, text(' / '), { ...text(' padded '), marks: [{ type: 'code' }] })));
    expect(exported.text).toBe('`````rust\n```\n````\n`````\n\n`` `a` `` \\/ `  padded  `');
    expect(exported.warnings).toEqual([]);
  });
  it('preserves visible list start, nested indentation, task status, quotes and hard breaks', () => {
    const exported = exportMarkdownDocument(corpus);
    expect(exported.text).toContain('9. First\n   \n   - Nested\n10. Second');
    expect(exported.text).toContain('- [x] Done\n- [ ] Open');
    expect(exported.text).toContain('> Quote\n> \n> More');
    expect(exported.text).toContain('\\\nNext line');
  });
  it('does not silently erase references or files and deduplicates conversion warnings', () => {
    const exported = exportMarkdownDocument(corpus);
    expect(exported.text).toContain('@someone');
    expect(exported.text).toContain('\\[Attachment omitted\\]');
    expect(exported.text).not.toContain(id);
    expect(exported.warnings.map(warning => warning.code)).toEqual(['github-markdown', 'user-mentions', 'attachments', 'empty-paragraphs']);
  });
  it('protects link destinations with angle brackets and entities without exposing an active URL from plain text', () => {
    const body = document(paragraph({ ...text('Link <label>'), marks: [{ type: 'link', attrs: { href: 'https://example.test/a(b)?q=1&x=2' } }] }));
    expect(exportMarkdownDocument(body).text).toBe('[Link \\<label\\>](<https://example.test/a(b)?q=1&amp;x=2>)');
  });
  it('reports inline-code newline conversion and bounds repeated fence scanning without argument expansion', () => {
    const exported = exportMarkdownDocument(document(paragraph({ ...text('one\ntwo'), marks: [{ type: 'code' }] })));
    expect(exported.text).toBe('`one two`');
    expect(exported.warnings.map(warning => warning.code)).toEqual(['code-line-breaks']);
    expect(() => exportMarkdownDocument(document({ type: 'codeBlock', content: [text('`a'.repeat(70_000))] }))).not.toThrow();
  });
  it('reports native list starts too large for the Markdown grammar instead of emitting invalid list syntax', () => {
    const exported = exportMarkdownDocument(document({ type: 'orderedList', attrs: { start: 2147483647 }, content: [{ type: 'listItem', content: [paragraph(text('First'))] }] }));
    expect(exported.text).toBe('1. First'); expect(exported.warnings.map(warning => warning.code)).toEqual(['large-list-start']);
  });
  it('warns when escaped Markdown exceeds the import limit while native JSON still round-trips', () => {
    const body = fromPlainText('*'.repeat(140_000));
    expect(exportMarkdownDocument(body).warnings.map(warning => warning.code)).toEqual(['markdown-size']);
    expect(importNativeDocument(exportNativeDocument(body).text).body).toEqual(body);
  });
});
