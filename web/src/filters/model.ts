import type { Session } from '../types';
import type { components } from '../generated/api';

export const fields = ['status', 'priority', 'assignee', 'project', 'cycle', 'label', 'team', 'created_at', 'updated_at', 'due_date'] as const;
export type FilterField = components['schemas']['IssueQueryField'];
export type FilterOperator = components['schemas']['IssueQueryOperator'];
export type FilterValue = components['schemas']['IssueQueryValue'];
export type FilterNode = components['schemas']['IssueFilterNode'];
export type Predicate = Extract<FilterNode, { kind: 'predicate' }>;
export type FilterGroup = Exclude<FilterNode, Predicate>;
export interface IssueQuery {
  version: 1; filter: FilterNode | null; sort: 'created' | 'priority' | 'manual';
  team_id?: string; q?: string; archived: boolean; limit: number; timezone: string; cursor?: string;
}
export interface StructuredView extends Omit<IssueQuery, 'cursor'> { layout: 'list' | 'board' }
export type ParsedView = { kind: 'absent' } | { kind: 'valid'; view: StructuredView; commentId?: string } | { kind: 'invalid'; error: string };
export const structuredMarker = '_structured';
export const MAX_STATE_BYTES = 1024 * 1024;
export const fieldLabels: Record<FilterField, string> = {
  status: 'Status', priority: 'Priority', assignee: 'Assignee', project: 'Project', cycle: 'Cycle', label: 'Label', team: 'Team', created_at: 'Created', updated_at: 'Updated', due_date: 'Due date',
};
const identifiers: FilterField[] = ['status', 'assignee', 'project', 'cycle', 'label', 'team'];
const nullable: FilterField[] = ['assignee', 'project', 'cycle', 'label', 'due_date'];
export const isDateField = (field: FilterField) => ['created_at', 'updated_at', 'due_date'].includes(field);
export function operators(field: FilterField): FilterOperator[] {
  return ['eq', 'in', 'not_in', ...(isDateField(field) ? ['before', 'after'] as const : []), ...(nullable.includes(field) ? ['is_null'] as const : [])];
}
export const operatorLabels: Record<FilterOperator, string> = { eq: 'Is', in: 'Is any of', not_in: 'Is none of', before: 'Before', after: 'After', is_null: 'Is empty' };
export const uuid = (value: unknown): value is string => typeof value === 'string' && /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(value);
const object = (value: unknown): value is Record<string, unknown> => !!value && typeof value === 'object' && !Array.isArray(value);
function exact(value: Record<string, unknown>, allowed: string[]) { return Object.keys(value).every(key => allowed.includes(key)); }
function localDate(value: string) {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(value)) return false;
  const parsed = new Date(`${value}T00:00:00Z`);
  return Number.isFinite(parsed.valueOf()) && parsed.toISOString().slice(0, 10) === value;
}
function timestamp(value: string) {
  const time = value.match(/[Tt](\d{2}):(\d{2}):(\d{2})/);
  if (!time || Number(time[1]) > 23 || Number(time[2]) > 59 || Number(time[3]) > 59) return false;
  const fraction = value.match(/\.(\d+)/)?.[1];
  if (fraction && fraction.length > 6 && !/^0+$/.test(fraction.slice(6))) return false;
  return /^\d{4}-\d{2}-\d{2}[Tt]\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:[Zz]|[+-]\d{2}:\d{2})$/.test(value)
    && localDate(value.slice(0, 10)) && Number.isFinite(Date.parse(value));
}
function validValue(field: FilterField, value: unknown) {
  if (identifiers.includes(field)) return uuid(value);
  if (field === 'priority') return typeof value === 'number' && Number.isInteger(value) && value >= 0 && value <= 4;
  if (object(value)) return exact(value, ['relative_days']) && Number.isInteger(value.relative_days) && Math.abs(value.relative_days as number) <= 36500;
  return typeof value === 'string' && value.length <= 64 && (localDate(value) || field !== 'due_date' && timestamp(value));
}

export function validateFilter(filter: unknown): string | null {
  if (filter === null) return null;
  let nodes = 0; let predicates = 0;
  function visit(node: unknown, depth: number): string | null {
    if (++nodes > 201) return 'Filters support at most 201 total conditions and groups.';
    if (depth > 4) return 'Filters support at most four nested levels.';
    if (!object(node)) return 'A filter condition or group is malformed.';
    if (node.kind === 'all' || node.kind === 'any') {
      if (!exact(node, ['kind', 'children']) || !Array.isArray(node.children) || !node.children.length) return 'Every filter group must contain a condition.';
      for (const child of node.children) { const error = visit(child, depth + 1); if (error) return error; }
      return null;
    }
    if (node.kind !== 'predicate' || !exact(node, ['kind', 'field', 'op', 'value', 'values']) || !fields.includes(node.field as FilterField)) return 'This filter field or structure is unsupported.';
    if (++predicates > 50) return 'Filters support at most 50 conditions.';
    const field = node.field as FilterField;
    if (!operators(field).includes(node.op as FilterOperator)) return 'This operator is unavailable for the selected field.';
    const hasValue = Object.hasOwn(node, 'value'); const hasValues = Object.hasOwn(node, 'values');
    if (node.op === 'is_null') return hasValue && !hasValues && typeof node.value === 'boolean' ? null : 'Choose whether this field must be empty.';
    if (node.op === 'in' || node.op === 'not_in') {
      if (hasValue || !hasValues || !Array.isArray(node.values) || node.values.length < 1 || node.values.length > 100 || node.values.some(value => !validValue(field, value))) return 'Choose between one and 100 valid values for this condition.';
      return null;
    }
    return hasValue && !hasValues && validValue(field, node.value) ? null : 'Choose a valid value for this condition.';
  }
  return visit(filter, 1);
}

export function validateView(value: unknown): string | null {
  if (!object(value) || !exact(value, ['version', 'layout', 'filter', 'sort', 'team_id', 'q', 'archived', 'limit', 'timezone'])) return 'These filter settings contain unsupported fields.';
  if (value.version !== 1) return 'This filter version is unsupported.';
  if (!['list', 'board'].includes(value.layout as string) || !['created', 'priority', 'manual'].includes(value.sort as string)) return 'These filter display settings are unsupported.';
  if (typeof value.archived !== 'boolean' || !Number.isInteger(value.limit) || (value.limit as number) < 1 || (value.limit as number) > 100) return 'These filter limits are invalid.';
  if (value.layout === 'board' && (value.limit as number) > 50) return 'Boards support at most 50 issues per page.';
  if (value.team_id !== undefined && !uuid(value.team_id)) return 'Choose an available team.';
  if (value.q !== undefined && (typeof value.q !== 'string' || Array.from(value.q).length > 256 || value.q.includes('\0'))) return 'Issue search supports at most 256 characters without null characters.';
  if (typeof value.timezone !== 'string' || value.timezone.length > 100 || !/^(UTC|[A-Za-z_+-]+(?:\/[A-Za-z0-9_+-]+)+)$/.test(value.timezone)) return 'Choose an IANA query timezone, such as Europe/Zurich or UTC.';
  try { new Intl.DateTimeFormat('en', { timeZone: value.timezone }); } catch { return 'This query timezone is unavailable.'; }
  if (value.sort === 'manual' && (!value.team_id || value.archived)) return 'Manual ordering requires an explicit active team and active issues.';
  return validateFilter(value.filter);
}

export function parseStructuredHash(hash: string): ParsedView {
  if (!hash || legacyCommentId(hash)) return { kind: 'absent' };
  if (!hash.startsWith('#filters=') || hash.length > MAX_STATE_BYTES * 3 + 9) return { kind: 'invalid', error: 'These filter settings are unknown or too large.' };
  try {
    const pairs = hash.slice(1).split('&').map(part => {
      const separator = part.indexOf('='); if (separator <= 0) throw new Error('Invalid filter fragment field');
      return [decodeURIComponent(part.slice(0, separator)), decodeURIComponent(part.slice(separator + 1))] as const;
    });
    if (pairs.some(([key]) => !['filters', 'comment'].includes(key)) || pairs.filter(([key]) => key === 'filters').length !== 1 || pairs.filter(([key]) => key === 'comment').length > 1) throw new Error('Unsupported filter fragment fields');
    const json = pairs.find(([key]) => key === 'filters')![1];
    const comment = pairs.find(([key]) => key === 'comment')?.[1];
    if (comment !== undefined && !uuid(comment)) throw new Error('Invalid comment reference');
    if (new TextEncoder().encode(json).length > MAX_STATE_BYTES) return { kind: 'invalid', error: 'These filter settings are too large.' };
    const value: unknown = JSON.parse(json); const error = validateView(value);
    return error ? { kind: 'invalid', error } : { kind: 'valid', view: value as StructuredView, ...(comment ? { commentId: comment.toLowerCase() } : {}) };
  } catch { return { kind: 'invalid', error: 'These filter settings could not be read. Repair or clear them before running a query.' }; }
}
function legacyCommentId(hash: string) { const id = /^#comment-(.*)$/.exec(hash)?.[1]; return uuid(id) ? id.toLowerCase() : undefined; }
export function commentIdFromHash(hash: string) { const parsed = parseStructuredHash(hash); return parsed.kind === 'valid' ? parsed.commentId : legacyCommentId(hash); }
export function commentHash(hash: string, commentId: string) {
  const parsed = parseStructuredHash(hash);
  return parsed.kind === 'valid' ? `${viewHash(parsed.view)}&comment=${encodeURIComponent(commentId.toLowerCase())}` : parsed.kind === 'absent' ? `#comment-${commentId}` : hash;
}
export function queryFragment(hash: string) { const parsed = parseStructuredHash(hash); return parsed.kind === 'valid' ? viewHash(parsed.view) : parsed.kind === 'absent' ? '' : hash; }
export function viewHash(view: StructuredView) {
  const error = validateView(view); if (error) throw new Error(error);
  const json = JSON.stringify(view); if (new TextEncoder().encode(json).length > MAX_STATE_BYTES) throw new Error('These filter settings are too large.');
  return `#filters=${encodeURIComponent(json)}`;
}
export function queryBody(view: StructuredView, cursor?: string): IssueQuery {
  const { layout: _, ...query } = view;
  return (cursor ? { ...query, cursor } : query) satisfies components['schemas']['IssueQuery'];
}
export function viewParams(view: StructuredView) {
  const params = new URLSearchParams({ [structuredMarker]: '1', layout: view.layout, display_version: '1' });
  if (view.team_id) params.set('team_id', view.team_id);
  if (view.q) params.set('q', view.q);
  if (view.archived) params.set('archived', 'true');
  if (view.sort !== 'created') params.set('sort', view.sort);
  return params;
}
export function withBaseParams(view: StructuredView, params: URLSearchParams): StructuredView {
  const next: StructuredView = { ...view, layout: params.get('layout') === 'board' ? 'board' : 'list', sort: (params.get('sort') ?? 'created') as StructuredView['sort'], archived: params.get('archived') === 'true' };
  if (next.layout === 'board') next.limit = Math.min(next.limit, 50);
  if (params.has('team_id')) next.team_id = params.get('team_id')!; else delete next.team_id;
  if (params.has('q')) next.q = params.get('q')!; else delete next.q;
  return next;
}
export function fromFlat(params: URLSearchParams, session: Session): StructuredView {
  const conditions: FilterNode[] = [];
  const add = (field: FilterField, value: FilterValue) => conditions.push({ kind: 'predicate', field, op: 'eq', value });
  if (params.get('status_id')) add('status', params.get('status_id')!);
  if (params.get('priority')) add('priority', Number(params.get('priority')));
  if (params.get('view') === 'mine') add('assignee', session.user.id); else if (params.get('assignee_id')) add('assignee', params.get('assignee_id')!);
  const labels = [...new Set((params.get('label_ids') ?? '').split(',').filter(Boolean))];
  if (labels.length) {
    if (params.get('label_mode') === 'all') labels.forEach(id => add('label', id));
    else conditions.push({ kind: 'predicate', field: 'label', op: 'in', values: labels });
  }
  const view: StructuredView = { version: 1, layout: params.get('layout') === 'board' ? 'board' : 'list', filter: conditions.length === 1 ? conditions[0] : conditions.length ? { kind: 'all', children: conditions } : null, sort: (params.get('sort') ?? 'created') as StructuredView['sort'], archived: params.get('archived') === 'true', limit: 50, timezone: 'UTC' };
  if (params.get('team_id')) view.team_id = params.get('team_id')!;
  if (params.get('q')) view.q = params.get('q')!;
  return view;
}
export function predicateCount(node: FilterNode | null): number { return !node ? 0 : node.kind === 'predicate' ? 1 : node.children.reduce((count, child) => count + predicateCount(child), 0); }
export const defaultPredicate = (): Predicate => ({ kind: 'predicate', field: 'priority', op: 'eq', value: 3 });
