import { useEffect, useId, useMemo, useRef, useState } from 'react';
import { EditorContent, useEditor, useEditorState } from '@tiptap/react';
import { Extension, type Editor } from '@tiptap/core';
import { Plugin } from '@tiptap/pm/state';
import StarterKit from '@tiptap/starter-kit';
import { Code as CodeMark } from '@tiptap/extension-code';
import { TaskItem, TaskList } from '@tiptap/extension-list';
import { Bold, Italic, Strikethrough, Code, Link2, List, ListOrdered, ListTodo, Quote, Undo2, Redo2, Paperclip } from 'lucide-react';
import { Modal } from '../components';
import { canonicalBody, MAX_DOCUMENT_BYTES, safeHref, sameBody, type RichBody } from './model';
import './documents.css';
import { AttachmentNode } from '../attachments/node';
import { useUploads } from '../attachments/useUploads';
import { UploadQueue } from '../attachments/UploadQueue';
import { DocumentPortability } from './DocumentPortability';
import type { Attachment, AttachmentDraftState, AttachmentTarget } from '../attachments/types';
import { IssueReferenceNode } from '../references/node';
import { useReferenceEditor } from '../references/useReferenceEditor';
import { UserMentionNode } from '../mentions/node';
import { useMentions } from '../mentions/context';
import { useMentionSuggestions } from '../mentions/useMentionSuggestions';
import { MentionPicker } from '../mentions/MentionPicker';
import { mentionMatch, type MentionMatch } from '../mentions/trigger';
import type { MentionCandidate } from '../mentions/types';

export type RichTextEditorProps = {
  value: RichBody; onChange: (value: RichBody) => void; label: string; disabled?: boolean;
  attachmentTarget?: AttachmentTarget; attachmentsChanged?: (state: AttachmentDraftState) => void;
  compact?: boolean; onCompositionChange?: (composing: boolean) => void; onSave?: () => void;
};
const blocks = [
  { label: 'Paragraph', run: (editor: Editor) => editor.chain().focus().setParagraph().run() },
  ...([1, 2, 3] as const).map(level => ({ label: `Heading ${level}`, run: (editor: Editor) => editor.chain().focus().setHeading({ level }).run() })),
  { label: 'Bullet list', run: (editor: Editor) => editor.chain().focus().toggleBulletList().run() },
  { label: 'Numbered list', run: (editor: Editor) => editor.chain().focus().toggleOrderedList().run() },
  { label: 'Task list', run: (editor: Editor) => editor.chain().focus().toggleTaskList().run() },
  { label: 'Quote', run: (editor: Editor) => editor.chain().focus().toggleBlockquote().run() },
  { label: 'Code block', run: (editor: Editor) => editor.chain().focus().toggleCodeBlock().run() },
];

export default function RichTextEditor(props: RichTextEditorProps) {
  const current = useRef(props); current.current = props;
  const anchors = useRef(new Map<string, number>()); const fileInput = useRef<HTMLInputElement>(null); const pickerAnchor = useRef<string | null>(null);
  const [dragging, setDragging] = useState(false); const uploadEvents = useRef<{ files: (files: File[]) => void } | null>(null);
  const [notice, setNotice] = useState(''); const [linkOpen, setLinkOpen] = useState(false); const [link, setLink] = useState('');
  const [slash, setSlash] = useState<number | null>(null); const slashRef = useRef(slash); slashRef.current = slash;
  const references = useReferenceEditor(!!props.disabled); const referencesRef = useRef(references); referencesRef.current = references;
  const mentionContext = useMentions(); const contextRef = useRef(mentionContext); contextRef.current = mentionContext;
  const [mentionState, setMention] = useState<{ key: string; match: MentionMatch } | null>(null); const [mentionActive, setMentionActive] = useState(0);
  const composing = useRef(false); const dismissedMention = useRef(''); const mentionList = useId();
  const mention = !props.disabled && mentionState?.key === mentionContext?.key ? mentionState?.match ?? null : null;
  const suggestions = useMentionSuggestions(mention?.query ?? null); const mentionKeys = useRef<(event: KeyboardEvent) => boolean>(() => false);
  useEffect(() => setMentionActive(0), [suggestions.key]);
  function updateMention(target: Editor) {
    referencesRef.current.observe(target);
    const match = !current.current.disabled && !composing.current && contextRef.current && target.view.hasFocus() ? mentionMatch(target.state) : null;
    const token = match ? `${match.from}:${match.to}:${match.query}` : '';
    if (!match) dismissedMention.current = '';
    setMention(old => {
      if (!match || !contextRef.current || token === dismissedMention.current) return null;
      return old?.key === contextRef.current.key && old.match.from === match.from && old.match.to === match.to && old.match.query === match.query ? old : { key: contextRef.current.key, match };
    });
  }
  useEffect(() => { const input = fileInput.current; const cancel = () => { if (pickerAnchor.current) anchors.current.delete(pickerAnchor.current); pickerAnchor.current = null; }; input?.addEventListener('cancel', cancel); return () => input?.removeEventListener('cancel', cancel); }, []);
  const alive = useRef(true); useEffect(() => { alive.current = true; return () => { alive.current = false; }; }, []);
  const extensions = useMemo(() => [
    StarterKit.configure({ heading: { levels: [1, 2, 3] }, code: false, underline: false, horizontalRule: false, trailingNode: false,
      link: { openOnClick: false, autolink: false, linkOnPaste: true, defaultProtocol: 'https', isAllowedUri: value => safeHref(value),
        HTMLAttributes: { target: '_blank', rel: 'noopener noreferrer' } } }),
    CodeMark.extend({ excludes: '' }),
    AttachmentNode, UserMentionNode, IssueReferenceNode, TaskList, TaskItem.configure({ nested: true, a11y: { checkboxLabel: node => `Task: ${node.textContent || 'Untitled'}` } }),
    Extension.create({ name: 'documentBounds', addProseMirrorPlugins: () => [new Plugin({ filterTransaction(transaction) {
      if (!transaction.docChanged) return true;
      try { canonicalBody(transaction.doc.toJSON()); return true; }
      catch (error) { queueMicrotask(() => { if (alive.current) setNotice(error instanceof Error ? error.message : 'This change cannot be added.'); }); return false; }
    } })] }),
  ], []);
  const editor = useEditor({ extensions, content: props.value, editable: !props.disabled, immediatelyRender: true,
    shouldRerenderOnTransaction: false, enableContentCheck: true,
    editorProps: {
      attributes: { class: 'document-prose', role: 'textbox', 'aria-label': props.label, 'aria-multiline': 'true', 'aria-readonly': String(!!props.disabled), spellcheck: 'true' },
      handlePaste(_view, event) {
        const files = Array.from(event.clipboardData?.files ?? []);
        if (files.length && current.current.attachmentTarget) { uploadEvents.current?.files(files); return true; }
        const text = event.clipboardData?.getData('text/html') || event.clipboardData?.getData('text/plain') || '';
        if (new TextEncoder().encode(text).length > MAX_DOCUMENT_BYTES) { setNotice('This paste is too large. Add less than 256 KiB at a time.'); return true; }
        return false;
      },
      handleDOMEvents: {
        compositionstart() { referencesRef.current.composition(true); composing.current = true; setMention(null); current.current.onCompositionChange?.(true); return false; },
        compositionend() { referencesRef.current.composition(false); composing.current = false; current.current.onCompositionChange?.(false); queueMicrotask(() => { if (alive.current && instance.current) updateMention(instance.current); }); return false; },
      },
      handleKeyDown(view, event) {
        if (event.isComposing || event.keyCode === 229 || view.composing || event.altKey || event.getModifierState('AltGraph')) return false;
        if (referencesRef.current.keyDown(event)) return true;
        if (mentionKeys.current(event)) return true;
        if (slashRef.current !== null) {
          if (event.key === 'ArrowDown' || event.key === 'ArrowUp') { event.preventDefault(); setSlash((slashRef.current + (event.key === 'ArrowDown' ? 1 : blocks.length - 1)) % blocks.length); return true; }
          if (event.key === 'Enter') { event.preventDefault(); const selected = slashRef.current; setSlash(null); if (instance.current) blocks[selected].run(instance.current); return true; }
          if (event.key === 'Escape') { event.preventDefault(); setSlash(null); view.dispatch(view.state.tr.insertText('/')); return true; }
          setSlash(null);
        }
        if (event.key === '/' && !event.ctrlKey && !event.metaKey && view.state.selection.empty && view.state.selection.$from.parent.type.name === 'paragraph' && view.state.selection.$from.parent.content.size === 0) { setSlash(0); return true; }
        if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'k') { event.preventDefault(); event.stopPropagation(); openLink(); return true; }
        if ((event.ctrlKey || event.metaKey) && event.key === 'Enter' && !event.repeat && current.current.onSave) { event.preventDefault(); event.stopPropagation(); current.current.onSave(); return true; }
        return false;
      },
    },
    onTransaction({ transaction, editor }) { if (transaction.docChanged) anchors.current.forEach((position, key) => anchors.current.set(key, transaction.mapping.map(position, 1))); updateMention(editor); },
    onFocus({ editor }) { updateMention(editor); },
    onBlur() { setMention(null); referencesRef.current.clear(); },
    onUpdate({ editor }) { setNotice(''); current.current.onChange(canonicalBody(editor.getJSON())); },
    onContentError() { setNotice('This document contains unsupported formatting. Your saved content has not been changed.'); },
  });
  const instance = useRef(editor); instance.current = editor;
  function chooseMention(person: MentionCandidate) {
    const target = instance.current; if (!target || !mention || !mentionContext || target.isDestroyed || !target.isEditable || props.disabled || suggestions.loading || suggestions.error || !suggestions.choices.some(item => item.id === person.id)) return;
    const currentMatch = mentionMatch(target.state);
    if (!currentMatch || currentMatch.from !== mention.from || currentMatch.to !== mention.to || currentMatch.query !== mention.query) return;
    const before = target.state.doc;
    target.chain().focus().insertContentAt({ from: mention.from, to: mention.to }, [{ type: 'userMention', attrs: { id: person.id } }, { type: 'text', text: ' ' }]).run();
    if (target.state.doc !== before) mentionContext.remember(person);
    setMention(null);
  }
  mentionKeys.current = event => {
    if (!mention || composing.current) return false;
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') { event.preventDefault(); event.stopPropagation(); if (suggestions.choices.length) setMentionActive(value => (value + (event.key === 'ArrowDown' ? 1 : suggestions.choices.length - 1)) % suggestions.choices.length); return true; }
    if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); dismissedMention.current = `${mention.from}:${mention.to}:${mention.query}`; setMention(null); return true; }
    if (event.key === 'Enter' || event.key === 'Tab') {
      const person = suggestions.choices[mentionActive];
      if (event.key === 'Tab' && (!person || suggestions.loading || suggestions.error)) { setMention(null); return false; }
      event.preventDefault(); event.stopPropagation(); if (person) chooseMention(person); return true;
    }
    return false;
  };
  useEffect(() => {
    if (!editor || editor.isDestroyed) return;
    const dom = editor.view.dom;
    if (mention) { dom.setAttribute('aria-autocomplete', 'list'); dom.setAttribute('aria-haspopup', 'listbox'); dom.setAttribute('aria-expanded', 'true'); dom.setAttribute('aria-controls', mentionList); if (suggestions.choices[mentionActive]) dom.setAttribute('aria-activedescendant', `${mentionList}-${mentionActive}`); else dom.removeAttribute('aria-activedescendant'); }
    else if (references.accessibility) { dom.setAttribute('aria-autocomplete', 'list'); dom.setAttribute('aria-haspopup', 'listbox'); dom.setAttribute('aria-expanded', 'true'); dom.setAttribute('aria-controls', references.accessibility.listId); if (references.accessibility.activeId) dom.setAttribute('aria-activedescendant', references.accessibility.activeId); else dom.removeAttribute('aria-activedescendant'); }
    else for (const name of ['aria-autocomplete', 'aria-haspopup', 'aria-expanded', 'aria-controls', 'aria-activedescendant']) dom.removeAttribute(name);
  }, [editor, mention, mentionActive, mentionList, suggestions.choices, references.accessibility?.listId, references.accessibility?.activeId]);
  function prepareAnchor(from?: string) { const id = crypto.randomUUID(); anchors.current.set(id, from && anchors.current.has(from) ? anchors.current.get(from)! : instance.current?.state.selection.to ?? 0); return id; }
  function insertAttachment(value: Attachment, anchor: string) {
    const target = instance.current; const position = anchors.current.get(anchor);
    if (!target || target.isDestroyed || !target.isEditable || current.current.disabled || position === undefined) return false;
    const before = target.state.doc;
    const following = [...anchors.current].filter(([, point]) => point === position).map(([key]) => key);
    target.commands.insertContentAt(Math.max(0, Math.min(position, before.content.size)), { type: 'attachment', attrs: { id: value.id } }, { updateSelection: false });
    if (target.state.doc === before) return false;
    // Inserting a block at a paragraph end can place it outside the mapped
    // text position. Keep the rest of this batch after the actual inserted node.
    let insertedEnd: number | undefined;
    target.state.doc.descendants((node, point) => {
      if (node.type.name === 'attachment' && node.attrs.id === value.id) insertedEnd = point + node.nodeSize;
    });
    if (insertedEnd !== undefined) following.forEach(key => {
      const mapped = anchors.current.get(key);
      if (mapped !== undefined) anchors.current.set(key, Math.max(mapped, insertedEnd!));
    });
    return true;
  }
  const uploads = useUploads({ target: props.attachmentTarget, enabled: !props.disabled, prepareAnchor, ready: insertAttachment, forgetAnchor: anchor => anchors.current.delete(anchor), changed: props.attachmentsChanged });
  uploadEvents.current = { files: files => uploads.addBrowserFiles(files, prepareAnchor()) };
  function chooseFiles() {
    if (!uploads.available) return;
    if (pickerAnchor.current) anchors.current.delete(pickerAnchor.current);
    const anchor = prepareAnchor();
    if (uploads.native) void uploads.pickNative(anchor);
    else { pickerAnchor.current = anchor; fileInput.current?.click(); }
  }

  useEffect(() => { editor?.setEditable(!props.disabled, false); if (props.disabled) setSlash(null); }, [editor, props.disabled]);
  useEffect(() => {
    if (!editor || editor.isDestroyed) return;
    if (!sameBody(canonicalBody(editor.getJSON()), props.value)) editor.commands.setContent(props.value, { emitUpdate: false, errorOnInvalidContent: true });
  }, [editor, props.value]);
  const state = useEditorState({ editor, selector: ({ editor }) => ({ bold: editor?.isActive('bold'), italic: editor?.isActive('italic'), strike: editor?.isActive('strike'), code: editor?.isActive('code'), bullet: editor?.isActive('bulletList'), ordered: editor?.isActive('orderedList'), task: editor?.isActive('taskList'), quote: editor?.isActive('blockquote'), link: editor?.isActive('link'), codeBlock: editor?.isActive('codeBlock'), language: editor?.getAttributes('codeBlock').language as string | null,
    block: editor?.isActive('heading') ? `h${editor.getAttributes('heading').level}` : 'p', undo: editor?.can().undo(), redo: editor?.can().redo() }) });
  function applyImportedBody(body: RichBody) {
    const target = instance.current;
    if (!target || target.isDestroyed || !target.isEditable || current.current.disabled) throw new Error('The editor is unavailable.');
    const normalized = target.schema.nodeFromJSON(body);
    normalized.check();
    canonicalBody(normalized.toJSON());
    target.commands.setContent(normalized, { emitUpdate: true, errorOnInvalidContent: true });
    if (!target.state.doc.eq(normalized)) throw new Error('The editor rejected this replacement.');
  }
  function openLink() { const target = instance.current; if (!target || current.current.disabled) return; setLink(String(target.getAttributes('link').href ?? '')); setLinkOpen(true); }
  function applyLink() { if (!safeHref(link)) return; editor?.chain().focus().extendMarkRange('link').setLink({ href: link }).run(); setLinkOpen(false); }
  function toolbar(label: string, Icon: typeof Bold, active: boolean | undefined, run: () => void, unavailable = false) {
    return <button type="button" className="document-tool" aria-label={label} title={label} aria-pressed={active === undefined ? undefined : active} disabled={props.disabled || !editor || unavailable} onMouseDown={event => event.preventDefault()} onClick={run}><Icon aria-hidden="true" /></button>;
  }
  return <div className={`document-editor${props.compact ? ' compact' : ''}${props.disabled ? ' is-readonly' : ''}${dragging ? ' is-file-drop' : ''}`} onDragOver={event => { if (uploads.available && event.dataTransfer.types.includes('Files')) { event.preventDefault(); event.dataTransfer.dropEffect = 'copy'; setDragging(true); } }} onDragLeave={event => { if (!event.currentTarget.contains(event.relatedTarget as globalThis.Node | null)) setDragging(false); }} onDrop={event => { setDragging(false); if (!event.dataTransfer.files.length) return; event.preventDefault(); event.stopPropagation(); if (uploads.available) uploads.addBrowserFiles(Array.from(event.dataTransfer.files), prepareAnchor()); }}>
    <div className="document-toolbar" role="toolbar" aria-label={`${props.label} formatting`}>
      <select className="document-block-select" aria-label="Text style" value={state?.block ?? 'p'} disabled={props.disabled || !editor} onChange={event => { if (event.target.value === 'p') editor?.chain().focus().setParagraph().run(); else editor?.chain().focus().setHeading({ level: Number(event.target.value.slice(1)) as 1 | 2 | 3 }).run(); }}><option value="p">Text</option><option value="h1">Heading 1</option><option value="h2">Heading 2</option><option value="h3">Heading 3</option></select>
      <span className="document-toolbar-divider" aria-hidden="true" />
      {toolbar('Bold', Bold, state?.bold, () => editor?.chain().focus().toggleBold().run())}{toolbar('Italic', Italic, state?.italic, () => editor?.chain().focus().toggleItalic().run())}{toolbar('Strikethrough', Strikethrough, state?.strike, () => editor?.chain().focus().toggleStrike().run())}{toolbar('Inline code', Code, state?.code, () => editor?.chain().focus().toggleCode().run())}{toolbar('Insert or edit link', Link2, state?.link, openLink)}
      <span className="document-toolbar-divider" aria-hidden="true" />
      {toolbar('Bullet list', List, state?.bullet, () => editor?.chain().focus().toggleBulletList().run())}{toolbar('Numbered list', ListOrdered, state?.ordered, () => editor?.chain().focus().toggleOrderedList().run())}{toolbar('Task list', ListTodo, state?.task, () => editor?.chain().focus().toggleTaskList().run())}{toolbar('Quote', Quote, state?.quote, () => editor?.chain().focus().toggleBlockquote().run())}{toolbar('Code block', Code, state?.codeBlock, () => editor?.chain().focus().toggleCodeBlock().run())}
      {uploads.offered && toolbar('Attach files', Paperclip, undefined, chooseFiles, !uploads.available)}
      <span className="document-toolbar-spacer" />{toolbar('Undo', Undo2, undefined, () => editor?.chain().focus().undo().run(), !state?.undo)}{toolbar('Redo', Redo2, undefined, () => editor?.chain().focus().redo().run(), !state?.redo)}
    </div>
    {state?.codeBlock && <label className="document-language">Language<input aria-label="Code language" placeholder="Plain text" maxLength={64} value={state.language ?? ''} disabled={props.disabled} onChange={event => { if (/^[a-zA-Z0-9_+#-]*$/.test(event.target.value)) editor?.commands.updateAttributes('codeBlock', { language: event.target.value || null }); }} /></label>}
    <input ref={fileInput} type="file" multiple className="attachment-file-input" aria-label="Choose attachments" tabIndex={-1} onChange={event => { const anchor = pickerAnchor.current ?? prepareAnchor(); pickerAnchor.current = null; uploads.addBrowserFiles(Array.from(event.target.files ?? []), anchor); event.target.value = ''; }} />
    <EditorContent editor={editor} />
    {editor && mention && <MentionPicker key={mentionContext?.key} editor={editor} match={mention} id={mentionList} choices={suggestions.choices} active={mentionActive} loading={suggestions.loading} error={suggestions.error} hasMore={suggestions.hasMore} select={chooseMention} refresh={suggestions.refresh} />}
    {references.popup}
    <UploadQueue rows={uploads.rows} notice={uploads.notice} cancel={uploads.cancel} retry={uploads.retry} reselect={uploads.native ? uploads.reselect : undefined} />
    <div className="document-transfer-actions"><DocumentPortability value={props.value} disabled={props.disabled || uploads.rows.length > 0} onImport={applyImportedBody} /></div>
    {slash !== null && !props.disabled && <div className="document-block-menu" role="listbox" aria-label="Insert a block">{blocks.map((block, index) => <button type="button" role="option" aria-selected={slash === index} key={block.label} onMouseDown={event => event.preventDefault()} onClick={() => { setSlash(null); if (editor) block.run(editor); }}>{block.label}</button>)}</div>}
    {notice && <p className="document-notice" role="alert">{notice}</p>}
    {linkOpen && <Modal title="Link" close={() => setLinkOpen(false)}><div className="settings-modal-content" onKeyDown={event => { if (event.key === 'Enter' && !event.nativeEvent.isComposing) { event.preventDefault(); event.stopPropagation(); applyLink(); } }}><label className="field">URL<input data-autofocus value={link} onChange={event => setLink(event.target.value)} placeholder="https://example.com" /></label>{link && !safeHref(link) && <p className="document-notice">Use http, https or mailto without spaces or embedded passwords.</p>}<div className="modal-footer">{state?.link && <button type="button" className="button" onClick={() => { editor?.chain().focus().extendMarkRange('link').unsetLink().run(); setLinkOpen(false); }}>Remove link</button>}<button type="button" className="button" onClick={() => setLinkOpen(false)}>Cancel</button><button type="button" className="button primary" disabled={!safeHref(link)} onClick={applyLink}>Apply link</button></div></div></Modal>}
  </div>;
}
