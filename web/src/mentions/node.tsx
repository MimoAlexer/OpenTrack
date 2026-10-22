import { Node } from '@tiptap/core';
import { NodeViewWrapper, ReactNodeViewRenderer, type NodeViewProps } from '@tiptap/react';
import { MentionText } from './MentionText';
function MentionNodeView({ node }: NodeViewProps) { return <NodeViewWrapper as="span" contentEditable={false}><MentionText id={String(node.attrs.id)} /></NodeViewWrapper>; }
export const UserMentionNode = Node.create({
  name: 'userMention', group: 'inline', inline: true, atom: true, selectable: true, marks: '',
  addAttributes: () => ({ id: { default: null } }),
  parseHTML: () => [],
  renderHTML: () => ['span', { 'data-type': 'userMention' }, '@someone'],
  renderText: () => '@someone',
  addNodeView: () => ReactNodeViewRenderer(MentionNodeView),
});
