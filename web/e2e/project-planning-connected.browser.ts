import { randomUUID } from 'node:crypto';
import type { Locator, Page } from '@playwright/test';
import { test, expect, SavedWorkspace } from './fixtures';
import type { Issue, Team } from '../src/types';
import type { Workflow } from '../src/workflows/types';
import type { FullProject, Page as ResultPage, ProjectIssue, ProjectProgress, ProjectUpdate } from '../src/projects/types';
import type { ProjectScopeSnapshot } from '../src/projects/ProjectScopeHistory';

async function keyboardActivate(control: Locator) {
  await expect(control).toBeEnabled();
  await control.focus();
  await expect(control).toBeFocused();
  await control.press('Enter');
}

async function section(page: Page, name: 'Overview' | 'Issues' | 'Updates' | 'Scope history') {
  await keyboardActivate(page.getByRole('navigation', { name: 'Project sections', exact: true }).getByRole('button', { name, exact: true }));
}

async function write<T>(page: Page, saved: SavedWorkspace, control: Locator, path: string, method: string, status: number) {
  const pending = page.waitForResponse(response => response.url() === `${saved.origin}${saved.path}${path}` && response.request().method() === method);
  await keyboardActivate(control);
  const response = await pending;
  expect(response.status(), 'The real API accepts the reviewed planning change').toBe(status);
  return response.json() as Promise<T>;
}

async function addIssue(page: Page, issue: Issue) {
  await section(page, 'Issues');
  await keyboardActivate(page.getByRole('region', { name: 'Project issues', exact: true }).getByRole('button', { name: 'Add issues', exact: true }));
  const dialog = page.getByRole('dialog', { name: 'Add an issue to this project', exact: true });
  await dialog.getByRole('textbox', { name: 'Find an issue', exact: true }).fill(issue.title);
  await keyboardActivate(dialog.getByRole('button').filter({ hasText: issue.title }));
  await expect(dialog).toHaveCount(0);
  await expect(page.getByRole('region', { name: 'Project issues', exact: true }).getByRole('link').filter({ hasText: issue.title })).toBeVisible();
}

async function baseline(page: Page, saved: SavedWorkspace, path: string, label: string, reason: string) {
  const form = page.getByRole('form', { name: 'Set scope baseline', exact: true });
  await form.getByRole('textbox', { name: /^Baseline name\b/ }).fill(label);
  await form.getByRole('textbox', { name: /^Baseline reason\b/ }).fill(reason);
  const submit = form.getByRole('button', { name: 'Set baseline', exact: true });
  await expect(submit).toBeDisabled();
  const agreement = form.getByRole('checkbox', { name: 'I agree the current project scope as a baseline.', exact: true });
  await agreement.focus();
  await agreement.press('Space');
  const snapshot = await write<ProjectScopeSnapshot>(page, saved, submit, `${path}/scope-baselines`, 'POST', 201);
  await expect(page.getByRole('region', { name: 'Baseline comparison', exact: true })).toContainText(reason);
  expect(snapshot.baseline).toMatchObject({ label, reason, author_id: saved.session.user.id, author_name: 'Example Owner' });
  return snapshot;
}

test('agreed project scope, explained changes and authored update context remain connected after later planning and reload', async ({ page, saved: owner }) => {
  const token = randomUUID();
  const team = await owner.request<Team>('/teams', 'POST', { name: `Connected planning ${token}`, key: `P${token.replaceAll('-', '').slice(0, 8).toUpperCase()}`, private: true }, { 'Idempotency-Key': randomUUID() });
  const saved = new SavedWorkspace(owner.http, owner.session, { ...owner.data, teams: [team] }, owner.origin, owner.workspace);
  const workflow = await saved.request<Workflow>(`/teams/${team.id}/workflow`);
  const completed = workflow.statuses.find(status => status.category === 'completed');
  expect(completed).toBeDefined();
  const delivered = await saved.create(`Deliver agreed result ${token}`);
  await saved.patch(delivered, { status_id: completed!.id });
  const postponed = await saved.create(`Review deferred dependency ${token}`);
  const removed = await saved.create(`Remove superseded approach ${token}`);
  const replacement = await saved.create(`Validate replacement approach ${token}`);

  const directory = `/teams/${team.id}/projects`;
  await page.goto(`/w/${saved.workspace}/projects?team_id=${team.id}`);
  await keyboardActivate(page.getByRole('button', { name: 'New project', exact: true }));
  const create = page.getByRole('dialog', { name: 'New project', exact: true });
  const name = `Connected delivery plan ${token}`;
  await create.getByRole('textbox', { name: 'Name', exact: true }).fill(name);
  await create.getByRole('textbox', { name: 'Summary', exact: true }).fill('Deliver the reviewed scope while keeping dependency decisions visible.');
  await create.getByLabel('Start date', { exact: true }).fill('2026-11-01');
  await create.getByLabel('Target date', { exact: true }).fill('2026-11-20');
  await create.getByRole('textbox', { name: 'Project brief', exact: true }).fill('Agree the deliverable, record scope changes, then review the replacement.');
  const project = await write<FullProject>(page, saved, create.getByRole('button', { name: 'Create project', exact: true }), directory, 'POST', 201);
  const path = `${directory}/${project.id}`;
  await expect(create).toHaveCount(0);
  await expect(page.getByRole('heading', { name, exact: true })).toBeVisible();
  for (const issue of [delivered, postponed, removed]) await addIssue(page, issue);

  await section(page, 'Scope history');
  const scope = page.getByRole('region', { name: 'Project scope history', exact: true });
  await expect(scope.getByRole('heading', { name: 'No baseline yet', exact: true })).toBeVisible();
  const initial = await baseline(page, saved, path, 'Agreed delivery scope', 'The team reviewed these three issues before committing to the target date.');
  expect(initial.current.map(issue => issue.id).sort()).toEqual([delivered.id, postponed.id, removed.id].sort());
  expect(initial.comparison).toEqual({ unchanged: 3, added: [], removed: [], postponed: [] });
  const postponementReason = 'The dependency needs a separate review before it can return to this plan.';
  const removalReason = 'The replacement approach supersedes this work; preserve the issue for reference.';
  for (const [issue, action, reason] of [[postponed, 'postponed', postponementReason], [removed, 'removed', removalReason]] as const) {
    const change = scope.getByRole('form', { name: 'Record scope change', exact: true });
    await change.getByRole('combobox', { name: 'Project issue', exact: true }).selectOption(issue.id);
    await change.getByRole('combobox', { name: 'Change', exact: true }).selectOption(action);
    const submit = change.getByRole('button', { name: 'Record scope change', exact: true });
    await expect(submit).toBeDisabled();
    await change.getByRole('textbox', { name: /^Change reason\b/ }).fill(reason);
    await write<ProjectScopeSnapshot>(page, saved, submit, `${path}/scope-changes`, 'POST', 200);
    await expect(scope.getByRole('region', { name: action === 'postponed' ? 'Postponed since agreement' : 'Removed since agreement', exact: true })).toContainText(reason);
    expect((await saved.issue(issue.id)).status_id, 'Scope changes preserve the independent issue workflow').toBe(issue.status_id);
  }
  const history = await saved.request<ProjectScopeSnapshot>(`${path}/scope-history?limit=25&baseline_id=${initial.baseline!.id}`);
  expect(history.events.data).toHaveLength(2);
  expect(history.events.data.map(event => [event.action, event.reason]).sort()).toEqual([['postponed', postponementReason], ['removed', removalReason]].sort());

  await section(page, 'Overview');
  const progress = page.locator('.project-progress-section');
  await expect(progress).toContainText('1 / 1 issues');
  await expect(progress.getByRole('progressbar', { name: 'Project issue completion', exact: true })).toHaveAttribute('max', '1');
  expect((await saved.request<ResultPage<ProjectIssue>>(`${path}/issues`)).data.map(issue => issue.id)).toEqual([delivered.id]);
  expect(await saved.request<ProjectProgress>(`${path}/progress`)).toMatchObject({ completed: 1, eligible: 1, open: 0, total: 1 });

  await section(page, 'Updates');
  const updates = page.getByRole('region', { name: 'Project updates', exact: true });
  const firstBody = 'The agreed result is delivered. The dependency is postponed and the superseded work is removed with reasons.';
  const riskTitle = 'Dependency review could delay the replacement';
  const decisionTitle = 'Approve the replacement approach';
  await updates.getByRole('textbox', { name: 'Write an update', exact: true }).fill(firstBody);
  await updates.getByRole('checkbox', { name: 'Record reviewed risks and decisions in this update', exact: true }).check();
  await keyboardActivate(updates.getByRole('button', { name: 'Add risk', exact: true }));
  await expect(updates.getByRole('textbox', { name: 'Risk 1 title', exact: true })).toBeFocused();
  await updates.getByRole('textbox', { name: 'Risk 1 title', exact: true }).fill(riskTitle);
  await updates.getByRole('textbox', { name: 'Risk 1 details', exact: true }).fill(postponementReason);
  await keyboardActivate(updates.getByRole('button', { name: 'Add decision', exact: true }));
  await expect(updates.getByRole('textbox', { name: 'Decision 1 title', exact: true })).toBeFocused();
  await updates.getByRole('textbox', { name: 'Decision 1 title', exact: true }).fill(decisionTitle);
  await updates.getByRole('textbox', { name: 'Decision 1 details', exact: true }).fill('Review the replacement before extending the target date.');
  const first = await write<ProjectUpdate>(page, saved, updates.getByRole('button', { name: 'Publish update', exact: true }), `${path}/updates`, 'POST', 201);
  expect(first).toMatchObject({ body: firstBody, author_id: saved.session.user.id, completed: 1, eligible: 1, state: 'planned', health: 'on_track', context: { start_date: '2026-11-01', target_date: '2026-11-20', notes: { risks: [{ title: riskTitle, status: 'open' }], decisions: [{ title: decisionTitle, status: 'needed' }] } } });
  await section(page, 'Overview');
  const latest = page.getByRole('region', { name: 'Latest project update', exact: true });
  await expect(latest).toContainText(firstBody);
  await expect(latest.getByRole('region', { name: 'Open risks in latest update', exact: true })).toContainText(riskTitle);
  await expect(latest.getByRole('region', { name: 'Decisions needed in latest update', exact: true })).toContainText(decisionTitle);

  await addIssue(page, replacement);
  await keyboardActivate(page.getByRole('button', { name: 'Edit project', exact: true }));
  const edit = page.getByRole('dialog', { name: 'Edit project', exact: true });
  await edit.getByRole('combobox', { name: 'Project state', exact: true }).selectOption('active');
  await edit.getByRole('combobox', { name: 'Project health', exact: true }).selectOption('at_risk');
  await edit.getByLabel('Target date', { exact: true }).fill('2026-11-27');
  await keyboardActivate(edit.getByRole('button', { name: 'Save project', exact: true }));
  await expect(edit).toHaveCount(0);
  await section(page, 'Scope history');
  const revised = await baseline(page, saved, path, 'Reviewed replacement scope', 'The replacement has been accepted with a later target date.');
  expect(revised.baseline!.id).not.toBe(initial.baseline!.id);
  expect(revised.comparison).toEqual({ unchanged: 2, added: [], removed: [], postponed: [] });

  await section(page, 'Updates');
  const secondBody = 'The replacement is accepted with a revised target. The risk is mitigated and the approach is approved.';
  await updates.getByRole('textbox', { name: 'Write an update', exact: true }).fill(secondBody);
  await updates.getByRole('checkbox', { name: 'Record reviewed risks and decisions in this update', exact: true }).check();
  await expect(updates.getByRole('textbox', { name: 'Risk 1 title', exact: true })).toHaveValue(riskTitle);
  await expect(updates.getByRole('textbox', { name: 'Decision 1 title', exact: true })).toHaveValue(decisionTitle);
  await updates.getByRole('combobox', { name: 'Risk 1 status', exact: true }).selectOption('mitigated');
  await updates.getByRole('combobox', { name: 'Decision 1 status', exact: true }).selectOption('decided');
  const second = await write<ProjectUpdate>(page, saved, updates.getByRole('button', { name: 'Publish update', exact: true }), `${path}/updates`, 'POST', 201);
  expect(second).toMatchObject({ completed: 1, eligible: 2, state: 'active', health: 'at_risk', context: { target_date: '2026-11-27', notes: { risks: [{ id: first.context!.notes!.risks[0].id, status: 'mitigated' }], decisions: [{ id: first.context!.notes!.decisions[0].id, status: 'decided' }] } } });

  await page.reload();
  await expect(latest).toContainText(secondBody);
  await expect(latest).not.toContainText(firstBody);
  await expect(latest.getByRole('region', { name: 'Open risks in latest update', exact: true })).toContainText('No open risks recorded in this update.');
  await expect(latest.getByRole('region', { name: 'Decisions needed in latest update', exact: true })).toContainText('No decisions awaiting action recorded in this update.');
  await expect(page.getByRole('region', { name: 'Project brief', exact: true })).toContainText('Agree the deliverable, record scope changes, then review the replacement.');
  await expect(page.getByRole('complementary', { name: 'Project properties', exact: true })).toContainText('Nov 27, 2026');
  await expect(progress).toContainText('50% complete');
  await expect(progress).toContainText('1 / 2 issues');
  await expect(progress.getByRole('progressbar', { name: 'Project issue completion', exact: true })).toHaveAttribute('value', '1');
  await expect(progress.getByRole('progressbar', { name: 'Project issue completion', exact: true })).toHaveAttribute('max', '2');
  const counts = await saved.request<ProjectProgress>(`${path}/progress`);
  expect(counts).toMatchObject({ completed: 1, eligible: 2, open: 1, canceled: 0, total: 2, unscoped: false });
  const membership = await saved.request<ResultPage<ProjectIssue>>(`${path}/issues`);
  expect(membership.has_more).toBe(false);
  expect(membership.data.map(issue => issue.id).sort()).toEqual([delivered.id, replacement.id].sort());
  await section(page, 'Issues');
  const issues = page.getByRole('region', { name: 'Project issues', exact: true });
  await expect(issues.getByRole('link')).toHaveCount(2);
  for (const issue of [delivered, replacement]) await expect(issues.getByRole('link').filter({ hasText: issue.title })).toBeVisible();
  for (const issue of [postponed, removed]) await expect(issues.getByRole('link').filter({ hasText: issue.title })).toHaveCount(0);

  await section(page, 'Updates');
  const earlier = updates.getByRole('article').filter({ hasText: firstBody });
  await expect(earlier).toContainText('1 of 1 completed');
  await expect(earlier).toContainText('Nov 20, 2026');
  await expect(earlier.getByRole('region', { name: 'Risks recorded in update', exact: true })).toContainText('Open');
  await expect(earlier.getByRole('region', { name: 'Decisions recorded in update', exact: true })).toContainText('Decision needed');
  const recent = updates.getByRole('article').filter({ hasText: secondBody });
  await expect(recent).toContainText('Target date: Nov 20, 2026 → Nov 27, 2026');
  await expect(recent).toContainText('Risk changed: Dependency review could delay the replacement (Open → Mitigated).');
  await expect(recent).toContainText('Decision changed: Approve the replacement approach (Decision needed → Decided).');
  const persistedUpdates = await saved.request<ResultPage<ProjectUpdate>>(`${path}/updates`);
  expect(persistedUpdates.data.find(update => update.id === first.id)).toEqual(first);
  expect(persistedUpdates.data.find(update => update.id === second.id)).toEqual(second);

  await section(page, 'Scope history');
  await page.setViewportSize({ width: 390, height: 844 });
  const compare = scope.getByRole('combobox', { name: 'Compare with baseline', exact: true });
  await compare.selectOption(initial.baseline!.id);
  const comparison = scope.getByRole('region', { name: 'Baseline comparison', exact: true });
  await expect(comparison.getByRole('heading', { name: initial.baseline!.label, exact: true })).toBeVisible();
  await expect(comparison.getByRole('region', { name: 'Added since agreement', exact: true })).toContainText(replacement.title);
  await expect(comparison.getByRole('region', { name: 'Postponed since agreement', exact: true })).toContainText(postponementReason);
  await expect(comparison.getByRole('region', { name: 'Removed since agreement', exact: true })).toContainText(removalReason);
  const retained = await saved.request<ProjectScopeSnapshot>(`${path}/scope-history?limit=25&baseline_id=${initial.baseline!.id}`);
  expect(retained.baseline).toEqual(initial.baseline);
  expect(retained.comparison).toMatchObject({ unchanged: 1, added: [{ id: replacement.id }], removed: [{ id: removed.id, reason: removalReason }], postponed: [{ id: postponed.id, reason: postponementReason }] });
  for (const event of history.events.data) expect(retained.events.data.find(current => current.id === event.id)).toEqual(event);
  await expect(scope.getByRole('region', { name: 'Recorded scope changes', exact: true })).toContainText(postponementReason);
  expect(await page.evaluate(() => document.documentElement.scrollWidth - innerWidth), 'The comparison and consent controls fit a narrow viewport').toBeLessThanOrEqual(1);
  await compare.selectOption(revised.baseline!.id);
  await expect(comparison.getByRole('heading', { name: revised.baseline!.label, exact: true })).toBeVisible();
  await expect(comparison).toContainText('2 currently accessible issues remain in the plan.');
  await expect(comparison).toContainText('No changes among currently accessible issues.');
});
