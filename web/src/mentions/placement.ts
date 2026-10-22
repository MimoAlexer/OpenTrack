export interface PopupBounds { left: number; right: number; top: number; bottom: number }
export function popupPosition(anchor: { left: number; top: number; bottom: number }, bounds: PopupBounds, preferredWidth = 300, preferredHeight = 280) {
  const width = Math.max(0, Math.min(preferredWidth, bounds.right - bounds.left));
  const below = bounds.bottom - anchor.bottom - 6; const above = anchor.top - bounds.top - 6;
  const upward = below < Math.min(170, preferredHeight) && above > below;
  const maxHeight = Math.max(0, Math.min(preferredHeight, upward ? above : below));
  return { width, maxHeight, left: Math.max(bounds.left, Math.min(anchor.left, bounds.right - width)), top: upward ? Math.max(bounds.top, anchor.top - maxHeight - 6) : Math.max(bounds.top, Math.min(anchor.bottom + 6, bounds.bottom - maxHeight)) };
}
export function popupBounds(element: HTMLElement): PopupBounds {
  let left = 12, right = window.innerWidth - 12, top = 12, bottom = window.innerHeight - 12;
  const dialog = element.closest('dialog'); const pane = element.closest('.issue-detail, .issue-list');
  for (const target of [dialog, pane && (!dialog || dialog.contains(pane)) ? pane : null]) {
    if (!target) continue;
    const box = target.getBoundingClientRect(); if (!box.width) continue;
    const clientLeft = box.left + target.clientLeft; const clientTop = box.top + target.clientTop;
    left = Math.max(left, clientLeft + 12); right = Math.min(right, clientLeft + target.clientWidth - 12);
    top = Math.max(top, clientTop + 12); bottom = Math.min(bottom, clientTop + target.clientHeight - 12);
  }
  return { left, right, top, bottom };
}
export function localPopupPosition(element: HTMLElement, position: ReturnType<typeof popupPosition>) {
  const parent = element.offsetParent;
  if (!(parent instanceof HTMLElement)) return position;
  const box = parent.getBoundingClientRect();
  return { ...position, left: position.left - box.left - parent.clientLeft + parent.scrollLeft, top: position.top - box.top - parent.clientTop + parent.scrollTop };
}

export function popupContentHeight(element: HTMLElement) {
  const style = getComputedStyle(element);
  return element.scrollHeight + (Number.parseFloat(style.borderTopWidth) || 0) + (Number.parseFloat(style.borderBottomWidth) || 0);
}
