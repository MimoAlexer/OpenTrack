import { useEffect, useLayoutEffect, useRef, useState, type CSSProperties } from 'react';
import { Bell, BellOff, ChevronDown } from 'lucide-react';
import { ApiError, request } from '../api';
import { useSessionScope } from '../desktop/context';
import type { Session } from '../types';
import type { IssueSubscription as Subscription, SubscriptionPreference } from '../mentions/types';
import '../mentions/mentions.css';
import { localPopupPosition, popupBounds, popupContentHeight, popupPosition } from '../mentions/placement';
const choices: { value: SubscriptionPreference; label: string; detail: string }[] = [
  { value: 'automatic', label: 'Automatic', detail: 'Follow issues you create, are assigned, or are mentioned in.' },
  { value: 'subscribed', label: 'Subscribed', detail: 'Keep this issue in your subscriptions.' },
  { value: 'unsubscribed', label: 'Unsubscribed', detail: 'Stay opted out, including after new assignments or mentions.' },
];
export function IssueSubscription({ workspaceId, issueId, session, refreshVersion = '' }: { workspaceId: string; issueId: string; session: Session; refreshVersion?: string }) {
  const scope = useSessionScope(session); const path = `/workspaces/${workspaceId}/issues/${issueId}/subscription`; const key = `${scope}:${path}`;
  const [state, setState] = useState<{ key: string; value: Subscription | null; loading: boolean; error: string }>({ key: '', value: null, loading: true, error: '' });
  const [open, setOpen] = useState(false); const [retry, setRetry] = useState(0); const [busy, setBusy] = useState(false); const [review, setReview] = useState(''); const [uncertain, setUncertain] = useState<SubscriptionPreference | null>(null);
  const menu = useRef<HTMLDivElement>(null); const [placement, setPlacement] = useState<CSSProperties>({ visibility: 'hidden' });
  const trigger = useRef<HTMLButtonElement>(null); const wrapper = useRef<HTMLDivElement>(null); const currentKey = useRef(key); currentKey.current = key; const operation = useRef<AbortController | null>(null);
  const value = state.key === key ? state.value : null;
  useLayoutEffect(() => {
    if (!open || !menu.current || !trigger.current) return;
    const measure = () => { if (!menu.current || !trigger.current) return; const anchor = trigger.current.getBoundingClientRect(); setPlacement({ position: 'absolute', right: 'auto', ...localPopupPosition(menu.current, popupPosition(anchor, popupBounds(menu.current), 300, Math.min(360, popupContentHeight(menu.current)))) }); };
    measure(); const observer = typeof ResizeObserver === 'undefined' ? null : new ResizeObserver(measure); observer?.observe(menu.current); window.addEventListener('resize', measure); window.addEventListener('scroll', measure, true);
    return () => { observer?.disconnect(); window.removeEventListener('resize', measure); window.removeEventListener('scroll', measure, true); };
  }, [open, value, review, uncertain, state.loading, state.error]);
  function checked(result: Subscription) { if (result.issue_id !== issueId || !['automatic', 'subscribed', 'unsubscribed'].includes(result.preference) || typeof result.etag !== 'string') throw new Error('Subscription response unavailable.'); return result; }
  useEffect(() => {
    setOpen(false); setBusy(false); setUncertain(null); setReview('');
    return () => operation.current?.abort();
  }, [key]);
  useEffect(() => {
    // A source refresh must never cancel a preference write or discard its
    // uncertain outcome. Refresh after that operation has been resolved.
    if (busy || uncertain) return;
    const abort = new AbortController(); setState(old => ({ key, value: old.key === key ? old.value : null, loading: true, error: '' }));
    request<Subscription>(path, { signal: abort.signal }).then(result => { if (!abort.signal.aborted) setState({ key, value: checked(result), loading: false, error: '' }); }).catch(error => {
      if (!abort.signal.aborted) setState(old => ({ key, value: error instanceof ApiError && [401,403,404].includes(error.status) ? null : old.key === key ? old.value : null, loading: false, error: 'Subscription unavailable.' }));
    });
    return () => abort.abort();
  }, [key, retry, refreshVersion, busy, uncertain]);
  useEffect(() => {
    if (!open) return;
    const outside = (event: PointerEvent) => { if (!wrapper.current?.contains(event.target as Node)) setOpen(false); };
    document.addEventListener('pointerdown', outside); return () => document.removeEventListener('pointerdown', outside);
  }, [open]);
  useEffect(() => { if (review && open) trigger.current?.focus({ preventScroll: true }); }, [review]);
  function close() { setOpen(false); trigger.current?.focus({ preventScroll: true }); }
  async function change(preference: SubscriptionPreference) {
    if (!value || !value.can_change || busy || state.loading || uncertain) return;
    const abort = new AbortController(); operation.current = abort; setBusy(true); setReview('');
    try {
      const result = checked(await request<Subscription>(path, { method: 'PATCH', body: { preference }, csrf: session.csrf_token, headers: { 'If-Match': value.etag }, signal: abort.signal }));
      if (abort.signal.aborted || currentKey.current !== key) return;
      setState({ key, value: result, loading: false, error: '' }); close();
    } catch (error) {
      if (abort.signal.aborted || currentKey.current !== key) return;
      if (error instanceof ApiError && error.status === 412) {
        const latest = error.details?.current as Subscription | undefined;
        try { const result = checked(latest ?? await request<Subscription>(path, { signal: abort.signal })); if (abort.signal.aborted || currentKey.current !== key) return; setState({ key, value: result, loading: false, error: '' }); setReview('This preference changed in another session. Review the current value, then choose again.'); }
        catch { if (!abort.signal.aborted && currentKey.current === key) setState({ key, value: null, loading: false, error: 'Could not verify the current subscription.' }); }
      } else if (error instanceof ApiError && [401,403,404].includes(error.status)) setState({ key, value: null, loading: false, error: 'Subscription unavailable with your current access.' });
      else if (!(error instanceof ApiError) || error.status === 0 || error.status === 408 || error.status >= 500) { setUncertain(preference); setReview('The change was not confirmed. Check its result before changing this preference again.'); }
      else setReview('The preference could not be saved. Review it and try again.');
    } finally { if (!abort.signal.aborted && currentKey.current === key) setBusy(false); }
  }
  async function reconcile() {
    if (busy || !uncertain) return; const intended = uncertain; const abort = new AbortController(); operation.current = abort; setBusy(true);
    try { const result = checked(await request<Subscription>(path, { signal: abort.signal })); if (abort.signal.aborted || currentKey.current !== key) return; setState({ key, value: result, loading: false, error: '' }); setUncertain(null); setReview(result.preference === intended ? 'Your preference was saved.' : 'The current preference is shown. Choose again if you still want to change it.'); }
    catch (error) { if (!abort.signal.aborted && currentKey.current === key) { if (error instanceof ApiError && [401,403,404].includes(error.status)) setState({ key, value: null, loading: false, error: 'Subscription unavailable with your current access.' }); else setReview('Could not check the result. Your pending choice is preserved; try again.'); } }
    finally { if (!abort.signal.aborted && currentKey.current === key) setBusy(false); }
  }
  return <div className="issue-subscription" ref={wrapper} onKeyDown={event => {
    if (open && event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); close(); }
    else if (open && ['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) {
      const items = [...(wrapper.current?.querySelectorAll<HTMLButtonElement>('[role=menuitemradio]:not(:disabled)') ?? [])]; if (!items.length) return;
      event.preventDefault(); event.stopPropagation(); const at = items.indexOf(document.activeElement as HTMLButtonElement); const next = event.key === 'Home' ? 0 : event.key === 'End' ? items.length - 1 : at < 0 ? event.key === 'ArrowUp' ? items.length - 1 : 0 : (at + (event.key === 'ArrowUp' ? items.length - 1 : 1)) % items.length; items[next].focus();
    }
  }}>
    <button ref={trigger} type="button" className="quiet subscription-trigger" aria-label="Issue subscription" aria-haspopup="menu" aria-expanded={open} aria-busy={busy || state.loading} onClick={() => setOpen(value => !value)}>{!value || value.subscribed ? <Bell aria-hidden="true"/> : <BellOff aria-hidden="true"/>}<span>{value ? value.subscribed ? 'Subscribed' : 'Subscribe' : 'Subscription'}</span><ChevronDown aria-hidden="true"/></button>
    {open && <div ref={menu} style={placement} className="subscription-menu"><strong>Issue subscription</strong>{(!value && (state.loading || state.key !== key)) && <p role="status">Checking subscription…</p>}{state.error && <p role="alert">{state.error} <button type="button" onClick={() => setRetry(value => value + 1)}>Try again</button></p>}{value && <><div role="menu" aria-label="Subscription preference">{choices.map(choice => <button key={choice.value} type="button" role="menuitemradio" aria-checked={value.preference === choice.value} disabled={busy || state.loading || !!state.error || !!uncertain || !value.can_change} onClick={() => { void change(choice.value); }}><span>{choice.label}{value.preference === choice.value && <span aria-hidden="true"> ✓</span>}</span><small>{choice.detail}</small></button>)}</div>{value.preference === 'automatic' && <p>{value.subscribed ? `Following automatically: ${value.sources.map(source => source === 'creator' ? 'you created this issue' : source === 'assignment' ? 'assignment' : 'mention').join(', ')}.` : 'No automatic subscription applies.'}</p>}{!value.can_change && <p>This subscription is currently read-only.</p>}</>}{review && <p role="status">{review}</p>}{uncertain && <button type="button" className="button compact" disabled={busy} onClick={() => { void reconcile(); }}>Check result</button>}</div>}
  </div>;
}
