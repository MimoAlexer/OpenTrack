// @vitest-environment jsdom
import { act } from 'react';
import { createRoot,type Root } from 'react-dom/client';
import { afterEach,beforeEach,expect,it,vi } from 'vitest';
import { AttachmentCard } from './AttachmentCard';
import { AttachmentProvider } from './context';
import { downloadBrowserAttachment } from './browser';
import type { Attachment } from './types';
import type { Session } from '../types';
vi.mock('./browser',()=>({downloadBrowserAttachment:vi.fn()}));
const record:Attachment={id:'a',document_id:'d',original_name:'notes.txt',size_bytes:4,sha256:'a'.repeat(64),state:'ready',detected_mime:'text/plain',scan_status:'not_scanned',version:'1',etag:'"a1"',expires_at:null,created_at:'2026-10-18T12:00:00Z',can_delete:true};
const session:Session={audience:'web',user:{id:'u',display_name:'Example Owner',email:'owner@example.test'},csrf_token:'csrf',workspaces:[]};
let container:HTMLDivElement;let root:Root;
beforeEach(()=>{vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT',true);container=document.createElement('div');document.body.append(container);root=createRoot(container);vi.stubGlobal('fetch',vi.fn((url:string)=>Promise.resolve(new Response(JSON.stringify(url.endsWith('/capabilities')?{features:{attachments:true},limits:{max_upload_bytes:25*1024*1024}}:record),{status:200}))));});
afterEach(async()=>{await act(async()=>root.unmount());container.remove();vi.clearAllMocks();vi.unstubAllGlobals();});
async function render(token:string){await act(async()=>root.render(<AttachmentProvider workspaceId="w" session={{...session,csrf_token:token}}><AttachmentCard id="a"/></AttachmentProvider>));}
const button=()=>container.querySelector<HTMLButtonElement>('[aria-label="Download notes.txt"]')!;
it('releases canceled download busy state after reauthentication without letting its late completion release a newer download',async()=>{
 const finishes:Array<()=>void>=[];vi.mocked(downloadBrowserAttachment).mockImplementation(()=>new Promise<void>(resolve=>finishes.push(resolve)));await render('first');await act(async()=>button().click());expect(button().disabled).toBe(true);const firstSignal=vi.mocked(downloadBrowserAttachment).mock.calls[0][2];
 await render('second');expect(firstSignal.aborted).toBe(true);expect(button().disabled).toBe(false);await act(async()=>button().click());expect(button().disabled).toBe(true);
 await act(async()=>finishes[0]());expect(button().disabled).toBe(true);await act(async()=>finishes[1]());expect(button().disabled).toBe(false);expect(container.textContent).not.toContain('failed');
});

import { useAttachments,type AttachmentContextValue } from './context';
it('uses a just-completed receipt without a loading flash and never carries that seed across auth or workspace boundaries',async()=>{
 let context!:AttachmentContextValue;function Surface({show}:{show:boolean}){context=useAttachments()!;return show?<AttachmentCard id="a"/>:null;}
 const replies:Array<(value:Response)=>void>=[];vi.stubGlobal('fetch',vi.fn((url:string)=>url.endsWith('/capabilities')?Promise.resolve(new Response(JSON.stringify({features:{attachments:true},limits:{max_upload_bytes:25*1024*1024}}),{status:200})):new Promise<Response>(resolve=>replies.push(resolve))));
 async function show(token:string,workspaceId:string,visible:boolean){await act(async()=>root.render(<AttachmentProvider workspaceId={workspaceId} session={{...session,csrf_token:token}}><Surface show={visible}/></AttachmentProvider>));}
 await show('first','w',false);context.rememberReady(record);const firstContext=context;await show('first','w',true);expect(container.textContent).toContain('notes.txt');expect(container.textContent).not.toContain('Loading file');
 await show('second','w',true);expect(container.textContent).not.toContain('notes.txt');expect(container.textContent).toContain('Loading file');firstContext.rememberReady(record);await show('second','w',true);expect(container.textContent).not.toContain('notes.txt');
 await act(async()=>replies[1](new Response(JSON.stringify(record),{status:200})));expect(container.textContent).toContain('notes.txt');context.rememberReady(record);await show('second','other',true);expect(container.textContent).not.toContain('notes.txt');
});
