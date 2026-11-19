import { useId } from 'react';
import { errorMessage } from '../api';
import { Link } from '../router';
import { favoriteDiscoveryUrl } from './favoriteIntent';
import type { FavoriteDiscoveries, FavoriteGroup, FavoriteKind } from './useFavoriteDiscoveries';
import './favorites.css';

export function SidebarFavorites({ workspaceId, favorites }: { workspaceId: string; favorites: FavoriteDiscoveries }) {
  const heading = useId();
  if (!favorites.enabled) return null;
  return <nav className="sidebar-favorites" aria-label="Favorites">
    <h2 id={heading}>Favorites</h2>
    <button type="button" className="favorites-refresh" onClick={favorites.refresh}>Refresh favorites</button>
    <FavoriteSection workspaceId={workspaceId} kind="view" group={favorites.views}/>
    <FavoriteSection workspaceId={workspaceId} kind="search" group={favorites.searches}/>
  </nav>;
}
function FavoriteSection({ workspaceId, kind, group }: { workspaceId: string; kind: FavoriteKind; group: FavoriteGroup }) {
  const label = kind === 'view' ? 'views' : 'searches';
  const heading = useId();
  return <section aria-label={`Favorite ${label}`} aria-busy={group.busy}>
    <h3 id={heading}>{kind === 'view' ? 'Views' : 'Searches'}</h3>
    {group.busy && <p role="status">Loading favorite {label}…</p>}
    {!!group.error && <div className="favorites-error"><p role="alert">{errorMessage(group.error)}</p><button type="button" onClick={group.retry} aria-label={`Retry favorite ${label}`}>Retry</button></div>}
    {!group.busy && !group.error && !group.items.length && <p className="favorites-empty">No favorite {label}.</p>}
    <ul>{group.items.map(item => <li key={item.id}><Link className="nav-item favorite-link" href={favoriteDiscoveryUrl(workspaceId, kind, item.id)}>
      <span className="favorite-name">{item.name}</span>
      <span className="favorite-kind">{kind === 'view' ? 'View' : 'Search'}{item.repair_required ? ' · Needs repair' : ''}</span>
    </Link></li>)}</ul>
    {(group.canPrevious || group.hasMore) && <div className="favorites-pages">
      <button type="button" disabled={group.busy || !group.canPrevious} aria-label={`Previous favorite ${label}`} onClick={group.previous}>Previous</button>
      <button type="button" disabled={group.busy || !group.hasMore} aria-label={`Next favorite ${label}`} onClick={group.next}>Next</button>
    </div>}
  </section>;
}
