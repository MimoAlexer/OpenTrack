import type { components } from '../generated/api';
type Schemas = components['schemas'];
export type WorkflowStatus = Schemas['Status'];
export type Workflow = Schemas['Workflow'];
export type ReplacementPreview = Schemas['WorkflowReplacementPreview'];
export type WorkflowReplacement = Schemas['WorkflowReplacement'];
