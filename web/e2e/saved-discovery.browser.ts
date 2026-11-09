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

test('narrow keyboard draft survives realtime reconciliation of a remote version', async ({ page, saved }) => {
  const title = `Conflict view ${randomUUID()}`;await page.setViewportSize({width:390,height:844});
  await saved.open(page);await page.locator('.discovery > summary').focus();await page.keyboard.press('Enter');
  await page.getByLabel('Definition name', { exact: true }).fill(title);await page.getByRole('combobox',{name:'View audience',exact:true}).selectOption('workspace');await page.getByRole('button', { name:'Save new view',exact:true }).click();
  await page.getByRole('region',{name:'Saved views',exact:true}).getByRole('button',{name:title,exact:true}).click();
  const current=(await saved.request<{data:Saved[]}>('/views')).data.find(item=>item.name===title)!;
  await saved.request(`/views/${current.id}`,'PATCH',{name:`${title} remote`,kind:'view',visibility:current.visibility,team_id:current.team_id,definition:{...current.definition!,query:{...current.definition!.query,filter:{kind:'predicate',field:'priority',op:'eq',value:1}}}},{'If-Match':current.etag,'Idempotency-Key':randomUUID()});
  await page.getByLabel('Definition name',{exact:true}).fill(`${title} draft`);
  await expect(page.getByRole('region',{name:'Review latest saved filters',exact:true})).toBeVisible();
  await expect(page.getByRole('button',{name:'Save changes',exact:true})).toBeDisabled();
  await expect(page.getByLabel('Definition name',{exact:true})).toHaveValue(`${title} draft`);
  await page.getByRole('button',{name:'Refresh saved version',exact:true}).click();
  const review=page.getByRole('region',{name:'Review latest saved filters',exact:true});await expect(review).toContainText('priority');expect(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth)).toBe(true);
  await expect(page.getByRole('button',{name:'Save changes',exact:true})).toBeDisabled();
  await review.getByRole('button',{name:'Use latest filters and keep draft name',exact:true}).click();
  await expect(page.getByRole('button',{name:'Save changes',exact:true})).toBeEnabled();
  const committed=page.waitForResponse(response=>response.request().method()==='PATCH'&&new URL(response.url()).pathname===`${saved.path}/views/${current.id}`);
  await page.getByRole('button',{name:'Save changes',exact:true}).click();expect((await committed).status()).toBe(200);
  await expect(page.getByRole('alert')).toHaveCount(0);
  const persisted=await saved.request<Saved>(`/views/${current.id}`);expect(persisted.name).toBe(`${title} draft`);expect(persisted.definition!.query.filter).toEqual({kind:'predicate',field:'priority',op:'eq',value:1});
  await page.reload();await page.locator('.discovery > summary').click();await page.getByRole('region',{name:'Saved views',exact:true}).getByRole('button',{name:`${title} draft`,exact:true}).click();
  const restored=parseStructuredHash(new URL(page.url()).hash);expect(restored.kind).toBe('valid');if(restored.kind==='valid')expect(restored.view.filter).toEqual(persisted.definition!.query.filter);
  expect(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth)).toBe(true);
});

test('Open revalidates remote narrowing and deletion while denial cancels a delayed real search response',async({page,saved})=>{
  const title=`Fresh open ${randomUUID()}`,token=`pending${randomUUID().replaceAll('-','')}`;
  const issue=await saved.create(`Delayed authorized result ${randomUUID()}`,paragraph(token));await saved.patch(issue,{priority:1});
  const current=await saved.request<Saved>('/views','POST',{name:title,kind:'view',visibility:'workspace',definition:{query:{version:1,filter:null},layout:'list',group_by:'status',columns:['key','title']}},{'Idempotency-Key':randomUUID()});
  await saved.open(page);await page.locator('.discovery > summary').click();
  const directory=page.getByRole('region',{name:'Saved views',exact:true});await expect(directory.getByRole('button',{name:title,exact:true})).toBeVisible();
  const filter={kind:'predicate',field:'priority',op:'eq',value:1};
  const narrowed=await saved.request<Saved>(`/views/${current.id}`,'PATCH',{name:title,kind:'view',visibility:'workspace',team_id:null,definition:{...current.definition!,query:{...current.definition!.query,filter}}},{'If-Match':current.etag,'Idempotency-Key':randomUUID()});
  await directory.getByRole('button',{name:title,exact:true}).click();
  await expect.poll(()=>{const parsed=parseStructuredHash(new URL(page.url()).hash);return parsed.kind==='valid'?parsed.view.filter:null;}).toEqual(filter);
  let release!:()=>void,started!:()=>void,finished!:()=>void;
  const gate=new Promise<void>(r=>{release=r;}),fetched=new Promise<void>(r=>{started=r;}),settled=new Promise<void>(r=>{finished=r;});
  await page.route('**/search/query',async route=>{try{const response=await route.fetch();expect(response.status()).toBe(200);expect((await response.json() as {data:{id:string}[]}).data.some(hit=>hit.id===issue.id)).toBe(true);started();await gate;try{await route.fulfill({response});}catch{/* The current-authority denial aborts this original request. */}}finally{finished();}});
  await page.getByRole('searchbox',{name:'Search issues and projects',exact:true}).fill(token);await fetched;
  const removed=await saved.http.delete(`${saved.path}/views/${current.id}`,{headers:{Origin:saved.origin,'X-CSRF-Token':saved.session.csrf_token,'If-Match':narrowed.etag,'Idempotency-Key':randomUUID()}});expect(removed.status()).toBe(204);
  await directory.getByRole('button',{name:title,exact:true}).click();await expect(page.getByRole('alert')).toBeVisible();await expect(directory.getByRole('button',{name:title,exact:true})).toHaveCount(0);
  release();await settled;await page.evaluate(()=>new Promise<void>(resolve=>requestAnimationFrame(()=>requestAnimationFrame(()=>resolve()))));
  await expect(page.locator('.discovery-results')).toHaveCount(0);await expect(page.locator('.discovery')).not.toContainText(issue.title);await expect(page.getByRole('searchbox',{name:'Search issues and projects',exact:true})).toHaveValue('');
});

test('an authorized editor must explicitly review replacement of a redacted shared definition',async({page,saved,playwright})=>{
  const suffix=randomUUID(),name=`Restricted view ${suffix}`,privateName=`Hidden team ${suffix}`;
  const team=await saved.request<{id:string}>('/teams','POST',{name:privateName,key:`P${suffix.replaceAll('-','').slice(0,8).toUpperCase()}`,private:true},{'Idempotency-Key':randomUUID()});
  const original=await saved.request<Saved>('/views','POST',{name,kind:'view',visibility:'workspace',definition:{query:{version:1,filter:{kind:'predicate',field:'team',op:'eq',value:team.id}},layout:'list',group_by:'status',columns:['key','title']}},{'Idempotency-Key':randomUUID()});
  const invite=await saved.request<{token:string}>('/invitations','POST',{email:`admin-${suffix}@example.test`,role:'admin'},{'Idempotency-Key':randomUUID()});
  const admin=await playwright.request.newContext({baseURL:saved.origin,extraHTTPHeaders:{Origin:saved.origin},storageState:{cookies:[],origins:[]}});
  try{
    expect((await admin.get('/api/v1/auth/session')).status()).toBe(401);
    const accepted=await admin.post('/api/v1/invitations/accept',{data:{token:invite.token,display_name:'Example Admin',password:process.env.OPENTRACK_BROWSER_PASSWORD}});expect(accepted.status()).toBe(200);
    await page.context().clearCookies();await page.context().addCookies((await admin.storageState()).cookies);
    await page.goto(`/w/${saved.workspace}/issues`);await page.locator('.discovery > summary').click();
    await page.getByRole('region',{name:'Saved views',exact:true}).getByRole('button',{name,exact:true}).click();
    await expect(page.getByRole('alert')).toBeVisible();await expect(page.getByRole('button',{name:'Save changes',exact:true})).toBeDisabled();
    await expect(page.locator('.discovery')).not.toContainText(privateName);await expect(page.locator('.discovery')).not.toContainText(team.id);
    await page.getByLabel('Definition name',{exact:true}).fill(`${name} repaired`);
    const unchanged=await saved.request<Saved>(`/views/${original.id}`);expect(unchanged.definition!.query.filter).toEqual(original.definition!.query.filter);
    await page.getByRole('button',{name:'Review replacement filters',exact:true}).click();const review=page.getByRole('region',{name:'Review replacement filters',exact:true});
    await expect(review).toContainText('replace the unavailable saved definition');await expect(review.getByRole('button',{name:'Replace saved definition with reviewed filters',exact:true})).toBeDisabled();
    await review.getByRole('checkbox').check();const committed=page.waitForResponse(response=>response.request().method()==='PATCH'&&new URL(response.url()).pathname===`${saved.path}/views/${original.id}`);await review.getByRole('button',{name:'Replace saved definition with reviewed filters',exact:true}).click();expect((await committed).status()).toBe(200);
    await expect(page.getByRole('alert')).toHaveCount(0);const repaired=await saved.request<Saved>(`/views/${original.id}`);expect(repaired.name).toBe(`${name} repaired`);expect(repaired.definition!.query.filter).toBeNull();
  }finally{await admin.dispose();}
});

test('realtime updates an open saved directory and remote deletion preserves the draft',async({page,saved})=>{
 const title=`Live view ${randomUUID()}`;
 await saved.open(page);await page.locator('.discovery > summary').click();
 await page.getByLabel('Definition name',{exact:true}).fill(title);
 await page.getByRole('button',{name:'Save new view',exact:true}).click();
 const directory=page.getByRole('region',{name:'Saved views',exact:true});
 await directory.getByRole('button',{name:title,exact:true}).click();
 const current=(await saved.request<{data:Saved[]}>('/views')).data.find(item=>item.name===title)!;
 await page.getByLabel('Definition name',{exact:true}).fill(`${title} draft`);
 const remote=await saved.request<Saved>(`/views/${current.id}`,'PATCH',{name:`${title} remote`,kind:'view',visibility:current.visibility,team_id:current.team_id,definition:current.definition},{'If-Match':current.etag,'Idempotency-Key':randomUUID()});
 await expect(directory.getByRole('button',{name:remote.name,exact:true})).toBeVisible();
 await expect(page.getByLabel('Definition name',{exact:true})).toHaveValue(`${title} draft`);
 await expect(page.getByRole('region',{name:'Review latest saved filters',exact:true})).toBeVisible();
 // The fixture helper expects JSON; DELETE intentionally has an empty response.
 const response=await saved.http.delete(`${saved.path}/views/${remote.id}`,{headers:{Origin:saved.origin,'X-CSRF-Token':saved.session.csrf_token,'If-Match':remote.etag,'Idempotency-Key':randomUUID()}});
 expect(response.status()).toBe(204);
 await expect(directory.getByRole('button',{name:remote.name,exact:true})).toHaveCount(0);
 await expect(page.getByRole('button',{name:'Save changes',exact:true})).toHaveCount(0);
 await expect(page.getByLabel('Definition name',{exact:true})).toHaveValue(`${title} draft`);
});
