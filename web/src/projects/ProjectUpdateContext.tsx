import type { ProjectUpdate } from './types';
import { compareUpdateSnapshots, decisionStatuses, riskStatuses, updateDate, updateTime, type UpdateContext, type UpdateSnapshot } from './updateContext';

export function ProjectUpdateContext({ context, attentionOnly = false }: { context: UpdateContext | null | undefined; attentionOnly?: boolean }) {
  const risks = context?.notes?.risks.filter(note => !attentionOnly || note.status === 'open');
  const decisions = context?.notes?.decisions.filter(note => !attentionOnly || note.status === 'needed');
  return <div className="project-update-context">
    {context ? <dl className="project-update-dates"><div><dt>Start date when published</dt><dd>{updateDate(context.start_date)}</dd></div><div><dt>Target date when published</dt><dd>{updateDate(context.target_date)}</dd></div></dl> : <p className="muted">Project dates were not captured for this update.</p>}
    {!context?.notes ? <p className="muted">Risks and decisions were not recorded for this update.</p> : <div className="project-update-note-groups">
      <section aria-label={attentionOnly ? 'Open risks in latest update' : 'Risks recorded in update'}><h4>{attentionOnly ? 'Open risks' : 'Risks'}</h4>{risks?.length ? <ul>{risks.map(note => <li key={note.id}><div><strong>{note.title}</strong><span>{riskStatuses[note.status]}</span></div>{note.detail && <p>{note.detail}</p>}</li>)}</ul> : <p>{attentionOnly ? 'No open risks recorded in this update.' : 'No risks recorded in this update.'}</p>}</section>
      <section aria-label={attentionOnly ? 'Decisions needed in latest update' : 'Decisions recorded in update'}><h4>{attentionOnly ? 'Decisions needed' : 'Decisions'}</h4>{decisions?.length ? <ul>{decisions.map(note => <li key={note.id}><div><strong>{note.title}</strong><span>{decisionStatuses[note.status]}</span></div>{note.detail && <p>{note.detail}</p>}</li>)}</ul> : <p>{attentionOnly ? 'No decisions awaiting action recorded in this update.' : 'No decisions recorded in this update.'}</p>}</section>
    </div>}
  </div>;
}

export function ProjectUpdateComparison({ update, previous, targetId }: { update: ProjectUpdate; previous: UpdateSnapshot; targetId?: string }) {
  const changes = compareUpdateSnapshots(update, previous);
  return <div className="project-update-comparison"><p>Compared with the update from {targetId ? <a href={`#${targetId}`} aria-label={`Go to compared update from ${updateTime(previous.created_at)}`} onClick={event => { event.preventDefault(); document.getElementById(targetId)?.focus(); }}><time dateTime={previous.created_at}>{updateTime(previous.created_at)}</time></a> : <time dateTime={previous.created_at}>{updateTime(previous.created_at)}</time>}:</p>
    {changes.facts.length ? <ul>{changes.facts.map(change => <li key={change}>{change}</li>)}</ul> : <p>State, health and progress match.</p>}
    {changes.dates === null ? <p className="muted">Date comparison unavailable: project dates were not captured in one or both updates.</p> : changes.dates.length ? <ul>{changes.dates.map(change => <li key={change}>{change}</li>)}</ul> : <p>Recorded project dates match.</p>}
    {changes.notes === null ? <p className="muted">Risk and decision comparison unavailable: notes were not recorded in one or both updates.</p> : changes.notes.length ? <ul>{changes.notes.map(change => <li key={change}>{change}</li>)}</ul> : <p>Recorded risks and decisions match.</p>}
  </div>;
}
