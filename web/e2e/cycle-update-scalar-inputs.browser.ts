import { createHash, randomUUID } from 'node:crypto';
import { writeFileSync } from 'node:fs';
import type { Locator, Page } from '@playwright/test';
import { test, expect, type SavedWorkspace } from './fixtures';
import type { Issue, Team } from '../src/types';
import type { ClosePreview, CloseReport, Cycle, SnapshotPoint } from '../src/cycles/types';
import type { FullProject, Project, ProjectUpdate } from '../src/projects/types';

const digest = (text: string) => createHash('sha256').update(text).digest('hex');
let bulkStep = 0;
function fact(name: string, value: object) { writeFileSync(test.info().outputPath(`${name}.json`), JSON.stringify(value)); }
async function team(saved: SavedWorkspace) {
  return saved.request<Team>('/teams', 'POST', { name: `Long planning text ${randomUUID()}`, key: `L${randomUUID().replaceAll('-', '').slice(0, 8).toUpperCase()}`, private: false }, { 'Idempotency-Key': randomUUID() });
}
async function bulk(page: Page, input: Locator, control: string, text: string) {
  await input.fill(''); await input.focus(); await page.keyboard.insertText(text);
  const actual = await input.inputValue();
  fact(`bulk-input-${control}-${bulkStep++}`, { kind: 'native_bulk_input', engine: test.info().project.name, control, expected_scalars: Array.from(text).length, actual_scalars: Array.from(actual).length, expected_utf16: text.length, actual_utf16: actual.length, expected_sha256: digest(text), actual_sha256: digest(actual) });
  expect.soft(actual, 'Native bulk input preserves the full supplementary text').toBe(text); return actual === text;
}
async function write(page: Page, scope: Locator, action: string, path: string, status: number) {
  const pending = page.waitForResponse(response => new URL(response.url()).pathname === `${path}` && response.request().method() === 'POST');
  await scope.getByRole('button', { name: action, exact: true }).click(); const response = await pending; expect(response.status()).toBe(status); return response;
}
function cycleFacts(saved: SavedWorkspace, cycle: Cycle, reason: string, expectedRows: number) {
  fact('cycle-reopen-facts', { kind: 'cycle_reopen_facts', engine: test.info().project.name, actor_id: saved.session.user.id, workspace_id: saved.workspace, team_id: cycle.team_id, cycle_id: cycle.id, expected_rows: expectedRows, reason_scalars: Array.from(reason).length, reason_sha256: digest(reason) });
}

test('cycle reopening preserves a scalar-boundary reason, rejects overlength without history changes and persists one repair', async ({ page, saved }) => {
  const owning = await team(saved); const base = `/teams/${owning.id}/cycles`;
  let current = await saved.request<Cycle>(base, 'POST', { name: 'Scalar reason cycle', start_date: '2026-03-28', end_date_exclusive: '2026-03-31', timezone: 'Europe/Berlin' }, { 'Idempotency-Key': randomUUID() }); const path = `${base}/${current.id}`;
  current = await saved.request<Cycle>(`${path}/open`, 'POST', undefined, { 'If-Match': current.etag });
  const preview = await saved.request<ClosePreview>(`${path}/close-preview`); const closed = await saved.request<CloseReport>(`${path}/close`, 'POST', { review_token: preview.review_token }, { 'If-Match': preview.cycle.etag, 'Idempotency-Key': randomUUID() }); current = closed.cycle;
  const report = () => saved.request<CloseReport>(`${path}/reports/${current.latest_report_id}`); const history = () => saved.request<{ data: SnapshotPoint[] }>(`${path}/snapshots`);
  const originalReport = await report(); const originalHistory = await history(); const reason = '😀'.repeat(1000);
  await page.goto(`/w/${saved.workspace}/cycles/${current.id}?team_id=${owning.id}`); await page.getByRole('button', { name: 'Reopen cycle', exact: true }).click(); const dialog = page.getByRole('dialog', { name: 'Reopen cycle', exact: true }); const input = dialog.getByRole('textbox', { name: 'Reason', exact: true });
  if (!await bulk(page, input, 'cycle_reason', reason)) { cycleFacts(saved, current, reason, 0); return; }
  await bulk(page, input, 'cycle_reason', '😀'.repeat(1001)); const rejection = await write(page, dialog, 'Reopen cycle', `${saved.path}${path}/reopen`, 422);
  expect(rejection.request().postDataJSON()).toEqual({ reason: '😀'.repeat(1001) }); await expect(dialog.getByRole('alert')).toContainText('must contain'); await expect(input).toHaveValue('😀'.repeat(1001)); expect(await saved.request<Cycle>(path)).toEqual(current); expect(await report()).toEqual(originalReport); expect(await history()).toEqual(originalHistory);
  await bulk(page, input, 'cycle_reason', reason); const accepted = await write(page, dialog, 'Reopen cycle', `${saved.path}${path}/reopen`, 200); expect(accepted.request().postDataJSON()).toEqual({ reason }); await expect(dialog).toHaveCount(0); const reopened = await accepted.json() as Cycle;
  expect(reopened).toMatchObject({ id: current.id, state: 'active', version: (BigInt(current.version) + 1n).toString(), timezone: current.timezone, start_date: current.start_date, end_date_exclusive: current.end_date_exclusive, starts_at: current.starts_at, ends_at: current.ends_at, latest_report_id: current.latest_report_id });
  expect(await report()).toEqual(originalReport); const updatedHistory = await history(); expect(updatedHistory.data).toHaveLength(originalHistory.data.length + 1); expect(updatedHistory.data.filter(item => item.kind === 'reopened')).toHaveLength(1);
  await page.reload(); await expect(page.locator('.cycle-state')).toHaveText('active'); expect(await saved.request<Cycle>(path)).toEqual(reopened); expect(await report()).toEqual(originalReport); expect(await history()).toEqual(updatedHistory); cycleFacts(saved, current, reason, 1);
});

test('project update preserves 8000 scalars and its displayed count, rejects 8001 and publishes one immutable snapshot', async ({ page, saved }) => {
  const owning = await team(saved); const base = `/teams/${owning.id}/projects`; const created = await saved.request<FullProject>(base, 'POST', { name: 'Scalar update project' }, { 'Idempotency-Key': randomUUID() }); const path = `${base}/${created.id}`;
  let project = await saved.request<Project>(`${path}/metadata`); project = await saved.request<Project>(`${path}/metadata`, 'PATCH', { state: 'active', health: 'at_risk' }, { 'If-Match': project.etag });
  const issue = await saved.request<Issue>('/issues', 'POST', { team_id: owning.id, title: 'Project update scope' }, { 'Idempotency-Key': randomUUID() }); await saved.request<FullProject>(`${path}/issues/${issue.id}`, 'PUT', { included: true }, { 'If-Match': project.etag }); project = await saved.request<Project>(`${path}/metadata`);
  const updates = () => saved.request<{ data: ProjectUpdate[] }>(`${path}/updates`); expect((await updates()).data).toHaveLength(0);
  await page.goto(`/w/${saved.workspace}/projects/${created.id}?team_id=${owning.id}`); await page.getByRole('navigation', { name: 'Project sections', exact: true }).getByRole('button', { name: 'Updates', exact: true }).click(); const scope = page.getByRole('region', { name: 'Project updates', exact: true }); const input = scope.getByRole('textbox', { name: 'Write an update', exact: true }); const body = '🧭'.repeat(8000);
  if (!await bulk(page, input, 'project_update', body)) return;
  await expect(scope.locator('.project-update-actions > span')).toHaveText('8,000 / 8,000');
  await bulk(page, input, 'project_update', '🧭'.repeat(8001)); const rejected = await write(page, scope, 'Publish update', `${saved.path}${path}/updates`, 422); expect(rejected.request().postDataJSON()).toEqual({ body: '🧭'.repeat(8001) }); await expect(scope.getByRole('alert')).toContainText('1–8000'); await expect(input).toHaveValue('🧭'.repeat(8001)); expect((await updates()).data).toHaveLength(0); expect(await saved.request<Project>(`${path}/metadata`)).toEqual(project);
  await bulk(page, input, 'project_update', body); await expect(scope.locator('.project-update-actions > span')).toHaveText('8,000 / 8,000'); const accepted = await write(page, scope, 'Publish update', `${saved.path}${path}/updates`, 201); expect(accepted.request().postDataJSON()).toEqual({ body }); const update = await accepted.json() as ProjectUpdate;
  expect(update).toMatchObject({ body, state: 'active', health: 'at_risk', completed: 0, eligible: 1 }); await expect(input).toHaveValue(''); await expect(scope.locator('.project-update-body')).toHaveText(body); expect((await updates()).data).toEqual([update]); expect(await saved.request<Project>(`${path}/metadata`)).toEqual(project);
  await page.reload(); await page.getByRole('navigation', { name: 'Project sections', exact: true }).getByRole('button', { name: 'Updates', exact: true }).click(); await expect(scope.locator('.project-update-body')).toHaveText(body); expect((await updates()).data).toEqual([update]); expect(await saved.request<ProjectUpdate>(`${path}/updates/${update.id}`)).toEqual(update); expect(await saved.request<Project>(`${path}/metadata`)).toEqual(project);
});
