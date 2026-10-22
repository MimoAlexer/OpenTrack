// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach,beforeEach,expect,it,vi } from 'vitest';
import { IssueEditor } from '../issues/IssueEditor';
import { hasUnsavedChanges,useUnsavedChanges } from '../router';
import { fromPlainText,plainText } from './model';
import type { Bootstrap,Issue,Session } from '../types';
vi.mock('./Editor',()=>({StructuredEditor:({value,onChange,label}:any)=><textarea aria-label={label} value={plainText(value)} onChange={e=>onChange(fromPlainText(e.target.value))}/>}));
vi.mock('./IssueComments',()=>({IssueComments:()=>null}));
vi.mock('../issues/IssueFields',()=>({IssueFields:()=>null}));
vi.mock('../issues/IssueActivity',()=>({IssueActivity:()=>null}));
vi.mock('../relations/IssueRelationships',()=>({IssueRelationships:()=>null}));
vi.mock('../workflows/useIssueWorkflow',()=>({useIssueWorkflow:()=>({statuses:[{id:'s',name:'Ready',category:'unstarted',archived_at:null,retiring_at:null}],currentIds:new Set(['s']),error:null,refresh:()=>{}})}));
const session:Session={audience:'web',user:{id:'u',display_name:'Example Owner',email:'owner@example.test'},csrf_token:'csrf',workspaces:[{id:'w',name:'Workspace',slug:'w',role:'member'}]};
const issue:Issue={id:'i',workspace_id:'w',team_id:'t',key:'ENG-1',title:'Original title',description:'Original body',body_version:'1',status_id:'s',priority:0,assignee_id:null,assignee:null,label_ids:[],labels:[],etag:'"issue:i:1:body1"',estimate:null,due_date:null,version:'1',created_at:'2026-10-18T12:00:00Z',updated_at:'2026-10-18T12:00:00Z',archived_at:null};
const data:Bootstrap={auth_epoch:'1',teams:[{id:'t',name:'Engineering',key:'ENG',private:false,archived_at:null}],statuses:[],members:[],issues:[issue]};
const doc=(text:string,version:string)=>({id:'d',owner_id:'i',owner_type:'issue',schema_version:1,schema_supported:true,body:fromPlainText(text),plain_text:text,version,etag:`"d:${version}"`,can_edit:true,created_at:issue.created_at,updated_at:issue.updated_at});
const json=(value:unknown)=>new Response(JSON.stringify(value),{status:200});let container:HTMLDivElement;let root:Root;
beforeEach(()=>{vi.useFakeTimers();vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT',true);container=document.createElement('div');document.body.append(container);root=createRoot(container);});
afterEach(async()=>{await act(async()=>root.unmount());container.remove();vi.useRealTimers();vi.unstubAllGlobals();});
async function type(selector:string,value:string){const el=container.querySelector<HTMLTextAreaElement>(selector)!;await act(async()=>{Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype,'value')!.set!.call(el,value);el.dispatchEvent(new Event('input',{bubbles:true}));});}
it('serializes metadata behind body save and uses the acknowledged representation tag without refreshing issue pages',async()=>{
 let finish!: (value:Response)=>void;let latest=issue;const updated=vi.fn();const fetch=vi.fn((url:string,opts:RequestInit)=>{
 if(url.endsWith('/issues/i/document'))return Promise.resolve(json(doc(latest.description,latest.body_version!)));
 if(url.endsWith('/documents/d'))return new Promise<Response>(resolve=>{finish=resolve;});
 if(opts.method==='PATCH'){expect(new Headers(opts.headers).get('If-Match')).toBe('"issue:i:1:body2"');latest={...latest,title:'Edited title',version:'2',etag:'"issue:i:2:body2"'};return Promise.resolve(json(latest));}
 return Promise.resolve(json(latest));});vi.stubGlobal('fetch',fetch);
 await act(async()=>root.render(<IssueEditor workspaceId="w" issueId="i" data={data} session={session} close={()=>{}} updated={updated}/>));await type('#issue-title','Edited title');await type('textarea[aria-label="Description"]','Edited body');await act(async()=>vi.advanceTimersByTime(750));
 const save=()=>[...container.querySelectorAll<HTMLButtonElement>('button')].find(button=>button.textContent==='Save changes')!;expect(save().disabled).toBe(true);expect(fetch.mock.calls.filter(([url,opts])=>url.endsWith('/issues/i')&&opts.method==='PATCH')).toHaveLength(0);
 latest={...issue,description:'Edited body',body_version:'2',etag:'"issue:i:1:body2"'};await act(async()=>finish(json(doc('Edited body','2'))));expect(container.querySelector<HTMLTextAreaElement>('#issue-title')!.value).toBe('Edited title');expect(updated).toHaveBeenCalledWith(latest,{refreshList:false});expect(save().disabled).toBe(false);await act(async()=>save().click());expect(latest.title).toBe('Edited title');expect(container.textContent).not.toContain('This issue changed while');
});
it('keeps independent unsaved registrations until every editor is clean',async()=>{function Guard({dirty}:{dirty:boolean}){useUnsavedChanges(dirty);return null;}await act(async()=>root.render(<><Guard dirty/><Guard dirty/></>));expect(hasUnsavedChanges()).toBe(true);await act(async()=>root.render(<><Guard dirty/><Guard dirty={false}/></>));expect(hasUnsavedChanges()).toBe(true);await act(async()=>root.render(<><Guard dirty={false}/><Guard dirty={false}/></>));expect(hasUnsavedChanges()).toBe(false);});

it('keeps legacy IME and modified editing keys from submitting issue metadata', async () => {
 const fetch = vi.fn((url: string, options: RequestInit) => Promise.resolve(json(url.endsWith('/document') ? doc('Original body', '1') : options.method === 'PATCH' ? { ...issue, title: 'Reviewed title', version: '2' } : issue))); vi.stubGlobal('fetch', fetch);
 await act(async () => root.render(<IssueEditor workspaceId="w" issueId="i" data={data} session={session} close={() => {}} updated={() => {}} />)); await type('#issue-title', 'Reviewed title');
 const input = container.querySelector<HTMLTextAreaElement>('#issue-title')!;
 for (const options of [{ keyCode: 229 }, { isComposing: true }, { altKey: true }, { repeat: true }]) await act(async () => input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', ctrlKey: true, bubbles: true, cancelable: true, ...options })));
 const event = new KeyboardEvent('keydown', { key: 'Enter', ctrlKey: true, bubbles: true, cancelable: true }); Object.defineProperty(event, 'getModifierState', { value: (name: string) => name === 'AltGraph' }); await act(async () => input.dispatchEvent(event));
 expect(fetch.mock.calls.filter(([, options]) => options.method === 'PATCH')).toHaveLength(0);
 await act(async () => input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', ctrlKey: true, bubbles: true, cancelable: true })));
 expect(fetch.mock.calls.filter(([, options]) => options.method === 'PATCH')).toHaveLength(1);
});
