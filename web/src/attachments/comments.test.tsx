// @vitest-environment jsdom
import { act } from 'react';
import { createRoot,type Root } from 'react-dom/client';
import { afterEach,beforeEach,expect,it,vi } from 'vitest';
import { IssueComments,type CommentDraftState } from '../documents/IssueComments';
import { attachmentIds,fromPlainText,type RichBody } from '../documents/model';
import type { RichTextEditorProps } from '../documents/RichTextEditor';
import type { Session } from '../types';
let editor:RichTextEditorProps;let state:CommentDraftState;
vi.mock('../documents/Editor',()=>({StructuredEditor:(props:RichTextEditorProps)=>{editor=props;return <div aria-label={props.label}/>;}}));
const id='11111111-1111-4111-8111-111111111111';const added='22222222-2222-4222-8222-222222222222';
const attachment=(id:string)=>({type:'attachment',attrs:{id}});
const session:Session={audience:'web',user:{id:'u',display_name:'Example Owner',email:'owner@example.test'},csrf_token:'csrf',workspaces:[]};
const json=(data:unknown)=>new Response(JSON.stringify(data),{status:200});let container:HTMLDivElement;let root:Root;
beforeEach(()=>{vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT',true);container=document.createElement('div');document.body.append(container);root=createRoot(container);});afterEach(async()=>{await act(async()=>root.unmount());container.remove();vi.unstubAllGlobals();});
const post=()=>[...container.querySelectorAll<HTMLButtonElement>('button')].find(button=>button.textContent==='Post comment')!;
it('blocks posts while files are pending, then posts an attachment-only comment and preserves newer text without reusing its bound file',async()=>{
 let finish!:(value:Response)=>void;const fetch=vi.fn((_url:string,options:RequestInit)=>options.method==='POST'?new Promise<Response>(resolve=>{finish=resolve;}):Promise.resolve(json({data:[],has_more:false,next_cursor:null})));vi.stubGlobal('fetch',fetch);
 await act(async()=>root.render(<IssueComments workspaceId="w" issueId="i" teamId="t" session={session} readOnly={false} stateChanged={value=>{state=value;}} changed={()=>{}}/>));expect(editor.attachmentTarget).toEqual({team_id:'t'});
 const submitted:RichBody={type:'doc',content:[attachment(id)]};await act(async()=>{editor.onChange(submitted);editor.attachmentsChanged?.({pending:true,busy:true,cancelAll:async()=>true});});expect(post().disabled).toBe(true);expect(state.dirty).toBe(true);expect(state.blocked).toBe(true);
 await act(async()=>editor.attachmentsChanged?.({pending:false,busy:false,cancelAll:async()=>true}));expect(post().disabled).toBe(false);await act(async()=>post().click());
 await act(async()=>editor.onChange({type:'doc',content:[...fromPlainText('New text').content,attachment(id),attachment(added)]}));
 await act(async()=>finish(json({id:'c',issue_id:'i',author:{id:'u',display_name:'Example Owner'},version:'1',etag:'"c1"',created_at:'2026-10-18T12:00:00Z',edited_at:null,deleted_at:null,can_edit:true,can_delete:true,document:{id:'d',owner_id:'c',owner_type:'comment',schema_version:1,schema_supported:true,body:submitted,plain_text:'',version:'1',etag:'"d1"',created_at:'2026-10-18T12:00:00Z',updated_at:'2026-10-18T12:00:00Z',can_edit:true}})));
 expect(attachmentIds(editor.value)).toEqual([added]);expect(editor.value.content[0].content?.[0].text).toBe('New text');expect(state.dirty).toBe(true);expect(container.textContent).toContain('Files stayed with the posted comment');const sent=JSON.parse(fetch.mock.calls.find(([,options])=>options.method==='POST')![1].body as string);expect(sent.body).toEqual(submitted);
});
