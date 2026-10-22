import { describe, expect, it } from 'vitest';
import { canonicalBody, EMPTY_BODY, fromPlainText, MAX_DOCUMENT_BYTES, safeHref } from './model';

describe('document wire boundary', () => {
  it('normalizes both valid empty document representations without changing meaning', () => { expect(canonicalBody({ type: 'doc' }, true)).toEqual(EMPTY_BODY); expect(canonicalBody({ type: 'doc', content: [] }, true)).toEqual(EMPTY_BODY); });
  it('rejects unknown fields and malformed containers instead of dropping them', () => {
    for (const value of [{ type: 'doc', future: true }, { type: 'doc', attrs: null }, { type: 'doc', content: {} }, { type: 'doc', content: [{ type: 'paragraph', marks: [] }] }, { type: 'doc', content: [{ type: 'paragraph', content: [{ type: 'text', text: 'hello', marks: {} }] }] }, { type: 'doc', content: [{ type: 'paragraph', content: [{ type: 'text', text: 'hello', marks: [{ type: 'bold', future: true }] }] }] }]) expect(() => canonicalBody(value, true)).toThrow();
  });
  it('checks node placement, required task state and headings as integer numbers', () => {
    for (const node of [{ type: 'heading', attrs: { level: '1' } }, { type: 'taskList', content: [{ type: 'taskItem', content: [{ type: 'paragraph' }] }] }, { type: 'text', text: 'root text' }, { type: 'bulletList', content: [{ type: 'heading', attrs: { level: 1 } }] }]) expect(() => canonicalBody({ type: 'doc', content: [node] }, true)).toThrow();
  });
  it('strips known rendering defaults only when serializing the local editor', () => {
    const body = { type: 'doc', content: [{ type: 'paragraph', content: [{ type: 'text', text: 'Link', marks: [{ type: 'link', attrs: { href: 'https://example.test', rel: 'noopener', target: '_blank', class: null } }] }] }] };
    expect(() => canonicalBody(body, true)).toThrow(); expect(canonicalBody(body).content[0].content?.[0].marks?.[0].attrs).toEqual({ href: 'https://example.test' });
  });
  it('matches safe external-link protocols without URL-normalization bypasses', () => {
    for (const href of ['https://example.test/path', 'http://localhost:8080/', 'mailto:owner@example.test']) expect(safeHref(href)).toBe(true);
    for (const href of ['javascript:alert(1)', 'https://user:secret@example.test', 'https://example.test\\path', 'mailto:owner@example.test?subject=hello', 'mailto:owner@example.test#x', 'mailto:owner@example.test?', 'https://example.test/a b', 'mailto:', 'https://example.test/\u0080', `https://example.test/${'x'.repeat(2048)}`]) expect(safeHref(href)).toBe(false);
  });
  it('bounds writes without truncating text', () => { expect(() => canonicalBody(fromPlainText('x'.repeat(MAX_DOCUMENT_BYTES)))).toThrow('too large'); });
});

import { attachmentIds, hasContent, hasText, withoutAttachments } from './model';
const fileNode = (id = '11111111-1111-4111-8111-111111111111') => ({ type: 'attachment', attrs: { id } });
it('accepts only bounded UUID attachment leaves and treats attachment-only comments as meaningful', () => {
  const body = canonicalBody({ type: 'doc', content: [fileNode()] }, true);
  expect(hasContent(body)).toBe(true); expect(hasText(body)).toBe(false); expect(attachmentIds(body)).toEqual(['11111111-1111-4111-8111-111111111111']);
  for (const node of [fileNode('bad'), { ...fileNode(), attrs: { ...fileNode().attrs, url: 'https://example.test/file' } }, { ...fileNode(), content: [] }, { ...fileNode(), text: 'x' }, { ...fileNode(), marks: [] }]) expect(() => canonicalBody({ type: 'doc', content: [node] }, true)).toThrow();
  expect(() => canonicalBody({ type: 'doc', content: [fileNode(), fileNode()] }, true)).toThrow();
  expect(() => canonicalBody({ type: 'doc', content: [{ type: 'paragraph', content: [fileNode()] }] }, true)).toThrow();
  const files = Array.from({ length: 101 }, (_, i) => fileNode(`11111111-1111-4111-8111-${String(i).padStart(12, '0')}`));
  expect(() => canonicalBody({ type: 'doc', content: files }, true)).toThrow(); expect(canonicalBody({ type: 'doc', content: files.slice(0, 100) }, true).content).toHaveLength(100);
});
it('removes submitted attachment references from a continuing comment without dropping its newer text', () => {
  const old = fileNode(); const added = fileNode('22222222-2222-4222-8222-222222222222');
  const body = canonicalBody({ type: 'doc', content: [...fromPlainText('Newer text').content, { type: 'blockquote', content: [old] }, added] }, true);
  const retained = withoutAttachments(body, new Set([old.attrs.id])); expect(attachmentIds(retained)).toEqual([added.attrs.id]); expect(retained.content[0].content?.[0].text).toBe('Newer text'); expect(retained.content[1].content).toEqual([{ type: 'paragraph' }]);
});

import { markdown, mentionIds, plainText } from './model';
const mentionNode = (id = '11111111-1111-4111-8111-111111111111') => ({ type: 'userMention', attrs: { id } });
it('preserves ID-only mentions with generic portable text and treats mentions-only comments as meaningful', () => {
  const body = canonicalBody({ type: 'doc', content: [{ type: 'paragraph', content: [mentionNode(), mentionNode()] }] }, true);
  expect(body.content[0].content).toEqual([mentionNode(), mentionNode()]); expect(mentionIds(body)).toHaveLength(2); expect(hasContent(body)).toBe(true); expect(plainText(body)).toBe('@someone@someone'); expect(markdown(body)).toBe('@someone@someone');
  for (const node of [mentionNode('bad'), { ...mentionNode(), attrs: { ...mentionNode().attrs, display_name: 'Private name' } }, { ...mentionNode(), marks: [] }, { ...mentionNode(), content: [] }, { ...mentionNode(), text: 'Secret' }]) expect(() => canonicalBody({ type: 'doc', content: [{ type: 'paragraph', content: [node] }] }, true)).toThrow();
  expect(() => canonicalBody({ type: 'doc', content: [{ type: 'codeBlock', content: [mentionNode()] }] }, true)).toThrow();
  expect(() => canonicalBody({ type: 'doc', content: [mentionNode()] }, true)).toThrow();
});
it('bounds distinct mentioned people without rejecting repeated references to the same person', () => {
  const mentions = Array.from({ length: 51 }, (_, index) => mentionNode(`11111111-1111-4111-8111-${String(index).padStart(12, '0')}`));
  expect(() => canonicalBody({ type: 'doc', content: [{ type: 'paragraph', content: mentions }] })).toThrow('50 different');
  expect(canonicalBody({ type: 'doc', content: [{ type: 'paragraph', content: [...mentions.slice(0, 50), mentions[0]] }] }).content[0].content).toHaveLength(51);
});

import { issueReferenceIds } from './model';
it('keeps only bounded ID leaves for issue references and never embeds private titles', () => {
  const reference = (id: string) => ({ type: 'issueReference', attrs: { id } });
  const id = 'abcdefab-abcd-4abc-8abc-abcdefabcdef';
  const doc = (content: unknown[]) => ({ type: 'doc', content: [{ type: 'paragraph', content }] });
  const body = canonicalBody(doc([reference(id), reference(id)]), true);
  expect(issueReferenceIds(canonicalBody(doc([reference(id.toUpperCase())])))).toEqual([id]);
  expect(issueReferenceIds(body)).toEqual([id, id]); expect(hasContent(body)).toBe(true);
  expect(plainText(body)).toBe('#issue#issue'); expect(markdown(body)).toBe('#issue#issue');
  for (const node of [reference('bad'), { type: 'issueReference', attrs: { id, title: 'Private' } }, { ...reference(id), marks: [] }, { ...reference(id), content: [] }]) expect(() => canonicalBody(doc([node]), true)).toThrow();
  const nodes = Array.from({length:51}, (_, index) => reference(`11111111-1111-4111-8111-${String(index).padStart(12,'0')}`));
  expect(() => canonicalBody(doc(nodes))).toThrow('50 different');
  expect(canonicalBody(doc([...nodes.slice(0,50),nodes[0]])).content[0].content).toHaveLength(51);
  expect(() => canonicalBody({type:'doc',content:[reference(id)]})).toThrow();
  expect(() => canonicalBody({type:'doc',content:[{type:'codeBlock',content:[reference(id)]}]})).toThrow();
});
