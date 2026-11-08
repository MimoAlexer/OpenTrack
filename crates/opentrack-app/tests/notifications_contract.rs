//! Recipient privacy, delivery-order snapshots and inbox-only state in real PostgreSQL.
mod graph_support;
#[allow(dead_code)]
mod support;
use chrono::{Duration, Utc};
use graph_support::*;
use opentrack_domain::*;
use opentrack_store as store;
use serde_json::json;
use support::Harness;
use uuid::Uuid;

fn query(filter: NotificationFilter) -> NotificationQuery {
    NotificationQuery {
        filter: Some(filter),
        ..Default::default()
    }
}
fn action(action: NotificationAction) -> UpdateNotification {
    UpdateNotification {
        action,
        until: None,
    }
}
async fn seed(
    h: &Harness,
    actor: &Actor,
    recipient: &Actor,
    w: Uuid,
    source: &Issue,
    origin: Uuid,
    count: i64,
) -> Vec<Uuid> {
    let mut tx = h.context(actor.user.id, w).await;
    let end: i64 = sqlx::query_scalar("INSERT INTO notification_heads(workspace_id,user_id,last_seq) VALUES($1,$2,$3) ON CONFLICT(workspace_id,user_id) DO UPDATE SET last_seq=notification_heads.last_seq+EXCLUDED.last_seq RETURNING last_seq")
        .bind(w).bind(recipient.user.id).bind(count).fetch_one(&mut *tx).await.unwrap();
    let ids: Vec<Uuid> = sqlx::query_scalar("INSERT INTO notifications(workspace_id,user_id,id,inbox_seq,event_id,event_seq,entity_kind,entity_id,reason,origin_team_id,actor_id) SELECT $1,$2,gen_random_uuid(),$3+g,gen_random_uuid(),1,'issue',$4,'assignment',$5,$6 FROM generate_series(1,$7::bigint) g RETURNING id")
        .bind(w).bind(recipient.user.id).bind(end-count).bind(source.id).bind(origin).bind(actor.user.id).bind(count).fetch_all(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    ids
}

async fn invalidation_count(h: &Harness, owner: &Actor, w: Uuid, recipient: &Actor) -> i64 {
    let mut tx = h.context(owner.user.id, w).await;
    let count = sqlx::query_scalar(
        "SELECT count(*) FROM notification_invalidations WHERE workspace_id=$1 AND user_id=$2",
    )
    .bind(w)
    .bind(recipient.user.id)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    count
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_postgres_notifications_recipient_snapshot_race_and_snooze() {
    let h = Harness::new().await;
    let copy = h.clone();
    let result = tokio::spawn(async move { snapshot_and_snooze(&copy).await }).await;
    h.cleanup().await;
    if let Err(error) = result {
        std::panic::resume_unwind(error.into_panic());
    }
}

async fn snapshot_and_snooze(h: &Harness) {
    let (owner, w, t) = setup(h).await;
    let recipient = person(h, &owner, w, "viewer").await;
    let source = issue(h, &owner, w, t, json!({"title":"Current source title"})).await;
    let ids = seed(h, &owner, &recipient, w, &source, t, 101).await;
    let page = h
        .app
        .notifications(
            &recipient,
            w,
            NotificationQuery {
                limit: Some(2),
                ..query(NotificationFilter::Unread)
            },
        )
        .await
        .unwrap();
    assert_eq!(page.items.len(), 2);
    assert_eq!(page.unread_count, 101);
    assert_eq!(page.items[0].seq, "101");
    assert_eq!(
        page.items[0].source.title.as_deref(),
        Some("Current source title")
    );
    assert_eq!(page.items[0].source.team_id, Some(t));
    assert!(
        h.app
            .notifications(&owner, w, query(NotificationFilter::All))
            .await
            .unwrap()
            .items
            .is_empty()
    );
    assert!(matches!(
        h.app
            .update_notification(
                &owner,
                w,
                ids[0],
                &page.items[0].etag,
                action(NotificationAction::Read)
            )
            .await,
        Err(Error::NotFound)
    ));
    assert!(
        h.app
            .mark_notifications_read(
                &owner,
                w,
                MarkNotificationsRead {
                    snapshot: page.snapshot.clone()
                }
            )
            .await
            .is_err()
    );
    for other in [
        NotificationQuery {
            filter: Some(NotificationFilter::All),
            limit: Some(2),
            cursor: page.next_cursor.clone(),
        },
        NotificationQuery {
            filter: Some(NotificationFilter::Unread),
            limit: Some(3),
            cursor: page.next_cursor.clone(),
        },
    ] {
        assert!(h.app.notifications(&recipient, w, other).await.is_err());
    }
    // Block the first batch, deliver a source event that predates the snapshot,
    // then let mark-all proceed. Delivery sequence, not source sequence, bounds it.
    let mut gate = h.context(owner.user.id, w).await;
    sqlx::query("SELECT id FROM notifications WHERE workspace_id=$1 AND user_id=$2 AND inbox_seq=1 FOR UPDATE")
        .bind(w).bind(recipient.user.id).fetch_one(&mut *gate).await.unwrap();
    let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *gate)
        .await
        .unwrap();
    let app = h.app.clone();
    let target = recipient.clone();
    let snapshot = page.snapshot.clone();
    let pending = tokio::spawn(async move {
        app.mark_notifications_read(&target, w, MarkNotificationsRead { snapshot })
            .await
    });
    blocked(h, pid).await;
    let mut arrival = h.context(owner.user.id, w).await;
    let delivered: i64 = sqlx::query_scalar("UPDATE notification_heads SET last_seq=last_seq+1 WHERE workspace_id=$1 AND user_id=$2 RETURNING last_seq")
        .bind(w).bind(recipient.user.id).fetch_one(&mut *arrival).await.unwrap();
    let late: Uuid = sqlx::query_scalar("INSERT INTO notifications(workspace_id,user_id,inbox_seq,event_id,event_seq,entity_kind,entity_id,reason,origin_team_id,actor_id) SELECT $1,$2,$3,e.event_id,e.seq,'issue',$4,'assignment',$5,$6 FROM workspace_events e WHERE e.workspace_id=$1 AND e.entity_id=$4 AND e.event_type='issue.created' ORDER BY e.seq LIMIT 1 RETURNING id")
        .bind(w).bind(recipient.user.id).bind(delivered).bind(source.id).bind(t).bind(owner.user.id).fetch_one(&mut *arrival).await.unwrap();
    arrival.commit().await.unwrap();
    let continuation = h
        .app
        .notifications(
            &recipient,
            w,
            NotificationQuery {
                filter: Some(NotificationFilter::Unread),
                limit: Some(2),
                cursor: page.next_cursor.clone(),
            },
        )
        .await
        .unwrap();
    assert_eq!(continuation.snapshot, page.snapshot);
    assert_eq!(
        continuation
            .items
            .iter()
            .map(|item| item.seq.as_str())
            .collect::<Vec<_>>(),
        vec!["99", "98"]
    );
    assert!(continuation.items.iter().all(|item| item.id != late));
    gate.commit().await.unwrap();
    let first = pending.await.unwrap().unwrap();
    assert_eq!(first.updated_count, 100);
    assert!(first.has_more);
    assert_eq!(invalidation_count(h, &owner, w, &recipient).await, 1);
    let second = h
        .app
        .mark_notifications_read(
            &recipient,
            w,
            MarkNotificationsRead {
                snapshot: page.snapshot.clone(),
            },
        )
        .await
        .unwrap();
    assert_eq!(second.updated_count, 1);
    assert!(!second.has_more);
    assert_eq!(second.unread_count, 1);
    assert_eq!(invalidation_count(h, &owner, w, &recipient).await, 2);
    let replay = h
        .app
        .mark_notifications_read(
            &recipient,
            w,
            MarkNotificationsRead {
                snapshot: page.snapshot.clone(),
            },
        )
        .await
        .unwrap();
    assert_eq!(replay.updated_count, 0);
    assert_eq!(replay.unread_count, 1);
    assert_eq!(invalidation_count(h, &owner, w, &recipient).await, 2);
    let continued = h
        .app
        .notifications(
            &recipient,
            w,
            NotificationQuery {
                filter: Some(NotificationFilter::All),
                limit: Some(2),
                cursor: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(continued.items[0].id, late);
    let mut item = continued.items[0].clone();
    assert!(item.read_at.is_none());
    let unchanged = h
        .app
        .update_notification(
            &recipient,
            w,
            late,
            &item.etag,
            action(NotificationAction::Unread),
        )
        .await
        .unwrap();
    assert_eq!(unchanged.version, item.version);
    assert_eq!(invalidation_count(h, &owner, w, &recipient).await, 2);
    fault(h, "notification_invalidations", "INSERT").await;
    assert!(
        h.app
            .update_notification(
                &recipient,
                w,
                late,
                &item.etag,
                action(NotificationAction::Read)
            )
            .await
            .is_err()
    );
    clear(h, "notification_invalidations").await;
    let rolled_back = h
        .app
        .notifications(&recipient, w, query(NotificationFilter::Unread))
        .await
        .unwrap();
    assert_eq!(rolled_back.items[0].version, item.version);
    assert!(rolled_back.items[0].read_at.is_none());
    assert_eq!(invalidation_count(h, &owner, w, &recipient).await, 2);
    // Publication may remain behind saturated stream admission; processing the
    // personal inbox still commits its durable private invalidation intent.
    let mut saturation = h.context(owner.user.id, w).await;
    sqlx::query("UPDATE workspace_stream_heads SET max_pending_events=1 WHERE workspace_id=$1")
        .bind(w)
        .execute(&mut *saturation)
        .await
        .unwrap();
    saturation.commit().await.unwrap();
    let before_source =
        serde_json::to_value(h.app.get_issue(&owner, w, source.id).await.unwrap()).unwrap();
    let until = Utc::now() + Duration::milliseconds(700);
    item = h
        .app
        .update_notification(
            &recipient,
            w,
            late,
            &item.etag,
            UpdateNotification {
                action: NotificationAction::Snooze,
                until: Some(until),
            },
        )
        .await
        .unwrap();
    assert_eq!(
        h.app
            .notifications(&recipient, w, query(NotificationFilter::Unread))
            .await
            .unwrap()
            .items
            .len(),
        0
    );
    assert_eq!(
        h.app
            .notifications(&recipient, w, query(NotificationFilter::Snoozed))
            .await
            .unwrap()
            .items[0]
            .id,
        late
    );
    let repeated = h
        .app
        .update_notification(
            &recipient,
            w,
            late,
            &item.etag,
            UpdateNotification {
                action: NotificationAction::Snooze,
                until: Some(until),
            },
        )
        .await
        .unwrap();
    assert_eq!(repeated.version, item.version);
    assert_eq!(invalidation_count(h, &owner, w, &recipient).await, 3);
    tokio::time::sleep(std::time::Duration::from_millis(750)).await;
    let returned = h
        .app
        .notifications(&recipient, w, query(NotificationFilter::Unread))
        .await
        .unwrap();
    assert_eq!(returned.items.len(), 1);
    assert_eq!(returned.items[0].id, late);
    assert_eq!(returned.items[0].version, item.version);
    item = h
        .app
        .update_notification(
            &recipient,
            w,
            late,
            &item.etag,
            action(NotificationAction::Archive),
        )
        .await
        .unwrap();
    assert_eq!(
        h.app
            .notifications(&recipient, w, query(NotificationFilter::Archived))
            .await
            .unwrap()
            .items[0]
            .id,
        late
    );
    assert!(
        h.app
            .notifications(&recipient, w, query(NotificationFilter::Unread))
            .await
            .unwrap()
            .items
            .is_empty()
    );
    assert_eq!(
        serde_json::to_value(h.app.get_issue(&owner, w, source.id).await.unwrap()).unwrap(),
        before_source
    );
    assert!(matches!(
        h.app
            .update_notification(
                &recipient,
                w,
                late,
                &unchanged.etag,
                action(NotificationAction::Read)
            )
            .await,
        Err(Error::VersionConflict(_))
    ));
    assert!(
        h.app
            .update_notification(
                &recipient,
                w,
                late,
                &item.etag,
                UpdateNotification {
                    action: NotificationAction::Snooze,
                    until: Some(Utc::now() + Duration::days(91))
                }
            )
            .await
            .is_err()
    );
    let mut runtime = store::begin(&h.app.pool).await.unwrap();
    store::context(&mut runtime, owner.user.id, Some(w))
        .await
        .unwrap();
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM notifications WHERE user_id=$1")
        .bind(recipient.user.id)
        .fetch_one(&mut *runtime)
        .await
        .unwrap();
    assert_eq!(count, 0);
    assert_eq!(
        sqlx::query(
            "UPDATE notifications SET read_at=clock_timestamp(),version=version+1 WHERE user_id=$1"
        )
        .bind(recipient.user.id)
        .execute(&mut *runtime)
        .await
        .unwrap()
        .rows_affected(),
        0
    );
    runtime.rollback().await.unwrap();
}

#[tokio::test]
async fn real_postgres_notifications_recheck_origin_deleted_source_and_preferences() {
    let h = Harness::new().await;
    let copy = h.clone();
    let result = tokio::spawn(async move { privacy_and_preferences(&copy).await }).await;
    h.cleanup().await;
    if let Err(error) = result {
        std::panic::resume_unwind(error.into_panic());
    }
}
async fn privacy_and_preferences(h: &Harness) {
    let (owner, w, t) = setup(h).await;
    let recipient = person(h, &owner, w, "member").await;
    let private = team(h, &owner, w, true).await;
    grant(h, &owner, w, private, recipient.user.id).await;
    let source = issue(
        h,
        &owner,
        w,
        t,
        json!({"title":"Private original audience"}),
    )
    .await;
    // A moved source retains its original audience reference even when its
    // current issue team is public.
    seed(h, &owner, &recipient, w, &source, private, 1).await;
    let visible = h
        .app
        .notifications(&recipient, w, query(NotificationFilter::All))
        .await
        .unwrap();
    assert!(visible.items[0].source.available);
    let mut tx = h.context(owner.user.id, w).await;
    sqlx::query("DELETE FROM team_members WHERE workspace_id=$1 AND team_id=$2 AND user_id=$3")
        .bind(w)
        .bind(private)
        .bind(recipient.user.id)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("UPDATE workspace_auth_guards SET epoch=epoch+1 WHERE workspace_id=$1")
        .bind(w)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert!(
        h.app
            .mark_notifications_read(
                &recipient,
                w,
                MarkNotificationsRead {
                    snapshot: visible.snapshot
                }
            )
            .await
            .is_err()
    );
    let unavailable = h
        .app
        .notifications(&recipient, w, query(NotificationFilter::All))
        .await
        .unwrap();
    let wire = serde_json::to_value(&unavailable.items[0].source).unwrap();
    assert_eq!(
        wire,
        json!({"kind":"issue","id":source.id,"available":false})
    );
    let marked = h
        .app
        .update_notification(
            &recipient,
            w,
            unavailable.items[0].id,
            &unavailable.items[0].etag,
            action(NotificationAction::Read),
        )
        .await
        .unwrap();
    assert!(!marked.source.available);
    let public = issue(
        h,
        &owner,
        w,
        t,
        json!({"title":"Deleted title must disappear"}),
    )
    .await;
    seed(h, &owner, &recipient, w, &public, t, 1).await;
    let changed = h
        .app
        .update_issue(
            &owner,
            w,
            public.id,
            &public.etag(),
            json!({"title":"Changed current title"}),
        )
        .await
        .unwrap();
    assert_eq!(
        h.app
            .notifications(&recipient, w, query(NotificationFilter::All))
            .await
            .unwrap()
            .items[0]
            .source
            .title
            .as_deref(),
        Some("Changed current title")
    );
    h.app
        .delete_issue(&owner, w, public.id, &changed.etag())
        .await
        .unwrap();
    let deleted = h
        .app
        .notifications(&recipient, w, query(NotificationFilter::All))
        .await
        .unwrap();
    assert!(!deleted.items[0].source.available);
    assert!(
        !serde_json::to_string(&deleted)
            .unwrap()
            .contains("Deleted title")
    );
    let initial = h.app.notification_preferences(&recipient, w).await.unwrap();
    assert_eq!(initial.version, "0");
    assert!(initial.assignments && initial.mentions && initial.subscribed_comments);
    let owner_prefs = h.app.notification_preferences(&owner, w).await.unwrap();
    assert_ne!(owner_prefs.etag, initial.etag);
    let request = UpdateNotificationPreferences {
        assignments: false,
        mentions: true,
        subscribed_comments: false,
    };
    assert!(matches!(
        h.app
            .update_notification_preferences(&recipient, w, &owner_prefs.etag, request.clone())
            .await,
        Err(Error::VersionConflict(_))
    ));
    let saved = h
        .app
        .update_notification_preferences(&recipient, w, &initial.etag, request.clone())
        .await
        .unwrap();
    assert_eq!(saved.version, "1");
    assert_eq!(
        h.app
            .update_notification_preferences(&recipient, w, &saved.etag, request)
            .await
            .unwrap()
            .version,
        "1"
    );
    assert!(
        h.app
            .notification_preferences(&owner, w)
            .await
            .unwrap()
            .assignments
    );
    let workspace = h.app.workspace(&owner, w).await.unwrap();
    h.app
        .update_workspace(&owner, w, &workspace.etag(), req(json!({"archived":true})))
        .await
        .unwrap();
    h.app
        .update_notification(
            &recipient,
            w,
            marked.id,
            &marked.etag,
            action(NotificationAction::Unread),
        )
        .await
        .unwrap();
    h.app
        .update_notification_preferences(
            &recipient,
            w,
            &saved.etag,
            UpdateNotificationPreferences {
                assignments: true,
                mentions: true,
                subscribed_comments: true,
            },
        )
        .await
        .unwrap();
    let mut tx = h.context(owner.user.id, w).await;
    sqlx::query("UPDATE workspace_members SET active=false WHERE workspace_id=$1 AND user_id=$2")
        .bind(w)
        .bind(recipient.user.id)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert!(matches!(
        h.app
            .notifications(&recipient, w, query(NotificationFilter::All))
            .await,
        Err(Error::NotFound)
    ));
    assert!(matches!(
        h.app.notification_preferences(&recipient, w).await,
        Err(Error::NotFound)
    ));
}

#[tokio::test]
async fn real_postgres_notifications_project_and_comment_sources_use_current_access() {
    let h = Harness::new().await;
    let copy = h.clone();
    let result = tokio::spawn(async move { project_and_comment(&copy).await }).await;
    h.cleanup().await;
    if let Err(error) = result {
        std::panic::resume_unwind(error.into_panic());
    }
}

async fn project_and_comment(h: &Harness) {
    let (owner, w, t) = setup(h).await;
    let recipient = person(h, &owner, w, "viewer").await;
    let project = h
        .app
        .create_project(
            &owner,
            w,
            t,
            "notification-project",
            req(json!({"name":"Initial project name"})),
        )
        .await
        .unwrap();
    let project_document = h
        .app
        .project_document(&owner, w, t, project.id)
        .await
        .unwrap();
    let source = issue(h, &owner, w, t, json!({"title":"Comment source"})).await;
    let comment = h
        .app
        .create_comment(
            &owner,
            w,
            source.id,
            "notification-comment",
            DocumentWrite::from_plain_text("Current comment body"),
        )
        .await
        .unwrap();
    let comment_document = comment.document.as_ref().unwrap();
    let mut tx = h.context(owner.user.id, w).await;
    sqlx::query("INSERT INTO notification_heads(workspace_id,user_id,last_seq) VALUES($1,$2,2)")
        .bind(w)
        .bind(recipient.user.id)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("INSERT INTO notifications(workspace_id,user_id,inbox_seq,event_id,entity_kind,entity_id,reason,origin_team_id,actor_id,document_id) VALUES($1,$2,1,$3,'project',$4,'mention',$5,$6,$7)").bind(w).bind(recipient.user.id).bind(Uuid::new_v4()).bind(project.id).bind(t).bind(owner.user.id).bind(project_document.id).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO notifications(workspace_id,user_id,inbox_seq,event_id,entity_kind,entity_id,reason,origin_team_id,actor_id,document_id,comment_id) VALUES($1,$2,2,$3,'issue',$4,'subscribed_comment',$5,$6,$7,$8)").bind(w).bind(recipient.user.id).bind(Uuid::new_v4()).bind(source.id).bind(t).bind(owner.user.id).bind(comment_document.id).bind(comment.id).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    let initial = h
        .app
        .notifications(&recipient, w, query(NotificationFilter::All))
        .await
        .unwrap();
    assert_eq!(initial.items[0].source.comment_id, Some(comment.id));
    assert_eq!(
        initial.items[0].source.document_id,
        Some(comment_document.id)
    );
    assert_eq!(
        initial.items[1].source.kind,
        NotificationSourceKind::Project
    );
    assert_eq!(initial.items[1].source.team_id, Some(t));
    assert_eq!(
        initial.items[1].source.title.as_deref(),
        Some("Initial project name")
    );
    assert!(initial.items[1].source.key.is_none());
    h.app
        .update_project_metadata(
            &owner,
            w,
            t,
            project.id,
            &project.metadata_etag,
            req(json!({"name":"Current project name"})),
        )
        .await
        .unwrap();
    h.app
        .delete_comment(&owner, w, comment.id, &comment.etag, req(json!({})))
        .await
        .unwrap();
    let current = h
        .app
        .notifications(&recipient, w, query(NotificationFilter::All))
        .await
        .unwrap();
    assert_eq!(
        current.items[1].source.title.as_deref(),
        Some("Current project name")
    );
    assert_eq!(
        serde_json::to_value(&current.items[0].source).unwrap(),
        json!({"kind":"issue","id":source.id,"available":false})
    );
    assert!(matches!(
        h.app
            .notifications(&recipient, Uuid::new_v4(), query(NotificationFilter::All))
            .await,
        Err(Error::NotFound)
    ));
}

#[tokio::test]
async fn real_postgres_notification_next_snooze_deadline_includes_read_and_excludes_archive() {
    let h = Harness::new().await;
    let copy = h.clone();
    let result = tokio::spawn(async move {
        let (owner,w,t) = setup(&copy).await;
        let recipient = person(&copy,&owner,w,"viewer").await;
        let source = issue(&copy,&owner,w,t,json!({})).await;
        seed(&copy,&owner,&recipient,w,&source,t,2).await;
        let due = chrono::DateTime::from_timestamp_micros((Utc::now()+Duration::milliseconds(700)).timestamp_micros()).unwrap();
        let mut tx = copy.context(owner.user.id,w).await;
        sqlx::query("UPDATE notifications SET read_at=clock_timestamp(),snoozed_until=CASE WHEN inbox_seq=2 THEN $3+interval '1 hour' ELSE $3 END,archived_at=CASE WHEN inbox_seq=2 THEN clock_timestamp() END,version=version+1 WHERE workspace_id=$1 AND user_id=$2")
            .bind(w).bind(recipient.user.id).bind(due).execute(&mut *tx).await.unwrap();
        tx.commit().await.unwrap();
        let upcoming = copy.app.notifications(&recipient,w,query(NotificationFilter::Unread)).await.unwrap();
        assert!(upcoming.items.is_empty());
        assert_eq!(serde_json::to_value(upcoming).unwrap().get("next_snoozed_at"),Some(&json!(due)));
        tokio::time::sleep(std::time::Duration::from_millis(750)).await;
        let returned = copy.app.notifications(&recipient,w,query(NotificationFilter::All)).await.unwrap();
        assert_eq!(returned.items.len(),1);
        assert_eq!(serde_json::to_value(returned).unwrap().get("next_snoozed_at"),Some(&serde_json::Value::Null));
    }).await;
    h.cleanup().await;
    if let Err(error) = result {
        std::panic::resume_unwind(error.into_panic());
    }
}
