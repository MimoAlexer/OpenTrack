//! Milestone persistence, ordering, authorization and detach contracts against PostgreSQL.
#[allow(dead_code)]
mod support;
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{HeaderMap, Request, StatusCode},
};
use serde_json::{Value, json};
use support::{Harness, ORIGIN};
use tower::ServiceExt;

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
    let out=request(&h.router,None,"POST","/api/v1/setup",Some(json!({"setup_token":h.setup_token,"email":"owner@projects.example.test","display_name":"Example Owner","password":"Project acceptance verification password 2026!","workspace_name":"Project contracts","team_name":"Engineering","team_key":"ENG"})),&[]).await;
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
async fn create(h: &Harness, c: &Client, url: &str, name: &str, key: &str) -> Reply {
    let out = request(
        &h.router,
        Some(c),
        "POST",
        url,
        Some(json!({"name":name})),
        &[("idempotency-key", key)],
    )
    .await;
    out.expect(StatusCode::CREATED);
    out
}
async fn issue(h: &Harness, c: &Client, base: &str, t: &str, status: &str, name: &str) -> Reply {
    let out = request(
        &h.router,
        Some(c),
        "POST",
        &format!("{base}/issues"),
        Some(json!({"team_id":t,"title":name,"status_id":status})),
        &[("idempotency-key", &format!("project-issue-{name}"))],
    )
    .await;
    out.expect(StatusCode::CREATED);
    out
}
async fn attach(h: &Harness, c: &Client, url: &str, p: &Reply, i: &Reply) -> Reply {
    let out = request(
        &h.router,
        Some(c),
        "PUT",
        &format!("{url}/issues/{}", i.body["id"].as_str().unwrap()),
        Some(json!({"included":true})),
        &[("if-match", p.etag())],
    )
    .await;
    out.expect(StatusCode::OK);
    out
}
async fn milestone(h: &Harness, c: &Client, p: &str, title: &str) -> Reply {
    let project = get(h, c, p).await;
    let out = request(
        &h.router,
        Some(c),
        "POST",
        &format!("{p}/milestones"),
        Some(json!({"title":title,"target_date":"2026-12-31"})),
        &[
            ("idempotency-key", &format!("milestone-{title}")),
            ("if-match", project.etag()),
        ],
    )
    .await;
    out.expect(StatusCode::CREATED);
    out
}
async fn assign(h: &Harness, c: &Client, p: &str, i: &Reply, m: &Reply) -> Reply {
    let project = get(h, c, p).await;
    let out = request(
        &h.router,
        Some(c),
        "PUT",
        &format!("{p}/issues/{}/milestone", i.body["id"].as_str().unwrap()),
        Some(json!({"milestone_id":m.body["id"]})),
        &[("if-match", project.etag())],
    )
    .await;
    out.expect(StatusCode::OK);
    out
}
#[tokio::test]
async fn milestone_lifecycle_scope_progress_and_order() {
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
    let dir = format!("{base}/teams/{t}/projects");
    let mut project = create(h, &c, &dir, "Milestone project", "milestone-project").await;
    let p = project.headers["location"].to_str().unwrap().to_owned();
    let m = milestone(h, &c, &p, "First outcome").await;
    let url = m.headers["location"].to_str().unwrap();
    assert_eq!(get(h, &c, url).await.body, m.body);
    let empty = get(h, &c, &format!("{url}/progress")).await;
    assert_eq!(empty.body["eligible"], 0);
    assert_eq!(empty.body["unscoped"], true);
    assert!(empty.body.get("percentage").is_none());
    let replay = request(
        &h.router,
        Some(&c),
        "POST",
        &format!("{p}/milestones"),
        Some(json!({"title":"First outcome","target_date":"2026-12-31"})),
        &[
            ("idempotency-key", "milestone-First outcome"),
            ("if-match", project.etag()),
        ],
    )
    .await;
    replay.expect(StatusCode::CREATED);
    assert_eq!(replay.body, m.body);
    request(
        &h.router,
        Some(&c),
        "POST",
        &format!("{p}/milestones"),
        Some(json!({"title":"Different outcome"})),
        &[
            ("idempotency-key", "milestone-First outcome"),
            ("if-match", project.etag()),
        ],
    )
    .await
    .problem(StatusCode::CONFLICT, "conflict");
    let second = milestone(h, &c, &p, "Second outcome").await;
    let third = milestone(h, &c, &p, "Third outcome").await;
    let first_page = get(h, &c, &format!("{p}/milestones?limit=2")).await;
    assert_eq!(first_page.body["data"][0]["milestone"]["id"], m.body["id"]);
    assert_eq!(first_page.body["has_more"], true);
    let query = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("cursor", first_page.body["next_cursor"].as_str().unwrap())
        .finish();
    assert_eq!(
        get(h, &c, &format!("{p}/milestones?limit=2&{query}"))
            .await
            .body["data"][0]["milestone"]["id"],
        third.body["id"]
    );
    project = get(h, &c, &p).await;
    let old_etag = project.etag().to_owned();
    request(
        &h.router,
        Some(&c),
        "PUT",
        &format!("{p}/milestones/order"),
        Some(json!({"milestone_ids":[m.body["id"],m.body["id"],third.body["id"]]})),
        &[("if-match", project.etag())],
    )
    .await
    .problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    project = request(
        &h.router,
        Some(&c),
        "PUT",
        &format!("{p}/milestones/order"),
        Some(json!({"milestone_ids":[second.body["id"],m.body["id"],third.body["id"]]})),
        &[("if-match", project.etag())],
    )
    .await;
    project.expect(StatusCode::OK);
    request(
        &h.router,
        Some(&c),
        "PUT",
        &format!("{p}/milestones/order"),
        Some(json!({"milestone_ids":[m.body["id"],second.body["id"],third.body["id"]]})),
        &[("if-match", &old_etag)],
    )
    .await
    .problem(StatusCode::PRECONDITION_FAILED, "version_conflict");
    assert_eq!(
        get(h, &c, &format!("{p}/milestones")).await.body["data"][0]["milestone"]["id"],
        second.body["id"]
    );
    // Reorder invalidates page cursors as well as stale entity validators.
    request(
        &h.router,
        Some(&c),
        "GET",
        &format!("{p}/milestones?limit=2&{query}"),
        None,
        &[],
    )
    .await
    .problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    let status = |category: &str| {
        boot["statuses"]
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["team_id"] == t && v["category"] == category)
            .unwrap()["id"]
            .as_str()
            .unwrap()
    };
    let open = issue(h, &c, &base, &t, status("unstarted"), "Open milestone work").await;
    let done = issue(h, &c, &base, &t, status("completed"), "Done milestone work").await;
    let canceled = issue(
        h,
        &c,
        &base,
        &t,
        status("canceled"),
        "Canceled milestone work",
    )
    .await;
    let archived = issue(
        h,
        &c,
        &base,
        &t,
        status("unstarted"),
        "Archived milestone work",
    )
    .await;
    let deleted = issue(
        h,
        &c,
        &base,
        &t,
        status("unstarted"),
        "Deleted milestone work",
    )
    .await;
    for i in [&open, &done, &canceled, &archived, &deleted] {
        attach(h, &c, &p, &project, i).await;
        project = assign(h, &c, &p, i, &m).await;
    }
    request(
        &h.router,
        Some(&c),
        "PATCH",
        &format!("{base}/issues/{}", archived.body["id"].as_str().unwrap()),
        Some(json!({"archived":true})),
        &[("if-match", archived.etag())],
    )
    .await
    .expect(StatusCode::OK);
    request(
        &h.router,
        Some(&c),
        "DELETE",
        &format!("{base}/issues/{}", deleted.body["id"].as_str().unwrap()),
        None,
        &[("if-match", deleted.etag())],
    )
    .await
    .expect(StatusCode::NO_CONTENT);
    let counts = get(h, &c, &format!("{url}/progress")).await;
    for (k, v) in [
        ("total", 3),
        ("eligible", 2),
        ("completed", 1),
        ("canceled", 1),
        ("open", 1),
    ] {
        assert_eq!(counts.body[k], v, "{k}");
    }
    let overview = get(h, &c, &format!("{p}/milestones")).await;
    let listed = overview.body["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["milestone"]["id"] == m.body["id"])
        .unwrap();
    for k in [
        "total",
        "eligible",
        "completed",
        "canceled",
        "open",
        "unscoped",
    ] {
        assert_eq!(listed["progress"][k], counts.body[k], "{k}");
    }
    let visible = get(h, &c, &format!("{p}/issues")).await;
    assert!(
        visible.body["data"]
            .as_array()
            .unwrap()
            .iter()
            .all(|i| i["milestone_id"] == m.body["id"])
    );
    assign(h, &c, &p, &canceled, &second).await;
    let all_canceled = get(
        h,
        &c,
        &format!("{}/progress", second.headers["location"].to_str().unwrap()),
    )
    .await;
    assert_eq!(all_canceled.body["canceled"], 1);
    assert_eq!(all_canceled.body["eligible"], 0);
    assert_eq!(all_canceled.body["unscoped"], true);
    assign(h, &c, &p, &canceled, &m).await;
    let current = get(h, &c, url).await;
    let completed = request(
        &h.router,
        Some(&c),
        "PATCH",
        url,
        Some(json!({"state":"completed","target_date":null})),
        &[("if-match", current.etag())],
    )
    .await;
    completed.expect(StatusCode::OK);
    assert!(completed.body["completed_at"].is_string());
    assert!(completed.body["target_date"].is_null());
    assert_eq!(
        get(
            h,
            &c,
            &format!("{base}/issues/{}", open.body["id"].as_str().unwrap())
        )
        .await
        .body["status_id"],
        open.body["status_id"]
    );
    request(
        &h.router,
        Some(&c),
        "PATCH",
        url,
        Some(json!({"title":"Stale rename"})),
        &[("if-match", current.etag())],
    )
    .await
    .problem(StatusCode::PRECONDITION_FAILED, "version_conflict");
    let reopened = request(
        &h.router,
        Some(&c),
        "PATCH",
        url,
        Some(json!({"state":"planned"})),
        &[("if-match", completed.etag())],
    )
    .await;
    reopened.expect(StatusCode::OK);
    assert!(reopened.body["completed_at"].is_null());
    let other = create(
        h,
        &c,
        &dir,
        "Other milestone project",
        "other-milestone-project",
    )
    .await;
    let other_p = other.headers["location"].to_str().unwrap();
    let foreign = milestone(h, &c, other_p, "Foreign outcome").await;
    project = get(h, &c, &p).await;
    request(
        &h.router,
        Some(&c),
        "PUT",
        &format!("{p}/issues/{}/milestone", open.body["id"].as_str().unwrap()),
        Some(json!({"milestone_id":foreign.body["id"]})),
        &[("if-match", project.etag())],
    )
    .await
    .problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    let w = base.rsplit('/').next().unwrap().parse().unwrap();
    let actor = boot["members"][0]["id"].as_str().unwrap().parse().unwrap();
    let mut tx = h.lock_team(w, t.parse().unwrap(), actor).await;
    let error = sqlx::query(
        "UPDATE project_issues SET milestone_id=$3 WHERE workspace_id=$1 AND issue_id=$2",
    )
    .bind(w)
    .bind(
        open.body["id"]
            .as_str()
            .unwrap()
            .parse::<uuid::Uuid>()
            .unwrap(),
    )
    .bind(
        foreign.body["id"]
            .as_str()
            .unwrap()
            .parse::<uuid::Uuid>()
            .unwrap(),
    )
    .execute(&mut *tx)
    .await
    .unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("23503")
    );
    tx.rollback().await.unwrap();
    request(
        &h.router,
        Some(&c),
        "DELETE",
        url,
        None,
        &[("if-match", reopened.etag())],
    )
    .await
    .expect(StatusCode::NO_CONTENT);
    get(
        h,
        &c,
        &format!("{base}/issues/{}", open.body["id"].as_str().unwrap()),
    )
    .await;
    assert_eq!(
        get(h, &c, &format!("{p}/issues")).await.body["data"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    let mut tx = h.lock_team(w, t.parse().unwrap(), actor).await;
    let attached: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM project_issues WHERE workspace_id=$1 AND milestone_id=$2",
    )
    .bind(w)
    .bind(
        m.body["id"]
            .as_str()
            .unwrap()
            .parse::<uuid::Uuid>()
            .unwrap(),
    )
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert_eq!(attached, 0);
    tx.rollback().await.unwrap();
    // Project detachment removes milestone assignment without deleting either entity.
    project = assign(h, &c, &p, &open, &second).await;
    project = request(
        &h.router,
        Some(&c),
        "PUT",
        &format!("{p}/issues/{}", open.body["id"].as_str().unwrap()),
        Some(json!({"included":false})),
        &[("if-match", project.etag())],
    )
    .await;
    project.expect(StatusCode::OK);
    assert_eq!(
        get(
            h,
            &c,
            &format!("{}/progress", second.headers["location"].to_str().unwrap())
        )
        .await
        .body["eligible"],
        0
    );
    let archived_project = request(
        &h.router,
        Some(&c),
        "PATCH",
        &p,
        Some(json!({"archived":true})),
        &[("if-match", project.etag())],
    )
    .await;
    archived_project.expect(StatusCode::OK);
    request(
        &h.router,
        Some(&c),
        "POST",
        &format!("{p}/milestones"),
        Some(json!({"title":"Blocked archived"})),
        &[
            ("idempotency-key", "milestone-archived-denied"),
            ("if-match", archived_project.etag()),
        ],
    )
    .await
    .problem(StatusCode::CONFLICT, "conflict");
}
async fn invite(h: &Harness, c: &Client, base: &str, role: &str) -> Client {
    let invited = request(
        &h.router,
        Some(c),
        "POST",
        &format!("{base}/invitations"),
        Some(json!({"email":format!("{role}@milestones.example.test"),"role":role,"team_ids":[]})),
        &[("idempotency-key", &format!("milestone-invite-{role}"))],
    )
    .await;
    invited.expect(StatusCode::CREATED);
    let accepted=request(&h.router,None,"POST","/api/v1/invitations/accept",Some(json!({"token":invited.body["token"],"display_name":format!("Example {role}"),"password":"Synthetic milestone contract password 2026!"})),&[]).await;
    accepted.expect(StatusCode::OK);
    client(&accepted)
}
#[tokio::test]
async fn milestone_authorization_and_atomic_audit_events() {
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
    let member = invite(h, &owner, &base, "member").await;
    let viewer = invite(h, &owner, &base, "viewer").await;
    let dir = format!("{base}/teams/{t}/projects");
    let project = create(h, &owner, &dir, "Managed milestones", "managed-milestones").await;
    let p = project.headers["location"].to_str().unwrap();
    let m = milestone(h, &owner, p, "Managed outcome").await;
    let url = m.headers["location"].to_str().unwrap();
    let seen = get(h, &viewer, url).await;
    assert_eq!(seen.body["can_manage"], false);
    assert_eq!(seen.body["can_assign"], false);
    get(h, &viewer, &format!("{p}/milestones")).await;
    get(h, &viewer, &format!("{url}/progress")).await;
    for c in [&member, &viewer] {
        let snapshot = get(h, c, p).await;
        request(
            &h.router,
            Some(c),
            "POST",
            &format!("{p}/milestones"),
            Some(json!({"title":"Denied structure"})),
            &[
                ("idempotency-key", "milestone-structure-denied"),
                ("if-match", snapshot.etag()),
            ],
        )
        .await
        .problem(StatusCode::FORBIDDEN, "forbidden");
        let snapshot = get(h, c, url).await;
        request(
            &h.router,
            Some(c),
            "PATCH",
            url,
            Some(json!({"title":"Denied rename"})),
            &[("if-match", snapshot.etag())],
        )
        .await
        .problem(StatusCode::FORBIDDEN, "forbidden");
        request(
            &h.router,
            Some(c),
            "DELETE",
            url,
            None,
            &[("if-match", snapshot.etag())],
        )
        .await
        .problem(StatusCode::FORBIDDEN, "forbidden");
    }
    let member_boot = get(h, &owner, &format!("{base}/bootstrap")).await;
    let member_id = member_boot.body["members"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["email"] == "member@milestones.example.test")
        .unwrap()["id"]
        .clone();
    let project = get(h, &owner, p).await;
    request(
        &h.router,
        Some(&owner),
        "PATCH",
        p,
        Some(json!({"lead_id":member_id})),
        &[("if-match", project.etag())],
    )
    .await
    .expect(StatusCode::OK);
    let lead_created = milestone(h, &member, p, "Lead outcome").await;
    assert_eq!(lead_created.body["can_manage"], true);
    let s = boot["statuses"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["team_id"] == t && v["is_default"] == true)
        .unwrap()["id"]
        .as_str()
        .unwrap();
    let task = issue(h, &owner, &base, &t, s, "Contributed scope").await;
    let project = get(h, &owner, p).await;
    attach(h, &owner, p, &project, &task).await;
    assign(h, &member, p, &task, &m).await;
    let project = get(h, &viewer, p).await;
    request(
        &h.router,
        Some(&viewer),
        "PUT",
        &format!("{p}/issues/{}/milestone", task.body["id"].as_str().unwrap()),
        Some(json!({"milestone_id":null})),
        &[("if-match", project.etag())],
    )
    .await
    .problem(StatusCode::FORBIDDEN, "forbidden");
    let bad = Client {
        csrf: "invalid".into(),
        ..owner.clone()
    };
    request(
        &h.router,
        Some(&bad),
        "DELETE",
        url,
        None,
        &[("if-match", m.etag())],
    )
    .await
    .problem(StatusCode::FORBIDDEN, "forbidden");
    request(
        &h.router,
        Some(&owner),
        "PATCH",
        url,
        Some(json!({"state":null})),
        &[("if-match", m.etag())],
    )
    .await
    .problem(StatusCode::BAD_REQUEST, "bad_request");
    let private = request(
        &h.router,
        Some(&owner),
        "POST",
        &format!("{base}/teams"),
        Some(json!({"name":"Private milestone team","key":"PVT","private":true})),
        &[("idempotency-key", "milestone-private-team")],
    )
    .await;
    private.expect(StatusCode::CREATED);
    let private_dir = format!(
        "{base}/teams/{}/projects",
        private.body["id"].as_str().unwrap()
    );
    let secret = create(
        h,
        &owner,
        &private_dir,
        "Private outcomes",
        "private-outcomes",
    )
    .await;
    let secret_p = secret.headers["location"].to_str().unwrap();
    let hidden = milestone(h, &owner, secret_p, "Hidden outcome").await;
    let hidden_url = hidden.headers["location"].to_str().unwrap();
    for path in [
        format!("{secret_p}/milestones"),
        hidden_url.to_owned(),
        format!("{hidden_url}/progress"),
    ] {
        request(&h.router, Some(&member), "GET", &path, None, &[])
            .await
            .problem(StatusCode::NOT_FOUND, "not_found");
    }
    request(
        &h.router,
        Some(&member),
        "PATCH",
        hidden_url,
        Some(json!({"title":"Probe"})),
        &[("if-match", hidden.etag())],
    )
    .await
    .problem(StatusCode::NOT_FOUND, "not_found");
    let w = base.rsplit('/').next().unwrap().parse().unwrap();
    let actor = boot["members"][0]["id"].as_str().unwrap().parse().unwrap();
    let mut tx = h.lock_team(w, t.parse().unwrap(), actor).await;
    let audits: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_entries WHERE workspace_id=$1 AND action='milestone.created'",
    )
    .bind(w)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert_eq!(audits, 3);
    sqlx::query("CREATE FUNCTION suppress_milestone_event() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.event_type='milestone.created' THEN RETURN NULL; END IF; RETURN NEW; END $$").execute(&mut *tx).await.unwrap();
    sqlx::query("CREATE TRIGGER suppress_milestone_event BEFORE INSERT ON workspace_events FOR EACH ROW EXECUTE FUNCTION suppress_milestone_event()").execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    let snapshot = get(h, &owner, p).await;
    request(
        &h.router,
        Some(&owner),
        "POST",
        &format!("{p}/milestones"),
        Some(json!({"title":"Must roll back"})),
        &[
            ("idempotency-key", "milestone-event-rollback"),
            ("if-match", snapshot.etag()),
        ],
    )
    .await
    .problem(StatusCode::INTERNAL_SERVER_ERROR, "internal_error");
    assert_eq!(get(h, &owner, p).await.etag(), snapshot.etag());
    assert_eq!(
        get(h, &owner, &format!("{p}/milestones")).await.body["data"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let mut tx = h.lock_team(w, t.parse().unwrap(), actor).await;
    let audits_after: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_entries WHERE workspace_id=$1 AND action='milestone.created'",
    )
    .bind(w)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert_eq!(audits_after, audits);
    opentrack_store::context(&mut tx, actor, Some(uuid::Uuid::new_v4()))
        .await
        .unwrap();
    let exposed: i64 = sqlx::query_scalar("SELECT count(*) FROM milestones")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert_eq!(exposed, 0);
    tx.rollback().await.unwrap();
}
#[tokio::test]
async fn milestone_delete_and_assign_are_serialized_and_rechecked() {
    let h = Harness::new().await;
    let copy = h.clone();
    let run = tokio::spawn(async move { race(&copy).await }).await;
    h.cleanup().await;
    if let Err(e) = run {
        std::panic::resume_unwind(e.into_panic());
    }
}
async fn race(h: &Harness) {
    let (c, base, t, boot) = setup(h).await;
    let dir = format!("{base}/teams/{t}/projects");
    let project = create(h, &c, &dir, "Milestone races", "milestone-races").await;
    let p = project.headers["location"].to_str().unwrap().to_owned();
    let m = milestone(h, &c, &p, "Concurrent outcome").await;
    let url = m.headers["location"].to_str().unwrap().to_owned();
    let s = boot["statuses"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["team_id"] == t && v["is_default"] == true)
        .unwrap()["id"]
        .as_str()
        .unwrap();
    let task = issue(h, &c, &base, &t, s, "Concurrent assignment").await;
    let project = get(h, &c, &p).await;
    let project = attach(h, &c, &p, &project, &task).await;
    let w = base.rsplit('/').next().unwrap().parse().unwrap();
    let actor = boot["members"][0]["id"].as_str().unwrap().parse().unwrap();
    let mut tx = h.lock_team(w, t.parse().unwrap(), actor).await;
    let r = h.router.clone();
    let caller = c.clone();
    let path = format!("{p}/issues/{}/milestone", task.body["id"].as_str().unwrap());
    let tag = project.etag().to_owned();
    let id = m.body["id"].clone();
    let mut waiting = tokio::spawn(async move {
        request(
            &r,
            Some(&caller),
            "PUT",
            &path,
            Some(json!({"milestone_id":id})),
            &[("if-match", &tag)],
        )
        .await
    });
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(100), &mut waiting)
            .await
            .is_err()
    );
    sqlx::query("DELETE FROM milestones WHERE workspace_id=$1 AND id=$2")
        .bind(w)
        .bind(
            m.body["id"]
                .as_str()
                .unwrap()
                .parse::<uuid::Uuid>()
                .unwrap(),
        )
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    waiting
        .await
        .unwrap()
        .problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    request(&h.router, Some(&c), "GET", &url, None, &[])
        .await
        .problem(StatusCode::NOT_FOUND, "not_found");
    get(
        h,
        &c,
        &format!("{base}/issues/{}", task.body["id"].as_str().unwrap()),
    )
    .await;
}
