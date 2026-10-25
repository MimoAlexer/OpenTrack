// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { IssueComments, type CommentDraftState } from './IssueComments';
import { fromPlainText, plainText } from './model';
import type { IssueComment } from './types';
import type { Session } from '../types';
vi.mock('./Editor', () => ({ StructuredEditor: ({ value, onChange, label, disabled }: any) => <textarea aria-label={label} value={plainText(value)} disabled={disabled} onChange={e => onChange(fromPlainText(e.target.value))} /> }));
const session: Session = { audience: 'web', user: { id: 'u', display_name: 'Example Owner', email: 'owner@example.test' }, csrf_token: 'csrf', workspaces: [{ id: 'w', name: 'Workspace', slug: 'workspace', role: 'member' }] };
const comment = (id:string,text:string,author='u'):IssueComment => ({id,issue_id:'i',author:{id:author,display_name:author==='u'?'Example Owner':'Other person'},version:'1',etag:`"comment:${id}:1"`,created_at:`2026-10-18T12:${id==='old'?'00':'30'}:00Z`,edited_at:null,deleted_at:null,can_edit:true,can_delete:true,document:{mentions:[],issue_references:[],id:`doc-${id}`,owner_id:id,owner_type:'comment',body:fromPlainText(text),plain_text:text,version:'1',etag:`"document:${id}:1"`,schema_supported:true,schema_version:1,can_edit:true,created_at:'2026-10-18T12:00:00Z',updated_at:'2026-10-18T12:00:00Z'}});
const json=(value:unknown,status=200)=>new Response(JSON.stringify(value),{status}); const page=(data:IssueComment[],cursor:string|null=null)=>({data,has_more:!!cursor,next_cursor:cursor});
let container:HTMLDivElement;let root:Root;let state:CommentDraftState;
beforeEach(()=>{history.replaceState(history.state,'','/w/w/issues/i');vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT',true);container=document.createElement('div');document.body.append(container);root=createRoot(container); HTMLDialogElement.prototype.showModal=function(){this.setAttribute('open','');}; HTMLDialogElement.prototype.close=function(){this.removeAttribute('open');};});
afterEach(async()=>{await act(async()=>root.unmount());container.remove();vi.unstubAllGlobals();});
async function render(token='csrf'){await act(async()=>root.render(<IssueComments workspaceId="w" issueId="i" session={{...session,csrf_token:token}} readOnly={false} stateChanged={value=>{state=value;}} changed={()=>{}}/>));}
function buttons(label:string){return [...container.querySelectorAll<HTMLButtonElement>('button')].filter(button=>button.textContent===label);}
async function click(label:string,index=0){await act(async()=>buttons(label)[index].click());}
async function type(label:string,text:string){const el=container.querySelector<HTMLTextAreaElement>(`textarea[aria-label="${label}"]`)!;await act(async()=>{Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype,'value')!.set!.call(el,text);el.dispatchEvent(new Event('input',{bubbles:true}));});}
it.each(['edit', 'delete'])('keeps a rejected supplementary moderation %s reason and repairs it with the original comment precondition', async mode => {
  const original = comment('a', 'Other member comment', 'other'); let current = original; const writes: RequestInit[] = [];
  const reason = '\u{1F680}'.repeat(500); const draft = 'Manager draft';
  vi.stubGlobal('fetch', vi.fn((url: string, options: RequestInit) => {
    if (options.method === 'PATCH' || options.method === 'DELETE') {
      writes.push(options);
      if (writes.length === 1) return Promise.resolve(json({ code: 'validation_failed', detail: 'Moderation reason must contain 1–500 characters without control characters' }, 422));
      current = { ...original, version: '2', etag: '"comment:a:2"', ...(mode === 'delete' ? { deleted_at: '2026-10-25T12:00:00Z', document: null, can_edit: false, can_delete: false } : { document: { ...original.document!, version: '2', body: fromPlainText(draft), plain_text: draft } }) };
      return Promise.resolve(mode === 'delete' ? new Response(null, { status: 204 }) : json(current));
    }
    return Promise.resolve(json(url.endsWith('/comments/a') ? current : page([current])));
  }));
  await render(); await click(mode === 'edit' ? 'Edit comment' : 'Delete comment');
  if (mode === 'edit') await type('Edit comment', draft);
  const action = mode === 'edit' ? 'Save comment' : 'Delete comment';
  const reasonInput = container.querySelector<HTMLInputElement>(mode === 'edit' ? '.moderation-reason input' : 'dialog input')!;
  const fillReason = async (value: string) => { await act(async () => { Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(reasonInput, value); reasonInput.dispatchEvent(new Event('input', { bubbles: true })); }); };
  const submitAction = () => click(action, mode === 'delete' ? 1 : 0);
  expect(buttons(action)[mode === 'delete' ? 1 : 0].disabled).toBe(true);
  await fillReason('   '); expect(buttons(action)[mode === 'delete' ? 1 : 0].disabled).toBe(true);
  await fillReason(reason + '\u{1F680}'); await submitAction();
  expect(reasonInput.value).toBe(reason + '\u{1F680}'); expect(reasonInput.disabled).toBe(false);
  expect(container.textContent).toContain('Moderation reason must contain'); expect(current).toEqual(original);
  if (mode === 'edit') expect(container.querySelector<HTMLTextAreaElement>('[aria-label="Edit comment"]')!.value).toBe(draft);
  await fillReason(reason); await submitAction(); expect(writes).toHaveLength(2);
  for (const [index, write] of writes.entries()) {
    expect(write.method).toBe(mode === 'edit' ? 'PATCH' : 'DELETE'); expect(new Headers(write.headers).get('If-Match')).toBe(original.etag);
    expect(JSON.parse(write.body as string)).toEqual({ ...(mode === 'edit' ? { schema_version: 1, body: fromPlainText(draft) } : {}), moderation_reason: reason + (index === 0 ? '\u{1F680}' : '') });
  }
  expect(container.querySelector(mode === 'edit' ? '[aria-label="Edit comment"]' : 'dialog')).toBeNull(); expect(current.version).toBe('2');
});

it.each(['edit', 'delete'])('keeps ordinary author %s requests free of a moderation reason', async mode => {
  let current = comment('a', 'Own comment'); const writes: RequestInit[] = [];
  vi.stubGlobal('fetch', vi.fn((url: string, options: RequestInit) => {
    if (options.method === 'PATCH' || options.method === 'DELETE') { writes.push(options); current = { ...current, version: '2', ...(mode === 'delete' ? { deleted_at: '2026-10-25T12:00:00Z', document: null, can_edit: false, can_delete: false } : { document: { ...current.document!, body: fromPlainText('Author edit'), plain_text: 'Author edit' } }) }; return Promise.resolve(mode === 'delete' ? new Response(null, { status: 204 }) : json(current)); }
    return Promise.resolve(json(url.endsWith('/comments/a') ? current : page([current])));
  }));
  await render(); await click(mode === 'edit' ? 'Edit comment' : 'Delete comment');
  expect(container.querySelector('.moderation-reason input,dialog input')).toBeNull();
  if (mode === 'edit') await type('Edit comment', 'Author edit');
  await click(mode === 'edit' ? 'Save comment' : 'Delete comment', mode === 'delete' ? 1 : 0);
  expect(writes).toHaveLength(1); expect(writes[0].body ? JSON.parse(writes[0].body as string) : {}).not.toHaveProperty('moderation_reason');
});

it('offers no moderation actions when current server permissions deny both commands', async () => {
  const denied = { ...comment('a', 'Visible member comment', 'other'), can_edit: false, can_delete: false };
  vi.stubGlobal('fetch', vi.fn(() => Promise.resolve(json(page([denied]))))); await render();
  expect(container.textContent).toContain('Visible member comment'); expect(buttons('Edit comment')).toHaveLength(0); expect(buttons('Delete comment')).toHaveLength(0);
});

describe('comment drafts and durable commands',()=>{
 it('retries an unknown create with the exact same key/body and one visible comment',async()=>{let first=true;const fetch=vi.fn((_url:string,opts:RequestInit)=>{if(opts.method==='POST'){if(first){first=false;return Promise.reject(new Error('Disconnected'));}return Promise.resolve(json(comment('posted','My comment')));}return Promise.resolve(json(page([])));});vi.stubGlobal('fetch',fetch);await render();await type('Comment','My comment');await click('Post comment');expect(state.blocked).toBe(true);expect(container.querySelector<HTMLTextAreaElement>('[aria-label="Comment"]')!.value).toBe('My comment');await click('Retry safely');const writes=fetch.mock.calls.filter(([,opts])=>opts.method==='POST');expect(new Headers(writes[0][1].headers).get('Idempotency-Key')).toBe(new Headers(writes[1][1].headers).get('Idempotency-Key'));expect(writes[0][1].body).toBe(writes[1][1].body);expect(container.querySelectorAll('.comment')).toHaveLength(1);expect(state.dirty).toBe(false);});
 it('prevents another edit from discarding a dirty comment',async()=>{vi.stubGlobal('fetch',vi.fn(() => Promise.resolve(json(page([comment('a','First'),comment('b','Second')])))));await render();await click('Edit comment');await type('Edit comment','Unsaved first edit');expect(buttons('Edit comment').every(button=>button.disabled)).toBe(true);await click('Edit comment',1);expect(container.querySelector<HTMLTextAreaElement>('[aria-label="Edit comment"]')!.value).toBe('Unsaved first edit');});
 it('pins and reauthorizes an older editing row during same-user reauthentication',async()=>{const old=comment('old','Older comment');const recent=comment('recent','New comment');const fetch=vi.fn((url:string)=>Promise.resolve(json(url.includes('cursor=')?page([old]):url.endsWith('/comments/old')?old:page([recent],'cursor-1'))));vi.stubGlobal('fetch',fetch);await render();await click('Load older comments');await click('Edit comment');await type('Edit comment','Keep this old edit');const element=container.querySelector('[aria-label="Edit comment"]');await render('rotated-csrf');expect(fetch.mock.calls.some(([url])=>url.endsWith('/comments/old'))).toBe(true);expect(container.querySelector('[aria-label="Edit comment"]')).toBe(element);expect((element as HTMLTextAreaElement).value).toBe('Keep this old edit');expect(state.dirty).toBe(true);});
 it('keeps a comment draft when a refresh returns a remote tombstone',async()=>{const original=comment('a','Original');const fetch=vi.fn(() => Promise.resolve(json(page([original]))));vi.stubGlobal('fetch',fetch);await render();await click('Edit comment');await type('Edit comment','My unsaved edit');fetch.mockImplementation(() => Promise.resolve(json(page([{...original,deleted_at:'2026-10-18T13:00:00Z',version:'2',etag:'"deleted"',document:null,can_edit:false,can_delete:false}]))));await click('Refresh');expect(container.querySelector<HTMLTextAreaElement>('[aria-label="Edit comment"]')!.value).toBe('My unsaved edit');expect(buttons('Save comment')[0].disabled).toBe(true);expect(container.textContent).toContain('Your draft is preserved');});
 it('requires a moderation reason and sends the comment representation tag',async()=>{const original=comment('a','Other comment','other');const fetch=vi.fn((_url:string,opts:RequestInit)=>Promise.resolve(json(opts.method==='PATCH'?{...comment('a','Edited','other'),version:'2',etag:'"comment:a:2"'}:page([original]))));vi.stubGlobal('fetch',fetch);await render();await click('Edit comment');await type('Edit comment','Edited');expect(buttons('Save comment')[0].disabled).toBe(true);const reason=container.querySelector<HTMLInputElement>('.moderation-reason input')!;await act(async()=>{Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value')!.set!.call(reason,'Remove personal information');reason.dispatchEvent(new Event('input',{bubbles:true}));});await click('Save comment');const write=fetch.mock.calls.find(([,opts])=>opts.method==='PATCH')!;expect(new Headers(write[1].headers).get('If-Match')).toBe(original.etag);expect(JSON.parse(write[1].body as string).moderation_reason).toBe('Remove personal information');expect(container.querySelector('[aria-label="Edit comment"]')).toBeNull();});
 it('resolves an older permalink once and does not steal composer focus on reauthentication',async()=>{const id='00000000-0000-4000-8000-000000000123';const linked=comment(id,'Linked older comment');history.replaceState(history.state,'',`/w/w/issues/i#comment-${id}`);const fetch=vi.fn((url:string)=>Promise.resolve(json(url.endsWith(`/comments/${id}`)?linked:page([comment('recent','Recent comment')]))));vi.stubGlobal('fetch',fetch);await render();const target=container.querySelector(`#comment-${id}`)!;expect(document.activeElement).toBe(target);const composer=container.querySelector<HTMLTextAreaElement>('[aria-label="Comment"]')!;composer.focus();await type('Comment','Continue typing');await render('rotated');expect(document.activeElement).toBe(composer);expect(composer.value).toBe('Continue typing');expect(fetch.mock.calls.some(([url])=>url.endsWith(`/comments/${id}`))).toBe(true);});

 it('renders an unsupported comment schema as its plaintext projection even when JSON nodes look familiar',async()=>{const future=comment('a','JSON interpretation');future.document={...future.document!,schema_version:2,schema_supported:false,plain_text:'Safe server text'};vi.stubGlobal('fetch',vi.fn(() => Promise.resolve(json(page([future])))));await render();expect(container.querySelector('.comment-content pre')?.textContent).toBe('Safe server text');expect(container.textContent).not.toContain('JSON interpretation');});

});
it('retains a newly posted comment when an older refresh resolves after the write', async () => {
  let resolveRefresh: (value: Response) => void = () => {}; let reads = 0;
  vi.stubGlobal('fetch', vi.fn((url: string, options: RequestInit) => {
    if (url.endsWith('/subscription')) return Promise.resolve(json({ issue_id: 'i', preference: 'automatic', sources: [], subscribed: false, version: '0', etag: '"sub:0"', can_change: true }));
    if (options.method === 'POST') return Promise.resolve(json(comment('new', 'New confirmed comment')));
    if (++reads === 2) return new Promise<Response>(resolve => { resolveRefresh = resolve; });
    return Promise.resolve(json(page([comment('old', 'Original comment')])));
  }));
  await render(); await click('Refresh'); await type('Comment', 'New confirmed comment'); await click('Post comment');
  await act(async () => resolveRefresh(json(page([comment('old', 'Original comment')]))));
  expect(container.querySelectorAll('.comment')).toHaveLength(2); expect(container.textContent).toContain('New confirmed comment'); expect(container.querySelector<HTMLTextAreaElement>('[aria-label="Comment"]')!.value).toBe(''); expect(state.dirty).toBe(false);
});
it('closes only the open comment actions menu on Escape and returns focus to its summary', async () => {
  vi.stubGlobal('fetch', vi.fn(() => Promise.resolve(json(page([comment('a', 'Comment')]))))); await render();
  const details = container.querySelector<HTMLDetailsElement>('.comment-menu')!; const summary = details.querySelector('summary')!; details.open = true; const outer = vi.fn(); window.addEventListener('keydown', outer);
  await act(async () => buttons('Edit comment')[0].dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true })));
  expect(details.open).toBe(false); expect(document.activeElement).toBe(summary); expect(outer).not.toHaveBeenCalled(); expect(container.querySelector('[aria-label="Comment"]')).not.toBeNull(); window.removeEventListener('keydown', outer);
});
it.each(['edit', 'delete'])('keeps a confirmed %s when an older page resolves late', async action => {
  const id = '00000000-0000-4000-8000-000000000123'; let record = comment(id, 'Original pinned comment'); let finish: (response: Response) => void = () => {};
  history.replaceState(history.state, '', `/w/w/issues/i#comment-${id}`);
  vi.stubGlobal('fetch', vi.fn((url: string, options: RequestInit) => {
    if (url.endsWith('/subscription')) return Promise.resolve(json({ issue_id:'i', preference:'automatic', sources:[], subscribed:false, version:'0', etag:'"s:0"', can_change:true }));
    if (url.includes('cursor=')) return new Promise<Response>(resolve => { finish = resolve; });
    if (options.method === 'PATCH') { record = { ...comment(id, 'Confirmed edit'), version:'2', etag:'"c:2"' }; return Promise.resolve(json(record)); }
    if (options.method === 'DELETE') { record = { ...record, version:'2', etag:'"c:2"', deleted_at:'2026-10-18T14:00:00Z', document:null, can_edit:false, can_delete:false }; return Promise.resolve(new Response(null,{status:204})); }
    return Promise.resolve(json(url.endsWith(`/comments/${id}`) ? record : page([comment('recent','Recent')], 'older')));
  }));
  await render(); await click('Load older comments');
  if (action === 'edit') { await click('Edit comment'); await type('Edit comment','Confirmed edit'); await click('Save comment'); }
  else { await click('Delete comment'); await click('Delete comment',2); }
  await act(async () => finish(json(page([comment(id,'Original pinned comment')]))));
  const row = container.querySelector(`#comment-${id}`)!; expect(row.textContent).not.toContain('Original pinned comment'); expect(row.textContent).toContain(action === 'edit' ? 'Confirmed edit' : 'Comment deleted.');
});
it.each(['2','3'])('prefers a fetched version %s projection over an equal or older local commit', async version => {
  let finish: (response: Response) => void = () => {}; let reads=0;
  vi.stubGlobal('fetch', vi.fn((url:string,options:RequestInit) => {
    if (url.endsWith('/subscription')) return Promise.resolve(json({issue_id:'i',preference:'automatic',sources:[],subscribed:false,version:'0',etag:'"s:0"',can_change:true}));
    if (options.method === 'POST') return Promise.resolve(json({...comment('new','Local receipt'),version:'2'}));
    if (++reads === 2) return new Promise<Response>(resolve => { finish=resolve; });
    return Promise.resolve(json(page([])));
  }));
  await render(); await click('Refresh'); await type('Comment','Local receipt'); await click('Post comment');
  await act(async () => finish(json(page([{...comment('new','Authoritative result'),version,can_edit:false,can_delete:false}]))));
  expect(container.textContent).toContain('Authoritative result'); expect(container.textContent).not.toContain('Local receipt'); expect(buttons('Edit comment')).toHaveLength(0);
});
it('ignores an older page from the previous auth scope without changing new pagination', async () => {
  let finish: (response:Response)=>void=()=>{}; let signal: AbortSignal|undefined; let currentScope=false;
  const fetch=vi.fn((url:string,options:RequestInit)=>{
    if(url.endsWith('/subscription'))return Promise.resolve(json({issue_id:'i',preference:'automatic',sources:[],subscribed:false,version:'0',etag:'"s:0"',can_change:true}));
    if(url.includes('cursor=old')){signal=options.signal as AbortSignal;return new Promise<Response>(resolve=>{finish=resolve;});}
    return Promise.resolve(json(page([comment('current','Current authorized row')],currentScope?'new':'old')));
  });vi.stubGlobal('fetch',fetch);await render();await click('Load older comments');currentScope=true;await render('rotated');expect(signal?.aborted).toBe(true);
  await act(async()=>finish(json(page([comment('private','Old private result')]))));expect(container.textContent).not.toContain('Old private result');expect(buttons('Load older comments')[0].disabled).toBe(false);await click('Load older comments');expect(fetch.mock.calls.some(([url])=>url.includes('cursor=new'))).toBe(true);
});
it('reauthorizes a write completed after reauthentication before showing its old projection', async()=>{
  let finish:(response:Response)=>void=()=>{};let checked=0;
  vi.stubGlobal('fetch',vi.fn((url:string,options:RequestInit)=>{
    if(url.endsWith('/subscription'))return Promise.resolve(json({issue_id:'i',preference:'automatic',sources:[],subscribed:false,version:'0',etag:'"s:0"',can_change:true}));
    if(options.method==='POST')return new Promise<Response>(resolve=>{finish=resolve;});
    if(url.endsWith('/comments/posted')){checked++;return Promise.resolve(json({...comment('posted','Reauthorized text'),author:{id:'u',display_name:'Current author'}}));}
    return Promise.resolve(json(page([])));
  }));await render();await type('Comment','Submitted text');await click('Post comment');await render('rotated');await act(async()=>finish(json({...comment('posted','Old response text'),author:{id:'u',display_name:'Old private author'}})));
  expect(checked).toBe(1);expect(container.textContent).toContain('Reauthorized text');expect(container.textContent).not.toContain('Old private author');expect(container.textContent).not.toContain('Old response text');expect(state.dirty).toBe(false);
});
