import { useEffect, useId, useRef, useState } from 'react';
import type { Editor } from '@tiptap/core';
import { useIssueReferences } from './context';
import { useReferenceSuggestions } from './useReferenceSuggestions';
import { referenceMatch, type ReferenceMatch } from './trigger';
import { IssueReferencePicker } from './IssueReferencePicker';
import type { IssueReferenceCandidate } from './types';

export function useReferenceEditor(disabled: boolean) {
  const context = useIssueReferences(); const live = useRef({ context, disabled }); live.current = { context, disabled };
  const editor = useRef<Editor | null>(null); const composing = useRef(false); const dismissed = useRef('');
  const [state, setState] = useState<{ key: string; match: ReferenceMatch } | null>(null); const [active, setActive] = useState(0); const listId = useId();
  const match = !disabled && state?.key === context?.key ? state?.match ?? null : null;
  const suggestions = useReferenceSuggestions(match?.query ?? null);
  useEffect(() => setActive(0), [suggestions.key]);
  function observe(target: Editor) {
    editor.current = target;
    const next = !live.current.disabled && !composing.current && live.current.context && target.view.hasFocus() ? referenceMatch(target.state) : null;
    const token = next ? `${next.from}:${next.to}:${next.query}` : '';
    if (!next) dismissed.current = '';
    setState(old => {
      const key = live.current.context?.key;
      if (!next || !key || token === dismissed.current) return null;
      return old?.key === key && old.match.from === next.from && old.match.to === next.to && old.match.query === next.query ? old : { key, match: next };
    });
  }
  function select(issue: IssueReferenceCandidate) {
    const target = editor.current;
    if (!target || !match || !context || target.isDestroyed || !target.isEditable || disabled || suggestions.loading || suggestions.error || !suggestions.choices.some(item => item.id === issue.id)) return;
    const current = referenceMatch(target.state);
    if (!current || current.from !== match.from || current.to !== match.to || current.query !== match.query) return;
    const before = target.state.doc;
    target.chain().focus().insertContentAt({ from: match.from, to: match.to }, [{ type: 'issueReference', attrs: { id: issue.id } }, { type: 'text', text: ' ' }]).run();
    if (target.state.doc !== before) context.remember(issue);
    setState(null);
  }
  function keyDown(event: KeyboardEvent) {
    if (!match || composing.current) return false;
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault(); event.stopPropagation();
      if (suggestions.choices.length) setActive(value => (value + (event.key === 'ArrowDown' ? 1 : suggestions.choices.length - 1)) % suggestions.choices.length);
      return true;
    }
    if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); dismissed.current = `${match.from}:${match.to}:${match.query}`; setState(null); return true; }
    if (event.key === 'Enter' || event.key === 'Tab') {
      const issue = suggestions.choices[active];
      if (event.key === 'Tab' && (!issue || suggestions.loading || suggestions.error)) { setState(null); return false; }
      event.preventDefault(); event.stopPropagation(); if (issue) select(issue); return true;
    }
    return false;
  }
  return {
    observe, keyDown, clear: () => setState(null),
    composition(value: boolean) { composing.current = value; if (value) setState(null); },
    accessibility: match ? { listId, activeId: suggestions.choices[active] ? `${listId}-${active}` : null } : null,
    popup: editor.current && match ? <IssueReferencePicker key={context?.key} editor={editor.current} match={match} id={listId} choices={suggestions.choices} active={active} loading={suggestions.loading} error={suggestions.error} hasMore={suggestions.hasMore} select={select} refresh={suggestions.refresh} /> : null,
  };
}
