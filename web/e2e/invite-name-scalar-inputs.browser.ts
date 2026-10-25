import { createHash, randomUUID } from 'node:crypto';
import { writeFileSync } from 'node:fs';
import { devices } from '@playwright/test';
import { test, expect } from './fixtures';
import type { Session } from '../src/types';
import type { Invitation, InvitationIssued, WorkspaceMember } from '../src/settings/types';

const digest = (name: string) => createHash('sha256').update(name).digest('hex');
function fact(name: string, value: object) { writeFileSync(test.info().outputPath(`${name}.json`), JSON.stringify(value)); }

test('new invited account preserves 100 name scalars, rejects 101 without consuming membership and repairs once', async ({ browser, browserName, saved }) => {
  const email = `invited-scalar-${randomUUID()}@example.test`;
  const issued = await saved.request<InvitationIssued>('/invitations', 'POST', { email, role: 'member', team_ids: [] }, { 'Idempotency-Key': randomUUID() }); expect(Boolean(issued.token)).toBe(true); const token = issued.token!;
  const memberNames = async () => (await saved.request<{ data: WorkspaceMember[] }>('/members?state=active&limit=50')).data.filter(item => item.user.email === email).map(item => item.user.display_name);
  const pendingCount = async () => (await saved.request<{ data: Invitation[] }>('/invitations?state=pending&limit=50')).data.filter(item => item.id === issued.invitation.id).length;
  const acceptedCount = async () => (await saved.request<{ data: Invitation[] }>('/invitations?state=all&limit=50')).data.filter(item => item.id === issued.invitation.id && !!item.accepted_at).length;
  expect((await memberNames()).length).toBe(0); expect(await pendingCount()).toBe(1);
  const profile = { chromium: 'Desktop Chrome', firefox: 'Desktop Firefox', webkit: 'Desktop Safari' } as const; const canonical = test.info().project.use;
  const anonymous = await browser.newContext({ ...devices[profile[browserName]], baseURL: saved.origin, storageState: { cookies: [], origins: [] }, locale: canonical.locale, timezoneId: canonical.timezoneId, serviceWorkers: canonical.serviceWorkers });
  anonymous.setDefaultTimeout(canonical.actionTimeout ?? 0); anonymous.setDefaultNavigationTimeout(canonical.navigationTimeout ?? 0);
  await anonymous.route('**/*', async route => { if (new URL(route.request().url()).origin === new URL(saved.origin).origin) await route.continue(); else await route.abort('blockedbyclient'); });
  try {
    expect((await anonymous.cookies()).length).toBe(0); const page = await anonymous.newPage(); await page.goto('/join'); await page.getByRole('textbox', { name: 'Invitation token', exact: true }).fill(token); await page.getByRole('button', { name: 'Review invitation', exact: true }).click(); await page.getByRole('button', { name: 'Create an account', exact: true }).click();
    const input = page.getByRole('textbox', { name: 'Your name', exact: true }); const name = '😀'.repeat(100); await input.pressSequentially(name); const actual = await input.inputValue();
    fact('native-name-input', { engine: browserName, expected_scalars: 100, actual_scalars: Array.from(actual).length, expected_utf16: name.length, actual_utf16: actual.length, expected_sha256: digest(name), actual_sha256: digest(actual), member_count: (await memberNames()).length, pending_count: await pendingCount() });
    expect.soft(actual, 'Native sequential invitation name input preserves every supplementary scalar').toBe(name); if (actual !== name) return;
    await page.locator('input[name="password"]').fill(process.env.OPENTRACK_BROWSER_PASSWORD!); await input.fill(''); await input.pressSequentially('😀'.repeat(101));
    const rejecting = page.waitForResponse(response => new URL(response.url()).pathname === '/api/v1/invitations/accept' && response.request().method() === 'POST'); await page.getByRole('button', { name: 'Create account and join', exact: true }).click(); const rejected = await rejecting;
    expect(rejected.status()).toBe(422); expect(rejected.request().postDataJSON().display_name).toBe('😀'.repeat(101)); await expect(page.getByRole('alert')).toContainText('Name must contain'); await expect(input).toHaveValue('😀'.repeat(101)); expect((await memberNames()).length).toBe(0); expect(await pendingCount()).toBe(1); expect(await acceptedCount()).toBe(0);
    const stillUsable = await anonymous.request.post('/api/v1/invitations/preview', { data: { token }, headers: { Origin: saved.origin } }); expect(stillUsable.status()).toBe(200); const unsigned = await anonymous.request.get('/api/v1/auth/session'); expect(unsigned.status()).toBe(401);
    await input.fill(''); await input.pressSequentially(name); const accepting = page.waitForResponse(response => new URL(response.url()).pathname === '/api/v1/invitations/accept' && response.request().method() === 'POST'); await page.getByRole('button', { name: 'Create account and join', exact: true }).click(); const accepted = await accepting;
    expect(accepted.status()).toBe(200); expect(accepted.request().postDataJSON().display_name).toBe(name); expect((await accepted.json() as Session).user.display_name).toBe(name); await expect(page).toHaveURL(`${saved.origin}/w/${saved.workspace}/issues`);
    expect(await memberNames()).toEqual([name]); expect(await pendingCount()).toBe(0); expect(await acceptedCount()).toBe(1);
    await page.reload(); const sessionResponse = await anonymous.request.get('/api/v1/auth/session'); expect(sessionResponse.status()).toBe(200); const session = await sessionResponse.json() as Session; expect(session.user.display_name).toBe(name); expect(await memberNames()).toEqual([name]);
    const replay = await anonymous.request.post('/api/v1/invitations/accept', { data: { token }, headers: { Origin: saved.origin, 'X-CSRF-Token': session.csrf_token } }); expect(replay.status()).toBe(404); expect(await memberNames()).toEqual([name]); expect(await acceptedCount()).toBe(1);
    const ownerResponse = await saved.http.get('/api/v1/auth/session'); expect(ownerResponse.status()).toBe(200); expect((await ownerResponse.json() as Session).user.display_name).toBe(saved.session.user.display_name);
    fact('acceptance-name-proof', { engine: browserName, rejected_status: rejected.status(), invitation_after_reject_status: stillUsable.status(), anonymous_after_reject_status: unsigned.status(), accepted_status: accepted.status(), session_reload_status: sessionResponse.status(), replay_status: replay.status(), owner_status: ownerResponse.status(), name_scalars: Array.from(session.user.display_name).length, name_sha256: digest(session.user.display_name), member_count: (await memberNames()).length, pending_count: await pendingCount(), accepted_count: await acceptedCount() });
  } finally { await anonymous.close(); }
});
