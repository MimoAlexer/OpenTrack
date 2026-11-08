// @vitest-environment jsdom
import { act, StrictMode, type ReactNode } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { advanceSessionGeneration } from '../api';
import { inboxUrl, parseRoute } from '../router';
import type { Session } from '../types';
import { Inbox } from './Inbox';
import { invalidateNotifications, useUnreadNotifications } from './invalidation';
import { defaultSnooze, localSnoozeInstant, type NotificationItem, type NotificationPage } from './types';

const session: Session = { audience: 'web', user: { id: 'recipient', display_name: 'Example Member', email: 'member@example.test' }, csrf_token: 'session-a', workspaces: [{ id: 'workspace', name: 'Example Workspace', slug: 'example', role: 'member' }] };
const stamp = '2026-11-08T09:00:00Z';
function item(id = 'one', seq = '2', overrides: Partial<NotificationItem> = {}): NotificationItem { return { id, seq, reason: 'mention', created_at: stamp, read_at: null, archived_at: null, snoozed_until: null, version: '1', etag: `"notification:${id}:1"`, source: { kind: 'issue', id: `issue-${id}`, team_id: 'team', available: true, key: `ENG-${seq}`, title: `Safe row ${id}`, actor_display_name: 'Example Author' }, ...overrides }; }
function page(items: NotificationItem[] = [item()], overrides: Partial<NotificationPage> = {}): NotificationPage { return { items, next_cursor: null, next_snoozed_at: null, snapshot: 'signed-snapshot-original', unread_count: items.filter(item => !item.read_at).length, ...overrides }; }
const json = (value: unknown, status = 200) => new Response(JSON.stringify(value), { status });
function source(url: string): Response {
  if (url.endsWith('/document')) return json({ id: 'document', plain_text: 'Current authorized source body' });
  return json({ id: url.split('/').at(-1), key: 'ENG-2', title: 'Current authorized source title' });
}
type Fetch = (url: string, options: RequestInit) => Response | Promise<Response>;
function mockFetch(respond: Fetch = url => url.includes('/notifications?') ? json(page()) : source(url)) {
  const fetch = vi.fn((url: string, options: RequestInit) => Promise.resolve(respond(url, options))); vi.stubGlobal('fetch', fetch); return fetch;
}
let container: HTMLDivElement; let root: Root;
beforeEach(() => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true);
  history.replaceState(history.state, '', '/w/workspace/inbox');
  container = document.createElement('div'); document.body.append(container); root = createRoot(container);
});
afterEach(async () => { await act(async () => root.unmount()); container.remove(); vi.useRealTimers(); vi.restoreAllMocks(); vi.unstubAllGlobals(); localStorage.clear(); });
function screen(props: Partial<Parameters<typeof Inbox>[0]> = {}) { return <Inbox workspaceId="workspace" session={session} accessScope="epoch-1" params={new URLSearchParams()} refresh={0} {...props}/>; }
async function render(node: ReactNode = screen()) { await act(async () => root.render(node)); }
function button(label: string): HTMLButtonElement { const value = [...container.querySelectorAll<HTMLButtonElement>('button')].find(button => button.textContent === label || button.getAttribute('aria-label') === label); if (!value) throw new Error(`Missing button: ${label}`); return value; }
async function click(label: string) { await act(async () => button(label).click()); }
function row(id: string) { return [...container.querySelectorAll<HTMLButtonElement>('[data-notification-id]')].find(row => row.dataset.notificationId === id)!; }
async function key(key: string) { await act(async () => (document.activeElement ?? container.querySelector('section')!).dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true }))); }
async function input(value: string) { const field = container.querySelector<HTMLInputElement>('input[type="datetime-local"]')!; await act(async () => { Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(field, value); field.dispatchEvent(new Event('input', { bubbles: true })); }); }

describe('inbox routes and source authorization', () => {
  it('recognizes inbox list and detail routes without carrying issue state', () => {
    expect(parseRoute('/w/workspace/inbox?filter=archived')).toMatchObject({ area: 'inbox', valid: true, notificationId: undefined });
    expect(parseRoute('/w/workspace/inbox/one')).toMatchObject({ area: 'inbox', valid: true, notificationId: 'one' });
    expect(parseRoute('/w/workspace/inbox/one/extra').valid).toBe(false);
    expect(inboxUrl('space/name', new URLSearchParams({ filter: 'all' }), 'update/name')).toBe('/w/space%2Fname/inbox/update%2Fname?filter=all');
  });
  it('renders a real source read separately from safe notification metadata', async () => {
    const fetch = mockFetch(); await render();
    expect(container.textContent).toContain('Safe row one'); expect(container.textContent).toContain('Current authorized source body');
    expect(container.querySelector('article h2')?.textContent).toBe('Current authorized source title');
    expect(fetch.mock.calls.some(([url]) => url.endsWith('/issues/issue-one/document'))).toBe(true);
    expect(container.querySelector('article a')?.getAttribute('href')).toBe('/w/workspace/issues/issue-one');
  });
  it.each([403, 404])('removes every old source field after a preview %s', async status => {
    let denied = false; let listReads = 0;
    mockFetch(url => url.includes('/notifications?') ? json(page([denied && ++listReads >= 2 ? { ...item(), source: { ...item().source, available: false } } : item()])) : denied ? json({ detail: 'Unavailable' }, status) : source(url));
    await render(); expect(container.textContent).toContain('Current authorized source body');
    denied = true; await render(screen({ refresh: 1 }));
    expect(container.textContent).toContain('Source unavailable');
    for (const privateText of ['Safe row one', 'Current authorized source body', 'Current authorized source title', 'Example Author', 'ENG-2']) expect(container.textContent).not.toContain(privateText);
    await act(async () => invalidateNotifications('workspace'));
    expect(container.textContent).not.toContain('Safe row one'); expect(container.querySelector('article a')).toBeNull();
  });
  it('reauthorizes a restored source from a fresh list after a preview denial', async () => {
    let restored = false;
    mockFetch(url => url.includes('/notifications?') ? json(page()) : restored ? source(url) : json({ detail: 'Source deleted' }, 404));
    await render(); expect(container.textContent).toContain('Source unavailable'); expect(container.textContent).not.toContain('Safe row one');
    restored = true; await render(screen({ refresh: 1 }));
    expect(container.querySelector('article h2')?.textContent).toBe('Current authorized source title'); expect(container.textContent).toContain('Safe row one'); expect(container.textContent).toContain('Current authorized source body');
  });
  it('does not revive source metadata from a list request started before the source denial', async () => {
    let listReads = 0; let finishList!: (value: Response) => void; let finishSource!: (value: Response) => void;
    mockFetch(url => url.includes('/notifications?') ? ++listReads === 1 ? json(page()) : new Promise(resolve => { finishList = resolve; }) : url.endsWith('/issues/issue-one') ? new Promise(resolve => { finishSource = resolve; }) : source(url));
    await render(); await act(async () => invalidateNotifications('workspace'));
    await act(async () => finishSource(json({ detail: 'Source unavailable' }, 404)));
    expect(container.textContent).not.toContain('Safe row one');
    await act(async () => finishList(json(page())));
    expect(container.textContent).not.toContain('Safe row one'); expect(container.textContent).not.toContain('Example Author'); expect(container.querySelector('article a')).toBeNull();
  });
  it('renders the normal project source through its current team scope', async () => {
    const project = item('project', '3', { source: { kind: 'project', id: 'project', available: true, team_id: 'team', title: 'Earlier project title' } });
    const fetch = mockFetch(url => url.includes('/notifications?') ? json(page([project])) : json({ id: 'project', team_id: 'team', name: 'Current project title', narrative: { plain_text: 'Current project brief' } }));
    await render();
    expect(fetch.mock.calls.some(([url]) => url.endsWith('/teams/team/projects/project'))).toBe(true); expect(container.textContent).toContain('Current project brief');
    expect(container.querySelector('article a')?.getAttribute('href')).toBe('/w/workspace/projects/project?team_id=team');
  });
  it('does not display a comment whose normal API owner differs from the notification', async () => {
    const notification = item('one', '2', { source: { ...item().source, comment_id: 'comment' } });
    mockFetch(url => url.includes('/notifications?') ? json(page([notification])) : url.endsWith('/comments/comment') ? json({ issue_id: 'another-issue', document: { plain_text: 'Wrong source private text' } }) : source(url));
    await render(); expect(container.textContent).toContain('Source unavailable'); expect(container.textContent).not.toContain('Wrong source private text'); expect(container.textContent).not.toContain('Safe row one');
  });
});

describe('keyboard processing and failure recovery', () => {
  it('moves with arrow keys and archives only the notification, keeping next-row focus', async () => {
    let archived = false; const first = item(); const second = item('two', '1');
    const fetch = mockFetch((url, options) => {
      if (options.method === 'PATCH') { archived = true; return json({ ...first, archived_at: stamp, version: '2' }); }
      return url.includes('/notifications?') ? json(page(archived ? [second] : [first, second])) : source(url);
    });
    await render(); row('one').focus(); await key('ArrowDown'); expect(document.activeElement).toBe(row('two')); await key('ArrowUp'); expect(document.activeElement).toBe(row('one'));
    await key('e');
    expect(row('one')).toBeUndefined(); expect(document.activeElement).toBe(row('two'));
    const writes = fetch.mock.calls.filter(([, options]) => options.method === 'PATCH'); expect(writes).toHaveLength(1);
    expect(writes[0][0]).toBe('/api/v1/workspaces/workspace/notifications/one'); expect(JSON.parse(writes[0][1].body as string)).toEqual({ action: 'archive' });
    expect(new Headers(writes[0][1].headers).get('If-Match')).toBe(first.etag);
  });
  it('restores the failed item and its focus, then retries the same action', async () => {
    let finish!: (value: Response) => void; let attempts = 0; let archived = false; const first = item(); const second = item('two', '1');
    mockFetch((url, options) => {
      if (options.method === 'PATCH') { attempts += 1; if (attempts === 1) return new Promise(resolve => { finish = resolve; }); archived = true; return json({ ...first, archived_at: stamp, version: '2' }); }
      return url.includes('/notifications?') ? json(page(archived ? [second] : [first, second])) : source(url);
    });
    await render(); row('one').focus(); await click('Archive'); expect(row('one')).toBeUndefined(); expect(document.activeElement).toBe(row('two'));
    await act(async () => finish(json({ detail: 'Temporary failure' }, 503)));
    expect(row('one')).toBeDefined(); expect(document.activeElement).toBe(row('one')); expect(container.textContent).toContain('Temporary failure');
    await click('Retry action'); expect(attempts).toBe(2); expect(row('one')).toBeUndefined(); expect(document.activeElement).toBe(row('two'));
  });
  it('does not discard a concurrent arrival while restoring a failed archive', async () => {
    let finish!: (value: Response) => void; let arrival = false; const first = item(); const second = item('two', '1'); const late = item('late', '3');
    mockFetch((url, options) => options.method === 'PATCH' ? new Promise(resolve => { finish = resolve; }) : url.includes('/notifications?') ? json(page(arrival ? [late, first, second] : [first, second])) : source(url));
    await render(); await click('Archive'); arrival = true; await act(async () => invalidateNotifications('workspace'));
    await act(async () => finish(json({ detail: 'Retry later' }, 503)));
    expect(row('one')).toBeDefined(); expect(row('late')).toBeDefined(); expect(document.activeElement).toBe(row('one'));
  });
  it('honors the character shortcut preference while retaining arrow navigation', async () => {
    const fetch = mockFetch(url => url.includes('/notifications?') ? json(page([item(), item('two', '1')])) : source(url));
    await render(screen({ charactersEnabled: false })); row('one').focus(); await key('e'); expect(fetch.mock.calls.some(([, options]) => options.method === 'PATCH')).toBe(false);
    await key('ArrowDown'); expect(document.activeElement).toBe(row('two'));
  });
});

describe('narrow inbox keyboard focus', () => {
  beforeEach(() => {
    vi.stubGlobal('matchMedia', vi.fn(() => ({ matches: true, media: '(max-width: 767px)', addEventListener: vi.fn(), removeEventListener: vi.fn() })));
  });
  it('keeps list movement in the visible list until a row is explicitly opened', async () => {
    mockFetch(url => url.includes('/notifications?') ? json(page([item(), item('two', '1')])) : source(url));
    await render(); row('one').focus(); await key('ArrowDown');
    expect(row('two').getAttribute('aria-pressed')).toBe('true'); expect(document.activeElement).toBe(row('two'));
    expect(container.querySelector('.inbox-screen')?.classList.contains('inbox-preview-open')).toBe(false);
    await key('k'); expect(document.activeElement).toBe(row('one')); expect(container.querySelector('.inbox-screen')?.classList.contains('inbox-preview-open')).toBe(false);
  });
  it('moves explicit preview activation to its visible Back control and returns to the selected row', async () => {
    mockFetch(); await render();
    await act(async () => { row('one').focus(); row('one').click(); });
    expect(container.querySelector('.inbox-screen')?.classList.contains('inbox-preview-open')).toBe(true);
    expect(document.activeElement).toBe(button('Back to inbox'));
    await click('Back to inbox');
    expect(container.querySelector('.inbox-screen')?.classList.contains('inbox-preview-open')).toBe(false); expect(document.activeElement).toBe(row('one'));
  });
  it.each(['Archive', 'Mark read'])('keeps focus in the visible preview after %s selects the next notification', async action => {
    const first = item(); const second = item('two', '1'); let processed = false;
    mockFetch((url, options) => {
      if (options.method === 'PATCH') { processed = true; return json({ ...first, ...(action === 'Archive' ? { archived_at: stamp } : { read_at: stamp }), version: '2' }); }
      return url.includes('/notifications?') ? json(page(processed ? [second] : [first, second])) : source(url);
    });
    await render(); await act(async () => row('one').click()); await click(action);
    expect(row('one')).toBeUndefined(); expect(row('two').getAttribute('aria-pressed')).toBe('true');
    expect(document.activeElement).toBe(button('Back to inbox'));
    await click('Back to inbox'); expect(document.activeElement).toBe(row('two'));
  });
});

describe('snapshot and pagination behavior', () => {
  it('retains the original mark-all snapshot through batches and leaves a late arrival unread', async () => {
    let finish!: (value: Response) => void; let arrived = false; let completed = false; let batches = 0; const late = item('late', '3');
    const fetch = mockFetch((url, options) => {
      if (options.method === 'POST') { batches += 1; if (batches === 1) return new Promise(resolve => { finish = resolve; }); completed = true; return json({ updated_count: 1, unread_count: 1, has_more: false }); }
      return url.includes('/notifications?') ? json(page(completed ? [late] : arrived ? [late, item()] : [item()], { snapshot: arrived ? 'newer-snapshot' : 'signed-snapshot-original' })) : source(url);
    });
    await render(); await click('Mark all read'); arrived = true; await act(async () => invalidateNotifications('workspace'));
    await act(async () => finish(json({ updated_count: 100, unread_count: 2, has_more: true })));
    const requests = fetch.mock.calls.filter(([, options]) => options.method === 'POST'); expect(requests).toHaveLength(2);
    for (const [, options] of requests) expect(JSON.parse(options.body as string)).toEqual({ snapshot: 'signed-snapshot-original' });
    expect(row('late')?.className).toContain('inbox-unread'); expect(row('one')).toBeUndefined();
  });
  it('retries a failed mark-all using its original snapshot after a refresh', async () => {
    let newer = false; let attempts = 0;
    const fetch = mockFetch((url, options) => {
      if (options.method === 'POST') { attempts += 1; return attempts === 1 ? json({ detail: 'Try later' }, 503) : json({ updated_count: 1, unread_count: 1, has_more: false }); }
      return url.includes('/notifications?') ? json(page([item()], { snapshot: newer ? 'newer-snapshot' : 'signed-snapshot-original' })) : source(url);
    });
    await render(); await click('Mark all read'); newer = true; await act(async () => invalidateNotifications('workspace')); await click('Retry action');
    expect(fetch.mock.calls.filter(([, options]) => options.method === 'POST').map(([, options]) => JSON.parse(options.body as string).snapshot)).toEqual(['signed-snapshot-original', 'signed-snapshot-original']);
  });
  it('loads a bounded next page without duplicating rows or moving selection', async () => {
    const fetch = mockFetch(url => url.includes('/notifications?') ? json(url.includes('cursor=') ? page([item(), item('two', '1')]) : page([item()], { next_cursor: 'signed-cursor' })) : source(url));
    await render(); row('one').focus(); await click('Load more notifications');
    expect(container.querySelectorAll('[data-notification-id]')).toHaveLength(2); expect(row('one').getAttribute('aria-pressed')).toBe('true');
    expect(fetch.mock.calls.some(([url]) => url.includes('limit=50') && url.includes('cursor=signed-cursor'))).toBe(true);
  });
  it('revalidates the bounded loaded window without retargeting a later-page keyboard action', async () => {
    const firstPage = Array.from({ length: 50 }, (_, index) => item(`entry-${index}`, String(100 - index)));
    const later = item('later-page', '1'); let read = false;
    const fetch = mockFetch((url, options) => {
      if (options.method === 'PATCH') { read = true; return json({ ...later, read_at: stamp, version: '2' }); }
      if (url.includes('/notifications?')) return json(url.includes('cursor=') ? page(read ? [] : [later]) : page(firstPage, { next_cursor: 'next-page' }));
      return source(url);
    });
    await render(); await click('Load more notifications'); await act(async () => { row(later.id).focus(); row(later.id).click(); });
    await act(async () => invalidateNotifications('workspace'));
    expect(row(later.id)?.getAttribute('aria-pressed')).toBe('true'); expect(document.activeElement).toBe(row(later.id));
    await key('r');
    expect(fetch.mock.calls.find(([, options]) => options.method === 'PATCH')?.[0]).toBe(`/api/v1/workspaces/workspace/notifications/${later.id}`);
  });
  it('clears the actionable target and focuses a neutral heading when a refreshed later-page row is truly absent', async () => {
    const later = item('later-page', '1'); let removed = false;
    const fetch = mockFetch(url => url.includes('/notifications?') ? json(url.includes('cursor=') ? page(removed ? [] : [later]) : page([item()], { next_cursor: 'next-page' })) : source(url));
    await render(); await click('Load more notifications'); await act(async () => { row(later.id).focus(); row(later.id).click(); });
    removed = true; await act(async () => invalidateNotifications('workspace'));
    expect(row(later.id)).toBeUndefined(); expect(row('one').getAttribute('aria-pressed')).toBe('false');
    expect(document.activeElement).toBe(container.querySelector('.inbox-list h2'));
    await key('r'); await key('e');
    expect(fetch.mock.calls.some(([, options]) => options.method === 'PATCH')).toBe(false);
  });
  it('reconciles a missing selection without taking focus from a control outside the inbox', async () => {
    const later = item('later-page', '1');
    mockFetch(url => url.includes('/notifications?') ? json(url.includes('cursor=') ? page([later]) : page([item()], { next_cursor: 'next-page' })) : source(url));
    await render(); await click('Load more notifications'); await act(async () => { row(later.id).focus(); row(later.id).click(); });
    const outside = document.createElement('input'); document.body.append(outside);
    try {
      outside.focus(); await act(async () => invalidateNotifications('workspace'));
      expect(row('one').tabIndex).toBe(-1); expect(document.activeElement).toBe(outside);
    } finally { outside.remove(); }
  });
});

describe('inbox states and personal preferences', () => {
  it.each([['all', 'No activity yet'], ['unread', 'You’re all caught up'], ['archived', 'No archived notifications'], ['snoozed', 'No snoozed notifications']])('distinguishes the %s empty state', async (filter, expected) => {
    mockFetch(() => json(page([]))); await render(screen({ params: new URLSearchParams({ filter }) })); expect(container.textContent).toContain(expected);
    expect([...container.querySelectorAll('button')].some(button => button.textContent === 'View all notifications')).toBe(filter !== 'all');
  });
  it('recovers from a failed inbox read through the visible retry control', async () => {
    let failed = true; mockFetch(url => url.includes('/notifications?') ? failed ? json({ detail: 'Inbox temporarily unavailable' }, 503) : json(page()) : source(url));
    await render(); expect(container.textContent).toContain('Inbox temporarily unavailable'); expect(container.textContent).not.toContain('You’re all caught up');
    failed = false; await click('Try again'); expect(container.textContent).toContain('Safe row one');
  });
  it('requires review after an ETag conflict and never replays the stale action automatically', async () => {
    const fetch = mockFetch((url, options) => options.method === 'PATCH' ? json({ detail: 'This notification changed.' }, 412) : url.includes('/notifications?') ? json(page()) : source(url));
    await render(); await click('Archive'); expect(row('one')).toBeDefined(); await click('Refresh inbox');
    expect(fetch.mock.calls.filter(([, options]) => options.method === 'PATCH')).toHaveLength(1); expect(container.textContent).not.toContain('Retry action');
  });
  it('saves event preferences with their own ETag and retains the form on conflict', async () => {
    let conflict = false; const preference = { assignments: true, mentions: true, subscribed_comments: true, version: '1', etag: '"notification-preferences:1"' };
    const fetch = mockFetch((url, options) => url.endsWith('/notification-preferences') ? options.method === 'PATCH' ? conflict ? json({ detail: 'Preferences changed elsewhere' }, 412) : json({ ...preference, ...JSON.parse(options.body as string), version: '2', etag: '"notification-preferences:2"' }) : json(preference) : url.includes('/notifications?') ? json(page()) : source(url));
    await render(); await click('Inbox preferences');
    const checkbox = [...container.querySelectorAll<HTMLInputElement>('input[type="checkbox"]')].at(-1)!;
    await act(async () => checkbox.click()); await click('Save preferences');
    const write = fetch.mock.calls.find(([, options]) => options.method === 'PATCH')!;
    expect(new Headers(write[1].headers).get('If-Match')).toBe(preference.etag); expect(JSON.parse(write[1].body as string)).toEqual({ assignments: true, mentions: true, subscribed_comments: false }); expect(container.textContent).toContain('Preferences saved.');
    conflict = true; await act(async () => checkbox.click()); await click('Save preferences'); expect(container.textContent).toContain('Preferences changed elsewhere');
    expect(button('Save preferences').disabled).toBe(true); await click('Reload preferences'); expect(button('Save preferences').disabled).toBe(false);
  });
  it('treats an unrecognized filter as unread without reading object prototype properties', async () => {
    const fetch = mockFetch(); await render(screen({ params: new URLSearchParams({ filter: 'toString' }) })); expect(fetch.mock.calls[0][0]).toContain('filter=unread');
  });
});

describe('time and recipient scope', () => {
  it('returns the same snoozed notification and recomputes its badge at the ten-second deadline without an action or event', async () => {
    vi.useFakeTimers(); vi.setSystemTime(new Date('2026-11-08T09:00:00Z'));
    vi.spyOn(document, 'visibilityState', 'get').mockReturnValue('visible');
    const due = Date.now() + 10_000; const snoozed = item('existing-snooze', '2', { snoozed_until: new Date(due).toISOString() });
    const fetch = mockFetch(url => url.includes('/notifications?') ? json(Date.now() < due ? page([], { unread_count: 0, next_snoozed_at: new Date(due).toISOString() }) : page([snoozed], { next_snoozed_at: null })) : source(url));
    function Badge() { const count = useUnreadNotifications('workspace', session, 'epoch-1', true); return <output aria-label="Unread count">{count ?? 'unknown'}</output>; }
    await render(<>{screen()}<Badge/></>); expect(row(snoozed.id)).toBeUndefined(); expect(container.querySelector('output')?.textContent).toBe('0');
    await act(async () => vi.advanceTimersByTimeAsync(9_999)); expect(row(snoozed.id)).toBeUndefined();
    await act(async () => vi.advanceTimersByTimeAsync(1));
    expect(row(snoozed.id)).toBeDefined(); expect(container.querySelectorAll('[data-notification-id]')).toHaveLength(1); expect(container.querySelector('output')?.textContent).toBe('1');
    expect(fetch.mock.calls.every(([, options]) => !options.method || options.method === 'GET')).toBe(true);
  });
  it('converts a valid local snooze to UTC, rejects invalid and out-of-range times, and displays the equivalent', async () => {
    const now = Date.now(); const local = defaultSnooze(now); const until = localSnoozeInstant(local, now)!;
    expect(until).toBe(new Date(local).toISOString()); expect(localSnoozeInstant('2026-02-30T10:00', new Date('2026-02-01').getTime())).toBeNull();
    expect(localSnoozeInstant(defaultSnooze(now - 2 * 60 * 60 * 1000), now)).toBeNull(); expect(localSnoozeInstant(defaultSnooze(now + 91 * 86400000), now)).toBeNull();
    const fetch = mockFetch((url, options) => options.method === 'PATCH' ? json({ ...item(), snoozed_until: until, version: '2' }) : url.includes('/notifications?') ? json(page()) : source(url));
    await render(); await click('Snooze'); await input(local);
    expect(container.querySelector('.inbox-snooze time')?.getAttribute('title')).toBe(new Date(until).toUTCString());
    await act(async () => container.querySelector('form.inbox-snooze')!.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true })));
    const write = fetch.mock.calls.find(([, options]) => options.method === 'PATCH')!; expect(JSON.parse(write[1].body as string)).toEqual({ action: 'snooze', until });
  });
  it.each(['recipient', 'workspace', 'access', 'generation'])('fences a late private source response after %s changes', async change => {
    let finish!: (value: Response) => void; let changed = false;
    mockFetch(url => url.includes('/notifications?') ? json(page(changed ? [] : [item()])) : url.endsWith('/document') ? new Promise(resolve => { finish = resolve; }) : source(url));
    await render(); changed = true;
    if (change === 'generation') advanceSessionGeneration();
    await render(screen(change === 'recipient' ? { session: { ...session, user: { ...session.user, id: 'another-recipient' }, csrf_token: 'other-session' } } : change === 'workspace' ? { workspaceId: 'other-workspace' } : change === 'access' ? { accessScope: 'epoch-2' } : {}));
    expect(container.textContent).not.toContain('Safe row one');
    await act(async () => finish(json({ plain_text: 'Earlier private body' })));
    expect(container.textContent).not.toContain('Earlier private body'); expect(container.textContent).not.toContain('Current authorized source title');
  });
  it('remains functional under React strict effect replay', async () => {
    mockFetch(); await render(<StrictMode>{screen()}</StrictMode>); expect(container.textContent).toContain('Current authorized source body');
  });
  it('recomputes the badge after inbox invalidation and clears it at a recipient boundary', async () => {
    let count = 2; let finish!: (value: Response) => void; let pending = false;
    mockFetch(() => pending ? new Promise(resolve => { finish = resolve; }) : json(page([], { unread_count: count })));
    function Badge({ activeSession = session }: { activeSession?: Session }) { const count = useUnreadNotifications('workspace', activeSession, 'epoch-1', true); return <span>{count ?? 'unknown'}</span>; }
    await render(<Badge/>); expect(container.textContent).toBe('2'); count = 3; await act(async () => invalidateNotifications('workspace')); expect(container.textContent).toBe('3');
    pending = true; await render(<Badge activeSession={{ ...session, csrf_token: 'new-session' }}/>); expect(container.textContent).toBe('unknown');
    await act(async () => finish(json(page([], { unread_count: 1 })))); expect(container.textContent).toBe('1');
  });
});

describe('late mutation authority reconciliation', () => {
  it.each(['failure', 'success'])('keeps source denials after a delayed mutation %s and permits a later authorized read', async outcome => {
    const original = item(); let unavailable = false; let restored = false; let finish!: (response: Response) => void; let reads = 0;
    mockFetch((url, options) => {
      if (options.method === 'PATCH') return new Promise(resolve => { finish = resolve; });
      if (url.includes('/notifications?')) {
        reads++;
        if (reads > 2 && !restored) return new Promise(() => {});
        return json(page([unavailable && !restored ? { ...original, source: { kind: 'issue', id: original.source.id, available: false } } : original]));
      }
      return source(url);
    });
    await render(screen({ params: new URLSearchParams({ filter: 'all' }) })); row('one').focus(); await click('Mark read');
    unavailable = true; await act(async () => invalidateNotifications('workspace'));
    expect(container.textContent).not.toContain('Safe row one');
    await act(async () => finish(outcome === 'failure' ? json({ detail: 'Try later' }, 503) : json({ ...original, read_at: stamp, version: '2' })));
    for (const secret of ['Safe row one', 'ENG-2', 'Example Author', 'Current authorized source body']) expect(container.textContent).not.toContain(secret);
    expect(row('one')?.textContent).toContain('Source unavailable'); expect(container.querySelector('.inbox-source-unavailable')).not.toBeNull();
    expect(document.activeElement).toBe(row('one'));
    restored = true; await act(async () => invalidateNotifications('workspace'));
    expect(container.textContent).toContain('Safe row one'); expect(container.textContent).toContain('Current authorized source body');
  });
  it('does not replace a newer authoritative item with an older successful mutation response', async () => {
    const original = item(); const fresh = item('one', '2', { version: '3', source: { ...original.source, title: 'Newly authorized title' } });
    let reads = 0; let finish!: (response: Response) => void;
    mockFetch((url, options) => options.method === 'PATCH' ? new Promise(resolve => { finish = resolve; }) : url.includes('/notifications?') ? ++reads === 1 ? json(page([original])) : reads === 2 ? json(page([fresh])) : new Promise(() => {}) : source(url));
    await render(screen({ params: new URLSearchParams({ filter: 'all' }) })); await click('Mark read');
    await act(async () => invalidateNotifications('workspace')); expect(row('one').textContent).toContain('Newly authorized title');
    await act(async () => finish(json({ ...original, read_at: stamp, version: '2' })));
    expect(row('one').textContent).toContain('Newly authorized title'); expect(row('one').className).toContain('inbox-unread');
  });
  it('reauthorizes personal state after a failed action races an unrelated in-flight next page', async () => {
    let nextReads = 0; let finishPage!: (response: Response) => void; let finishMutation!: (response: Response) => void;
    mockFetch((url, options) => options.method === 'PATCH' ? new Promise(resolve => { finishMutation = resolve; }) : url.includes('cursor=') ? ++nextReads === 1 ? new Promise(resolve => { finishPage = resolve; }) : json(page([])) : url.includes('/notifications?') ? json(page([item()], { next_cursor: 'next-page' })) : source(url));
    await render(screen({ params: new URLSearchParams({ filter: 'all' }) })); await click('Load more notifications'); await click('Mark read');
    await act(async () => finishPage(json(page([]))));
    await act(async () => finishMutation(json({ detail: 'Try later' }, 503)));
    expect(row('one').className).toContain('inbox-unread'); expect(button('Mark read')).toBeDefined();
  });

});
