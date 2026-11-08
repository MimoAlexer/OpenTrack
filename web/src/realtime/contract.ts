// Local until the realtime schema is included in the generated API contract.
export type CanonicalEvent = {
  event_id: string; schema_version: 1; type: string; workspace_id: string;
  entity_type: string; entity_id: string; entity_version: string; actor_id: null;
  occurred_at: string; mutation_id: string | null; changed_fields: string[];
};
export type ResetReason = 'access_changed' | 'cursor_expired' | 'reset_required' | 'scope_changed';
export const resetReason = (value: unknown): value is ResetReason => ['access_changed', 'cursor_expired', 'reset_required', 'scope_changed'].includes(value as string);
export function cursorValue(value: unknown): value is string {
  return typeof value === 'string' && value.length > 0 && value.length <= 4096 && /^[\x21-\x7e]+$/.test(value);
}
const uuid = (value: unknown) => typeof value === 'string' && /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/.test(value);
const word = (value: unknown) => typeof value === 'string' && value.length > 0 && value.length <= 128 && /^[a-zA-Z0-9_.-]+$/.test(value);
export function canonicalEvent(value: unknown, workspaceId: string): CanonicalEvent {
  if (!value || typeof value !== 'object') throw new Error('Invalid realtime event.');
  const event = value as CanonicalEvent;
  if (event.schema_version !== 1 || event.workspace_id !== workspaceId || !uuid(event.event_id) || !uuid(event.entity_id)
    || !word(event.type) || !word(event.entity_type) || event.actor_id !== null
    || typeof event.entity_version !== 'string' || !/^(0|[1-9][0-9]{0,18})$/.test(event.entity_version) || BigInt(event.entity_version) > 9223372036854775807n
    || (event.mutation_id !== null && !uuid(event.mutation_id)) || typeof event.occurred_at !== 'string' || event.occurred_at.length > 64 || !Number.isFinite(Date.parse(event.occurred_at))
    || !Array.isArray(event.changed_fields) || event.changed_fields.length > 100 || !event.changed_fields.every(word)) throw new Error('Invalid realtime event.');
  return event;
}

/** At-least-once replay is bounded in memory; versions decide freshness, never time. */
export class EventDeduplicator {
  private ids = new Map<string, true>();
  private versions = new Map<string, bigint>();
  accept(event: CanonicalEvent): boolean {
    if (this.ids.has(event.event_id)) return false;
    this.ids.set(event.event_id, true);
    if (this.ids.size > 1024) this.ids.delete(this.ids.keys().next().value!);
    const key = `${event.entity_type}:${event.entity_id}`; const version = BigInt(event.entity_version);
    const previous = this.versions.get(key);
    if (previous !== undefined && previous >= version) return false;
    this.versions.delete(key); this.versions.set(key, version);
    if (this.versions.size > 1024) this.versions.delete(this.versions.keys().next().value!);
    return true;
  }
}
