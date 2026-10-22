import { createContext, useContext, useMemo, useState, type ReactNode } from 'react';
import { useSessionScope } from '../desktop/context';
import type { Session } from '../types';
import type { MentionCandidate, MentionReference, MentionTarget } from './types';
interface MentionContextValue { workspaceId: string; scope: string; key: string; target: MentionTarget; get: (id: string) => MentionReference | undefined; remember: (person: MentionCandidate) => void }
const Context = createContext<MentionContextValue | null>(null);
export function MentionProvider({ workspaceId, session, target, references = [], projectionScope, children }: { workspaceId: string; session: Session; target: MentionTarget; references?: MentionReference[]; projectionScope?: string; children: ReactNode }) {
  const scope = useSessionScope(session); const key = `${workspaceId}:${scope}:${JSON.stringify(target)}`;
  const seeds = useMemo(() => new Map<string, MentionReference>(), [key]); const [revision, setRevision] = useState(0);
  // A receipt is bound to the context in which it arrived. Reusing its array
  // during a workspace/owner change cannot briefly reveal the former names.
  const receipt = useMemo(() => ({ key: projectionScope === scope ? key : '', references }), [references, projectionScope]);
  const projected = useMemo(() => new Map((receipt.key === key ? receipt.references : []).map(item => [item.id.toLowerCase(), item])), [receipt, key]);
  const value = useMemo(() => ({ workspaceId, scope, key, target,
    get: (id: string) => projected.get(id.toLowerCase()) ?? seeds.get(id.toLowerCase()),
    remember(person: MentionCandidate) { const id = person.id.toLowerCase(); if (seeds.size >= 50 && !seeds.has(id)) seeds.delete(seeds.keys().next().value!); seeds.set(id, { ...person, available: true }); setRevision(value => value + 1); },
  }), [key, projected, seeds, revision]);
  return <Context.Provider value={value}>{children}</Context.Provider>;
}
export function useMentions() { return useContext(Context); }
