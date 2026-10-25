import { randomUUID } from 'node:crypto';
import type { Locator, Page, Request as BrowserRequest, Response as BrowserResponse, Route } from '@playwright/test';
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
  const saved = new SavedWorkspace(owner.http, owner.session, { ...bootstrap, teams: [team] }, owner.origin, owner.workspace);
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

for (const recovery of ['automatic', 'Try again', 'Open all issues'] as const) {
  test('connected private board authority: ' + recovery + ' reconstructs the parent before public recovery', async ({ page, saved: owner, playwright }) => {
    const { saved, team, issues, target: originalTarget } = await fixture(owner, 1, true);
    const control = await fixture(owner, 1);
    const query = 'Board authority ' + randomUUID();
    const issue = await saved.patch(issues[0], { title: query + ' private' });
    const publicIssue = await control.saved.patch(control.issues[0], { title: query + ' public' });
    const filters = { q: query, priority: '2', sort: 'priority' };
    const workflow = await saved.request<Workflow>('/teams/' + team.id + '/workflow');
    const renamed = await saved.request<Workflow>('/teams/' + team.id + '/statuses/' + originalTarget.id, 'PATCH', {
      name: 'Private board stage ' + randomUUID(),
    }, { 'If-Match': '"workflow:' + team.id + ':' + workflow.version + ':true"' });
    const target = renamed.statuses.find(status => status.id === originalTarget.id)!;
    const issued = await owner.request<InvitationIssued>('/invitations', 'POST', {
      email: 'board-authority-' + randomUUID() + '@example.test', role: 'member', team_ids: [team.id],
    }, { 'Idempotency-Key': randomUUID() });
    const member = await playwright.request.newContext({ baseURL: saved.origin, extraHTTPHeaders: { Origin: saved.origin }, storageState: { cookies: [], origins: [] } });
    const endpoint = saved.origin + saved.path + '/issues/' + issue.id;
    const workflowEndpoint = saved.origin + saved.path + '/teams/' + team.id + '/workflow';
    const sessionEndpoint = saved.origin + '/api/v1/auth/session';
    const bootstrapEndpoint = saved.origin + saved.path + '/bootstrap';
    const accessEndpoint = saved.origin + saved.path + '/access';
    const listRoute = (address: URL) => address.origin === saved.origin && address.pathname === saved.path + '/issues';
    function deferred<T>() {
      let resolve!: (value: T) => void;
      const promise = new Promise<T>(done => { resolve = done; });
      return { promise, resolve };
    }
    const finish = deferred<void>(), moveGate = deferred<void>(), proofGate = deferred<void>();
    const sessionGate = deferred<void>(), bootstrapGate = deferred<void>(), accessGate = deferred<void>(), recoveryGate = deferred<void>(), retryGate = deferred<void>();
    const moveSeen = deferred<BrowserRequest>(), pollSeen = deferred<BrowserRequest>(), workflowPollSeen = deferred<BrowserRequest>();
    const firstProofSeen = deferred<BrowserRequest>(), proofSeen = deferred<BrowserRequest>();
    const sessionSeen = deferred<BrowserRequest>(), bootstrapSeen = deferred<BrowserRequest>(), accessSeen = deferred<BrowserRequest>();
    const recoverySeen = deferred<BrowserRequest>(), retrySeen = deferred<BrowserRequest>();
    const moves: BrowserRequest[] = [], proofs: BrowserRequest[] = [], sessions: BrowserRequest[] = [];
    const bootstraps: BrowserRequest[] = [], accesses: BrowserRequest[] = [], earlyPair: BrowserRequest[] = [], recoveryReads: BrowserRequest[] = [], earlyRecoveryReads: BrowserRequest[] = [], retryReads: BrowserRequest[] = [];
    let backgroundPoll: BrowserRequest | undefined, backgroundResponded = false, proofResponded = false, sessionResponded = false, accessResponded = false, recoveryResponded = false, recoveryArmed = false, retryArmed = false, retryResponded = false, closing = false;
    let earlySessions = 0;
    async function forward(route: Route, gate: Promise<void>) {
      await gate;
      if (closing) await route.abort('failed').catch(() => {});
      else await route.continue();
    }
    const observe = (response: BrowserResponse) => {
      if (response.request() === backgroundPoll) backgroundResponded = true;
      if (response.request() === proofs[recovery === 'automatic' ? 0 : 1] && response.status() === 404) proofResponded = true;
      if (response.request() === sessions[0] && response.status() === 200) sessionResponded = true;
      if (response.request() === accesses[0] && response.status() === 200) accessResponded = true;
      if (response.request() === recoveryReads[0]) recoveryResponded = true;
      if (response.request() === retryReads[0]) retryResponded = true;
    };
    async function responseOf(request: BrowserRequest, status: number) {
      const response = await request.response();
      expect(response !== null).toBe(true);
      expect(response!.request() === request).toBe(true);
      expect(response!.status()).toBe(status);
      return response!;
    }
    const concealed = async () => {
      for (const value of [team.name, team.key, target.name, issue.title]) await expect(page.locator('body')).not.toContainText(value);
      await expect(page.locator('option[value="' + team.id + '"], option[value="' + target.id + '"], option[value="' + issue.status_id + '"]')).toHaveCount(0);
      await expect(page.locator('#row-' + issue.id)).toHaveCount(0);
      await expect(page).not.toHaveTitle(new RegExp(team.name));
    };
    page.on('response', observe);
    try {
      const accepted = await member.post('/api/v1/invitations/accept', { data: {
        token: issued.token, display_name: 'Example Board Member', password: process.env.OPENTRACK_BROWSER_PASSWORD,
      } });
      expect(accepted.status()).toBe(200);
      const session = await accepted.json() as Session;
      const grantedResponse = await member.get(bootstrapEndpoint); expect(grantedResponse.status()).toBe(200);
      const granted = await grantedResponse.json() as Bootstrap;
      expect(granted.teams.some(value => value.id === team.id)).toBe(true);
      expect(granted.teams.some(value => value.id === control.team.id)).toBe(true);
      expect((await member.get(saved.path + '/issues/' + publicIssue.id)).status()).toBe(200);
      await page.context().clearCookies(); await page.context().addCookies((await member.storageState()).cookies);
      await open(page, saved, { ...filters, status_id: issue.status_id });
      expect(new URL(page.url()).searchParams.get('status_id')).toBe(issue.status_id);
      await expect(page.locator('#row-' + issue.id)).toBeVisible();
      await expect(page.locator('body')).toContainText(team.name);
      await chooseMove(page, issue, target.id);
      await expect(moveDialog(page, issue)).toContainText(target.name);
      await expect(moveDialog(page, issue).getByRole('button', { name: 'Move ' + issue.key, exact: true })).toBeEnabled();

      await page.route(accessEndpoint, async route => {
        if (route.request().method() !== 'GET') { await route.continue(); return; }
        if (!backgroundPoll) {
          backgroundPoll = route.request(); pollSeen.resolve(backgroundPoll);
          await finish.promise; await route.abort('failed').catch(() => {}); return;
        }
        if (!sessionResponded) earlyPair.push(route.request());
        accesses.push(route.request()); accessSeen.resolve(route.request());
        await forward(route, accessGate.promise);
      });
      await page.route(workflowEndpoint, async route => {
        if (route.request().method() !== 'GET') { await route.continue(); return; }
        if (route.request().headers()['if-none-match']) {
          workflowPollSeen.resolve(route.request());
          await finish.promise; await route.abort('failed').catch(() => {}); return;
        }
        proofs.push(route.request()); firstProofSeen.resolve(route.request());
        if (recovery !== 'automatic' && proofs.length === 1) { await route.abort('failed'); return; }
        proofSeen.resolve(route.request()); await forward(route, proofGate.promise);
      });
      await page.route(sessionEndpoint, async route => {
        if (route.request().method() !== 'GET') { await route.continue(); return; }
        if (!proofResponded) earlySessions += 1;
        sessions.push(route.request()); sessionSeen.resolve(route.request());
        await forward(route, sessionGate.promise);
      });
      await page.route(bootstrapEndpoint, async route => {
        if (route.request().method() !== 'GET') { await route.continue(); return; }
        if (!sessionResponded) earlyPair.push(route.request());
        bootstraps.push(route.request()); bootstrapSeen.resolve(route.request());
        await forward(route, bootstrapGate.promise);
      });
      await page.route(listRoute, async route => {
        const request = route.request(), address = new URL(request.url());
        if (retryArmed && request.method() === 'GET' && address.searchParams.get('team_id') === team.id) {
          retryReads.push(request); retrySeen.resolve(request);
          await forward(route, retryGate.promise); return;
        }
        const expectedTeam = recovery === 'Open all issues' ? null : control.team.id;
        if (request.method() === 'GET' && address.searchParams.get('team_id') === expectedTeam
            && Object.entries(filters).every(([key, value]) => address.searchParams.get(key) === value)) {
          if (!recoveryArmed || !accessResponded) earlyRecoveryReads.push(request);
          recoveryReads.push(request); recoverySeen.resolve(request);
          await forward(route, recoveryGate.promise); return;
        }
        await route.continue();
      });
      await page.route(endpoint, async route => {
        if (route.request().method() !== 'PATCH') { await route.continue(); return; }
        moves.push(route.request()); moveSeen.resolve(route.request()); await forward(route, moveGate.promise);
      });
      await page.evaluate(() => window.dispatchEvent(new Event('focus')));
      const heldPoll = await pollSeen.promise;
      const heldWorkflowPoll = await workflowPollSeen.promise;
      expect(heldWorkflowPoll.headers()['if-none-match'] !== undefined).toBe(true);
      await moveDialog(page, issue).getByRole('button', { name: 'Move ' + issue.key, exact: true }).click();
      const move = await moveSeen.promise;
      expect(move.headers()['if-match'] === issue.etag).toBe(true);
      expect(move.postDataJSON()).toEqual({ status_id: target.id });
      const membership = await owner.request<TeamMember>('/teams/' + team.id + '/members/' + session.user.id);
      const removed = await owner.http.delete(owner.path + '/teams/' + team.id + '/members/' + session.user.id, { headers: {
        Origin: owner.origin, 'X-CSRF-Token': owner.session.csrf_token,
        'If-Match': '"team-member:' + team.id + ':' + session.user.id + ':' + membership.version + '"',
      } });
      expect(removed.status()).toBe(204);
      moveGate.resolve(); await responseOf(move, 404);
      const firstProof = await firstProofSeen.promise;
      expect(firstProof.headers()['if-none-match']).toBeUndefined();
      let retryRead: Promise<BrowserRequest> | undefined;
      if (recovery !== 'automatic') {
        expect(await firstProof.response()).toBeNull();
        expect(firstProof.failure() !== null).toBe(true);
        const list = page.locator('.issue-list');
        await expect(list.getByRole('heading', { name: 'Board unavailable.', exact: true })).toBeVisible();
        await expect(list.getByRole('button', { name: 'Try again', exact: true })).toBeEnabled();
        await expect(list.getByRole('button', { name: 'Open all issues', exact: true })).toHaveAttribute('aria-disabled', 'false');
        await expect(list).not.toContainText(team.name);
        expect(sessions.length + bootstraps.length + accesses.length).toBe(0);
        if (recovery === 'Try again') { retryRead = retrySeen.promise; retryArmed = true; }
        await list.getByRole('button', { name: recovery, exact: true }).click();
        if (retryRead) {
          const heldRetry = await retryRead, retryQuery = new URL(heldRetry.url()).searchParams;
          expect(retryReads.length).toBe(1); expect(retryResponded).toBe(false); expect(heldRetry.failure()).toBeNull();
          for (const [key, value] of [...Object.entries(filters), ['team_id', team.id], ['status_id', issue.status_id], ['limit', '50']]) expect(retryQuery.get(key)).toBe(value);
          await expect(list).toHaveAttribute('aria-busy', 'true');
          await expect(list.getByRole('combobox', { name: 'Board team', exact: true })).toHaveCount(0);
          await expect(list.locator('option[value="' + team.id + '"], option[value="' + target.id + '"], option[value="' + issue.status_id + '"]')).toHaveCount(0);
          await expect(list).not.toContainText(team.name);
          await expect(page.locator('#row-' + issue.id)).toHaveCount(0);
          await expect(page.getByText(issue.title, { exact: true })).toHaveCount(0);
          expect(sessions.length + bootstraps.length + accesses.length).toBe(0); expect(proofs.length).toBe(1);
          retryGate.resolve();
          expect((await (await responseOf(heldRetry, 200)).json() as IssuePage).data).toEqual([]);
          expect(retryResponded).toBe(true);
        }
      }
      const proof = await proofSeen.promise;
      expect(proof.url() === workflowEndpoint).toBe(true);
      expect(proof.headers()['if-none-match']).toBeUndefined();
      expect(sessions.length + bootstraps.length + accesses.length).toBe(0);
      const pendingBoard = page.locator('.issue-list');
      await expect(pendingBoard).toHaveAttribute('aria-busy', 'true');
      await expect(pendingBoard.getByRole('combobox', { name: 'Board team', exact: true })).toHaveCount(0);
      await expect(pendingBoard.locator('option[value="' + team.id + '"], option[value="' + target.id + '"], option[value="' + issue.status_id + '"]')).toHaveCount(0);
      await expect(pendingBoard).not.toContainText(team.name);
      proofGate.resolve(); await responseOf(proof, 404);
      await concealed();
      expect((await member.get(saved.path + '/issues/' + issue.id)).status()).toBe(404);
      await expect(page.getByRole('dialog', { name: 'Move ' + issue.key, exact: true })).toHaveCount(0);
      await expect(page.locator('#row-' + publicIssue.id)).toHaveCount(0);
      expect(earlyRecoveryReads.length).toBe(0); expect(recoveryReads.length).toBe(0);
      expect(backgroundResponded).toBe(false); expect(heldPoll.failure()).toBeNull();

      const canonicalSessionRequest = await sessionSeen.promise;
      expect(earlySessions).toBe(0);
      expect(sessions.length).toBe(1); expect(bootstraps.length + accesses.length).toBe(0);
      expect(earlyRecoveryReads.length).toBe(0); expect(recoveryReads.length).toBe(0);
      sessionGate.resolve();
      const refreshedSession = await (await responseOf(canonicalSessionRequest, 200)).json() as Session;
      expect(refreshedSession.user.id === session.user.id).toBe(true);
      expect(refreshedSession.csrf_token === session.csrf_token).toBe(true);
      expect(refreshedSession.workspaces.find(value => value.id === saved.workspace)?.role).toBe('member');
      const acceptedBootstrapRequest = await bootstrapSeen.promise;
      const acceptedAccessRequest = await accessSeen.promise;
      expect(earlyPair.length).toBe(0);
      expect(bootstraps.length).toBe(1); expect(accesses.length).toBe(1);
      bootstrapGate.resolve();
      const refreshed = await (await responseOf(acceptedBootstrapRequest, 200)).json() as Bootstrap;
      expect(refreshed.auth_epoch === granted.auth_epoch).toBe(false);
      expect(refreshed.teams.some(value => value.id === team.id)).toBe(false);
      expect(refreshed.statuses.some(value => value.team_id === team.id)).toBe(false);
      expect(refreshed.teams.some(value => value.id === control.team.id)).toBe(true);
      await concealed();
      await expect(page.locator('#row-' + publicIssue.id)).toHaveCount(0);
      expect(earlyRecoveryReads.length).toBe(0); expect(recoveryReads.length).toBe(0);
      recoveryArmed = true;
      accessGate.resolve();
      const access = await (await responseOf(acceptedAccessRequest, 200)).json() as { auth_epoch: string; role: string; archived: boolean };
      expect(access.auth_epoch === refreshed.auth_epoch).toBe(true);
      expect(access.role).toBe('member'); expect(access.archived).toBe(false);
      if (retryRead) expect((await (await responseOf(await retryRead, 200)).json() as IssuePage).data.length).toBe(0);
      for (const [key, value] of Object.entries(filters)) expect(new URL(page.url()).searchParams.get(key)).toBe(value);
      if (recovery === 'Open all issues') {
        await expect(page).toHaveURL(address => !address.searchParams.has('team_id') && !address.searchParams.has('status_id')
          && (address.searchParams.get('layout') ?? 'list') === 'list');
      } else {
        expect(new URL(page.url()).searchParams.get('team_id')).toBe(team.id);
        expect(new URL(page.url()).searchParams.get('layout')).toBe('board');
        expect(new URL(page.url()).searchParams.get('status_id')).toBe(issue.status_id);
        await expect(page.getByRole('heading', { name: 'Team unavailable.', exact: true })).toBeVisible();
        await page.getByRole('combobox', { name: 'Board team', exact: true }).selectOption(control.team.id);
      }
      const recoveredRequest = await recoverySeen.promise;
      expect(new URL(page.url()).searchParams.has('status_id')).toBe(false);
      const receivedQuery = new URL(recoveredRequest.url()).searchParams;
      for (const [key, value] of Object.entries(filters)) expect(receivedQuery.get(key)).toBe(value);
      expect(receivedQuery.get('team_id') === (recovery === 'Open all issues' ? null : control.team.id)).toBe(true);
      expect(receivedQuery.get('limit')).toBe('50');
      expect(['status_id', 'layout', 'display_version'].some(key => receivedQuery.has(key))).toBe(false);
      expect(earlyRecoveryReads.length).toBe(0); expect(recoveryReads.length).toBe(1);
      expect(recoveredRequest.failure()).toBeNull(); expect(recoveryResponded).toBe(false);
      await expect(page.locator('.issue-list')).toHaveAttribute('aria-busy', 'true');
      await expect(page.locator('#row-' + publicIssue.id)).toHaveCount(0);
      await concealed();
      recoveryGate.resolve();
      const recovered = await (await responseOf(recoveredRequest, 200)).json() as IssuePage;
      expect(recoveryResponded).toBe(true);
      expect(recovered.data.some(value => value.id === publicIssue.id)).toBe(true);
      expect(recovered.data.some(value => value.id === issue.id)).toBe(false);
      await expect(page.locator('#row-' + publicIssue.id)).toBeVisible();
      await concealed();
      expect(recoveryReads.length).toBe(1);
      expect(retryReads.length).toBe(recovery === 'Try again' ? 1 : 0);
      expect(sessions.length).toBe(1); expect(bootstraps.length).toBe(1); expect(accesses.length).toBe(1);
      expect(proofs.length).toBe(recovery === 'automatic' ? 1 : 2);
      expect(backgroundResponded).toBe(false); expect(heldPoll.failure()).toBeNull();
      expect(moves.length).toBe(1);
      expect(await saved.issue(issue.id)).toEqual(issue);
    } finally {
      closing = true; finish.resolve(); moveGate.resolve(); proofGate.resolve();
      sessionGate.resolve(); bootstrapGate.resolve(); accessGate.resolve(); recoveryGate.resolve(); retryGate.resolve();
      page.off('response', observe);
      await page.unroute(endpoint); await page.unroute(workflowEndpoint); await page.unroute(sessionEndpoint);
      await page.unroute(bootstrapEndpoint); await page.unroute(accessEndpoint); await page.unroute(listRoute);
      await member.dispose();
    }
    if (recovery === 'Try again') {
      await page.goto(url(saved, { ...filters, status_id: issue.status_id }));
      await page.reload();
      await expect(page.getByRole('heading', { name: 'Team unavailable.', exact: true })).toBeVisible();
      await concealed();
    }
  });
}

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
