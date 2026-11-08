import { useEffect, useRef } from 'react';
import { decisionStatuses, riskStatuses, type UpdateNotes } from './updateContext';

interface Note { id: string; title: string; detail?: string; status: string }

function NotesGroup<T extends Note>({ label, items, statuses, initialStatus, change }: { label: 'Risk' | 'Decision'; items: T[]; statuses: Record<T['status'], string>; initialStatus: T['status']; change: (items: T[]) => void }) {
  const container = useRef<HTMLFieldSetElement>(null); const addButton = useRef<HTMLButtonElement>(null); const focusAfterChange = useRef<string | null>(null);
  useEffect(() => {
    const target = focusAfterChange.current; if (!target) return; focusAfterChange.current = null;
    if (target === 'add') addButton.current?.focus(); else container.current?.querySelector<HTMLInputElement>(`[data-note-id="${target}"] input`)?.focus();
  }, [items]);
  const set = (index: number, patch: Partial<T>) => change(items.map((note, at) => at === index ? { ...note, ...patch } : note));
  function add() {
    if (items.length >= 6) return;
    const id = crypto.randomUUID(); focusAfterChange.current = id;
    change([...items, { id, title: '', detail: '', status: initialStatus } as T]);
  }
  function remove(index: number) { focusAfterChange.current = items[index + 1]?.id ?? items[index - 1]?.id ?? 'add'; change(items.filter((_, at) => at !== index)); }
  return <fieldset className="project-update-note-editor" ref={container}><legend>{label === 'Risk' ? 'Risks' : 'Decisions'}</legend>
    <p>Record up to six {label.toLowerCase()}s. This list belongs to this update.</p>
    {items.map((note, index) => <div key={note.id} className="project-update-note-fields" data-note-id={note.id}>
      <div className="project-update-note-heading"><strong>{label} {index + 1}</strong><button type="button" className="quiet" aria-label={`Remove ${label.toLowerCase()} ${index + 1}`} onClick={() => remove(index)}>Remove</button></div>
      <label className="field">Title<input aria-label={`${label} ${index + 1} title`} value={note.title} onChange={event => set(index, { title: event.target.value } as Partial<T>)}/><span className="muted">{Array.from(note.title).length} / 160</span></label>
      <label className="field">Status<select aria-label={`${label} ${index + 1} status`} value={note.status} onChange={event => set(index, { status: event.target.value as T['status'] } as Partial<T>)}>{Object.entries(statuses).map(([value, text]) => <option value={value} key={value}>{text as string}</option>)}</select></label>
      <label className="field">Details<textarea aria-label={`${label} ${index + 1} details`} rows={2} value={note.detail ?? ''} onChange={event => set(index, { detail: event.target.value } as Partial<T>)}/><span className="muted">{Array.from(note.detail ?? '').length} / 1,000</span></label>
    </div>)}
    {!items.length && <p>No {label.toLowerCase()}s recorded in this update.</p>}
    <button ref={addButton} type="button" className="button compact" disabled={items.length >= 6} onClick={add}>Add {label.toLowerCase()}</button>
  </fieldset>;
}

export function ProjectUpdateNotesEditor({ notes, change }: { notes: UpdateNotes; change: (notes: UpdateNotes) => void }) {
  return <div className="project-update-notes-editor">
    <NotesGroup label="Risk" items={notes.risks} statuses={riskStatuses} initialStatus="open" change={risks => change({ ...notes, risks })}/>
    <NotesGroup label="Decision" items={notes.decisions} statuses={decisionStatuses} initialStatus="needed" change={decisions => change({ ...notes, decisions })}/>
  </div>;
}
