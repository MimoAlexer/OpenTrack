// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { FilterBuilder } from './FilterBuilder';
import { data, id, json, session, workspaceId } from './test-fixtures';
import type { StructuredView } from './model';

let container: HTMLDivElement; let root: Root;
const empty = (): StructuredView => ({ version: 1, filter: null, layout: 'list', sort: 'created', timezone: 'UTC', archived: false, limit: 50 });
beforeEach(() => { vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true); container = document.createElement('div'); document.body.append(container); root = createRoot(container); });
afterEach(async () => { await act(async () => root.unmount()); container.remove(); vi.restoreAllMocks(); vi.unstubAllGlobals(); });
async function flush() { await act(async () => { await new Promise(resolve => setTimeout(resolve, 5)); }); }
async function click(element: Element) { await act(async () => (element as HTMLElement).click()); await flush(); }
function button(text: string) { const value = [...container.querySelectorAll('button')].find(button => button.textContent === text); if (!value) throw new Error(`Missing button ${text}`); return value; }
async function select(label: string, value: string) { const element = container.querySelector<HTMLSelectElement>(`select[aria-label="${label}"]`)!; await act(async () => { element.value = value; element.dispatchEvent(new Event('change', { bubbles: true })); }); }
async function input(label: string, value: string) { const element = container.querySelector<HTMLInputElement>(`input[aria-label="${label}"]`)!; await act(async () => { Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(element, value); element.dispatchEvent(new Event('input', { bubbles: true })); }); }
async function render(initial = empty()) {
  const apply = vi.fn((_view: StructuredView) => true); const clear = vi.fn();
  await act(async () => root.render(<FilterBuilder workspaceId={workspaceId} data={data} session={session} initial={initial} active={false} apply={apply} clear={clear}/>));
  await click(container.querySelector('summary')!);
  return apply;
}

it('builds nested All/Any conditions with visible meanings rather than raw query text', async () => {
  const apply = await render();
  await click(button('Add condition'));
  await click(button('Add condition'));
  await select('Group 1 match', 'any');
  await select('Condition 1.2 field', 'due_date');
  await select('Condition 1.2 operator', 'before');
  await input('Condition 1.2 date 1 days', '1');
  await click(button('Add nested group'));
  await select('Condition 1.3.1 field', 'assignee');
  await select('Condition 1.3.1 operator', 'is_null');
  expect(container.textContent).toContain('0 is today in the query timezone.');
  await click(button('Apply filters'));
  expect(apply).toHaveBeenCalledWith({ ...empty(), filter: { kind: 'any', children: [
    { kind: 'predicate', field: 'priority', op: 'eq', value: 3 },
    { kind: 'predicate', field: 'due_date', op: 'before', value: { relative_days: 1 } },
    { kind: 'any', children: [{ kind: 'predicate', field: 'assignee', op: 'is_null', value: true }] },
  ] } });
});

it('preserves a DST local day and an exact microsecond instant without browser timezone conversion', async () => {
  const apply = await render({ ...empty(), timezone: 'Europe/Zurich', filter: { kind: 'predicate', field: 'created_at', op: 'after', value: '2026-03-29' } });
  expect(container.querySelector<HTMLInputElement>('input[type="date"]')?.value).toBe('2026-03-29');
  expect(container.textContent).toContain('After starts at the next local day.');
  await select('Condition 1 date 1 meaning', 'instant');
  const instant = '2026-03-29T12:00:00.123456+02:00';
  await input('Condition 1 date 1 value', instant);
  await click(button('Apply filters'));
  expect(apply.mock.calls[0][0].filter).toEqual({ kind: 'predicate', field: 'created_at', op: 'after', value: instant });
});

it('shows null and label-not-in distinctions and rejects an empty value set', async () => {
  const apply = await render({ ...empty(), filter: { kind: 'predicate', field: 'assignee', op: 'not_in', values: [session.user.id] } });
  expect(container.textContent).toContain('Empty values are excluded.');
  await select('Condition 1 field', 'label');
  await select('Condition 1 operator', 'not_in');
  expect(container.textContent).toContain('including issues without labels');
  expect(button('Apply filters').disabled).toBe(true);
  await click(button('Apply filters'));
  expect(apply).not.toHaveBeenCalled();
});

it('uses paged named project choices and preserves previously selected values', async () => {
  const calls: string[] = [];
  vi.stubGlobal('fetch', vi.fn(async (url: string) => { calls.push(url); return json({ data: [{ id: id(calls.length === 1 ? 20 : 21), name: calls.length === 1 ? 'Launch project' : 'Support project' }], has_more: calls.length === 1, next_cursor: calls.length === 1 ? 'next-options' : null }); }));
  const apply = await render({ ...empty(), filter: { kind: 'predicate', field: 'project', op: 'in', values: [] } });
  await click(container.querySelector('button[aria-haspopup="listbox"]')!);
  await click([...container.querySelectorAll('[role="option"]')].find(option => option.textContent?.includes('Launch project'))!);
  const more = [...container.querySelectorAll('button')].find(value => value.textContent?.includes('Load more'))!;
  await click(more);
  await click([...container.querySelectorAll('[role="option"]')].find(option => option.textContent?.includes('Support project'))!);
  await click(button('Apply filters'));
  expect(calls[1]).toContain('cursor=next-options');
  expect(container.textContent).not.toContain(id(20));
  expect(apply.mock.calls[0][0].filter).toEqual({ kind: 'predicate', field: 'project', op: 'in', values: [id(20), id(21)] });
});

it('does not accept a stale option page after the browsing team changes', async () => {
  let resolveOld!: (response: Response) => void;
  vi.stubGlobal('fetch', vi.fn((url: string) => url.includes(`/teams/${id(2)}/`) ? new Promise<Response>(resolve => { resolveOld = resolve; }) : Promise.resolve(json({ data: [{ id: id(31), name: 'Current team project' }], has_more: false, next_cursor: null }))));
  await render({ ...empty(), filter: { kind: 'predicate', field: 'project', op: 'eq', value: '' } });
  await click(container.querySelector('button[aria-haspopup="listbox"]')!);
  await select('Condition 1 browse team', id(6));
  await flush();
  await act(async () => resolveOld(json({ data: [{ id: id(30), name: 'Stale private project' }], has_more: false, next_cursor: null })));
  expect(container.textContent).not.toContain('Stale private project');
  expect(container.textContent).toContain('Current team project');
});
