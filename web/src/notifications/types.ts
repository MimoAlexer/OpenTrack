import type { components } from '../generated/api';

export type InboxFilter = components['schemas']['NotificationFilter'];
export type NotificationAction = components['schemas']['NotificationAction'];
export type NotificationItem = components['schemas']['NotificationItem'];
export type NotificationPage = components['schemas']['NotificationPage'];
export type NotificationPreferences = components['schemas']['NotificationPreferences'];
export const filters: Record<InboxFilter, string> = { unread: 'Unread', all: 'All', archived: 'Archived', snoozed: 'Snoozed' };
export const reasons: Record<NotificationItem['reason'], string> = { assignment: 'Assigned to you', mention: 'Mentioned you', subscribed_comment: 'New comment' };
export const notificationPath = (workspace: string, id?: string) => `/workspaces/${encodeURIComponent(workspace)}/notifications${id ? `/${encodeURIComponent(id)}` : ''}`;

export function safeSource(item: NotificationItem): NotificationItem {
  return { ...item, source: { kind: item.source.kind, id: item.source.id, available: false } };
}

export function visibleInFilter(item: NotificationItem, filter: InboxFilter, now = Date.now()): boolean {
  if (filter === 'archived') return !!item.archived_at;
  if (filter === 'snoozed') return !item.archived_at && !!item.snoozed_until && Date.parse(item.snoozed_until) > now;
  if (item.archived_at || item.snoozed_until && Date.parse(item.snoozed_until) > now) return false;
  return filter === 'all' || !item.read_at;
}

// datetime-local intentionally uses the viewer's timezone. Reject DST gaps rather
// than silently moving a requested local time to another wall-clock instant.
export function localSnoozeInstant(value: string, now = Date.now()): string | null {
  if (!/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}$/.test(value)) return null;
  const date = new Date(value); const [day, time] = value.split('T');
  const [year, month, dateOfMonth] = day.split('-').map(Number); const [hour, minute] = time.split(':').map(Number);
  if (!Number.isFinite(date.getTime()) || date.getTime() <= now || date.getTime() > now + 90 * 24 * 60 * 60 * 1000 || date.getFullYear() !== year || date.getMonth() !== month - 1 || date.getDate() !== dateOfMonth || date.getHours() !== hour || date.getMinutes() !== minute) return null;
  return date.toISOString();
}
export function defaultSnooze(now = Date.now()): string {
  const date = new Date(now + 60 * 60 * 1000);
  const pad = (value: number) => String(value).padStart(2, '0');
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}T${pad(date.getHours())}:${pad(date.getMinutes())}`;
}
