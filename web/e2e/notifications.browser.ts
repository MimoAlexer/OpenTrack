import { randomUUID } from 'node:crypto';
import type { APIRequest, APIRequestContext, Page } from '@playwright/test';
import { test, expect, paragraph, registerAdmission, type SavedWorkspace } from './fixtures';
import type { Issue, Session } from '../src/types';
import type { IssueComment } from '../src/documents/types';
import type { TeamDetail, TeamMember } from '../src/settings/types';
import type { NotificationItem, NotificationPage, NotificationPreferences } from '../src/notifications/types';

registerAdmission(3);
test.use({ timezoneId: 'Europe/Zurich' });

// These journeys require the real notification worker. No notification or source
// response is fulfilled by a browser route; SMTP can remain disabled throughout.
async function recipient(api: APIRequest, saved: SavedWorkspace, teamId: string) {
  const invitation = await saved.request<{ token: string }>('/invitations', 'POST', {
    email: `inbox-${randomUUID()}@example.test`, role: 'member', team_ids: [teamId],
  }, { 'Idempotency-Key': randomUUID() });
  const http = await api.newContext({ baseURL: saved.origin, extraHTTPHeaders: { Origin: saved.origin }, storageState: { cookies: [], origins: [] } });
  try {
    const response = await http.post('/api/v1/invitations/accept', { data: { token: invitation.token,
      display_name: 'Example Inbox Member', password: process.env.OPENTRACK_BROWSER_PASSWORD } });
    expect(response.status()).toBe(200); const session = await response.json() as Session;
    const request = async <T,>(path: string, method = 'GET', data?: unknown, headers: Record<string, string> = {}): Promise<T> => {
      const response = await http.fetch(`${saved.path}${path}`, { method, data, headers: { Origin: saved.origin,
        ...(method !== 'GET' ? { 'X-CSRF-Token': session.csrf_token } : {}), ...headers } });
      expect(response.ok(), `Recipient ${method} ${path.split('?')[0]}`).toBe(true);
      return response.json() as Promise<T>;
    };
    return { http, session, request };
  } catch (error) { await http.dispose(); throw error; }
}
async function privateTeam(saved: SavedWorkspace) {
  return saved.request<TeamDetail>('/teams', 'POST', { name: `Inbox acceptance ${randomUUID().slice(0, 8)}`,
    key: `N${randomUUID().replaceAll('-', '').slice(0, 8).toUpperCase()}`, private: true }, { 'Idempotency-Key': randomUUID() });
}
async function assignedIssue(saved: SavedWorkspace, teamId: string, recipientId: string) {
  const issue = await saved.request<Issue>('/issues', 'POST', { team_id: teamId,
    title: `Inbox source ${randomUUID()}`, document: { schema_version: 1, body: paragraph('Current private inbox source body') } }, { 'Idempotency-Key': randomUUID() });
  return saved.patch(issue, { assignee_id: recipientId });
}
async function inboxItems(member: Awaited<ReturnType<typeof recipient>>, filter = 'all') {
  return member.request<NotificationPage>(`/notifications?filter=${filter}&limit=50`);
}
async function openRecipient(page: Page, http: APIRequestContext, workspace: string, notificationId?: string) {
  await page.context().clearCookies(); await page.context().addCookies((await http.storageState()).cookies);
  await page.goto(`/w/${workspace}/inbox${notificationId ? `/${notificationId}` : ''}`);
  await expect(page.getByRole('heading', { name: 'Inbox', exact: true })).toBeVisible();
}
const row = (page: Page, notification: NotificationItem) => page.locator(`[data-notification-id="${notification.id}"]`);

test('real worker fan-out reaches the durable inbox and keyboard processing persists', async ({ page, saved, playwright }) => {
  const team = await privateTeam(saved); const member = await recipient(playwright.request, saved, team.id);
  try {
    const issue = await assignedIssue(saved, team.id, member.session.user.id);
    const comment = await saved.request<IssueComment>(`/issues/${issue.id}/comments`, 'POST', { schema_version: 1, body: paragraph('A followed issue has a new comment') }, { 'Idempotency-Key': randomUUID() });
    const mentioned = await saved.request<Issue>('/issues', 'POST', { team_id: team.id, title: `Mention source ${randomUUID()}` }, { 'Idempotency-Key': randomUUID() });
    await saved.request(`/issues/${mentioned.id}/comments`, 'POST', { schema_version: 1, body: { type: 'doc', content: [{ type: 'paragraph', content: [
      { type: 'text', text: 'Please review ' }, { type: 'userMention', attrs: { id: member.session.user.id } },
    ] }] } }, { 'Idempotency-Key': randomUUID() });
    await expect.poll(async () => (await inboxItems(member)).items.map(item => item.reason), { timeout: 20_000,
      message: 'The actual durable worker creates assignment, mention and subscribed-comment reasons' }).toEqual(expect.arrayContaining(['assignment', 'mention', 'subscribed_comment']));
    const notifications = (await inboxItems(member)).items;
    expect(notifications.filter(item => item.source.id === issue.id && item.reason === 'assignment')).toHaveLength(1);
    expect(notifications.filter(item => item.source.comment_id === comment.id && item.reason === 'subscribed_comment')).toHaveLength(1);
    const external: string[] = []; page.on('request', request => { if (new URL(request.url()).origin !== saved.origin) external.push('external request'); });
    await openRecipient(page, member.http, saved.workspace);
    const rows = page.locator('[data-notification-id]'); await expect(rows).toHaveCount(notifications.length);
    const firstId = await rows.first().getAttribute('data-notification-id'); const nextId = await rows.nth(1).getAttribute('data-notification-id');
    const sourceBefore = await saved.issue(issue.id);
    await rows.first().focus(); await page.keyboard.press('ArrowDown'); await expect(rows.nth(1)).toBeFocused(); await page.keyboard.press('ArrowUp'); await expect(rows.first()).toBeFocused();
    const desktopViewport = page.viewportSize()!; await page.setViewportSize({ width: 640, height: 900 });
    await rows.first().focus(); await page.keyboard.press('ArrowDown'); await expect(rows.nth(1)).toBeVisible(); await expect(rows.nth(1)).toBeFocused();
    await expect(page.locator('.inbox-detail')).toBeHidden(); await page.keyboard.press('Enter');
    const back = page.getByRole('button', { name: 'Back to inbox', exact: true });
    await expect(back).toBeVisible(); await expect(back).toBeFocused(); await expect(page.locator('.inbox-list')).toBeHidden();
    await back.press('Enter'); await expect(rows.nth(1)).toBeVisible(); await expect(rows.nth(1)).toBeFocused();
    await rows.first().press('Enter'); await expect(back).toBeFocused();
    await page.keyboard.press('e'); await expect(page.locator(`[data-notification-id="${firstId}"]`)).toHaveCount(0); await expect(back).toBeVisible(); await expect(back).toBeFocused();
    await back.press('Enter'); await expect(page.locator(`[data-notification-id="${nextId}"]`)).toBeVisible();
    await expect(page.locator(`[data-notification-id="${nextId}"]`)).toBeFocused();
    await page.setViewportSize(desktopViewport);
    await expect.poll(async () => (await inboxItems(member, 'archived')).items.some(item => item.id === firstId)).toBe(true);
    expect(await saved.issue(issue.id)).toMatchObject({ version: sourceBefore.version, status_id: sourceBefore.status_id, archived_at: sourceBefore.archived_at });
    await page.keyboard.press('r'); await expect(page.locator(`[data-notification-id="${nextId}"]`)).toHaveCount(0);
    await expect.poll(async () => (await inboxItems(member)).items.find(item => item.id === nextId)?.read_at).toBeTruthy();
    await page.getByRole('navigation', { name: 'Inbox filters' }).getByRole('button', { name: 'Archived', exact: true }).click();
    await expect(page.locator(`[data-notification-id="${firstId}"]`)).toBeVisible(); await page.reload();
    await expect(page.locator(`[data-notification-id="${firstId}"]`)).toBeVisible();
    expect(external).toEqual([]);
    test.info().annotations.push({ type: 'notification-proof', description: JSON.stringify({ durable_reasons: ['assignment', 'mention', 'subscribed_comment'], keyboard_archive_and_read: true, archive_source_unchanged: true, external_browser_requests: external.length }) });
  } finally { await member.http.dispose(); }
});

test('local timezone snooze and personal inbox preferences survive reload without SMTP', async ({ page, saved, playwright }) => {
  const team = await privateTeam(saved); const member = await recipient(playwright.request, saved, team.id);
  try {
    const issue = await assignedIssue(saved, team.id, member.session.user.id);
    await expect.poll(async () => (await inboxItems(member)).items.some(item => item.source.id === issue.id), { timeout: 20_000 }).toBe(true);
    const notification = (await inboxItems(member)).items.find(item => item.source.id === issue.id)!;
    await openRecipient(page, member.http, saved.workspace);
    await row(page, notification).click(); await expect(page.locator('.inbox-source')).toContainText('Current private inbox source body');
    await page.getByRole('button', { name: 'Snooze', exact: true }).click();
    const chosen = await page.evaluate(() => {
      const date = new Date(); date.setDate(date.getDate() + 1); date.setHours(10, 30, 0, 0); const pad = (value: number) => String(value).padStart(2, '0');
      return { local: `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}T${pad(date.getHours())}:${pad(date.getMinutes())}`, utc: date.toISOString(), tooltip: date.toUTCString() };
    });
    await page.getByLabel('Snooze until', { exact: true }).fill(chosen.local);
    await expect(page.locator('.inbox-snooze time')).toHaveAttribute('title', chosen.tooltip);
    await expect(page.locator('.inbox-snooze')).toContainText('Europe/Zurich');
    await page.getByRole('button', { name: 'Confirm snooze', exact: true }).click(); await expect(row(page, notification)).toHaveCount(0);
    await expect.poll(async () => { const value = (await inboxItems(member, 'snoozed')).items.find(item => item.id === notification.id)?.snoozed_until; return value ? Date.parse(value) : null; }).toBe(Date.parse(chosen.utc));
    await page.getByRole('navigation', { name: 'Inbox filters' }).getByRole('button', { name: 'Snoozed', exact: true }).click(); await page.reload();
    await expect(row(page, notification)).toBeVisible(); await expect(row(page, notification).locator('time').last()).toHaveAttribute('title', chosen.tooltip);
    await row(page, notification).click(); await page.getByRole('button', { name: 'Unsnooze', exact: true }).click();
    await page.getByRole('navigation', { name: 'Inbox filters' }).getByRole('button', { name: 'Unread', exact: true }).click(); await expect(row(page, notification)).toBeVisible();
    await page.getByRole('button', { name: 'Inbox preferences', exact: true }).click();
    const preferences = page.getByRole('region', { name: 'Inbox preferences', exact: true });
    await preferences.getByRole('checkbox', { name: 'Comments on followed issues', exact: true }).uncheck();
    await preferences.getByRole('button', { name: 'Save preferences', exact: true }).click(); await expect(preferences.getByRole('status')).toHaveText('Preferences saved.');
    expect((await member.request<NotificationPreferences>('/notification-preferences')).subscribed_comments).toBe(false);
    await page.reload(); await page.getByRole('button', { name: 'Inbox preferences', exact: true }).click();
    await expect(preferences.getByRole('checkbox', { name: 'Comments on followed issues', exact: true })).not.toBeChecked();
  } finally { await member.http.dispose(); }
});

test('revoked private-team access removes every saved source title and body from the inbox', async ({ page, saved, playwright }) => {
  const team = await privateTeam(saved); const member = await recipient(playwright.request, saved, team.id);
  try {
    const issue = await assignedIssue(saved, team.id, member.session.user.id);
    await expect.poll(async () => (await inboxItems(member)).items.some(item => item.source.id === issue.id), { timeout: 20_000 }).toBe(true);
    const notification = (await inboxItems(member)).items.find(item => item.source.id === issue.id)!;
    await openRecipient(page, member.http, saved.workspace, notification.id);
    await expect(page.locator('.inbox-source')).toContainText(issue.title); await expect(page.locator('.inbox-source')).toContainText('Current private inbox source body');
    const membership = await saved.request<TeamMember>(`/teams/${team.id}/members/${member.session.user.id}`);
    const removed = await saved.http.delete(`${saved.path}/teams/${team.id}/members/${member.session.user.id}`, { headers: {
      Origin: saved.origin, 'X-CSRF-Token': saved.session.csrf_token, 'If-Match': `"team-member:${team.id}:${member.session.user.id}:${membership.version}"`,
    } });
    expect(removed.status()).toBe(204); expect((await member.http.get(`${saved.path}/issues/${issue.id}`)).status()).toBe(404);
    await page.reload(); await expect(page.locator('.inbox-source-unavailable')).toBeVisible();
    await expect(page.locator('.inbox-screen')).not.toContainText(issue.title); await expect(page.locator('.inbox-screen')).not.toContainText(issue.key);
    await expect(page.locator('.inbox-screen')).not.toContainText('Current private inbox source body'); await expect(page.locator('.inbox-source a')).toHaveCount(0);
    const scrubbed = (await inboxItems(member)).items.find(item => item.id === notification.id)!;
    expect(scrubbed.source.available).toBe(false); expect(scrubbed.source.title ?? null).toBeNull(); expect(scrubbed.source.key ?? null).toBeNull();
  } finally { await member.http.dispose(); }
});

test('canonical recipient stream refreshes inbox, unread badge and source preview before revocation clears private data', async ({ page, saved, playwright }) => {
  const team = await privateTeam(saved); const member = await recipient(playwright.request, saved, team.id);
  try {
    const stream = page.waitForResponse(response => response.url() === `${saved.origin}${saved.path}/events` && response.request().method() === 'GET');
    await openRecipient(page, member.http, saved.workspace);
    const connected = await stream;
    expect(connected.status()).toBe(200); expect(await connected.headerValue('content-type')).toContain('text/event-stream');
    await expect(page.locator('.inbox-badge')).toHaveCount(0);
    // The stream is already connected and the App integration disables separate
    // inbox polling. These are actual API writes and ordinary worker delivery.
    const issue = await assignedIssue(saved, team.id, member.session.user.id);
    await expect(page.locator('.inbox-list')).toContainText(issue.title);
    await expect(page.locator('.inbox-badge')).toHaveText('1');
    const notification = (await inboxItems(member)).items.find(item => item.source.id === issue.id)!;
    expect(notification).toBeDefined();
    await row(page, notification).click();
    await expect(page.locator('.inbox-source')).toContainText('Current private inbox source body');
    await member.request(`/notifications/${notification.id}`, 'PATCH', { action: 'read' }, { 'If-Match': notification.etag });
    await expect(row(page, notification)).toHaveCount(0); await expect(page.locator('.inbox-badge')).toHaveCount(0);
    await page.getByRole('navigation', { name: 'Inbox filters' }).getByRole('button', { name: 'All', exact: true }).click();
    await expect(row(page, notification)).toBeVisible(); await row(page, notification).click();
    await expect(page.locator('.inbox-source')).toContainText(issue.title);
    const membership = await saved.request<TeamMember>(`/teams/${team.id}/members/${member.session.user.id}`);
    const removed = await saved.http.delete(`${saved.path}/teams/${team.id}/members/${member.session.user.id}`, { headers: {
      Origin: saved.origin, 'X-CSRF-Token': saved.session.csrf_token, 'If-Match': `"team-member:${team.id}:${member.session.user.id}:${membership.version}"`,
    } });
    expect(removed.status()).toBe(204);
    await expect(page.locator('.inbox-source-unavailable')).toBeVisible();
    for (const text of [issue.title, issue.key, 'Current private inbox source body']) await expect(page.locator('.inbox-screen')).not.toContainText(text);
    expect((await member.http.get(`${saved.path}/issues/${issue.id}`)).status()).toBe(404);
    test.info().annotations.push({ type: 'notification-proof', description: JSON.stringify({ canonical_recipient_stream: true, worker_delivery_updates_list_and_badge: true, remote_read_updates_list_and_badge: true, private_source_revocation_without_reload: true }) });
  } finally { await member.http.dispose(); }
});
