//! Actual guard contention, private preference races, and exact schema18 upgrade.
mod graph_support;
#[allow(dead_code)]
mod support;
use graph_support::*;
use opentrack_app::{
    Operator,
    worker::{Worker, WorkerOptions},
};
use opentrack_domain::*;
use opentrack_store as store;
use serde_json::json;
use support::Harness;
use uuid::Uuid;

fn mention(id: Uuid) -> DocumentWrite {
    req(
        json!({"schema_version":1,"body":{"type":"doc","content":[{"type":"paragraph","content":[{"type":"userMention","attrs":{"id":id}}]}]}}),
    )
}
async fn head(h: &Harness, a: &Actor, w: Uuid) -> (store::Tx, i32) {
    let mut tx = h.context(a.user.id, w).await;
    sqlx::query("SELECT workspace_id FROM workspace_stream_heads WHERE workspace_id=$1 FOR UPDATE")
        .bind(w)
        .execute(&mut *tx)
        .await
        .unwrap();
    let pid = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    (tx, pid)
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_postgres_mentions_access_and_preference_orders() {
    let h = Harness::with_connections(8).await;
    let copy = h.clone();
    let result = tokio::spawn(async move { orders(&copy).await }).await;
    h.cleanup().await;
    if let Err(e) = result {
        std::panic::resume_unwind(e.into_panic());
    }
}
async fn orders(h: &Harness) {
    let (a, w, _) = setup(h).await;
    let operator = Operator::connect(&h.operator_url().await).await.unwrap();
    for kind in ["team", "workspace", "disable"] {
        for first in [false, true] {
            authority_order(h, &operator, &a, w, kind, first).await;
        }
    }
    for first in [false, true] {
        preference_order(h, &a, w, first).await;
    }
}
async fn authority_order(
    h: &Harness,
    operator: &Operator,
    a: &Actor,
    w: Uuid,
    kind: &'static str,
    revoke_first: bool,
) {
    let t = team(h, a, w, kind == "team").await;
    let recipient = person(h, a, w, "viewer").await;
    if kind == "team" {
        grant(h, a, w, t, recipient.user.id).await;
    }
    let item = issue(h, a, w, t, json!({})).await;
    let document = h.app.issue_document(a, w, item.id).await.unwrap();
    let team_member = if kind == "team" {
        Some(
            h.app
                .get_team_member(a, w, t, recipient.user.id)
                .await
                .unwrap(),
        )
    } else {
        None
    };
    let member = h
        .app
        .get_workspace_member(a, w, recipient.user.id)
        .await
        .unwrap();
    let (holder, pid) = head(h, a, w).await;
    let app = h.app.clone();
    let actor = a.clone();
    let target = recipient.user.id;
    let op = operator.clone();
    let revocation = async move {
        match kind {
            "team" => {
                app.remove_team_member(&actor, w, t, target, &team_member.unwrap().etag(t))
                    .await
            }
            "workspace" => app
                .update_member(
                    &actor,
                    w,
                    target,
                    &member.etag(w),
                    req(json!({"active":false})),
                )
                .await
                .map(|_| ()),
            "disable" => op
                .disable_account(target, "Mention eligibility ordering")
                .await
                .map(|_| ()),
            _ => unreachable!(),
        }
    };
    let app = h.app.clone();
    let actor = a.clone();
    let before = document.clone();
    let write = async move {
        app.update_document(&actor, w, before.id, &before.etag, mention(target))
            .await
    };
    let written = if revoke_first {
        let first = tokio::spawn(revocation);
        let first_pid = blocked(h, pid).await;
        let second = tokio::spawn(write);
        blocked(h, first_pid).await;
        holder.commit().await.unwrap();
        first.await.unwrap().unwrap();
        second.await.unwrap()
    } else {
        let first = tokio::spawn(write);
        let first_pid = blocked(h, pid).await;
        let second = tokio::spawn(revocation);
        blocked(h, first_pid).await;
        holder.commit().await.unwrap();
        let out = first.await.unwrap();
        second.await.unwrap().unwrap();
        out
    };
    if revoke_first {
        assert!(
            matches!(written, Err(Error::Invalid(_))),
            "{kind}: revoked recipient cannot be added"
        );
    } else {
        written.unwrap();
    }
    let current = h.app.get_document(a, w, document.id).await.unwrap();
    let mut tx = h.context(a.user.id, w).await;
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM mention_intents WHERE document_id=$1 AND recipient_id=$2",
    )
    .bind(document.id)
    .bind(target)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert_eq!(count, i64::from(!revoke_first));
    tx.commit().await.unwrap();
    if revoke_first {
        assert_eq!(current.body, document.body);
        assert_eq!(current.version, document.version);
    } else {
        assert_eq!(current.mentions.len(), 1);
        assert!(!current.mentions[0].available);
        assert!(current.mentions[0].display_name.is_none());
    }
}
async fn preference_order(h: &Harness, a: &Actor, w: Uuid, preference_first: bool) {
    let t = team(h, a, w, false).await;
    let recipient = person(h, a, w, "viewer").await;
    let item = issue(h, a, w, t, json!({})).await;
    let doc = h.app.issue_document(a, w, item.id).await.unwrap();
    let old = h
        .app
        .issue_subscription(&recipient, w, item.id)
        .await
        .unwrap();
    assert_eq!(old.version, "0");
    let (holder, pid) = gate(h, a, w, t, Some(item.id)).await;
    let app = h.app.clone();
    let actor = recipient.clone();
    let etag = old.etag;
    let preference = async move {
        app.update_issue_subscription(
            &actor,
            w,
            item.id,
            &etag,
            req(json!({"preference":"unsubscribed"})),
        )
        .await
    };
    let app = h.app.clone();
    let actor = a.clone();
    let target = recipient.user.id;
    let write = async move {
        app.update_document(&actor, w, doc.id, &doc.etag, mention(target))
            .await
    };
    let changed = if preference_first {
        let first = tokio::spawn(preference);
        let first_pid = blocked(h, pid).await;
        let second = tokio::spawn(write);
        blocked(h, first_pid).await;
        holder.commit().await.unwrap();
        let result = first.await.unwrap();
        second.await.unwrap().unwrap();
        result
    } else {
        let first = tokio::spawn(write);
        let first_pid = blocked(h, pid).await;
        let second = tokio::spawn(preference);
        blocked(h, first_pid).await;
        holder.commit().await.unwrap();
        first.await.unwrap().unwrap();
        second.await.unwrap()
    };
    if preference_first {
        changed.unwrap();
    } else {
        assert!(matches!(changed, Err(Error::VersionConflict(_))));
        let fresh = h
            .app
            .issue_subscription(&recipient, w, item.id)
            .await
            .unwrap();
        h.app
            .update_issue_subscription(
                &recipient,
                w,
                item.id,
                &fresh.etag,
                req(json!({"preference":"unsubscribed"})),
            )
            .await
            .unwrap();
    }
    let current = h
        .app
        .issue_subscription(&recipient, w, item.id)
        .await
        .unwrap();
    assert!(!current.subscribed);
    assert_eq!(current.preference, SubscriptionPreference::Unsubscribed);
    assert_eq!(
        serde_json::to_value(current.sources).unwrap(),
        json!(["mention"])
    );
    let mut tx = h.context(a.user.id, w).await;
    let intents: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM mention_intents WHERE issue_id=$1 AND recipient_id=$2",
    )
    .bind(item.id)
    .bind(target)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert_eq!(intents, 1, "Direct mention survives subscription opt-out");
    tx.commit().await.unwrap();
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_postgres_mentions_0018_upgrade_backfill_and_receipts() {
    let h = Harness::with_version(6, 18).await;
    let copy = h.clone();
    let result = tokio::spawn(async move { upgrade(&copy).await }).await;
    h.cleanup().await;
    if let Err(e) = result {
        std::panic::resume_unwind(e.into_panic());
    }
}
async fn upgrade(h: &Harness) {
    let (a, w, t) = setup(h).await;
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT max(version) FROM _sqlx_migrations")
            .fetch_one(&h.owner)
            .await
            .unwrap(),
        18
    );
    let user = person(h, &a, w, "member").await;
    let removed = person(h, &a, w, "member").await;
    let disabled = person(h, &a, w, "member").await;
    let private = team(h, &a, w, true).await;
    grant(h, &a, w, private, removed.user.id).await;
    let command: CreateIssue = req(
        json!({"team_id":t,"title":"Pre19 receipt","description":"Legacy body unchanged","assignee_id":user.user.id}),
    );
    let bytes = serde_json::to_vec(&command).unwrap();
    let item = h
        .app
        .create_issue(&a, w, "legacy-mentions-receipt", command.clone())
        .await
        .unwrap();
    // Capture schema 18 directly; current document owner resolution requires schema 26.
    let mut tx = h.context(a.user.id, w).await;
    let mut olddoc: Document = req(sqlx::query_scalar::<_, serde_json::Value>("SELECT jsonb_build_object('id',id,'owner_type','issue','owner_id',issue_id,'schema_supported',schema_version=1,'schema_version',schema_version,'body',body,'plain_text',plain_text,'mentions','[]'::jsonb,'issue_references','[]'::jsonb,'version',version::text,'etag','','created_at',created_at,'updated_at',updated_at,'can_edit',true) FROM documents WHERE workspace_id=$1 AND issue_id=$2")
        .bind(w).bind(item.id).fetch_one(&mut *tx).await.unwrap());
    tx.commit().await.unwrap();
    olddoc.refresh_etag();
    let hidden = issue(h, &a, w, private, json!({"assignee_id":removed.user.id})).await;
    let hiddenmember = h
        .app
        .get_team_member(&a, w, private, removed.user.id)
        .await
        .unwrap();
    h.app
        .remove_team_member(&a, w, private, removed.user.id, &hiddenmember.etag(private))
        .await
        .unwrap();
    let unavailable = issue(h, &disabled, w, t, json!({"assignee_id":disabled.user.id})).await;
    let op = Operator::connect(&h.operator_url().await).await.unwrap();
    op.disable_account(disabled.user.id, "Pre-upgrade disabled account")
        .await
        .unwrap();
    let worker = Worker::connect(&h.worker_url().await, WorkerOptions::default())
        .await
        .unwrap();
    store::MIGRATOR.run(&h.owner).await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT max(version) FROM _sqlx_migrations")
            .fetch_one(&h.owner)
            .await
            .unwrap(),
        store::MIGRATOR
            .iter()
            .map(|migration| migration.version)
            .max()
            .unwrap(),
        "The schema-18 fixture must upgrade through all current migrations"
    );
    assert_eq!(bytes, serde_json::to_vec(&command).unwrap());
    assert_eq!(
        serde_json::to_value(
            h.app
                .create_issue(&a, w, "legacy-mentions-receipt", command)
                .await
                .unwrap()
        )
        .unwrap(),
        serde_json::to_value(item.clone()).unwrap()
    );
    assert_eq!(
        serde_json::to_value(h.app.issue_document(&a, w, item.id).await.unwrap()).unwrap(),
        serde_json::to_value(olddoc).unwrap()
    );
    let creator = h.app.issue_subscription(&a, w, item.id).await.unwrap();
    assert!(creator.subscribed);
    assert_eq!(
        serde_json::to_value(creator.sources).unwrap(),
        json!(["creator"])
    );
    let assignee = h.app.issue_subscription(&user, w, item.id).await.unwrap();
    assert!(assignee.subscribed);
    assert_eq!(
        serde_json::to_value(assignee.sources).unwrap(),
        json!(["assignment"])
    );
    let mut tx = h.context(a.user.id, w).await;
    let bad:i64=sqlx::query_scalar("SELECT count(*) FROM issue_subscriptions WHERE (issue_id=$1 AND user_id=$2) OR issue_id=$3").bind(hidden.id).bind(removed.user.id).bind(unavailable.id).fetch_one(&mut *tx).await.unwrap();
    assert_eq!(
        bad, 0,
        "Hidden and disabled historical assignees/creators are not enrolled"
    );
    for table in [
        "mention_intents",
        "discussion_sources",
        "subscription_events",
    ] {
        let query = sqlx::AssertSqlSafe(format!("SELECT count(*) FROM {table}"));
        assert_eq!(
            sqlx::query_scalar::<_, i64>(query)
                .fetch_one(&mut *tx)
                .await
                .unwrap(),
            0,
            "No retroactive source effects"
        );
    }
    tx.commit().await.unwrap();
    for table in ["document_mentions", "mention_intents", "discussion_sources"] {
        assert!(!sqlx::query_scalar::<_,bool>("SELECT has_any_column_privilege(current_user,$1,'SELECT,INSERT,UPDATE,REFERENCES') OR has_table_privilege(current_user,$1,'DELETE,TRUNCATE,TRIGGER')").bind(table).fetch_one(&h.app.pool).await.unwrap());
    }
    for table in [
        "issue_subscriptions",
        "subscription_events",
        "document_mentions",
        "mention_intents",
        "discussion_sources",
    ] {
        assert!(!sqlx::query_scalar::<_,bool>("SELECT has_any_column_privilege(current_user,$1,'SELECT,INSERT,UPDATE,REFERENCES') OR has_table_privilege(current_user,$1,'DELETE,TRUNCATE,TRIGGER')").bind(table).fetch_one(&worker.pool).await.unwrap());
    }
    let flow = h.app.workflow(&a, w, t).await.unwrap();
    let replacement = flow
        .statuses
        .iter()
        .find(|s| s.id != item.status_id)
        .unwrap()
        .id;
    let job = h
        .app
        .replace_status(
            &a,
            w,
            t,
            item.status_id,
            &Uuid::new_v4().to_string(),
            &flow.etag(),
            req(json!({"replacement_status_id":replacement})),
        )
        .await
        .unwrap();
    let claim = worker.claim(w).await.unwrap().unwrap();
    assert_eq!(claim.id, job.id);
    worker.run_claim(&claim).await.unwrap();
    assert_eq!(
        h.app.get_issue(&a, w, item.id).await.unwrap().status_id,
        replacement
    );
    worker.pool.close().await;
}
