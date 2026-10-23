import type { components } from '../generated/api';

type Schemas = components['schemas'];
export type ProjectState = Schemas['ProjectState'];
export type ProjectHealth = Schemas['ProjectHealth'];
export type Project = Schemas['ProjectSummary'];
export type FullProject = Schemas['Project'];
export type ProjectSummary = Schemas['ProjectSummary'];
export type ProjectProgress = Schemas['ProjectProgress'];
export type ProjectIssue = Schemas['ProjectIssue'];
export type ProjectUpdate = Schemas['ProjectUpdate'];
export interface Page<T> { data: T[]; has_more: boolean; next_cursor: string | null }
export const states: Record<ProjectState, string> = { planned: 'Planned', active: 'Active', paused: 'Paused', completed: 'Completed', canceled: 'Canceled' };
export const healthLabels: Record<ProjectHealth, string> = { on_track: 'On track', at_risk: 'At risk', off_track: 'Off track' };
export const projectPath = (workspace: string, team: string, project?: string) => `/workspaces/${encodeURIComponent(workspace)}/teams/${encodeURIComponent(team)}/projects${project ? `/${encodeURIComponent(project)}` : ''}`;
export const projectMetadataPath = (workspace: string, team: string, project: string) => `${projectPath(workspace, team, project)}/metadata`;
export function projectMetadata(value: FullProject): Project {
  const { narrative: _, narrative_version: _version, metadata_etag, ...metadata } = value;
  return { ...metadata, etag: metadata_etag };
}
export const projectUrl = (workspace: string, team: string, project?: string) => `/w/${encodeURIComponent(workspace)}/projects${project ? `/${encodeURIComponent(project)}` : ''}?${new URLSearchParams({ team_id: team })}`;
