import { expect, it } from 'vitest';
import type { Session } from './types';
import { selectSyntheticWorkspace } from '../e2e/fixture-session';

const canonical = { id: 'canonical', name: 'Browser acceptance', slug: 'canonical', role: 'owner' as const };
const earlier = { id: 'earlier', name: 'A prior workspace', slug: 'earlier', role: 'owner' as const };
function session(workspaces: Session['workspaces']): Session {
  return { audience: 'web', user: { id: 'owner', email: 'owner@example.test', display_name: 'Example Owner' }, csrf_token: 'fixture-csrf', workspaces };
}

it('selects the unique synthetic workspace without depending on or changing server order', () => {
  const value = session([earlier, canonical]);
  const original = structuredClone(value);
  expect(selectSyntheticWorkspace(value)).toBe(canonical);
  expect(value).toEqual(original);
  expect(value.workspaces[0]).toBe(earlier);
});

it.each([
  { kind: 'empty', workspaces: [] },
  { kind: 'missing', workspaces: [earlier] },
  { kind: 'ambiguous', workspaces: [canonical, { ...canonical, id: 'duplicate' }] },
])('rejects a $kind canonical workspace', ({ workspaces }) => {
  expect(() => selectSyntheticWorkspace(session(workspaces))).toThrow('Only the synthetic acceptance workspace is eligible.');
});

it.each(['email', 'display_name'] as const)('rejects a different synthetic-owner %s', field => {
  const value = session([canonical]);
  value.user[field] = field === 'email' ? 'other@example.test' : 'Example Member';
  expect(() => selectSyntheticWorkspace(value)).toThrow('Only the synthetic acceptance workspace is eligible.');
});
