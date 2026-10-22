import { Node } from '@tiptap/core';
import { NodeViewWrapper, ReactNodeViewRenderer, type NodeViewProps } from '@tiptap/react';
import { IssueReferenceText } from './IssueReferenceText';
function ReferenceNodeView({ node }: NodeViewProps) { return <NodeViewWrapper as="span" contentEditable={false}><IssueReferenceText id={String(node.attrs.id)} /></NodeViewWrapper>; }
export const IssueReferenceNode = Node.create({
  name: 'issueReference', group: 'inline', inline: true, atom: true, selectable: true, marks: '',
  addAttributes: () => ({ id: { default: null } }),
  parseHTML: () => [],
  renderHTML: () => ['span', { 'data-type': 'issueReference' }, '#issue'],
  renderText: () => '#issue',
  addNodeView: () => ReactNodeViewRenderer(ReferenceNodeView),
});
