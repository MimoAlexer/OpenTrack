import { createContext, useContext, useMemo, useState, type ReactNode } from 'react';
import { useSessionScope } from '../desktop/context';
import type { Session } from '../types';
import type { IssueReference, IssueReferenceCandidate, ReferenceTarget } from './types';

interface ReferenceContextValue {
  workspaceId: string; scope: string; key: string; target: ReferenceTarget;
  get: (id: string) => IssueReference | undefined;
  remember: (issue: IssueReferenceCandidate) => void;
}
const Context = createContext<ReferenceContextValue | null>(null);
const EMPTY_REFERENCES: IssueReference[] = [];
export function IssueReferenceProvider({ workspaceId, session, target, references = EMPTY_REFERENCES, projectionScope, children }: {
  workspaceId: string; session: Session; target: ReferenceTarget; references?: IssueReference[]; projectionScope?: string; children: ReactNode;
}) {
  const scope = useSessionScope(session); const key = `${workspaceId}:${scope}:${JSON.stringify(target)}`;
  const seeds = useMemo(() => new Map<string, IssueReference>(), [key]); const [revision, setRevision] = useState(0);
  const receipt = useMemo(() => ({ key: projectionScope === scope ? key : '', references }), [references, projectionScope]);
  const projected = useMemo(() => new Map((receipt.key === key ? receipt.references : []).map(item => [item.id.toLowerCase(), item])), [receipt, key]);
  const value = useMemo(() => ({ workspaceId, scope, key, target,
    get: (id: string) => projected.get(id.toLowerCase()) ?? seeds.get(id.toLowerCase()),
    remember(issue: IssueReferenceCandidate) {
      const id = issue.id.toLowerCase();
      if (seeds.size >= 50 && !seeds.has(id)) seeds.delete(seeds.keys().next().value!);
      seeds.set(id, { id: issue.id, key: issue.key, title: issue.title, available: true }); setRevision(value => value + 1);
    },
  }), [key, projected, seeds, revision]);
  return <Context.Provider value={value}>{children}</Context.Provider>;
}
export function useIssueReferences() { return useContext(Context); }
