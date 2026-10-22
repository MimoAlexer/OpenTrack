import { AlertCircle, Circle, CircleCheck, CircleDashed, CircleX, LoaderCircle, PanelsTopLeft, X } from 'lucide-react';
import { useEffect, useId, useRef, type ReactNode } from 'react';
import { ApiError, errorMessage } from './api';
import { initials } from './issue-model';
import type { Status, User } from './types';

export function Brand() { return <span className="brand"><PanelsTopLeft aria-hidden="true" />OpenTrack</span>; }

export function Avatar({ user, small = false }: { user?: Pick<User, 'display_name'>; small?: boolean }) {
  return <span className={`avatar${small ? ' small' : ''}`} aria-label={user?.display_name ?? 'Unassigned'} title={user?.display_name ?? 'Unassigned'}>{user ? initials(user.display_name) : '—'}</span>;
}

export function StatusMark({ status }: { status?: Status }) {
  const Icon = status?.category === 'completed' ? CircleCheck : status?.category === 'canceled' ? CircleX : status?.category === 'started' ? LoaderCircle : status?.category === 'backlog' ? CircleDashed : Circle;
  const color = status?.color && ['gray', 'slate', 'red', 'orange', 'amber', 'green', 'teal', 'blue', 'indigo', 'purple', 'pink'].includes(status.color) ? ` status-color-${status.color}` : '';
  return <Icon className={`status-mark status-${status?.category ?? 'unstarted'}${color}`} aria-label={status?.name ?? 'Unknown status'} />;
}

export function ErrorBanner({ error, retry }: { error: unknown; retry?: () => void }) {
  if (!error) return null;
  return <div className="error-banner" role="alert"><AlertCircle aria-hidden="true" /><div><p>{errorMessage(error)}</p>{error instanceof ApiError && error.requestId && <small>Request {error.requestId}</small>}</div>{retry && <button type="button" className="button compact" onClick={retry}>Try again</button>}</div>;
}

export function Loading({ label = 'Loading workspace…' }: { label?: string }) {
  return <div className="loading" role="status"><LoaderCircle className="spinner" aria-hidden="true" />{label}</div>;
}

export function Modal({ title, children, close, canClose = true }: { title: string; children: ReactNode; close: () => void; canClose?: boolean }) {
  const ref = useRef<HTMLDialogElement>(null);
  const titleId = useId();
  const closeRef = useRef(close); closeRef.current = close;
  useEffect(() => {
    const dialog = ref.current!;
    const trigger = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    dialog.showModal();
    dialog.querySelector<HTMLElement>('[data-autofocus]')?.focus();
    return () => {
      dialog.close();
      const underlying = [...document.querySelectorAll<HTMLDialogElement>('dialog[open]')].filter(item => item !== dialog).at(-1);
      if (trigger?.isConnected && !trigger.matches(':disabled') && (!underlying || underlying.contains(trigger))) trigger.focus();
      else if (underlying) {
        const target = underlying.querySelector<HTMLElement>('[data-autofocus]:not(:disabled)') ?? underlying.querySelector<HTMLElement>('input:not(:disabled):not([type="hidden"]), textarea:not(:disabled), select:not(:disabled), button:not(:disabled), a[href], [tabindex="0"]');
        (target ?? underlying).focus();
      }
    };
  }, []);
  return <dialog className="modal" ref={ref} aria-labelledby={titleId} onCancel={event => { event.preventDefault(); event.stopPropagation(); if (canClose) closeRef.current(); }}>
    <header className="modal-header"><h2 id={titleId}>{title}</h2><button type="button" className="icon-button" aria-label="Close dialog" disabled={!canClose} onClick={close}><X aria-hidden="true" /></button></header>
    {children}
  </dialog>;
}
