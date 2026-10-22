// @vitest-environment jsdom
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { fromPlainText, MAX_DOCUMENT_BYTES } from './model';
import { MARKDOWN_REVIEW_TIMEOUT_MS, reviewMarkdownInWorker } from './portability-worker';

class FakeWorker {
  static instances: FakeWorker[] = [];
  onmessage: ((event: MessageEvent) => void) | null = null;
  onerror: ((event: ErrorEvent) => void) | null = null;
  postMessage = vi.fn(); terminate = vi.fn();
  constructor() { FakeWorker.instances.push(this); }
}
beforeEach(() => { FakeWorker.instances = []; vi.stubGlobal('Worker', FakeWorker); vi.useFakeTimers(); });
afterEach(() => { vi.unstubAllGlobals(); vi.useRealTimers(); });
const worker = () => FakeWorker.instances.at(-1)!;

it('returns only validated structured content and terminates its worker on success', async () => {
  const pending = reviewMarkdownInWorker('# Header', new AbortController().signal);
  expect(worker().postMessage).toHaveBeenCalledWith('# Header');
  worker().onmessage!(new MessageEvent('message', { data: { ok: true, result: { body: fromPlainText('Header'), warnings: [] } } }));
  await expect(pending).resolves.toEqual({ body: fromPlainText('Header'), warnings: [] }); expect(worker().terminate).toHaveBeenCalledOnce(); expect(vi.getTimerCount()).toBe(0);
});

it('terminates and rejects overdue parsing without accepting a late result', async () => {
  const pending = reviewMarkdownInWorker('Slow', new AbortController().signal); const observed = expect(pending).rejects.toThrow('too long');
  await vi.advanceTimersByTimeAsync(MARKDOWN_REVIEW_TIMEOUT_MS); await observed;
  worker().onmessage!(new MessageEvent('message', { data: { ok: true, result: { body: fromPlainText('Late'), warnings: [] } } }));
  expect(worker().terminate).toHaveBeenCalledOnce();
});

it('aborts a stale request and removes the timeout', async () => {
  const controller = new AbortController(); const pending = reviewMarkdownInWorker('Stale', controller.signal); const observed = expect(pending).rejects.toMatchObject({ name: 'AbortError' });
  controller.abort(); await observed; expect(worker().terminate).toHaveBeenCalledOnce(); expect(vi.getTimerCount()).toBe(0);
});

it('bounds input and rejects canceled work before creating a worker', async () => {
  const controller = new AbortController(); controller.abort();
  await expect(reviewMarkdownInWorker('Canceled', controller.signal)).rejects.toMatchObject({ name: 'AbortError' });
  await expect(reviewMarkdownInWorker('x'.repeat(MAX_DOCUMENT_BYTES + 1), new AbortController().signal)).rejects.toThrow('too large');
  expect(FakeWorker.instances).toHaveLength(0);
});

it('reports parser failures, startup errors and unavailable worker support without a main-thread fallback', async () => {
  const failed = reviewMarkdownInWorker('Invalid', new AbortController().signal);
  worker().onmessage!(new MessageEvent('message', { data: { ok: false, error: 'Unsafe link.' } }));
  await expect(failed).rejects.toThrow('Unsafe link.'); expect(worker().terminate).toHaveBeenCalledOnce();
  const crashed = reviewMarkdownInWorker('Crash', new AbortController().signal);
  const event = new ErrorEvent('error', { message: 'Internal detail' }); const prevent = vi.spyOn(event, 'preventDefault'); worker().onerror!(event);
  await expect(crashed).rejects.toThrow('Try a smaller document'); expect(prevent).toHaveBeenCalledOnce();
  vi.stubGlobal('Worker', undefined); await expect(reviewMarkdownInWorker('Unavailable', new AbortController().signal)).rejects.toThrow('unavailable');
});

it('rejects an unsupported returned document even if the worker reports success', async () => {
  const pending = reviewMarkdownInWorker('Bad', new AbortController().signal);
  worker().onmessage!(new MessageEvent('message', { data: { ok: true, result: { body: { type: 'doc', content: [{ type: 'iframe' }] }, warnings: [] } } }));
  await expect(pending).rejects.toThrow('unsupported structure'); expect(worker().terminate).toHaveBeenCalledOnce();
});

it('rejects malformed worker messages and warnings without leaving an unhandled event or timeout', async () => {
  for (const data of [null, { ok: true, result: { body: fromPlainText('Text'), warnings: null } }, { ok: true, result: { body: fromPlainText('Text'), warnings: [{ code: 'x', message: { arbitrary: true } }] } }]) {
    const pending = reviewMarkdownInWorker('Bad result', new AbortController().signal);
    worker().onmessage!(new MessageEvent('message', { data }));
    await expect(pending).rejects.toThrow(); expect(worker().terminate).toHaveBeenCalledOnce(); expect(vi.getTimerCount()).toBe(0);
  }
});
