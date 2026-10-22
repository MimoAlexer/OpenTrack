import { randomUUID } from 'node:crypto';
import { test, expect, exported, importText, native, paragraph, savedBody, transfer } from './fixtures';
import type { RichBody } from '../src/documents/model';
import type { ActivityEntry, Issue } from '../src/types';
import type { IssueTemplate, TemplatePreview } from '../src/templates/types';

test('document JSON saves references, exports exact structure, undoes and reloads', async ({ page, saved }) => {
  const target = await saved.create('Reference target');
  const original = paragraph('Original saved description');
  const issue = await saved.create('Portable document acceptance', original);
  const body: RichBody = { type: 'doc', content: [
    { type: 'heading', attrs: { level: 2 }, content: [{ type: 'text', text: 'Reviewed document' }] },
    { type: 'paragraph', content: [{ type: 'text', text: 'Strong text', marks: [{ type: 'bold' }] },
      { type: 'text', text: ' with ' }, { type: 'issueReference', attrs: { id: target.id } },
      { type: 'text', text: ' and ' }, { type: 'userMention', attrs: { id: saved.session.user.id } }] },
    { type: 'codeBlock', attrs: { language: 'rust' }, content: [{ type: 'text', text: 'fn main() {}' }] },
  ] };
  await saved.open(page, issue);
  await importText(page, native(body), 'native');
  await savedBody(saved, issue.id, body);
  expect(JSON.parse(await exported(page, 'native')).body).toEqual(body);
  const description = page.getByRole('region', { name: 'Description', exact: true });
  await description.getByRole('button', { name: 'Undo', exact: true }).click();
  await savedBody(saved, issue.id, original);
  await description.getByRole('button', { name: 'Redo', exact: true }).click();
  await savedBody(saved, issue.id, body);
  await page.reload();
  await expect(description.getByRole('link', { name: `${target.key}: ${target.title}`, exact: true })).toBeVisible();
  expect(JSON.parse(await exported(page, 'native')).body).toEqual(body);
  const document = await saved.document(issue.id);
  expect(document.issue_references?.some(reference => reference.id === target.id)).toBe(true);
  const markdown = await transfer(page);
  await markdown.getByRole('combobox', { name: 'Format', exact: true }).selectOption('markdown');
  await expect(markdown.getByLabel('Transfer limitations')).toContainText('Issue references become the text #issue');
  await expect(markdown.getByLabel('Transfer limitations')).toContainText('User mentions become the text @someone');
  await markdown.getByRole('button', { name: 'Done', exact: true }).click();
});

test('Markdown worker runs under production CSP, round-trips saved semantics and keeps hostile text inert', async ({ page, saved }) => {
  const issue = await saved.create('Markdown and CSP acceptance');
  const response = await saved.open(page, issue);
  expect(response.headers()['content-security-policy']).toContain("script-src 'self'");
  const source = '# Markdown heading\n\n**Strong** and *emphasis* with ~~strike~~.\n\n- [x] Reviewed task\n- [ ] Pending task\n\n```rust\nfn main() {}\n```\n\n<script>window.__hostileExecuted = true</script>\n\n<img src="x" onerror="window.__hostileExecuted = true">';
  const workers: string[] = [];
  page.on('worker', worker => workers.push(worker.url()));
  await importText(page, source, 'markdown');
  expect(workers).toHaveLength(1);
  const workerURL = new URL(workers[0]);
  expect(workerURL.origin).toBe(saved.origin);
  expect(workerURL.pathname).toMatch(/^\/assets\/portability\.worker-.*\.js$/);
  const body = JSON.parse(await exported(page, 'native')).body as RichBody;
  await savedBody(saved, issue.id, body);
  await page.reload();
  const description = page.getByRole('region', { name: 'Description', exact: true });
  await expect(description.getByRole('heading', { name: 'Markdown heading' })).toBeVisible();
  await expect(description.locator('strong')).toHaveText('Strong');
  await expect(description.locator('script, img, iframe')).toHaveCount(0);
  expect(await page.evaluate(() => Reflect.get(window, '__hostileExecuted'))).toBeUndefined();
  const markdown = await exported(page, 'markdown');
  await importText(page, markdown, 'markdown');
  await savedBody(saved, issue.id, body);
  expect(JSON.parse(await exported(page, 'native')).body).toEqual(body);
  const dialog = await transfer(page);
  await dialog.getByRole('combobox', { name: 'Action', exact: true }).selectOption('import');
  await dialog.getByRole('combobox', { name: 'Import format', exact: true }).selectOption('markdown');
  await dialog.getByRole('textbox', { name: 'Markdown', exact: true }).fill('[unsafe](javascript:alert(1))');
  await dialog.getByRole('button', { name: 'Review import', exact: true }).click();
  await expect(dialog.getByRole('alert')).toBeVisible();
  await expect(dialog.getByRole('button', { name: 'Replace current draft' })).toHaveCount(0);
  await dialog.getByRole('button', { name: 'Cancel', exact: true }).click();
  expect((await saved.document(issue.id)).body).toEqual(body);
  // A real browser CSP decision, without contacting any external origin.
  const blocked = await page.evaluate(async () => {
    const url = URL.createObjectURL(new Blob(['postMessage("unexpected")'], { type: 'text/javascript' }));
    return new Promise<boolean>(resolve => {
      let worker: Worker | undefined;
      const finish = (value: boolean) => { clearTimeout(timer); worker?.terminate(); URL.revokeObjectURL(url); document.removeEventListener('securitypolicyviolation', violation); resolve(value); };
      const violation = (event: SecurityPolicyViolationEvent) => { if (event.blockedURI === 'blob' || event.blockedURI.startsWith('blob:')) finish(true); };
      const timer = setTimeout(() => finish(false), 3000);
      document.addEventListener('securitypolicyviolation', violation);
      try { worker = new Worker(url); worker.onmessage = () => finish(false); }
      catch { /* Wait for the policy violation event rather than treating any exception as proof. */ }
    });
  });
  expect(blocked, 'The app CSP rejects blob workers while its bundled same-origin module worker succeeds').toBe(true);
});

test('character preference persists while visible controls, palette and sequences remain usable', async ({ page, saved }) => {
  const preference = await saved.request<{ etag: string }>('/preferences');
  await saved.request('/preferences', 'PATCH', { shortcut_mode: 'enabled' }, { 'If-Match': preference.etag });
  const issue = await saved.create('Keyboard acceptance');
  await saved.open(page, issue);
  const comment = page.getByRole('textbox', { name: 'Comment', exact: true });
  await comment.pressSequentially('c g m / normal comment text');
  await expect(page.getByRole('dialog', { name: 'New issue', exact: true })).toHaveCount(0);
  await comment.press('Control+Enter');
  await expect(page.locator('.comment-list')).toContainText('c g m / normal comment text');
  await page.getByRole('button', { name: 'Close issue', exact: true }).click();
  await page.getByRole('button', { name: /^Keyboard shortcuts/ }).click();
  const help = page.getByRole('dialog', { name: 'Keyboard shortcuts', exact: true });
  await help.getByLabel('Enable character shortcuts').uncheck();
  await expect(help.getByRole('status')).toHaveText('Character shortcuts are disabled.');
  await help.getByRole('button', { name: 'Done', exact: true }).click();
  await page.reload();
  await expect(page.getByRole('button', { name: 'Search commands', exact: true })).toBeEnabled();
  await page.locator('body').click({ position: { x: 5, y: 5 } });
  const unchangedURL = page.url();
  await page.keyboard.press('c');
  await page.keyboard.press('g');
  await page.keyboard.press('m');
  await expect(page.getByRole('dialog')).toHaveCount(0);
  expect(page.url()).toBe(unchangedURL);
  await page.keyboard.press('Control+k');
  const palette = page.getByRole('dialog', { name: 'Commands', exact: true });
  await palette.getByRole('combobox', { name: 'Search commands' }).fill('Create issue');
  await page.keyboard.press('Enter');
  const create = page.getByRole('dialog', { name: 'New issue', exact: true });
  await expect(create).toBeVisible();
  await create.getByRole('button', { name: 'Cancel', exact: true }).click();
  await page.getByRole('button', { name: /^New issue/ }).click();
  await expect(create).toBeVisible();
  await create.getByRole('button', { name: 'Cancel', exact: true }).click();
  await page.getByRole('button', { name: /^Keyboard shortcuts/ }).click();
  await help.getByLabel('Enable character shortcuts').check();
  await expect(help.getByRole('status')).toHaveText('Character shortcuts are enabled.');
  await help.getByRole('button', { name: 'Done', exact: true }).click();
  await page.locator('body').click({ position: { x: 5, y: 5 } });
  await page.keyboard.press('g');
  await page.keyboard.press('m');
  await expect(page.getByRole('heading', { name: 'My work', exact: true })).toBeVisible();
  await page.getByRole('link', { name: 'All issues', exact: true }).click();
  await page.locator('body').click({ position: { x: 5, y: 5 } });
  await page.keyboard.press('g');
  // This deliberate wait crosses the documented 800 ms sequence boundary.
  await page.waitForTimeout(900);
  await page.keyboard.press('m');
  await expect(page.getByRole('heading', { name: 'All issues', exact: true })).toBeVisible();
  // Synthetic composition guards only; this does not qualify a native OS IME.
  await page.locator('body').dispatchEvent('compositionstart', { data: '' });
  await page.keyboard.press('c');
  await expect(create).toHaveCount(0);
  await page.locator('body').dispatchEvent('compositionend', { data: '' });
  await page.keyboard.press('c');
  await expect(create).toBeVisible();
  await create.getByRole('button', { name: 'Cancel', exact: true }).click();
});

test('nested transfer Escape unwinds once and Mod+Enter cannot save or create behind it', async ({ page, saved }) => {
  const issue = await saved.create('Nested dialog acceptance');
  await saved.open(page, issue);
  const behindDialogWrites: string[] = [];
  page.on('request', request => {
    if (['POST', 'PATCH'].includes(request.method()) && request.url().startsWith(`${saved.origin}${saved.path}/issues`)) behindDialogWrites.push(request.method());
  });
  await page.getByRole('textbox', { name: 'Issue title', exact: true }).fill('Unsaved metadata stays local');
  const dialog = await transfer(page);
  for (const mode of ['export', 'import']) {
    await dialog.getByRole('combobox', { name: 'Action', exact: true }).selectOption(mode);
    await dialog.getByRole('textbox', { name: mode === 'export' ? 'Exported document JSON' : 'Document JSON', exact: true }).focus();
    for (const modifier of ['Control', 'Meta']) await page.keyboard.press(`${modifier}+Enter`);
    await expect(dialog).toBeVisible();
  }
  await page.evaluate(() => new Promise<void>(resolve => requestAnimationFrame(() => requestAnimationFrame(() => resolve()))));
  expect(behindDialogWrites, 'Nested transfer shortcuts do not send issue mutations').toEqual([]);
  expect((await saved.issue(issue.id)).title).toBe(issue.title);
  await page.keyboard.press('Escape');
  await expect(dialog).toHaveCount(0);
  await expect(page.getByRole('region', { name: 'Issue detail', exact: true })).toBeVisible();
  await expect(page.getByRole('textbox', { name: 'Issue title', exact: true })).toHaveValue('Unsaved metadata stays local');
  await page.getByRole('textbox', { name: 'Issue title', exact: true }).fill(issue.title);
  await page.getByRole('button', { name: 'Close issue', exact: true }).click();
  await page.getByRole('button', { name: /^New issue/ }).click();
  const create = page.getByRole('dialog', { name: 'New issue', exact: true });
  const title = `Nested draft ${randomUUID()}`;
  await create.getByRole('textbox', { name: 'Title', exact: true }).fill(title);
  await create.getByRole('button', { name: 'Import / export text' }).click();
  await dialog.getByRole('combobox', { name: 'Action', exact: true }).selectOption('import');
  await dialog.getByRole('textbox', { name: 'Document JSON', exact: true }).fill(native(paragraph('Reviewed nested draft')));
  for (const modifier of ['Control', 'Meta']) await page.keyboard.press(`${modifier}+Enter`);
  await expect(dialog).toBeVisible();
  await expect(page.locator('dialog[open]')).toHaveCount(2);
  expect(behindDialogWrites).toEqual([]);
  const existing = await saved.request<{ data: Issue[] }>(`/issues?q=${encodeURIComponent(title)}`);
  expect(existing.data.filter(item => item.title === title)).toHaveLength(0);
  await page.keyboard.press('Escape');
  await expect(dialog).toHaveCount(0);
  await expect(create).toBeVisible();
  await expect(create.getByRole('textbox', { name: 'Title', exact: true })).toHaveValue(title);
  await create.getByRole('button', { name: 'Cancel', exact: true }).click();
  await page.getByRole('dialog', { name: 'Discard this issue draft?' }).getByRole('button', { name: 'Discard draft', exact: true }).click();
});

test('template preview materializes titles and requires cancel, merge or replace before durable creation', async ({ page, saved }) => {
  const path = `/teams/${saved.team.id}/templates`;
  const template = await saved.request<IssueTemplate>(path, 'POST', { name: `Reviewed pattern ${randomUUID()}`,
    title: '{{team.key}} {{team.name}} {{date}}', title_mode: 'pattern', document: { schema_version: 1, body: paragraph('Template description') },
    defaults: { priority: 2, status_id: null, label_ids: [] } }, { 'Idempotency-Key': randomUUID() });
  await saved.open(page);
  await page.getByRole('button', { name: /^New issue/ }).click();
  const create = page.getByRole('dialog', { name: 'New issue', exact: true });
  await create.getByRole('textbox', { name: 'Title', exact: true }).fill('Keep reviewed draft');
  await create.getByRole('textbox', { name: 'Description', exact: true }).fill('Original draft description');
  await create.getByRole('combobox', { name: 'Priority', exact: true }).selectOption('3');
  const chooser = page.getByRole('dialog', { name: 'Choose an issue template', exact: true });
  const choose = async (trigger: string) => {
    await create.getByRole('button', { name: trigger, exact: true }).click();
    await chooser.getByRole('button').filter({ has: page.getByText(template.name, { exact: true }) }).click();
    await expect(chooser.getByRole('region', { name: 'Template preview' })).toBeVisible();
  };
  await choose('Use a template');
  const materialized = await chooser.getByRole('region', { name: 'Template preview' }).locator('h4').innerText();
  expect(materialized).toMatch(/^EXA Example Team \d{4}-\d{2}-\d{2}$/);
  await chooser.getByRole('button', { name: 'Cancel', exact: true }).click();
  await expect(create.getByRole('textbox', { name: 'Title', exact: true })).toHaveValue('Keep reviewed draft');
  await choose('Use a template');
  await chooser.getByRole('button', { name: 'Merge into draft', exact: true }).click();
  await expect(create.getByRole('textbox', { name: 'Title', exact: true })).toHaveValue('Keep reviewed draft');
  await expect(create.getByRole('textbox', { name: 'Description', exact: true })).toContainText('Original draft description');
  await expect(create.getByRole('textbox', { name: 'Description', exact: true })).toContainText('Template description');
  await expect(create.getByRole('combobox', { name: 'Priority', exact: true })).toHaveValue('3');
  await choose(template.name);
  const replacementTitle = await chooser.getByRole('region', { name: 'Template preview' }).locator('h4').innerText();
  await chooser.getByRole('button', { name: 'Replace template fields', exact: true }).click();
  await expect(create.getByRole('textbox', { name: 'Title', exact: true })).toHaveValue(replacementTitle);
  await expect(create.getByRole('textbox', { name: 'Description', exact: true })).toHaveText('Template description');
  await expect(create.getByRole('combobox', { name: 'Priority', exact: true })).toHaveValue('2');
  const created = page.waitForResponse(response => response.request().method() === 'POST' && response.url() === `${saved.origin}${saved.path}/issues`);
  await create.getByRole('button', { name: 'Create issue', exact: true }).click();
  const issue = await (await created).json() as Issue;
  expect(issue.title).toBe(replacementTitle);
  await expect(page.getByRole('textbox', { name: 'Issue title', exact: true })).toHaveValue(replacementTitle);
  await saved.request<IssueTemplate>(`${path}/${template.id}`, 'PATCH', { title: 'Changed future title', title_mode: 'literal' }, { 'If-Match': template.etag });
  const preview = await saved.request<TemplatePreview>(`${path}/${template.id}/preview`);
  expect(preview.materialized_title).toBe('Changed future title');
  await page.reload();
  await expect(page.getByRole('textbox', { name: 'Issue title', exact: true })).toHaveValue(replacementTitle);
  expect((await saved.document(issue.id)).body).toEqual(paragraph('Template description'));
});

test('activity retains attributable facts and every event across grouped pagination', async ({ page, saved }) => {
  let issue = await saved.create('Activity acceptance');
  for (let index = 0; index < 27; index++) issue = await saved.patch(issue, { priority: index % 2 + 1 });
  await saved.open(page, issue);
  const nextStatus = saved.data.statuses.find(status => status.team_id === saved.team.id && status.id !== issue.status_id)!;
  const previousStatus = saved.data.statuses.find(status => status.id === issue.status_id)!;
  await page.getByRole('complementary', { name: 'Issue properties', exact: true }).getByRole('combobox', { name: 'Status', exact: true }).selectOption(nextStatus.id);
  await page.getByRole('button', { name: 'Save changes', exact: true }).click();
  const activity = page.getByRole('region', { name: 'Activity', exact: true });
  await expect(activity).toContainText(`${previousStatus.name} → ${nextStatus.name}`);
  await expect(activity).toContainText('Example Owner');
  await expect(activity.locator('.activity-list p')).toHaveCount(25);
  await expect(activity.getByRole('button', { name: 'Load older activity', exact: true })).toBeEnabled();
  const first = await saved.request<{ data: ActivityEntry[]; has_more: boolean; next_cursor: string }>(`/issues/${issue.id}/activity-page?limit=25`);
  expect(first.data).toHaveLength(25);
  expect(first.has_more).toBe(true);
  const older = await saved.request<{ data: ActivityEntry[] }>(`/issues/${issue.id}/activity-page?limit=25&cursor=${encodeURIComponent(first.next_cursor)}`);
  const all = [...older.data, ...first.data];
  expect(new Set(all.map(item => item.id)).size).toBe(29);
  await activity.getByRole('button', { name: 'Load older activity', exact: true }).click();
  await expect(activity.locator('.activity-list p')).toHaveCount(29);
  await expect(activity.getByRole('button', { name: 'Load older activity', exact: true })).toHaveCount(0);
  for (const group of await activity.locator('details').all()) {
    await group.locator('summary').click();
    const events = group.getByRole('listitem');
    expect(await events.count()).toBeLessThanOrEqual(5);
    await expect(events.first().locator('time')).toBeVisible();
  }
  await expect(activity).toContainText('Urgent → High');
  await expect(activity).toContainText('created this issue');
  await page.reload();
  await expect(activity.locator('.activity-list p')).toHaveCount(25);
});
