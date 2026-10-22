// @vitest-environment jsdom
import { act, type ReactNode } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { EMPTY_BODY } from '../documents/model';
import { Milestones, type Milestone, type MilestoneOverview } from '../projects/Milestones';
import { ProjectIssues } from '../projects/ProjectIssues';
import type { Project } from '../projects/types';
import { hasUnsavedChanges, navigate, parseRoute } from '../router';
import type { Bootstrap, Session, Team } from '../types';
import { CarryForward, CloseCycle } from './CycleActions';
import { CycleEditor } from './CycleEditor';
import { CycleHistory, ScopeSummary, ScopeTable } from './CycleScope';
import { Cycles } from './Cycles';
import type { CloseReport, Cycle, Snapshot } from './types';

const team: Team = { id: 'team', name: 'Example Team', key: 'EXA', private: false, archived_at: null };
const session: Session = { audience: 'web', user: { id: 'owner', email: 'owner@example.test', display_name: 'Example Owner' }, csrf_token: 'csrf', workspaces: [{ id: 'workspace', name: 'Example Workspace', slug: 'example', role: 'owner' }] };
const data: Bootstrap = { auth_epoch: '1', teams: [team], issues: [], members: [session.user], statuses: [] };
const project: Project = { id: 'project', workspace_id: 'workspace', team_id: 'team', name: 'Example project', summary: '', narrative: { body: EMPTY_BODY, plain_text: '', schema_supported: true, schema_version: 1 }, state: 'planned', health: 'on_track', lead_id: null, lead: null, start_date: null, target_date: null, version: '1', etag: '"project:1"', created_at: '2026-10-22T10:00:00Z', updated_at: '2026-10-22T10:00:00Z', archived_at: null, can_edit: true, can_manage: true };
const milestone: Milestone = { id: 'first', workspace_id: 'workspace', team_id: 'team', project_id: 'project', title: 'First outcome', target_date: null, rank: '1', state: 'planned', completed_at: null, version: '1', etag: '"milestone:1"', can_manage: true, can_assign: true, created_at: project.created_at, updated_at: project.updated_at };
const progress = { completed: 0, eligible: 0, canceled: 0, open: 0, total: 0, unscoped: true, as_of: project.created_at };
const cycle: Cycle = { id: 'cycle', workspace_id: 'workspace', team_id: 'team', name: 'Spring interval', start_date: '2026-03-28', end_date_exclusive: '2026-03-31', timezone: 'Europe/Berlin', starts_at: '2026-03-27T23:00:00Z', ends_at: '2026-03-30T22:00:00Z', state: 'active', version: '1', etag: '"cycle:1"', closed_at: null, latest_report_id: null, created_at: project.created_at, updated_at: project.updated_at, can_plan: true, can_manage: true };
const snapshot: Snapshot = { id: 'snapshot', cycle_id: 'cycle', local_date: '2026-03-28', captured_at: '2026-03-28T12:00:00Z', kind: 'scope', definition_version: 1, scope_count: 0, completed_count: 0, canceled_count: 0, unfinished_count: 0, excluded_count: 0, estimate_total: 0, items: [], redacted: false };
const report: CloseReport = { id: 'report', cycle: { ...cycle, state: 'closed', latest_report_id: 'report' }, closed_at: cycle.updated_at, scope: { ...snapshot, scope_count: 2, unfinished_count: 1, completed_count: 1, items: [{ issue_id: 'unfinished', issue_version: '3', category: 'started', estimate: 3, excluded: false }, { issue_id: 'done', issue_version: '4', category: 'completed', estimate: 1, excluded: false }] } };
const json = (value: unknown, status = 200) => new Response(JSON.stringify(value), { status });
const page = (data: unknown[], cursor: string | null = null) => ({ data, has_more: !!cursor, next_cursor: cursor });
let container: HTMLDivElement; let root: Root;
beforeEach(() => { vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true); vi.spyOn(window, 'confirm').mockReturnValue(true); HTMLDialogElement.prototype.showModal = function () { this.setAttribute('open', ''); }; HTMLDialogElement.prototype.close = function () { this.removeAttribute('open'); }; container = document.createElement('div'); document.body.append(container); root = createRoot(container); });
afterEach(async () => { await act(async () => root.unmount()); container.remove(); vi.restoreAllMocks(); vi.unstubAllGlobals(); });
async function render(node: ReactNode) { await act(async () => root.render(node)); }
function button(name: string) { const found = [...container.querySelectorAll<HTMLButtonElement>('button')].find(item => item.textContent === name || item.getAttribute('aria-label') === name); if (!found) throw new Error(`Missing button ${name}`); return found; }
async function click(name: string) { await act(async () => button(name).click()); }
async function input(selector: string, value: string) { const element = container.querySelector<HTMLInputElement | HTMLSelectElement>(selector)!; await act(async () => { const proto = element instanceof HTMLSelectElement ? HTMLSelectElement.prototype : HTMLInputElement.prototype; Object.getOwnPropertyDescriptor(proto, 'value')!.set!.call(element, value); element.dispatchEvent(new Event(element instanceof HTMLSelectElement ? 'change' : 'input', { bubbles: true })); }); }
async function submit() { await act(async () => container.querySelector('form')!.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }))); }
const resource = (items: MilestoneOverview[]) => ({ items, loading: false, error: null, retry: vi.fn() });

describe('milestone planning', () => {
  it('shows manual milestone state independently and never invents empty completion', async () => {
    await render(<Milestones project={project} session={session} resource={resource([{ milestone: { ...milestone, state: 'completed' }, progress }])} readOnly changed={() => {}} denied={() => {}}/>);
    expect(container.textContent).toContain('Manually completed'); expect(container.textContent).toContain('No eligible issue scope'); expect(container.textContent).not.toContain('% complete'); expect(container.querySelector('progress')).toBeNull(); expect(container.querySelector('button')).toBeNull();
  });
  it('uses full persisted milestone order and stale project validator without optimistic movement', async () => {
    const changed = vi.fn(); const retry = vi.fn(); const items = [{ milestone, progress }, { milestone: { ...milestone, id: 'second', title: 'Second outcome', rank: '2' }, progress }];
    const fetch = vi.fn((_url: string, options: RequestInit) => Promise.resolve(options.method === 'PUT' ? json({ code: 'version_conflict', detail: 'Order changed' }, 412) : json({ ...project, version: '2', etag: '"project:2"' }))); vi.stubGlobal('fetch', fetch);
    await render(<Milestones project={project} session={session} resource={{ ...resource(items), retry }} readOnly={false} changed={changed} denied={() => {}}/>); await click('Move Second outcome up');
    const write = fetch.mock.calls.find(([, options]) => options.method === 'PUT')![1]; expect(JSON.parse(write.body as string)).toEqual({ milestone_ids: ['second', 'first'] }); expect(new Headers(write.headers).get('If-Match')).toBe(project.etag); expect(changed).toHaveBeenCalledWith(expect.objectContaining({ version: '2' })); expect(retry).toHaveBeenCalled(); expect(container.textContent).toContain('review it before moving again');
  });
  it('assigns and clears milestone through the authoritative project version', async () => {
    const issue = { id: 'issue', key: 'EXA-1', title: 'First issue', status_name: 'Ready', status_category: 'unstarted', milestone_id: null };
    const fetch = vi.fn((_url: string, options: RequestInit) => Promise.resolve(json(options.method === 'PUT' ? project : page([issue])))); vi.stubGlobal('fetch', fetch);
    const changed = vi.fn(); await render(<ProjectIssues project={project} session={session} refresh={0} readOnly={false} changed={changed} denied={() => {}} milestones={[{ milestone, progress }]}/>); await input('[aria-label="Milestone for EXA-1"]', 'first');
    const [url, write] = fetch.mock.calls.find(([, options]) => options.method === 'PUT')!; expect(url).toContain('/issues/issue/milestone'); expect(new Headers(write.headers).get('If-Match')).toBe(project.etag); expect(JSON.parse(write.body as string)).toEqual({ milestone_id: 'first' }); expect(changed).toHaveBeenCalledWith(project);
  });
  it('reconciles an uncertain milestone creation with an identical receipt', async () => {
    let writes = 0; const fetch = vi.fn((_url: string, options: RequestInit) => options.method === 'POST' ? ++writes === 1 ? Promise.reject(new TypeError('connection lost')) : Promise.resolve(json(milestone)) : Promise.resolve(json(project))); vi.stubGlobal('fetch', fetch);
    await render(<Milestones project={project} session={session} resource={resource([])} readOnly={false} changed={() => {}} denied={() => {}}/>); await click('New milestone'); await input('input[name="milestone-title"]', 'First outcome'); await submit();
    expect(container.querySelector<HTMLInputElement>('input[name="milestone-title"]')?.disabled).toBe(true); await click('Retry safely');
    const posts = fetch.mock.calls.filter(([, options]) => options.method === 'POST'); expect(posts).toHaveLength(2); expect(posts[0][1].body).toBe(posts[1][1].body); expect(new Headers(posts[0][1].headers).get('Idempotency-Key')).toBe(new Headers(posts[1][1].headers).get('Idempotency-Key'));
  });
});
describe('cycle planning screens', () => {
  it('routes list and detail while rejecting nested unknown paths', () => {
    expect(parseRoute('/w/workspace/cycles/cycle?team_id=team')).toMatchObject({ area: 'cycles', cycleId: 'cycle', valid: true }); expect(parseRoute('/w/workspace/cycles/cycle/nope').valid).toBe(false);
  });
  it('shows real empty scope without a percentage and access-filtering context', async () => {
    await render(<ScopeSummary scope={{ ...snapshot, redacted: true }}/>); expect(container.textContent).toContain('No eligible issue scope'); expect(container.textContent).toContain('Hidden issue facts and counts are omitted'); expect(container.querySelector('progress')).toBeNull();
  });
  it('provides exact chart values in a real table with sparse snapshot dates', async () => {
    vi.stubGlobal('fetch', vi.fn(() => Promise.resolve(json(page([{ ...snapshot, id: 'one', unfinished_count: 3 }, { ...snapshot, id: 'two', local_date: '2026-03-30', captured_at: '2026-03-30T12:00:00Z', unfinished_count: 1 }])))));
    await render(<CycleHistory path="/cycles/cycle" refresh={0}/>); expect(container.querySelector('svg')).not.toBeNull(); expect(container.querySelectorAll('tbody tr')).toHaveLength(2); expect(container.querySelector('caption')?.textContent).toContain('exact chart values'); expect(container.textContent).toContain('2026-03-30'); expect(container.textContent).not.toContain('2026-03-29'); expect(container.textContent).toContain('Gaps are not interpolated');
  });
  it('limits scope hydration and exposes a keyboard-operable next page', async () => {
    vi.stubGlobal('fetch', vi.fn(() => Promise.resolve(json({ id: 'issue', key: 'EXA-1', title: 'Example issue' }))));
    await render(<ScopeTable workspace="workspace" team="team" scope={{ ...snapshot, items: Array.from({ length: 30 }, (_, i) => ({ issue_id: String(i), issue_version: '1', category: 'started', excluded: false, estimate: null })) }}/>); expect(container.querySelectorAll('tbody tr')).toHaveLength(25); await click('Show more scope issues'); expect(container.querySelectorAll('tbody tr')).toHaveLength(30);
  });
  it('keeps date boundaries and timezone explicit, and hides writes for read-only viewers', async () => {
    vi.stubGlobal('fetch', vi.fn((url: string) => Promise.resolve(json(url.endsWith('/scope') ? snapshot : url.includes('/snapshots') || url.includes('/issues') ? page([]) : cycle))));
    await render(<Cycles workspaceId="workspace" session={session} data={data} cycleId="cycle" params={new URLSearchParams({ team_id: 'team' })} refresh={0} readOnly/>); expect(container.textContent).toContain('Europe/Berlin'); expect(container.textContent).toContain('exclusive'); expect(container.querySelector('time[datetime="2026-03-30T22:00:00Z"]')).not.toBeNull(); expect([...container.querySelectorAll('button')].some(item => /Edit cycle|Review closure|Add issues|Capture snapshot/.test(item.textContent ?? ''))).toBe(false);
  });
  it('does not fall back to another team when access changes', async () => {
    const fetch = vi.fn(); vi.stubGlobal('fetch', fetch); await render(<Cycles workspaceId="workspace" session={session} data={data} params={new URLSearchParams({ team_id: 'missing' })} refresh={0} readOnly={false}/>); expect(container.textContent).toContain('Team unavailable'); expect(fetch).not.toHaveBeenCalled();
  });
  it('clears confirmed cycle facts when the current issue projection becomes inaccessible', async () => {
    vi.stubGlobal('fetch', vi.fn((url: string) => Promise.resolve(url.includes('/issues') ? json({ code: 'not_found', detail: 'Unavailable' }, 404) : json(url.endsWith('/scope') ? snapshot : url.includes('/snapshots') ? page([]) : cycle))));
    await render(<Cycles workspaceId="workspace" session={session} data={data} cycleId="cycle" params={new URLSearchParams({ team_id: 'team' })} refresh={0} readOnly/>); expect(container.textContent).toContain('Cycle unavailable'); expect(container.textContent).not.toContain(cycle.name); expect(container.querySelector('table')).toBeNull();
  });
  it('requires explicit close review, sends only its token, and never moves issues', async () => {
    const fetch = vi.fn((_url: string, options: RequestInit) => Promise.resolve(json(options.method === 'POST' ? report : { cycle, scope: snapshot, review_token: 'preview-token' }))); vi.stubGlobal('fetch', fetch); const closed = vi.fn();
    await render(<CloseCycle cycle={cycle} session={session} close={() => {}} closed={closed} denied={() => {}}/>); expect(button('Close cycle').disabled).toBe(true); await act(async () => container.querySelector<HTMLInputElement>('.planning-ack input')!.click()); await click('Close cycle');
    const writes = fetch.mock.calls.filter(([, options]) => options.method === 'POST'); expect(writes).toHaveLength(1); expect(writes[0][0]).toMatch(/\/close$/); expect(JSON.parse(writes[0][1].body as string)).toEqual({ review_token: 'preview-token' }); expect(new Headers(writes[0][1].headers).get('If-Match')).toBe(cycle.etag); expect(closed).toHaveBeenCalledWith(report);
  });
  it('requires a new close preview after a concurrent scope change', async () => {
    let reads = 0; const fetch = vi.fn((_url: string, options: RequestInit) => Promise.resolve(options.method === 'POST' ? json({ code: 'version_conflict', detail: 'Changed' }, 412) : json({ cycle, scope: snapshot, review_token: `preview-${++reads}` }))); vi.stubGlobal('fetch', fetch);
    await render(<CloseCycle cycle={cycle} session={session} close={() => {}} closed={() => {}} denied={() => {}}/>); await act(async () => container.querySelector<HTMLInputElement>('.planning-ack input')!.click()); await click('Close cycle'); expect(button('Close cycle').disabled).toBe(true); await click('Reload close preview'); expect(button('Close cycle').disabled).toBe(true); expect(reads).toBe(2);
  });
  it('starts carry-forward with no implicit selection and reports per-item conflicts', async () => {
    const target = { ...cycle, id: 'destination', name: 'Next interval', state: 'planned', start_date: '2026-04-01' }; const fetch = vi.fn((url: string, options: RequestInit) => Promise.resolve(json(options.method === 'POST' ? { id: 'operation', state: 'needs_review', redacted: false, results: [{ issue_id: 'unfinished', outcome: 'issue_changed' }] } : url.includes('/issues/') ? { id: 'unfinished', key: 'EXA-1', title: 'Example issue' } : page([target])))); vi.stubGlobal('fetch', fetch);
    await render(<CarryForward cycle={report.cycle} report={report} session={session} close={() => {}} changed={() => {}} denied={() => {}}/>); expect(button('Move 0 selected issues').disabled).toBe(true); expect(container.querySelectorAll('tbody input[type="checkbox"]')).toHaveLength(1); await input('select', 'destination'); await act(async () => container.querySelector<HTMLInputElement>('tbody input')!.click()); await click('Move 1 selected issues');
    const writes = fetch.mock.calls.filter(([, options]) => options.method === 'POST'); expect(writes).toHaveLength(1); expect(JSON.parse(writes[0][1].body as string)).toEqual({ report_id: 'report', destination_cycle_id: 'destination', destination_etag: cycle.etag, items: [{ issue_id: 'unfinished', issue_version: '3' }] }); expect(container.textContent).toContain('issue changed'); expect(container.textContent).toContain('Some issues need review');
  });
  it('resumes only explicitly reviewed unresolved issues using current versions', async () => {
    const target = { ...cycle, id: 'destination', name: 'Next interval', start_date: '2026-04-01', state: 'planned', etag: '"destination:2"' };
    const operation = { id: 'operation', etag: '"operation:1"', destination_cycle_id: target.id, state: 'needs_review', redacted: false, results: [{ issue_id: 'unfinished', outcome: 'version_conflict' }] };
    const fetch = vi.fn((url: string, _options: RequestInit) => Promise.resolve(json(url.endsWith('/resume') ? { ...operation, state: 'complete', results: [{ issue_id: 'unfinished', outcome: 'moved' }] } : url.includes('/carry-forward') ? operation : url.includes('/issues/') ? { id: 'unfinished', key: 'EXA-1', title: 'Reviewed latest issue', team_id: 'team', version: '8', archived_at: null } : url.endsWith('/destination') ? target : page([target])))); vi.stubGlobal('fetch', fetch);
    await render(<CarryForward cycle={report.cycle} report={report} session={session} close={() => {}} changed={() => {}} denied={() => {}}/>); await input('select', 'destination'); await act(async () => container.querySelector<HTMLInputElement>('tbody input')!.click()); await click('Move 1 selected issues'); await click('Review unresolved issues');
    expect(container.textContent).toContain('Reviewed latest issue'); expect(button('Retry selected unresolved issues').disabled).toBe(true); await act(async () => container.querySelector<HTMLInputElement>('section[aria-label="Review carry-forward retry"] input')!.click()); await click('Retry selected unresolved issues');
    const resume = fetch.mock.calls.find(([url]) => url.endsWith('/resume'))![1]; expect(new Headers(resume.headers).get('If-Match')).toBe(operation.etag); expect(JSON.parse(resume.body as string)).toEqual({ destination_etag: target.etag, items: [{ issue_id: 'unfinished', issue_version: '8' }] }); expect(container.textContent).toContain('Carry-forward complete');
  });
  it('guards navigation before an initial carry response and releases its own guard for the receipt URL', async () => {
    let finish!: (value: Response) => void;
    const target = { ...cycle, id: 'destination', name: 'Next interval', start_date: '2026-04-01', state: 'planned' };
    const operation = { id: 'operation', state: 'needs_review', redacted: false, results: [{ issue_id: 'unfinished', outcome: 'version_conflict' }] };
    const fetch = vi.fn((url: string, options: RequestInit) => options.method === 'POST' ? new Promise<Response>(resolve => { finish = resolve; }) : Promise.resolve(json(url.includes('/issues/') ? { id: 'unfinished', key: 'EXA-1', title: 'Example issue' } : page([target])))); vi.stubGlobal('fetch', fetch);
    const saved = vi.fn(() => navigate('/w/workspace/cycles/cycle?team_id=team&carry=operation', true));
    await render(<CarryForward cycle={report.cycle} report={report} session={session} close={() => {}} changed={() => {}} denied={() => {}} operationSaved={saved}/>); await input('select', 'destination'); await act(async () => container.querySelector<HTMLInputElement>('tbody input')!.click()); await click('Move 1 selected issues');
    expect(hasUnsavedChanges()).toBe(true); vi.mocked(window.confirm).mockReturnValue(false); expect(navigate('/w/workspace/issues/unfinished')).toBe(false); expect(hasUnsavedChanges()).toBe(true);
    await act(async () => finish(json(operation))); expect(saved).toHaveBeenCalledTimes(1); expect(window.confirm).toHaveBeenCalledTimes(1); expect(hasUnsavedChanges()).toBe(false);
  });
  it.each([409, 412])('refreshes and reselects after a confirmed resume %s conflict', async status => {
    let reads = 0; let resumes = 0;
    const target = { ...cycle, id: 'destination', name: 'Next interval', start_date: '2026-04-01', state: 'planned' };
    const operation = { id: 'operation', etag: '"operation:1"', destination_cycle_id: target.id, state: 'needs_review', redacted: false, results: [{ issue_id: 'unfinished', outcome: 'version_conflict' }] };
    const fetch = vi.fn((url: string, _options: RequestInit) => Promise.resolve(url.endsWith('/resume') ? ++resumes === 1 ? json({ code: 'version_conflict', detail: 'Destination or operation changed' }, status) : json({ ...operation, state: 'complete', results: [{ issue_id: 'unfinished', outcome: 'moved' }] }) : json(url.endsWith('/carry-forward/operation') ? { ...operation, etag: `"operation:${++reads}"` } : url.includes('/carry-forward') ? operation : url.endsWith('/destination') ? { ...target, etag: `"destination:${reads + 1}"` } : url.includes('/issues/') ? { id: 'unfinished', key: 'EXA-1', title: 'Reviewed latest issue', team_id: 'team', version: String(reads + 7), archived_at: null } : page([target])))); vi.stubGlobal('fetch', fetch);
    await render(<CarryForward cycle={report.cycle} report={report} session={session} close={() => {}} changed={() => {}} denied={() => {}}/>); await input('select', 'destination'); await act(async () => container.querySelector<HTMLInputElement>('tbody input')!.click()); await click('Move 1 selected issues'); await click('Review unresolved issues'); await act(async () => container.querySelector<HTMLInputElement>('section[aria-label="Review carry-forward retry"] input')!.click()); await click('Retry selected unresolved issues');
    expect(container.querySelector('section[aria-label="Review carry-forward retry"]')).toBeNull(); await click('Review unresolved issues'); expect(button('Retry selected unresolved issues').disabled).toBe(true); await act(async () => container.querySelector<HTMLInputElement>('section[aria-label="Review carry-forward retry"] input')!.click()); await click('Retry selected unresolved issues');
    const writes = fetch.mock.calls.filter(([url]) => url.endsWith('/resume')); expect(writes).toHaveLength(2); expect(new Headers(writes[1][1].headers).get('If-Match')).toBe('"operation:2"'); expect(JSON.parse(writes[1][1].body as string)).toEqual({ destination_etag: '"destination:3"', items: [{ issue_id: 'unfinished', issue_version: '9' }] }); expect(container.textContent).toContain('Carry-forward complete');
  });
  it.each([true, false])('restores a carry permalink as a read-only receipt without writes (workspace read-only %s)', async readOnly => {
    const closed = { ...report.cycle, can_manage: false, can_plan: false };
    const operation = { id: 'operation', report_id: report.id, destination_cycle_id: 'destination', state: 'needs_review', redacted: false, results: [{ issue_id: 'unfinished', outcome: 'version_conflict' }] };
    const fetch = vi.fn((url: string, _options: RequestInit) => Promise.resolve(json(url.includes('/carry-forward/') ? operation : url.includes('/reports/') ? report : url.endsWith('/scope') ? report.scope : url.includes('/snapshots') || url.includes('/issues?') || url.includes('/cycles?') ? page([]) : url.includes('/issues/') ? { id: 'unfinished', key: 'EXA-1', title: 'Read-only issue' } : closed))); vi.stubGlobal('fetch', fetch);
    await render(<Cycles workspaceId="workspace" session={session} data={data} cycleId="cycle" params={new URLSearchParams({ team_id: 'team', carry: 'operation' })} refresh={0} readOnly={readOnly}/>);
    expect(container.textContent).toContain('Read-only receipt'); expect(container.textContent).toContain('version conflict'); expect(container.querySelector('input[type="checkbox"]')).toBeNull(); expect([...container.querySelectorAll('button')].some(item => /Review unresolved|Retry selected|Move .*selected/.test(item.textContent ?? ''))).toBe(false); expect(fetch.mock.calls.some(([, options]) => options.method === 'POST')).toBe(false);
  });
  it('creates a cycle with literal local dates across DST', async () => {
    const fetch = vi.fn(() => Promise.resolve(json(cycle))); vi.stubGlobal('fetch', fetch); const saved = vi.fn(); await render(<CycleEditor workspace="workspace" team="team" session={session} close={() => {}} saved={saved} denied={() => {}}/>);
    await input('input:not([type])', 'Spring interval'); const dates = container.querySelectorAll<HTMLInputElement>('input[type="date"]'); await input(`input[type="date"]:nth-of-type(1)`, '2026-03-28'); await act(async () => { Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(dates[1], '2026-03-31'); dates[1].dispatchEvent(new Event('input', { bubbles: true })); }); const inputs = container.querySelectorAll<HTMLInputElement>('input:not([type])'); await act(async () => { Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(inputs[1], 'Europe/Berlin'); inputs[1].dispatchEvent(new Event('input', { bubbles: true })); }); await submit();
    expect(JSON.parse((fetch.mock.calls as unknown as [string, RequestInit][])[0][1].body as string)).toEqual({ name: cycle.name, start_date: '2026-03-28', end_date_exclusive: '2026-03-31', timezone: cycle.timezone }); expect(saved).toHaveBeenCalledWith(cycle);
  });
});
