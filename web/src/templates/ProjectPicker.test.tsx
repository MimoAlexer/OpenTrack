// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { ProjectPicker, projectSignature } from './ProjectPicker';
import type { Session } from '../types';

const session: Session = { audience: 'web', user: { id: 'u', display_name: 'Example Owner', email: 'owner@example.test' }, csrf_token: 'old', workspaces: [] };
const json = (value: unknown, status = 200) => new Response(JSON.stringify(value), { status });
const page = (team: string, name: string) => ({ data: [{ id: team, workspace_id: 'w', team_id: team, name, etag: '"1"', archived_at: null, can_edit: true }], has_more: false, next_cursor: null });
let container: HTMLDivElement; let root: Root;
beforeEach(() => { vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true); container = document.createElement('div'); document.body.append(container); root = createRoot(container); });
afterEach(async () => { await act(async () => root.unmount()); container.remove(); vi.unstubAllGlobals(); });
function surface(team: string, account = session) { return <ProjectPicker workspaceId="w" teamId={team} session={account} value={null} label="Project" disabled={false} change={() => {}}/>; }

it('does not display a late project response from a previous team', async () => {
  let finish!: (value: Response) => void;
  vi.stubGlobal('fetch', vi.fn((url: string) => url.includes('/private/') ? new Promise<Response>(resolve => { finish = resolve; }) : Promise.resolve(json(page('public', 'Public project')))));
  await act(async () => root.render(surface('private'))); await act(async () => root.render(surface('public')));
  await act(async () => finish(json(page('private', 'Private project'))));
  expect(container.textContent).toContain('Public project'); expect(container.textContent).not.toContain('Private project');
});

it('hides prior project names while reauthentication and a denied read are pending', async () => {
  let changed = false; let finish!: (value: Response) => void;
  vi.stubGlobal('fetch', vi.fn(() => changed ? new Promise<Response>(resolve => { finish = resolve; }) : Promise.resolve(json(page('private', 'Private project')))));
  await act(async () => root.render(surface('private'))); expect(container.textContent).toContain('Private project');
  changed = true; await act(async () => root.render(surface('private', { ...session, csrf_token: 'new' })));
  expect(container.textContent).not.toContain('Private project');
  await act(async () => finish(json({ code: 'forbidden', detail: 'Access changed.' }, 403)));
  expect(container.textContent).not.toContain('Private project'); expect(container.textContent).toContain('Projects could not be loaded');
});

it('retains and verifies a selected project after loading its later page', async () => {
  const verified = vi.fn(); const selected = { id: 'later', etag: '"1"' };
  vi.stubGlobal('fetch', vi.fn((url: string) => Promise.resolve(json(url.includes('cursor=next') ? { ...page('t', 'Later project'), data: [{ ...page('t', 'Later project').data[0], id: 'later' }] } : { data: [], has_more: true, next_cursor: 'next' }))));
  await act(async () => root.render(<ProjectPicker workspaceId="w" teamId="t" session={session} value={selected} label="Project" disabled={false} verified={verified} change={() => {}}/>));
  expect(container.textContent).toContain('Selected project unavailable');
  await act(async () => [...container.querySelectorAll('button')].find(item => item.textContent === 'Load more projects')!.click());
  expect(container.textContent).toContain('Later project'); expect(verified).toHaveBeenLastCalledWith(projectSignature('u:old', 'w', 't', selected));
});
