// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { CommandPalette } from './CommandPalette';
import { usePaletteEntities } from './usePaletteEntities';
import type { PaletteIssueSource } from './usePaletteIssues';
import { useUnsavedChanges } from '../router';

let container: HTMLDivElement; let root: Root;
let entities: ReturnType<typeof usePaletteEntities>;
beforeEach(() => {
  vi.useFakeTimers(); vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true);
  HTMLDialogElement.prototype.showModal = function () { this.setAttribute('open', ''); };
  HTMLDialogElement.prototype.close = function () { this.removeAttribute('open'); };
  history.replaceState(null, '', '/w/workspace/issues?team_id=team');
  container = document.createElement('div'); document.body.append(container); root = createRoot(container);
});
afterEach(async () => {
  await act(async () => root.unmount()); container.remove();
  vi.useRealTimers(); vi.unstubAllGlobals(); vi.restoreAllMocks();
});
const json = (value: unknown, status = 200) => new Response(JSON.stringify(value), { status, headers: { 'Content-Type': 'application/json' } });
const team = (id: string, name = id, workspace_id = 'workspace') => ({ id, name, workspace_id, key: `EX-${id}`, archived_at: null });
const project = (id: string, name = id, workspace_id = 'workspace', team_id = 'team') => ({ id, name, workspace_id, team_id, archived_at: null });
const issue = (id: string) => ({ id, title: `Match issue ${id}`, workspace_id: 'workspace', key: `EX-${id}`, archived_at: null });
const page = (...data: unknown[]) => ({ data, has_more: false, next_cursor: null });
const source = (overrides: Partial<PaletteIssueSource> = {}): PaletteIssueSource => ({ workspaceId: 'workspace', scope: 'account:workspace:epoch', recentIds: [], open: vi.fn(), ...overrides });
function deferred() { let resolve!: (response: Response) => void; const promise = new Promise<Response>(done => { resolve = done; }); return { promise, resolve }; }
function Harness({ value, query, teamId }: { value?: PaletteIssueSource; query: string; teamId?: string }) {
  entities = usePaletteEntities(value, query, teamId);
  return <>{(['teams', 'projects'] as const).map(kind => <section key={kind} aria-label={kind} aria-busy={entities[kind].busy}>
    {entities[kind].items.map(item => <p key={item.id}>{item.name}</p>)}
    {entities[kind].error instanceof Error && <span role="alert">{entities[kind].error.message}</span>}
  </section>)}</>;
}
async function render(query = 'Match', value: PaletteIssueSource | undefined = source(), teamId: string | undefined = 'team') {
  await act(async () => root.render(<Harness value={value} query={query} teamId={teamId} />));
}
async function tick(ms = 150) { await act(async () => { await vi.advanceTimersByTimeAsync(ms); }); }
async function typeQuery(value: string) {
  const input = container.querySelector('input')!;
  await act(async () => { Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(input, value); input.dispatchEvent(new Event('input', { bubbles: true })); });
}
async function key(key: string, options: KeyboardEventInit = {}) {
  await act(async () => { container.querySelector('input')!.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true, ...options })); });
}

it('debounces exact normalized queries for 150 ms and bounds both directory requests', async () => {
  const fetch = vi.fn().mockImplementation(() => Promise.resolve(json(page()))); vi.stubGlobal('fetch', fetch);
  const query = '  Match & %_' + '\u{1f680}'.repeat(100) + '  ';
  await render(query); await tick(149); expect(fetch).not.toHaveBeenCalled(); await tick(1);
  expect(fetch).toHaveBeenCalledTimes(2);
  const urls = fetch.mock.calls.map(([url]) => new URL(String(url), 'https://example.test'));
  expect(urls.map(url => url.pathname)).toEqual(['/api/v1/workspaces/workspace/teams', '/api/v1/workspaces/workspace/teams/team/projects']);
  for (const url of urls) { expect(url.searchParams.get('q')).toBe(query.trim()); expect(url.searchParams.get('limit')).toBe('20'); expect(url.searchParams.has('cursor')).toBe(false); }
  expect(urls[0].searchParams.get('state')).toBe('active'); expect(urls[1].searchParams.get('archived')).toBe('false');
});

it('searches teams without a selected team and never fans out project requests across returned teams', async () => {
  const fetch = vi.fn().mockImplementation(() => Promise.resolve(json(page(team('first'), team('second'))))); vi.stubGlobal('fetch', fetch);
  await act(async () => root.render(<Harness value={source()} query="Match" />)); await tick();
  expect(fetch).toHaveBeenCalledTimes(1); expect(String(fetch.mock.calls[0][0])).toContain('/teams?');
  expect(entities.teams.items.map(item => item.id)).toEqual(['first', 'second']); expect(entities.projects.items).toEqual([]);
  await tick(500); expect(fetch).toHaveBeenCalledTimes(1);
  await act(async () => root.render(<Harness value={undefined} query="Match" teamId="team" />));
  expect(container.textContent).toBe(''); await tick(); expect(fetch).toHaveBeenCalledTimes(1);
});

it.each(['query', 'team', 'workspace', 'authority'] as const)('clears rows immediately on %s changes and ignores responses from the old context', async change => {
  const pending: ReturnType<typeof deferred>[] = [];
  let slow = false;
  const fetch = vi.fn((url: string) => {
    if (slow) { const response = deferred(); pending.push(response); return response.promise; }
    return Promise.resolve(json(page(url.includes('/projects?') ? project('old', 'Previous project') : team('old', 'Previous team'))));
  }); vi.stubGlobal('fetch', fetch);
  const original = source(); await render('Match', original); await tick();
  expect(container.textContent).toContain('Previous team'); expect(container.textContent).toContain('Previous project');
  slow = true; await render('Pending', original); expect(container.textContent).toBe(''); await tick();
  expect(pending).toHaveLength(2);
  const next = change === 'workspace' ? source({ workspaceId: 'next', scope: 'account:next:epoch' }) : change === 'authority' ? source({ scope: 'account:workspace:new-epoch' }) : original;
  await render(change === 'query' ? 'Newest' : 'Pending', next, change === 'team' ? 'next-team' : 'team');
  expect(container.textContent).toBe('');
  for (const [, options] of fetch.mock.calls.slice(2) as unknown as [string, RequestInit][]) expect(options.signal?.aborted).toBe(true);
  await act(async () => { pending[0].resolve(json(page(team('stale', 'Forbidden old team')))); pending[1].resolve(json(page(project('stale', 'Forbidden old project')))); });
  expect(container.textContent).toBe(''); await tick(); expect(pending).toHaveLength(4);
  await act(async () => {
    pending[2].resolve(json(page(team('latest', 'Current team', next.workspaceId))));
    pending[3].resolve(json(page(project('latest', 'Current project', next.workspaceId, change === 'team' ? 'next-team' : 'team'))));
  });
  expect(container.textContent).toContain('Current team'); expect(container.textContent).toContain('Current project'); expect(container.textContent).not.toContain('Forbidden');
});

it('filters archived and foreign list rows while keeping read-only accessible rows', async () => {
  vi.stubGlobal('fetch', vi.fn((url: string) => Promise.resolve(json(url.includes('/projects?')
    ? page(project('visible'), { ...project('archived'), archived_at: '2026-10-25T00:00:00Z' }, project('foreign', 'Foreign project', 'elsewhere'), project('wrong-team', 'Wrong team', 'workspace', 'other-team'))
    : page({ ...team('visible'), can_manage: false }, { ...team('archived'), archived_at: '2026-10-25T00:00:00Z' }, team('foreign', 'Foreign team', 'elsewhere'))))));
  await render(); await tick();
  expect(entities.teams.items.map(item => item.id)).toEqual(['visible']); expect(entities.projects.items.map(item => item.id)).toEqual(['visible']);
});

it.each([
  ['team', 'denied'], ['team', 'archived'], ['team', 'id'], ['team', 'workspace'],
  ['project', 'denied'], ['project', 'archived'], ['project', 'id'], ['project', 'workspace'], ['project', 'team'],
] as const)('rejects a selected %s whose current detail is %s and removes its suggestion', async (kind, failure) => {
  const item = kind === 'team' ? team('selected', 'Selected team') : project('selected', 'Selected project');
  const fetch = vi.fn((url: string) => {
    if (url.includes('?')) return Promise.resolve(json(page(url.includes('/projects?') ? project('selected', 'Selected project') : team('selected', 'Selected team'))));
    if (failure === 'denied') return Promise.resolve(json({ code: 'not_found', detail: 'Current access denied' }, 404));
    return Promise.resolve(json({ ...item, ...(failure === 'archived' ? { archived_at: '2026-10-25T00:00:00Z' } : failure === 'id' ? { id: 'different' } : failure === 'workspace' ? { workspace_id: 'different' } : { team_id: 'different' }) }));
  }); vi.stubGlobal('fetch', fetch); await render(); await tick();
  const group = entities[kind === 'team' ? 'teams' : 'projects']; let allowed = true;
  await act(async () => { allowed = await group.resolve(group.items[0]); });
  expect(allowed).toBe(false); expect(entities[kind === 'team' ? 'teams' : 'projects'].items).toEqual([]);
  expect(container.querySelector('[role="alert"]')?.textContent).toContain(failure === 'denied' ? 'Current access denied' : `This ${kind} is unavailable.`);
  expect(fetch.mock.calls.at(-1)![0]).toBe(kind === 'team' ? '/api/v1/workspaces/workspace/teams/selected' : '/api/v1/workspaces/workspace/teams/team/projects/selected/metadata');
});

it('ignores a successful detail response when authority changes during selection', async () => {
  const pending = deferred(); vi.stubGlobal('fetch', vi.fn((url: string) => url.includes('?') ? Promise.resolve(json(page(url.includes('/projects?') ? project('selected') : team('selected')))) : pending.promise));
  await render(); await tick(); const group = entities.teams; let allowed!: Promise<boolean>;
  await act(async () => { allowed = group.resolve(group.items[0]); });
  await render('Match', source({ scope: 'account:workspace:new-epoch' })); expect(container.textContent).toBe('');
  await act(async () => pending.resolve(json(team('selected')))); expect(await allowed).toBe(false); expect(container.textContent).toBe('');
});

it('shows independent team/project validation failures while a 101-scalar issue query remains usable', async () => {
  const text = '\u{1f680}'.repeat(101); const close = vi.fn(); const options = source();
  const fetch = vi.fn((url: string) => {
    const request = new URL(url, 'https://example.test'); expect(request.searchParams.get('q')).toBe(text);
    if (request.pathname.endsWith('/issues')) return Promise.resolve(json(page(issue('available'))));
    return Promise.resolve(json({ code: 'validation_failed', detail: request.pathname.endsWith('/projects') ? 'Project query limit is 100' : 'Team query limit is 100' }, 422));
  }); vi.stubGlobal('fetch', fetch);
  await act(async () => root.render(<CommandPalette commands={[]} close={close} issueSource={options} />)); await typeQuery(text); await tick();
  expect(fetch).toHaveBeenCalledTimes(3); expect(container.querySelector('input')!.value).toBe(text);
  expect(container.textContent).toContain('Team query limit is 100'); expect(container.textContent).toContain('Project query limit is 100');
  expect(container.querySelectorAll('[role="option"]')).toHaveLength(1); expect(container.textContent).toContain('Match issue available');
  expect(close).not.toHaveBeenCalled(); expect(options.open).not.toHaveBeenCalled();
});

it('fairly bounds all remote groups to twenty rows while retaining matching local commands', async () => {
  const data = Array.from({ length: 20 }, (_, index) => String(index));
  vi.stubGlobal('fetch', vi.fn((url: string) => Promise.resolve(json(page(...data.map(id => url.includes('/issues?') ? issue(id) : url.includes('/projects?') ? project(id, `Match project ${id}`) : team(id, `Match team ${id}`)))))));
  await act(async () => root.render(<CommandPalette commands={[{ id: 'local', label: 'Match local action', section: 'Actions', run: vi.fn() }]} close={vi.fn()} issueSource={source()} />));
  await typeQuery('Match'); await tick(); const rows = Array.from(container.querySelectorAll('[role="option"]'));
  expect(rows).toHaveLength(21); expect(rows[0].textContent).toContain('Match local action');
  expect(rows.filter(row => row.textContent?.includes('Match issue'))).toHaveLength(7);
  expect(rows.filter(row => row.textContent?.includes('Match team'))).toHaveLength(7);
  expect(rows.filter(row => row.textContent?.includes('Match project'))).toHaveLength(6);
  expect(rows.slice(1, 4).map(row => row.textContent)).toEqual(expect.arrayContaining([expect.stringContaining('Match issue 0'), expect.stringContaining('Match team 0'), expect.stringContaining('Match project 0')]));
});

it.each(['team', 'project'] as const)('reauthorizes keyboard-selected %s navigation once and preserves the dirty-navigation guard', async kind => {
  const frames: FrameRequestCallback[] = []; vi.stubGlobal('requestAnimationFrame', (frame: FrameRequestCallback) => { frames.push(frame); return frames.length; });
  const confirm = vi.spyOn(window, 'confirm').mockReturnValue(false); const close = vi.fn(); const detail = deferred();
  const fetch = vi.fn((url: string) => {
    if (!url.includes('?')) return detail.promise;
    return Promise.resolve(json(page(...(kind === 'team' && url.includes('/teams?') ? [team('destination', 'Match team')] : kind === 'project' && url.includes('/projects?') ? [project('destination', 'Match project')] : []))));
  }); vi.stubGlobal('fetch', fetch);
  function DirtyPalette() { useUnsavedChanges(true); return <CommandPalette commands={[]} close={close} issueSource={source()} />; }
  await act(async () => root.render(<DirtyPalette />)); await typeQuery('Match'); await tick();
  for (const options of [{ isComposing: true }, { repeat: true }, { ctrlKey: true }]) await key('Enter', options);
  expect(fetch).toHaveBeenCalledTimes(3); await key('Enter'); await key('Enter'); expect(fetch).toHaveBeenCalledTimes(4); expect(close).not.toHaveBeenCalled();
  await act(async () => detail.resolve(json(kind === 'team' ? team('destination') : project('destination'))));
  expect(close).toHaveBeenCalledOnce(); expect(frames).toHaveLength(1); expect(confirm).not.toHaveBeenCalled();
  await act(async () => frames[0](0)); expect(confirm).toHaveBeenCalledOnce(); expect(location.pathname + location.search).toBe('/w/workspace/issues?team_id=team');
});

it('does not request projects when the palette route has no explicit team', async () => {
  history.replaceState(null, '', '/w/workspace/issues');
  const fetch = vi.fn().mockImplementation(() => Promise.resolve(json(page()))); vi.stubGlobal('fetch', fetch);
  await act(async () => root.render(<CommandPalette commands={[]} close={vi.fn()} issueSource={source()} />)); await typeQuery('Match'); await tick();
  expect(fetch).toHaveBeenCalledTimes(2); expect(fetch.mock.calls.every(([url]) => !String(url).includes('/projects'))).toBe(true);
  expect(container.textContent).toContain('Choose a team to search its projects.');
});

it('replaces cached registry team navigation with one freshly authorized search result', async () => {
  const frames: FrameRequestCallback[] = []; vi.stubGlobal('requestAnimationFrame', (frame: FrameRequestCallback) => { frames.push(frame); return frames.length; });
  const cached = vi.fn(); const close = vi.fn(); const detail = deferred();
  const fetch = vi.fn((url: string) => {
    if (url.endsWith('/teams/destination')) return detail.promise;
    return Promise.resolve(json(page(...(url.includes('/teams?') ? [team('destination', 'Match current team')] : []))));
  }); vi.stubGlobal('fetch', fetch);
  await act(async () => root.render(<CommandPalette commands={[{ id: 'team-destination', label: 'Match cached team issues', section: 'Navigation', run: cached }]} close={close} issueSource={source()} />));
  expect(container.querySelectorAll('[role="option"]')).toHaveLength(0);
  await typeQuery('Match'); expect(container.querySelectorAll('[role="option"]')).toHaveLength(0); await tick();
  const rows = Array.from(container.querySelectorAll('[role="option"]'));
  expect(rows).toHaveLength(1); expect(rows[0].textContent).toContain('Match current team'); expect(container.textContent).not.toContain('Match cached team issues');
  expect(new Set(rows.map(row => row.id)).size).toBe(rows.length); expect(container.querySelectorAll('[role="option"][aria-selected="true"]')).toHaveLength(1);
  await key('Enter'); await key('Enter');
  expect(fetch.mock.calls.filter(([url]) => String(url).endsWith('/teams/destination'))).toHaveLength(1);
  expect(close).not.toHaveBeenCalled(); expect(cached).not.toHaveBeenCalled(); expect(frames).toHaveLength(0);
  await act(async () => detail.resolve(json(team('destination', 'Match current team'))));
  expect(close).toHaveBeenCalledOnce(); expect(cached).not.toHaveBeenCalled(); expect(frames).toHaveLength(1);
  await act(async () => frames[0](0)); expect(location.pathname + location.search).toBe('/w/workspace/issues?team_id=destination'); expect(cached).not.toHaveBeenCalled();
});
