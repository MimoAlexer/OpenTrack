import { randomUUID } from 'node:crypto';
import { expect, registerAdmission, test, type SavedWorkspace } from './fixtures';
import type { Issue } from '../src/types';
import type { TeamDetail } from '../src/settings/types';
import type { Workflow } from '../src/workflows/types';

registerAdmission(1);

async function remoteTitle(saved: SavedWorkspace, issue: Issue, title: string): Promise<Issue> {
  const response = await saved.http.patch(`${saved.path}/issues/${issue.id}`, {
    data: { title },
    headers: { Origin: saved.origin, 'X-CSRF-Token': saved.session.csrf_token, 'If-Match': issue.etag },
  });
  expect(response.status()).toBe(200);
  return response.json() as Promise<Issue>;
}

test('replays remote issue changes after offline recovery while preserving an editable local draft', async ({ page, saved }) => {
  const issue = await saved.create(`Realtime recovery ${randomUUID()}`);
  const eventsEndpoint = new URL(`${saved.path}/events`, saved.origin).href;
  const connected = page.waitForResponse(response => response.url() === eventsEndpoint && response.request().method() === 'GET');
  await saved.open(page, issue);
  const stream = await connected;
  expect(stream.status()).toBe(200);
  expect(await stream.headerValue('content-type')).toContain('text/event-stream');

  const title = page.getByRole('textbox', { name: 'Issue title', exact: true });
  const save = page.getByRole('button', { name: 'Save changes', exact: true });
  const conflict = page.getByRole('alert').filter({ hasText: 'This issue changed while you were editing.' });
  const draft = `Local realtime draft ${randomUUID()}`;
  await title.fill(draft);
  await expect(save).toBeEnabled();
  const peer = await page.context().newPage();
  try {
    const connectedPeer = peer.waitForResponse(response => response.url() === eventsEndpoint && response.status() === 200);
    await peer.goto(`/w/${saved.workspace}/issues/${issue.id}`); await connectedPeer;
    const team = await saved.request<TeamDetail>(`/teams/${issue.team_id}`);
    const teamName = `Renamed realtime team ${randomUUID().slice(0, 8)}`;
    await saved.request(`/teams/${issue.team_id}`, 'PATCH', { name: teamName }, { 'If-Match': `"team:${team.id}:${team.version}"` });
    const workflow = await saved.request<Workflow>(`/teams/${issue.team_id}/workflow`);
    const statusName = `Renamed realtime status ${randomUUID().slice(0, 8)}`;
    await saved.request(`/teams/${issue.team_id}/statuses/${issue.status_id}`, 'PATCH', { name: statusName }, { 'If-Match': `"workflow:${issue.team_id}:${workflow.version}:true"` });
    for (const client of [page, peer]) {
      await expect(client.getByRole('navigation', { name: 'Workspace navigation' }).getByText(teamName, { exact: true })).toBeVisible();
      await client.locator('.filter-menu summary').click();
      await expect(client.getByLabel('Team', { exact: true }).locator(`option[value="${team.id}"]`)).toHaveText(teamName);
      await expect(client.getByLabel('Status', { exact: true }).locator(`option[value="${issue.status_id}"]`)).toContainText(statusName);
      await client.locator('.filter-menu summary').click();
    }
    await expect(title).toHaveValue(draft); await expect(save).toBeEnabled();
  } finally { await peer.close(); }
  const first = await remoteTitle(saved, issue, `Remote connected title ${randomUUID()}`);
  await expect(conflict).toBeVisible();
  await expect(conflict).toContainText(first.title);
  await expect(conflict).toContainText(draft);
  await expect(title).toHaveValue(draft);
  await expect(save).toBeDisabled();

  // Review the first conflict without saving, so the outage must independently
  // disable a save that was enabled immediately before going offline.
  await conflict.getByRole('button', { name: 'Keep my edits for review', exact: true }).click();
  await expect(conflict).toHaveCount(0);
  await expect(save).toBeEnabled();
  await expect(title).toHaveValue(draft);

  const context = page.context();
  const offlineBanner = page.getByRole('status').filter({ hasText: 'You are offline.' });
  const offlineDraft = `Draft edited while offline ${randomUUID()}`;
  try {
    await context.setOffline(true);
    await expect(offlineBanner).toBeVisible();
    await expect(save).toBeDisabled();
    await expect(title).toBeEditable();
    await title.fill(offlineDraft);
    await expect(title).toHaveValue(offlineDraft);
    await expect(save).toBeDisabled();

    // The fixture's independent API client stays online while this browser is
    // disconnected; its authorized write must be replayed after reconnection.
    const second = await remoteTitle(saved, first, `Remote offline title ${randomUUID()}`);
    expect((await saved.issue(issue.id)).title).toBe(second.title);
    const reconnected = page.waitForResponse(response => response.url() === eventsEndpoint && response.request().method() === 'GET');
    await context.setOffline(false);
    const replay = await reconnected;
    expect(replay.status()).toBe(200);
    expect(await replay.headerValue('content-type')).toContain('text/event-stream');
    await expect(offlineBanner).toHaveCount(0);
    await expect(conflict).toBeVisible();
    await expect(conflict).toContainText(second.title);
    await expect(conflict).toContainText(offlineDraft);
    await expect(title).toHaveValue(offlineDraft);
    await expect(title).toBeEditable();
    await expect(save).toBeDisabled();
    expect((await saved.issue(issue.id)).title).toBe(second.title);
  } finally {
    await context.setOffline(false);
  }
});
