//! Personal accessibility preferences: durable authority, isolation and conflicts.
mod graph_support;
#[allow(dead_code)]
mod support;
use graph_support::*;
use opentrack_domain::*;
use opentrack_store as store;
use serde_json::{Value, json};
use support::Harness;
use uuid::Uuid;

fn change(shortcut_mode: ShortcutMode) -> UpdateUserPreferences {
    UpdateUserPreferences { shortcut_mode }
}
async fn snapshot_preferences(h: &Harness, a: &Actor, w: Uuid) -> Value {
    let mut tx = h.context(a.user.id, w).await;
    let result = sqlx::query_scalar("SELECT jsonb_build_object('preferences',(SELECT coalesce(jsonb_agg(to_jsonb(p)),'[]') FROM user_preferences p),'events',(SELECT coalesce(jsonb_agg(to_jsonb(e) ORDER BY preference_version),'[]') FROM user_preference_events e),'audit',(SELECT count(*) FROM audit_entries WHERE action='user.preferences_changed'))").fetch_one(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    result
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_postgres_personal_shortcut_preferences() {
    let h = Harness::new().await;
    let copy = h.clone();
    let result = tokio::spawn(async move { exercise(&copy).await }).await;
    h.cleanup().await;
    if let Err(error) = result {
        std::panic::resume_unwind(error.into_panic());
    }
}
async fn exercise(h: &Harness) {
    let (a, w, _) = setup(h).await;
    let viewer = person(h, &a, w, "viewer").await;
    let current = h.app.user_preferences(&a, w).await.unwrap();
    assert_eq!(current.shortcut_mode, ShortcutMode::Enabled);
    assert_eq!(current.version, "0");
    assert_eq!(
        snapshot_preferences(h, &a, w).await,
        json!({"preferences":[],"events":[],"audit":0})
    );
    let other = h.app.user_preferences(&viewer, w).await.unwrap();
    assert_ne!(current.etag, other.etag);
    assert!(matches!(
        h.app
            .update_user_preferences(&viewer, w, &current.etag, change(ShortcutMode::Disabled))
            .await,
        Err(Error::VersionConflict(_))
    ));
    assert!(matches!(
        h.app
            .update_user_preferences(&a, w, "", change(ShortcutMode::Disabled))
            .await,
        Err(Error::PreconditionRequired)
    ));
    let (left, right) = tokio::join!(
        h.app
            .update_user_preferences(&a, w, &current.etag, change(ShortcutMode::Disabled)),
        h.app
            .update_user_preferences(&a, w, &current.etag, change(ShortcutMode::Disabled))
    );
    assert_eq!(usize::from(left.is_ok()) + usize::from(right.is_ok()), 1);
    assert!(matches!(
        left.as_ref().err().or(right.as_ref().err()),
        Some(Error::VersionConflict(_))
    ));
    let saved = h.app.user_preferences(&a, w).await.unwrap();
    assert_eq!(saved.version, "1");
    assert_eq!(saved.shortcut_mode, ShortcutMode::Disabled);
    assert_eq!(
        h.app
            .user_preferences(&viewer, w)
            .await
            .unwrap()
            .shortcut_mode,
        ShortcutMode::Enabled
    );
    h.app
        .update_user_preferences(&viewer, w, &other.etag, change(ShortcutMode::Disabled))
        .await
        .unwrap();
    let before = snapshot_preferences(h, &a, w).await;
    h.app
        .update_user_preferences(&a, w, &saved.etag, change(ShortcutMode::Disabled))
        .await
        .unwrap();
    assert_eq!(snapshot_preferences(h, &a, w).await, before);
    for table in ["user_preference_events", "audit_entries"] {
        fault(h, table, "INSERT").await;
        assert!(
            h.app
                .update_user_preferences(&a, w, &saved.etag, change(ShortcutMode::Enabled))
                .await
                .is_err()
        );
        clear(h, table).await;
        assert_eq!(snapshot_preferences(h, &a, w).await, before);
    }
    let detail = h.app.workspace(&a, w).await.unwrap();
    let second = h.app.create_workspace(&a,"preferences-workspace",req(json!({"organization_id":detail.organization_id,"name":"Second workspace","team_name":"Operations","team_key":"OPS"}))).await.unwrap();
    assert_eq!(
        h.app
            .user_preferences(&a, second.id)
            .await
            .unwrap()
            .shortcut_mode,
        ShortcutMode::Enabled
    );
    assert!(matches!(
        h.app.user_preferences(&viewer, second.id).await,
        Err(Error::NotFound)
    ));
    assert!(matches!(
        h.app
            .update_user_preferences(&a, second.id, &saved.etag, change(ShortcutMode::Disabled))
            .await,
        Err(Error::VersionConflict(_))
    ));
    h.app
        .update_workspace(&a, w, &detail.etag(), req(json!({"archived":true})))
        .await
        .unwrap();
    let enabled = h
        .app
        .update_user_preferences(&a, w, &saved.etag, change(ShortcutMode::Enabled))
        .await
        .unwrap();
    assert_eq!(enabled.version, "2");
    // Runtime SQL cannot read or write another caller's preference, even in the same workspace.
    let mut tx = store::begin(&h.app.pool).await.unwrap();
    store::context(&mut tx, a.user.id, Some(w)).await.unwrap();
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM user_preferences WHERE user_id=$1")
        .bind(viewer.user.id)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert_eq!(count, 0);
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM user_preference_events WHERE user_id=$1")
            .bind(viewer.user.id)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    assert_eq!(count, 0);
    assert_eq!(
        sqlx::query(
            "UPDATE user_preferences SET shortcut_mode='enabled',version=version+1 WHERE user_id=$1"
        )
        .bind(viewer.user.id)
        .execute(&mut *tx)
        .await
        .unwrap()
        .rows_affected(),
        0
    );
    assert!(sqlx::query("INSERT INTO user_preferences(workspace_id,user_id,shortcut_mode) VALUES($1,$2,'disabled')").bind(second.id).bind(a.user.id).execute(&mut *tx).await.is_err());
    tx.rollback().await.unwrap();
    let mut tx = store::begin(&h.app.pool).await.unwrap();
    store::context(&mut tx, a.user.id, Some(w)).await.unwrap();
    assert!(
        sqlx::query("DELETE FROM user_preferences WHERE workspace_id=$1")
            .bind(w)
            .execute(&mut *tx)
            .await
            .is_err()
    );
    tx.rollback().await.unwrap();
    // A durable setting survives a new authenticated session, without browser storage.
    let (_, token) = h
        .app
        .login(req(json!({"email":a.user.email,"password":PASSWORD})))
        .await
        .unwrap();
    let reloaded = h.app.authenticate(&token).await.unwrap();
    assert_eq!(
        h.app.user_preferences(&reloaded, w).await.unwrap().etag,
        enabled.etag
    );
    // Workspace revocation wins before a waiting preference command rechecks access.
    let mut gate = h.context(a.user.id, w).await;
    sqlx::query("SELECT workspace_id FROM workspace_auth_guards WHERE workspace_id=$1 FOR UPDATE")
        .bind(w)
        .execute(&mut *gate)
        .await
        .unwrap();
    let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *gate)
        .await
        .unwrap();
    let app = h.app.clone();
    let target = viewer.clone();
    let pending = tokio::spawn(async move {
        app.update_user_preferences(&target, w, &other.etag, change(ShortcutMode::Enabled))
            .await
    });
    blocked(h, pid).await;
    sqlx::query("UPDATE workspace_members SET active=false WHERE workspace_id=$1 AND user_id=$2")
        .bind(w)
        .bind(viewer.user.id)
        .execute(&mut *gate)
        .await
        .unwrap();
    gate.commit().await.unwrap();
    assert!(matches!(pending.await.unwrap(), Err(Error::NotFound)));
}

#[tokio::test]
async fn real_postgres_preferences_upgrade_from_schema_19() {
    let h = Harness::with_version(4, 19).await;
    let copy = h.clone();
    let result = tokio::spawn(async move {
        let (a, w, t) = setup(&copy).await;
        let existing = issue(&copy, &a, w, t, json!({})).await;
        store::MIGRATOR.run(&copy.owner).await.unwrap();
        assert_eq!(
            copy.app.get_issue(&a, w, existing.id).await.unwrap().title,
            existing.title
        );
        let preference = copy.app.user_preferences(&a, w).await.unwrap();
        assert_eq!(preference.version, "0");
        let saved = copy
            .app
            .update_user_preferences(&a, w, &preference.etag, change(ShortcutMode::Disabled))
            .await
            .unwrap();
        assert_eq!(saved.version, "1");
    })
    .await;
    h.cleanup().await;
    if let Err(error) = result {
        std::panic::resume_unwind(error.into_panic());
    }
}
