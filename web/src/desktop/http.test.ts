import { describe, expect, it, vi } from 'vitest';
import { createNativeHttp } from './http';
import type { NativeInvoke } from './types';

describe('native HTTP boundary', () => {
  it('sends only a relative prefixed route, exact query, allowed headers and JSON with a generation fence', async () => {
    const invoke = vi.fn().mockResolvedValue({ generation: '7', status: 200, headers: { ETag: '"issue:1:2"' }, body: { id: '1' } });
    const http = createNativeHttp(invoke as NativeInvoke, () => '7');
    const response = await http('/workspaces/w/issues?label_ids=a&label_ids=b&cursor=a%2Bb', { method: 'PATCH', headers: { 'If-Match': '"issue:1:1"', 'X-CSRF-Token': 'browser-secret', Accept: 'application/json', 'Content-Type': 'application/json' }, body: JSON.stringify({ title: 'Changed' }) });
    expect(invoke).toHaveBeenCalledWith('api_request', { request: { expected_generation: '7', method: 'PATCH', path: '/api/v1/workspaces/w/issues', query: 'label_ids=a&label_ids=b&cursor=a%2Bb', headers: { 'if-match': '"issue:1:1"' }, body: { title: 'Changed' } } });
    expect(response.headers.get('ETag')).toBe('"issue:1:2"');
    expect(await response.json()).toEqual({ id: '1' });
  });
  it.each(['/auth/login', '/setup', '/auth/recovery/complete', '/invitations/accept', '/invitations/preview', '/auth/native/requests'])('does not dispatch a browser-only action %s', async path => {
    const invoke = vi.fn();
    await expect(createNativeHttp(invoke as NativeInvoke, () => '7')(path, { method: 'POST' })).rejects.toMatchObject({ code: 'native_route_forbidden' });
    expect(invoke).not.toHaveBeenCalled();
  });
  it('rejects an attempt to provide a credential or another origin before IPC dispatch', async () => {
    const invoke = vi.fn(); const http = createNativeHttp(invoke as NativeInvoke, () => '7');
    await expect(http('/auth/session', { headers: { Authorization: 'Bearer not-permitted' } })).rejects.toMatchObject({ code: 'native_header_forbidden' });
    await expect(http('https://other.test/api/v1/auth/session', {})).rejects.toMatchObject({ code: 'native_route_forbidden' });
    await expect(http('//other.test/api/v1/auth/session', {})).rejects.toMatchObject({ code: 'native_route_forbidden' });
    expect(invoke).not.toHaveBeenCalled();
  });
  it('preserves empty responses and RFC9457 status without inventing successful JSON', async () => {
    const invoke = vi.fn().mockResolvedValueOnce({ generation: '7', status: 204, headers: {}, body: null }).mockResolvedValueOnce({ generation: '7', status: 304, headers: { ETag: '"workflow:1:2:true"' }, body: null }).mockResolvedValueOnce({ generation: '7', status: 412, headers: {}, body: { code: 'conflict', detail: 'Review latest.' } });
    const http = createNativeHttp(invoke as NativeInvoke, () => '7');
    expect((await http('/auth/logout', { method: 'POST' })).status).toBe(204);
    expect((await http('/workspaces/w/teams/t/workflow', {})).status).toBe(304);
    const failure = await http('/workspaces/w/issues/i', { method: 'PATCH', body: '{}' });
    expect(failure.status).toBe(412); expect(await failure.json()).toEqual({ code: 'conflict', detail: 'Review latest.' });
  });
  it('drops a late old-server result and does not replay a mutation', async () => {
    let generation = '7'; let resolve!: (value: unknown) => void;
    const invoke = vi.fn().mockImplementation(() => new Promise(done => { resolve = done; }));
    const pending = createNativeHttp(invoke as NativeInvoke, () => generation)('/workspaces/w/issues', { method: 'POST', headers: { 'Idempotency-Key': 'same-key' }, body: '{}' });
    generation = '8'; resolve({ generation: '7', status: 200, headers: {}, body: { id: 'old-server' } });
    await expect(pending).rejects.toMatchObject({ name: 'AbortError' });
    expect(invoke).toHaveBeenCalledTimes(1);
  });
  it('maps native pre-dispatch generation rejection to cancellation', async () => {
    const invoke = vi.fn().mockRejectedValue({ code: 'stale_generation', message: 'Closed connection.', retryable: false });
    await expect(createNativeHttp(invoke as NativeInvoke, () => '7')('/auth/session', {})).rejects.toMatchObject({ name: 'AbortError' });
    expect(invoke).toHaveBeenCalledTimes(1);
  });
  it('does not suppress a same-generation committed invalidation merely because its editor was closed', async () => {
    const abort = new AbortController(); let resolve!: (value: unknown) => void;
    const invoke = vi.fn().mockImplementation(() => new Promise(done => { resolve = done; }));
    const pending = createNativeHttp(invoke as NativeInvoke, () => '7')('/workspaces/w/members/u', { method: 'PATCH', body: '{}', signal: abort.signal });
    abort.abort(); resolve({ generation: '7', status: 200, headers: { 'X-OpenTrack-Session-Invalidated': 'true' }, body: {} });
    expect((await pending).headers.get('X-OpenTrack-Session-Invalidated')).toBe('true');
  });
});

it('preserves uncertainty for an in-flight write when only native authentication rotates', async () => {
  let generation = '7'; let finish!: (value: unknown) => void;
  const invoke = vi.fn().mockImplementation(() => new Promise(resolve => { finish = resolve; }));
  const pending = createNativeHttp(invoke as NativeInvoke, () => generation, () => 'same-account')('/workspaces/w/issues', { method: 'POST', headers: { 'Idempotency-Key': 'original-key' }, body: '{}' });
  generation = '8'; finish({ generation: '7', status: 201, headers: {}, body: { id: 'created' } });
  await expect(pending).rejects.toMatchObject({ code: 'native_result_uncertain' });
  expect(invoke).toHaveBeenCalledTimes(1);
});

it('distinguishes native post-dispatch stale responses from definite pre-dispatch rejection', async () => {
  const invoke = vi.fn().mockRejectedValue({ code: 'stale_response', message: 'Outcome unknown.', retryable: false });
  const http = createNativeHttp(invoke as NativeInvoke, () => '7', () => 'same-account');
  await expect(http('/workspaces/w/issues', { method: 'POST', body: '{}' })).rejects.toMatchObject({ code: 'native_result_uncertain' });
  await expect(http('/workspaces/w/issues', {})).rejects.toMatchObject({ name: 'AbortError' });
});
