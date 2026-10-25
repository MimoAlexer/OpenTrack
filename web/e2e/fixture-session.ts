import type { Session } from '../src/types';

export function selectSyntheticWorkspace(session: Session) {
  const matches = session.workspaces.filter(workspace => workspace.name === 'Browser acceptance');
  if (session.user.email !== 'owner@example.test' || session.user.display_name !== 'Example Owner' || matches.length !== 1) {
    throw new Error('Only the synthetic acceptance workspace is eligible.');
  }
  return matches[0];
}
