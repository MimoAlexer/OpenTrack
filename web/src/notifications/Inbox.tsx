import { useEffect, useRef, useState, type KeyboardEvent } from 'react';
import { Archive, ArchiveRestore, ArrowLeft, ArrowDown, ArrowUp, Bell, CheckCheck, Clock, Mail, MailOpen, Settings2 } from 'lucide-react';
import { ApiError } from '../api';
import { ErrorBanner, Loading } from '../components';
import { useSessionBoundary } from '../desktop/context';
import { inboxUrl, navigate } from '../router';
import type { Session } from '../types';
import { NotificationPreferencesPanel } from './Preferences';
import { SourcePreview } from './SourcePreview';
import { defaultSnooze, filters, localSnoozeInstant, reasons, visibleInFilter, type InboxFilter, type NotificationAction, type NotificationItem } from './types';
import { useInbox } from './useInbox';
import { isAborted } from './scope';
import './notifications.css';

interface Props { workspaceId: string; session: Session; accessScope: string; params: URLSearchParams; notificationId?: string; refresh: number; charactersEnabled?: boolean; externallyRefreshed?: boolean }
type Pending = { item: NotificationItem; action: NotificationAction; until?: string } | { snapshot: string };
function useNarrowInbox() {
  const [narrow, setNarrow] = useState(() => window.matchMedia?.('(max-width: 767px)').matches ?? false);
  useEffect(() => {
    const media = window.matchMedia?.('(max-width: 767px)'); if (!media) return;
    const changed = () => setNarrow(media.matches); changed(); media.addEventListener('change', changed);
    return () => media.removeEventListener('change', changed);
  }, []);
  return narrow;
}
export function Inbox(props: Props) {
  const boundary = useSessionBoundary(props.session);
  return <InboxScreen key={`${boundary}:${props.workspaceId}:${props.accessScope}`} {...props}/>;
}
function InboxScreen({ workspaceId, session, accessScope, params, notificationId, refresh, charactersEnabled = true, externallyRefreshed = false }: Props) {
  const narrow = useNarrowInbox();
  const requestedFilter = params.get('filter'); const filter: InboxFilter = requestedFilter && Object.hasOwn(filters, requestedFilter) ? requestedFilter as InboxFilter : 'unread';
  const inbox = useInbox(workspaceId, session, accessScope, filter, refresh, externallyRefreshed);
  const items = inbox.value?.items ?? []; const itemsRef = useRef(items); itemsRef.current = items;
  const [selection, setSelection] = useState<string | null | undefined>(notificationId); const [mobilePreview, setMobilePreview] = useState(!!notificationId);
  const [busy, setBusy] = useState(false); const [actionError, setActionError] = useState<unknown>(null); const [pending, setPending] = useState<Pending | null>(null);
  const [notice, setNotice] = useState(''); const [preferences, setPreferences] = useState(false); const [snoozing, setSnoozing] = useState(false); const [snooze, setSnooze] = useState(defaultSnooze);
  const list = useRef<HTMLDivElement>(null); const heading = useRef<HTMLHeadingElement>(null); const focusedAfterRender = useRef<string | null | undefined>(undefined);
  const detail = useRef<HTMLDivElement>(null); const back = useRef<HTMLButtonElement>(null);
  const inboxOwnedFocus = useRef(false);
  const selectionRef = useRef(selection); selectionRef.current = selection; const selectionRevision = useRef(0);
  const selected = selection === undefined ? items[0] : items.find(item => item.id === selection);
  const selectedId = selected?.id;
  useEffect(() => { if (notificationId) { setSelection(notificationId); setMobilePreview(true); } }, [notificationId]);
  useEffect(() => { setSelection(notificationId); setMobilePreview(!!notificationId); setActionError(null); setPending(null); setSnoozing(false); setBusy(false); }, [filter]);
  useEffect(() => {
    if (selection === undefined && items[0]) { selectionRef.current = items[0].id; setSelection(items[0].id); return; }
    if (!inbox.value || !selection || items.some(item => item.id === selection)) return;
    const next = null;
    setSelection(next); setSnoozing(false);
    if (inboxOwnedFocus.current && (document.activeElement === document.body || list.current?.contains(document.activeElement))) focusedAfterRender.current = next;
  }, [items, selection, inbox.value]);
  useEffect(() => {
    const previewVisible = narrow && mobilePreview && !!selected;
    const active = document.activeElement;
    const hiddenPaneFocused = previewVisible ? list.current?.contains(active) : narrow ? detail.current?.contains(active) : active === back.current;
    if (focusedAfterRender.current === undefined && !hiddenPaneFocused) return;
    const target = focusedAfterRender.current === undefined ? selectedId : focusedAfterRender.current; focusedAfterRender.current = undefined;
    if (previewVisible) { back.current?.focus(); return; }
    const row = [...(list.current?.querySelectorAll<HTMLButtonElement>('[data-notification-id]') ?? [])].find(element => element.dataset.notificationId === target);
    (row ?? heading.current)?.focus();
  }, [items, selection, selectedId, mobilePreview, narrow]);
  function select(id: string, focus = false) { if (selectionRef.current !== id) selectionRevision.current += 1; selectionRef.current = id; setSelection(id); setSnoozing(false); if (focus) focusedAfterRender.current = id; }
  function openPreview(id: string) { select(id); setMobilePreview(true); if (narrow) focusedAfterRender.current = id; }
  function closePreview() { setMobilePreview(false); focusedAfterRender.current = selectedId ?? null; }
  function move(direction: number) {
    const index = selectedId ? items.findIndex(item => item.id === selectedId) : direction > 0 ? -1 : items.length; const next = items[Math.min(items.length - 1, Math.max(0, index + direction))];
    if (next) select(next.id, true);
  }
  function keyboard(event: KeyboardEvent) {
    if (busy || event.defaultPrevented || event.nativeEvent.isComposing || event.altKey || event.ctrlKey || event.metaKey || (event.target as Element).closest('input, textarea, select, [contenteditable="true"]')) return;
    if (event.key === 'ArrowDown' || charactersEnabled && event.key === 'j') { event.preventDefault(); event.stopPropagation(); move(1); }
    else if (event.key === 'ArrowUp' || charactersEnabled && event.key === 'k') { event.preventDefault(); event.stopPropagation(); move(-1); }
    else if (charactersEnabled && event.key === 'e' && selected) { event.preventDefault(); event.stopPropagation(); void perform({ item: selected, action: selected.archived_at ? 'unarchive' : 'archive' }); }
    else if (charactersEnabled && event.key === 'r' && selected) { event.preventDefault(); event.stopPropagation(); void perform({ item: selected, action: selected.read_at ? 'unread' : 'read' }); }
  }
  async function perform(operation: Pending) {
    if (busy) return;
    setBusy(true); setActionError(null); setPending(operation); setNotice(''); setSnoozing(false);
    const readAtStart = inbox.reconciliationToken(); const selectionAtStart = selectionRevision.current;
    const before = itemsRef.current; const previousSelection = selectedId ?? null;
    if ('item' in operation) {
      const stamp = new Date().toISOString(); const item = operation.item;
      const optimistic = { ...item, ...(operation.action === 'archive' ? { archived_at: stamp } : operation.action === 'unarchive' ? { archived_at: null } : operation.action === 'read' ? { read_at: stamp } : operation.action === 'unread' ? { read_at: null } : operation.action === 'snooze' ? { snoozed_until: operation.until! } : { snoozed_until: null }) };
      if (!visibleInFilter(optimistic, filter)) {
        const index = before.findIndex(old => old.id === item.id); const next = before[index + 1] ?? before[index - 1];
        inbox.replaceItems(items => items.filter(old => old.id !== item.id)); setSelection(next?.id ?? null); focusedAfterRender.current = next?.id ?? null;
      } else inbox.replaceItems(items => items.map(old => old.id === item.id ? optimistic : old));
    }
    try {
      if ('snapshot' in operation) { await inbox.markRead(operation.snapshot); if (inbox.current()) setNotice('Earlier notifications marked read. New arrivals stay unread.'); }
      else await inbox.mutate(operation.item, operation.action, operation.until);
      if (!inbox.current()) return;
      setPending(null);
    } catch (error) {
      if (!inbox.current() || isAborted(error)) return;
      if ('item' in operation) {
        inbox.rollback(operation.item, readAtStart);
        if (selectionRevision.current === selectionAtStart) { setSelection(previousSelection); focusedAfterRender.current = previousSelection; }
      }
      setActionError(error);
    } finally { if (inbox.current()) setBusy(false); }
  }
  const conflict = actionError instanceof ApiError && [410, 412].includes(actionError.status);
  const until = localSnoozeInstant(snooze);
  return <section className={`inbox-screen${mobilePreview && selected ? ' inbox-preview-open' : ''}`} aria-label="Inbox" onKeyDown={keyboard} onFocusCapture={event => { inboxOwnedFocus.current = !!(event.target as Element).closest('.inbox-list, .inbox-detail'); }}>
    <div className="inbox-toolbar"><nav aria-label="Inbox filters">{(Object.keys(filters) as InboxFilter[]).map(value => <button type="button" key={value} aria-current={filter === value ? 'page' : undefined} disabled={busy} onClick={() => { navigate(inboxUrl(workspaceId, new URLSearchParams(value === 'unread' ? {} : { filter: value }))); }}>{filters[value]}</button>)}</nav><div className="button-row"><button className="button compact" disabled={busy || !inbox.value?.snapshot || !inbox.value.unread_count} onClick={() => { if (inbox.value) void perform({ snapshot: inbox.value.snapshot }); }}><CheckCheck aria-hidden="true"/>{busy && pending && 'snapshot' in pending ? 'Marking read…' : 'Mark all read'}</button><button type="button" className="icon-button" aria-label="Inbox preferences" aria-expanded={preferences} onClick={() => setPreferences(value => !value)}><Settings2 aria-hidden="true"/></button></div></div>
    {preferences && <NotificationPreferencesPanel workspaceId={workspaceId} session={session} accessScope={accessScope} close={() => setPreferences(false)}/>}
    <div className="inbox-feedback" aria-live="polite">{notice && <p role="status">{notice}</p>}<ErrorBanner error={actionError}/>{!!actionError && pending && <div className="button-row"><button className="button compact" disabled={busy} onClick={() => { if (conflict) { setActionError(null); setPending(null); inbox.reload(); } else void perform(pending); }}>{conflict ? 'Refresh inbox' : 'Retry action'}</button><span>{conflict ? 'This notification changed. Refresh before trying another action.' : 'Your place in the inbox has been restored.'}</span></div>}</div>
    <div className="inbox-panes"><div className="inbox-list" ref={list} aria-busy={inbox.loading}><h2 ref={heading} tabIndex={-1} className="sr-only">{filters[filter]} notifications</h2><ErrorBanner error={inbox.error} retry={inbox.reload}/>
      {!inbox.value && inbox.loading ? <Loading label="Loading inbox…"/> : items.length ? <><ul aria-label="Notifications">{items.map(item => <li key={item.id}><button type="button" className={`inbox-row${!item.read_at ? ' inbox-unread' : ''}`} data-notification-id={item.id} aria-pressed={selectedId === item.id} tabIndex={selectedId === item.id ? 0 : -1} onFocus={() => select(item.id)} onClick={() => openPreview(item.id)}><span className="inbox-reason">{item.source.available && item.source.actor_display_name ? `${item.source.actor_display_name} · ` : ''}{reasons[item.reason]}</span><strong>{item.source.available ? `${item.source.key ? `${item.source.key} · ` : ''}${item.source.title ?? 'Update'}` : 'Source unavailable'}</strong><span className="inbox-row-meta"><time dateTime={item.created_at} title={new Date(item.created_at).toLocaleString()}>{new Date(item.created_at).toLocaleDateString(undefined, { month: 'short', day: 'numeric' })}</time>{item.snoozed_until && <time dateTime={item.snoozed_until} title={new Date(item.snoozed_until).toUTCString()}><Clock aria-hidden="true"/>{new Date(item.snoozed_until).toLocaleString()}</time>}{!item.read_at && <span className="inbox-unread-dot" aria-label="Unread"/>}</span></button></li>)}</ul>{inbox.value?.next_cursor && <button className="button compact inbox-more" disabled={busy || inbox.loading} onClick={inbox.more}>{inbox.loading ? 'Loading…' : 'Load more notifications'}</button>}</> : !inbox.error && <div className="inbox-empty"><Bell aria-hidden="true"/><h3>{filter === 'all' ? 'No activity yet' : filter === 'unread' ? 'You’re all caught up' : `No ${filter} notifications`}</h3><p>{filter === 'all' ? 'Assignments, mentions and comments on followed issues will appear here.' : filter === 'unread' ? 'New updates will appear here. Read notifications are in All.' : `Updates you ${filter === 'archived' ? 'archive' : 'snooze'} will appear here.`}</p>{filter !== 'all' && <button className="button compact" onClick={() => navigate(inboxUrl(workspaceId, new URLSearchParams({ filter: 'all' })))}>View all notifications</button>}</div>}
    </div><div className="inbox-detail" ref={detail} aria-label="Notification context">{selected ? <><div className="inbox-actions"><button className="icon-button inbox-mobile-back" ref={back} aria-label="Back to inbox" onClick={closePreview}><ArrowLeft aria-hidden="true"/></button><div className="button-row"><button className="icon-button" aria-label="Previous notification" disabled={busy || items[0]?.id === selectedId} onClick={() => move(-1)}><ArrowUp aria-hidden="true"/></button><button className="icon-button" aria-label="Next notification" disabled={busy || items.at(-1)?.id === selectedId} onClick={() => move(1)}><ArrowDown aria-hidden="true"/></button></div><div className="button-row"><button className="button compact" disabled={busy} onClick={() => void perform({ item: selected, action: selected.read_at ? 'unread' : 'read' })}>{selected.read_at ? <Mail aria-hidden="true"/> : <MailOpen aria-hidden="true"/>}{selected.read_at ? 'Mark unread' : 'Mark read'}</button><button className="button compact" disabled={busy} onClick={() => void perform({ item: selected, action: selected.archived_at ? 'unarchive' : 'archive' })}>{selected.archived_at ? <ArchiveRestore aria-hidden="true"/> : <Archive aria-hidden="true"/>}{selected.archived_at ? 'Unarchive' : 'Archive'}</button>{selected.snoozed_until ? <button className="button compact" disabled={busy} onClick={() => void perform({ item: selected, action: 'unsnooze' })}><Clock aria-hidden="true"/>Unsnooze</button> : <button className="button compact" disabled={busy} aria-expanded={snoozing} onClick={() => { setSnooze(defaultSnooze()); setSnoozing(value => !value); }}><Clock aria-hidden="true"/>Snooze</button>}</div></div>
      {snoozing && <form className="inbox-snooze" onSubmit={event => { event.preventDefault(); if (until) void perform({ item: selected, action: 'snooze', until }); }}><label className="field">Snooze until<input type="datetime-local" aria-label="Snooze until" required value={snooze} onChange={event => setSnooze(event.target.value)}/></label><p>{until ? <time dateTime={until} title={new Date(until).toUTCString()}>{new Date(until).toLocaleString()} · {Intl.DateTimeFormat().resolvedOptions().timeZone}</time> : 'Choose a valid future time within 90 days in your timezone.'}</p><div className="button-row"><button className="button compact primary" disabled={!until || busy}>Confirm snooze</button><button className="button compact" type="button" onClick={() => setSnoozing(false)}>Cancel</button></div></form>}
      <SourcePreview key={selected.id} workspaceId={workspaceId} session={session} accessScope={accessScope} item={selected} refresh={refresh} unavailable={inbox.unavailable}/>
    </> : <div className="inbox-empty"><Bell aria-hidden="true"/><h3>{selection && items.length ? 'Notification unavailable' : 'Choose an update'}</h3><p>{selection && items.length ? 'This update may be outside the current filter or no longer available.' : 'Read its source and process the notification here.'}</p></div>}</div></div>
    <p className="inbox-keyboard-hint">Arrow keys: previous / next{charactersEnabled ? ' · J / K: next / previous · E: archive · R: read / unread' : ''}</p>
  </section>;
}
