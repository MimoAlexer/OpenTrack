// @vitest-environment jsdom
import { act, useState } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { CommandHelp, CommandPalette } from './CommandPalette';
import { searchCommands, shortcutLabel, useCommandShortcuts, type Command } from './registry';

let container: HTMLDivElement; let root: Root;
beforeEach(() => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true);
  HTMLDialogElement.prototype.showModal = function () { this.setAttribute('open', ''); };
  HTMLDialogElement.prototype.close = function () { this.removeAttribute('open'); };
  container = document.createElement('div'); document.body.append(container); root = createRoot(container);
});
afterEach(async () => { await act(async () => root.unmount()); container.remove(); vi.useRealTimers(); vi.unstubAllGlobals(); });
const command = (overrides: Partial<Command> = {}): Command => ({ id: 'new', label: 'New issue', section: 'Actions', shortcut: { key: 'c' }, run: vi.fn(), ...overrides });
function keyboard(target: EventTarget, key: string, options: KeyboardEventInit = {}) {
  const event = new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true, ...options }); target.dispatchEvent(event); return event;
}
function Bindings({ commands, blocked = false, enabled = true, scope = '' }: { commands: Command[]; blocked?: boolean; enabled?: boolean; scope?: string }) {
  useCommandShortcuts(commands, blocked, enabled, scope);
  return <><input aria-label="Issue title" /><textarea /><select /><div contentEditable suppressContentEditableWarning><span>Editable text</span></div><div role="combobox" tabIndex={0} /><button>Trigger</button></>;
}

it('searches every term across labels and keywords and omits unavailable commands', () => {
  const items = [command({ keywords: 'create ticket' }), command({ id: 'trash', label: 'Trash', keywords: 'deleted restore', section: 'Navigation' }), command({ id: 'hidden', label: 'New private issue', available: false })];
  expect(searchCommands(items, '  CREATE issue ')).toEqual([items[0]]);
  expect(searchCommands(items, 'restore')).toEqual([items[1]]);
  expect(searchCommands(items, '')).toEqual(items.slice(0, 2));
  expect(searchCommands(items, 'missing')).toEqual([]);
});

it('disables printable shortcuts and sequences while retaining Mod+K, Escape and palette actions', async () => {
  const create = command(); const mine = command({ id: 'mine', label: 'My work', shortcut: { key: 'g', then: 'm' } });
  const palette = command({ id: 'palette', shortcut: { key: 'k', primary: true } }); const escape = command({ id: 'escape', shortcut: { key: 'Escape' } });
  await act(async () => root.render(<Bindings commands={[create, mine, palette, escape]} enabled={false} />));
  for (const key of ['c', 'g', 'm']) expect(keyboard(window, key).defaultPrevented).toBe(false);
  expect(create.run).not.toHaveBeenCalled(); expect(mine.run).not.toHaveBeenCalled();
  keyboard(window, 'k', { ctrlKey: true }); keyboard(window, 'Escape');
  expect(palette.run).toHaveBeenCalledOnce(); expect(escape.run).toHaveBeenCalledOnce();
  await act(async () => root.render(<CommandPalette commands={[create, mine, palette]} charactersEnabled={false} close={vi.fn()} />));
  expect(container.querySelectorAll('[role="option"]')).toHaveLength(3);
  expect(container.querySelectorAll('[role="option"] kbd')).toHaveLength(1);
  expect(container.textContent).toContain('My work');
});

it('completes available navigation sequences before 800 ms and never captures an unknown continuation', async () => {
  vi.useFakeTimers();
  const mine = command({ id: 'mine', shortcut: { key: 'g', then: 'm' } });
  const inbox = command({ id: 'inbox', label: 'Inbox', available: false, shortcut: { key: 'g', then: 'i' } });
  const create = command();
  await act(async () => root.render(<Bindings commands={[mine, inbox, create]} />));
  expect(shortcutLabel(mine.shortcut!)).toBe('G then M');
  expect(keyboard(window, 'g').defaultPrevented).toBe(false);
  vi.advanceTimersByTime(799);
  expect(keyboard(window, 'm').defaultPrevented).toBe(true); expect(mine.run).toHaveBeenCalledOnce();
  keyboard(window, 'g'); vi.advanceTimersByTime(800);
  expect(keyboard(window, 'm').defaultPrevented).toBe(false); expect(mine.run).toHaveBeenCalledOnce();
  keyboard(window, 'g'); expect(keyboard(window, 'x').defaultPrevented).toBe(false);
  expect(keyboard(window, 'm').defaultPrevented).toBe(false);
  keyboard(window, 'g'); expect(keyboard(window, 'i').defaultPrevented).toBe(false); expect(inbox.run).not.toHaveBeenCalled();
  keyboard(window, 'g'); keyboard(window, 'c'); expect(create.run).toHaveBeenCalledOnce();
});

it('rejects modifiers, AltGr, repeated keys and composition at both sequence steps', async () => {
  const mine = command({ id: 'mine', shortcut: { key: 'g', then: 'm' } });
  const create = command(); const palette = command({ id: 'palette', shortcut: { key: 'k', primary: true } });
  await act(async () => root.render(<Bindings commands={[mine, create, palette]} />));
  for (const options of [{ shiftKey: true }, { ctrlKey: true }, { metaKey: true }, { altKey: true }, { ctrlKey: true, altKey: true }, { repeat: true }, { isComposing: true }, { keyCode: 229 }]) {
    keyboard(window, 'g', options); expect(keyboard(window, 'm').defaultPrevented).toBe(false);
    keyboard(window, 'g'); expect(keyboard(window, 'm', options).defaultPrevented).toBe(false);
    keyboard(window, 'c', options);
  }
  keyboard(window, 'k', { ctrlKey: true, metaKey: true });
  const altgr = new KeyboardEvent('keydown', { key: 'c', bubbles: true, cancelable: true });
  Object.defineProperty(altgr, 'getModifierState', { value: (key: string) => key === 'AltGraph' }); window.dispatchEvent(altgr);
  window.dispatchEvent(new CompositionEvent('compositionstart')); keyboard(window, 'g'); keyboard(window, 'm'); keyboard(window, 'c');
  window.dispatchEvent(new CompositionEvent('compositionend'));
  expect(mine.run).not.toHaveBeenCalled(); expect(create.run).not.toHaveBeenCalled(); expect(palette.run).not.toHaveBeenCalled();
  keyboard(window, 'g'); keyboard(window, 'm'); expect(mine.run).toHaveBeenCalledOnce();
});

it('clears pending sequences on editing, handled events, pointer/focus changes, dialogs and scope changes', async () => {
  const mine = command({ id: 'mine', shortcut: { key: 'g', then: 'm' } });
  await act(async () => root.render(<Bindings commands={[mine]} />));
  for (const field of container.querySelectorAll('input,textarea,select,[contenteditable] span,[role="combobox"]')) {
    keyboard(field, 'g'); keyboard(window, 'm'); keyboard(window, 'g'); keyboard(field, 'm'); keyboard(window, 'm');
  }
  for (const event of ['pointerdown', 'focusin', 'blur', 'compositionstart', 'compositionend']) {
    keyboard(window, 'g'); window.dispatchEvent(new Event(event)); keyboard(window, 'm');
  }
  keyboard(window, 'g'); document.dispatchEvent(new Event('visibilitychange')); keyboard(window, 'm');
  keyboard(window, 'g'); const canceled = new KeyboardEvent('keydown', { key: 'x', cancelable: true }); canceled.preventDefault(); window.dispatchEvent(canceled); keyboard(window, 'm');
  keyboard(window, 'g'); const dialog = document.createElement('dialog'); dialog.open = true; container.append(dialog); keyboard(window, 'm'); dialog.remove(); keyboard(window, 'm');
  keyboard(window, 'g'); await act(async () => root.render(<Bindings commands={[mine]} scope="other-workspace" />)); keyboard(window, 'm');
  keyboard(window, 'g'); await act(async () => root.render(<Bindings commands={[mine]} scope="other-account" />)); keyboard(window, 'm');
  keyboard(window, 'g'); await act(async () => root.render(<Bindings commands={[mine]} blocked />)); keyboard(window, 'm');
  await act(async () => root.render(<Bindings commands={[mine]} />));
  keyboard(window, 'g'); await act(async () => root.render(<Bindings commands={[{ ...mine, available: false }]} />)); keyboard(window, 'm');
  expect(mine.run).not.toHaveBeenCalled();
});

it('keeps single-key shortcuts out of editable fields and composition or repeated input', async () => {
  const create = command(); await act(async () => root.render(<Bindings commands={[create]} />));
  for (const field of container.querySelectorAll('input,textarea,select,[contenteditable] span,[role="combobox"]')) {
    expect(keyboard(field, 'c').defaultPrevented).toBe(false);
  }
  for (const options of [{ isComposing: true }, { repeat: true }, { altKey: true }, { ctrlKey: true }, { metaKey: true }]) keyboard(window, 'c', options);
  const canceled = new KeyboardEvent('keydown', { key: 'c', cancelable: true }); canceled.preventDefault(); window.dispatchEvent(canceled);
  expect(create.run).not.toHaveBeenCalled();
  expect(keyboard(container.querySelector('button')!, 'c').defaultPrevented).toBe(true);
  expect(create.run).toHaveBeenCalledTimes(1);
});

it('uses current command availability and handlers without accumulating listeners', async () => {
  const original = command(); await act(async () => root.render(<Bindings commands={[original]} />));
  const latest = command(); await act(async () => root.render(<Bindings commands={[latest]} />));
  keyboard(window, 'c'); expect(original.run).not.toHaveBeenCalled(); expect(latest.run).toHaveBeenCalledTimes(1);
  await act(async () => root.render(<Bindings commands={[{ ...latest, available: false }]} />));
  expect(keyboard(window, 'c').defaultPrevented).toBe(false); expect(latest.run).toHaveBeenCalledTimes(1);
  await act(async () => root.render(<Bindings commands={[latest]} blocked />));
  expect(keyboard(window, 'c').defaultPrevented).toBe(false); expect(latest.run).toHaveBeenCalledTimes(1);
});

it('allows the primary palette shortcut from an input but never underneath an open dialog', async () => {
  const open = command({ id: 'palette', shortcut: { key: 'k', primary: true } });
  await act(async () => root.render(<Bindings commands={[open]} />));
  keyboard(container.querySelector('input')!, 'k', { ctrlKey: true }); keyboard(window, 'k', { metaKey: true });
  expect(open.run).toHaveBeenCalledTimes(2);
  const dialog = document.createElement('dialog'); dialog.setAttribute('open', ''); container.append(dialog);
  expect(keyboard(window, 'k', { ctrlKey: true }).defaultPrevented).toBe(false);
  expect(open.run).toHaveBeenCalledTimes(2); dialog.remove();
});

it('dispatches the selected palette command once, after closing and returning focus', async () => {
  const frames: FrameRequestCallback[] = []; vi.stubGlobal('requestAnimationFrame', (callback: FrameRequestCallback) => { frames.push(callback); return frames.length; });
  let trigger: HTMLButtonElement;
  const selected = command({ id: 'trash', label: 'Trash', section: 'Navigation', run: vi.fn(() => expect(document.activeElement).toBe(trigger)) });
  function PaletteHarness() {
    const [open, setOpen] = useState(false);
    return <><button onClick={() => setOpen(true)}>Commands</button>{open && <CommandPalette commands={[command(), selected]} close={() => setOpen(false)} />}</>;
  }
  await act(async () => root.render(<PaletteHarness />)); trigger = container.querySelector('button')!; trigger.focus();
  await act(async () => trigger.click()); const input = container.querySelector<HTMLInputElement>('[role="combobox"]')!;
  expect(document.activeElement).toBe(input);
  await act(async () => keyboard(input, 'ArrowDown'));
  expect(container.querySelector('[aria-selected="true"]')?.textContent).toContain('Trash');
  expect(input.getAttribute('aria-activedescendant')).toBe(container.querySelector('[aria-selected="true"]')?.id);
  await act(async () => { keyboard(input, 'Enter'); keyboard(input, 'Enter'); });
  expect(container.querySelector('dialog')).toBeNull(); expect(selected.run).not.toHaveBeenCalled(); expect(frames).toHaveLength(1);
  await act(async () => frames[0](0)); expect(selected.run).toHaveBeenCalledTimes(1);
});

it('does not dispatch a composing Enter, and exposes only available shortcuts in help', async () => {
  const create = command();
  await act(async () => root.render(<CommandPalette commands={[create]} close={vi.fn()} />));
  keyboard(container.querySelector('input')!, 'Enter', { isComposing: true }); expect(create.run).not.toHaveBeenCalled();
  for (const options of [{ keyCode: 229 }, { ctrlKey: true }, { metaKey: true }, { altKey: true }, { repeat: true }]) keyboard(container.querySelector('input')!, 'Enter', options);
  expect(create.run).not.toHaveBeenCalled();
  await act(async () => root.render(<CommandHelp commands={[create, command({ id: 'hidden', label: 'Hidden action', available: false })]} close={vi.fn()} />));
  expect(container.textContent).toContain('New issue'); expect(container.textContent).not.toContain('Hidden action'); expect(container.textContent).toContain('Save or create in a form');
});

it('drops an unavailable selection without losing search focus', async () => {
  const items = [command(), command({ id: 'trash', label: 'Trash', section: 'Navigation' })];
  const close = vi.fn();
  await act(async () => root.render(<CommandPalette commands={items} close={close} />));
  const input = container.querySelector('input')!;
  await act(async () => keyboard(input, 'ArrowDown'));
  expect(container.querySelector('[aria-selected="true"]')?.textContent).toContain('Trash');
  await act(async () => root.render(<CommandPalette commands={[items[0], { ...items[1], available: false }]} close={close} />));
  expect(container.querySelector('[aria-selected="true"]')?.textContent).toContain('New issue');
  expect(container.querySelectorAll('[role="option"]')).toHaveLength(1);
  expect(document.activeElement).toBe(input);
  await act(async () => root.render(<CommandPalette commands={[]} close={close} />));
  expect(input.hasAttribute('aria-activedescendant')).toBe(false);
  expect(container.querySelector('[role="status"]')?.textContent).toContain('No commands match');
  keyboard(input, 'Enter'); expect(close).not.toHaveBeenCalled();
});

it('cancels the palette and returns focus without dispatching an action', async () => {
  const create = command();
  function CancelHarness() {
    const [open, setOpen] = useState(false);
    return <><button onClick={() => setOpen(true)}>Commands</button>{open && <CommandPalette commands={[create]} close={() => setOpen(false)} />}</>;
  }
  await act(async () => root.render(<CancelHarness />));
  const trigger = container.querySelector('button')!; trigger.focus(); await act(async () => trigger.click());
  const cancel = new Event('cancel', { cancelable: true });
  await act(async () => container.querySelector('dialog')!.dispatchEvent(cancel));
  expect(cancel.defaultPrevented).toBe(true); expect(container.querySelector('dialog')).toBeNull();
  expect(document.activeElement).toBe(trigger); expect(create.run).not.toHaveBeenCalled();
});
