import { test, expect } from './fixtures';
import type { Issue } from '../src/types';

test('supplementary Unicode titles preserve 256 scalars through creation and editing while 257-scalar rejections retain drafts', async ({ page, saved }) => {
  const createScalar = '\u{1F680}';
  const editScalar = '\u{1F30D}';
  const createdTitle = createScalar.repeat(256);
  const editedTitle = editScalar.repeat(256);
  expect(createdTitle.length).toBe(512);
  expect(editedTitle.length).toBe(512);
  await saved.open(page);
  await page.getByRole('button', { name: /^New issue/ }).click();
  const dialog = page.getByRole('dialog', { name: 'New issue', exact: true });
  await dialog.getByRole('combobox', { name: 'Team', exact: true }).selectOption(saved.team.id);
  const createTitle = dialog.getByRole('textbox', { name: 'Title', exact: true });
  await createTitle.pressSequentially(createdTitle);
  await expect(createTitle).toHaveValue(createdTitle);
  expect(Array.from(await createTitle.inputValue())).toHaveLength(256);

  const createURL = `${saved.origin}${saved.path}/issues`;
  await createTitle.pressSequentially(createScalar);
  const rejectedCreateTitle = createdTitle + createScalar;
  await expect(createTitle).toHaveValue(rejectedCreateTitle);
  const [rejectedCreate] = await Promise.all([
    page.waitForResponse(response => response.url() === createURL && response.request().method() === 'POST'),
    dialog.getByRole('button', { name: 'Create issue', exact: true }).click(),
  ]);
  expect(rejectedCreate.request().postDataJSON()).toMatchObject({ team_id: saved.team.id, title: rejectedCreateTitle });
  expect(rejectedCreate.status()).toBe(422);
  expect(await rejectedCreate.json()).toMatchObject({ code: 'validation_failed' });
  await expect(dialog).toBeVisible();
  await expect(createTitle).toHaveValue(rejectedCreateTitle);
  await expect(dialog.getByRole('alert').filter({ hasText: 'Title must contain' })).toBeVisible();

  await createTitle.press('ControlOrMeta+A');
  await createTitle.pressSequentially(createdTitle);
  await expect(createTitle).toHaveValue(createdTitle);
  const [createdResponse] = await Promise.all([
    page.waitForResponse(response => response.url() === createURL && response.request().method() === 'POST'),
    dialog.getByRole('button', { name: 'Create issue', exact: true }).click(),
  ]);
  expect(createdResponse.status()).toBe(201);
  expect(createdResponse.request().headers()['idempotency-key']).toBeTruthy();
  expect(createdResponse.request().postDataJSON()).toMatchObject({ team_id: saved.team.id, title: createdTitle });
  const created = await createdResponse.json() as Issue;
  expect(created).toMatchObject({ workspace_id: saved.workspace, team_id: saved.team.id, title: createdTitle, version: '1' });
  expect(Array.from(created.title)).toHaveLength(256);
  const issueURL = `${createURL}/${created.id}`;
  await expect.poll(() => new URL(page.url()).pathname).toBe(`/w/${saved.workspace}/issues/${created.id}`);
  await expect(dialog).toHaveCount(0);
  const detail = page.getByRole('region', { name: 'Issue detail', exact: true });
  const editTitle = detail.getByRole('textbox', { name: 'Issue title', exact: true });
  await expect(editTitle).toHaveValue(createdTitle);
  expect(await saved.issue(created.id)).toMatchObject({ title: createdTitle, version: created.version });
  await page.reload();
  await expect(editTitle).toHaveValue(createdTitle);

  await editTitle.press('ControlOrMeta+A');
  await editTitle.pressSequentially(editedTitle);
  await expect(editTitle).toHaveValue(editedTitle);
  expect(Array.from(await editTitle.inputValue())).toHaveLength(256);
  await editTitle.pressSequentially(editScalar);
  const rejectedEditTitle = editedTitle + editScalar;
  await expect(editTitle).toHaveValue(rejectedEditTitle);
  const [rejectedEdit] = await Promise.all([
    page.waitForResponse(response => response.url() === issueURL && response.request().method() === 'PATCH'),
    detail.getByRole('button', { name: 'Save changes', exact: true }).click(),
  ]);
  expect(rejectedEdit.request().postDataJSON()).toEqual({ title: rejectedEditTitle });
  expect(rejectedEdit.status()).toBe(422);
  expect(await rejectedEdit.json()).toMatchObject({ code: 'validation_failed' });
  await expect(editTitle).toHaveValue(rejectedEditTitle);
  await expect(detail.getByRole('alert').filter({ hasText: 'Title must contain' })).toBeVisible();
  expect(await saved.issue(created.id)).toMatchObject({ id: created.id, title: createdTitle, version: created.version });

  await editTitle.press('ControlOrMeta+A');
  await editTitle.pressSequentially(editedTitle);
  await expect(editTitle).toHaveValue(editedTitle);
  const [editedResponse] = await Promise.all([
    page.waitForResponse(response => response.url() === issueURL && response.request().method() === 'PATCH'),
    detail.getByRole('button', { name: 'Save changes', exact: true }).click(),
  ]);
  expect(editedResponse.status()).toBe(200);
  expect(editedResponse.request().postDataJSON()).toEqual({ title: editedTitle });
  const edited = await editedResponse.json() as Issue;
  expect(edited.title).toBe(editedTitle);
  expect(Array.from(edited.title)).toHaveLength(256);
  expect(BigInt(edited.version)).toBe(BigInt(created.version) + 1n);
  await expect(detail.getByRole('button', { name: 'Save changes', exact: true })).toBeDisabled();
  await page.reload();
  await expect(editTitle).toHaveValue(editedTitle);
  expect(await saved.issue(created.id)).toMatchObject({ id: created.id, title: editedTitle, version: edited.version });
});
