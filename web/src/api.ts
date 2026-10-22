import { sendRequest } from './platform/transport';
import { NativeBridgeError } from './desktop/http';

export class ApiError extends Error {
  constructor(
    public readonly status: number,
    public readonly code: string,
    message: string,
    public readonly requestId?: string,
    public readonly details?: Record<string, unknown>,
  ) { super(message); this.name = 'ApiError'; }
}

// The write committed. Stop authenticated follow-up work without presenting it
// as a failed write that should be replayed.
export class SessionInvalidatedError extends Error {
  readonly committed = true;
  constructor() { super('Access updated. Sign in again.'); this.name = 'SessionInvalidatedError'; }
}

interface RequestOptions extends Omit<RequestInit, 'body'> {
  body?: unknown;
  csrf?: string;
  timeoutMs?: number;
}

let sessionGeneration = 0;
// Invalid credentials on these public endpoints do not invalidate an existing session.
const publicAuthenticationEndpoints = new Set(['/auth/login', '/auth/recovery/complete', '/setup', '/setup/status', '/invitations/preview']);
// A response from an earlier login must not expire a newer session.
export function advanceSessionGeneration(): void { sessionGeneration += 1; }

export async function request<T>(path: string, options: RequestOptions = {}): Promise<T> {
  const generation = sessionGeneration;
  const { body, csrf, signal, timeoutMs = 30_000, headers: extraHeaders, ...init } = options;
  const headers = new Headers(extraHeaders);
  headers.set('Accept', 'application/json, application/problem+json');
  if (body !== undefined) headers.set('Content-Type', 'application/json');
  if (csrf) headers.set('X-CSRF-Token', csrf);
  const timeout = AbortSignal.timeout(Math.min(180_000, Math.max(1, timeoutMs)));
  let response: Response;
  try {
    response = await sendRequest(path, {
      ...init, headers, credentials: 'same-origin',
      body: body === undefined ? undefined : JSON.stringify(body),
      signal: signal ? AbortSignal.any([signal, timeout]) : timeout,
    });
  } catch (error) {
    if (signal?.aborted || error instanceof DOMException && error.name === 'AbortError' && !timeout.aborted) throw error;
    if (error instanceof NativeBridgeError) {
      const rejectedBeforeDispatch = ['native_route_forbidden', 'native_method_forbidden', 'native_header_forbidden', 'native_body_invalid', 'invalid_request', 'no_connection'].includes(error.code);
      throw new ApiError(rejectedBeforeDispatch ? 400 : 0, error.code, error.message);
    }
    throw new ApiError(0, 'network_error', timeout.aborted
      ? 'The request timed out. Check your connection and try again.'
      : 'Could not reach OpenTrack. Check your connection and try again.');
  }
  if (response.ok && response.headers.get('X-OpenTrack-Session-Invalidated') === 'true') {
    if (generation !== sessionGeneration) throw new DOMException('The request belongs to a closed session.', 'AbortError');
    // Navigation closes a view, not its account session. An observed committed
    // invalidation still clears other views belonging to that same session.
    window.dispatchEvent(new CustomEvent('opentrack:session-expired', { detail: { reason: 'access_changed' } }));
    if (signal?.aborted) throw new DOMException('The view closed after its access change committed.', 'AbortError');
    throw new SessionInvalidatedError();
  }
  const data = response.status === 204 ? undefined : await response.json().catch(() => undefined);
  if (!response.ok) {
    if (response.status === 401 && !publicAuthenticationEndpoints.has(path.split('?')[0]) && generation === sessionGeneration && !signal?.aborted) window.dispatchEvent(new CustomEvent('opentrack:session-expired'));
    throw new ApiError(response.status, data?.code ?? 'request_failed',
      data?.detail ?? `The request failed (${response.status}).`,
      data?.request_id, data?.details);
  }
  if (response.status !== 204 && data === undefined) {
    throw new ApiError(0, 'invalid_response', 'The server returned an unexpected response. Please retry.');
  }
  return data as T;
}

export function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : 'Something went wrong. Please try again.';
}
