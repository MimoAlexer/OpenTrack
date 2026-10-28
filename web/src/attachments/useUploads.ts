import { useEffect, useRef, useState } from 'react';
import { isTauri } from '@tauri-apps/api/core';
import { pickAttachments, releasePickedAttachment, uploadAttachment } from '../desktop/bridge';
import { request } from '../api';
import { useUnsavedChanges } from '../router';
import { useAttachments, type AttachmentContextValue } from './context';
import { hashFile, uploadBrowserFile, validateFile } from './browser';
import type { Attachment, AttachmentDraftState, AttachmentTarget, CreateAttachment } from './types';

function releaseHandle(handle: string) { void Promise.resolve().then(() => releasePickedAttachment(handle)).catch(() => {}); }

type Selected = { name: string; size: number; mime: string | null; file?: File; handle?: string; sha256?: string };
export type UploadRow = { key: string; name: string; size: number; phase: 'hashing' | 'reserving' | 'uploading' | 'checking' | 'waiting' | 'failed' | 'canceling'; sent: number | null; total: number; attachment: Attachment | null; error: string | null; canceled: boolean };
type ScopedRow = UploadRow & { scope: string };
type Entry = UploadRow & { source: Selected; abort: AbortController; scope: string; revision: number; target: AttachmentTarget; reservation: CreateAttachment | null; attemptedReservation: boolean; anchor: string; running: boolean; cancelTask: Promise<boolean> | null };
export function useUploads({ target, enabled, prepareAnchor, ready, forgetAnchor, changed }: {
  target?: AttachmentTarget; enabled: boolean; prepareAnchor: (from?: string) => string; ready: (value: Attachment, anchor: string) => boolean; forgetAnchor: (anchor: string) => void; changed?: (state: AttachmentDraftState) => void;
}) {
  const context = useAttachments(); const [rows, setRows] = useState<ScopedRow[]>([]); const [notice, setNotice] = useState('');
  const entries = useRef(new Map<string, Entry>()); const lane = useRef(Promise.resolve()); const mounted = useRef(true);
  const current = useRef({ context, target, enabled, ready, prepareAnchor, forgetAnchor, changed }); current.current = { context, target, enabled, ready, prepareAnchor, forgetAnchor, changed };
  const scope = context && target ? `${context.workspaceId}:${context.binding}:${JSON.stringify(target)}` : null;
  const scopeRevision = useRef({ scope, value: 0 });
  if (scopeRevision.current.scope !== scope) scopeRevision.current = { scope, value: scopeRevision.current.value + 1 };
  const sync = () => { if (mounted.current) setRows([...entries.current.values()].map(({ key, name, size, phase, sent, total, attachment, error, canceled, scope }) => ({ key, name, size, phase, sent, total, attachment, error, canceled, scope }))); };
  const patch = (entry: Entry, values: Partial<Entry>) => { Object.assign(entry, values); sync(); };
  useEffect(() => { mounted.current = true; return () => { mounted.current = false; entries.current.forEach(entry => { entry.abort.abort(); if (entry.source.handle) releaseHandle(entry.source.handle); }); }; }, []);
  const pending = rows.length > 0; const busy = rows.some(row => ['hashing', 'reserving', 'uploading', 'checking', 'canceling'].includes(row.phase));
  useUnsavedChanges(pending);
  useEffect(() => { current.current.changed?.({ pending, busy, cancelAll: async () => { const results = await Promise.all([...entries.current.keys()].map(cancel)); return results.every(Boolean); } }); }, [pending, busy, rows]);
  function activeContext(entry: Entry, writing = true) {
    const now = current.current;
    if (!now.context || !now.target || `${now.context.workspaceId}:${now.context.binding}:${JSON.stringify(now.target)}` !== entry.scope) throw new Error('The account, team, or document changed. Choose the file again in the current document.');
    if (writing && scopeRevision.current.value !== entry.revision) throw new Error('The account, team, or document changed. Choose the file again in the current document.');
    if (writing && !now.enabled) throw new Error('Editing is paused. Your upload is preserved; retry when editing is available.');
    return now.context;
  }
  async function reserve(entry: Entry, signal: AbortSignal) {
    const ctx = activeContext(entry, false); patch(entry, { phase: entry.canceled ? 'canceling' : 'reserving', attemptedReservation: true });
    const result = await request<Attachment>(`/workspaces/${ctx.workspaceId}/attachments`, { method: 'POST', signal, csrf: ctx.session.csrf_token, headers: { 'Idempotency-Key': entry.key }, body: entry.reservation });
    patch(entry, { attachment: result }); return result;
  }
  function finish(entry: Entry, value: Attachment, receiptContext: AttachmentContextValue) {
    activeContext(entry); if (entry.canceled || !mounted.current) return;
    patch(entry, { attachment: value });
    if (value.state === 'ready') {
      receiptContext.rememberReady(value);
      if (!current.current.ready(value, entry.anchor)) { patch(entry, { phase: 'failed', error: 'The file is ready, but it could not be inserted here. Retry after making room in the document.' }); return; }
      entries.current.delete(entry.key); current.current.forgetAnchor(entry.anchor); sync();
      if (entry.source.handle) releaseHandle(entry.source.handle);
    } else if (value.state === 'quarantined') patch(entry, { phase: 'waiting', error: value.scan_status === 'unavailable' ? 'The scanner is unavailable. The file is not attached yet.' : null });
    else if (value.state === 'rejected') patch(entry, { phase: 'failed', error: 'This file was rejected and cannot be attached.' });
    else if (value.state === 'deleted') patch(entry, { phase: 'failed', error: 'This upload was canceled or expired. Remove it and choose the file again.' });
    else patch(entry, { phase: 'failed', error: 'The transfer is not complete. Retry to continue with the same upload.' });
  }
  async function run(entry: Entry) {
    if (!entries.current.has(entry.key) || entry.canceled || entry.running) return;
    entry.running = true; const controller = new AbortController(); entry.abort = controller; const { signal } = controller; patch(entry, { error: null });
    try {
      let ctx = activeContext(entry);
      if (!entry.reservation) {
        patch(entry, { phase: 'hashing' }); const hash = entry.source.sha256 ?? await hashFile(entry.source.file!, signal, ctx.maximum); signal.throwIfAborted();
        entry.reservation = { ...entry.target, original_name: entry.name, size_bytes: entry.size, sha256: hash, mime_hint: entry.source.mime };
      }
      ctx = activeContext(entry);
      let receiptContext = ctx;
      let result = entry.attachment ? await request<Attachment>(`/workspaces/${ctx.workspaceId}/attachments/${entry.attachment.id}`, { signal }) : await reserve(entry, signal);
      signal.throwIfAborted(); ctx = activeContext(entry);
      if (result.state === 'pending') {
        receiptContext = ctx;
        patch(entry, { phase: 'uploading', attachment: result, sent: null });
        const progress = (sent: number, total: number) => { if (!signal.aborted && entries.current.has(entry.key)) patch(entry, { sent: Math.min(sent, total), total, phase: sent >= total ? 'checking' : 'uploading' }); };
        result = entry.source.file
          ? await uploadBrowserFile(ctx.workspaceId, result, entry.source.file, ctx.session.csrf_token, signal, progress)
          : await uploadAttachment({ workspace_id: ctx.workspaceId, attachment_id: result.id, handle: entry.source.handle!, etag: result.etag, signal, onProgress: progress });
      } else if (result.state === 'quarantined') {
        receiptContext = ctx;
        patch(entry, { phase: 'checking', attachment: result });
        result = await request<Attachment>(`/workspaces/${ctx.workspaceId}/attachments/${result.id}/finalize`, { method: 'POST', csrf: ctx.session.csrf_token, signal, timeoutMs: 90_000, headers: { 'If-Match': result.etag } });
      }
      signal.throwIfAborted(); finish(entry, result, receiptContext);
    } catch (error) {
      if (entry.canceled || !mounted.current) return;
      patch(entry, { phase: 'failed', error: `${error instanceof Error ? error.message : 'The upload did not complete. Retry to check its current status.'}${entry.source.handle ? ' File access expires after 30 minutes or when you sign in again. Choose the same file again if needed.' : ''}` });
    } finally { entry.running = false; sync(); }
  }
  function schedule(entry: Entry) { lane.current = lane.current.then(() => run(entry)).catch(() => {}); }
  function release(sources: Selected[]) { sources.forEach(source => { if (source.handle) releaseHandle(source.handle); }); }
  function add(sources: Selected[], anchor?: string) {
    const now = current.current; const ctx = now.context;
    if (!ctx?.enabled || !now.target || !now.enabled) { release(sources); if (anchor) now.forgetAnchor(anchor); return; }
    if (sources.length > 10 || entries.current.size + sources.length > 20) { setNotice('Choose up to 10 files at a time, with no more than 20 unfinished uploads.'); release(sources); if (anchor) now.forgetAnchor(anchor); return; }
    setNotice(''); let usedAnchor = false;
    for (const source of sources) {
      try { validateFile(source, ctx.maximum); } catch (error) { setNotice(error instanceof Error ? error.message : 'This file cannot be uploaded.'); release([source]); continue; }
      const key = crypto.randomUUID(); const entry: Entry = { key, name: source.name, size: source.size, source, phase: 'hashing', sent: null, total: source.size, attachment: null, error: null, abort: new AbortController(), scope: `${ctx.workspaceId}:${ctx.binding}:${JSON.stringify(now.target)}`, revision: scopeRevision.current.value, target: { ...now.target }, reservation: null, attemptedReservation: false, anchor: !usedAnchor && anchor ? anchor : now.prepareAnchor(anchor), canceled: false, running: false, cancelTask: null };
      usedAnchor = true; entries.current.set(key, entry); schedule(entry);
    }
    if (!usedAnchor && anchor) now.forgetAnchor(anchor); sync();
  }
  function addBrowserFiles(files: File[], anchor?: string, revision?: number) {
    if (revision !== undefined && revision !== scopeRevision.current.value) { if (anchor) current.current.forgetAnchor(anchor); setNotice('The account, team, or document changed. Choose the file again in the current document.'); return; }
    add(files.map(file => ({ name: file.name, size: file.size, mime: file.type || null, file })), anchor);
  }
  async function pickNative(anchor: string) {
    setNotice(''); const revision = scopeRevision.current.value; const forget = current.current.forgetAnchor;
    try {
      const selected = await pickAttachments();
      const sources = selected.map(file => ({ name: file.original_name, size: file.size_bytes, sha256: file.sha256, mime: file.mime_hint, handle: file.handle }));
      if (!mounted.current || revision !== scopeRevision.current.value) { release(sources); forget(anchor); if (mounted.current) setNotice('The account, team, or document changed. Choose the file again in the current document.'); return; }
      add(sources, anchor);
    }
    catch (error) { forget(anchor); if (mounted.current) setNotice(error instanceof Error ? error.message : 'The file picker could not be opened.'); }
  }
  function cancel(key: string): Promise<boolean> {
    const entry = entries.current.get(key); if (!entry) return Promise.resolve(true);
    if (entry.cancelTask) return entry.cancelTask;
    const task = cancelEntry(entry); entry.cancelTask = task;
    void task.then(() => { if (entry.cancelTask === task) entry.cancelTask = null; }, () => { if (entry.cancelTask === task) entry.cancelTask = null; });
    return task;
  }
  async function cancelEntry(entry: Entry): Promise<boolean> {
    const { key } = entry;
    entry.canceled = true; entry.abort.abort(); patch(entry, { phase: 'canceling', error: null });
    if (!entry.attachment && !entry.attemptedReservation) {
      entries.current.delete(key); current.current.forgetAnchor(entry.anchor); sync(); release([entry.source]); return true;
    }
    // Resolve an ambiguous reservation with its original key before deleting it.
    // This controller is distinct from the old transfer; that transfer cannot
    // adopt the cleanup signal and restart after cancellation.
    const controller = new AbortController(); entry.abort = controller; const { signal } = controller;
    try {
      const ctx = activeContext(entry, false); let record = entry.attachment;
      if (!record && entry.attemptedReservation) record = await reserve(entry, signal);
      if (record) {
        record = await request<Attachment>(`/workspaces/${ctx.workspaceId}/attachments/${record.id}`, { signal });
        if (record.state !== 'deleted') await request<void>(`/workspaces/${ctx.workspaceId}/attachments/${record.id}`, { method: 'DELETE', csrf: ctx.session.csrf_token, signal, headers: { 'If-Match': record.etag } });
      }
      entries.current.delete(key); current.current.forgetAnchor(entry.anchor); sync(); release([entry.source]); return true;
    } catch (error) { patch(entry, { phase: 'failed', error: `Cancellation could not be confirmed. ${error instanceof Error ? error.message : 'Try again.'}` }); if (entry.scope !== scopeRevision.current.scope && mounted.current) setNotice('An upload belongs to another account, team, or document. Return to it to confirm cancellation.'); return false; }
  }
  async function reselect(key: string) {
    let selectedSources: Selected[] = []; let adopted = false;
    const entry = entries.current.get(key); if (!entry || entry.running || entry.canceled) return;
    try {
      const selected = await pickAttachments();
      const sources = selected.map(file => ({ name: file.original_name, size: file.size_bytes, sha256: file.sha256, mime: file.mime_hint, handle: file.handle })); selectedSources = sources;
      if (sources.length !== 1 || sources[0].size !== entry.size || sources[0].sha256 !== (entry.reservation?.sha256 ?? entry.source.sha256)) { release(sources); patch(entry, { error: 'Choose the same file with the same contents to retry this upload.' }); return; }
      if (!mounted.current || !entries.current.has(key) || entry.canceled) { release(sources); return; }
      activeContext(entry); release([entry.source]); entry.source = { ...sources[0], name: entry.name }; adopted = true; schedule(entry);
    } catch (error) { if (!adopted) release(selectedSources); patch(entry, { error: error instanceof Error ? error.message : 'The file could not be selected.' }); }
  }
  function retry(key: string) { const entry = entries.current.get(key); if (!entry || entry.running) return; if (entry.canceled) { void cancel(key); return; } schedule(entry); }
  return { rows: rows.filter(row => row.scope === scope), notice, offered: !!context && !!target, available: !!context?.enabled && !!target && enabled, native: isTauri(), pickerRevision: () => scopeRevision.current.value, addBrowserFiles, pickNative, cancel, retry, reselect };
}
