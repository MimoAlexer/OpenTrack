import { createElement, useState, type ReactNode } from 'react';
import { openExternalDocumentLink } from '../desktop/bridge';
import { canonicalBody, safeHref, type RichNode } from './model';
import './documents.css';
import { AttachmentCard } from '../attachments/AttachmentCard';
import { IssueReferenceText } from '../references/IssueReferenceText';
import { MentionText } from '../mentions/MentionText';

export function DocumentView({ body, fallback = '', className = '', supported = true }: { body: unknown; fallback?: string; className?: string; supported?: boolean }) {
  const [linkError, setLinkError] = useState('');
  let document: RichNode;
  try { if (!supported) throw new Error('Unsupported schema'); document = canonicalBody(body, true); } catch { return <div className={`document-prose document-fallback ${className}`}><pre>{fallback || 'This document cannot be displayed by this version.'}</pre></div>; }
  function render(node: RichNode, key: string): ReactNode {
    let children: ReactNode = node.content?.map((child, index) => render(child, `${key}.${index}`));
    if (node.type === 'text') {
      children = node.text;
      for (const [index, mark] of (node.marks ?? []).entries()) {
        if (mark.type === 'link' && safeHref(mark.attrs?.href)) { const href = mark.attrs.href; children = <a key={`${key}:mark:${index}`} href={href} target="_blank" rel="noopener noreferrer" onClick={event => { event.preventDefault(); void openExternalDocumentLink(href).catch(() => setLinkError('The link could not be opened. Try again.')); }}>{children}</a>; }
        else { const tag = mark.type === 'bold' ? 'strong' : mark.type === 'italic' ? 'em' : mark.type === 'strike' ? 's' : mark.type === 'code' ? 'code' : 'span'; children = createElement(tag, { key: `${key}:mark:${index}` }, children); }
      }
      return <span key={key}>{children}</span>;
    }
    if (node.type === 'attachment') return <AttachmentCard key={key} id={String(node.attrs?.id)} />;
    if (node.type === 'issueReference') return <IssueReferenceText key={key} id={String(node.attrs?.id)} />;
    if (node.type === 'userMention') return <MentionText key={key} id={String(node.attrs?.id)} />;
    if (node.type === 'doc') return <div key={key}>{children}</div>;
    if (node.type === 'hardBreak') return <br key={key} />;
    if (node.type === 'taskList') return <ul key={key} data-type="taskList">{children}</ul>;
    if (node.type === 'taskItem') return <li key={key} data-type="taskItem" data-checked={!!node.attrs?.checked}><label><input type="checkbox" checked={!!node.attrs?.checked} readOnly disabled aria-label="Task status" /></label><div>{children}</div></li>;
    if (node.type === 'codeBlock') return <pre key={key}><code>{children}</code></pre>;
    const tag = node.type === 'heading' ? `h${node.attrs?.level}` : node.type === 'paragraph' ? 'p' : node.type === 'bulletList' ? 'ul' : node.type === 'orderedList' ? 'ol' : node.type === 'listItem' ? 'li' : 'blockquote';
    return createElement(tag, { key, ...(node.type === 'orderedList' ? { start: node.attrs?.start ?? 1 } : {}) }, children);
  }
  return <><div className={`document-prose ${className}`}>{render(document, 'document')}</div>{linkError && <p role="alert" className="document-notice">{linkError}</p>}</>;
}
