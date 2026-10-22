import { useEffect, useId, useRef, useState, type FormEvent } from 'react';
import { ArrowLeft, ArrowRight } from 'lucide-react';
import { ApiError } from '../api';
import { Brand, ErrorBanner } from '../components';
import { formatDate } from '../issue-model';
import { issuesUrl, Link, navigate } from '../router';
import type { Session } from '../types';
import type { InvitationPreview } from './types';
import { useScopedRequest } from './shared';
import './settings.css';

export function JoinWorkspace({ session, authenticated }: { session: Session | null; authenticated: (session: Session) => void }) {
  const request = useScopedRequest();
  const [token, setToken] = useState(''); const [preview, setPreview] = useState<InvitationPreview | null>(null);
  const [mode, setMode] = useState<'signin' | 'create'>('signin'); const [busy, setBusy] = useState(false); const [error, setError] = useState<unknown>(null);
  const heading = useRef<HTMLHeadingElement>(null); const tokenInput = useRef<HTMLTextAreaElement>(null); const passwordInput = useRef<HTMLInputElement>(null); const errorRef = useRef<HTMLDivElement>(null); const errorId = useId();
  const tokenRejected = !preview && error instanceof ApiError && [404, 409, 410, 422].includes(error.status);
  const passwordRejected = error instanceof ApiError && (error.status === 401 || error.code === 'invite_sign_in_required');
  useEffect(() => { (preview ? heading.current : tokenInput.current)?.focus(); }, [preview]);
  useEffect(() => { if (error && !busy) (tokenRejected ? tokenInput.current : passwordRejected ? passwordInput.current : errorRef.current)?.focus(); }, [error, busy, tokenRejected, passwordRejected]);
  const matching = !!session && session.user.email.toLowerCase() === preview?.email.toLowerCase();
  async function inspect(event: FormEvent) { event.preventDefault(); if (busy) return; setBusy(true); setError(null); try { setPreview(await request<InvitationPreview>('/invitations/preview', { method: 'POST', body: { token: token.trim() } })); } catch (error) { setError(error); } finally { setBusy(false); } }
  async function accept(event: FormEvent<HTMLFormElement>) {
    event.preventDefault(); if (busy || !preview) return; setBusy(true); setError(null);
    const values = Object.fromEntries(new FormData(event.currentTarget));
    try {
      let current = session;
      if (!matching && mode === 'signin') {
        current = await request<Session>('/auth/login', { method: 'POST', body: { email: preview.email, password: values.password } });
        authenticated(current);
      }
      const body = { token: token.trim(), ...(!matching && mode === 'create' ? { display_name: values.display_name, password: values.password } : {}) };
      const accepted = await request<Session>('/invitations/accept', { method: 'POST', body, csrf: current?.csrf_token });
      setToken(''); authenticated(accepted); navigate(issuesUrl(preview.workspace_id), true);
    } catch (error) { setError(error); if (error instanceof ApiError && error.code === 'invite_sign_in_required') setMode('signin'); }
    finally { setBusy(false); }
  }
  return <main className="auth-page"><header className="auth-brand"><Brand /><span>JOIN A WORKSPACE</span></header><section className="auth-panel join-panel"><Link className="auth-back" href={session?.workspaces[0] ? issuesUrl(session.workspaces[0].id) : '/'}><ArrowLeft aria-hidden="true" />{session ? 'Back to your workspace' : 'Back to sign in'}</Link><h1 ref={heading} tabIndex={-1}>{preview ? `Join ${preview.workspace_name}.` : 'Bring your invitation.'}</h1><p className="auth-intro">{preview ? `Invited as ${preview.email} · ${preview.role} · Expires ${formatDate(preview.expires_at)}` : 'Paste the single-use token shared with you by a workspace administrator.'}</p>{!!error && <div ref={errorRef} id={errorId} tabIndex={-1}><ErrorBanner error={error} /></div>}{!preview ? <form onSubmit={inspect} aria-busy={busy}><label className="field">Invitation token<textarea ref={tokenInput} data-autofocus autoFocus aria-invalid={tokenRejected || undefined} aria-describedby={error ? errorId : undefined} value={token} onChange={e => setToken(e.target.value)} required rows={3} autoComplete="off" spellCheck={false} disabled={busy} /></label><button className="button primary auth-submit" disabled={busy || !token.trim()}>{busy ? 'Checking…' : 'Review invitation'}<ArrowRight aria-hidden="true" /></button></form> : <form onSubmit={accept} aria-busy={busy}>{matching ? <div className="settings-info">You are signed in as <strong>{session.user.display_name}</strong> ({session.user.email}). Accepting adds this account to the workspace.</div> : <>{session && <div className="notice">You are signed in as {session.user.email}. Sign in to the invited account to accept this token.</div>}<div className="join-mode" role="group" aria-label="Account option"><button type="button" aria-pressed={mode === 'signin'} onClick={() => setMode('signin')}>I have an account</button><button type="button" aria-pressed={mode === 'create'} onClick={() => setMode('create')} disabled={!!session}>Create an account</button></div>{mode === 'create' && <label className="field">Your name<input name="display_name" required maxLength={100} autoComplete="name" disabled={busy} /></label>}<label className="field">{mode === 'signin' ? 'Account password' : 'Choose a password'}<input ref={passwordInput} aria-invalid={passwordRejected || undefined} aria-describedby={passwordRejected ? errorId : undefined} name="password" type="password" autoComplete={mode === 'signin' ? 'current-password' : 'new-password'} required minLength={mode === 'create' ? 12 : undefined} maxLength={1024} disabled={busy} />{mode === 'create' && <small>At least 12 characters. This creates an account for {preview.email}.</small>}</label></>}<button className="button primary auth-submit" disabled={busy}>{busy ? 'Joining…' : matching ? 'Accept invitation' : mode === 'signin' ? 'Sign in and join' : 'Create account and join'}<ArrowRight aria-hidden="true" /></button><button type="button" className="quiet" disabled={busy} onClick={() => { setPreview(null); setError(null); }}>Use a different token</button></form>}</section><footer className="auth-footer">OpenTrack<span>The token stays in this tab until you leave.</span></footer></main>;
}
