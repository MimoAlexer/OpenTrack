import { lazy, Suspense } from 'react';
import type { RichTextEditorProps } from './RichTextEditor';
import './documents.css';

const RichTextEditor = lazy(() => import('./RichTextEditor'));
export function StructuredEditor(props: RichTextEditorProps) {
  return <Suspense fallback={<div className={`document-editor document-loading${props.compact ? ' compact' : ''}`} role="status">Preparing editor…</div>}><RichTextEditor {...props} /></Suspense>;
}
