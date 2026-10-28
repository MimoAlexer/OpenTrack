import { useEffect, useState } from 'react';
import { useSessionScope } from '../desktop/context';
import { isAccessError } from '../documents/model';
import { projectPath, type ProjectSummary } from '../projects/types';
import { useProjectPage } from '../projects/useProjectData';
import type { Session } from '../types';

export type ProjectChoice = Pick<ProjectSummary, 'id' | 'etag'>;
export const projectSignature = (scope: string, workspace: string, team: string, value: ProjectChoice) => JSON.stringify([scope, workspace, team, value.id, value.etag]);

export function ProjectPicker({ workspaceId, teamId, session, value, disabled, label, clearNeeded = false, conceal = false, versionRequired = true, reviewNeeded = false, refreshKey = 0, accessChanged, verified, change }: {
  workspaceId: string; teamId: string; session: Session; value: ProjectChoice | null; disabled: boolean; label: string; clearNeeded?: boolean; conceal?: boolean; versionRequired?: boolean; reviewNeeded?: boolean; refreshKey?: number; accessChanged?: (denied: boolean) => void; verified?: (signature: string | null) => void; change: (value: ProjectChoice | null) => void;
}) {
  const scope = useSessionScope(session);
  return <ScopedProjectPicker key={`${workspaceId}:${teamId}:${scope}`} {...{ workspaceId, teamId, scope, value, disabled, label, clearNeeded, conceal, versionRequired, reviewNeeded, refreshKey, accessChanged, verified, change }}/>;
}

function ScopedProjectPicker({ workspaceId, teamId, scope, value, disabled, label, clearNeeded, conceal, versionRequired, reviewNeeded, refreshKey, accessChanged, verified, change }: {
  workspaceId: string; teamId: string; scope: string; value: ProjectChoice | null; disabled: boolean; label: string; clearNeeded: boolean; conceal: boolean; versionRequired: boolean; reviewNeeded: boolean; refreshKey: number; accessChanged?: (denied: boolean) => void; verified?: (signature: string | null) => void; change: (value: ProjectChoice | null) => void;
}) {
  const [revision, setRevision] = useState(0);
  const directory = useProjectPage<ProjectSummary>(projectPath(workspaceId, teamId), revision + refreshKey, 100);
  useEffect(() => { if (isAccessError(directory.error)) accessChanged?.(true); else if (!directory.loading && !directory.error && directory.value) accessChanged?.(false); }, [directory.error, directory.loading, directory.value, accessChanged]);
  const projects = conceal || directory.loading || directory.error ? [] : (directory.value?.data ?? []).filter(item => item.team_id === teamId && item.workspace_id === workspaceId && !item.archived_at && item.can_edit);
  const selected = projects.find(item => item.id === value?.id);
  const signature = value && selected && (!versionRequired || selected.etag === value.etag) ? projectSignature(scope, workspaceId, teamId, value) : null;
  useEffect(() => { verified?.(signature); }, [signature, verified]);
  return <div><label className="field">{label}<select aria-label={label} value={value?.id ?? ''} disabled={disabled} onChange={event => {
    if (!event.target.value) change(null);
    else { const item = projects.find(project => project.id === event.target.value); if (item) change({ id: item.id, etag: item.etag }); }
  }}><option value="">No project</option>{value && !selected && <option value={value.id} disabled>Selected project unavailable — choose a project or clear</option>}{projects.map(item => <option key={item.id} value={item.id}>{item.name}</option>)}</select></label>{versionRequired && selected && value && selected.etag !== value.etag && <button type="button" disabled={disabled} onClick={() => change({ id: selected.id, etag: selected.etag })}>Use current project version</button>}{reviewNeeded && signature && value && <button type="button" disabled={disabled} onClick={() => change(value)}>Confirm selected project</button>}{clearNeeded && <button type="button" disabled={disabled} onClick={() => change(null)}>Continue without a project</button>}{directory.loading && <p role="status">Loading projects…</p>}{!!directory.error && <p role="alert">Projects could not be loaded. Your selection is preserved. <button type="button" onClick={directory.retry}>Try again</button></p>}{!directory.error && directory.value?.has_more && <button type="button" disabled={directory.loading} onClick={directory.more}>Load more projects</button>}{value && <button type="button" disabled={disabled || directory.loading} onClick={() => setRevision(current => current + 1)}>Refresh projects</button>}</div>;
}
