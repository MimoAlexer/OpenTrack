// @vitest-environment jsdom
import { act, createRef, type ComponentProps } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { hasUnsavedChanges } from '../router';
import type { Bootstrap, Issue, Session, Status } from '../types';
import type { Workflow } from '../workflows/types';
import { IssueBoard } from './IssueBoard';

type Props = ComponentProps<typeof IssueBoard>;
const session: Session = { audience: 'web', user: { id: 'owner', display_name: 'Example Owner', email: 'owner@example.test' }, csrf_token: 'csrf', workspaces: [{ id: 'w', name: 'Example Workspace', slug: 'example', role: 'owner' }] };
const status = (id: string, name: string, position: number): Status => ({ id, team_id: 't', name, category: 'unstarted', color: 'slate', position, is_default: position === 0, archived_at: null, retiring_at: null, replacement_status_id: null, replacement_job_id: null });
const todo = status('todo', 'Planned', 0); const doing: Status = { ...status('doing', 'In progress', 1), category: 'started' }; const done: Status = { ...status('done', 'Completed', 2), category: 'completed' };
const statuses = [todo, doing, done];
const issue: Issue = { id: 'i', workspace_id: 'w', team_id: 't', key: 'ENG-1', title: 'Private planned work', description: '', status_id: todo.id, priority: 0, assignee_id: null, assignee: null, label_ids: [], labels: [], etag: '"issue:i:1:body-projection"', estimate: null, due_date: null, version: '1', created_at: '2026-10-23T08:00:00Z', updated_at: '2026-10-23T08:00:00Z', archived_at: null };
const data: Bootstrap = { auth_epoch: '1', teams: [{ id: 't', name: 'Example Team', key: 'ENG', private: true, archived_at: null }], statuses, members: [session.user], issues: [issue] };
const json = (body: unknown, statusCode = 200) => new Response(JSON.stringify(body), { status: statusCode });
const problem = (statusCode: number, detail: string, current?: Issue) => json({ code: statusCode === 412 ? 'version_conflict' : 'request_failed', detail, ...(current ? { details: { current } } : {}) }, statusCode);
function deferred<T>() { let resolve!: (value: T) => void; const promise = new Promise<T>(done => { resolve = done; }); return { promise, resolve }; }
let container: HTMLDivElement; let root: Root;
let moved: ReturnType<typeof vi.fn<Props['moved']>>; let reload: ReturnType<typeof vi.fn<Props['reload']>>; let denied: ReturnType<typeof vi.fn<Props['denied']>>; let blockingChanged: ReturnType<typeof vi.fn<Props['blockingChanged']>>;
let workflow: Workflow; let workflowResponse: () => Response | Promise<Response>;

beforeEach(() => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true);
  HTMLDialogElement.prototype.showModal = function () { this.setAttribute('open', ''); };
  HTMLDialogElement.prototype.close = function () { this.removeAttribute('open'); };
  container = document.createElement('div'); document.body.append(container); root = createRoot(container);
  moved = vi.fn(); reload = vi.fn(); denied = vi.fn(); blockingChanged = vi.fn();
  workflow = { team_id: 't', version: '1', can_manage: true, statuses };
  workflowResponse = () => json(workflow);
});
afterEach(async () => { await act(async () => root.unmount()); container.remove(); vi.restoreAllMocks(); vi.unstubAllGlobals(); });

function transport(command: (url: string, options: RequestInit) => Response | Promise<Response> = () => json(issue)) {
  const fetch = vi.fn((url: string, options: RequestInit) => Promise.resolve(url.endsWith('/workflow') ? workflowResponse() : command(url, options)));
  vi.stubGlobal('fetch', fetch); return fetch;
}
async function render(overrides: Partial<Props> = {}) {
  await act(async () => root.render(<IssueBoard workspaceId="w" teamId="t" data={data} session={session} items={[issue]} statuses={statuses} params={new URLSearchParams('team_id=t&layout=board&display_version=1&sort=priority')} disabled={false} readOnly={false} headingRef={createRef<HTMLHeadingElement>()} moved={moved} reload={reload} denied={denied} blockingChanged={blockingChanged} {...overrides} />));
}
function findButton(label: string, scope: ParentNode = container) { return [...scope.querySelectorAll<HTMLButtonElement>('button')].find(button => button.textContent === label || button.getAttribute('aria-label') === label); }
function button(label: string, scope?: ParentNode) { const value = findButton(label, scope); if (!value) throw new Error(`Missing button: ${label}`); return value; }
async function click(label: string, scope?: ParentNode) { await act(async () => button(label, scope).click()); }
async function choose(value: string) {
  const select = container.querySelector<HTMLSelectElement>('[aria-label="Move ENG-1 to status"]');
  if (!select) throw new Error('Missing status choice');
  await act(async () => { Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, 'value')!.set!.call(select, value); select.dispatchEvent(new Event('change', { bubbles: true })); });
}
async function move(value = doing.id) { await click('Move ENG-1'); await choose(value); await click('Move ENG-1', container.querySelector('dialog')!); }
function writes(fetch: ReturnType<typeof transport>) { return fetch.mock.calls.filter(([, options]) => options.method === 'PATCH'); }

it('keeps a retained assignment distinct from an unassigned issue when its profile is unavailable', async () => {
  transport();
  await render({ items: [{ ...issue, assignee_id: 'unavailable-member', assignee: null }] });
  expect(container.querySelector('.avatar')?.getAttribute('aria-label')).toBe('Assignee unavailable');
  expect(container.querySelector('.avatar')?.getAttribute('aria-label')).not.toBe('Unassigned');
});

it('uses the canonical issue ETag and one status-only PATCH after explicit choice', async () => {
  const saved = { ...issue, status_id: doing.id, version: '2', etag: '"issue:i:2:body-projection"' };
  const fetch = transport((_url, options) => options.method === 'PATCH' ? json(saved) : json(issue));
  await render();
  const link = container.querySelector<HTMLAnchorElement>('#row-i')!;
  expect(link.matches('.issue-groups a.issue-row')).toBe(true); expect(link.querySelector('button')).toBeNull();
  expect(link.getAttribute('href')).toContain('layout=board'); expect(link.getAttribute('href')).toContain('sort=priority');
  await move();
  const [[url, options]] = writes(fetch);
  expect(url).toBe('/api/v1/workspaces/w/issues/i'); expect(JSON.parse(options.body as string)).toEqual({ status_id: doing.id });
  expect(new Headers(options.headers).get('If-Match')).toBe(issue.etag); expect(new Headers(options.headers).get('X-CSRF-Token')).toBe(session.csrf_token);
  expect(writes(fetch)).toHaveLength(1); expect(moved).toHaveBeenCalledWith(saved); expect(hasUnsavedChanges()).toBe(false);
});

it('does not send a same-column command or offer unavailable or foreign destinations', async () => {
  const retiring = { ...status('retiring', 'Being replaced', 3), retiring_at: '2026-10-23T09:00:00Z' };
  const archived = { ...status('archived', 'Archived status', 4), archived_at: '2026-10-23T09:00:00Z' };
  const foreign = { ...status('foreign', 'Other team status', 0), team_id: 'other' };
  workflow = { ...workflow, statuses: [...statuses, retiring, archived, foreign] };
  const fetch = transport(); await render({ statuses: [...statuses, retiring, archived, foreign] }); await click('Move ENG-1');
  const select = container.querySelector<HTMLSelectElement>('[aria-label="Move ENG-1 to status"]')!;
  expect([...select.options].filter(option => !option.disabled).map(option => option.value)).not.toContain(retiring.id);
  expect([...select.options].filter(option => !option.disabled).map(option => option.value)).not.toContain(archived.id);
  expect([...select.options].filter(option => !option.disabled).map(option => option.value)).not.toContain(foreign.id);
  await choose(todo.id); await click('Move ENG-1', container.querySelector('dialog')!);
  expect(writes(fetch)).toHaveLength(0); expect(moved).not.toHaveBeenCalled(); expect(container.querySelector('dialog')).toBeNull(); expect(hasUnsavedChanges()).toBe(false);
});

const readOnlyCases: [string, Partial<Props>][] = [
  ['explicit read-only', { readOnly: true }],
  ['pending query', { disabled: true }],
  ['selected issue editor', { selectedId: issue.id }],
  ['viewer', { session: { ...session, workspaces: [{ ...session.workspaces[0], role: 'viewer' }] } }],
  ['archived issue', { items: [{ ...issue, archived_at: '2026-10-23T09:00:00Z' }] }],
  ['archived team', { data: { ...data, teams: [{ ...data.teams[0], archived_at: '2026-10-23T09:00:00Z' }] } }],
];
it.each(readOnlyCases)('blocks movement for %s without dispatching a write', async (_name, overrides) => {
  const fetch = transport(); await render(overrides);
  for (const label of ['Move ENG-1', 'Drag ENG-1']) { const control = findButton(label); expect(!control || control.disabled).toBe(true); if (control) await act(async () => control.click()); }
  expect(container.querySelector('dialog')).toBeNull(); expect(writes(fetch)).toHaveLength(0);
});

it('waits for a live workflow before allowing mutation', async () => {
  const pending = deferred<Response>(); workflowResponse = () => pending.promise;
  const fetch = transport(); await render(); const control = findButton('Move ENG-1'); expect(!control || control.disabled).toBe(true);
  expect(writes(fetch)).toHaveLength(0); await act(async () => pending.resolve(json(workflow)));
  expect(button('Move ENG-1').disabled).toBe(false);
});

it('retains a stale move until review and applies it with the fresh issue ETag', async () => {
  const current = { ...issue, title: 'Changed by another editor', status_id: done.id, version: '2', etag: '"issue:i:2:current"' };
  const saved = { ...current, status_id: doing.id, version: '3', etag: '"issue:i:3:saved"' }; let attempts = 0;
  const fetch = transport((_url, options) => options.method === 'PATCH' ? ++attempts === 1 ? problem(412, 'This issue changed.', current) : json(saved) : json(current));
  await render(); await move(); expect(writes(fetch)).toHaveLength(1); expect(moved).not.toHaveBeenCalled(); expect(hasUnsavedChanges()).toBe(true);
  expect(fetch.mock.calls.some(([url, options]) => url.endsWith('/issues/i') && options.method !== 'PATCH')).toBe(true);
  await click('Review move'); expect(writes(fetch)).toHaveLength(1);
  await click('Apply move');
  expect(writes(fetch)).toHaveLength(2); expect(new Headers(writes(fetch)[1][1].headers).get('If-Match')).toBe(current.etag);
  expect(JSON.parse(writes(fetch)[1][1].body as string)).toEqual({ status_id: doing.id }); expect(moved).toHaveBeenCalledWith(saved); expect(hasUnsavedChanges()).toBe(false);
});

it('checks an interrupted write before offering an explicit reviewed retry', async () => {
  const current = { ...issue, title: 'Latest saved source', version: '2', etag: '"issue:i:2:current"' };
  let attempts = 0;
  const fetch = transport((_url, options) => {
    if (options.method !== 'PATCH') return json(current);
    if (++attempts === 1) return Promise.reject(new TypeError('Interrupted response'));
    return json({ ...current, status_id: doing.id, version: '3', etag: '"issue:i:3:saved"' });
  });
  await render(); await move(); expect(writes(fetch)).toHaveLength(1); expect(hasUnsavedChanges()).toBe(true);
  expect(container.querySelector<HTMLSelectElement>('[aria-label="Move ENG-1 to status"]')?.disabled).toBe(true); expect(findButton('Apply move')).toBeUndefined();
  const before = fetch.mock.calls.length; await click('Check move result');
  expect(fetch.mock.calls.slice(before).some(([url, options]) => url.endsWith('/issues/i') && options.method !== 'PATCH')).toBe(true);
  expect(writes(fetch)).toHaveLength(1); await click('Review move'); expect(writes(fetch)).toHaveLength(1); await click('Apply move'); expect(writes(fetch)).toHaveLength(2);
  expect(new Headers(writes(fetch)[1][1].headers).get('If-Match')).toBe(current.etag);
});

it('reconciles a committed interrupted move without submitting it twice', async () => {
  const saved = { ...issue, status_id: doing.id, version: '2', etag: '"issue:i:2:committed"' };
  const fetch = transport((_url, options) => options.method === 'PATCH' ? Promise.reject(new TypeError('Interrupted response')) : json(saved));
  await render(); await move(); expect(writes(fetch)).toHaveLength(1); expect(moved).not.toHaveBeenCalled();
  await click('Check move result'); expect(writes(fetch)).toHaveLength(1); expect(moved).toHaveBeenCalledWith(saved); expect(hasUnsavedChanges()).toBe(false);
});

it('does not roll unrelated newer parent rows back when an optimistic move is rejected', async () => {
  const pending = deferred<Response>(); const other = { ...issue, id: 'other', key: 'ENG-2', title: 'Older second issue' };
  const fetch = transport((_url, options) => options.method === 'PATCH' ? pending.promise : json(issue));
  await render({ items: [issue, other] }); await move();
  await render({ items: [issue, { ...other, title: 'Newer parent rendering', version: '2' }] });
  await act(async () => pending.resolve(problem(422, 'The destination is no longer available.')));
  expect(container.textContent).toContain('Newer parent rendering'); expect(container.textContent).not.toContain('Older second issue'); expect(writes(fetch)).toHaveLength(1); expect(moved).not.toHaveBeenCalled();
});

it.each([403, 404])('conceals the source and columns when its mutation loses access (%s)', async statusCode => {
  const fetch = transport((_url, options) => options.method === 'PATCH' ? problem(statusCode, 'Access changed.') : json(issue));
  await render(); await move();
  expect(denied).toHaveBeenCalledWith(expect.objectContaining({ status: statusCode }));
  expect(container.querySelector('#row-i')).toBeNull(); expect(container.textContent).not.toContain(issue.title); expect(container.textContent).not.toContain(todo.name); expect(writes(fetch)).toHaveLength(1);
});

it.each(['issue', 'workflow'])('denies stale recovery when the current %s is inaccessible', async deniedResource => {
  const fetch = transport((_url, options) => {
    if (options.method !== 'PATCH') return deniedResource === 'issue' ? problem(404, 'Issue access changed.') : json(issue);
    if (deniedResource === 'workflow') workflowResponse = () => problem(404, 'Workflow access changed.');
    return problem(412, 'This issue changed.', issue);
  });
  await render(); await move();
  expect(denied).toHaveBeenCalledWith(expect.objectContaining({ status: 404 })); expect(moved).not.toHaveBeenCalled(); expect(writes(fetch)).toHaveLength(1);
  expect(container.querySelector('#row-i')).toBeNull(); expect(container.textContent).not.toContain(todo.name); expect(findButton('Review move')).toBeUndefined(); expect(hasUnsavedChanges()).toBe(false);
});

it('purges denied live workflow metadata and ignores a pending move acknowledgement', async () => {
  const pending = deferred<Response>(); transport((_url, options) => options.method === 'PATCH' ? pending.promise : json(issue));
  await render(); await move(); workflowResponse = () => problem(404, 'Team access changed.');
  await act(async () => window.dispatchEvent(new Event('focus')));
  expect(container.querySelector('#row-i')).toBeNull(); expect(container.textContent).not.toContain(todo.name); expect(denied).toHaveBeenCalled();
  const callbacks = [moved.mock.calls.length, reload.mock.calls.length, denied.mock.calls.length];
  await act(async () => pending.resolve(json({ ...issue, title: 'Late private response', status_id: doing.id, version: '2' })));
  expect(container.textContent).not.toContain('Late private response'); expect([moved.mock.calls.length, reload.mock.calls.length, denied.mock.calls.length]).toEqual(callbacks);
});

it.each(['account', 'filters', 'unmount'])('ignores a move completed after %s and releases its dirty guard', async change => {
  const pending = deferred<Response>(); transport((_url, options) => options.method === 'PATCH' ? pending.promise : json(issue)); await render(); await move();
  expect(hasUnsavedChanges()).toBe(true);
  if (change === 'account') await render({ session: { ...session, user: { id: 'member', display_name: 'Example Member', email: 'member@example.test' }, csrf_token: 'replacement' }, items: [] });
  else if (change === 'filters') await render({ params: new URLSearchParams('team_id=t&layout=board&display_version=1&sort=priority&assignee_id=member'), items: [] });
  else await act(async () => root.render(<p>Another view</p>));
  const callbacks = [moved.mock.calls.length, reload.mock.calls.length, denied.mock.calls.length];
  await act(async () => pending.resolve(json({ ...issue, title: 'Previous account response', status_id: doing.id, version: '2' })));
  expect([moved.mock.calls.length, reload.mock.calls.length, denied.mock.calls.length]).toEqual(callbacks); expect(container.textContent).not.toContain('Previous account response'); expect(hasUnsavedChanges()).toBe(false);
});

it('keeps at most five columns and fifty cards mounted while every page card remains reachable', async () => {
  const manyStatuses = Array.from({ length: 7 }, (_, index) => status(`s${index}`, `Stage ${index + 1}`, index));
  const manyIssues = Array.from({ length: 51 }, (_, index) => ({ ...issue, id: `item-${index}`, key: `ENG-${index + 1}`, title: `Page issue ${index + 1}`, status_id: manyStatuses[index % 7].id }));
  workflow = { ...workflow, statuses: manyStatuses }; transport(); await render({ items: manyIssues, statuses: manyStatuses, data: { ...data, statuses: manyStatuses } });
  const seen = new Set<string>();
  for (let page = 0; page < manyStatuses.length; page += 1) {
    const links = [...container.querySelectorAll<HTMLAnchorElement>('.issue-groups a.issue-row')]; expect(links.length).toBeLessThanOrEqual(50);
    const presentStatuses = new Set(links.map(link => manyIssues.find(item => `row-${item.id}` === link.id)!.status_id)); expect(presentStatuses.size).toBeLessThanOrEqual(5);
    links.forEach(link => seen.add(link.id)); const next = findButton('Next columns'); if (!next || next.disabled) break; await click('Next columns');
  }
  expect(seen.size).toBe(50); expect(seen.has('row-item-50')).toBe(false);
});
