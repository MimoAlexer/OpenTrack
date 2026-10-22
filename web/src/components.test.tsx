// @vitest-environment jsdom
import { act, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, expect, it, vi } from 'vitest';
import { Modal } from './components';

afterEach(() => vi.unstubAllGlobals());

it.each([false, true])('restores stacked-dialog focus when the trigger is removed: %s', async removeTrigger => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true);
  // jsdom does not implement the native dialog top layer; DOM focus is real here.
  HTMLDialogElement.prototype.showModal = function () { this.setAttribute('open', ''); };
  HTMLDialogElement.prototype.close = function () { this.removeAttribute('open'); };
  function StackedDialogs() {
    const [top, setTop] = useState(false); const [triggerVisible, setTriggerVisible] = useState(true);
    return <><Modal title="Create workspace" close={() => {}}><input data-autofocus aria-label="Workspace name" defaultValue="Retained draft" />{triggerVisible && <button onClick={() => setTop(true)}>Confirm identity</button>}</Modal>{top && <Modal title="Confirm identity" close={() => setTop(false)}><button data-autofocus onClick={() => { if (removeTrigger) setTriggerVisible(false); setTop(false); }}>Finish</button></Modal>}</>;
  }
  const container = document.createElement('div'); document.body.append(container); const root = createRoot(container);
  try {
    await act(async () => root.render(<StackedDialogs />));
    const trigger = [...container.querySelectorAll('button')].find(button => button.textContent === 'Confirm identity')!;
    trigger.focus(); await act(async () => trigger.click());
    const finish = [...container.querySelectorAll('button')].find(button => button.textContent === 'Finish')!;
    expect(document.activeElement).toBe(finish);
    await act(async () => finish.click());
    expect(document.activeElement).toBe(removeTrigger ? container.querySelector('input') : trigger);
    expect(container.querySelector<HTMLInputElement>('input')?.value).toBe('Retained draft');
    expect(container.querySelectorAll('dialog[open]')).toHaveLength(1);
  } finally { await act(async () => root.unmount()); container.remove(); }
});

it('restores the invoking button after a pointer click that did not focus it', async () => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true);
  HTMLDialogElement.prototype.showModal = function () { this.setAttribute('open', ''); };
  HTMLDialogElement.prototype.close = function () { this.removeAttribute('open'); };
  function Example() {
    const [open, setOpen] = useState(false);
    return <><button onClick={() => setOpen(true)}>Open form</button>{open && <Modal title="Edit example" close={() => setOpen(false)}><input data-autofocus aria-label="Example title" /></Modal>}</>;
  }
  const container = document.createElement('div'); document.body.append(container); const root = createRoot(container);
  try {
    await act(async () => root.render(<Example />));
    const trigger = container.querySelector('button')!;
    await act(async () => { trigger.dispatchEvent(new MouseEvent('pointerdown', { bubbles: true })); trigger.click(); });
    expect(document.activeElement).toBe(container.querySelector('input'));
    await act(async () => container.querySelector<HTMLButtonElement>('[aria-label="Close dialog"]')!.click());
    expect(document.activeElement).toBe(trigger);
  } finally { await act(async () => root.unmount()); container.remove(); }
});
