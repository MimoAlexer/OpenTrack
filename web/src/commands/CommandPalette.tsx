import { useEffect, useId, useRef, useState } from 'react';
import { ArrowUpRight, Command as CommandIcon, Search } from 'lucide-react';
import { ErrorBanner, Modal } from '../components';
import { activeShortcut, availableCommands, searchCommands, shortcutLabel, type Command } from './registry';
import type { ShortcutPreference } from './useShortcutPreference';
import { usePaletteIssues, type PaletteIssueSource } from './usePaletteIssues';
import { usePaletteEntities, type PaletteEntity } from './usePaletteEntities';
import { issuesUrl, navigate, parseRoute, useLocation } from '../router';
import { projectUrl } from '../projects/types';
import './commands.css';

type PaletteCommand = Command & { issueId?: string; entity?: PaletteEntity; group?: string };

export function CommandPalette({ commands, close, charactersEnabled = true, issueSource }: { commands: readonly Command[]; close: () => void; charactersEnabled?: boolean; issueSource?: PaletteIssueSource }) {
  const [query, setQuery] = useState('');
  const [selected, setSelected] = useState<string | null>(null);
  const dispatched = useRef(false);
  const resolving = useRef<{ scope: string } | null>(null);
  const route = parseRoute(useLocation());
  const teamId = route.workspaceId === issueSource?.workspaceId ? route.search.get('team_id') || undefined : undefined;
  const source = issueSource && { ...issueSource, scope: JSON.stringify([issueSource.scope, route.workspaceId, teamId]) };
  const selectionScope = JSON.stringify([source?.scope, query]);
  const id = useId();
  const issues = usePaletteIssues(source, query);
  const entities = usePaletteEntities(route.valid && route.workspaceId === source?.workspaceId ? source : undefined, query, teamId);
  const groups: PaletteCommand[][] = [
    issues.items.map(issue => ({ id: `issue-${issue.id}`, issueId: issue.id, label: `${issue.key} · ${issue.title}`, section: query.trim() ? 'Issues' : 'Recent issues', run: () => issueSource!.open(issue.id) })),
    entities.teams.items.map(entity => ({ id: `team-${entity.id}`, entity, label: `${entity.key} · ${entity.name}`, section: 'Navigation', group: 'Teams', run: () => { navigate(issuesUrl(entity.workspaceId, new URLSearchParams({ team_id: entity.id }))); } })),
    entities.projects.items.map(entity => ({ id: `project-${entity.id}`, entity, label: entity.name, section: 'Navigation', group: 'Projects', run: () => { navigate(projectUrl(entity.workspaceId, entity.teamId!, entity.id)); } })),
  ];
  const remote: PaletteCommand[] = [];
  for (let index = 0; remote.length < 20 && groups.some(group => index < group.length); index += 1) {
    for (const group of groups) if (group[index] && remote.length < 20) remote.push(group[index]);
  }
  const matches: PaletteCommand[] = [...searchCommands(commands, query).filter(command => !source || !command.id.startsWith('team-')), ...remote];
  const busy = issues.busy || entities.teams.busy || entities.projects.busy;
  const errors = [{ label: 'Issues', ...issues }, { label: 'Teams', ...entities.teams }, { label: 'Projects', ...entities.projects }];
  const active = matches.find(command => command.id === selected) ?? matches[0];
  const activeIndex = matches.findIndex(command => command.id === active?.id);
  useEffect(() => {
    if (active) document.getElementById(`${id}-${active.id}`)?.scrollIntoView?.({ block: 'nearest' });
  }, [active?.id, id]);
  async function choose(command: PaletteCommand) {
    if (dispatched.current || resolving.current?.scope === selectionScope || command.available === false) return;
    if (command.issueId || command.entity) {
      const pending = { scope: selectionScope }; resolving.current = pending;
      const accessible = command.issueId ? await issues.resolve(command.issueId) : await entities[command.entity!.kind === 'team' ? 'teams' : 'projects'].resolve(command.entity!);
      if (resolving.current === pending) resolving.current = null;
      if (!accessible) return;
    }
    dispatched.current = true;
    close();
    // Let the modal return focus before opening another dialog or focusing a field.
    requestAnimationFrame(command.run);
  }
  return <Modal title="Commands" close={close}>
    <div className="command-palette">
      <label className="command-search"><Search aria-hidden="true" /><span className="sr-only">Search commands</span><input data-autofocus role="combobox" aria-label="Search commands" aria-autocomplete="list" aria-expanded="true" aria-controls={`${id}-results`} aria-activedescendant={active ? `${id}-${active.id}` : undefined} value={query} placeholder="Search actions or go to…" autoComplete="off" onChange={event => { setQuery(event.target.value); setSelected(null); }} onKeyDown={event => {
        if (event.nativeEvent.isComposing || event.keyCode === 229 || event.repeat || event.altKey || event.ctrlKey || event.metaKey || event.getModifierState('AltGraph')) return;
        if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
          event.preventDefault();
          if (matches.length) setSelected(matches[(activeIndex + (event.key === 'ArrowDown' ? 1 : -1) + matches.length) % matches.length].id);
        }
        if (event.key === 'Enter' && active) { event.preventDefault(); void choose(active); }
      }} /><kbd>Esc</kbd></label>
      <div className="command-results" id={`${id}-results`} role="listbox" aria-label="Workspace commands" aria-busy={busy}>
        {matches.map(command => <div key={command.id} id={`${id}-${command.id}`} role="option" aria-selected={active?.id === command.id} className="command-option" onPointerMove={() => setSelected(command.id)} onMouseDown={event => event.preventDefault()} onClick={() => { void choose(command); }}>
          {command.section === 'Navigation' || command.issueId ? <ArrowUpRight aria-hidden="true" /> : <CommandIcon aria-hidden="true" />}<span><strong>{command.label}</strong><small>{command.group ?? command.section}</small></span>{activeShortcut(command, charactersEnabled) && <kbd>{shortcutLabel(command.shortcut!)}</kbd>}
        </div>)}
      </div>
      {issueSource && <p className="sr-only" role="status" aria-live="polite">{busy ? 'Searching accessible results…' : `${remote.filter(item => item.issueId).length} ${query.trim() ? 'issue results' : 'recent issues'}, ${remote.filter(item => item.entity?.kind === 'team').length} teams and ${remote.filter(item => item.entity?.kind === 'project').length} projects in this workspace.`}</p>}
      {source && query.trim() && !teamId && <p className="command-scope">Choose a team to search its projects.</p>}
      {errors.filter(group => group.error).map(group => <div className="command-error" key={group.label}><strong>{group.label}</strong><ErrorBanner error={group.error} retry={group.retry} /></div>)}
      {!matches.length && !busy && !errors.some(group => group.error) && <p className="command-empty" role="status">No commands match “{query}”.</p>}
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
