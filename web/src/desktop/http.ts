import type { NativeApiResponse, NativeInvoke } from './types';

export class NativeBridgeError extends Error {
  constructor(public readonly code: string, message: string, public readonly retryable: boolean) { super(message); this.name = 'NativeBridgeError'; }
}

export function nativeError(error: unknown): Error {
  if (error instanceof Error) return error;
  if (error && typeof error === 'object' && 'code' in error && 'message' in error && typeof error.code === 'string' && typeof error.message === 'string') {
    if (error.code === 'stale_generation') return new DOMException('This request belongs to a closed connection.', 'AbortError');
    return new NativeBridgeError(error.code, error.message, 'retryable' in error && error.retryable === true);
  }
  return new NativeBridgeError('native_unavailable', 'The desktop connection could not complete this request.', false);
}

const allowedHeaders = new Set(['if-match', 'if-none-match', 'idempotency-key']);
const ignoredHeaders = new Set(['accept', 'content-type', 'x-csrf-token']);
const browserAuthentication = /^\/(?:auth\/(?:login|native(?:\/|$)|recovery(?:\/|$))|setup(?:$|\/)|invitations\/(?:preview|accept)(?:$|\/))/;

// This adapter receives the already serialized shared HTTP request. Rust remains
// authoritative for route, origin, credential, size and timeout validation.
export function createNativeHttp(invoke: NativeInvoke, getGeneration: () => string, getScope: () => string = getGeneration) {
  return async function nativeHttp(path: string, init: RequestInit): Promise<Response> {
    const generation = getGeneration();
    const scope = getScope();
    const signal = init.signal;
    if (signal?.aborted) throw new DOMException('The request was cancelled.', 'AbortError');
    const separator = path.indexOf('?');
    const route = separator < 0 ? path : path.slice(0, separator);
    const query = separator < 0 ? null : path.slice(separator + 1);
    const method = (init.method ?? 'GET').toUpperCase();
    if (!/^\/[a-z]/.test(route) || route.includes('\\') || route.includes('#') || (browserAuthentication.test(route) && !(method === 'GET' && route === '/setup/status'))) throw new NativeBridgeError('native_route_forbidden', 'Open this account action in your browser.', false);
    if (!['GET', 'POST', 'PUT', 'PATCH', 'DELETE'].includes(method)) throw new NativeBridgeError('native_method_forbidden', 'This request method is not available in the desktop app.', false);
    const headers: Record<string, string> = {};
    for (const [name, value] of new Headers(init.headers)) {
      if (allowedHeaders.has(name)) headers[name] = value;
      else if (!ignoredHeaders.has(name)) throw new NativeBridgeError('native_header_forbidden', 'This request cannot be sent by the desktop app.', false);
    }
    let body: unknown = null;
    if (init.body !== undefined && init.body !== null) {
      if (typeof init.body !== 'string') throw new NativeBridgeError('native_body_invalid', 'The desktop app accepts JSON requests only.', false);
      try { body = JSON.parse(init.body); }
      catch { throw new NativeBridgeError('native_body_invalid', 'The request body is not valid JSON.', false); }
    }
    function staleResult(): never {
      if (scope === getScope() && !['GET', 'HEAD'].includes(method) && !signal?.aborted) throw new NativeBridgeError('native_result_uncertain', 'Authentication changed while this request was finishing. Its result is uncertain. Review or retry the original request before making another change.', false);
      throw new DOMException('This response belongs to a closed connection.', 'AbortError');
    }
    let result: NativeApiResponse;
    try {
      result = await invoke<NativeApiResponse>('api_request', { request: { expected_generation: generation, method, path: `/api/v1${route}`, query, headers, body } });
    } catch (error) {
      const failure = nativeError(error);
      // stale_generation is a definite pre-dispatch rejection; stale_response
      // means the native HTTP operation was already sent.
      if (failure instanceof NativeBridgeError && failure.code === 'stale_response') staleResult();
      if (generation !== getGeneration() && !(failure instanceof DOMException && failure.name === 'AbortError')) staleResult();
      throw failure;
    }
    if (generation !== getGeneration() || result.generation !== generation) staleResult();
    // Preserve committed invalidation for the central transport even when its
    // view closed. Native also emits an authoritative signed-out event.
    const responseHeaders = new Headers(result.headers);
    if (signal?.aborted && !(result.status >= 200 && result.status < 300 && responseHeaders.get('X-OpenTrack-Session-Invalidated') === 'true')) throw new DOMException('The request was cancelled.', 'AbortError');
    if (!Number.isInteger(result.status) || result.status < 200 || result.status > 599) throw new NativeBridgeError('native_response_invalid', 'The desktop connection returned an invalid response.', false);
    const noBody = [204, 205, 304].includes(result.status);
    return new Response(noBody ? null : JSON.stringify(result.body), { status: result.status, headers: responseHeaders });
  };
}
