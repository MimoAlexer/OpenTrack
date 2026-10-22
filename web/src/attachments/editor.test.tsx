// @vitest-environment jsdom
import { act, useState } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import RichTextEditor from '../documents/RichTextEditor';
import { AttachmentProvider } from './context';
import { hashFile, uploadBrowserFile } from './browser';
import { attachmentIds, EMPTY_BODY, fromPlainText, plainText, type RichBody } from '../documents/model';
import type { Attachment, AttachmentDraftState } from './types';
import type { Session } from '../types';
vi.mock('./browser', async original => ({ ...await original<typeof import('./browser')>(), hashFile: vi.fn(), uploadBrowserFile: vi.fn() }));
const id='11111111-1111-4111-8111-111111111111'; const hash='a'.repeat(64);
const record=(state:Attachment['state']):Attachment=>({id,document_id:null,original_name:'notes.txt',size_bytes:4,sha256:hash,state,detected_mime:'text/plain',scan_status:state==='ready'?'not_scanned':'pending',version:'1',etag:'"a1"',expires_at:null,created_at:'2026-10-18T12:00:00Z',can_delete:true});
const session:Session={audience:'web',user:{id:'u',display_name:'Example Owner',email:'owner@example.test'},csrf_token:'csrf',workspaces:[]};
const json=(data:unknown)=>new Response(JSON.stringify(data),{status:200});
let container:HTMLDivElement;let root:Root;let latest:RichBody;let uploads:AttachmentDraftState;
function Surface({initial=fromPlainText('Original text')}:{initial?:RichBody}){const[body,setBody]=useState(initial);latest=body;return <AttachmentProvider workspaceId="w" session={session}><RichTextEditor value={body} onChange={setBody} label="Description" attachmentTarget={{team_id:'t'}} attachmentsChanged={value=>{uploads=value;}}/></AttachmentProvider>;}
beforeEach(()=>{vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT',true);Range.prototype.getClientRects=()=>document.createElement('span').getClientRects();Range.prototype.getBoundingClientRect=()=>new DOMRect(0,0,0,0);container=document.createElement('div');document.body.append(container);root=createRoot(container);vi.mocked(hashFile).mockResolvedValue(hash);vi.stubGlobal('fetch',vi.fn((url:string,options:RequestInit)=>Promise.resolve(json(url.endsWith('/capabilities')?{features:{attachments:true},limits:{max_upload_bytes:25*1024*1024}}:record(options.method==='POST'?'pending':'ready')))));});
afterEach(async()=>{await act(async()=>root.unmount());container.remove();vi.clearAllMocks();vi.unstubAllGlobals();});
it('keeps typed text and focus when a ready file arrives at a mapped insertion point',async()=>{
 let finish!:(value:Attachment)=>void;vi.mocked(uploadBrowserFile).mockImplementation(()=>new Promise(resolve=>{finish=resolve;}));await act(async()=>root.render(<Surface/>));const editable=container.querySelector<HTMLElement>('[contenteditable="true"]')!;editable.focus();
 const text=editable.querySelector('p')!.firstChild!;const selection=window.getSelection()!;const range=document.createRange();range.setStart(text,8);range.collapse(true);selection.removeAllRanges();selection.addRange(range);document.dispatchEvent(new Event('selectionchange'));
 await act(async()=>{container.querySelector<HTMLButtonElement>('[aria-label="Attach files"]')!.click();const input=container.querySelector<HTMLInputElement>('input[type=file]')!;Object.defineProperty(input,'files',{configurable:true,value:[new File(['body'],'notes.txt')]});input.dispatchEvent(new Event('change',{bubbles:true}));});
 expect(uploads.pending).toBe(true);expect(container.querySelector('[data-attachment-id]')).toBeNull();
 await act(async()=>{const paragraph=editable.querySelector('p')!;paragraph.textContent='Newer Original text';editable.dispatchEvent(new InputEvent('input',{bubbles:true,inputType:'insertText',data:'Newer '}));await new Promise(resolve=>setTimeout(resolve,20));});editable.focus();
 await act(async()=>finish(record('ready')));expect(plainText(latest)).toContain('Newer Original');expect(plainText(latest)).toContain('text');expect(attachmentIds(latest)).toEqual([id]);expect(latest.content.findIndex(node=>node.type==='attachment')).toBe(1);expect(plainText(latest.content[0])).toBe('Newer Original');expect(container.querySelector('[contenteditable="true"]')).toBe(editable);expect(document.activeElement).toBe(editable);expect(uploads.pending).toBe(false);expect(container.textContent).toContain('Not scanned');
});
it('accepts dropped files through the same queue and rejects fabricated attachment HTML paste',async()=>{
 vi.mocked(uploadBrowserFile).mockResolvedValue(record('ready'));await act(async()=>root.render(<Surface/>));const drop=new Event('drop',{bubbles:true,cancelable:true});Object.defineProperty(drop,'dataTransfer',{value:{files:[new File(['body'],'notes.txt')],types:['Files']}});await act(async()=>container.querySelector('.document-editor')!.dispatchEvent(drop));expect(uploadBrowserFile).toHaveBeenCalledOnce();expect(attachmentIds(latest)).toEqual([id]);
 const { AttachmentNode }=await import('./node');expect(AttachmentNode.config.parseHTML?.call({} as never)).toEqual([]);
});

it('keeps the attachment toolbar slot and DOM stable while capabilities load and editing pauses',async()=>{
 let loaded!:(value:Response)=>void;vi.stubGlobal('fetch',vi.fn(()=>new Promise<Response>(resolve=>{loaded=resolve;})));
 const render=(disabled:boolean)=><AttachmentProvider workspaceId="w" session={session}><RichTextEditor value={fromPlainText('Text')} onChange={()=>{}} label="Description" attachmentTarget={{team_id:'t'}} disabled={disabled}/></AttachmentProvider>;
 await act(async()=>root.render(render(false)));const button=container.querySelector<HTMLButtonElement>('[aria-label="Attach files"]')!;expect(button).toBeTruthy();expect(button.disabled).toBe(true);const count=container.querySelector('.document-toolbar')!.children.length;
 await act(async()=>loaded(json({features:{attachments:true},limits:{max_upload_bytes:25*1024*1024}})));expect(container.querySelector('[aria-label="Attach files"]')).toBe(button);expect(button.disabled).toBe(false);
 await act(async()=>root.render(render(true)));expect(container.querySelector('[aria-label="Attach files"]')).toBe(button);expect(button.disabled).toBe(true);expect(container.querySelector('.document-toolbar')!.children.length).toBe(count);
 await act(async()=>root.render(render(false)));expect(container.querySelector('[aria-label="Attach files"]')).toBe(button);expect(button.disabled).toBe(false);
});

it.each(['empty', 'start', 'middle', 'end'])('inserts a selected batch in file order at the same %s anchor', async position => {
 const secondId='22222222-2222-4222-8222-222222222222';
 const first={...record('ready'),original_name:'A.txt'};const second={...record('ready'),id:secondId,original_name:'B.txt'};
 vi.stubGlobal('fetch',vi.fn((url:string,options:RequestInit)=>Promise.resolve(json(url.endsWith('/capabilities')?{features:{attachments:true},limits:{max_upload_bytes:25*1024*1024}}:options.method==='POST'?{...(JSON.parse(options.body as string).original_name==='A.txt'?first:second),state:'pending'}:url.endsWith(id)?first:second))));
 vi.mocked(uploadBrowserFile).mockImplementation(async(_workspace,attachment)=>({...attachment,state:'ready',scan_status:'not_scanned'}));
 await act(async()=>root.render(<Surface initial={position==='empty'?EMPTY_BODY:fromPlainText('Original text')}/>));
 const editable=container.querySelector<HTMLElement>('[contenteditable="true"]')!;editable.focus();
 if(position==='middle'||position==='end'){const text=editable.querySelector('p')!.firstChild!;const range=document.createRange();range.setStart(text,position==='end'?text.textContent!.length:8);range.collapse(true);const selection=window.getSelection()!;selection.removeAllRanges();selection.addRange(range);await act(async()=>{document.dispatchEvent(new Event('selectionchange'));await new Promise(resolve=>setTimeout(resolve,0));});}
 await act(async()=>{container.querySelector<HTMLButtonElement>('[aria-label="Attach files"]')!.click();const input=container.querySelector<HTMLInputElement>('input[type=file]')!;Object.defineProperty(input,'files',{configurable:true,value:[new File(['body'],'A.txt'),new File(['body'],'B.txt')]});input.dispatchEvent(new Event('change',{bubbles:true}));});
 expect(attachmentIds(latest)).toEqual([id,secondId]);if(position!=='empty')expect(plainText(latest).replace(/\s+/g,' ').trim()).toContain('Original text');expect(uploads.pending).toBe(false);
});
