import { describe, expect, it } from 'vitest';
import { canonicalBody, fromPlainText, plainText, MAX_DOCUMENT_BYTES, type RichBody, type RichMark, type RichNode } from './model';
import { exportMarkdownDocument } from './portability';
import { importMarkdownDocument } from './portability-markdown';

const text = (value: string, marks: RichMark[] = []): RichNode => ({ type: 'text', text: value, ...(marks.length ? { marks } : {}) });
const paragraph = (...content: RichNode[]): RichNode => ({ type: 'paragraph', ...(content.length ? { content } : {}) });
const document = (...content: RichNode[]): RichBody => ({ type: 'doc', content });
function semantics(value: RichBody): RichBody {
  const normalize = (node: RichNode): RichNode => {
    const marks = node.marks ? [...node.marks].sort((a, b) => a.type.localeCompare(b.type)) : undefined;
    if (!node.content) return { ...node, ...(marks ? { marks } : {}) };
    const content: RichNode[] = [];
    for (const raw of node.content) {
      const child = normalize(raw); const prior = content.at(-1);
      if (prior?.type === 'text' && child.type === 'text' && JSON.stringify(prior.marks ?? []) === JSON.stringify(child.marks ?? [])) prior.text! += child.text;
      else content.push(child);
    }
    return { ...node, content, ...(marks ? { marks } : {}) };
  };
  return normalize(canonicalBody(value, true)) as RichBody;
}

describe('Markdown structured import and semantic round trips', () => {
  it('round-trips supported headings, nested lists/tasks/quotes, marks, links, hard breaks and code', () => {
    const body = document(
      { type: 'heading', attrs: { level: 1 }, content: [text('Title <literal>')] },
      paragraph(text('Unmarked '), text('bold', [{ type: 'bold' }]), text('italic', [{ type: 'italic' }]), text('strike', [{ type: 'strike' }]), text(' combined ', [{ type: 'bold' }, { type: 'italic' }, { type: 'strike' }]), { type: 'hardBreak' }, text('https://plain.test &copy; 👋')),
      paragraph(text('Link & text', [{ type: 'link', attrs: { href: 'https://example.test/a(b)?q=1&x=2' } }, { type: 'bold' }]), text('` literal `', [{ type: 'code' }])),
      { type: 'orderedList', attrs: { start: 9 }, content: [{ type: 'listItem', content: [paragraph(text('First')), { type: 'bulletList', content: [{ type: 'listItem', content: [paragraph(text('Child'))] }] }] }, { type: 'listItem', content: [paragraph(text('Second'))] }] },
      { type: 'taskList', content: [{ type: 'taskItem', attrs: { checked: true }, content: [paragraph(text('Done')), { type: 'blockquote', content: [paragraph(text('Nested quote'))] }] }, { type: 'taskItem', attrs: { checked: false }, content: [paragraph(text('Pending'))] }] },
      { type: 'blockquote', content: [{ type: 'heading', attrs: { level: 2 }, content: [text('Quoted heading')] }, paragraph(text('Quoted text'))] },
      { type: 'codeBlock', attrs: { language: 'rust' }, content: [text('let x = "``` &copy; <script>";\n\n')] },
    );
    const converted = exportMarkdownDocument(body); const imported = importMarkdownDocument(converted.text);
    expect(imported.warnings).toEqual([]);
    expect(semantics(imported.body)).toEqual(semantics(body));
  });

  it('preserves every adjacent pair of supported Markdown mark combinations without delimiter collisions', () => {
    const possible: RichMark[] = [{ type: 'bold' }, { type: 'italic' }, { type: 'strike' }, { type: 'code' }, { type: 'link', attrs: { href: 'https://example.test' } }];
    const combinations = Array.from({ length: 32 }, (_, mask) => possible.filter((_, index) => (mask & (1 << index)) !== 0)).filter(marks => !marks.some(mark => mark.type === 'code') || !marks.some(mark => ['bold', 'italic', 'strike'].includes(mark.type)));
    for (const left of combinations) for (const right of combinations) {
      const body = document(paragraph(text('a', left), text('b', right), text('c')));
      const encoded = exportMarkdownDocument(body).text;
      expect(semantics(importMarkdownDocument(encoded).body), encoded).toEqual(semantics(body));
    }
  });

  it('preserves marked punctuation, emoji and edge whitespace next to text and other marks', () => {
    const variants: RichMark[][] = [[], [{ type: 'bold' }], [{ type: 'italic' }], [{ type: 'strike' }], [{ type: 'bold' }, { type: 'italic' }]];
    for (const value of ['😀', '!', '.', '中文', '\tx\t', '\nx\n', '\u00a0x\u00a0', '*']) for (const left of variants) for (const right of variants) {
      const body = document(paragraph(text('a', left), text(value, right), text('b')));
      const exported = exportMarkdownDocument(body);
      expect(semantics(importMarkdownDocument(exported.text).body), exported.text).toEqual(semantics(body));
    }
  });

  it('retains literal newlines inside text without creating new blocks and preserves adjacent list boundaries', () => {
    for (const value of ['a\n\nb', '\ra\r\nb\n', 'a\n# title\n\nb']) {
      const body = document(paragraph(text(value)));
      expect(semantics(importMarkdownDocument(exportMarkdownDocument(body).text).body)).toEqual(semantics(body));
    }
    for (const type of ['bulletList', 'orderedList', 'taskList']) {
      const list = (value: string): RichNode => ({ type, ...(type === 'orderedList' ? { attrs: { start: 3 } } : {}), content: [{ type: type === 'taskList' ? 'taskItem' : 'listItem', ...(type === 'taskList' ? { attrs: { checked: true } } : {}), content: [paragraph(text(value))] }] });
      const body = document(list('First'), list('Second'), list('Third'));
      expect(semantics(importMarkdownDocument(exportMarkdownDocument(body).text).body)).toEqual(semantics(body));
    }
  });

  it('explicitly reports unrepresentable trailing hard breaks and code carriage returns', () => {
    const body = document(paragraph(text('a'), { type: 'hardBreak' }), { type: 'codeBlock', content: [text('a\rb\r\nc')] });
    const exported = exportMarkdownDocument(body);
    expect(exported.warnings.map(warning => warning.code)).toEqual(['trailing-hard-breaks', 'code-carriage-returns']);
    expect(importMarkdownDocument(exported.text).body.content[0]).toEqual(paragraph(text('a')));
  });
  it('reports decorative marks combined with inline code as an explicit lossy export', () => {
    const body = document(paragraph(text('a', [{ type: 'bold' }, { type: 'italic' }, { type: 'strike' }, { type: 'code' }, { type: 'link', attrs: { href: 'https://example.test' } }])));
    const exported = exportMarkdownDocument(body);
    expect(exported.warnings.map(warning => warning.code)).toEqual(['code-formatting']);
    expect(importMarkdownDocument(exported.text).body.content[0].content?.[0].marks).toEqual([{ type: 'code' }, { type: 'link', attrs: { href: 'https://example.test' } }]);
  });

  it('decodes character references exactly once, retaining literal escapes and code', () => {
    const imported = importMarkdownDocument('&#38;amp; &copy; &lt; \\&amp; `&copy;` <https://example.test/?a=1&amp;b=2>');
    expect(plainText(imported.body)).toBe('&amp; © < &amp; &copy; https://example.test/?a=1&amp;b=2');
    const last = imported.body.content[0].content!.at(-1)!;
    expect(last.marks?.[0].attrs?.href).toBe('https://example.test/?a=1&amp;b=2');
    expect(semantics(importMarkdownDocument(exportMarkdownDocument(fromPlainText('  literal\n# title\n- list\n&amp; &copy;')).text).body)).toEqual(semantics(fromPlainText('  literal\n# title\n- list\n&amp; &copy;')));
  });

  it('keeps unsupported HTML, tables, images, deeper headings and code metadata as inert source with review warnings', () => {
    const source = '<script>alert(1)</script>\n\n| A | B |\n| - | - |\n| 1 | 2 |\n\n![Image](https://example.test/image.png)\n\n#### Deep\n\n```js title=demo\ncode\n```';
    const result = importMarkdownDocument(source);
    expect(result.warnings.map(warning => warning.code)).toEqual(['html-literal', 'unsupported-literal', 'images-literal', 'heading-level-literal', 'code-info-literal']);
    const plain = plainText(result.body);
    for (const retained of ['<script>alert(1)</script>', '| 1 | 2 |', '![Image](https://example.test/image.png)', '#### Deep', '```js title=demo']) expect(plain).toContain(retained);
    expect(JSON.stringify(result.body)).not.toContain('"type":"image"'); expect(JSON.stringify(result.body)).not.toContain('"type":"html"');
  });

  it('rejects unsafe, encoded, credential-bearing and unsupported URLs before a draft is returned', () => {
    for (const url of ['javascript:alert(1)', 'jav&#x61;script:alert(1)', 'data:text/html,hi', 'https://name:secret@example.test', '/relative/path', 'mailto:x@example.test?subject=x', 'file:///tmp/a']) {
      for (const source of [`[Danger](<${url}>)`, `![Danger](<${url}>)`, `[unused]: ${url}`]) expect(() => importMarkdownDocument(source), source).toThrow('unsafe or unsupported link');
    }
  });

  it('resolves safe reference links with explicit title/definition conversion and never manufactures app references', () => {
    const result = importMarkdownDocument('[target]: https://example.test "Tooltip"\n\n[target] @someone #ENG-1');
    expect(result.warnings.map(warning => warning.code)).toEqual(['reference-definitions', 'link-titles']);
    expect(result.body.content[1].content?.[0].marks).toEqual([{ type: 'link', attrs: { href: 'https://example.test' } }]);
    expect(JSON.stringify(result.body)).not.toContain('userMention'); expect(JSON.stringify(result.body)).not.toContain('issueReference');
  });

  it('preserves mixed list checkbox states and unsupported ordered task lists without guessing', () => {
    const result = importMarkdownDocument('- [x] Done\n- Plain\n- [ ] Open');
    expect(result.body.content.map(node => node.type)).toEqual(['taskList', 'bulletList', 'taskList']);
    expect(result.warnings.map(warning => warning.code)).toEqual(['mixed-lists']);
    expect(importMarkdownDocument('1. [x] Task').warnings.map(warning => warning.code)).toEqual(['ordered-tasks-literal']);
    expect(importMarkdownDocument('0. Zero').warnings.map(warning => warning.code)).toEqual(['list-start-literal']);
  });

  it('bounds bytes, depth and expansion; empty Markdown yields an editable empty paragraph', () => {
    expect(importMarkdownDocument('').body).toEqual(fromPlainText(''));
    expect(() => importMarkdownDocument('x'.repeat(MAX_DOCUMENT_BYTES + 1))).toThrow('too large');
    expect(() => importMarkdownDocument('👋'.repeat(MAX_DOCUMENT_BYTES / 4 + 1))).toThrow('too large');
    expect(() => importMarkdownDocument('> '.repeat(40) + 'Deep')).toThrow('too complex');
    expect(() => importMarkdownDocument('[x](https://example.test) '.repeat(4000))).toThrow();
    expect(() => importMarkdownDocument('null\0character')).toThrow('null characters');
  });
});

it('preserves leading indentation, tabs and trailing spaces as text instead of Markdown code or hard breaks', () => {
  for (const value of ['    indented text', '\tindented text', 'text    ', ' \t mixed whitespace \t ', '    ']) {
    const body = fromPlainText(value);
    expect(semantics(importMarkdownDocument(exportMarkdownDocument(body).text).body)).toEqual(semantics(body));
  }
});
