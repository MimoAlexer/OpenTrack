import { createHash, randomUUID } from 'node:crypto';
import type { APIResponse, Request, Route } from '@playwright/test';
import { expect, paragraph, registerAdmission, test } from './fixtures';
import { attachmentIds, type RichBody } from '../src/documents/model';
import type { Attachment } from '../src/attachments/types';
import type { Issue } from '../src/types';
import type { TeamDetail } from '../src/settings/types';

registerAdmission(1);

test('a browser picker selection is rejected after the draft team changes', async ({ page, saved }) => {
  const other = await saved.request<TeamDetail>('/teams', 'POST', {
    name: `Attachment picker ${randomUUID()}`,
    key: `A${randomUUID().replaceAll('-', '').slice(0, 8).toUpperCase()}`,
    private: false,
  }, { 'Idempotency-Key': randomUUID() });
  await saved.open(page);
  await page.getByRole('button', { name: /^New issue/ }).click();
  const dialog = page.getByRole('dialog', { name: 'New issue', exact: true });
  const team = dialog.getByRole('combobox', { name: 'Team', exact: true });
  await team.selectOption(saved.team.id);
  await dialog.getByRole('textbox', { name: 'Title', exact: true }).fill('Attachment picker scope');
  const reservations: { name: string; team: string }[] = [];
  const observe = (request: Request) => {
    if (request.method() === 'POST' && request.url() === `${saved.origin}${saved.path}/attachments`) {
      const body = request.postDataJSON() as { original_name: string; team_id: string };
      reservations.push({ name: body.original_name, team: body.team_id });
    }
  };
  page.on('request', observe);
  try {
    const [stale] = await Promise.all([
      page.waitForEvent('filechooser'),
      dialog.getByRole('button', { name: 'Attach files', exact: true }).click(),
    ]);
    await team.selectOption(other.id);
    await stale.setFiles({ name: 'stale-selection.txt', mimeType: 'text/plain', buffer: Buffer.from('Stale picker selection.') });
    await expect(dialog.getByRole('alert')).toContainText('The account, team, or document changed. Choose the file again in the current document.');
    expect(reservations).toEqual([]);
    await expect(dialog.getByRole('list', { name: 'Pending attachments' })).toHaveCount(0);

    const bytes = Buffer.from('Current team attachment.');
    const [current] = await Promise.all([
      page.waitForEvent('filechooser'),
      dialog.getByRole('button', { name: 'Attach files', exact: true }).click(),
    ]);
    await current.setFiles({ name: 'current-selection.txt', mimeType: 'text/plain', buffer: bytes });
    await expect(dialog.getByRole('button', { name: 'Download current-selection.txt', exact: true })).toBeVisible();
    expect(reservations).toEqual([{ name: 'current-selection.txt', team: other.id }]);
    const response = page.waitForResponse(value => value.request().method() === 'POST' && value.url() === `${saved.origin}${saved.path}/issues`);
    void response.catch(() => {});
    await dialog.getByRole('button', { name: 'Create issue', exact: true }).click();
    const createdResponse = await response;
    expect(createdResponse.status()).toBe(201);
    const created = await createdResponse.json() as Issue;
    expect(created.team_id).toBe(other.id);
    await expect.poll(async () => attachmentIds((await saved.document(created.id)).body as RichBody)).toHaveLength(1);
    const ids = attachmentIds((await saved.document(created.id)).body as RichBody);
    expect(await saved.request<Attachment>(`/attachments/${ids[0]}`)).toMatchObject({
      original_name: 'current-selection.txt', state: 'ready', size_bytes: bytes.length,
      sha256: createHash('sha256').update(bytes).digest('hex'),
    });
  } finally { page.off('request', observe); }
});

test('canceling a committed upload before its response arrives marks it deleted without linking the document', async ({ page, saved }) => {
  const bytes = Buffer.from('Committed upload canceled before response.');
  const filename = 'cancel-committed-upload.txt';
  const sha256 = createHash('sha256').update(bytes).digest('hex');
  const issue = await saved.create('Cancel committed attachment', paragraph('Keep the original description.'));
  await page.addInitScript(() => {
    const send = XMLHttpRequest.prototype.send;
    (window as unknown as { attachmentBodies: Promise<number[]>[] }).attachmentBodies = [];
    XMLHttpRequest.prototype.send = function (body?: Document | XMLHttpRequestBodyInit | null) {
      if (body instanceof Blob) (window as unknown as { attachmentBodies: Promise<number[]>[] }).attachmentBodies.push(body.arrayBuffer().then(value => Array.from(new Uint8Array(value))));
      return Reflect.apply(send, this, arguments);
    };
  });
  await saved.open(page, issue);
  const initial = await saved.document(issue.id);
  const description = page.getByRole('region', { name: 'Description', exact: true });
  const prefix = `${saved.origin}${saved.path}/attachments/`;
  const operations: Promise<void>[] = []; const errors: unknown[] = [];
  let uploaded: Attachment | undefined; let release!: () => void; let canceled = false;
  const responseHeld = new Promise<void>(resolve => { release = resolve; });
  const isContent = (url: URL) => url.origin === new URL(saved.origin).origin && !url.search && !url.hash
    && url.pathname.startsWith(`${saved.path}/attachments/`)
    && /^[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}\/content$/i.test(url.pathname.slice(`${saved.path}/attachments/`.length));
  const hold = (route: Route): Promise<void> => {
    if (route.request().method() !== 'PUT') return route.fallback();
    const operation = (async () => {
      let response: APIResponse | undefined;
      try {
        const observed = await page.evaluate(async () => {
          const bodies = (window as unknown as { attachmentBodies: Promise<number[]>[] }).attachmentBodies;
          return Promise.all(bodies);
        });
        expect(observed).toHaveLength(1);
        expect(Buffer.from(observed[0]!)).toEqual(bytes);
        response = await route.fetch({ postData: Buffer.from(observed[0]!), maxRetries: 0, maxRedirects: 0 });
        expect(response.status()).toBe(200);
        uploaded = await response.json() as Attachment;
        expect(uploaded).toMatchObject({ original_name: filename, state: 'ready', sha256, size_bytes: bytes.length });
        await responseHeld;
        // The real XHR may already have aborted when cancellation was requested.
        await route.abort('failed').catch(error => { if (!canceled) errors.push(error); });
      } catch (error) { errors.push(error); await route.abort('failed').catch(abortError => { if (!canceled) errors.push(abortError); }); }
      finally { await response?.dispose(); }
    })();
    operations.push(operation); return operation;
  };
  await page.route(isContent, hold);
  try {
    const [chooser] = await Promise.all([
      page.waitForEvent('filechooser'),
      description.getByRole('button', { name: 'Attach files', exact: true }).click(),
    ]);
    await chooser.setFiles({ name: filename, mimeType: 'text/plain', buffer: bytes });
    await expect.poll(() => !!uploaded || errors.length > 0).toBe(true);
    if (errors.length) throw errors[0];
    expect((await saved.document(issue.id)).body).toEqual(initial.body);
    const deleted = page.waitForResponse(value => value.request().method() === 'DELETE' && value.url() === `${prefix}${uploaded!.id}`);
    void deleted.catch(() => {});
    canceled = true;
    await description.getByRole('button', { name: `Cancel ${filename}`, exact: true }).click();
    expect((await deleted).status()).toBe(204);
    await expect(description.getByRole('list', { name: 'Pending attachments' })).toHaveCount(0);
    release();
    await expect.poll(async () => (await saved.request<Attachment>(`/attachments/${uploaded!.id}`)).state).toBe('deleted');
    const download = await saved.http.get(`${saved.path}/attachments/${uploaded!.id}/content`);
    try { expect(download.status()).toBe(404); } finally { await download.dispose(); }
    expect(attachmentIds((await saved.document(issue.id)).body as RichBody)).toEqual([]);
    await expect(description.getByRole('button', { name: `Download ${filename}`, exact: true })).toHaveCount(0);
    await page.reload();
    await expect(page.getByRole('textbox', { name: 'Description', exact: true })).toBeVisible();
    expect((await saved.document(issue.id)).body).toEqual(initial.body);
  } finally {
    release();
    try { await page.unroute(isContent, hold); } catch (error) { errors.push(error); }
    for (const result of await Promise.allSettled(operations)) if (result.status === 'rejected') errors.push(result.reason);
  }
  if (errors.length) throw errors[0];
});
