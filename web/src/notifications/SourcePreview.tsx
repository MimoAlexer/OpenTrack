import { useEffect, useRef, useState } from 'react';
import { ArrowUpRight, LockKeyhole } from 'lucide-react';
import { ApiError } from '../api';
import { ErrorBanner, Loading } from '../components';
import type { IssueComment, StructuredDocument } from '../documents/types';
import { issuesUrl, Link } from '../router';
import { projectPath, projectUrl, type FullProject } from '../projects/types';
import type { Issue, Session } from '../types';
import { isAborted, useNotificationScope } from './scope';
import type { NotificationItem } from './types';

type DenialScope = 'entity' | 'preview';
class SourceDenied extends Error { constructor(readonly scope: DenialScope) { super('Source unavailable'); } }
interface Preview { title: string; key?: string; body: string; href: string }
export function SourcePreview({ workspaceId, session, accessScope, item, refresh, unavailable }: { workspaceId: string; session: Session; accessScope: string; item: NotificationItem; refresh: number; unavailable: (source: NotificationItem['source'], kind: DenialScope) => void }) {
  const scope = useNotificationScope(workspaceId, session, accessScope); const source = item.source;
  const sourceKey = `${scope.key}:${item.id}:${source.kind}:${source.id}:${source.team_id ?? ''}:${source.document_id ?? ''}:${source.comment_id ?? ''}:${source.available}`;
  const [result, setResult] = useState<{ key: string; value: Preview } | null>(null);
  const [error, setError] = useState<unknown>(null); const [denied, setDenied] = useState(false); const [retry, setRetry] = useState(0);
  const callback = useRef(unavailable); callback.current = unavailable;
  useEffect(() => {
    const abort = new AbortController(); setResult(null); setError(null); setDenied(false);
    if (!source.available || source.kind === 'project' && !source.team_id) return () => abort.abort();
    async function read<T>(path: string, kind: DenialScope): Promise<T> {
      try { return await scope.read<T>(path, { signal: abort.signal }); }
      catch (error) { if (error instanceof ApiError && [403, 404].includes(error.status)) throw new SourceDenied(kind); throw error; }
    }
    async function preview(): Promise<Preview> {
      if (source.kind === 'project') {
        const project = await read<FullProject>(projectPath(workspaceId, source.team_id!, source.id), 'entity');
        return { title: project.name, body: project.narrative.plain_text, href: projectUrl(workspaceId, project.team_id, project.id) };
      }
      const path = `/workspaces/${encodeURIComponent(workspaceId)}`;
      // Resolve entity authority before a narrower comment preview. A missing
      // comment cannot turn an otherwise readable assignment into a denial.
      const issue = await read<Issue>(`${path}/issues/${encodeURIComponent(source.id)}`, 'entity');
      let document: StructuredDocument;
      if (source.comment_id) {
        const comment = await read<IssueComment>(`${path}/comments/${encodeURIComponent(source.comment_id)}`, 'preview');
        if (comment.issue_id !== source.id || !comment.document) throw new SourceDenied('preview');
        document = comment.document;
      } else document = await read<StructuredDocument>(`${path}/issues/${encodeURIComponent(source.id)}/document`, 'entity');
      return { title: issue.title, key: issue.key, body: document.plain_text, href: `${issuesUrl(workspaceId, new URLSearchParams(), issue.id)}${source.comment_id ? `#comment-${encodeURIComponent(source.comment_id)}` : ''}` };
    }
    void preview().then(value => { if (!abort.signal.aborted && scope.current()) setResult({ key: sourceKey, value }); }).catch(error => {
      if (abort.signal.aborted || !scope.current() || isAborted(error)) return;
      if (error instanceof SourceDenied) { setResult(null); setDenied(true); callback.current(source, error.scope); }
      else { setResult(null); setError(error); }
    });
    return () => abort.abort();
  }, [sourceKey, scope.read, scope.current, workspaceId, refresh, retry]);
  if (!source.available || denied || source.kind === 'project' && !source.team_id) return <div className="inbox-source-unavailable"><LockKeyhole aria-hidden="true"/><h3>Source unavailable</h3><p>This source may have been removed, or your access has changed.</p></div>;
  if (error) return <ErrorBanner error={error} retry={() => setRetry(value => value + 1)}/>;
  const preview = result?.key === sourceKey ? result.value : null;
  if (!preview) return <Loading label="Loading source…"/>;
  return <article className="inbox-source"><header>{preview.key && <span className="inbox-source-key">{preview.key}</span>}<h2>{preview.title}</h2><Link className="button compact" href={preview.href}>Open {source.kind}<ArrowUpRight aria-hidden="true"/></Link></header><div className="inbox-source-body">{preview.body || <span className="muted">No description yet.</span>}</div></article>;
}
