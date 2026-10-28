import type { ProjectProgress } from './types';

export function PlanningCountSource({ source, factsAsOf }: { source: ProjectProgress['calculation_source']; factsAsOf: string | null }) {
  if (source === 'snapshot') return null;
  return <p className="project-help">Count source: {source === 'projection' ? 'Projection verified against current issues' : 'Live issue data'}.{source === 'projection' && factsAsOf && <> Oldest contributing fact calculated <time data-planning-facts dateTime={factsAsOf}>{new Date(factsAsOf).toLocaleString(undefined, { dateStyle: 'medium', timeStyle: 'short' })}</time>.</>}</p>;
}
