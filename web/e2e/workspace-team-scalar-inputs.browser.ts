import { randomUUID } from 'node:crypto';
import type { Locator, Page } from '@playwright/test';
import { test, expect, type SavedWorkspace } from './fixtures';
import type { Bootstrap, Issue, Session } from '../src/types';
import type { OrganizationSummary, TeamDetail, WorkspaceDetail } from '../src/settings/types';

async function read<T>(saved: SavedWorkspace, path: string): Promise<T> {
  const response = await saved.http.get(`/api/v1${path}`); expect(response.status()).toBe(200); return response.json() as Promise<T>;
}
async function fixture(saved: SavedWorkspace) {
  const organizations = await read<OrganizationSummary[]>(saved, '/organizations');
  const response = await saved.http.post('/api/v1/workspaces', { data: { organization_id: organizations[0].id, name: `Scalar fixture ${randomUUID()}`, team_name: 'Scalar seed team', team_key: 'SEED' }, headers: { 'X-CSRF-Token': saved.session.csrf_token, 'Idempotency-Key': randomUUID() } });
  expect(response.status()).toBe(201); return response.json() as Promise<WorkspaceDetail>;
}
async function nativeText(input: Locator, text: string) {
  await input.fill(''); await input.pressSequentially(text);
  const actual = await input.inputValue(); expect.soft(actual, 'Native input preserves every supplementary scalar').toBe(text); return actual === text;
}
async function write(page: Page, scope: Locator, action: string, path: string, method: string, status: number) {
  const pending = page.waitForResponse(response => new URL(response.url()).pathname === `/api/v1${path}` && response.request().method() === method);
  await scope.getByRole('button', { name: action, exact: true }).click(); const response = await pending; expect(response.status()).toBe(status); return response;
}
async function openCreate(page: Page, saved: SavedWorkspace) {
  await saved.open(page); const menu = page.locator('.workspace-menu'); await menu.locator('summary').click(); await menu.getByRole('button', { name: 'Create workspace', exact: true }).click();
  return page.getByRole('dialog', { name: 'Create workspace', exact: true });
}

test('workspace and first-team names preserve scalar boundaries, rejected drafts and stable links', async ({ page, saved }) => {
  let dialog = await openCreate(page, saved); const name = '😀'.repeat(100); const firstTeam = '🧭'.repeat(100);
  const workspaceCreatePreserved = await nativeText(dialog.getByRole('textbox', { name: 'Workspace name', exact: true }), name);
  const firstTeamPreserved = await nativeText(dialog.getByRole('textbox', { name: 'First team', exact: true }), firstTeam);
  await dialog.getByRole('button', { name: 'Cancel', exact: true }).click();
  const probe = await fixture(saved); await page.goto(`/w/${probe.id}/settings/workspace`);
  const probeName = page.getByRole('textbox', { name: /^Workspace name/ });
  const workspaceRenamePreserved = await nativeText(probeName, '🚀'.repeat(100)); await probeName.fill(probe.name);
  if (!workspaceCreatePreserved || !firstTeamPreserved || !workspaceRenamePreserved) return;
  dialog = await openCreate(page, saved); const organization = dialog.getByRole('combobox', { name: 'Organization', exact: true }); await expect(organization).toBeVisible(); const organizationId = await organization.inputValue();
  const prefix = `U${randomUUID().replaceAll('-', '').slice(0, 7).toUpperCase()}`;
  await nativeText(dialog.getByRole('textbox', { name: 'Workspace name', exact: true }), name); await nativeText(dialog.getByRole('textbox', { name: 'First team', exact: true }), firstTeam); await dialog.getByRole('textbox', { name: 'Issue prefix', exact: true }).fill(prefix);
  const before = await read<Session>(saved, '/auth/session');
  for (const [label, field, value] of [['Workspace name', 'name', name], ['First team', 'team_name', firstTeam]] as const) {
    const input = dialog.getByRole('textbox', { name: label, exact: true }); await nativeText(input, value + '🌍');
    const rejected = await write(page, dialog, 'Create workspace', '/workspaces', 'POST', 422); expect(rejected.request().postDataJSON()[field]).toBe(value + '🌍'); await expect(dialog.getByRole('alert')).toContainText('must contain'); await expect(input).toHaveValue(value + '🌍');
    expect((await read<Session>(saved, '/auth/session')).workspaces).toEqual(before.workspaces); await nativeText(input, value);
  }
  const creation = await write(page, dialog, 'Create workspace', '/workspaces', 'POST', 201);
  expect(creation.request().postDataJSON()).toEqual({ organization_id: organizationId, name, team_name: firstTeam, team_key: prefix }); const created = await creation.json() as WorkspaceDetail;
  await expect(dialog).toHaveCount(0); await expect(page).toHaveURL(`${saved.origin}/w/${created.id}/issues`); await expect(page.locator('.workspace-identity strong')).toHaveText(name);
  expect((await read<Session>(saved, '/auth/session')).workspaces).toHaveLength(before.workspaces.length + 1);
  const base = `/workspaces/${created.id}`; const bootstrap = await read<Bootstrap>(saved, `${base}/bootstrap`); expect(bootstrap.teams).toHaveLength(1); const team = bootstrap.teams[0]; expect(team).toMatchObject({ name: firstTeam, key: prefix, private: false });
  const issueResponse = await saved.http.post(`/api/v1${base}/issues`, { data: { team_id: team.id, title: 'Stable scalar workspace link' }, headers: { 'X-CSRF-Token': saved.session.csrf_token, 'Idempotency-Key': randomUUID() } }); expect(issueResponse.status()).toBe(201); const issue = await issueResponse.json() as Issue;
  await page.reload(); await expect(page.locator('.workspace-identity strong')).toHaveText(name); expect((await read<Bootstrap>(saved, `${base}/bootstrap`)).teams).toEqual(bootstrap.teams);
  await page.goto(`/w/${created.id}/settings/workspace`); const scope = page.locator('.settings-form'); const input = scope.getByRole('textbox', { name: /^Workspace name/ }); await expect(input).toHaveValue(name); const current = await read<WorkspaceDetail>(saved, base);
  const overlong = '🚀'.repeat(101); await nativeText(input, overlong); const rejection = await write(page, scope, 'Save changes', base, 'PATCH', 422); expect(rejection.request().postDataJSON()).toEqual({ name: overlong }); await expect(scope.getByRole('alert')).toContainText('must contain'); await expect(input).toHaveValue(overlong); expect(await read<WorkspaceDetail>(saved, base)).toEqual(current);
  await nativeText(input, '🚀'.repeat(100)); await write(page, scope, 'Save changes', base, 'PATCH', 200); await expect(scope.getByRole('button', { name: 'Save changes', exact: true })).toBeDisabled();
  expect(await read<WorkspaceDetail>(saved, base)).toMatchObject({ id: created.id, organization_id: organizationId, name: '🚀'.repeat(100), slug: current.slug, version: (BigInt(current.version) + 1n).toString() });
  await page.reload(); await expect(input).toHaveValue('🚀'.repeat(100)); expect((await read<Bootstrap>(saved, `${base}/bootstrap`)).teams).toEqual(bootstrap.teams); expect(await read<Issue>(saved, `${base}/issues/${issue.id}`)).toEqual(issue);
  await page.goto(`/w/${created.id}/issues/${issue.id}`); await expect(page.getByRole('textbox', { name: 'Issue title', exact: true })).toHaveValue(issue.title); expect(issue.key.startsWith(`${prefix}-`)).toBe(true);
});

test('ordinary private-team names preserve scalar boundaries, rejected drafts and stable issue keys', async ({ page, saved }) => {
  const workspace = await fixture(saved); const base = `/workspaces/${workspace.id}`; const bootstrap = await read<Bootstrap>(saved, `${base}/bootstrap`); const seed = bootstrap.teams[0];
  await page.goto(`/w/${workspace.id}/settings/teams?team_id=${seed.id}`); await page.getByRole('button', { name: 'New team', exact: true }).click(); const dialog = page.getByRole('dialog', { name: 'Create team', exact: true });
  const name = '🧭'.repeat(100); const createPreserved = await nativeText(dialog.getByRole('textbox', { name: 'Team name', exact: true }), name); await dialog.getByRole('button', { name: 'Cancel', exact: true }).click();
  const seedName = page.locator('.settings-team-detail').getByRole('textbox', { name: 'Team name', exact: true }); const renamePreserved = await nativeText(seedName, '😀'.repeat(100)); await seedName.fill(seed.name);
  if (!createPreserved || !renamePreserved) return;
  await page.getByRole('button', { name: 'New team', exact: true }).click(); const input = dialog.getByRole('textbox', { name: 'Team name', exact: true }); const prefix = `T${randomUUID().replaceAll('-', '').slice(0, 7).toUpperCase()}`;
  await nativeText(input, '🧭'.repeat(101)); await dialog.getByRole('textbox', { name: /^Issue prefix/ }).fill(prefix); await dialog.getByRole('checkbox', { name: /^Private team/ }).check();
  const before = await read<{ data: TeamDetail[] }>(saved, `${base}/teams`); const rejected = await write(page, dialog, 'Create team', `${base}/teams`, 'POST', 422); expect(rejected.request().postDataJSON()).toEqual({ name: '🧭'.repeat(101), key: prefix, private: true }); await expect(dialog.getByRole('alert')).toContainText('must contain'); await expect(input).toHaveValue('🧭'.repeat(101)); expect(await read<{ data: TeamDetail[] }>(saved, `${base}/teams`)).toEqual(before);
  await nativeText(input, name); const response = await write(page, dialog, 'Create team', `${base}/teams`, 'POST', 201); const created = await response.json() as TeamDetail; await expect(dialog).toHaveCount(0); expect(created).toMatchObject({ name, key: prefix, private: true, version: '1', can_manage: true }); expect((await read<{ data: TeamDetail[] }>(saved, `${base}/teams`)).data).toHaveLength(before.data.length + 1);
  const issueResponse = await saved.http.post(`/api/v1${base}/issues`, { data: { team_id: created.id, title: 'Stable scalar team link' }, headers: { 'X-CSRF-Token': saved.session.csrf_token, 'Idempotency-Key': randomUUID() } }); expect(issueResponse.status()).toBe(201); const issue = await issueResponse.json() as Issue;
  await page.goto(`/w/${workspace.id}/settings/teams?team_id=${created.id}`); const scope = page.locator('.settings-team-detail'); const edit = scope.getByRole('textbox', { name: 'Team name', exact: true }); await expect(edit).toHaveValue(name); await page.reload(); await expect(edit).toHaveValue(name);
  const path = `${base}/teams/${created.id}`; const current = await read<TeamDetail>(saved, path); await nativeText(edit, '🚀'.repeat(101)); const rejection = await write(page, scope, 'Save team', path, 'PATCH', 422); expect(rejection.request().postDataJSON()).toEqual({ name: '🚀'.repeat(101), private: true }); await expect(scope.getByRole('alert')).toContainText('must contain'); await expect(edit).toHaveValue('🚀'.repeat(101)); expect(await read<TeamDetail>(saved, path)).toEqual(current);
  await nativeText(edit, '🚀'.repeat(100)); await write(page, scope, 'Save team', path, 'PATCH', 200); await expect(scope.getByRole('button', { name: 'Save team', exact: true })).toBeDisabled(); expect(await read<TeamDetail>(saved, path)).toMatchObject({ id: created.id, name: '🚀'.repeat(100), key: prefix, private: true, version: (BigInt(current.version) + 1n).toString() });
  await page.reload(); await expect(edit).toHaveValue('🚀'.repeat(100)); expect(await read<Issue>(saved, `${base}/issues/${issue.id}`)).toEqual(issue); await page.goto(`/w/${workspace.id}/issues/${issue.id}`); await expect(page.getByRole('textbox', { name: 'Issue title', exact: true })).toHaveValue(issue.title); expect(issue.key.startsWith(`${prefix}-`)).toBe(true);
});
