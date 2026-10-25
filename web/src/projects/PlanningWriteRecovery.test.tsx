// @vitest-environment jsdom
import { act, useState, type ReactNode } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { Milestones, type Milestone, type MilestoneOverview } from './Milestones';
import { ProjectIssues } from './ProjectIssues';
import { EMPTY_BODY } from '../documents/model';
import { hasUnsavedChanges } from '../router';
import type { Issue, Session } from '../types';
import type { FullProject, Project, ProjectIssue } from './types';

const session: Session = { audience: 'web', user: { id: 'owner', email: 'owner@example.test', display_name: 'Example Owner' }, csrf_token: 'csrf', workspaces: [{ id: 'workspace', name: 'Example Workspace', slug: 'example', role: 'owner' }] };
const project: Project = { id: 'project', workspace_id: 'workspace', team_id: 'team', name: 'Example project', summary: '', state: 'planned', health: 'on_track', lead_id: null, lead: null, start_date: null, target_date: null, version: '1', etag: '"project-metadata:project:1"', created_at: '2026-10-25T10:00:00Z', updated_at: '2026-10-25T10:00:00Z', archived_at: null, can_edit: true, can_manage: true };
const base = '/api/v1/workspaces/workspace/teams/team/projects/project';
const latest: Project = { ...project, version: '2', etag: '"project-metadata:project:2"' };
function full(value = latest): FullProject { return { ...value, narrative: { body: EMPTY_BODY, plain_text: '', schema_supported: true, schema_version: 1 }, narrative_version: '1', metadata_etag: value.etag, etag: `"project:project:${value.version}:document:1"` }; }
function milestone(id: string, title = id, rank = '1'): Milestone { return { id, title, rank, workspace_id: 'workspace', team_id: 'team', project_id: 'project', target_date: null, state: 'planned', completed_at: null, version: '1', etag: `"milestone:${id}:1"`, can_manage: true, can_assign: true, created_at: project.created_at, updated_at: project.updated_at }; }
const progress = { completed: 0, eligible: 0, canceled: 0, open: 0, total: 0, unscoped: true, as_of: project.created_at };
const overview = (ids: string[]): MilestoneOverview[] => ids.map((id, index) => ({ milestone: milestone(id, id, String(index + 1)), progress }));
const initial = overview(['First', 'Second', 'Third']);
const member: ProjectIssue = { id: 'issue', key: 'EX-1', title: 'Example issue', status_id: 'ready', status_name: 'Ready', status_category: 'unstarted', priority: 0, archived_at: null, milestone_id: null };
const otherMembers = (prefix: string) => Array.from({ length: 100 }, (_, index) => ({ ...member, id: `${prefix}-${index}`, key: `OTHER-${prefix}-${index}` }));
const liveIssue: Issue = { id: member.id, key: member.key, title: member.title, workspace_id: 'workspace', team_id: 'team', description: '', body_version: '1', status_id: member.status_id, priority: 0, assignee_id: null, assignee: null, label_ids: [], labels: [], etag: '"issue:issue:1"', estimate: null, due_date: null, version: '1', created_at: project.created_at, updated_at: project.updated_at, archived_at: null };
const json = (value: unknown, status = 200) => new Response(JSON.stringify(value), { status, headers: { 'Content-Type': 'application/json' } });
const page = (data: unknown[], cursor: string | null = null) => ({ data, has_more: !!cursor, next_cursor: cursor });
const milestonePage = (data: MilestoneOverview[], cursor: string | null = null, etag = full().etag) => ({ ...page(data, cursor), project_etag: etag });
const conflict = () => json({ code: 'version_conflict', detail: 'Project changed.' }, 412);
function deferred() { let resolve!: (value: Response) => void; const promise = new Promise<Response>(done => { resolve = done; }); return { promise, resolve }; }
let container: HTMLDivElement; let root: Root;
beforeEach(() => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true);
  HTMLDialogElement.prototype.showModal = function () { this.setAttribute('open', ''); };
  HTMLDialogElement.prototype.close = function () { this.removeAttribute('open'); };
  container = document.createElement('div'); document.body.append(container); root = createRoot(container);
});
afterEach(async () => { await act(async () => root.unmount()); container.remove(); vi.restoreAllMocks(); vi.unstubAllGlobals(); });
async function render(node: ReactNode) { await act(async () => root.render(node)); }
function button(label: string) { const item = [...container.querySelectorAll<HTMLButtonElement>('button')].find(item => item.textContent === label || item.getAttribute('aria-label') === label); if (!item) throw new Error(`Missing button ${label}`); return item; }
function optionalButton(label: string) { return [...container.querySelectorAll<HTMLButtonElement>('button')].find(item => item.textContent === label); }
async function click(label: string) { await act(async () => button(label).click()); }
async function select(value: string) { const element = container.querySelector<HTMLSelectElement>('[aria-label="Milestone for EX-1"]')!; await act(async () => { Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, 'value')!.set!.call(element, value); element.dispatchEvent(new Event('change', { bubbles: true })); }); }
function milestoneScreen(options: { currentSession?: Session; changed?: (value: Project) => void; denied?: (error: unknown) => void } = {}) {
  return <Milestones project={project} session={options.currentSession ?? session} resource={{ items: initial, loading: false, error: null, retry: vi.fn() }} readOnly={false} changed={options.changed ?? vi.fn()} denied={options.denied ?? vi.fn()} />;
}
function assignmentScreen(options: { currentSession?: Session; denied?: (error: unknown) => void } = {}) {
  return <ProjectIssues project={project} session={options.currentSession ?? session} refresh={0} readOnly={false} changed={vi.fn()} denied={options.denied ?? vi.fn()} milestones={[{ milestone: milestone('destination', 'Destination'), progress }]} />;
}
function expectUnavailable(label: string) { expect(optionalButton(label)?.disabled ?? true).toBe(true); }

it('rebases the retained relative milestone move onto a complete current order and preserves new milestones', async () => {
  let puts = 0; const changed = vi.fn();
  const additional = Array.from({ length: 98 }, (_, index) => `Added-${index}`);
  const fetch = vi.fn((url: string, options: RequestInit) => {
    if (options.method === 'PUT') return Promise.resolve(++puts === 1 ? conflict() : json(full({ ...latest, version: '3', etag: '"project-metadata:project:3"' })));
    if (url === base) return Promise.resolve(json(full()));
    if (url === `${base}/metadata`) return Promise.resolve(json(latest));
    const cursor = new URL(url, 'https://example.test').searchParams.get('cursor');
    return Promise.resolve(json(cursor ? milestonePage(overview(['Second', 'Third'])) : milestonePage(overview(['First', 'New', ...additional]), 'next-order')));
  }); vi.stubGlobal('fetch', fetch);
  await render(milestoneScreen({ changed })); await click('Move Second up');
  expect(container.querySelector('dialog')?.textContent).toContain('Review milestone move'); expect(puts).toBe(1);
  expect(button('Apply reviewed move').disabled).toBe(false); expect(hasUnsavedChanges()).toBe(true);
  await click('Apply reviewed move'); expect(puts).toBe(2);
  const writes = fetch.mock.calls.filter(([, options]) => options.method === 'PUT');
  expect(JSON.parse(writes[1][1].body as string)).toEqual({ milestone_ids: ['Second', 'First', 'New', ...additional, 'Third'] });
  expect([latest.etag, full().etag]).toContain(new Headers(writes[1][1].headers).get('If-Match'));
  expect(changed).toHaveBeenCalledWith(expect.objectContaining({ version: '3' }));
  expect(fetch.mock.calls.filter(([url]) => url.includes('/milestones?')).map(([url]) => new URL(url, 'https://example.test').searchParams.get('limit'))).toEqual(['100', '100']);
});

it('observes an interrupted move already at its desired position without replaying or claiming acknowledgement', async () => {
  const fetch = vi.fn((url: string, options: RequestInit) => options.method === 'PUT' ? Promise.reject(new TypeError('Response lost')) : Promise.resolve(json(url === base ? full() : url.endsWith('/metadata') ? latest : milestonePage(overview(['Second', 'First', 'Third'])))));
  vi.stubGlobal('fetch', fetch); await render(milestoneScreen()); await click('Move Second up');
  expect(button('Done').disabled).toBe(false); expectUnavailable('Apply reviewed move');
  expect(fetch.mock.calls.filter(([, options]) => options.method === 'PUT')).toHaveLength(1);
  expect(container.querySelector('dialog')?.textContent).not.toMatch(/original (request|write).*(succeeded|confirmed)|acknowledged/i);
  await click('Done'); expect(hasUnsavedChanges()).toBe(false);
});

it.each(['metadata tag used as full tag', 'missing cursor', 'repeated cursor', 'more than 500 milestones'] as const)('keeps a milestone recovery unresolved for %s', async failure => {
  let pages = 0;
  const fetch = vi.fn((url: string, options: RequestInit) => {
    if (options.method === 'PUT') return Promise.resolve(conflict());
    if (url === base) return Promise.resolve(json(full()));
    if (url.endsWith('/metadata')) return Promise.resolve(json(latest));
    pages += 1;
    if (failure === 'metadata tag used as full tag') return Promise.resolve(json(milestonePage(initial, null, latest.etag)));
    if (failure === 'missing cursor') return Promise.resolve(json({ ...milestonePage(initial), has_more: true }));
    if (failure === 'repeated cursor') return Promise.resolve(json(milestonePage(pages === 1 ? initial : overview(['Fourth']), 'same')));
    return Promise.resolve(json(milestonePage(Array.from({ length: 100 }, (_, index) => ({ milestone: milestone(`entry-${pages}-${index}`), progress })), `page-${pages}`)));
  }); vi.stubGlobal('fetch', fetch); await render(milestoneScreen()); await click('Move Second up');
  expect(container.querySelector('dialog')?.textContent).toContain('Review milestone move'); expectUnavailable('Apply reviewed move');
  expect(fetch.mock.calls.filter(([, options]) => options.method === 'PUT')).toHaveLength(1); expect(pages).toBeLessThanOrEqual(5);
  expect(button('Reload current order').disabled).toBe(false); expect(hasUnsavedChanges()).toBe(true);
});

it('cannot apply a retained milestone move after its anchor disappears or a later order page fails', async () => {
  let failPage = false;
  vi.stubGlobal('fetch', vi.fn((url: string, options: RequestInit) => {
    if (options.method === 'PUT') return Promise.resolve(conflict());
    if (url === base) return Promise.resolve(json(full()));
    if (url.endsWith('/metadata')) return Promise.resolve(json(latest));
    if (!failPage) return Promise.resolve(json(milestonePage(overview(['Second', 'Third']))));
    return Promise.resolve(url.includes('cursor=') ? json({ code: 'unavailable', detail: 'Order page unavailable' }, 503) : json(milestonePage(initial, 'next')));
  }));
  await render(milestoneScreen()); await click('Move Second up'); expectUnavailable('Apply reviewed move');
  failPage = true; await click('Reload current order'); expectUnavailable('Apply reviewed move'); expect(container.textContent).toContain('Order page unavailable');
});

it('discards stale milestone recovery responses after the account session changes', async () => {
  const pending = deferred(); const fetch = vi.fn((url: string, options: RequestInit) => options.method === 'PUT' ? Promise.resolve(conflict()) : url === base ? pending.promise : Promise.resolve(json(url.endsWith('/metadata') ? latest : milestonePage(initial))));
  vi.stubGlobal('fetch', fetch); await render(milestoneScreen()); await click('Move Second up');
  expect(hasUnsavedChanges()).toBe(true); await render(milestoneScreen({ currentSession: { ...session, csrf_token: 'next-session' } }));
  await act(async () => pending.resolve(json(full())));
  expect(container.querySelector('dialog')).toBeNull(); expect(hasUnsavedChanges()).toBe(false);
  expect(fetch.mock.calls.filter(([, options]) => options.method === 'PUT')).toHaveLength(1);
});

it('finds a selected issue on a later page before offering an explicitly reviewed removal', async () => {
  let puts = 0;
  const fetch = vi.fn((url: string, options: RequestInit) => {
    if (options.method === 'PUT') return Promise.resolve(++puts === 1 ? conflict() : json(full({ ...latest, version: '3', etag: '"project-metadata:project:3"' })));
    if (url === `${base}/metadata`) return Promise.resolve(json(latest));
    if (url.endsWith('/workspaces/workspace/issues/issue')) return Promise.resolve(json(liveIssue));
    const query = new URL(url, 'https://example.test').searchParams;
    if (query.get('limit') === '50') return Promise.resolve(json(page([member])));
    return Promise.resolve(json(query.has('cursor') ? page([member]) : page(otherMembers('first'), 'later')));
  }); vi.stubGlobal('fetch', fetch); await render(assignmentScreen()); await click('Remove EX-1 from project');
  expect(puts).toBe(1); expect(button('Apply reviewed assignment').disabled).toBe(false); expect(hasUnsavedChanges()).toBe(true);
  const scans = fetch.mock.calls.filter(([url]) => url.includes('/issues?') && new URL(url, 'https://example.test').searchParams.get('limit') === '100');
  expect(scans).toHaveLength(2); expect(new URL(scans[1][0], 'https://example.test').searchParams.get('cursor')).toBe('later');
  await click('Apply reviewed assignment'); expect(puts).toBe(2);
  const write = fetch.mock.calls.filter(([, options]) => options.method === 'PUT')[1];
  expect(JSON.parse(write[1].body as string)).toEqual({ included: false }); expect(new Headers(write[1].headers).get('If-Match')).toBe(latest.etag);
});

it('observes an absent membership only after terminal pagination and stable live issue and project reads', async () => {
  const fetch = vi.fn((url: string, options: RequestInit) => {
    if (options.method === 'PUT') return Promise.reject(new TypeError('Response lost'));
    if (url === `${base}/metadata`) return Promise.resolve(json(latest));
    if (url.endsWith('/workspaces/workspace/issues/issue')) return Promise.resolve(json(liveIssue));
    const query = new URL(url, 'https://example.test').searchParams;
    return Promise.resolve(json(query.get('limit') === '50' ? page([member]) : query.has('cursor') ? page([]) : page(otherMembers('first'), 'terminal')));
  }); vi.stubGlobal('fetch', fetch); await render(assignmentScreen()); await click('Remove EX-1 from project');
  expect(button('Done').disabled).toBe(false); expectUnavailable('Apply reviewed assignment');
  expect(fetch.mock.calls.filter(([, options]) => options.method === 'PUT')).toHaveLength(1);
  expect(fetch.mock.calls.filter(([url]) => url === `${base}/metadata`)).toHaveLength(2);
  expect(fetch.mock.calls.filter(([url]) => url.endsWith('/workspaces/workspace/issues/issue'))).toHaveLength(2);
});

it.each(['missing cursor', 'repeated cursor', 'duplicate issue', 'failed later page', 'ten-page budget'] as const)('never treats membership as absent after %s', async failure => {
  let scans = 0;
  const fetch = vi.fn((url: string, options: RequestInit) => {
    if (options.method === 'PUT') return Promise.reject(new TypeError('Response lost'));
    if (url === `${base}/metadata`) return Promise.resolve(json(latest));
    if (url.endsWith('/workspaces/workspace/issues/issue')) return Promise.resolve(json(liveIssue));
    const query = new URL(url, 'https://example.test').searchParams;
    if (query.get('limit') === '50') return Promise.resolve(json(page([member])));
    scans += 1;
    if (failure === 'missing cursor') return Promise.resolve(json({ ...page([]), has_more: true }));
    if (failure === 'failed later page' && scans > 1) return Promise.resolve(json({ code: 'unavailable', detail: 'Scope page unavailable' }, 503));
    const rows = failure === 'duplicate issue' ? scans === 1 ? otherMembers('duplicate') : [otherMembers('duplicate')[0]] : otherMembers(`page-${scans}`);
    return Promise.resolve(json(page(rows, failure === 'repeated cursor' ? 'same' : failure === 'duplicate issue' && scans === 2 ? null : `page-${scans}`)));
  }); vi.stubGlobal('fetch', fetch); await render(assignmentScreen()); await click('Remove EX-1 from project');
  expectUnavailable('Apply reviewed assignment'); expect(optionalButton('Done')).toBeUndefined();
  expect(button('Reload current assignment').disabled).toBe(false); expect(scans).toBeLessThanOrEqual(10); expect(hasUnsavedChanges()).toBe(true);
  expect(fetch.mock.calls.filter(([, options]) => options.method === 'PUT')).toHaveLength(1);
});

it.each(['project changes', 'issue changes', 'issue archived', 'issue moved'] as const)('keeps membership absence unresolved when %s during reconciliation', async failure => {
  let metadataReads = 0; let issueReads = 0;
  const fetch = vi.fn((url: string, options: RequestInit) => {
    if (options.method === 'PUT') return Promise.reject(new TypeError('Response lost'));
    if (url === `${base}/metadata`) return Promise.resolve(json(++metadataReads > 1 && failure === 'project changes' ? { ...latest, version: '3', etag: '"project-metadata:project:3"' } : latest));
    if (url.endsWith('/workspaces/workspace/issues/issue')) {
      issueReads += 1; return Promise.resolve(json({ ...liveIssue, ...(failure === 'issue changes' && issueReads > 1 ? { version: '2', etag: '"issue:issue:2"' } : failure === 'issue archived' ? { archived_at: project.created_at } : failure === 'issue moved' ? { team_id: 'other-team' } : {}) }));
    }
    return Promise.resolve(json(page(new URL(url, 'https://example.test').searchParams.get('limit') === '50' ? [member] : [])));
  }); vi.stubGlobal('fetch', fetch); await render(assignmentScreen()); await click('Remove EX-1 from project');
  expectUnavailable('Apply reviewed assignment'); expect(optionalButton('Done')).toBeUndefined();
  expect(fetch.mock.calls.filter(([, options]) => options.method === 'PUT')).toHaveLength(1);
});

it('rechecks the chosen milestone scope and assignment permission before enabling reviewed assignment', async () => {
  let valid = false;
  const fetch = vi.fn((url: string, options: RequestInit) => {
    if (options.method === 'PUT') return Promise.resolve(conflict());
    if (url === `${base}/metadata`) return Promise.resolve(json(latest));
    if (url.endsWith('/workspaces/workspace/issues/issue')) return Promise.resolve(json(liveIssue));
    if (url.endsWith('/milestones/destination')) return Promise.resolve(json({ ...milestone('destination'), project_id: valid ? 'project' : 'different-project', can_assign: valid }));
    return Promise.resolve(json(page([member])));
  }); vi.stubGlobal('fetch', fetch); await render(assignmentScreen()); await select('destination');
  expectUnavailable('Apply reviewed assignment'); valid = true; await click('Reload current assignment');
  expect(button('Apply reviewed assignment').disabled).toBe(false);
  expect(fetch.mock.calls.filter(([, options]) => options.method === 'PUT')).toHaveLength(1);
});

it('shows the authorized current milestone title and rejects a current milestone from another project', async () => {
  let valid = false;
  const fetch = vi.fn((url: string, options: RequestInit) => {
    if (options.method === 'PUT') return Promise.resolve(conflict());
    if (url === `${base}/metadata`) return Promise.resolve(json(latest));
    if (url.endsWith('/workspaces/workspace/issues/issue')) return Promise.resolve(json(liveIssue));
    if (url.endsWith('/milestones/destination')) return Promise.resolve(json(milestone('destination', 'Authoritative desired outcome')));
    if (url.endsWith('/milestones/current')) return Promise.resolve(json({ ...milestone('current', valid ? 'Authoritative saved outcome' : 'Foreign project outcome'), project_id: valid ? 'project' : 'other-project' }));
    const recovery = new URL(url, 'https://example.test').searchParams.get('limit') === '100';
    return Promise.resolve(json(page([{ ...member, milestone_id: recovery ? 'current' : null }])));
  }); vi.stubGlobal('fetch', fetch); await render(assignmentScreen()); await select('destination');
  expectUnavailable('Apply reviewed assignment'); expect(container.querySelector('dialog')?.textContent).not.toContain('Foreign project outcome');
  valid = true; await click('Reload current assignment');
  expect(container.querySelector('dialog')?.textContent).toContain('Current milestone: Authoritative saved outcome');
  expect(container.querySelector('dialog')?.textContent).toContain('Authoritative desired outcome'); expect(button('Apply reviewed assignment').disabled).toBe(false);
  expect(fetch.mock.calls.filter(([url]) => url.endsWith('/milestones/current'))).toHaveLength(2);
  expect(fetch.mock.calls.filter(([, options]) => options.method === 'PUT')).toHaveLength(1);
});

it.each(['milestone', 'assignment'] as const)('conceals a denied %s recovery without retaining its intent labels or allowing another write', async kind => {
  const denied = vi.fn();
  const fetch = vi.fn((url: string, options: RequestInit) => {
    if (options.method === 'PUT') return Promise.resolve(conflict());
    if (url.includes('/issues?')) return Promise.resolve(json(page([member])));
    return Promise.resolve(json({ code: 'not_found', detail: 'Unavailable' }, 404));
  }); vi.stubGlobal('fetch', fetch);
  await render(kind === 'milestone' ? milestoneScreen({ denied }) : assignmentScreen({ denied }));
  await click(kind === 'milestone' ? 'Move Second up' : 'Remove EX-1 from project');
  expect(denied).toHaveBeenCalled(); expect(container.querySelector('dialog')).toBeNull();
  expect(container.textContent).not.toContain(kind === 'milestone' ? 'Second' : 'Example issue');
  expectUnavailable(kind === 'milestone' ? 'Apply reviewed move' : 'Apply reviewed assignment');
  expect(fetch.mock.calls.filter(([, options]) => options.method === 'PUT')).toHaveLength(1);
});

it('ignores an old assignment scan after a session change even when transport returns its result', async () => {
  const pending = deferred();
  const fetch = vi.fn((url: string, options: RequestInit) => {
    if (options.method === 'PUT') return Promise.resolve(conflict());
    if (url === `${base}/metadata`) return Promise.resolve(json(latest));
    if (url.endsWith('/workspaces/workspace/issues/issue')) return Promise.resolve(json(liveIssue));
    return new URL(url, 'https://example.test').searchParams.get('limit') === '100' ? pending.promise : Promise.resolve(json(page([member])));
  }); vi.stubGlobal('fetch', fetch); await render(assignmentScreen()); await click('Remove EX-1 from project');
  expect(hasUnsavedChanges()).toBe(true); await render(assignmentScreen({ currentSession: { ...session, csrf_token: 'next-session' } }));
  await act(async () => pending.resolve(json(page([]))));
  expect(container.querySelector('dialog')).toBeNull(); expect(hasUnsavedChanges()).toBe(false);
  expect(fetch.mock.calls.filter(([, options]) => options.method === 'PUT')).toHaveLength(1);
});

it('restores recovery focus to the moved milestone after applying a reviewed order disables the original move button', async () => {
  let puts = 0; let persistedOrder = ['First', 'Second', 'Third'];
  vi.stubGlobal('fetch', vi.fn((url: string, options: RequestInit) => {
    if (options.method === 'PUT') {
      if (++puts === 1) return Promise.resolve(conflict());
      persistedOrder = JSON.parse(options.body as string).milestone_ids;
      return Promise.resolve(json(full({ ...latest, version: '3', etag: '"project-metadata:project:3"' })));
    }
    return Promise.resolve(json(url === base ? full() : url.endsWith('/metadata') ? latest : milestonePage(overview(persistedOrder))));
  }));
  function Parent() {
    const [current, setCurrent] = useState(project); const [items, setItems] = useState(initial);
    return <Milestones project={current} session={session} resource={{ items, loading: false, error: null, retry: () => setItems(overview(persistedOrder)) }} readOnly={false} changed={setCurrent} denied={vi.fn()} />;
  }
  await render(<Parent />); const origin = button('Move Second up'); origin.focus(); await click('Move Second up');
  const apply = button('Apply reviewed move'); apply.focus(); await click('Apply reviewed move');
  expect(persistedOrder).toEqual(['Second', 'First', 'Third']); expect(origin.disabled).toBe(true); expect(container.querySelector('dialog')).toBeNull();
  expect([button('Edit Second'), button('Move Second down')]).toContain(document.activeElement);
});

it('restores recovery focus to the project issues section after observed removal closes with its origin row gone', async () => {
  let removed = false;
  vi.stubGlobal('fetch', vi.fn((url: string, options: RequestInit) => {
    if (options.method === 'PUT') { removed = true; return Promise.reject(new TypeError('Committed removal response lost')); }
    if (url === `${base}/metadata`) return Promise.resolve(json(latest));
    if (url.endsWith('/workspaces/workspace/issues/issue')) return Promise.resolve(json(liveIssue));
    return Promise.resolve(json(page(removed ? [] : [member])));
  }));
  function Parent() {
    const [current, setCurrent] = useState(project); const [refresh, setRefresh] = useState(0);
    return <ProjectIssues project={current} session={session} refresh={refresh} readOnly={false} changed={value => { setCurrent(value); setRefresh(value => value + 1); }} denied={vi.fn()} />;
  }
  await render(<Parent />); button('Remove EX-1 from project').focus(); await click('Remove EX-1 from project');
  expect(container.querySelector('[aria-label="Remove EX-1 from project"]')).toBeNull();
  const done = button('Done'); done.focus(); await click('Done'); expect(container.querySelector('dialog')).toBeNull();
  const section = container.querySelector('section[aria-label="Project issues"]')!;
  expect([section, section.querySelector('h3')]).toContain(document.activeElement);
});

it.each([['truthy nonstring', 42], ['empty string', '']] as const)('review correction: rejects a terminal project issue with a %s id before confirming absence', async (_label, id) => {
  const fetch = vi.fn((url: string, options: RequestInit) => {
    if (options.method === 'PUT') return Promise.reject(new TypeError('Response lost'));
    if (url === `${base}/metadata`) return Promise.resolve(json(latest));
    if (url.endsWith('/workspaces/workspace/issues/issue')) return Promise.resolve(json(liveIssue));
    const recovery = new URL(url, 'https://example.test').searchParams.get('limit') === '100';
    return Promise.resolve(json(page(recovery ? [{ ...member, id }] : [member])));
  }); vi.stubGlobal('fetch', fetch); await render(assignmentScreen()); await click('Remove EX-1 from project');
  expect(container.querySelector('dialog')?.textContent).toContain('Review project assignment');
  expectUnavailable('Apply reviewed assignment'); expect(optionalButton('Done')).toBeUndefined(); expect(hasUnsavedChanges()).toBe(true);
  expect(fetch.mock.calls.filter(([, options]) => options.method === 'PUT')).toHaveLength(1);
});

it.each(['missing', 'number', 'empty string'] as const)('review correction: does not confirm milestone clearing from a %s milestone_id', async malformed => {
  const { milestone_id: _milestone, ...withoutMilestone } = member;
  const row = malformed === 'missing' ? withoutMilestone : { ...member, milestone_id: malformed === 'number' ? 0 : '' };
  const fetch = vi.fn((url: string, options: RequestInit) => {
    if (options.method === 'PUT') return Promise.reject(new TypeError('Response lost'));
    if (url === `${base}/metadata`) return Promise.resolve(json(latest));
    if (url.endsWith('/workspaces/workspace/issues/issue')) return Promise.resolve(json(liveIssue));
    const recovery = new URL(url, 'https://example.test').searchParams.get('limit') === '100';
    return Promise.resolve(json(page(recovery ? [row] : [{ ...member, milestone_id: 'destination' }])));
  }); vi.stubGlobal('fetch', fetch); await render(assignmentScreen()); await select('');
  expectUnavailable('Apply reviewed assignment'); expect(optionalButton('Done')).toBeUndefined(); expect(hasUnsavedChanges()).toBe(true);
  expect(JSON.parse(fetch.mock.calls.find(([, options]) => options.method === 'PUT')![1].body as string)).toEqual({ milestone_id: null });
  expect(fetch.mock.calls.filter(([, options]) => options.method === 'PUT')).toHaveLength(1);
});

it('review correction: retains observed milestone clearing for an explicit null milestone_id', async () => {
  const fetch = vi.fn((url: string, options: RequestInit) => {
    if (options.method === 'PUT') return Promise.reject(new TypeError('Response lost'));
    if (url === `${base}/metadata`) return Promise.resolve(json(latest));
    if (url.endsWith('/workspaces/workspace/issues/issue')) return Promise.resolve(json(liveIssue));
    const recovery = new URL(url, 'https://example.test').searchParams.get('limit') === '100';
    return Promise.resolve(json(page([{ ...member, milestone_id: recovery ? null : 'destination' }])));
  }); vi.stubGlobal('fetch', fetch); await render(assignmentScreen()); await select('');
  expect(button('Done').disabled).toBe(false); expectUnavailable('Apply reviewed assignment');
  expect(fetch.mock.calls.filter(([, options]) => options.method === 'PUT')).toHaveLength(1);
});

it.each([403, 404])('review correction: conceals assignment labels when a selected milestone 404 is followed by project %s', async status => {
  let metadataReads = 0; const denied = vi.fn();
  const fetch = vi.fn((url: string, options: RequestInit) => {
    if (options.method === 'PUT') return Promise.resolve(conflict());
    if (url === `${base}/metadata`) return Promise.resolve(++metadataReads === 1 ? json(latest) : json({ code: status === 403 ? 'forbidden' : 'not_found', detail: 'Project unavailable' }, status));
    if (url.endsWith('/workspaces/workspace/issues/issue')) return Promise.resolve(json(liveIssue));
    if (url.endsWith('/milestones/destination')) return Promise.resolve(json({ code: 'not_found', detail: 'Milestone unavailable' }, 404));
    return Promise.resolve(json(page([member])));
  }); vi.stubGlobal('fetch', fetch); await render(assignmentScreen({ denied })); await select('destination');
  expect(denied).toHaveBeenCalledOnce(); expect(metadataReads).toBe(2); expect(container.querySelector('dialog')).toBeNull();
  expect(container.textContent).not.toContain(member.title); expect(container.textContent).not.toContain(member.key); expect(container.textContent).not.toContain('Destination');
  expectUnavailable('Apply reviewed assignment'); expect(hasUnsavedChanges()).toBe(false);
  expect(fetch.mock.calls.filter(([, options]) => options.method === 'PUT')).toHaveLength(1);
});

it('review correction: preserves unresolved selection after a missing milestone when fresh project access still matches', async () => {
  let metadataReads = 0; const denied = vi.fn();
  const fetch = vi.fn((url: string, options: RequestInit) => {
    if (options.method === 'PUT') return Promise.resolve(conflict());
    if (url === `${base}/metadata`) { metadataReads += 1; return Promise.resolve(json(latest)); }
    if (url.endsWith('/workspaces/workspace/issues/issue')) return Promise.resolve(json(liveIssue));
    if (url.endsWith('/milestones/destination')) return Promise.resolve(json({ code: 'not_found', detail: 'Milestone unavailable' }, 404));
    return Promise.resolve(json(page([member])));
  }); vi.stubGlobal('fetch', fetch); await render(assignmentScreen({ denied })); await select('destination');
  expect(metadataReads).toBe(2); expect(denied).not.toHaveBeenCalled(); expect(container.querySelector('dialog')?.textContent).toContain(member.key);
  expectUnavailable('Apply reviewed assignment'); expect(optionalButton('Done')).toBeUndefined(); expect(hasUnsavedChanges()).toBe(true);
  expect(fetch.mock.calls.filter(([, options]) => options.method === 'PUT')).toHaveLength(1);
});

it('review correction: restores discard focus by surviving milestone identity after a missing anchor and deferred refresh', async () => {
  const fetch = vi.fn((url: string, options: RequestInit) => Promise.resolve(options.method === 'PUT' ? conflict() : json(url === base ? full() : url.endsWith('/metadata') ? latest : milestonePage(overview(['Second', 'Third'])))));
  vi.stubGlobal('fetch', fetch); let finishRefresh!: () => void;
  function Parent() {
    const [current, setCurrent] = useState(project); const [items, setItems] = useState(initial); const [loading, setLoading] = useState(false);
    function refresh() { setLoading(true); finishRefresh = () => { setItems(overview(['Second', 'Third'])); setLoading(false); }; }
    return <Milestones project={current} session={session} resource={{ items, loading, error: null, retry: refresh }} readOnly={false} changed={setCurrent} denied={vi.fn()} />;
  }
  await render(<Parent />); const origin = button('Move Second up'); origin.focus(); await click('Move Second up');
  expectUnavailable('Apply reviewed move'); expect(button('Edit First').isConnected).toBe(true);
  const discard = button('Discard move'); discard.focus(); await click('Discard move'); expect(container.querySelector('dialog')).toBeNull();
  await act(async () => finishRefresh()); expect(container.querySelector('[aria-label="Edit First"]')).toBeNull(); expect(origin.disabled).toBe(true);
  const section = container.querySelector('section[aria-label="Project milestones"]')!;
  expect([button('Edit Second'), button('Move Second down'), section, section.querySelector('h3')]).toContain(document.activeElement);
  expect(fetch.mock.calls.filter(([, options]) => options.method === 'PUT')).toHaveLength(1);
});
