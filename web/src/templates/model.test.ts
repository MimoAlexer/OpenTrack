import { expect, it } from 'vitest';
import { attachmentIds, EMPTY_BODY, fromPlainText, plainText } from '../documents/model';
import { applyTemplate, templateBody, type TemplateFields } from './model';
const id = '11111111-1111-4111-8111-111111111111';
const draft: TemplateFields = { title: '', body: EMPTY_BODY, status_id: null, priority: 0, label_ids: [] };
const defaults: TemplateFields = { title: 'Investigate issue', body: fromPlainText('Expected behavior'), status_id: 'todo', priority: 2, label_ids: ['bug'] };
it('merges defaults while preserving explicitly chosen metadata, including zero priority and team default status', () => {
  const result = applyTemplate({ ...draft, title: 'My title', body: fromPlainText('My findings'), label_ids: ['local'] }, defaults, 'merge', new Set(['priority', 'status_id']));
  expect(result.title).toBe('My title'); expect(result.priority).toBe(0); expect(result.status_id).toBeNull();
  expect(result.label_ids).toEqual(['local', 'bug']); expect(plainText(result.body)).toBe('My findings\nExpected behavior');
  expect(applyTemplate(draft, defaults, 'merge', new Set())).toEqual(defaults);
});
it('replaces supported defaults while preserving ready attachment references without copying unsupported reusable content', () => {
  const body = { type: 'doc' as const, content: [{ type: 'blockquote', content: [{ type: 'paragraph', content: [{ type: 'text', text: 'Old text' }] }, { type: 'attachment', attrs: { id } }] }] };
  const result = applyTemplate({ ...draft, body }, defaults, 'replace', new Set());
  expect(plainText(result.body).trim()).toBe('Expected behavior'); expect(attachmentIds(result.body)).toEqual([id]);
  expect(() => templateBody(body)).toThrow('cannot contain attachments');
  expect(() => templateBody({ type: 'doc', content: [{ type: 'futureBlock' }] })).toThrow();
});
it('refuses an over-limit merge instead of dropping selected labels or changing the original draft', () => {
  const current = { ...draft, label_ids: Array.from({ length: 50 }, (_, i) => `label-${i}`) };
  expect(() => applyTemplate(current, defaults, 'merge', new Set())).toThrow('more than 50 labels');
  expect(current.label_ids).toHaveLength(50); expect(current.body).toBe(EMPTY_BODY);
});

it.each(['bytes', 'nodes'])('refuses a %s-over-limit combined document without modifying either input', limit => {
  const body = limit === 'bytes' ? fromPlainText('a'.repeat(140000)) : { type: 'doc' as const, content: Array.from({ length: 2500 }, () => ({ type: 'paragraph', content: [{ type: 'text', text: 'x' }] })) };
  const valid = templateBody(body); const before = JSON.stringify(valid);
  expect(() => applyTemplate({ ...draft, body: valid }, { ...defaults, body: valid }, 'merge', new Set())).toThrow(limit === 'bytes' ? 'too large' : 'too complex');
  expect(JSON.stringify(valid)).toBe(before);
});
it('rejects reusable mention definitions while preserving mentions when merging an issue draft', () => {
  const body = { type: 'doc' as const, content: [{ type: 'paragraph', content: [{ type: 'userMention', attrs: { id } }] }] };
  expect(() => templateBody(body)).toThrow('cannot contain mentions');
  expect(plainText(applyTemplate({ ...draft, body }, defaults, 'merge', new Set()).body)).toContain('@someone');
});
