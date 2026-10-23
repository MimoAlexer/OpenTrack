// @vitest-environment jsdom
import { act, createRef } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import type { Bootstrap, Issue, Session, Status } from '../types';
import { IssueList } from './IssueList';
import { restoreIssueRowFocus } from './focus';
import { issuesUrl, navigate, parseRoute, useLocation } from '../router';

const session: Session = { audience: 'web', user: { id: 'u', display_name: 'Owner', email: 'owner@example.test' }, csrf_token: 'csrf-one', workspaces: [{ id: 'w', name: 'Workspace', slug: 'workspace', role: 'owner' }] };
const status: Status = { id: 's', team_id: 't', name: 'Ready', category: 'unstarted', color: 'slate', position: 0, is_default: true, archived_at: null, retiring_at: null, replacement_status_id: null, replacement_job_id: null };
const issue: Issue = { id: 'i', workspace_id: 'w', team_id: 't', key: 'ENG-1', title: 'Keep this issue visible', description: '', status_id: 's', priority: 0, assignee_id: null, assignee: null, label_ids: [], labels: [], etag: '"issue:i:1:projection"', estimate: null, due_date: null, version: '1', created_at: '2026-10-17T10:00:00Z', updated_at: '2026-10-17T10:00:00Z', archived_at: null };
const data: Bootstrap = { auth_epoch: '1', teams: [{ id: 't', key: 'ENG', name: 'Engineering', private: false, archived_at: null }], statuses: [status], members: [session.user], issues: [] };
const json = (body: unknown, status = 200) => new Response(JSON.stringify(body), { status });
const page = (issues = [issue]) => json({ data: issues, next_cursor: null, has_more: false });
let container: HTMLDivElement; let root: Root;
beforeEach(() => { vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true); container = document.createElement('div'); document.body.append(container); root = createRoot(container); });
afterEach(async () => { await act(async () => root.unmount()); container.remove(); vi.useRealTimers(); vi.restoreAllMocks(); vi.unstubAllGlobals(); });
async function render(refresh = 0, params = new URLSearchParams(), account = session, workspaceId = 'w') { await act(async () => root.render(<IssueList workspaceId={workspaceId} data={data} session={account} params={params} selectedId="i" refresh={refresh} create={() => {}} searchRef={createRef()} />)); }

it('keeps the same row and scroll container mounted while a same-scope refresh is pending', async () => {
  let finish!: (response: Response) => void;
  vi.stubGlobal('fetch', vi.fn().mockResolvedValueOnce(page()).mockImplementationOnce(() => new Promise<Response>(resolve => { finish = resolve; })));
  await render(); const row = container.querySelector('#row-i'); const list = container.querySelector('.issue-list');
  await render(1);
  expect(container.querySelector('#row-i')).toBe(row); expect(container.querySelector('.issue-list')).toBe(list);
  expect(container.querySelector('.loading')).toBeNull(); expect(container.textContent).toContain('Updating…'); expect(row?.getAttribute('aria-current')).toBe('page');
  await act(async () => finish(page([{ ...issue, title: 'Confirmed updated title' }])));
  expect(container.querySelector('#row-i')).toBe(row); expect(container.textContent).toContain('Confirmed updated title'); expect(container.textContent).not.toContain('Updating…');
});

it.each(['workspace', 'auth'] as const)('hides old rows while a changed %s scope loads', async change => {
  let finish!: (response: Response) => void;
  vi.stubGlobal('fetch', vi.fn().mockResolvedValueOnce(page()).mockImplementationOnce(() => new Promise<Response>(resolve => { finish = resolve; })));
  await render(); expect(container.querySelector('#row-i')).not.toBeNull();
  await render(0, new URLSearchParams(), change === 'auth' ? { ...session, csrf_token: 'csrf-two' } : session, change === 'workspace' ? 'other-workspace' : 'w');
  expect(container.querySelector('#row-i')).toBeNull(); expect(container.textContent).not.toContain(issue.title); expect(container.querySelector('.loading')).not.toBeNull();
  await act(async () => finish(page([])));
  expect(container.querySelector('.loading')).toBeNull(); expect(container.querySelector('#row-i')).toBeNull();
});

it('does not restore old rows from a late previous-filter response', async () => {
  let previous!: (response: Response) => void; let current!: (response: Response) => void;
  vi.stubGlobal('fetch', vi.fn().mockResolvedValueOnce(page()).mockImplementationOnce(() => new Promise<Response>(resolve => { previous = resolve; })).mockImplementationOnce(() => new Promise<Response>(resolve => { current = resolve; })));
  await render(); await render(1); await render(1, new URLSearchParams('priority=1'));
  await act(async () => previous(page()));
  expect(container.querySelector('#row-i')).not.toBeNull(); expect(container.querySelector('.loading')).toBeNull(); expect(container.textContent).toContain('Updating results…');
  await act(async () => current(page([{ ...issue, id: 'new', title: 'Current result' }])));
  expect(container.querySelector('#row-i')).toBeNull(); expect(container.textContent).toContain('Current result');
});

it('keeps same-scope rows on a transient refresh failure and offers a retry', async () => {
  vi.stubGlobal('fetch', vi.fn().mockResolvedValueOnce(page()).mockRejectedValueOnce(new TypeError('Offline')));
  await render(); await render(1);
  expect(container.querySelector('#row-i')).not.toBeNull(); expect(container.textContent).toContain('Could not reach OpenTrack');
  expect([...container.querySelectorAll('button')].some(button => button.textContent === 'Try again')).toBe(true);
});

it.each([401, 403, 404])('removes a retained snapshot when a refresh denies access with %s', async status => {
  vi.stubGlobal('fetch', vi.fn().mockResolvedValueOnce(page()).mockResolvedValueOnce(json({ code: 'not_available', detail: 'This issue list is unavailable.' }, status)));
  await render(); await render(1);
  expect(container.querySelector('#row-i')).toBeNull(); expect(container.textContent).not.toContain(issue.title); expect(container.textContent).toContain('This issue list is unavailable.');
});


it('keeps rows and the search input mounted during a new query, then replaces the result together', async () => {
  let finish!: (response: Response) => void;
  vi.stubGlobal('fetch', vi.fn().mockResolvedValueOnce(page()).mockImplementationOnce(() => new Promise<Response>(resolve => { finish = resolve; })));
  await render(); const row = container.querySelector('#row-i'); const input = container.querySelector('input[type=search]') as HTMLInputElement;
  input.focus();
  await render(0, new URLSearchParams('q=different'));
  expect(container.querySelector('#row-i')).toBe(row); expect(document.activeElement).toBe(input);
  expect(container.querySelector('.loading')).toBeNull(); expect(container.textContent).toContain('Updating results…');
  await act(async () => finish(page([])));
  expect(container.querySelector('#row-i')).toBeNull(); expect(container.textContent).toContain('No issues match these filters.');
});

it('clears previous-query rows when the new query denies access', async () => {
  vi.stubGlobal('fetch', vi.fn().mockResolvedValueOnce(page()).mockResolvedValueOnce(json({ code: 'forbidden', detail: 'Access changed.' }, 403)));
  await render(); await render(0, new URLSearchParams('q=new'));
  expect(container.querySelector('#row-i')).toBeNull(); expect(container.textContent).toContain('Access changed.');
});

it('does not retain rows when the workspace authorization epoch changes', async () => {
  let finish!: (response: Response) => void;
  vi.stubGlobal('fetch', vi.fn().mockResolvedValueOnce(page()).mockImplementationOnce(() => new Promise<Response>(resolve => { finish = resolve; })));
  await render();
  await act(async () => root.render(<IssueList workspaceId="w" data={{ ...data, auth_epoch: '2' }} session={session} params={new URLSearchParams()} refresh={0} create={() => {}} searchRef={createRef()} />));
  expect(container.querySelector('#row-i')).toBeNull();
  await act(async () => finish(page([])));
});

it('pauses a pending list search while an issue is selected, then applies it after closing without losing filters', async () => {
  vi.useFakeTimers();
  const previous = { url: `${location.pathname}${location.search}`, state: history.state };
  history.replaceState({ otIndex: 0 }, '', '/w/w/issues?team_id=t&sort=priority&q=Keep');
  const fetch = vi.fn((_url: string) => Promise.resolve(page())); vi.stubGlobal('fetch', fetch);
  function RoutedIssueList() {
    const route = parseRoute(useLocation());
    return <><IssueList workspaceId="w" data={data} session={session} params={route.search} selectedId={route.issueId} refresh={0} create={() => {}} searchRef={createRef()} />{route.issueId && <button onClick={() => navigate(issuesUrl('w', route.search))}>Close issue</button>}</>;
  }
  try {
    await act(async () => root.render(<RoutedIssueList />));
    const input = container.querySelector<HTMLInputElement>('input[type=search]')!;
    await act(async () => { Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(input, 'visible'); input.dispatchEvent(new Event('input', { bubbles: true })); });
    await act(async () => vi.advanceTimersByTime(100));
    await act(async () => container.querySelector<HTMLAnchorElement>('#row-i')!.click());
    expect(location.pathname).toBe('/w/w/issues/i');
    expect(location.search).toBe('?team_id=t&sort=priority&q=Keep');
    await act(async () => vi.advanceTimersByTime(500));
    expect(location.pathname).toBe('/w/w/issues/i');
    expect(location.search).toBe('?team_id=t&sort=priority&q=Keep');
    expect(input.value).toBe('visible');
    expect(container.querySelector('#row-i')?.getAttribute('aria-current')).toBe('page');
    expect(fetch.mock.calls.some(([url]) => new URLSearchParams(url.split('?')[1]).get('q') === 'visible')).toBe(false);
    await act(async () => [...container.querySelectorAll<HTMLButtonElement>('button')].find(button => button.textContent === 'Close issue')!.click());
    await act(async () => vi.advanceTimersByTime(280));
    expect(location.pathname).toBe('/w/w/issues');
    expect(location.search).toBe('?team_id=t&sort=priority&q=visible');
    expect(fetch.mock.calls.some(([url]) => new URLSearchParams(url.split('?')[1]).get('q') === 'visible')).toBe(true);
  } finally { history.replaceState(previous.state, '', previous.url); }
});

async function rowKey(target: Element, key: string, options: KeyboardEventInit = {}, altGraph = false) {
  const event = new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true, ...options });
  if (altGraph) Object.defineProperty(event, 'getModifierState', { value: (name: string) => name === 'AltGraph' });
  await act(async () => target.dispatchEvent(event));
  return event;
}

it('moves across rendered status groups with bounded arrows and Home/End while leaving link keys native', async () => {
  const done: Status = { ...status, id: 'done', name: 'Done', category: 'completed' };
  vi.stubGlobal('fetch', vi.fn().mockResolvedValue(page([{ ...issue, id: 'last', status_id: done.id }, issue, { ...issue, id: 'next' }])));
  await act(async () => root.render(<IssueList workspaceId="w" data={{ ...data, statuses: [status, done] }} session={session} params={new URLSearchParams()} refresh={0} create={() => {}} searchRef={createRef()} />));
  const rows = [...container.querySelectorAll<HTMLAnchorElement>('a.issue-row')];
  expect(rows.map(row => row.id)).toEqual(['row-i', 'row-next', 'row-last']);
  rows[0].focus();
  for (const [key, index] of [['ArrowUp', 0], ['ArrowDown', 1], ['ArrowDown', 2], ['ArrowDown', 2], ['Home', 0], ['End', 2], ['ArrowUp', 1]] as const) {
    expect((await rowKey(document.activeElement!, key)).defaultPrevented).toBe(true);
    expect(document.activeElement).toBe(rows[index]);
  }
  for (const key of ['Enter', 'Tab', ' ', 'ArrowLeft', 'ArrowRight']) {
    expect((await rowKey(rows[1], key)).defaultPrevented).toBe(false);
    expect(document.activeElement).toBe(rows[1]);
  }
  expect(rows.every(row => !row.hasAttribute('tabindex'))).toBe(true);
  expect(rows[1].getAttribute('href')).toBe('/w/w/issues/next');
});

it('keeps composition, modifiers and non-row controls outside row navigation', async () => {
  vi.stubGlobal('fetch', vi.fn().mockResolvedValue(page([issue, { ...issue, id: 'next' }])));
  await render();
  const row = container.querySelector<HTMLAnchorElement>('#row-i')!;
  row.focus();
  for (const options of [{ isComposing: true }, { keyCode: 229 }, { altKey: true }, { ctrlKey: true }, { metaKey: true }, { shiftKey: true }]) {
    for (const key of ['ArrowUp', 'ArrowDown', 'Home', 'End']) {
      expect((await rowKey(row, key, options)).defaultPrevented).toBe(false);
      expect(document.activeElement).toBe(row);
    }
  }
  expect((await rowKey(row, 'End', {}, true)).defaultPrevented).toBe(false);
  await act(async () => row.dispatchEvent(new CompositionEvent('compositionstart', { bubbles: true })));
  expect((await rowKey(row, 'End')).defaultPrevented).toBe(false);
  expect(document.activeElement).toBe(row);
  await act(async () => row.dispatchEvent(new CompositionEvent('compositionend', { bubbles: true })));
  for (const control of [container.querySelector('input')!, container.querySelector('select')!]) {
    (control as HTMLElement).focus();
    expect((await rowKey(control, 'Home')).defaultPrevented).toBe(false);
    expect(document.activeElement).toBe(control);
  }
  row.focus();
  expect((await rowKey(row, 'End')).defaultPrevented).toBe(true);
  expect(document.activeElement?.id).toBe('row-next');
});

it.each(['neighbor', 'heading', 'search', 'account'] as const)('recovers a closed origin after the delayed list commit while respecting %s focus', async destination => {
  let finish!: (response: Response) => void;
  const frames: FrameRequestCallback[] = [];
  vi.stubGlobal('requestAnimationFrame', (callback: FrameRequestCallback) => { frames.push(callback); return frames.length; });
  const flushFrames = () => act(async () => { frames.splice(0).forEach(callback => callback(0)); });
  vi.stubGlobal('fetch', vi.fn().mockResolvedValueOnce(page([{ ...issue, id: 'before' }, issue, { ...issue, id: 'after' }]))
    .mockImplementationOnce(() => new Promise<Response>(resolve => { finish = resolve; }))
    .mockResolvedValueOnce(page([{ ...issue, id: 'other-account', title: 'Other account issue' }])));
  const headingRef = createRef<HTMLHeadingElement>();
  const draw = (refresh: number, account = session) => act(async () => root.render(<><header><h1 ref={headingRef} tabIndex={-1}>All issues</h1></header><main><IssueList workspaceId="w" data={data} session={account} params={new URLSearchParams()} selectedId="i" refresh={refresh} create={() => {}} searchRef={createRef()} headingRef={headingRef} /></main></>));
  await draw(0);
  const row = container.querySelector<HTMLAnchorElement>('#row-i')!;
  const rowIds = [...container.querySelectorAll<HTMLAnchorElement>('a.issue-row')].map(item => item.id);
  expect(container.querySelector('main h1')).toBeNull();
  await draw(1);
  const close = document.createElement('button'); document.body.append(close); close.focus(); close.remove();
  restoreIssueRowFocus('i', rowIds, headingRef.current); await flushFrames();
  expect(document.activeElement).toBe(row);
  await act(async () => {
    finish(page(destination === 'heading' ? [] : [{ ...issue, id: 'before' }, { ...issue, id: 'after' }]));
    await new Promise<void>(resolve => setImmediate(resolve));
    expect(row.isConnected).toBe(true);
    frames.splice(0).forEach(callback => callback(0));
  });
  expect(row.isConnected).toBe(false);
  const search = container.querySelector<HTMLInputElement>('input[type=search]')!;
  if (destination === 'search') search.focus();
  if (destination === 'account') await draw(1, { ...session, csrf_token: 'csrf-two' });
  await flushFrames();
  const expected = destination === 'neighbor' ? container.querySelector('#row-after') : destination === 'heading' ? headingRef.current : destination === 'search' ? search : document.body;
  expect(document.activeElement).toBe(expected);
});
