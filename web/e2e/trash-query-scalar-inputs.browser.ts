import { randomUUID } from 'node:crypto';
import type { Response } from '@playwright/test';
import { test, expect } from './fixtures';
import type { TeamDetail } from '../src/settings/types';
import type { Issue, Session } from '../src/types';
import type { DeletedIssue, DeletedIssuePage } from '../src/issues/deletion-types';

test('Trash preserves full scalar queries, rejects 257 without stale actions and reloads the repaired authorized detail', async ({ page, saved, playwright }) => {
  const query = [...randomUUID().replaceAll('-', '')].map(value => String.fromCodePoint(0x1F600 + Number.parseInt(value, 16))).join('') + '\u{1F680}'.repeat(224);
  const prefix = Array.from(query).slice(0, 100).join(''); expect(Array.from(query)).toHaveLength(256);
  const team = await saved.request<TeamDetail>('/teams', 'POST', { name: `Trash query ${randomUUID().slice(0, 8)}`,
    key: `T${randomUUID().replaceAll('-', '').slice(0, 8).toUpperCase()}`, private: true }, { 'Idempotency-Key': randomUUID() });
  const target = await saved.request<Issue>('/issues', 'POST', { team_id: team.id, title: query }, { 'Idempotency-Key': randomUUID() });
  const distractor = await saved.request<Issue>('/issues', 'POST', { team_id: team.id, title: prefix + ' partial deleted result' }, { 'Idempotency-Key': randomUUID() });
  const invitation = await saved.request<{ token: string }>('/invitations', 'POST', {
    email: `trash-${randomUUID()}@example.test`, role: 'admin', team_ids: [],
  }, { 'Idempotency-Key': randomUUID() });
  const admin = await playwright.request.newContext({ baseURL: saved.origin, extraHTTPHeaders: { Origin: saved.origin }, storageState: { cookies: [], origins: [] } });
  try {
    const accepted = await admin.post('/api/v1/invitations/accept', { data: {
      token: invitation.token, display_name: 'Example Administrator', password: process.env.OPENTRACK_BROWSER_PASSWORD,
    } });
    expect(accepted.status()).toBe(200); const account = await accepted.json() as Session;
    const privateTeamResponse = await admin.post(`${saved.path}/teams`, {
      headers: { 'X-CSRF-Token': account.csrf_token, 'Idempotency-Key': randomUUID() },
      data: { name: 'Private Trash control', key: `H${randomUUID().replaceAll('-', '').slice(0, 8).toUpperCase()}`, private: true },
    });
    expect(privateTeamResponse.status()).toBe(201); const privateTeam = await privateTeamResponse.json() as TeamDetail;
    const hiddenResponse = await admin.post(`${saved.path}/issues`, {
      headers: { 'X-CSRF-Token': account.csrf_token, 'Idempotency-Key': randomUUID() }, data: { team_id: privateTeam.id, title: query },
    });
    expect(hiddenResponse.status()).toBe(201); const hidden = await hiddenResponse.json() as Issue;
    for (const issue of [target, distractor]) {
      const current = await saved.issue(issue.id);
      const response = await saved.http.delete(`${saved.path}/issues/${issue.id}`, {
        headers: { Origin: saved.origin, 'X-CSRF-Token': saved.session.csrf_token, 'If-Match': current.etag },
      });
      expect(response.status()).toBe(204);
    }
    const hiddenDelete = await admin.delete(`${saved.path}/issues/${hidden.id}`, {
      headers: { 'X-CSRF-Token': account.csrf_token, 'If-Match': hidden.etag },
    });
    expect(hiddenDelete.status()).toBe(204);
    const path = `/issues/${target.id}/deleted`;
    const tombstone = await saved.request<DeletedIssue>(path);
    const partial = await saved.request<DeletedIssue>(`/issues/${distractor.id}/deleted`);
    expect(tombstone).toMatchObject({ issue: { id: target.id, title: query, version: '2' }, can_restore: true });
    const denied = await saved.http.get(`${saved.path}/issues/${hidden.id}/deleted`); expect(denied.status()).toBe(404);
    const replies: Response[] = []; const requests: (string | null)[] = []; const detailRequests: string[] = []; const mutations: string[] = [];
    page.on('request', request => {
      const url = new URL(request.url());
      if (request.method() === 'GET' && url.pathname === `${saved.path}/issues/deleted`) requests.push(url.searchParams.get('q'));
      if (request.method() === 'GET' && url.pathname === `${saved.path}${path}`) detailRequests.push(request.url());
      if (url.pathname.startsWith(saved.path + '/') && ['POST', 'PATCH', 'DELETE', 'PUT'].includes(request.method())) mutations.push(request.method());
    });
    page.on('response', response => {
      const url = new URL(response.url());
      if (response.request().method() === 'GET' && url.pathname === `${saved.path}/issues/deleted`) replies.push(response);
    });
    await page.goto(`/w/${saved.workspace}/trash`);
    const screen = page.getByRole('region', { name: 'Trash', exact: true });
    const input = screen.getByRole('searchbox', { name: 'Search Trash by title or key', exact: true });
    await input.pressSequentially(query); const native = await input.inputValue();
    await expect.poll(() => replies.some(response => new URL(response.url()).searchParams.get('q') === native)).toBe(true);
    const boundaryResponse = replies.filter(response => new URL(response.url()).searchParams.get('q') === native).at(-1)!;
    expect(boundaryResponse.status()).toBe(200); const boundary = await boundaryResponse.json() as DeletedIssuePage;
    await test.info().attach('native-boundary-evidence', { contentType: 'application/json', body: JSON.stringify({
      input_scalars: Array.from(native).length, input_utf16_units: native.length,
      requested_scalars: Array.from(new URL(boundaryResponse.url()).searchParams.get('q')!).length,
      limit: new URL(boundaryResponse.url()).searchParams.get('limit'), result_count: boundary.data.length,
      target_present: boundary.data.some(item => item.issue.id === target.id), partial_present: boundary.data.some(item => item.issue.id === distractor.id),
      private_present: boundary.data.some(item => item.issue.id === hidden.id), private_deleted_read_status: denied.status(),
    }) });
    await expect(input).toHaveValue(query); expect(boundary.data.map(item => item.issue.id)).toEqual([target.id]);
    const rows = screen.locator('.trash-row'); await expect(rows).toHaveCount(1);
    await expect(rows).toContainText(target.key); await expect(rows).not.toContainText(distractor.key); await expect(rows).not.toContainText(hidden.key);
    const beforeError = requests.length;
    const rejectedResponse = page.waitForResponse(response => response.request().method() === 'GET'
      && new URL(response.url()).pathname === `${saved.path}/issues/deleted` && new URL(response.url()).searchParams.get('q') === query + '\u{1F680}');
    await input.pressSequentially('\u{1F680}'); const rejected = await rejectedResponse;
    expect(rejected.status()).toBe(422); expect(await rejected.json()).toMatchObject({ code: 'validation_failed' });
    await expect(input).toHaveValue(query + '\u{1F680}');
    await expect(screen.getByRole('alert').filter({ hasText: 'Invalid trash search or page size' })).toBeVisible();
    await expect(rows).toHaveCount(0);
    expect(detailRequests).toHaveLength(0); await input.press('Enter'); expect(detailRequests).toHaveLength(0); expect(mutations).toEqual([]);
    expect(new URL(page.url()).pathname).toBe(`/w/${saved.workspace}/trash`);
    const rejectedWindow = requests.slice(beforeError); expect(rejectedWindow).toEqual([query + '\u{1F680}']);
    const repairedResponse = page.waitForResponse(response => response.request().method() === 'GET'
      && new URL(response.url()).pathname === `${saved.path}/issues/deleted` && new URL(response.url()).searchParams.get('q') === query);
    await input.press('ControlOrMeta+A'); await input.pressSequentially(query); const repaired = await repairedResponse;
    expect(repaired.status()).toBe(200); expect((await repaired.json() as DeletedIssuePage).data.map(item => item.issue.id)).toEqual([target.id]);
    await expect(rows).toHaveCount(1);
    const [detailResponse] = await Promise.all([
      page.waitForResponse(response => response.request().method() === 'GET' && response.url() === `${saved.origin}${saved.path}${path}`),
      rows.filter({ hasText: target.key }).press('Enter'),
    ]);
    expect(detailResponse.status()).toBe(200); expect(await detailResponse.json()).toEqual(tombstone); expect(detailRequests).toHaveLength(1);
    const detail = page.getByRole('dialog', { name: `Restore ${target.key}`, exact: true }); await expect(detail).toContainText(query);
    expect(new URL(page.url()).pathname).toBe(`/w/${saved.workspace}/trash/${target.id}`);
    expect(new URL(page.url()).searchParams.get('q')).toBe(query);
    await page.reload(); await expect(detail).toContainText(query); expect(detailRequests).toHaveLength(2);
    expect(await saved.request<DeletedIssue>(path)).toEqual(tombstone);
    await detail.getByRole('button', { name: 'Cancel', exact: true }).click(); await expect(detail).toHaveCount(0);
    await expect(input).toHaveValue(query); await expect(rows).toHaveCount(1);
    expect(await saved.request<DeletedIssue>(`/issues/${distractor.id}/deleted`)).toEqual(partial);
    expect((await saved.http.get(`${saved.path}/issues/${hidden.id}/deleted`)).status()).toBe(404); expect(mutations).toEqual([]); expect(detailRequests).toHaveLength(2);
    await test.info().attach('query-detail-receipt', { contentType: 'application/json', body: JSON.stringify({
      query_scalars: [256, 257, 256], statuses: [boundaryResponse.status(), rejected.status(), repaired.status()],
      no_stale_action_or_unfiltered_fallback: true, private_deleted_read_status: denied.status(), private_control_excluded: true,
      current_detail_status: detailResponse.status(), correct_detail_and_query_after_reload: true,
      saved_deleted_version: tombstone.issue.version, mutations_during_search_and_detail: mutations.length,
      every_list_get_recorded_including_absent_or_empty_query: true, rejected_window_queries: rejectedWindow.map(value => value === null ? 'absent' : value === '' ? 'empty' : Array.from(value).length),
      issued_detail_requests_after_deliberate_open_and_reload: detailRequests.length,
    }) });
  } finally { await admin.dispose(); }
});
