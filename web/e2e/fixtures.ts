import { randomUUID } from 'node:crypto';
import { test as base, expect, type APIRequestContext, type Page } from '@playwright/test';
import type { Bootstrap, Issue, Session } from '../src/types';
import type { StructuredDocument } from '../src/documents/types';
import type { RichBody } from '../src/documents/model';

export const paragraph = (text: string): RichBody => ({ type: 'doc', content: [{ type: 'paragraph', content: [{ type: 'text', text }] }] });
export const native = (body: RichBody) => JSON.stringify({ format: 'opentrack.document', format_version: 1, schema_version: 1, body });

export class SavedWorkspace {
  constructor(readonly http: APIRequestContext, readonly session: Session, readonly data: Bootstrap, readonly origin: string) {}
  get workspace() { return this.session.workspaces[0].id; }
  get team() { return this.data.teams[0]; }
  get path() { return `/api/v1/workspaces/${this.workspace}`; }
  async request<T>(path: string, method = 'GET', data?: unknown, headers: Record<string, string> = {}): Promise<T> {
    const response = await this.http.fetch(`${this.path}${path}`, { method, data,
      headers: { Origin: this.origin, ...(method !== 'GET' ? { 'X-CSRF-Token': this.session.csrf_token } : {}), ...headers } });
    if (!response.ok()) throw new Error(`Saved fixture request failed: ${method} ${path.split('?')[0]} HTTP ${response.status()}`);
    return response.json() as Promise<T>;
  }
  create(title: string, body = paragraph('Original saved description')) {
    return this.request<Issue>('/issues', 'POST', { team_id: this.team.id, title, document: { schema_version: 1, body } }, { 'Idempotency-Key': randomUUID() });
  }
  issue(id: string) { return this.request<Issue>(`/issues/${id}`); }
  document(id: string) { return this.request<StructuredDocument>(`/issues/${id}/document`); }
  async patch(issue: Issue, patch: unknown) {
    return this.request<Issue>(`/issues/${issue.id}`, 'PATCH', patch, { 'If-Match': issue.etag });
  }
  async open(page: Page, issue?: Issue) {
    const response = await page.goto(`/w/${this.workspace}/issues${issue ? `/${issue.id}` : ''}`);
    expect(response?.ok(), 'Built frontend is served by the real Docker app').toBe(true);
    await expect(page.getByRole('button', { name: 'Search commands', exact: true })).toBeEnabled();
    if (issue) await expect(page.getByRole('textbox', { name: 'Description', exact: true })).toBeVisible();
    return response!;
  }
}

type WorkerFixtures = { saved: SavedWorkspace };
export const test = base.extend<Record<never, never>, WorkerFixtures>({
  saved: [async ({ playwright }, use) => {
    if (process.env.GITHUB_ACTIONS !== 'true' || process.env.RUNNER_OS !== 'Linux'
        || !process.env.OPENTRACK_BROWSER_SETUP_TOKEN || !process.env.OPENTRACK_BROWSER_PASSWORD) {
      throw new Error('Saved browser workflows run only through the guarded GitHub Docker acceptance runner.');
    }
    const origin = process.env.OPENTRACK_BROWSER_URL!;
    const http = await playwright.request.newContext({ baseURL: origin, extraHTTPHeaders: { Origin: origin } });
    try {
      const status = await http.get('/api/v1/setup/status');
      if (!status.ok()) throw new Error('Fresh installation status is unavailable.');
      const setup = (await status.json() as { setup_required: boolean }).setup_required;
      const login = { email: 'owner@example.test', password: process.env.OPENTRACK_BROWSER_PASSWORD };
      const response = await http.post(setup ? '/api/v1/setup' : '/api/v1/auth/login', { data: setup
        ? { ...login, setup_token: process.env.OPENTRACK_BROWSER_SETUP_TOKEN, display_name: 'Example Owner',
          workspace_name: 'Browser acceptance', team_name: 'Example Team', team_key: 'EXA' }
        : login });
      if (!response.ok()) throw new Error(`Synthetic owner authentication failed: HTTP ${response.status()}`);
      const session = await response.json() as Session;
      const bootstrap = await http.get(`/api/v1/workspaces/${session.workspaces[0].id}/bootstrap`);
      if (!bootstrap.ok()) throw new Error('Synthetic workspace bootstrap failed.');
      await use(new SavedWorkspace(http, session, await bootstrap.json() as Bootstrap, origin));
    } finally { await http.dispose(); }
  }, { scope: 'worker' }],
  storageState: async ({ saved }, use) => { await use(await saved.http.storageState()); },
  context: async ({ context, baseURL }, use) => {
    // Defense in depth: test content cannot send browser requests off this origin.
    await context.route('**/*', async route => {
      if (new URL(route.request().url()).origin === new URL(baseURL!).origin) await route.continue();
      else await route.abort('blockedbyclient');
    });
    await use(context);
  },
});
export { expect };

export async function transfer(page: Page) {
  await page.getByRole('region', { name: 'Description', exact: true }).getByRole('button', { name: 'Import / export text' }).click();
  return page.getByRole('dialog', { name: 'Import and export text', exact: true });
}

export async function importText(page: Page, text: string, format: 'native' | 'markdown') {
  const dialog = await transfer(page);
  await dialog.getByRole('combobox', { name: 'Action', exact: true }).selectOption('import');
  await dialog.getByRole('combobox', { name: 'Import format', exact: true }).selectOption(format);
  await dialog.getByRole('textbox', { name: format === 'native' ? 'Document JSON' : 'Markdown', exact: true }).fill(text);
  await dialog.getByRole('button', { name: 'Review import', exact: true }).click();
  await expect(dialog.getByRole('region', { name: 'Import preview' })).toBeVisible();
  await dialog.getByRole('button', { name: 'Replace current draft', exact: true }).click();
  await expect(dialog).toHaveCount(0);
}

export async function exported(page: Page, format: 'native' | 'markdown') {
  const dialog = await transfer(page);
  await dialog.getByRole('combobox', { name: 'Format', exact: true }).selectOption(format);
  const text = await dialog.getByRole('textbox', { name: format === 'native' ? 'Exported document JSON' : 'Exported Markdown', exact: true }).inputValue();
  await dialog.getByRole('button', { name: 'Done', exact: true }).click();
  return text;
}

export async function savedBody(saved: SavedWorkspace, id: string, body: RichBody) {
  await expect.poll(async () => (await saved.document(id)).body, { message: 'The real API durably stores the reviewed body' }).toEqual(body);
}
