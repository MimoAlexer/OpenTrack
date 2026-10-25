import { createHash } from 'node:crypto';
import { writeFileSync } from 'node:fs';
import { test, expect } from '@playwright/test';
import type { Bootstrap, Session } from '../src/types';

const names = { display_name: '😀'.repeat(100), workspace_name: '🧭'.repeat(100), team_name: '🚀'.repeat(100) };
const fields = Object.keys(names) as (keyof typeof names)[];
const hash = (name: string) => createHash('sha256').update(name).digest('hex');
function fact(name: string, value: object) { writeFileSync(test.info().outputPath(`${name}.json`), JSON.stringify(value)); }
function selectedNames(body: Record<string, unknown>) { return { display_name: body.display_name, workspace_name: body.workspace_name, team_name: body.team_name }; }

test.beforeAll(({ browserName }) => {
  if (process.env.OPENTRACK_BROWSER_SETUP_MODE !== 'dedicated-disposable-startup'
      || process.env.OPENTRACK_BROWSER_SETUP_ENGINE !== browserName
      || !['baseline', 'corrected'].includes(process.env.OPENTRACK_BROWSER_SETUP_PHASE ?? '')
      || !process.env.OPENTRACK_BROWSER_SETUP_TOKEN || !process.env.OPENTRACK_BROWSER_PASSWORD) {
    throw new Error('First-setup acceptance requires its dedicated new empty fixture and authority.');
  }
});
test.beforeEach(async ({ context, baseURL }) => {
  const origin = new URL(baseURL!);
  if (origin.protocol !== 'http:' || !['localhost', '127.0.0.1'].includes(origin.hostname) || origin.port !== '18081' || origin.pathname !== '/' || origin.username || origin.password || origin.search || origin.hash) throw new Error('Dedicated setup requires the canonical loopback origin.');
  await context.route('**/*', async route => { if (new URL(route.request().url()).origin === origin.origin) await route.continue(); else await route.abort('blockedbyclient'); });
  expect((await context.cookies()).length).toBe(0);
});

test('first setup preserves all three 100-scalar names, rejects independent 101 drafts and creates one owner', async ({ page, browserName, baseURL }) => {
  const status = await page.request.get('/api/v1/setup/status'); expect(status.status()).toBe(200); expect((await status.json() as { setup_required: boolean }).setup_required).toBe(true);
  await page.goto('/'); const native: object[] = []; let preserved = true;
  for (const field of fields) {
    const input = page.locator(`input[name="${field}"]`); await input.pressSequentially(names[field]); const actual = await input.inputValue();
    native.push({ field, expected_scalars: 100, actual_scalars: Array.from(actual).length, expected_sha256: hash(names[field]), actual_sha256: hash(actual) });
    expect.soft(actual, 'Native first-setup name input preserves every supplementary scalar').toBe(names[field]); preserved &&= actual === names[field];
  }
  fact('native-setup-names', { engine: browserName, fields: native }); if (!preserved) return;
  const credentials = { setup_token: process.env.OPENTRACK_BROWSER_SETUP_TOKEN!, email: 'owner@example.test', password: process.env.OPENTRACK_BROWSER_PASSWORD!, team_key: 'SET' };
  for (const [field, value] of Object.entries(credentials)) await page.locator(`input[name="${field}"]`).fill(value);
  const rejections: number[] = []; const available: boolean[] = [];
  for (const field of fields) {
    const overlong = names[field] + '🌍'; const input = page.locator(`input[name="${field}"]`); await input.fill(''); await input.pressSequentially(overlong);
    const pending = page.waitForResponse(response => new URL(response.url()).pathname === '/api/v1/setup' && response.request().method() === 'POST'); await page.getByRole('button', { name: 'Create workspace', exact: true }).click(); const response = await pending;
    expect(response.status()).toBe(422); expect(selectedNames(response.request().postDataJSON())).toEqual({ ...names, [field]: overlong }); await expect(page.getByRole('alert')).toContainText('must contain');
    for (const key of fields) await expect(page.locator(`input[name="${key}"]`)).toHaveValue(key === field ? overlong : names[key]);
    const stillAvailable = await page.request.get('/api/v1/setup/status'); expect(stillAvailable.status()).toBe(200); const required = (await stillAvailable.json() as { setup_required: boolean }).setup_required; expect(required).toBe(true); rejections.push(response.status()); available.push(required);
    await input.fill(''); await input.pressSequentially(names[field]);
  }
  const pending = page.waitForResponse(response => new URL(response.url()).pathname === '/api/v1/setup' && response.request().method() === 'POST'); await page.getByRole('button', { name: 'Create workspace', exact: true }).click(); const accepted = await pending; expect(accepted.status()).toBe(201); expect(selectedNames(accepted.request().postDataJSON())).toEqual(names);
  const session = await accepted.json() as Session; expect(session.user.display_name).toBe(names.display_name); expect(session.workspaces.length).toBe(1); expect(session.workspaces[0].name).toBe(names.workspace_name); expect(session.workspaces[0].role).toBe('owner'); const workspace = session.workspaces[0].id;
  await page.goto(`/w/${workspace}/issues`); await expect(page.getByRole('button', { name: /^New issue/ })).toBeEnabled(); await page.reload();
  const currentResponse = await page.request.get('/api/v1/auth/session'); expect(currentResponse.status()).toBe(200); const current = await currentResponse.json() as Session; expect(current.user.display_name).toBe(names.display_name); expect(current.workspaces.map(item => item.name)).toEqual([names.workspace_name]);
  const bootstrapResponse = await page.request.get(`/api/v1/workspaces/${workspace}/bootstrap`); expect(bootstrapResponse.status()).toBe(200); const bootstrap = await bootstrapResponse.json() as Bootstrap; expect(bootstrap.teams.map(item => item.name)).toEqual([names.team_name]); expect(bootstrap.teams[0].key).toBe('SET'); expect(bootstrap.statuses.filter(item => item.is_default && !item.archived_at && !item.retiring_at).length).toBe(1);
  const second = await page.request.post('/api/v1/setup', { data: { ...credentials, ...names }, headers: { Origin: new URL(baseURL!).origin } }); expect(second.status()).toBe(409); const final = await page.request.get('/api/v1/setup/status'); expect((await final.json() as { setup_required: boolean }).setup_required).toBe(false);
  fact('setup-name-proof', { engine: browserName, rejected_statuses: rejections, setup_available_after_rejections: available, accepted_status: accepted.status(), session_status: currentResponse.status(), bootstrap_status: bootstrapResponse.status(), second_setup_status: second.status(), workspace_count: current.workspaces.length, team_count: bootstrap.teams.length, names: fields.map(field => ({ field, scalars: Array.from(names[field]).length, sha256: hash(names[field]) })) });
});
