// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { IssueSubscription } from './IssueSubscription';
import type { IssueSubscription as Subscription } from '../mentions/types';
import type { Session } from '../types';
let root: Root; let container: HTMLDivElement; let current: Subscription; let command: (options: RequestInit) => Promise<Response>; let fetchMock: ReturnType<typeof vi.fn>;
const session: Session = { audience: 'web', user: { id: 'u', display_name: 'Example Owner', email: 'owner@example.test' }, csrf_token: 'csrf', workspaces: [] };
const json = (value: unknown, status = 200) => new Response(JSON.stringify(value), { status });
beforeEach(() => { vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true); container = document.createElement('div'); document.body.append(container); root = createRoot(container); current = { issue_id: 'i', preference: 'automatic', sources: ['creator'], subscribed: true, version: '1', etag: '"sub:1"', can_change: true }; command = async options => { current = { ...current, preference: JSON.parse(options.body as string).preference, version: '2', etag: '"sub:2"' }; current.subscribed = current.preference !== 'unsubscribed'; return json(current); }; fetchMock = vi.fn((_url: string, options: RequestInit) => options.method === 'PATCH' ? command(options) : Promise.resolve(json(current))); vi.stubGlobal('fetch', fetchMock); });
afterEach(async () => { await act(async () => root.unmount()); container.remove(); vi.unstubAllGlobals(); });
async function render(account = session, issue = 'i') { await act(async () => root.render(<IssueSubscription workspaceId="w" issueId={issue} session={account}/>)); }
const trigger = () => container.querySelector<HTMLButtonElement>('[aria-label="Issue subscription"]')!;
const choice = (text: string) => [...container.querySelectorAll<HTMLButtonElement>('[role=menuitemradio]')].find(button => button.textContent?.startsWith(text))!;
async function open() { await act(async () => trigger().click()); }
it('uses the server preference and ETag, preserves explicit opt-out, and restores trigger focus', async () => {
  await render(); await open(); expect(container.textContent).toContain('you created this issue'); await act(async () => choice('Unsubscribed').click());
  const write = fetchMock.mock.calls.find(([, options]) => (options as RequestInit).method === 'PATCH')![1] as RequestInit; expect(JSON.parse(write.body as string)).toEqual({ preference: 'unsubscribed' }); expect(new Headers(write.headers).get('If-Match')).toBe('"sub:1"'); expect(trigger().textContent).toContain('Subscribe'); expect(document.activeElement).toBe(trigger());
  await open(); expect(choice('Unsubscribed').getAttribute('aria-checked')).toBe('true'); expect(container.textContent).toContain('including after new assignments or mentions');
});
it('requires an explicit new choice after a conflict instead of overwriting a newer preference', async () => {
  await render(); await open(); command = async () => { current = { ...current, preference: 'unsubscribed', subscribed: false, version: '2', etag: '"sub:2"' }; return json({ code: 'version_conflict', detail: 'Changed', details: { current } }, 412); };
  await act(async () => choice('Subscribed').click()); expect(container.textContent).toContain('changed in another session'); expect(choice('Unsubscribed').getAttribute('aria-checked')).toBe('true'); expect(fetchMock.mock.calls.filter(([, options]) => (options as RequestInit).method === 'PATCH')).toHaveLength(1); expect(document.activeElement).toBe(trigger());
  command = async () => json({ ...current, preference: 'automatic', subscribed: true, version: '3', etag: '"sub:3"' }); await act(async () => choice('Automatic').click()); const write = fetchMock.mock.calls.filter(([, options]) => (options as RequestInit).method === 'PATCH').at(-1)![1] as RequestInit; expect(new Headers(write.headers).get('If-Match')).toBe('"sub:2"');
});
it('reconciles an unknown response without sending a second change', async () => {
  await render(); await open(); command = async () => { current = { ...current, preference: 'unsubscribed', subscribed: false, version: '2', etag: '"sub:2"' }; throw new TypeError('response lost'); };
  await act(async () => choice('Unsubscribed').click()); expect(choice('Subscribed').disabled).toBe(true); const check = [...container.querySelectorAll<HTMLButtonElement>('button')].find(button => button.textContent === 'Check result')!; await act(async () => check.click());
  expect(container.textContent).toContain('Your preference was saved'); expect(choice('Unsubscribed').getAttribute('aria-checked')).toBe('true'); expect(fetchMock.mock.calls.filter(([, options]) => (options as RequestInit).method === 'PATCH')).toHaveLength(1);
});
it('hides stale state after denied auth refresh and confines Escape and arrow keys to its popup', async () => {
  const outer = vi.fn(); window.addEventListener('keydown', outer); await render(); await open(); trigger().focus();
  await act(async () => trigger().dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true, cancelable: true }))); expect(document.activeElement).toBe(choice('Automatic'));
  await act(async () => choice('Automatic').dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }))); expect(outer).not.toHaveBeenCalled(); expect(document.activeElement).toBe(trigger()); expect(container.querySelector('[role=menu]')).toBeNull();
  fetchMock.mockImplementation(() => Promise.resolve(json({ code: 'not_found', detail: 'Unavailable' }, 404))); await render({ ...session, csrf_token: 'new' }); await open(); expect(container.querySelector('[role=menuitemradio]')).toBeNull(); expect(container.textContent).toContain('Subscription unavailable'); window.removeEventListener('keydown', outer);
});
it('keeps a neutral trigger while loading and leaves a failed lookup open for retry', async () => {
  let resolve: (value: Response) => void = () => {}; fetchMock.mockImplementationOnce(() => new Promise<Response>(done => { resolve = done; }));
  await render(); expect(trigger().textContent).toBe('Subscription'); await open(); expect(container.textContent).toContain('Checking subscription');
  await act(async () => resolve(json({ detail: 'Temporarily unavailable' }, 503)));
  const retry = [...container.querySelectorAll<HTMLButtonElement>('button')].find(button => button.textContent === 'Try again')!; await act(async () => retry.click());
  expect(trigger().getAttribute('aria-expanded')).toBe('true'); expect(choice('Automatic').getAttribute('aria-checked')).toBe('true');
});
it.each(['success', 'failure'])('ignores a late conflict fallback %s from the former account scope', async outcome => {
  await render(); await open(); let finish: (value: Response) => void = () => {}; let fallbackSignal: AbortSignal | undefined;
  command = async () => json({ code: 'version_conflict', detail: 'Changed' }, 412);
  fetchMock.mockImplementation((_url: string, options: RequestInit) => {
    if (options.method === 'PATCH') return command(options);
    fallbackSignal = options.signal as AbortSignal; return new Promise<Response>(resolve => { finish = resolve; });
  });
  await act(async () => choice('Subscribed').click()); expect(fallbackSignal).toBeDefined();
  fetchMock.mockImplementation(() => Promise.resolve(json({ ...current, preference: 'unsubscribed', subscribed: false, version: '9', etag: '"new-scope"' })));
  await render({ ...session, csrf_token: 'rotated' }); await open(); expect(fallbackSignal!.aborted).toBe(true);
  await act(async () => finish(outcome === 'success' ? json(current) : json({ detail: 'Former scope denied' }, 403)));
  expect(choice('Unsubscribed').getAttribute('aria-checked')).toBe('true'); expect(container.textContent).not.toContain('Could not verify'); expect(container.textContent).not.toContain('changed in another session');
});
it('refreshes automatic sources without blanking the menu or cancelling a pending preference change', async () => {
  current = { ...current, sources: [], subscribed: false }; await render(); await open(); const triggerNode = trigger(); const automatic = choice('Automatic');
  let finishRead: (value: Response) => void = () => {}; fetchMock.mockImplementationOnce(() => new Promise<Response>(resolve => { finishRead = resolve; }));
  await act(async () => root.render(<IssueSubscription workspaceId="w" issueId="i" session={session} refreshVersion="assigned"/>));
  expect(trigger()).toBe(triggerNode); expect(trigger().textContent).toContain('Subscribe'); expect(choice('Automatic')).toBe(automatic); expect(automatic.disabled).toBe(true);
  current = { ...current, sources: ['assignment'], subscribed: true, version: '2', etag: '"sub:2"' }; await act(async () => finishRead(json(current))); expect(trigger().textContent).toContain('Subscribed');
  let finishWrite: (value: Response) => void = () => {}; let writeSignal: AbortSignal | undefined;
  command = options => { writeSignal = options.signal as AbortSignal; return new Promise<Response>(resolve => { finishWrite = resolve; }); };
  await act(async () => choice('Unsubscribed').click());
  await act(async () => root.render(<IssueSubscription workspaceId="w" issueId="i" session={session} refreshVersion="mentioned"/>));
  expect(writeSignal?.aborted).toBe(false); expect(choice('Unsubscribed').disabled).toBe(true);
  current = { ...current, preference: 'unsubscribed', subscribed: false, version: '3', etag: '"sub:3"' }; await act(async () => finishWrite(json(current))); expect(trigger().textContent).toContain('Subscribe'); await open(); expect(choice('Unsubscribed').getAttribute('aria-checked')).toBe('true');
});
