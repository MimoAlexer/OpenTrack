import { ArrowRight, CircleCheck, KeyRound, LoaderCircle } from 'lucide-react';
import { useEffect, useRef, useState, type FormEvent } from 'react';
import { ApiError } from '../api';
import { Brand, ErrorBanner } from '../components';
import { Link } from '../router';
import type { RecoveryComplete } from '../types';
import { useScopedRequest } from '../settings/shared';
import './recovery.css';

type InvalidField = 'token' | 'password' | 'confirmation' | null;

export function RecoveryScreen({ onCompleted }: { onCompleted: () => void }) {
  const request = useScopedRequest();
  const [token, setToken] = useState('');
  const [password, setPassword] = useState('');
  const [confirmation, setConfirmation] = useState('');
  const [busy, setBusy] = useState(false);
  const [uncertain, setUncertain] = useState(false);
  const [complete, setComplete] = useState(false);
  const [error, setError] = useState<unknown>(null);
  const [invalidField, setInvalidField] = useState<InvalidField>(null);
  const submitted = useRef<RecoveryComplete | null>(null);
  const inFlight = useRef(false);
  const tokenInput = useRef<HTMLInputElement>(null);
  const passwordInput = useRef<HTMLInputElement>(null);
  const confirmationInput = useRef<HTMLInputElement>(null);
  const successHeading = useRef<HTMLHeadingElement>(null);

  useEffect(() => { document.title = 'Reset password · OpenTrack'; }, []);
  useEffect(() => {
    if (!busy && invalidField) ({ token: tokenInput, password: passwordInput, confirmation: confirmationInput })[invalidField].current?.focus();
  }, [busy, invalidField]);
  useEffect(() => { if (complete) successHeading.current?.focus(); }, [complete]);

  function invalid(field: Exclude<InvalidField, null>, message: string) {
    setInvalidField(field); setError(new Error(message));
  }
  function clear() {
    submitted.current = null; setToken(''); setPassword(''); setConfirmation('');
  }
  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (inFlight.current || complete) return;
    if (!uncertain) {
      // Read the actual controls so password-manager fills work without synthetic input events.
      const form = new FormData(event.currentTarget);
      const nextToken = String(form.get('recovery_token') ?? '');
      const nextPassword = String(form.get('password') ?? '');
      const nextConfirmation = String(form.get('password_confirmation') ?? '');
      setToken(nextToken); setPassword(nextPassword); setConfirmation(nextConfirmation);
      if (!nextToken.trim()) { invalid('token', 'Enter the recovery code provided by your installation operator.'); return; }
      if ([...nextPassword].length < 12 || new TextEncoder().encode(nextPassword).length > 1024) { invalid('password', 'Use a password of at least 12 characters (maximum 1024 bytes).'); return; }
      if (nextConfirmation !== nextPassword) { invalid('confirmation', 'The passwords do not match. Enter the same password in both fields.'); return; }
      submitted.current = { token: nextToken.trim(), password: nextPassword };
    }
    if (!submitted.current) return;
    inFlight.current = true; setBusy(true); setError(null); setInvalidField(null);
    try {
      const result = await request<unknown>('/auth/recovery/complete', { method: 'POST', body: submitted.current });
      if (result !== undefined) throw new ApiError(0, 'invalid_response', 'The server returned an unexpected response. Check sign-in before retrying.');
      clear(); setUncertain(false); setComplete(true); onCompleted();
    } catch (error) {
      if (error instanceof DOMException && error.name === 'AbortError') return;
      if (error instanceof ApiError && (error.status === 0 || error.status === 408 || error.status >= 500)) setUncertain(true);
      if (error instanceof ApiError && (error.code === 'recovery_invalid' || error.status === 401)) {
        setError(new ApiError(error.status, error.code, 'This recovery code is invalid, expired, or no longer available. Ask your installation operator for a new code.', error.requestId));
        if (!uncertain) setInvalidField('token');
      } else setError(error);
    } finally { inFlight.current = false; setBusy(false); }
  }
  function startAgain() {
    clear(); setUncertain(false); setError(null); setInvalidField(null); tokenInput.current?.focus();
  }

  return <main className="auth-page recovery-page">
    <header className="auth-brand"><Brand /><span>ACCOUNT RECOVERY</span></header>
    <section className="auth-panel recovery-panel" aria-labelledby="recovery-heading">
      {complete ? <>
        <CircleCheck className="recovery-success-icon" aria-hidden="true" />
        <h1 id="recovery-heading" ref={successHeading} tabIndex={-1}>Password reset.</h1>
        <p className="auth-intro">All sessions for the recovered account have ended. This browser is signed out. Sign in with your new password to continue.</p>
        <Link className="button primary auth-submit" href="/signin"><ArrowRight aria-hidden="true" />Sign in</Link>
      </> : <>
        <div className="eyebrow">ACCOUNT RECOVERY</div>
        <h1 id="recovery-heading">Reset your password.</h1>
        <p className="auth-intro">Ask the person who operates this OpenTrack installation to verify your identity and issue a recovery code. No recovery email is sent.</p>
        <p id="recovery-effect" className="recovery-explanation">The code identifies the account to reset. Resetting ends all of that account’s sessions and signs this browser out. Workspace access stays the same.</p>
        <div id="recovery-error"><ErrorBanner error={error} /></div>
        {uncertain && <div className="notice recovery-uncertain" role="status"><strong>The result is uncertain.</strong><p>Your password may already have changed. Try signing in with the new password first. Retrying uses the same code and password; a used-code error does not confirm whether the reset succeeded. If you cannot sign in, ask your installation operator for a new code.</p><Link href="/signin" onClick={event => { if (busy) event.preventDefault(); }} aria-disabled={busy}>Try signing in</Link></div>}
        <form onSubmit={submit} aria-busy={busy} aria-describedby="recovery-effect">
          <fieldset disabled={busy}>
            <label className="field" htmlFor="recovery-token">Recovery code<input id="recovery-token" ref={tokenInput} name="recovery_token" type="password" autoComplete="one-time-code" autoCapitalize="off" spellCheck={false} required autoFocus maxLength={512} value={token} onChange={event => setToken(event.target.value)} readOnly={uncertain} aria-invalid={invalidField === 'token'} aria-describedby={`recovery-code-help${invalidField === 'token' ? ' recovery-error' : ''}`} /><small id="recovery-code-help">Paste the complete code here. It is single-use and has an expiry.</small></label>
            <label className="field" htmlFor="recovery-password">New password<input id="recovery-password" ref={passwordInput} name="password" type="password" autoComplete="new-password" required minLength={12} maxLength={1024} value={password} onChange={event => setPassword(event.target.value)} readOnly={uncertain} aria-invalid={invalidField === 'password'} aria-describedby={`recovery-password-help${invalidField === 'password' ? ' recovery-error' : ''}`} /><small id="recovery-password-help">At least 12 characters. Use a password you do not use elsewhere.</small></label>
            <label className="field" htmlFor="recovery-confirmation">Confirm new password<input id="recovery-confirmation" ref={confirmationInput} name="password_confirmation" type="password" autoComplete="new-password" required minLength={12} maxLength={1024} value={confirmation} onChange={event => setConfirmation(event.target.value)} readOnly={uncertain} aria-invalid={invalidField === 'confirmation'} aria-describedby={invalidField === 'confirmation' ? 'recovery-error' : undefined} /></label>
            <button className="button primary auth-submit" type="submit">{busy ? <LoaderCircle className="spinner" aria-hidden="true" /> : <KeyRound aria-hidden="true" />}{busy ? 'Resetting password…' : uncertain ? 'Retry same reset' : 'Reset password'}</button>
            {uncertain && <button type="button" className="quiet recovery-start-again" onClick={startAgain}>Use a different code</button>}
          </fieldset>
        </form>
        <p className="auth-secondary"><Link href="/signin" aria-disabled={busy} onClick={event => { if (busy) event.preventDefault(); }}>Back to sign in</Link></p>
      </>}
    </section>
    <footer className="auth-footer">OpenTrack <span>Account recovery</span></footer>
  </main>;
}
