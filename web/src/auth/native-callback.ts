const uuid = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;

export function nativeRequestId(search: string): string | null {
  const params = new URLSearchParams(search);
  const id = params.get('request_id');
  return id && uuid.test(id) && [...params.keys()].length === 1 ? id : null;
}

// The API owns the destination. This checks its narrow shape before navigation;
// native Rust independently validates the pending state and proof of possession.
export function nativeCallbackUri(value: string, requestId: string): string | null {
  if (value.length > 1024 || !/^http:\/\/(127\.0\.0\.1|\[::1\]):[0-9]+\//.test(value)) return null;
  try {
    const uri = new URL(value);
    const port = Number(uri.port);
    if (uri.href !== value || value.includes('#') || uri.protocol !== 'http:' || !['127.0.0.1', '[::1]'].includes(uri.hostname) || port < 1024 || port > 65535 || uri.username || uri.password || uri.pathname !== '/opentrack/native-callback') return null;
    const params = uri.searchParams;
    const keys = [...params.keys()];
    if (keys.length !== 3 || !['request_id', 'code', 'state'].every(key => params.getAll(key).length === 1)) return null;
    if (!uuid.test(requestId) || params.get('request_id') !== requestId || !/^[0-9a-f]{64}$/.test(params.get('code') ?? '') || !/^[0-9a-f]{64}$/.test(params.get('state') ?? '')) return null;
    return value;
  } catch { return null; }
}
