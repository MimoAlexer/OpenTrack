// @vitest-environment jsdom
import { act, type ReactNode } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { DesktopContext } from '../desktop/context';
import type { DesktopSnapshot, DesktopStore } from '../desktop/store';
import type { ActivityEntry } from '../types';
import { IssueActivity } from './IssueActivity';

const entry: ActivityEntry = { id: 'event-1', event_kind: 'issue.updated', actor: 'Example Member', created_at: '2026-10-17T10:30:00Z', changes: { fields: ['title'] } };
const json = (body: unknown, status = 200) => new Response(JSON.stringify(body), { status });
const page = (entries = [entry]) => json({ data: entries });
let container: HTMLDivElement; let root: Root;
beforeEach(() => { vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true); container = document.createElement('div'); document.body.append(container); root = createRoot(container); });
afterEach(async () => { await act(async () => root.unmount()); container.remove(); vi.restoreAllMocks(); vi.unstubAllGlobals(); });
async function render(node: ReactNode = <IssueActivity workspaceId="w" issueId="i" version="1" authScope="session-one" />) { await act(async () => root.render(node)); }

it('retains initial loading until the first activity snapshot arrives', async () => {
  let finish!: (response: Response) => void;
  vi.stubGlobal('fetch', vi.fn(() => new Promise<Response>(resolve => { finish = resolve; })));
  await render();
  expect(container.querySelector('.activity-loading')).not.toBeNull(); expect(container.querySelector('.activity-list')).toBeNull();
  await act(async () => finish(page()));
  expect(container.textContent).toContain('Example Member changed title.'); expect(container.querySelector('.activity-loading')).toBeNull();
});

it('keeps the timeline row mounted when a new issue version refreshes activity', async () => {
  let finish!: (response: Response) => void;
  vi.stubGlobal('fetch', vi.fn().mockResolvedValueOnce(page()).mockImplementationOnce(() => new Promise<Response>(resolve => { finish = resolve; })));
  await render(); const row = container.querySelector('li');
  await render(<IssueActivity workspaceId="w" issueId="i" version="2" authScope="session-one" />);
  expect(container.querySelector('li')).toBe(row); expect(container.querySelector('.activity-loading')).toBeNull(); expect(container.querySelector('header')?.textContent).toContain('Updating…');
  await act(async () => finish(page([entry, { ...entry, id: 'event-2', actor: 'Example Owner' }])));
  expect(container.querySelector('li')).toBe(row); expect(container.textContent).toContain('Example Owner changed title.'); expect(container.textContent).not.toContain('Updating…');
});

it('keeps activity after a transient failure and throughout an explicit retry', async () => {
  let finish!: (response: Response) => void;
  vi.stubGlobal('fetch', vi.fn().mockResolvedValueOnce(page()).mockRejectedValueOnce(new TypeError('Offline')).mockImplementationOnce(() => new Promise<Response>(resolve => { finish = resolve; })));
  await render(); const row = container.querySelector('li');
  await render(<IssueActivity workspaceId="w" issueId="i" version="2" authScope="session-one" />);
  expect(container.querySelector('li')).toBe(row); expect(container.textContent).toContain('Could not reach OpenTrack');
  await act(async () => [...container.querySelectorAll('button')].find(button => button.textContent === 'Try again')!.click());
  expect(container.querySelector('li')).toBe(row); expect(container.textContent).toContain('Updating…');
  await act(async () => finish(page())); expect(container.querySelector('li')).toBe(row);
});

it.each(['workspace', 'issue', 'auth'] as const)('hides old activity while a changed %s scope loads', async change => {
  let finish!: (response: Response) => void;
  vi.stubGlobal('fetch', vi.fn().mockResolvedValueOnce(page()).mockImplementationOnce(() => new Promise<Response>(resolve => { finish = resolve; })));
  await render();
  await render(<IssueActivity workspaceId={change === 'workspace' ? 'other-workspace' : 'w'} issueId={change === 'issue' ? 'other-issue' : 'i'} version="1" authScope={change === 'auth' ? 'session-two' : 'session-one'} />);
  expect(container.textContent).not.toContain('Example Member'); expect(container.querySelector('li')).toBeNull(); expect(container.querySelector('.activity-loading')).not.toBeNull();
  await act(async () => finish(page([{ ...entry, actor: 'Current account' }])));
  expect(container.textContent).toContain('Current account'); expect(container.textContent).not.toContain('Example Member');
});

it('hides cached activity on a native generation change even without an explicit browser auth scope', async () => {
  let finish!: (response: Response) => void;
  vi.stubGlobal('fetch', vi.fn().mockResolvedValueOnce(page()).mockImplementationOnce(() => new Promise<Response>(resolve => { finish = resolve; })));
  const view = (generation: string) => {
    const snapshot: DesktopSnapshot = { connection: { generation, scope_generation: '1', selected: { origin: 'https://work.example.test', development_http: false }, credential_present: true, pending: null }, session: null, issueLink: null, authEvent: null, loading: false, error: null };
    return <DesktopContext.Provider value={{ snapshot, store: {} as DesktopStore }}><IssueActivity workspaceId="w" issueId="i" version="1" /></DesktopContext.Provider>;
  };
  await render(view('1')); await render(view('2'));
  expect(container.textContent).not.toContain('Example Member'); expect(container.querySelector('.activity-loading')).not.toBeNull();
  await act(async () => finish(page([]))); expect(container.textContent).not.toContain('Example Member');
});

it.each([401, 403, 404])('clears cached activity when refresh denies authorization with %s', async status => {
  vi.stubGlobal('fetch', vi.fn().mockResolvedValueOnce(page()).mockResolvedValueOnce(json({ code: 'not_available', detail: 'Activity unavailable.' }, status)));
  await render(); await render(<IssueActivity workspaceId="w" issueId="i" version="2" authScope="session-one" />);
  expect(container.querySelector('li')).toBeNull(); expect(container.textContent).not.toContain('Example Member'); expect(container.textContent).toContain('Activity unavailable.');
});

it('ignores a late refresh response after authentication scope changes', async () => {
  let old!: (response: Response) => void; let current!: (response: Response) => void;
  vi.stubGlobal('fetch', vi.fn().mockResolvedValueOnce(page()).mockImplementationOnce(() => new Promise<Response>(resolve => { old = resolve; })).mockImplementationOnce(() => new Promise<Response>(resolve => { current = resolve; })));
  await render(); await render(<IssueActivity workspaceId="w" issueId="i" version="2" authScope="session-one" />);
  await render(<IssueActivity workspaceId="w" issueId="i" version="2" authScope="session-two" />);
  await act(async () => old(page([{ ...entry, actor: 'Old account' }])));
  expect(container.querySelector('li')).toBeNull(); expect(container.textContent).not.toContain('Old account');
  await act(async () => current(page([{ ...entry, actor: 'New account' }])));
  expect(container.textContent).toContain('New account'); expect(container.textContent).not.toContain('Old account');
});

it.each([200, 403])('ignores a delayed older-page response after a same-issue refresh (%s)', async status => {
  let older!: (response: Response) => void; let signal!: AbortSignal;
  const fetch = vi.fn().mockResolvedValueOnce(json({ data: [entry], has_more: true, next_cursor: 'older' }))
    .mockImplementationOnce((_url: string, options: RequestInit) => { signal = options.signal!; return new Promise<Response>(resolve => { older = resolve; }); })
    .mockResolvedValueOnce(page([{ ...entry, actor: 'Current reader' }]));
  vi.stubGlobal('fetch', fetch);
  await render();
  await act(async () => container.querySelector<HTMLButtonElement>('.comments-older')!.click());
  await render(<IssueActivity workspaceId="w" issueId="i" version="2" authScope="session-one" />);
  expect(signal.aborted).toBe(true);
  await act(async () => older(status === 200 ? page([{ ...entry, id: 'old', actor: 'Stale reader' }]) : json({ detail: 'Stale access denial' }, status)));
  expect(container.textContent).toContain('Current reader'); expect(container.textContent).not.toContain('Stale reader'); expect(container.textContent).not.toContain('Stale access denial');
});

it('does not restore older rows after a refresh revokes access', async () => {
  let older!: (response: Response) => void;
  vi.stubGlobal('fetch', vi.fn().mockResolvedValueOnce(json({ data: [entry], has_more: true, next_cursor: 'older' }))
    .mockImplementationOnce(() => new Promise<Response>(resolve => { older = resolve; }))
    .mockResolvedValueOnce(json({ detail: 'Activity unavailable.' }, 403)));
  await render(); await act(async () => container.querySelector<HTMLButtonElement>('.comments-older')!.click());
  await render(<IssueActivity workspaceId="w" issueId="i" version="2" authScope="session-one" />);
  await act(async () => older(page([{ ...entry, id: 'old', actor: 'Stale reader' }])));
  expect(container.querySelector('li')).toBeNull(); expect(container.textContent).toContain('Activity unavailable.'); expect(container.textContent).not.toContain('Stale reader');
});
