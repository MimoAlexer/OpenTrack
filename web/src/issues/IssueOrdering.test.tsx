// @vitest-environment jsdom
import { act, type ReactNode } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { ApiError } from '../api';
import { hasUnsavedChanges } from '../router';
import type { TeamDetail } from '../settings/types';
import type { Issue, Session, Status } from '../types';
import type { Workflow } from '../workflows/types';
import { IssueOrderingDialog, IssueOrderProgress, orderMaintenance, useIssueOrderDrag, useIssueOrdering, type IssueOrderingController, type OrderMaintenance } from './IssueOrdering';

const session: Session = { audience: 'web', user: { id: 'owner', display_name: 'Example Owner', email: 'owner@example.test' }, csrf_token: 'csrf-one', workspaces: [{ id: 'workspace', name: 'Example Workspace', slug: 'example', role: 'owner' }] };
const status: Status = { id: 'status', team_id: 'team', name: 'Ready', category: 'unstarted', color: 'slate', position: 0, is_default: true, archived_at: null, retiring_at: null, replacement_status_id: null, replacement_job_id: null };
const targetStatus: Status = { ...status, id: 'target-status', name: 'In progress', category: 'started', position: 1, is_default: false };
const workflow: Workflow = { team_id: 'team', version: '1', can_manage: true, statuses: [status, targetStatus] };
const source: Issue = { id: 'issue', workspace_id: 'workspace', team_id: 'team', key: 'EX-1', title: 'Current private issue', description: '', body_version: '1', status_id: 'status', priority: 0, assignee_id: null, assignee: null, label_ids: [], labels: [], etag: '"issue:issue:1"', estimate: null, due_date: null, version: '1', created_at: '2026-10-25T10:00:00Z', updated_at: '2026-10-25T10:00:00Z', archived_at: null };
const anchor: Issue = { ...source, id: 'anchor', key: 'EX-2', title: 'Current reference issue', etag: '"issue:anchor:1"' };
const team: TeamDetail = { id: 'team', workspace_id: 'workspace', name: 'Example private team', key: 'EX', private: true, archived_at: null, can_manage: true, version: '1' };
const job: OrderMaintenance = { id: 'job', team_id: 'team', status_id: 'status', state: 'queued', phase: 'staging', staged_count: '0', applied_count: '0', total_count: null, version: '1', error_code: null, can_resume: false, etag: '"issue-order-maintenance:job:1:false:authority"' };
const issuePath = '/api/v1/workspaces/workspace/issues/issue';
const teamPath = '/api/v1/workspaces/workspace/teams/team';
const jobPath = `${teamPath}/issue-order-maintenance/job`;
const json = (value: unknown, statusCode = 200) => new Response(JSON.stringify(value), { status: statusCode, headers: { 'Content-Type': 'application/json' } });
const page = (items: Issue[] = [source, anchor], version = '1:1') => ({ data: items, next_cursor: null, has_more: false, order_version: version });
const maintenance = (jobs = [job]) => json({ code: 'issue_order_maintenance', detail: 'Issue order maintenance is active; this change was not applied', details: { applied: false, maintenance: jobs } }, 409);
function deferred() { let resolve!: (value: Response) => void; const promise = new Promise<Response>(done => { resolve = done; }); return { promise, resolve }; }
type Options = Parameters<typeof useIssueOrdering>[0];
function options(overrides: Partial<Options> = {}): Options { return { workspaceId: 'workspace', session, authority: 'account:workspace:epoch-one', ready: true, readOnly: false, updated: vi.fn(), reload: vi.fn(), sourceDenied: vi.fn(), teamDenied: vi.fn(), ...overrides }; }
let controller: IssueOrderingController;
function Ordering({ config }: { config: Options }) { controller = useIssueOrdering(config); return <IssueOrderingDialog controller={controller} workspaceId={config.workspaceId} session={config.session} teamDenied={config.teamDenied} />; }
let container: HTMLDivElement; let root: Root; let hit: Element | null;
const originalHitTest = Object.getOwnPropertyDescriptor(document, 'elementFromPoint');
beforeEach(() => {
  vi.useFakeTimers(); vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true);
  HTMLDialogElement.prototype.showModal = function () { this.setAttribute('open', ''); };
  HTMLDialogElement.prototype.close = function () { this.removeAttribute('open'); };
  hit = null; Object.defineProperty(document, 'elementFromPoint', { configurable: true, value: () => hit });
  container = document.createElement('div'); document.body.append(container); root = createRoot(container);
});
afterEach(async () => {
  await act(async () => root.unmount()); container.remove(); vi.useRealTimers(); vi.restoreAllMocks(); vi.unstubAllGlobals();
  if (originalHitTest) Object.defineProperty(document, 'elementFromPoint', originalHitTest); else delete (document as Partial<Document>).elementFromPoint;
});
async function render(node: ReactNode) { await act(async () => root.render(node)); }
function button(label: string) { const value = [...container.querySelectorAll<HTMLButtonElement>('button')].find(item => item.textContent === label || item.getAttribute('aria-label') === label); if (!value) throw new Error(`Missing button ${label}`); return value; }
async function click(label: string) { await act(async () => button(label).click()); }
async function begin(pointer = false, choice?: Parameters<IssueOrderingController['begin']>[3]) { await act(async () => controller.begin(source, 'team', '1:1', choice, pointer)); }
async function tick(ms: number) { await act(async () => { await vi.advanceTimersByTimeAsync(ms); }); }
function readFixture(url: string, current = source, version = '1:1') {
  if (url === issuePath) return json(current);
  if (url === `${teamPath}/workflow`) return json(workflow);
  if (url === `${teamPath}`) return json(team);
  if (url.endsWith('/issues/anchor')) return json(anchor);
  if (url.includes('/issues?')) return json(page([current, anchor].filter(item => item.status_id === new URL(url, 'https://example.test').searchParams.get('status_id')), version));
  throw new Error(`Unexpected scoped request ${url}`);
}

it('checks an unknown position with the identical request across authority refresh and installs only canonical saved state', async () => {
  let posts = 0; const canonical = { ...source, version: '7', etag: '"issue:issue:7"', title: 'Latest saved title' };
  const historical = { ...source, version: '2', etag: '"issue:issue:2"', title: 'Historical acknowledgement title' };
  const fetch = vi.fn((url: string, init: RequestInit) => init.method === 'POST' ? ++posts === 1 ? Promise.reject(new TypeError('Response lost')) : Promise.resolve(json({ issue: historical, order_version: '2:1' })) : Promise.resolve(readFixture(url, canonical, '7:1')));
  vi.stubGlobal('fetch', fetch); const config = options(); await render(<Ordering config={config} />); await begin(true, { placement: 'before', anchorId: anchor.id });
  expect(hasUnsavedChanges()).toBe(true); expect(container.textContent).toContain('outcome is uncertain');
  const next = { ...config, authority: 'account:workspace:epoch-two', session: { ...session, csrf_token: 'csrf-two' } };
  await render(<Ordering config={{ ...next, ready: false }} />); expect(container.querySelector('dialog')).toBeNull(); expect(container.textContent).not.toContain(source.title); expect(hasUnsavedChanges()).toBe(true);
  await render(<Ordering config={next} />); expect(posts).toBe(1);
  await act(async () => controller.choose({ placement: 'after', anchorId: 'replacement', statusId: targetStatus.id }));
  await click('Check same position request');
  const writes = fetch.mock.calls.filter(([, init]) => init.method === 'POST'); expect(writes).toHaveLength(2);
  expect(writes[1][1].body).toBe(writes[0][1].body); expect(JSON.parse(writes[1][1].body as string)).toEqual({ order_version: '1:1', status_id: status.id, before_id: anchor.id });
  for (const name of ['If-Match', 'Idempotency-Key']) expect(new Headers(writes[1][1].headers).get(name)).toBe(new Headers(writes[0][1].headers).get(name));
  expect(new Headers(writes[1][1].headers).get('X-CSRF-Token')).toBe('csrf-two');
  expect(config.updated).toHaveBeenCalledExactlyOnceWith(canonical); expect(config.reload).toHaveBeenCalledOnce(); expect(container.textContent).not.toContain(historical.title); expect(controller.state).toBeNull();
});

it.each([409, 412])('requires fresh review and a new key after a confirmed position %s without automatically retrying', async statusCode => {
  let posts = 0; let current = { ...source, version: '2', etag: '"issue:issue:2"' };
  const fetch = vi.fn((url: string, init: RequestInit) => {
    if (init.method !== 'POST') return Promise.resolve(readFixture(url, current, `${current.version}:1`));
    posts += 1;
    if (posts === 1) return Promise.resolve(json({ code: statusCode === 412 ? 'version_conflict' : 'conflict', detail: 'The saved order changed.', ...(statusCode === 412 ? { details: { current } } : {}) }, statusCode));
    current = { ...current, version: '3', etag: '"issue:issue:3"' }; return Promise.resolve(json({ issue: current, order_version: '3:1' }));
  }); vi.stubGlobal('fetch', fetch); const config = options(); await render(<Ordering config={config} />); await begin(true, { placement: 'before', anchorId: anchor.id });
  expect(posts).toBe(1); expect(container.textContent).toContain('move was not applied'); expect(button('Apply reviewed position').disabled).toBe(false); expect(config.updated).not.toHaveBeenCalled();
  await click('Apply reviewed position'); const writes = fetch.mock.calls.filter(([, init]) => init.method === 'POST'); expect(writes).toHaveLength(2);
  expect(new Headers(writes[1][1].headers).get('Idempotency-Key')).not.toBe(new Headers(writes[0][1].headers).get('Idempotency-Key'));
  expect(new Headers(writes[1][1].headers).get('If-Match')).toBe('"issue:issue:2"'); expect(JSON.parse(writes[1][1].body as string)).toEqual({ order_version: '2:1', status_id: status.id, before_id: anchor.id });
  expect(config.updated).toHaveBeenCalledExactlyOnceWith(current);
});

it('keeps a maintenance-rejected intent, shows an unknown total and never automatically moves it after completion', async () => {
  let posts = 0; let completed = false;
  const fetch = vi.fn((url: string, init: RequestInit) => {
    if (init.method === 'POST') { posts += 1; return Promise.resolve(maintenance()); }
    if (url === jobPath) return Promise.resolve(json(completed ? { ...job, phase: 'completed', state: 'succeeded', staged_count: '501', applied_count: '501', total_count: '501', version: '4', etag: '"completed"' } : job));
    if (url.includes('/issues?') && posts > 0 && !completed) return Promise.resolve(maintenance());
    return Promise.resolve(readFixture(url, source, completed ? '2:1' : '1:1'));
  }); vi.stubGlobal('fetch', fetch); await render(<Ordering config={options()} />); await begin(); await click('Apply reviewed position');
  expect(controller.state?.outcome).toBe('not-applied'); expect(container.textContent).toContain('total not known yet'); expect(button('Apply reviewed position').disabled).toBe(true); expect(posts).toBe(1);
  completed = true; await tick(2000); expect(container.textContent).toContain('Order preparation complete.'); expect(posts).toBe(1);
  await click('Refresh current issue and order'); expect(button('Apply reviewed position').disabled).toBe(false); expect(posts).toBe(1);
});

it('recognizes only the complete maintenance contract as confirmation that a move was not applied', () => {
  expect(orderMaintenance(new ApiError(409, 'issue_order_maintenance', 'Not applied', undefined, { applied: false, maintenance: [job] }))).toEqual([job]);
  for (const details of [{ applied: true, maintenance: [job] }, { applied: false, maintenance: [] }, { applied: false, maintenance: [{ ...job, total_count: undefined }] }]) expect(orderMaintenance(new ApiError(409, 'issue_order_maintenance', 'Unverified', undefined, details))).toBeNull();
  expect(orderMaintenance(new ApiError(409, 'conflict', 'Changed', undefined, { applied: false, maintenance: [job] }))).toBeNull();
});

it('uses only the fresh server resume permission and ETag before posting a maintenance resume', async () => {
  let reads = 0; let resumed = false; const changed = vi.fn();
  const displayed = { ...job, state: 'blocked' as const, can_resume: true, error_code: 'access_lost' as const, etag: '"displayed-job-tag"' };
  const fresh = { ...displayed, version: '3', etag: '"fresh-authorized-job-tag"' };
  const fetch = vi.fn((url: string, init: RequestInit) => {
    expect(url === jobPath || url === `${jobPath}/resume`).toBe(true);
    if (init.method === 'POST') { resumed = true; return Promise.resolve(json({ ...job, version: '4', etag: '"queued-job-tag"' })); }
    reads += 1; return Promise.resolve(json(resumed ? { ...job, version: '4', etag: '"queued-job-tag"' } : reads === 1 ? displayed : fresh));
  }); vi.stubGlobal('fetch', fetch); await render(<IssueOrderProgress workspaceId="workspace" session={session} initial={[displayed]} changed={changed} />);
  await click('Resume order preparation'); const writes = fetch.mock.calls.filter(([, init]) => init.method === 'POST'); expect(writes).toHaveLength(1);
  expect(new Headers(writes[0][1].headers).get('If-Match')).toBe(fresh.etag); expect(new Headers(writes[0][1].headers).has('Idempotency-Key')).toBe(false); expect(writes[0][1].body).toBeUndefined();
  expect(reads).toBeGreaterThanOrEqual(2); expect(changed).not.toHaveBeenCalled();
});

it('does not resume after the fresh job read withdraws can_resume even if the displayed job permitted it', async () => {
  let reads = 0; const permitted = { ...job, state: 'blocked' as const, can_resume: true };
  const fetch = vi.fn((_url: string, _init: RequestInit) => Promise.resolve(json(++reads === 1 ? permitted : { ...permitted, can_resume: false, etag: '"permission-withdrawn"' })));
  vi.stubGlobal('fetch', fetch); await render(<IssueOrderProgress workspaceId="workspace" session={session} initial={[permitted]} />); await click('Resume order preparation');
  expect(fetch.mock.calls.filter(([, init]) => init.method === 'POST')).toHaveLength(0); expect(container.textContent).toContain('cannot currently be resumed');
});

it.each([200, 403, 404])('checks team access after job 404 and purges old progress with team response %s', async teamStatus => {
  let reads = 0; const teamDenied = vi.fn();
  const displayed = { ...job, phase: 'applying' as const, state: 'blocked' as const, staged_count: '501', applied_count: '100', total_count: '501', can_resume: true };
  const fetch = vi.fn((url: string) => {
    if (url === teamPath) return Promise.resolve(teamStatus === 200 ? json(team) : json({ code: 'not_found', detail: 'Team unavailable' }, teamStatus));
    expect(url).toBe(jobPath); return Promise.resolve(++reads === 1 ? json(displayed) : json({ code: 'not_found', detail: 'Job unavailable' }, 404));
  }); vi.stubGlobal('fetch', fetch); await render(<IssueOrderProgress workspaceId="workspace" session={session} initial={[displayed]} teamDenied={teamDenied} />);
  expect(container.textContent).toContain('Applied 100 of 501'); expect(button('Resume order preparation')).toBeDefined(); await tick(2000);
  expect(container.textContent).not.toContain('Applied 100 of 501'); expect([...container.querySelectorAll('button')].some(item => item.textContent === 'Resume order preparation')).toBe(false);
  expect(fetch.mock.calls.some(([url]) => url === teamPath)).toBe(true);
  if (teamStatus === 200) expect(teamDenied).not.toHaveBeenCalled(); else expect(teamDenied).toHaveBeenCalledExactlyOnceWith('team', expect.objectContaining({ status: teamStatus }));
});

it('ignores an old issue read after authority changes and hides all facts while authority is unknown', async () => {
  const pending = deferred(); let reads = 0;
  const fetch = vi.fn((url: string, _init: RequestInit) => url === issuePath && ++reads === 1 ? pending.promise : Promise.resolve(readsForLatest(url)));
  function readsForLatest(url: string) { return readFixture(url, { ...source, title: 'Authorized new title', version: '3', etag: '"issue:issue:3"' }, '3:1'); }
  vi.stubGlobal('fetch', fetch); const config = options(); await render(<Ordering config={config} />); await begin();
  const next = { ...config, authority: 'account:workspace:epoch-two' }; await render(<Ordering config={{ ...next, ready: false }} />);
  expect(container.querySelector('dialog')).toBeNull(); expect(container.textContent).not.toContain(source.title);
  await render(<Ordering config={next} />); expect(container.textContent).toContain('Authorized new title');
  await act(async () => pending.resolve(json({ ...source, title: 'Stale former title' })));
  expect(container.textContent).not.toContain('Stale former title'); expect(container.textContent).toContain('Authorized new title');
  expect(config.updated).not.toHaveBeenCalled(); expect(config.sourceDenied).not.toHaveBeenCalled();
});

it('ignores late maintenance progress from the previous session without restoring its resume control', async () => {
  const pending = deferred(); let reads = 0;
  vi.stubGlobal('fetch', vi.fn(() => ++reads === 1 ? pending.promise : Promise.resolve(json(job))));
  await render(<IssueOrderProgress workspaceId="workspace" session={session} initial={[job]} />);
  await render(<IssueOrderProgress workspaceId="workspace" session={{ ...session, csrf_token: 'csrf-two' }} initial={[job]} />);
  await act(async () => pending.resolve(json({ ...job, state: 'blocked', can_resume: true, phase: 'applying', staged_count: '501', applied_count: '100', total_count: '501' })));
  expect(container.textContent).toContain('total not known yet'); expect(container.textContent).not.toContain('Applied 100 of 501'); expect([...container.querySelectorAll('button')].some(item => item.textContent === 'Resume order preparation')).toBe(false);
});

function Dragging({ config }: { config: Options }) {
  const order = useIssueOrdering(config); const target = { ...anchor, status_id: targetStatus.id };
  const drag = useIssueOrderDrag({ enabled: true, controller: order, teamId: 'team', orderVersion: '1:1', items: [source, target] });
  return <><section data-order-status={status.id}><article data-order-issue={source.id}><button onPointerDown={event => drag.pointerDown(event, source)} onPointerMove={drag.pointerMove} onPointerUp={drag.pointerUp}>Drag source</button></article></section>
    <section data-order-status={targetStatus.id}><article data-order-issue={target.id}>Reference row</article><div data-testid="empty-target">Empty column area</div></section><output>{drag.drag ? 'Drag active' : 'No drag'}</output></>;
}
async function pointer(target: Element, type: string, y: number) {
  const event = new MouseEvent(type, { bubbles: true, cancelable: true, button: 0, clientX: 20, clientY: y });
  Object.defineProperties(event, { pointerId: { value: 1 }, pointerType: { value: 'mouse' }, isPrimary: { value: true } });
  await act(async () => { target.dispatchEvent(event); });
}

it.each(['before', 'after', 'append'] as const)('posts a pointer %s placement with only one server-resolved anchor or an empty-column append', async placement => {
  let current = source;
  const fetch = vi.fn((url: string, init: RequestInit) => {
    if (init.method !== 'POST') return Promise.resolve(readFixture(url, current, '2:1'));
    current = { ...source, status_id: targetStatus.id, version: '2', etag: '"issue:issue:2"' }; return Promise.resolve(json({ issue: current, order_version: '2:1' }));
  }); vi.stubGlobal('fetch', fetch); await render(<Dragging config={options()} />);
  const handle = button('Drag source'); const row = container.querySelector('[data-order-issue="anchor"]')!;
  vi.spyOn(row, 'getBoundingClientRect').mockReturnValue(new DOMRect(0, 100, 100, 100)); hit = placement === 'append' ? container.querySelector('[data-testid="empty-target"]') : row;
  await pointer(handle, 'pointerdown', 0); await pointer(handle, 'pointermove', placement === 'before' ? 110 : 190); await pointer(handle, 'pointerup', placement === 'before' ? 110 : 190);
  const writes = fetch.mock.calls.filter(([, init]) => init.method === 'POST'); expect(writes).toHaveLength(1);
  expect(JSON.parse(writes[0][1].body as string)).toEqual({ order_version: '1:1', status_id: targetStatus.id, ...(placement === 'append' ? {} : placement === 'before' ? { before_id: anchor.id } : { after_id: anchor.id }) });
  expect(new Headers(writes[0][1].headers).get('If-Match')).toBe(source.etag);
});

it('cancels a lifted pointer move with Escape before any position request is dispatched', async () => {
  const fetch = vi.fn(() => Promise.resolve(json({}))); vi.stubGlobal('fetch', fetch); await render(<Dragging config={options()} />);
  const handle = button('Drag source'); hit = container.querySelector('[data-testid="empty-target"]');
  await pointer(handle, 'pointerdown', 0); await pointer(handle, 'pointermove', 100); expect(container.textContent).toContain('Drag active');
  const escape = new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }); await act(async () => { window.dispatchEvent(escape); });
  await pointer(handle, 'pointerup', 100); expect(escape.defaultPrevented).toBe(true); expect(container.textContent).toContain('No drag'); expect(fetch).not.toHaveBeenCalled();
});
