import { describe, expect, it } from 'vitest';
import canonical from '../../../crates/opentrack-domain/tests/fixtures/issue-query-v1.json';
import { commentHash, commentIdFromHash, fromFlat, parseStructuredHash, queryBody, queryFragment, validateFilter, validateView, viewHash, type FilterNode, type StructuredView } from './model';
import type { Session } from '../types';

const view = (): StructuredView => ({ ...structuredClone(canonical), layout: 'list' }) as StructuredView;
const id = (number: number) => `00000000-0000-4000-8000-${number.toString(16).padStart(12, '0')}`;

describe('structured query URL state', () => {
  it('uses the canonical Rust fixture unchanged as the query body', () => {
    const state = view();
    expect(validateView(state)).toBeNull();
    const parsed = parseStructuredHash(viewHash(state));
    expect(parsed.kind).toBe('valid');
    if (parsed.kind !== 'valid') throw new Error('Expected valid state');
    expect(queryBody(parsed.view)).toEqual(canonical);
    expect(queryBody(parsed.view, 'opaque-cursor')).toEqual({ ...canonical, cursor: 'opaque-cursor' });
    expect(queryBody(parsed.view)).not.toHaveProperty('layout');
    expect(queryBody(parsed.view)).not.toHaveProperty('as_of');
  });

  it('keeps comment anchors separate from complete query intent and preserves literal plus signs', () => {
    const state = { ...view(), q: 'C++ and a+b' }; const comment = id(70);
    const linked = commentHash(viewHash(state), comment);
    expect(parseStructuredHash(linked)).toEqual({ kind: 'valid', view: state, commentId: comment });
    expect(queryFragment(linked)).toBe(viewHash(state));
    expect(commentIdFromHash(linked)).toBe(comment);
    expect(parseStructuredHash(`#comment-${comment}`)).toEqual({ kind: 'absent' });
    expect(commentIdFromHash(`#comment-${comment}`)).toBe(comment);
    expect(parseStructuredHash(`${linked}&comment=${comment}`).kind).toBe('invalid');
    expect(parseStructuredHash(`${linked}&unknown=1`).kind).toBe('invalid');
    expect(parseStructuredHash(`${viewHash(state)}&comment=bad`).kind).toBe('invalid');
    expect(parseStructuredHash(`#filters=${JSON.stringify(state)}`)).toEqual({ kind: 'valid', view: state });
    expect(commentHash('#filters=invalid', comment)).toBe('#filters=invalid');
  });

  it('round-trips the legal 151-node, fifty-condition, five-thousand-value shape', () => {
    const predicate: FilterNode = { kind: 'predicate', field: 'status', op: 'in', values: Array.from({ length: 100 }, (_, index) => id(index)) };
    const maximum: FilterNode = { kind: 'all', children: Array.from({ length: 50 }, () => ({ kind: 'all', children: [{ kind: 'any', children: [structuredClone(predicate)] }] })) };
    const state = { ...view(), filter: maximum };
    expect(validateFilter(maximum)).toBeNull();
    expect(parseStructuredHash(viewHash(state))).toEqual({ kind: 'valid', view: state });
  });

  it.each([
    { kind: 'all', children: [] }, { kind: 'any', children: [] },
    { kind: 'all', children: [{ kind: 'predicate', field: 'priority', op: 'eq', value: 1 }], sql: 'OR true' },
    { kind: 'predicate', field: 'title', op: 'eq', value: 'anything' },
    { kind: 'predicate', field: 'priority', op: 'execute', value: 1 },
    { kind: 'predicate', field: 'priority', op: 'eq', value: 1, values: [] },
    { kind: 'predicate', field: 'priority', op: 'in', value: 1, values: [1] },
    { kind: 'predicate', field: 'priority', op: 'eq', value: null },
    { kind: 'predicate', field: 'priority', op: 'eq', value: 5 },
    { kind: 'predicate', field: 'assignee', op: 'eq', value: null },
    { kind: 'predicate', field: 'assignee', op: 'in', values: [null] },
    { kind: 'predicate', field: 'priority', op: 'is_null', value: true },
    { kind: 'predicate', field: 'due_date', op: 'is_null', value: 'true' },
    { kind: 'predicate', field: 'due_date', op: 'before', value: '2026-02-30' },
    { kind: 'predicate', field: 'due_date', op: 'eq', value: '2026-11-08T12:00:00Z' },
    { kind: 'predicate', field: 'updated_at', op: 'eq', value: '2026-11-08T12:00:00' },
    { kind: 'predicate', field: 'updated_at', op: 'eq', value: '2026-11-08T24:00:00Z' },
    { kind: 'predicate', field: 'updated_at', op: 'eq', value: '2026-11-08T12:00:00.000000001Z' },
    { kind: 'predicate', field: 'created_at', op: 'after', value: { relative_days: 36501 } },
    { kind: 'predicate', field: 'created_at', op: 'after', value: { relative_days: 1, clock: 'client' } },
    { kind: 'predicate', field: 'label', op: 'in', values: [] },
    { kind: 'predicate', field: 'team', op: 'eq', value: 'missing or private' },
  ])('rejects malformed or unsupported conditions without broadening: %j', filter => {
    expect(validateFilter(filter)).not.toBeNull();
    const state = { ...view(), filter };
    expect(parseStructuredHash(`#filters=${encodeURIComponent(JSON.stringify(state))}`).kind).toBe('invalid');
  });

  it('bounds depth, total nodes, conditions and set cardinality independently', () => {
    const leaf: FilterNode = { kind: 'predicate', field: 'priority', op: 'eq', value: 1 };
    let nested: FilterNode = leaf; for (let index = 0; index < 4; index++) nested = { kind: 'all', children: [nested] };
    expect(validateFilter(nested)).toMatch(/four/);
    expect(validateFilter({ kind: 'all', children: Array.from({ length: 51 }, () => leaf) })).toMatch(/50/);
    expect(validateFilter({ kind: 'all', children: Array.from({ length: 202 }, () => ({ kind: 'all', children: [leaf] })) })).not.toBeNull();
    expect(validateFilter({ kind: 'predicate', field: 'label', op: 'in', values: Array.from({ length: 101 }, (_, index) => id(index)) })).toMatch(/100/);
  });

  it.each(['#other=1', '#filters=%', '#filters=null', `#filters=${encodeURIComponent(JSON.stringify({ ...view(), version: 2 }))}`, `#filters=${encodeURIComponent(JSON.stringify({ ...view(), cursor: 'persisted' }))}`])('blocks unreadable, unknown and expired state contracts: %s', hash => {
    expect(parseStructuredHash(hash).kind).toBe('invalid');
  });

  it('keeps calendar dates and relative days unchanged across a DST boundary', () => {
    const state = { ...view(), timezone: 'Europe/Zurich', filter: { kind: 'predicate', field: 'created_at', op: 'after', value: '2026-03-29' } as FilterNode };
    expect(queryBody(state).filter).toEqual({ kind: 'predicate', field: 'created_at', op: 'after', value: '2026-03-29' });
    expect(parseStructuredHash(viewHash(state))).toEqual({ kind: 'valid', view: state });
    expect(validateFilter({ kind: 'predicate', field: 'due_date', op: 'before', value: { relative_days: -36500 } })).toBeNull();
  });

  it.each([
    '2026-01-01T12:00:00.123456000000Z',
    `2026-01-01T12:00:00.123456${'0'.repeat(37)}Z`,
  ])('round-trips bounded exact microsecond timestamps with extra zero digits: %s', instant => {
    const state = { ...view(), filter: { kind: 'predicate', field: 'created_at', op: 'eq', value: instant } as FilterNode };
    expect(instant.length).toBeLessThanOrEqual(64);
    expect(validateView(state)).toBeNull();
    expect(queryBody(state).filter).toEqual({ kind: 'predicate', field: 'created_at', op: 'eq', value: instant });
    expect(parseStructuredHash(viewHash(state))).toEqual({ kind: 'valid', view: state });
  });

  it.each([
    '2026-01-01T12:00:00.123456000001Z',
    '2016-12-31T23:59:60Z',
    `2026-01-01T12:00:00.123456${'0'.repeat(38)}Z`,
  ])('rejects significant discarded timestamp digits, leap seconds and values beyond 64 characters: %s', instant => {
    expect(validateFilter({ kind: 'predicate', field: 'updated_at', op: 'eq', value: instant })).not.toBeNull();
  });

  it('blocks imported text queries containing a null character before constructing a query', () => {
    const state = { ...view(), q: 'visible\0hidden' };
    expect(validateView(state)).not.toBeNull();
    expect(parseStructuredHash(`#filters=${encodeURIComponent(JSON.stringify(state))}`).kind).toBe('invalid');
  });

  it.each([
    ['America/Havana', '2026-11-01'],
    ['America/Goose_Bay', '1988-10-30'],
    ['Pacific/Apia', '2011-12-30'],
  ])('preserves a calendar date independently of midnight uniqueness in %s', (timezone, date) => {
    const state = { ...view(), timezone, filter: { kind: 'predicate', field: 'created_at', op: 'eq', value: date } as FilterNode };
    expect(validateView(state)).toBeNull();
    expect(queryBody(state).filter).toEqual({ kind: 'predicate', field: 'created_at', op: 'eq', value: date });
    expect(parseStructuredHash(viewHash(state))).toEqual({ kind: 'valid', view: state });
  });

  it('rejects invalid timezone, display and manual scope without rewriting them', () => {
    for (const changes of [{ timezone: 'MadeUp/Nowhere' }, { timezone: 'utc' }, { layout: 'timeline' }, { sort: 'raw SQL' }, { sort: 'manual' }, { sort: 'manual', team_id: id(1), archived: true }, { tenant_timezone: 'UTC' }]) expect(validateView({ ...view(), ...changes })).not.toBeNull();
    expect(validateView({ ...view(), filter: null })).toBeNull();
  });

  it('translates existing flat label-all and My work intent without losing predicates', () => {
    const session = { user: { id: id(1) } } as Session;
    const state = fromFlat(new URLSearchParams({ view: 'mine', priority: '0', status_id: id(2), label_ids: `${id(3)},${id(4)}`, label_mode: 'all', team_id: id(5), q: 'keep this', layout: 'board' }), session);
    expect(validateView(state)).toBeNull();
    expect(state.filter?.kind).toBe('all');
    if (state.filter?.kind !== 'all') throw new Error('Expected all group');
    expect(state.filter.children).toEqual([
      { kind: 'predicate', field: 'status', op: 'eq', value: id(2) },
      { kind: 'predicate', field: 'priority', op: 'eq', value: 0 },
      { kind: 'predicate', field: 'assignee', op: 'eq', value: id(1) },
      { kind: 'predicate', field: 'label', op: 'eq', value: id(3) },
      { kind: 'predicate', field: 'label', op: 'eq', value: id(4) },
    ]);
    expect(state).toMatchObject({ team_id: id(5), q: 'keep this', layout: 'board' });
  });
});
