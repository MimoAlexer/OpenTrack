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

interface Preview { title: string; key?: string; body: string; href: string }
export function SourcePreview({ workspaceId, session, accessScope, item, refresh, unavailable }: { workspaceId: string; session: Session; accessScope: string; item: NotificationItem; refresh: number; unavailable: (source: NotificationItem['source']) => void }) {
  const scope = useNotificationScope(workspaceId, session, accessScope); const source = item.source;
  const sourceKey = `${scope.key}:${item.id}:${source.kind}:${source.id}:${source.team_id ?? ''}:${source.document_id ?? ''}:${source.comment_id ?? ''}:${source.available}`;
  const [result, setResult] = useState<{ key: string; value: Preview } | null>(null);
  const [error, setError] = useState<unknown>(null); const [denied, setDenied] = useState(false); const [retry, setRetry] = useState(0);
  const callback = useRef(unavailable); callback.current = unavailable;
  useEffect(() => {
    const abort = new AbortController(); setResult(null); setError(null); setDenied(false);
    if (!source.available || source.kind === 'project' && !source.team_id) return () => abort.abort();
    async function preview(): Promise<Preview> {
      if (source.kind === 'project') {
        const project = await scope.read<FullProject>(projectPath(workspaceId, source.team_id!, source.id), { signal: abort.signal });
        return { title: project.name, body: project.narrative.plain_text, href: projectUrl(workspaceId, project.team_id, project.id) };
      }
      const path = `/workspaces/${encodeURIComponent(workspaceId)}`;
      const [issue, document] = await Promise.all([
        scope.read<Issue>(`${path}/issues/${encodeURIComponent(source.id)}`, { signal: abort.signal }),
        source.comment_id ? scope.read<IssueComment>(`${path}/comments/${encodeURIComponent(source.comment_id)}`, { signal: abort.signal }).then(comment => {
          if (comment.issue_id !== source.id || !comment.document) throw new ApiError(404, 'source_unavailable', 'Source unavailable.');
          return comment.document;
        }) : scope.read<StructuredDocument>(`${path}/issues/${encodeURIComponent(source.id)}/document`, { signal: abort.signal }),
      ]);
      return { title: issue.title, key: issue.key, body: document.plain_text, href: `${issuesUrl(workspaceId, new URLSearchParams(), issue.id)}${source.comment_id ? `#comment-${encodeURIComponent(source.comment_id)}` : ''}` };
    }
    void preview().then(value => { if (!abort.signal.aborted && scope.current()) setResult({ key: sourceKey, value }); }).catch(error => {
      if (abort.signal.aborted || !scope.current() || isAborted(error)) return;
      if (error instanceof ApiError && [403, 404].includes(error.status)) { setResult(null); setDenied(true); callback.current(source); }
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
