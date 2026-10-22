import { useDesktop } from '../desktop/context';
import { NativeReauthenticate } from '../desktop/NativeReauthenticate';
import { useEffect, useRef, useState, type FormEvent, type ReactNode } from 'react';
import { ApiError, request } from '../api';
import { ErrorBanner, Modal } from '../components';
import type { Session } from '../types';
import type { Page } from './types';

export function useScopedRequest() {
  const scope = useRef(new AbortController());
  useEffect(() => { if (scope.current.signal.aborted) scope.current = new AbortController(); return () => scope.current.abort(); }, []);
  return async function scopedRequest<T>(path: string, options: Parameters<typeof request>[1] = {}): Promise<T> {
    const signal = options.signal ? AbortSignal.any([scope.current.signal, options.signal]) : scope.current.signal;
    const result = await request<T>(path, { ...options, signal });
    if (signal.aborted) throw new DOMException('The view was closed.', 'AbortError');
    return result;
  };
}
export function tag(kind: string, ...values: string[]) { return `"${kind}:${values.join(':')}"`; }
export function useDirectory<T>(path: string) {
  const [items, setItems] = useState<T[]>([]); const [itemsPath, setItemsPath] = useState(path); const [page, setPage] = useState<{ path: string; cursor: string | null }>({ path, cursor: null });
  const cursor = page.path === path ? page.cursor : null;
  const [next, setNext] = useState<string | null>(null); const [loading, setLoading] = useState(true);
  const [error, setError] = useState<unknown>(null); const [revision, setRevision] = useState(0);
  useEffect(() => {
    const abort = new AbortController(); let active = true;
    setLoading(true); setError(null); setItemsPath(path); if (!cursor) { setItems([]); setNext(null); }
    const url = `${path}${path.includes('?') ? '&' : '?'}limit=50${cursor ? `&cursor=${encodeURIComponent(cursor)}` : ''}`;
    request<Page<T>>(url, { signal: abort.signal }).then(page => {
      if (active) { setItems(old => cursor ? [...old, ...page.data] : page.data); setNext(page.has_more ? page.next_cursor : null); }
    }).catch(error => { if (active && !abort.signal.aborted) setError(error); }).finally(() => { if (active) setLoading(false); });
    return () => { active = false; abort.abort(); };
  }, [path, cursor, revision]);
  return { items: itemsPath === path ? items : [], loading: itemsPath !== path || loading, error: itemsPath === path ? error : null, hasMore: itemsPath === path && !!next, more: () => { if (loading || !next) return; setLoading(true); setPage({ path, cursor: next }); setRevision(value => value + 1); }, reload: () => { setLoading(true); setItems([]); setNext(null); setPage({ path, cursor: null }); setRevision(value => value + 1); } };
}
export function DirectoryFooter({ directory }: { directory: { hasMore: boolean; loading: boolean; more: () => void } }) {
  return directory.hasMore ? <div className="load-more"><button type="button" className="button" disabled={directory.loading} onClick={directory.more}>{directory.loading ? 'Loading…' : 'Load more'}</button></div> : null;
}
export function SettingsHeading({ title, description, children }: { title: string; description: string; children?: ReactNode }) {
  return <header className="settings-heading"><div><h2>{title}</h2><p>{description}</p></div>{children}</header>;
}
export function SettingsError({ error, reload, retry, reauthenticate }: { error: unknown; reload?: () => void; retry?: () => void; reauthenticate: () => void }) {
  const desktop = useDesktop();
  const [confirmedError, setConfirmedError] = useState<unknown>(null);
  const errorRef = useRef(error); errorRef.current = error;
  useEffect(() => {
    const confirmed = () => { if (errorRef.current instanceof ApiError && errorRef.current.code === 'reauthentication_required') setConfirmedError(errorRef.current); };
    window.addEventListener('opentrack:reauthenticated', confirmed);
    return () => window.removeEventListener('opentrack:reauthenticated', confirmed);
  }, []);
  if (error && error === confirmedError) return <div className="notice" role="status">{desktop ? 'Identity confirmed.' : 'Password confirmed.'} Retry your action.</div>;
  return error ? <><ErrorBanner error={error} retry={error instanceof ApiError && error.status === 412 ? undefined : retry} />{error instanceof ApiError && error.code === 'reauthentication_required' && <div className="button-row"><button type="button" className="button" onClick={reauthenticate}>{desktop ? 'Confirm your identity' : 'Confirm your password'}</button><span className="muted">Then retry this action.</span></div>}{error instanceof ApiError && error.status === 412 && reload && <div className="notice">This record changed. Your proposed changes have not been applied. <button type="button" className="button compact" onClick={() => { if (window.confirm('Reload the latest record and discard pending edits?')) reload(); }}>Reload latest</button></div>}</> : null;
}
export function Reauthenticate({ session, close, authenticated }: { session: Session; close: () => void; authenticated: (session: Session) => void }) {
  const desktop = useDesktop();
  return desktop ? <NativeReauthenticate session={session} close={close} authenticated={authenticated} /> : <BrowserReauthenticate session={session} close={close} authenticated={authenticated} />;
}

function BrowserReauthenticate({ session, close, authenticated }: { session: Session; close: () => void; authenticated: (session: Session) => void }) {
  const request = useScopedRequest();
  const [busy, setBusy] = useState(false); const [error, setError] = useState<unknown>(null);
  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault(); if (busy) return;
    const password = new FormData(event.currentTarget).get('password'); setBusy(true); setError(null);
    try {
      authenticated(await request<Session>('/auth/login', { method: 'POST', body: { email: session.user.email, password } }));
      window.dispatchEvent(new CustomEvent('opentrack:reauthenticated'));
      close();
    } catch (error) {
      setError(error instanceof ApiError && error.status === 401 ? new ApiError(error.status, error.code, 'That password was not accepted. Try again.', error.requestId, error.details) : error);
    } finally { setBusy(false); }
  }
  return <Modal title="Confirm it’s you" close={close} canClose={!busy}><form className="settings-modal-content" onSubmit={submit}><p>Confirm your password to continue. You will stay signed in as {session.user.email}.</p><ErrorBanner error={error} /><label className="field">Password<input data-autofocus name="password" type="password" required autoComplete="current-password" disabled={busy} /></label><div className="modal-footer"><button type="button" className="button" disabled={busy} onClick={close}>Cancel</button><button className="button primary" disabled={busy}>{busy ? 'Confirming…' : 'Confirm password'}</button></div></form></Modal>;
}
export function useCreateRequest<T>(path: string, csrf: string) {
  const request = useScopedRequest();
  const pending = useRef<{ body: unknown; key: string } | null>(null);
  const [busy, setBusy] = useState(false); const [uncertain, setUncertain] = useState(false); const [error, setError] = useState<unknown>(null);
  async function submit(body: unknown): Promise<T | undefined> {
    if (busy) return;
    if (!uncertain || !pending.current) pending.current = { body, key: crypto.randomUUID() };
    setBusy(true); setError(null);
    try {
      const result = await request<T>(path, { method: 'POST', body: pending.current.body, csrf, headers: { 'Idempotency-Key': pending.current.key } });
      setUncertain(false); return result;
    } catch (error) { setError(error); setUncertain(error instanceof ApiError && (error.status === 0 || error.status === 408 || error.status >= 500)); }
    finally { setBusy(false); }
  }
  return { submit, busy, uncertain, error };
}
export function UncertainNotice({ show }: { show: boolean }) { return show ? <div className="notice" role="status">The response was interrupted. Retry with the same request before making another change; the retry will not create a duplicate.</div> : null; }
