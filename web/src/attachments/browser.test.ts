// @vitest-environment jsdom
import { Blob as NativeBlob } from 'node:buffer';
import { webcrypto } from 'node:crypto';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { downloadBrowserAttachment, hashFile, uploadBrowserFile, validateFile } from './browser';
import type { Attachment } from './types';
const fileHash = '230d8358dc8e8890b4c58deeb62912ee2f20357ae92a5cc861b98e68fe31acb5'; // SHA256("body")
const attachment: Attachment = { id:'a',document_id:null,original_name:'page.html',size_bytes:4,sha256:fileHash,state:'ready',detected_mime:'text/html',scan_status:'not_scanned',version:'1',etag:'"a1"',expires_at:null,created_at:'2026-10-18T12:00:00Z',can_delete:true };
afterEach(() => { vi.restoreAllMocks(); vi.unstubAllGlobals(); });
describe('bounded browser file transport', () => {
  it('checks allocation bounds before hashing and produces SHA256 over the selected bytes', async () => {
    vi.stubGlobal('crypto', webcrypto); const arrayBuffer = vi.fn().mockResolvedValue(new TextEncoder().encode('body').buffer); const file = { name:'page.html',size:4,arrayBuffer } as unknown as File;
    expect(await hashFile(file, new AbortController().signal)).toBe(fileHash); expect(arrayBuffer).toHaveBeenCalledOnce();
    expect(() => validateFile({name:'empty',size:0})).toThrow('Empty'); expect(() => validateFile({name:'large',size:5},4)).toThrow('4 B');
    await expect(hashFile({...file,size:101*1024*1024},new AbortController().signal)).rejects.toThrow(); expect(arrayBuffer).toHaveBeenCalledOnce();
  });
  it('sends the original File with CSRF/ETag, lets the browser set length, and reports only observed bytes', async () => {
    class Xhr { upload = { onprogress: null as null | ((event: {lengthComputable:boolean;loaded:number;total:number}) => void) }; onload: (()=>void)|null=null; onerror=null; ontimeout=null; onabort:(()=>void)|null=null; status=200; responseText=JSON.stringify(attachment); withCredentials=false; timeout=0; headers:Record<string,string>={}; open=vi.fn(); setRequestHeader=(key:string,value:string)=>{this.headers[key]=value;}; send=vi.fn(); abort=vi.fn(()=>this.onabort?.()); }
    const xhr = new Xhr(); vi.stubGlobal('XMLHttpRequest', function(){return xhr;}); const file=new File(['body'],'page.html'); const progress=vi.fn(); const signal=new AbortController(); const pending=uploadBrowserFile('w',attachment,file,'csrf',signal.signal,progress);
    expect(xhr.send).toHaveBeenCalledWith(file); expect(xhr.timeout).toBe(180000); expect(xhr.headers).toEqual({Accept:'application/json, application/problem+json','Content-Type':'application/octet-stream','X-CSRF-Token':'csrf','If-Match':'"a1"'});
    xhr.upload.onprogress?.({lengthComputable:true,loaded:2,total:4}); expect(progress).toHaveBeenCalledWith(2,4); xhr.onload?.(); expect(await pending).toEqual(attachment);
  });
  it('requires recorded length and SHA256 before saving a download, with inert Blob MIME', async () => {
    vi.stubGlobal('crypto', webcrypto); vi.stubGlobal('Blob',NativeBlob); const create=vi.fn(()=> 'blob:safe'); const revoke=vi.fn(); vi.stubGlobal('URL',Object.assign(URL,{createObjectURL:create,revokeObjectURL:revoke})); const click=vi.spyOn(HTMLAnchorElement.prototype,'click').mockImplementation(()=>{}); const timer=vi.spyOn(window,'setTimeout').mockImplementation(()=>0 as unknown as ReturnType<typeof setTimeout>);
    vi.stubGlobal('fetch',vi.fn().mockResolvedValue(new Response('body',{headers:{'Content-Length':'4'}}))); await downloadBrowserAttachment('w',attachment,new AbortController().signal); expect(click).toHaveBeenCalledOnce(); expect((create.mock.calls[0] as unknown as [Blob])[0].type).toBe('application/octet-stream'); expect(timer).toHaveBeenCalled();
    create.mockClear(); vi.stubGlobal('fetch',vi.fn().mockResolvedValue(new Response('evil',{headers:{'Content-Length':'4'}}))); await expect(downloadBrowserAttachment('w',attachment,new AbortController().signal)).rejects.toThrow('integrity'); expect(create).not.toHaveBeenCalled();
    vi.stubGlobal('fetch',vi.fn().mockResolvedValue(new Response('body'))); await expect(downloadBrowserAttachment('w',attachment,new AbortController().signal)).rejects.toThrow('size');
  });
});
