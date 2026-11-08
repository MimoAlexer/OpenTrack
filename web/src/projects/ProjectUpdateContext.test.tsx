// @vitest-environment jsdom
import { act, type ReactNode } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ApiError } from '../api';
import { hasUnsavedChanges } from '../router';
import type { Session } from '../types';
import { ProjectUpdateOverview } from './ProjectUpdateOverview';
import { ProjectUpdates } from './ProjectUpdates';
import type { Project, ProjectUpdate } from './types';
import { compareUpdateSnapshots, emptyNotes, notesProblem, type UpdateNotes } from './updateContext';

const project: Project = { id: 'project', workspace_id: 'workspace', team_id: 'team', name: 'Delivery', summary: '', state: 'active', health: 'on_track', lead_id: null, lead: null, start_date: '2026-11-01', target_date: '2026-12-01', version: '1', etag: '"project:1"', created_at: '2026-11-01T10:00:00Z', updated_at: '2026-11-01T10:00:00Z', archived_at: null, can_edit: true, can_manage: true };
const session: Session = { audience: 'web', user: { id: 'owner', email: 'owner@example.test', display_name: 'Example Owner' }, csrf_token: 'csrf', workspaces: [{ id: 'workspace', name: 'Workspace', slug: 'workspace', role: 'owner' }] };
const riskId = '10000000-0000-4000-8000-000000000001'; const decisionId = '10000000-0000-4000-8000-000000000002';
const notes = (): UpdateNotes => ({ risks: [{ id: riskId, title: 'Dependency delay', detail: 'Review the delivery window.', status: 'open' }], decisions: [{ id: decisionId, title: 'Choose rollout date', detail: 'Confirm with the team.', status: 'needed' }] });
function update(overrides: Record<string, unknown> = {}): ProjectUpdate { return { id: 'latest', author_id: 'owner', author_name: 'Example Owner', body: 'Progress update', state: 'active', health: 'on_track', completed: 2, eligible: 5, created_at: '2026-11-08T10:00:00Z', context: { start_date: '2026-11-01', target_date: '2026-12-01', notes: notes() }, previous: null, ...overrides } as unknown as ProjectUpdate; }
const json = (value: unknown, status = 200) => new Response(JSON.stringify(value), { status });
const page = (data: ProjectUpdate[], next: string | null = null) => ({ data, has_more: !!next, next_cursor: next });
let container: HTMLDivElement; let root: Root;
beforeEach(() => { vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true); container = document.createElement('div'); document.body.append(container); root = createRoot(container); });
afterEach(async () => { await act(async () => root.unmount()); container.remove(); vi.restoreAllMocks(); vi.unstubAllGlobals(); });
async function render(node: ReactNode) { await act(async () => root.render(node)); }
function button(label: string) { const item = [...container.querySelectorAll<HTMLButtonElement>('button')].find(item => item.textContent === label || item.getAttribute('aria-label') === label); if (!item) throw new Error(`Missing button ${label}`); return item; }
async function click(label: string) { await act(async () => button(label).click()); }
async function input(label: string, value: string) { const item = container.querySelector<HTMLInputElement | HTMLTextAreaElement | HTMLSelectElement>(`[aria-label="${label}"]`)!; await act(async () => { const proto = item instanceof HTMLSelectElement ? HTMLSelectElement.prototype : item instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype; Object.getOwnPropertyDescriptor(proto, 'value')!.set!.call(item, value); item.dispatchEvent(new Event(item instanceof HTMLSelectElement ? 'change' : 'input', { bubbles: true })); }); }
async function body(value: string) { const item = container.querySelector<HTMLTextAreaElement>('[name="project-update"]')!; await act(async () => { Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value')!.set!.call(item, value); item.dispatchEvent(new Event('input', { bubbles: true })); }); }
async function includeNotes() { await act(async () => container.querySelector<HTMLInputElement>('input[type="checkbox"]')!.click()); }
async function submit() { await act(async () => container.querySelector('form')!.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }))); }
const screen = (props: Partial<Parameters<typeof ProjectUpdates>[0]> = {}) => <ProjectUpdates project={project} session={session} refresh={0} readOnly={false} denied={() => {}} {...props}/>;

describe('recorded update comparisons', () => {
  it('compares dates and stable risk/decision identities without treating missing notes as empty', () => {
    const previous = update({ id: 'older', context: { start_date: null, target_date: '2026-11-20', notes: { risks: [{ ...notes().risks[0], status: 'mitigated' }], decisions: [] } } });
    const changes = compareUpdateSnapshots(update(), previous);
    expect(changes.dates).toHaveLength(2); expect(changes.dates?.[0]).toContain('Start date: Not set');
    expect(changes.notes).toContain('Risk changed: Dependency delay (Mitigated → Open).');
    expect(changes.notes).toContain('Decision added: Choose rollout date (Decision needed).');
    const unknown = compareUpdateSnapshots(update({ context: null }), previous); expect(unknown.dates).toBeNull(); expect(unknown.notes).toBeNull();
    const omitted = compareUpdateSnapshots(update({ context: { start_date: null, target_date: null, notes: null } }), update()); expect(omitted.notes).toBeNull();
    const removal = compareUpdateSnapshots(update({ context: { start_date: null, target_date: null, notes: emptyNotes() } }), update());
    expect(removal.notes).toContain('Risk removed from this update: Dependency delay.');
    expect(removal.notes?.join(' ')).not.toContain('resolved');
  });
  it('compares a server-provided previous snapshot before the next page has loaded', async () => {
    const previous = update({ id: 'older', created_at: '2026-11-07T10:00:00Z', context: { start_date: null, target_date: '2026-11-20', notes: emptyNotes() } });
    vi.stubGlobal('fetch', vi.fn((url: string) => Promise.resolve(json(url.includes('cursor=') ? page([previous]) : page([update({ previous })], 'older-page')))));
    await render(screen({ readOnly: true }));
    expect(container.querySelectorAll('article')).toHaveLength(1); expect(container.textContent).toContain('Target date:'); expect(container.textContent).toContain('Risk added: Dependency delay');
    expect(container.querySelector('.project-update-comparison a')).toBeNull();
    await click('Load more updates');
    const articles = container.querySelectorAll('article'); const link = articles[0].querySelector<HTMLAnchorElement>('.project-update-comparison a')!;
    expect(link.getAttribute('href')).toBe(`#${articles[1].id}`); await act(async () => link.click()); expect(document.activeElement).toBe(articles[1]);
  });
  it('keeps legacy dates and notes explicitly unknown, while reviewed empty lists are affirmative', async () => {
    vi.stubGlobal('fetch', vi.fn(() => Promise.resolve(json(page([update({ id: 'known-empty', context: { start_date: null, target_date: null, notes: emptyNotes() } }), update({ id: 'legacy', context: null })])))));
    await render(screen({ readOnly: true }));
    const articles = container.querySelectorAll('article'); expect(articles[0].textContent).toContain('No risks recorded in this update.');
    expect(articles[1].textContent).toContain('Project dates were not captured'); expect(articles[1].textContent).toContain('Risks and decisions were not recorded'); expect(articles[1].textContent).not.toContain('No risks recorded');
    expect(container.querySelector('form')).toBeNull();
  });
});

describe('update notes authoring', () => {
  it('omits unreviewed notes and retains the project precondition', async () => {
    const fetch = vi.fn((_url: string, options: RequestInit) => Promise.resolve(json(options.method === 'POST' ? update() : page([])))); vi.stubGlobal('fetch', fetch);
    await render(screen()); await body('Written progress only'); await submit();
    const sent = fetch.mock.calls.find(([, options]) => options.method === 'POST')![1];
    expect(JSON.parse(sent.body as string)).toEqual({ body: 'Written progress only' }); expect(new Headers(sent.headers).get('If-Match')).toBe(project.etag);
  });
  it('records deliberately empty lists separately from omitted context', async () => {
    const fetch = vi.fn((_url: string, options: RequestInit) => Promise.resolve(json(options.method === 'POST' ? update() : page([])))); vi.stubGlobal('fetch', fetch);
    await render(screen()); await body('Reviewed the current risks'); await includeNotes(); await submit();
    const sent = fetch.mock.calls.find(([, options]) => options.method === 'POST')![1]; expect(JSON.parse(sent.body as string).notes).toEqual(emptyNotes());
  });
  it('authors stable notes with bounded fields and restores keyboard focus after removal', async () => {
    const fetch = vi.fn((_url: string, options: RequestInit) => Promise.resolve(json(options.method === 'POST' ? update() : page([])))); vi.stubGlobal('fetch', fetch);
    await render(screen()); await body('Reviewing risks and decisions'); await includeNotes(); await click('Add risk');
    expect(document.activeElement).toBe(container.querySelector('[aria-label="Risk 1 title"]'));
    expect(button('Publish update').disabled).toBe(true);
    await input('Risk 1 title', '🧭'.repeat(161)); expect(container.querySelector<HTMLInputElement>('[aria-label="Risk 1 title"]')!.value).toBe('🧭'.repeat(161)); expect(button('Publish update').disabled).toBe(true);
    await input('Risk 1 title', 'Dependency delay'); await input('Risk 1 details', '  Review the delivery window.  '); await input('Risk 1 status', 'mitigated');
    await click('Add risk'); await input('Risk 2 title', 'Temporary risk'); await click('Remove risk 2'); expect(document.activeElement).toBe(container.querySelector('[aria-label="Risk 1 title"]'));
    await click('Add decision'); await input('Decision 1 title', 'Choose a release date'); await input('Decision 1 status', 'deferred'); await submit();
    const sent = JSON.parse(fetch.mock.calls.find(([, options]) => options.method === 'POST')![1].body as string);
    expect(sent.notes.risks).toHaveLength(1); expect(sent.notes.risks[0]).toMatchObject({ title: 'Dependency delay', status: 'mitigated' }); expect(sent.notes.risks[0].id).toMatch(/^[0-9a-f-]{36}$/);
    expect(sent.notes.decisions[0]).toMatchObject({ title: 'Choose a release date', status: 'deferred' }); expect(hasUnsavedChanges()).toBe(false);
  });
  it('limits list admission without silently dropping an existing note', async () => {
    const six = { risks: Array.from({ length: 6 }, (_, at) => ({ ...notes().risks[0], id: `10000000-0000-4000-8000-00000000000${at}`, title: `Risk ${at}` })), decisions: [] } as UpdateNotes;
    vi.stubGlobal('fetch', vi.fn(() => Promise.resolve(json(page([update({ context: { start_date: null, target_date: null, notes: six } })])))));
    await render(screen()); await includeNotes(); expect(container.querySelectorAll('[data-note-id]')).toHaveLength(6); expect(button('Add risk').disabled).toBe(true);
    expect(notesProblem({ ...six, risks: [...six.risks, six.risks[0]] })).toContain('at most six');
  });
  it('retries the exact authored notes, precondition and key after a lost response', async () => {
    let posts = 0; const fetch = vi.fn((_url: string, options: RequestInit) => options.method === 'POST' ? ++posts === 1 ? Promise.reject(new TypeError('lost response')) : Promise.resolve(json(update(), 201)) : Promise.resolve(json(page([])))); vi.stubGlobal('fetch', fetch);
    await render(screen()); await body('Risk review'); await includeNotes(); await click('Add risk'); await input('Risk 1 title', 'Supplier delay'); await submit();
    expect(container.querySelector<HTMLTextAreaElement>('[name="project-update"]')!.disabled).toBe(true); expect(hasUnsavedChanges()).toBe(true);
    await click('Retry safely'); const requests = fetch.mock.calls.filter(([, options]) => options.method === 'POST').map(([, options]) => options);
    expect(requests[0].body).toBe(requests[1].body); expect(new Headers(requests[0].headers).get('Idempotency-Key')).toBe(new Headers(requests[1].headers).get('Idempotency-Key')); expect(new Headers(requests[0].headers).get('If-Match')).toBe(new Headers(requests[1].headers).get('If-Match')); expect(hasUnsavedChanges()).toBe(false);
  });
  it('preserves authored notes through an explicit review of changed project dates', async () => {
    let posts = 0; const latest = { ...project, target_date: '2026-12-15', etag: '"project:2"', version: '2' }; const changed = vi.fn();
    const fetch = vi.fn((url: string, options: RequestInit) => Promise.resolve(options.method === 'POST' ? ++posts === 1 ? json({ code: 'version_conflict', detail: 'Changed' }, 412) : json(update(), 201) : json(url.endsWith('/metadata') ? latest : page([])))); vi.stubGlobal('fetch', fetch);
    await render(screen({ changed })); await body('Keep my review'); await includeNotes(); await click('Add decision'); await input('Decision 1 title', 'Choose date'); await submit();
    expect(button('Publish update').disabled).toBe(true); expect(container.querySelector<HTMLInputElement>('[aria-label="Decision 1 title"]')!.value).toBe('Choose date');
    await click('Use current project context'); expect(changed).toHaveBeenCalledWith(latest); await submit();
    const requests = fetch.mock.calls.filter(([, options]) => options.method === 'POST').map(([, options]) => options);
    expect(new Headers(requests[1].headers).get('If-Match')).toBe(latest.etag); expect(new Headers(requests[0].headers).get('Idempotency-Key')).not.toBe(new Headers(requests[1].headers).get('Idempotency-Key')); expect(JSON.parse(requests[1].body as string).notes.decisions[0].title).toBe('Choose date');
  });
  it('removes private notes and the composer after access revocation', async () => {
    let reads = 0; const denied = vi.fn(); vi.stubGlobal('fetch', vi.fn(() => Promise.resolve(++reads === 1 ? json(page([update()])) : json({ code: 'not_found', detail: 'Not found' }, 404))));
    await render(screen({ denied })); expect(container.textContent).toContain('Dependency delay'); await render(screen({ denied, refresh: 1 }));
    expect(container.textContent).not.toContain('Dependency delay'); expect(container.querySelector('form')).toBeNull(); expect(denied).toHaveBeenCalled();
  });
  it('keeps a draft read-only when conflict review observes revoked write authority, then reloads it', async () => {
    let currentReads = 0; let posts = 0;
    const fetch = vi.fn((url: string, options: RequestInit) => Promise.resolve(options.method === 'POST' ? (++posts, json({ code: 'version_conflict', detail: 'Changed' }, 412)) : json(url.endsWith('/metadata') ? { ...project, etag: '"project:2"', can_edit: ++currentReads > 1 } : page([])))); vi.stubGlobal('fetch', fetch);
    await render(screen()); await body('Preserve this review'); await submit();
    expect(button('Use current project context').disabled).toBe(true); expect(container.querySelector<HTMLTextAreaElement>('[name="project-update"]')!.disabled).toBe(true); expect(container.textContent).toContain('Your draft remains preserved.');
    await click('Reload current project context'); expect(button('Use current project context').disabled).toBe(false); await click('Use current project context');
    expect(container.querySelector<HTMLTextAreaElement>('[name="project-update"]')!.value).toBe('Preserve this review'); expect(button('Publish update').disabled).toBe(false); expect(posts).toBe(1);
  });
});

describe('latest update overview', () => {
  it('surfaces only recorded open risks and decisions needing attention with the latest update', async () => {
    const currentNotes = notes(); currentNotes.risks.push({ ...currentNotes.risks[0], id: 'closed-risk', title: 'Closed risk', status: 'closed' }); currentNotes.decisions.push({ ...currentNotes.decisions[0], id: 'settled', title: 'Settled decision', status: 'decided' }); const open = vi.fn();
    await render(<ProjectUpdateOverview resource={{ value: page([update({ context: { start_date: null, target_date: '2026-12-01', notes: currentNotes } })]), loading: false, error: null, retry() {} }} openUpdates={open}/>);
    expect(container.textContent).toContain('Progress update'); expect(container.textContent).toContain('Dependency delay'); expect(container.textContent).toContain('Choose rollout date'); expect(container.textContent).not.toContain('Closed risk'); expect(container.textContent).not.toContain('Settled decision');
    await click('View updates'); expect(open).toHaveBeenCalledOnce();
  });
  it('distinguishes loading, no update, legacy context and transient refresh failure', async () => {
    const props = { value: null, loading: true, error: null, retry() {} };
    await render(<ProjectUpdateOverview resource={props} openUpdates={() => {}}/>); expect(container.textContent).toContain('Loading latest project update');
    await render(<ProjectUpdateOverview resource={{ ...props, loading: false }} openUpdates={() => {}}/>); expect(container.textContent).toContain('No project update has been published');
    await render(<ProjectUpdateOverview resource={{ ...props, loading: false, value: page([update({ context: null })]), error: new ApiError(503, 'unavailable', 'Try again') }} openUpdates={() => {}}/>);
    expect(container.textContent).toContain('Showing the last confirmed update'); expect(container.textContent).toContain('Risks and decisions were not recorded'); expect(container.textContent).not.toContain('No open risks recorded');
    await render(<ProjectUpdateOverview resource={{ ...props, loading: false, value: page([update()]), error: new ApiError(404, 'not_found', 'Not found') }} openUpdates={() => {}}/>); expect(container.textContent).not.toContain('Progress update'); expect(container.textContent).not.toContain('Dependency delay');
  });
});
