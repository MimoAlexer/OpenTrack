import { useEffect, useState, type FormEvent } from 'react';
import { ApiError } from '../api';
import { ErrorBanner, Loading } from '../components';
import type { Session } from '../types';
import { isAborted, useNotificationScope } from './scope';
import type { NotificationPreferences } from './types';

const labels = { assignments: 'Assignments', mentions: 'Mentions', subscribed_comments: 'Comments on followed issues' } as const;
export function NotificationPreferencesPanel({ workspaceId, session, accessScope, close }: { workspaceId: string; session: Session; accessScope: string; close: () => void }) {
  const scope = useNotificationScope(workspaceId, session, accessScope); const path = `/workspaces/${encodeURIComponent(workspaceId)}/notification-preferences`;
  const [base, setBase] = useState<NotificationPreferences | null>(null); const [draft, setDraft] = useState<NotificationPreferences | null>(null);
  const [busy, setBusy] = useState(false); const [error, setError] = useState<unknown>(null); const [attempt, setAttempt] = useState(0); const [saved, setSaved] = useState(false);
  useEffect(() => {
    const abort = new AbortController(); setBase(null); setDraft(null); setError(null);
    scope.read<NotificationPreferences>(path, { signal: abort.signal }).then(value => { setBase(value); setDraft(value); }).catch(error => { if (!abort.signal.aborted && scope.current() && !isAborted(error)) setError(error); });
    return () => abort.abort();
  }, [path, scope.read, scope.current, attempt]);
  async function submit(event: FormEvent) {
    event.preventDefault(); if (busy || !base || !draft) return; setBusy(true); setError(null); setSaved(false);
    try {
      const value = await scope.read<NotificationPreferences>(path, { method: 'PATCH', body: { assignments: draft.assignments, mentions: draft.mentions, subscribed_comments: draft.subscribed_comments }, csrf: session.csrf_token, headers: { 'If-Match': base.etag } });
      setBase(value); setDraft(value); setSaved(true);
    } catch (error) { if (scope.current() && !isAborted(error)) setError(error); }
    finally { if (scope.current()) setBusy(false); }
  }
  const conflict = error instanceof ApiError && error.status === 412;
  return <section className="inbox-preferences" aria-label="Inbox preferences"><header><div><h2>Inbox preferences</h2><p>Choose the updates you receive in this workspace.</p></div><button className="button compact" type="button" disabled={busy} onClick={close}>Close preferences</button></header><ErrorBanner error={error} retry={!base ? () => setAttempt(value => value + 1) : undefined}/>{conflict && <p className="notice">Your preferences changed elsewhere. <button className="button compact" onClick={() => setAttempt(value => value + 1)}>Reload preferences</button></p>}{draft ? <form onSubmit={submit}><fieldset disabled={busy || conflict}>{(Object.keys(labels) as (keyof typeof labels)[]).map(key => <label key={key}><input type="checkbox" checked={draft[key]} onChange={event => { setDraft({ ...draft, [key]: event.target.checked }); setSaved(false); }}/>{labels[key]}</label>)}</fieldset><button className="button compact" disabled={busy || conflict}>{busy ? 'Saving…' : 'Save preferences'}</button>{saved && <span role="status">Preferences saved.</span>}</form> : !error && <Loading label="Loading preferences…"/>}</section>;
}
