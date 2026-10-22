import { randomUUID } from 'node:crypto';
import type { Locator, Page } from '@playwright/test';
import { test, expect } from './fixtures';
import type { Issue, Session } from '../src/types';
import type { InvitationIssued, TeamDetail, TeamMember } from '../src/settings/types';

async function tabTo(page: Page, target: Locator, direction = 'Tab') {
  for (let count = 0; count < 80; count++) {
    if (await target.evaluate(element => element === document.activeElement)) return;
    await page.keyboard.press(direction);
  }
  await expect(target).toBeFocused();
}

test('keyboard-only create, status, save, search and recent navigation work with character shortcuts disabled', async ({ page, saved }) => {
  const preference = await saved.request<{ etag: string }>('/preferences');
  await saved.request('/preferences', 'PATCH', { shortcut_mode: 'disabled' }, { 'If-Match': preference.etag });
  await saved.open(page);
  await page.keyboard.press('c'); await page.keyboard.press('g'); await page.keyboard.press('m');
  await expect(page.getByRole('dialog')).toHaveCount(0);
  await page.keyboard.press('Control+k');
  await expect(page.getByRole('combobox', { name: 'Search commands', exact: true })).toBeFocused();
  await page.keyboard.press('Enter');
  const dialog = page.getByRole('dialog', { name: 'New issue', exact: true });
  await expect(dialog.getByRole('textbox', { name: 'Title', exact: true })).toBeFocused();
  const title = `Keyboard core ${randomUUID()}`;
  await page.keyboard.insertText(title);
  const created = page.waitForResponse(response => response.request().method() === 'POST' && response.url() === `${saved.origin}${saved.path}/issues`);
  await page.keyboard.press('Control+Enter');
  const response = await created; expect(response.status()).toBe(201);
  const issue = await response.json() as Issue;
  const status = page.getByRole('combobox', { name: 'Status', exact: true });
  await expect(status).toBeVisible(); await tabTo(page, status);
  await page.keyboard.press('Home'); await page.keyboard.press('ArrowDown');
  const statusId = await status.inputValue();
  const priority = page.getByRole('combobox', { name: 'Priority', exact: true });
  await tabTo(page, priority); await page.keyboard.press('Home'); await page.keyboard.press('ArrowDown');
  await page.keyboard.press('Control+Enter');
  await expect.poll(async () => (await saved.issue(issue.id)).priority).toBe(1);
  expect((await saved.issue(issue.id)).status_id).toBe(statusId);
  await tabTo(page, page.getByRole('button', { name: 'Close issue', exact: true }), 'Shift+Tab'); await page.keyboard.press('Enter');
  await page.keyboard.press('Control+k'); await page.keyboard.insertText(title);
  const palette = page.getByRole('dialog', { name: 'Commands', exact: true });
  await expect(palette.getByRole('option').filter({ hasText: title })).toBeVisible();
  await page.keyboard.press('Enter');
  await expect(page.getByRole('textbox', { name: 'Issue title', exact: true })).toHaveValue(title);
  await page.keyboard.press('Control+k');
  await expect(palette.getByRole('option').filter({ hasText: title })).toContainText('Recent issues');
  await page.keyboard.press('Escape'); await expect(palette).toHaveCount(0);
});

test('picker ignores synthetic composition and modified text keys before ordinary keyboard selection', async ({ page, saved }) => {
  await saved.open(page); await page.keyboard.press('Control+k'); await page.keyboard.press('Enter');
  const dialog = page.getByRole('dialog', { name: 'New issue', exact: true });
  const assignee = dialog.getByRole('button', { name: /^Assignee/ });
  await tabTo(page, assignee); await page.keyboard.press('Enter');
  const search = dialog.getByRole('combobox', { name: 'Search assignee', exact: true });
  await expect(search).toBeFocused(); await expect(dialog.getByRole('option', { name: /Assign to me/ })).toBeVisible();
  await page.keyboard.press('ArrowDown');
  const active = await search.getAttribute('aria-activedescendant'); expect(active).toBeTruthy();
  await search.evaluate(element => {
    element.dispatchEvent(new CompositionEvent('compositionstart', { bubbles: true }));
    for (const key of ['End', 'ArrowDown', 'Enter', 'Escape']) element.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true }));
    element.dispatchEvent(new CompositionEvent('compositionend', { bubbles: true }));
    for (const options of [{ keyCode: 229 }, { isComposing: true }, { altKey: true }, { ctrlKey: true }, { metaKey: true }, { shiftKey: true }]) {
      for (const key of ['End', 'ArrowDown', 'Enter', 'Escape']) element.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true, ...options }));
    }
    const altGraph = new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true });
    Object.defineProperty(altGraph, 'getModifierState', { value: (name: string) => name === 'AltGraph' }); element.dispatchEvent(altGraph);
  });
  await expect(search).toBeFocused(); await expect(search).toHaveAttribute('aria-activedescendant', active!);
  await expect(assignee).toContainText('Unassigned'); await expect(dialog).toBeVisible();
  await page.keyboard.press('ArrowDown'); await page.keyboard.press('Enter');
  await expect(assignee).toBeFocused(); await expect(assignee).toContainText('Example Owner');
  await page.keyboard.press('Escape');
});

test('private-team revocation removes a real recent issue title and excludes it from search', async ({ page, saved, playwright }) => {
  const team = await saved.request<TeamDetail>('/teams', 'POST', { name: `Restricted keyboard ${randomUUID()}`, key: `K${randomUUID().replaceAll('-', '').slice(0, 8)}`, private: true }, { 'Idempotency-Key': randomUUID() });
  const issued = await saved.request<InvitationIssued>('/invitations', 'POST', { email: `keyboard-${randomUUID()}@example.test`, role: 'member', team_ids: [team.id] }, { 'Idempotency-Key': randomUUID() });
  const member = await playwright.request.newContext({ baseURL: saved.origin, extraHTTPHeaders: { Origin: saved.origin }, storageState: { cookies: [], origins: [] } });
  try {
    expect((await member.get('/api/v1/auth/session')).status()).toBe(401);
    const accepted = await member.post('/api/v1/invitations/accept', { data: { token: issued.token, display_name: 'Example Member', password: process.env.OPENTRACK_BROWSER_PASSWORD } });
    expect(accepted.status()).toBe(200);
    const session = await accepted.json() as Session;
    const issue = await saved.request<Issue>('/issues', 'POST', { team_id: team.id, title: `Restricted recent ${randomUUID()}` }, { 'Idempotency-Key': randomUUID() });
    await page.context().clearCookies(); await page.context().addCookies((await member.storageState()).cookies);
    await saved.open(page, issue); await page.keyboard.press('Control+k');
    const palette = page.getByRole('dialog', { name: 'Commands', exact: true });
    await expect(palette.getByRole('option').filter({ hasText: issue.title })).toContainText('Recent issues');
    const membership = await saved.request<TeamMember>(`/teams/${team.id}/members/${session.user.id}`);
    const removed = await saved.http.delete(`${saved.path}/teams/${team.id}/members/${session.user.id}`, { headers: { Origin: saved.origin, 'X-CSRF-Token': saved.session.csrf_token, 'If-Match': `"team-member:${team.id}:${session.user.id}:${membership.version}"` } });
    expect(removed.status()).toBe(204);
    expect((await member.get(`${saved.path}/issues/${issue.id}`)).status()).toBe(404);
    await expect(page.locator('body')).not.toContainText(issue.title);
    await expect(palette).toHaveCount(0);
    await page.keyboard.press('Control+k');
    await expect(palette.getByRole('combobox', { name: 'Search commands', exact: true })).toBeFocused();
    await expect(palette).not.toContainText(issue.title);
    await page.keyboard.insertText(issue.title);
    await expect(palette.getByRole('status').filter({ hasText: '0 issue results' })).toBeVisible();
    await expect(palette.getByRole('option').filter({ hasText: issue.title })).toHaveCount(0);
    const results = await member.get(`${saved.path}/issues?q=${encodeURIComponent(issue.title)}`);
    expect(results.status()).toBe(200); expect((await results.json() as { data: Issue[] }).data).toEqual([]);
  } finally { await member.dispose(); }
});
