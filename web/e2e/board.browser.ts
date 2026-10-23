import { randomUUID } from 'node:crypto';
import type { Locator, Page } from '@playwright/test';
import { test, expect, SavedWorkspace } from './fixtures';
import type { Bootstrap, Issue, IssuePage, Session, Status } from '../src/types';
import type { InvitationIssued, TeamDetail, TeamMember } from '../src/settings/types';
import type { Workflow } from '../src/workflows/types';

async function fixture(owner: SavedWorkspace, count = 3, privateTeam = false) {
  const team = await owner.request<TeamDetail>('/teams', 'POST', {
    name: `Board acceptance ${randomUUID()}`, key: `B${randomUUID().replaceAll('-', '').slice(0, 8).toUpperCase()}`,
    private: privateTeam,
  }, { 'Idempotency-Key': randomUUID() });
  const bootstrap = await owner.request<Bootstrap>('/bootstrap');
  const saved = new SavedWorkspace(owner.http, owner.session, { ...bootstrap, teams: [team] }, owner.origin);
  const issues: Issue[] = [];
  for (let index = 0; index < count; index++) issues.push(await saved.request<Issue>('/issues', 'POST', {
    team_id: team.id, title: `Board work ${index} ${randomUUID()}`, priority: 2, estimate: 3, due_date: '2030-10-24',
  }, { 'Idempotency-Key': randomUUID() }));
  const statuses = bootstrap.statuses.filter(status => status.team_id === team.id && !status.archived_at && !status.retiring_at);
  const target = statuses.find(status => status.category === 'unstarted' && status.id !== issues[0].status_id)!;
  return { saved, team, issues, statuses, target };
}

function url(saved: SavedWorkspace, query: Record<string, string> = {}) {
  return `/w/${saved.workspace}/issues?${new URLSearchParams({ team_id: saved.team.id, layout: 'board', display_version: '1', ...query })}`;
}

async function open(page: Page, saved: SavedWorkspace, query: Record<string, string> = {}) {
  await page.goto(url(saved, query));
  const board = page.getByRole('region', { name: 'Issue board', exact: true });
  await expect(board).toBeVisible();
  await expect(page.getByRole('combobox', { name: 'Issue layout', exact: true })).toHaveValue('board');
  return board;
}

function patchResponse(page: Page, saved: SavedWorkspace, issue: Issue) {
  return page.waitForResponse(response => response.request().method() === 'PATCH'
    && response.url() === `${saved.origin}${saved.path}/issues/${issue.id}`);
}

async function chooseMove(page: Page, issue: Issue, statusId: string) {
  await page.getByRole('button', { name: `Move ${issue.key}`, exact: true }).click();
  await moveDialog(page, issue).getByRole('combobox', { name: `Move ${issue.key} to status`, exact: true }).selectOption(statusId);
}

const moveDialog = (page: Page, issue: Issue) => page.getByRole('dialog', { name: `Move ${issue.key}`, exact: true });

async function drag(page: Page, handle: Locator, column: Locator, cancel = false) {
  await handle.scrollIntoViewIfNeeded();
  const source = await handle.boundingBox();
  expect(source).not.toBeNull();
  await page.mouse.move(source!.x + source!.width / 2, source!.y + source!.height / 2);
  await page.mouse.down();
  await page.mouse.move(source!.x + source!.width / 2 + 10, source!.y + source!.height / 2, { steps: 3 });
  await column.scrollIntoViewIfNeeded();
  const destination = await column.boundingBox();
  expect(destination).not.toBeNull();
  await page.mouse.move(destination!.x + destination!.width / 2, destination!.y + Math.min(80, destination!.height / 2), { steps: 8 });
  if (cancel) await page.keyboard.press('Escape');
  await page.mouse.up();
}

function onlyStatusChanged(before: Issue, after: Issue, statusId: string) {
  expect(after).toEqual({ ...before, status_id: statusId, version: (BigInt(before.version) + 1n).toString(),
    etag: after.etag, updated_at: after.updated_at });
}

test('pointer and keyboard board moves persist only status with one version increment', async ({ page, saved: owner }) => {
  const { saved, issues, target } = await fixture(owner);
  const preference = await owner.request<{ etag: string }>('/preferences');
  await owner.request('/preferences', 'PATCH', { shortcut_mode: 'disabled' }, { 'If-Match': preference.etag });
  await saved.open(page);
  await page.getByRole('combobox', { name: 'Issue layout', exact: true }).selectOption('board');
  await expect(page.getByRole('heading', { name: 'Choose a team for this board.', exact: true })).toBeVisible();
  await page.getByRole('combobox', { name: 'Board team', exact: true }).selectOption(saved.team.id);
  const board = page.getByRole('region', { name: 'Issue board', exact: true });
  await expect(board).toBeVisible();
  const column = board.locator(`[data-board-status="${target.id}"]`);
  const response = patchResponse(page, saved, issues[0]);
  await drag(page, board.getByRole('button', { name: `Drag ${issues[0].key}`, exact: true }), column);
  const moved = await response; expect(moved.status()).toBe(200);
  expect(moved.request().postDataJSON()).toEqual({ status_id: target.id });
  expect(moved.request().headers()['if-match']).toBe(issues[0].etag);
  onlyStatusChanged(issues[0], await saved.issue(issues[0].id), target.id);
  await expect(column.locator(`#row-${issues[0].id}`)).toBeVisible();
  const opener = page.getByRole('button', { name: `Move ${issues[1].key}`, exact: true });
  await expect(opener).toBeEnabled(); await opener.focus();
  await page.keyboard.press('Enter');
  const dialog = moveDialog(page, issues[1]);
  const status = dialog.getByRole('combobox', { name: `Move ${issues[1].key} to status`, exact: true });
  await expect(status).toBeFocused();
  const options = await status.locator('option').evaluateAll(elements => elements.map(element => (element as HTMLOptionElement).value));
  await page.keyboard.press('Home');
  for (let index = 0; index < options.indexOf(target.id); index++) await page.keyboard.press('ArrowDown');
  await expect(status).toHaveValue(target.id);
  await page.keyboard.press('Tab');
  await expect(dialog.getByRole('button', { name: 'Cancel move', exact: true })).toBeFocused();
  await page.keyboard.press('Tab');
  await expect(dialog.getByRole('button', { name: `Move ${issues[1].key}`, exact: true })).toBeFocused();
  const second = patchResponse(page, saved, issues[1]);
  await page.keyboard.press('Enter'); expect((await second).status()).toBe(200);
  onlyStatusChanged(issues[1], await saved.issue(issues[1].id), target.id);
  expect(await saved.issue(issues[2].id)).toEqual(issues[2]);
  await page.reload();
  for (const issue of issues.slice(0, 2)) await expect(page.locator(`[data-board-status="${target.id}"] #row-${issue.id}`)).toBeVisible();
});

test('a real stale board move preserves its intent and requires review before applying the newest ETag', async ({ page, saved: owner }) => {
  const { saved, issues, target } = await fixture(owner);
  const board = await open(page, saved);
  const endpoint = `${saved.origin}${saved.path}/issues/${issues[0].id}`;
  let arrived!: () => void; let release!: () => void; let held = false;
  const received = new Promise<void>(resolve => { arrived = resolve; });
  const gate = new Promise<void>(resolve => { release = resolve; });
  await page.route(endpoint, async route => {
    if (route.request().method() === 'PATCH' && !held) { held = true; arrived(); await gate; }
    await route.continue();
  });
  try {
    await chooseMove(page, issues[0], target.id);
    const stale = patchResponse(page, saved, issues[0]);
    await moveDialog(page, issues[0]).getByRole('button', { name: `Move ${issues[0].key}`, exact: true }).click();
    await received;
    const current = await saved.patch(issues[0], { title: `Saved competing title ${randomUUID()}`, priority: 1 });
    release(); expect((await stale).status()).toBe(412);
    expect(await saved.issue(issues[0].id)).toEqual(current);
    await expect(board.locator(`[data-board-status="${current.status_id}"] #row-${current.id}`)).toBeVisible();
    const dialog = moveDialog(page, issues[0]);
    await expect(dialog.getByRole('combobox', { name: `Move ${issues[0].key} to status`, exact: true })).toHaveValue(target.id);
    await expect(dialog.getByRole('button', { name: 'Review move', exact: true })).toBeVisible();
    const patches: string[] = [];
    page.on('request', request => { if (request.method() === 'PATCH' && request.url() === endpoint) patches.push(request.method()); });
    await dialog.getByRole('button', { name: 'Review move', exact: true }).click();
    expect((await saved.issue(current.id)).version).toBe(current.version);
    expect(await saved.issue(current.id)).toEqual(current);
    expect(patches).toEqual([]);
    await expect(dialog.getByRole('button', { name: 'Apply move', exact: true })).toBeEnabled();
    const applying = patchResponse(page, saved, current);
    await dialog.getByRole('button', { name: 'Apply move', exact: true }).click();
    const applied = await applying; expect(applied.status()).toBe(200);
    expect(applied.request().headers()['if-match']).toBe(current.etag);
    expect(applied.request().postDataJSON()).toEqual({ status_id: target.id });
    onlyStatusChanged(current, await saved.issue(current.id), target.id);
    for (const issue of issues.slice(1)) expect(await saved.issue(issue.id)).toEqual(issue);
    await page.reload();
    await expect(page.locator(`[data-board-status="${target.id}"] #row-${current.id}`)).toContainText(current.title);
  } finally { release(); await page.unroute(endpoint); }
});

test('canceled, short and same-status board drags do not send a write', async ({ page, saved: owner }) => {
  const { saved, issues, target } = await fixture(owner, 1);
  const board = await open(page, saved);
  const writes: string[] = [];
  page.on('request', request => { if (request.method() === 'PATCH' && request.url() === `${saved.origin}${saved.path}/issues/${issues[0].id}`) writes.push(request.method()); });
  const handle = board.getByRole('button', { name: `Drag ${issues[0].key}`, exact: true });
  const origin = board.locator(`[data-board-status="${issues[0].status_id}"]`);
  await handle.click();
  await drag(page, handle, board.locator(`[data-board-status="${target.id}"]`), true);
  await drag(page, handle, origin);
  await drag(page, handle, page.getByRole('combobox', { name: 'Issue layout', exact: true }));
  expect(writes).toEqual([]);
  expect(await saved.issue(issues[0].id)).toEqual(issues[0]);
});

test('list and board preserve the exact filtered query through display changes, reload and Back', async ({ page, saved: owner }) => {
  const { saved, issues } = await fixture(owner);
  await saved.patch(issues[2], { priority: 1 });
  const query = { priority: '2', sort: 'priority', q: 'Board work' };
  const expected = await saved.request<IssuePage>(`/issues?${new URLSearchParams({ team_id: saved.team.id, ...query, limit: '50' })}`);
  expect(expected.data.map(issue => issue.id).sort()).toEqual(issues.slice(0, 2).map(issue => issue.id).sort());
  await open(page, saved, query);
  const ids = async () => page.locator('.issue-row').evaluateAll(rows => rows.map(row => row.id.slice(4)).sort());
  await expect.poll(ids).toEqual(expected.data.map(issue => issue.id).sort());
  await page.getByRole('combobox', { name: 'Issue layout', exact: true }).selectOption('list');
  await expect(page.getByRole('region', { name: 'Issue board', exact: true })).toHaveCount(0);
  await expect.poll(ids).toEqual(expected.data.map(issue => issue.id).sort());
  await page.getByRole('combobox', { name: 'Issue layout', exact: true }).selectOption('board');
  for (const [key, value] of Object.entries({ team_id: saved.team.id, ...query, layout: 'board', display_version: '1' })) expect(new URL(page.url()).searchParams.get(key)).toBe(value);
  await page.reload();
  await expect.poll(ids).toEqual(expected.data.map(issue => issue.id).sort());
  await page.locator(`#row-${issues[0].id}`).click();
  await expect(page.getByRole('textbox', { name: 'Issue title', exact: true })).toHaveValue(issues[0].title);
  await page.goBack();
  await expect(page.getByRole('region', { name: 'Issue board', exact: true })).toBeVisible();
  await expect.poll(ids).toEqual(expected.data.map(issue => issue.id).sort());
  expect(new URL(page.url()).searchParams.get('sort')).toBe('priority');
});

test('board pages replace at most fifty cards and every current status remains discoverable', async ({ page, saved: owner }) => {
  const { saved } = await fixture(owner, 51);
  const workflow = await saved.request<Workflow>(`/teams/${saved.team.id}/workflow`);
  const expanded = await saved.request<Workflow>(`/teams/${saved.team.id}/statuses`, 'POST', {
    name: 'Additional board status', category: 'canceled', color: 'gray',
  }, { 'If-Match': `"workflow:${saved.team.id}:${workflow.version}:true"`, 'Idempotency-Key': randomUUID() });
  const statuses = expanded.statuses;
  expect(statuses.length).toBeGreaterThan(5);
  const first = await saved.request<IssuePage>(`/issues?team_id=${saved.team.id}&limit=50`);
  expect(first.data).toHaveLength(50); expect(first.has_more).toBe(true);
  const second = await saved.request<IssuePage>(`/issues?${new URLSearchParams({ team_id: saved.team.id, limit: '50', cursor: first.next_cursor! })}`);
  const board = await open(page, saved);
  const ids = async () => board.locator('.issue-row').evaluateAll(rows => rows.map(row => row.id.slice(4)).sort());
  await expect.poll(ids).toEqual(first.data.map(issue => issue.id).sort());
  await expect(board.locator('[data-board-status]')).toHaveCount(Math.min(5, statuses.length));
  await expect(page.locator('.list-caption').getByText('50 issues on this page', { exact: true })).toBeVisible();
  await page.getByRole('button', { name: 'Next board page', exact: true }).click();
  await expect.poll(ids).toEqual(second.data.map(issue => issue.id).sort());
  await expect(page.getByRole('button', { name: 'Next board page', exact: true })).toBeFocused();
  await expect(board.locator('.issue-row')).toHaveCount(1);
  await page.getByRole('button', { name: 'Previous board page', exact: true }).click();
  await expect.poll(ids).toEqual(first.data.map(issue => issue.id).sort());
  await expect(page.getByRole('button', { name: 'Previous board page', exact: true })).toBeFocused();
  const columns = page.getByRole('combobox', { name: 'Board columns', exact: true });
  expect(await columns.locator('option').evaluateAll(options => options.map(option => (option as HTMLOptionElement).value))).toEqual(expect.arrayContaining(statuses.map(status => status.id)));
  for (const status of statuses) {
    await columns.selectOption(status.id);
    await expect(board.locator(`[data-board-status="${status.id}"]`)).toBeVisible();
    expect(await board.locator('[data-board-status]').count()).toBeLessThanOrEqual(5);
    expect(await board.locator('.issue-row').count()).toBeLessThanOrEqual(50);
  }
});

test('a lost board response is reconciled by reading the committed move without sending another write', async ({ page, saved: owner }) => {
  const { saved, issues, target } = await fixture(owner, 1);
  await open(page, saved);
  const endpoint = `${saved.origin}${saved.path}/issues/${issues[0].id}`;
  let writes = 0;
  await page.route(endpoint, async route => {
    if (route.request().method() !== 'PATCH') { await route.continue(); return; }
    writes++;
    const committed = await route.fetch(); expect(committed.status()).toBe(200);
    await route.abort('failed');
  });
  try {
    await chooseMove(page, issues[0], target.id);
    const dialog = moveDialog(page, issues[0]);
    await dialog.getByRole('button', { name: `Move ${issues[0].key}`, exact: true }).click();
    await expect(dialog.getByRole('button', { name: 'Check move result', exact: true })).toBeVisible();
    onlyStatusChanged(issues[0], await saved.issue(issues[0].id), target.id);
    await dialog.getByRole('button', { name: 'Check move result', exact: true }).click();
    await expect(page.locator(`[data-board-status="${target.id}"] #row-${issues[0].id}`)).toBeVisible();
    await expect(dialog).toHaveCount(0);
    expect(writes).toBe(1);
    onlyStatusChanged(issues[0], await saved.issue(issues[0].id), target.id);
  } finally { await page.unroute(endpoint); }
});

test('private-team membership revocation conceals board cards and rejects the pending move', async ({ page, saved: owner, playwright }) => {
  const { saved, team, issues, target } = await fixture(owner, 1, true);
  const issued = await owner.request<InvitationIssued>('/invitations', 'POST', {
    email: `board-member-${randomUUID()}@example.test`, role: 'member', team_ids: [team.id],
  }, { 'Idempotency-Key': randomUUID() });
  const member = await playwright.request.newContext({ baseURL: saved.origin, extraHTTPHeaders: { Origin: saved.origin }, storageState: { cookies: [], origins: [] } });
  const endpoint = `${saved.origin}${saved.path}/issues/${issues[0].id}`;
  const accessEndpoint = `${saved.origin}${saved.path}/access`;
  const listRoute = (url: URL) => url.pathname === `${saved.path}/issues` && url.searchParams.get('team_id') === team.id;
  let release = () => {}; let releaseAccess = () => {}; let releaseRetry = () => {};
  try {
    const accepted = await member.post('/api/v1/invitations/accept', { data: {
      token: issued.token, display_name: 'Example Board Member', password: process.env.OPENTRACK_BROWSER_PASSWORD,
    } });
    expect(accepted.status()).toBe(200);
    const session = await accepted.json() as Session;
    await page.context().clearCookies(); await page.context().addCookies((await member.storageState()).cookies);
    await open(page, saved); await expect(page.locator(`#row-${issues[0].id}`)).toBeVisible();
    await chooseMove(page, issues[0], target.id);
    const accessGate = new Promise<void>(resolve => { releaseAccess = resolve; });
    await page.route(accessEndpoint, async route => {
      if (route.request().method() === 'GET') await accessGate;
      await route.continue();
    });
    let arrived!: () => void;
    const received = new Promise<void>(resolve => { arrived = resolve; });
    const gate = new Promise<void>(resolve => { release = resolve; });
    await page.route(endpoint, async route => {
      if (route.request().method() === 'PATCH') { arrived(); await gate; }
      await route.continue();
    });
    const rejected = patchResponse(page, saved, issues[0]);
    await moveDialog(page, issues[0]).getByRole('button', { name: `Move ${issues[0].key}`, exact: true }).click();
    await received;
    const membership = await owner.request<TeamMember>(`/teams/${team.id}/members/${session.user.id}`);
    const removed = await owner.http.delete(`${owner.path}/teams/${team.id}/members/${session.user.id}`, { headers: {
      Origin: owner.origin, 'X-CSRF-Token': owner.session.csrf_token,
      'If-Match': `"team-member:${team.id}:${session.user.id}:${membership.version}"`,
    } });
    expect(removed.status()).toBe(204);
    release();
    expect((await rejected).status()).toBe(404);
    await expect(page.locator(`#row-${issues[0].id}`)).toHaveCount(0);
    await expect(page.getByText(issues[0].title, { exact: true })).toHaveCount(0);
    expect((await member.get(`${saved.path}/issues/${issues[0].id}`)).status()).toBe(404);
    expect(await saved.issue(issues[0].id)).toEqual(issues[0]);
    let retryArrived!: () => void; let retryHeld = false;
    const retryReceived = new Promise<void>(resolve => { retryArrived = resolve; });
    const retryGate = new Promise<void>(resolve => { releaseRetry = resolve; });
    await page.route(listRoute, async route => {
      if (route.request().method() === 'GET' && !retryHeld) { retryHeld = true; retryArrived(); await retryGate; }
      await route.continue();
    });
    const list = page.locator('.issue-list');
    await list.getByRole('button', { name: 'Try again', exact: true }).click();
    await retryReceived;
    await expect(list.getByRole('combobox', { name: 'Board team', exact: true })).toHaveCount(0);
    await expect(list.locator(`option[value="${team.id}"], option[value="${target.id}"]`)).toHaveCount(0);
    await expect(list).not.toContainText(team.name);
    const retryResponse = page.waitForResponse(response => response.request().method() === 'GET' && listRoute(new URL(response.url())));
    const workflowResponse = page.waitForResponse(response => response.request().method() === 'GET'
      && new URL(response.url()).pathname === `${saved.path}/teams/${team.id}/workflow`);
    releaseRetry();
    const retried = await retryResponse; expect(retried.status()).toBe(200);
    expect((await retried.json() as IssuePage).data).toEqual([]);
    expect((await workflowResponse).status()).toBe(404);
    await expect(list.getByRole('combobox', { name: 'Board team', exact: true })).toHaveCount(0);
    await expect(list.locator(`option[value="${team.id}"], option[value="${target.id}"]`)).toHaveCount(0);
    await expect(list).not.toContainText(team.name);
    releaseAccess(); await page.unroute(accessEndpoint);
    await page.reload();
    await expect(page.locator(`#row-${issues[0].id}`)).toHaveCount(0);
    await expect(page.getByText(issues[0].title, { exact: true })).toHaveCount(0);
  } finally {
    release(); releaseAccess(); releaseRetry();
    await page.unroute(endpoint); await page.unroute(accessEndpoint); await page.unroute(listRoute);
    await member.dispose();
  }
});

test('an archived drop target preserves intent until an explicit active replacement is reviewed and saved', async ({ page, saved: owner }) => {
  const { saved, issues, statuses, target } = await fixture(owner, 1);
  const alternate = statuses.find(status => status.category === 'started')!;
  const source = statuses.find(status => status.id === issues[0].status_id)!;
  const board = await open(page, saved);
  const endpoint = `${saved.origin}${saved.path}/issues/${issues[0].id}`;
  let arrived!: () => void; let release!: () => void; let writes = 0;
  const received = new Promise<void>(resolve => { arrived = resolve; });
  const gate = new Promise<void>(resolve => { release = resolve; });
  await page.route(endpoint, async route => {
    if (route.request().method() === 'PATCH' && ++writes === 1) { arrived(); await gate; }
    await route.continue();
  });
  try {
    await chooseMove(page, issues[0], target.id);
    const rejected = patchResponse(page, saved, issues[0]);
    const dialog = moveDialog(page, issues[0]);
    await dialog.getByRole('button', { name: `Move ${issues[0].key}`, exact: true }).click();
    await received;
    const workflow = await saved.request<Workflow>(`/teams/${saved.team.id}/workflow`);
    const archived = await saved.request<Workflow>(`/teams/${saved.team.id}/statuses/${target.id}/archive`, 'POST', undefined,
      { 'If-Match': `"workflow:${saved.team.id}:${workflow.version}:true"` });
    expect(archived.statuses.some(status => status.id === target.id)).toBe(false);
    expect((await saved.request<Status>(`/teams/${saved.team.id}/statuses/${target.id}`)).archived_at).toBeTruthy();
    release(); expect((await rejected).status()).toBe(422);
    const unchanged = await saved.issue(issues[0].id);
    expect(unchanged).toEqual(issues[0]);
    await expect(board.locator(`[data-board-status="${source.id}"] #row-${unchanged.id}`)).toBeVisible();
    const select = dialog.getByRole('combobox', { name: `Move ${unchanged.key} to status`, exact: true });
    await expect(select).toHaveValue(target.id);
    await expect(select.locator(`option[value="${target.id}"]`)).toBeDisabled();
    await expect(dialog).toContainText(`Current status: ${source.name}. Requested status: ${target.name}.`);
    await dialog.getByRole('button', { name: 'Review move', exact: true }).click();
    await expect(select).toBeEnabled();
    await expect(dialog.getByRole('button', { name: 'Apply move', exact: true })).toBeDisabled();
    expect(writes).toBe(1); expect(await saved.issue(unchanged.id)).toEqual(unchanged);
    await select.selectOption(alternate.id);
    await expect(select).toHaveValue(alternate.id);
    await expect(dialog.getByRole('button', { name: 'Apply move', exact: true })).toBeEnabled();
    expect(writes).toBe(1);
    const applying = patchResponse(page, saved, unchanged);
    await dialog.getByRole('button', { name: 'Apply move', exact: true }).click();
    const applied = await applying; expect(applied.status()).toBe(200);
    expect(applied.request().headers()['if-match']).toBe(unchanged.etag);
    expect(applied.request().postDataJSON()).toEqual({ status_id: alternate.id });
    onlyStatusChanged(unchanged, await saved.issue(unchanged.id), alternate.id);
    expect(writes).toBe(2);
    await page.reload();
    await expect(page.locator(`[data-board-status="${alternate.id}"] #row-${unchanged.id}`)).toBeVisible();
  } finally { release(); await page.unroute(endpoint); }
});

test('archived and viewer boards remain read-only and the real API rejects status writes', async ({ page, saved: owner, playwright }) => {
  const { saved, issues, target } = await fixture(owner, 2);
  const archived = await saved.patch(issues[0], { archived: true });
  let board = await open(page, saved, { archived: 'true' });
  await expect(board.locator(`#row-${archived.id}`)).toBeVisible();
  await expect(board.getByRole('button', { name: `Move ${archived.key}`, exact: true })).toBeDisabled();
  await expect(board.getByRole('button', { name: `Drag ${archived.key}`, exact: true })).toBeDisabled();
  const rejectedArchive = await saved.http.patch(`${saved.path}/issues/${archived.id}`, { data: { status_id: target.id }, headers: {
    Origin: saved.origin, 'X-CSRF-Token': saved.session.csrf_token, 'If-Match': archived.etag,
  } });
  expect(rejectedArchive.status()).toBe(409);
  expect(await saved.issue(archived.id)).toEqual(archived);
  const issued = await owner.request<InvitationIssued>('/invitations', 'POST', {
    email: `board-viewer-${randomUUID()}@example.test`, role: 'viewer', team_ids: [],
  }, { 'Idempotency-Key': randomUUID() });
  const viewer = await playwright.request.newContext({ baseURL: saved.origin, extraHTTPHeaders: { Origin: saved.origin }, storageState: { cookies: [], origins: [] } });
  try {
    const accepted = await viewer.post('/api/v1/invitations/accept', { data: {
      token: issued.token, display_name: 'Example Board Viewer', password: process.env.OPENTRACK_BROWSER_PASSWORD,
    } });
    expect(accepted.status()).toBe(200);
    const session = await accepted.json() as Session;
    await page.context().clearCookies(); await page.context().addCookies((await viewer.storageState()).cookies);
    board = await open(page, saved);
    await expect(board.locator(`#row-${issues[1].id}`)).toBeVisible();
    await expect(board.getByRole('button', { name: `Move ${issues[1].key}`, exact: true })).toBeDisabled();
    await expect(board.getByRole('button', { name: `Drag ${issues[1].key}`, exact: true })).toBeDisabled();
    const read = await viewer.get(`${saved.path}/issues/${issues[1].id}`); expect(read.status()).toBe(200);
    const visible = await read.json() as Issue;
    const rejectedViewer = await viewer.patch(`${saved.path}/issues/${visible.id}`, { data: { status_id: target.id }, headers: {
      Origin: saved.origin, 'X-CSRF-Token': session.csrf_token, 'If-Match': visible.etag,
    } });
    expect(rejectedViewer.status()).toBe(403);
    expect(await saved.issue(visible.id)).toEqual(issues[1]);
  } finally { await viewer.dispose(); }
});
