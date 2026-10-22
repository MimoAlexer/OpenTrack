// @vitest-environment jsdom
import { act } from 'react';
import { createRoot,type Root } from 'react-dom/client';
import { afterEach,beforeEach,expect,it,vi } from 'vitest';
import { DiscardUploadDraft } from './DiscardUploadDraft';
let container:HTMLDivElement;let root:Root;
beforeEach(()=>{vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT',true);HTMLDialogElement.prototype.showModal=function(){this.setAttribute('open','');};HTMLDialogElement.prototype.close=function(){this.removeAttribute('open');};container=document.createElement('div');document.body.append(container);root=createRoot(container);});
afterEach(async()=>{await act(async()=>root.unmount());container.remove();vi.unstubAllGlobals();});
it('shows and guards cancellation, retains the draft on failure, and retries within the dialog',async()=>{
 let finish!:(value:boolean)=>void;const cancelAll=vi.fn(()=>new Promise<boolean>(resolve=>{finish=resolve;}));const discard=vi.fn();const close=vi.fn();await act(async()=>root.render(<DiscardUploadDraft title="Discard draft?" description="Your draft will be discarded." action="Discard draft" uploads={{pending:true,busy:false,cancelAll}} close={close} discard={discard}/>));
 const action=()=>container.querySelector<HTMLButtonElement>('.button.primary')!;await act(async()=>{action().click();action().click();});expect(cancelAll).toHaveBeenCalledOnce();expect(action().disabled).toBe(true);expect(container.querySelector('[role=status]')?.textContent).toBe('Canceling uploads…');expect([...container.querySelectorAll<HTMLButtonElement>('button')].filter(button=>button.textContent==='Keep editing').every(button=>button.disabled)).toBe(true);
 await act(async()=>finish(false));expect(discard).not.toHaveBeenCalled();expect(container.querySelector('[role=alert]')?.textContent).toContain('Your draft is still here');expect(action().textContent).toBe('Retry cancellation');expect(action().disabled).toBe(false);
 await act(async()=>action().click());await act(async()=>finish(true));expect(discard).toHaveBeenCalledOnce();expect(close).not.toHaveBeenCalled();
});
