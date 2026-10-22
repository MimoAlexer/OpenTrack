use crate::Tx;
use opentrack_domain::{ShortcutMode, UserPreferences};
use sqlx::Row;
use uuid::Uuid;

pub async fn user_preferences(
    tx: &mut Tx,
    workspace: Uuid,
    user: Uuid,
) -> Result<UserPreferences, sqlx::Error> {
    let row = sqlx::query(
        "SELECT shortcut_mode,version FROM user_preferences WHERE workspace_id=$1 AND user_id=$2",
    )
    .bind(workspace)
    .bind(user)
    .fetch_optional(&mut **tx)
    .await?;
    let (mode, version) = match row {
        None => (ShortcutMode::Enabled, 0_i64),
        Some(row) => (
            match row.try_get::<&str, _>("shortcut_mode")? {
                "enabled" => ShortcutMode::Enabled,
                "disabled" => ShortcutMode::Disabled,
                _ => return Err(sqlx::Error::Protocol("Unknown shortcut preference".into())),
            },
            row.try_get("version")?,
        ),
    };
    Ok(UserPreferences {
        workspace_id: workspace,
        shortcut_mode: mode,
        version: version.to_string(),
        etag: format!("\"preferences:{workspace}:{user}:{version}\""),
    })
}

pub async fn lock_user_preferences(
    tx: &mut Tx,
    workspace: Uuid,
    user: Uuid,
) -> Result<(), sqlx::Error> {
    // The existing membership serializes even the first update without making GET create a row.
    sqlx::query("SELECT user_id FROM workspace_members WHERE workspace_id=$1 AND user_id=$2 AND active FOR UPDATE")
        .bind(workspace).bind(user).fetch_one(&mut **tx).await?;
    Ok(())
}

pub async fn save_user_preferences(
    tx: &mut Tx,
    workspace: Uuid,
    user: Uuid,
    mode: ShortcutMode,
) -> Result<u64, sqlx::Error> {
    Ok(sqlx::query("INSERT INTO user_preferences(workspace_id,user_id,shortcut_mode) VALUES($1,$2,$3) ON CONFLICT(workspace_id,user_id) DO UPDATE SET shortcut_mode=EXCLUDED.shortcut_mode,version=user_preferences.version+1,updated_at=clock_timestamp()")
        .bind(workspace).bind(user).bind(mode.as_str()).execute(&mut **tx).await?.rows_affected())
}
