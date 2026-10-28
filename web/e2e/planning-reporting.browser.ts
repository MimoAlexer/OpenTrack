import { randomUUID } from 'node:crypto';
import { writeFileSync } from 'node:fs';
import { devices, type APIRequest, type APIResponse, type BrowserContext, type Locator, type Page } from '@playwright/test';
import { test, expect, SavedWorkspace, registerAdmission } from './fixtures';
import type { Bootstrap, Issue, IssuePage, Session } from '../src/types';
import type { InvitationIssued, TeamDetail, TeamMember } from '../src/settings/types';
import type { Workflow } from '../src/workflows/types';
import type { FullProject, ProjectProgress } from '../src/projects/types';
import type { Milestone } from '../src/projects/Milestones';
import type { Cycle, Snapshot, SnapshotPoint } from '../src/cycles/types';

registerAdmission(3);

async function planning(saved: SavedWorkspace, privateTeam = false) {
  const token = randomUUID();
  const team = await saved.request<TeamDetail>('/teams', 'POST', {
    name: `Reporting ${token}`, key: `R${token.replaceAll('-', '').slice(0, 8).toUpperCase()}`, private: privateTeam,
  }, { 'Idempotency-Key': randomUUID() });
  const workflow = await saved.request<Workflow>(`/teams/${team.id}/workflow`);
  const projectDirectory = `/teams/${team.id}/projects`;
  let project = await saved.request<FullProject>(projectDirectory, 'POST', { name: `Reporting plan ${token}` }, { 'Idempotency-Key': randomUUID() });
  const projectPath = `${projectDirectory}/${project.id}`;
  const milestone = await saved.request<Milestone>(`${projectPath}/milestones`, 'POST', { title: `Reporting outcome ${token}` },
    { 'If-Match': project.etag, 'Idempotency-Key': randomUUID() });
  project = await saved.request<FullProject>(projectPath);
  const cycleDirectory = `/teams/${team.id}/cycles`;
  let cycle = await saved.request<Cycle>(cycleDirectory, 'POST', {
    name: `Reporting cycle ${token}`, start_date: '2026-06-01', end_date_exclusive: '2026-06-08', timezone: 'UTC',
  }, { 'Idempotency-Key': randomUUID() });
  const cyclePath = `${cycleDirectory}/${cycle.id}`;
  const issues: Issue[] = [];
  for (const [index, category] of ['unstarted', 'completed', 'canceled', 'unstarted', 'unstarted'].entries()) {
    const status = workflow.statuses.find(status => status.category === category)!;
    expect(status).toBeDefined();
    const issue = await saved.request<Issue>('/issues', 'POST', {
      team_id: team.id, status_id: status.id, title: `Reporting work ${index} ${token}`,
    }, { 'Idempotency-Key': randomUUID() });
    project = await saved.request<FullProject>(`${projectPath}/issues/${issue.id}`, 'PUT', { included: true }, { 'If-Match': project.etag });
    project = await saved.request<FullProject>(`${projectPath}/issues/${issue.id}/milestone`, 'PUT', { milestone_id: milestone.id }, { 'If-Match': project.etag });
    cycle = await saved.request<Cycle>(`${cyclePath}/issues/${issue.id}`, 'PUT', {
      included: true, issue_etag: (await saved.issue(issue.id)).etag,
    }, { 'If-Match': cycle.etag });
    issues.push(issue);
  }
  cycle = await saved.request<Cycle>(`${cyclePath}/open`, 'POST', undefined, { 'If-Match': cycle.etag });
  const daily = await saved.request<Snapshot>(`${cyclePath}/snapshots`, 'POST', undefined, { 'If-Match': cycle.etag });
  return { team, workflow, project, projectPath, milestone, milestonePath: `${projectPath}/milestones/${milestone.id}`, cycle, cyclePath, issues, daily };
}
type Planning = Awaited<ReturnType<typeof planning>>;
const projectUrl = (saved: SavedWorkspace, plan: Planning, id = plan.project.id) => `/w/${saved.workspace}/projects/${id}?team_id=${plan.team.id}`;
const cycleUrl = (saved: SavedWorkspace, plan: Planning) => `/w/${saved.workspace}/cycles/${plan.cycle.id}?team_id=${plan.team.id}`;
const ids = (rows: Issue[]) => rows.map(issue => issue.id).sort();
function category(plan: Planning, issue: Issue) {
  const status = plan.workflow.statuses.find(status => status.id === issue.status_id);
  expect(status).toBeDefined();
  return status!.category;
}

async function rows(saved: SavedWorkspace, plan: Planning, sort: 'created' | 'manual') {
  const all: Issue[] = []; const seen = new Set<string>(); let cursor: string | null = null;
  for (let page = 0; page < 6; page++) {
    const result: IssuePage = await saved.request(`/issues?${new URLSearchParams({ team_id: plan.team.id, sort, limit: '1', ...(cursor ? { cursor } : {}) })}`);
    for (const issue of result.data) { expect(all.some(item => item.id === issue.id)).toBe(false); all.push(issue); }
    if (!result.has_more) { expect(result.next_cursor).toBeNull(); return all; }
    expect(result.next_cursor).toBeTruthy(); expect(seen.has(result.next_cursor!)).toBe(false);
    seen.add(result.next_cursor!); cursor = result.next_cursor;
  }
  throw new Error('Planning source pagination exceeded its six-page bound.');
}
async function authoritative(saved: SavedWorkspace, plan: Planning) {
  const source = await rows(saved, plan, 'created');
  const manual = await rows(saved, plan, 'manual');
  expect(ids(manual)).toEqual(ids(source));
  for (const issue of source) expect(manual.find(item => item.id === issue.id)).toEqual(issue);
  const completed = source.filter(issue => category(plan, issue) === 'completed').length;
  const canceled = source.filter(issue => category(plan, issue) === 'canceled').length;
  const expected = { total: source.length, completed, canceled, eligible: source.length - canceled,
    open: source.length - completed - canceled, unscoped: source.length === canceled };
  const project = await saved.request<ProjectProgress>(`${plan.projectPath}/progress`);
  const milestone = await saved.request<ProjectProgress>(`${plan.milestonePath}/progress`);
  expect(project).toMatchObject(expected); expect(milestone).toMatchObject(expected);
  const scope = await saved.request<Snapshot>(`${plan.cyclePath}/scope`);
  expect(scope).toMatchObject({ kind: 'current', scope_count: source.length, completed_count: completed,
    canceled_count: canceled, unfinished_count: expected.open });
  expect(scope.items.filter(item => !item.excluded).map(item => item.issue_id).sort()).toEqual(ids(source));
  const history = await saved.request<{ data: SnapshotPoint[] }>(`${plan.cyclePath}/snapshots`);
  const daily = history.data.find(point => point.id === plan.daily.id)!;
  expect(daily).toBeDefined(); expect(daily.captured_at).toBe(plan.daily.captured_at); expect(daily.kind).toBe('daily');
  expect(history.data).toHaveLength(2);
  return { source, expected, project, milestone, scope };
}
async function progress(panel: Locator, completed: number, eligible: number, cycle = false) {
  await expect(panel).toContainText(`${completed} / ${eligible} ${cycle ? 'eligible issues' : 'issues'}`);
  if (!eligible) {
    await expect(panel).toContainText('No eligible issue scope');
    await expect(panel.locator('progress')).toHaveCount(0);
    await expect(panel).not.toContainText(/\d+% complete/);
  } else {
    await expect(panel.locator('progress')).toHaveAttribute('value', String(completed));
    await expect(panel.locator('progress')).toHaveAttribute('max', String(eligible));
  }
}
async function rendered(saved: SavedWorkspace, plan: Planning, page: Page, source: Issue[], counts: Pick<ProjectProgress, 'completed' | 'eligible'>) {
  for (const layout of ['list', 'board']) {
    await page.goto(`/w/${saved.workspace}/issues?${new URLSearchParams({ team_id: plan.team.id, sort: 'manual', layout, display_version: '1' })}`);
    if (layout === 'list') {
      await expect.poll(() => page.locator('.issue-groups a.issue-row').evaluateAll(nodes => nodes.map(node => node.id.replace('row-', '')).sort())).toEqual(ids(source));
    } else {
      const list = page.getByRole('region', { name: 'Issue list', exact: true });
      await expect(page.getByRole('combobox', { name: 'Issue layout', exact: true })).toHaveValue('board');
      await expect(list).toHaveAttribute('aria-busy', 'false');
      const board = list.locator('.issue-board-shell'); await expect(board).toBeVisible();
      const observed = new Set<string>();
      for (const status of new Set(source.map(issue => issue.status_id))) {
        await board.getByRole('combobox', { name: 'Board columns', exact: true }).selectOption(status);
        const columns = await board.locator('[data-order-status]').evaluateAll(nodes => nodes.map(node => node.getAttribute('data-order-status')));
        const expected = source.filter(issue => columns.includes(issue.status_id));
        await expect.poll(() => board.locator('[id^="row-"]').evaluateAll(nodes => nodes.map(node => node.id.replace('row-', '')).sort())).toEqual(ids(expected));
        for (const id of columns) {
          const column = board.locator(`[data-order-status="${id}"]`);
          await expect.poll(() => column.locator('a.issue-row').evaluateAll(nodes => nodes.map(node => node.id.replace('row-', '')).sort())).toEqual(ids(source.filter(issue => issue.status_id === id)));
        }
        for (const issue of expected) observed.add(issue.id);
      }
      if (!source.length) await expect(board.locator('[id^="row-"]')).toHaveCount(0);
      expect([...observed].sort()).toEqual(ids(source));
    }
  }
  await page.goto(projectUrl(saved, plan));
  await expect(page.getByRole('heading', { name: plan.project.name, exact: true })).toBeVisible();
  const { completed, eligible } = counts;
  await progress(page.locator('.project-progress-section'), completed, eligible);
  await progress(page.locator('.milestone-group').filter({ has: page.getByRole('heading', { name: plan.milestone.title, exact: true }) }), completed, eligible);
  await page.goto(cycleUrl(saved, plan));
  const scope = page.getByRole('region', { name: 'Cycle scope', exact: true });
  await expect(scope).toContainText(/Live scope calculated|Current scope validated/); await progress(scope, completed, eligible, true);
}
function fact(kind: string, value: object) {
  writeFileSync(test.info().outputPath(`planning-reporting-${kind}.json`), JSON.stringify({ kind, engine: test.info().project.name,
    reporting_consumer_exercised: false, ...value }));
}

async function holdRead(page: Page, url: string) {
  let ready!: (value: APIResponse) => void; let failed!: (error: Error) => void;
  let release!: (value: { fail: boolean; abortExpected: boolean }) => void;
  let finish!: (value: 'fulfilled' | 'aborted' | 'read_failed') => void; let releaseFailed!: (error: Error) => void; let started = false;
  const response = new Promise<APIResponse>((resolve, reject) => { ready = resolve; failed = reject; });
  const permit = new Promise<{ fail: boolean; abortExpected: boolean }>(resolve => { release = resolve; });
  const done = new Promise<'fulfilled' | 'aborted' | 'read_failed'>((resolve, reject) => { finish = resolve; releaseFailed = reject; });
  await page.route(url, async route => {
    started = true;
    let value: APIResponse;
    try { value = await route.fetch(); }
    catch { failed(new Error('Held planning read failed; details withheld.')); finish('read_failed'); await route.abort('failed').catch(() => {}); return; }
    if (value.status() !== 200) { failed(new Error(`Held planning read returned HTTP ${value.status()}`)); finish('read_failed'); await route.fulfill({ response: value }); return; }
    ready(value);
    const action = await permit;
    try {
      await route.fulfill(action.fail ? { status: 503, contentType: 'application/problem+json', body: JSON.stringify({ code: 'synthetic_transport_failure', detail: 'Synthetic transport failure' }) } : { response: value });
      finish('fulfilled');
    } catch {
      if (!action.abortExpected) { releaseFailed(new Error('Planning response release failed; details withheld.')); return; }
      try { await expect.poll(() => route.request().failure() !== null).toBe(true); finish('aborted'); }
      catch { releaseFailed(new Error('A late planning response did not reach a confirmed aborted request.')); }
    }
  }, { times: 1 });
  return { ready: response, release: async (fail = false, abortExpected = false) => { release({ fail, abortExpected }); return started ? done : 'not_observed'; } };
}

test('current planning counts match exact authorized rows through exclusions and zero scope', async ({ page, saved }) => {
  const plan = await planning(saved); const observed: object[] = [];
  const check = async (phase: string) => { const current = await authoritative(saved, plan); await rendered(saved, plan, page, current.source, current.expected); observed.push({ phase, ids: ids(current.source), ...current.expected }); };
  await check('populated');
  const moving = await saved.issue(plan.issues[0].id);
  const order = await saved.request<{ order_version: string }>(`/issues?${new URLSearchParams({ team_id: plan.team.id, status_id: moving.status_id, sort: 'manual', limit: '100' })}`);
  const body = { order_version: order.order_version, after_id: plan.issues[3].id };
  const headers = { 'If-Match': moving.etag, 'Idempotency-Key': randomUUID() };
  const acknowledgement = await saved.request(`/issues/${moving.id}/position`, 'POST', body, headers);
  expect(await saved.request(`/issues/${moving.id}/position`, 'POST', body, headers)).toEqual(acknowledgement);
  await check('manual-replay');
  const completed = plan.workflow.statuses.find(status => status.category === 'completed')!;
  await saved.patch(await saved.issue(moving.id), { status_id: completed.id });
  await saved.patch(await saved.issue(plan.issues[3].id), { archived: true });
  const deleted = await saved.issue(plan.issues[4].id);
  expect((await saved.http.delete(`${saved.path}/issues/${deleted.id}`, { headers: { Origin: saved.origin, 'X-CSRF-Token': saved.session.csrf_token, 'If-Match': deleted.etag } })).status()).toBe(204);
  await check('excluded');
  const canceled = plan.workflow.statuses.find(status => status.category === 'canceled')!;
  for (const issue of plan.issues.slice(0, 2)) await saved.patch(await saved.issue(issue.id), { status_id: canceled.id });
  await check('all-canceled');
  for (const issue of plan.issues.slice(0, 3)) await saved.patch(await saved.issue(issue.id), { archived: true });
  await check('empty');
  fact('source-parity', { observed, daily_id: plan.daily.id, daily_captured_at: plan.daily.captured_at });
});

test('planning counts retain calculation times through held reads, failures and reviewed refresh', async ({ page, saved }) => {
  const plan = await planning(saved); const gates: Awaited<ReturnType<typeof holdRead>>[] = [];
  try {
    await page.goto(projectUrl(saved, plan));
    const project = page.locator('.project-progress-section'); const milestone = page.locator('.milestone-group');
    await progress(project, 1, 4); await progress(milestone, 1, 4);
    const projectTime = await project.locator('time[data-planning-calculation]').getAttribute('datetime'); const milestoneTime = await milestone.locator('time[data-planning-calculation]').getAttribute('datetime');
    const projectGate = await holdRead(page, `${saved.origin}${saved.path}${plan.projectPath}/progress`);
    const milestoneGate = await holdRead(page, `${saved.origin}${saved.path}${plan.projectPath}/milestones?*`);
    gates.push(projectGate, milestoneGate);
    await page.getByRole('button', { name: 'Refresh planning counts', exact: true }).click();
    await Promise.all([projectGate.ready, milestoneGate.ready]);
    await expect(project).toContainText('Updating planning counts'); await expect(page.getByRole('region', { name: 'Project milestones', exact: true })).toContainText('Updating milestone counts');
    await expect(project.locator('time[data-planning-calculation]')).toHaveAttribute('datetime', projectTime!); await expect(milestone.locator('time[data-planning-calculation]')).toHaveAttribute('datetime', milestoneTime!);
    await Promise.all([projectGate.release(true), milestoneGate.release(true)]);
    await expect(project).toContainText('Counts could not be refreshed'); await expect(page.getByRole('region', { name: 'Project milestones', exact: true })).toContainText('Milestone counts could not be refreshed');
    await progress(project, 1, 4); await expect(project.locator('time[data-planning-calculation]')).toHaveAttribute('datetime', projectTime!);
    await saved.patch(await saved.issue(plan.issues[0].id), { status_id: plan.workflow.statuses.find(status => status.category === 'completed')!.id });
    const freshProject = page.waitForResponse(response => response.url().endsWith(`${plan.projectPath}/progress`) && response.request().method() === 'GET');
    await page.getByRole('button', { name: 'Refresh planning counts', exact: true }).click();
    const projectRead = await (await freshProject).json() as ProjectProgress;
    await progress(project, 2, 4); await progress(milestone, 2, 4); await expect(project.locator('time[data-planning-calculation]')).toHaveAttribute('datetime', projectRead.as_of);
    await page.getByRole('navigation', { name: 'Project sections', exact: true }).getByRole('button', { name: 'Issues', exact: true }).click();
    await expect(page.locator('.project-issue-row').filter({ hasText: plan.issues[0].title }).locator('.project-issue-status')).toHaveText(plan.workflow.statuses.find(status => status.category === 'completed')!.name);

    await page.goto(cycleUrl(saved, plan)); const scope = page.getByRole('region', { name: 'Cycle scope', exact: true }); await progress(scope, 2, 4, true);
    const cycleTime = await scope.locator('time[data-planning-calculation]').getAttribute('datetime'); const cycleGate = await holdRead(page, `${saved.origin}${saved.path}${plan.cyclePath}/scope`); gates.push(cycleGate);
    await page.getByRole('button', { name: 'Refresh cycle counts', exact: true }).click(); await cycleGate.ready;
    await expect(scope).toContainText('Updating cycle counts'); await expect(scope.locator('time[data-planning-calculation]')).toHaveAttribute('datetime', cycleTime!);
    await cycleGate.release(true); await expect(scope).toContainText('Cycle counts could not be refreshed'); await progress(scope, 2, 4, true);
    await saved.patch(await saved.issue(plan.issues[3].id), { status_id: plan.workflow.statuses.find(status => status.category === 'completed')!.id });
    const freshCycle = page.waitForResponse(response => response.url().endsWith(`${plan.cyclePath}/scope`) && response.request().method() === 'GET');
    await page.getByRole('button', { name: 'Refresh cycle counts', exact: true }).click(); const cycleRead = await (await freshCycle).json() as Snapshot;
    await progress(scope, 3, 4, true); await expect(scope.locator('time[data-planning-calculation]')).toHaveAttribute('datetime', cycleRead.captured_at);
    await expect(scope.getByRole('row').filter({ hasText: plan.issues[3].title }).getByRole('cell').nth(1)).toHaveText(plan.workflow.statuses.find(status => status.category === 'completed')!.name);

    const control = await saved.request<FullProject>(`/teams/${plan.team.id}/projects`, 'POST', { name: `Fresh control ${randomUUID()}` }, { 'Idempotency-Key': randomUUID() });
    await page.goto(projectUrl(saved, plan)); await progress(project, 3, 4);
    const old = await holdRead(page, `${saved.origin}${saved.path}${plan.projectPath}/progress`); gates.push(old);
    await page.getByRole('button', { name: 'Refresh planning counts', exact: true }).click(); await old.ready;
    await page.locator('.project-back').click(); await page.getByRole('link', { name: new RegExp(control.name) }).click();
    await expect(page.getByRole('heading', { name: control.name, exact: true })).toBeVisible(); await progress(project, 0, 0);
    const late = await old.release(false, true); await progress(project, 0, 0); await expect(page.getByRole('heading', { name: plan.project.name, exact: true })).toHaveCount(0);
    fact('transport-freshness', { synthetic_failed_gets: 3, project_time_before: projectTime, project_time_after: projectRead.as_of,
      cycle_time_before: cycleTime, cycle_time_after: cycleRead.captured_at, late_response: late, authoritative_reads: true });
  } finally { for (const gate of gates) await gate.release(false, true); }
});

async function invited(api: APIRequest, saved: SavedWorkspace, teamIds: string[], role: 'member' | 'viewer' = 'viewer') {
  const invitation = await saved.request<InvitationIssued>('/invitations', 'POST', {
    email: `reporting-viewer-${randomUUID()}@example.test`, role, team_ids: teamIds,
  }, { 'Idempotency-Key': randomUUID() });
  const http = await api.newContext({ baseURL: saved.origin, extraHTTPHeaders: { Origin: saved.origin }, storageState: { cookies: [], origins: [] } });
  try {
    const accepted = await http.post('/api/v1/invitations/accept', { data: { token: invitation.token, display_name: 'Example Viewer', password: process.env.OPENTRACK_BROWSER_PASSWORD } });
    expect(accepted.status()).toBe(200); const session = await accepted.json() as Session;
    const bootstrap = await http.get(`${saved.path}/bootstrap`); expect(bootstrap.status()).toBe(200);
    return new SavedWorkspace(http, session, await bootstrap.json() as Bootstrap, saved.origin, saved.workspace);
  } catch (error) { await http.dispose(); throw error; }
}

test('populated private planning counts disappear after real access revocation without stale resurrection', async ({ browser, browserName, playwright, saved }) => {
  const plan = await planning(saved, true); const viewer = await invited(playwright.request, saved, [plan.team.id]);
  const gates: Awaited<ReturnType<typeof holdRead>>[] = []; let outsider: SavedWorkspace | undefined; let context: BrowserContext | undefined;
  try {
    outsider = await invited(playwright.request, saved, []);
    const profile = { chromium: 'Desktop Chrome', firefox: 'Desktop Firefox', webkit: 'Desktop Safari' } as const; const canonical = test.info().project.use;
    expect(canonical.actionTimeout).toBe(10_000);
    context = await browser.newContext({ ...devices[profile[browserName]], baseURL: saved.origin, storageState: await viewer.http.storageState(), locale: canonical.locale, timezoneId: canonical.timezoneId, serviceWorkers: canonical.serviceWorkers });
    context.setDefaultTimeout(canonical.actionTimeout!); context.setDefaultNavigationTimeout(canonical.navigationTimeout ?? 0);
    await context.route('**/*', async route => { if (new URL(route.request().url()).origin === new URL(saved.origin).origin) await route.continue(); else await route.abort('blockedbyclient'); });
    expect((await authoritative(viewer, plan)).expected).toMatchObject({ completed: 1, eligible: 4, total: 5 });
    const paths = [`${plan.projectPath}/progress`, `${plan.milestonePath}/progress`, `${plan.cyclePath}/scope`];
    for (const path of paths) expect((await outsider.http.get(`${saved.path}${path}`)).status()).toBe(404);
    const projectPage = await context.newPage(); await projectPage.goto(projectUrl(saved, plan));
    const cyclePage = await context.newPage(); await cyclePage.goto(cycleUrl(saved, plan));
    await progress(projectPage.locator('.project-progress-section'), 1, 4); await progress(projectPage.locator('.milestone-group'), 1, 4);
    await progress(cyclePage.getByRole('region', { name: 'Cycle scope', exact: true }), 1, 4, true);
    const projectGate = await holdRead(projectPage, `${saved.origin}${saved.path}${plan.projectPath}/progress`);
    const cycleGate = await holdRead(cyclePage, `${saved.origin}${saved.path}${plan.cyclePath}/scope`); gates.push(projectGate, cycleGate);
    await projectPage.getByRole('button', { name: 'Refresh planning counts', exact: true }).click();
    await cyclePage.getByRole('button', { name: 'Refresh cycle counts', exact: true }).click(); await Promise.all([projectGate.ready, cycleGate.ready]);
    const member = await saved.request<TeamMember>(`/teams/${plan.team.id}/members/${viewer.session.user.id}`);
    const removed = await saved.http.delete(`${saved.path}/teams/${plan.team.id}/members/${viewer.session.user.id}`, { headers: {
      Origin: saved.origin, 'X-CSRF-Token': saved.session.csrf_token, 'If-Match': `"team-member:${plan.team.id}:${viewer.session.user.id}:${member.version}"`,
    } }); expect(removed.status()).toBe(204);
    for (const path of paths) expect((await viewer.http.get(`${saved.path}${path}`)).status()).toBe(404);
    for (const page of [projectPage, cyclePage]) await page.evaluate(() => window.dispatchEvent(new Event('focus')));
    await expect(projectPage.getByRole('heading', { name: /^(Team|Project) unavailable$/ })).toBeVisible();
    await expect(cyclePage.getByRole('heading', { name: /^(Team|Cycle) unavailable$/ })).toBeVisible();
    const late = await Promise.all([projectGate.release(false, true), cycleGate.release(false, true)]);
    for (const page of [projectPage, cyclePage]) {
      await expect(page.locator('.project-progress, .cycle-summary')).toHaveCount(0);
      await expect(page.getByText('No eligible issue scope', { exact: true })).toHaveCount(0);
      await expect(page.getByRole('heading', { name: plan.project.name, exact: true })).toHaveCount(0);
    }
    expect((await authoritative(saved, plan)).expected).toMatchObject({ completed: 1, eligible: 4, total: 5 });
    fact('private-authority', { outsider_statuses: [404, 404, 404], revoked_statuses: [404, 404, 404], late_responses: late, owner_total: 5, viewer_scope_removed: true });
  } finally {
    for (const gate of gates) await gate.release(false, true);
    await context?.close(); await viewer.http.dispose(); await outsider?.http.dispose();
  }
});


test('planning projections show actual validation and fact times while the default worker also clears revoked assignments', async ({ page, playwright, saved, context }) => {
  const plan = await planning(saved);
  await expect.poll(async () => {
    const current = await authoritative(saved, plan);
    return [current.project.calculation_source, current.milestone.calculation_source, current.scope.calculation_source];
  }, { timeout: 25_000 }).toEqual(['projection', 'projection', 'projection']);
  const projected = await authoritative(saved, plan);
  await page.goto(projectUrl(saved, plan));
  const project = page.locator('.project-progress-section');
  const milestone = page.locator('.milestone-group').filter({ has: page.getByRole('heading', { name: plan.milestone.title, exact: true }) });
  await progress(project, 1, 4); await progress(milestone, 1, 4);
  const cyclePage = await context.newPage(); await cyclePage.goto(cycleUrl(saved, plan));
  const cycle = cyclePage.getByRole('region', { name: 'Cycle scope', exact: true });
  await progress(cycle, 1, 4, true);
  for (const [panel, value] of [[project, projected.project], [milestone, projected.milestone], [cycle, projected.scope]] as const) {
    await expect(panel).toContainText('Projection verified against current issues');
    await expect(panel.locator('time[data-planning-facts]')).toHaveAttribute('datetime', value.facts_as_of!);
    const validation = await panel.locator('time[data-planning-calculation]').getAttribute('datetime');
    expect(Date.parse(validation!)).toBeGreaterThanOrEqual(Date.parse(value.facts_as_of!));
  }
  await expect(cycle).toContainText('Current scope validated');
  expect(plan.daily.calculation_source).toBe('snapshot'); expect(plan.daily.facts_as_of).toBeNull();
  const hidden = await saved.request<TeamDetail>('/teams', 'POST', { name: `Cleanup ${randomUUID()}`, key: `C${randomUUID().replaceAll('-', '').slice(0, 8).toUpperCase()}`, private: true }, { 'Idempotency-Key': randomUUID() });
  const member = await invited(playwright.request, saved, [hidden.id], 'member');
  const gates: Awaited<ReturnType<typeof holdRead>>[] = [];
  try {
    const assigned = await saved.request<Issue>('/issues', 'POST', { team_id: hidden.id, title: `Revoked assignment ${randomUUID()}`, assignee_id: member.session.user.id }, { 'Idempotency-Key': randomUUID() });
    expect(assigned.assignee_id).toBe(member.session.user.id);
    const membership = await saved.request<TeamMember>(`/teams/${hidden.id}/members/${member.session.user.id}`);
    const revoked = await saved.http.delete(`${saved.path}/teams/${hidden.id}/members/${member.session.user.id}`, { headers: { Origin: saved.origin, 'X-CSRF-Token': saved.session.csrf_token, 'If-Match': `"team-member:${hidden.id}:${member.session.user.id}:${membership.version}"` } });
    expect(revoked.status()).toBe(204);
    await expect.poll(async () => (await saved.issue(assigned.id)).assignee_id, { timeout: 25_000 }).toBeNull();
    const after = await authoritative(saved, plan);
    for (const key of ['project', 'milestone', 'scope'] as const) {
      expect(after[key].calculation_source).toBe('projection'); expect(after[key].facts_as_of).toBe(projected[key].facts_as_of);
    }
    await page.bringToFront(); await page.reload();
    await progress(project, 1, 4); await expect(project).toHaveAttribute('aria-busy', 'false');
    const calculated = await project.locator('time[data-planning-calculation]').getAttribute('datetime');
    const facts = await project.locator('time[data-planning-facts]').getAttribute('datetime');
    const gate = await holdRead(page, `${saved.origin}${saved.path}${plan.projectPath}/progress`); gates.push(gate);
    await page.getByRole('button', { name: 'Refresh planning counts', exact: true }).click(); await gate.ready;
    await expect(project).toContainText('Updating planning counts');
    await gate.release(true);
    await expect(project).toContainText('Counts could not be refreshed');
    await expect(project).toContainText('Projection verified against current issues');
    await expect(project.locator('time[data-planning-calculation]')).toHaveAttribute('datetime', calculated!);
    await expect(project.locator('time[data-planning-facts]')).toHaveAttribute('datetime', facts!);
    const current = await saved.request<FullProject>(plan.projectPath);
    const empty = await saved.request<Milestone>(`${plan.projectPath}/milestones`, 'POST', { title: `Empty projection scope ${randomUUID()}` }, { 'If-Match': current.etag, 'Idempotency-Key': randomUUID() });
    await page.getByRole('button', { name: 'Refresh planning counts', exact: true }).click();
    const emptyPanel = page.locator('.milestone-group').filter({ has: page.getByRole('heading', { name: empty.title, exact: true }) });
    await expect(emptyPanel).toContainText('No eligible issue scope'); await expect(emptyPanel).toContainText('Live issue data');
    await expect(emptyPanel.locator('[data-planning-facts]')).toHaveCount(0);
    const history = await saved.request<{ data: SnapshotPoint[] }>(`${plan.cyclePath}/snapshots`);
    expect(history.data.find(item => item.id === plan.daily.id)?.captured_at).toBe(plan.daily.captured_at);
    fact('projection-provenance', { reporting_consumer_exercised: true, source: 'projection', project_fact_time: projected.project.facts_as_of, milestone_fact_time: projected.milestone.facts_as_of, cycle_fact_time: projected.scope.facts_as_of, revoked_assignment_cleared: true, unrelated_facts_unchanged: true, failed_refresh_retained_both_dates: true, empty_scope_source: 'live', historical_capture_unchanged: true });
  } finally {
    for (const gate of gates) await gate.release(false, true);
    await member.http.dispose(); await cyclePage.close();
  }
});
