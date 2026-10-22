// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { MentionProvider, useMentions } from './context';
import { MentionText } from './MentionText';
import { useMentionSuggestions } from './useMentionSuggestions';
import type { MentionReference } from './types';
import type { Session } from '../types';
let container: HTMLDivElement; let root: Root;
const session: Session = { audience: 'web', user: { id: 'user', display_name: 'Example Owner', email: 'owner@example.test' }, csrf_token: 'csrf', workspaces: [] };
const id = '11111111-1111-4111-8111-111111111111';
const refs: MentionReference[] = [{ id, display_name: 'Private colleague', available: true }];
const json = (value: unknown, status = 200) => new Response(JSON.stringify(value), { status });
beforeEach(() => { vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true); container = document.createElement('div'); document.body.append(container); root = createRoot(container); });
afterEach(async () => { await act(async () => root.unmount()); container.remove(); vi.unstubAllGlobals(); });
function Projection({ workspace = 'w', current = session, references = refs, projectionScope = 'user:csrf' }: { workspace?: string; current?: Session; references?: MentionReference[]; projectionScope?: string }) { return <MentionProvider workspaceId={workspace} session={current} target={{ document_id: 'document' }} references={references} projectionScope={projectionScope}><MentionText id={id}/></MentionProvider>; }
it('clears old projected names synchronously across workspace and authentication changes', async () => {
  await act(async () => root.render(<Projection/>)); const node = container.querySelector('span'); expect(node?.textContent).toBe('@Private colleague');
  await act(async () => root.render(<Projection workspace="other"/>)); expect(container.textContent).toBe('@someone'); expect(container.querySelector('span')).toBe(node);
  await act(async () => root.render(<Projection workspace="other" references={[...refs]}/>)); expect(container.textContent).toBe('@Private colleague');
  await act(async () => root.render(<Projection workspace="other" references={refs} current={{ ...session, csrf_token: 'rotated' }}/>)); expect(container.textContent).toBe('@someone');
});
function Seed() { const context = useMentions(); return <><button onClick={() => context?.remember({ id, display_name: 'Candidate name' })}>Pick</button><MentionText id={id}/></>; }
it('keeps unsaved seeds scoped and gives an explicit unavailable server projection precedence', async () => {
  const render = (references: MentionReference[] = []) => root.render(<MentionProvider workspaceId="w" session={session} target={{ issue_id: 'i' }} references={references} projectionScope="user:csrf"><Seed/></MentionProvider>);
  await act(async () => render()); await act(async () => container.querySelector('button')!.click()); expect(container.textContent).toContain('@Candidate name');
  await act(async () => render([{ id, display_name: null, available: false }])); expect(container.textContent).toContain('@someone'); expect(container.textContent).not.toContain('Candidate name');
});
function Suggestions({ query }: { query: string }) { const value = useMentionSuggestions(query); return <div aria-busy={value.loading}>{value.choices.map(person => <span key={person.id}>{person.display_name}</span>)}{value.error ? 'Unavailable' : ''}</div>; }
it('discards late lookup responses and denied results while keeping exact document scope', async () => {
  let first: (value: Response) => void = () => {}; const calls: string[] = [];
  vi.stubGlobal('fetch', vi.fn((url: string) => { calls.push(url); if (url.includes('q=Ex&')) return new Promise<Response>(resolve => { first = resolve; }); return Promise.resolve(json({ code: 'forbidden', detail: 'Unavailable' }, 403)); }));
  const render = (query: string) => root.render(<MentionProvider workspaceId="w" session={session} target={{ document_id: 'private-comment-document' }}><Suggestions query={query}/></MentionProvider>);
  await act(async () => render('Ex')); await act(async () => new Promise(resolve => setTimeout(resolve, 180)));
  await act(async () => render('Be')); await act(async () => new Promise(resolve => setTimeout(resolve, 180))); await act(async () => first(json({ data: [{ id, display_name: 'Old private result' }], has_more: false, next_cursor: null })));
  expect(container.textContent).toBe('Unavailable'); expect(container.textContent).not.toContain('Old private result'); expect(calls.every(url => url.includes('document_id=private-comment-document') && !url.includes('team_id'))).toBe(true);
});
it('resolves equivalent UUID casing without changing persisted node attributes', async () => {
  const lower = 'abcdefab-abcd-4abc-8abc-abcdefabcdef';
  await act(async () => root.render(<MentionProvider workspaceId="w" session={session} target={{ issue_id: 'i' }} references={[{ id: lower, display_name: 'Current name', available: true }]} projectionScope="user:csrf"><MentionText id={lower.toUpperCase()}/></MentionProvider>));
  expect(container.textContent).toBe('@Current name');
});
