import { nativeError, NativeBridgeError } from './http';
import { ApiError } from '../api';
import type { Attachment } from '../attachments/types';
import type { NativeApiResponse } from './types';
import { invoke, isTauri } from '@tauri-apps/api/core';
import type { BrowserAction, ConnectionState, NativeAuthEvent, NativeInvoke, NativeIssueLink, NativeListen, NativePending } from './types';

// Only this module knows native command names. No generic invoke is exposed to
// forms; callbacks receive session metadata, never a native bearer.
export function createNativeBridge(invoke: NativeInvoke, listen: NativeListen, generation: () => string) {
  async function call<T>(command: string, body?: Record<string, unknown>): Promise<T> {
    try { return await invoke<T>(command, body === undefined ? undefined : { request: { expected_generation: generation(), ...body } }); }
    catch (error) { throw nativeError(error); }
  }
  async function attachmentReply(result: NativeApiResponse, expected: string): Promise<Attachment> {
    if (expected !== generation() || result.generation !== expected) throw new NativeBridgeError('native_result_uncertain', 'The connection changed while this file was being transferred. Refresh its status before retrying.', false);
    if (result.status < 200 || result.status > 299) {
      const problem = result.body as {code?:string;detail?:string;request_id?:string;details?:Record<string,unknown>} | null;
      throw new ApiError(result.status, problem?.code ?? 'request_failed', problem?.detail ?? 'The file could not be transferred.', problem?.request_id, problem?.details);
    }
    const file = result.body as Attachment;
    if (!file || typeof file.id !== 'string' || typeof file.etag !== 'string' || !['pending','quarantined','ready','rejected','deleted'].includes(file.state)) throw new NativeBridgeError('native_response_invalid','The file transfer returned an invalid response. Refresh its status.',false);
    return file;
  }
  const files = {
    async pickAttachments(): Promise<PickedAttachment[]> {
      const expected = generation();
      const result = await call<PickedAttachment[]>('attachment_pick', {});
      if (generation() !== expected) throw new DOMException('The connection changed.', 'AbortError');
      return result;
    },
    releasePickedAttachment: (handle: string) => call<void>('attachment_release', {handle}),
    async uploadAttachment(input: AttachmentUpload): Promise<Attachment> {
      const { signal, onProgress, ...request } = input;
      if (signal?.aborted) throw new DOMException('Upload cancelled.', 'AbortError');
      const expected = generation(); const operation_id = crypto.randomUUID();
      const stop = await listen('opentrack-attachment-progress', ({payload}) => {
        if (payload.operation_id === operation_id && expected === generation() && !signal?.aborted
          && Number.isSafeInteger(payload.sent_bytes) && Number.isSafeInteger(payload.total_bytes)
          && payload.sent_bytes >= 0 && payload.sent_bytes <= payload.total_bytes) onProgress?.(payload.sent_bytes,payload.total_bytes);
      });
      const cancel = () => { void invoke('attachment_cancel', {request:{expected_generation:expected,operation_id}}).catch(() => undefined); };
      signal?.addEventListener('abort', cancel, {once:true});
      try {
        if (signal?.aborted) throw new DOMException('Upload cancelled.', 'AbortError');
        const result = await invoke<NativeApiResponse>('attachment_upload', {request:{expected_generation:expected,...request,operation_id}});
        if (signal?.aborted) throw new DOMException('Upload cancelled. Refresh its status before retrying.', 'AbortError');
        return await attachmentReply(result,expected);
      } catch(error) {
        if (error instanceof Error) throw error;
        throw nativeError(error);
      } finally { stop(); signal?.removeEventListener('abort',cancel); }
    },
    async downloadAttachment(input: AttachmentDownload): Promise<{saved:boolean}> {
      const expected = generation();
      const result = await call<{saved:boolean;generation:string}>('attachment_download',{...input});
      if (expected !== generation() || result.generation !== expected) throw new DOMException('The connection changed.', 'AbortError');
      return {saved:result.saved};
    },
  };
  return {
    ...files,
    read: () => call<ConnectionState>('connection_read'),
    select: (origin: string, developmentHttp: boolean) => call<ConnectionState>('connection_select', { origin, development_http: developmentHttp }),
    forget: () => call<ConnectionState>('connection_forget', {}),
    begin: (deviceName: string) => call<NativePending>('auth_begin', { device_name: deviceName }),
    cancel: () => call<ConnectionState>('auth_cancel', {}),
    logout: () => call<ConnectionState & { remote_revocation_confirmed: boolean }>('auth_logout', {}),
    openBrowser: (action: BrowserAction) => call<{ generation: string }>('open_browser_action', { action }),
    onAuth: (handler: (event: NativeAuthEvent) => void) => listen('opentrack-native-auth', event => handler(event.payload)),
    readIssueLink: () => call<NativeIssueLink | null>('issue_link_read'),
    dismissIssueLink: async (id: string) => { try { await invoke('issue_link_dismiss', { request: { id } }); } catch (error) { throw nativeError(error); } },
    onIssueLink: (handler: (link: NativeIssueLink) => void) => listen('opentrack-open-issue', event => handler(event.payload)),
  };
}
export type NativeBridge = ReturnType<typeof createNativeBridge>;

// Call only from an explicit link activation. Native repeats this validation
// before handing the URL to the system, without attaching server credentials.
export async function openExternalDocumentLink(href: string): Promise<void> {
  if (href.length > 2048 || /[\s\u0000-\u001f\u007f\\]/.test(href)) throw new Error('This link is not supported.');
  let url: URL;
  try { url = new URL(href); } catch { throw new Error('This link is not supported.'); }
  if (url.username || url.password || !(['http:', 'https:'].includes(url.protocol) && url.hostname || url.protocol === 'mailto:' && url.pathname && !url.search && !url.hash)) throw new Error('This link is not supported.');
  if (isTauri()) {
    try { await invoke('open_document_link', { request: { href } }); }
    catch (error) { throw nativeError(error); }
  } else window.open(href, '_blank', 'noopener,noreferrer');
}

export interface PickedAttachment { handle:string;original_name:string;size_bytes:number;sha256:string;mime_hint:string|null }
export interface AttachmentUpload {workspace_id:string;attachment_id:string;handle:string;etag:string;signal?:AbortSignal;onProgress?:(sentBytes:number,totalBytes:number)=>void}
export interface AttachmentDownload {workspace_id:string;attachment_id:string;original_name:string}
let attachmentBridge: NativeBridge | undefined;
export function installAttachmentBridge(bridge:NativeBridge):()=>void {
  attachmentBridge=bridge;
  return ()=>{if(attachmentBridge===bridge)attachmentBridge=undefined;};
}
function files():NativeBridge {if(!attachmentBridge)throw new NativeBridgeError('native_unavailable','The desktop connection is unavailable.',true);return attachmentBridge;}
export const pickAttachments=():Promise<PickedAttachment[]>=>files().pickAttachments();
export const releasePickedAttachment=(handle:string):Promise<void>=>files().releasePickedAttachment(handle);
export const uploadAttachment=(input:AttachmentUpload):Promise<Attachment>=>files().uploadAttachment(input);
export const downloadAttachment=(input:AttachmentDownload):Promise<{saved:boolean}>=>files().downloadAttachment(input);
