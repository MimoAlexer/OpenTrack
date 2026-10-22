// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { useWorkspaceAccess } from './useWorkspaceAccess';

let container: HTMLDivElement; let root: Root;
const json = (body: unknown, status = 200) => new Response(JSON.stringify(body), { status });
beforeEach(() => { vi.useFakeTimers(); vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true); container = document.createElement('div'); document.body.append(container); root = createRoot(container); });
afterEach(async () => { await act(async () => root.unmount()); container.remove(); vi.useRealTimers(); vi.restoreAllMocks(); vi.unstubAllGlobals(); });
function Probe({ epoch, invalidate, scope = 'session-one' }: { epoch: string | null; invalidate: (revoked: boolean) => Promise<void>; scope?: string }) { useWorkspaceAccess('w', scope, epoch, invalidate); return null; }

it('invalidates immediately once per observed epoch without repeatedly restarting a slow bootstrap', async () => {
  let serverEpoch = '2'; const invalidate = vi.fn(async () => {});
  const fetch = vi.fn(async () => json({ auth_epoch: serverEpoch, role: 'member', archived: false })); vi.stubGlobal('fetch', fetch);
  await act(async () => root.render(<Probe epoch="1" invalidate={invalidate} />));
  expect(invalidate).toHaveBeenCalledExactlyOnceWith(false);
  await act(async () => vi.advanceTimersByTimeAsync(6_000));
  expect(fetch).toHaveBeenCalledTimes(4); expect(invalidate).toHaveBeenCalledOnce();
  serverEpoch = '3'; await act(async () => vi.advanceTimersByTimeAsync(2_000));
  expect(invalidate).toHaveBeenCalledTimes(2);
  await act(async () => root.render(<Probe epoch="3" invalidate={invalidate} />));
  await act(async () => vi.advanceTimersByTimeAsync(2_000));
  expect(invalidate).toHaveBeenCalledTimes(2);
});

it('continues checking after an unsuccessful refresh and still clears revoked access', async () => {
  let revoked = false;
  vi.stubGlobal('fetch', vi.fn(async () => revoked ? json({ code: 'not_found', detail: 'Unavailable.' }, 404) : json({ auth_epoch: '2', role: 'member', archived: false })));
  const invalidate = vi.fn(async (isRevoked: boolean) => { if (!isRevoked) throw new Error('Bootstrap failed'); });
  await act(async () => root.render(<Probe epoch="1" invalidate={invalidate} />));
  await act(async () => vi.advanceTimersByTimeAsync(2_000));
  expect(invalidate).toHaveBeenCalledOnce();
  revoked = true; await act(async () => vi.advanceTimersByTimeAsync(2_000));
  expect(invalidate.mock.calls).toEqual([[false], [true]]);
  await act(async () => vi.advanceTimersByTimeAsync(4_000));
  expect(invalidate).toHaveBeenCalledTimes(2);
});
