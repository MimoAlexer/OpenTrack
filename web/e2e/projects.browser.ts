import { randomUUID } from 'node:crypto';
import type { Page } from '@playwright/test';
import { test, expect, paragraph, type SavedWorkspace } from './fixtures';
import type { Project, ProjectIssue, ProjectProgress, ProjectUpdate } from '../src/projects/types';

const projectsPath = (saved: SavedWorkspace, id?: string) => `/teams/${saved.team.id}/projects${id ? `/${id}` : ''}`;
const projectURL = (saved: SavedWorkspace, id: string) => `/w/${saved.workspace}/projects/${id}?team_id=${saved.team.id}`;

async function createProject(saved: SavedWorkspace, prefix: string) {
  return saved.request<Project>(projectsPath(saved), 'POST', { name: `${prefix} ${randomUUID()}` }, { 'Idempotency-Key': randomUUID() });
}

async function openProject(page: Page, saved: SavedWorkspace, project: Project) {
  const response = await page.goto(projectURL(saved, project.id));
  expect(response?.ok(), 'The built frontend serves the saved project route').toBe(true);
  await expect(page.getByRole('heading', { name: project.name, exact: true })).toBeVisible();
}

test('project creation saves a formatted brief, lead and dates, with independent health and reload', async ({ page, saved }) => {
  const name = `Saved project ${randomUUID()}`;
  const briefText = 'Ship the reviewed onboarding journey';
  await saved.open(page);
  await page.getByRole('navigation', { name: 'Workspace navigation' }).getByRole('link', { name: 'Projects', exact: true }).click();
  await page.getByRole('button', { name: 'New project', exact: true }).click();
  const create = page.getByRole('dialog', { name: 'New project', exact: true });
  await create.getByRole('textbox', { name: 'Name', exact: true }).fill(name);
  await create.getByRole('textbox', { name: 'Summary', exact: true }).fill('A saved deliverable with its own scope');
  await create.getByLabel('Start date', { exact: true }).fill('2026-11-02');
  await create.getByLabel('Target date', { exact: true }).fill('2026-11-20');
  await create.getByRole('button', { name: /^Project lead/ }).click();
  await create.getByRole('listbox', { name: 'Project lead', exact: true }).getByRole('option', { name: /Example Owner/ }).click();
  const brief = create.getByRole('textbox', { name: 'Project brief', exact: true });
  await brief.fill(briefText);
  await brief.press('Control+a');
  await create.getByRole('toolbar', { name: 'Project brief formatting' }).getByRole('button', { name: 'Bold', exact: true }).click();
  const created = page.waitForResponse(response => response.request().method() === 'POST'
    && response.url() === `${saved.origin}${saved.path}${projectsPath(saved)}`);
  await create.getByRole('button', { name: 'Create project', exact: true }).click();
  const response = await created;
  expect(response.status()).toBe(201);
  const project = await response.json() as Project;
  await expect(create).toHaveCount(0);
  await expect(page.getByRole('heading', { name, exact: true })).toBeVisible();
  await expect(page.getByText('No eligible issue scope', { exact: true })).toBeVisible();
  await expect(page.getByRole('progressbar', { name: 'Project issue completion' })).toHaveCount(0);
  const persisted = await saved.request<Project>(projectsPath(saved, project.id));
  expect(persisted).toMatchObject({ name, lead_id: saved.session.user.id, start_date: '2026-11-02',
    target_date: '2026-11-20', state: 'planned', health: 'on_track' });
  expect(persisted.narrative.body).toEqual({ type: 'doc', content: [{ type: 'paragraph',
    content: [{ type: 'text', text: briefText, marks: [{ type: 'bold' }] }] }] });

  await page.getByRole('button', { name: 'Edit project', exact: true }).click();
  const edit = page.getByRole('dialog', { name: 'Edit project', exact: true });
  await edit.getByRole('combobox', { name: 'Project state', exact: true }).selectOption('active');
  await edit.getByRole('combobox', { name: 'Project health', exact: true }).selectOption('at_risk');
  await edit.getByRole('button', { name: 'Save project', exact: true }).click();
  await expect(edit).toHaveCount(0);
  await page.reload();
  const properties = page.getByRole('complementary', { name: 'Project properties', exact: true });
  await expect(properties).toContainText('Active');
  await expect(properties).toContainText('At risk');
  await expect(properties).toContainText('Example Owner');
  await expect(properties).toContainText('Nov 2, 2026');
  await expect(properties).toContainText('Nov 20, 2026');
  await expect(page.locator('.project-brief strong')).toHaveText(briefText);
  await expect(page.getByText('No eligible issue scope', { exact: true })).toBeVisible();
  expect(await saved.request<Project>(projectsPath(saved, project.id))).toMatchObject({ state: 'active', health: 'at_risk' });
});

test('project scope excludes canceled, archived and deleted work while completion and updates preserve issue facts', async ({ page, saved }) => {
  let project = await createProject(saved, 'Scope acceptance');
  const path = projectsPath(saved, project.id);
  project = await saved.request<Project>(path, 'PATCH', { state: 'active', health: 'at_risk' }, { 'If-Match': project.etag });
  const [open, done, canceled, archived, deleted] = await Promise.all(
    ['Open', 'Done', 'Canceled', 'Archived', 'Deleted'].map(label => saved.create(`${label} scope work ${randomUUID()}`)));
  await openProject(page, saved, project);
  await page.getByRole('navigation', { name: 'Project sections' }).getByRole('button', { name: 'Issues', exact: true }).click();
  await page.getByRole('button', { name: 'Add issues', exact: true }).click();
  const add = page.getByRole('dialog', { name: 'Add an issue to this project', exact: true });
  await add.getByRole('textbox', { name: 'Find an issue', exact: true }).fill(open.title);
  await add.getByRole('button').filter({ hasText: open.title }).click();
  await expect(add).toHaveCount(0);
  await expect(page.getByRole('region', { name: 'Project issues' }).getByRole('link').filter({ hasText: open.title })).toBeVisible();
  project = await saved.request<Project>(path);
  for (const issue of [done, canceled, archived, deleted]) {
    project = await saved.request<Project>(`${path}/issues/${issue.id}`, 'PUT', { included: true }, { 'If-Match': project.etag });
  }
  const doneStatus = saved.data.statuses.find(status => status.team_id === saved.team.id && status.category === 'completed')!;
  const canceledStatus = saved.data.statuses.find(status => status.team_id === saved.team.id && status.category === 'canceled')!;
  await saved.patch(await saved.issue(done.id), { status_id: doneStatus.id });
  await saved.patch(await saved.issue(canceled.id), { status_id: canceledStatus.id });
  await saved.patch(await saved.issue(archived.id), { archived: true });
  const currentDeleted = await saved.issue(deleted.id);
  const deletion = await saved.http.delete(`${saved.path}/issues/${deleted.id}`, {
    headers: { Origin: saved.origin, 'X-CSRF-Token': saved.session.csrf_token, 'If-Match': currentDeleted.etag },
  });
  expect(deletion.status()).toBe(204);

  await page.reload();
  await expect(page.getByText('50% complete', { exact: true })).toBeVisible();
  await expect(page.getByText('1 / 2 issues', { exact: true })).toBeVisible();
  await expect(page.getByText('1 canceled · excluded', { exact: true })).toBeVisible();
  await expect(page.getByRole('progressbar', { name: 'Project issue completion' })).toHaveAttribute('value', '1');
  await expect(page.getByRole('progressbar', { name: 'Project issue completion' })).toHaveAttribute('max', '2');
  const counts = await saved.request<ProjectProgress>(`${path}/progress`);
  expect(counts).toMatchObject({ completed: 1, eligible: 2, canceled: 1, open: 1, total: 3, unscoped: false });
  expect(Number.isFinite(Date.parse(counts.as_of))).toBe(true);
  const scope = await saved.request<{ data: ProjectIssue[] }>(`${path}/issues`);
  expect(scope.data.map(issue => issue.id).sort()).toEqual([open.id, done.id, canceled.id].sort());

  await page.getByRole('navigation', { name: 'Project sections' }).getByRole('button', { name: 'Updates', exact: true }).click();
  const updates = page.getByRole('region', { name: 'Project updates', exact: true });
  const updateText = 'Reviewed scope: one remaining issue, one canceled item excluded.';
  await updates.getByRole('textbox', { name: 'Write an update', exact: true }).fill(updateText);
  await updates.getByRole('button', { name: 'Publish update', exact: true }).click();
  await expect(updates.getByRole('status')).toHaveText('Update published.');
  const snapshots = await saved.request<{ data: ProjectUpdate[] }>(`${path}/updates`);
  expect(snapshots.data).toHaveLength(1);
  expect(snapshots.data[0]).toMatchObject({ body: updateText, author_id: saved.session.user.id,
    author_name: 'Example Owner', state: 'active', health: 'at_risk', completed: 1, eligible: 2 });

  const beforeCompletion = await Promise.all([open, done, canceled, archived].map(issue => saved.issue(issue.id)));
  await page.getByRole('button', { name: 'Edit project', exact: true }).click();
  const edit = page.getByRole('dialog', { name: 'Edit project', exact: true });
  await edit.getByRole('combobox', { name: 'Project state', exact: true }).selectOption('completed');
  await expect(edit.getByRole('button', { name: 'Save project', exact: true })).toBeDisabled();
  await edit.getByRole('checkbox', { name: 'I understand that completing this project leaves its issues in their current states.', exact: true }).check();
  await edit.getByRole('button', { name: 'Save project', exact: true }).click();
  await expect(edit).toHaveCount(0);
  const afterCompletion = await Promise.all([open, done, canceled, archived].map(issue => saved.issue(issue.id)));
  expect(afterCompletion).toEqual(beforeCompletion);
  project = await saved.request<Project>(path);
  expect(project.state).toBe('completed');

  await saved.patch(await saved.issue(open.id), { status_id: doneStatus.id });
  await saved.request<Project>(path, 'PATCH', { health: 'off_track' }, { 'If-Match': project.etag });
  await page.reload();
  await expect(page.getByText('100% complete', { exact: true })).toBeVisible();
  await expect(page.getByRole('complementary', { name: 'Project properties' })).toContainText('Off track');
  await page.getByRole('navigation', { name: 'Project sections' }).getByRole('button', { name: 'Updates', exact: true }).click();
  const entry = updates.getByRole('article').filter({ hasText: updateText });
  await expect(entry).toContainText('Example Owner');
  await expect(entry).toContainText('Active');
  await expect(entry).toContainText('At risk');
  await expect(entry).toContainText('1 of 2 completed');
  expect((await saved.request<{ data: ProjectUpdate[] }>(`${path}/updates`)).data).toEqual(snapshots.data);
});

test('stale project edits preserve the local brief until the saved changes are reviewed', async ({ page, saved }) => {
  const project = await createProject(saved, 'Concurrent project');
  const path = projectsPath(saved, project.id);
  await openProject(page, saved, project);
  await page.getByRole('button', { name: 'Edit project', exact: true }).click();
  const edit = page.getByRole('dialog', { name: 'Edit project', exact: true });
  await edit.getByRole('textbox', { name: 'Summary', exact: true }).fill('Reviewed local summary');
  await edit.getByRole('textbox', { name: 'Project brief', exact: true }).fill('Keep this unsaved local brief');
  const concurrent = await saved.request<Project>(path, 'PATCH', { summary: 'A separately saved summary',
    narrative: { schema_version: 1, body: paragraph('A separately saved brief') } }, { 'If-Match': project.etag });
  const stale = page.waitForResponse(response => response.request().method() === 'PATCH'
    && response.url() === `${saved.origin}${saved.path}${path}`);
  await edit.getByRole('button', { name: 'Save project', exact: true }).click();
  expect((await stale).status()).toBe(412);
  await expect(edit.getByRole('heading', { name: 'This project changed.', exact: true })).toBeVisible();
  await expect(edit).toContainText('A separately saved summary');
  await expect(edit).toContainText('A separately saved brief');
  await expect(edit.getByRole('textbox', { name: 'Summary', exact: true })).toHaveValue('Reviewed local summary');
  await expect(edit.getByRole('textbox', { name: 'Project brief', exact: true })).toHaveText('Keep this unsaved local brief');
  await expect(edit.getByRole('button', { name: 'Save project', exact: true })).toBeDisabled();
  expect(await saved.request<Project>(path)).toEqual(concurrent);
  await edit.getByRole('button', { name: 'Keep my draft for review', exact: true }).click();
  await expect(edit.getByRole('heading', { name: 'This project changed.', exact: true })).toHaveCount(0);
  expect(await saved.request<Project>(path)).toEqual(concurrent);
  await edit.getByRole('button', { name: 'Save project', exact: true }).click();
  await expect(edit).toHaveCount(0);
  await page.reload();
  await expect(page.getByText('Reviewed local summary', { exact: true })).toBeVisible();
  await expect(page.locator('.project-brief')).toContainText('Keep this unsaved local brief');
  expect(await saved.request<Project>(path)).toMatchObject({ summary: 'Reviewed local summary',
    narrative: { body: paragraph('Keep this unsaved local brief') } });
});
