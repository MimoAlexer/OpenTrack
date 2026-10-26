// @vitest-environment jsdom
import { act, type ReactNode } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { RichTextEditorProps } from '../documents/RichTextEditor';
import { EMPTY_BODY, fromPlainText } from '../documents/model';
import type { StructuredDocument } from '../documents/types';
import { hasUnsavedChanges, parseRoute } from '../router';
import type { Bootstrap, Session, Team } from '../types';
import { Projects, Progress } from './Projects';
import { ProjectEditor } from './ProjectEditor';
import { ProjectUpdates } from './ProjectUpdates';
import { ProjectIssues } from './ProjectIssues';
import { projectMetadata, type FullProject, type Project, type ProjectProgress } from './types';
import { useProjectData, useProjectPage } from './useProjectData';

vi.mock('../documents/Editor', () => ({ StructuredEditor: (props: RichTextEditorProps) => <textarea aria-label={props.label} value={JSON.stringify(props.value)} disabled={props.disabled} onChange={event => props.onChange(JSON.parse(event.target.value))}/> }));
const team: Team = { id: 'team', key: 'ENG', name: 'Engineering', private: false, archived_at: null };
const session: Session = { audience: 'web', user: { id: 'owner', email: 'owner@example.test', display_name: 'Example Owner' }, csrf_token: 'csrf', workspaces: [{ id: 'workspace', name: 'Example Workspace', slug: 'example', role: 'owner' }] };
const data: Bootstrap = { auth_epoch: '1', teams: [team], issues: [], members: [session.user], statuses: [] };
const project: Project = { id: 'project', workspace_id: 'workspace', team_id: 'team', name: 'New onboarding', summary: 'Help people reach their first result', state: 'planned', health: 'on_track', lead_id: null, lead: null, start_date: null, target_date: null, version: '1', etag: '"project-metadata:project:1"', created_at: '2026-10-20T10:00:00Z', updated_at: '2026-10-20T10:00:00Z', archived_at: null, can_edit: true, can_manage: true };
function fullProject(overrides: Partial<FullProject> = {}): FullProject { return { ...project, narrative: { body: EMPTY_BODY, plain_text: '', schema_supported: true, schema_version: 1 }, narrative_version: '1', etag: '"project:project:1:document:1"', metadata_etag: project.etag, ...overrides }; }
function documentRecord(text = 'Saved project brief', version = '1'): StructuredDocument { return { id: 'document', owner_id: project.id, owner_type: 'project', body: fromPlainText(text), plain_text: text, schema_version: 1, schema_supported: true, version, etag: `"document:document:${version}"`, can_edit: true, mentions: [], issue_references: [], created_at: project.created_at, updated_at: project.updated_at }; }
const progress: ProjectProgress = { completed: 2, eligible: 5, canceled: 1, open: 3, total: 6, unscoped: false, as_of: '2026-10-20T10:00:00Z' };
const json = (value: unknown, status = 200) => new Response(JSON.stringify(value), { status });
const page = (data: unknown[], next: string | null = null) => ({ data, has_more: !!next, next_cursor: next });
let container: HTMLDivElement; let root: Root;
beforeEach(() => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true);
  vi.spyOn(window, 'confirm').mockReturnValue(true);
  HTMLDialogElement.prototype.showModal = function () { this.setAttribute('open', ''); };
  HTMLDialogElement.prototype.close = function () { this.removeAttribute('open'); };
  container = document.createElement('div'); document.body.append(container); root = createRoot(container);
});
afterEach(async () => { await act(async () => root.unmount()); container.remove(); vi.useRealTimers(); vi.restoreAllMocks(); vi.unstubAllGlobals(); });
async function render(node: ReactNode) { await act(async () => root.render(node)); }
function button(label: string) { const found = [...container.querySelectorAll<HTMLButtonElement>('button')].find(item => item.textContent === label || item.getAttribute('aria-label') === label); if (!found) throw new Error(`Missing button ${label}`); return found; }
async function click(label: string) { await act(async () => button(label).click()); }
async function input(selector: string, value: string) {
  const element = container.querySelector<HTMLInputElement | HTMLTextAreaElement | HTMLSelectElement>(selector)!;
  await act(async () => { const proto = element instanceof HTMLSelectElement ? HTMLSelectElement.prototype : element instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype; Object.getOwnPropertyDescriptor(proto, 'value')!.set!.call(element, value); element.dispatchEvent(new Event(element instanceof HTMLSelectElement ? 'change' : 'input', { bubbles: true })); });
}
async function submit(selector = 'form') { await act(async () => container.querySelector(selector)!.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }))); }
function detailFetch(url: string) { return json(url === '/api/v1/capabilities' ? { features: { attachments: true }, limits: { max_upload_bytes: 1024 } } : url.includes('/revisions') ? page([]) : url.endsWith('/document') || url.endsWith('/documents/document') ? documentRecord() : url.includes('/progress') ? progress : url.includes('/milestones') ? { ...page([]), project_etag: project.etag } : url.includes('/issues') || url.includes('/updates') ? page([]) : url.endsWith('/metadata') ? project : fullProject()); }
function screen(props: Partial<Parameters<typeof Projects>[0]> = {}) { return <Projects workspaceId="workspace" session={session} data={data} params={new URLSearchParams({ team_id: 'team' })} refresh={0} readOnly={false} {...props}/>; }

describe('project navigation and scope', () => {
  it('recognizes project list and detail routes without changing issue routes', () => {
    expect(parseRoute('/w/workspace/projects')).toMatchObject({ area: 'projects', valid: true, projectId: undefined });
    expect(parseRoute('/w/workspace/projects/project?team_id=team')).toMatchObject({ area: 'projects', valid: true, projectId: 'project' });
    expect(parseRoute('/w/workspace/issues/issue')).toMatchObject({ area: 'issues', valid: true, issueId: 'issue' });
    expect(parseRoute('/w/workspace/projects/project/unknown').valid).toBe(false);
  });
  it('lists real project metadata and uses a team-qualified detail link', async () => {
    const fetch = vi.fn(() => Promise.resolve(json(page([project])))); vi.stubGlobal('fetch', fetch);
    await render(screen());
    const link = container.querySelector<HTMLAnchorElement>('a.project-row')!;
    expect(link.getAttribute('href')).toBe('/w/workspace/projects/project?team_id=team');
    expect(link.textContent).toContain('New onboarding'); expect(link.textContent).toContain('On track');
    expect(fetch.mock.calls).toHaveLength(1);
  });
  it('does not fall back to another team when a selected team is unavailable', async () => {
    const fetch = vi.fn(); vi.stubGlobal('fetch', fetch);
    await render(screen({ params: new URLSearchParams({ team_id: 'private-team' }) }));
    expect(container.textContent).toContain('Team unavailable'); expect(fetch).not.toHaveBeenCalled();
  });
  it('does not offer mutation controls in a read-only workspace', async () => {
    vi.stubGlobal('fetch', vi.fn((url: string) => Promise.resolve(detailFetch(url))));
    await render(screen({ projectId: project.id, readOnly: true }));
    expect(container.textContent).toContain('New onboarding');
    expect([...container.querySelectorAll('button')].some(item => /Edit project|Add issues|Publish update|Archive project/.test(item.textContent ?? '') || item.getAttribute('aria-label') === 'Archive project')).toBe(false);
  });
  it('removes drafts and project data when the authenticated session boundary changes', async () => {
    let finish!: (value: Response) => void; let second = false;
    vi.stubGlobal('fetch', vi.fn((url: string) => second ? new Promise<Response>(resolve => { finish = resolve; }) : Promise.resolve(detailFetch(url))));
    await render(screen({ projectId: project.id })); await click('Updates'); await input('textarea[name="project-update"]', 'Private old session draft');
    second = true; await render(screen({ projectId: project.id, session: { ...session, csrf_token: 'replacement-session' } }));
    expect(container.textContent).not.toContain(project.name); expect(container.querySelector('textarea')).toBeNull(); expect(hasUnsavedChanges()).toBe(false);
    await act(async () => finish(json(project))); expect(container.textContent).not.toContain('Private old session draft');
  });
  it('keeps update drafts when moving between project tabs', async () => {
    vi.stubGlobal('fetch', vi.fn((url: string) => Promise.resolve(detailFetch(url))));
    await render(screen({ projectId: project.id }));
    await click('Updates'); await input('textarea[name="project-update"]', 'Keep this progress note.'); await click('Overview'); await click('Updates');
    expect(container.querySelector<HTMLTextAreaElement>('textarea[name="project-update"]')?.value).toBe('Keep this progress note.'); expect(hasUnsavedChanges()).toBe(true);
  });
});

describe('project edits', () => {
  it.each(['create', 'edit'] as const)('retains scalar-boundary drafts and repairs server rejections during %s', async mode => {
    const saved = vi.fn(); const denied = vi.fn(); let reject = true;
    const fetch = vi.fn((_url: string, _options: RequestInit) => Promise.resolve(reject ? json({ code: 'invalid', detail: 'Project text exceeds its limit' }, 422) : json(mode === 'create' ? fullProject() : project))); vi.stubGlobal('fetch', fetch);
    await render(editor({ project: mode === 'create' ? undefined : project, saved, denied }));
    const name = '😀'.repeat(160); const summary = '🧭'.repeat(500);
    await input('input[name="project-name"]', name + '😀'); await input('input[name="project-summary"]', summary + '🧭'); await submit();
    expect(JSON.parse(fetch.mock.calls[0][1].body as string)).toMatchObject({ name: name + '😀', summary: summary + '🧭' });
    expect(container.querySelector<HTMLInputElement>('input[name="project-name"]')!.value).toBe(name + '😀'); expect(container.querySelector<HTMLInputElement>('input[name="project-summary"]')!.value).toBe(summary + '🧭'); expect(saved).not.toHaveBeenCalled(); expect(denied).not.toHaveBeenCalled(); expect(hasUnsavedChanges()).toBe(true);
    reject = false; await input('input[name="project-name"]', ` ${name} `); await input('input[name="project-summary"]', ` ${summary} `); await submit();
    expect(JSON.parse(fetch.mock.calls[1][1].body as string)).toMatchObject({ name, summary }); expect(fetch).toHaveBeenCalledTimes(2); expect(saved).toHaveBeenCalledTimes(1); expect(hasUnsavedChanges()).toBe(false);
  });
  it('keeps names required and normalizes an empty project summary', async () => {
    const fetch = vi.fn((_url: string, _options: RequestInit) => Promise.resolve(json(project))); vi.stubGlobal('fetch', fetch);
    await render(editor()); await input('input[name="project-name"]', '   '); await input('input[name="project-summary"]', '   '); await submit(); expect(fetch).not.toHaveBeenCalled(); expect(button('Save project').disabled).toBe(true);
    await input('input[name="project-name"]', ' Valid name '); await submit(); expect(JSON.parse(fetch.mock.calls[0][1].body as string)).toMatchObject({ name: 'Valid name', summary: '' });
  });
  const editor = (props: Partial<Parameters<typeof ProjectEditor>[0]> = {}) => <ProjectEditor workspaceId="workspace" team={team} session={session} project={project} close={() => {}} saved={() => {}} denied={() => {}} {...props}/>;
  it('preserves a historical unavailable lead when changing unrelated project fields', async () => {
    const historical = { ...project, lead_id: 'former-lead', lead: { id: 'former-lead', display_name: 'Former Lead', assignable: false } };
    const saved = vi.fn();
    const fetch = vi.fn((_url: string, options: RequestInit) => {
      const body = JSON.parse(options.body as string);
      return Promise.resolve('lead_id' in body
        ? json({ code: 'invalid', detail: 'Choose an active contributor who can access this team' }, 422)
        : json({ ...historical, summary: body.summary, version: '2' }));
    });
    vi.stubGlobal('fetch', fetch); await render(editor({ project: historical, saved }));
    await input('input[name="project-summary"]', 'Updated project summary'); await submit();
    expect(saved).toHaveBeenCalledWith(expect.objectContaining({ lead_id: historical.lead_id, summary: 'Updated project summary' }));
    expect(JSON.parse(fetch.mock.calls[0][1].body as string)).not.toHaveProperty('lead_id');
    expect(JSON.parse(fetch.mock.calls[0][1].body as string)).not.toHaveProperty('narrative');
    expect(fetch.mock.calls[0][0]).toBe('/api/v1/workspaces/workspace/teams/team/projects/project/metadata');
    expect(container.querySelector('textarea[aria-label="Project brief"]')).toBeNull();
  });
  it('sends an explicit null when the user removes the project lead', async () => {
    const historical = { ...project, lead_id: 'former-lead', lead: { id: 'former-lead', display_name: 'Former Lead', assignable: false } };
    const fetch = vi.fn((_url: string, options: RequestInit) => Promise.resolve(json(options.method === 'PATCH' ? project : page([]))));
    vi.stubGlobal('fetch', fetch); await render(editor({ project: historical }));
    await act(async () => container.querySelector<HTMLButtonElement>('.reference-trigger')!.click());
    await act(async () => [...container.querySelectorAll<HTMLElement>('[role="option"]')].find(option => option.textContent === 'No lead')!.click());
    await submit();
    const write = fetch.mock.calls.find(([, options]) => options.method === 'PATCH')!;
    expect(JSON.parse(write[1].body as string)).toHaveProperty('lead_id', null);
  });
  it('requires explicit acknowledgement before completing without touching issue states', async () => {
    const saved = vi.fn(); const fetch = vi.fn(() => Promise.resolve(json({ ...project, state: 'completed', version: '2' })));
    vi.stubGlobal('fetch', fetch); await render(editor({ saved }));
    await input('[aria-label="Project state"]', 'completed'); expect(button('Save project').disabled).toBe(true);
    await act(async () => container.querySelector<HTMLInputElement>('input[type="checkbox"]')!.click()); await submit();
    const options = (fetch.mock.calls as unknown as [string, RequestInit][])[0][1];
    expect(new Headers(options.headers).get('If-Match')).toBe(project.etag);
    expect(JSON.parse(options.body as string)).toMatchObject({ state: 'completed', acknowledge_open_issues: true });
    expect(fetch).toHaveBeenCalledTimes(1); expect(saved).toHaveBeenCalledTimes(1); expect(hasUnsavedChanges()).toBe(false);
  });
  it('blocks reversed dates and retains the typed values', async () => {
    const fetch = vi.fn(); vi.stubGlobal('fetch', fetch); await render(editor());
    await input('input[name="project-start"]', '2026-11-20'); await input('input[name="project-target"]', '2026-11-10');
    expect(container.textContent).toContain('Target date must be on or after the start date.'); expect(button('Save project').disabled).toBe(true); await submit(); expect(fetch).not.toHaveBeenCalled();
  });
  it('preserves a rejected draft and requires review before retrying the latest ETag', async () => {
    const latest = { ...project, name: 'Saved by a teammate', version: '2', etag: '"project-metadata:project:2"' }; let writes = 0;
    const fetch = vi.fn((_url: string, options: RequestInit) => Promise.resolve(options.method === 'PATCH' ? ++writes === 1 ? json({ code: 'version_conflict', detail: 'This project changed.' }, 412) : json({ ...latest, name: 'My draft' }) : json(latest)));
    vi.stubGlobal('fetch', fetch); await render(editor()); await input('input[name="project-name"]', 'My draft'); await submit();
    expect(container.textContent).toContain('Saved by a teammate'); expect(container.querySelector<HTMLInputElement>('input[name="project-name"]')?.value).toBe('My draft'); expect(button('Save project').disabled).toBe(true);
    await click('Keep my draft for review'); expect(button('Save project').disabled).toBe(false); await submit();
    const requests = fetch.mock.calls.filter(([, options]) => options.method === 'PATCH'); expect(new Headers(requests[1][1].headers).get('If-Match')).toBe(latest.etag);
    expect(fetch.mock.calls.every(([url]) => url.endsWith('/metadata'))).toBe(true);
    expect(requests.every(([, options]) => !('narrative' in JSON.parse(options.body as string)))).toBe(true);
  });
  it('retries an uncertain creation with the same body and idempotency key', async () => {
    let writes = 0; const fetch = vi.fn(() => ++writes === 1 ? Promise.reject(new TypeError('connection lost')) : Promise.resolve(json(fullProject())));
    vi.stubGlobal('fetch', fetch); const saved = vi.fn(); await render(editor({ project: undefined, saved })); await input('input[name="project-name"]', 'New onboarding'); await input('textarea[aria-label="Project brief"]', JSON.stringify(fromPlainText('Initial brief'))); await submit();
    expect(button('Cancel').disabled).toBe(true); expect(container.querySelector<HTMLInputElement>('input[name="project-name"]')?.disabled).toBe(false); // the enclosing fieldset owns disabled state
    expect(container.querySelector('fieldset')?.disabled).toBe(true); await click('Retry safely');
    const requests = fetch.mock.calls as unknown as [string, RequestInit][];
    expect(new Headers(requests[0][1].headers).get('Idempotency-Key')).toBe(new Headers(requests[1][1].headers).get('Idempotency-Key')); expect(requests[0][1].body).toBe(requests[1][1].body); expect(saved).toHaveBeenCalledTimes(1);
    expect(requests[0][0]).toBe('/api/v1/workspaces/workspace/teams/team/projects');
    expect(JSON.parse(requests[0][1].body as string)).toHaveProperty('narrative', { schema_version: 1, body: fromPlainText('Initial brief') });
    expect(saved).toHaveBeenCalledWith(project);
  });
  it('archives with a versioned metadata-only command', async () => {
    const fetch = vi.fn(() => Promise.resolve(json({ ...project, archived_at: '2026-10-20T12:00:00Z' })));
    vi.stubGlobal('fetch', fetch); await render(editor({ mode: 'archive' })); await submit();
    const request = (fetch.mock.calls as unknown as [string, RequestInit][])[0][1];
    expect(new Headers(request.headers).get('If-Match')).toBe(project.etag); expect(JSON.parse(request.body as string)).toEqual({ archived: true });
    expect((fetch.mock.calls as unknown as [string, RequestInit][])[0][0]).toBe('/api/v1/workspaces/workspace/teams/team/projects/project/metadata');
  });
  it('does not run a stale save callback after its editor closes', async () => {
    let finish!: (value: Response) => void;
    vi.stubGlobal('fetch', vi.fn(() => new Promise<Response>(resolve => { finish = resolve; })));
    const saved = vi.fn(); await render(editor({ saved })); await input('input[name="project-name"]', 'Edited project'); await submit();
    await render(<div>New session</div>); await act(async () => finish(json({ ...project, name: 'Edited project' })));
    expect(saved).not.toHaveBeenCalled(); expect(container.textContent).toBe('New session');
  });
  it('asks before discarding and keeps focus on the existing draft', async () => {
    vi.stubGlobal('fetch', vi.fn()); const close = vi.fn(); await render(editor({ close })); await input('input[name="project-name"]', 'Unsaved name'); await click('Cancel');
    expect(container.textContent).toContain('Discard project changes?'); await click('Keep editing'); expect(close).not.toHaveBeenCalled(); expect(container.querySelector<HTMLInputElement>('input[name="project-name"]')?.value).toBe('Unsaved name');
  });
});

describe('independent project brief', () => {
  it('discovers and saves its document without refetching metadata or progress', async () => {
    const fetch = vi.fn((url: string, options: RequestInit) => Promise.resolve(options.method === 'PATCH' ? json(documentRecord('Revised brief', '2')) : detailFetch(url)));
    vi.stubGlobal('fetch', fetch); await render(screen({ projectId: project.id }));
    const editor = container.querySelector<HTMLTextAreaElement>('textarea[aria-label="Project brief"]')!;
    expect(editor.value).toBe(JSON.stringify(documentRecord().body));
    expect(fetch.mock.calls.some(([url]) => url.endsWith('/projects/project/document'))).toBe(true);
    expect(fetch.mock.calls.some(([url]) => url.endsWith('/projects/project/metadata'))).toBe(true);
    expect(fetch.mock.calls.some(([url]) => url.endsWith('/projects/project'))).toBe(false);
    const reads = fetch.mock.calls.filter(([url]) => /\/(metadata|progress)$/.test(url)).length;
    await input('textarea[aria-label="Project brief"]', JSON.stringify(fromPlainText('Revised brief'))); await click('Save text now');
    const writes = fetch.mock.calls.filter(([, options]) => options.method === 'PATCH');
    expect(writes).toHaveLength(1); expect(writes[0][0]).toBe('/api/v1/workspaces/workspace/documents/document');
    expect(new Headers(writes[0][1].headers).get('If-Match')).toBe(documentRecord().etag);
    expect(JSON.parse(writes[0][1].body as string)).toEqual({ schema_version: 1, body: fromPlainText('Revised brief') });
    expect(fetch.mock.calls.filter(([url]) => /\/(metadata|progress)$/.test(url))).toHaveLength(reads);
    expect(container.querySelector('textarea[aria-label="Project brief"]')).toBe(editor); expect(hasUnsavedChanges()).toBe(false);
  });
  it('preserves the brief editor and dirty text across tabs and project refreshes', async () => {
    vi.useFakeTimers(); vi.stubGlobal('fetch', vi.fn((url: string) => Promise.resolve(detailFetch(url))));
    await render(screen({ projectId: project.id }));
    const editor = container.querySelector<HTMLTextAreaElement>('textarea[aria-label="Project brief"]')!;
    const draft = JSON.stringify(fromPlainText('Keep this unsaved brief'));
    await input('textarea[aria-label="Project brief"]', draft); await click('Issues'); await click('Overview');
    expect(container.querySelector('textarea[aria-label="Project brief"]')).toBe(editor); expect(editor.value).toBe(draft);
    await render(screen({ projectId: project.id, refresh: 1 }));
    expect(container.querySelector('textarea[aria-label="Project brief"]')).toBe(editor); expect(editor.value).toBe(draft); expect(hasUnsavedChanges()).toBe(true);
  });
  it('keeps an unsaved brief when a metadata save advances the project version', async () => {
    vi.useFakeTimers(); const fetch = vi.fn((url: string, options: RequestInit) => Promise.resolve(options.method === 'PATCH' ? json({ ...project, name: 'Revised project name', version: '2', etag: '"project-metadata:project:2"' }) : detailFetch(url)));
    vi.stubGlobal('fetch', fetch); await render(screen({ projectId: project.id }));
    const editor = container.querySelector<HTMLTextAreaElement>('textarea[aria-label="Project brief"]')!; const draft = JSON.stringify(fromPlainText('Independent brief draft'));
    await input('textarea[aria-label="Project brief"]', draft); await click('Edit project'); await input('input[name="project-name"]', 'Revised project name'); await submit('dialog[open] form');
    expect(container.textContent).toContain('Revised project name'); expect(container.querySelector('textarea[aria-label="Project brief"]')).toBe(editor); expect(editor.value).toBe(draft); expect(hasUnsavedChanges()).toBe(true); expect(button('Archive project').disabled).toBe(true);
    expect(fetch.mock.calls.filter(([, options]) => options.method === 'PATCH').map(([url]) => url)).toEqual(['/api/v1/workspaces/workspace/teams/team/projects/project/metadata']);
  });
  it('opens document history lazily without replacing the current brief draft', async () => {
    vi.useFakeTimers(); const summary = { revision: '1', author: null, migrated: true, schema_version: 1, created_at: project.created_at, expires_at: '2027-10-20T10:00:00Z' };
    const fetch = vi.fn((url: string) => Promise.resolve(url.endsWith('/revisions/1') ? json({ ...summary, body: fromPlainText('Migrated brief snapshot'), plain_text: 'Migrated brief snapshot', mentions: [], issue_references: [] }) : url.includes('/revisions?') ? json(page([summary])) : detailFetch(url)));
    vi.stubGlobal('fetch', fetch); await render(screen({ projectId: project.id }));
    expect(fetch.mock.calls.some(([url]) => url.includes('/revisions'))).toBe(false);
    const editor = container.querySelector<HTMLTextAreaElement>('textarea[aria-label="Project brief"]')!; const draft = JSON.stringify(fromPlainText('Current brief draft'));
    await input('textarea[aria-label="Project brief"]', draft); await click('Brief history'); await click('View revision 1');
    expect(fetch.mock.calls.some(([url]) => url === '/api/v1/workspaces/workspace/documents/document/revisions/1')).toBe(true);
    expect(container.textContent).toContain('Migrated brief snapshot'); expect(container.textContent).toContain('Unknown author');
    expect(container.querySelector('textarea[aria-label="Project brief"]')).toBe(editor); expect(editor.value).toBe(draft); expect(hasUnsavedChanges()).toBe(true);
    await click('Done'); expect(editor.value).toBe(draft);
  });
  it('blocks archive for a dirty brief and for its pending save, then allows it after confirmation', async () => {
    vi.useFakeTimers(); let finish!: (value: Response) => void;
    const fetch = vi.fn((url: string, options: RequestInit) => options.method === 'PATCH' ? new Promise<Response>(resolve => { finish = resolve; }) : Promise.resolve(detailFetch(url)));
    vi.stubGlobal('fetch', fetch); await render(screen({ projectId: project.id }));
    expect(button('Archive project').disabled).toBe(false);
    await input('textarea[aria-label="Project brief"]', JSON.stringify(fromPlainText('Pending brief'))); expect(button('Archive project').disabled).toBe(true);
    await click('Save text now'); expect(button('Archive project').disabled).toBe(true);
    await act(async () => finish(json(documentRecord('Pending brief', '2')))); expect(button('Archive project').disabled).toBe(false);
  });
  it('pauses brief writes while the archive dialog is open', async () => {
    vi.useFakeTimers(); const fetch = vi.fn((url: string, options: RequestInit) => Promise.resolve(options.method === 'PATCH' ? json(documentRecord('Deferred brief', '2')) : detailFetch(url)));
    vi.stubGlobal('fetch', fetch); await render(screen({ projectId: project.id })); await click('Archive project');
    await input('textarea[aria-label="Project brief"]', JSON.stringify(fromPlainText('Deferred brief')));
    await act(async () => vi.advanceTimersByTime(1000)); expect(fetch.mock.calls.filter(([, options]) => options.method === 'PATCH')).toHaveLength(0);
    expect(button('Save text now').disabled).toBe(true);
    expect(container.querySelector<HTMLButtonElement>('.modal-footer .primary')?.disabled).toBe(true); await submit('dialog[open] form');
    expect(fetch.mock.calls.filter(([, options]) => options.method === 'PATCH')).toHaveLength(0);
    await click('Cancel'); await act(async () => vi.advanceTimersByTime(750));
    expect(fetch.mock.calls.filter(([, options]) => options.method === 'PATCH')).toHaveLength(1);
  });
  it('allows metadata editing and archive when a newer brief format is read-only', async () => {
    const unsupported = { ...documentRecord('Future brief'), schema_version: 2, schema_supported: false, can_edit: false, body: { type: 'future' } };
    const fetch = vi.fn((url: string, options: RequestInit) => Promise.resolve(options.method === 'PATCH' ? json({ ...project, summary: 'Metadata remains editable', version: '2' }) : url.endsWith('/document') ? json(unsupported) : detailFetch(url)));
    vi.stubGlobal('fetch', fetch); await render(screen({ projectId: project.id }));
    expect(container.textContent).toContain('Future brief'); expect(container.querySelector('textarea[aria-label="Project brief"]')).toBeNull(); expect(button('Archive project').disabled).toBe(false);
    await click('Edit project'); await input('input[name="project-summary"]', 'Metadata remains editable'); await submit('dialog[open] form');
    const write = fetch.mock.calls.find(([, options]) => options.method === 'PATCH')!;
    expect(write[0]).toBe('/api/v1/workspaces/workspace/teams/team/projects/project/metadata'); expect(new Headers(write[1].headers).get('If-Match')).toBe(project.etag); expect(JSON.parse(write[1].body as string)).not.toHaveProperty('narrative');
    expect(container.textContent).toContain('Metadata remains editable'); expect(button('Archive project').disabled).toBe(false);
  });
  it('conceals parent project facts when current document ownership is denied', async () => {
    vi.stubGlobal('fetch', vi.fn((url: string) => Promise.resolve(url.endsWith('/document') ? json({ code: 'not_found', detail: 'Unavailable' }, 404) : detailFetch(url))));
    await render(screen({ projectId: project.id }));
    expect(container.textContent).toContain('Project unavailable'); expect(container.textContent).not.toContain(project.name); expect(container.textContent).not.toContain(project.summary); expect(container.querySelector('.project-properties')).toBeNull(); expect(document.title).not.toContain(project.name);
  });
});

describe('progress and updates', () => {
  it('displays scalar counts and repairs a rejected supplementary update without duplicate publication', async () => {
    const body = '🧭'.repeat(8000); const update = { id: 'scalar-update', author_id: 'owner', author_name: 'Example Owner', body, state: project.state, health: project.health, completed: 2, eligible: 5, created_at: project.updated_at }; let published = false; let posts = 0;
    const fetch = vi.fn((_url: string, options: RequestInit) => { if (options.method === 'POST') { if (++posts === 1) return Promise.resolve(json({ code: 'invalid', detail: 'Project updates require 1–8000 characters' }, 422)); published = true; return Promise.resolve(json(update, 201)); } return Promise.resolve(json(page(published ? [update] : []))); }); vi.stubGlobal('fetch', fetch);
    await render(<ProjectUpdates project={project} session={session} refresh={0} readOnly={false} denied={() => {}}/>); await input('textarea', body); expect(container.querySelector('.project-update-actions > span')?.textContent).toBe('8,000 / 8,000');
    await input('textarea', body + '🧭'); await submit(); const writes = () => fetch.mock.calls.filter(([, options]) => options.method === 'POST'); expect(JSON.parse(writes()[0][1].body as string)).toEqual({ body: body + '🧭' }); expect(container.querySelector('textarea')?.value).toBe(body + '🧭'); expect(published).toBe(false); expect(hasUnsavedChanges()).toBe(true);
    await input('textarea', ` ${body} `); await submit(); expect(JSON.parse(writes()[1][1].body as string)).toEqual({ body }); expect(writes()).toHaveLength(2); expect(published).toBe(true); expect(container.querySelector('textarea')?.value).toBe(''); expect(container.querySelectorAll('.project-update')).toHaveLength(1); expect(container.textContent).toContain('2 of 5 completed'); expect(hasUnsavedChanges()).toBe(false);
  });
  it('uses the metadata validator and removes the embedded brief from full project responses', () => {
    expect(projectMetadata(fullProject())).toEqual(project);
    expect(projectMetadata(fullProject())).not.toHaveProperty('metadata_etag'); expect(projectMetadata(fullProject())).not.toHaveProperty('narrative');
  });
  it('normalizes an issue mutation response before replacing project metadata', async () => {
    const issue = { id: 'issue', key: 'ENG-8', title: 'A useful step', status_id: 'ready', status_name: 'Ready', status_category: 'unstarted', priority: 0, archived_at: null };
    const updated = fullProject({ version: '2', etag: '"full-body-sensitive-token"', metadata_etag: '"independent-metadata-token"' });
    const fetch = vi.fn((_url: string, options: RequestInit) => Promise.resolve(json(options.method === 'PUT' ? updated : page([issue]))));
    vi.stubGlobal('fetch', fetch); const changed = vi.fn(); await render(<ProjectIssues project={project} session={session} refresh={0} readOnly={false} changed={changed} denied={() => {}}/>); await click('Remove ENG-8 from project');
    expect(changed).toHaveBeenCalledWith({ ...project, version: '2', etag: updated.metadata_etag });
  });
  it('shows the denominator and exclusions while zero eligible scope is not 100 percent', async () => {
    await render(<Progress value={progress}/>); expect(container.textContent).toContain('40% complete'); expect(container.textContent).toContain('2 / 5 issues'); expect(container.textContent).toContain('1 canceled · excluded'); expect(container.querySelector('progress')?.max).toBe(5);
    await render(<Progress value={{ ...progress, completed: 0, eligible: 0, total: 0, open: 0, canceled: 0, unscoped: true }}/>); expect(container.textContent).toContain('No eligible issue scope'); expect(container.textContent).not.toContain('100%'); expect(container.querySelector('progress')).toBeNull();
  });
  it('publishes a retry-safe update and clears the draft only after success', async () => {
    let writes = 0; const update = { id: 'update', author_id: 'owner', author_name: 'Example Owner', body: 'A measured improvement', state: 'active', health: 'on_track', completed: 2, eligible: 5, created_at: '2026-10-20T12:00:00Z' };
    const fetch = vi.fn((_url: string, options: RequestInit) => options.method === 'POST' ? ++writes === 1 ? Promise.reject(new TypeError('offline')) : Promise.resolve(json(update)) : Promise.resolve(json(page(writes > 1 ? [update] : []))));
    vi.stubGlobal('fetch', fetch); await render(<ProjectUpdates project={project} session={session} refresh={0} readOnly={false} denied={() => {}}/>); await input('textarea', update.body); await submit(); expect(container.querySelector('textarea')?.value).toBe(update.body); expect(container.querySelector('textarea')?.disabled).toBe(true); await click('Retry safely');
    const posts = fetch.mock.calls.filter(([, options]) => options.method === 'POST'); expect(new Headers(posts[0][1].headers).get('Idempotency-Key')).toBe(new Headers(posts[1][1].headers).get('Idempotency-Key')); expect(container.querySelector('textarea')?.value).toBe(''); expect(container.textContent).toContain('2 of 5 completed');
  });
  it('uses the project version when removing an issue and reports stale scope without replay', async () => {
    const issue = { id: 'issue', key: 'ENG-8', title: 'A useful step', status_id: 'ready', status_name: 'Ready', status_category: 'unstarted', priority: 0, archived_at: null, milestone_id: null };
    const latest = { ...project, etag: '"project-metadata:project:2"', version: '2' };
    const live = { ...issue, workspace_id: project.workspace_id, team_id: project.team_id, etag: '"issue:issue:1"', version: '1', description: '', body_version: '1', assignee_id: null, assignee: null, label_ids: [], labels: [], estimate: null, due_date: null, created_at: project.created_at, updated_at: project.updated_at };
    const fetch = vi.fn((url: string, options: RequestInit) => Promise.resolve(options.method === 'PUT' ? json({ code: 'version_conflict', detail: 'Scope changed.' }, 412) : url.endsWith('/workspaces/workspace/issues/issue') ? json(live) : url.includes('/issues?') ? json(page([issue])) : json(latest)));
    vi.stubGlobal('fetch', fetch); const changed = vi.fn(); await render(<ProjectIssues project={project} session={session} refresh={0} readOnly={false} changed={changed} denied={() => {}}/>); await click('Remove ENG-8 from project');
    const writes = fetch.mock.calls.filter(([, options]) => options.method === 'PUT'); expect(writes).toHaveLength(1); expect(new Headers(writes[0][1].headers).get('If-Match')).toBe(project.etag); expect(JSON.parse(writes[0][1].body as string)).toEqual({ included: false }); expect(changed).toHaveBeenCalledWith(expect.objectContaining({ version: '2' })); expect(container.textContent).toContain('Your selection is preserved for review'); expect(container.querySelector('dialog[open]')?.textContent).toContain('Current state: in this project'); expect(button('Apply reviewed assignment').disabled).toBe(false);
    expect(fetch.mock.calls.some(([url]) => url.endsWith('/projects/project/metadata'))).toBe(true);
  });
});

describe('refresh and access boundaries', () => {
  function Item({ path, refresh = 0 }: { path: string; refresh?: number }) { const resource = useProjectData<{ id: string }>(path, refresh); return <div>{resource.value?.id ?? 'empty'}{resource.error ? 'failed' : ''}</div>; }
  it('keeps confirmed content during refresh but removes it after denied access', async () => {
    let finish!: (value: Response) => void; let calls = 0; vi.stubGlobal('fetch', vi.fn(() => ++calls === 1 ? Promise.resolve(json({ id: 'confirmed project' })) : new Promise<Response>(resolve => { finish = resolve; })));
    await render(<Item path="/projects/first"/>); const node = container.firstElementChild; await render(<Item path="/projects/first" refresh={1}/>); expect(container.textContent).toBe('confirmed project'); expect(container.firstElementChild).toBe(node);
    await act(async () => finish(json({ code: 'not_found', detail: 'Unavailable' }, 404))); expect(container.textContent).toBe('emptyfailed');
  });
  it('ignores an old team response after the route scope changes', async () => {
    let finish!: (value: Response) => void; vi.stubGlobal('fetch', vi.fn((url: string) => url.endsWith('/old') ? new Promise<Response>(resolve => { finish = resolve; }) : Promise.resolve(json({ id: 'new team project' }))));
    await render(<Item path="/projects/old"/>); await render(<Item path="/projects/new"/>); await act(async () => finish(json({ id: 'old team private project' }))); expect(container.textContent).toBe('new team project');
  });
  it('paginates without duplicates and refreshes back to the first page', async () => {
    function Items({ refresh }: { refresh: number }) { const resource = useProjectPage<{ id: string }>('/projects', refresh); return <><p>{resource.value?.data.map(item => item.id).join(',')}</p><button onClick={resource.more}>More</button></>; }
    const fetch = vi.fn((url: string) => Promise.resolve(json(url.includes('cursor=next') ? page([{ id: 'a' }, { id: 'b' }]) : page([{ id: 'a' }], 'next')))); vi.stubGlobal('fetch', fetch);
    await render(<Items refresh={0}/>); await click('More'); expect(container.textContent).toBe('a,bMore'); await render(<Items refresh={1}/>); expect(container.textContent).toBe('aMore'); expect(fetch.mock.calls.at(-1)?.[0]).not.toContain('cursor=');
  });
});

describe('planning freshness', () => {
  it('retains confirmed project counts and their time through a failed refresh, then reads a real zero', async () => {
    let finish!: (value: Response) => void; let reads = 0;
    const fresh = { completed: 0, eligible: 0, canceled: 0, open: 0, total: 0, unscoped: true, as_of: '2026-10-26T12:00:00Z' };
    const fetch = vi.fn((url: string, _options: RequestInit) => url.endsWith('/progress')
      ? ++reads === 1 ? Promise.resolve(json(progress)) : reads === 2 ? new Promise<Response>(resolve => { finish = resolve; }) : Promise.resolve(json(fresh))
      : Promise.resolve(detailFetch(url)));
    vi.stubGlobal('fetch', fetch);
    await render(screen({ projectId: project.id }));
    const section = container.querySelector('.project-progress-section')!;
    expect(section.textContent).toContain('2 / 5 issues');
    await render(screen({ projectId: project.id, refresh: 1 }));
    expect(section.querySelector('[role="status"]')?.textContent).toContain('Updating planning counts');
    expect(section.querySelector('time')?.dateTime).toBe(progress.as_of);
    expect(section.textContent).not.toContain('No eligible issue scope');
    await act(async () => finish(json({ code: 'unavailable', detail: 'Synthetic transport failure' }, 503)));
    expect(section.textContent).toContain('Counts could not be refreshed');
    expect(section.textContent).toContain('2 / 5 issues');
    expect(section.querySelector('time')?.dateTime).toBe(progress.as_of);
    await click('Refresh planning counts');
    expect(section.textContent).toContain('No eligible issue scope');
    expect(section.querySelector('progress')).toBeNull();
    expect(section.querySelector('time')?.dateTime).toBe(fresh.as_of);
    expect(fetch.mock.calls.every(([, options]) => !options.method || options.method === 'GET')).toBe(true);
  });

  it('does not resurrect retained project counts when an older allowed response arrives after denial', async () => {
    let finish!: (value: Response) => void; let reads = 0;
    vi.stubGlobal('fetch', vi.fn((url: string) => url.endsWith('/progress')
      ? ++reads === 1 ? Promise.resolve(json(progress)) : reads === 2 ? new Promise<Response>(resolve => { finish = resolve; }) : Promise.resolve(json({ code: 'not_found', detail: 'Unavailable' }, 404))
      : Promise.resolve(detailFetch(url))));
    await render(screen({ projectId: project.id }));
    await render(screen({ projectId: project.id, refresh: 1 }));
    await render(screen({ projectId: project.id, refresh: 2 }));
    expect(container.textContent).toContain('Project unavailable');
    await act(async () => finish(json({ ...progress, completed: 17, eligible: 19 })));
    expect(container.textContent).toContain('Project unavailable');
    expect(container.textContent).not.toContain('17 / 19');
    expect(container.textContent).not.toContain('No eligible issue scope');
    expect(container.querySelector('.project-progress')).toBeNull();
  });
});

describe('planning refresh consistency', () => {
  it('refreshes visible project issue statuses with their current counts', async () => {
    let changed = false;
    const issue = { id: 'refresh-issue', key: 'ENG-9', title: 'Refresh source row', status_id: 'started', status_name: 'In progress', status_category: 'started', priority: 0, archived_at: null, milestone_id: null };
    const fetch = vi.fn((url: string, _options: RequestInit) => Promise.resolve(url.includes('/projects/project/issues?')
      ? json(page([{ ...issue, ...(changed ? { status_id: 'done', status_name: 'Done', status_category: 'completed' } : {}) }]))
      : url.endsWith('/progress') ? json({ ...progress, completed: changed ? 3 : 2, open: changed ? 2 : 3 }) : detailFetch(url)));
    vi.stubGlobal('fetch', fetch);
    await render(screen({ projectId: project.id }));
    await click('Issues');
    expect(container.querySelector('.project-issue-row')?.textContent).toContain('In progress');
    await click('Overview'); changed = true;
    await click('Refresh planning counts');
    expect(container.querySelector('.project-progress-section')?.textContent).toContain('3 / 5 issues');
    await click('Issues');
    expect(container.querySelector('.project-issue-status')?.textContent).toBe('Done');
    expect(fetch.mock.calls.filter(([url]) => url.includes('/projects/project/issues?'))).toHaveLength(2);
    expect(fetch.mock.calls.every(([, options]) => !options.method || options.method === 'GET')).toBe(true);
  });
});
