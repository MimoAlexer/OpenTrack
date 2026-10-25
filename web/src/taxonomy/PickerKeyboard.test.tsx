// @vitest-environment jsdom
import { act, useState } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { Picker } from './Picker';
import { ApiError } from '../api';

let container: HTMLDivElement; let root: Root;
const select = vi.fn(); const setOpen = vi.fn(); const parentKey = vi.fn();
beforeEach(async () => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true); select.mockClear(); setOpen.mockClear(); parentKey.mockClear();
  container = document.createElement('div'); document.body.append(container); root = createRoot(container);
  await act(async () => root.render(<div onKeyDown={parentKey}><Picker label="Assignee" value="Unassigned" open setOpen={setOpen} query="Example" setQuery={() => {}} choices={[{ id: 'first', text: 'First' }, { id: 'last', text: 'Last' }]} select={select} loading={false} error={null} refresh={() => {}} more={() => {}} hasMore={false} /></div>));
});
afterEach(async () => { await act(async () => root.unmount()); container.remove(); vi.unstubAllGlobals(); });
const input = () => container.querySelector<HTMLInputElement>('input')!;
async function key(value: string, options: KeyboardEventInit = {}, altGraph = false) {
  const event = new KeyboardEvent('keydown', { key: value, bubbles: true, cancelable: true, ...options });
  if (altGraph) Object.defineProperty(event, 'getModifierState', { value: (name: string) => name === 'AltGraph' });
  await act(async () => input().dispatchEvent(event)); return event;
}
const active = () => document.getElementById(input().getAttribute('aria-activedescendant')!)?.textContent;

it('preserves composition and modified text editing without moving, selecting or closing', async () => {
  await key('ArrowDown'); expect(active()).toBe('First'); parentKey.mockClear();
  for (const options of [{ isComposing: true }, { keyCode: 229 }, { altKey: true }, { ctrlKey: true }, { metaKey: true }, { shiftKey: true }]) {
    for (const value of ['ArrowDown', 'ArrowUp', 'Home', 'End', 'Enter', 'Escape']) {
      expect((await key(value, options)).defaultPrevented).toBe(false);
      expect(active()).toBe('First'); expect(select).not.toHaveBeenCalled(); expect(setOpen).not.toHaveBeenCalled();
    }
  }
  for (const value of ['ArrowDown', 'Home', 'End', 'Enter', 'Escape']) expect((await key(value, {}, true)).defaultPrevented).toBe(false);
  expect(active()).toBe('First'); expect(select).not.toHaveBeenCalled(); expect(setOpen).not.toHaveBeenCalled(); expect(parentKey).not.toHaveBeenCalled();
});

it('tracks composition lifetime even when the native key flag is absent, then resumes ordinary selection', async () => {
  await key('ArrowDown');
  await act(async () => input().dispatchEvent(new CompositionEvent('compositionstart', { bubbles: true })));
  for (const value of ['ArrowDown', 'End', 'Enter', 'Escape']) expect((await key(value)).defaultPrevented).toBe(false);
  expect(active()).toBe('First'); expect(select).not.toHaveBeenCalled(); expect(setOpen).not.toHaveBeenCalled();
  await act(async () => input().dispatchEvent(new CompositionEvent('compositionend', { bubbles: true })));
  await key('End'); expect(active()).toBe('Last');
  expect((await key('Enter', { repeat: true })).defaultPrevented).toBe(true); expect(select).not.toHaveBeenCalled();
  await key('Enter'); expect(select).toHaveBeenCalledExactlyOnceWith('last'); expect(setOpen).toHaveBeenCalledExactlyOnceWith(false);
});

it('leaves Home and End in a typed query until list navigation is active', async () => {
  expect((await key('Home')).defaultPrevented).toBe(false); expect((await key('End')).defaultPrevented).toBe(false);
  expect(input().hasAttribute('aria-activedescendant')).toBe(false);
  await key('ArrowDown'); await key('Escape'); expect(setOpen).toHaveBeenCalledExactlyOnceWith(false);
  expect(document.activeElement).toBe(container.querySelector('button'));
});

it('forwards supplementary query text intact and clears active selection through a rejected query and repair', async () => {
  const name = '\u{1F680}'.repeat(100); const seen = vi.fn();
  function Controlled({ failed = false }: { failed?: boolean }) {
    const [query, setQuery] = useState('');
    return <Picker label="Assignee" value="Unassigned" open setOpen={setOpen} query={query} setQuery={value => { seen(value); setQuery(value); }}
      choices={failed ? [] : [{ id: 'eligible', text: name }]} select={select} loading={false}
      error={failed ? new ApiError(422, 'validation_failed', 'Search must contain at most 100 characters') : null}
      refresh={() => {}} more={() => {}} hasMore={false}/>;
  }
  const change = async (value: string) => act(async () => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(input(), value);
    input().dispatchEvent(new Event('input', { bubbles: true }));
  });
  await act(async () => root.render(<Controlled/>));
  await key('ArrowDown'); expect(active()).toBe(name);
  await change(name); expect(seen).toHaveBeenLastCalledWith(name); expect(input().value).toBe(name);
  expect(input().hasAttribute('aria-activedescendant')).toBe(false); await key('Enter'); expect(select).not.toHaveBeenCalled();
  await change(name + '\u{1F680}'); await act(async () => root.render(<Controlled failed/>));
  expect(input().value).toBe(name + '\u{1F680}'); expect(seen).toHaveBeenLastCalledWith(name + '\u{1F680}');
  expect(container.textContent).toContain('Search must contain at most 100 characters');
  expect(container.querySelectorAll('[role=option]')).toHaveLength(0); await key('Enter'); expect(select).not.toHaveBeenCalled();
  await act(async () => root.render(<Controlled/>)); await change(name);
  await key('ArrowDown'); await key('Enter'); expect(select).toHaveBeenCalledExactlyOnceWith('eligible');
  expect(setOpen).toHaveBeenCalledExactlyOnceWith(false); expect(document.activeElement).toBe(container.querySelector('.reference-trigger'));
});
