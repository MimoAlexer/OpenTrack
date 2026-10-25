import { randomUUID } from 'node:crypto';
import { test, expect, SavedWorkspace } from './fixtures';
import type { Bootstrap } from '../src/types';
import type { OrganizationSummary, Page, TeamDetail, WorkspaceDetail } from '../src/settings/types';

test('team settings deep links load and save a team beyond the first 50 without directory paging', async ({ page, saved }) => {
  const organizationsResponse = await saved.http.get('/api/v1/organizations'); expect(organizationsResponse.ok()).toBe(true);
  const organizations = await organizationsResponse.json() as OrganizationSummary[];
  const created = await saved.http.post('/api/v1/workspaces', {
    data: { organization_id: organizations[0].id, name: `Team directory acceptance ${randomUUID()}`, team_name: 'Directory seed team', team_key: 'DIR' },
    headers: { 'X-CSRF-Token': saved.session.csrf_token, 'Idempotency-Key': randomUUID() },
  });
  expect(created.status()).toBe(201);
  const workspace = await created.json() as WorkspaceDetail;
  const bootstrapResponse = await saved.http.get(`/api/v1/workspaces/${workspace.id}/bootstrap`); expect(bootstrapResponse.ok()).toBe(true);
  const isolated = new SavedWorkspace(saved.http, { ...saved.session, workspaces: [workspace] }, await bootstrapResponse.json() as Bootstrap, saved.origin, workspace.id);
  const teams: TeamDetail[] = [];
  for (let index = 0; index < 51; index++) teams.push(await isolated.request<TeamDetail>('/teams', 'POST', { name: `Directory team ${index}`, key: `T${String(index).padStart(2, '0')}`, private: false }, { 'Idempotency-Key': randomUUID() }));
  const first = await isolated.request<Page<TeamDetail>>('/teams?state=active&limit=50');
  expect(first.data).toHaveLength(50); expect(first.has_more).toBe(true);
  const loaded = new Set(first.data.map(team => team.id));
  const target = teams.find(team => !loaded.has(team.id)); expect(target).toBeDefined();
  const readable = await isolated.request<TeamDetail>(`/teams/${target!.id}`);
  expect(readable.id).toBe(target!.id); expect(readable.name).toBe(target!.name); expect(readable.can_manage).toBe(true);
  const path = `/w/${workspace.id}/settings/teams?team_id=${target!.id}`;
  await page.goto(path);
  await expect(page.locator('.settings-team-list > button')).toHaveCount(50);
  await expect(page.locator('.settings-team-detail').getByRole('textbox', { name: 'Team name', exact: true })).toHaveValue(target!.name);
  await expect(page.locator('.settings-team-list').getByRole('button', { name: 'Load more', exact: true })).toBeVisible();
  const renamed = `${target!.name} renamed`;
  await page.getByRole('textbox', { name: 'Team name', exact: true }).fill(renamed);
  await page.getByRole('button', { name: 'Save team', exact: true }).click();
  await expect.poll(async () => (await isolated.request<TeamDetail>(`/teams/${target!.id}`)).name).toBe(renamed);
  await expect(page.getByRole('textbox', { name: 'Team name', exact: true })).toHaveValue(renamed);
  await page.reload();
  await expect(page.getByRole('textbox', { name: 'Team name', exact: true })).toHaveValue(renamed);
  const secondName = `${target!.name} verified`;
  await page.getByRole('textbox', { name: 'Team name', exact: true }).fill(secondName);
  await page.getByRole('button', { name: 'Save team', exact: true }).click();
  await expect.poll(async () => (await isolated.request<TeamDetail>(`/teams/${target!.id}`)).name).toBe(secondName);
  await expect(page.locator('.settings-team-list > button')).toHaveCount(50);
  const final = await isolated.request<TeamDetail>(`/teams/${target!.id}`);
  expect(BigInt(final.version)).toBe(BigInt(target!.version) + 2n);
});
