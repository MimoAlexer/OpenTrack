import { uuid } from '../filters/model';

export type FavoriteKind = 'view' | 'search';
export type FavoriteIntent = { state: 'absent' } | { state: 'invalid'; error: string } | { state: 'valid'; kind: FavoriteKind; id: string };

export function favoriteDiscoveryUrl(workspaceId: string, kind: FavoriteKind, id: string): string {
  if (!uuid(id) || !['view', 'search'].includes(kind)) throw new Error('A favorite requires a saved view or search UUID.');
  return `/w/${encodeURIComponent(workspaceId)}/issues?open_saved=${kind}:${id.toLowerCase()}`;
}

export function parseFavoriteIntent(url: string, workspaceId: string): FavoriteIntent {
  const invalid: FavoriteIntent = { state: 'invalid', error: 'This saved discovery link is invalid. Open All issues or choose a favorite again.' };
  let parsed: URL;
  try { parsed = new URL(url, 'https://opentrack.invalid'); } catch { return invalid; }
  if (!parsed.searchParams.has('open_saved')) return { state: 'absent' };
  if (parsed.pathname !== `/w/${encodeURIComponent(workspaceId)}/issues` || url.includes('#') || [...parsed.searchParams].length !== 1) return invalid;
  const match = /^(view|search):(.+)$/.exec(parsed.searchParams.get('open_saved') ?? '');
  if (!match || !uuid(match[2])) return invalid;
  return { state: 'valid', kind: match[1] as FavoriteKind, id: match[2].toLowerCase() };
}
