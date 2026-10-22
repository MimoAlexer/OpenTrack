import { describe, expect, it, vi } from 'vitest';
import { createNativeBridge } from './bridge';
import type { NativeInvoke, NativeListen } from './types';

function harness() {
  let generation='7';const stop=vi.fn();let progress:((event:{payload:{operation_id:string;sent_bytes:number;total_bytes:number}})=>void)|undefined;
  const invoke=vi.fn();const listen=vi.fn(async(name,callback)=>{if(name==='opentrack-attachment-progress')progress=callback;return stop;}) as NativeListen;
  return {bridge:createNativeBridge(invoke as NativeInvoke,listen,()=>generation),invoke,stop,setGeneration:(value:string)=>{generation=value;},progress:(value:{operation_id:string;sent_bytes:number;total_bytes:number})=>progress?.({payload:value})};
}
const input={workspace_id:'w',attachment_id:'a',handle:'opaque',etag:'"attachment:a:1"'};
const ready={id:'a',etag:'"attachment:a:2"',state:'ready'};
describe('native file bridge',()=>{
  it('sends handles and exact destinations without renderer paths, files or credentials',async()=>{
    const h=harness();h.invoke.mockResolvedValue({generation:'7',status:200,headers:{},body:ready});
    expect(await h.bridge.uploadAttachment(input)).toEqual(ready);
    expect(h.invoke).toHaveBeenCalledWith('attachment_upload',{request:{...input,expected_generation:'7',operation_id:expect.any(String)}});
    expect(h.stop).toHaveBeenCalledOnce();
  });
  it('forwards only progress for this operation, and stops listening after completion',async()=>{
    const h=harness();let finish!:(value:unknown)=>void;h.invoke.mockImplementation(()=>new Promise(resolve=>{finish=resolve;}));
    const onProgress=vi.fn();const pending=h.bridge.uploadAttachment({...input,onProgress});await vi.waitFor(()=>expect(h.invoke).toHaveBeenCalled());
    const operation_id=h.invoke.mock.calls[0][1].request.operation_id;
    h.progress({operation_id:'another',sent_bytes:1,total_bytes:4});h.progress({operation_id,sent_bytes:5,total_bytes:4});h.progress({operation_id,sent_bytes:2,total_bytes:4});
    expect(onProgress).toHaveBeenCalledExactlyOnceWith(2,4);
    finish({generation:'7',status:202,headers:{},body:{...ready,state:'quarantined'}});await pending;expect(h.stop).toHaveBeenCalledOnce();
  });
  it('keeps ambiguous transfer results distinct from a definite pre-dispatch refusal',async()=>{
    const h=harness();let finish!:(value:unknown)=>void;h.invoke.mockImplementation(()=>new Promise(resolve=>{finish=resolve;}));
    const pending=h.bridge.uploadAttachment(input);await vi.waitFor(()=>expect(h.invoke).toHaveBeenCalled());h.setGeneration('8');
    finish({generation:'7',status:200,headers:{},body:ready});await expect(pending).rejects.toMatchObject({code:'native_result_uncertain'});
  });
  it('cancels the native operation, then leaves reconciliation to its caller',async()=>{
    const h=harness();let finish!:(value:unknown)=>void;h.invoke.mockImplementation((command)=>command==='attachment_upload'?new Promise(resolve=>{finish=resolve;}):Promise.resolve());
    const controller=new AbortController();const pending=h.bridge.uploadAttachment({...input,signal:controller.signal});await vi.waitFor(()=>expect(h.invoke).toHaveBeenCalled());controller.abort();
    expect(h.invoke.mock.calls[1][0]).toBe('attachment_cancel');finish({generation:'7',status:200,headers:{},body:ready});
    await expect(pending).rejects.toMatchObject({name:'AbortError'});expect(h.stop).toHaveBeenCalledOnce();
  });
  it('returns server conflict details without retrying the upload',async()=>{
    const h=harness();h.invoke.mockResolvedValue({generation:'7',status:412,headers:{},body:{code:'version_conflict',detail:'Refresh this file.',details:{current:{id:'a'}}}});
    await expect(h.bridge.uploadAttachment(input)).rejects.toMatchObject({status:412,code:'version_conflict',details:{current:{id:'a'}}});expect(h.invoke).toHaveBeenCalledOnce();
  });
  it('treats save-dialog cancellation as a normal result',async()=>{
    const h=harness();h.invoke.mockResolvedValue({saved:false,generation:'7'});
    expect(await h.bridge.downloadAttachment({workspace_id:'w',attachment_id:'a',original_name:'design.txt'})).toEqual({saved:false});
  });
});
