import { registerAdmission } from './fixtures';
import { randomUUID } from 'node:crypto';
import type { Response } from '@playwright/test';
import { test, expect } from './fixtures';
import type { TeamDetail } from '../src/settings/types';
import type { Issue, IssuePage, Session } from '../src/types';
registerAdmission(1);

test('palette preserves full scalar queries, rejects 257 without fallback and resolves the authorized result before keyboard navigation', async ({ page, saved, playwright }) => {
  const query = [...randomUUID().replaceAll('-', '')].map(value => String.fromCodePoint(0x1F600 + Number.parseInt(value, 16))).join('') + '\u{1F680}'.repeat(224);
  const prefix = Array.from(query).slice(0, 60).join('');
  expect(Array.from(query)).toHaveLength(256); expect(query.length).toBe(512);
  const team = await saved.request<TeamDetail>('/teams', 'POST', {
    name: `Palette scalar acceptance ${randomUUID().slice(0, 8)}`,
    key: `Q${randomUUID().replaceAll('-', '').slice(0, 8).toUpperCase()}`, private: true,
  }, { 'Idempotency-Key': randomUUID() });
  const target = await saved.request<Issue>('/issues', 'POST', { team_id: team.id, title: query }, { 'Idempotency-Key': randomUUID() });
  const distractor = await saved.request<Issue>('/issues', 'POST', { team_id: team.id, title: prefix + ' unrelated partial result' }, { 'Idempotency-Key': randomUUID() });
  const invitation = await saved.request<{ token: string }>('/invitations', 'POST', {
    email: `palette-${randomUUID()}@example.test`, role: 'admin', team_ids: [],
  }, { 'Idempotency-Key': randomUUID() });
  const admin = await playwright.request.newContext({ baseURL: saved.origin, extraHTTPHeaders: { Origin: saved.origin }, storageState: { cookies: [], origins: [] } });
  try {
    const accepted = await admin.post('/api/v1/invitations/accept', { data: {
      token: invitation.token, display_name: 'Example Administrator', password: process.env.OPENTRACK_BROWSER_PASSWORD,
    } });
    expect(accepted.status()).toBe(200);
    const account = await accepted.json() as Session;
    expect(account.workspaces.find(workspace => workspace.id === saved.workspace)?.role).toBe('admin');
    const privateTeamResponse = await admin.post(`${saved.path}/teams`, {
      headers: { 'X-CSRF-Token': account.csrf_token, 'Idempotency-Key': randomUUID() },
      data: { name: 'Private palette negative control', key: `P${randomUUID().replaceAll('-', '').slice(0, 8).toUpperCase()}`, private: true },
    });
    expect(privateTeamResponse.status()).toBe(201);
    const privateTeam = await privateTeamResponse.json() as TeamDetail;
    const hiddenResponse = await admin.post(`${saved.path}/issues`, {
      headers: { 'X-CSRF-Token': account.csrf_token, 'Idempotency-Key': randomUUID() }, data: { team_id: privateTeam.id, title: query },
    });
    expect(hiddenResponse.status()).toBe(201);
    const hidden = await hiddenResponse.json() as Issue;
    const denied = await saved.http.get(`${saved.path}/issues/${hidden.id}`);
    expect(denied.status()).toBe(404);
    expect(await denied.json()).toMatchObject({ code: 'not_found' });
    const endpoint = `${saved.origin}${saved.path}/issues`;
    const searches: Response[] = []; const requested: string[] = []; const reads: Response[] = [];
    page.on('request', request => {
      const url = new URL(request.url());
      if (request.method() === 'GET' && url.pathname === `${saved.path}/issues` && url.searchParams.has('q')) requested.push(url.searchParams.get('q')!);
    });
    page.on('response', response => {
      const url = new URL(response.url());
      if (response.request().method() === 'GET' && url.pathname === `${saved.path}/issues` && url.searchParams.has('q')) searches.push(response);
      if (response.request().method() === 'GET' && response.url() === `${endpoint}/${target.id}`) reads.push(response);
    });
    await saved.open(page);
    await page.getByRole('button', { name: 'Search commands', exact: true }).click();
    const palette = page.getByRole('dialog', { name: 'Commands', exact: true });
    const input = palette.getByRole('combobox', { name: 'Search commands', exact: true });
    await expect(input).toBeFocused();
    await input.pressSequentially(query);
    const native = await input.inputValue();
    await expect.poll(() => searches.some(response => new URL(response.url()).searchParams.get('q') === native)).toBe(true);
    const boundaryResponse = searches.filter(response => new URL(response.url()).searchParams.get('q') === native).at(-1)!;
    expect(boundaryResponse.status()).toBe(200);
    const boundary = await boundaryResponse.json() as IssuePage;
    const boundaryQuery = new URL(boundaryResponse.url()).searchParams;
    await test.info().attach('native-boundary-evidence', { contentType: 'application/json', body: JSON.stringify({
      input_scalars: Array.from(native).length, input_utf16_units: native.length,
      requested_query_scalars: Array.from(boundaryQuery.get('q')!).length, limit: boundaryQuery.get('limit'),
      result_count: boundary.data.length, target_present: boundary.data.some(issue => issue.id === target.id),
      partial_distractor_present: boundary.data.some(issue => issue.id === distractor.id),
      private_control_present: boundary.data.some(issue => issue.id === hidden.id), private_control_direct_read_status: denied.status(),
    }) });
    await expect(input).toHaveValue(query);
    expect(boundaryQuery.get('q')).toBe(query); expect(boundaryQuery.get('limit')).toBe('20');
    expect(boundary.data.map(issue => issue.id)).toEqual([target.id]);
    await expect(palette.getByRole('option')).toHaveCount(1);
    await expect(palette.getByRole('option')).toContainText(`${target.key} · ${query}`);
    await expect(palette).not.toContainText(distractor.key); await expect(palette).not.toContainText(hidden.key);

    const beforeRejected = requested.length;
    const rejectedResponse = page.waitForResponse(response => response.request().method() === 'GET'
      && new URL(response.url()).pathname === `${saved.path}/issues` && new URL(response.url()).searchParams.get('q') === query + '\u{1F680}');
    await input.pressSequentially('\u{1F680}');
    const rejected = await rejectedResponse;
    expect(rejected.status()).toBe(422);
    expect(await rejected.json()).toMatchObject({ code: 'validation_failed' });
    await expect(input).toHaveValue(query + '\u{1F680}');
    await expect(palette.getByRole('alert').filter({ hasText: 'Invalid issue filter or page size' })).toBeVisible();
    await expect(palette.getByRole('option')).toHaveCount(0);
    expect(requested.slice(beforeRejected)).toEqual([query + '\u{1F680}']);
    const unresolvedReads = reads.length;
    await input.press('Enter');
    await expect(palette).toBeVisible(); await expect(input).toBeFocused(); expect(reads).toHaveLength(unresolvedReads);
    expect(new URL(page.url()).pathname).toBe(`/w/${saved.workspace}/issues`);

    const repairedResponse = page.waitForResponse(response => response.request().method() === 'GET'
      && new URL(response.url()).pathname === `${saved.path}/issues` && new URL(response.url()).searchParams.get('q') === query);
    await input.press('ControlOrMeta+A'); await input.pressSequentially(query);
    const repaired = await repairedResponse;
    expect(repaired.status()).toBe(200);
    expect((await repaired.json() as IssuePage).data.map(issue => issue.id)).toEqual([target.id]);
    await expect(palette.getByRole('option')).toHaveCount(1);
    await expect(palette.getByRole('option')).toContainText(`${target.key} · ${query}`);
    const [resolved] = await Promise.all([
      page.waitForResponse(response => response.request().method() === 'GET' && response.url() === `${endpoint}/${target.id}`),
      input.press('Enter'),
    ]);
    expect(resolved.status()).toBe(200);
    expect(await resolved.json()).toMatchObject({ id: target.id, workspace_id: saved.workspace, title: query, key: target.key });
    await expect(palette).toHaveCount(0);
    await expect.poll(() => new URL(page.url()).pathname).toBe(`/w/${saved.workspace}/issues/${target.id}`);
    await expect(page.getByRole('textbox', { name: 'Issue title', exact: true })).toHaveValue(query);
    await page.reload();
    await expect(page.getByRole('textbox', { name: 'Issue title', exact: true })).toHaveValue(query);
    expect(await saved.issue(target.id)).toEqual(target);
    expect(await saved.issue(distractor.id)).toEqual(distractor);
    expect((await saved.http.get(`${saved.path}/issues/${hidden.id}`)).status()).toBe(404);
    await test.info().attach('query-resolution-receipt', { contentType: 'application/json', body: JSON.stringify({
      query_scalars: [256, 257, 256], statuses: [boundaryResponse.status(), rejected.status(), repaired.status()],
      target_only_for_full_query: true, no_stale_result_or_unfiltered_fallback: true,
      private_control_direct_read_status: denied.status(), private_control_excluded: true,
      current_resolve_status: resolved.status(), keyboard_opened_same_issue: true, saved_issue_version: target.version,
    }) });
  } finally { await admin.dispose(); }
});
