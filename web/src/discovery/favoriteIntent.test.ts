import { describe, expect, it } from 'vitest';
import { favoriteDiscoveryUrl, parseFavoriteIntent } from './favoriteIntent';

const workspace = '00000000-0000-4000-8000-000000000001';
const id = 'ABCDEFAB-1234-4567-890A-ABCDEFABCDEF';
const base = `/w/${workspace}/issues`;

describe('favorite discovery intent', () => {
  it.each(['view', 'search'] as const)('round trips the stable %s identity in a base issues link', kind => {
    const url = favoriteDiscoveryUrl(workspace, kind, id);
    expect(url).toBe(`${base}?open_saved=${kind}:${id.toLowerCase()}`);
    expect(parseFavoriteIntent(url, workspace)).toEqual({ state: 'valid', kind, id: id.toLowerCase() });
    expect(parseFavoriteIntent(`${base}?open_saved=${kind}%3A${id}`, workspace)).toEqual({ state: 'valid', kind, id: id.toLowerCase() });
  });
  it('does not treat ordinary issue URLs as favorites', () => {
    expect(parseFavoriteIntent(`${base}?q=work#comment-example`, workspace)).toEqual({ state: 'absent' });
  });
  it.each([
    '?open_saved=', '?open_saved=view:', '?open_saved=search:bad',
    `?open_saved=issue:${id}`, `?open_saved=view:${id}:extra`,
    `?open_saved=view:${id}&open_saved=view:${id}`, `?open_saved=view:${id}&q=work`,
    `?open_saved=view:${id}&extra=`, `?open_saved=view:${id}#comment-example`, `?open_saved=view:${id}#`,
    `?open_saved=view:${id.slice(0, -1)}`, `?open_saved=view:${id}%20`,
  ])('rejects incomplete or ambiguous intent %s', suffix => {
    expect(parseFavoriteIntent(`${base}${suffix}`, workspace).state).toBe('invalid');
  });
  it.each([`${base}/another`, `${base}/`, `/w/another/issues`, `/w/${workspace}/projects`])('rejects intent on a different path %s', path => {
    expect(parseFavoriteIntent(`${path}?open_saved=view:${id}`, workspace).state).toBe('invalid');
  });
});
