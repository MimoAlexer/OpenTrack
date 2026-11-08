import { healthLabels, states, type ProjectUpdate } from './types';

export type UpdateContext = NonNullable<ProjectUpdate['context']>;
export type UpdateNotes = NonNullable<UpdateContext['notes']>;
export type UpdateRisk = UpdateNotes['risks'][number];
export type UpdateDecision = UpdateNotes['decisions'][number];
export type UpdateSnapshot = Pick<ProjectUpdate, 'id' | 'created_at' | 'state' | 'health' | 'completed' | 'eligible' | 'context'>;

export const riskStatuses: Record<UpdateRisk['status'], string> = { open: 'Open', mitigated: 'Mitigated', closed: 'Closed' };
export const decisionStatuses: Record<UpdateDecision['status'], string> = { needed: 'Decision needed', decided: 'Decided', deferred: 'Deferred' };
export const emptyNotes = (): UpdateNotes => ({ risks: [], decisions: [] });
export const copyNotes = (notes: UpdateNotes): UpdateNotes => ({ risks: notes.risks.map(note => ({ ...note })), decisions: notes.decisions.map(note => ({ ...note })) });
export const updateTime = (value: string) => new Date(value).toLocaleString(undefined, { dateStyle: 'medium', timeStyle: 'short' });
export const updateDate = (value: string | null) => value ? new Date(`${value}T12:00:00`).toLocaleDateString(undefined, { day: 'numeric', month: 'short', year: 'numeric' }) : 'Not set';

const progress = (item: UpdateSnapshot) => item.eligible ? `${item.completed} of ${item.eligible} eligible issues completed` : 'no eligible issue scope';

export function compareUpdateSnapshots(update: UpdateSnapshot, previous: UpdateSnapshot) {
  const facts = [
    update.state !== previous.state && `State: this update ${states[update.state]}; compared update ${states[previous.state]}`,
    update.health !== previous.health && `Health: this update ${healthLabels[update.health]}; compared update ${healthLabels[previous.health]}`,
    (update.completed !== previous.completed || update.eligible !== previous.eligible) && `Progress: this update ${progress(update)}; compared update ${progress(previous)}`,
  ].filter((value): value is string => Boolean(value));
  const dates = update.context && previous.context ? [
    update.context.start_date !== previous.context.start_date && `Start date: ${updateDate(previous.context.start_date)} → ${updateDate(update.context.start_date)}`,
    update.context.target_date !== previous.context.target_date && `Target date: ${updateDate(previous.context.target_date)} → ${updateDate(update.context.target_date)}`,
  ].filter((value): value is string => Boolean(value)) : null;
  const notes = update.context?.notes && previous.context?.notes ? [
    ...compareNotes('Risk', update.context.notes.risks, previous.context.notes.risks, riskStatuses),
    ...compareNotes('Decision', update.context.notes.decisions, previous.context.notes.decisions, decisionStatuses),
  ] : null;
  return { facts, dates, notes };
}

function compareNotes<T extends { id: string; title: string; detail?: string; status: string }>(label: string, current: T[], previous: T[], statuses: Readonly<Record<string, string>>): string[] {
  const old = new Map(previous.map(note => [note.id, note]));
  const changes: string[] = [];
  for (const note of current) {
    const prior = old.get(note.id);
    if (!prior) changes.push(`${label} added: ${note.title} (${statuses[note.status]}).`);
    else {
      const fields = [prior.title !== note.title && `title was “${prior.title}”`, (prior.detail ?? '') !== (note.detail ?? '') && 'details changed', prior.status !== note.status && `${statuses[prior.status]} → ${statuses[note.status]}`].filter(Boolean);
      if (fields.length) changes.push(`${label} changed: ${note.title} (${fields.join('; ')}).`);
      old.delete(note.id);
    }
  }
  for (const note of old.values()) changes.push(`${label} removed from this update: ${note.title}.`);
  return changes;
}

export function notesProblem(notes: UpdateNotes): string | null {
  if (notes.risks.length > 6 || notes.decisions.length > 6) return 'Record at most six risks and six decisions in an update.';
  for (const [label, items] of [['Risk', notes.risks], ['Decision', notes.decisions]] as const) {
    for (const item of items) {
      const titleLength = Array.from(item.title.trim()).length;
      if (!titleLength || titleLength > 160) return `${label} titles require 1–160 characters.`;
      if (Array.from((item.detail ?? '').trim()).length > 1000) return `${label} details allow at most 1,000 characters.`;
    }
  }
  return null;
}
