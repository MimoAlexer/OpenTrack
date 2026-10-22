// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { useCreateRequest } from '../settings/shared';
import { installTransport } from '../platform/transport';
import { createNativeHttp } from './http';
import type { NativeInvoke } from './types';

let container: HTMLDivElement; let root: Root; let release: (() => void) | undefined;
beforeEach(() => { vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true); container = document.createElement('div'); document.body.append(container); root = createRoot(container); });
afterEach(async () => { await act(async () => root.unmount()); release?.(); release = undefined; container.remove(); vi.restoreAllMocks(); vi.unstubAllGlobals(); });
function Create() { const command = useCreateRequest('/workspaces', ''); return <><span>{command.uncertain ? 'Outcome uncertain' : 'Ready'}</span><button disabled={command.busy} onClick={() => void command.submit({ name: 'Draft workspace' })}>Create or retry</button></>; }

it('reuses the original creation body and key after a same-account authentication rotation hides its response', async () => {
  let generation = '1'; let finish!: (value: unknown) => void;
  const invoke = vi.fn().mockImplementationOnce(() => new Promise(resolve => { finish = resolve; })).mockImplementation(async () => ({ generation, status: 201, headers: {}, body: { id: 'workspace' } }));
  release = installTransport(createNativeHttp(invoke as NativeInvoke, () => generation, () => 'same-account'));
  await act(async () => root.render(<Create />));
  await act(async () => container.querySelector('button')!.click());
  await act(async () => { generation = '2'; finish({ generation: '1', status: 201, headers: {}, body: { id: 'workspace' } }); });
  expect(container.textContent).toContain('Outcome uncertain'); expect(invoke).toHaveBeenCalledOnce();
  await act(async () => container.querySelector('button')!.click());
  const first = invoke.mock.calls[0][1].request; const second = invoke.mock.calls[1][1].request;
  expect(second.headers['idempotency-key']).toBe(first.headers['idempotency-key']); expect(second.body).toEqual(first.body);
  expect(second.expected_generation).toBe('2'); expect(container.textContent).toContain('Ready');
});
