import { randomUUID } from 'node:crypto';
import { test, expect, registerAdmission } from './fixtures';
import type { FullProject, Project, ProjectUpdate } from '../src/projects/types';

registerAdmission(1);

test('project update notes survive date conflicts and remain immutable in the overview after reload', async ({ page, saved }) => {
  const base = `/teams/${saved.team.id}/projects`;
  const created = await saved.request<FullProject>(base, 'POST', {
    name: `Project context ${randomUUID()}`, start_date: '2026-11-01', target_date: '2026-12-01',
  }, { 'Idempotency-Key': randomUUID() });
  const path = `${base}/${created.id}`;
  const first = await saved.request<ProjectUpdate>(`${path}/updates`, 'POST', { body: 'Initial progress without reviewed notes.' }, { 'Idempotency-Key': randomUUID() });
  expect(first.context?.notes).toBeNull();
  await page.goto(`/w/${saved.workspace}/projects/${created.id}?team_id=${saved.team.id}`);
  await expect(page.getByRole('heading', { name: created.name, exact: true })).toBeVisible();
  const overview = page.getByRole('region', { name: 'Latest project update', exact: true });
  await expect(overview).toContainText('Risks and decisions were not recorded for this update.');
  await overview.getByRole('button', { name: 'View updates', exact: true }).press('Enter');
  const updates = page.getByRole('region', { name: 'Project updates', exact: true });
  await updates.getByRole('textbox', { name: 'Write an update', exact: true }).fill('Reviewed delivery context.');
  await updates.getByRole('checkbox', { name: 'Record reviewed risks and decisions in this update', exact: true }).check();
  await updates.getByRole('button', { name: 'Add risk', exact: true }).press('Enter');
  await expect(updates.getByRole('textbox', { name: 'Risk 1 title', exact: true })).toBeFocused();
  await updates.getByRole('textbox', { name: 'Risk 1 title', exact: true }).fill('Delivery dependency');
  await updates.getByRole('textbox', { name: 'Risk 1 details', exact: true }).fill('Need scope signoff.');
  await updates.getByRole('button', { name: 'Add decision', exact: true }).press('Enter');
  await updates.getByRole('textbox', { name: 'Decision 1 title', exact: true }).fill('Approve rollout date');
  const current = await saved.request<Project>(`${path}/metadata`);
  await saved.request<Project>(`${path}/metadata`, 'PATCH', { target_date: '2026-12-15' }, { 'If-Match': current.etag });
  const stale = page.waitForResponse(response => response.request().method() === 'POST' && response.url() === `${saved.origin}${saved.path}${path}/updates`);
  await updates.getByRole('button', { name: 'Publish update', exact: true }).click();
  expect((await stale).status()).toBe(412);
  await expect(updates.getByRole('heading', { name: 'The project changed before publication', exact: true })).toBeVisible();
  await expect(updates.getByRole('textbox', { name: 'Risk 1 title', exact: true })).toHaveValue('Delivery dependency');
  await updates.getByRole('button', { name: 'Use current project context', exact: true }).press('Enter');
  const published = page.waitForResponse(response => response.request().method() === 'POST' && response.url() === `${saved.origin}${saved.path}${path}/updates`);
  await updates.getByRole('button', { name: 'Publish update', exact: true }).press('Enter');
  const response = await published; expect(response.status()).toBe(201);
  const update = await response.json() as ProjectUpdate;
  expect(update.context).toMatchObject({ start_date: '2026-11-01', target_date: '2026-12-15', notes: { risks: [{ title: 'Delivery dependency', detail: 'Need scope signoff.', status: 'open' }], decisions: [{ title: 'Approve rollout date', status: 'needed' }] } });
  expect(update.previous?.id).toBe(first.id);
  await expect(updates.getByText('Update published.', { exact: true })).toBeVisible();
  await page.getByRole('button', { name: 'Overview', exact: true }).press('Enter');
  await expect(overview).toContainText('Reviewed delivery context.'); await expect(overview).toContainText('Delivery dependency'); await expect(overview).toContainText('Approve rollout date');
  const latest = await saved.request<Project>(`${path}/metadata`);
  await saved.request<Project>(`${path}/metadata`, 'PATCH', { target_date: '2027-01-15' }, { 'If-Match': latest.etag });
  await page.reload();
  await expect(overview).toContainText('Reviewed delivery context.'); await expect(overview).toContainText('Delivery dependency');
  const stored = await saved.request<ProjectUpdate>(`${path}/updates/${update.id}`);
  expect(stored.context).toEqual(update.context);
  expect((await saved.request<ProjectUpdate>(`${path}/updates/${first.id}`)).context?.target_date).toBe('2026-12-01');
});
