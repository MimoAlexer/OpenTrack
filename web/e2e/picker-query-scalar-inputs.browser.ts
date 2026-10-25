import { randomUUID } from 'node:crypto';
import type { APIRequest, APIRequestContext, Locator, Page, Response } from '@playwright/test';
import { test, expect, type SavedWorkspace } from './fixtures';
import type { TeamDetail } from '../src/settings/types';
import type { Issue, Session } from '../src/types';
import type { Label } from '../src/taxonomy/types';
import type { FullProject, Project } from '../src/projects/types';

const longName = () => [...randomUUID().replaceAll('-', '')].map(value => String.fromCodePoint(0x1F600 + Number.parseInt(value, 16))).join('') + '\u{1F680}'.repeat(68);
async function team(saved: SavedWorkspace) {
  return saved.request<TeamDetail>('/teams', 'POST', { name: `Picker acceptance ${randomUUID().slice(0, 8)}`,
    key: `K${randomUUID().replaceAll('-', '').slice(0, 8).toUpperCase()}`, private: true }, { 'Idempotency-Key': randomUUID() });
}
async function person(api: APIRequest, saved: SavedWorkspace, name: string, teamIds: string[], role = 'member') {
  const invitation = await saved.request<{ token: string }>('/invitations', 'POST', {
    email: `picker-${randomUUID()}@example.test`, role, team_ids: teamIds,
  }, { 'Idempotency-Key': randomUUID() });
  const http = await api.newContext({ baseURL: saved.origin, extraHTTPHeaders: { Origin: saved.origin }, storageState: { cookies: [], origins: [] } });
  try {
    const response = await http.post('/api/v1/invitations/accept', { data: {
      token: invitation.token, display_name: name, password: process.env.OPENTRACK_BROWSER_PASSWORD,
    } });
    expect(response.status()).toBe(200);
    const session = await response.json() as Session; expect(session.user.display_name).toBe(name);
    return { http, session };
  } catch (error) { await http.dispose(); throw error; }
}
function watch(page: Page, path: string, teamId?: string) {
  const requests: string[] = []; const replies: Response[] = [];
  const matches = (value: string) => { const url = new URL(value); return url.pathname === path && url.searchParams.has('q') && (!teamId || url.searchParams.get('team_id') === teamId); };
  page.on('request', request => { if (request.method() === 'GET' && matches(request.url())) requests.push(new URL(request.url()).searchParams.get('q')!); });
  page.on('response', response => { if (response.request().method() === 'GET' && matches(response.url())) replies.push(response); });
  return { requests, replies };
}
async function typed(input: Locator, value: string, log: ReturnType<typeof watch>) {
  const start = log.replies.length; await input.pressSequentially(value); const actual = await input.inputValue();
  await expect.poll(() => log.replies.slice(start).some(reply => new URL(reply.url()).searchParams.get('q') === actual)).toBe(true);
  const response = log.replies.slice(start).filter(reply => new URL(reply.url()).searchParams.get('q') === actual).at(-1)!;
  expect(new URL(response.url()).searchParams.get('limit')).toBe('20');
  return { actual, response };
}
async function proof(result: Awaited<ReturnType<typeof typed>>, target: string, partial: string, excluded: string) {
  const data = (await result.response.json() as { data: { id: string }[] }).data;
  return { input_scalars: Array.from(result.actual).length, input_utf16_units: result.actual.length,
    requested_scalars: Array.from(new URL(result.response.url()).searchParams.get('q')!).length, status: result.response.status(),
    result_count: data.length, target_present: data.some(item => item.id === target),
    partial_present: data.some(item => item.id === partial), ineligible_present: data.some(item => item.id === excluded) };
}

test('shared label and assignee queries retain full scalar text and persist repaired authorized issue selections', async ({ page, saved, playwright }) => {
  const currentTeam = await team(saved); const name = longName(); const prefix = Array.from(name).slice(0, 50).join('');
  const contexts: APIRequestContext[] = [];
  try {
    const member = await person(playwright.request, saved, name, [currentTeam.id]); contexts.push(member.http);
    const partial = await person(playwright.request, saved, prefix + ' partial person', [currentTeam.id]); contexts.push(partial.http);
    const outsider = await person(playwright.request, saved, name, [], 'admin'); contexts.push(outsider.http);
    const label = await saved.request<Label>('/labels', 'POST', { name, color: 'blue', team_id: currentTeam.id }, { 'Idempotency-Key': randomUUID() });
    const partialLabel = await saved.request<Label>('/labels', 'POST', { name: prefix + ' partial label', color: 'green', team_id: currentTeam.id }, { 'Idempotency-Key': randomUUID() });
    const privateTeamResponse = await outsider.http.post(`${saved.path}/teams`, {
      headers: { 'X-CSRF-Token': outsider.session.csrf_token, 'Idempotency-Key': randomUUID() },
      data: { name: 'Private picker control', key: `H${randomUUID().replaceAll('-', '').slice(0, 8).toUpperCase()}`, private: true },
    });
    expect(privateTeamResponse.status()).toBe(201); const privateTeam = await privateTeamResponse.json() as TeamDetail;
    const hiddenLabelResponse = await outsider.http.post(`${saved.path}/labels`, {
      headers: { 'X-CSRF-Token': outsider.session.csrf_token, 'Idempotency-Key': randomUUID() }, data: { name, color: 'red', team_id: privateTeam.id },
    });
    expect(hiddenLabelResponse.status()).toBe(201); const hiddenLabel = await hiddenLabelResponse.json() as Label;
    expect((await saved.http.get(`${saved.path}/labels/${hiddenLabel.id}`)).status()).toBe(404);
    const labels = watch(page, `${saved.path}/labels`, currentTeam.id);
    const assignees = watch(page, `${saved.path}/teams/${currentTeam.id}/assignees`);
    const writes: Response[] = [];
    page.on('response', response => { if (response.url() === `${saved.origin}${saved.path}/issues` && response.request().method() === 'POST') writes.push(response); });
    await saved.open(page); await page.getByRole('button', { name: /^New issue/ }).click();
    const create = page.getByRole('dialog', { name: 'New issue', exact: true });
    await create.getByRole('combobox', { name: 'Team', exact: true }).selectOption(currentTeam.id);
    const labelField = create.locator('.reference-picker').filter({ has: page.getByText('Labels', { exact: true }) });
    const assigneeField = create.locator('.reference-picker').filter({ has: page.getByText('Assignee', { exact: true }) });
    await labelField.getByRole('button', { name: /^Labels/ }).click();
    const labelInput = labelField.getByRole('combobox', { name: 'Search labels', exact: true });
    const labelProbe = await typed(labelInput, name, labels);
    const labelProof = await proof(labelProbe, label.id, partialLabel.id, hiddenLabel.id);
    await labelInput.press('Escape'); await expect(create).toBeVisible();
    await assigneeField.getByRole('button', { name: /^Assignee/ }).click();
    const assigneeInput = assigneeField.getByRole('combobox', { name: 'Search assignee', exact: true });
    const assigneeProbe = await typed(assigneeInput, name, assignees);
    const assigneeProof = await proof(assigneeProbe, member.session.user.id, partial.session.user.id, outsider.session.user.id);
    await assigneeInput.press('Escape'); await expect(create).toBeVisible();
    await test.info().attach('native-boundary-evidence', { contentType: 'application/json', body: JSON.stringify({ labels: labelProof, assignee: assigneeProof }) });
    expect.soft(labelProbe.actual).toBe(name); expect.soft(assigneeProbe.actual).toBe(name); expect(test.info().errors).toHaveLength(0);
    expect(labelProof).toMatchObject({ input_scalars: 100, requested_scalars: 100, status: 200, result_count: 1, target_present: true, partial_present: false, ineligible_present: false });
    expect(assigneeProof).toMatchObject({ input_scalars: 100, requested_scalars: 100, status: 200, result_count: 1, target_present: true, partial_present: false, ineligible_present: false });

    await labelField.getByRole('button', { name: /^Labels/ }).click(); await typed(labelInput, name, labels);
    const beforeLabelError = labels.requests.length; const badLabel = await typed(labelInput, '\u{1F680}', labels);
    expect(badLabel.actual).toBe(name + '\u{1F680}'); expect(badLabel.response.status()).toBe(422);
    expect(await badLabel.response.json()).toMatchObject({ code: 'validation_failed' });
    await expect(labelField.getByRole('alert')).toBeVisible(); await expect(labelField.getByRole('option').filter({ hasText: name })).toHaveCount(0);
    expect(labels.requests.slice(beforeLabelError)).toEqual([name + '\u{1F680}']);
    await labelInput.press('Enter'); await expect(labelInput).toBeVisible(); expect(writes).toHaveLength(0);
    await labelInput.press('ControlOrMeta+A'); const repairedLabel = await typed(labelInput, name, labels); expect(repairedLabel.response.status()).toBe(200);
    await labelField.getByRole('option').filter({ hasText: name }).click(); await labelInput.press('Escape');
    await expect(labelField.getByRole('button', { name: /^Labels/ })).toContainText('1 selected');

    await assigneeField.getByRole('button', { name: /^Assignee/ }).click(); await typed(assigneeInput, name, assignees);
    const beforeAssigneeError = assignees.requests.length; const badAssignee = await typed(assigneeInput, '\u{1F680}', assignees);
    expect(badAssignee.actual).toBe(name + '\u{1F680}'); expect(badAssignee.response.status()).toBe(422);
    expect(await badAssignee.response.json()).toMatchObject({ code: 'validation_failed' });
    await expect(assigneeField.getByRole('alert')).toBeVisible(); await expect(assigneeField.getByRole('option').filter({ hasText: name })).toHaveCount(0);
    expect(assignees.requests.slice(beforeAssigneeError)).toEqual([name + '\u{1F680}']);
    await assigneeInput.press('Enter'); await expect(assigneeInput).toBeVisible(); expect(writes).toHaveLength(0);
    await assigneeInput.press('ControlOrMeta+A'); const repairedAssignee = await typed(assigneeInput, name, assignees); expect(repairedAssignee.response.status()).toBe(200);
    await assigneeField.getByRole('option').filter({ hasText: name }).click();
    await expect(assigneeField.getByRole('button', { name: /^Assignee/ })).toContainText(name);
    await create.getByRole('textbox', { name: 'Title', exact: true }).fill('Persisted shared picker selections');
    const [createdResponse] = await Promise.all([
      page.waitForResponse(response => response.url() === `${saved.origin}${saved.path}/issues` && response.request().method() === 'POST'),
      create.getByRole('button', { name: 'Create issue', exact: true }).click(),
    ]);
    expect(createdResponse.status()).toBe(201);
    expect(createdResponse.request().postDataJSON()).toMatchObject({ team_id: currentTeam.id, assignee_id: member.session.user.id, label_ids: [label.id] });
    const created = await createdResponse.json() as Issue;
    expect(created).toMatchObject({ version: '1', assignee_id: member.session.user.id, assignee: { display_name: name, assignable: true }, label_ids: [label.id] });
    expect(created.labels[0].name).toBe(name); await expect(create).toHaveCount(0);
    await page.reload(); const detail = page.getByRole('region', { name: 'Issue detail', exact: true });
    await expect(detail.getByRole('button', { name: /^Assignee/ })).toContainText(name);
    await expect(detail.getByRole('button', { name: `Remove ${name}`, exact: true })).toBeVisible();
    expect(await saved.issue(created.id)).toEqual(created); expect(writes).toHaveLength(1);
    await test.info().attach('persisted-selection-receipt', { contentType: 'application/json', body: JSON.stringify({
      consumers: ['LabelPicker', 'AssigneePicker'], query_scalars: [100, 101, 100], statuses: [200, 422, 200],
      no_stale_selection_or_unfiltered_fallback: true, private_label_read_status: 404, ineligible_person_excluded: true,
      persisted_label_and_assignee_names: [100, 100], accepted_writes: 1, saved_version: created.version,
    }) });
  } finally { await Promise.all(contexts.map(context => context.dispose())); }
});

test('project lead uses the shared scalar query and persists only the repaired eligible selection', async ({ page, saved, playwright }) => {
  const currentTeam = await team(saved); const name = longName(); const prefix = Array.from(name).slice(0, 50).join('');
  const contexts: APIRequestContext[] = [];
  try {
    const member = await person(playwright.request, saved, name, [currentTeam.id]); contexts.push(member.http);
    const partial = await person(playwright.request, saved, prefix + ' partial lead', [currentTeam.id]); contexts.push(partial.http);
    const outsider = await person(playwright.request, saved, name, []); contexts.push(outsider.http);
    const assignees = watch(page, `${saved.path}/teams/${currentTeam.id}/assignees`);
    const endpoint = `${saved.origin}${saved.path}/teams/${currentTeam.id}/projects`; const writes: Response[] = [];
    page.on('response', response => { if (response.url() === endpoint && response.request().method() === 'POST') writes.push(response); });
    await page.goto(`/w/${saved.workspace}/projects?team_id=${currentTeam.id}`);
    await expect(page.getByRole('combobox', { name: 'Project team', exact: true })).toHaveValue(currentTeam.id);
    await page.getByRole('button', { name: 'New project', exact: true }).click();
    const create = page.getByRole('dialog', { name: 'New project', exact: true });
    const field = create.locator('.project-lead-field'); await field.getByRole('button', { name: /^Project lead/ }).click();
    const input = field.getByRole('combobox', { name: 'Search project lead', exact: true });
    const probe = await typed(input, name, assignees); const native = await proof(probe, member.session.user.id, partial.session.user.id, outsider.session.user.id);
    await test.info().attach('native-boundary-evidence', { contentType: 'application/json', body: JSON.stringify({ project_lead: native }) });
    await expect(input).toHaveValue(name);
    expect(native).toMatchObject({ input_scalars: 100, requested_scalars: 100, status: 200, result_count: 1, target_present: true, partial_present: false, ineligible_present: false });
    const beforeError = assignees.requests.length; const rejected = await typed(input, '\u{1F680}', assignees);
    expect(rejected.actual).toBe(name + '\u{1F680}'); expect(rejected.response.status()).toBe(422);
    expect(await rejected.response.json()).toMatchObject({ code: 'validation_failed' });
    await expect(field.getByRole('alert')).toBeVisible(); await expect(field.getByRole('option').filter({ hasText: name })).toHaveCount(0);
    expect(assignees.requests.slice(beforeError)).toEqual([name + '\u{1F680}']);
    await input.press('Enter'); await expect(input).toBeVisible(); expect(writes).toHaveLength(0);
    await input.press('ControlOrMeta+A'); const repaired = await typed(input, name, assignees); expect(repaired.response.status()).toBe(200);
    await field.getByRole('option').filter({ hasText: name }).click();
    await expect(field.getByRole('button', { name: /^Project lead/ })).toContainText(name);
    const projectName = `Persisted picker project ${randomUUID().slice(0, 8)}`;
    await create.getByRole('textbox', { name: 'Name', exact: true }).fill(projectName);
    const [createdResponse] = await Promise.all([
      page.waitForResponse(response => response.url() === endpoint && response.request().method() === 'POST'),
      create.getByRole('button', { name: 'Create project', exact: true }).click(),
    ]);
    expect(createdResponse.status()).toBe(201); expect(createdResponse.request().postDataJSON()).toMatchObject({ lead_id: member.session.user.id });
    const created = await createdResponse.json() as FullProject;
    expect(created).toMatchObject({ version: '1', lead_id: member.session.user.id, lead: { display_name: name, assignable: true } });
    await expect(create).toHaveCount(0); await expect(page.getByRole('heading', { name: projectName, exact: true })).toBeVisible();
    const path = `/teams/${currentTeam.id}/projects/${created.id}/metadata`;
    const metadata = await saved.request<Project>(path); expect(metadata.lead_id).toBe(member.session.user.id);
    await page.reload(); await page.getByRole('button', { name: 'Edit project', exact: true }).click();
    await expect(page.getByRole('dialog', { name: 'Edit project', exact: true }).getByRole('button', { name: /^Project lead/ })).toContainText(name);
    expect(await saved.request<Project>(path)).toEqual(metadata); expect(writes).toHaveLength(1);
    await test.info().attach('persisted-selection-receipt', { contentType: 'application/json', body: JSON.stringify({
      consumers: ['ProjectLead'], query_scalars: [100, 101, 100], statuses: [200, 422, 200],
      no_stale_selection_or_unfiltered_fallback: true, ineligible_person_excluded: true,
      persisted_lead_name_scalars: 100, accepted_writes: 1, saved_version: metadata.version,
    }) });
  } finally { await Promise.all(contexts.map(context => context.dispose())); }
});
