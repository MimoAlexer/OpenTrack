import { createHash, randomUUID } from 'node:crypto';
import { writeFileSync } from 'node:fs';
import { test, expect, paragraph, native, transfer, exported } from './fixtures';
import type { RichBody, RichMark, RichNode } from '../src/documents/model';
import { canonicalBody } from '../src/documents/model';
import type { StructuredDocument } from '../src/documents/types';

const text = (value: string, marks?: RichMark[]): RichNode => ({ type: 'text', text: value, ...(marks ? { marks } : {}) });
const p = (...content: RichNode[]): RichNode => ({ type: 'paragraph', content });
const source = ['# Portable title', '', '## Scope', '', '### Details', '', 'Paragraph **bold** *italic* ~~strike~~ and `x()` with [safe](https://example.test/path?q=1&x=2).', '', '3. Ordered one', '   - Nested bullet', '4. Ordered two', '', '* Bullet one', '* Bullet two', '', '- [x] Done task', '- [ ] Open task', '', '> A quote.', '>', '> Second paragraph.', '', 'Line one  ', 'Line two', '', '```rust', 'let x = "<literal> & text";', '```'].join('\n');
const expected: RichBody = { type: 'doc', content: [
  ...[1, 2, 3].map((level, index): RichNode => ({ type: 'heading', attrs: { level }, content: [text(['Portable title', 'Scope', 'Details'][index])] })),
  p(text('Paragraph '), text('bold', [{ type: 'bold' }]), text(' '), text('italic', [{ type: 'italic' }]), text(' '), text('strike', [{ type: 'strike' }]), text(' and '), text('x()', [{ type: 'code' }]), text(' with '), text('safe', [{ type: 'link', attrs: { href: 'https://example.test/path?q=1&x=2' } }]), text('.')),
  { type: 'orderedList', attrs: { start: 3 }, content: [{ type: 'listItem', content: [p(text('Ordered one')), { type: 'bulletList', content: [{ type: 'listItem', content: [p(text('Nested bullet'))] }] }] }, { type: 'listItem', content: [p(text('Ordered two'))] }] },
  { type: 'bulletList', content: [{ type: 'listItem', content: [p(text('Bullet one'))] }, { type: 'listItem', content: [p(text('Bullet two'))] }] },
  { type: 'taskList', content: [{ type: 'taskItem', attrs: { checked: true }, content: [p(text('Done task'))] }, { type: 'taskItem', attrs: { checked: false }, content: [p(text('Open task'))] }] },
  { type: 'blockquote', content: [p(text('A quote.')), p(text('Second paragraph.'))] },
  p(text('Line one'), { type: 'hardBreak' }, text('Line two')),
  { type: 'codeBlock', attrs: { language: 'rust' }, content: [text('let x = "<literal> & text";')] },
] };
function semantics(value: unknown): RichBody {
  function node(value: RichNode): RichNode {
    const marks = value.marks ? [...value.marks].sort((a, b) => a.type.localeCompare(b.type)) : undefined;
    if (!value.content) return { ...value, ...(marks ? { marks } : {}) };
    const content: RichNode[] = [];
    for (const child of value.content.map(node)) { const prior = content.at(-1); if (prior?.type === 'text' && child.type === 'text' && JSON.stringify(prior.marks ?? []) === JSON.stringify(child.marks ?? [])) prior.text! += child.text; else content.push(child); }
    return { ...value, content, ...(marks ? { marks } : {}) };
  }
  return node(canonicalBody(value, true)) as RichBody;
}
const hash = (body: unknown) => createHash('sha256').update(JSON.stringify(semantics(body))).digest('hex');
function fact(name: string, data: object) { writeFileSync(test.info().outputPath(`${name}.json`), JSON.stringify(data)); }

test('reviewed Markdown replace persists an independent full semantic corpus through export and reimport', async ({ page, saved }) => {
  const original = paragraph('Original text remains until confirmation'); const issue = await saved.create(`Semantic transfer ${randomUUID()}`, original); await saved.open(page, issue); const before = await saved.document(issue.id); const description = page.getByRole('region', { name: 'Description', exact: true });
  let dialog = await transfer(page); await dialog.getByRole('combobox', { name: 'Action', exact: true }).selectOption('import'); await dialog.getByRole('combobox', { name: 'Import format', exact: true }).selectOption('markdown'); await dialog.getByRole('textbox', { name: 'Markdown', exact: true }).fill(source); await dialog.getByRole('button', { name: 'Review import', exact: true }).click();
  const preview = dialog.getByRole('region', { name: 'Import preview', exact: true }); await expect(preview.getByRole('heading', { name: 'Details', exact: true })).toBeVisible(); await expect(preview.locator('ol ul')).toContainText('Nested bullet'); await expect(preview.locator('blockquote')).toContainText('Second paragraph.'); await expect(preview.getByRole('link', { name: 'safe', exact: true })).toHaveAttribute('href', 'https://example.test/path?q=1&x=2');
  expect((await saved.document(issue.id)).body).toEqual(original); await expect(description.getByRole('textbox', { name: 'Description', exact: true })).toHaveText('Original text remains until confirmation');
  await dialog.getByRole('button', { name: 'Replace current draft', exact: true }).click(); await description.getByRole('button', { name: 'Save text now', exact: true }).click();
  await expect.poll(async () => semantics((await saved.document(issue.id)).body)).toEqual(semantics(expected)); const first = await saved.document(issue.id); expect(first.version).toBe((BigInt(before.version) + 1n).toString());
  await page.reload(); expect(semantics((await saved.document(issue.id)).body)).toEqual(semantics(expected));
  const markdown = await exported(page, 'markdown'); dialog = await transfer(page); await dialog.getByRole('combobox', { name: 'Action', exact: true }).selectOption('import'); await dialog.getByRole('combobox', { name: 'Import format', exact: true }).selectOption('markdown'); await dialog.getByRole('textbox', { name: 'Markdown', exact: true }).fill(markdown); await dialog.getByRole('button', { name: 'Review import', exact: true }).click(); await expect(dialog.getByRole('region', { name: 'Import preview', exact: true })).toBeVisible();
  expect(semantics((await saved.document(issue.id)).body)).toEqual(semantics(expected)); await dialog.getByRole('button', { name: 'Replace current draft', exact: true }).click();
  expect(semantics(JSON.parse(await exported(page, 'native')).body as RichBody)).toEqual(semantics(expected)); await description.getByRole('textbox', { name: 'Description', exact: true }).press('Control+Enter'); await expect.poll(async () => semantics((await saved.document(issue.id)).body)).toEqual(semantics(expected));
  await page.reload(); const persisted = await saved.document(issue.id); expect(semantics(persisted.body)).toEqual(semantics(expected)); fact('semantic-roundtrip', { engine: test.info().project.name, expected_sha256: hash(expected), persisted_sha256: hash(persisted.body), initial_version: before.version, first_saved_version: first.version, final_version: persisted.version, independently_specified_expected_ast: true });
});

test('reviewed append preserves an unsaved draft through a real stale save and keeps reference loss explicit', async ({ page, saved }) => {
  const target = await saved.create(`Append reference ${randomUUID()}`); const original = paragraph('Original saved document'); const issue = await saved.create(`Append conflict ${randomUUID()}`, original); await saved.open(page, issue); const before = await saved.document(issue.id); const endpoint = `${saved.origin}${saved.path}/documents/${before.id}`;
  let release!: () => void; const gate = new Promise<void>(resolve => { release = resolve; }); let held = false; let hold = true;
  await page.route(endpoint, async route => { if (route.request().method() === 'PATCH' && hold) { hold = false; held = true; await gate; } await route.continue(); });
  const description = page.getByRole('region', { name: 'Description', exact: true }); const editor = description.getByRole('textbox', { name: 'Description', exact: true });
  const imported: RichBody = { type: 'doc', content: [{ type: 'heading', attrs: { level: 3 }, content: [text('Appended outcome')] }, p(text('Reference '), { type: 'issueReference', attrs: { id: target.id } }, text(' and '), { type: 'userMention', attrs: { id: saved.session.user.id } })] }; const combined: RichBody = { type: 'doc', content: [...paragraph('Unsaved original draft').content, ...imported.content] };
  try {
    await editor.fill('Unsaved original draft'); await expect.poll(() => held).toBe(true);
    let dialog = await transfer(page); await dialog.getByRole('combobox', { name: 'Action', exact: true }).selectOption('import'); await dialog.getByRole('combobox', { name: 'Import format', exact: true }).selectOption('markdown'); await dialog.getByRole('textbox', { name: 'Markdown', exact: true }).fill('[unsafe](javascript:alert(1))'); await dialog.getByRole('button', { name: 'Review import', exact: true }).click(); await expect(dialog.getByRole('alert')).toContainText('unsafe'); await expect(dialog.getByRole('button', { name: 'Append to draft', exact: true })).toHaveCount(0); await dialog.getByRole('button', { name: 'Cancel', exact: true }).click();
    await expect(editor).toHaveText('Unsaved original draft'); expect((await saved.document(issue.id)).body).toEqual(original);
    dialog = await transfer(page); await dialog.getByRole('combobox', { name: 'Action', exact: true }).selectOption('import'); await dialog.getByRole('textbox', { name: 'Document JSON', exact: true }).fill(native(imported)); await dialog.getByRole('button', { name: 'Review import', exact: true }).click(); await expect(dialog.getByLabel('Transfer limitations')).toContainText('Saving checks current access again'); await expect(editor).toHaveText('Unsaved original draft'); expect((await saved.document(issue.id)).body).toEqual(original);
    await dialog.getByRole('button', { name: 'Append to draft', exact: true }).click(); await expect(editor).toContainText('Unsaved original draft'); await expect(editor.getByRole('heading', { name: 'Appended outcome', exact: true })).toBeVisible();
    const remote = await saved.request<StructuredDocument>(`/documents/${before.id}`, 'PATCH', { schema_version: 1, body: paragraph('Remote saved while import was reviewed') }, { 'If-Match': before.etag });
    const stale = page.waitForResponse(response => response.url() === endpoint && response.request().method() === 'PATCH'); release(); expect((await stale).status()).toBe(412);
    const conflict = page.getByRole('alert', { name: 'Document conflict', exact: true }); await expect(conflict).toContainText('Remote saved while import was reviewed'); await expect(conflict).toContainText('Appended outcome'); await expect(description.getByRole('button', { name: 'Save text now', exact: true })).toBeDisabled(); expect((await saved.document(issue.id)).body).toEqual(remote.body);
    await conflict.getByRole('button', { name: 'Keep my draft for review', exact: true }).click(); await description.getByRole('button', { name: 'Save text now', exact: true }).click(); await expect.poll(async () => semantics((await saved.document(issue.id)).body)).toEqual(semantics(combined)); const persisted = await saved.document(issue.id); expect(persisted.version).toBe((BigInt(before.version) + 2n).toString());
    await page.reload(); expect(semantics((await saved.document(issue.id)).body)).toEqual(semantics(combined)); dialog = await transfer(page); await dialog.getByRole('combobox', { name: 'Format', exact: true }).selectOption('markdown'); await expect(dialog.getByLabel('Transfer limitations')).toContainText('User mentions become the text @someone'); await expect(dialog.getByLabel('Transfer limitations')).toContainText('Issue references become the text #issue'); const lossy = await dialog.getByRole('textbox', { name: 'Exported Markdown', exact: true }).inputValue(); expect(lossy.includes(target.id) || lossy.includes(saved.session.user.id)).toBe(false); await dialog.getByRole('button', { name: 'Done', exact: true }).click(); fact('append-conflict', { engine: test.info().project.name, stale_status: 412, expected_sha256: hash(combined), persisted_sha256: hash(persisted.body), original_version: before.version, remote_version: remote.version, final_version: persisted.version, explicit_append_and_conflict_review: true });
  } finally { release(); await page.unroute(endpoint); }
});
