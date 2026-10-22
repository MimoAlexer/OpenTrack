import { randomUUID } from 'node:crypto';
import { test, expect } from './fixtures';
import type { Issue } from '../src/types';

for (const resolution of ['keep', 'latest'] as const) {
  test(`competing issue editors preserve the rejected draft and ${resolution === 'keep' ? 'require an explicit reviewed save' : 'can adopt the saved version'}`, async ({ page, context, saved }) => {
    const issue = await saved.create(`Concurrent edits ${randomUUID()}`);
    const other = await context.newPage();
    try {
      await saved.open(page, issue); await saved.open(other, issue);
      const firstTitle = `First draft ${randomUUID()}`;
      const secondTitle = `Second draft ${randomUUID()}`;
      await page.getByRole('textbox', { name: 'Issue title', exact: true }).fill(firstTitle);
      await page.getByRole('combobox', { name: 'Priority', exact: true }).selectOption('1');
      await other.getByRole('textbox', { name: 'Issue title', exact: true }).fill(secondTitle);
      await other.getByRole('spinbutton', { name: 'Estimate', exact: true }).fill('8');
      const endpoint = `${saved.origin}${saved.path}/issues/${issue.id}`;
      const firstResponse = page.waitForResponse(response => response.url() === endpoint && response.request().method() === 'PATCH');
      const secondResponse = other.waitForResponse(response => response.url() === endpoint && response.request().method() === 'PATCH');
      await Promise.all([
        page.getByRole('button', { name: 'Save changes', exact: true }).click(),
        other.getByRole('button', { name: 'Save changes', exact: true }).click(),
      ]);
      const replies = await Promise.all([firstResponse, secondResponse]);
      expect(replies.map(response => response.status()).sort()).toEqual([200, 412]);
      const firstWon = replies[0].status() === 200;
      const winner = await replies[firstWon ? 0 : 1].json() as Issue;
      const rejected = firstWon ? other : page;
      const localTitle = firstWon ? secondTitle : firstTitle;
      const persisted = await saved.issue(issue.id);
      expect(persisted.title).toBe(firstWon ? firstTitle : secondTitle);
      expect(persisted.version).toBe((BigInt(issue.version) + 1n).toString());
      expect(persisted.priority).toBe(firstWon ? 1 : issue.priority);
      expect(persisted.estimate).toBe(firstWon ? issue.estimate : 8);
      const conflict = rejected.getByRole('alert').filter({ hasText: 'This issue changed while you were editing.' });
      await expect(conflict).toBeVisible();
      await expect(conflict).toContainText(winner.title);
      await expect(conflict).toContainText(localTitle);
      await expect(rejected.getByRole('textbox', { name: 'Issue title', exact: true })).toHaveValue(localTitle);
      await expect(rejected.getByRole('button', { name: 'Save changes', exact: true })).toBeDisabled();
      await conflict.getByRole('button', { name: resolution === 'keep' ? 'Keep my edits for review' : 'Use latest version', exact: true }).click();
      await expect(conflict).toHaveCount(0);
      expect((await saved.issue(issue.id)).version).toBe(winner.version);
      if (resolution === 'keep') {
        await expect(rejected.getByRole('textbox', { name: 'Issue title', exact: true })).toHaveValue(localTitle);
        await expect(rejected.getByRole('combobox', { name: 'Priority', exact: true })).toHaveValue('1');
        await expect(rejected.getByRole('spinbutton', { name: 'Estimate', exact: true })).toHaveValue('8');
        await rejected.getByRole('button', { name: 'Save changes', exact: true }).click();
        await expect.poll(async () => (await saved.issue(issue.id)).version).toBe((BigInt(winner.version) + 1n).toString());
      } else {
        await expect(rejected.getByRole('textbox', { name: 'Issue title', exact: true })).toHaveValue(winner.title);
        await expect(rejected.getByRole('combobox', { name: 'Priority', exact: true })).toHaveValue(String(winner.priority));
        await expect(rejected.getByRole('spinbutton', { name: 'Estimate', exact: true })).toHaveValue(winner.estimate === null ? '' : String(winner.estimate));
        await expect(rejected.getByRole('button', { name: 'Save changes', exact: true })).toBeDisabled();
      }
      const final = await saved.issue(issue.id);
      expect(final.title).toBe(resolution === 'keep' ? localTitle : winner.title);
      expect(final.priority).toBe(resolution === 'keep' ? 1 : winner.priority);
      expect(final.estimate).toBe(resolution === 'keep' ? 8 : winner.estimate);
      for (const client of [page, other]) {
        await client.reload();
        await expect(client.getByRole('textbox', { name: 'Issue title', exact: true })).toHaveValue(final.title);
        await expect(client.getByRole('combobox', { name: 'Priority', exact: true })).toHaveValue(String(final.priority));
        await expect(client.getByRole('spinbutton', { name: 'Estimate', exact: true })).toHaveValue(final.estimate === null ? '' : String(final.estimate));
      }
    } finally { await other.close(); }
  });
}
