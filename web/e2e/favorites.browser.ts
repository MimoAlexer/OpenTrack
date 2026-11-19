import { randomUUID } from 'node:crypto';
import type { Locator, Page } from '@playwright/test';
import { test, expect, paragraph, pollingFallbackForHeldHttp, registerAdmission, SavedWorkspace } from './fixtures';
import { parseStructuredHash } from '../src/filters/model';
import type { components } from '../src/generated/api';
import type { Bootstrap, Session } from '../src/types';

type Saved = components['schemas']['SavedDiscovery'];
type Definition = components['schemas']['DiscoveryDefinition'];
const resource = (kind: Saved['kind']) => kind === 'view' ? 'views' : 'saved-searches';
const priority = { kind: 'predicate', field: 'priority', op: 'eq', value: 1 } as const;
const definition = (team: string, q?: string): Definition => ({ query: { version: 1, filter: priority, team_id: team, sort: 'created', archived: false, limit: 50, timezone: 'UTC', ...(q ? { q } : {}) }, layout: 'list', group_by: 'status', columns: ['key', 'title'] });
const href = (saved: SavedWorkspace, item: Saved) => `/w/${saved.workspace}/issues?open_saved=${item.kind}:${item.id}`;
const favorites = (page: Page) => page.getByRole('navigation', { name: 'Favorites', exact: true }).filter({ visible: true });
const group = (page: Page, kind: Saved['kind']) => favorites(page).getByRole('region', { name: kind === 'view' ? 'Favorite views' : 'Favorite searches', exact: true });
registerAdmission(3);

async function create(saved: SavedWorkspace, kind: Saved['kind'], name: string, value = definition(saved.team.id)) {
  const item = await saved.request<Saved>(`/${resource(kind)}`, 'POST', { kind, name, visibility: 'personal', definition: value }, { 'Idempotency-Key': randomUUID() });
  return saved.request<Saved>(`/${resource(kind)}/${item.id}/favorite`, 'PUT', { favorite: true }, { 'Idempotency-Key': randomUUID() });
}
async function remove(saved: SavedWorkspace, item: Saved) {
  const latest = await saved.request<Saved>(`/${resource(item.kind)}/${item.id}`);
  const response = await saved.http.delete(`${saved.path}/${resource(item.kind)}/${item.id}`, { headers: { Origin: saved.origin, 'X-CSRF-Token': saved.session.csrf_token, 'If-Match': latest.etag, 'Idempotency-Key': randomUUID() } });
  expect(response.status()).toBe(204);
}
async function rename(saved: SavedWorkspace, item: Saved, name: string, value = item.definition!) {
  const current = await saved.request<Saved>(`/${resource(item.kind)}/${item.id}`);
  return saved.request<Saved>(`/${resource(item.kind)}/${item.id}`, 'PATCH', { kind: item.kind, name, visibility: current.visibility, team_id: current.team_id, definition: value }, { 'If-Match': current.etag, 'Idempotency-Key': randomUUID() });
}
async function keyboardOpen(link: Locator) { await link.focus(); await expect(link).toBeFocused(); await link.press('Enter'); }
function queryReads(page: Page, saved: SavedWorkspace) {
  const reads: string[] = [];
  page.on('request', request => {
    const url = new URL(request.url());
    if ([`${saved.path}/issues`, `${saved.path}/issues/query`, `${saved.path}/search`, `${saved.path}/search/query`].includes(url.pathname)) reads.push(url.pathname);
  });
  return reads;
}
async function assertBlocked(page: Page, reads: string[]) {
  await expect(page.getByRole('button', { name: 'Open all issues', exact: true })).toBeVisible();
  await page.evaluate(() => new Promise<void>(resolve => requestAnimationFrame(() => requestAnimationFrame(() => resolve()))));
  expect(reads).toEqual([]);
  await expect(page.locator('.issue-row')).toHaveCount(0);
  await expect(page.locator('.discovery-results')).toHaveCount(0);
}

test('favorite keyboard links reopen durable filters and body search with stable UUIDs after rename', async ({ page, saved }) => {
  const token = `favorite${randomUUID().replaceAll('-', '')}`;
  const issue = await saved.create(`Favorite body ${randomUUID()}`, paragraph(token));
  await saved.patch(issue, { priority: 1 });
  const excluded = await saved.create(`Excluded priority ${randomUUID()}`, paragraph(token));
  await saved.patch(excluded, { priority: 2 });
  const view = await create(saved, 'view', `View ${randomUUID()}`);
  const search = await create(saved, 'search', `Search ${randomUUID()}`, definition(saved.team.id, token));
  try {
    await saved.open(page);
    const viewLink = group(page, 'view').getByRole('link').filter({ has: page.getByText(view.name, { exact: true }) });
    await expect(viewLink).toHaveAttribute('href', href(saved, view));
    await keyboardOpen(viewLink);
    await expect(page.getByLabel('Definition name', { exact: true })).toHaveValue(view.name);
    await expect.poll(() => { const result = parseStructuredHash(new URL(page.url()).hash); return result.kind === 'valid' ? result.view.filter : null; }).toEqual(priority);
    await expect(page.locator('.issue-row').filter({ hasText: issue.title })).toBeVisible();
    await expect(page.locator('.issue-row').filter({ hasText: excluded.title })).toHaveCount(0);
    const searchLink = group(page, 'search').getByRole('link').filter({ has: page.getByText(search.name, { exact: true }) });
    await expect(searchLink).toHaveAttribute('href', href(saved, search)); await keyboardOpen(searchLink);
    await expect(page.getByRole('searchbox', { name: 'Search issues and projects', exact: true })).toHaveValue(token);
    await expect(page.locator('.discovery-results').getByRole('link')).toHaveText([`${issue.key} · ${issue.title}`]);
    const searchDirectory = page.getByRole('region', { name: 'Saved searches', exact: true });
    await searchDirectory.getByRole('button', { name: `Unfavorite ${search.name}`, exact: true }).click();
    await expect(group(page, 'search').getByRole('link').filter({ has: page.getByText(search.name, { exact: true }) })).toHaveCount(0);
    await searchDirectory.getByRole('button', { name: `Favorite ${search.name}`, exact: true }).click();
    await expect(group(page, 'search').getByRole('link').filter({ has: page.getByText(search.name, { exact: true }) })).toHaveAttribute('href', href(saved, search));
    await page.getByLabel('Definition name', { exact: true }).fill(`${search.name} renamed`);
    await page.getByRole('button', { name: 'Save changes', exact: true }).click();
    await expect(group(page, 'search').getByRole('link').filter({ has: page.getByText(`${search.name} renamed`, { exact: true }) })).toHaveAttribute('href', href(saved, search));
    const currentFilter = { ...priority, value: 2 };
    const currentView = await rename(saved, view, `${view.name} current`, { ...view.definition!, query: { ...view.definition!.query, filter: currentFilter } });
    await expect(group(page, 'view').getByRole('link').filter({ has: page.getByText(currentView.name, { exact: true }) })).toHaveAttribute('href', href(saved, view));
    await page.reload();
    await expect(group(page, 'view').getByRole('link').filter({ has: page.getByText(currentView.name, { exact: true }) })).toHaveAttribute('href', href(saved, view));
    await keyboardOpen(group(page, 'view').getByRole('link').filter({ has: page.getByText(currentView.name, { exact: true }) }));
    await expect.poll(() => { const result = parseStructuredHash(new URL(page.url()).hash); return result.kind === 'valid' ? result.view.filter : null; }).toEqual(currentFilter);
    await expect(page.locator('.issue-row').filter({ hasText: excluded.title })).toBeVisible();
    await expect(page.locator('.issue-row').filter({ hasText: issue.title })).toHaveCount(0);
    await keyboardOpen(group(page, 'search').getByRole('link').filter({ has: page.getByText(`${search.name} renamed`, { exact: true }) }));
    await expect(page.getByLabel('Definition name', { exact: true })).toHaveValue(`${search.name} renamed`);
    await expect(page.locator('.discovery-results').getByRole('link')).toHaveText([`${issue.key} · ${issue.title}`]);
    expect((await saved.request<Saved>(`/saved-searches/${search.id}`)).definition).toEqual(search.definition);
  } finally { await remove(saved, view); await remove(saved, search); }
});

test('private other-user favorites and malformed direct links never broaden issue or search queries', async ({ page, saved, playwright }) => {
  const invitation = await saved.request<{ token: string }>('/invitations', 'POST', { email: `favorite-private-${randomUUID()}@example.test`, role: 'member', team_ids: [saved.team.id] }, { 'Idempotency-Key': randomUUID() });
  const member = await playwright.request.newContext({ baseURL: saved.origin, extraHTTPHeaders: { Origin: saved.origin }, storageState: { cookies: [], origins: [] } });
  try {
    const accepted = await member.post('/api/v1/invitations/accept', { data: { token: invitation.token, display_name: 'Example Member', password: process.env.OPENTRACK_BROWSER_PASSWORD } });
    expect(accepted.status()).toBe(200); const session = await accepted.json() as Session;
    const name = `Private favorite ${randomUUID()}`;
    const response = await member.post(`${saved.path}/saved-searches`, { data: { name, kind: 'search', visibility: 'personal', definition: definition(saved.team.id, 'private') }, headers: { 'X-CSRF-Token': session.csrf_token, 'Idempotency-Key': randomUUID() } });
    expect(response.status()).toBe(201); const item = await response.json() as Saved;
    expect((await member.put(`${saved.path}/saved-searches/${item.id}/favorite`, { data: { favorite: true }, headers: { 'X-CSRF-Token': session.csrf_token, 'Idempotency-Key': randomUUID() } })).status()).toBe(200);
    const ownerFavorites = page.waitForResponse(value => value.request().method() === 'GET'
      && new URL(value.url()).pathname === `${saved.path}/saved-searches`
      && new URL(value.url()).searchParams.get('favorites') === 'true');
    await saved.open(page); expect((await ownerFavorites).status()).toBe(200);
    await expect(group(page, 'search')).toHaveAttribute('aria-busy', 'false');
    await expect(favorites(page)).not.toContainText(name);
    // Isolate request accounting from the preceding All issues document.
    const target = await page.context().newPage();
    try {
      const reads = queryReads(target, saved);
      const denied = target.waitForResponse(value => new URL(value.url()).pathname === `${saved.path}/saved-searches/${item.id}`);
      await target.goto(href(saved, item)); expect((await denied).status()).toBe(404);
      await expect(target.getByRole('alert').first()).toBeVisible(); await assertBlocked(target, reads);
      await expect(target.locator('.discovery')).not.toContainText(name);
      for (const invalid of ['view:not-a-uuid', `search:${item.id}:extra`]) {
        reads.length = 0;
        await target.goto(`/w/${saved.workspace}/issues?open_saved=${encodeURIComponent(invalid)}`);
        await expect(target.getByRole('alert').filter({ hasText: 'saved discovery link is invalid' })).toBeVisible();
        await assertBlocked(target, reads);
      }
      await target.getByRole('button', { name: 'Open all issues', exact: true }).click();
      await expect(target.getByRole('button', { name: /^New issue/ })).toBeEnabled();
      await expect.poll(() => reads.length).toBeGreaterThan(0);
    } finally { await target.close(); }
  } finally { await member.dispose(); }
});

test('held current favorite reads hide prior rows and deletion denial requires explicit exit', async ({ page, saved }) => {
  const current = await create(saved, 'view', `Held view ${randomUUID()}`);
  const marker = await saved.create(`Old row ${randomUUID()}`);
  await pollingFallbackForHeldHttp(page, saved); await saved.open(page);
  await expect(page.locator('.issue-row').filter({ hasText: marker.title })).toBeVisible();
  let release!: () => void; let finished!: () => void; let captured = false;
  const gate = new Promise<void>(resolve => { release = resolve; });
  const settled = new Promise<void>(resolve => { finished = resolve; });
  await page.route(`${saved.origin}${saved.path}/views/${current.id}`, async route => {
    const response = await route.fetch({ maxRetries: 0, maxRedirects: 0 });
    try { expect(response.status()).toBe(200); captured = true; await gate; try { await route.fulfill({ response }); } catch { /* An explicit exit may abort this held read. */ } }
    finally { await response.dispose(); finished(); }
  }, { times: 1 });
  const reads = queryReads(page, saved);
  try {
    await keyboardOpen(group(page, 'view').getByRole('link').filter({ has: page.getByText(current.name, { exact: true }) })); await expect.poll(() => captured).toBe(true);
    await expect(page.getByRole('status').filter({ hasText: 'Checking the current saved definition' })).toBeVisible();
    await assertBlocked(page, reads); await expect(page.locator('.issue-row').filter({ hasText: marker.title })).toHaveCount(0);
    release(); await settled;
    await expect(page.getByLabel('Definition name', { exact: true })).toHaveValue(current.name);
    await expect.poll(() => reads.length).toBeGreaterThan(0);
    // Drop live invalidation only for the separate explicit current-authority read.
    await page.route('**/events', route => route.abort('connectionfailed'));
    await remove(saved, current);
    reads.length = 0;
    const denial = page.waitForResponse(response => new URL(response.url()).pathname === `${saved.path}/views/${current.id}` && response.request().method() === 'GET');
    await keyboardOpen(group(page, 'view').getByRole('link').filter({ has: page.getByText(current.name, { exact: true }) }));
    expect((await denial).status()).toBe(404);
    await expect(page.getByRole('alert').first()).toBeVisible(); await assertBlocked(page, reads);
    await expect(group(page, 'view').getByRole('link').filter({ has: page.getByText(current.name, { exact: true }) })).toHaveCount(0);
    const retry = page.waitForResponse(response => new URL(response.url()).pathname === `${saved.path}/views/${current.id}` && response.request().method() === 'GET');
    await page.getByRole('button', { name: 'Try again', exact: true }).first().click(); expect((await retry).status()).toBe(404);
    await expect(page.getByRole('alert').first()).toBeVisible(); await assertBlocked(page, reads);
  } finally {
    release(); if (captured) await settled;
    const remaining = await saved.http.get(`${saved.path}/views/${current.id}`);
    if (remaining.status() === 200) await remove(saved, current); else expect(remaining.status()).toBe(404);
  }
});

test('favorite pagination shares state across desktop and the 390px navigation drawer with realtime metadata', async ({ page, saved: owner, playwright }) => {
  const invitation = await owner.request<{ token: string }>('/invitations', 'POST', { email: `favorite-paging-${randomUUID()}@example.test`, role: 'member', team_ids: [owner.team.id] }, { 'Idempotency-Key': randomUUID() });
  const member = await playwright.request.newContext({ baseURL: owner.origin, extraHTTPHeaders: { Origin: owner.origin }, storageState: { cookies: [], origins: [] } });
  const accepted = await member.post('/api/v1/invitations/accept', { data: { token: invitation.token, display_name: 'Example Navigation Member', password: process.env.OPENTRACK_BROWSER_PASSWORD } });
  expect(accepted.status()).toBe(200); const session = await accepted.json() as Session;
  const bootstrap = await member.get(`${owner.path}/bootstrap`); expect(bootstrap.status()).toBe(200);
  const saved = new SavedWorkspace(member, session, await bootstrap.json() as Bootstrap, owner.origin, owner.workspace);
  await page.context().clearCookies(); await page.context().addCookies((await member.storageState()).cookies);
  const items: Saved[] = [];
  for (const kind of ['view', 'search'] as const) for (let index = 0; index < 11; index += 1) items.push(await create(saved, kind, `${kind} ${randomUUID()}`, definition(saved.team.id, kind === 'search' ? 'navigation' : undefined)));
  try {
    const reads: URL[] = [];
    page.on('request', request => { const url = new URL(request.url()); if (request.method() === 'GET' && [`${saved.path}/views`, `${saved.path}/saved-searches`].includes(url.pathname) && url.searchParams.get('favorites') === 'true') reads.push(url); });
    await saved.open(page);
    for (const kind of ['view', 'search'] as const) {
      await expect(group(page, kind).getByRole('link')).toHaveCount(10);
      await group(page, kind).getByRole('button', { name: `Next favorite ${kind === 'view' ? 'views' : 'searches'}`, exact: true }).click();
      await expect(group(page, kind).getByRole('link')).toHaveCount(1);
    }
    expect(reads.every(url => url.searchParams.get('limit') === '10')).toBe(true);
    const beforeDrawer = reads.length;
    await page.setViewportSize({ width: 390, height: 844 });
    await page.getByRole('button', { name: 'Open navigation', exact: true }).click();
    for (const kind of ['view', 'search'] as const) await expect(group(page, kind).getByRole('link')).toHaveCount(1);
    expect(reads.length).toBe(beforeDrawer);
    const visibleView = await group(page, 'view').getByRole('link').first().getAttribute('href');
    const item = items.find(candidate => href(saved, candidate) === visibleView)!; const changed = await rename(saved, item, `${item.name} remote`);
    await expect(group(page, 'view').getByRole('link').filter({ has: page.getByText(changed.name, { exact: true }) })).toHaveAttribute('href', href(saved, item));
    await saved.request(`/views/${item.id}/favorite`, 'PUT', { favorite: false }, { 'Idempotency-Key': randomUUID() });
    await expect(group(page, 'view').getByRole('link').filter({ has: page.getByText(changed.name, { exact: true }) })).toHaveCount(0);
    const target = group(page, 'search').getByRole('link').first(); const destination = await target.getAttribute('href');
    await keyboardOpen(target);
    await expect(page.getByRole('button', { name: 'Open navigation', exact: true })).toBeVisible();
    await expect(page.getByLabel('Definition name', { exact: true })).toBeVisible();
    const selected = items.find(candidate => href(saved, candidate) === destination)!;
    await expect(page.getByLabel('Definition name', { exact: true })).toHaveValue(selected.name);
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  } finally { for (const item of items) await remove(saved, item); await member.dispose(); }
});

test('a favorite with redacted shared filters stays blocked until explicit reviewed replacement', async ({ page, saved, playwright }) => {
  const suffix = randomUUID();
  const team = await saved.request<{ id: string }>('/teams', 'POST', { name: `Hidden favorite team ${suffix}`, key: `F${suffix.replaceAll('-', '').slice(0, 8).toUpperCase()}`, private: true }, { 'Idempotency-Key': randomUUID() });
  const hidden = await saved.request<{ id: string; title: string }>('/issues', 'POST', { team_id: team.id, title: `Hidden favorite issue ${suffix}`, document: { schema_version: 1, body: paragraph('Private saved work') } }, { 'Idempotency-Key': randomUUID() });
  const item = await saved.request<Saved>('/views', 'POST', { name: `Repair favorite ${suffix}`, kind: 'view', visibility: 'workspace', definition: { ...definition(saved.team.id), query: { version: 1, filter: { kind: 'predicate', field: 'team', op: 'eq', value: team.id } } } }, { 'Idempotency-Key': randomUUID() });
  const invitation = await saved.request<{ token: string }>('/invitations', 'POST', { email: `favorite-repair-${suffix}@example.test`, role: 'admin' }, { 'Idempotency-Key': randomUUID() });
  const admin = await playwright.request.newContext({ baseURL: saved.origin, extraHTTPHeaders: { Origin: saved.origin }, storageState: { cookies: [], origins: [] } });
  try {
    const accepted = await admin.post('/api/v1/invitations/accept', { data: { token: invitation.token, display_name: 'Example Repair Administrator', password: process.env.OPENTRACK_BROWSER_PASSWORD } });
    expect(accepted.status()).toBe(200); const session = await accepted.json() as Session;
    const starred = await admin.put(`${saved.path}/views/${item.id}/favorite`, { data: { favorite: true }, headers: { 'X-CSRF-Token': session.csrf_token, 'Idempotency-Key': randomUUID() } });
    expect(starred.status()).toBe(200);
    const redacted = await starred.json() as Saved; expect(redacted.definition).toBeNull(); expect(redacted.repair_required).toBeTruthy(); expect(redacted.can_edit).toBe(true);
    expect((await admin.get(`${saved.path}/issues/${hidden.id}`)).status()).toBe(404);
    await page.context().clearCookies(); await page.context().addCookies((await admin.storageState()).cookies);
    const reads = queryReads(page, saved);
    const detail = page.waitForResponse(response => new URL(response.url()).pathname === `${saved.path}/views/${item.id}` && response.request().method() === 'GET');
    await page.goto(href(saved, item)); expect((await detail).status()).toBe(200);
    await expect(page.getByRole('button', { name: 'Review replacement filters', exact: true })).toBeVisible();
    await assertBlocked(page, reads);
    await expect(page.getByRole('button', { name: 'Save changes', exact: true })).toBeDisabled();
    await expect(page.locator('.discovery')).not.toContainText(team.id); await expect(page.locator('.discovery')).not.toContainText(`Hidden favorite team ${suffix}`);
    await page.getByRole('button', { name: 'Review replacement filters', exact: true }).click();
    const review = page.getByRole('region', { name: 'Review replacement filters', exact: true });
    const replace = review.getByRole('button', { name: 'Replace saved definition with reviewed filters', exact: true });
    await expect(replace).toBeDisabled(); await review.getByRole('checkbox').check();
    const committed = page.waitForResponse(response => new URL(response.url()).pathname === `${saved.path}/views/${item.id}` && response.request().method() === 'PATCH');
    await replace.click(); expect((await committed).status()).toBe(200);
    await expect(page.getByRole('button', { name: 'Open all issues', exact: true })).toHaveCount(0);
    await expect.poll(() => reads.length).toBeGreaterThan(0);
    await expect(page.locator('.issue-row').filter({ hasText: hidden.title })).toHaveCount(0);
    await expect(group(page, 'view').getByRole('link').filter({ has: page.getByText(item.name, { exact: true }) })).toHaveAttribute('href', href(saved, item));
    const repaired = await saved.request<Saved>(`/views/${item.id}`); expect(repaired.definition!.query.filter).toBeNull(); expect(repaired.id).toBe(item.id);
  } finally { await admin.dispose(); await remove(saved, item); }
});
