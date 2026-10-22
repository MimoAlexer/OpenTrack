import { useEffect, useRef } from 'react';

export interface Command {
  id: string;
  label: string;
  section: 'Actions' | 'Navigation' | 'Help' | 'Issues' | 'Recent issues';
  keywords?: string;
  shortcut?: { key: string; primary?: boolean; then?: string };
  available?: boolean;
  run: () => void;
}

export function availableCommands(commands: readonly Command[]) {
  return commands.filter(command => command.available !== false);
}

export function searchCommands(commands: readonly Command[], query: string) {
  const terms = query.trim().toLocaleLowerCase().split(/\s+/).filter(Boolean);
  return availableCommands(commands).filter(command => {
    const haystack = `${command.label} ${command.keywords ?? ''}`.toLocaleLowerCase();
    return terms.every(term => haystack.includes(term));
  });
}

export function shortcutLabel(shortcut: NonNullable<Command['shortcut']>) {
  const key = shortcut.key === 'Escape' ? 'Esc' : shortcut.key.toUpperCase();
  if (shortcut.then) return `${key} then ${shortcut.then.toUpperCase()}`;
  return shortcut.primary ? `Ctrl / ⌘ ${key}` : key;
}

export function characterShortcut(command: Command) {
  return !!command.shortcut && !command.shortcut.primary && command.shortcut.key.length === 1;
}

export function activeShortcut(command: Command, charactersEnabled: boolean) {
  return command.shortcut && (charactersEnabled || !characterShortcut(command));
}

function editable(target: EventTarget | null) {
  return target instanceof Element && (!!target.closest('input, textarea, select, [role="textbox"], [role="combobox"], [contenteditable]:not([contenteditable="false"])') || target instanceof HTMLElement && target.isContentEditable);
}

export function matchesShortcut(event: KeyboardEvent, command: Command, charactersEnabled = true) {
  const shortcut = command.shortcut;
  if (!shortcut || command.available === false || !activeShortcut(command, charactersEnabled) || event.altKey || event.getModifierState('AltGraph') || event.isComposing || event.keyCode === 229 || event.repeat || event.defaultPrevented) return false;
  if (event.key.toLowerCase() !== shortcut.key.toLowerCase()) return false;
  if (shortcut.primary) return event.ctrlKey !== event.metaKey && !event.shiftKey;
  return !event.ctrlKey && !event.metaKey && (!event.shiftKey || shortcut.key === '?') && !editable(event.target);
}

/** One registry drives the palette, shortcut help and workspace key bindings. */
export function useCommandShortcuts(commands: readonly Command[], blocked = false, charactersEnabled = true, scope = '') {
  const current = useRef({ commands, blocked, charactersEnabled, scope }); current.current = { commands, blocked, charactersEnabled, scope };
  useEffect(() => {
    let pending: { prefix: string; ids: string[]; deadline: number } | null = null;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let composing = false;
    const reset = () => { pending = null; clearTimeout(timer); };
    const startComposition = () => { composing = true; reset(); };
    const endComposition = () => { composing = false; reset(); };
    const handler = (event: KeyboardEvent) => {
      if (current.current.blocked || current.current.scope !== scope || current.current.charactersEnabled !== charactersEnabled || composing || document.querySelector('dialog[open]')) { reset(); return; }
      const previous = pending; reset();
      if (previous && Date.now() < previous.deadline) {
        const completed = current.current.commands.find(command => previous.ids.includes(command.id) && command.shortcut?.key === previous.prefix && command.shortcut.then && matchesShortcut(event, { ...command, shortcut: { key: command.shortcut.then } }, charactersEnabled));
        if (completed) { event.preventDefault(); completed.run(); return; }
      }
      const matching = current.current.commands.filter(command => matchesShortcut(event, command, charactersEnabled));
      const sequences = matching.filter(command => command.shortcut?.then);
      if (sequences.length) {
        pending = { prefix: sequences[0].shortcut!.key, ids: sequences.map(command => command.id), deadline: Date.now() + 800 };
        timer = setTimeout(reset, 800);
        return;
      }
      const command = matching.find(command => !command.shortcut?.then);
      if (!command) return;
      event.preventDefault(); command.run();
    };
    window.addEventListener('keydown', handler);
    window.addEventListener('compositionstart', startComposition);
    window.addEventListener('compositionend', endComposition);
    window.addEventListener('focusin', reset);
    window.addEventListener('pointerdown', reset);
    window.addEventListener('blur', reset);
    document.addEventListener('visibilitychange', reset);
    return () => {
      reset(); window.removeEventListener('keydown', handler);
      window.removeEventListener('compositionstart', startComposition);
      window.removeEventListener('compositionend', endComposition);
      window.removeEventListener('focusin', reset); window.removeEventListener('pointerdown', reset); window.removeEventListener('blur', reset);
      document.removeEventListener('visibilitychange', reset);
    };
  }, [blocked, charactersEnabled, scope]);
}
