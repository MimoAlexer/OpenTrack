// @vitest-environment jsdom
import { act, type ReactNode } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import type { Bootstrap, Issue, Session, Status } from '../types';
import { IssueRelationships } from './IssueRelationships';
import { LinkIssue } from './LinkIssue';
import { MoveIssue } from './MoveIssue';
import { IssueKeyResolver } from './IssueKeyResolver';
import { CreateIssue } from '../issues/CreateIssue';
import { IssueEditor } from '../issues/IssueEditor';
import { parseRoute } from '../router';
import type { IssueGraphTarget, IssueMovePreview, IssueRelation } from './types';

const session: Session = { audience: 'web', user: { id: 'u', display_name: 'Example Owner', email: 'owner@example.test' }, csrf_token: 'csrf', workspaces: [{ id: 'w', name: 'Workspace', slug: 'workspace', role: 'member' }] };
const status: Status = { id: 's', team_id: 't', name: 'Ready', category: 'unstarted', color: 'blue', position: 0, is_default: true, retiring_at: null, archived_at: null, replacement_status_id: null, replacement_job_id: null };
const destinationStatus = { ...status, id: 'destination-ready', team_id: 'dest' };
const source: Issue = { id: 'i', workspace_id: 'w', team_id: 't', key: 'ENG-1', title: 'Main task', description: 'Original context', status_id: 's', priority: 0, assignee_id: null, assignee: null, label_ids: [], labels: [], etag: '"issue:i:1:p"', estimate: null, due_date: null, version: '1', created_at: '2026-10-17T10:00:00Z', updated_at: '2026-10-17T10:00:00Z', archived_at: null };
const other: Issue = { ...source, id: 'other', key: 'ENG-2', title: 'Other work', etag: '"issue:other:1:p"' };
const target: IssueGraphTarget = { id: other.id, key: other.key, title: other.title, status, archived_at: null, etag: other.etag };
const data: Bootstrap = { auth_epoch: '1', teams: [{ id: 't', name: 'Engineering', key: 'ENG', private: true, archived_at: null }, { id: 'dest', name: 'Product', key: 'PRO', private: false, archived_at: null }], statuses: [status, destinationStatus], members: [], issues: [source] };
const destination = { id: 'dest', name: 'Product', key: 'PRO', private: false, can_move_issue: true };
const preview: IssueMovePreview = { issue_id: source.id, issue_etag: source.etag, source_team_id: 't', destination, destination_workflow_version: '1', destination_default_status_id: destinationStatus.id, audience_change: 'private_to_public', current_assignee: null, retained_label_ids: [], removed_label_ids: [], blockers: [], preview_token: 'preview-one', expires_at: '2099-01-01T00:00:00Z' };
const page = (values: unknown[], next: string | null = null) => ({ data: values, next_cursor: next, has_more: !!next });
const graphPage = (values: unknown[], version = '1', next: string | null = null) => ({ ...page(values, next), graph_version: version, issue_etag: source.etag });
const parent = (value: IssueGraphTarget | null = null) => ({ issue_id: source.id, issue_etag: source.etag, graph_version: '1', parent: value ? { target: value, can_detach: true } : null, can_change: true });
const json = (value: unknown, status = 200) => new Response(JSON.stringify(value), { status });
function baseGet(url: string) {
  if (url.includes('/move-destinations?')) return json(page([destination]));
  if (url.includes('/move-preview?')) return json(preview);
  if (url.includes('/issues/lookup?')) return json(page([target]));
  if (url.endsWith('/parent')) return json(parent());
  if (/\/(children|relations)\?/.test(url)) return json(graphPage([]));
  if (url.endsWith('/workflow')) { const dest = url.includes('/dest/'); return json({ team_id: dest ? 'dest' : 't', version: '1', can_manage: false, statuses: [dest ? destinationStatus : status] }); }
  if (url.includes('/statuses/')) return json(status);
  if (url.includes('/assignees?')) return json({ ...page([{ id: 'eligible', display_name: 'Eligible person' }]), self_assignable: false });
  if (url.includes('/labels?')) return json(page([]));
  if (url.endsWith('/activity')) return json(page([]));
  if (url.endsWith('/issues/other')) return json(other);
  if (url.endsWith('/issues/i')) return json(source);
  throw new Error(`Unexpected request: ${url}`);
}
let container: HTMLDivElement; let root: Root;
beforeEach(() => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true); vi.spyOn(window, 'confirm').mockReturnValue(false);
  HTMLDialogElement.prototype.showModal = function () { this.setAttribute('open', ''); };
  HTMLDialogElement.prototype.close = function () { this.removeAttribute('open'); };
  history.replaceState({ otIndex: 0 }, '', '/w/w/issues/i');
  container = document.createElement('div'); document.body.append(container); root = createRoot(container);
});
afterEach(async () => { await act(async () => root.unmount()); container.remove(); vi.restoreAllMocks(); vi.unstubAllGlobals(); });
async function render(node: ReactNode) { await act(async () => root.render(node)); }
async function settle(ms = 10) { await act(async () => new Promise(resolve => setTimeout(resolve, ms))); }
function button(name: string, scope: ParentNode = container) { const found = [...scope.querySelectorAll<HTMLButtonElement>('button')].find(item => item.textContent === name || item.getAttribute('aria-label') === name); if (!found) throw new Error(`Missing button ${name}`); return found; }
async function click(name: string, scope?: ParentNode) { await act(async () => button(name, scope).click()); }
async function submit(scope: ParentNode = container) { await act(async () => scope.querySelector('form')!.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }))); }
async function input(element: HTMLInputElement | HTMLTextAreaElement | HTMLSelectElement, value: string) { await act(async () => { Object.getOwnPropertyDescriptor(element instanceof HTMLSelectElement ? HTMLSelectElement.prototype : element instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype, 'value')!.set!.call(element, value); element.dispatchEvent(new Event(element instanceof HTMLSelectElement ? 'change' : 'input', { bubbles: true })); }); }
function picker(name: string) { return [...container.querySelectorAll<HTMLElement>('.reference-picker')].find(item => item.querySelector('.field-label')?.textContent === name)!; }
async function open(name: string) { await act(async () => picker(name).querySelector<HTMLButtonElement>('button')!.click()); await settle(); }
async function choose(name: string) { const item = [...container.querySelectorAll<HTMLElement>('[role=option]')].find(item => item.textContent?.includes(name))!; await act(async () => item.click()); }
async function acknowledge() { for (const field of container.querySelectorAll<HTMLInputElement>('input[type=checkbox]')) if (!field.checked) await act(async () => field.click()); }

it('renders relative relationship directions and removes an opaque historical link without requesting its hidden target', async () => {
  const links: IssueRelation[] = [{ id: 'opaque', kind: 'blocked_by', target: null, can_remove: true }, { id: 'related', kind: 'related', target, can_remove: true }, { id: 'duplicate', kind: 'has_duplicate', target, can_remove: true }];
  const writes: { url: string; options: RequestInit }[] = [];
  const fetch = vi.fn((url: string, options: RequestInit) => { if (options.method === 'DELETE') { writes.push({ url, options }); return Promise.resolve(json({ ...source, version: '2' })); } return Promise.resolve(url.includes('/relations?') ? json(graphPage(links)) : baseGet(url)); }); vi.stubGlobal('fetch', fetch);
  const updated = vi.fn(); await render(<IssueRelationships workspaceId="w" issue={source} session={session} data={data} disabled={false} dirty={false} updated={updated} openLink={() => {}} />);
  expect(container.textContent).toContain('Blocked by'); expect(container.textContent).toContain('Duplicates'); expect(container.textContent).toContain('Issue unavailable');
  await click('Remove blocked by unavailable issue'); await click('Remove link');
  expect(writes[0].url).toBe('/api/v1/workspaces/w/issues/i/relations/opaque'); expect(writes[0].options.body).toBeUndefined(); expect(new Headers(writes[0].options.headers).get('If-Match')).toBe(source.etag); expect(updated).toHaveBeenCalledOnce();
  expect(fetch.mock.calls.some(([url]) => url.includes('/issues/null'))).toBe(false);
});

it('keeps graph mutations unavailable for dirty or read-only issues while retaining navigable summaries', async () => {
  vi.stubGlobal('fetch', vi.fn((url: string) => Promise.resolve(url.endsWith('/parent') ? json(parent(target)) : baseGet(url))));
  await render(<IssueRelationships workspaceId="w" issue={source} session={session} data={data} disabled={false} dirty updated={() => {}} openLink={() => {}} />);
  expect(button('Add connection').disabled).toBe(true); expect(button('Detach parent').disabled).toBe(true); expect(container.querySelector('a')?.getAttribute('href')).toBe('/w/w/issues/other');
  await click('More connection actions'); expect(button('Move to team').disabled).toBe(true); expect(button('Refresh relationships').disabled).toBe(false);
  await render(<IssueRelationships workspaceId="w" issue={source} session={session} data={data} disabled dirty={false} updated={() => {}} openLink={() => {}} />);
  expect(container.textContent).not.toContain('Add connection'); expect(container.textContent).not.toContain('Move to team'); expect(container.textContent).toContain('Other work');
});

it('keeps an empty graph compact and returns focus to Add connection after its dialog closes', async () => {
  vi.stubGlobal('fetch', vi.fn((url: string) => Promise.resolve(baseGet(url))));
  await render(<IssueRelationships workspaceId="w" issue={source} session={session} data={data} disabled={false} dirty={false} updated={() => {}} openLink={() => {}} />);
  expect(container.querySelector('[role=status]')?.textContent).toBe('No connections yet.');
  expect(container.querySelectorAll('.graph-group')).toHaveLength(0);
  expect(container.textContent).not.toContain('Create subissue');
  await click('Add connection');
  expect(document.activeElement).toBe(button('Set parent'));
  expect(button('Add existing subissue').disabled).toBe(false); expect(button('Create subissue').disabled).toBe(false); expect(button('Add relationship').disabled).toBe(false);
  await click('Set parent');
  expect(container.querySelector('.graph-action-popover')).toBeNull();
  expect(container.querySelector('dialog h2')?.textContent).toBe('Set parent issue');
  await click('Cancel');
  expect(document.activeElement).toBe(button('Add connection'));
});

it('dismisses connection actions with Escape or an outside pointer and keeps parent authority intact', async () => {
  vi.stubGlobal('fetch', vi.fn((url: string) => Promise.resolve(url.endsWith('/parent') ? json({ ...parent(target), can_change: false }) : baseGet(url))));
  await render(<IssueRelationships workspaceId="w" issue={source} session={session} data={data} disabled={false} dirty={false} updated={() => {}} openLink={() => {}} />);
  await click('Add connection');
  expect(container.textContent).not.toContain('Change parent'); expect(container.textContent).toContain('Other work');
  const escape = new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true });
  await act(async () => document.activeElement!.dispatchEvent(escape));
  expect(escape.defaultPrevented).toBe(true); expect(container.querySelector('.graph-action-popover')).toBeNull(); expect(document.activeElement).toBe(button('Add connection'));
  await click('More connection actions');
  await act(async () => document.body.dispatchEvent(new MouseEvent('pointerdown', { bubbles: true })));
  expect(container.querySelector('.graph-action-popover')).toBeNull();
});

it('refreshes connections and opens the reviewed team move from overflow', async () => {
  const fetch = vi.fn((url: string) => Promise.resolve(baseGet(url))); vi.stubGlobal('fetch', fetch);
  await render(<IssueRelationships workspaceId="w" issue={source} session={session} data={data} disabled={false} dirty={false} updated={() => {}} openLink={() => {}} />);
  await click('More connection actions'); await click('Refresh relationships');
  expect(fetch.mock.calls.filter(([url]) => url.endsWith('/parent'))).toHaveLength(2);
  expect(container.querySelector('.graph-action-popover')).toBeNull();
  await click('More connection actions'); await click('Move to team');
  expect(container.querySelector('dialog h2')?.textContent).toBe('Move ENG-1 to another team');
});

it('preserves a selected relationship on target conflict and retries an unknown result with its original body, tag and key', async () => {
  const writes: RequestInit[] = []; let refreshed = false;
  vi.stubGlobal('fetch', vi.fn((url: string, options: RequestInit) => {
    if (options.method === 'POST') { writes.push(options); if (writes.length === 1) return Promise.resolve(json({ code: 'target_changed', detail: 'Target changed.' }, 412)); if (writes.length === 2) return Promise.reject(new TypeError('Lost response')); return Promise.resolve(json({ issue: { ...source, version: '3' }, relation: { id: 'r', kind: 'related', target, can_remove: true } }, 201)); }
    if (url.endsWith('/issues/i')) { refreshed = true; return Promise.resolve(json({ ...source, etag: '"issue:i:2:p"' })); }
    if (url.endsWith('/issues/other')) return Promise.resolve(json({ ...other, etag: '"issue:other:2:p"' }));
    return Promise.resolve(baseGet(url));
  }));
  const saved = vi.fn(); await render(<LinkIssue workspaceId="w" issue={source} session={session} mode="relation" close={() => {}} saved={saved} reviewed={() => {}} />); await open('Related issue'); await choose('Other work'); await submit();
  expect(picker('Related issue').textContent).toContain('Other work'); expect(button('Add relationship').disabled).toBe(true); await click('Refresh selected issues'); expect(refreshed).toBe(true); await submit();
  expect(button('Cancel').disabled).toBe(true); expect(picker('Related issue').querySelector<HTMLButtonElement>('button')?.disabled).toBe(true); await submit();
  expect(writes[2].body).toBe(writes[1].body); expect(new Headers(writes[2].headers).get('Idempotency-Key')).toBe(new Headers(writes[1].headers).get('Idempotency-Key')); expect(new Headers(writes[2].headers).get('If-Match')).toBe('"issue:i:2:p"'); expect(JSON.parse(writes[2].body as string).target_etag).toBe('"issue:other:2:p"'); expect(saved).toHaveBeenCalledOnce();
});

it('adds an existing child using the child validator then reloads the parent after both versions change', async () => {
  const calls: { url: string; options: RequestInit }[] = [];
  vi.stubGlobal('fetch', vi.fn((url: string, options: RequestInit) => { calls.push({ url, options }); return Promise.resolve(options.method === 'PATCH' ? json({ ...other, version: '2' }) : url.endsWith('/issues/i') ? json({ ...source, version: '2', etag: '"issue:i:2:p"' }) : baseGet(url)); }));
  const saved = vi.fn(); await render(<LinkIssue workspaceId="w" issue={source} session={session} mode="child" close={() => {}} saved={saved} reviewed={() => {}} />); await open('Subissue'); await choose('Other work'); await submit();
  const write = calls.find(call => call.options.method === 'PATCH')!; expect(write.url).toContain('/issues/other/parent'); expect(new Headers(write.options.headers).get('If-Match')).toBe(other.etag); expect(JSON.parse(write.options.body as string)).toEqual({ parent_id: source.id, parent_etag: source.etag }); expect(calls.at(-1)?.url).toContain('/issues/i'); expect(saved.mock.calls[0][0].etag).toBe('"issue:i:2:p"');
});

it('creates a subissue in its fixed team with only the parent relationship inherited, preserving title on parent conflict', async () => {
  const writes: RequestInit[] = [];
  vi.stubGlobal('fetch', vi.fn((url: string, options: RequestInit) => { if (options.method === 'POST') { writes.push(options); return Promise.resolve(writes.length === 1 ? json({ code: 'conflict', detail: 'Parent changed.' }, 412) : json(other, 201)); } return Promise.resolve(url.endsWith('/issues/i') ? json({ ...source, etag: '"issue:i:2:p"' }) : baseGet(url)); }));
  await render(<CreateIssue workspaceId="w" data={data} session={session} initialTeam="dest" parent={{ ...source, priority: 1, assignee_id: 'inherited-person', label_ids: ['inherited-label'] }} close={() => {}} created={() => {}} />);
  const team = container.querySelector<HTMLSelectElement>('select')!; expect(team.value).toBe('t'); expect(team.disabled).toBe(true);
  const title = container.querySelector<HTMLInputElement>('input[data-autofocus]')!; await input(title, 'Child title'); await submit(); expect(title.value).toBe('Child title'); expect(button('Create subissue').disabled).toBe(true); await click('Refresh parent'); await submit();
  const body = JSON.parse(writes[1].body as string); expect(body).toMatchObject({ title: 'Child title', team_id: 't', parent_id: 'i', parent_etag: '"issue:i:2:p"', priority: 0, assignee_id: null, label_ids: [] }); expect(body.status_id).toBeUndefined();
});

it('closes only the issue lookup on Escape and restores focus to its trigger', async () => {
  vi.stubGlobal('fetch', vi.fn((url: string) => Promise.resolve(baseGet(url)))); const close = vi.fn(); await render(<LinkIssue workspaceId="w" issue={source} session={session} mode="parent" close={close} saved={() => {}} reviewed={() => {}} />); await open('Parent issue');
  await act(async () => container.querySelector<HTMLInputElement>('[role=combobox]')!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true })));
  expect(close).not.toHaveBeenCalled(); expect(container.querySelector('[role=combobox]')).toBeNull(); expect(document.activeElement).toBe(picker('Parent issue').querySelector('button'));
});

it('requires explicit destination assignment repair, removed-label approval and private-to-public access approval', async () => {
  const labeled: Issue = { ...source, assignee_id: 'former', assignee: { id: 'former', display_name: 'Former teammate', assignable: true }, label_ids: ['team-label', 'shared'], labels: [{ id: 'team-label', name: 'Internal', team_id: 't', color: 'blue', archived_at: null }, { id: 'shared', name: 'Shared label', team_id: null, color: 'gray', archived_at: null }] };
  const reviewedPreview = { ...preview, current_assignee: { ...labeled.assignee!, assignable: false }, retained_label_ids: ['shared'], removed_label_ids: ['team-label'] }; const writes: RequestInit[] = [];
  vi.stubGlobal('fetch', vi.fn((url: string, options: RequestInit) => { if (options.method === 'POST') { writes.push(options); return Promise.resolve(json({ ...labeled, team_id: 'dest', key: 'PRO-1' })); } return Promise.resolve(url.includes('/move-preview?') ? json(reviewedPreview) : url.endsWith('/issues/i') ? json(labeled) : baseGet(url)); }));
  const moved = vi.fn(); await render(<MoveIssue workspaceId="w" issue={labeled} data={data} session={session} close={() => {}} moved={moved} reviewed={() => {}} />); await open('Destination team'); await choose('Product');
  expect(container.textContent).toContain('Former teammate'); expect(container.textContent).toContain('Internal'); expect(container.textContent).toContain('will become visible to the workspace'); expect(button('Confirm move').disabled).toBe(true);
  await open('Assignee'); await choose('Unassigned'); expect(button('Confirm move').disabled).toBe(true); await acknowledge(); expect(button('Confirm move').disabled).toBe(false); await submit();
  expect(JSON.parse(writes[0].body as string)).toEqual({ destination_team_id: 'dest', status_id: 'destination-ready', assignee_id: null, label_ids: ['shared'], preview_token: 'preview-one' }); expect(new Headers(writes[0].headers).get('If-Match')).toBe(source.etag); expect(moved).toHaveBeenCalledOnce();
});

it('shows only blocker categories and never submits a move while relationships remain', async () => {
  const fetch = vi.fn((url: string) => Promise.resolve(url.includes('/move-preview?') ? json({ ...preview, blockers: ['children', 'has_duplicate'] }) : baseGet(url))); vi.stubGlobal('fetch', fetch);
  await render(<MoveIssue workspaceId="w" issue={source} data={data} session={session} close={() => {}} moved={() => {}} reviewed={() => {}} />); await open('Destination team'); await choose('Product'); expect(container.textContent).toContain('subissues, duplicates'); expect(button('Confirm move').disabled).toBe(true); await submit(); expect(fetch.mock.calls.every(([url]) => !url.endsWith('/move'))).toBe(true);
});

it('preserves move repairs on409 re-preview and locks an uncertain retry to the original validator and preview', async () => {
  const writes: RequestInit[] = []; let previews = 0;
  vi.stubGlobal('fetch', vi.fn((url: string, options: RequestInit) => { if (url.includes('/move-preview?')) return Promise.resolve(json({ ...preview, preview_token: ++previews === 1 ? 'first-token' : 'reviewed-token' })); if (options.method === 'POST') { writes.push(options); if (writes.length === 1) return Promise.resolve(json({ code: 'preview_stale', detail: 'Preview changed.' }, 409)); if (writes.length === 2) return Promise.reject(new TypeError('Connection interrupted')); return Promise.resolve(json({ ...source, team_id: 'dest', key: 'PRO-1' })); } return Promise.resolve(baseGet(url)); }));
  const moved = vi.fn(); await render(<MoveIssue workspaceId="w" issue={source} data={data} session={session} close={() => {}} moved={moved} reviewed={() => {}} />); await open('Destination team'); await choose('Product'); await open('Assignee'); await choose('Eligible person'); await acknowledge(); await submit();
  expect(button('Confirm move').disabled).toBe(true); await click('Refresh move review'); expect(picker('Assignee').textContent).toContain('Eligible person'); expect(button('Confirm move').disabled).toBe(true); await acknowledge(); await submit(); expect(button('Cancel').disabled).toBe(true); await submit();
  expect(writes[2].body).toBe(writes[1].body); expect(JSON.parse(writes[2].body as string)).toMatchObject({ assignee_id: 'eligible', preview_token: 'reviewed-token' }); expect(new Headers(writes[2].headers).get('Idempotency-Key')).toBe(new Headers(writes[1].headers).get('Idempotency-Key')); expect(moved).toHaveBeenCalledOnce();
});

it('keeps dirty navigation in an in-app save/discard dialog and only follows after the save succeeds', async () => {
  let writes = 0; const fetch = vi.fn((url: string, options: RequestInit) => { if (options.method === 'PATCH') { writes += 1; return Promise.resolve(json({ ...source, title: 'Edited title', version: '2' })); } return Promise.resolve(url.endsWith('/parent') ? json(parent(target)) : baseGet(url)); }); vi.stubGlobal('fetch', fetch);
  await render(<IssueEditor workspaceId="w" issueId="i" data={data} session={session} close={() => {}} updated={() => {}} />); await input(container.querySelector<HTMLTextAreaElement>('#issue-title')!, 'Edited title'); await act(async () => container.querySelector<HTMLAnchorElement>('.graph-target')!.click());
  expect(location.pathname).toBe('/w/w/issues/i'); expect(container.textContent).toContain('Save changes before opening another issue?'); await click('Keep editing'); expect(container.querySelector<HTMLTextAreaElement>('#issue-title')!.value).toBe('Edited title');
  await act(async () => container.querySelector<HTMLAnchorElement>('.graph-target')!.click()); await click('Save and open'); await settle(25); expect(writes).toBe(1); expect(location.pathname).toBe('/w/w/issues/other'); expect(window.confirm).not.toHaveBeenCalled();
});

it('clears graph content and aborts old reads when the issue changes', async () => {
  let finish!: (response: Response) => void; let oldSignal!: AbortSignal;
  vi.stubGlobal('fetch', vi.fn((url: string, options: RequestInit) => { if (url.endsWith('/issues/i/parent')) { oldSignal = options.signal!; return new Promise<Response>(resolve => { finish = resolve; }); } return Promise.resolve(url.endsWith('/parent') ? json({ ...parent(), issue_id: 'new' }) : baseGet(url)); }));
  const props = { workspaceId: 'w', session, data, disabled: false, dirty: false, updated: () => {}, openLink: () => {} };
  await render(<IssueRelationships {...props} issue={source} />); await render(<IssueRelationships {...props} issue={{ ...source, id: 'new' }} />); expect(oldSignal.aborted).toBe(true); await act(async () => finish(json(parent({ ...target, title: 'Old private title' })))); expect(container.textContent).not.toContain('Old private title');
});

it('rejects mixed graph pages and offers a first-page refresh without merging stale data', async () => {
  vi.stubGlobal('fetch', vi.fn((url: string) => Promise.resolve(url.includes('/relations?') ? json(graphPage([{ id: 'r', kind: 'related', target, can_remove: true }], url.includes('cursor=') ? '2' : '1', url.includes('cursor=') ? null : 'old-cursor')) : baseGet(url))));
  await render(<IssueRelationships workspaceId="w" issue={source} session={session} data={data} disabled={false} dirty={false} updated={() => {}} openLink={() => {}} />); await click('Load more relationships'); expect(container.textContent).toContain('Refresh to reload the first page'); expect(container.querySelectorAll('.graph-target')).toHaveLength(1);
});

it('does not redirect a key until the authorized canonical response arrives and preserves its query', async () => {
  history.replaceState({ otIndex: 0 }, '', '/w/w/issues/by-key/ENG-9?view=mine'); let finish!: (response: Response) => void;
  vi.stubGlobal('fetch', vi.fn(() => new Promise<Response>(resolve => { finish = resolve; })));
  const route = parseRoute(location.pathname + location.search); expect(route.issueId).toBeUndefined(); expect(route.issueKey).toBe('ENG-9'); expect(route.valid).toBe(true);
  await render(<IssueKeyResolver workspaceId="w" issueKey="ENG-9" search={route.search} />); expect(location.pathname).toContain('/by-key/'); await act(async () => finish(json({ ...source, id: 'canonical', key: 'PRO-5' }))); expect(location.pathname).toBe('/w/w/issues/canonical'); expect(location.search).toBe('?view=mine');
});

it('keeps an unavailable alias opaque and ignores a late key response after unmount', async () => {
  history.replaceState({ otIndex: 0 }, '', '/w/w/issues/by-key/OLD-2'); let finish!: (response: Response) => void; let signal!: AbortSignal;
  vi.stubGlobal('fetch', vi.fn((_url: string, options: RequestInit) => { signal = options.signal!; return new Promise<Response>(resolve => { finish = resolve; }); }));
  await render(<IssueKeyResolver workspaceId="w" issueKey="OLD-2" search={new URLSearchParams()} />); await render(<p>Another route</p>); expect(signal.aborted).toBe(true); await act(async () => finish(json(source))); expect(location.pathname).toContain('/by-key/OLD-2');
  vi.stubGlobal('fetch', vi.fn(() => Promise.resolve(json({ code: 'not_found', detail: 'Issue unavailable.' }, 404)))); await render(<IssueKeyResolver workspaceId="w" issueKey="OLD-2" search={new URLSearchParams()} />); expect(location.pathname).toContain('/by-key/OLD-2'); expect(container.textContent).not.toContain(source.title); expect(container.textContent).toContain('Issue unavailable.');
});

it('does not navigate away after a save conflict and keeps the original draft available for review', async () => {
  vi.stubGlobal('fetch', vi.fn((url: string, options: RequestInit) => Promise.resolve(options.method === 'PATCH' ? json({ code: 'conflict', detail: 'Issue changed.', details: { current: { ...source, title: 'Remote edit', version: '2', etag: '"issue:i:2:p"' } } }, 412) : url.endsWith('/parent') ? json(parent(target)) : baseGet(url))));
  await render(<IssueEditor workspaceId="w" issueId="i" data={data} session={session} close={() => {}} updated={() => {}} />); await input(container.querySelector<HTMLTextAreaElement>('#issue-title')!, 'My edit'); await act(async () => container.querySelector<HTMLAnchorElement>('.graph-target')!.click()); await click('Save and open'); await settle(25);
  expect(location.pathname).toBe('/w/w/issues/i'); expect(container.querySelector<HTMLTextAreaElement>('#issue-title')?.value).toBe('My edit'); expect(container.textContent).toContain('Remote edit'); expect(container.querySelector('dialog')).toBeNull(); expect(window.confirm).not.toHaveBeenCalled();
});

it('discards only after explicit in-app approval when following a relationship', async () => {
  const fetch = vi.fn((url: string) => Promise.resolve(url.endsWith('/parent') ? json(parent(target)) : baseGet(url))); vi.stubGlobal('fetch', fetch);
  await render(<IssueEditor workspaceId="w" issueId="i" data={data} session={session} close={() => {}} updated={() => {}} />); await input(container.querySelector<HTMLTextAreaElement>('#issue-title')!, 'My edit'); await act(async () => container.querySelector<HTMLAnchorElement>('.graph-target')!.click()); await click('Discard and open'); await settle(25);
  expect(location.pathname).toBe('/w/w/issues/other'); expect(window.confirm).not.toHaveBeenCalled(); expect(container.querySelector<HTMLTextAreaElement>('#issue-title')?.value).toBe('Main task');
});

it('requires a fresh move preview after expiration and ignores a preview arriving after its view closes', async () => {
  let finish!: (response: Response) => void; let signal!: AbortSignal; let delayed = false; const reviewed = vi.fn();
  vi.stubGlobal('fetch', vi.fn((url: string, options: RequestInit) => {
    if (url.includes('/move-preview?')) { if (delayed) { signal = options.signal!; return new Promise<Response>(resolve => { finish = resolve; }); } return Promise.resolve(json({ ...preview, expires_at: '2000-01-01T00:00:00Z' })); }
    return Promise.resolve(baseGet(url));
  }));
  await render(<MoveIssue workspaceId="w" issue={source} data={data} session={session} close={() => {}} moved={() => {}} reviewed={reviewed} />); await open('Destination team'); await choose('Product'); await acknowledge(); expect(container.textContent).toContain('This review expired.'); expect(button('Confirm move').disabled).toBe(true);
  delayed = true; await click('Refresh move review'); await render(<p>Signed out</p>); const callsBefore = reviewed.mock.calls.length; expect(signal.aborted).toBe(true); await act(async () => finish(json(preview))); expect(reviewed).toHaveBeenCalledTimes(callsBefore); expect(container.textContent).toBe('Signed out');
});

it.each([false, true])('preserves typing during the deferred parent refresh after subissue creation (remote fields changed: %s)', async remoteChanged => {
  let reads = 0; let finishParent!: (response: Response) => void; const writes: RequestInit[] = [];
  vi.stubGlobal('fetch', vi.fn((url: string, options: RequestInit) => {
    if (options.method === 'POST') return Promise.resolve(json(other, 201));
    if (options.method === 'PATCH') { writes.push(options); return Promise.resolve(json({ ...source, title: 'Typed after child creation', version: '3', etag: '"issue:i:3:p"' })); }
    if (url.endsWith('/issues/i') && ++reads > 1) return new Promise<Response>(resolve => { finishParent = resolve; });
    return Promise.resolve(baseGet(url));
  }));
  await render(<IssueEditor workspaceId="w" issueId="i" data={data} session={session} close={() => {}} updated={() => {}} />);
  await click('Add connection'); await click('Create subissue'); await input(container.querySelector<HTMLInputElement>('dialog input[data-autofocus]')!, 'New child'); await submit(container.querySelector('dialog')!);
  expect(container.querySelector('dialog')).toBeNull(); await input(container.querySelector<HTMLTextAreaElement>('#issue-title')!, 'Typed after child creation');
  await act(async () => finishParent(json({ ...source, title: remoteChanged ? 'Edited by another person' : source.title, version: '2', etag: '"issue:i:2:p"' })));
  expect(container.querySelector<HTMLTextAreaElement>('#issue-title')?.value).toBe('Typed after child creation');
  if (remoteChanged) { expect(container.textContent).toContain('Edited by another person'); expect(button('Save changes').disabled).toBe(true); expect(writes).toHaveLength(0); }
  else { await submit(); expect(new Headers(writes[0].headers).get('If-Match')).toBe('"issue:i:2:p"'); expect(JSON.parse(writes[0].body as string)).toEqual({ title: 'Typed after child creation' }); }
});

it('does not deliver a late parent refresh after navigating to another issue', async () => {
  let reads = 0; let finishParent!: (response: Response) => void; let signal!: AbortSignal; const updated = vi.fn();
  vi.stubGlobal('fetch', vi.fn((url: string, options: RequestInit) => {
    if (options.method === 'POST') return Promise.resolve(json(other, 201));
    if (url.endsWith('/issues/i') && ++reads > 1) { signal = options.signal!; return new Promise<Response>(resolve => { finishParent = resolve; }); }
    return Promise.resolve(baseGet(url));
  }));
  await render(<IssueEditor key="i" workspaceId="w" issueId="i" data={data} session={session} close={() => {}} updated={updated} />); await click('Add connection'); await click('Create subissue'); await input(container.querySelector<HTMLInputElement>('dialog input[data-autofocus]')!, 'New child'); await submit(container.querySelector('dialog')!);
  await render(<IssueEditor key="other" workspaceId="w" issueId="other" data={data} session={session} close={() => {}} updated={updated} />); expect(signal.aborted).toBe(true); await act(async () => finishParent(json({ ...source, version: '2', etag: '"issue:i:2:p"' })));
  expect(container.querySelector<HTMLTextAreaElement>('#issue-title')?.value).toBe(other.title); expect(updated).not.toHaveBeenCalled();
});
