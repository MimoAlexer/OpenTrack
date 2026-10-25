import { randomUUID } from 'node:crypto';
import type { Route } from '@playwright/test';
import { test, expect } from './fixtures';
import type { Milestone } from '../src/projects/Milestones';
import type { FullProject, Page, ProjectIssue } from '../src/projects/types';
import type { Issue, Session, Team } from '../src/types';

test('milestone conflicts preserve relative intent and new milestones; a lost committed response is reconciled without replay', async ({ page, saved }) => {
  const suffix = randomUUID().slice(0, 8);
  const team = await saved.request<Team>('/teams', 'POST', { name: `Move review ${suffix}`, key: `R${suffix.toUpperCase()}`, private: false }, { 'Idempotency-Key': randomUUID() });
  const base = `/teams/${team.id}/projects`;
  const project = await saved.request<FullProject>(base, 'POST', { name: `Move recovery ${suffix}` }, { 'Idempotency-Key': randomUUID() });
  const path = `${base}/${project.id}`;
  const milestones: Milestone[] = [];
  let creationTag = project.metadata_etag;
  for (const title of ['First outcome', 'Second outcome', 'Third outcome']) {
    milestones.push(await saved.request<Milestone>(`${path}/milestones`, 'POST', { title }, { 'Idempotency-Key': randomUUID(), 'If-Match': creationTag }));
    creationTag = (await saved.request<FullProject>(path)).metadata_etag;
  }
  await saved.open(page);
  await page.goto(`/w/${saved.workspace}/projects/${project.id}?team_id=${team.id}`);
  const section = page.getByRole('region', { name: 'Project milestones', exact: true });
  await expect(section.getByRole('heading', { level: 4 })).toHaveText(milestones.map(item => item.title));
  const added = await saved.request<Milestone>(`${path}/milestones`, 'POST', { title: 'Concurrent new outcome' }, { 'Idempotency-Key': randomUUID(), 'If-Match': creationTag });
  const endpoint = `${saved.origin}${saved.path}${path}/milestones/order`;
  const writes: { body: { milestone_ids: string[] }; etag: string | undefined }[] = [];
  page.on('request', request => { if (request.method() === 'PUT' && request.url() === endpoint) writes.push({ body: request.postDataJSON(), etag: request.headers()['if-match'] }); });
  const conflict = page.waitForResponse(response => response.request().method() === 'PUT' && response.url() === endpoint);
  await section.getByRole('button', { name: 'Move Second outcome up', exact: true }).click();
  expect((await conflict).status()).toBe(412);
  const review = page.getByRole('dialog', { name: 'Review milestone move', exact: true });
  await expect(review.getByRole('button', { name: 'Apply reviewed move', exact: true })).toBeEnabled();
  await expect(review.getByRole('list', { name: 'Current milestone order', exact: true })).toHaveText(milestones.map(item => item.title).join('') + added.title);
  expect(writes).toHaveLength(1);
  expect(writes[0].body.milestone_ids).toEqual([milestones[1].id, milestones[0].id, milestones[2].id]);
  const current = await saved.request<FullProject>(path);
  await review.getByRole('button', { name: 'Apply reviewed move', exact: true }).click();
  await expect(review).toHaveCount(0);
  const expected = [milestones[1], milestones[0], milestones[2], added];
  await expect(section.getByRole('heading', { level: 4 })).toHaveText(expected.map(item => item.title));
  await expect(section.getByRole('button', { name: 'Edit Second outcome', exact: true })).toBeFocused();
  expect(writes).toHaveLength(2);
  expect(writes[1].body.milestone_ids).toEqual(expected.map(item => item.id));
  expect(writes[1].etag).toBe(current.metadata_etag);

  let committed: FullProject | null = null;
  const relay = async (route: Route) => {
    if (route.request().method() !== 'PUT') { await route.continue(); return; }
    const response = await route.fetch(); expect(response.status()).toBe(200);
    committed = await response.json() as FullProject;
    await route.abort('failed');
  };
  await page.route(endpoint, relay);
  try {
    await section.getByRole('button', { name: 'Move Third outcome up', exact: true }).click();
    await expect(review.getByRole('status')).toContainText('This confirms the current state, not who applied it.');
    expect(committed).not.toBeNull(); expect(writes).toHaveLength(3);
    await expect(review.getByRole('button', { name: 'Apply reviewed move', exact: true })).toHaveCount(0);
    await review.getByRole('button', { name: 'Done', exact: true }).click();
    await expect(section.getByRole('button', { name: 'Move Third outcome up', exact: true })).toBeFocused();
  } finally { await page.unroute(endpoint, relay); }
  await page.reload();
  const finalOrder = [milestones[1], milestones[2], milestones[0], added];
  await expect(section.getByRole('heading', { level: 4 })).toHaveText(finalOrder.map(item => item.title));
  const persisted = await saved.request<FullProject>(path);
  expect(persisted.version).toBe((committed as unknown as FullProject).version);
  expect(writes).toHaveLength(3);
  await test.info().attach('milestone-intent-recovery', { contentType: 'application/json', body: JSON.stringify({ first_status: 412, explicit_reviewed_retry: true, concurrent_milestone_retained: true, committed_response_relayed_as_loss: true, desired_current_order_verified: true, original_acknowledgment_inferred: false, total_put_requests: writes.length, reload_persisted: true }) });
});

test('project assignment review exhausts pagination, preserves milestone intent and reconciles absence without replay or stale private labels', async ({ page, saved, playwright }) => {
  const suffix = randomUUID().slice(0, 8);
  const invitation = await saved.request<{ token: string }>('/invitations', 'POST', { email: `planning-${randomUUID()}@example.test`, role: 'admin', team_ids: [] }, { 'Idempotency-Key': randomUUID() });
  const admin = await playwright.request.newContext({ baseURL: saved.origin, extraHTTPHeaders: { Origin: saved.origin }, storageState: { cookies: [], origins: [] } });
  try {
    const accepted = await admin.post('/api/v1/invitations/accept', { data: { token: invitation.token, display_name: 'Example Administrator', password: process.env.OPENTRACK_BROWSER_PASSWORD } });
    expect(accepted.status()).toBe(200);
    const session = await accepted.json() as Session;
    async function request<T>(path: string, method = 'GET', data?: unknown, etag?: string): Promise<T> {
      const response = await admin.fetch(`${saved.path}${path}`, { method, data, headers: { ...(method !== 'GET' ? { 'X-CSRF-Token': session.csrf_token } : {}), ...(method === 'POST' ? { 'Idempotency-Key': randomUUID() } : {}), ...(etag ? { 'If-Match': etag } : {}) } });
      expect(response.ok()).toBe(true); return response.json() as Promise<T>;
    }
    const team = await request<Team>('/teams', 'POST', { name: `Assignment review ${suffix}`, key: `A${suffix.toUpperCase()}`, private: false });
    const project = await request<FullProject>(`/teams/${team.id}/projects`, 'POST', { name: `Assignment recovery ${suffix}` });
    const path = `/teams/${team.id}/projects/${project.id}`;
    const milestone = await request<Milestone>(`${path}/milestones`, 'POST', { title: 'Reviewed assignment target' }, project.metadata_etag);
    const issues: Issue[] = [];
    for (let index = 0; index < 102; index += 1) issues.push(await request<Issue>('/issues', 'POST', { team_id: team.id, title: `Recovery issue ${suffix} ${index}` }));
    let current = await request<FullProject>(path);
    for (const issue of issues) current = await request<FullProject>(`${path}/issues/${issue.id}`, 'PUT', { included: true }, current.metadata_etag);
    const target = [...issues].sort((a, b) => a.id.localeCompare(b.id)).at(-1)!;
    const targetBefore = await request<Issue>(`/issues/${target.id}`);
    await saved.open(page);
    await page.goto(`/w/${saved.workspace}/projects/${project.id}?team_id=${team.id}`);
    await expect(page.getByRole('heading', { name: project.name, exact: true })).toBeVisible();
    await page.getByRole('navigation', { name: 'Project sections', exact: true }).getByRole('button', { name: 'Issues', exact: true }).click();
    const section = page.getByRole('region', { name: 'Project issues', exact: true });
    await section.getByRole('button', { name: 'Load more issues', exact: true }).click();
    await expect(section.locator('.project-issue-row')).toHaveCount(100);
    await section.getByRole('button', { name: 'Load more issues', exact: true }).click();
    await expect(section.locator('.project-issue-row')).toHaveCount(102);
    await request(`${path}/metadata`, 'PATCH', { summary: 'A concurrent metadata change' }, current.metadata_etag);
    const milestoneEndpoint = `${saved.origin}${saved.path}${path}/issues/${target.id}/milestone`;
    const membershipEndpoint = `${saved.origin}${saved.path}${path}/issues/${target.id}`;
    const writes: string[] = []; const pages: { cursor: string | null; count: number; target: boolean; more: boolean }[] = [];
    page.on('request', request => { if (request.method() === 'PUT' && [milestoneEndpoint, membershipEndpoint].includes(request.url())) writes.push(request.url()); });
    page.on('response', async response => {
      const url = new URL(response.url());
      if (response.request().method() === 'GET' && url.pathname === `${saved.path}${path}/issues` && url.searchParams.get('limit') === '100' && response.ok()) {
        const data = await response.json() as Page<ProjectIssue>;
        pages.push({ cursor: url.searchParams.get('cursor'), count: data.data.length, target: data.data.some(item => item.id === target.id), more: data.has_more });
      }
    });
    const conflict = page.waitForResponse(response => response.request().method() === 'PUT' && response.url() === milestoneEndpoint);
    await section.getByRole('combobox', { name: `Milestone for ${target.key}`, exact: true }).selectOption(milestone.id);
    expect((await conflict).status()).toBe(412);
    const review = page.getByRole('dialog', { name: 'Review project assignment', exact: true });
    await expect(review.getByRole('button', { name: 'Apply reviewed assignment', exact: true })).toBeEnabled();
    await expect(review).toContainText(milestone.title); expect(writes).toHaveLength(1);
    await expect.poll(() => pages.length).toBe(2);
    expect(pages[0]).toMatchObject({ cursor: null, count: 100, target: false, more: true });
    expect(pages[1]).toMatchObject({ count: 2, target: true, more: false }); expect(pages[1].cursor).toBeTruthy();
    await review.getByRole('button', { name: 'Apply reviewed assignment', exact: true }).click();
    await expect(review).toHaveCount(0); expect(writes).toHaveLength(2);
    await section.getByRole('button', { name: 'Load more issues', exact: true }).click();
    await expect(section.locator('.project-issue-row')).toHaveCount(100);
    await section.getByRole('button', { name: 'Load more issues', exact: true }).click();
    await expect(section.getByRole('combobox', { name: `Milestone for ${target.key}`, exact: true })).toHaveValue(milestone.id);
    const beforeAbsent = pages.length;
    let committedVersion: string | null = null;
    const relay = async (route: Route) => {
      if (route.request().method() !== 'PUT') { await route.continue(); return; }
      const response = await route.fetch(); expect(response.status()).toBe(200);
      committedVersion = (await response.json() as FullProject).version;
      await route.abort('failed');
    };
    await page.route(membershipEndpoint, relay);
    try {
      await section.getByRole('button', { name: `Remove ${target.key} from project`, exact: true }).click();
      await expect(review.getByRole('status')).toContainText('This does not identify which request or person applied it.');
      expect(committedVersion).not.toBeNull(); expect(writes).toHaveLength(3);
      await expect.poll(() => pages.length).toBe(beforeAbsent + 2);
      expect(pages.slice(beforeAbsent)).toEqual([{ cursor: null, count: 100, target: false, more: true }, { cursor: expect.any(String), count: 1, target: false, more: false }]);
      await expect(review.getByRole('button', { name: 'Apply reviewed assignment', exact: true })).toHaveCount(0);
      await review.getByRole('button', { name: 'Done', exact: true }).click();
      await expect(section).toBeFocused();
    } finally { await page.unroute(membershipEndpoint, relay); }
    await page.reload();
    await page.getByRole('navigation', { name: 'Project sections', exact: true }).getByRole('button', { name: 'Issues', exact: true }).click();
    await expect(section.locator('.project-issue-row')).toHaveCount(50);
    expect((await request<FullProject>(path)).version).toBe(committedVersion);
    expect(await request<Issue>(`/issues/${target.id}`)).toEqual(targetBefore); expect(writes).toHaveLength(3);
    const other = [...issues].sort((a, b) => a.id.localeCompare(b.id))[0];
    const response = await admin.get(`${saved.path}/teams/${team.id}`);
    expect(response.status()).toBe(200);
    await request(`/teams/${team.id}`, 'PATCH', { private: true }, response.headers().etag);
    const denied = page.waitForResponse(response => response.request().method() === 'PUT' && new URL(response.url()).pathname === `${saved.path}${path}/issues/${other.id}`);
    await section.getByRole('button', { name: `Remove ${other.key} from project`, exact: true }).click();
    expect((await denied).status()).toBe(404);
    await expect(page.getByRole('heading', { name: project.name, exact: true })).toHaveCount(0);
    await expect(page.getByText(other.title, { exact: true })).toHaveCount(0);
    expect((await saved.http.get(`${saved.path}${path}/metadata`)).status()).toBe(404);
    await test.info().attach('assignment-recovery', { contentType: 'application/json', body: JSON.stringify({ target_after_first_100: true, exhausted_pages_before_absence: 2, stable_live_issue: true, explicit_milestone_review: true, committed_response_relayed_as_loss: true, desired_current_state_only: true, no_replay: true, private_denial: 404, stale_project_labels_removed: true, team_scope: team.id }) });
  } finally { await admin.dispose(); }
});
