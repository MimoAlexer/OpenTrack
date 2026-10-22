import { randomUUID } from 'node:crypto';
import { test, expect, type SavedWorkspace } from './fixtures';
import type { Bootstrap, Issue, Team } from '../src/types';
import type { Milestone, MilestoneOverview } from '../src/projects/Milestones';
import type { Project } from '../src/projects/types';
import type { CarryOperation, CloseReport, Cycle, Snapshot } from '../src/cycles/types';

const projectPath = (saved: SavedWorkspace, project?: string) => `/teams/${saved.team.id}/projects${project ? `/${project}` : ''}`;
const projectUrl = (saved: SavedWorkspace, project: string) => `/w/${saved.workspace}/projects/${project}?team_id=${saved.team.id}`;
const cyclesPath = (team: string, cycle?: string) => `/teams/${team}/cycles${cycle ? `/${cycle}` : ''}`;
const cycleUrl = (saved: SavedWorkspace, team: string, cycle: string) => `/w/${saved.workspace}/cycles/${cycle}?team_id=${team}`;
async function project(saved: SavedWorkspace) { return saved.request<Project>(projectPath(saved), 'POST', { name: `Planning acceptance ${randomUUID()}` }, { 'Idempotency-Key': randomUUID() }); }
async function planningTeam(saved: SavedWorkspace) {
  return saved.request<Team>('/teams', 'POST', { name: `Planning team ${randomUUID()}`, key: `P${randomUUID().replaceAll('-', '').slice(0, 8).toUpperCase()}`, private: false }, { 'Idempotency-Key': randomUUID() });
}
async function cycle(saved: SavedWorkspace, team: string, name: string, start: string, end: string) {
  return saved.request<Cycle>(cyclesPath(team), 'POST', { name, start_date: start, end_date_exclusive: end, timezone: 'Europe/Berlin' }, { 'Idempotency-Key': randomUUID() });
}
async function issue(saved: SavedWorkspace, team: string, title: string) {
  return saved.request<Issue>('/issues', 'POST', { team_id: team, title }, { 'Idempotency-Key': randomUUID() });
}

test('milestones persist outcomes, keyboard order and scoped assignment, then safely detach on deletion', async ({ page, saved }) => {
  let current = await project(saved); const path = projectPath(saved, current.id);
  const work = await saved.create(`Milestone work ${randomUUID()}`);
  current = await saved.request<Project>(`${path}/issues/${work.id}`, 'PUT', { included: true }, { 'If-Match': current.etag });
  await page.goto(projectUrl(saved, current.id));
  const milestones = page.getByRole('region', { name: 'Project milestones', exact: true });
  await expect(milestones).toContainText('No milestones yet');
  for (const name of ['Reviewed outcome', 'Second outcome']) {
    await milestones.getByRole('button', { name: 'New milestone', exact: true }).click();
    const dialog = page.getByRole('dialog', { name: 'New milestone', exact: true });
    await expect(dialog.getByRole('textbox', { name: 'Title', exact: true })).toBeFocused();
    await dialog.getByRole('textbox', { name: 'Title', exact: true }).fill(name);
    await dialog.getByLabel('Target date', { exact: true }).fill('2026-11-20');
    await dialog.getByRole('button', { name: 'Create milestone', exact: true }).click();
    await expect(dialog).toHaveCount(0); await expect(milestones.getByRole('heading', { name, exact: true })).toBeVisible();
  }
  let all = await saved.request<{ data: MilestoneOverview[] }>(`${path}/milestones`);
  const first = all.data.find(item => item.milestone.title === 'Reviewed outcome')!.milestone;
  const second = all.data.find(item => item.milestone.title === 'Second outcome')!.milestone;
  const move = milestones.getByRole('button', { name: 'Move Second outcome up', exact: true });
  await move.focus(); await move.press('Enter');
  await expect.poll(async () => (await saved.request<{ data: MilestoneOverview[] }>(`${path}/milestones`)).data.map(item => item.milestone.id)).toEqual([second.id, first.id]);
  await expect(milestones.getByRole('article').first()).toContainText('Second outcome'); await expect(milestones.getByRole('button', { name: 'Edit Second outcome', exact: true })).toBeFocused();
  await page.getByRole('navigation', { name: 'Project sections' }).getByRole('button', { name: 'Issues', exact: true }).click();
  await page.getByRole('combobox', { name: `Milestone for ${work.key}`, exact: true }).selectOption(first.id);
  await expect.poll(async () => (await saved.request<{ data: { id: string; milestone_id: string | null }[] }>(`${path}/issues`)).data.find(item => item.id === work.id)?.milestone_id).toBe(first.id);
  await page.getByRole('checkbox', { name: 'Group by milestone', exact: true }).check();
  await expect(page.getByRole('region', { name: 'Project issues', exact: true }).getByRole('heading', { name: 'Reviewed outcome', exact: true })).toBeVisible();
  await page.getByRole('navigation', { name: 'Project sections' }).getByRole('button', { name: 'Overview', exact: true }).click();
  await milestones.getByRole('button', { name: 'Edit Second outcome', exact: true }).click();
  const edit = page.getByRole('dialog', { name: 'Edit milestone', exact: true });
  await edit.getByRole('combobox', { name: 'Milestone state', exact: true }).selectOption('completed');
  await edit.getByRole('button', { name: 'Save milestone', exact: true }).click(); await expect(edit).toHaveCount(0);
  const emptyCompleted = milestones.getByRole('article').filter({ hasText: 'Second outcome' });
  await expect(emptyCompleted).toContainText('Manually completed'); await expect(emptyCompleted).toContainText('No eligible issue scope'); await expect(emptyCompleted.getByRole('progressbar')).toHaveCount(0);
  await milestones.getByRole('button', { name: 'Delete Reviewed outcome', exact: true }).click();
  const remove = page.getByRole('dialog', { name: 'Delete milestone?', exact: true });
  await expect(remove).toContainText('Issues remain in the project'); await remove.getByRole('button', { name: 'Delete milestone', exact: true }).click(); await expect(remove).toHaveCount(0);
  await expect.poll(async () => (await saved.request<{ data: { id: string; milestone_id: string | null }[] }>(`${path}/issues`)).data.find(item => item.id === work.id)?.milestone_id).toBeNull();
  expect((await saved.issue(work.id)).title).toBe(work.title);
  await page.reload(); await expect(milestones).toContainText('Second outcome'); await expect(milestones).not.toContainText('Reviewed outcome');
});

test('milestone conflicts retain the typed draft and require a review of the saved version', async ({ page, saved }) => {
  const current = await project(saved); const path = projectPath(saved, current.id);
  const milestone = await saved.request<Milestone>(`${path}/milestones`, 'POST', { title: 'Before concurrent edit' }, { 'If-Match': current.etag, 'Idempotency-Key': randomUUID() });
  await page.goto(projectUrl(saved, current.id)); await page.getByRole('button', { name: 'Edit Before concurrent edit', exact: true }).click();
  const dialog = page.getByRole('dialog', { name: 'Edit milestone', exact: true });
  await dialog.getByRole('textbox', { name: 'Title', exact: true }).fill('Retained reviewed draft');
  await saved.request<Milestone>(`${path}/milestones/${milestone.id}`, 'PATCH', { title: 'Saved by another editor' }, { 'If-Match': milestone.etag });
  await dialog.getByRole('button', { name: 'Save milestone', exact: true }).click();
  await expect(dialog).toContainText('Your draft is retained'); await expect(dialog.getByRole('textbox', { name: 'Title', exact: true })).toHaveValue('Retained reviewed draft'); await expect(dialog.getByRole('button', { name: 'Save milestone', exact: true })).toBeDisabled();
  await dialog.getByRole('button', { name: 'Load latest for review', exact: true }).click(); await expect(dialog).toContainText('Saved by another editor');
  await dialog.getByRole('button', { name: 'Keep my draft for review', exact: true }).click(); await dialog.getByRole('button', { name: 'Save milestone', exact: true }).click(); await expect(dialog).toHaveCount(0);
  expect((await saved.request<Milestone>(`${path}/milestones/${milestone.id}`)).title).toBe('Retained reviewed draft');
});

test('milestone loading and connection failure remain recoverable without fake empty counts', async ({ page, saved }) => {
  const current = await project(saved); let release!: () => void; const gate = new Promise<void>(resolve => { release = resolve; }); let first = true;
  await page.route('**/milestones?*', async route => { if (first) { first = false; await gate; await route.abort('failed'); } else await route.continue(); });
  try {
    await page.goto(projectUrl(saved, current.id)); const milestones = page.getByRole('region', { name: 'Project milestones', exact: true });
    await expect(milestones.getByRole('status')).toHaveText('Loading milestones…'); await expect(milestones).not.toContainText('No milestones yet'); release();
    await expect(milestones.getByRole('alert')).toContainText('Could not reach OpenTrack'); await expect(milestones).not.toContainText('% complete'); await milestones.getByRole('button', { name: 'Try again', exact: true }).click();
    await expect(milestones).toContainText('No milestones yet'); await expect(milestones.getByRole('alert')).toHaveCount(0);
  } finally { release(); }
});

test('cycle creation keeps DST calendar boundaries, rejects overlap and preserves a conflicting draft', async ({ page, saved }) => {
  const team = await planningTeam(saved); await page.goto(`/w/${saved.workspace}/cycles?team_id=${team.id}`);
  await expect(page.getByText('No cycles yet', { exact: true })).toBeVisible(); await page.getByRole('button', { name: 'New cycle', exact: true }).click();
  const dialog = page.getByRole('dialog', { name: 'New cycle', exact: true });
  await dialog.getByRole('textbox', { name: 'Cycle name', exact: true }).fill('DST reviewed cycle'); await dialog.getByLabel('Start date', { exact: true }).fill('2026-03-28'); await dialog.getByLabel('End date (exclusive)', { exact: true }).fill('2026-03-31'); await dialog.getByRole('textbox', { name: 'Timezone', exact: true }).fill('Europe/Berlin');
  const created = page.waitForResponse(response => response.request().method() === 'POST' && response.url().endsWith(cyclesPath(team.id)));
  await dialog.getByRole('button', { name: 'Create cycle', exact: true }).click(); const current = await (await created).json() as Cycle;
  await expect(dialog).toHaveCount(0); await expect(page.getByRole('heading', { name: current.name, exact: true })).toBeVisible();
  expect(current).toMatchObject({ start_date: '2026-03-28', end_date_exclusive: '2026-03-31', starts_at: '2026-03-27T23:00:00Z', ends_at: '2026-03-30T22:00:00Z', timezone: 'Europe/Berlin' });
  await page.getByText('Calendar boundaries', { exact: true }).click(); await expect(page.locator('.cycle-boundaries')).toContainText(current.ends_at);
  await page.getByRole('button', { name: 'Edit cycle', exact: true }).click(); const edit = page.getByRole('dialog', { name: 'Edit cycle', exact: true }); await edit.getByRole('textbox', { name: 'Cycle name', exact: true }).fill('Retained cycle draft');
  await saved.request<Cycle>(cyclesPath(team.id, current.id), 'PATCH', { name: 'Saved cycle change' }, { 'If-Match': current.etag }); await edit.getByRole('button', { name: 'Save cycle', exact: true }).click(); await expect(edit).toContainText('Saved cycle change'); await expect(edit.getByRole('textbox', { name: 'Cycle name', exact: true })).toHaveValue('Retained cycle draft'); await expect(edit.getByRole('button', { name: 'Save cycle', exact: true })).toBeDisabled(); await edit.getByRole('button', { name: 'Keep my draft for review', exact: true }).click(); await edit.getByRole('button', { name: 'Save cycle', exact: true }).click(); await expect(edit).toHaveCount(0);
  await page.getByRole('link', { name: 'Cycles', exact: true }).last().click(); await page.getByRole('button', { name: 'New cycle', exact: true }).click();
  await dialog.getByRole('textbox', { name: 'Cycle name', exact: true }).fill('Overlapping interval'); await dialog.getByLabel('Start date', { exact: true }).fill('2026-03-29'); await dialog.getByLabel('End date (exclusive)', { exact: true }).fill('2026-04-02'); await dialog.getByRole('button', { name: 'Create cycle', exact: true }).click(); await expect(dialog).toContainText('overlap'); await expect(dialog.getByRole('textbox', { name: 'Cycle name', exact: true })).toHaveValue('Overlapping interval');
});

test('cycle closure preserves issue state, accessible history and explicit conflict-aware carry-forward', async ({ page, saved, playwright }) => {
  const team = await planningTeam(saved); let current = await cycle(saved, team.id, 'Reviewed source cycle', '2026-03-28', '2026-03-31'); const destination = await cycle(saved, team.id, 'Reviewed destination cycle', '2026-03-31', '2026-04-07'); const path = cyclesPath(team.id, current.id);
  const [unfinished, completed] = await Promise.all([issue(saved, team.id, 'Unfinished accepted work'), issue(saved, team.id, 'Completed accepted work')]);
  const bootstrap = await saved.request<Bootstrap>('/bootstrap'); const done = bootstrap.statuses.find(status => status.team_id === team.id && status.category === 'completed')!;
  await saved.patch(completed, { status_id: done.id });
  for (const item of [await saved.issue(completed.id)]) current = await saved.request<Cycle>(`${path}/issues/${item.id}`, 'PUT', { included: true, issue_etag: item.etag }, { 'If-Match': current.etag });
  current = await saved.request<Cycle>(`${path}/open`, 'POST', undefined, { 'If-Match': current.etag });
  await page.goto(cycleUrl(saved, team.id, current.id));
  await expect(page.getByText('100% complete', { exact: true })).toBeVisible();
  for (let attempt = 0; attempt < 2; attempt += 1) {
    await page.getByRole('button', { name: 'Add issues', exact: true }).click(); const add = page.getByRole('dialog', { name: 'Add an issue to this cycle', exact: true }); await add.getByRole('textbox', { name: 'Find an issue', exact: true }).fill(unfinished.title); await add.getByRole('button').filter({ hasText: unfinished.title }).click(); await expect(add).toHaveCount(0);
    await expect(page.getByText('50% complete', { exact: true })).toBeVisible(); const row = page.getByRole('region', { name: 'Cycle scope', exact: true }).getByRole('row').filter({ hasText: unfinished.title }); await expect(row).toBeVisible();
    if (!attempt) { await row.getByRole('button', { name: 'Remove from cycle', exact: true }).click(); await expect(page.getByText('100% complete', { exact: true })).toBeVisible(); await expect(row).toHaveCount(0); expect((await saved.issue(unfinished.id)).status_id).toBe(unfinished.status_id); }
  }
  await page.getByRole('button', { name: 'Capture snapshot', exact: true }).focus(); await page.keyboard.press('Enter');
  const history = page.getByRole('region', { name: 'Cycle history', exact: true }); await expect(history.getByRole('table')).toBeVisible(); await expect(history.getByRole('img')).toBeVisible(); expect((await history.getByRole('img').boundingBox())!.height).toBeGreaterThan(100); await expect(history).toContainText('Gaps are not interpolated');
  await page.setViewportSize({ width: 390, height: 844 }); const scrollArea = history.getByRole('region', { name: 'Scrollable scope history table', exact: true }); await scrollArea.focus(); await scrollArea.press('ArrowRight'); await expect.poll(async () => scrollArea.evaluate(element => element.scrollLeft)).toBeGreaterThan(0); expect(await page.evaluate(() => document.documentElement.scrollWidth - innerWidth)).toBeLessThanOrEqual(1); await page.setViewportSize({ width: 1440, height: 900 });
  const before = await Promise.all([unfinished, completed].map(item => saved.issue(item.id)));
  await page.getByRole('button', { name: 'Review closure', exact: true }).click(); const review = page.getByRole('dialog', { name: 'Review cycle closure', exact: true }); await expect(review.getByRole('button', { name: 'Close cycle', exact: true })).toBeDisabled(); await review.getByRole('checkbox').check(); await review.getByRole('button', { name: 'Close cycle', exact: true }).click(); await expect(review).toHaveCount(0);
  expect(await Promise.all([unfinished, completed].map(item => saved.issue(item.id)))).toEqual(before);
  current = await saved.request<Cycle>(path); expect(current.state).toBe('closed'); const original = await saved.request<CloseReport>(`${path}/reports/${current.latest_report_id}`); expect(original.scope).toMatchObject({ completed_count: 1, unfinished_count: 1 });
  await saved.patch(await saved.issue(unfinished.id), { title: 'Reviewed newer unfinished work' });
  await page.getByRole('button', { name: 'Carry unfinished work', exact: true }).click(); const carry = page.getByRole('dialog', { name: 'Carry unfinished work forward', exact: true }); await expect(carry.getByRole('button', { name: 'Move 0 selected issues', exact: true })).toBeDisabled(); await carry.getByRole('combobox', { name: 'Destination cycle', exact: true }).selectOption(destination.id); await carry.getByRole('checkbox').check(); let release!: () => void; let committed!: () => void; const gate = new Promise<void>(resolve => { release = resolve; }); const committedRequest = new Promise<void>(resolve => { committed = resolve; });
  await page.route('**/carry-forward', async route => { const response = await route.fetch(); committed(); await gate; await route.fulfill({ response }); });
  try { await carry.getByRole('button', { name: 'Move 1 selected issues', exact: true }).click(); await committedRequest; page.once('dialog', dialog => dialog.dismiss()); await carry.getByRole('link').filter({ hasText: 'Reviewed newer unfinished work' }).click(); await expect(carry).toBeVisible(); expect(new URL(page.url()).searchParams.has('carry')).toBe(false); } finally { release(); }
  await expect(carry).toContainText('version conflict'); await expect(carry).toContainText('Some issues need review'); await expect(page).toHaveURL(/carry=/); await page.reload(); await expect(carry).toContainText('version conflict');
  const permalink = page.url(); const invitation = await saved.request<{ token: string }>('/invitations', 'POST', { email: `planning-viewer-${randomUUID()}@example.test`, role: 'viewer', team_ids: [] }, { 'Idempotency-Key': randomUUID() });
  const reader = await playwright.request.newContext({ baseURL: saved.origin, extraHTTPHeaders: { Origin: saved.origin }, storageState: { cookies: [], origins: [] } });
  try {
    const accepted = await reader.post('/api/v1/invitations/accept', { data: { token: invitation.token, display_name: 'Example Viewer', password: process.env.OPENTRACK_BROWSER_PASSWORD } }); expect(accepted.status()).toBe(200);
    const context = await page.context().browser()!.newContext({ baseURL: saved.origin, storageState: await reader.storageState() });
    try { await context.route('**/*', async route => { if (new URL(route.request().url()).origin === saved.origin) await route.continue(); else await route.abort(); }); const viewer = await context.newPage(); const posts: string[] = []; viewer.on('request', request => { if (request.method() === 'POST') posts.push('write'); }); await viewer.goto(permalink); const receipt = viewer.getByRole('dialog', { name: 'Carry unfinished work forward', exact: true }); await expect(receipt).toContainText('Read-only receipt'); await expect(receipt).toContainText('version conflict'); await expect(receipt.getByRole('checkbox')).toHaveCount(0); await expect(receipt.getByRole('button', { name: 'Review unresolved issues', exact: true })).toHaveCount(0); expect(posts).toEqual([]); } finally { await context.close(); }
  } finally { await reader.dispose(); }
  await page.reload(); await expect(carry).toContainText('version conflict');
  await carry.getByRole('button', { name: 'Review unresolved issues', exact: true }).click(); await expect(carry.getByRole('region', { name: 'Review carry-forward retry', exact: true })).toContainText('Reviewed newer unfinished work'); await carry.getByRole('checkbox').check();
  let target = await saved.request<Cycle>(cyclesPath(team.id, destination.id)); target = await saved.request<Cycle>(cyclesPath(team.id, destination.id), 'PATCH', { name: 'Reviewed destination changed' }, { 'If-Match': target.etag });
  await carry.getByRole('button', { name: 'Retry selected unresolved issues', exact: true }).click(); await expect(carry.getByRole('region', { name: 'Review carry-forward retry', exact: true })).toHaveCount(0); await carry.getByRole('button', { name: 'Review unresolved issues', exact: true }).click(); await expect(carry.getByRole('button', { name: 'Retry selected unresolved issues', exact: true })).toBeDisabled(); await carry.getByRole('checkbox').check();
  const operationPath = `${path}/carry-forward/${new URL(page.url()).searchParams.get('carry')}`; const operation = await saved.request<CarryOperation>(operationPath); await saved.request<CarryOperation>(`${operationPath}/resume`, 'POST', { destination_etag: target.etag, items: [{ issue_id: unfinished.id, issue_version: original.scope.items.find(item => item.issue_id === unfinished.id)!.issue_version }] }, { 'If-Match': operation.etag, 'Idempotency-Key': randomUUID() });
  await carry.getByRole('button', { name: 'Retry selected unresolved issues', exact: true }).click(); await expect(carry.getByRole('region', { name: 'Review carry-forward retry', exact: true })).toHaveCount(0); await carry.getByRole('button', { name: 'Review unresolved issues', exact: true }).click(); await expect(carry.getByRole('button', { name: 'Retry selected unresolved issues', exact: true })).toBeDisabled(); await carry.getByRole('checkbox').check(); await carry.getByRole('button', { name: 'Retry selected unresolved issues', exact: true }).click(); await expect(carry).toContainText('Carry-forward complete'); await carry.getByRole('button', { name: 'Done', exact: true }).click();
  const [sourceScope, targetScope] = await Promise.all([saved.request<Snapshot>(`${path}/scope`), saved.request<Snapshot>(`${cyclesPath(team.id, destination.id)}/scope`)]);
  expect(sourceScope.items.map(item => item.issue_id)).toEqual([completed.id]); expect(targetScope.items.map(item => item.issue_id)).toEqual([unfinished.id]); expect((await saved.issue(unfinished.id)).status_id).toBe(before[0].status_id); expect((await saved.request<CloseReport>(`${path}/reports/${current.latest_report_id}`)).scope).toEqual(original.scope);
  await page.reload(); await expect(page.getByRole('region', { name: 'Cycle history', exact: true }).getByRole('table')).toBeVisible();
  await page.getByRole('button', { name: 'View closure report', exact: true }).click(); const originalReport = page.getByRole('dialog', { name: 'Cycle closure report', exact: true }); await expect(originalReport).toContainText('1 / 2 eligible issues'); await originalReport.getByRole('button', { name: 'Done', exact: true }).click();
  await page.getByRole('button', { name: 'Reopen cycle', exact: true }).click(); const reopen = page.getByRole('dialog', { name: 'Reopen cycle', exact: true }); await expect(reopen.getByRole('button', { name: 'Reopen cycle', exact: true })).toBeDisabled(); await reopen.getByRole('textbox', { name: 'Reason', exact: true }).fill('Review a corrected scope after explicit carry-forward'); await reopen.getByRole('button', { name: 'Reopen cycle', exact: true }).click(); await expect(reopen).toHaveCount(0); expect((await saved.request<Cycle>(path)).state).toBe('active'); expect((await saved.request<CloseReport>(`${path}/reports/${current.latest_report_id}`)).scope).toEqual(original.scope);
});
