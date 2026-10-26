#[allow(dead_code)]
#[path = "../../opentrack-app/tests/support/mod.rs"]
mod support;
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use opentrack_api::{ApiState, router};
use opentrack_app::worker::{Worker, WorkerOptions, WorkerTick};
use opentrack_domain::*;
use serde_json::{Value, json};
use std::{path::PathBuf, sync::Arc};
use support::Harness;
use tower::ServiceExt;
use uuid::Uuid;
fn req<T: serde::de::DeserializeOwned>(v: Value) -> T {
    serde_json::from_value(v).unwrap()
}
async fn get(router: &Router, cookie: &str, path: &str) -> (StatusCode, Value) {
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri(path)
                .header("cookie", format!("opentrack_session={cookie}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    (status, serde_json::from_slice(&body).unwrap())
}
async fn drain(worker: &Worker, w: Uuid) {
    for _ in 0..50 {
        let tick = worker.apply_planning_events(w).await.unwrap();
        if let Some(claim) = worker.claim(w).await.unwrap() {
            assert!(matches!(
                worker.run_claim(&claim).await.unwrap(),
                WorkerTick::Progress | WorkerTick::Completed
            ));
        } else if tick.processed == 0 && !tick.backfilled {
            return;
        }
    }
    panic!("Planning fixture did not drain within its bound");
}
async fn counts(router: &Router, cookie: &str, paths: &[String]) -> Vec<Value> {
    let mut rows = Vec::new();
    for path in paths {
        let (status, body) = get(router, cookie, path).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        rows.push(body);
    }
    rows
}
fn provenance(rows: &[Value], source: &str) {
    for row in rows {
        assert_eq!(row["calculation_source"], source);
        assert_eq!(row["facts_as_of"].is_string(), source == "projection");
        if source == "projection" {
            assert!(
                row["facts_as_of"].as_str().unwrap()
                    <= row["as_of"]
                        .as_str()
                        .or(row["captured_at"].as_str())
                        .unwrap()
            );
        }
    }
}
#[tokio::test]
async fn authorized_planning_falls_back_drains_and_hides_unrelated_private_work() {
    let h = Harness::new().await;
    let r = h.clone();
    let result = tokio::spawn(async move { parity(&r).await }).await;
    if let Err(error) = result {
        std::panic::resume_unwind(error.into_panic());
    }
    h.cleanup().await;
}
async fn parity(h: &Harness) {
    let (session,cookie)=h.app.setup(req(json!({"setup_token":h.setup_token,"email":"owner@example.test","password":"Planning API contract password 947!","display_name":"Example Owner","workspace_name":"Planning API","team_name":"Public","team_key":"PUB"}))).await.unwrap();
    let a = h.app.authenticate(&cookie).await.unwrap();
    let w = session.workspaces[0].id;
    let t = h
        .app
        .create_team(
            &a,
            w,
            "private-planning",
            req(json!({"name":"Private planning","key":"PLAN","private":true})),
        )
        .await
        .unwrap()
        .id;
    let router = router(ApiState {
        app: h.app.clone(),
        attachments: None,
        origins: Arc::new(vec!["https://opentrack.contract.invalid".into()]),
        secure_cookie: true,
        static_dir: Arc::new(PathBuf::from("unused-contract-static")),
    });
    let mut project = h
        .app
        .create_project(
            &a,
            w,
            t,
            "planning-project",
            req(json!({"name":"Planning projection"})),
        )
        .await
        .unwrap();
    let m = h
        .app
        .create_milestone(
            &a,
            w,
            t,
            project.id,
            "planning-milestone",
            &project.etag,
            req(json!({"title":"Shared source"})),
        )
        .await
        .unwrap();
    project = h.app.get_project(&a, w, t, project.id).await.unwrap();
    let mut cycle=h.app.create_cycle(&a,w,t,"planning-cycle",req(json!({"name":"Current interval","start_date":"2026-10-01","end_date_exclusive":"2026-10-15","timezone":"UTC"}))).await.unwrap();
    let statuses = h.app.workflow(&a, w, t).await.unwrap().statuses;
    let mut issues = Vec::new();
    for (index, category) in ["unstarted", "completed", "canceled"].iter().enumerate() {
        let status = statuses
            .iter()
            .find(|s| s.category == *category)
            .unwrap()
            .id;
        let issue=h.app.create_issue(&a,w,&format!("planning-issue-{index}"),req(json!({"team_id":t,"title":format!("Planning source {index}"),"status_id":status,"estimate":if index==0 {Some(5)}else{None}}))).await.unwrap();
        project = h
            .app
            .set_project_issue(
                &a,
                w,
                t,
                project.id,
                issue.id,
                &project.etag,
                ProjectMembership { included: true },
            )
            .await
            .unwrap();
        project = h
            .app
            .assign_milestone(
                &a,
                w,
                t,
                project.id,
                issue.id,
                &project.etag,
                MilestoneAssignment {
                    milestone_id: Some(m.id),
                },
            )
            .await
            .unwrap();
        let current = h.app.get_issue(&a, w, issue.id).await.unwrap();
        cycle = h
            .app
            .set_cycle_issue(
                &a,
                w,
                t,
                cycle.id,
                issue.id,
                &cycle.etag,
                CycleMembership {
                    included: true,
                    issue_etag: current.etag(),
                },
            )
            .await
            .unwrap();
        issues.push(issue.id);
    }
    let base = format!("/api/v1/workspaces/{w}/teams/{t}");
    let project_path = format!("{base}/projects/{}", project.id);
    let paths = vec![
        format!("{project_path}/progress"),
        format!("{project_path}/milestones/{}/progress", m.id),
        format!("{base}/cycles/{}/scope", cycle.id),
    ];
    let live = counts(&router, &cookie, &paths).await;
    provenance(&live, "live");
    let worker = Worker::connect(
        &h.worker_url().await,
        WorkerOptions {
            max_connections: 1,
            ..WorkerOptions::default()
        },
    )
    .await
    .unwrap();
    drain(&worker, w).await;
    let projected = counts(&router, &cookie, &paths).await;
    provenance(&projected, "projection");
    for row in &projected[..2] {
        assert_eq!(
            (
                row["total"].as_i64().unwrap(),
                row["eligible"].as_i64().unwrap(),
                row["open"].as_i64().unwrap()
            ),
            (3, 2, 1)
        );
    }
    assert_eq!(projected[2]["scope_count"], 3);
    assert_eq!(projected[2]["unfinished_count"], 1);
    assert_eq!(projected[2]["estimate_total"], 5);
    let (_, page) = get(&router, &cookie, &format!("{project_path}/milestones")).await;
    assert_eq!(
        page["data"][0]["progress"]["calculation_source"],
        "projection"
    );
    cycle = h
        .app
        .open_cycle(&a, w, t, cycle.id, &cycle.etag)
        .await
        .unwrap();
    let current_project = h.app.get_project(&a, w, t, project.id).await.unwrap();
    let empty = h
        .app
        .create_milestone(
            &a,
            w,
            t,
            project.id,
            "empty-milestone",
            &current_project.etag,
            req(json!({"title":"Empty scope"})),
        )
        .await
        .unwrap();
    let (_, empty_counts) = get(
        &router,
        &cookie,
        &format!("{project_path}/milestones/{}/progress", empty.id),
    )
    .await;
    provenance(std::slice::from_ref(&empty_counts), "live");
    assert_eq!(empty_counts["total"], 0);
    assert_eq!(empty_counts["unscoped"], true);
    let (_, page) = get(
        &router,
        &cookie,
        &format!("{project_path}/milestones?limit=1"),
    )
    .await;
    assert_eq!(page["data"].as_array().unwrap().len(), 1);
    assert_eq!(page["has_more"], true);
    let next = page["next_cursor"].as_str().unwrap();
    let query = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("limit", "1")
        .append_pair("cursor", next)
        .finish();
    let (_, second_page) = get(
        &router,
        &cookie,
        &format!("{project_path}/milestones?{query}"),
    )
    .await;
    assert_eq!(
        second_page["data"][0]["progress"]["calculation_source"],
        "live"
    );
    assert!(second_page["data"][0]["progress"]["facts_as_of"].is_null());
    let empty_cycle=h.app.create_cycle(&a,w,t,"empty-cycle",req(json!({"name":"Empty cycle","start_date":"2026-10-15","end_date_exclusive":"2026-11-01","timezone":"UTC"}))).await.unwrap();
    let (_, empty_scope) = get(
        &router,
        &cookie,
        &format!("{base}/cycles/{}/scope", empty_cycle.id),
    )
    .await;
    provenance(std::slice::from_ref(&empty_scope), "live");
    assert_eq!(empty_scope["scope_count"], 0);
    assert!(empty_scope["captured_at"].is_string());
    let snapshot = h
        .app
        .capture_cycle_snapshot(&a, w, t, cycle.id, &cycle.etag)
        .await
        .unwrap();
    assert_eq!(
        snapshot.calculation_source,
        PlanningCalculationSource::Snapshot
    );
    assert!(snapshot.facts_as_of.is_none());
    let current = h.app.get_issue(&a, w, issues[0]).await.unwrap();
    h.app
        .update_issue(
            &a,
            w,
            current.id,
            &current.etag(),
            json!({"status_id":statuses.iter().find(|s|s.category=="completed").unwrap().id}),
        )
        .await
        .unwrap();
    let fallback = counts(&router, &cookie, &paths).await;
    provenance(&fallback, "live");
    assert_eq!(fallback[0]["completed"], 2);
    assert_eq!(fallback[1]["open"], 0);
    assert_eq!(fallback[2]["completed_count"], 2);
    assert!(fallback[0]["as_of"] != projected[0]["as_of"]);
    drain(&worker, w).await;
    let fresh = counts(&router, &cookie, &paths).await;
    provenance(&fresh, "projection");
    let history = h
        .app
        .cycle_snapshots(&a, w, t, cycle.id, CycleQuery::default())
        .await
        .unwrap();
    assert_eq!(
        history
            .data
            .iter()
            .find(|s| s.id == snapshot.id)
            .unwrap()
            .completed_count,
        1
    );
    let hidden = h
        .app
        .create_team(
            &a,
            w,
            "hidden-planning",
            req(json!({"name":"Hidden","key":"HID","private":true})),
        )
        .await
        .unwrap()
        .id;
    let hidden_issue = h
        .app
        .create_issue(
            &a,
            w,
            "hidden-issue",
            req(json!({"team_id":hidden,"title":"Hidden source"})),
        )
        .await
        .unwrap();
    let mut tx = h.context(a.user.id, w).await;
    sqlx::query("DELETE FROM team_members WHERE workspace_id=$1 AND team_id=$2 AND user_id=$3")
        .bind(w)
        .bind(hidden)
        .bind(a.user.id)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let mut tx = h.context(a.user.id, w).await;
    sqlx::query("UPDATE issues SET version=version+1 WHERE workspace_id=$1 AND id=$2")
        .bind(w)
        .bind(hidden_issue.id)
        .execute(&mut *tx)
        .await
        .unwrap();
    opentrack_store::identity_event(
        &mut tx,
        opentrack_store::IdentityEvent {
            workspace: w,
            actor: a.user.id,
            entity: hidden_issue.id,
            version: 2,
            team: Some(hidden),
            action: "issue.updated",
            access_changed: false,
        },
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    let unchanged = counts(&router, &cookie, &paths).await;
    provenance(&unchanged, "projection");
    for (before, after) in fresh.iter().zip(&unchanged) {
        assert_eq!(before["facts_as_of"], after["facts_as_of"]);
    }
    drain(&worker, w).await;
    let unchanged = counts(&router, &cookie, &paths).await;
    provenance(&unchanged, "projection");
    for (before, after) in fresh.iter().zip(&unchanged) {
        assert_eq!(before["facts_as_of"], after["facts_as_of"]);
    }
    let operator = opentrack_app::Operator::connect(&h.operator_url().await)
        .await
        .unwrap();
    operator
        .rebuild_planning_counts(
            w,
            "Recheck unchanged visible facts during private reconciliation",
        )
        .await
        .unwrap();
    let rebuilding = counts(&router, &cookie, &paths).await;
    provenance(&rebuilding, "projection");
    for (before, after) in fresh.iter().zip(&rebuilding) {
        assert_eq!(before["facts_as_of"], after["facts_as_of"]);
    }
    drain(&worker, w).await;
    let rebuilt = counts(&router, &cookie, &paths).await;
    provenance(&rebuilt, "projection");
    for (before, after) in fresh.iter().zip(&rebuilt) {
        assert_eq!(before["facts_as_of"], after["facts_as_of"]);
    }
    let mut tx = h.context(a.user.id, w).await;
    sqlx::query("DELETE FROM team_members WHERE workspace_id=$1 AND team_id=$2 AND user_id=$3")
        .bind(w)
        .bind(t)
        .bind(a.user.id)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    for path in &paths {
        let (status, body) = get(&router, &cookie, path).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert!(body.get("facts_as_of").is_none());
        assert!(body.get("calculation_source").is_none());
    }
    worker.pool.close().await;
}
