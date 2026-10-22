// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { IssueReferenceProvider, useIssueReferences } from './context';
import { IssueReferenceText } from './IssueReferenceText';
import { useReferenceSuggestions } from './useReferenceSuggestions';
import { useUnsavedChanges } from '../router';
import type { IssueReference } from './types';
import type { Session } from '../types';

let container: HTMLDivElement; let root: Root;
const session: Session = { audience: 'web', user: { id: 'user', display_name: 'Example Owner', email: 'owner@example.test' }, csrf_token: 'csrf', workspaces: [] };
const id = 'abcdefab-abcd-4abc-8abc-abcdefabcdef';
const refs: IssueReference[] = [{ id, key: 'SEC-1', title: 'Private issue', available: true }];
const json = (value: unknown, status = 200) => new Response(JSON.stringify(value), { status });
beforeEach(() => { vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true); container = document.createElement('div'); document.body.append(container); root = createRoot(container); });
afterEach(async () => { await act(async () => root.unmount()); container.remove(); vi.unstubAllGlobals(); vi.restoreAllMocks(); });
function Projection({ workspace = 'w', current = session, references = refs, projectionScope = 'user:csrf' }: { workspace?: string; current?: Session; references?: IssueReference[]; projectionScope?: string }) {
  return <IssueReferenceProvider workspaceId={workspace} session={current} target={{ document_id: 'document' }} references={references} projectionScope={projectionScope}><IssueReferenceText id={id.toUpperCase()}/></IssueReferenceProvider>;
}
it('removes title, key and navigation synchronously across workspace and authentication changes', async () => {
  await act(async () => root.render(<Projection/>)); expect(container.querySelector('a')?.title).toBe('SEC-1: Private issue');
  expect(container.querySelector('a')?.getAttribute('href')).toBe(`/w/w/issues/${id.toUpperCase()}`);
  await act(async () => root.render(<Projection workspace="other"/>)); expect(container.textContent).toBe('#issue'); expect(container.querySelector('a')).toBeNull();
  await act(async () => root.render(<Projection workspace="other" references={[...refs]}/>)); expect(container.textContent).toBe('SEC-1');
  await act(async () => root.render(<Projection workspace="other" references={refs} current={{ ...session, csrf_token: 'rotated' }}/>)); expect(container.textContent).toBe('#issue'); expect(container.innerHTML).not.toContain('Private issue');
});
function Seed() { const context = useIssueReferences(); return <><button onClick={() => context?.remember({ id, key: 'ENG-1', title: 'Candidate issue' })}>Pick</button><IssueReferenceText id={id}/></>; }
it('gives an explicit unavailable server projection precedence over an unsaved suggestion', async () => {
  const render = (references: IssueReference[] = []) => root.render(<IssueReferenceProvider workspaceId="w" session={session} target={{ issue_id: 'i' }} references={references} projectionScope="user:csrf"><Seed/></IssueReferenceProvider>);
  await act(async () => render()); await act(async () => container.querySelector('button')!.click()); expect(container.textContent).toContain('ENG-1');
  await act(async () => render([{ id, key: null, title: null, available: false }])); expect(container.textContent).toContain('#issue'); expect(container.querySelector('a')).toBeNull(); expect(container.innerHTML).not.toContain('Candidate issue');
});
function Suggestions({ query }: { query: string }) { const value = useReferenceSuggestions(query); return <div aria-busy={value.loading}>{value.choices.map(issue => <span key={issue.id}>{issue.title}</span>)}{value.error ? 'Unavailable' : ''}</div>; }
it('discards late lookup responses after scope denial and binds lookup to the exact source document', async () => {
  let first: (value: Response) => void = () => {}; const calls: string[] = [];
  vi.stubGlobal('fetch', vi.fn((url: string) => { calls.push(url); if (url.includes('q=ENG&')) return new Promise<Response>(resolve => { first = resolve; }); return Promise.resolve(json({ code: 'forbidden', detail: 'Unavailable' }, 403)); }));
  const render = (query: string) => root.render(<IssueReferenceProvider workspaceId="w" session={session} target={{ document_id: 'private-comment' }}><Suggestions query={query}/></IssueReferenceProvider>);
  await act(async () => render('ENG')); await act(async () => new Promise(resolve => setTimeout(resolve, 180)));
  await act(async () => render('SEC')); await act(async () => new Promise(resolve => setTimeout(resolve, 180))); await act(async () => first(json({ data: [{ id, key: 'SEC-1', title: 'Old private result' }], has_more: false, next_cursor: null })));
  expect(container.textContent).toBe('Unavailable'); expect(calls.every(url => url.includes('document_id=private-comment') && !url.includes('team_id'))).toBe(true);
});
function Draft() { useUnsavedChanges(true); return <Projection/>; }
it('uses the existing unsaved-draft guard before following an issue reference', async () => {
  history.replaceState(history.state, '', '/w/w/issues/source'); const confirm = vi.spyOn(window, 'confirm').mockReturnValue(false);
  await act(async () => root.render(<Draft/>)); await act(async () => container.querySelector('a')!.click());
  expect(confirm).toHaveBeenCalled(); expect(location.pathname).toBe('/w/w/issues/source');
  confirm.mockReturnValue(true); await act(async () => container.querySelector('a')!.click()); expect(location.pathname).toBe(`/w/w/issues/${id.toUpperCase()}`);
});
