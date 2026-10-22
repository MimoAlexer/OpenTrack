import type { ComponentProps } from 'react';
import { MentionProvider } from '../mentions/context';
import { IssueReferenceProvider } from '../references/context';
import type { IssueReference } from '../references/types';

export function ReferenceProvider({ issueReferences, children, ...props }: ComponentProps<typeof MentionProvider> & { issueReferences?: IssueReference[] }) {
  return <MentionProvider {...props}><IssueReferenceProvider workspaceId={props.workspaceId} session={props.session} target={props.target} references={issueReferences} projectionScope={props.projectionScope}>{children}</IssueReferenceProvider></MentionProvider>;
}
