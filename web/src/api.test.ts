import { afterEach, describe, expect, it, vi } from 'vitest';
import { advanceSessionGeneration, ApiError, SessionInvalidatedError, request } from './api';

afterEach(() => vi.unstubAllGlobals());

describe('API transport', () => {
  it('includes session credentials and CSRF/idempotency headers on create', async () => {
    const fetch = vi.fn().mockResolvedValue(new Response(JSON.stringify({ id: 'issue-1' }), { status: 201 }));
    vi.stubGlobal('fetch', fetch);
    await request('/workspaces/w/issues', { method: 'POST', body: { title: 'Example' }, csrf: 'csrf-value', headers: { 'Idempotency-Key': 'retry-key' } });
    const [url, options] = fetch.mock.calls[0];
    expect(url).toBe('/api/v1/workspaces/w/issues');
    expect(options.credentials).toBe('same-origin');
    expect(options.headers.get('X-CSRF-Token')).toBe('csrf-value');
    expect(options.headers.get('Idempotency-Key')).toBe('retry-key');
    expect(JSON.parse(options.body)).toEqual({ title: 'Example' });
  });
  it('keeps conflict details and request ID available to the comparison UI', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response(JSON.stringify({ code: 'version_conflict', detail: 'Changed', request_id: 'request-1', details: { current: { id: 'issue-1', version: '2' } } }), { status: 412 })));
    await expect(request('/issues/issue-1')).rejects.toMatchObject({ status: 412, code: 'version_conflict', requestId: 'request-1', details: { current: { version: '2' } } });
  });
  it('reports an ambiguous network failure without automatically retrying a write', async () => {
    const fetch = vi.fn().mockRejectedValue(new TypeError('Failed to fetch'));
    vi.stubGlobal('fetch', fetch);
    await expect(request('/issues/issue-1', { method: 'PATCH' })).rejects.toBeInstanceOf(ApiError);
    expect(fetch).toHaveBeenCalledTimes(1);
  });
  it('passes the server-issued issue validator without reconstruction', async () => {
    const fetch = vi.fn().mockResolvedValue(new Response('{}')); vi.stubGlobal('fetch', fetch);
    const etag = '"issue:issue-1:15:reference-digest"';
    await request('/workspaces/w/issues/issue-1', { method: 'PATCH', headers: { 'If-Match': etag }, body: { title: 'Edit' } });
    expect(fetch.mock.calls[0][1].headers.get('If-Match')).toBe(etag);
  });
  it('accepts successful empty logout responses', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response(null, { status: 204 })));
    await expect(request('/auth/logout', { method: 'POST' })).resolves.toBeUndefined();
  });
  it('preserves an existing session when password confirmation is incorrect', async () => {
    const session = { user: { id: 'u', email: 'owner@example.test' }, csrf_token: 'existing-csrf' };
    const dispatchEvent = vi.fn();
    vi.stubGlobal('window', { dispatchEvent });
    const fetch = vi.fn()
      .mockResolvedValueOnce(new Response(JSON.stringify(session)))
      .mockResolvedValueOnce(new Response(JSON.stringify({ code: 'invalid_credentials', detail: 'Email or password is incorrect.' }), { status: 401 }))
      .mockResolvedValueOnce(new Response(JSON.stringify(session)));
    vi.stubGlobal('fetch', fetch);
    await expect(request('/auth/session')).resolves.toEqual(session);
    advanceSessionGeneration();
    await expect(request('/auth/login', { method: 'POST', body: { email: session.user.email, password: 'incorrect-password' } })).rejects.toMatchObject({ status: 401, code: 'invalid_credentials' });
    expect(dispatchEvent).not.toHaveBeenCalled();
    await expect(request('/auth/session')).resolves.toEqual(session);
  });
  it.each(['/setup', '/setup/status', '/invitations/preview', '/auth/recovery/complete'])('keeps a public %s failure local', async path => {
    const dispatchEvent = vi.fn();
    vi.stubGlobal('window', { dispatchEvent });
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response(JSON.stringify({ code: 'unauthenticated', detail: 'Invalid token.' }), { status: 401 })));
    await expect(request(path, { method: path === '/setup/status' ? 'GET' : 'POST' })).rejects.toMatchObject({ status: 401 });
    expect(dispatchEvent).not.toHaveBeenCalled();
  });
  it.each(['/auth/session', '/workspaces/w/issues', '/invitations/accept'])('expires the current session on a protected %s failure', async path => {
    const dispatchEvent = vi.fn();
    vi.stubGlobal('window', { dispatchEvent });
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response(JSON.stringify({ code: 'unauthenticated', detail: 'Session expired.' }), { status: 401 })));
    await expect(request(path, { method: path === '/invitations/accept' ? 'POST' : 'GET', csrf: 'existing-csrf' })).rejects.toMatchObject({ status: 401 });
    expect(dispatchEvent).toHaveBeenCalledTimes(1);
    expect(dispatchEvent.mock.calls[0][0].type).toBe('opentrack:session-expired');
  });
  it('does not expire a newer login when an old request returns 401 late', async () => {
    let finish!: (response: Response) => void;
    const dispatchEvent = vi.fn();
    vi.stubGlobal('window', { dispatchEvent });
    vi.stubGlobal('fetch', vi.fn(() => new Promise<Response>(resolve => { finish = resolve; })));
    const pending = request('/workspaces/old/issues/old/activity');
    const rejection = expect(pending).rejects.toMatchObject({ status: 401 });
    advanceSessionGeneration();
    finish(new Response(JSON.stringify({ code: 'unauthenticated', detail: 'Expired' }), { status: 401 }));
    await rejection;
    expect(dispatchEvent).not.toHaveBeenCalled();
  });
  it('ends the current session after a committed self-elevation and stops follow-up writes', async () => {
    const dispatchEvent = vi.fn(); const followUp = vi.fn();
    vi.stubGlobal('window', { dispatchEvent });
    const fetch = vi.fn().mockResolvedValue(new Response(JSON.stringify({ role: 'maintainer' }), { headers: { 'X-OpenTrack-Session-Invalidated': 'true' } }));
    vi.stubGlobal('fetch', fetch);
    const pending = request('/workspaces/w/teams/t/members/u', { method: 'PUT' }).then(followUp);
    await expect(pending).rejects.toBeInstanceOf(SessionInvalidatedError);
    expect(followUp).not.toHaveBeenCalled(); expect(fetch).toHaveBeenCalledTimes(1);
    expect(dispatchEvent).toHaveBeenCalledTimes(1);
    expect(dispatchEvent.mock.calls[0][0].detail).toEqual({ reason: 'access_changed' });
  });
  it.each(['newer-login', 'closed-view'])('handles a late committed-invalidation signal after %s', async scenario => {
    let finish!: (response: Response) => void;
    const dispatchEvent = vi.fn(); const abort = new AbortController();
    vi.stubGlobal('window', { dispatchEvent });
    vi.stubGlobal('fetch', vi.fn(() => new Promise<Response>(resolve => { finish = resolve; })));
    const pending = request('/workspaces/w/teams/t/members/u', { method: 'PUT', signal: abort.signal });
    const rejection = expect(pending).rejects.toMatchObject({ name: 'AbortError' });
    if (scenario === 'newer-login') advanceSessionGeneration(); else abort.abort();
    finish(new Response('{}', { headers: { 'X-OpenTrack-Session-Invalidated': 'true' } }));
    await rejection;
    if (scenario === 'newer-login') expect(dispatchEvent).not.toHaveBeenCalled();
    else {
      expect(dispatchEvent).toHaveBeenCalledTimes(1);
      expect(dispatchEvent.mock.calls[0][0].detail).toEqual({ reason: 'access_changed' });
    }
  });
  it('returns another account’s successful access change without ending this session', async () => {
    const dispatchEvent = vi.fn(); vi.stubGlobal('window', { dispatchEvent });
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response(JSON.stringify({ role: 'member' }))));
    await expect(request('/workspaces/w/members/other', { method: 'PATCH' })).resolves.toEqual({ role: 'member' });
    expect(dispatchEvent).not.toHaveBeenCalled();
  });
});
