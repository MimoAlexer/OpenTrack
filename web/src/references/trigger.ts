import type { EditorState } from '@tiptap/pm/state';
export interface ReferenceMatch { from: number; to: number; query: string }
export function referenceMatch(state: EditorState): ReferenceMatch | null {
  const { selection } = state;
  if (!selection.empty || !selection.$from.parent.isTextblock || selection.$from.parent.type.name === 'codeBlock' || selection.$from.marks().some(mark => mark.type.name === 'code')) return null;
  const before = selection.$from.parent.textBetween(0, selection.$from.parentOffset, '\ufffc', '\ufffc');
  const match = /(?:^|[\s(\[{])#([\p{L}\p{N}\p{M}_-]{0,100})$/u.exec(before);
  return match ? { from: selection.from - match[1].length - 1, to: selection.from, query: match[1] } : null;
}
