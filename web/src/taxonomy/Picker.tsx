import { useEffect, useId, useLayoutEffect, useRef, useState, type CSSProperties, type ReactNode } from 'react';
import { Search } from 'lucide-react';
import { ErrorBanner } from '../components';
import './taxonomy.css';

export interface Choice { id: string; text: string; detail?: string; selected?: boolean; disabled?: boolean; icon?: ReactNode }
export function Picker({ label, value, open, setOpen, query, setQuery, choices, select, multiple = false, loading, error, refresh, more, hasMore, disabled = false, children }: {
  label: string; value: ReactNode; open: boolean; setOpen: (value: boolean) => void; query: string; setQuery: (value: string) => void; choices: Choice[]; select: (id: string) => void; multiple?: boolean; loading: boolean; error: unknown; refresh: () => void; more: () => void; hasMore: boolean; disabled?: boolean; children?: ReactNode;
}) {
  const id = useId(); const wrapper = useRef<HTMLDivElement>(null); const trigger = useRef<HTMLButtonElement>(null); const search = useRef<HTMLInputElement>(null); const composing = useRef(false);
  const [activeId, setActiveId] = useState<string | null>(null); const [placement, setPlacement] = useState<CSSProperties>({});
  const active = activeId === null ? -1 : choices.findIndex(choice => choice.id === activeId);
  useLayoutEffect(() => {
    if (!open || disabled) return;
    const dialog = trigger.current?.closest<HTMLElement>('dialog');
    const pane = trigger.current?.closest<HTMLElement>('.issue-list, .issue-detail');
    // A modal is in the top layer, outside any issue pane that contains it in the DOM.
    const bounds = [dialog, pane && (!dialog || dialog.contains(pane)) ? pane : null]
      .filter((element): element is HTMLElement => !!element);
    function measure() {
      const anchor = trigger.current?.getBoundingClientRect(); const box = wrapper.current?.getBoundingClientRect(); if (!anchor || !box) return;
      let left = 16; let right = window.innerWidth - 16; let top = 16; let bottom = window.innerHeight - 16;
      for (const element of bounds) {
        const rect = element.getBoundingClientRect();
        if (!rect.width) continue;
        // Scroll panes clip absolute children; keep clear of their scrollbar too.
        const contentLeft = rect.left + element.clientLeft;
        left = Math.max(left, contentLeft + 16);
        right = Math.min(right, contentLeft + (element.clientWidth || rect.width) - 16);
        const contentTop = rect.top + element.clientTop;
        top = Math.max(top, contentTop + 16);
        bottom = Math.min(bottom, contentTop + (element.clientHeight || rect.height) - 16);
      }
      const width = Math.min(330, Math.max(0, right - left));
      const below = Math.max(0, bottom - anchor.bottom - 6); const above = Math.max(0, anchor.top - top - 6);
      const flip = below < Math.min(230, above);
      setPlacement({ width, minWidth: 0, left: Math.max(left, Math.min(box.left, right - width)) - box.left, right: 'auto',
        top: (flip ? anchor.top - 6 : anchor.bottom + 6) - box.top, maxHeight: Math.min(390, flip ? above : below), transform: flip ? 'translateY(-100%)' : undefined });
    }
    measure();
    const observer = typeof ResizeObserver === 'undefined' ? undefined : new ResizeObserver(measure);
    bounds.forEach(element => observer?.observe(element));
    if (wrapper.current) observer?.observe(wrapper.current);
    window.addEventListener('resize', measure); window.addEventListener('scroll', measure, true);
    return () => { observer?.disconnect(); window.removeEventListener('resize', measure); window.removeEventListener('scroll', measure, true); };
  }, [open, disabled]);
  useEffect(() => { composing.current = false; if (open && !disabled) search.current?.focus(); }, [open, disabled]);
  useLayoutEffect(() => { setActiveId(null); }, [query]);
  useEffect(() => { if (disabled) setOpen(false); }, [disabled]);
  useEffect(() => {
    if (!open) return;
    const outside = (event: PointerEvent) => { if (!wrapper.current?.contains(event.target as Node)) setOpen(false); };
    document.addEventListener('pointerdown', outside); return () => document.removeEventListener('pointerdown', outside);
  }, [open]);
  function close() { setOpen(false); trigger.current?.focus(); }
  function choose(id: string) { select(id); if (!multiple) close(); else search.current?.focus(); }
  function activate(index: number) { setActiveId(choices[index]?.id ?? null); document.getElementById(`${id}-option-${index}`)?.scrollIntoView?.({ block: 'nearest' }); }
  function move(delta: number) {
    const indices = choices.map((item, index) => item.disabled ? -1 : index).filter(index => index >= 0);
    if (!indices.length) return;
    const at = indices.indexOf(active); const next = at < 0 ? indices[delta > 0 ? 0 : indices.length - 1] : indices[(at + delta + indices.length) % indices.length]; activate(next);
  }
  return <div className="reference-picker" ref={wrapper} onBlur={event => { if (event.relatedTarget && !event.currentTarget.contains(event.relatedTarget)) setOpen(false); }} onKeyDown={event => { if (open && event.key === 'Escape' && !composing.current && !event.nativeEvent.isComposing && event.keyCode !== 229 && !event.altKey && !event.ctrlKey && !event.metaKey && !event.shiftKey && !event.getModifierState('AltGraph')) { event.preventDefault(); event.stopPropagation(); close(); } }}>
    <span className="field-label" id={`${id}-label`}>{label}</span>
    <button ref={trigger} type="button" className="reference-trigger" aria-labelledby={`${id}-label ${id}-value`} aria-haspopup="listbox" aria-expanded={open && !disabled} aria-controls={open ? `${id}-list` : undefined} disabled={disabled} onClick={() => { setQuery(''); setOpen(!open); }}><span id={`${id}-value`}>{value}</span><Search aria-hidden="true" /></button>
    {children}
    {open && !disabled && <div className="reference-popover" style={placement}>
      <label className="reference-search"><Search aria-hidden="true" /><input ref={search} role="combobox" aria-label={`Search ${label.toLowerCase()}`} aria-expanded="true" aria-controls={`${id}-list`} aria-autocomplete="list" aria-activedescendant={active >= 0 && choices[active] && !choices[active].disabled ? `${id}-option-${active}` : undefined} value={query} maxLength={100} onCompositionStart={() => { composing.current = true; }} onCompositionEnd={() => { composing.current = false; }} onChange={event => setQuery(event.target.value)} onKeyDown={event => {
        if (composing.current || event.nativeEvent.isComposing || event.keyCode === 229 || event.altKey || event.ctrlKey || event.metaKey || event.shiftKey || event.getModifierState('AltGraph')) { event.stopPropagation(); return; }
        if (event.key === 'ArrowDown' || event.key === 'ArrowUp') { event.preventDefault(); move(event.key === 'ArrowDown' ? 1 : -1); }
        else if (event.key === 'Enter') { event.preventDefault(); if (!event.repeat && active >= 0 && choices[active] && !choices[active].disabled) choose(choices[active].id); }
        else if (event.key === 'Home' || event.key === 'End') { if (active >= 0 || !query) { event.preventDefault(); activate(event.key === 'Home' ? choices.findIndex(item => !item.disabled) : choices.map((item, index) => item.disabled ? -1 : index).filter(index => index >= 0).at(-1) ?? -1); } }
      }} /></label>
      <ErrorBanner error={error} retry={refresh} />
      <div id={`${id}-list`} role="listbox" aria-label={label} aria-multiselectable={multiple || undefined} aria-busy={loading} className="reference-options">{choices.map((choice, index) => <div role="option" aria-selected={!!choice.selected} aria-disabled={choice.disabled || undefined} id={`${id}-option-${index}`} key={choice.id} className={`reference-option${active === index && !choice.disabled ? ' active' : ''}`} onPointerMove={() => { if (!choice.disabled) setActiveId(choice.id); }} onMouseDown={event => event.preventDefault()} onClick={() => { if (!choice.disabled) choose(choice.id); }}>{choice.icon}<span>{choice.text}{choice.detail && <small>{choice.detail}</small>}</span><span aria-hidden="true">{choice.selected ? '✓' : ''}</span></div>)}</div>
      {loading && <p className="picker-message" role="status">Loading…</p>}{!loading && !error && !choices.length && <p className="picker-message" role="status">No matches.</p>}
      <div className="picker-footer">{hasMore && <button type="button" className="quiet" disabled={loading} onClick={more}>Load more</button>}<button type="button" className="quiet" onClick={close}>Done</button></div>
    </div>}
  </div>;
}
