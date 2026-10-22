// @vitest-environment jsdom
import { afterEach, expect, it, vi } from 'vitest';
const native = vi.hoisted(() => ({ invoke: vi.fn(), isTauri: vi.fn(() => false) }));
vi.mock('@tauri-apps/api/core', () => native);
import { openExternalDocumentLink } from './bridge';
afterEach(() => { vi.restoreAllMocks(); native.invoke.mockReset(); native.isTauri.mockReset().mockReturnValue(false); });

it('opens explicit browser links without opener access or a referrer', async () => {
  const open=vi.spyOn(window,'open').mockReturnValue(null);
  await openExternalDocumentLink('https://example.com/document');
  expect(open).toHaveBeenCalledWith('https://example.com/document','_blank','noopener,noreferrer');
  expect(native.invoke).not.toHaveBeenCalled();
});
it('uses the narrow native command without sending authentication state', async () => {
  native.isTauri.mockReturnValue(true);
  await openExternalDocumentLink('mailto:team@example.com');
  expect(native.invoke).toHaveBeenCalledExactlyOnceWith('open_document_link',{request:{href:'mailto:team@example.com'}});
});
it.each(['javascript:alert(1)','file:///etc/passwd','data:text/html,x','https://name:password@example.com','mailto:team@example.com?body=private','https://example.com\\@other.test','https://example.com\n'])('rejects unsafe destinations before native dispatch: %s', async href => {
  native.isTauri.mockReturnValue(true);
  await expect(openExternalDocumentLink(href)).rejects.toThrow('This link is not supported.');
  expect(native.invoke).not.toHaveBeenCalled();
});
