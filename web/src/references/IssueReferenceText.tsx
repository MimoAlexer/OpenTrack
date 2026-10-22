import { Link, issuesUrl } from '../router';
import { useIssueReferences } from './context';
import './references.css';

export function IssueReferenceText({ id }: { id: string }) {
  const context = useIssueReferences(); const value = context?.get(id);
  if (!context || !value?.available || !value.key || !value.title) return <span className="issue-reference unavailable" title="Issue unavailable">#issue</span>;
  const label = `${value.key}: ${value.title}`;
  return <Link className="issue-reference" href={issuesUrl(context.workspaceId, undefined, id)} title={label} aria-label={label}>{value.key}</Link>;
}
