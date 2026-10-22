import { expect, it } from 'vitest';
import { popupPosition } from './placement';
it('opens above a low heading and stays within a narrow pane', () => {
  const bounds = { left: 720, right: 1012, top: 80, bottom: 850 };
  const position = popupPosition({ left: 940, top: 780, bottom: 800 }, bounds, 300, 360);
  expect(position).toEqual({ width: 292, left: 720, top: 414, maxHeight: 360 }); expect(position.left + position.width).toBeLessThanOrEqual(bounds.right);
});
it('fits short mobile viewports without negative height or pushing controls below the window', () => {
  const bounds = { left: 12, right: 308, top: 12, bottom: 268 }; const position = popupPosition({ left: 250, top: 120, bottom: 140 }, bounds);
  expect(position.left).toBe(12); expect(position.top).toBeGreaterThanOrEqual(bounds.top); expect(position.top + position.maxHeight).toBeLessThanOrEqual(bounds.bottom);
});
