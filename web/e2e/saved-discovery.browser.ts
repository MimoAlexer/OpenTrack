import { randomUUID } from 'node:crypto';
import { test, expect, paragraph } from './fixtures';
import { parseStructuredHash, viewHash, type StructuredView } from '../src/filters/model';
import type { components } from '../src/generated/api';
type Saved = components['schemas']['SavedDiscovery'];
const initial = (team: string): StructuredView => ({ version: 1, filter: { kind: 'predicate', field: 'priority', op: 'eq', value: 1 }, layout: 'list', sort: 'created', team_id: team, archived: false, limit: 50, timezone: 'UTC' });

test('saved view round-trips filters and favorite through rename and explicit deletion', async ({ page, saved }) => {
  const title = `View ${randomUUID()}`; const renamed = `${title} renamed`;
  const view = initial(saved.team.id);
  await page.goto(`/w/${saved.workspace}/issues${viewHash(view)}`);
  await page.locator('.discovery > summary').click();
  await page.getByLabel('Definition name', { exact: true }).fill(title);
  await page.getByRole('button', { name: 'Save new view', exact: true }).click();
  const directory = page.getByRole('region', { name: 'Saved views', exact: true });
  await expect(directory.getByRole('button', { name: title, exact: true })).toBeVisible();
  await directory.getByRole('button', { name: `Favorite ${title}`, exact: true }).click();
  await expect(directory.getByRole('button', { name: `Unfavorite ${title}`, exact: true })).toBeVisible();
  await directory.getByRole('button', { name: title, exact: true }).click();
  await page.getByLabel('Definition name', { exact: true }).fill(renamed);
  await page.getByRole('button', { name: 'Save changes', exact: true }).click();
  await expect(directory.getByRole('button', { name: renamed, exact: true })).toBeVisible();
  await page.reload(); await page.locator('.discovery > summary').click();
  await directory.getByRole('button', { name: renamed, exact: true }).click();
  expect(parseStructuredHash(new URL(page.url()).hash)).toEqual({ kind: 'valid', view });
  await expect(directory.getByRole('button', { name: `Unfavorite ${renamed}`, exact: true })).toBeVisible();
  await page.getByRole('button', { name: 'Delete saved definition', exact: true }).click();
  await expect(directory.getByRole('button', { name: renamed, exact: true })).toHaveCount(0);
});

test('full-text body search renders safe snippets and opens current issue detail', async ({ page, saved }) => {
  const term = `quasar${randomUUID().replaceAll('-', '')}`;
  const issue = await saved.create(`Search ${randomUUID()}`, paragraph(`Committed ${term} <img src=x onerror=alert(1)>`));
  await saved.open(page);
  await page.locator('.discovery > summary').click();
  await page.getByRole('searchbox', { name: 'Search issues and projects', exact: true }).fill(term);
  const results = page.locator('.discovery-results');
  await expect(results.getByRole('link', { name: `${issue.key} · ${issue.title}`, exact: true })).toBeVisible();
  await expect(results).toContainText('<img src=x onerror=alert(1)>');
  await expect(results.locator('img')).toHaveCount(0);
  await page.getByRole('searchbox', { name: 'Search issues and projects', exact: true }).fill(issue.key);
  await expect(results.getByRole('link').first()).toHaveText(`${issue.key} · ${issue.title}`);
  await results.getByRole('link').first().click();
  await expect(page.getByRole('textbox', { name: 'Issue title', exact: true })).toHaveValue(issue.title);
});

test('private saved search runs current body results and keeps text overrides unsaved until explicit save', async ({ page, saved }) => {
  const title = `Search definition ${randomUUID()}`; const term = `nebula${randomUUID().replaceAll('-', '')}`;
  const issue = await saved.create(`Saved search ${randomUUID()}`, paragraph(term));
  await saved.open(page); await page.locator('.discovery > summary').click();
  await page.getByLabel('Definition name', { exact: true }).fill(title);
  const input = page.getByRole('searchbox', { name: 'Search issues and projects', exact: true });
  await input.fill(term); await expect(page.locator('.discovery-results').getByRole('link', { name: `${issue.key} · ${issue.title}`, exact: true })).toBeVisible();
  await page.getByRole('button', { name: 'Save search', exact: true }).click();
  const directory=page.getByRole('region', { name: 'Saved searches', exact: true });
  await directory.getByRole('button', { name: title, exact: true }).click();
  await input.fill(`${term} changed`); await expect(page.locator('.discovery .notice').filter({ hasText: 'Unsaved overrides' })).toBeVisible();
  const current = await saved.request<{data:Saved[]}>('/saved-searches');const original=current.data.find(item=>item.name===title)!;
  expect(original.definition!.query.q).toBe(term);
  const run=await saved.request<components['schemas']['DiscoveryExecution']>(`/saved-searches/${original.id}/execute`,'POST',{});
  expect(run.search!.data.some(hit=>hit.id===issue.id)).toBe(true);
});

test('narrow keyboard save preserves draft after a remote version conflict', async ({ page, saved }) => {
  const title = `Conflict view ${randomUUID()}`;await page.setViewportSize({width:390,height:844});
  await saved.open(page);await page.locator('.discovery > summary').focus();await page.keyboard.press('Enter');
  await page.getByLabel('Definition name', { exact: true }).fill(title);await page.getByRole('button', { name:'Save new view',exact:true }).click();
  await page.getByRole('region',{name:'Saved views',exact:true}).getByRole('button',{name:title,exact:true}).click();
  const current=(await saved.request<{data:Saved[]}>('/views')).data.find(item=>item.name===title)!;
  await saved.request(`/views/${current.id}`,'PATCH',{name:`${title} remote`,kind:'view',visibility:current.visibility,team_id:current.team_id,definition:current.definition},{'If-Match':current.etag,'Idempotency-Key':randomUUID()});
  await page.getByLabel('Definition name',{exact:true}).fill(`${title} draft`);await page.getByRole('button',{name:'Save changes',exact:true}).click();
  await expect(page.getByRole('alert')).toContainText('changed');await expect(page.getByLabel('Definition name',{exact:true})).toHaveValue(`${title} draft`);
  expect(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth)).toBe(true);
});
