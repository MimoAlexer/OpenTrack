import { randomUUID } from 'node:crypto';
import { test, expect, type SavedWorkspace } from './fixtures';
import type { Issue, Session } from '../src/types';
import type { InvitationIssued, TeamDetail, TeamMember } from '../src/settings/types';
import type { Workflow, WorkflowReplacement, WorkflowStatus } from '../src/workflows/types';
import { labelETag, type Label } from '../src/taxonomy/types';

async function team(saved: SavedWorkspace) {
  return saved.request<TeamDetail>('/teams', 'POST', {
    name: `Workflow acceptance ${randomUUID().slice(0, 8)}`,
    key: `W${randomUUID().replaceAll('-', '').slice(0, 8).toUpperCase()}`,
    private: false,
  }, { 'Idempotency-Key': randomUUID() });
}

test('light workflow settings persist defaults and order, then a saved replacement survives reload and restores its source identity', async ({ page, saved }) => {
  const currentTeam = await team(saved);
  const base = `/teams/${currentTeam.id}`;
  const url = `/w/${saved.workspace}/settings/teams?team_id=${currentTeam.id}&panel=workflow`;
  const responseFor = (method: string, suffix: string) => page.waitForResponse(response =>
    response.request().method() === method && new URL(response.url()).pathname === `${saved.path}${base}${suffix}`);
  const readWorkflow = () => saved.request<Workflow>(`${base}/workflow`);
  const categoryIds = (workflow: Workflow) => workflow.statuses.filter(status => status.category === 'unstarted')
    .sort((a, b) => a.position - b.position).map(status => status.id);

  await page.goto(url);
  const workflow = page.getByRole('region', { name: `${currentTeam.name} workflow`, exact: true });
  await expect(workflow).toBeVisible();
  await page.locator('.account-menu > summary').click();
  await expect(page.locator('.account-menu')).toHaveAttribute('open', '');
  await page.getByRole('button', { name: 'Use light appearance', exact: true }).click();
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'light');
  await page.getByRole('button', { name: 'Use light appearance', exact: true }).press('Escape');

  let current = await readWorkflow();
  for (const name of ['Pending qualification', 'Accepted qualification']) {
    await workflow.getByRole('button', { name: 'Add status', exact: true }).click();
    const dialog = page.getByRole('dialog', { name: 'Add status', exact: true });
    await expect(dialog.getByRole('textbox', { name: 'Status name', exact: true })).toBeFocused();
    await dialog.getByRole('textbox', { name: 'Status name', exact: true }).fill(name);
    await dialog.getByRole('combobox', { name: 'Category', exact: true }).selectOption('unstarted');
    await dialog.getByRole('combobox', { name: 'Color', exact: true }).selectOption('teal');
    const saving = responseFor('POST', '/statuses');
    await dialog.getByRole('button', { name: 'Add status', exact: true }).click();
    const response = await saving; expect(response.status()).toBe(201);
    current = await response.json() as Workflow;
    await expect(dialog).toHaveCount(0);
  }
  const source = current.statuses.find(status => status.name === 'Pending qualification')!;
  const target = current.statuses.find(status => status.name === 'Accepted qualification')!;
  const sourceRow = workflow.locator(`#workflow-row-${source.id}`);
  await sourceRow.getByRole('button', { name: 'Edit Pending qualification', exact: true }).click();
  const edit = page.getByRole('dialog', { name: 'Edit status', exact: true });
  const sourceName = 'Ready for qualification';
  await edit.getByRole('textbox', { name: 'Status name', exact: true }).fill(sourceName);
  await edit.getByRole('combobox', { name: 'Color', exact: true }).selectOption('blue');
  const renaming = responseFor('PATCH', `/statuses/${source.id}`);
  await edit.getByRole('button', { name: 'Save status', exact: true }).focus();
  await page.keyboard.press('Enter');
  const renamed = await renaming; expect(renamed.ok()).toBe(true);
  current = await renamed.json() as Workflow;
  await expect(sourceRow.getByText(sourceName, { exact: true })).toBeVisible();
  expect(current.statuses.find(status => status.id === source.id)).toMatchObject({ name: sourceName, color: 'blue' });

  const beforeOrder = categoryIds(current);
  const expectedOrder = [...beforeOrder];
  const oldIndex = expectedOrder.indexOf(source.id); expect(oldIndex).toBeGreaterThan(0);
  [expectedOrder[oldIndex - 1], expectedOrder[oldIndex]] = [expectedOrder[oldIndex], expectedOrder[oldIndex - 1]];
  await sourceRow.getByRole('button', { name: `Move ${sourceName} up`, exact: true }).click();
  await expect(workflow.getByRole('button', { name: 'Save order', exact: true })).toBeEnabled();
  const unsaved = await readWorkflow();
  expect(unsaved.version).toBe(current.version); expect(categoryIds(unsaved)).toEqual(beforeOrder);
  const reordering = responseFor('PUT', '/workflow/order');
  await workflow.getByRole('button', { name: 'Save order', exact: true }).click();
  const reordered = await reordering; expect(reordered.ok()).toBe(true);
  current = await reordered.json() as Workflow; expect(categoryIds(current)).toEqual(expectedOrder);
  await expect(workflow.getByRole('button', { name: 'Save order', exact: true })).toHaveCount(0);

  await sourceRow.locator('summary').click();
  const changingDefault = responseFor('PUT', '/workflow/default');
  await sourceRow.getByRole('button', { name: 'Make default', exact: true }).click();
  const defaultResponse = await changingDefault; expect(defaultResponse.ok()).toBe(true);
  current = await defaultResponse.json() as Workflow;
  expect(current.statuses.filter(status => status.is_default).map(status => status.id)).toEqual([source.id]);
  await expect(sourceRow.getByText('Default', { exact: true })).toBeVisible();
  await page.reload(); await expect(page.locator('html')).toHaveAttribute('data-theme', 'light');
  await expect(sourceRow.getByText('Default', { exact: true })).toBeVisible();
  expect(categoryIds(await readWorkflow())).toEqual(expectedOrder);

  await page.goto(`/w/${saved.workspace}/issues?team_id=${currentTeam.id}`);
  await page.getByRole('button', { name: /^New issue/ }).click();
  const create = page.getByRole('dialog', { name: 'New issue', exact: true });
  await create.getByRole('combobox', { name: 'Team', exact: true }).selectOption(currentTeam.id);
  await expect(create.getByRole('combobox', { name: /^Status/ })).toHaveValue(source.id);
  const title = `Workflow default ${randomUUID().slice(0, 8)}`;
  await create.getByRole('textbox', { name: 'Title', exact: true }).fill(title);
  const creating = page.waitForResponse(response => response.request().method() === 'POST'
    && new URL(response.url()).pathname === `${saved.path}/issues`);
  await create.getByRole('button', { name: 'Create issue', exact: true }).click();
  const createdResponse = await creating; expect(createdResponse.status()).toBe(201);
  expect(createdResponse.request().postDataJSON()).not.toHaveProperty('status_id');
  const created = await createdResponse.json() as Issue;
  expect(created).toMatchObject({ team_id: currentTeam.id, status_id: source.id, title });
  await page.reload();
  await expect(page.getByRole('textbox', { name: 'Issue title', exact: true })).toHaveValue(title);
  expect((await saved.issue(created.id)).status_id).toBe(source.id);

  await page.goto(url); await sourceRow.locator('summary').click();
  await sourceRow.getByRole('button', { name: 'Replace status…', exact: true }).click();
  const replace = page.getByRole('dialog', { name: `Replace ${sourceName}`, exact: true });
  await replace.getByRole('combobox', { name: 'Replacement status', exact: true }).selectOption(target.id);
  await expect(replace).toContainText(`1 issue currently use ${sourceName}.`);
  await expect(replace).toContainText(`${target.name} will become the default immediately.`);
  const replacing = responseFor('POST', `/statuses/${source.id}/replace`);
  await replace.getByRole('button', { name: 'Replace status', exact: true }).click();
  const queuedResponse = await replacing; expect(queuedResponse.status()).toBe(202);
  const queued = await queuedResponse.json() as WorkflowReplacement;
  expect(queued).toMatchObject({ source_status_id: source.id, replacement_status_id: target.id,
    state: 'queued', matched_count: '1', processed_count: '0', remaining_count: '1' });
  await expect(replace).toHaveCount(0);
  await page.reload(); await expect(workflow).toBeVisible();
  const jobPath = `${base}/workflow-replacements/${queued.id}`;
  await expect.poll(async () => (await saved.request<WorkflowReplacement>(jobPath)).state,
    { message: 'The real restricted worker finishes the persisted replacement', timeout: 30_000 }).toBe('succeeded');
  expect(await saved.request<WorkflowReplacement>(jobPath)).toMatchObject({ processed_count: '1', remaining_count: '0' });
  await workflow.getByRole('button', { name: 'Refresh workflow', exact: true }).click();
  await expect(sourceRow).toHaveCount(0);
  const retired = await saved.request<WorkflowStatus>(`${base}/statuses/${source.id}`);
  expect(retired.archived_at).not.toBeNull(); expect(retired.replacement_job_id).toBe(queued.id);
  const moved = await saved.issue(created.id);
  expect(moved).toMatchObject({ id: created.id, key: created.key, title, status_id: target.id,
    version: (BigInt(created.version) + 1n).toString() });

  await workflow.getByRole('button', { name: 'Archived statuses', exact: true }).click();
  const archived = workflow.getByRole('region', { name: 'Archived statuses', exact: true });
  const archivedRow = archived.locator('.workflow-row').filter({ has: page.getByText(sourceName, { exact: true }) });
  await archivedRow.getByRole('button', { name: 'View replacement', exact: true }).click();
  const progress = archived.getByRole('region', { name: `Replacement of ${sourceName}`, exact: true });
  await expect(progress.getByText('Completed', { exact: true })).toBeVisible();
  await expect(progress.getByRole('status')).toHaveText('1 moved · 0 remaining');
  const restoring = responseFor('POST', `/statuses/${source.id}/restore`);
  await archivedRow.getByRole('button', { name: 'Restore', exact: true }).click();
  const restoredResponse = await restoring; expect(restoredResponse.ok()).toBe(true);
  current = await restoredResponse.json() as Workflow;
  expect(current.statuses.find(status => status.id === source.id)).toMatchObject({ name: sourceName, color: 'blue',
    archived_at: null, retiring_at: null, replacement_job_id: null, replacement_status_id: null, is_default: false });
  expect(current.statuses.filter(status => status.is_default).map(status => status.id)).toEqual([target.id]);
  await expect(sourceRow).toBeVisible();
  await page.reload(); await expect(sourceRow.getByText(sourceName, { exact: true })).toBeVisible();
  expect((await saved.issue(created.id)).status_id).toBe(target.id);
});

test('label restoration preserves a proposed name through a real name collision and stale reference review', async ({ page, saved }) => {
  const currentTeam = await team(saved);
  const name = `Restore label ${randomUUID().slice(0, 8)}`;
  let label = await saved.request<Label>('/labels', 'POST', { team_id: currentTeam.id, name, color: 'teal' },
    { 'Idempotency-Key': randomUUID() });
  const path = `/labels/${label.id}`;
  label = await saved.request<Label>(path, 'PATCH', { archived: true }, { 'If-Match': labelETag(label) });
  const occupied = await saved.request<Label>('/labels', 'POST', { team_id: currentTeam.id, name, color: 'amber' },
    { 'Idempotency-Key': randomUUID() });
  await page.goto(`/w/${saved.workspace}/settings/teams?team_id=${currentTeam.id}&panel=labels`);
  const labels = page.getByRole('region', { name: `${currentTeam.name} labels`, exact: true });
  await labels.getByRole('combobox', { name: 'Label state', exact: true }).selectOption('archived');
  await labels.locator('.settings-row').filter({ has: page.getByText(name, { exact: true }) })
    .getByRole('button', { name: 'Restore', exact: true }).click();
  const restore = page.getByRole('dialog', { name: 'Restore label', exact: true });
  const input = restore.getByRole('textbox', { name: 'Label name', exact: true });
  const responseFor = () => page.waitForResponse(response => response.request().method() === 'PATCH'
    && new URL(response.url()).pathname === `${saved.path}${path}`);
  const collision = responseFor();
  await restore.getByRole('button', { name: 'Restore label', exact: true }).click();
  expect((await collision).status()).toBe(409);
  await expect(restore.getByRole('alert')).toBeVisible(); await expect(input).toHaveValue(name);
  expect((await saved.request<Label>(path)).archived_at).not.toBeNull();

  const proposed = `${name} repaired`;
  await input.fill(proposed);
  label = await saved.request<Label>(path, 'PATCH', { archived: false, name: `${name} remote`, color: 'blue' },
    { 'If-Match': labelETag(label) });
  label = await saved.request<Label>(path, 'PATCH', { archived: true }, { 'If-Match': labelETag(label) });
  const stale = responseFor();
  await restore.getByRole('button', { name: 'Restore label', exact: true }).click();
  expect((await stale).status()).toBe(412);
  await expect(restore).toContainText('This label changed.');
  await expect(input).toHaveValue(proposed);
  await expect(restore.getByRole('button', { name: 'Restore label', exact: true })).toBeDisabled();
  await restore.getByRole('button', { name: 'Keep my draft for review', exact: true }).click();
  await expect(input).toHaveValue(proposed);
  const restoring = responseFor();
  await restore.getByRole('button', { name: 'Restore label', exact: true }).click();
  const restoredResponse = await restoring; expect(restoredResponse.ok()).toBe(true);
  expect(restoredResponse.request().headers()['if-match']).toBe(labelETag(label));
  expect(await restoredResponse.json()).toMatchObject({ id: label.id, team_id: currentTeam.id, name: proposed,
    color: 'teal', archived_at: null });
  await expect(restore).toHaveCount(0);
  await labels.getByRole('combobox', { name: 'Label state', exact: true }).selectOption('active');
  await expect(labels.getByText(proposed, { exact: true })).toBeVisible();
  await expect(labels.getByText(name, { exact: true })).toBeVisible();
  expect((await saved.request<Label>(`/labels/${occupied.id}`)).name).toBe(name);
  await page.reload();
  await expect(labels.getByText(proposed, { exact: true })).toBeVisible();
});

test('a shared label directory pages beyond one hundred saved labels without omissions or duplicates', async ({ page, saved }) => {
  const prefix = `Directory ${randomUUID().slice(0, 8)}`;
  const names = Array.from({ length: 105 }, (_, index) => `${prefix} ${String(index).padStart(3, '0')}`);
  const labels: Label[] = [];
  for (const name of names) labels.push(await saved.request<Label>('/labels', 'POST', { team_id: null, name, color: 'teal' }, { 'Idempotency-Key': randomUUID() }));
  await page.goto(`/w/${saved.workspace}/settings/labels`);
  const directory = page.getByRole('region', { name: 'Browser acceptance labels', exact: true });
  const search = directory.getByRole('searchbox', { name: 'Search labels', exact: true });
  await search.fill(prefix);
  const rows = directory.locator('.settings-row-main strong');
  await expect(rows).toHaveText(names.slice(0, 20));
  for (let size = 40; size <= 120; size += 20) {
    await directory.getByRole('button', { name: 'Load more labels', exact: true }).click();
    await expect(rows).toHaveText(names.slice(0, size));
  }
  await expect(directory.getByRole('button', { name: 'Load more labels', exact: true })).toHaveCount(0);
  expect(new Set(await rows.allTextContents()).size).toBe(105);
  expect(new Set(labels.map(label => label.id)).size).toBe(105);
  await search.fill(names[104]);
  await expect(rows).toHaveText([names[104]]);
  await expect(directory.getByRole('button', { name: 'Load more labels', exact: true })).toHaveCount(0);
  expect(await saved.request<Label>(`/labels/${labels[104].id}`)).toMatchObject({ name: names[104], team_id: null, archived_at: null });
});

test('archived issue restoration requires an eligible assignee repair and preserves it after a real version conflict', async ({ page, saved, playwright }) => {
  const team = await saved.request<TeamDetail>('/teams', 'POST', { name: `Restore scope ${randomUUID().slice(0, 8)}`,
    key: `R${randomUUID().replaceAll('-', '').slice(0, 8).toUpperCase()}`, private: true }, { 'Idempotency-Key': randomUUID() });
  const issued = await saved.request<InvitationIssued>('/invitations', 'POST', { email: `restore-${randomUUID()}@example.test`,
    role: 'member', team_ids: [team.id] }, { 'Idempotency-Key': randomUUID() });
  const member = await playwright.request.newContext({ baseURL: saved.origin, extraHTTPHeaders: { Origin: saved.origin }, storageState: { cookies: [], origins: [] } });
  try {
    expect((await member.get('/api/v1/auth/session')).status()).toBe(401);
    const accepted = await member.post('/api/v1/invitations/accept', { data: { token: issued.token,
      display_name: 'Example Member', password: process.env.OPENTRACK_BROWSER_PASSWORD } });
    expect(accepted.status()).toBe(200);
    const session = await accepted.json() as Session;
    let issue = await saved.request<Issue>('/issues', 'POST', { team_id: team.id, title: `Restore repair ${randomUUID()}`,
      assignee_id: session.user.id }, { 'Idempotency-Key': randomUUID() });
    issue = await saved.patch(issue, { archived: true });
    const membership = await saved.request<TeamMember>(`/teams/${team.id}/members/${session.user.id}`);
    const removed = await saved.http.delete(`${saved.path}/teams/${team.id}/members/${session.user.id}`, { headers: {
      Origin: saved.origin, 'X-CSRF-Token': saved.session.csrf_token,
      'If-Match': `"team-member:${team.id}:${session.user.id}:${membership.version}"` } });
    expect(removed.status()).toBe(204);
    expect((await member.get(`${saved.path}/issues/${issue.id}`)).status()).toBe(404);
    issue = await saved.issue(issue.id);
    expect(issue.assignee_id).toBe(session.user.id); expect(issue.assignee?.assignable).toBe(false);
    await page.goto(`/w/${saved.workspace}/issues/${issue.id}`);
    await page.getByRole('button', { name: 'Restore', exact: true }).click();
    const dialog = page.getByRole('dialog', { name: `Restore ${issue.key}`, exact: true });
    const submit = dialog.getByRole('button', { name: 'Restore issue', exact: true });
    await expect(submit).toBeDisabled();
    await expect(dialog.getByText('Choose an eligible assignee or Unassigned before restoring.', { exact: true })).toBeVisible();
    await dialog.getByRole('button', { name: /^Assignee/ }).click();
    await dialog.getByRole('option', { name: 'Unassigned', exact: true }).click();
    await expect(submit).toBeEnabled();
    const remote = await saved.patch(issue, { archived: false, assignee_id: saved.session.user.id });
    const current = await saved.patch(remote, { archived: true });
    const endpoint = `${saved.origin}${saved.path}/issues/${issue.id}`;
    const conflict = page.waitForResponse(response => response.url() === endpoint && response.request().method() === 'PATCH');
    await submit.click(); expect((await conflict).status()).toBe(412);
    await expect(dialog).toContainText('Your proposed repairs are preserved.');
    await expect(dialog.getByRole('button', { name: /^Assignee/ })).toContainText('Unassigned');
    await expect(submit).toBeDisabled();
    await dialog.getByRole('button', { name: 'Keep repairs for review', exact: true }).click();
    expect((await saved.issue(issue.id)).version).toBe(current.version);
    const restoring = page.waitForResponse(response => response.url() === endpoint && response.request().method() === 'PATCH');
    await submit.click();
    const response = await restoring; expect(response.status()).toBe(200);
    expect(response.request().headers()['if-match']).toBe(current.etag);
    expect(response.request().postDataJSON()).toEqual({ archived: false, assignee_id: null });
    await expect(dialog).toHaveCount(0);
    const restored = await saved.issue(issue.id);
    expect(restored).toMatchObject({ id: issue.id, title: issue.title, assignee_id: null, archived_at: null,
      version: (BigInt(current.version) + 1n).toString() });
    await page.reload();
    await expect(page.getByRole('button', { name: /^Assignee/ })).toContainText('Unassigned');
    expect((await member.get(`${saved.path}/issues/${issue.id}`)).status()).toBe(404);
  } finally { await member.dispose(); }
});
