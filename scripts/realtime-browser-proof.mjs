// Explicit local proof for two independently owned API processes. The canonical
// Playwright configuration and its guarded 18081 acceptance runtime stay intact.
import { randomUUID } from 'node:crypto';
import { mkdir, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('..', import.meta.url));
const { chromium, firefox, webkit, expect: baseExpect } = await import(new URL('../web/node_modules/@playwright/test/index.mjs', import.meta.url));
const expect = baseExpect.configure({ timeout: 15_000 });
const nodeA = 'http://127.0.0.1:18082';
const nodeB = 'http://127.0.0.1:18083';
if (process.env.GITHUB_ACTIONS || process.env.OPENTRACK_REALTIME_PROOF_DISPOSABLE !== '1'
    || !process.env.OPENTRACK_SETUP_TOKEN || !process.env.OPENTRACK_REALTIME_PROOF_PASSWORD) {
  throw new Error('Use this local proof only with explicitly owned disposable API processes and private fixture credentials.');
}
const names = (process.env.OPENTRACK_REALTIME_PROOF_ENGINES ?? 'chromium,firefox,webkit').split(',');
const engines = { chromium, firefox, webkit };
if (!names.length || names.some(name => !Object.hasOwn(engines, name))) throw new Error('Unsupported local browser engine.');
const output = resolve(root, 'work/realtime-browser');
await mkdir(output, { recursive: true, mode: 0o700 });
const results = [];

async function json(context, origin, path, method = 'GET', data, session, extra = {}) {
  if (![nodeA, nodeB].includes(origin) || !path.startsWith('/api/v1/')) throw new Error('Proof requests remain inside the two owned API origins.');
  const reply = await context.request.fetch(origin + path, {
    method, data,
    headers: { Origin: origin, ...(session && method !== 'GET' ? { 'X-CSRF-Token': session.csrf_token } : {}), ...extra },
  });
  if (!reply.ok()) throw new Error(`Local proof request failed: ${method} ${path.split('?')[0]} HTTP ${reply.status()}`);
  return reply.json();
}

for (const name of names) {
  const browser = await engines[name].launch();
  const first = await browser.newContext({ viewport: { width: 1440, height: 1000 }, locale: 'en-US', serviceWorkers: 'block' });
  const second = await browser.newContext({ viewport: { width: 1440, height: 1000 }, locale: 'en-US', serviceWorkers: 'block' });
  const pages = [await first.newPage(), await second.newPage()];
  const errors = [];
  for (const context of [first, second]) await context.route('**/*', route => {
    if ([nodeA, nodeB].includes(new URL(route.request().url()).origin)) return route.continue();
    return route.abort('blockedbyclient');
  });
  for (const page of pages) page.on('pageerror', () => errors.push('unhandled browser error'));
  try {
    const login = { email: 'realtime-owner@example.test', password: process.env.OPENTRACK_REALTIME_PROOF_PASSWORD };
    const setup = (await json(first, nodeA, '/api/v1/setup/status')).setup_required;
    const sessionA = await json(first, nodeA, setup ? '/api/v1/setup' : '/api/v1/auth/login', 'POST', setup
      ? { ...login, setup_token: process.env.OPENTRACK_SETUP_TOKEN, display_name: 'Example Owner', workspace_name: 'Realtime proof', team_name: 'Example Team', team_key: 'RTP' }
      : login);
    const sessionB = await json(second, nodeB, '/api/v1/auth/login', 'POST', login);
    const workspace = sessionA.workspaces.find(item => item.name === 'Realtime proof');
    if (!workspace || !sessionB.workspaces.some(item => item.id === workspace.id)) throw new Error('The proof requires its own synthetic workspace on both nodes.');
    const prefix = `/api/v1/workspaces/${workspace.id}`;
    const bootstrap = await json(first, nodeA, `${prefix}/bootstrap`);
    if (typeof bootstrap.realtime_cursor !== 'string' || !bootstrap.realtime_cursor.startsWith('rt1.')) throw new Error('The owned API does not provide realtime bootstrap cursors.');
    const initial = `Saved ${name} issue ${randomUUID().slice(0, 8)}`;
    const item = await json(first, nodeA, `${prefix}/issues`, 'POST', {
      team_id: bootstrap.teams[0].id, title: initial,
      document: { schema_version: 1, body: { type: 'doc', content: [{ type: 'paragraph', content: [{ type: 'text', text: 'Saved description' }] }] } },
    }, sessionA, { 'Idempotency-Key': randomUUID() });
    const path = `/w/${workspace.id}/issues/${item.id}`;
    const streams = pages.map((page, index) => page.waitForResponse(response => response.status() === 200
      && response.url() === `${index ? nodeB : nodeA}${prefix}/events`));
    await Promise.all(pages.map((page, index) => page.goto((index ? nodeB : nodeA) + path)));
    await Promise.all(streams);
    for (const page of pages) await expect(page.getByRole('textbox', { name: 'Issue title', exact: true })).toHaveValue(initial);

    const draft = `Unsaved ${name} title`;
    await pages[0].getByRole('textbox', { name: 'Issue title', exact: true }).fill(draft);
    const remote = `Remote ${name} update`;
    await pages[1].getByRole('textbox', { name: 'Issue title', exact: true }).fill(remote);
    await pages[1].getByRole('button', { name: 'Save changes', exact: true }).click();
    await expect(pages[1].getByText('Changes saved.', { exact: true })).toBeVisible();
    await expect(pages[0].getByRole('heading', { name: 'This issue changed while you were editing.', exact: true })).toBeVisible();
    await expect(pages[0].locator('.conflict').getByText(remote, { exact: true })).toBeVisible();
    await expect(pages[0].getByRole('textbox', { name: 'Issue title', exact: true })).toHaveValue(draft);
    await pages[0].getByRole('button', { name: 'Keep my edits for review', exact: true }).click();
    await expect(pages[0].getByRole('button', { name: 'Save changes', exact: true })).toBeEnabled();

    await first.setOffline(true);
    await expect(pages[0].getByText(/Changes are read-only; your unsaved drafts are preserved/)).toBeVisible();
    const offlineDraft = `${draft} kept offline`;
    await pages[0].getByRole('textbox', { name: 'Issue title', exact: true }).fill(offlineDraft);
    await expect(pages[0].getByRole('button', { name: 'Save changes', exact: true })).toBeDisabled();
    const afterReconnect = `Remote ${name} update during disconnect`;
    await pages[1].getByRole('textbox', { name: 'Issue title', exact: true }).fill(afterReconnect);
    await pages[1].getByRole('button', { name: 'Save changes', exact: true }).click();
    await expect(pages[1].getByText('Changes saved.', { exact: true })).toBeVisible();
    await first.setOffline(false);
    await expect(pages[0].locator('.conflict').getByText(afterReconnect, { exact: true })).toBeVisible();
    await expect(pages[0].getByRole('textbox', { name: 'Issue title', exact: true })).toHaveValue(offlineDraft);
    await expect(pages[0].getByText(/Live updates are disconnected|You are offline/)).toHaveCount(0);
    const persisted = await json(second, nodeB, `${prefix}/issues/${item.id}`);
    if (persisted.title !== afterReconnect || errors.length) throw new Error('The local proof did not converge cleanly.');
    await pages[0].screenshot({ path: resolve(output, `${name}-draft-preserved.png`) });
    results.push({ engine: name, cross_node_invalidation: 'passed', offline_write_pause: 'passed', reconnect_draft_preservation: 'passed', unhandled_browser_errors: errors.length });
    console.log(`PASS ${name}: two-node invalidation, offline pause and replay preserve the local draft`);
  } finally {
    await Promise.all([first.close(), second.close()]);
    await browser.close();
  }
}
await writeFile(resolve(output, 'receipt.json'), JSON.stringify({ kind: 'explicit-local-owned-two-node-proof', canonical_ci_suite: false, origins: [nodeA, nodeB], results }, null, 2) + '\n', { mode: 0o600 });
