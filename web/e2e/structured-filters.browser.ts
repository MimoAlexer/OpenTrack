import { randomUUID } from 'node:crypto';
import type { Page, Request as BrowserRequest } from '@playwright/test';
import { test, expect, paragraph, SavedWorkspace } from './fixtures';
import type { Bootstrap, Issue } from '../src/types';
import type { TeamDetail } from '../src/settings/types';
import type { IssueComment } from '../src/documents/types';
import type { components } from '../src/generated/api';
import { parseStructuredHash, viewHash, type FilterNode, type StructuredView } from '../src/filters/model';

async function fixture(owner: SavedWorkspace) {
  const teamInput = {
    name: `Filter acceptance ${randomUUID()}`, key: `Q${randomUUID().replaceAll('-', '').slice(0, 8).toUpperCase()}`,
    private: false,
  } satisfies components['schemas']['CreateTeam'];
  const team = await owner.request<TeamDetail>('/teams', 'POST', teamInput, { 'Idempotency-Key': randomUUID() });
  const bootstrap = await owner.request<Bootstrap>('/bootstrap');
  const saved = new SavedWorkspace(owner.http, owner.session, { ...bootstrap, teams: [team] }, owner.origin, owner.workspace);
  const issues: Issue[] = [];
  for (const [index, priority, due] of [[1, 1, '2026-03-28'], [2, 3, '2026-03-30'], [3, 1, null]] as const) {
    issues.push(await saved.request<Issue>('/issues', 'POST', { team_id: team.id, title: `Filter work ${index} ${randomUUID()}`, priority, due_date: due }, { 'Idempotency-Key': randomUUID() }));
  }
  return { saved, team, issues };
}
const path = (saved: SavedWorkspace) => `/w/${saved.workspace}/issues`;
const queryUrl = (saved: SavedWorkspace) => `${saved.origin}${saved.path}/issues/query`;
const postedQuery = (page: Page, saved: SavedWorkspace) => page.waitForResponse(response => response.request().method() === 'POST' && response.url() === queryUrl(saved));
function parsedHash(url: string): StructuredView { const parsed = parseStructuredHash(new URL(url).hash); if (parsed.kind !== 'valid') throw new Error('Expected a preserved structured query'); return parsed.view; }

test('structured filters preserve saved results, local-day intent, detail navigation and list/board state', async ({ page, saved: owner }) => {
  const { saved, team, issues } = await fixture(owner);
  const comment = await saved.request<IssueComment>(`/issues/${issues[0].id}/comments`, 'POST', { schema_version: 1, body: paragraph('Saved structured filter context') }, { 'Idempotency-Key': randomUUID() });
  await page.goto(`${path(saved)}?${new URLSearchParams({ team_id: team.id, priority: '1' })}`);
  await page.locator('summary').filter({ hasText: 'Advanced filters' }).click();
  const builder = page.getByRole('form', { name: 'Advanced issue filters', exact: true });
  await builder.getByRole('button', { name: 'Add condition', exact: true }).click();
  await builder.getByRole('combobox', { name: 'Condition 1.2 field', exact: true }).selectOption('due_date');
  await builder.getByRole('combobox', { name: 'Condition 1.2 operator', exact: true }).selectOption('before');
  await builder.getByRole('combobox', { name: 'Condition 1.2 date 1 meaning', exact: true }).selectOption('date');
  await builder.getByRole('textbox', { name: 'Query timezone', exact: true }).fill('Europe/Zurich');
  await builder.locator('input[aria-label="Condition 1.2 date 1 value"]').fill('2026-03-29');
  const posted = postedQuery(page, saved);
  await builder.getByRole('button', { name: 'Apply filters', exact: true }).click();
  const response = await posted; expect(response.status()).toBe(200);
  const body = response.request().postDataJSON();
  expect(body.filter).toEqual({ kind: 'all', children: [
    { kind: 'predicate', field: 'priority', op: 'eq', value: 1 },
    { kind: 'predicate', field: 'due_date', op: 'before', value: '2026-03-29' },
  ] });
  expect(body).toMatchObject({ version: 1, timezone: 'Europe/Zurich', team_id: team.id, limit: 50, sort: 'created', archived: false });
  expect(body).not.toHaveProperty('layout');
  expect(response.request().headers()['idempotency-key']).toBeUndefined();
  await expect(page.locator('.issue-list .issue-row')).toHaveCount(1);
  await expect(page.locator(`#row-${issues[0].id}`)).toBeVisible();
  const original = parsedHash(page.url());
  const reload = postedQuery(page, saved); await page.reload(); expect((await reload).status()).toBe(200);
  expect(parsedHash(page.url())).toEqual(original);
  await page.locator(`#row-${issues[0].id}`).click();
  await expect(page.getByRole('textbox', { name: 'Issue title', exact: true })).toHaveValue(issues[0].title);
  expect(parsedHash(page.url())).toEqual(original);
  await page.locator(`#comment-${comment.id} .comment-time`).click();
  expect(parsedHash(page.url())).toEqual(original);
  expect(new URL(page.url()).hash).toContain(`&comment=${comment.id}`);
  await page.reload();
  await expect(page.locator(`#comment-${comment.id}`)).toBeFocused();
  await page.getByRole('button', { name: 'Close issue', exact: true }).click();
  const sorted = postedQuery(page, saved);
  await page.getByRole('combobox', { name: 'Issue order', exact: true }).selectOption('priority');
  expect((await sorted).request().postDataJSON().filter).toEqual(body.filter);
  await page.goBack(); await expect(page.getByRole('combobox', { name: 'Issue order', exact: true })).toHaveValue('created');
  expect(parsedHash(page.url())).toEqual(original);
  await page.goForward(); await expect(page.getByRole('combobox', { name: 'Issue order', exact: true })).toHaveValue('priority');
  const board = postedQuery(page, saved);
  await page.getByRole('combobox', { name: 'Issue layout', exact: true }).selectOption('board');
  const boardResponse = await board; expect(boardResponse.status()).toBe(200);
  expect(boardResponse.request().postDataJSON().filter).toEqual(body.filter);
  await expect(page.getByRole('region', { name: 'Issue board', exact: true })).toBeVisible();
  await expect(page.locator('.issue-list .issue-row')).toHaveCount(1);
  expect(await saved.issue(issues[0].id)).toEqual(issues[0]);
});

test('invalid structured state sends no issue listing query until explicitly cleared', async ({ page, saved }) => {
  const requests: BrowserRequest[] = [];
  page.on('request', request => { const url = new URL(request.url()); if (url.pathname === `${saved.path}/issues` || url.pathname === `${saved.path}/issues/query`) requests.push(request); });
  await page.goto(`${path(saved)}#filters=${encodeURIComponent(JSON.stringify({ version: 99, layout: 'list', filter: null, sort: 'created', timezone: 'UTC', limit: 50, archived: false }))}`);
  await expect(page.getByRole('heading', { name: 'These filters are unavailable.', exact: true })).toBeVisible();
  await expect(page.locator('.issue-list .issue-row')).toHaveCount(0);
  expect(requests).toEqual([]);
  const cleared = page.waitForResponse(response => response.request().method() === 'GET' && new URL(response.url()).pathname === `${saved.path}/issues`);
  await page.getByRole('button', { name: 'Clear filter settings', exact: true }).click();
  expect((await cleared).status()).toBe(200);
  expect(new URL(page.url()).hash).toBe('');
  expect(requests.some(request => request.method() === 'POST')).toBe(false);
});

test('hash-only filter navigation retains a dirty issue draft when its confirmation is declined', async ({ page, saved: owner }) => {
  const { saved, team, issues } = await fixture(owner);
  const view: StructuredView = { version: 1, layout: 'list', filter: { kind: 'predicate', field: 'priority', op: 'eq', value: 1 }, team_id: team.id, timezone: 'UTC', sort: 'created', limit: 50, archived: false };
  await page.goto(`${path(saved)}${viewHash(view)}`);
  await page.locator(`#row-${issues[0].id}`).click();
  const title = page.getByRole('textbox', { name: 'Issue title', exact: true });
  const draft = `Retained filter draft ${randomUUID()}`; await title.fill(draft);
  const original = new URL(page.url()).hash; let dialogs = 0;
  page.once('dialog', async dialog => { dialogs++; expect(dialog.type()).toBe('confirm'); await dialog.dismiss(); });
  await page.evaluate(hash => { location.hash = hash; }, viewHash({ ...view, filter: { kind: 'predicate', field: 'priority', op: 'eq', value: 4 } }));
  await expect.poll(() => dialogs).toBe(1);
  await expect.poll(() => new URL(page.url()).hash).toBe(original);
  await expect(title).toHaveValue(draft);
  expect((await saved.issue(issues[0].id)).title).toBe(issues[0].title);
  const savedResponse = page.waitForResponse(response => response.request().method() === 'PATCH' && response.url() === `${saved.origin}${saved.path}/issues/${issues[0].id}`);
  await page.getByRole('button', { name: 'Save changes', exact: true }).click();
  expect((await savedResponse).status()).toBe(200);
  await expect.poll(async () => (await saved.issue(issues[0].id)).title).toBe(draft);
  expect(new URL(page.url()).hash).toBe(original);
});

test('the complete maximum filter tree reaches the real body-based query without URL or value truncation', async ({ page, saved: owner }) => {
  const { saved, team, issues } = await fixture(owner);
  const leaf: FilterNode = { kind: 'predicate', field: 'status', op: 'in', values: Array.from({ length: 100 }, () => issues[0].status_id) };
  const filter: FilterNode = { kind: 'all', children: Array.from({ length: 50 }, () => ({ kind: 'all', children: [{ kind: 'any', children: [structuredClone(leaf)] }] })) };
  const view: StructuredView = { version: 1, layout: 'list', filter, team_id: team.id, timezone: 'UTC', sort: 'created', limit: 50, archived: false };
  const response = postedQuery(page, saved);
  await page.goto(`${path(saved)}${viewHash(view)}`);
  const actual = await response; expect(actual.status()).toBe(200);
  expect(actual.request().postDataJSON().filter).toEqual(filter);
  expect(Buffer.byteLength(actual.request().postData()!, 'utf8')).toBeGreaterThan(128 * 1024);
  expect(new URL(actual.request().url()).search).toBe('');
  expect(new URL(actual.request().url()).hash).toBe('');
  await expect(page.locator(`#row-${issues[0].id}`)).toBeVisible();
  expect(parsedHash(page.url())).toEqual(view);
});
