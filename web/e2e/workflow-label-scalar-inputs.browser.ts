import { randomUUID } from 'node:crypto';
import type { Locator } from '@playwright/test';
import { test, expect, type SavedWorkspace } from './fixtures';
import type { TeamDetail } from '../src/settings/types';
import type { Workflow } from '../src/workflows/types';
import { workflowETag } from '../src/workflows/model';
import { labelETag, type Label, type LabelPage } from '../src/taxonomy/types';

async function freshTeam(saved: SavedWorkspace) {
  return saved.request<TeamDetail>('/teams', 'POST', {
    name: `Scalar acceptance ${randomUUID().slice(0, 8)}`,
    key: `S${randomUUID().replaceAll('-', '').slice(0, 8).toUpperCase()}`, private: true,
  }, { 'Idempotency-Key': randomUUID() });
}
async function replace(input: Locator, value: string) {
  await input.press('ControlOrMeta+A');
  await input.pressSequentially(value);
}

test('status scalar boundaries preserve aggregate versions, defaults and order through rejected writes and repaired reloads', async ({ page, saved }) => {
  const team = await freshTeam(saved);
  const path = `/teams/${team.id}`;
  const endpoint = `${saved.origin}${saved.path}${path}/statuses`;
  const read = () => saved.request<Workflow>(`${path}/workflow`);
  const initial = await read();
  const createdName = '\u{1F680}'.repeat(100);
  const editedName = '\u{1F30D}'.repeat(100);
  const writes: { method: string; status: number }[] = [];
  page.on('response', response => {
    const method = response.request().method();
    if (response.url().startsWith(endpoint) && ['POST', 'PATCH'].includes(method)) writes.push({ method, status: response.status() });
  });
  await page.goto(`/w/${saved.workspace}/settings/teams?team_id=${team.id}&panel=workflow`);
  const settings = page.getByRole('region', { name: `${team.name} workflow`, exact: true });
  await settings.getByRole('button', { name: 'Add status', exact: true }).click();
  const create = page.getByRole('dialog', { name: 'Add status', exact: true });
  const name = create.getByRole('textbox', { name: 'Status name', exact: true });
  await name.pressSequentially(createdName);
  await test.info().attach('native-boundary-counts', { contentType: 'application/json', body: JSON.stringify({
    status_name: { scalars: Array.from(await name.inputValue()).length, utf16_units: (await name.inputValue()).length },
  }) });
  await expect(name).toHaveValue(createdName);
  await name.pressSequentially('\u{1F680}');
  const [rejectedCreate] = await Promise.all([
    page.waitForResponse(response => response.url() === endpoint && response.request().method() === 'POST'),
    create.getByRole('button', { name: 'Add status', exact: true }).click(),
  ]);
  expect(rejectedCreate.request().postDataJSON()).toMatchObject({ name: createdName + '\u{1F680}', category: 'unstarted', color: 'slate' });
  expect(rejectedCreate.request().headers()['if-match']).toBe(workflowETag(initial));
  expect(rejectedCreate.status()).toBe(422);
  expect(await rejectedCreate.json()).toMatchObject({ code: 'validation_failed' });
  await expect(create.getByRole('alert').filter({ hasText: 'Status name must contain' })).toBeVisible();
  await expect(name).toHaveValue(createdName + '\u{1F680}');
  expect(await read()).toEqual(initial);
  await replace(name, createdName);
  const [createdResponse] = await Promise.all([
    page.waitForResponse(response => response.url() === endpoint && response.request().method() === 'POST'),
    create.getByRole('button', { name: 'Add status', exact: true }).click(),
  ]);
  expect(createdResponse.status()).toBe(201);
  expect(createdResponse.request().postDataJSON()).toMatchObject({ name: createdName, category: 'unstarted', color: 'slate' });
  expect(createdResponse.request().headers()['idempotency-key']).toBeTruthy();
  expect(createdResponse.request().headers()['if-match']).toBe(workflowETag(initial));
  const created = await createdResponse.json() as Workflow;
  expect(BigInt(created.version)).toBe(BigInt(initial.version) + 1n);
  const added = created.statuses.find(status => status.name === createdName)!;
  expect(added).toMatchObject({ name: createdName, category: 'unstarted', color: 'slate', is_default: false });
  expect(created.statuses.filter(status => status.id !== added.id)).toEqual(initial.statuses);
  await expect(create).toHaveCount(0);
  expect(await read()).toEqual(created);
  await page.reload();
  await settings.getByRole('button', { name: `Edit ${createdName}`, exact: true }).click();
  const edit = page.getByRole('dialog', { name: 'Edit status', exact: true });
  const editName = edit.getByRole('textbox', { name: 'Status name', exact: true });
  await expect(editName).toHaveValue(createdName);
  await replace(editName, editedName);
  await expect(editName).toHaveValue(editedName);
  await editName.pressSequentially('\u{1F30D}');
  const [rejectedEdit] = await Promise.all([
    page.waitForResponse(response => response.url() === `${endpoint}/${added.id}` && response.request().method() === 'PATCH'),
    edit.getByRole('button', { name: 'Save status', exact: true }).click(),
  ]);
  expect(rejectedEdit.request().postDataJSON()).toEqual({ name: editedName + '\u{1F30D}', color: 'slate' });
  expect(rejectedEdit.request().headers()['if-match']).toBe(workflowETag(created));
  expect(rejectedEdit.status()).toBe(422);
  expect(await rejectedEdit.json()).toMatchObject({ code: 'validation_failed' });
  await expect(edit.getByRole('alert').filter({ hasText: 'Status name must contain' })).toBeVisible();
  await expect(editName).toHaveValue(editedName + '\u{1F30D}');
  expect(await read()).toEqual(created);
  await replace(editName, editedName);
  const [editedResponse] = await Promise.all([
    page.waitForResponse(response => response.url() === `${endpoint}/${added.id}` && response.request().method() === 'PATCH'),
    edit.getByRole('button', { name: 'Save status', exact: true }).click(),
  ]);
  expect(editedResponse.status()).toBe(200);
  expect(editedResponse.request().postDataJSON()).toEqual({ name: editedName, color: 'slate' });
  const edited = await editedResponse.json() as Workflow;
  expect(BigInt(edited.version)).toBe(BigInt(created.version) + 1n);
  expect(edited.statuses).toEqual(created.statuses.map(status => status.id === added.id ? { ...status, name: editedName } : status));
  await expect(edit).toHaveCount(0);
  await page.reload();
  await settings.getByRole('button', { name: `Edit ${editedName}`, exact: true }).click();
  await expect(editName).toHaveValue(editedName);
  expect(await read()).toEqual(edited);
  expect(writes).toEqual([{ method: 'POST', status: 422 }, { method: 'POST', status: 201 }, { method: 'PATCH', status: 422 }, { method: 'PATCH', status: 200 }]);
  await test.info().attach('persisted-scalar-receipt', { contentType: 'application/json', body: JSON.stringify({
    created_scalars: Array.from(added.name).length, edited_scalars: Array.from(editedName).length,
    aggregate_versions: [initial.version, created.version, edited.version], existing_statuses_default_category_order_preserved: true, writes,
  }) });
});

test('label scalar names and full search queries preserve drafts, saved versions and filtered results after 422 rejection', async ({ page, saved }) => {
  const team = await freshTeam(saved);
  const endpoint = `${saved.origin}${saved.path}/labels`;
  const directory = () => saved.request<LabelPage>(`/labels?scope=team&team_id=${team.id}`);
  const createdName = '\u{1F308}'.repeat(100);
  const editedName = '\u{1F31F}'.repeat(100);
  const writes: { method: string; status: number }[] = [];
  const queries: string[] = [];
  page.on('response', response => {
    const method = response.request().method();
    if (response.url().startsWith(endpoint) && ['POST', 'PATCH'].includes(method)) writes.push({ method, status: response.status() });
  });
  page.on('request', request => {
    const url = new URL(request.url());
    if (url.pathname === `${saved.path}/labels` && request.method() === 'GET' && url.searchParams.get('team_id') === team.id) queries.push(url.searchParams.get('q') ?? '');
  });
  await page.goto(`/w/${saved.workspace}/settings/teams?team_id=${team.id}&panel=labels`);
  const settings = page.getByRole('region', { name: `${team.name} labels`, exact: true });
  const search = settings.getByRole('searchbox', { name: 'Search labels', exact: true });
  await search.pressSequentially(editedName);
  const nativeQuery = await search.inputValue();
  await search.press('ControlOrMeta+A'); await search.press('Backspace');
  await settings.getByRole('button', { name: 'New label', exact: true }).click();
  const create = page.getByRole('dialog', { name: 'New label', exact: true });
  const name = create.getByRole('textbox', { name: 'Label name', exact: true });
  await name.pressSequentially(createdName);
  await test.info().attach('native-boundary-counts', { contentType: 'application/json', body: JSON.stringify({
    label_name: { scalars: Array.from(await name.inputValue()).length, utf16_units: (await name.inputValue()).length },
    label_query: { scalars: Array.from(nativeQuery).length, utf16_units: nativeQuery.length },
  }) });
  expect.soft(await name.inputValue()).toBe(createdName);
  expect.soft(nativeQuery).toBe(editedName);
  expect(test.info().errors).toHaveLength(0);
  await name.pressSequentially('\u{1F308}');
  const [rejectedCreate] = await Promise.all([
    page.waitForResponse(response => response.url() === endpoint && response.request().method() === 'POST'),
    create.getByRole('button', { name: 'Create label', exact: true }).click(),
  ]);
  expect(rejectedCreate.request().postDataJSON()).toEqual({ name: createdName + '\u{1F308}', color: 'gray', team_id: team.id });
  expect(rejectedCreate.status()).toBe(422);
  expect(await rejectedCreate.json()).toMatchObject({ code: 'validation_failed' });
  await expect(create.getByRole('alert').filter({ hasText: 'Label name must contain' })).toBeVisible();
  await expect(name).toHaveValue(createdName + '\u{1F308}');
  expect((await directory()).data).toHaveLength(0);
  await replace(name, createdName);
  const [createdResponse] = await Promise.all([
    page.waitForResponse(response => response.url() === endpoint && response.request().method() === 'POST'),
    create.getByRole('button', { name: 'Create label', exact: true }).click(),
  ]);
  expect(createdResponse.status()).toBe(201);
  expect(createdResponse.request().postDataJSON()).toEqual({ name: createdName, color: 'gray', team_id: team.id });
  expect(createdResponse.request().headers()['idempotency-key']).toBeTruthy();
  const created = await createdResponse.json() as Label;
  expect(created).toMatchObject({ name: createdName, team_id: team.id, version: '1', color: 'gray' });
  await expect(create).toHaveCount(0);
  expect((await directory()).data).toEqual([created]);
  await page.reload();
  await settings.getByRole('button', { name: `Edit ${createdName}`, exact: true }).click();
  const edit = page.getByRole('dialog', { name: 'Edit label', exact: true });
  const editName = edit.getByRole('textbox', { name: 'Label name', exact: true });
  await expect(editName).toHaveValue(createdName);
  await replace(editName, editedName);
  await expect(editName).toHaveValue(editedName);
  await editName.pressSequentially('\u{1F31F}');
  const [rejectedEdit] = await Promise.all([
    page.waitForResponse(response => response.url() === `${endpoint}/${created.id}` && response.request().method() === 'PATCH'),
    edit.getByRole('button', { name: 'Save label', exact: true }).click(),
  ]);
  expect(rejectedEdit.request().postDataJSON()).toEqual({ name: editedName + '\u{1F31F}', color: 'gray' });
  expect(rejectedEdit.request().headers()['if-match']).toBe(labelETag(created));
  expect(rejectedEdit.status()).toBe(422);
  expect(await rejectedEdit.json()).toMatchObject({ code: 'validation_failed' });
  await expect(edit.getByRole('alert').filter({ hasText: 'Label name must contain' })).toBeVisible();
  await expect(editName).toHaveValue(editedName + '\u{1F31F}');
  expect(await saved.request<Label>(`/labels/${created.id}`)).toEqual(created);
  expect((await directory()).data).toEqual([created]);
  await replace(editName, editedName);
  const [editedResponse] = await Promise.all([
    page.waitForResponse(response => response.url() === `${endpoint}/${created.id}` && response.request().method() === 'PATCH'),
    edit.getByRole('button', { name: 'Save label', exact: true }).click(),
  ]);
  expect(editedResponse.status()).toBe(200);
  expect(editedResponse.request().postDataJSON()).toEqual({ name: editedName, color: 'gray' });
  const edited = await editedResponse.json() as Label;
  expect(edited).toEqual({ ...created, name: editedName, version: '2' });
  await expect(edit).toHaveCount(0);
  await page.reload();
  await expect(settings.getByRole('button', { name: `Edit ${editedName}`, exact: true })).toBeVisible();
  expect(await saved.request<Label>(`/labels/${created.id}`)).toEqual(edited);
  const queryResponse = (value: string) => page.waitForResponse(response => {
    const url = new URL(response.url());
    return url.pathname === `${saved.path}/labels` && response.request().method() === 'GET' && url.searchParams.get('q') === value;
  });
  const fullQuery = queryResponse(editedName);
  await search.pressSequentially(editedName);
  const found = await fullQuery;
  expect(found.status()).toBe(200);
  expect((await found.json() as LabelPage).data).toEqual([edited]);
  await expect(settings.locator('.settings-row')).toHaveCount(1);
  const beforeRejectedQuery = queries.length;
  const rejectedQuery = queryResponse(editedName + '\u{1F31F}');
  await search.pressSequentially('\u{1F31F}');
  const rejected = await rejectedQuery;
  expect(rejected.status()).toBe(422);
  expect(await rejected.json()).toMatchObject({ code: 'validation_failed' });
  await expect(search).toHaveValue(editedName + '\u{1F31F}');
  await expect(settings.getByRole('alert').filter({ hasText: 'Search must contain at most 100 characters' })).toBeVisible();
  await expect(settings.locator('.settings-row')).toHaveCount(0);
  expect(queries.slice(beforeRejectedQuery)).toEqual([editedName + '\u{1F31F}']);
  expect(await saved.request<Label>(`/labels/${created.id}`)).toEqual(edited);
  const repairedQuery = queryResponse(editedName);
  await replace(search, editedName);
  expect((await repairedQuery).status()).toBe(200);
  await expect(settings.locator('.settings-row')).toHaveCount(1);
  await expect(settings.getByRole('button', { name: `Edit ${editedName}`, exact: true })).toBeVisible();
  expect((await directory()).data).toEqual([edited]);
  expect(writes).toEqual([{ method: 'POST', status: 422 }, { method: 'POST', status: 201 }, { method: 'PATCH', status: 422 }, { method: 'PATCH', status: 200 }]);
  await test.info().attach('persisted-scalar-receipt', { contentType: 'application/json', body: JSON.stringify({
    created_scalars: Array.from(created.name).length, edited_scalars: Array.from(edited.name).length,
    versions: [created.version, edited.version], query_scalars: [100, 101, 100], query_statuses: [found.status(), rejected.status(), 200],
    no_unfiltered_fallback: true, writes,
  }) });
});
