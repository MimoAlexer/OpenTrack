import type { Session } from '../types';

export interface ConnectionState {
  generation: string;
  scope_generation: string;
  selected: { origin: string; development_http: boolean } | null;
  credential_present: boolean;
  pending: { request_id: string; confirmation_code: string; expires_at: string } | null;
}
export interface NativeAuthEvent {
  generation: string;
  scope_generation: string;
  kind: 'pending' | 'authenticated' | 'failed' | 'cancelled' | 'signed_out';
  request_id: string | null;
  session: Session | null;
  error: { code: string; message: string; retryable: boolean } | null;
  remote_revocation_confirmed: boolean | null;
}
export interface NativeApiResponse {
  generation: string;
  status: number;
  headers: Record<string, string>;
  body: unknown;
}
export interface NativePending {
  generation: string;
  request_id: string;
  confirmation_code: string;
  expires_at: string;
}
export interface NativeIssueLink { id: string; server: string; workspace: string; issue: string }
export type BrowserAction = 'setup' | 'join' | 'recover';
export type NativeInvoke = <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
export interface NativeAttachmentProgress { operation_id: string; sent_bytes: number; total_bytes: number }
interface NativeEvents { 'opentrack-attachment-progress': NativeAttachmentProgress; 'opentrack-native-auth': NativeAuthEvent; 'opentrack-open-issue': NativeIssueLink }
export type NativeListen = <K extends keyof NativeEvents>(event: K, handler: (event: { payload: NativeEvents[K] }) => void) => Promise<() => void>;
