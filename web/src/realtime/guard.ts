const scopes = new Map<symbol, { workspaceId: string; generation: number; blocked: boolean }>();
export function workspaceWriteGuard(workspaceId: string, generation: number) {
  const token = Symbol(); const state = { workspaceId, generation, blocked: false }; scopes.set(token, state);
  return { set: (blocked: boolean) => { state.blocked = blocked; }, release: () => { scopes.delete(token); } };
}
export function realtimeWriteBlocked(path: string, method: string, generation: number): boolean {
  if (['GET', 'HEAD', 'OPTIONS'].includes(method)) return false;
  const match = /^\/workspaces\/([^/?]+)(?:\/|$)/.exec(path);
  if (!match) return false;
  // Structured filtering is an authenticated read with a JSON query body.
  if (method === 'POST' && path.split('?')[0] === `/workspaces/${match[1]}/issues/query`) return false;
  return [...scopes.values()].some(scope => scope.workspaceId === match[1] && scope.generation === generation && scope.blocked);
}
