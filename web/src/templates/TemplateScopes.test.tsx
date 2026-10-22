// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { TemplateChooser } from './TemplateChooser';
import { fromPlainText } from '../documents/model';
import type { Session } from '../types';
import type { TemplatePreview } from './types';
const session: Session = { audience: 'web', user: { id: 'u', display_name: 'Example Owner', email: 'owner@example.test' }, csrf_token: 'old', workspaces: [] };
const preview = (team = 'private'): TemplatePreview => ({ template: { id: `template-${team}`, team_id: team, name: `${team} template`, version: '1', etag: '"1"', archived_at: null, updated_at: '2026-10-18T12:00:00Z', can_manage: false, can_use: true, title_mode: 'literal', title: `${team} title`, defaults: { status_id: null, priority: 0, label_ids: [] }, document: { schema_version: 1, schema_supported: true, body: fromPlainText(`${team} body`), plain_text: `${team} body` } }, materialized_title: `${team} title`, title_context_at: '2026-10-20T12:00:00Z', resolved_status: null, labels: [], diagnostics: [], can_create: true });
const json = (data: unknown, status = 200) => new Response(JSON.stringify(data), { status });
let container: HTMLDivElement; let root: Root;
beforeEach(() => { vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true); HTMLDialogElement.prototype.showModal = function () { this.setAttribute('open', ''); }; HTMLDialogElement.prototype.close = function () { this.removeAttribute('open'); }; container = document.createElement('div'); document.body.append(container); root = createRoot(container); });
afterEach(async () => { await act(async () => root.unmount()); container.remove(); vi.unstubAllGlobals(); });
function surface(team = 'private', account = session) { return <TemplateChooser workspaceId="w" teamId={team} session={account} selection={null} selectedName="" disabled={false} pendingUploads={false} edited={false} needsReview={false} apply={() => null} review={() => {}} clear={() => {}}/>; }
async function click(text: string) { await act(async () => [...container.querySelectorAll<HTMLButtonElement>('button')].find(button => button.textContent === text)!.click()); }
it('ignores a late private-template preview after the team changes', async () => {
  let finish!: (response: Response) => void;
  vi.stubGlobal('fetch', vi.fn((url: string) => url.endsWith('/preview') ? new Promise<Response>(resolve => { finish = resolve; }) : Promise.resolve(json({ data: [preview(url.includes('/private/') ? 'private' : 'other').template], next_cursor: null, has_more: false }))));
  await act(async () => root.render(surface())); await click('Use a template'); await click('private templatev1');
  await act(async () => root.render(surface('other'))); await act(async () => finish(json(preview())));
  expect(container.textContent).not.toContain('private title'); expect(container.textContent).not.toContain('private body'); expect(container.textContent).not.toContain('private template'); expect(container.textContent).toContain('other template');
});
it('withholds prior private preview during reauthentication and after a denied refresh', async () => {
  let denied = false; let finish!: (response: Response) => void;
  vi.stubGlobal('fetch', vi.fn((url: string) => url.endsWith('/preview') ? denied ? new Promise<Response>(resolve => { finish = resolve; }) : Promise.resolve(json(preview())) : Promise.resolve(json({ data: [preview().template], next_cursor: null, has_more: false }))));
  await act(async () => root.render(surface())); await click('Use a template'); await click('private templatev1'); expect(container.textContent).toContain('private body');
  denied = true; await act(async () => root.render(surface('private', { ...session, csrf_token: 'new' }))); expect(container.textContent).not.toContain('private body'); expect(container.textContent).not.toContain('private title');
  await act(async () => finish(json({ code: 'not_found', detail: 'Template unavailable.' }, 404))); expect(container.textContent).toContain('Template unavailable.'); expect(container.querySelector('.template-preview')).toBeNull();
});
it('invalidates a loaded preview when the team template directory denies access', async () => {
  let denied = false;
  vi.stubGlobal('fetch', vi.fn((url: string) => Promise.resolve(url.endsWith('/preview') ? json(preview()) : denied ? json({ code: 'forbidden', detail: 'Template access changed.' }, 403) : json({ data: [preview().template], next_cursor: null, has_more: false }))));
  await act(async () => root.render(surface())); await click('Use a template'); await click('private templatev1'); expect(container.textContent).toContain('private body');
  denied = true; await act(async () => container.querySelector<HTMLButtonElement>('[aria-label="Refresh templates"]')!.click());
  expect(container.textContent).not.toContain('private body'); expect(container.textContent).not.toContain('private title'); expect(container.querySelector('.template-preview')).toBeNull();
  expect([...container.querySelectorAll<HTMLButtonElement>('button')].find(button => button.textContent === 'Use template')!.disabled).toBe(true);
});
it('keeps the same authorized preview mounted during refresh and transient failure while disabling application', async () => {
  let pending = false; let finish!: (response: Response) => void;
  vi.stubGlobal('fetch', vi.fn((url: string) => url.endsWith('/preview') && pending ? new Promise<Response>(resolve => { finish = resolve; }) : Promise.resolve(json(url.endsWith('/preview') ? preview() : { data: [preview().template], next_cursor: null, has_more: false }))));
  await act(async () => root.render(surface())); await click('Use a template'); await click('private templatev1'); const panel = container.querySelector('.template-preview')!;
  pending = true; await act(async () => container.querySelector<HTMLButtonElement>('[aria-label="Refresh templates"]')!.click());
  expect(container.querySelector('.template-preview')).toBe(panel); expect(container.textContent).toContain('private body'); expect([...container.querySelectorAll<HTMLButtonElement>('button')].find(button => button.textContent === 'Use template')!.disabled).toBe(true);
  await act(async () => finish(json({ code: 'unavailable', detail: 'Preview refresh failed.' }, 503))); expect(container.querySelector('.template-preview')).toBe(panel); expect(container.textContent).toContain('private body'); expect([...container.querySelectorAll<HTMLButtonElement>('button')].find(button => button.textContent === 'Use template')!.disabled).toBe(true);
  await click('Try again'); const next = preview(); next.template.document = { ...next.template.document, body: fromPlainText('Updated body'), plain_text: 'Updated body' }; await act(async () => finish(json(next)));
  expect(container.querySelector('.template-preview')).toBe(panel); expect(container.textContent).toContain('Updated body'); expect([...container.querySelectorAll<HTMLButtonElement>('button')].find(button => button.textContent === 'Use template')!.disabled).toBe(false);
});
