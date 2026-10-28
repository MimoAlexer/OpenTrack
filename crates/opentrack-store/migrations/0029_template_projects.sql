ALTER TABLE issue_templates
 ADD COLUMN default_project_id uuid,
 ADD FOREIGN KEY(workspace_id,team_id,default_project_id) REFERENCES projects(workspace_id,team_id,id);
