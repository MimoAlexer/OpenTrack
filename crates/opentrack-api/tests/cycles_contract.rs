//! Cycle lifecycle, concurrency and durable invariants against real PostgreSQL.
#[allow(dead_code)]
mod support;
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{HeaderMap, Request, StatusCode},
};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use support::{Harness, ORIGIN};
use tower::ServiceExt;
use url::form_urlencoded;

#[derive(Clone)]
struct Client {
    cookie: String,
    csrf: String,
}
struct Reply {
    status: StatusCode,
    headers: HeaderMap,
    body: Value,
}
impl Reply {
    #[track_caller]
    fn expect(&self, status: StatusCode) {
        assert_eq!(self.status, status, "{}", self.body);
    }
    fn etag(&self) -> &str {
        self.headers["etag"].to_str().unwrap()
    }
    #[track_caller]
    fn problem(&self, status: StatusCode, code: &str) {
        self.expect(status);
        assert_eq!(self.body["code"], code);
        assert_eq!(self.headers["content-type"], "application/problem+json");
        assert!(!self.headers.contains_key("etag"));
        assert!(!self.headers.contains_key("location"));
    }
}
async fn request(
    router: &Router,
    client: Option<&Client>,
    method: &str,
    path: &str,
    body: Option<Value>,
    extra: &[(&str, &str)],
) -> Reply {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header("origin", ORIGIN);
    if let Some(client) = client {
        builder = builder
            .header("cookie", &client.cookie)
            .header("x-csrf-token", &client.csrf);
    }
    for (key, value) in extra {
        builder = builder.header(*key, *value);
    }
    if body.is_some() {
        builder = builder.header("content-type", "application/json");
    }
    let response = router
        .clone()
        .oneshot(
            builder
                .body(Body::from(
                    body.map(|value| serde_json::to_vec(&value).unwrap())
                        .unwrap_or_default(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    assert_eq!(headers["cache-control"], "no-store");
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    Reply {
        status,
        headers,
        body: if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap()
        },
    }
}

fn client(reply: &Reply) -> Client {
    Client {
        cookie: reply.headers["set-cookie"]
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .into(),
        csrf: reply.body["csrf_token"].as_str().unwrap().into(),
    }
}
async fn setup(h: &Harness) -> (Client, String, String, Value) {
    let out=request(&h.router,None,"POST","/api/v1/setup",Some(json!({"setup_token":h.setup_token,"email":"owner@cycles.example.test","display_name":"Example Owner","password":"Cycle acceptance verification password 2026!","workspace_name":"Cycle contracts","team_name":"Engineering","team_key":"ENG"})),&[]).await;
    out.expect(StatusCode::CREATED);
    let c = client(&out);
    let base = format!(
        "/api/v1/workspaces/{}",
        out.body["workspaces"][0]["id"].as_str().unwrap()
    );
    let boot = request(
        &h.router,
        Some(&c),
        "GET",
        &format!("{base}/bootstrap"),
        None,
        &[],
    )
    .await;
    boot.expect(StatusCode::OK);
    let t = boot.body["teams"][0]["id"].as_str().unwrap().to_owned();
    (c, base, t, boot.body)
}
async fn get(h: &Harness, c: &Client, url: &str) -> Reply {
    let out = request(&h.router, Some(c), "GET", url, None, &[]).await;
    out.expect(StatusCode::OK);
    out
}
async fn issue(h: &Harness, c: &Client, base: &str, t: &str, status: &str, name: &str) -> Reply {
    let out = request(
        &h.router,
        Some(c),
        "POST",
        &format!("{base}/issues"),
        Some(json!({"team_id":t,"title":name,"status_id":status})),
        &[("idempotency-key", &format!("cycle-issue-{name}"))],
    )
    .await;
    out.expect(StatusCode::CREATED);
    out
}

async fn create_cycle(
    h: &Harness,
    c: &Client,
    url: &str,
    start: &str,
    end: &str,
    key: &str,
) -> Reply {
    let r=request(&h.router,Some(c),"POST",url,Some(json!({"name":"Delivery interval","start_date":start,"end_date_exclusive":end,"timezone":"Europe/Berlin"})),&[("idempotency-key",key)]).await;
    r.expect(StatusCode::CREATED);
    r
}
async fn action(
    h: &Harness,
    c: &Client,
    url: &str,
    p: &Reply,
    action: &str,
    body: Option<Value>,
) -> Reply {
    let r = request(
        &h.router,
        Some(c),
        "POST",
        &format!("{url}/{action}"),
        body,
        &[
            ("if-match", p.etag()),
            ("idempotency-key", &format!("cycle-action-{action}")),
        ],
    )
    .await;
    r.expect(StatusCode::OK);
    r
}
async fn attach(h: &Harness, c: &Client, url: &str, p: &Reply, i: &Reply) -> Reply {
    let r = request(
        &h.router,
        Some(c),
        "PUT",
        &format!("{url}/issues/{}", i.body["id"].as_str().unwrap()),
        Some(json!({"included":true,"issue_etag":i.etag()})),
        &[("if-match", p.etag())],
    )
    .await;
    r.expect(StatusCode::OK);
    r
}
async fn live_issue_pages(
    h: &Harness,
    c: &Client,
    base: &str,
    team: &str,
    sort: &str,
    status: Option<&str>,
) -> (Vec<Value>, Option<String>) {
    let mut rows = Vec::new();
    let mut ids = BTreeSet::new();
    let mut cursors = BTreeSet::new();
    let mut cursor: Option<String> = None;
    let mut order = None;
    for _ in 0..6 {
        let path = {
            let mut query = form_urlencoded::Serializer::new(String::new());
            query
                .append_pair("team_id", team)
                .append_pair("sort", sort)
                .append_pair("limit", "1");
            if let Some(status) = status {
                query.append_pair("status_id", status);
            }
            if let Some(cursor) = &cursor {
                query.append_pair("cursor", cursor);
            }
            format!("{base}/issues?{}", query.finish())
        };
        let page = get(h, c, &path).await;
        let data = page.body["data"].as_array().unwrap();
        assert!(data.len() <= 1);
        if sort == "manual" {
            let token = page.body["order_version"].as_str().unwrap().to_owned();
            if let Some(previous) = &order {
                assert_eq!(previous, &token);
            }
            order = Some(token);
        }
        for row in data {
            assert!(
                ids.insert(row["id"].as_str().unwrap().to_owned()),
                "Bounded pages repeated an issue"
            );
            assert!(row["archived_at"].is_null());
            if let Some(status) = status {
                assert_eq!(row["status_id"], status);
            }
            rows.push(row.clone());
        }
        if !page.body["has_more"].as_bool().unwrap() {
            assert!(page.body["next_cursor"].is_null());
            return (rows, order);
        }
        let next = page.body["next_cursor"].as_str().unwrap().to_owned();
        assert!(
            !next.is_empty() && cursors.insert(next.clone()),
            "Bounded cursor did not advance"
        );
        cursor = Some(next);
    }
    panic!("Five-issue parity fixture exceeded its bounded page budget")
}

fn issue_rows_by_id(rows: &[Value]) -> BTreeMap<String, Value> {
    rows.iter()
        .map(|row| (row["id"].as_str().unwrap().to_owned(), row.clone()))
        .collect()
}

async fn live_board_parity(
    h: &Harness,
    c: &Client,
    project: &str,
    scope: &Value,
    counts: [i64; 3],
) {
    let (base, tail) = project.split_once("/teams/").unwrap();
    let team = tail.split('/').next().unwrap();
    let workflow = get(h, c, &format!("{base}/teams/{team}/workflow")).await;
    let statuses = workflow.body["statuses"].as_array().unwrap();
    let (created, _) = live_issue_pages(h, c, base, team, "created", None).await;
    let (manual, _) = live_issue_pages(h, c, base, team, "manual", None).await;
    assert_eq!(issue_rows_by_id(&created), issue_rows_by_id(&manual));
    let expected: BTreeSet<_> = scope["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| !item["excluded"].as_bool().unwrap())
        .map(|item| item["issue_id"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(
        created
            .iter()
            .map(|row| row["id"].as_str().unwrap().to_owned())
            .collect::<BTreeSet<_>>(),
        expected
    );
    let mut observed = [0, 0, 0];
    let mut board_ids = BTreeSet::new();
    for status in statuses {
        let status_id = status["id"].as_str().unwrap();
        let (created_column, _) =
            live_issue_pages(h, c, base, team, "created", Some(status_id)).await;
        let (manual_column, _) =
            live_issue_pages(h, c, base, team, "manual", Some(status_id)).await;
        assert_eq!(
            issue_rows_by_id(&created_column),
            issue_rows_by_id(&manual_column)
        );
        let expected: BTreeMap<_, _> = issue_rows_by_id(&created)
            .into_iter()
            .filter(|(_, row)| row["status_id"] == status_id)
            .collect();
        assert_eq!(issue_rows_by_id(&manual_column), expected);
        let category = match status["category"].as_str().unwrap() {
            "completed" => 0,
            "canceled" => 1,
            _ => 2,
        };
        observed[category] += manual_column.len() as i64;
        for row in manual_column {
            assert!(
                board_ids.insert(row["id"].as_str().unwrap().to_owned()),
                "Issue appeared in two board columns"
            );
        }
    }
    assert_eq!(board_ids, expected);
    assert_eq!(observed, counts);
}

async fn consumer_facts(tx: &mut opentrack_store::Tx, workspace: uuid::Uuid) -> Value {
    sqlx::query_scalar("SELECT jsonb_build_object('registrations',(SELECT coalesce(jsonb_agg(to_jsonb(r) ORDER BY consumer_name,consumer_version),'[]'::jsonb) FROM consumer_registrations r WHERE workspace_id=$1),'checkpoints',(SELECT coalesce(jsonb_agg(to_jsonb(c) ORDER BY consumer_name,consumer_version),'[]'::jsonb) FROM consumer_checkpoints c WHERE workspace_id=$1),'receipts',(SELECT count(*) FROM consumer_receipts WHERE workspace_id=$1))")
        .bind(workspace).fetch_one(&mut **tx).await.unwrap()
}

async fn current_planning_counts(
    h: &Harness,
    c: &Client,
    project: &str,
    milestone: &str,
    cycle: &str,
    counts: [i64; 3],
) -> Reply {
    let [completed, canceled, unfinished] = counts;
    let total = completed + canceled + unfinished;
    for url in [project, milestone] {
        let progress = get(h, c, &format!("{url}/progress")).await;
        for (key, expected) in [
            ("total", total),
            ("eligible", total - canceled),
            ("completed", completed),
            ("canceled", canceled),
            ("open", unfinished),
        ] {
            assert_eq!(progress.body[key], expected, "{url}: {key}");
        }
        assert_eq!(progress.body["unscoped"], total == canceled);
        assert!(progress.body.get("percentage").is_none());
    }
    let scope = get(h, c, &format!("{cycle}/scope")).await;
    assert_eq!(scope.body["kind"], "current");
    assert_eq!(scope.body["scope_count"], total);
    assert_eq!(scope.body["completed_count"], completed);
    assert_eq!(scope.body["canceled_count"], canceled);
    assert_eq!(scope.body["unfinished_count"], unfinished);
    live_board_parity(h, c, project, &scope.body, counts).await;
    scope
}

#[tokio::test]
async fn current_scope_matches_project_and_milestone_without_rewriting_daily_snapshot() {
    let h = Harness::new().await;
    let copy = h.clone();
    let run = tokio::spawn(async move { reporting_parity(&copy).await }).await;
    if let Err(e) = run {
        std::panic::resume_unwind(e.into_panic());
    }
    h.cleanup().await;
}
async fn reporting_parity(h: &Harness) {
    let (c, base, t, boot) = setup(h).await;
    let mut project = request(
        &h.router,
        Some(&c),
        "POST",
        &format!("{base}/teams/{t}/projects"),
        Some(json!({"name":"Reporting parity"})),
        &[("idempotency-key", "parity-project")],
    )
    .await;
    project.expect(StatusCode::CREATED);
    let project_url = project.headers["location"].to_str().unwrap().to_owned();
    let milestone = request(
        &h.router,
        Some(&c),
        "POST",
        &format!("{project_url}/milestones"),
        Some(json!({"title":"Shared reporting scope"})),
        &[
            ("idempotency-key", "parity-milestone"),
            ("if-match", project.etag()),
        ],
    )
    .await;
    milestone.expect(StatusCode::CREATED);
    let milestone_url = milestone.headers["location"].to_str().unwrap();
    project = get(h, &c, &project_url).await;
    let mut cycle = create_cycle(
        h,
        &c,
        &format!("{base}/teams/{t}/cycles"),
        "2026-06-01",
        "2026-06-08",
        "parity-cycle",
    )
    .await;
    let cycle_url = cycle.headers["location"].to_str().unwrap().to_owned();
    current_planning_counts(h, &c, &project_url, milestone_url, &cycle_url, [0, 0, 0]).await;
    assert!(
        get(h, &c, &format!("{cycle_url}/snapshots")).await.body["data"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let statuses = boot["statuses"].as_array().unwrap();
    let status = |category: &str| {
        statuses
            .iter()
            .find(|v| v["team_id"] == t && v["category"] == category)
            .unwrap()["id"]
            .as_str()
            .unwrap()
    };
    let open = issue(h, &c, &base, &t, status("unstarted"), "Parity open").await;
    let done = issue(h, &c, &base, &t, status("completed"), "Parity done").await;
    let canceled = issue(h, &c, &base, &t, status("canceled"), "Parity canceled").await;
    let archived = issue(h, &c, &base, &t, status("unstarted"), "Parity archived").await;
    let deleted = issue(h, &c, &base, &t, status("unstarted"), "Parity deleted").await;
    for task in [&open, &done, &canceled, &archived, &deleted] {
        let membership = format!("{project_url}/issues/{}", task.body["id"].as_str().unwrap());
        project = request(
            &h.router,
            Some(&c),
            "PUT",
            &membership,
            Some(json!({"included":true})),
            &[("if-match", project.etag())],
        )
        .await;
        project.expect(StatusCode::OK);
        project = request(
            &h.router,
            Some(&c),
            "PUT",
            &format!("{membership}/milestone"),
            Some(json!({"milestone_id":milestone.body["id"]})),
            &[("if-match", project.etag())],
        )
        .await;
        project.expect(StatusCode::OK);
        let current = get(
            h,
            &c,
            &format!("{base}/issues/{}", task.body["id"].as_str().unwrap()),
        )
        .await;
        cycle = attach(h, &c, &cycle_url, &cycle, &current).await;
    }
    current_planning_counts(h, &c, &project_url, milestone_url, &cycle_url, [1, 1, 3]).await;
    assert!(
        get(h, &c, &format!("{cycle_url}/snapshots")).await.body["data"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    cycle = action(h, &c, &cycle_url, &cycle, "open", None).await;
    let daily = action(h, &c, &cycle_url, &cycle, "snapshots", None).await;
    assert_eq!(daily.body["kind"], "daily");
    assert_eq!(daily.body["scope_count"], 5);
    assert_eq!(daily.body["completed_count"], 1);
    let w = base.rsplit('/').next().unwrap().parse().unwrap();
    let actor = boot["members"][0]["id"].as_str().unwrap().parse().unwrap();
    let team = t.parse().unwrap();
    let snapshot_id = daily.body["id"]
        .as_str()
        .unwrap()
        .parse::<uuid::Uuid>()
        .unwrap();
    let mut tx = h.lock_team(w, team, actor).await;
    let saved: Value = sqlx::query_scalar(
        "SELECT to_jsonb(s) FROM cycle_snapshots s WHERE workspace_id=$1 AND id=$2",
    )
    .bind(w)
    .bind(snapshot_id)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    let consumers = consumer_facts(&mut tx, w).await;
    tx.rollback().await.unwrap();
    let open_id = open.body["id"].as_str().unwrap();
    let open_url = format!("{base}/issues/{open_id}");
    let (before, order) =
        live_issue_pages(h, &c, &base, &t, "manual", Some(status("unstarted"))).await;
    assert_eq!(
        before.iter().map(|row| &row["id"]).collect::<Vec<_>>(),
        vec![&open.body["id"], &archived.body["id"], &deleted.body["id"]]
    );
    let current = get(h, &c, &open_url).await;
    let reordered = request(
        &h.router,
        Some(&c),
        "POST",
        &format!("{open_url}/position"),
        Some(json!({"order_version":order.unwrap(),"after_id":deleted.body["id"]})),
        &[
            ("if-match", current.etag()),
            ("idempotency-key", "parity-manual-reorder"),
        ],
    )
    .await;
    reordered.expect(StatusCode::OK);
    assert_eq!(
        reordered.body["issue"]["version"]
            .as_str()
            .unwrap()
            .parse::<u64>()
            .unwrap(),
        current.body["version"]
            .as_str()
            .unwrap()
            .parse::<u64>()
            .unwrap()
            + 1
    );
    let (after, _) = live_issue_pages(h, &c, &base, &t, "manual", Some(status("unstarted"))).await;
    assert_eq!(
        after.iter().map(|row| &row["id"]).collect::<Vec<_>>(),
        vec![&archived.body["id"], &deleted.body["id"], &open.body["id"]]
    );
    current_planning_counts(h, &c, &project_url, milestone_url, &cycle_url, [1, 1, 3]).await;
    let current = get(h, &c, &open_url).await;
    let (_, order) = live_issue_pages(h, &c, &base, &t, "manual", None).await;
    let movement = json!({"order_version":order.unwrap(),"status_id":status("completed"),"after_id":done.body["id"]});
    let moved = request(
        &h.router,
        Some(&c),
        "POST",
        &format!("{open_url}/position"),
        Some(movement.clone()),
        &[
            ("if-match", current.etag()),
            ("idempotency-key", "parity-position-complete"),
        ],
    )
    .await;
    moved.expect(StatusCode::OK);
    assert_eq!(moved.body["issue"]["status_id"], status("completed"));
    current_planning_counts(h, &c, &project_url, milestone_url, &cycle_url, [2, 1, 2]).await;
    let (_, settled_order) = live_issue_pages(h, &c, &base, &t, "manual", None).await;
    let replay = request(
        &h.router,
        Some(&c),
        "POST",
        &format!("{open_url}/position"),
        Some(movement),
        &[
            ("if-match", current.etag()),
            ("idempotency-key", "parity-position-complete"),
        ],
    )
    .await;
    replay.expect(StatusCode::OK);
    assert_eq!(replay.body, moved.body);
    assert_eq!(
        live_issue_pages(h, &c, &base, &t, "manual", None).await.1,
        settled_order
    );
    assert_eq!(get(h, &c, &open_url).await.body, moved.body["issue"]);
    current_planning_counts(h, &c, &project_url, milestone_url, &cycle_url, [2, 1, 2]).await;
    let current = get(h, &c, &open_url).await;
    let returned = request(&h.router,Some(&c),"POST",&format!("{open_url}/position"),
        Some(json!({"order_version":settled_order.unwrap(),"status_id":status("unstarted"),"before_id":archived.body["id"]})),
        &[("if-match",current.etag()),("idempotency-key","parity-position-return")]).await;
    returned.expect(StatusCode::OK);
    current_planning_counts(h, &c, &project_url, milestone_url, &cycle_url, [1, 1, 3]).await;
    for (task, change, counts, excluded) in [
        (
            &open,
            json!({"status_id":status("completed")}),
            [2, 1, 2],
            0,
        ),
        (&archived, json!({"archived":true}), [2, 1, 1], 1),
    ] {
        let issue_url = format!("{base}/issues/{}", task.body["id"].as_str().unwrap());
        let current = get(h, &c, &issue_url).await;
        request(
            &h.router,
            Some(&c),
            "PATCH",
            &issue_url,
            Some(change),
            &[("if-match", current.etag())],
        )
        .await
        .expect(StatusCode::OK);
        let scope =
            current_planning_counts(h, &c, &project_url, milestone_url, &cycle_url, counts).await;
        assert_eq!(scope.body["excluded_count"], excluded);
    }
    let repeated = request(
        &h.router,
        Some(&c),
        "POST",
        &format!("{cycle_url}/snapshots"),
        None,
        &[
            ("if-match", cycle.etag()),
            ("idempotency-key", "parity-repeat-daily"),
        ],
    )
    .await;
    repeated.expect(StatusCode::OK);
    assert_eq!(repeated.body, daily.body);
    let issue_url = format!("{base}/issues/{}", deleted.body["id"].as_str().unwrap());
    let current = get(h, &c, &issue_url).await;
    request(
        &h.router,
        Some(&c),
        "DELETE",
        &issue_url,
        None,
        &[("if-match", current.etag())],
    )
    .await
    .expect(StatusCode::NO_CONTENT);
    let scope =
        current_planning_counts(h, &c, &project_url, milestone_url, &cycle_url, [2, 1, 0]).await;
    assert_eq!(scope.body["excluded_count"], 1);
    assert_eq!(scope.body["redacted"], true);
    assert!(
        scope.body["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["issue_id"] != deleted.body["id"])
    );
    for (task, counts) in [
        (&open, [1, 1, 0]),
        (&done, [0, 1, 0]),
        (&canceled, [0, 0, 0]),
    ] {
        let issue_url = format!("{base}/issues/{}", task.body["id"].as_str().unwrap());
        let current = get(h, &c, &issue_url).await;
        request(
            &h.router,
            Some(&c),
            "PATCH",
            &issue_url,
            Some(json!({"archived":true})),
            &[("if-match", current.etag())],
        )
        .await
        .expect(StatusCode::OK);
        current_planning_counts(h, &c, &project_url, milestone_url, &cycle_url, counts).await;
    }
    let history = get(h, &c, &format!("{cycle_url}/snapshots")).await;
    let points = history.body["data"].as_array().unwrap();
    assert_eq!(points.len(), 2);
    assert!(points.iter().all(|point| point["kind"] != "current"));
    let mut tx = h.lock_team(w, team, actor).await;
    let preserved: Value = sqlx::query_scalar(
        "SELECT to_jsonb(s) FROM cycle_snapshots s WHERE workspace_id=$1 AND id=$2",
    )
    .bind(w)
    .bind(snapshot_id)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert_eq!(preserved, saved);
    assert_eq!(consumer_facts(&mut tx, w).await, consumers);
    tx.rollback().await.unwrap();
}

#[tokio::test]
async fn cycle_lifecycle_dst_close_and_reviewed_resumable_carry() {
    let h = Harness::new().await;
    let copy = h.clone();
    let run = tokio::spawn(async move { lifecycle(&copy).await }).await;
    h.cleanup().await;
    if let Err(e) = run {
        std::panic::resume_unwind(e.into_panic());
    }
}
async fn lifecycle(h: &Harness) {
    let (c, base, t, boot) = setup(h).await;
    let directory = format!("{base}/teams/{t}/cycles");
    let mut spring = create_cycle(
        h,
        &c,
        &directory,
        "2026-03-29",
        "2026-03-30",
        "cycle-spring",
    )
    .await;
    let url = spring.headers["location"].to_str().unwrap().to_owned();
    assert_eq!(spring.body["starts_at"], "2026-03-28T23:00:00Z");
    assert_eq!(spring.body["ends_at"], "2026-03-29T22:00:00Z");
    let fall = create_cycle(h, &c, &directory, "2026-10-25", "2026-10-26", "cycle-fall").await;
    assert_eq!(fall.body["starts_at"], "2026-10-24T22:00:00Z");
    assert_eq!(fall.body["ends_at"], "2026-10-25T23:00:00Z");
    let destination = create_cycle(
        h,
        &c,
        &directory,
        "2026-03-30",
        "2026-04-06",
        "cycle-followup",
    )
    .await;
    let dest_url = destination.headers["location"].to_str().unwrap().to_owned();
    let temporary = create_cycle(
        h,
        &c,
        &directory,
        "2026-09-01",
        "2026-09-08",
        "temporary-planned-cycle",
    )
    .await;
    let temporary_url = temporary.headers["location"].to_str().unwrap();
    request(
        &h.router,
        Some(&c),
        "DELETE",
        temporary_url,
        None,
        &[("if-match", temporary.etag())],
    )
    .await
    .expect(StatusCode::NO_CONTENT);
    request(&h.router, Some(&c), "GET", temporary_url, None, &[])
        .await
        .problem(StatusCode::NOT_FOUND, "not_found");
    create_cycle(
        h,
        &c,
        &directory,
        "2026-09-01",
        "2026-09-08",
        "reused-planned-dates",
    )
    .await;
    let statuses = boot["statuses"].as_array().unwrap();
    let status = |category: &str| {
        statuses
            .iter()
            .find(|v| v["team_id"] == t && v["category"] == category)
            .unwrap()["id"]
            .as_str()
            .unwrap()
    };
    let unfinished = issue(h, &c, &base, &t, status("unstarted"), "Unfinished").await;
    let done = issue(h, &c, &base, &t, status("completed"), "Completed").await;
    let canceled = issue(h, &c, &base, &t, status("canceled"), "Canceled").await;
    for i in [&unfinished, &done, &canceled] {
        spring = attach(h, &c, &url, &spring, i).await;
    }
    let page = get(h, &c, &format!("{url}/issues?limit=2")).await;
    assert_eq!(page.body["data"].as_array().unwrap().len(), 2);
    assert_eq!(page.body["has_more"], true);
    assert!(page.body["data"][0]["title"].as_str().is_some());
    let first_id = page.body["data"][0]["id"].as_str().unwrap();
    let canonical = get(h, &c, &format!("{base}/issues/{first_id}")).await;
    assert_eq!(page.body["data"][0]["etag"], canonical.body["etag"]);
    let cursor = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("cursor", page.body["next_cursor"].as_str().unwrap())
        .finish();
    let next = get(h, &c, &format!("{url}/issues?limit=2&{cursor}")).await;
    assert_eq!(next.body["data"].as_array().unwrap().len(), 1);
    assert_eq!(next.body["has_more"], false);
    let stale = spring.etag().to_owned();
    spring = action(h, &c, &url, &spring, "open", None).await;
    let opened = get(h, &c, &format!("{url}/snapshots")).await;
    assert_eq!(opened.body["data"][0]["kind"], "opened");
    assert_eq!(opened.body["data"][0]["scope_count"], 3);
    let first = action(h, &c, &url, &spring, "snapshots", None).await;
    let repeat = action(h, &c, &url, &spring, "snapshots", None).await;
    assert_eq!(first.body, repeat.body);
    let preview = get(h, &c, &format!("{url}/close-preview")).await;
    assert_eq!(preview.body["scope"]["unfinished_count"], 1);
    assert_eq!(preview.body["scope"]["completed_count"], 1);
    assert_eq!(preview.body["scope"]["canceled_count"], 1);
    request(
        &h.router,
        Some(&c),
        "POST",
        &format!("{url}/close"),
        Some(json!({"review_token":preview.body["review_token"]})),
        &[
            ("if-match", &stale),
            ("idempotency-key", "stale-close-request"),
        ],
    )
    .await
    .problem(StatusCode::PRECONDITION_FAILED, "version_conflict");
    let issue_url = format!("{base}/issues/{}", unfinished.body["id"].as_str().unwrap());
    let current = get(h, &c, &issue_url).await;
    let changed = request(
        &h.router,
        Some(&c),
        "PATCH",
        &issue_url,
        Some(json!({"priority":2})),
        &[("if-match", current.etag())],
    )
    .await;
    changed.expect(StatusCode::OK);
    request(
        &h.router,
        Some(&c),
        "POST",
        &format!("{url}/close"),
        Some(json!({"review_token":preview.body["review_token"]})),
        &[
            ("if-match", spring.etag()),
            ("idempotency-key", "stale-preview-request"),
        ],
    )
    .await
    .problem(StatusCode::CONFLICT, "conflict");
    let preview = get(h, &c, &format!("{url}/close-preview")).await;
    let closed = action(
        h,
        &c,
        &url,
        &spring,
        "close",
        Some(json!({"review_token":preview.body["review_token"]})),
    )
    .await;
    let replay = action(
        h,
        &c,
        &url,
        &spring,
        "close",
        Some(json!({"review_token":preview.body["review_token"]})),
    )
    .await;
    assert_eq!(closed.body, replay.body);
    assert_eq!(
        get(h, &c, &issue_url).await.body["status_id"],
        changed.body["status_id"]
    );
    assert_eq!(
        get(h, &c, &issue_url).await.body["version"],
        changed.body["version"]
    );
    assert_eq!(
        get(h, &c, &format!("{url}/scope")).await.body["scope_count"],
        3
    );
    assert_eq!(
        get(h, &c, &format!("{dest_url}/scope")).await.body["scope_count"],
        0
    );
    let closed_cycle = get(h, &c, &url).await;
    request(
        &h.router,
        Some(&c),
        "PATCH",
        &url,
        Some(json!({"name":"Silent correction"})),
        &[("if-match", closed_cycle.etag())],
    )
    .await
    .problem(StatusCode::CONFLICT, "conflict");
    let iid = unfinished.body["id"].clone();
    let carry_body = json!({"report_id":closed.body["id"],"destination_cycle_id":destination.body["id"],"destination_etag":destination.etag(),"items":[{"issue_id":iid,"issue_version":"1"}]});
    let failed = action(
        h,
        &c,
        &url,
        &closed_cycle,
        "carry-forward",
        Some(carry_body.clone()),
    )
    .await;
    assert_eq!(failed.body["state"], "needs_review");
    assert_eq!(failed.body["results"][0]["outcome"], "version_conflict");
    let replay = action(
        h,
        &c,
        &url,
        &closed_cycle,
        "carry-forward",
        Some(carry_body),
    )
    .await;
    assert_eq!(replay.body, failed.body);
    let op_url = format!(
        "{url}/carry-forward/{}",
        failed.body["id"].as_str().unwrap()
    );
    let resume_body = json!({"destination_etag":destination.etag(),"items":[{"issue_id":iid,"issue_version":changed.body["version"]}]});
    let resumed = action(h, &c, &op_url, &failed, "resume", Some(resume_body.clone())).await;
    assert_eq!(resumed.body["state"], "complete");
    assert_eq!(resumed.body["results"][0]["outcome"], "moved");
    let replay = action(h, &c, &op_url, &failed, "resume", Some(resume_body)).await;
    assert_eq!(replay.body, resumed.body);
    assert_eq!(
        get(h, &c, &format!("{dest_url}/scope")).await.body["unfinished_count"],
        1
    );
    assert_eq!(
        get(h, &c, &format!("{url}/scope")).await.body["scope_count"],
        2
    );
    assert_eq!(
        get(
            h,
            &c,
            &format!("{url}/reports/{}", closed.body["id"].as_str().unwrap())
        )
        .await
        .body["scope"]["scope_count"],
        3
    );
    let current = get(h, &c, &url).await;
    let reopened = action(
        h,
        &c,
        &url,
        &current,
        "reopen",
        Some(json!({"reason":"Review a late correction"})),
    )
    .await;
    assert_eq!(reopened.body["state"], "active");
    let history = get(h, &c, &format!("{url}/snapshots")).await;
    assert!(
        history.body["data"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["kind"] == "closed")
    );
    assert_eq!(
        get(
            h,
            &c,
            &format!("{url}/reports/{}", closed.body["id"].as_str().unwrap())
        )
        .await
        .body["cycle"]["state"],
        "closed"
    );
}
async fn invite(h: &Harness, c: &Client, base: &str, role: &str) -> Client {
    let invited = request(
        &h.router,
        Some(c),
        "POST",
        &format!("{base}/invitations"),
        Some(json!({"email":format!("{role}@cycles.example.test"),"role":role,"team_ids":[]})),
        &[("idempotency-key", &format!("cycle-invite-{role}"))],
    )
    .await;
    invited.expect(StatusCode::CREATED);
    let accepted=request(&h.router,None,"POST","/api/v1/invitations/accept",Some(json!({"token":invited.body["token"],"display_name":format!("Example {role}"),"password":"Cycle member verification password 2026!"})),&[]).await;
    accepted.expect(StatusCode::OK);
    client(&accepted)
}

#[tokio::test]
async fn cycle_permissions_validation_private_scope_and_atomic_rollback() {
    let h = Harness::new().await;
    let copy = h.clone();
    let run = tokio::spawn(async move { permissions(&copy).await }).await;
    h.cleanup().await;
    if let Err(e) = run {
        std::panic::resume_unwind(e.into_panic());
    }
}
async fn permissions(h: &Harness) {
    let (owner, base, t, boot) = setup(h).await;
    let directory = format!("{base}/teams/{t}/cycles");
    let member = invite(h, &owner, &base, "member").await;
    let viewer = invite(h, &owner, &base, "viewer").await;
    let body = json!({"name":"Delivery","start_date":"2026-05-01","end_date_exclusive":"2026-05-08","timezone":"UTC"});
    for client in [&member, &viewer] {
        request(
            &h.router,
            Some(client),
            "POST",
            &directory,
            Some(body.clone()),
            &[("idempotency-key", "denied-cycle-create")],
        )
        .await
        .problem(StatusCode::FORBIDDEN, "forbidden");
    }
    for (key, value) in [
        ("timezone", json!("Invalid/Zone")),
        ("timezone", json!("EST")),
        ("end_date_exclusive", json!("2026-05-01")),
        ("end_date_exclusive", json!("2028-05-01")),
    ] {
        let mut invalid = body.clone();
        invalid[key] = value;
        request(
            &h.router,
            Some(&owner),
            "POST",
            &directory,
            Some(invalid),
            &[("idempotency-key", "invalid-cycle-create")],
        )
        .await
        .problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    }
    let made = create_cycle(
        h,
        &owner,
        &directory,
        "2026-05-01",
        "2026-05-08",
        "permission-cycle",
    )
    .await;
    let url = made.headers["location"].to_str().unwrap();
    request(
        &h.router,
        Some(&owner),
        "POST",
        &directory,
        Some(body),
        &[("idempotency-key", "overlap-cycle-create")],
    )
    .await
    .problem(StatusCode::CONFLICT, "conflict");
    let adjacent = create_cycle(
        h,
        &owner,
        &directory,
        "2026-05-08",
        "2026-05-15",
        "adjacent-cycle",
    )
    .await;
    let adjacent_url = adjacent.headers["location"].to_str().unwrap();
    request(
        &h.router,
        Some(&owner),
        "PATCH",
        adjacent_url,
        Some(json!({"start_date":"2026-05-07"})),
        &[("if-match", adjacent.etag())],
    )
    .await
    .problem(StatusCode::CONFLICT, "conflict");
    request(
        &h.router,
        Some(&owner),
        "PATCH",
        url,
        Some(json!({"name":"Missing validator"})),
        &[],
    )
    .await
    .problem(StatusCode::PRECONDITION_REQUIRED, "precondition_required");
    let task = issue(
        h,
        &owner,
        &base,
        &t,
        boot["statuses"][0]["id"].as_str().unwrap(),
        "Member planning",
    )
    .await;
    let seen = get(h, &member, url).await;
    let planned = attach(h, &member, url, &seen, &task).await;
    assert_eq!(planned.body["can_plan"], true);
    assert_eq!(planned.body["can_manage"], false);
    let member_scope = get(h, &member, &format!("{url}/scope")).await;
    assert_eq!(member_scope.body["kind"], "current");
    assert_eq!(member_scope.body["scope_count"], 1);
    request(
        &h.router,
        Some(&member),
        "POST",
        &format!("{url}/open"),
        None,
        &[("if-match", planned.etag())],
    )
    .await
    .problem(StatusCode::FORBIDDEN, "forbidden");
    let seen = get(h, &viewer, url).await;
    assert_eq!(seen.body["can_plan"], false);
    request(
        &h.router,
        Some(&viewer),
        "PUT",
        &format!("{url}/issues/{}", task.body["id"].as_str().unwrap()),
        Some(json!({"included":false,"issue_etag":task.etag()})),
        &[("if-match", seen.etag())],
    )
    .await
    .problem(StatusCode::FORBIDDEN, "forbidden");
    let private = request(
        &h.router,
        Some(&owner),
        "POST",
        &format!("{base}/teams"),
        Some(json!({"name":"Private delivery","key":"PRV","private":true})),
        &[("idempotency-key", "private-cycle-team")],
    )
    .await;
    private.expect(StatusCode::CREATED);
    let private_dir = format!(
        "{base}/teams/{}/cycles",
        private.body["id"].as_str().unwrap()
    );
    let private_cycle = create_cycle(
        h,
        &owner,
        &private_dir,
        "2026-05-01",
        "2026-05-08",
        "private-cycle-create",
    )
    .await;
    let private_url = private_cycle.headers["location"].to_str().unwrap();
    let private_scope = get(h, &owner, &format!("{private_url}/scope")).await;
    assert_eq!(private_scope.body["kind"], "current");
    assert_eq!(private_scope.body["scope_count"], 0);
    for endpoint in [
        &private_dir,
        private_url,
        &format!("{private_url}/scope"),
        &format!("{private_url}/issues"),
        &format!("{private_url}/snapshots"),
        &format!("{private_url}/close-preview"),
    ] {
        request(&h.router, Some(&member), "GET", endpoint, None, &[])
            .await
            .problem(StatusCode::NOT_FOUND, "not_found");
    }
    let w = base.rsplit('/').next().unwrap().parse().unwrap();
    let actor = boot["members"][0]["id"].as_str().unwrap().parse().unwrap();
    let team = t.parse().unwrap();
    let mut tx = h.lock_team(w, team, actor).await;
    sqlx::query("CREATE FUNCTION suppress_cycle_close() RETURNS trigger LANGUAGE plpgsql AS $$BEGIN IF NEW.event_type='cycle.closed' THEN RETURN NULL;END IF;RETURN NEW;END$$").execute(&mut *tx).await.unwrap();
    sqlx::query("CREATE TRIGGER suppress_cycle_close BEFORE INSERT ON workspace_events FOR EACH ROW EXECUTE FUNCTION suppress_cycle_close()").execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    let current = get(h, &owner, url).await;
    let opened = action(h, &owner, url, &current, "open", None).await;
    let preview = get(h, &owner, &format!("{url}/close-preview")).await;
    request(
        &h.router,
        Some(&owner),
        "POST",
        &format!("{url}/close"),
        Some(json!({"review_token":preview.body["review_token"]})),
        &[
            ("if-match", opened.etag()),
            ("idempotency-key", "must-rollback-close"),
        ],
    )
    .await
    .problem(StatusCode::INTERNAL_SERVER_ERROR, "internal_error");
    assert_eq!(get(h, &owner, url).await.body["state"], "active");
    let history = get(h, &owner, &format!("{url}/snapshots")).await;
    assert!(
        !history.body["data"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["kind"] == "closed")
    );
    let mut tx = h.lock_team(w, team, actor).await;
    let receipts:i64=sqlx::query_scalar("SELECT count(*) FROM idempotency_records WHERE workspace_id=$1 AND route LIKE 'cycles.close:%'").bind(w).fetch_one(&mut *tx).await.unwrap();
    assert_eq!(receipts, 0);
    opentrack_store::context(&mut tx, actor, Some(uuid::Uuid::new_v4()))
        .await
        .unwrap();
    for table in [
        "cycles",
        "cycle_issues",
        "cycle_snapshots",
        "cycle_close_reports",
        "cycle_carry_operations",
        "cycle_reopen_reasons",
    ] {
        let count: i64 =
            sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT count(*) FROM {table}")))
                .fetch_one(&mut *tx)
                .await
                .unwrap();
        assert_eq!(count, 0);
    }
    tx.rollback().await.unwrap();
}
