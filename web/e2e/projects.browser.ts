import { registerAdmission } from './fixtures';
import { randomUUID } from 'node:crypto';
import type { Locator, Page, Request, Response } from '@playwright/test';
import { test, expect, paragraph, SavedWorkspace } from './fixtures';
import type { StructuredDocument } from '../src/documents/types';
import type { Bootstrap, Team } from '../src/types';
import type { FullProject, Project, ProjectIssue, ProjectProgress, ProjectUpdate } from '../src/projects/types';
import type { ProjectLeadReadiness } from './receipt-reporter';
registerAdmission(1);

const projectsPath = (saved: SavedWorkspace, id?: string) => `/teams/${saved.team.id}/projects${id ? `/${id}` : ''}`;
const projectURL = (saved: SavedWorkspace, id: string) => `/w/${saved.workspace}/projects/${id}?team_id=${saved.team.id}`;

async function createProject(saved: SavedWorkspace, prefix: string) {
  return saved.request<FullProject>(projectsPath(saved), 'POST', { name: `${prefix} ${randomUUID()}` }, { 'Idempotency-Key': randomUUID() });
}

const briefDocument = (saved: SavedWorkspace, id: string) => saved.request<StructuredDocument>(`${projectsPath(saved, id)}/document`);

async function openProject(page: Page, saved: SavedWorkspace, project: Project) {
  const response = await page.goto(projectURL(saved, project.id));
  expect(response?.ok(), 'The built frontend serves the saved project route').toBe(true);
  await expect(page.getByRole('heading', { name: project.name, exact: true })).toBeVisible();
}

function observeProjectLead(page: Page, saved: SavedWorkspace) {
  const facts: ProjectLeadReadiness = { request_state: 'unobserved', status: null, elapsed_ms: 0,
    q_scalar_length: null, returned_count: null, has_more: null, self_assignable: null,
    fixture_owner_in_page: null, expected_display_name_match: null, current_user_id_match: null,
    option_observation: 'unobserved', owner_option_match_count: null, you_marker_option_count: null,
    owner_option_visible: null, owner_option_enabled: null, click_failure: null,
    center_hit_self_or_descendant: null, center_inside_dialog: null, center_inside_viewport: null,
    matching_dialog_count: null, option_rect: null, popover_rect: null, wrapper_rect: null,
    trigger_rect: null, dialog_rect: null, center_hit_another_option: null,
    owner_option_content_rect: null, hit_option_rect: null, hit_option_content_rect: null };
  const annotation = { type: 'project-lead-readiness', description: '' };
  test.info().annotations.push(annotation);
  let active: Request | undefined; let started = 0; let stopped = false;
  const publish = () => { annotation.description = JSON.stringify(facts); };
  const matching = (request: Request, path: string) => {
    const url = new URL(request.url());
    return request.method() === 'GET' && url.origin === saved.origin && url.pathname === path;
  };
  const begin = (request: Request) => {
    if (!matching(request, `${saved.path}/teams/${saved.team.id}/assignees`)) return;
    active = request; started = performance.now();
    Object.assign(facts, { request_state: 'pending', status: null, elapsed_ms: 0,
      q_scalar_length: [...(new URL(request.url()).searchParams.get('q') ?? '')].length,
      returned_count: null, has_more: null, self_assignable: null, fixture_owner_in_page: null });
    publish();
  };
  const received = async (response: Response) => {
    const request = response.request();
    if (matching(request, '/api/v1/auth/session') && response.status() === 200) {
      try {
        const body = await response.json();
        if (!stopped && typeof body?.user?.id === 'string' && typeof body.user.display_name === 'string') {
          facts.current_user_id_match = body.user.id === saved.session.user.id;
          facts.expected_display_name_match = body.user.display_name === 'Example Owner'; publish();
        }
      } catch { /* No response bodies or raw errors enter the diagnostic. */ }
    }
    if (stopped || request !== active) return;
    facts.status = response.status(); publish();
    if (response.status() !== 200) return;
    try {
      const body = await response.json();
      if (!stopped && request === active && Array.isArray(body?.data)
          && typeof body.has_more === 'boolean' && typeof body.self_assignable === 'boolean') {
        facts.returned_count = body.data.length; facts.has_more = body.has_more;
        facts.self_assignable = body.self_assignable;
        facts.fixture_owner_in_page = body.data.some((person: { id?: unknown }) => person?.id === saved.session.user.id);
        publish();
      }
    } catch { /* A missing body observation remains unknown. */ }
  };
  const finish = (request: Request, state: 'completed' | 'failed') => {
    if (request !== active) return;
    facts.request_state = state; facts.elapsed_ms = Math.round(performance.now() - started); publish();
  };
  const completed = (request: Request) => finish(request, 'completed');
  const failed = (request: Request) => finish(request, 'failed');
  const stop = () => {
    if (stopped) return;
    stopped = true;
    if (facts.request_state === 'pending') facts.elapsed_ms = Math.round(performance.now() - started);
    publish(); page.off('request', begin); page.off('response', received);
    page.off('requestfinished', completed); page.off('requestfailed', failed); page.off('close', stop);
  };
  page.on('request', begin); page.on('response', received);
  page.on('requestfinished', completed); page.on('requestfailed', failed); page.once('close', stop); publish();
  return { stop, failure: (error: unknown) => {
    const message = error instanceof Error ? error.message : '';
    facts.click_failure = message.includes('intercepts pointer events') ? 'pointer_intercepted'
      : message.includes('outside of the viewport') ? 'outside_viewport'
      : message.includes('element is not stable') ? 'not_stable'
      : message.includes('element was detached') || message.includes('not attached to the DOM') ? 'detached'
      : message.includes('element is not visible') ? 'not_visible'
      : message.includes('element is not enabled') ? 'not_enabled'
      : message.includes('strict mode violation') ? 'strict_match' : 'unknown';
    publish();
  }, probe: async (dialog: Locator, observation: 'before_click' | 'after_failed_click') => {
    try {
      const options = dialog.getByRole('listbox', { name: 'Project lead', exact: true }).getByRole('option');
      const owner = options.filter({ hasText: /Example Owner/ });
      const observed: Partial<ProjectLeadReadiness> = { option_observation: observation,
        owner_option_match_count: await owner.count(),
        you_marker_option_count: await options.filter({ has: page.locator('small').filter({ hasText: /^You$/ }) }).count(),
        matching_dialog_count: await dialog.count(), owner_option_visible: null, owner_option_enabled: null,
        center_hit_self_or_descendant: null, center_inside_dialog: null, center_inside_viewport: null,
        option_rect: null, popover_rect: null, wrapper_rect: null, trigger_rect: null, dialog_rect: null,
        center_hit_another_option: null, owner_option_content_rect: null, hit_option_rect: null, hit_option_content_rect: null };
      if (observed.owner_option_match_count === 1) {
        observed.owner_option_visible = await owner.isVisible(); observed.owner_option_enabled = await owner.isEnabled();
        Object.assign(observed, await owner.evaluate(element => {
          const rect = (node: Element | null) => {
            if (!node) return null;
            const value = node.getBoundingClientRect();
            return { x: Math.round(value.x), y: Math.round(value.y), width: Math.round(value.width), height: Math.round(value.height) };
          };
          const box = element.getBoundingClientRect(); const x = box.x + box.width / 2; const y = box.y + box.height / 2;
          const dialog = element.closest('dialog'); const bounds = dialog?.getBoundingClientRect();
          const hit = document.elementFromPoint(x, y); const wrapper = element.closest('.reference-picker');
          const hitOption = hit?.closest('.reference-option') ?? null;
          return { center_hit_self_or_descendant: !!hit && (hit === element || element.contains(hit)),
            center_hit_another_option: !!hitOption && hitOption !== element,
            center_inside_dialog: bounds ? x >= bounds.left && x < bounds.right && y >= bounds.top && y < bounds.bottom : null,
            center_inside_viewport: x >= 0 && x < window.innerWidth && y >= 0 && y < window.innerHeight,
            option_rect: rect(element), popover_rect: rect(element.closest('.reference-popover')),
            wrapper_rect: rect(wrapper), trigger_rect: rect(wrapper?.querySelector('.reference-trigger') ?? null), dialog_rect: rect(dialog),
            owner_option_content_rect: rect(element.querySelector('span')),
            hit_option_rect: rect(hitOption), hit_option_content_rect: rect(hitOption?.querySelector('span') ?? null) };
        }));
      }
      Object.assign(facts, observed);
    } catch { /* Closed pages leave unavailable observations unknown. */ }
    if (facts.request_state === 'pending') facts.elapsed_ms = Math.round(performance.now() - started);
    publish();
  } };
}

test('project creation and brief edits persist while metadata and body versions remain independent', async ({ page, saved: owner }) => {
  const team = await owner.request<Team>('/teams', 'POST', {
    name: `Project acceptance ${randomUUID()}`, key: `P${randomUUID().replaceAll('-', '').slice(0, 8).toUpperCase()}`,
    private: false,
  }, { 'Idempotency-Key': randomUUID() });
  const bootstrap = await owner.request<Bootstrap>('/bootstrap');
  const saved = new SavedWorkspace(owner.http, owner.session, { ...bootstrap, teams: [team] }, owner.origin, owner.workspace);
  const name = `Saved project ${randomUUID()}`;
  const briefText = 'Ship the reviewed onboarding journey';
  const readiness = observeProjectLead(page, saved);
  await saved.open(page);
  await page.getByRole('navigation', { name: 'Workspace navigation' }).getByRole('link', { name: 'Projects', exact: true }).click();
  const teamPicker = page.getByRole('combobox', { name: 'Project team', exact: true });
  await teamPicker.selectOption(saved.team.id);
  await expect(teamPicker).toHaveValue(saved.team.id);
  const selectedTeam = await teamPicker.inputValue();
  await page.getByRole('button', { name: 'New project', exact: true }).click();
  const create = page.getByRole('dialog', { name: 'New project', exact: true });
  await create.getByRole('textbox', { name: 'Name', exact: true }).fill(name);
  await create.getByRole('textbox', { name: 'Summary', exact: true }).fill('A saved deliverable with its own scope');
  await create.getByLabel('Start date', { exact: true }).fill('2026-11-02');
  await create.getByLabel('Target date', { exact: true }).fill('2026-11-20');
  await create.getByRole('button', { name: /^Project lead/ }).click();
  await readiness.probe(create, 'before_click');
  try {
    await create.getByRole('listbox', { name: 'Project lead', exact: true }).getByRole('option', { name: /Example Owner/ }).click();
  } catch (error) {
    readiness.failure(error); await readiness.probe(create, 'after_failed_click'); throw error;
  } finally { readiness.stop(); }
  const brief = create.getByRole('textbox', { name: 'Project brief', exact: true });
  await brief.fill(briefText);
  await brief.press('Control+a');
  await create.getByRole('toolbar', { name: 'Project brief formatting' }).getByRole('button', { name: 'Bold', exact: true }).click();
  const created = page.waitForResponse(response => response.request().method() === 'POST'
    && response.url() === `${saved.origin}${saved.path}/teams/${selectedTeam}/projects`);
  await create.getByRole('button', { name: 'Create project', exact: true }).click();
  const response = await created;
  expect(response.status()).toBe(201);
  expect(response.url()).toBe(`${saved.origin}${saved.path}${projectsPath(saved)}`);
  const project = await response.json() as FullProject;
  expect(project.team_id).toBe(saved.team.id);
  await expect(create).toHaveCount(0);
  await expect(page.getByRole('heading', { name, exact: true })).toBeVisible();
  await expect(page.getByText('No eligible issue scope', { exact: true })).toBeVisible();
  await expect(page.getByRole('progressbar', { name: 'Project issue completion' })).toHaveCount(0);
  const persisted = await saved.request<FullProject>(projectsPath(saved, project.id));
  expect(persisted).toMatchObject({ name, lead_id: saved.session.user.id, start_date: '2026-11-02',
    target_date: '2026-11-20', state: 'planned', health: 'on_track' });
  expect(persisted.narrative.body).toEqual({ type: 'doc', content: [{ type: 'paragraph',
    content: [{ type: 'text', text: briefText, marks: [{ type: 'bold' }] }] }] });
  const originalBrief = await briefDocument(saved, project.id);
  const originalMetadata = await saved.request<Project>(`${projectsPath(saved, project.id)}/metadata`);
  expect(originalBrief.body).toEqual(persisted.narrative.body);

  await page.getByRole('button', { name: 'Edit project', exact: true }).click();
  const edit = page.getByRole('dialog', { name: 'Edit project', exact: true });
  await expect(edit.getByRole('textbox', { name: 'Project brief', exact: true })).toHaveCount(0);
  await edit.getByRole('combobox', { name: 'Project state', exact: true }).selectOption('active');
  await edit.getByRole('combobox', { name: 'Project health', exact: true }).selectOption('at_risk');
  await edit.getByRole('button', { name: 'Save project', exact: true }).click();
  await expect(edit).toHaveCount(0);
  await page.reload();
  const properties = page.getByRole('complementary', { name: 'Project properties', exact: true });
  await expect(properties).toContainText('Active');
  await expect(properties).toContainText('At risk');
  await expect(properties).toContainText('Example Owner');
  await expect(properties).toContainText('Nov 2, 2026');
  await expect(properties).toContainText('Nov 20, 2026');
  await expect(page.locator('.project-brief strong')).toHaveText(briefText);
  await expect(page.getByText('No eligible issue scope', { exact: true })).toBeVisible();
  const changedMetadata = await saved.request<Project>(`${projectsPath(saved, project.id)}/metadata`);
  expect(changedMetadata).toMatchObject({ state: 'active', health: 'at_risk' });
  expect(changedMetadata.etag).not.toBe(originalMetadata.etag);
  expect(await briefDocument(saved, project.id)).toEqual(originalBrief);
  const liveBrief = page.getByRole('region', { name: 'Project brief', exact: true }).getByRole('textbox', { name: 'Project brief', exact: true });
  await liveBrief.fill('A separately saved project brief'); await liveBrief.press('Control+Enter');
  const editedBody = { type: 'doc', content: [{ type: 'paragraph', content: [{ type: 'text', text: 'A separately saved project brief', marks: [{ type: 'bold' }] }] }] };
  await expect.poll(async () => (await briefDocument(saved, project.id)).body).toEqual(editedBody);
  const changedBrief = await briefDocument(saved, project.id);
  expect(changedBrief.version).toBe((BigInt(originalBrief.version) + 1n).toString());
  expect(await saved.request<Project>(`${projectsPath(saved, project.id)}/metadata`)).toEqual(changedMetadata);
  const full = await saved.request<FullProject>(projectsPath(saved, project.id));
  expect(full.narrative.body).toEqual(changedBrief.body); expect(full.narrative_version).toBe(changedBrief.version); expect(full.metadata_etag).toBe(changedMetadata.etag);
  await page.reload(); await expect(liveBrief).toHaveText('A separately saved project brief'); await expect(properties).toContainText('At risk');
});

test('project scope excludes canceled, archived and deleted work while completion and updates preserve issue facts', async ({ page, saved }) => {
  let project = await createProject(saved, 'Scope acceptance');
  const path = projectsPath(saved, project.id);
  project = await saved.request<FullProject>(path, 'PATCH', { state: 'active', health: 'at_risk' }, { 'If-Match': project.etag });
  const [open, done, canceled, archived, deleted] = await Promise.all(
    ['Open', 'Done', 'Canceled', 'Archived', 'Deleted'].map(label => saved.create(`${label} scope work ${randomUUID()}`)));
  await openProject(page, saved, project);
  await page.getByRole('navigation', { name: 'Project sections' }).getByRole('button', { name: 'Issues', exact: true }).click();
  await page.getByRole('button', { name: 'Add issues', exact: true }).click();
  const add = page.getByRole('dialog', { name: 'Add an issue to this project', exact: true });
  await add.getByRole('textbox', { name: 'Find an issue', exact: true }).fill(open.title);
  await add.getByRole('button').filter({ hasText: open.title }).click();
  await expect(add).toHaveCount(0);
  await expect(page.getByRole('region', { name: 'Project issues' }).getByRole('link').filter({ hasText: open.title })).toBeVisible();
  project = await saved.request<FullProject>(path);
  for (const issue of [done, canceled, archived, deleted]) {
    project = await saved.request<FullProject>(`${path}/issues/${issue.id}`, 'PUT', { included: true }, { 'If-Match': project.etag });
  }
  const doneStatus = saved.data.statuses.find(status => status.team_id === saved.team.id && status.category === 'completed')!;
  const canceledStatus = saved.data.statuses.find(status => status.team_id === saved.team.id && status.category === 'canceled')!;
  await saved.patch(await saved.issue(done.id), { status_id: doneStatus.id });
  await saved.patch(await saved.issue(canceled.id), { status_id: canceledStatus.id });
  await saved.patch(await saved.issue(archived.id), { archived: true });
  const currentDeleted = await saved.issue(deleted.id);
  const deletion = await saved.http.delete(`${saved.path}/issues/${deleted.id}`, {
    headers: { Origin: saved.origin, 'X-CSRF-Token': saved.session.csrf_token, 'If-Match': currentDeleted.etag },
  });
  expect(deletion.status()).toBe(204);

  await page.reload();
  await expect(page.getByText('50% complete', { exact: true })).toBeVisible();
  await expect(page.getByText('1 / 2 issues', { exact: true })).toBeVisible();
  await expect(page.getByText('1 canceled · excluded', { exact: true })).toBeVisible();
  await expect(page.getByRole('progressbar', { name: 'Project issue completion' })).toHaveAttribute('value', '1');
  await expect(page.getByRole('progressbar', { name: 'Project issue completion' })).toHaveAttribute('max', '2');
  const counts = await saved.request<ProjectProgress>(`${path}/progress`);
  expect(counts).toMatchObject({ completed: 1, eligible: 2, canceled: 1, open: 1, total: 3, unscoped: false });
  expect(Number.isFinite(Date.parse(counts.as_of))).toBe(true);
  const scope = await saved.request<{ data: ProjectIssue[] }>(`${path}/issues`);
  expect(scope.data.map(issue => issue.id).sort()).toEqual([open.id, done.id, canceled.id].sort());

  await page.getByRole('navigation', { name: 'Project sections' }).getByRole('button', { name: 'Updates', exact: true }).click();
  const updates = page.getByRole('region', { name: 'Project updates', exact: true });
  const updateText = 'Reviewed scope: one remaining issue, one canceled item excluded.';
  await updates.getByRole('textbox', { name: 'Write an update', exact: true }).fill(updateText);
  await updates.getByRole('button', { name: 'Publish update', exact: true }).click();
  await expect(updates.getByRole('status')).toHaveText('Update published.');
  const snapshots = await saved.request<{ data: ProjectUpdate[] }>(`${path}/updates`);
  expect(snapshots.data).toHaveLength(1);
  expect(snapshots.data[0]).toMatchObject({ body: updateText, author_id: saved.session.user.id,
    author_name: 'Example Owner', state: 'active', health: 'at_risk', completed: 1, eligible: 2 });

  const beforeCompletion = await Promise.all([open, done, canceled, archived].map(issue => saved.issue(issue.id)));
  await page.getByRole('button', { name: 'Edit project', exact: true }).click();
  const edit = page.getByRole('dialog', { name: 'Edit project', exact: true });
  await edit.getByRole('combobox', { name: 'Project state', exact: true }).selectOption('completed');
  await expect(edit.getByRole('button', { name: 'Save project', exact: true })).toBeDisabled();
  await edit.getByRole('checkbox', { name: 'I understand that completing this project leaves its issues in their current states.', exact: true }).check();
  await edit.getByRole('button', { name: 'Save project', exact: true }).click();
  await expect(edit).toHaveCount(0);
  const afterCompletion = await Promise.all([open, done, canceled, archived].map(issue => saved.issue(issue.id)));
  expect(afterCompletion).toEqual(beforeCompletion);
  project = await saved.request<FullProject>(path);
  expect(project.state).toBe('completed');

  await saved.patch(await saved.issue(open.id), { status_id: doneStatus.id });
  await saved.request<FullProject>(path, 'PATCH', { health: 'off_track' }, { 'If-Match': project.etag });
  await page.reload();
  await expect(page.getByText('100% complete', { exact: true })).toBeVisible();
  await expect(page.getByRole('complementary', { name: 'Project properties' })).toContainText('Off track');
  await page.getByRole('navigation', { name: 'Project sections' }).getByRole('button', { name: 'Updates', exact: true }).click();
  const entry = updates.getByRole('article').filter({ hasText: updateText });
  await expect(entry).toContainText('Example Owner');
  await expect(entry).toContainText('Active');
  await expect(entry).toContainText('At risk');
  await expect(entry).toContainText('1 of 2 completed');
  expect((await saved.request<{ data: ProjectUpdate[] }>(`${path}/updates`)).data).toEqual(snapshots.data);
});

test('stale project metadata drafts are reviewed without overwriting an independently saved brief', async ({ page, saved }) => {
  const project = await createProject(saved, 'Concurrent project');
  const path = projectsPath(saved, project.id);
  await openProject(page, saved, project);
  await page.getByRole('button', { name: 'Edit project', exact: true }).click();
  const edit = page.getByRole('dialog', { name: 'Edit project', exact: true });
  await edit.getByRole('textbox', { name: 'Summary', exact: true }).fill('Reviewed local summary');
  await expect(edit.getByRole('textbox', { name: 'Project brief', exact: true })).toHaveCount(0);
  const concurrent = await saved.request<FullProject>(path, 'PATCH', { summary: 'A separately saved summary',
    narrative: { schema_version: 1, body: paragraph('A separately saved brief') } }, { 'If-Match': project.etag });
  const stale = page.waitForResponse(response => response.request().method() === 'PATCH'
    && response.url() === `${saved.origin}${saved.path}${path}/metadata`);
  await edit.getByRole('button', { name: 'Save project', exact: true }).click();
  expect((await stale).status()).toBe(412);
  await expect(edit.getByRole('heading', { name: 'This project changed.', exact: true })).toBeVisible();
  await expect(edit).toContainText('A separately saved summary');
  await expect(edit.getByRole('textbox', { name: 'Summary', exact: true })).toHaveValue('Reviewed local summary');
  await expect(edit.getByRole('button', { name: 'Save project', exact: true })).toBeDisabled();
  expect(await saved.request<FullProject>(path)).toEqual(concurrent);
  await edit.getByRole('button', { name: 'Keep my draft for review', exact: true }).click();
  await expect(edit.getByRole('heading', { name: 'This project changed.', exact: true })).toHaveCount(0);
  expect(await saved.request<FullProject>(path)).toEqual(concurrent);
  await edit.getByRole('button', { name: 'Save project', exact: true }).click();
  await expect(edit).toHaveCount(0);
  await page.reload();
  await expect(page.getByText('Reviewed local summary', { exact: true })).toBeVisible();
  await expect(page.getByRole('region', { name: 'Project brief', exact: true }).getByRole('textbox', { name: 'Project brief', exact: true })).toHaveText('A separately saved brief');
  expect(await saved.request<FullProject>(path)).toMatchObject({ summary: 'Reviewed local summary',
    narrative: { body: paragraph('A separately saved brief') } });
});

test('competing project brief editors retain the rejected draft until an explicit reviewed save', async ({ page, context, saved }) => {
  const project = await createProject(saved, 'Concurrent project briefs'); const other = await context.newPage();
  let release!: () => void; const gate = new Promise<void>(resolve => { release = resolve; }); let submitted = 0;
  try {
    await openProject(page, saved, project); await openProject(other, saved, project);
    const original = await briefDocument(saved, project.id); const metadata = await saved.request<Project>(`${projectsPath(saved, project.id)}/metadata`);
    const endpoint = `${saved.origin}${saved.path}/documents/${original.id}`;
    for (const client of [page, other]) await client.route(endpoint, async route => {
      if (route.request().method() === 'PATCH') { submitted += 1; if (submitted === 2) release(); await gate; }
      await route.continue();
    });
    const drafts = ['First reviewed project brief', 'Second reviewed project brief'];
    const editors = [page, other].map(client => client.getByRole('region', { name: 'Project brief', exact: true }).getByRole('textbox', { name: 'Project brief', exact: true }));
    const responses = [page, other].map(client => client.waitForResponse(response => response.url() === endpoint && response.request().method() === 'PATCH'));
    await Promise.all(editors.map((editor, index) => editor.fill(drafts[index])));
    await Promise.all(editors.map(editor => editor.press('Control+Enter')));
    const replies = await Promise.all(responses); expect(replies.map(response => response.status()).sort()).toEqual([200, 412]);
    const winner = await replies[replies[0].status() === 200 ? 0 : 1].json() as StructuredDocument;
    const rejectedIndex = replies[0].status() === 412 ? 0 : 1; const rejected = [page, other][rejectedIndex];
    const conflict = rejected.getByRole('alert', { name: 'Document conflict', exact: true });
    await expect(conflict).toContainText(drafts[0]); await expect(conflict).toContainText(drafts[1]);
    await expect(editors[rejectedIndex]).toHaveText(drafts[rejectedIndex]);
    const save = rejected.getByRole('region', { name: 'Project brief', exact: true }).getByRole('button', { name: 'Save text now', exact: true });
    await expect(save).toBeDisabled(); expect((await briefDocument(saved, project.id)).body).toEqual(winner.body);
    await rejected.getByRole('region', { name: 'Project brief', exact: true }).getByRole('button', { name: 'Brief history', exact: true }).click();
    const history = rejected.getByRole('dialog', { name: 'Brief history', exact: true });
    await history.getByRole('button', { name: `View revision ${winner.version}`, exact: true }).click();
    await expect(history.getByRole('region', { name: `Revision ${winner.version}`, exact: true })).toContainText(drafts[1 - rejectedIndex]);
    await history.getByRole('button', { name: 'Done', exact: true }).click();
    await expect(editors[rejectedIndex]).toHaveText(drafts[rejectedIndex]); expect((await briefDocument(saved, project.id)).body).toEqual(winner.body);
    await conflict.getByRole('button', { name: 'Keep my draft for review', exact: true }).click();
    await expect(conflict).toHaveCount(0); expect((await briefDocument(saved, project.id)).body).toEqual(winner.body);
    await save.focus(); await save.press('Enter');
    await expect.poll(async () => (await briefDocument(saved, project.id)).body).toEqual(paragraph(drafts[rejectedIndex]));
    expect((await briefDocument(saved, project.id)).version).toBe((BigInt(original.version) + 2n).toString());
    expect(await saved.request<Project>(`${projectsPath(saved, project.id)}/metadata`)).toEqual(metadata);
    for (const [index, client] of [page, other].entries()) { await client.reload(); await expect(editors[index]).toHaveText(drafts[rejectedIndex]); }
  } finally { release(); await other.close(); }
});

test('project brief references and file bytes persist while revision selection remains read-only and keyboard accessible', async ({ page, saved: owner, playwright }) => {
  const team = await owner.request<Team>('/teams', 'POST', { name: `Project mention selection ${randomUUID()}`, key: `P${randomUUID().replaceAll('-', '').slice(0, 8).toUpperCase()}`, private: true }, { 'Idempotency-Key': randomUUID() });
  const saved = new SavedWorkspace(owner.http, owner.session, { ...owner.data, teams: [team] }, owner.origin, owner.workspace);
  const invitation = await saved.request<{ token: string }>('/invitations', 'POST', { email: `project-mention-${randomUUID()}@example.test`, role: 'member', team_ids: [team.id] }, { 'Idempotency-Key': randomUUID() });
  const member = await playwright.request.newContext({ baseURL: saved.origin, extraHTTPHeaders: { Origin: saved.origin }, storageState: { cookies: [], origins: [] } });
  try {
    const accepted = await member.post('/api/v1/invitations/accept', { data: { token: invitation.token, display_name: 'Example A Owner', password: process.env.OPENTRACK_BROWSER_PASSWORD } });
    expect(accepted.status()).toBe(200);
  } finally { await member.dispose(); }
  const project = await createProject(saved, 'Connected project brief'); const target = await saved.create(`Project reference target ${randomUUID()}`);
  await openProject(page, saved, project);
  const brief = page.getByRole('region', { name: 'Project brief', exact: true }); const editor = brief.getByRole('textbox', { name: 'Project brief', exact: true });
  await editor.fill('Earlier reviewed project brief'); await editor.press('Control+Enter');
  await expect.poll(async () => (await briefDocument(saved, project.id)).body).toEqual(paragraph('Earlier reviewed project brief'));
  const earlier = await briefDocument(saved, project.id);
  await editor.fill('Review with '); await editor.press('End'); await editor.pressSequentially('@Owner');
  const people = brief.getByRole('listbox', { name: 'People to mention', exact: true }); const intendedOwner = people.getByRole('option', { name: 'Example Owner', exact: true });
  await expect(people).toHaveAttribute('aria-busy', 'false'); await expect(people.getByRole('option')).toHaveCount(2);
  await expect(intendedOwner).toBeVisible(); await expect(people.getByRole('option', { name: 'Example A Owner', exact: true })).toHaveAttribute('aria-selected', 'true');
  const candidateCount = await people.getByRole('option').count();
  for (let attempt = 0; attempt < candidateCount && await intendedOwner.getAttribute('aria-selected') !== 'true'; attempt += 1) await editor.press('ArrowDown');
  await expect(intendedOwner).toHaveAttribute('aria-selected', 'true');
  await editor.press('Enter'); await editor.pressSequentially(`and #${target.key}`);
  await expect(brief.getByRole('listbox', { name: 'Issues to reference', exact: true }).getByRole('option').filter({ hasText: target.key })).toBeVisible();
  await editor.press('Enter'); await editor.press('Control+Enter');
  await expect.poll(async () => (await briefDocument(saved, project.id)).mentions?.some(value => value.id === saved.session.user.id)).toBe(true);
  await expect.poll(async () => (await briefDocument(saved, project.id)).issue_references?.some(value => value.id === target.id)).toBe(true);
  const bytes = Buffer.from('Reviewed project attachment\n'); const filename = 'project-review-notes.txt';
  const reserved = page.waitForResponse(response => response.request().method() === 'POST' && response.url() === `${saved.origin}${saved.path}/attachments`);
  const chooser = page.waitForEvent('filechooser'); await brief.getByRole('button', { name: 'Attach files', exact: true }).click();
  await (await chooser).setFiles({ name: filename, mimeType: 'text/plain', buffer: bytes });
  const attachment = await (await reserved).json() as { id: string };
  await expect(brief.getByRole('button', { name: `Download ${filename}`, exact: true })).toBeVisible();
  await expect.poll(async () => JSON.stringify((await briefDocument(saved, project.id)).body)).toContain(attachment.id);
  await page.reload();
  await expect(brief.getByRole('link', { name: `${target.key}: ${target.title}`, exact: true })).toBeVisible(); await expect(brief).toContainText('@Example Owner');
  const downloaded = page.waitForEvent('download'); await brief.getByRole('button', { name: `Download ${filename}`, exact: true }).click();
  const download = await downloaded; expect(download.suggestedFilename()).toBe(filename);
  const stream = await download.createReadStream(); expect(stream).not.toBeNull(); const chunks: Buffer[] = [];
  for await (const chunk of stream!) chunks.push(Buffer.from(chunk)); expect(Buffer.concat(chunks)).toEqual(bytes);
  const current = await briefDocument(saved, project.id); const mutations: string[] = [];
  page.on('request', request => { if (request.url().startsWith(`${saved.origin}${saved.path}/documents/${current.id}`) && request.method() !== 'GET') mutations.push(request.method()); });
  await page.setViewportSize({ width: 390, height: 844 });
  await brief.getByRole('button', { name: 'Brief history', exact: true }).focus(); await page.keyboard.press('Enter');
  const history = page.getByRole('dialog', { name: 'Brief history', exact: true });
  const oldRevision = history.getByRole('button', { name: `View revision ${earlier.version}`, exact: true });
  await expect(oldRevision).toContainText('Example Owner'); await expect(oldRevision).not.toContainText('Migrated snapshot');
  await oldRevision.focus(); await oldRevision.press('Enter');
  const oldPreview = history.getByRole('region', { name: `Revision ${earlier.version}`, exact: true });
  await expect(oldPreview).toContainText('Earlier reviewed project brief'); await expect(oldPreview.getByRole('textbox')).toHaveCount(0);
  const latestRevision = history.getByRole('button', { name: `View revision ${current.version}`, exact: true });
  await latestRevision.focus(); await latestRevision.press('Enter');
  const latestPreview = history.getByRole('region', { name: `Revision ${current.version}`, exact: true });
  await expect(latestPreview.getByRole('link', { name: `${target.key}: ${target.title}`, exact: true })).toBeVisible(); await expect(latestPreview).toContainText('@Example Owner');
  await expect(history.getByRole('textbox')).toHaveCount(0); expect(await page.evaluate(() => document.documentElement.scrollWidth - innerWidth)).toBeLessThanOrEqual(1);
  await history.getByRole('button', { name: 'Done', exact: true }).focus(); await page.keyboard.press('Enter'); await expect(history).toHaveCount(0);
  expect(mutations).toEqual([]); expect(await briefDocument(saved, project.id)).toEqual(current); await expect(brief.getByRole('link', { name: `${target.key}: ${target.title}`, exact: true })).toBeVisible();
});
