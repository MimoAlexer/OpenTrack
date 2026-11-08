// @vitest-environment jsdom
import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, expect, it, vi } from 'vitest';
import { Discovery } from './Discovery';
import { request } from '../api';
import type { Session } from '../types';
vi.mock('../api', async () => ({ ...await vi.importActual('../api'), request: vi.fn() }));
afterEach(() => { vi.restoreAllMocks(); vi.unstubAllGlobals(); });
const session = { user: { id: 'principal' }, csrf_token: 'csrf' } as Session;
const view = { version: 1, filter: null, layout: 'list', sort: 'created', archived: false, limit: 50, timezone: 'UTC' } as const;
async function openPanel(host:HTMLElement) { await act(async()=>{const details=host.querySelector('details')!;details.open=true;details.dispatchEvent(new Event('toggle',{bubbles:true}));}); }
const saved = { id: 'saved', name: 'Private saved work', kind: 'view', visibility: 'personal', team_id: null, definition: { query: { ...view, layout: undefined }, layout: 'list', group_by: 'status', columns: ['key', 'title'] }, favorite: false, can_edit: true, version: '1', etag: '"v1"', repair_required: null };
it('saves and reopens current filters and never retains private names across authority changes', async () => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true);
  const mocked=vi.mocked(request);mocked.mockImplementation(async (path, options) => {
    if (options?.method==='POST') return saved as never;
    return {data:path.includes('/views')?[saved]:[],has_more:false,next_cursor:null} as never;
  });
  const host=document.createElement('div');document.body.append(host);const root=createRoot(host);const apply=vi.fn();
  try {
    await act(async()=>root.render(<Discovery workspaceId="workspace" scope="principal:1" session={session} view={view} teams={[]} apply={apply}/>));
    await openPanel(host);
    expect(host.textContent).toContain('Private saved work');
    const open=[...host.querySelectorAll('button')].find(b=>b.textContent==='Private saved work')!;
    await act(async()=>open.click());expect(apply).toHaveBeenCalledWith(expect.objectContaining({filter:null,layout:'list'}));
    mocked.mockImplementation(()=>new Promise(()=>{}));
    await act(async()=>root.render(<Discovery workspaceId="workspace" scope="principal:2" session={session} view={view} teams={[]} apply={apply}/>));
    expect(host.textContent).not.toContain('Private saved work');
  }finally{await act(async()=>root.unmount());host.remove();}
});
it('renders snippets as text and aborts superseded search requests', async()=>{
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT',true);vi.useFakeTimers();
  const signals:AbortSignal[]=[];const mocked=vi.mocked(request);
  mocked.mockImplementation(async(path,options)=>{
    if(path.endsWith('/search/query')){signals.push(options!.signal as AbortSignal);return {data:[{id:'issue',team_id:'team',kind:'issue',key:'PUB-1',title:'Result',snippet:'<img src=x onerror=alert(1)>'}],has_more:false,next_cursor:null,fuzzy_degraded:true,notice:'Fuzzy unavailable'} as never;}
    return {data:[],has_more:false,next_cursor:null} as never;
  });
  const host=document.createElement('div');document.body.append(host);const root=createRoot(host);
  try{
    await act(async()=>root.render(<Discovery workspaceId="workspace" scope="principal:1" session={session} view={view} teams={[]} apply={()=>true}/>));
    await openPanel(host);
    const input=host.querySelector<HTMLInputElement>('[aria-label="Search issues and projects"]')!;
    const change=async(value:string)=>act(async()=>{Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value')!.set!.call(input,value);input.dispatchEvent(new Event('input',{bubbles:true}));await vi.advanceTimersByTimeAsync(200);});
    await change('nebula');expect(host.textContent).toContain('<img src=x onerror=alert(1)>');expect(host.querySelector('img')).toBeNull();
    await change('quasar');expect(signals[0].aborted).toBe(true);
  }finally{await act(async()=>root.unmount());host.remove();vi.useRealTimers();}
});
it('explicit save clears overrides while preserving saved display columns and grouping', async()=>{
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT',true);
  const mocked=vi.mocked(request);const custom={...saved,definition:{...saved.definition,group_by:'assignee',columns:['title','project']}};
  mocked.mockImplementation(async(path,options)=>options?.method==='PATCH'?{...custom,definition:(options.body as {definition:unknown}).definition,version:'2',etag:'"v2"'} as never:{data:path.includes('/views')?[custom]:[],has_more:false,next_cursor:null} as never);
  const host=document.createElement('div');document.body.append(host);const root=createRoot(host);
  try {
    await act(async()=>root.render(<Discovery workspaceId="workspace" scope="principal:1" session={session} view={view} teams={[]} apply={()=>true}/>));await openPanel(host);
    await act(async()=>[...host.querySelectorAll('button')].find(b=>b.textContent==='Private saved work')!.click());
    const changed={...view,filter:{kind:'predicate' as const,field:'priority' as const,op:'eq' as const,value:1}};
    await act(async()=>root.render(<Discovery workspaceId="workspace" scope="principal:1" session={session} view={changed} teams={[]} apply={()=>true}/>));
    expect(host.textContent).toContain('Unsaved overrides');
    await act(async()=>[...host.querySelectorAll('button')].find(b=>b.textContent==='Save changes')!.click());
    expect(host.textContent).not.toContain('Unsaved overrides');
    const call=mocked.mock.calls.find(([,o])=>o?.method==='PATCH')!;expect(call[1]?.body).toMatchObject({definition:{group_by:'assignee',columns:['title','project'],query:{filter:changed.filter}}});
  }finally{await act(async()=>root.unmount());host.remove();}
});
