import { Node } from '@tiptap/core';
import { NodeViewWrapper, ReactNodeViewRenderer, type NodeViewProps } from '@tiptap/react';
import { AttachmentCard } from './AttachmentCard';
function AttachmentNodeView({ node, editor, deleteNode }: NodeViewProps) {
  return <NodeViewWrapper className="attachment-node"><AttachmentCard id={String(node.attrs.id)} remove={editor.isEditable ? () => { if (editor.isEditable) deleteNode(); } : undefined} /></NodeViewWrapper>;
}
export const AttachmentNode = Node.create({
  name: 'attachment', group: 'block', atom: true, selectable: true,
  addAttributes: () => ({ id: { default: null } }),
  // Browser HTML paste cannot manufacture authorized attachment references.
  parseHTML: () => [],
  renderHTML: ({ node }) => ['div', { 'data-attachment-id': node.attrs.id, 'data-type': 'attachment' }, 'Attachment'],
  addNodeView: () => ReactNodeViewRenderer(AttachmentNodeView),
});
