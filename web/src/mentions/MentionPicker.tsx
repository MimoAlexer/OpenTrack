import { useLayoutEffect, useRef, useState, type CSSProperties } from 'react';
import type { Editor } from '@tiptap/core';
import type { MentionCandidate } from './types';
import type { MentionMatch } from './trigger';
import './mentions.css';
import { localPopupPosition, popupBounds, popupContentHeight, popupPosition } from './placement';
export function MentionPicker({ editor, match, id, choices, active, loading, error, hasMore, select, refresh }: { editor: Editor; match: MentionMatch; id: string; choices: MentionCandidate[]; active: number; loading: boolean; error: unknown; hasMore: boolean; select: (value: MentionCandidate) => void; refresh: () => void }) {
  const previousHeight = useRef(0);
  const ref = useRef<HTMLDivElement>(null); const [style, setStyle] = useState<CSSProperties>({ visibility: 'hidden' });
  useLayoutEffect(() => {
    const measure = () => {
      if (editor.isDestroyed || !ref.current) return;
      const caret = editor.view.coordsAtPos(Math.min(match.to, editor.state.doc.content.size));
      const height = Math.min(280, Math.max(popupContentHeight(ref.current), loading ? previousHeight.current : 0));
      const position = popupPosition(caret, popupBounds(ref.current), 300, height);
      if (!loading) previousHeight.current = position.maxHeight;
      setStyle({ position: 'absolute', minHeight: loading ? Math.min(previousHeight.current, position.maxHeight) : undefined, ...localPopupPosition(ref.current, position) });
    };
    measure(); window.addEventListener('resize', measure); window.addEventListener('scroll', measure, true);
    return () => { window.removeEventListener('resize', measure); window.removeEventListener('scroll', measure, true); };
  }, [editor, match.from, match.to, choices.length, loading, error]);
  useLayoutEffect(() => { ref.current?.querySelector('[aria-selected="true"]')?.scrollIntoView?.({ block: 'nearest' }); }, [active]);
  return <div ref={ref} style={style} className="mention-picker" onMouseDown={event => event.preventDefault()}><div className="mention-picker-heading">Mention someone</div><div id={id} role="listbox" aria-label="People to mention" aria-busy={loading}>{choices.map((value, index) => <div key={value.id} id={`${id}-${index}`} role="option" aria-selected={index === active} onClick={() => select(value)}>{value.display_name}</div>)}</div>{loading && <p role="status">Searching…</p>}{!loading && !error && !choices.length && <p role="status">No people found.</p>}{error ? <p role="alert">Could not load people. <button type="button" onClick={refresh}>Try again</button></p> : hasMore && <p>Keep typing to narrow the results.</p>}</div>;
}
