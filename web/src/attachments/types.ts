import type { components } from '../generated/api';
export type Attachment = components['schemas']['Attachment'];
export type AttachmentState = Attachment['state'];
export type CreateAttachment = components['schemas']['CreateAttachment'];
export type AttachmentTarget = { document_id: string; team_id?: never } | { team_id: string; document_id?: never };
export type AttachmentDraftState = { pending: boolean; busy: boolean; cancelAll: () => Promise<boolean> };
export const MAX_ATTACHMENT_BYTES = 25 * 1024 * 1024;
export const HARD_MAX_ATTACHMENT_BYTES = 100 * 1024 * 1024;
export function formatBytes(value: number): string {
  if (value < 1024) return `${value} B`;
  if (value < 1024 * 1024) return `${(value / 1024).toFixed(value < 10240 ? 1 : 0)} KB`;
  return `${(value / (1024 * 1024)).toFixed(1)} MB`;
}
export function scanLabel(value: Attachment['scan_status']): string {
  return { pending: 'Waiting for scan', clean: 'Scan passed', not_scanned: 'Not scanned', infected: 'Rejected by scanner', unavailable: 'Scanner unavailable' }[value];
}
