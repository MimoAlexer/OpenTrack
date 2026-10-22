use super::*;
use opentrack_domain::{UpdateUserPreferences, UserPreferences};
use serde_json::json;

impl App {
    pub async fn user_preferences(
        &self,
        actor: &Actor,
        workspace: Uuid,
    ) -> Result<UserPreferences> {
        let mut tx = self.scoped(actor, workspace, false).await?;
        let result = store::user_preferences(&mut tx, workspace, actor.user.id)
            .await
            .map_err(db_error)?;
        tx.commit().await.map_err(db_error)?;
        Ok(result)
    }

    pub async fn update_user_preferences(
        &self,
        actor: &Actor,
        workspace: Uuid,
        etag: &str,
        request: UpdateUserPreferences,
    ) -> Result<UserPreferences> {
        // Personal accessibility preferences remain changeable by viewers and in archived workspaces.
        let mut tx = self.scoped(actor, workspace, false).await?;
        store::lock_user_preferences(&mut tx, workspace, actor.user.id)
            .await
            .map_err(db_error)?;
        let old = store::user_preferences(&mut tx, workspace, actor.user.id)
            .await
            .map_err(db_error)?;
        lifecycle::version(etag, &old.etag, &old)?;
        if old.shortcut_mode == request.shortcut_mode {
            tx.commit().await.map_err(db_error)?;
            return Ok(old);
        }
        graph::one(
            store::save_user_preferences(&mut tx, workspace, actor.user.id, request.shortcut_mode)
                .await
                .map_err(db_error)?,
        )?;
        let result = store::user_preferences(&mut tx, workspace, actor.user.id)
            .await
            .map_err(db_error)?;
        let mutation = Uuid::new_v4();
        graph::one(sqlx::query("INSERT INTO user_preference_events(workspace_id,id,user_id,preference_version,shortcut_mode) VALUES($1,$2,$3,$4,$5)")
            .bind(workspace).bind(mutation).bind(actor.user.id).bind(result.version.parse::<i64>().map_err(|_| Error::Internal)?).bind(request.shortcut_mode.as_str()).execute(&mut *tx).await.map_err(db_error)?.rows_affected())?;
        graph::one(sqlx::query("INSERT INTO audit_entries(id,workspace_id,actor_id,action,target_id,metadata) VALUES($1,$2,$3,'user.preferences_changed',$3,$4)")
            .bind(mutation).bind(workspace).bind(actor.user.id).bind(json!({"shortcut_mode":request.shortcut_mode})).execute(&mut *tx).await.map_err(db_error)?.rows_affected())?;
        tx.commit().await.map_err(db_error)?;
        Ok(result)
    }
}
