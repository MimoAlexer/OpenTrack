// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { Picker } from './Picker';

let container: HTMLDivElement; let root: Root;
let paneRect: DOMRect; let wrapperRect: DOMRect; let dialogRect: DOMRect;
let resizePane: () => void;
const disconnect = vi.fn();

beforeEach(() => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true);
  vi.stubGlobal('innerWidth', 1360);
  paneRect = new DOMRect(220, 0, 400, 800);
  wrapperRect = new DOMRect(390, 100, 150, 60);
  dialogRect = new DOMRect(400, 80, 560, 600);
  disconnect.mockClear();
  vi.stubGlobal('ResizeObserver', class {
    constructor(callback: () => void) { resizePane = callback; }
    observe() {}
    disconnect = disconnect;
  });
  vi.spyOn(Element.prototype, 'getBoundingClientRect').mockImplementation(function (this: Element) {
    if (this.matches('.issue-list, .issue-detail')) return paneRect;
    if (this.matches('dialog')) return dialogRect;
    if (this.matches('.reference-picker')) return wrapperRect;
    if (this.matches('.reference-trigger')) return new DOMRect(wrapperRect.x, wrapperRect.y + 20, wrapperRect.width, 30);
    return new DOMRect();
  });
  container = document.createElement('div'); document.body.append(container); root = createRoot(container);
});

afterEach(async () => {
  await act(async () => root.unmount()); container.remove(); vi.restoreAllMocks(); vi.unstubAllGlobals();
});

async function render(pane: 'issue-list' | 'issue-detail', inDialog = false) {
  const picker = <Picker label="Labels" value="Select labels" open setOpen={() => {}} query="" setQuery={() => {}} choices={[]} select={() => {}} loading={false} error={null} refresh={() => {}} more={() => {}} hasMore={false} />;
  await act(async () => root.render(<div className={pane}>{inDialog ? <dialog open>{picker}</dialog> : picker}</div>));
}

function position() {
  const style = container.querySelector<HTMLElement>('.reference-popover')!.style;
  const left = wrapperRect.left + Number.parseFloat(style.left); const width = Number.parseFloat(style.width);
  return { left, width, right: left + width };
}

it('keeps a filter picker inside its scrolling issue list, including the scrollbar gutter', async () => {
  await render('issue-list');
  expect(position()).toEqual({ left: 274, width: 330, right: 604 });
  Object.defineProperty(container.querySelector('.issue-list'), 'clientWidth', { configurable: true, value: 385 });
  await act(async () => resizePane());
  expect(position()).toEqual({ left: 259, width: 330, right: 589 });
  expect(document.activeElement).toBe(container.querySelector('[role=combobox]'));
});

it('shrinks a picker below its old minimum when the detail pane becomes narrow', async () => {
  await render('issue-detail');
  paneRect = new DOMRect(220, 0, 190, 800);
  wrapperRect = new DOMRect(330, 100, 64, 60);
  await act(async () => resizePane());
  expect(position()).toEqual({ left: 236, width: 158, right: 394 });
  expect(container.querySelector<HTMLElement>('.reference-popover')!.style.right).toBe('auto');
});

it('bounds a modal picker by the viewport and dialog, not the pane behind the modal', async () => {
  vi.stubGlobal('innerWidth', 640);
  paneRect = new DOMRect(880, 0, 480, 800);
  dialogRect = new DOMRect(100, 80, 600, 600);
  wrapperRect = new DOMRect(480, 180, 160, 60);
  await render('issue-detail', true);
  expect(position()).toEqual({ left: 294, width: 330, right: 624 });
  vi.stubGlobal('innerWidth', 400);
  await act(async () => window.dispatchEvent(new Event('resize')));
  expect(position()).toEqual({ left: 116, width: 268, right: 384 });
});

it('repositions after pane scrolling and releases observation on close', async () => {
  await render('issue-list');
  wrapperRect = new DOMRect(210, 100, 150, 60);
  await act(async () => container.querySelector('.issue-list')!.dispatchEvent(new Event('scroll')));
  expect(position()).toEqual({ left: 236, width: 330, right: 566 });
  await act(async () => root.render(null));
  expect(disconnect).toHaveBeenCalledOnce();
});
