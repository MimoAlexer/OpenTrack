import { ArrowRight, LockKeyhole, LoaderCircle } from 'lucide-react';
import { useEffect, useState, type FormEvent } from 'react';
import { useScopedRequest } from '../settings/shared';
import { Brand, ErrorBanner } from '../components';
import { Link } from '../router';
import type { Session } from '../types';

export function AuthScreen({ setup, onAuthenticated, notice }: { setup: boolean; onAuthenticated: (session: Session) => void; notice?: string }) {
  useEffect(() => { document.title = setup ? 'Set up · OpenTrack' : 'Sign in · OpenTrack'; }, [setup]);
  const request = useScopedRequest();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<unknown>(null);
  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault(); if (busy) return;
    const data = Object.fromEntries(new FormData(event.currentTarget));
    setBusy(true); setError(null);
    try {
      const result = await request<Session>(setup ? '/setup' : '/auth/login', { method: 'POST', body: data });
      onAuthenticated(result);
    } catch (error) { setError(error); } finally { setBusy(false); }
  }
  return <main className="auth-page">
    <header className="auth-brand"><Brand /><span>YOUR WORKSPACE</span></header>
    <section className={`auth-panel${setup ? ' setup-panel' : ''}`}>
      <div className="eyebrow">{setup ? 'FIRST-TIME SETUP' : 'WELCOME BACK'}</div>
      <h1>{setup ? 'Make room for your work.' : 'Sign in to OpenTrack.'}</h1>
      <p className="auth-intro">{setup ? 'Create the owner account and your first team. Everything stays on this installation.' : 'Pick up where you left off.'}</p>
      {notice && <div className="notice" role="status">{notice}</div>}
      <ErrorBanner error={error} />
      <form onSubmit={submit} aria-busy={busy}>
        <fieldset disabled={busy}>
          {setup && <label className="field">Installation token<input name="setup_token" type="password" autoComplete="off" required autoFocus aria-describedby="token-help" /><small id="token-help">Use the setup token provided by the person running this installation.</small></label>}
          {setup && <label className="field">Your name<input name="display_name" autoComplete="name" required maxLength={100} /></label>}
          <label className="field">Email<input name="email" type="email" autoComplete="username" autoFocus={!setup} required maxLength={254} /></label>
          <label className="field">Password<input name="password" type="password" autoComplete={setup ? 'new-password' : 'current-password'} required minLength={setup ? 12 : undefined} maxLength={1024} aria-describedby={setup ? 'password-help' : undefined} />{setup && <small id="password-help">At least 12 characters. A longer passphrase works well.</small>}</label>
          {setup && <>
            <div className="form-divider">Your workspace</div>
            <label className="field">Workspace name<input name="workspace_name" placeholder="e.g. Northstar" required maxLength={100} /></label>
            <div className="form-grid"><label className="field">First team<input name="team_name" placeholder="e.g. Engineering" required maxLength={100} /></label><label className="field">Issue prefix<input name="team_key" placeholder="ENG" pattern="[A-Za-z][A-Za-z0-9]{1,9}" title="2–10 letters or digits, starting with a letter" required maxLength={10} autoCapitalize="characters" className="uppercase" /><small>Appears in keys like ENG-1.</small></label></div>
          </>}
          <button className="button primary auth-submit" type="submit">{busy ? <LoaderCircle className="spinner" aria-hidden="true" /> : <ArrowRight aria-hidden="true" />}{busy ? (setup ? 'Creating workspace…' : 'Signing in…') : (setup ? 'Create workspace' : 'Sign in')}</button>
        </fieldset>
      </form>
      {!setup && <p className="auth-secondary"><Link href="/recover">Reset your password</Link></p>}
      {!setup && <p className="auth-secondary">Have an invitation? <Link href="/join">Join a workspace</Link></p>}
      <p className="auth-footnote"><LockKeyhole aria-hidden="true" />{setup ? 'You’ll be the workspace owner.' : 'Your account belongs to this installation.'}</p>
    </section>
    <footer className="auth-footer">OpenTrack <span>Work, in your hands.</span></footer>
  </main>;
}
