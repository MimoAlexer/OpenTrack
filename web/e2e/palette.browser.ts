import { randomUUID } from 'node:crypto';
import { test, expect } from './fixtures';

test('palette searches saved issues and reauthorizes recent titles with character shortcuts disabled', async ({ page, saved }) => {
  const preference = await saved.request<{ etag: string }>('/preferences');
  await saved.request('/preferences', 'PATCH', { shortcut_mode: 'disabled' }, { 'If-Match': preference.etag });
  const issue = await saved.create(`Palette search ${randomUUID()}`);
  await saved.open(page);
  await page.getByRole('button', { name: 'Search commands', exact: true }).click();
  const palette = page.getByRole('dialog', { name: 'Commands', exact: true });
  await palette.getByRole('combobox', { name: 'Search commands' }).fill(issue.title);
  await expect(palette.getByRole('option').filter({ hasText: issue.title })).toBeVisible();
  await page.keyboard.press('Enter');
  await expect(palette).toHaveCount(0);
  await expect(page.getByRole('textbox', { name: 'Issue title', exact: true })).toHaveValue(issue.title);
  await page.getByRole('button', { name: 'Close issue', exact: true }).click();
  const title = `Renamed palette issue ${randomUUID()}`;
  await saved.patch(await saved.issue(issue.id), { title });
  await page.getByRole('button', { name: 'Search commands', exact: true }).click();
  await expect(palette.getByRole('option').filter({ hasText: title })).toContainText('Recent issues');
  await expect(palette).not.toContainText(issue.title);
  await page.keyboard.press('Escape');
  await expect(page.getByRole('button', { name: 'Search commands', exact: true })).toBeFocused();
});

test('palette keeps focus and removes a saved result deleted before it is selected', async ({ page, saved }) => {
  const issue = await saved.create(`Palette unavailable ${randomUUID()}`);
  await saved.open(page);
  await page.getByRole('button', { name: 'Search commands', exact: true }).click();
  const palette = page.getByRole('dialog', { name: 'Commands', exact: true });
  const input = palette.getByRole('combobox', { name: 'Search commands' });
  await input.fill(issue.title);
  const result = palette.getByRole('option').filter({ hasText: issue.title });
  await expect(result).toBeVisible();
  const current = await saved.issue(issue.id);
  const deletion = await saved.http.delete(`${saved.path}/issues/${issue.id}`, {
    headers: { Origin: saved.origin, 'X-CSRF-Token': saved.session.csrf_token, 'If-Match': current.etag },
  });
  expect(deletion.status()).toBe(204);
  await page.keyboard.press('Enter');
  await expect(palette).toBeVisible();
  await expect(result).toHaveCount(0);
  await expect(palette.getByRole('alert')).toBeVisible();
  await expect(input).toBeFocused();
});
