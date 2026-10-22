import type { components } from '../generated/api';
export type StructuredDocument = components['schemas']['Document'];
export type IssueComment = components['schemas']['Comment'];
export type DocumentWrite = components['schemas']['DocumentWrite'];
export type Page<T> = { data: T[]; has_more: boolean; next_cursor: string | null };
