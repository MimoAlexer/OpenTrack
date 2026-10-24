import { createHash } from 'node:crypto';
import type { APIResponse, Request, Route } from '@playwright/test';
import { test, expect, paragraph } from './fixtures';
import { attachmentIds, plainText, type RichBody } from '../src/documents/model';
import type { Attachment } from '../src/attachments/types';

type AttachmentSend = { url: string; original_send_succeeded: boolean; size: number | null; body: Promise<number[] | null> };
type AttachmentSendWitness = { sends: AttachmentSend[]; overflow: boolean;
  open: typeof XMLHttpRequest.prototype.open; send: typeof XMLHttpRequest.prototype.send };

test('ordinary browser attachment upload commits its real file bytes and permits an authorized exact download', async ({ page, saved }) => {
  const filename = 'response-recovery-control.txt';
  const bytes = Buffer.from('Benign attachment response-recovery control.\n');
  const sha256 = createHash('sha256').update(bytes).digest('hex');
  const issue = await saved.create('Ordinary attachment upload control', paragraph('Ordinary browser upload.'));
  await saved.open(page, issue);
  const initial = await saved.document(issue.id);
  const description = page.getByRole('region', { name: 'Description', exact: true });
  const [chooser] = await Promise.all([
    page.waitForEvent('filechooser'),
    description.getByRole('button', { name: 'Attach files', exact: true }).click(),
  ]);
  const prefix = `${saved.origin}${saved.path}/attachments/`;
  const [put] = await Promise.all([
    page.waitForResponse(response => response.request().method() === 'PUT'
      && response.url().startsWith(prefix) && response.url().endsWith('/content')),
    chooser.setFiles({ name: filename, mimeType: 'text/plain', buffer: bytes }),
  ]);
  expect(put.status()).toBe(200);
  const uploaded = await put.json() as Attachment;
  expect(put.url()).toBe(`${prefix}${uploaded.id}/content`);
  expect(uploaded).toMatchObject({ original_name: filename, size_bytes: bytes.length, sha256, state: 'ready' });
  await expect.poll(async () => attachmentIds((await saved.document(issue.id)).body as RichBody)).toEqual([uploaded.id]);
  expect(await saved.request<Attachment>(`/attachments/${uploaded.id}`)).toMatchObject({
    id: uploaded.id, document_id: initial.id, original_name: filename, size_bytes: bytes.length, sha256, state: 'ready',
  });
  const downloaded = await saved.http.get(`${saved.path}/attachments/${uploaded.id}/content`);
  try {
    expect(downloaded.status()).toBe(200);
    const received = await downloaded.body();
    expect(received).toEqual(bytes);
    expect(createHash('sha256').update(received).digest('hex')).toBe(sha256);
  } finally { await downloaded.dispose(); }
});

test('a lost committed upload response keeps its pending draft and retries the same ready attachment without resending bytes', async ({ page, saved }) => {
  const original = 'Saved text before the upload.';
  const additional = ' More text typed while its response is held.';
  const filename = 'response-recovery-control.txt';
  const bytes = Buffer.from('Benign attachment response-recovery control.\n');
  const sha256 = createHash('sha256').update(bytes).digest('hex');
  const issue = await saved.create('Attachment response recovery', paragraph(original));
  await page.addInitScript(() => {
    const originalOpen = XMLHttpRequest.prototype.open;
    const originalSend = XMLHttpRequest.prototype.send;
    const state: AttachmentSendWitness = { sends: [], overflow: false, open: originalOpen, send: originalSend };
    (window as unknown as { __attachmentSendWitness: AttachmentSendWitness }).__attachmentSendWitness = state;
    const opened = new WeakMap<XMLHttpRequest, string>();
    XMLHttpRequest.prototype.open = function (this: XMLHttpRequest, method: string, url: string | URL, ..._rest: unknown[]) {
      opened.delete(this);
      const result = Reflect.apply(originalOpen, this, arguments);
      try {
        const target = new URL(url, location.href);
        if (method.toUpperCase() === 'PUT' && target.origin === location.origin && !target.search && !target.hash
          && /^\/api\/v1\/workspaces\/[0-9a-f-]{36}\/attachments\/[0-9a-f-]{36}\/content$/i.test(target.pathname)) opened.set(this, target.href);
      } catch { /* Unrelated request observation must not change the successful open. */ }
      return result;
    };
    XMLHttpRequest.prototype.send = function (this: XMLHttpRequest, body?: Document | XMLHttpRequestBodyInit | null) {
      const url = opened.get(this);
      let observed: AttachmentSend | undefined;
      if (url) {
        if (state.sends.length >= 8) state.overflow = true;
        else {
          observed = { url, original_send_succeeded: false, size: body instanceof Blob ? body.size : null, body: Promise.resolve(null) };
          state.sends.push(observed);
        }
      }
      const result = Reflect.apply(originalSend, this, arguments);
      if (observed) {
        observed.original_send_succeeded = true;
        if (body instanceof Blob && body.size <= 1024) {
          observed.body = Promise.resolve().then(() => body.arrayBuffer())
            .then(value => Array.from(new Uint8Array(value))).catch(() => null);
        }
      }
      return result;
    };
  });
  await saved.open(page, issue);
  const description = page.getByRole('region', { name: 'Description', exact: true });
  const editor = description.getByRole('textbox', { name: 'Description', exact: true });
  const pending = description.getByRole('list', { name: 'Pending attachments', exact: true });
  const initial = await saved.document(issue.id);
  await expect(editor).toHaveText(original);
  expect(initial.body).toEqual(paragraph(original));
  const reserveURL = `${saved.origin}${saved.path}/attachments`;
  const contentPrefix = `${saved.path}/attachments/`;
  const reservationRequests: { key: string | undefined; body: string | null }[] = [];
  const putRequests: string[] = [];
  const handlerErrors: unknown[] = [];
  const operations: Promise<void>[] = [];
  let forwarded = 0;
  let ready: Attachment | undefined;
  let metadataRead: Attachment | undefined;
  let metadataIntercepted = false;
  let metadataURL: string | undefined;
  let releasePut!: () => void;
  const delivery = new Promise<void>(resolve => { releasePut = resolve; });
  let failure: unknown = null;
  const matchesContent = (url: URL) => url.origin === new URL(saved.origin).origin
    && !url.search && url.pathname.startsWith(contentPrefix)
    && /^[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}\/content$/i.test(url.pathname.slice(contentPrefix.length));
  const observe = (request: Request) => {
    if (request.method() === 'POST' && request.url() === reserveURL) {
      reservationRequests.push({ key: request.headers()['idempotency-key'], body: request.postData() });
    }
    if (request.method() === 'PUT' && matchesContent(new URL(request.url()))) putRequests.push(request.url());
  };
  const holdCommittedPut = (route: Route): Promise<void> => {
    if (route.request().method() !== 'PUT') return route.fallback();
    const operation = (async () => {
      let response: APIResponse | undefined;
      let ended = false;
      try {
        expect(forwarded, 'Only the first owned content PUT may reach the server').toBe(0);
        const content = new URL(route.request().url());
        const id = content.pathname.slice(contentPrefix.length, -'/content'.length);
        const reservation = await saved.request<Attachment>(`/attachments/${id}`);
        expect(reservation).toMatchObject({ id, document_id: null, original_name: filename,
          size_bytes: bytes.length, sha256, state: 'pending' });
        expect(route.request().headers()['if-match']).toBe(reservation.etag);
        const observed = await page.evaluate(async url => {
          const state = (window as unknown as { __attachmentSendWitness?: AttachmentSendWitness }).__attachmentSendWitness;
          return {
            present: state !== undefined, overflow: state?.overflow ?? false,
            sends: await Promise.all((state?.sends ?? []).filter(entry => entry.url === url).map(async entry => ({
              url: entry.url, original_send_succeeded: entry.original_send_succeeded, size: entry.size, body: await entry.body,
            }))),
          };
        }, route.request().url());
        expect(observed.present).toBe(true); expect(observed.overflow).toBe(false);
        expect(observed.sends).toHaveLength(1);
        const sent = observed.sends[0]!;
        expect(sent.url).toBe(route.request().url());
        expect(sent.original_send_succeeded).toBe(true);
        expect(sent.size).toBe(bytes.length); expect(sent.size).toBeLessThanOrEqual(1024);
        expect(sent.body).not.toBeNull();
        const actualObservedBody = Buffer.from(sent.body!);
        expect(actualObservedBody).toEqual(bytes);
        expect(createHash('sha256').update(actualObservedBody).digest('hex')).toBe(sha256);
        const protocolBody = route.request().postDataBuffer();
        if (protocolBody !== null) expect(protocolBody).toEqual(actualObservedBody);
        const contentLength = route.request().headers()['content-length'];
        if (contentLength !== undefined) {
          expect(contentLength).toMatch(/^[0-9]+$/);
          expect(Number(contentLength)).toBe(actualObservedBody.byteLength);
        }
        forwarded++;
        // Interception relays only the bounded bytes passed to the original XHR send.
        response = await route.fetch({ postData: actualObservedBody, maxRetries: 0, maxRedirects: 0 });
        expect(response.status()).toBe(200);
        const committed = await response.json() as Attachment;
        expect(committed).toMatchObject({ id, document_id: null, original_name: filename,
          size_bytes: bytes.length, sha256, state: 'ready' });
        expect(response.headers()['etag']).toBe(committed.etag);
        ready = committed;
        await delivery;
        await route.abort('failed');
        ended = true;
      } catch (error) { handlerErrors.push(error); }
      finally {
        if (!ended) {
          try { await route.abort('failed'); } catch (error) { handlerErrors.push(error); }
        }
        try { await response?.dispose(); } catch (error) { handlerErrors.push(error); }
      }
    })();
    operations.push(operation);
    return operation;
  };
  const inspectRetryRead = (route: Route): Promise<void> => {
    if (route.request().method() !== 'GET' || metadataIntercepted) return route.fallback();
    metadataIntercepted = true;
    const operation = (async () => {
      let response: APIResponse | undefined;
      let ended = false;
      try {
        expect(route.request().url()).toBe(metadataURL);
        expect(await description.locator('.attachment-card').count(),
          'The status read must precede insertion, rather than just reauthorize an inserted card').toBe(0);
        expect(attachmentIds((await saved.document(issue.id)).body as RichBody)).toEqual([]);
        response = await route.fetch({ maxRetries: 0, maxRedirects: 0 });
        expect(response.status()).toBe(200);
        const current = await response.json() as Attachment;
        expect(current).toMatchObject({ id: ready!.id, document_id: null, state: 'ready', size_bytes: bytes.length, sha256 });
        expect(response.headers()['etag']).toBe(current.etag);
        metadataRead = current;
        await route.fulfill({ response });
        ended = true;
      } catch (error) { handlerErrors.push(error); }
      finally {
        if (!ended) {
          try { await route.abort('failed'); } catch (error) { handlerErrors.push(error); }
        }
        try { await response?.dispose(); } catch (error) { handlerErrors.push(error); }
      }
    })();
    operations.push(operation);
    return operation;
  };
  page.on('request', observe);
  await page.route(matchesContent, holdCommittedPut);
  try {
    const reserved = page.waitForResponse(response => response.request().method() === 'POST' && response.url() === reserveURL);
    // Keep a preceding chooser failure from leaving an unhandled event-wait rejection.
    void reserved.catch(() => {});
    const chooser = page.waitForEvent('filechooser');
    void chooser.catch(() => {});
    await description.getByRole('button', { name: 'Attach files', exact: true }).click();
    await (await chooser).setFiles({ name: filename, mimeType: 'text/plain', buffer: bytes });
    const reservationResponse = await reserved;
    expect(reservationResponse.status()).toBe(201);
    const reservation = await reservationResponse.json() as Attachment;
    expect(reservation).toMatchObject({ document_id: null, original_name: filename,
      size_bytes: bytes.length, sha256, state: 'pending' });
    await expect.poll(() => !!ready || handlerErrors.length > 0).toBe(true);
    if (handlerErrors.length) throw handlerErrors[0];
    expect(ready!.id).toBe(reservation.id);
    expect(ready!.etag).not.toBe(reservation.etag);
    expect(reservationRequests).toHaveLength(1);
    expect(reservationRequests[0].key).toBeTruthy();
    expect(JSON.parse(reservationRequests[0].body!)).toEqual({ document_id: initial.id,
      original_name: filename, size_bytes: bytes.length, sha256, mime_hint: 'text/plain' });
    expect(await saved.request<Attachment>(`/attachments/${reservation.id}`)).toMatchObject({ id: reservation.id, document_id: null, state: 'ready', sha256 });
    await expect(pending).toBeVisible();
    expect((await saved.document(issue.id)).body).toEqual(initial.body);
    await expect(editor).toHaveText(original);
    const beforeClose = page.url();
    const [guard] = await Promise.all([
      page.waitForEvent('dialog').then(async dialog => {
        const observed = { type: dialog.type(), message: dialog.message() };
        await dialog.dismiss();
        return observed;
      }),
      page.getByRole('button', { name: 'Close issue', exact: true }).click(),
    ]);
    expect(guard).toEqual({ type: 'confirm', message: 'Discard your unsaved changes?' });
    expect(page.url()).toBe(beforeClose);
    await expect(pending).toBeVisible();
    await expect(editor).toHaveText(original);
    expect((await saved.document(issue.id)).body).toEqual(initial.body);
    await editor.press('End');
    await editor.pressSequentially(additional);
    await expect(editor).toHaveText(original + additional);
    releasePut();
    await expect(pending).toContainText('Upload needs attention');
    await expect(pending.getByRole('alert')).toContainText('interrupted');
    await expect(pending.getByRole('button', { name: 'Retry', exact: true })).toBeVisible();
    expect(attachmentIds((await saved.document(issue.id)).body as RichBody)).toEqual([]);
    await expect(editor).toHaveText(original + additional);
    metadataURL = `${reserveURL}/${reservation.id}`;
    await page.route(metadataURL, inspectRetryRead);
    await pending.getByRole('button', { name: 'Retry', exact: true }).click();
    await expect.poll(() => !!metadataRead || handlerErrors.length > 0).toBe(true);
    if (handlerErrors.length) throw handlerErrors[0];
    expect(metadataRead!.id).toBe(reservation.id);
    await expect(pending).toHaveCount(0);
    await expect(description.getByRole('button', { name: `Download ${filename}`, exact: true })).toBeVisible();
    expect(reservationRequests).toHaveLength(1);
    expect(putRequests).toEqual([`${reserveURL}/${reservation.id}/content`]);
    expect(forwarded).toBe(1);
    await expect.poll(async () => {
      const value = await saved.document(issue.id);
      return { ids: attachmentIds(value.body as RichBody), text: plainText(value.body as RichBody).replace(/\s+/g, ' ').trim() };
    }).toEqual({ ids: [reservation.id], text: original + additional });
    const persisted = await saved.document(issue.id);
    expect(await page.evaluate(url => {
      const state = (window as unknown as { __attachmentSendWitness?: AttachmentSendWitness }).__attachmentSendWitness;
      return { overflow: state?.overflow ?? true, total: state?.sends.filter(entry => entry.url === url).length ?? 0,
        successful: state?.sends.filter(entry => entry.url === url && entry.original_send_succeeded).length ?? 0 };
    }, `${reserveURL}/${reservation.id}/content`)).toEqual({ overflow: false, total: 1, successful: 1 });
    await page.reload();
    await expect(editor).toBeVisible();
    await expect(pending).toHaveCount(0);
    await expect(description.locator(`.attachment-card[data-attachment-id="${reservation.id}"]`)).toHaveCount(1);
    expect((await saved.document(issue.id)).body).toEqual(persisted.body);
    expect(await saved.request<Attachment>(`/attachments/${reservation.id}`)).toMatchObject({ id: reservation.id, document_id: initial.id,
      state: 'ready', size_bytes: bytes.length, sha256 });
    const inventory = await saved.request<{ data: Attachment[]; has_more: boolean }>(`/attachments?document_id=${initial.id}&limit=20`);
    expect(inventory.data.map(value => value.id)).toEqual([reservation.id]);
    expect(inventory.has_more).toBe(false);
    const downloaded = page.waitForEvent('download');
    void downloaded.catch(() => {});
    await description.getByRole('button', { name: `Download ${filename}`, exact: true }).click();
    const download = await downloaded;
    expect(download.suggestedFilename()).toBe(filename);
    const stream = await download.createReadStream();
    expect(stream).not.toBeNull();
    const chunks: Buffer[] = [];
    for await (const chunk of stream!) chunks.push(Buffer.from(chunk));
    const received = Buffer.concat(chunks);
    expect(received).toEqual(bytes);
    expect(createHash('sha256').update(received).digest('hex')).toBe(sha256);
    expect(reservationRequests).toHaveLength(1);
    expect(putRequests).toHaveLength(1);
  } catch (error) { failure = error; }
  finally {
    releasePut();
    try { await page.unroute(matchesContent, holdCommittedPut); } catch (error) { handlerErrors.push(error); }
    if (metadataURL) {
      try { await page.unroute(metadataURL, inspectRetryRead); } catch (error) { handlerErrors.push(error); }
    }
    const settled = await Promise.allSettled(operations);
    for (const result of settled) if (result.status === 'rejected') handlerErrors.push(result.reason);
    page.off('request', observe);
    try {
      await page.evaluate(() => {
        const target = window as unknown as { __attachmentSendWitness?: AttachmentSendWitness };
        const state = target.__attachmentSendWitness;
        if (!state) return;
        XMLHttpRequest.prototype.open = state.open;
        XMLHttpRequest.prototype.send = state.send;
        for (const entry of state.sends) entry.body = Promise.resolve(null);
        state.sends.length = 0;
        delete target.__attachmentSendWitness;
      });
    } catch (error) { handlerErrors.push(error); }
    if (failure === null && handlerErrors.length) failure = handlerErrors[0];
  }
  if (failure !== null) throw failure;
});
