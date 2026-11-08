// @vitest-environment jsdom
import { act, useState } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, expect, it, vi } from 'vitest';
import { ApiError, request } from '../api';
import { Discovery } from './Discovery';
import type { Session } from '../types';
import type { StructuredView } from '../filters/model';
vi.mock('../api',async()=>({...await vi.importActual('../api'),request:vi.fn()}));
afterEach(()=>{vi.restoreAllMocks();vi.unstubAllGlobals();});
const session={user:{id:'principal'},csrf_token:'csrf'} as Session;
const broad:StructuredView={version:1,filter:null,layout:'list',sort:'created',archived:false,limit:50,timezone:'UTC'};
const narrow:StructuredView={...broad,filter:{kind:'predicate',field:'priority',op:'eq',value:1}};
const record=(id='saved',state:StructuredView=broad)=>({id,name:`Private ${id}`,kind:'view',visibility:'workspace',team_id:null,definition:{query:{...state,layout:undefined},layout:state.layout,group_by:'status',columns:['key','title']},repair_required:null,favorite:false,can_edit:true,etag:`"${id}:1"`,version:'1'});
const directory=(data:unknown[],more=false)=>({data,has_more:more,next_cursor:more?'next-page':null});
function deferred<T>(){let resolve!:(v:T)=>void;let reject!:(e:unknown)=>void;const promise=new Promise<T>((a,b)=>{resolve=a;reject=b;});return{promise,resolve,reject};}
async function click(host:HTMLElement,label:string){await act(async()=>{const button=[...host.querySelectorAll('button')].find(b=>b.textContent===label||b.getAttribute('aria-label')===label);if(!button)throw Error(`Missing button ${label}`);button.click();});}
async function input(host:HTMLElement,label:string,value:string){await act(async()=>{const field=[...host.querySelectorAll('input')].find(e=>e.getAttribute('aria-label')===label||e.closest('label')?.textContent?.startsWith(label));if(!field)throw Error(`Missing field ${label}`);Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value')!.set!.call(field,value);field.dispatchEvent(new Event('input',{bubbles:true}));});}
async function searchTick(){await act(async()=>{await new Promise(resolve=>setTimeout(resolve,180));});}
async function mount(applyResult=true){vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT',true);const host=document.createElement('div');document.body.append(host);const root:Root=createRoot(host);const applied:StructuredView[]=[];function Host(){const[state,set]=useState(broad);return <Discovery workspaceId="workspace" scope="principal:1" session={session} teams={[]} view={state} apply={next=>{applied.push(next);if(applyResult)set(next);return applyResult;}}/>;}await act(async()=>root.render(<Host/>));await act(async()=>{const details=host.querySelector('details')!;details.open=true;details.dispatchEvent(new Event('toggle',{bubbles:true}));});return{host,applied,close:async()=>{await act(async()=>root.unmount());host.remove();}};}

it('denial fences delayed directory and search completions, then permits an explicit fresh read',async()=>{
 const more=deferred<unknown>(),search=deferred<unknown>();let fresh=false;
 vi.mocked(request).mockImplementation(async(path,options)=>{
  if(path.includes('?cursor='))return more.promise as never;
  if(path.endsWith('/search/query'))return search.promise as never;
  if(options?.method==='PUT')throw new ApiError(403,'forbidden','Access denied');
  if(path.endsWith('/views'))return directory([record(fresh?'fresh':'saved')],!fresh) as never;
  return directory([]) as never;
 });
 const x=await mount();try{
  await click(x.host,'Load more saved views');await input(x.host,'Search issues and projects','nebula');await searchTick();
  await click(x.host,'Favorite Private saved');expect(x.host.textContent).not.toContain('Private saved');
  await act(async()=>{more.resolve(directory([record('late-private')]));search.resolve({data:[{id:'secret',kind:'issue',team_id:'team',title:'Late private title',snippet:'Late private snippet'}],has_more:false});});
  expect(x.host.textContent).not.toContain('late-private');expect(x.host.textContent).not.toContain('Late private');
  fresh=true;await click(x.host,'Try again');expect(x.host.textContent).toContain('Private fresh');
 }finally{await x.close();}
});
it('denial invalidates an uncertain mutation and pending replay before its late successful response',async()=>{
 const mutation=deferred<unknown>();let puts=0;
 vi.mocked(request).mockImplementation(async(path,options)=>{
  if(options?.method==='PUT'){puts++;return mutation.promise as never;}
  if(path.endsWith('/search/query'))throw new ApiError(404,'not_found','Unavailable');
  return directory(path.endsWith('/views')?[record()]:[]) as never;
 });
 const x=await mount();try{
  await click(x.host,'Favorite Private saved');await input(x.host,'Search issues and projects','nebula');await searchTick();
  await act(async()=>mutation.resolve({...record(),favorite:true}));expect(x.host.textContent).not.toContain('Private saved');
  await click(x.host,'Try again');expect(puts).toBe(1);
 }finally{await x.close();}
});
it('denial fences an initial directory read still pending in the same authority epoch',async()=>{
 const initial=deferred<unknown>();
 vi.mocked(request).mockImplementation(async(path)=>path.endsWith('/search/query')?Promise.reject(new ApiError(403,'forbidden','Denied')) as never:path.endsWith('/views')?initial.promise as never:directory([]) as never);
 const x=await mount();try{await input(x.host,'Search issues and projects','nebula');await searchTick();await act(async()=>initial.resolve(directory([record('stale-initial')])));expect(x.host.textContent).not.toContain('stale-initial');}finally{await x.close();}
});
it('redacted definitions cannot be replaced by an ordinary rename and require reviewed explicit repair',async()=>{
 const redacted={...record(),definition:null,repair_required:'A saved target is unavailable'};const patches:unknown[]=[];
 vi.mocked(request).mockImplementation(async(path,options)=>{
  if(options?.method==='PATCH'){patches.push(options.body);return {...record(),definition:(options.body as {definition:unknown}).definition} as never;}
  if(path.endsWith('/views/saved'))return redacted as never;
  return directory(path.endsWith('/views')?[redacted]:[]) as never;
 });
 const x=await mount();try{
  await click(x.host,'Private saved');await input(x.host,'Definition name','Only rename');
  const save=[...x.host.querySelectorAll('button')].find(b=>b.textContent==='Save changes')!;expect(save.disabled).toBe(true);save.click();expect(patches).toHaveLength(0);
  await click(x.host,'Review replacement filters');expect(x.host.textContent).toContain('replace the unavailable saved definition');expect(x.host.querySelector('pre')?.textContent).toContain('"filter": null');
  await act(async()=>{x.host.querySelector<HTMLInputElement>('input[type="checkbox"]')!.click();});
  await click(x.host,'Replace saved definition with reviewed filters');expect(patches).toHaveLength(1);expect(patches[0]).toMatchObject({name:'Only rename',definition:{query:{filter:null}}});
 }finally{await x.close();}
});
it.each(['unchanged','narrowed','deleted'] as const)('Open revalidates a %s object from an already loaded directory',async(mode)=>{
 vi.mocked(request).mockImplementation(async(path)=>{
  if(path.endsWith('/views/saved')){if(mode==='deleted')throw new ApiError(404,'not_found','Saved definition was deleted');return record('saved',mode==='narrowed'?narrow:broad) as never;}
  return directory(path.endsWith('/views')?[record()]:[]) as never;
 });
 const x=await mount();try{await click(x.host,'Private saved');if(mode==='deleted'){expect(x.applied).toHaveLength(0);expect(x.host.textContent).toContain('deleted');}else{expect(x.applied).toHaveLength(1);expect(x.applied[0].filter).toEqual(mode==='narrowed'?narrow.filter:null);}}finally{await x.close();}
});
it('Open cancels superseded selections',async()=>{
 const one=deferred<unknown>(),two=deferred<unknown>();
 vi.mocked(request).mockImplementation(async(path)=>path.endsWith('/views/one')?one.promise as never:path.endsWith('/views/two')?two.promise as never:directory(path.endsWith('/views')?[record('one'),record('two')]:[]) as never);
 const x=await mount();try{await click(x.host,'Private one');await click(x.host,'Private two');await act(async()=>{two.resolve(record('two',narrow));one.resolve(record('one'));});expect(x.applied).toHaveLength(1);expect(x.applied[0].filter).toEqual(narrow.filter);}finally{await x.close();}
});
it('Open surfaces rejection from the page filter transport',async()=>{
 vi.mocked(request).mockImplementation(async(path)=>path.endsWith('/views/saved')?record() as never:directory(path.endsWith('/views')?[record()]:[]) as never);
 const y=await mount(false);try{await click(y.host,'Private saved');expect(y.host.querySelector('[role="alert"]')?.textContent).toContain('could not be opened');}finally{await y.close();}
});
it('refresh requires review and applies a remote narrowing before a name-only draft can be saved with its ETag',async()=>{
 const remote={...record('saved',narrow),version:'2',etag:'"saved:2"',name:'Remote narrowed view'};let current=record();const patches:unknown[]=[];
 vi.mocked(request).mockImplementation(async(path,options)=>{
  if(options?.method==='PATCH'){patches.push(options.body);if(patches.length===1)throw new ApiError(412,'version_conflict','This item changed');return {...remote,name:(options.body as {name:string}).name,definition:(options.body as {definition:unknown}).definition} as never;}
  if(path.endsWith('/views/saved'))return current as never;
  return directory(path.endsWith('/views')?[record()]:[]) as never;
 });
 const x=await mount();try{
  await click(x.host,'Private saved');await input(x.host,'Definition name','My rename draft');await click(x.host,'Save changes');current=remote;
  await click(x.host,'Refresh saved version');expect(x.host.textContent).toContain('Review latest saved filters');expect(x.host.querySelector('pre')?.textContent).toContain('"priority"');
  const save=[...x.host.querySelectorAll('button')].find(b=>b.textContent==='Save changes')!;expect(save.disabled).toBe(true);save.click();expect(patches).toHaveLength(1);
  await click(x.host,'Use latest filters and keep draft name');await click(x.host,'Save changes');
  expect(patches[1]).toMatchObject({name:'My rename draft',definition:{query:{filter:narrow.filter}}});
  const last=vi.mocked(request).mock.calls.filter(([,o])=>o?.method==='PATCH').at(-1)!;expect(last[1]?.headers).toMatchObject({'If-Match':'"saved:2"'});
 }finally{await x.close();}
});
it.each([false,true])('keeps a lost-response write key for an authorized retry and drops it after denial=%s',async(deny)=>{
 const attempts:{key:string;body:unknown}[]=[];
 vi.mocked(request).mockImplementation(async(path,options)=>{
  if(options?.method==='PUT'){attempts.push({key:(options.headers as Record<string,string>)['Idempotency-Key'],body:options.body});if(attempts.length===1)throw new ApiError(0,'network_error','Response lost');return {...record(),favorite:true} as never;}
  if(path.endsWith('/search/query'))throw new ApiError(403,'forbidden','Access revoked');
  return directory(path.endsWith('/views')?[record()]:[]) as never;
 });
 const x=await mount();try{
  await click(x.host,'Favorite Private saved');
  if(deny){await input(x.host,'Search issues and projects','nebula');await searchTick();}
  await click(x.host,'Try again');
  expect(attempts).toHaveLength(deny?1:2);if(!deny)expect(attempts[1]).toEqual(attempts[0]);
 }finally{await x.close();}
});
it('Open holds editable metadata until the current saved definition is ready',async()=>{
 const detail=deferred<unknown>();const patches:unknown[]=[];
 vi.mocked(request).mockImplementation(async(path,options)=>{
  if(options?.method==='PATCH'){patches.push(options.body);return {...record(),name:(options.body as {name:string}).name} as never;}
  if(path.endsWith('/views/saved'))return detail.promise as never;
  return directory(path.endsWith('/views')?[record()]:[]) as never;
 });
 const x=await mount();try{
  await click(x.host,'Private saved');const field=x.host.querySelector<HTMLInputElement>('.discovery-save input')!;expect(field.disabled).toBe(true);
  await act(async()=>detail.resolve(record()));expect(field.disabled).toBe(false);
  await input(x.host,'Definition name','Retained rename');await click(x.host,'Save changes');expect(patches[0]).toMatchObject({name:'Retained rename'});
 }finally{await x.close();}
});
