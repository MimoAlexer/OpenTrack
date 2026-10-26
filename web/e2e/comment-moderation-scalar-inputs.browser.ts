import { registerAdmission } from './fixtures';
import { randomUUID } from 'node:crypto';
import { test, expect, paragraph } from './fixtures';
import type { IssueComment } from '../src/documents/types';
import type { TeamDetail } from '../src/settings/types';
import type { Issue, Session } from '../src/types';
registerAdmission(1);

test('moderation reasons preserve 500 supplementary scalars through edit and deletion while 501 rejections retain drafts and saved versions', async ({ page, saved, playwright }) => {
  const team = await saved.request<TeamDetail>('/teams', 'POST', {
    name: `Moderation scalar acceptance ${randomUUID().slice(0, 8)}`,
    key: `M${randomUUID().replaceAll('-', '').slice(0, 8).toUpperCase()}`, private: true,
  }, { 'Idempotency-Key': randomUUID() });
  const issue = await saved.request<Issue>('/issues', 'POST', { team_id: team.id, title: 'Moderation scalar acceptance' }, { 'Idempotency-Key': randomUUID() });
  const invitation = await saved.request<{ token: string }>('/invitations', 'POST', {
    email: `moderation-${randomUUID()}@example.test`, role: 'member', team_ids: [team.id],
  }, { 'Idempotency-Key': randomUUID() });
  const member = await playwright.request.newContext({ baseURL: saved.origin, extraHTTPHeaders: { Origin: saved.origin }, storageState: { cookies: [], origins: [] } });
  try {
    const accepted = await member.post('/api/v1/invitations/accept', { data: {
      token: invitation.token, display_name: 'Example Member', password: process.env.OPENTRACK_BROWSER_PASSWORD,
    } });
    expect(accepted.status()).toBe(200);
    const account = await accepted.json() as Session;
    expect(account.user.id).not.toBe(saved.session.user.id);
    const posted = await member.post(`${saved.path}/issues/${issue.id}/comments`, {
      headers: { 'X-CSRF-Token': account.csrf_token, 'Idempotency-Key': randomUUID() },
      data: { schema_version: 1, body: paragraph('Original member comment') },
    });
    expect(posted.status()).toBe(201);
    const authored = await posted.json() as IssueComment;
    expect(authored.author.id).toBe(account.user.id);
    const path = `/comments/${authored.id}`;
    const endpoint = `${saved.origin}${saved.path}${path}`;
    const read = () => saved.request<IssueComment>(path);
    const initial = await read();
    expect(initial).toMatchObject({ can_edit: true, can_delete: true, deleted_at: null });
    const editReason = '\u{1F680}'.repeat(500);
    const deleteReason = '\u{1F30D}'.repeat(500);
    const editedText = 'Comment edited by an authorized manager';
    const writes: { method: string; status: number; reason_scalars: number }[] = [];
    page.on('response', response => {
      const request = response.request();
      if (response.url() === endpoint && ['PATCH', 'DELETE'].includes(request.method())) writes.push({
        method: request.method(), status: response.status(), reason_scalars: Array.from(request.postDataJSON().moderation_reason as string).length,
      });
    });
    await saved.open(page, issue);
    const row = page.locator(`#comment-${authored.id}`);
    const action = async (name: string) => {
      await row.locator('.comment-menu summary').click();
      await row.getByRole('button', { name, exact: true }).click();
    };
    await action('Edit comment');
    const edit = row.locator('.comment-edit');
    const editInput = edit.getByRole('textbox', { name: /^Reason for editing/ });
    await editInput.pressSequentially(editReason);
    const nativeEdit = await editInput.inputValue();
    await edit.getByRole('button', { name: 'Cancel edit', exact: true }).click();
    await action('Delete comment');
    const deletion = page.getByRole('dialog', { name: 'Delete comment?', exact: true });
    const deleteInput = deletion.getByRole('textbox', { name: /^Reason for deleting/ });
    await deleteInput.pressSequentially(deleteReason);
    const nativeDelete = await deleteInput.inputValue();
    await test.info().attach('native-boundary-counts', { contentType: 'application/json', body: JSON.stringify({
      edit_reason: { scalars: Array.from(nativeEdit).length, utf16_units: nativeEdit.length },
      delete_reason: { scalars: Array.from(nativeDelete).length, utf16_units: nativeDelete.length },
    }) });
    expect.soft(nativeEdit).toBe(editReason);
    expect.soft(nativeDelete).toBe(deleteReason);
    expect(test.info().errors).toHaveLength(0);
    await deletion.getByRole('button', { name: 'Cancel', exact: true }).click();
    expect(writes).toEqual([]);
    expect(await read()).toEqual(initial);

    await action('Edit comment');
    const body = edit.getByRole('textbox', { name: 'Edit comment', exact: true });
    await body.fill(editedText);
    await editInput.pressSequentially(editReason);
    await expect(editInput).toHaveValue(editReason);
    await editInput.pressSequentially('\u{1F680}');
    const [rejectedEdit] = await Promise.all([
      page.waitForResponse(response => response.url() === endpoint && response.request().method() === 'PATCH'),
      edit.getByRole('button', { name: 'Save comment', exact: true }).click(),
    ]);
    expect(rejectedEdit.request().headers()['if-match']).toBe(initial.etag);
    expect(rejectedEdit.request().postDataJSON()).toEqual({ schema_version: 1, body: paragraph(editedText), moderation_reason: editReason + '\u{1F680}' });
    expect(rejectedEdit.status()).toBe(422);
    expect(await rejectedEdit.json()).toMatchObject({ code: 'validation_failed' });
    await expect(edit.getByRole('alert').filter({ hasText: 'Moderation reason must contain' })).toBeVisible();
    await expect(editInput).toHaveValue(editReason + '\u{1F680}');
    await expect(body).toHaveText(editedText);
    expect(await read()).toEqual(initial);
    await editInput.press('ControlOrMeta+A'); await editInput.pressSequentially(editReason);
    const [editedResponse] = await Promise.all([
      page.waitForResponse(response => response.url() === endpoint && response.request().method() === 'PATCH'),
      edit.getByRole('button', { name: 'Save comment', exact: true }).click(),
    ]);
    expect(editedResponse.status()).toBe(200);
    expect(editedResponse.request().headers()['if-match']).toBe(initial.etag);
    expect(editedResponse.request().postDataJSON()).toEqual({ schema_version: 1, body: paragraph(editedText), moderation_reason: editReason });
    const edited = await editedResponse.json() as IssueComment;
    expect(BigInt(edited.version)).toBe(BigInt(initial.version) + 1n);
    expect(BigInt(edited.document!.version)).toBe(BigInt(initial.document!.version) + 1n);
    expect(edited.document!.body).toEqual(paragraph(editedText));
    await expect(edit).toHaveCount(0);
    await expect(row).toContainText(editedText);
    expect(await read()).toEqual(edited);
    await page.reload(); await expect(row).toContainText(editedText);

    await action('Delete comment');
    await deleteInput.pressSequentially(deleteReason);
    await expect(deleteInput).toHaveValue(deleteReason);
    await deleteInput.pressSequentially('\u{1F30D}');
    const [rejectedDelete] = await Promise.all([
      page.waitForResponse(response => response.url() === endpoint && response.request().method() === 'DELETE'),
      deletion.getByRole('button', { name: 'Delete comment', exact: true }).click(),
    ]);
    expect(rejectedDelete.request().headers()['if-match']).toBe(edited.etag);
    expect(rejectedDelete.request().postDataJSON()).toEqual({ moderation_reason: deleteReason + '\u{1F30D}' });
    expect(rejectedDelete.status()).toBe(422);
    expect(await rejectedDelete.json()).toMatchObject({ code: 'validation_failed' });
    await expect(deletion.getByRole('alert').filter({ hasText: 'Moderation reason must contain' })).toBeVisible();
    await expect(deleteInput).toHaveValue(deleteReason + '\u{1F30D}');
    await expect(deletion).toContainText(editedText);
    await expect(row).not.toHaveClass(/tombstone/);
    expect(await read()).toEqual(edited);
    await deleteInput.press('ControlOrMeta+A'); await deleteInput.pressSequentially(deleteReason);
    const [deletedResponse] = await Promise.all([
      page.waitForResponse(response => response.url() === endpoint && response.request().method() === 'DELETE'),
      deletion.getByRole('button', { name: 'Delete comment', exact: true }).click(),
    ]);
    expect(deletedResponse.status()).toBe(204);
    expect(deletedResponse.request().headers()['if-match']).toBe(edited.etag);
    expect(deletedResponse.request().postDataJSON()).toEqual({ moderation_reason: deleteReason });
    await expect(deletion).toHaveCount(0);
    const deleted = await read();
    expect(deleted.document).toBeNull(); expect(deleted.deleted_at).toBeTruthy();
    expect(BigInt(deleted.version)).toBe(BigInt(edited.version) + 1n);
    await page.reload();
    await expect(row).toHaveClass(/tombstone/);
    await expect(row).not.toContainText(editedText);
    expect(await read()).toEqual(deleted);
    expect(writes).toEqual([
      { method: 'PATCH', status: 422, reason_scalars: 501 }, { method: 'PATCH', status: 200, reason_scalars: 500 },
      { method: 'DELETE', status: 422, reason_scalars: 501 }, { method: 'DELETE', status: 204, reason_scalars: 500 },
    ]);
    await test.info().attach('persisted-scalar-receipt', { contentType: 'application/json', body: JSON.stringify({
      comment_versions: [initial.version, edited.version, deleted.version],
      document_versions_before_deletion: [initial.document!.version, edited.document!.version],
      deleted_document_hidden: deleted.document === null, exact_request_reason_text_verified: true,
      audit_storage_observed: false, writes,
    }) });
  } finally { await member.dispose(); }
});
