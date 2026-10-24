import { randomUUID } from 'node:crypto';
import type { APIRequestContext, Request, Route } from '@playwright/test';
import { test, expect } from './fixtures';
import type { Bootstrap, Issue, Session } from '../src/types';
import type { TeamDetail, WorkspaceDetail } from '../src/settings/types';

async function sessionFor(proof: APIRequestContext) {
  const response = await proof.get('/api/v1/auth/session');
  expect(response.status()).toBe(200);
  return response.json() as Promise<Session>;
}

for (const interruptedRefresh of [false, true]) {
  test(interruptedRefresh
    ? 'a committed workspace survives a failed session refresh and retries opening without another create'
    : 'workspace-menu creation seeds a usable team and default, then an issue survives reload', async ({ page, saved, playwright }) => {
    await saved.open(page);
    const proof = await playwright.request.newContext({ baseURL: saved.origin, extraHTTPHeaders: { Origin: saved.origin },
      storageState: { cookies: await page.context().cookies(), origins: [] } });
    const creationUrl = `${saved.origin}/api/v1/workspaces`;
    const sessionUrl = `${saved.origin}/api/v1/auth/session`;
    let posts = 0; let committed = false; let abortedRefreshes = 0;
    const countPost = (request: Request) => { if (request.method() === 'POST' && request.url() === creationUrl) posts++; };
    const refreshRoute = async (route: Route) => {
      if (route.request().method() === 'GET' && committed && abortedRefreshes === 0) {
        abortedRefreshes++; await route.abort('failed');
      } else await route.continue();
    };
    page.on('request', countPost);
    try {
      const name = `Workspace ${interruptedRefresh ? 'opening recovery' : 'creation acceptance'} ${randomUUID()}`;
      const teamName = `Workspace seed team ${randomUUID().slice(0, 8)}`;
      const teamKey = `W${randomUUID().replaceAll('-', '').slice(0, 7).toUpperCase()}`;
      const before = await sessionFor(proof);
      expect(before.user.id).toBe(saved.session.user.id);
      expect(before.workspaces[0]).toMatchObject({ id: saved.workspace, name: 'Browser acceptance' });
      expect(before.workspaces.filter(workspace => workspace.name === name)).toEqual([]);
      if (interruptedRefresh) await page.route(sessionUrl, refreshRoute);

      const menu = page.locator('.workspace-menu');
      await menu.locator('summary').click();
      await expect(menu).toHaveAttribute('open', '');
      await menu.getByRole('button', { name: 'Create workspace', exact: true }).click();
      const dialog = page.getByRole('dialog', { name: 'Create workspace', exact: true });
      const organization = dialog.getByRole('combobox', { name: 'Organization', exact: true });
      await expect(organization).toBeVisible();
      const organizationId = await organization.inputValue(); expect(organizationId).not.toBe('');
      await dialog.getByRole('textbox', { name: 'Workspace name', exact: true }).fill(name);
      await dialog.getByRole('textbox', { name: 'First team', exact: true }).fill(teamName);
      await dialog.getByRole('textbox', { name: 'Issue prefix', exact: true }).fill(teamKey);
      const [response] = await Promise.all([
        page.waitForResponse(response => {
          if (response.request().method() !== 'POST' || response.url() !== creationUrl) return false;
          if (response.status() === 201) committed = true;
          return true;
        }),
        dialog.getByRole('button', { name: 'Create workspace', exact: true }).click(),
      ]);
      expect(response.status()).toBe(201);
      expect(response.request().postDataJSON()).toEqual({ organization_id: organizationId, name, team_name: teamName, team_key: teamKey });
      const created = await response.json() as WorkspaceDetail;
      expect(created).toMatchObject({ organization_id: organizationId, name, role: 'owner', archived_at: null });
      expect(posts).toBe(1);

      const afterCommit = await sessionFor(proof);
      expect(afterCommit.workspaces).toHaveLength(before.workspaces.length + 1);
      expect(afterCommit.workspaces.filter(workspace => workspace.name === name).map(workspace => workspace.id)).toEqual([created.id]);
      const workspaceResponse = await proof.get(`/api/v1/workspaces/${created.id}`); expect(workspaceResponse.status()).toBe(200);
      expect(await workspaceResponse.json()).toMatchObject({ id: created.id, organization_id: organizationId, name, role: 'owner' });

      if (interruptedRefresh) {
        await expect.poll(() => abortedRefreshes).toBe(1);
        await expect(dialog).toBeVisible();
        await expect(dialog).toContainText(created.name);
        const retry = dialog.getByRole('button', { name: 'Retry opening workspace', exact: true });
        await expect(retry).toBeEnabled();
        expect(posts).toBe(1);
        const [refreshed] = await Promise.all([
          page.waitForResponse(item => item.request().method() === 'GET' && item.url() === sessionUrl && item.status() === 200),
          retry.click(),
        ]);
        expect(refreshed.status()).toBe(200);
      }

      await expect.poll(() => new URL(page.url()).pathname).toBe(`/w/${created.id}/issues`);
      await expect(dialog).toHaveCount(0);
      await expect(page.locator('.workspace-identity strong')).toHaveText(name);
      await expect(page.getByRole('button', { name: /^New issue/ })).toBeEnabled();
      expect(posts).toBe(1); expect(abortedRefreshes).toBe(interruptedRefresh ? 1 : 0);
      const opened = await sessionFor(proof);
      expect(opened.workspaces.filter(workspace => workspace.name === name).map(workspace => workspace.id)).toEqual([created.id]);
      expect(opened.workspaces).toHaveLength(before.workspaces.length + 1);

      const bootstrapResponse = await proof.get(`/api/v1/workspaces/${created.id}/bootstrap`); expect(bootstrapResponse.status()).toBe(200);
      const bootstrap = await bootstrapResponse.json() as Bootstrap;
      expect(bootstrap.teams).toHaveLength(1);
      const team = bootstrap.teams[0];
      expect(team).toMatchObject({ name: teamName, key: teamKey, private: false, archived_at: null });
      const teamResponse = await proof.get(`/api/v1/workspaces/${created.id}/teams/${team.id}`); expect(teamResponse.status()).toBe(200);
      expect(await teamResponse.json() as TeamDetail).toMatchObject({ id: team.id, workspace_id: created.id, name: teamName, key: teamKey, can_manage: true });
      const defaults = bootstrap.statuses.filter(status => status.team_id === team.id && status.is_default && !status.archived_at && !status.retiring_at);
      expect(defaults).toHaveLength(1);
      const navigation = page.getByRole('navigation', { name: 'Workspace navigation', exact: true });
      await expect(navigation.getByRole('link').filter({ hasText: teamName })).toBeVisible();

      await page.getByRole('button', { name: /^New issue/ }).click();
      const issueDialog = page.getByRole('dialog', { name: 'New issue', exact: true });
      await issueDialog.getByRole('combobox', { name: 'Team', exact: true }).selectOption(team.id);
      await expect(issueDialog.getByRole('combobox', { name: /^Status/ })).toHaveValue(defaults[0].id);
      const title = `Workspace issue ${randomUUID()}`;
      await issueDialog.getByRole('textbox', { name: 'Title', exact: true }).fill(title);
      const issueUrl = `${saved.origin}/api/v1/workspaces/${created.id}/issues`;
      const [savedIssue] = await Promise.all([
        page.waitForResponse(item => item.request().method() === 'POST' && item.url() === issueUrl),
        issueDialog.getByRole('button', { name: 'Create issue', exact: true }).click(),
      ]);
      expect(savedIssue.status()).toBe(201);
      expect(savedIssue.request().postDataJSON()).not.toHaveProperty('status_id');
      const issue = await savedIssue.json() as Issue;
      expect(issue).toMatchObject({ workspace_id: created.id, team_id: team.id, status_id: defaults[0].id, title });
      await expect.poll(() => new URL(page.url()).pathname).toBe(`/w/${created.id}/issues/${issue.id}`);
      const detail = page.getByRole('region', { name: 'Issue detail', exact: true });
      await expect(detail.getByRole('textbox', { name: 'Issue title', exact: true })).toHaveValue(title);
      await page.reload();
      await expect(detail.getByRole('textbox', { name: 'Issue title', exact: true })).toHaveValue(title);
      await expect(detail.getByRole('combobox', { name: 'Status', exact: true })).toHaveValue(defaults[0].id);
      const persisted = await proof.get(`/api/v1/workspaces/${created.id}/issues/${issue.id}`); expect(persisted.status()).toBe(200);
      expect(await persisted.json() as Issue).toMatchObject({ id: issue.id, key: issue.key, workspace_id: created.id, team_id: team.id, status_id: defaults[0].id, title, version: issue.version });
      expect(posts).toBe(1);

      await page.locator('.workspace-menu > summary').click();
      await page.getByRole('combobox', { name: 'Workspace', exact: true }).selectOption(saved.workspace);
      await expect.poll(() => new URL(page.url()).pathname).toBe(`/w/${saved.workspace}/issues`);
      await expect(page.locator('.workspace-identity strong')).toHaveText('Browser acceptance');
      expect((await sessionFor(proof)).workspaces[0].id).toBe(saved.workspace);
    } finally {
      page.off('request', countPost);
      if (interruptedRefresh) await page.unroute(sessionUrl, refreshRoute);
      await proof.dispose();
    }
  });
}


test('declining navigation after workspace creation keeps the original issue draft until it is explicitly saved', async ({ page, saved }) => {
  const issue = await saved.create(`Original workspace issue ${randomUUID()}`);
  await saved.open(page, issue);
  const originalUrl = `/w/${saved.workspace}/issues/${issue.id}`;
  const issueUrl = `${saved.origin}${saved.path}/issues/${issue.id}`;
  const creationUrl = `${saved.origin}/api/v1/workspaces`;
  const sessionUrl = `${saved.origin}/api/v1/auth/session`;
  const detail = page.getByRole('region', { name: 'Issue detail', exact: true });
  const title = detail.getByRole('textbox', { name: 'Issue title', exact: true });
  const originalField = await title.elementHandle(); expect(originalField).not.toBeNull();
  const draft = `Preserved issue draft ${randomUUID()}`;
  let workspacePosts = 0; let issuePatches = 0;
  const count = (request: Request) => {
    if (request.method() === 'POST' && request.url() === creationUrl) workspacePosts++;
    if (request.method() === 'PATCH' && request.url() === issueUrl) issuePatches++;
  };
  const beforeUnloadBlocked = () => page.evaluate(() => {
    const event = new Event('beforeunload', { cancelable: true });
    window.dispatchEvent(event);
    return event.defaultPrevented;
  });
  page.on('request', count);
  try {
    await title.fill(draft);
    await expect(detail.getByRole('button', { name: 'Save changes', exact: true })).toBeEnabled();
    await expect.poll(beforeUnloadBlocked).toBe(true);
    const menu = page.locator('.workspace-menu');
    await menu.locator('summary').click();
    await menu.getByRole('button', { name: 'Create workspace', exact: true }).click();
    const dialog = page.getByRole('dialog', { name: 'Create workspace', exact: true });
    const organization = dialog.getByRole('combobox', { name: 'Organization', exact: true });
    await expect(organization).toBeVisible();
    const organizationId = await organization.inputValue(); expect(organizationId).not.toBe('');
    const name = `Workspace draft preservation ${randomUUID()}`;
    const teamName = `Workspace seed team ${randomUUID().slice(0, 8)}`;
    const teamKey = `W${randomUUID().replaceAll('-', '').slice(0, 7).toUpperCase()}`;
    await dialog.getByRole('textbox', { name: 'Workspace name', exact: true }).fill(name);
    await dialog.getByRole('textbox', { name: 'First team', exact: true }).fill(teamName);
    await dialog.getByRole('textbox', { name: 'Issue prefix', exact: true }).fill(teamKey);
    const [creation, refreshed] = await Promise.all([
      page.waitForResponse(response => response.request().method() === 'POST' && response.url() === creationUrl),
      page.waitForResponse(response => response.request().method() === 'GET' && response.url() === sessionUrl),
      page.waitForEvent('dialog').then(async confirmation => {
        const actual = { type: confirmation.type(), message: confirmation.message() };
        await confirmation.dismiss();
        expect(actual).toEqual({ type: 'confirm', message: 'Discard your unsaved changes?' });
      }),
      dialog.getByRole('button', { name: 'Create workspace', exact: true }).click(),
    ]);
    expect(creation.status()).toBe(201);
    expect(creation.request().postDataJSON()).toEqual({ organization_id: organizationId, name, team_name: teamName, team_key: teamKey });
    const created = await creation.json() as WorkspaceDetail;
    expect(created).toMatchObject({ organization_id: organizationId, name, role: 'owner', archived_at: null });
    expect(refreshed.status()).toBe(200);
    const session = await refreshed.json() as Session;
    expect(session.user.id).toBe(saved.session.user.id);
    expect(session.workspaces.filter(workspace => workspace.name === name).map(workspace => workspace.id)).toEqual([created.id]);
    await expect(dialog).toHaveCount(0);
    await expect.poll(() => new URL(page.url()).pathname).toBe(originalUrl);
    await expect(page.locator('.workspace-identity strong')).toHaveText('Browser acceptance');
    await expect(title).toHaveValue(draft);
    expect(await originalField!.evaluate(field => field.isConnected)).toBe(true);
    await expect(detail.getByRole('button', { name: 'Save changes', exact: true })).toBeEnabled();
    await expect.poll(beforeUnloadBlocked).toBe(true);
    expect({ workspacePosts, issuePatches }).toEqual({ workspacePosts: 1, issuePatches: 0 });
    expect(await saved.issue(issue.id)).toMatchObject({ id: issue.id, key: issue.key, workspace_id: saved.workspace, title: issue.title, version: issue.version });
    const workspaceResponse = await saved.http.get(`/api/v1/workspaces/${created.id}`); expect(workspaceResponse.status()).toBe(200);
    expect(await workspaceResponse.json()).toMatchObject({ id: created.id, organization_id: organizationId, name });

    const [response] = await Promise.all([
      page.waitForResponse(response => response.request().method() === 'PATCH' && response.url() === issueUrl),
      detail.getByRole('button', { name: 'Save changes', exact: true }).click(),
    ]);
    expect(response.status()).toBe(200);
    expect(response.request().postDataJSON()).toEqual({ title: draft });
    expect(response.request().headers()['if-match']).toBe(issue.etag);
    const updated = await response.json() as Issue;
    expect(updated).toMatchObject({ id: issue.id, key: issue.key, workspace_id: saved.workspace, title: draft });
    expect(BigInt(updated.version)).toBe(BigInt(issue.version) + 1n);
    await expect(detail.locator('.detail-footer [role="status"]')).toHaveText('Changes saved.');
    await expect(detail.getByRole('button', { name: 'Save changes', exact: true })).toBeDisabled();
    await expect.poll(beforeUnloadBlocked).toBe(false);
    expect({ workspacePosts, issuePatches }).toEqual({ workspacePosts: 1, issuePatches: 1 });
    await page.reload();
    await expect(title).toHaveValue(draft);
    expect(await saved.issue(issue.id)).toMatchObject({ id: issue.id, key: issue.key, workspace_id: saved.workspace, title: draft, version: updated.version });
    expect({ workspacePosts, issuePatches }).toEqual({ workspacePosts: 1, issuePatches: 1 });
    await page.getByRole('button', { name: 'Close issue', exact: true }).click();
    await expect.poll(() => new URL(page.url()).pathname).toBe(`/w/${saved.workspace}/issues`);
    await expect(page.locator('.workspace-identity strong')).toHaveText('Browser acceptance');
  } finally {
    page.off('request', count);
    await originalField?.dispose();
  }
});
