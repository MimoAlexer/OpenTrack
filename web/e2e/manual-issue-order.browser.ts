import { createHash, randomUUID } from 'node:crypto';
import type { TiptapEditorHTMLElement } from '@tiptap/core';
import { readFileSync } from 'node:fs';
import type { APIRequestContext, Locator, Page, Request, Route } from '@playwright/test';
import { test, expect, paragraph, type SavedWorkspace } from './fixtures';
import type { Issue, IssuePage, Session } from '../src/types';
import type { TeamDetail, InvitationIssued, WorkspaceMember } from '../src/settings/types';
import type { Workflow } from '../src/workflows/types';
import type { OrderMaintenance } from '../src/issues/IssueOrdering';
import type { Attachment } from '../src/attachments/types';
import { attachmentIds, plainText, type RichBody } from '../src/documents/model';

type Reference = { team_id: string; status_id: string; job_id: string; requester_id: string; staged_count: string; total_count: null };
type ManualPage = IssuePage & { order_version: string };
type Position = { order_version: string; status_id: string; before_id?: string; after_id?: string };
function reference(saved: SavedWorkspace, browser: string): Reference {
  const path = process.env.OPENTRACK_BROWSER_ORDER_FIXTURES_FILE;
  if (!path) throw new Error('Manual ordering acceptance requires its explicit fresh fixture manifest.');
  const value = JSON.parse(readFileSync(path, 'utf8')) as { COMPLETE?: unknown; workspace_id?: unknown; maintenance?: Record<string, unknown> };
  const item = value.maintenance?.[browser] as Partial<Reference> | undefined;
  const uuid = (id: unknown) => typeof id === 'string' && /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(id);
  if (value.COMPLETE !== true || value.workspace_id !== saved.workspace || !item || ![item.team_id, item.status_id, item.job_id, item.requester_id].every(uuid) || typeof item.staged_count !== 'string' || !/^[1-9][0-9]*$/.test(item.staged_count) || BigInt(item.staged_count) >= 501n || item.total_count !== null) throw new Error('Manual ordering fixture references do not match the authenticated fresh workspace.');
  return item as Reference;
}
function url(saved: SavedWorkspace, team: string, layout = 'board', q?: string) { return `/w/${saved.workspace}/issues?${new URLSearchParams({ team_id: team, sort: 'manual', layout, display_version: '1', ...(q ? { q } : {}) })}`; }
function manual(saved: SavedWorkspace, team: string, status: string) { return saved.request<ManualPage>(`/issues?${new URLSearchParams({ team_id: team, status_id: status, sort: 'manual', limit: '100' })}`); }
async function fixture(saved: SavedWorkspace, count = 3) {
  const token = randomUUID();
  const team = await saved.request<TeamDetail>('/teams', 'POST', { name: `Manual acceptance ${token}`, key: `M${token.replaceAll('-', '').slice(0, 8).toUpperCase()}`, private: false }, { 'Idempotency-Key': randomUUID() });
  const workflow = await saved.request<Workflow>(`/teams/${team.id}/workflow`); const status = workflow.statuses.find(item => item.is_default)!;
  const target = workflow.statuses.find(item => !item.archived_at && !item.retiring_at && item.id !== status.id)!;
  expect(status).toBeDefined(); expect(target).toBeDefined(); const issues: Issue[] = [];
  for (let index = 0; index < count; index++) issues.push(await saved.request<Issue>('/issues', 'POST', { team_id: team.id, status_id: status.id, title: `${index === 1 ? 'Hidden' : token} manual ${index}`, document: { schema_version: 1, body: paragraph('Manual order acceptance') } }, { 'Idempotency-Key': randomUUID() }));
  return { team, status, target, issues, token };
}
function positionResponse(page: Page, saved: SavedWorkspace, issue: Issue) { return page.waitForResponse(response => response.request().method() === 'POST' && response.url() === `${new URL(saved.origin).origin}${saved.path}/issues/${issue.id}/position`); }
async function position(page: Page, saved: SavedWorkspace, issue: Issue, target: string, placement: 'before' | 'after' | 'append', anchor: Issue | undefined, action: () => Promise<void>) {
  const current = await saved.issue(issue.id); const before = await manual(saved, current.team_id, target); const pending = positionResponse(page, saved, issue);
  void pending.catch(() => {});
  await action(); const response = await pending; expect(response.status()).toBe(200);
  const body = response.request().postDataJSON() as Position;
  expect(body).toEqual({ order_version: before.order_version, status_id: target, ...(placement === 'before' ? { before_id: anchor!.id } : placement === 'after' ? { after_id: anchor!.id } : {}) });
  expect(body.order_version).toMatch(/^[1-9][0-9]*:[1-9][0-9]*$/); expect(response.request().headers()['if-match']).toBe(current.etag); expect(response.request().headers()['idempotency-key']).toBeTruthy();
  await expect(page.getByRole('dialog')).toHaveCount(0); return saved.issue(issue.id);
}
async function drag(page: Page, handle: Locator, destination: Locator, placement: 'before' | 'after' | 'append', held?: () => Promise<void>) {
  await handle.scrollIntoViewIfNeeded(); const from = await handle.boundingBox(); expect(from).not.toBeNull();
  await page.mouse.move(from!.x + from!.width / 2, from!.y + from!.height / 2); await page.mouse.down();
  try {
    await page.mouse.move(from!.x + from!.width / 2 + 10, from!.y + from!.height / 2, { steps: 3 });
    await destination.scrollIntoViewIfNeeded(); const to = await destination.boundingBox(); expect(to).not.toBeNull();
    await page.mouse.move(to!.x + to!.width / 2, to!.y + to!.height * (placement === 'before' ? .25 : placement === 'after' ? .75 : .5), { steps: 8 });
    await held?.();
  } finally { await page.mouse.up(); }
}
async function keyboardSelect(page: Page, control: Locator, value: string) {
  await expect(control).toBeEnabled(); const values = await control.locator('option').evaluateAll(nodes => nodes.map(node => (node as HTMLOptionElement).value));
  expect(values).toContain(value); await control.focus(); await page.keyboard.press('Home');
  for (let index = 0; index < values.indexOf(value); index++) await page.keyboard.press('ArrowDown');
  await expect(control).toHaveValue(value);
}
async function editorBody(editor: Locator) { return editor.evaluate(element => (element as TiptapEditorHTMLElement).editor!.getJSON() as RichBody); }
async function keyboardMove(page: Page, issue: Issue, status: string, placement: 'append' | 'before' | 'after', anchor?: Issue, reviewed?: () => Promise<void>) {
  const opener = page.getByRole('button', { name: `Position ${issue.key}`, exact: true }); await expect(opener).toBeEnabled(); await opener.focus(); await page.keyboard.press('Enter');
  const dialog = page.getByRole('dialog', { name: `Position ${issue.key}`, exact: true }); await expect(dialog).toBeVisible();
  await keyboardSelect(page, dialog.getByRole('combobox', { name: 'Position status', exact: true }), status);
  await keyboardSelect(page, dialog.getByRole('combobox', { name: 'Position placement', exact: true }), placement);
  if (anchor) await keyboardSelect(page, dialog.getByRole('combobox', { name: 'Position reference issue', exact: true }), anchor.id);
  const apply = dialog.getByRole('button', { name: 'Apply reviewed position', exact: true }); await expect(apply).toBeEnabled(); await reviewed?.(); await apply.focus(); await page.keyboard.press('Enter');
}
function deferred() { let resolve!: () => void; const promise = new Promise<void>(done => { resolve = done; }); return { promise, resolve }; }
async function raw<T>(http: APIRequestContext, path: string) { const response = await http.get(path); try { expect(response.status()).toBe(200); return { body: await response.json() as T, etag: response.headers().etag }; } finally { await response.dispose(); } }

test('manual list and board use one full-group reference for pointer and keyboard placement across statuses', async ({ page, saved }) => {
  const { team, status, target, issues: [a, h, c], token } = await fixture(saved);
  expect((await manual(saved, team.id, status.id)).data.map(item => item.id)).toEqual([a.id, h.id, c.id]);
  await page.goto(url(saved, team.id, 'board', token)); await expect(page.locator(`#row-${a.id}`)).toBeVisible(); await expect(page.locator(`#row-${h.id}`)).toHaveCount(0);
  const reference = page.locator(`[data-order-issue="${a.id}"]`); const handle = () => page.getByRole('button', { name: `Drag position ${c.key}`, exact: true });
  await position(page, saved, c, status.id, 'after', a, () => drag(page, handle(), reference, 'after'));
  expect((await manual(saved, team.id, status.id)).data.map(item => item.id)).toEqual([a.id, c.id, h.id]);
  await position(page, saved, c, status.id, 'before', a, () => drag(page, handle(), reference, 'before'));
  expect((await manual(saved, team.id, status.id)).data.map(item => item.id)).toEqual([c.id, a.id, h.id]);
  const empty = page.locator(`[data-order-status="${target.id}"] .issue-board-empty`);
  await position(page, saved, c, target.id, 'append', undefined, () => drag(page, handle(), empty, 'append'));
  expect((await manual(saved, team.id, status.id)).data.map(item => item.id)).toEqual([a.id, h.id]); expect((await manual(saved, team.id, target.id)).data.map(item => item.id)).toEqual([c.id]);
  await page.getByRole('combobox', { name: 'Issue layout', exact: true }).selectOption('list');
  await position(page, saved, c, status.id, 'append', undefined, () => keyboardMove(page, c, status.id, 'append'));
  expect((await manual(saved, team.id, status.id)).data.map(item => item.id)).toEqual([a.id, h.id, c.id]);
  await position(page, saved, c, status.id, 'before', a, () => keyboardMove(page, c, status.id, 'before', a));
  expect((await manual(saved, team.id, status.id)).data.map(item => item.id)).toEqual([c.id, a.id, h.id]); expect(await saved.issue(h.id)).toEqual(h);
  await page.reload(); await expect(page.locator('.issue-groups a.issue-row')).toHaveCount(2);
  expect(await page.locator('.issue-groups a.issue-row').evaluateAll(nodes => nodes.map(node => node.id))).toEqual([`row-${c.id}`, `row-${a.id}`]);
  for (const input of ['pointer', 'keyboard'] as const) for (const conflict of [412, 409]) {
    await page.goto(url(saved, team.id, input === 'pointer' ? 'board' : 'list', token)); await expect(page.locator(`#row-${c.id}`)).toBeVisible();
    const current = await saved.issue(c.id); const snapshot = await manual(saved, team.id, status.id); let authoritative: ManualPage | undefined; let canonical: Issue | undefined;
    const concurrent = async () => {
      if (conflict === 412) canonical = await saved.patch(current, { priority: current.priority === 1 ? 2 : 1 });
      else {
        const other = await saved.issue(h.id); const group = await manual(saved, team.id, status.id);
        await saved.request(`/issues/${h.id}/position`, 'POST', { order_version: group.order_version, status_id: status.id, ...(group.data[0].id === h.id ? {} : { before_id: c.id }) }, { 'If-Match': other.etag, 'Idempotency-Key': randomUUID() });
        canonical = await saved.issue(c.id); expect(canonical).toEqual(current);
      }
      authoritative = await manual(saved, team.id, status.id);
      expect(authoritative.order_version).not.toBe(snapshot.order_version);
    };
    const pending = positionResponse(page, saved, c); void pending.catch(() => {});
    if (input === 'pointer') await drag(page, handle(), reference, 'after', concurrent);
    else await keyboardMove(page, c, status.id, 'after', a, concurrent);
    const rejected = await pending; expect(rejected.status()).toBe(conflict);
    expect(rejected.request().postDataJSON()).toEqual({ order_version: snapshot.order_version, status_id: status.id, after_id: a.id });
    expect(rejected.request().headers()['if-match']).toBe(current.etag); expect(rejected.request().headers()['idempotency-key']).toBeTruthy();
    const review = page.getByRole('dialog', { name: `Position ${c.key}`, exact: true });
    await expect(review).toContainText('The requested move was not applied.'); await expect(review.getByRole('button', { name: 'Apply reviewed position', exact: true })).toBeEnabled();
    await expect(review.getByRole('combobox', { name: 'Position status', exact: true })).toHaveValue(status.id);
    await expect(review.getByRole('combobox', { name: 'Position placement', exact: true })).toHaveValue('after');
    await expect(review.getByRole('combobox', { name: 'Position reference issue', exact: true })).toHaveValue(a.id);
    expect(await manual(saved, team.id, status.id)).toEqual(authoritative); expect(await saved.issue(c.id)).toEqual(canonical);
    const rows = input === 'pointer' ? page.locator(`[data-order-status="${status.id}"] [data-order-issue] a.issue-row`) : page.locator('.issue-groups a.issue-row');
    await expect.poll(() => rows.evaluateAll(nodes => nodes.map(node => node.id))).toEqual(authoritative!.data.filter(issue => issue.id !== h.id).map(issue => `row-${issue.id}`));
    await review.getByRole('button', { name: 'Discard local move', exact: true }).click(); await expect(review).toHaveCount(0);
    expect(await manual(saved, team.id, status.id)).toEqual(authoritative);
  }
  await page.goto(`/w/${saved.workspace}/issues/${c.id}?${new URLSearchParams({ team_id: team.id, sort: 'manual' })}`);
  await expect(page.getByRole('region', { name: 'Activity', exact: true })).toContainText('changed position.');
});

test('an unknown position survives authority refresh with its exact request and conceals an actually denied private source', async ({ page, saved, playwright, browserName }) => {
  const ref = reference(saved, browserName); const { team, status, issues } = await fixture(saved, 2); const source = issues[1], anchor = issues[0];
  const endpoint = `${new URL(saved.origin).origin}${saved.path}/issues/${source.id}/position`; const requests: Request[] = []; let acknowledged: unknown;
  const lost = async (route: Route) => {
    if (route.request().method() !== 'POST') { await route.continue(); return; }
    requests.push(route.request()); const response = await route.fetch({ maxRetries: 0, maxRedirects: 0 });
    try {
      expect(response.status()).toBe(200); const value = await response.json() as { issue: Issue; order_version: string };
      if (requests.length === 1) { acknowledged = value; await route.abort('failed'); }
      else {
        expect(value).toEqual(acknowledged);
        if (requests.length === 2) await route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify({ ...value, issue: { ...value.issue, etag: '' } }) });
        else if (requests.length === 3) await route.fulfill({ status: 409, contentType: 'application/problem+json', body: JSON.stringify({ code: 'conflict', detail: 'The retained request result cannot currently be replayed.' }) });
        else await route.fulfill({ response });
      }
    } finally { await response.dispose(); }
  };
  await page.route(endpoint, lost);
  try {
    await page.goto(url(saved, team.id, 'list')); await keyboardMove(page, source, status.id, 'before', anchor);
    const dialog = page.getByRole('dialog', { name: `Position ${source.key}`, exact: true }); await expect(dialog.getByRole('button', { name: 'Check same position request', exact: true })).toBeEnabled();
    const committed = await saved.issue(source.id); const canonical = await saved.patch(committed, { title: `Canonical after lost position ${randomUUID()}` });
    const previous = await saved.request<{ auth_epoch: string }>('/access'); const member = await raw<WorkspaceMember>(saved.http, `${saved.path}/members/${ref.requester_id}`);
    expect(member.body.user.id).not.toBe(saved.session.user.id); expect(member.body.role).toBe('member'); expect(member.etag).toBeTruthy();
    const bootstrap = page.waitForResponse(response => response.request().method() === 'GET' && response.url() === `${new URL(saved.origin).origin}${saved.path}/bootstrap`);
    await saved.request(`/members/${ref.requester_id}`, 'PATCH', { role: 'viewer' }, { 'If-Match': member.etag! });
    await page.evaluate(() => window.dispatchEvent(new Event('focus'))); const refreshed = await (await bootstrap).json() as { auth_epoch: string }; expect(refreshed.auth_epoch).not.toBe(previous.auth_epoch);
    await expect(dialog).toContainText(canonical.title);
    for (let rejectedReplay = 0; rejectedReplay < 2; rejectedReplay++) {
      await dialog.getByRole('button', { name: 'Check same position request', exact: true }).click();
      await expect(dialog).toContainText('The outcome is uncertain.'); await expect(dialog.getByRole('button', { name: 'Check same position request', exact: true })).toBeEnabled();
      await expect(dialog.getByRole('button', { name: 'Discard local move', exact: true })).toBeDisabled(); await expect(dialog.getByRole('button', { name: 'Apply reviewed position', exact: true })).toHaveCount(0);
      expect(await saved.issue(source.id)).toEqual(canonical);
    }
    await dialog.getByRole('button', { name: 'Check same position request', exact: true }).click(); await expect(dialog).toHaveCount(0);
    expect(requests).toHaveLength(4);
    for (const replay of requests.slice(1)) { expect(replay.postData()).toBe(requests[0].postData()); for (const header of ['if-match', 'idempotency-key']) expect(replay.headers()[header]).toBe(requests[0].headers()[header]); }
    test.info().annotations.push({ type: 'response-injection', description: 'Real committed/cached position responses: first delivery aborted; second acknowledgement has invalid empty ETag; third is injected generic409 representing unavailable/expired successful replay. Fourth delivers the actual cached success. No backend record or expiry was changed.' });
    expect(await saved.issue(source.id)).toEqual(canonical); await expect(page.locator(`#row-${source.id}`)).toContainText(canonical.title); await page.reload(); await expect(page.locator(`#row-${source.id}`)).toContainText(canonical.title);
  } finally { await page.unroute(endpoint, lost); }

  const issued = await saved.request<InvitationIssued>('/invitations', 'POST', { email: `manual-admin-${randomUUID()}@example.test`, role: 'admin', team_ids: [] }, { 'Idempotency-Key': randomUUID() });
  const admin = await playwright.request.newContext({ baseURL: saved.origin, extraHTTPHeaders: { Origin: saved.origin }, storageState: { cookies: [], origins: [] } });
  const release = deferred(); let holding = false; let closing = false; let positionDenied = false;
  let privateEndpoint = ''; let accessEndpoint = ''; let workflowEndpoint = ''; let sessionEndpoint = '';
  const held = async (route: Route) => { if (!holding || route.request().method() !== 'GET' || route.request().url() === workflowEndpoint && !route.request().headers()['if-none-match']) { await route.continue(); return; } await release.promise; if (closing) await route.abort('failed').catch(() => {}); else await route.continue().catch(() => {}); };
  const sessionRoute = async (route: Route) => { expect(positionDenied).toBe(true); await route.continue(); };
  const observe = (response: import('@playwright/test').Response) => { if (response.url() === privateEndpoint && response.request().method() === 'POST' && response.status() === 404) positionDenied = true; };
  let privateRoute: ((route: Route) => Promise<void>) | undefined;
  try {
    const accepted = await admin.post('/api/v1/invitations/accept', { data: { token: issued.token, display_name: 'Example Administrator', password: 'Synthetic administrator acceptance passphrase 2026!' } }); expect(accepted.status()).toBe(200); const account = await accepted.json() as Session; await accepted.dispose();
    const adminRequest = async <T,>(path: string, method = 'GET', body?: unknown, headers: Record<string, string> = {}) => { const response = await admin.fetch(`${saved.path}${path}`, { method, data: body, headers: { Origin: saved.origin, ...(method === 'GET' ? {} : { 'X-CSRF-Token': account.csrf_token }), ...headers } }); try { expect(response.ok()).toBe(true); return await response.json() as T; } finally { await response.dispose(); } };
    const token = randomUUID(); const publicTeam = await adminRequest<TeamDetail>('/teams', 'POST', { name: `Confidential order team ${token}`, key: `P${token.replaceAll('-', '').slice(0, 8).toUpperCase()}`, private: false }, { 'Idempotency-Key': randomUUID() });
    const members = await adminRequest<{ data: { user: { id: string } }[] }>(`/teams/${publicTeam.id}/members?limit=100`); expect(members.data.some(item => item.user.id === saved.session.user.id)).toBe(false);
    let flow = await adminRequest<Workflow>(`/teams/${publicTeam.id}/workflow`); const initial = flow.statuses.find(item => item.is_default)!;
    const privateStage = `Confidential position stage ${token}`;
    flow = await adminRequest<Workflow>(`/teams/${publicTeam.id}/statuses/${initial.id}`, 'PATCH', { name: privateStage }, { 'If-Match': `"workflow:${publicTeam.id}:${flow.version}:true"` });
    const deniedIssue = await adminRequest<Issue>('/issues', 'POST', { team_id: publicTeam.id, status_id: initial.id, title: `Confidential position issue ${token}` }, { 'Idempotency-Key': randomUUID() });
    await page.goto(url(saved, publicTeam.id, 'board')); await page.getByRole('button', { name: `Position ${deniedIssue.key}`, exact: true }).click();
    const review = page.getByRole('dialog', { name: `Position ${deniedIssue.key}`, exact: true }); await expect(review.getByRole('button', { name: 'Apply reviewed position', exact: true })).toBeEnabled();
    privateEndpoint = `${new URL(saved.origin).origin}${saved.path}/issues/${deniedIssue.id}/position`; accessEndpoint = `${new URL(saved.origin).origin}${saved.path}/access`; workflowEndpoint = `${new URL(saved.origin).origin}${saved.path}/teams/${publicTeam.id}/workflow`; sessionEndpoint = `${new URL(saved.origin).origin}/api/v1/auth/session`;
    await page.route(accessEndpoint, held); await page.route(workflowEndpoint, held); await page.route(sessionEndpoint, sessionRoute); page.on('response', observe);
    holding = true; const accessHeld = page.waitForRequest(request => request.url() === accessEndpoint); const workflowHeld = page.waitForRequest(request => request.url() === workflowEndpoint && !!request.headers()['if-none-match']);
    await page.evaluate(() => window.dispatchEvent(new Event('focus'))); await Promise.all([accessHeld, workflowHeld]);
    privateRoute = async route => { const current = await adminRequest<TeamDetail>(`/teams/${publicTeam.id}`); await adminRequest(`/teams/${publicTeam.id}`, 'PATCH', { private: true }, { 'If-Match': `"team:${publicTeam.id}:${current.version}"` }); await route.continue(); };
    await page.route(privateEndpoint, privateRoute); const denied = page.waitForResponse(response => response.url() === privateEndpoint && response.request().method() === 'POST'); const teamDenied = page.waitForResponse(response => response.url() === `${new URL(saved.origin).origin}${saved.path}/teams/${publicTeam.id}` && response.status() === 404);
    await review.getByRole('button', { name: 'Apply reviewed position', exact: true }).click(); expect((await denied).status()).toBe(404); await teamDenied; expect(positionDenied).toBe(true);
    holding = false; release.resolve();
    for (const label of [publicTeam.name, privateStage, deniedIssue.title, deniedIssue.key]) await expect(page.locator('body')).not.toContainText(label);
    await expect(page.locator(`[data-order-issue="${deniedIssue.id}"], option[value="${publicTeam.id}"]`)).toHaveCount(0);
    expect(new URL(page.url()).searchParams.get('team_id')).toBe(publicTeam.id); expect(new URL(page.url()).searchParams.get('sort')).toBe('manual');
    expect(await adminRequest<Issue>(`/issues/${deniedIssue.id}`)).toEqual(deniedIssue);
  } finally {
    closing = true; holding = false; release.resolve(); page.off('response', observe);
    if (privateEndpoint && privateRoute) await page.unroute(privateEndpoint, privateRoute); if (accessEndpoint) await page.unroute(accessEndpoint, held); if (workflowEndpoint) await page.unroute(workflowEndpoint, held); if (sessionEndpoint) await page.unroute(sessionEndpoint, sessionRoute); await admin.dispose();
  }
});

test('typed large-group maintenance preserves an editable create draft and attachment through reviewed manager resume', async ({ page, saved, browserName }) => {
  const ref = reference(saved, browserName); const path = `/teams/${ref.team_id}/issue-order-maintenance/${ref.job_id}`;
  const initial = await saved.request<OrderMaintenance>(path); expect(initial).toMatchObject({ id: ref.job_id, team_id: ref.team_id, status_id: ref.status_id, state: 'blocked', phase: 'staging', staged_count: ref.staged_count, applied_count: '0', total_count: null, can_resume: true, error_code: 'access_lost' });
  const title = `Maintenance draft ${randomUUID()}`; const edited = `${title} reviewed`; const filename = 'manual-order-evidence.txt'; const bytes = Buffer.from('Benign manual ordering attachment acceptance.\n');
  const creates: Request[] = [], positions: Request[] = [], jobReads: { request: Request; value: Promise<OrderMaintenance> }[] = []; const sequence = new Map<Request, number>(); let next = 0;
  const origin = new URL(saved.origin).origin; const createEndpoint = `${origin}${saved.path}/issues`; const jobEndpoint = `${origin}${saved.path}${path}`;
  const observeRequest = (request: Request) => { sequence.set(request, next++); if (request.method() === 'POST' && request.url() === createEndpoint) creates.push(request); if (request.method() === 'POST' && request.url().endsWith('/position')) positions.push(request); };
  const observeResponse = (response: import('@playwright/test').Response) => { if (response.url() === jobEndpoint && response.request().method() === 'GET' && response.status() === 200) jobReads.push({ request: response.request(), value: response.json() as Promise<OrderMaintenance> }); };
  page.on('request', observeRequest); page.on('response', observeResponse);
  try {
    await saved.open(page); await page.getByRole('button', { name: /^New issue/ }).click(); const dialog = page.getByRole('dialog', { name: 'New issue', exact: true });
    await dialog.getByRole('combobox', { name: 'Team', exact: true }).selectOption(ref.team_id); await dialog.getByRole('combobox', { name: /^Status/ }).selectOption(ref.status_id);
    await dialog.getByRole('textbox', { name: 'Title', exact: true }).fill(title); const description = dialog.getByRole('textbox', { name: 'Description', exact: true }); await description.fill('Ready draft and attachment survive order preparation.');
    const chooser = page.waitForEvent('filechooser'); await dialog.getByRole('button', { name: 'Attach files', exact: true }).click(); const uploaded = page.waitForResponse(response => response.request().method() === 'PUT' && response.url().startsWith(`${origin}${saved.path}/attachments/`) && response.url().endsWith('/content'));
    await (await chooser).setFiles({ name: filename, mimeType: 'text/plain', buffer: bytes }); const attachment = await (await uploaded).json() as Attachment; expect(attachment.state).toBe('ready'); await expect(dialog.getByRole('button', { name: `Download ${filename}`, exact: true })).toBeEnabled();
    const readyBody = await editorBody(description); expect(attachmentIds(readyBody)).toEqual([attachment.id]); expect(plainText(readyBody)).toContain('Ready draft and attachment survive order preparation.');
    const create = async (name: string) => { const response = page.waitForResponse(value => value.url() === createEndpoint && value.request().method() === 'POST'); await dialog.getByRole('button', { name, exact: true }).click(); return response; };
    const first = await create('Create issue'); expect(first.status()).toBe(409); expect(await first.json()).toMatchObject({ code: 'issue_order_maintenance', details: { applied: false, maintenance: [expect.objectContaining({ id: ref.job_id })] } });
    await expect(dialog).toContainText('No issue was created by this request'); await expect(dialog).toContainText('total not known yet'); await expect(dialog.getByRole('textbox', { name: 'Title', exact: true })).toBeEditable(); await expect(description).toBeEditable(); await expect(dialog.getByRole('button', { name: 'Cancel', exact: true })).toBeEnabled();
    await dialog.getByRole('button', { name: 'Cancel', exact: true }).click(); const discard = page.getByRole('dialog', { name: 'Discard this issue draft?', exact: true }); await discard.getByRole('button', { name: 'Keep editing', exact: true }).click(); await expect(dialog.getByRole('textbox', { name: 'Title', exact: true })).toHaveValue(title); await expect(dialog.getByRole('button', { name: `Download ${filename}`, exact: true })).toBeEnabled();
    await dialog.getByRole('textbox', { name: 'Title', exact: true }).fill(edited); const second = await create('Create issue'); expect(second.status()).toBe(409); expect(await second.json()).toMatchObject({ code: 'issue_order_maintenance', details: { applied: false, maintenance: [expect.objectContaining({ id: ref.job_id })] } });
    expect(creates).toHaveLength(2); expect(creates[1].headers()['idempotency-key']).not.toBe(creates[0].headers()['idempotency-key']);
    const secondBody = creates[1].postDataJSON() as { title: string; document: { body: RichBody } }; expect(secondBody.title).toBe(edited); expect(attachmentIds(secondBody.document.body)).toEqual([attachment.id]);
    const progress = dialog.getByRole('region', { name: 'Issue order maintenance', exact: true }); const mark = next; const resumed = page.waitForResponse(response => response.url() === `${jobEndpoint}/resume` && response.request().method() === 'POST');
    await progress.getByRole('button', { name: 'Resume order preparation', exact: true }).click(); const response = await resumed; expect(response.status()).toBe(200); expect(response.request().postData()).toBeNull();
    const freshReads = jobReads.filter(item => (sequence.get(item.request) ?? -1) >= mark && (sequence.get(item.request) ?? Infinity) < sequence.get(response.request())!); expect(freshReads.length).toBeGreaterThan(0); const fresh = await freshReads.at(-1)!.value;
    expect(fresh.can_resume).toBe(true); expect(response.request().headers()['if-match']).toBe(fresh.etag);
    let current = await saved.request<OrderMaintenance>(path); const checkpoints: OrderMaintenance[] = [initial, current];
    for (let batch = 0; batch < 12 && current.phase !== 'completed'; batch++) {
      const previous = `${current.phase}:${current.staged_count}:${current.applied_count}`;
      await expect.poll(async () => { current = await saved.request<OrderMaintenance>(path); return `${current.phase}:${current.staged_count}:${current.applied_count}`; }).not.toBe(previous);
      expect(current.id).toBe(ref.job_id); expect(BigInt(current.staged_count)).toBeGreaterThanOrEqual(BigInt(checkpoints.at(-1)!.staged_count)); expect(BigInt(current.applied_count)).toBeGreaterThanOrEqual(BigInt(checkpoints.at(-1)!.applied_count)); checkpoints.push(current);
    }
    expect(current).toMatchObject({ id: ref.job_id, state: 'succeeded', phase: 'completed', staged_count: '501', applied_count: '501', total_count: '501' }); await expect(progress).toContainText('Order preparation complete.'); expect(creates).toHaveLength(2); expect(positions).toHaveLength(0);
    expect((await saved.request<IssuePage>(`/issues?${new URLSearchParams({ team_id: ref.team_id, q: title, limit: '100' })}`)).data).toHaveLength(0);
    let committed: Issue | undefined;
    const loseCreate = async (route: Route) => { const value = await route.fetch({ maxRetries: 0, maxRedirects: 0 }); try { expect(value.status()).toBe(201); committed = await value.json() as Issue; await route.abort('failed'); } finally { await value.dispose(); } };
    await page.route(createEndpoint, loseCreate);
    try {
      await dialog.getByRole('button', { name: 'Retry create', exact: true }).click();
      await expect(dialog).toContainText('this issue may already exist'); await expect(dialog).not.toContainText('No issue was created by this request');
      await expect(dialog.getByRole('textbox', { name: 'Title', exact: true })).toBeDisabled(); await expect(description).not.toBeEditable(); await expect(description).toHaveAttribute('aria-readonly', 'true'); await expect(description).toHaveAttribute('contenteditable', 'false');
      expect(await description.evaluate(element => (element as HTMLElement).isContentEditable)).toBe(false);
      const lockedBody = await editorBody(description); expect(lockedBody).toEqual(readyBody);
      await description.locator('p').first().click(); await page.keyboard.type('BlockedUncertainEdit');
      expect(await editorBody(description)).toEqual(lockedBody);
      await expect(dialog.getByRole('button', { name: 'Cancel', exact: true })).toBeDisabled();
      await expect(dialog.getByRole('button', { name: 'Retry safely', exact: true })).toBeEnabled(); expect(creates).toHaveLength(3);
    } finally { await page.unroute(createEndpoint, loseCreate); }
    const last = await create('Retry safely'); expect(last.status()).toBe(201); const issue = await last.json() as Issue; expect(issue).toEqual(committed); expect(creates).toHaveLength(4);
    for (const replay of creates.slice(2)) { expect(replay.postData()).toBe(creates[1].postData()); expect(replay.headers()['idempotency-key']).toBe(creates[1].headers()['idempotency-key']); }
    test.info().annotations.push({ type: 'response-injection', description: 'Actual201 create response is delivered as a network abort; safe retry recovers the real cached201. No issue or idempotency record is fabricated.' });
    await expect(dialog).toHaveCount(0); await page.reload(); await expect(page.getByRole('textbox', { name: 'Description', exact: true })).toContainText('Ready draft and attachment survive order preparation.');
    const document = await saved.document(issue.id); expect(attachmentIds(document.body as RichBody)).toEqual([attachment.id]); expect(plainText(document.body as RichBody)).toContain('Ready draft and attachment'); expect((await saved.issue(issue.id)).title).toBe(edited);
    expect((await saved.request<IssuePage>(`/issues?${new URLSearchParams({ team_id: ref.team_id, q: title, limit: '100' })}`)).data.map(item => item.id)).toEqual([issue.id]);
    const downloaded = await saved.http.get(`${saved.path}/attachments/${attachment.id}/content`); try { expect(downloaded.status()).toBe(200); const received = await downloaded.body(); expect(received).toEqual(bytes); expect(createHash('sha256').update(received).digest('hex')).toBe(attachment.sha256); } finally { await downloaded.dispose(); }
    expect((await saved.request<OrderMaintenance>(path)).id).toBe(ref.job_id); test.info().annotations.push({ type: 'manual-order-checkpoints', description: JSON.stringify(checkpoints.map(item => ({ id: item.id, phase: item.phase, staged_count: item.staged_count, applied_count: item.applied_count, total_count: item.total_count }))) });
  } finally { page.off('request', observeRequest); page.off('response', observeResponse); await Promise.allSettled(jobReads.map(item => item.value)); }
});
