// @vitest-environment jsdom
import { act } from 'react';
import { createRoot } from 'react-dom/client';
import type { Editor } from '@tiptap/core';
import { expect, it, vi } from 'vitest';
import { MentionPicker } from './MentionPicker';
import { popupBounds, popupPosition } from './placement';
it('retains popup height during a query refresh without exposing stale selectable results', async () => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true);
  const css = document.createElement('style'); css.textContent = '.mention-picker { border-top:1px solid; border-bottom:1px solid; }'; document.head.append(css);
  const container = document.createElement('div'); document.body.append(container); const root = createRoot(container);
  const height = vi.spyOn(HTMLElement.prototype, 'scrollHeight', 'get').mockImplementation(function(this: HTMLElement) { return this.querySelector('[role=option]') ? 220 : 64; });
  const editor = { isDestroyed: false, view: { coordsAtPos: () => ({ left: 40, top: 40, bottom: 60 }) }, state: { doc: { content: { size: 100 } } } } as unknown as Editor;
  const props = { editor, match: { from: 1, to: 3, query: 'Al' }, id: 'people', active: 0, loading: false, error: null, hasMore: false, select: () => {}, refresh: () => {} };
  try {
    await act(async () => root.render(<MentionPicker {...props} choices={[{id: 'user', display_name: 'First result'}]}/>));
    await act(async () => root.render(<MentionPicker {...props} match={{from:1,to:4,query:'Ale'}} loading choices={[]}/>));
    const popup = container.querySelector<HTMLElement>('.mention-picker')!; expect(popup.style.minHeight).toBe('222px'); expect(container.querySelector('[role=option]')).toBeNull(); expect(container.textContent).toContain('Searching'); expect(container.textContent).not.toContain('No people found');
    await act(async () => root.render(<MentionPicker key="new-scope" {...props} loading choices={[]}/>)); expect(container.querySelector<HTMLElement>('.mention-picker')!.style.minHeight).toBe('0px');
  } finally { await act(async () => root.unmount()); container.remove(); css.remove(); height.mockRestore(); vi.unstubAllGlobals(); }
});

it('excludes pane borders and scrollbar gutters from popup placement', () => {
  vi.stubGlobal('innerWidth', 1280); vi.stubGlobal('innerHeight', 900);
  const pane = document.createElement('section'); pane.className = 'issue-detail'; const popup = document.createElement('div'); pane.append(popup); document.body.append(pane);
  pane.getBoundingClientRect = () => new DOMRect(670.5, 20, 609.5, 880);
  Object.defineProperties(pane, { clientLeft: { value: 1 }, clientTop: { value: 1 }, clientWidth: { value: 594 }, clientHeight: { value: 863 } });
  try { const bounds = popupBounds(popup); expect(bounds).toEqual({left:683.5,right:1253.5,top:33,bottom:872}); const position = popupPosition({left:1150,top:780,bottom:800},bounds,300,286); expect(position.left+position.width).toBe(1253.5); expect(position.top+position.maxHeight).toBeLessThanOrEqual(bounds.bottom); }
  finally { pane.remove(); vi.unstubAllGlobals(); }
});
