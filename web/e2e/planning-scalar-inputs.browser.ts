import { randomUUID } from 'node:crypto';
import type { Locator, Page } from '@playwright/test';
import { test, expect, type SavedWorkspace } from './fixtures';
import type { Team } from '../src/types';
import type { FullProject, Project } from '../src/projects/types';
import type { Milestone, MilestoneOverview } from '../src/projects/Milestones';
import type { Cycle } from '../src/cycles/types';

async function team(saved: SavedWorkspace) {
  return saved.request<Team>('/teams', 'POST', { name: `Scalar planning ${randomUUID()}`, key: `U${randomUUID().replaceAll('-', '').slice(0, 8).toUpperCase()}`, private: false }, { 'Idempotency-Key': randomUUID() });
}
async function nativeText(input: Locator, text: string) {
  await input.fill('');
  await input.pressSequentially(text);
  const actual = await input.inputValue();
  expect.soft(actual, 'Native sequential input preserves every supplementary scalar').toBe(text);
  return actual === text;
}
async function write(page: Page, dialog: Locator, action: string, path: string, method: string, status: number) {
  const pending = page.waitForResponse(response => new URL(response.url()).pathname === path && response.request().method() === method);
  await dialog.getByRole('button', { name: action, exact: true }).click();
  const response = await pending;
  expect(response.status()).toBe(status);
  return response;
}
async function rejected(dialog: Locator, fields: [string, string][]) {
  await expect(dialog.getByRole('alert')).toContainText('must contain');
  for (const [label, value] of fields) await expect(dialog.getByRole('textbox', { name: label, exact: true })).toHaveValue(value);
}

test('project supplementary names and summaries survive create, rejected writes, repair and reload', async ({ page, saved }) => {
  const owning = await team(saved); const path = `/teams/${owning.id}/projects`; const api = `${saved.path}${path}`;
  const list = () => saved.request<{ data: Project[] }>(path);
  await page.goto(`/w/${saved.workspace}/projects?team_id=${owning.id}`);
  await page.getByRole('button', { name: 'New project', exact: true }).click();
  const create = page.getByRole('dialog', { name: 'New project', exact: true });
  const name = create.getByRole('textbox', { name: 'Name', exact: true }); const summary = create.getByRole('textbox', { name: 'Summary', exact: true });
  const initialName = '😀'.repeat(160); const initialSummary = '🧭'.repeat(500);
  const namePreserved = await nativeText(name, initialName); const summaryPreserved = await nativeText(summary, initialSummary);
  if (!namePreserved || !summaryPreserved) return;
  expect((await list()).data).toHaveLength(0);
  await nativeText(name, '😀'.repeat(161));
  let response = await write(page, create, 'Create project', api, 'POST', 422);
  expect(response.request().postDataJSON()).toMatchObject({ name: '😀'.repeat(161), summary: initialSummary });
  await rejected(create, [['Name', '😀'.repeat(161)], ['Summary', initialSummary]]); expect((await list()).data).toHaveLength(0);
  await nativeText(name, initialName); await nativeText(summary, '🧭'.repeat(501));
  response = await write(page, create, 'Create project', api, 'POST', 422);
  expect(response.request().postDataJSON()).toMatchObject({ name: initialName, summary: '🧭'.repeat(501) });
  await rejected(create, [['Name', initialName], ['Summary', '🧭'.repeat(501)]]); expect((await list()).data).toHaveLength(0);
  await nativeText(summary, initialSummary);
  response = await write(page, create, 'Create project', api, 'POST', 201);
  const created = await response.json() as FullProject; await expect(create).toHaveCount(0);
  const metadata = `${path}/${created.id}/metadata`; const read = () => saved.request<Project>(metadata);
  let current = await read(); expect(current).toMatchObject({ name: initialName, summary: initialSummary, version: '1', state: 'planned' }); expect((await list()).data).toHaveLength(1);
  await page.reload(); await expect(page.getByRole('heading', { name: initialName, exact: true })).toBeVisible();
  await page.getByRole('button', { name: 'Edit project', exact: true }).click();
  const edit = page.getByRole('dialog', { name: 'Edit project', exact: true });
  await expect(edit.getByRole('textbox', { name: 'Name', exact: true })).toHaveValue(initialName); await expect(edit.getByRole('textbox', { name: 'Summary', exact: true })).toHaveValue(initialSummary);
  for (const [label, field, limit, glyph] of [['Name', 'name', 160, '🚀'], ['Summary', 'summary', 500, '🌍']] as const) {
    const input = edit.getByRole('textbox', { name: label, exact: true }); const overlong = glyph.repeat(limit + 1); const repaired = glyph.repeat(limit);
    await nativeText(input, overlong);
    response = await write(page, edit, 'Save project', `${saved.path}${metadata}`, 'PATCH', 422);
    expect(response.request().postDataJSON()[field]).toBe(overlong); await rejected(edit, [[label, overlong]]); expect(await read()).toEqual(current);
    await nativeText(input, repaired); await write(page, edit, 'Save project', `${saved.path}${metadata}`, 'PATCH', 200); await expect(edit).toHaveCount(0);
    const updated = await read(); expect(updated[field]).toBe(repaired); expect(updated.version).toBe((BigInt(current.version) + 1n).toString()); current = updated;
    await page.reload(); await page.getByRole('button', { name: 'Edit project', exact: true }).click(); await expect(input).toHaveValue(repaired);
  }
  await expect(edit.getByRole('textbox', { name: 'Name', exact: true })).toHaveValue('🚀'.repeat(160)); await expect(edit.getByRole('textbox', { name: 'Summary', exact: true })).toHaveValue('🌍'.repeat(500));
  expect(await read()).toMatchObject({ name: '🚀'.repeat(160), summary: '🌍'.repeat(500), version: '3' }); expect((await list()).data).toHaveLength(1);
});

test('milestone and cycle supplementary text survives create, rejected writes, repair and reload', async ({ page, saved }) => {
  const owning = await team(saved); const projects = `/teams/${owning.id}/projects`; const cycles = `/teams/${owning.id}/cycles`;
  const project = await saved.request<FullProject>(projects, 'POST', { name: 'Scalar planning outcomes' }, { 'Idempotency-Key': randomUUID() });
  const path = `${projects}/${project.id}/milestones`; const projectUrl = `/w/${saved.workspace}/projects/${project.id}?team_id=${owning.id}`;
  const listMilestones = () => saved.request<{ data: MilestoneOverview[] }>(path); const listCycles = () => saved.request<{ data: Cycle[] }>(cycles);
  await page.goto(projectUrl); await page.getByRole('button', { name: 'New milestone', exact: true }).click();
  let dialog = page.getByRole('dialog', { name: 'New milestone', exact: true }); const title = '😀'.repeat(160);
  const milestonePreserved = await nativeText(dialog.getByRole('textbox', { name: 'Title', exact: true }), title);
  page.once('dialog', prompt => prompt.accept()); await dialog.getByRole('button', { name: 'Cancel', exact: true }).click();
  await page.goto(`/w/${saved.workspace}/cycles?team_id=${owning.id}`); await page.getByRole('button', { name: 'New cycle', exact: true }).click();
  dialog = page.getByRole('dialog', { name: 'New cycle', exact: true }); const name = '🧭'.repeat(160);
  const cyclePreserved = await nativeText(dialog.getByRole('textbox', { name: 'Cycle name', exact: true }), name);
  if (!milestonePreserved || !cyclePreserved) return;
  await dialog.getByLabel('Start date', { exact: true }).fill('2026-03-28'); await dialog.getByLabel('End date (exclusive)', { exact: true }).fill('2026-03-31'); await dialog.getByRole('textbox', { name: 'Timezone', exact: true }).fill('Europe/Berlin');
  await nativeText(dialog.getByRole('textbox', { name: 'Cycle name', exact: true }), '🧭'.repeat(161));
  let response = await write(page, dialog, 'Create cycle', `${saved.path}${cycles}`, 'POST', 422);
  expect(response.request().postDataJSON().name).toBe('🧭'.repeat(161)); await rejected(dialog, [['Cycle name', '🧭'.repeat(161)]]); expect((await listCycles()).data).toHaveLength(0);
  await nativeText(dialog.getByRole('textbox', { name: 'Cycle name', exact: true }), name); response = await write(page, dialog, 'Create cycle', `${saved.path}${cycles}`, 'POST', 201);
  const createdCycle = await response.json() as Cycle; await expect(dialog).toHaveCount(0);
  const cyclePath = `${cycles}/${createdCycle.id}`; const readCycle = () => saved.request<Cycle>(cyclePath); const currentCycle = await readCycle();
  expect(currentCycle).toMatchObject({ name, version: '1', timezone: 'Europe/Berlin', start_date: '2026-03-28', end_date_exclusive: '2026-03-31', starts_at: '2026-03-27T23:00:00Z', ends_at: '2026-03-30T22:00:00Z' });
  await page.reload(); await expect(page.getByRole('heading', { name, exact: true })).toBeVisible(); await page.getByRole('button', { name: 'Edit cycle', exact: true }).click();
  dialog = page.getByRole('dialog', { name: 'Edit cycle', exact: true }); await expect(dialog.getByRole('textbox', { name: 'Cycle name', exact: true })).toHaveValue(name);
  await nativeText(dialog.getByRole('textbox', { name: 'Cycle name', exact: true }), '🚀'.repeat(161)); response = await write(page, dialog, 'Save cycle', `${saved.path}${cyclePath}`, 'PATCH', 422);
  expect(response.request().postDataJSON().name).toBe('🚀'.repeat(161)); await rejected(dialog, [['Cycle name', '🚀'.repeat(161)]]); expect(await readCycle()).toEqual(currentCycle);
  await nativeText(dialog.getByRole('textbox', { name: 'Cycle name', exact: true }), '🚀'.repeat(160)); await write(page, dialog, 'Save cycle', `${saved.path}${cyclePath}`, 'PATCH', 200); await expect(dialog).toHaveCount(0);
  expect(await readCycle()).toMatchObject({ ...currentCycle, name: '🚀'.repeat(160), version: '2', etag: expect.any(String), updated_at: expect.any(String) }); expect((await listCycles()).data).toHaveLength(1);
  await page.reload(); await page.getByRole('button', { name: 'Edit cycle', exact: true }).click(); await expect(dialog.getByRole('textbox', { name: 'Cycle name', exact: true })).toHaveValue('🚀'.repeat(160)); await dialog.getByRole('button', { name: 'Cancel', exact: true }).click();
  await page.goto(projectUrl); await page.getByRole('button', { name: 'New milestone', exact: true }).click(); dialog = page.getByRole('dialog', { name: 'New milestone', exact: true });
  const originalProject = await saved.request<Project>(`${projects}/${project.id}/metadata`);
  await nativeText(dialog.getByRole('textbox', { name: 'Title', exact: true }), '😀'.repeat(161)); response = await write(page, dialog, 'Create milestone', `${saved.path}${path}`, 'POST', 422);
  expect(response.request().postDataJSON().title).toBe('😀'.repeat(161)); await rejected(dialog, [['Title', '😀'.repeat(161)]]); expect((await listMilestones()).data).toHaveLength(0); expect(await saved.request<Project>(`${projects}/${project.id}/metadata`)).toEqual(originalProject);
  await nativeText(dialog.getByRole('textbox', { name: 'Title', exact: true }), title); response = await write(page, dialog, 'Create milestone', `${saved.path}${path}`, 'POST', 201);
  const milestone = await response.json() as Milestone; await expect(dialog).toHaveCount(0); const readMilestone = () => saved.request<Milestone>(`${path}/${milestone.id}`); const currentMilestone = await readMilestone(); expect(currentMilestone).toMatchObject({ title, version: '1' });
  await page.reload(); await page.getByRole('button', { name: `Edit ${title}`, exact: true }).click(); dialog = page.getByRole('dialog', { name: 'Edit milestone', exact: true }); await expect(dialog.getByRole('textbox', { name: 'Title', exact: true })).toHaveValue(title);
  const beforeEditProject = await saved.request<Project>(`${projects}/${project.id}/metadata`);
  await nativeText(dialog.getByRole('textbox', { name: 'Title', exact: true }), '🌍'.repeat(161)); response = await write(page, dialog, 'Save milestone', `${saved.path}${path}/${milestone.id}`, 'PATCH', 422);
  expect(response.request().postDataJSON().title).toBe('🌍'.repeat(161)); await rejected(dialog, [['Title', '🌍'.repeat(161)]]); expect(await readMilestone()).toEqual(currentMilestone); expect(await saved.request<Project>(`${projects}/${project.id}/metadata`)).toEqual(beforeEditProject);
  await nativeText(dialog.getByRole('textbox', { name: 'Title', exact: true }), '🌍'.repeat(160)); await write(page, dialog, 'Save milestone', `${saved.path}${path}/${milestone.id}`, 'PATCH', 200); await expect(dialog).toHaveCount(0);
  expect(await readMilestone()).toMatchObject({ title: '🌍'.repeat(160), version: '2' }); expect((await listMilestones()).data).toHaveLength(1);
  await page.reload(); await page.getByRole('button', { name: `Edit ${'🌍'.repeat(160)}`, exact: true }).click(); await expect(dialog.getByRole('textbox', { name: 'Title', exact: true })).toHaveValue('🌍'.repeat(160));
});
