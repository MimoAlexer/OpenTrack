// @vitest-environment jsdom
import { act } from 'react';
import { createRoot,type Root } from 'react-dom/client';
import { afterEach,beforeEach,describe,expect,it,vi } from 'vitest';
import RichTextEditor from './RichTextEditor';
import { canonicalBody,fromPlainText,type RichBody } from './model';
let root:Root;let container:HTMLDivElement;
beforeEach(()=>{Range.prototype.getClientRects = () => document.createElement('span').getClientRects(); Range.prototype.getBoundingClientRect = () => new DOMRect(0,0,0,0); vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT',true);container=document.createElement('div');document.body.append(container);root=createRoot(container);vi.stubGlobal('requestAnimationFrame',(fn:FrameRequestCallback)=>setTimeout(()=>fn(0),0));});
afterEach(async()=>{await act(async()=>root.unmount());container.remove();vi.unstubAllGlobals();});
describe('real Tiptap surface',()=>{
 it('renders supported structured formatting and exposes the working toolbar',async()=>{const body=canonicalBody({type:'doc',content:[
 {type:'heading',attrs:{level:2},content:[{type:'text',text:'Heading'}]},
 {type:'paragraph',content:[{type:'text',text:'Bold',marks:[{type:'bold'}]}]},
 {type:'taskList',content:[{type:'taskItem',attrs:{checked:true},content:[{type:'paragraph',content:[{type:'text',text:'Done'}]}]}]}
 ]} as RichBody,true);await act(async()=>root.render(<RichTextEditor value={body} onChange={()=>{}} label="Description"/>));expect(container.querySelector('h2')?.textContent).toBe('Heading');expect(container.querySelector('strong')?.textContent).toBe('Bold');expect(container.querySelector<HTMLInputElement>('input[type="checkbox"]')?.checked).toBe(true);expect(container.querySelector('[aria-label="Task list"]')).toBeTruthy();});
 it('retains the same editable DOM, focus and selection when the same body is acknowledged',async()=>{const body=fromPlainText('A stable caret');const change=vi.fn();await act(async()=>root.render(<RichTextEditor value={body} onChange={change} label="Description"/>));const editable=container.querySelector<HTMLElement>('[contenteditable="true"]')!;const text=editable.querySelector('p')!.firstChild!;const selection=window.getSelection()!;await act(async()=>{editable.focus();const range=document.createRange();range.setStart(text,5);range.collapse(true);selection.removeAllRanges();selection.addRange(range);document.dispatchEvent(new Event('selectionchange'));await new Promise(resolve=>setTimeout(resolve,30));});expect(selection.anchorOffset).toBe(5);await act(async()=>root.render(<RichTextEditor value={JSON.parse(JSON.stringify(body))} onChange={change} label="Description"/>));expect(container.querySelector('[contenteditable="true"]')).toBe(editable);expect(document.activeElement).toBe(editable);expect(selection.anchorNode).toBe(text);expect(selection.anchorOffset).toBe(5);expect(change).not.toHaveBeenCalled();});
});

import { IssueComments } from './IssueComments';
import type { Session } from '../types';
const session: Session = { audience:'web',user:{id:'u',display_name:'Example Owner',email:'owner@example.test'},csrf_token:'csrf',workspaces:[{id:'w',name:'Workspace',slug:'workspace',role:'member'}] };
it('clears the actual composer after a confirmed post without restoring the submitted text',async()=>{
 const reply=(data:unknown)=>new Response(JSON.stringify(data),{status:200});
 vi.stubGlobal('fetch',vi.fn((_url:string,options:RequestInit)=>Promise.resolve(reply(options.method==='POST'?{id:'c',issue_id:'i',author:{id:'u',display_name:'Example Owner'},document:{id:'d',owner_type:'comment',owner_id:'c',schema_supported:true,schema_version:1,body:fromPlainText('A real comment'),plain_text:'A real comment',version:'1',etag:'"d1"',created_at:'2026-10-18T12:00:00Z',updated_at:'2026-10-18T12:00:00Z',can_edit:true},version:'1',etag:'"c1"',created_at:'2026-10-18T12:00:00Z',edited_at:null,deleted_at:null,can_edit:true,can_delete:true}:{data:[],has_more:false,next_cursor:null}))));
 await act(async()=>root.render(<IssueComments workspaceId="w" issueId="i" session={session} readOnly={false} stateChanged={()=>{}} changed={()=>{}}/>));
 await act(async()=>{await new Promise(resolve=>setTimeout(resolve,20));});
 const editable=container.querySelector<HTMLElement>('[contenteditable="true"]')!;editable.focus();
 await act(async()=>{const paragraph=editable.querySelector('p')!;paragraph.textContent='A real comment';editable.dispatchEvent(new InputEvent('input',{bubbles:true,inputType:'insertText',data:'A real comment'}));await new Promise(resolve=>setTimeout(resolve,20));});
 const post=[...container.querySelectorAll<HTMLButtonElement>('button')].find(button=>button.textContent==='Post comment')!;expect(post.disabled).toBe(false);
 await act(async()=>post.click());expect(container.querySelector('.comment-list')?.textContent).toContain('A real comment');expect(editable.textContent).toBe('');expect(editable.getAttribute('contenteditable')).toBe('true');expect(post.disabled).toBe(true);editable.focus();expect(document.activeElement).toBe(editable);
});
