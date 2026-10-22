//! Real PostgreSQL history facts, atomicity, pagination and historical audience.
mod graph_support;
#[allow(dead_code)]
mod support;
use graph_support::*;
use opentrack_domain::*;
use serde_json::{Value, json};
use support::Harness;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_postgres_activity_facts_are_atomic_and_content_free() {
    let h = Harness::new().await;
    let copy = h.clone();
    let run = tokio::spawn(async move { facts(&copy).await }).await;
    h.cleanup().await;
    if let Err(error) = run {
        std::panic::resume_unwind(error.into_panic());
    }
}

async fn facts(h: &Harness) {
    let (owner, w, t) = setup(h).await;
    let initial = issue(
        h,
        &owner,
        w,
        t,
        json!({"title":"Initial content canary","description":"Original body canary"}),
    )
    .await;
    let workflow = h.app.workflow(&owner, w, t).await.unwrap();
    let before = workflow
        .statuses
        .iter()
        .find(|s| s.id == initial.status_id)
        .unwrap();
    let after = workflow
        .statuses
        .iter()
        .find(|s| s.id != initial.status_id && s.category == "started")
        .unwrap();
    let label = h
        .app
        .create_label(
            &owner,
            w,
            "activity-label",
            req(json!({"name":"Example label"})),
        )
        .await
        .unwrap();
    let patch = json!({"title":"Updated content canary","description":"Changed body canary","status_id":after.id,"priority":2,"estimate":5,"due_date":"2026-11-01","assignee_id":owner.user.id,"label_ids":[label.id]});
    for table in ["activity_entries", "workspace_events"] {
        let snapshot_before = snapshot(h, &owner, w).await;
        fault(h, table, "INSERT").await;
        assert!(matches!(
            h.app
                .update_issue(&owner, w, initial.id, &initial.etag(), patch.clone())
                .await,
            Err(Error::Internal)
        ));
        assert_eq!(
            snapshot(h, &owner, w).await,
            snapshot_before,
            "Status, version, history and event must commit together"
        );
        clear(h, table).await;
    }
    let changed = h
        .app
        .update_issue(&owner, w, initial.id, &initial.etag(), patch)
        .await
        .unwrap();
    let entries = h.app.issue_activity(&owner, w, initial.id).await.unwrap();
    assert_eq!(entries.len(), 2);
    let entry = &entries[0];
    assert_eq!(entry["actor_id"], json!(owner.user.id));
    assert_eq!(entry["actor"], json!(owner.user.display_name));
    assert_eq!(
        chrono::DateTime::parse_from_rfc3339(entry["created_at"].as_str().unwrap()).unwrap(),
        changed.updated_at
    );
    assert_eq!(
        entry["changes"],
        json!({
            "fields":["title","description","status_id","priority","assignee_id","label_ids","estimate","due_date"],
            "source":"manual","status_id":{"before":before.id,"after":after.id},
            "status_names":{"before":before.name,"after":after.name},"priority":{"before":0,"after":2},
            "assignee_id":{"before":null,"after":owner.user.id},"label_ids":{"before":[],"after":[label.id]},
            "estimate":{"before":null,"after":5},"due_date":{"before":null,"after":"2026-11-01"}
        })
    );
    assert!(!serde_json::to_string(&entries).unwrap().contains("canary"));
    let mut tx = h.context(owner.user.id, w).await;
    let durable: Vec<Value> = sqlx::query_scalar(
        "SELECT changes FROM activity_entries WHERE workspace_id=$1 AND entity_id=$2",
    )
    .bind(w)
    .bind(initial.id)
    .fetch_all(&mut *tx)
    .await
    .unwrap();
    assert!(!serde_json::to_string(&durable).unwrap().contains("canary"));
    tx.commit().await.unwrap();
    let unchanged = h
        .app
        .update_issue(
            &owner,
            w,
            changed.id,
            &changed.etag(),
            json!({"priority":2,"title":changed.title}),
        )
        .await
        .unwrap();
    assert_eq!(
        h.app.issue_activity(&owner, w, initial.id).await.unwrap()[0]["changes"],
        json!({"fields":[],"source":"manual"}),
        "Submitted unchanged fields are not presented as changes"
    );
    let archived = h
        .app
        .update_issue(
            &owner,
            w,
            unchanged.id,
            &unchanged.etag(),
            json!({"archived":true}),
        )
        .await
        .unwrap();
    assert_eq!(
        h.app.issue_activity(&owner, w, initial.id).await.unwrap()[0]["changes"],
        json!({"fields":["archived"],"archived":{"before":false,"after":true},"source":"manual"})
    );
    h.app
        .update_issue(
            &owner,
            w,
            archived.id,
            &archived.etag(),
            json!({"archived":false}),
        )
        .await
        .unwrap();
    assert_eq!(
        h.app.issue_activity(&owner, w, initial.id).await.unwrap()[0]["changes"]["archived"],
        json!({"before":true,"after":false})
    );
    let all = h.app.issue_activity(&owner, w, initial.id).await.unwrap();
    let mut collected = Vec::new();
    let mut cursor = None;
    loop {
        let page = h
            .app
            .issue_activity_page(
                &owner,
                w,
                initial.id,
                req(json!({"limit":2,"cursor":cursor})),
            )
            .await
            .unwrap();
        collected.extend(page.data);
        if !page.has_more {
            break;
        }
        cursor = page.next_cursor;
        assert!(cursor.is_some());
    }
    assert_eq!(
        collected, all,
        "Pages retain each individual durable event exactly once"
    );
    h.app
        .update_status(
            &owner,
            w,
            t,
            after.id,
            &workflow.etag(),
            req(json!({"name":"Renamed status"})),
        )
        .await
        .unwrap();
    let history = h.app.issue_activity(&owner, w, initial.id).await.unwrap();
    let recorded = history
        .iter()
        .find(|item| item["id"] == entry["id"])
        .unwrap();
    assert_eq!(
        recorded["changes"]["status_names"],
        json!({"before":before.name,"after":after.name}),
        "A later status rename cannot rewrite captured facts"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_postgres_activity_hides_content_and_inaccessible_recorded_teams() {
    let h = Harness::new().await;
    let copy = h.clone();
    let run = tokio::spawn(async move { privacy(&copy).await }).await;
    h.cleanup().await;
    if let Err(error) = run {
        std::panic::resume_unwind(error.into_panic());
    }
}

async fn privacy(h: &Harness) {
    let (owner, w, public) = setup(h).await;
    let reader = person(h, &owner, w, "member").await;
    let private = team(h, &owner, w, true).await;
    let item = issue(h, &owner, w, private, json!({})).await;
    let workflow = h.app.workflow(&owner, w, private).await.unwrap();
    let status = workflow
        .statuses
        .iter()
        .find(|s| s.id != item.status_id)
        .unwrap();
    let changed = h
        .app
        .update_issue(
            &owner,
            w,
            item.id,
            &item.etag(),
            json!({"status_id":status.id}),
        )
        .await
        .unwrap();
    assert!(matches!(
        h.app
            .issue_activity_page(&reader, w, item.id, DiscussionQuery::default())
            .await,
        Err(Error::NotFound)
    ));
    let mut tx = h.context(owner.user.id, w).await;
    sqlx::query("INSERT INTO activity_entries(id,workspace_id,team_id,entity_id,actor_id,event_kind,changes) VALUES($1,$2,$3,$4,$5,'issue.created',$6)")
        .bind(uuid::Uuid::new_v4()).bind(w).bind(private).bind(item.id).bind(owner.user.id)
        .bind(json!({"title":"Historical title canary","description":"Historical body canary","body":{"text":"Unknown content canary"},"unexpected":"Unknown metadata canary","fields":["title","description","Private field canary"]}))
        .execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    let page = h
        .app
        .issue_activity_page(&owner, w, item.id, req(json!({"limit":2})))
        .await
        .unwrap();
    assert!(!serde_json::to_string(&page).unwrap().contains("canary"));
    assert!(
        !serde_json::to_string(&h.app.issue_activity(&owner, w, item.id).await.unwrap())
            .unwrap()
            .contains("canary")
    );
    move_to(h, &owner, w, changed.id, public).await;
    let visible = h
        .app
        .issue_activity_page(&reader, w, item.id, DiscussionQuery::default())
        .await
        .unwrap();
    assert!(
        visible
            .data
            .iter()
            .all(|entry| entry["changes"]["status_names"].is_null())
    );
    assert!(
        visible
            .data
            .iter()
            .all(|entry| entry["event_kind"] != "issue.created")
    );
    let full = h
        .app
        .issue_activity_page(&owner, w, item.id, DiscussionQuery::default())
        .await
        .unwrap();
    assert!(
        full.data
            .iter()
            .any(|entry| entry["changes"]["status_names"].is_object())
    );
    assert!(
        matches!(
            h.app
                .issue_activity_page(
                    &reader,
                    w,
                    item.id,
                    req(json!({"limit":2,"cursor":page.next_cursor}))
                )
                .await,
            Err(Error::Invalid(_))
        ),
        "History cursors are bound to the reader"
    );
}
