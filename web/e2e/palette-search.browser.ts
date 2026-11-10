import { randomUUID } from 'node:crypto';
import type { Locator, Response } from '@playwright/test';
import { test, expect, paragraph, pollingFallbackForHeldHttp, registerAdmission } from './fixtures';
import type { components } from '../src/generated/api';
import type { StructuredDocument } from '../src/documents/types';
import type { FullProject } from '../src/projects/types';
import type { TeamDetail } from '../src/settings/types';
import type { Issue, Session } from '../src/types';

type SearchPage = components['schemas']['SearchPage'];
registerAdmission(1);

async function invokeByKeyboard(input: Locator, option: Locator) {
  for (let step = 0; step < 20 && await option.getAttribute('aria-selected') !== 'true'; step += 1) await input.press('ArrowDown');
  await expect(option).toHaveAttribute('aria-selected', 'true'); await input.press('Enter');
}

test('palette finds saved issue bodies and current project briefs across teams with literal safe results', async ({ page, saved }) => {
  const term = `palette${randomUUID().replaceAll('-', '')}`;
  const literal = '<img data-palette-injected src=x onerror=alert(1)>';
  const body = paragraph(`${term} ${literal} ${'x'.repeat(260)}`);
  const issue = await saved.create(`Literal <b data-palette-injected>issue</b> ${randomUUID()}`, body);
  const team = await saved.request<TeamDetail>('/teams', 'POST', {
    name: `Other accessible team ${randomUUID()}`, key: `B${randomUUID().replaceAll('-', '').slice(0, 8).toUpperCase()}`, private: false,
  }, { 'Idempotency-Key': randomUUID() });
  expect(team.id).not.toBe(saved.team.id);
  const projectPath = `/teams/${team.id}/projects`;
  const project = await saved.request<FullProject>(projectPath, 'POST', {
    name: `Literal <b data-palette-injected>project</b> ${randomUUID()}`,
    narrative: { schema_version: 1, body: paragraph('Earlier brief without the search term') },
  }, { 'Idempotency-Key': randomUUID() });
  const original = await saved.request<StructuredDocument>(`${projectPath}/${project.id}/document`);
  const current = await saved.request<StructuredDocument>(`/documents/${original.id}`, 'PATCH', {
    schema_version: 1, body,
  }, { 'If-Match': original.etag });
  expect(current.version).not.toBe(original.version);
  expect(issue.title).not.toContain(term); expect(project.name).not.toContain(term);

  await saved.open(page);
  await page.setViewportSize({ width: 320, height: 720 });
  expect(new URL(page.url()).searchParams.has('team_id')).toBe(false);
  const queries: URL[] = []; const reads: string[] = []; const dialogs: string[] = [];
  page.on('request', request => {
    if (request.method() !== 'GET') return;
    const url = new URL(request.url());
    if (url.pathname === `${saved.path}/search`) queries.push(url);
    if (url.pathname === `${saved.path}/issues/${issue.id}` || url.pathname === `${saved.path}${projectPath}/${project.id}/metadata`) reads.push(url.pathname);
  });
  page.on('dialog', dialog => { dialogs.push(dialog.type()); void dialog.dismiss(); });
  await page.keyboard.press('Control+k');
  const palette = page.getByRole('dialog', { name: 'Commands', exact: true });
  const input = palette.getByRole('combobox', { name: 'Search commands', exact: true });
  await expect(input).toBeFocused();
  const search = page.waitForResponse(response => response.request().method() === 'GET' && new URL(response.url()).pathname === `${saved.path}/search` && new URL(response.url()).searchParams.get('q') === term);
  await input.fill(term);
  const results = await search; expect(results.status()).toBe(200);
  const hits = await results.json() as SearchPage;
  expect(hits.data.map(hit => hit.id).sort()).toEqual([issue.id, project.id].sort());
  const issueOption = palette.getByRole('option').filter({ hasText: issue.title });
  const projectOption = palette.getByRole('option').filter({ hasText: project.name });
  await expect(issueOption).toBeVisible(); await expect(projectOption).toBeVisible();
  await expect(palette.getByRole('option').locator('strong')).toHaveText(hits.data.map(hit => hit.key ? `${hit.key} · ${hit.title}` : hit.title));
  await expect(issueOption).toContainText(literal); await expect(projectOption).toContainText(literal);
  expect(hits.fuzzy_degraded).toBe(true); expect(hits.notice).toBeTruthy(); await expect(palette).toContainText(hits.notice!);
  await expect(palette.locator('[data-palette-injected]')).toHaveCount(0);
  expect(await palette.evaluate(element => element.scrollWidth - element.clientWidth), 'Long literal snippets fit the narrow palette').toBeLessThanOrEqual(1);
  expect(await page.evaluate(() => document.documentElement.scrollWidth - innerWidth), 'The narrow palette fits the viewport').toBeLessThanOrEqual(1);
  await expect.poll(() => queries.length).toBe(1);
  expect(queries[0].searchParams.get('q')).toBe(term); expect(queries[0].searchParams.get('limit')).toBe('20');
  expect(queries[0].searchParams.has('team_id')).toBe(false); expect(queries[0].searchParams.has('kind')).toBe(false);
  expect(reads).toEqual([]); expect(dialogs).toEqual([]);
  const issueRead = page.waitForResponse(response => response.request().method() === 'GET' && new URL(response.url()).pathname === `${saved.path}/issues/${issue.id}`);
  await invokeByKeyboard(input, issueOption);
  expect((await issueRead).status()).toBe(200);
  await expect(palette).toHaveCount(0);
  await expect(page.getByRole('textbox', { name: 'Issue title', exact: true })).toHaveValue(issue.title);
  await page.keyboard.press('Control+k');
  await input.fill(term); await expect(projectOption).toBeVisible();
  const projectRead = page.waitForResponse(response => response.request().method() === 'GET' && new URL(response.url()).pathname === `${saved.path}${projectPath}/${project.id}/metadata`);
  await invokeByKeyboard(input, projectOption);
  expect((await projectRead).status()).toBe(200);
  await expect(palette).toHaveCount(0);
  await expect.poll(() => new URL(page.url()).pathname).toBe(`/w/${saved.workspace}/projects/${project.id}`);
  expect(new URL(page.url()).searchParams.get('team_id')).toBe(team.id);
  await expect(page.getByRole('heading', { name: project.name, exact: true })).toBeVisible();
  await page.reload(); await expect(page.getByRole('heading', { name: project.name, exact: true })).toBeVisible();
  expect(await saved.request<StructuredDocument>(`${projectPath}/${project.id}/document`)).toEqual(current);
  expect(await saved.issue(issue.id)).toEqual(issue);
  await test.info().attach('mixed-palette-search', { contentType: 'application/json', body: JSON.stringify({
    issue_body_only_match: true, current_project_brief_only_match: true, project_in_other_team: true,
    unscoped_search_limit: 20, literal_labels_and_snippets: true, narrow_snippet_wrapping: true, selection_revalidations: 2, detail_gets_observed: reads.length,
    keyboard_navigation: true, project_reload_persisted: true, no_entity_mutation: true,
  }) });
});

test('palette excludes private search controls and revalidates stale issue and project results after access is revoked', async ({ page, saved, playwright }) => {
  const term = `access${randomUUID().replaceAll('-', '')}`;
  const invitation = await saved.request<{ token: string }>('/invitations', 'POST', {
    email: `palette-access-${randomUUID()}@example.test`, role: 'admin', team_ids: [],
  }, { 'Idempotency-Key': randomUUID() });
  const admin = await playwright.request.newContext({ baseURL: saved.origin, extraHTTPHeaders: { Origin: saved.origin }, storageState: { cookies: [], origins: [] } });
  try {
    const accepted = await admin.post('/api/v1/invitations/accept', { data: { token: invitation.token, display_name: 'Example Administrator', password: process.env.OPENTRACK_BROWSER_PASSWORD } });
    expect(accepted.status()).toBe(200);
    const session = await accepted.json() as Session;
    const create = async <T,>(path: string, data: unknown): Promise<T> => {
      const response = await admin.post(`${saved.path}${path}`, { data, headers: { 'X-CSRF-Token': session.csrf_token, 'Idempotency-Key': randomUUID() } });
      expect(response.status()).toBe(201); return response.json() as Promise<T>;
    };
    const team = await create<TeamDetail>('/teams', { name: `Initially accessible ${randomUUID()}`, key: `A${randomUUID().replaceAll('-', '').slice(0, 8).toUpperCase()}`, private: false });
    const hiddenTeam = await create<TeamDetail>('/teams', { name: `Private search control ${randomUUID()}`, key: `H${randomUUID().replaceAll('-', '').slice(0, 8).toUpperCase()}`, private: true });
    const issue = await create<Issue>('/issues', { team_id: team.id, title: `Visible issue ${randomUUID()}`, document: { schema_version: 1, body: paragraph(term) } });
    const project = await create<FullProject>(`/teams/${team.id}/projects`, { name: `Visible project ${randomUUID()}`, narrative: { schema_version: 1, body: paragraph(term) } });
    const hiddenIssue = await create<Issue>('/issues', { team_id: hiddenTeam.id, title: `Forbidden issue ${randomUUID()}`, document: { schema_version: 1, body: paragraph(`${term} forbidden snippet`) } });
    const hiddenProject = await create<FullProject>(`/teams/${hiddenTeam.id}/projects`, { name: `Forbidden project ${randomUUID()}`, narrative: { schema_version: 1, body: paragraph(`${term} forbidden snippet`) } });
    expect((await saved.http.get(`${saved.path}/issues/${hiddenIssue.id}`)).status()).toBe(404);
    expect((await saved.http.get(`${saved.path}/teams/${hiddenTeam.id}/projects/${hiddenProject.id}/metadata`)).status()).toBe(404);
    // Hold live invalidation apart from this explicit current-read denial journey.
    await pollingFallbackForHeldHttp(page, saved); await saved.open(page);
    await page.getByRole('button', { name: 'Search commands', exact: true }).click();
    const palette = page.getByRole('dialog', { name: 'Commands', exact: true });
    const input = palette.getByRole('combobox', { name: 'Search commands', exact: true });
    const search = page.waitForResponse(response => response.request().method() === 'GET' && new URL(response.url()).pathname === `${saved.path}/search` && new URL(response.url()).searchParams.get('q') === term);
    await input.fill(term);
    const response = await search; expect(response.status()).toBe(200);
    const matches = await response.json() as SearchPage;
    expect(matches.data.map(hit => hit.id).sort()).toEqual([issue.id, project.id].sort());
    await expect(palette.getByRole('option')).toHaveCount(2);
    await expect(palette).not.toContainText(hiddenIssue.title); await expect(palette).not.toContainText(hiddenProject.name); await expect(palette).not.toContainText('forbidden snippet');
    const writes: string[] = [];
    page.on('request', request => { if (request.method() !== 'GET') writes.push(new URL(request.url()).pathname); });
    const currentTeam = await admin.get(`${saved.path}/teams/${team.id}`); expect(currentTeam.status()).toBe(200);
    const changed = await admin.patch(`${saved.path}/teams/${team.id}`, { data: { private: true }, headers: { 'X-CSRF-Token': session.csrf_token, 'If-Match': currentTeam.headers().etag } });
    expect(changed.status()).toBe(200);
    for (const target of [
      { title: project.name, path: `${saved.path}/teams/${team.id}/projects/${project.id}/metadata` },
      { title: issue.title, path: `${saved.path}/issues/${issue.id}` },
    ]) {
      const currentRead = page.waitForResponse(value => value.request().method() === 'GET' && new URL(value.url()).pathname === target.path);
      await palette.getByRole('option').filter({ hasText: target.title }).click();
      expect((await currentRead).status()).toBe(404);
      await expect(palette).toBeVisible(); await expect(input).toBeFocused();
      await expect(palette.getByRole('option').filter({ hasText: target.title })).toHaveCount(0);
      expect(new URL(page.url()).pathname).toBe(`/w/${saved.workspace}/issues`);
    }
    await expect(palette.getByRole('option')).toHaveCount(0); await expect(palette.getByRole('alert').first()).toBeVisible();
    expect(writes).toEqual([]);
    await test.info().attach('mixed-palette-current-access', { contentType: 'application/json', body: JSON.stringify({ private_issue_and_project_excluded: true, private_direct_statuses: [404, 404], stale_current_read_statuses: [404, 404], palette_kept_open: true, browser_mutations: 0 }) });
  } finally { await admin.dispose(); }
});

test('palette ignores a delayed real search response after a newer query has resolved', async ({ page, saved }) => {
  const firstTerm = `earlier${randomUUID().replaceAll('-', '')}`; const currentTerm = `current${randomUUID().replaceAll('-', '')}`;
  const earlier = await saved.create(`Earlier result ${randomUUID()}`, paragraph(firstTerm));
  const current = await saved.create(`Current result ${randomUUID()}`, paragraph(currentTerm));
  let release!: () => void; let fetched!: () => void; let finished!: () => void;
  const gate = new Promise<void>(resolve => { release = resolve; });
  const started = new Promise<void>(resolve => { fetched = resolve; });
  const settled = new Promise<void>(resolve => { finished = resolve; });
  let heldResponse: Response | undefined;
  await saved.open(page);
  await page.route(`${saved.origin}${saved.path}/search?*`, async route => {
    if (new URL(route.request().url()).searchParams.get('q') !== firstTerm) { await route.continue(); return; }
    const response = await route.fetch();
    try {
      expect(response.status()).toBe(200);
      expect((await response.json() as SearchPage).data.map(hit => hit.id)).toEqual([earlier.id]);
      fetched(); await gate;
      try { await route.fulfill({ response }); } catch { /* Superseded browser requests may already be aborted. */ }
    } finally { finished(); await response.dispose(); }
  });
  page.on('response', response => { if (new URL(response.url()).pathname === `${saved.path}/search` && new URL(response.url()).searchParams.get('q') === firstTerm) heldResponse = response; });
  try {
    await page.getByRole('button', { name: 'Search commands', exact: true }).click();
    const palette = page.getByRole('dialog', { name: 'Commands', exact: true });
    const input = palette.getByRole('combobox', { name: 'Search commands', exact: true });
    await input.fill(firstTerm); await started;
    const latest = page.waitForResponse(response => response.request().method() === 'GET' && new URL(response.url()).pathname === `${saved.path}/search` && new URL(response.url()).searchParams.get('q') === currentTerm);
    await input.fill(currentTerm); expect((await latest).status()).toBe(200);
    await expect(palette.getByRole('option')).toHaveCount(1); await expect(palette.getByRole('option')).toContainText(current.title);
    release(); await settled;
    await page.evaluate(() => new Promise<void>(resolve => requestAnimationFrame(() => requestAnimationFrame(() => resolve()))));
    await expect(input).toHaveValue(currentTerm); await expect(palette.getByRole('option')).toHaveCount(1);
    await expect(palette).not.toContainText(earlier.title); await expect(palette.getByRole('option')).toContainText(current.title);
    await input.press('Enter'); await expect(palette).toHaveCount(0);
    await expect(page.getByRole('textbox', { name: 'Issue title', exact: true })).toHaveValue(current.title);
    await test.info().attach('mixed-palette-query-order', { contentType: 'application/json', body: JSON.stringify({ held_authorized_response: true, latest_real_response_status: 200, held_browser_response_received: !!heldResponse, newer_result_preserved: true, keyboard_opened_current_result: true }) });
  } finally { release(); }
});
