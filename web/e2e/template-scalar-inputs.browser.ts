import { randomUUID } from 'node:crypto';
import { test, expect } from './fixtures';
import type { IssueTemplate } from '../src/templates/types';
import type { TeamDetail } from '../src/settings/types';

test('template supplementary scalars survive create, edit and reload while rejected writes preserve drafts and saved versions', async ({ page, saved }) => {
  const team = await saved.request<TeamDetail>('/teams', 'POST', {
    name: `Template scalar acceptance ${randomUUID().slice(0, 8)}`,
    key: `T${randomUUID().replaceAll('-', '').slice(0, 8).toUpperCase()}`, private: true,
  }, { 'Idempotency-Key': randomUUID() });
  const path = `/teams/${team.id}/templates`;
  const endpoint = `${saved.origin}${saved.path}${path}`;
  const directory = () => saved.request<{ data: IssueTemplate[]; has_more: boolean }>(path);
  const createdText = { name: '\u{1F680}'.repeat(100), title: '\u{1F30D}'.repeat(256) };
  const editedText = { name: '\u{1F308}'.repeat(100), title: '\u{1F31F}'.repeat(256) };
  const writes: { method: string; status: number }[] = [];
  page.on('response', response => {
    const method = response.request().method();
    if (response.url().startsWith(endpoint) && ['POST', 'PATCH'].includes(method)) writes.push({ method, status: response.status() });
  });
  await page.goto(`/w/${saved.workspace}/settings/teams?team_id=${team.id}&panel=templates`);
  const settings = page.getByRole('region', { name: `${team.name} templates`, exact: true });
  await settings.getByRole('button', { name: 'New template', exact: true }).click();
  const create = page.getByRole('dialog', { name: 'New issue template', exact: true });
  const name = create.getByRole('textbox', { name: 'Template name', exact: true });
  const title = create.getByRole('textbox', { name: 'Issue title', exact: false });
  await expect(create.getByRole('combobox', { name: 'Title mode', exact: true })).toHaveValue('literal');
  await name.pressSequentially(createdText.name);
  await title.pressSequentially(createdText.title);
  await test.info().attach('native-boundary-counts', { contentType: 'application/json', body: JSON.stringify({
    name: { scalars: Array.from(await name.inputValue()).length, utf16_units: (await name.inputValue()).length },
    title: { scalars: Array.from(await title.inputValue()).length, utf16_units: (await title.inputValue()).length },
  }) });
  expect.soft(await name.inputValue()).toBe(createdText.name);
  expect.soft(await title.inputValue()).toBe(createdText.title);
  expect(test.info().errors).toHaveLength(0);

  await name.pressSequentially('\u{1F680}');
  for (const field of ['name', 'title'] as const) {
    const rejected = { ...createdText, [field]: createdText[field] + (field === 'name' ? '\u{1F680}' : '\u{1F30D}') };
    const [response] = await Promise.all([
      page.waitForResponse(value => value.url() === endpoint && value.request().method() === 'POST'),
      create.getByRole('button', { name: 'Create template', exact: true }).click(),
    ]);
    expect(response.request().postDataJSON()).toMatchObject({ ...rejected, title_mode: 'literal' });
    expect(response.status()).toBe(422);
    expect(await response.json()).toMatchObject({ code: 'validation_failed' });
    await expect(create.getByRole('alert').filter({ hasText: field === 'name' ? 'Template name must contain' : 'Template title must contain' })).toBeVisible();
    await expect(name).toHaveValue(rejected.name);
    await expect(title).toHaveValue(rejected.title);
    await expect(create).toBeVisible();
    expect(await directory()).toMatchObject({ data: [], has_more: false });
    const input = field === 'name' ? name : title;
    await input.press('ControlOrMeta+A');
    await input.pressSequentially(createdText[field]);
    if (field === 'name') await title.pressSequentially('\u{1F30D}');
  }
  const [createdResponse] = await Promise.all([
    page.waitForResponse(value => value.url() === endpoint && value.request().method() === 'POST'),
    create.getByRole('button', { name: 'Create template', exact: true }).click(),
  ]);
  expect(createdResponse.status()).toBe(201);
  expect(createdResponse.request().headers()['idempotency-key']).toBeTruthy();
  expect(createdResponse.request().postDataJSON()).toMatchObject({ ...createdText, title_mode: 'literal' });
  const created = await createdResponse.json() as IssueTemplate;
  expect(created).toMatchObject({ ...createdText, title_mode: 'literal', version: '1' });
  await expect(create).toHaveCount(0);
  expect((await directory()).data).toHaveLength(1);
  expect(await saved.request<IssueTemplate>(`${path}/${created.id}`)).toEqual(created);
  await page.reload();
  await settings.getByRole('button', { name: `Edit ${createdText.name}`, exact: true }).click();
  const edit = page.getByRole('dialog', { name: 'Edit issue template', exact: true });
  const editName = edit.getByRole('textbox', { name: 'Template name', exact: true });
  const editTitle = edit.getByRole('textbox', { name: 'Issue title', exact: false });
  await expect(editName).toHaveValue(createdText.name);
  await expect(editTitle).toHaveValue(createdText.title);
  await editName.press('ControlOrMeta+A');
  await editName.pressSequentially(editedText.name);
  await editTitle.press('ControlOrMeta+A');
  await editTitle.pressSequentially(editedText.title);
  await expect(editName).toHaveValue(editedText.name);
  await expect(editTitle).toHaveValue(editedText.title);
  await editName.pressSequentially('\u{1F308}');
  for (const field of ['name', 'title'] as const) {
    const rejected = { ...editedText, [field]: editedText[field] + (field === 'name' ? '\u{1F308}' : '\u{1F31F}') };
    const [response] = await Promise.all([
      page.waitForResponse(value => value.url() === `${endpoint}/${created.id}` && value.request().method() === 'PATCH'),
      edit.getByRole('button', { name: 'Save template', exact: true }).click(),
    ]);
    expect(response.request().postDataJSON()).toMatchObject({ ...rejected, title_mode: 'literal' });
    expect(response.request().headers()['if-match']).toBe(created.etag);
    expect(response.status()).toBe(422);
    expect(await response.json()).toMatchObject({ code: 'validation_failed' });
    await expect(edit.getByRole('alert').filter({ hasText: field === 'name' ? 'Template name must contain' : 'Template title must contain' })).toBeVisible();
    await expect(editName).toHaveValue(rejected.name);
    await expect(editTitle).toHaveValue(rejected.title);
    await expect(edit).toBeVisible();
    expect(await saved.request<IssueTemplate>(`${path}/${created.id}`)).toEqual(created);
    expect((await directory()).data).toHaveLength(1);
    const input = field === 'name' ? editName : editTitle;
    await input.press('ControlOrMeta+A');
    await input.pressSequentially(editedText[field]);
    if (field === 'name') await editTitle.pressSequentially('\u{1F31F}');
  }
  const [editedResponse] = await Promise.all([
    page.waitForResponse(value => value.url() === `${endpoint}/${created.id}` && value.request().method() === 'PATCH'),
    edit.getByRole('button', { name: 'Save template', exact: true }).click(),
  ]);
  expect(editedResponse.status()).toBe(200);
  expect(editedResponse.request().postDataJSON()).toMatchObject({ ...editedText, title_mode: 'literal' });
  const edited = await editedResponse.json() as IssueTemplate;
  expect(edited).toMatchObject({ ...editedText, title_mode: 'literal' });
  expect(BigInt(edited.version)).toBe(BigInt(created.version) + 1n);
  await expect(edit).toHaveCount(0);
  await page.reload();
  await settings.getByRole('button', { name: `Edit ${editedText.name}`, exact: true }).click();
  await expect(editName).toHaveValue(editedText.name);
  await expect(editTitle).toHaveValue(editedText.title);
  expect(await saved.request<IssueTemplate>(`${path}/${created.id}`)).toEqual(edited);
  expect((await directory()).data).toHaveLength(1);
  expect(writes).toEqual([
    { method: 'POST', status: 422 }, { method: 'POST', status: 422 }, { method: 'POST', status: 201 },
    { method: 'PATCH', status: 422 }, { method: 'PATCH', status: 422 }, { method: 'PATCH', status: 200 },
  ]);
  await test.info().attach('persisted-scalar-receipt', { contentType: 'application/json', body: JSON.stringify({
    created_scalars: { name: Array.from(created.name).length, title: Array.from(created.title).length },
    edited_scalars: { name: Array.from(edited.name).length, title: Array.from(edited.title).length },
    created_version: created.version, edited_version: edited.version, writes,
  }) });
});
