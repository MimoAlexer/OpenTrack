import { describe, expect, it } from 'vitest';
import { nativeCallbackUri, nativeRequestId } from './native-callback';

const id = '01234567-89ab-4cde-8fab-0123456789ab';
const query = `request_id=${id}&code=${'a'.repeat(64)}&state=${'b'.repeat(64)}`;
const callback = `http://127.0.0.1:42123/opentrack/native-callback?${query}`;

describe('native browser callback boundary', () => {
  it('accepts a canonical request ID only once', () => {
    expect(nativeRequestId(`?request_id=${id}`)).toBe(id);
    for (const value of ['', '?request_id=bad', `?request_id=${id}&request_id=${id}`, `?request_id=${id}&server=https://other.test`]) expect(nativeRequestId(value)).toBeNull();
  });
  it('keeps the exact validated IPv4 or IPv6 loopback destination', () => {
    expect(nativeCallbackUri(callback, id)).toBe(callback);
    const ipv6 = callback.replace('127.0.0.1', '[::1]');
    expect(nativeCallbackUri(ipv6, id)).toBe(ipv6);
  });
  it.each([
    callback.replace('127.0.0.1', 'localhost'),
    callback.replace('127.0.0.1', '127.1'),
    callback.replace('127.0.0.1', '127.0.0.2'),
    callback.replace('127.0.0.1', 'user@127.0.0.1'),
    callback.replace('42123', '042123'),
    callback.replace('42123', '80'),
    callback.replace('http:', 'https:'),
    callback.replace('/opentrack/native-callback', '/other'),
    callback.replace('/opentrack/native-callback', '/opentrack/../opentrack/native-callback'),
    `${callback}&request_id=${id}`,
    `${callback}&next=https://other.test`,
    `${callback}#`,
    `${callback}#fragment`,
    callback.replace('a'.repeat(64), 'invalid'),
    callback.replace('b'.repeat(64), 'invalid'),
  ])('rejects a callback outside the exact handoff contract', value => {
    expect(nativeCallbackUri(value, id)).toBeNull();
  });
  it('rejects a valid callback for a different request', () => {
    expect(nativeCallbackUri(callback, id.replace('01234567', '11234567'))).toBeNull();
  });
});
