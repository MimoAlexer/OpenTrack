import { ErrorBanner, Loading } from '../components';
import { isAccessError } from '../documents/model';
import { ProjectUpdateContext } from './ProjectUpdateContext';
import type { Page, ProjectUpdate } from './types';
import { updateTime } from './updateContext';

interface Resource { value: Page<ProjectUpdate> | null; loading: boolean; error: unknown; retry: () => void }
export function ProjectUpdateOverview({ resource, openUpdates }: { resource: Resource; openUpdates: () => void }) {
  const latest = !isAccessError(resource.error) && resource.value?.data[0];
  return <section className="project-latest-update" aria-label="Latest project update" aria-busy={resource.loading}>
    <header className="project-section-heading"><h3>Latest update</h3><button type="button" className="quiet" onClick={openUpdates}>View updates</button></header>
    <ErrorBanner error={resource.error} retry={resource.retry}/>
    {latest ? <><p className="project-update-attribution"><strong>{latest.author_name}</strong><time dateTime={latest.created_at}>{updateTime(latest.created_at)}</time></p>{resource.loading ? <p role="status">Refreshing the latest update. Showing the last confirmed update.</p> : resource.error ? <p role="status">The latest update could not be refreshed. Showing the last confirmed update.</p> : null}<p className="project-update-body">{latest.body}</p><ProjectUpdateContext context={latest.context} attentionOnly/></> : resource.loading ? <Loading label="Loading latest project update…"/> : !resource.error && <p>No project update has been published yet. Open Updates to record progress, risks and decisions.</p>}
  </section>;
}
