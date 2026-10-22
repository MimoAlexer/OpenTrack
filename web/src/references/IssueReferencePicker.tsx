import { useLayoutEffect, useRef, useState, type CSSProperties } from 'react';
import type { Editor } from '@tiptap/core';
import type { IssueReferenceCandidate } from './types';
import type { ReferenceMatch } from './trigger';
import '../mentions/mentions.css';
import './references.css';
import { localPopupPosition, popupBounds, popupContentHeight, popupPosition } from '../mentions/placement';
export function IssueReferencePicker({ editor, match, id, choices, active, loading, error, hasMore, select, refresh }: { editor: Editor; match: ReferenceMatch; id: string; choices: IssueReferenceCandidate[]; active: number; loading: boolean; error: unknown; hasMore: boolean; select: (value: IssueReferenceCandidate) => void; refresh: () => void }) {
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
  return <div ref={ref} style={style} className="mention-picker" onMouseDown={event => event.preventDefault()}><div className="mention-picker-heading">Reference an issue</div><div id={id} role="listbox" aria-label="Issues to reference" aria-busy={loading}>{choices.map((value, index) => <div key={value.id} id={`${id}-${index}`} role="option" aria-selected={index === active} onClick={() => select(value)}><span className="reference-choice-key">{value.key}</span><span className="reference-choice-title">{value.title}</span></div>)}</div>{loading && <p role="status">Searching…</p>}{!loading && !error && !choices.length && <p role="status">No issues found.</p>}{error ? <p role="alert">Could not load issues. <button type="button" onClick={refresh}>Try again</button></p> : hasMore && <p>Keep typing to narrow the results.</p>}</div>;
}
