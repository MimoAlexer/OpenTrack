import { useEffect, useId, useRef, useState } from 'react';
import { ArrowUpRight, Command as CommandIcon, Search } from 'lucide-react';
import { ErrorBanner, Modal } from '../components';
import { activeShortcut, availableCommands, searchCommands, shortcutLabel, type Command } from './registry';
import type { ShortcutPreference } from './useShortcutPreference';
import './commands.css';

export function CommandPalette({ commands, close, charactersEnabled = true }: { commands: readonly Command[]; close: () => void; charactersEnabled?: boolean }) {
  const [query, setQuery] = useState('');
  const [selected, setSelected] = useState<string | null>(null);
  const dispatched = useRef(false);
  const id = useId();
  const matches = searchCommands(commands, query);
  const active = matches.find(command => command.id === selected) ?? matches[0];
  const activeIndex = matches.findIndex(command => command.id === active?.id);
  useEffect(() => {
    if (active) document.getElementById(`${id}-${active.id}`)?.scrollIntoView?.({ block: 'nearest' });
  }, [active?.id, id]);
  function choose(command: Command) {
    if (dispatched.current || command.available === false) return;
    dispatched.current = true;
    close();
    // Let the modal return focus before opening another dialog or focusing a field.
    requestAnimationFrame(command.run);
  }
  return <Modal title="Commands" close={close}>
    <div className="command-palette">
      <label className="command-search"><Search aria-hidden="true" /><span className="sr-only">Search commands</span><input data-autofocus role="combobox" aria-label="Search commands" aria-autocomplete="list" aria-expanded="true" aria-controls={`${id}-results`} aria-activedescendant={active ? `${id}-${active.id}` : undefined} value={query} placeholder="Search actions or go to…" autoComplete="off" maxLength={120} onChange={event => { setQuery(event.target.value); setSelected(null); }} onKeyDown={event => {
        if (event.nativeEvent.isComposing || event.repeat) return;
        if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
          event.preventDefault();
          if (matches.length) setSelected(matches[(activeIndex + (event.key === 'ArrowDown' ? 1 : -1) + matches.length) % matches.length].id);
        }
        if (event.key === 'Enter' && active) { event.preventDefault(); choose(active); }
      }} /><kbd>Esc</kbd></label>
      <div className="command-results" id={`${id}-results`} role="listbox" aria-label="Workspace commands">
        {matches.map(command => <div key={command.id} id={`${id}-${command.id}`} role="option" aria-selected={active?.id === command.id} className="command-option" onPointerMove={() => setSelected(command.id)} onMouseDown={event => event.preventDefault()} onClick={() => choose(command)}>
          {command.section === 'Navigation' ? <ArrowUpRight aria-hidden="true" /> : <CommandIcon aria-hidden="true" />}<span><strong>{command.label}</strong><small>{command.section}</small></span>{activeShortcut(command, charactersEnabled) && <kbd>{shortcutLabel(command.shortcut!)}</kbd>}
        </div>)}
      </div>
      {!matches.length && <p className="command-empty" role="status">No commands match “{query}”.</p>}
      <footer className="command-footer"><span><kbd>↑</kbd><kbd>↓</kbd> to navigate</span><span><kbd>Enter</kbd> to select</span></footer>
    </div>
  </Modal>;
}

export function CommandHelp({ commands, close, preference }: { commands: readonly Command[]; close: () => void; preference?: ShortcutPreference }) {
  return <Modal title="Keyboard shortcuts" close={close}><div className="command-help">
    {preference && <div className="shortcut-preference"><label><input type="checkbox" checked={preference.desired === 'enabled'} disabled={preference.status !== 'ready'} onChange={event => void preference.change(event.target.checked ? 'enabled' : 'disabled')} />Enable character shortcuts</label><p>For your account in this workspace. Includes single keys and G sequences. Commands and visible controls remain available.</p>
      {preference.status === 'loading' && <p role="status">Loading saved preference… Character shortcuts are paused.</p>}
      {preference.status === 'saving' && <p role="status">Saving preference…</p>}
      {preference.status === 'ready' && <p role="status">Character shortcuts are {preference.charactersEnabled ? 'enabled' : 'disabled'}.</p>}
      {preference.status === 'conflict' && <div role="alert"><p>This preference changed in another session. The saved setting is {preference.saved}; your choice is {preference.desired}. Review before applying.</p><div className="button-row"><button type="button" className="button" onClick={preference.useSaved}>Use saved setting</button><button type="button" className="button" onClick={() => void preference.change(preference.desired)}>Apply my choice</button></div></div>}
      {preference.status === 'error' && <><ErrorBanner error={preference.error} /><p>Character shortcuts are paused. Reload the saved setting before trying again.</p><button type="button" className="button" onClick={preference.reload}>Reload saved setting</button></>}
    </div>}
    <dl>
    {availableCommands(commands).filter(command => activeShortcut(command, preference?.charactersEnabled ?? true)).map(command => <div key={command.id}><dt>{command.label}</dt><dd><kbd>{shortcutLabel(command.shortcut!)}</kbd></dd></div>)}
    <div><dt>Save or create in a form</dt><dd><kbd>Ctrl / ⌘ Enter</kbd></dd></div>
  </dl><p>Character shortcuts are paused while you type. Navigation sequences expire after 800 ms.</p><div className="modal-footer"><button type="button" className="button" data-autofocus onClick={close}>Done</button></div></div></Modal>;
}
