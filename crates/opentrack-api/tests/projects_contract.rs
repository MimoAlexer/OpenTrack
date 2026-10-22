//! Project lifecycle, authorization and durable invariants against real PostgreSQL.
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
#[tokio::test]
async fn project_lifecycle_progress_and_membership_constraints() {
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
    let directory = format!("{base}/teams/{t}/projects");
    request(&h.router, None, "GET", &directory, None, &[])
        .await
        .problem(StatusCode::UNAUTHORIZED, "unauthenticated");
    request(
        &h.router,
        Some(&c),
        "POST",
        &directory,
        Some(json!({"name":"No key"})),
        &[],
    )
    .await
    .problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    request(
        &h.router,
        Some(&c),
        "POST",
        &directory,
        Some(json!({"name":"Dates","start_date":"2026-02-02","target_date":"2026-02-01"})),
        &[("idempotency-key", "project-bad-dates")],
    )
    .await
    .problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    let created = create(h, &c, &directory, "Release readiness", "project-release").await;
    let url = created.headers["location"].to_str().unwrap().to_owned();
    assert_eq!(created.body["state"], "planned");
    assert_eq!(created.body["health"], "on_track");
    assert_eq!(get(h, &c, &url).await.body, created.body);
    let directory_result = get(h, &c, &directory).await;
    assert!(
        directory_result.body["data"][0].get("narrative").is_none(),
        "Directory responses contain metadata only"
    );
    let empty = get(h, &c, &format!("{url}/progress")).await;
    assert_eq!(empty.body["unscoped"], true);
    assert_eq!(empty.body["eligible"], 0);
    assert!(!empty.headers.contains_key("etag"));
    let replay = create(h, &c, &directory, "Release readiness", "project-release").await;
    assert_eq!(replay.body, created.body);
    request(
        &h.router,
        Some(&c),
        "POST",
        &directory,
        Some(json!({"name":"Changed body"})),
        &[("idempotency-key", "project-release")],
    )
    .await
    .problem(StatusCode::CONFLICT, "conflict");
    request(
        &h.router,
        Some(&c),
        "PATCH",
        &url,
        Some(json!({"summary":"Requires version"})),
        &[],
    )
    .await
    .problem(StatusCode::PRECONDITION_REQUIRED, "precondition_required");
    let mut project=request(&h.router,Some(&c),"PATCH",&url,Some(json!({"summary":"A real deliverable","start_date":"2026-02-01","target_date":"2026-03-01","narrative":{"schema_version":1,"body":{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"The launch plan"}]}]}}})),&[("if-match",created.etag())]).await;
    project.expect(StatusCode::OK);
    assert_eq!(project.body["narrative"]["plain_text"], "The launch plan");
    request(
        &h.router,
        Some(&c),
        "PATCH",
        &url,
        Some(json!({"name":"Stale"})),
        &[("if-match", created.etag())],
    )
    .await
    .problem(StatusCode::PRECONDITION_FAILED, "version_conflict");
    let statuses = boot["statuses"].as_array().unwrap();
    let status = |category: &str| {
        statuses
            .iter()
            .find(|v| v["team_id"] == t && v["category"] == category)
            .unwrap()["id"]
            .as_str()
            .unwrap()
    };
    let open = issue(h, &c, &base, &t, status("unstarted"), "Open work").await;
    let done = issue(h, &c, &base, &t, status("completed"), "Done work").await;
    let canceled = issue(h, &c, &base, &t, status("canceled"), "Canceled work").await;
    let archived = issue(h, &c, &base, &t, status("unstarted"), "Archived work").await;
    let deleted = issue(h, &c, &base, &t, status("unstarted"), "Deleted work").await;
    for i in [&open, &done, &canceled, &archived, &deleted] {
        project = attach(h, &c, &url, &project, i).await;
    }
    let archived_path = format!("{base}/issues/{}", archived.body["id"].as_str().unwrap());
    request(
        &h.router,
        Some(&c),
        "PATCH",
        &archived_path,
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
        ("completed", 1),
        ("eligible", 2),
        ("canceled", 1),
        ("open", 1),
        ("total", 3),
    ] {
        assert_eq!(counts.body[k], v, "{k}");
    }
    assert_eq!(counts.body["unscoped"], false);
    let listed = get(h, &c, &format!("{url}/issues?limit=2")).await;
    assert_eq!(listed.body["data"].as_array().unwrap().len(), 2);
    assert_eq!(listed.body["has_more"], true);
    let cursor = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("cursor", listed.body["next_cursor"].as_str().unwrap())
        .finish();
    let next = get(h, &c, &format!("{url}/issues?limit=2&{cursor}")).await;
    assert_eq!(next.body["data"].as_array().unwrap().len(), 1);
    request(
        &h.router,
        Some(&c),
        "PATCH",
        &url,
        Some(json!({"state":"completed"})),
        &[("if-match", project.etag())],
    )
    .await
    .problem(StatusCode::CONFLICT, "conflict");
    project = request(
        &h.router,
        Some(&c),
        "PATCH",
        &url,
        Some(json!({"state":"completed","acknowledge_open_issues":true})),
        &[("if-match", project.etag())],
    )
    .await;
    project.expect(StatusCode::OK);
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
    let update = request(
        &h.router,
        Some(&c),
        "POST",
        &format!("{url}/updates"),
        Some(json!({"body":"Completed the project; the remaining issue is tracked separately."})),
        &[("idempotency-key", "project-progress-update")],
    )
    .await;
    update.expect(StatusCode::CREATED);
    assert_eq!(update.body["completed"], 1);
    assert_eq!(update.body["eligible"], 2);
    assert_eq!(update.body["state"], "completed");
    assert_eq!(update.body["author_name"], "Example Owner");
    assert_eq!(
        get(h, &c, update.headers["location"].to_str().unwrap())
            .await
            .body,
        update.body
    );
    let replay = request(
        &h.router,
        Some(&c),
        "POST",
        &format!("{url}/updates"),
        Some(json!({"body":"Completed the project; the remaining issue is tracked separately."})),
        &[("idempotency-key", "project-progress-update")],
    )
    .await;
    replay.expect(StatusCode::CREATED);
    assert_eq!(replay.body, update.body);
    assert_eq!(
        get(h, &c, &format!("{url}/updates")).await.body["data"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let second = create(h, &c, &directory, "Another project", "another-project").await;
    let second_url = second.headers["location"].to_str().unwrap();
    request(
        &h.router,
        Some(&c),
        "PUT",
        &format!("{second_url}/issues/{}", open.body["id"].as_str().unwrap()),
        Some(json!({"included":true})),
        &[("if-match", second.etag())],
    )
    .await
    .problem(StatusCode::CONFLICT, "conflict");
    let team = request(
        &h.router,
        Some(&c),
        "POST",
        &format!("{base}/teams"),
        Some(json!({"name":"Other team","key":"OTH","private":false})),
        &[("idempotency-key", "project-other-team")],
    )
    .await;
    team.expect(StatusCode::CREATED);
    let other_t = team.body["id"].as_str().unwrap();
    let other_boot = get(h, &c, &format!("{base}/bootstrap")).await;
    let other_status = other_boot.body["statuses"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["team_id"] == other_t && v["is_default"] == true)
        .unwrap()["id"]
        .as_str()
        .unwrap();
    let other_issue = issue(h, &c, &base, other_t, other_status, "Other team work").await;
    request(
        &h.router,
        Some(&c),
        "PUT",
        &format!("{url}/issues/{}", other_issue.body["id"].as_str().unwrap()),
        Some(json!({"included":true})),
        &[("if-match", project.etag())],
    )
    .await
    .problem(StatusCode::NOT_FOUND, "not_found");
    // The database itself enforces the same-team relation, independently of API validation.
    let w: uuid::Uuid = base.rsplit('/').next().unwrap().parse().unwrap();
    let owner: uuid::Uuid = boot["members"][0]["id"].as_str().unwrap().parse().unwrap();
    let mut tx = h.lock_team(w, t.parse().unwrap(), owner).await;
    let bad = sqlx::query(
        "INSERT INTO project_issues(workspace_id,team_id,project_id,issue_id) VALUES($1,$2,$3,$4)",
    )
    .bind(w)
    .bind(t.parse::<uuid::Uuid>().unwrap())
    .bind(
        project.body["id"]
            .as_str()
            .unwrap()
            .parse::<uuid::Uuid>()
            .unwrap(),
    )
    .bind(
        other_issue.body["id"]
            .as_str()
            .unwrap()
            .parse::<uuid::Uuid>()
            .unwrap(),
    )
    .execute(&mut *tx)
    .await
    .unwrap_err();
    assert_eq!(
        bad.as_database_error().unwrap().code().as_deref(),
        Some("23503")
    );
    tx.rollback().await.unwrap();
    let archived_project = request(
        &h.router,
        Some(&c),
        "PATCH",
        &url,
        Some(json!({"archived":true})),
        &[("if-match", project.etag())],
    )
    .await;
    archived_project.expect(StatusCode::OK);
    assert_eq!(archived_project.body["can_edit"], false);
    request(
        &h.router,
        Some(&c),
        "PATCH",
        &url,
        Some(json!({"summary":"Blocked"})),
        &[("if-match", archived_project.etag())],
    )
    .await
    .problem(StatusCode::CONFLICT, "conflict");
    assert_eq!(
        get(h, &c, &format!("{directory}?archived=true")).await.body["data"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let restored = request(
        &h.router,
        Some(&c),
        "PATCH",
        &url,
        Some(json!({"archived":false})),
        &[("if-match", archived_project.etag())],
    )
    .await;
    restored.expect(StatusCode::OK);
    assert_eq!(restored.body["can_edit"], true);
}
async fn invite(h: &Harness, c: &Client, base: &str, role: &str) -> Client {
    let invited = request(
        &h.router,
        Some(c),
        "POST",
        &format!("{base}/invitations"),
        Some(json!({"email":format!("{role}@projects.example.test"),"role":role,"team_ids":[]})),
        &[("idempotency-key", &format!("project-invite-{role}"))],
    )
    .await;
    invited.expect(StatusCode::CREATED);
    let accepted=request(&h.router,None,"POST","/api/v1/invitations/accept",Some(json!({"token":invited.body["token"],"display_name":format!("Example {role}"),"password":"Project member verification password 2026!"})),&[]).await;
    accepted.expect(StatusCode::OK);
    client(&accepted)
}
#[tokio::test]
async fn project_permissions_privacy_and_atomic_events() {
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
    let directory = format!("{base}/teams/{t}/projects");
    let member = invite(h, &owner, &base, "member").await;
    let viewer = invite(h, &owner, &base, "viewer").await;
    let made = create(
        h,
        &member,
        &directory,
        "Member project",
        "member-project-create",
    )
    .await;
    let url = made.headers["location"].to_str().unwrap();
    assert_eq!(made.body["can_edit"], true);
    assert_eq!(made.body["can_manage"], false);
    let seen = get(h, &viewer, url).await;
    assert_eq!(seen.body["can_edit"], false);
    assert_eq!(seen.body["can_manage"], false);
    request(
        &h.router,
        Some(&viewer),
        "POST",
        &directory,
        Some(json!({"name":"Blocked viewer"})),
        &[("idempotency-key", "project-viewer-denied")],
    )
    .await
    .problem(StatusCode::FORBIDDEN, "forbidden");
    request(
        &h.router,
        Some(&viewer),
        "PATCH",
        url,
        Some(json!({"summary":"Blocked viewer"})),
        &[("if-match", seen.etag())],
    )
    .await
    .problem(StatusCode::FORBIDDEN, "forbidden");
    request(
        &h.router,
        Some(&member),
        "PATCH",
        url,
        Some(json!({"archived":true})),
        &[("if-match", made.etag())],
    )
    .await
    .problem(StatusCode::FORBIDDEN, "forbidden");
    let bad_csrf = Client {
        csrf: "invalid".into(),
        ..owner.clone()
    };
    request(
        &h.router,
        Some(&bad_csrf),
        "POST",
        &directory,
        Some(json!({"name":"No CSRF"})),
        &[("idempotency-key", "project-csrf-denied")],
    )
    .await
    .problem(StatusCode::FORBIDDEN, "forbidden");
    let private = request(
        &h.router,
        Some(&owner),
        "POST",
        &format!("{base}/teams"),
        Some(json!({"name":"Private planning","key":"PRV","private":true})),
        &[("idempotency-key", "project-private-team")],
    )
    .await;
    private.expect(StatusCode::CREATED);
    let private_directory = format!(
        "{base}/teams/{}/projects",
        private.body["id"].as_str().unwrap()
    );
    let secret = create(
        h,
        &owner,
        &private_directory,
        "Confidential plan",
        "project-private-plan",
    )
    .await;
    let secret_url = secret.headers["location"].to_str().unwrap();
    for endpoint in [
        private_directory.as_str(),
        secret_url,
        &format!("{secret_url}/issues"),
        &format!("{secret_url}/progress"),
        &format!("{secret_url}/updates"),
    ] {
        request(&h.router, Some(&member), "GET", endpoint, None, &[])
            .await
            .problem(StatusCode::NOT_FOUND, "not_found");
    }
    request(
        &h.router,
        Some(&member),
        "POST",
        &private_directory,
        Some(json!({"name":"Try private"})),
        &[("idempotency-key", "project-private-denied")],
    )
    .await
    .problem(StatusCode::NOT_FOUND, "not_found");
    // Missing mandatory event insertion rolls back the project and its receipt.
    // Install the failure only in this harness's private schema.
    let w: uuid::Uuid = base.rsplit('/').next().unwrap().parse().unwrap();
    let actor: uuid::Uuid = boot["members"][0]["id"].as_str().unwrap().parse().unwrap();
    let mut tx = h.lock_team(w, t.parse().unwrap(), actor).await;
    sqlx::query("CREATE FUNCTION suppress_project_event() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.event_type='project.created' THEN RETURN NULL; END IF; RETURN NEW; END $$").execute(&mut *tx).await.unwrap();
    sqlx::query("CREATE TRIGGER suppress_project_event BEFORE INSERT ON workspace_events FOR EACH ROW EXECUTE FUNCTION suppress_project_event()").execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    request(
        &h.router,
        Some(&owner),
        "POST",
        &directory,
        Some(json!({"name":"Must roll back"})),
        &[("idempotency-key", "project-event-rollback")],
    )
    .await
    .problem(StatusCode::INTERNAL_SERVER_ERROR, "internal_error");
    assert!(
        get(h, &owner, &format!("{directory}?q=Must%20roll%20back"))
            .await
            .body["data"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let mut tx = h.lock_team(w, t.parse().unwrap(), actor).await;
    let receipts: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM idempotency_records WHERE workspace_id=$1 AND route=$2",
    )
    .bind(w)
    .bind(format!("projects.create:{t}"))
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert_eq!(receipts, 1);
    // FORCE RLS also protects new tables from direct queries under another tenant.
    opentrack_store::context(&mut tx, actor, Some(uuid::Uuid::new_v4()))
        .await
        .unwrap();
    let visible: i64 = sqlx::query_scalar("SELECT count(*) FROM projects")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert_eq!(visible, 0);
    tx.rollback().await.unwrap();
}
#[tokio::test]
async fn project_scope_waits_for_team_writes_and_rechecks_after_commit() {
    let h = Harness::new().await;
    let copy = h.clone();
    let run = tokio::spawn(async move { serialized(&copy).await }).await;
    h.cleanup().await;
    if let Err(e) = run {
        std::panic::resume_unwind(e.into_panic());
    }
}
async fn serialized(h: &Harness) {
    let (c, base, t, boot) = setup(h).await;
    let directory = format!("{base}/teams/{t}/projects");
    let made = create(
        h,
        &c,
        &directory,
        "Serialized planning",
        "serialized-planning",
    )
    .await;
    let url = made.headers["location"].to_str().unwrap().to_owned();
    let statuses = boot["statuses"].as_array().unwrap();
    let status = |category: &str| {
        statuses
            .iter()
            .find(|v| v["team_id"] == t && v["category"] == category)
            .unwrap()["id"]
            .as_str()
            .unwrap()
    };
    let task = issue(h, &c, &base, &t, status("completed"), "A completed task").await;
    let project = attach(h, &c, &url, &made, &task).await;
    let w: uuid::Uuid = base.rsplit('/').next().unwrap().parse().unwrap();
    let actor: uuid::Uuid = boot["members"][0]["id"].as_str().unwrap().parse().unwrap();
    let team = t.parse().unwrap();
    let mut tx = h.lock_team(w, team, actor).await;
    let r = h.router.clone();
    let caller = c.clone();
    let path = url.clone();
    let validator = project.etag().to_owned();
    let mut finishing = tokio::spawn(async move {
        request(
            &r,
            Some(&caller),
            "PATCH",
            &path,
            Some(json!({"state":"completed"})),
            &[("if-match", &validator)],
        )
        .await
    });
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(100), &mut finishing)
            .await
            .is_err(),
        "Project completion must wait for the team's in-flight issue write"
    );
    sqlx::query("UPDATE issues SET status_id=$3,version=version+1 WHERE workspace_id=$1 AND id=$2")
        .bind(w)
        .bind(
            task.body["id"]
                .as_str()
                .unwrap()
                .parse::<uuid::Uuid>()
                .unwrap(),
        )
        .bind(status("unstarted").parse::<uuid::Uuid>().unwrap())
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    finishing
        .await
        .unwrap()
        .problem(StatusCode::CONFLICT, "conflict");
    assert_eq!(get(h, &c, &url).await.body["state"], "planned");
    let candidate = issue(
        h,
        &c,
        &base,
        &t,
        status("unstarted"),
        "Candidate being archived",
    )
    .await;
    let mut tx = h.lock_team(w, team, actor).await;
    let r = h.router.clone();
    let caller = c.clone();
    let path = format!("{url}/issues/{}", candidate.body["id"].as_str().unwrap());
    let validator = project.etag().to_owned();
    let mut adding = tokio::spawn(async move {
        request(
            &r,
            Some(&caller),
            "PUT",
            &path,
            Some(json!({"included":true})),
            &[("if-match", &validator)],
        )
        .await
    });
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(100), &mut adding)
            .await
            .is_err(),
        "Project scope must wait for the team's in-flight issue write"
    );
    sqlx::query("UPDATE issues SET archived_at=clock_timestamp(),version=version+1 WHERE workspace_id=$1 AND id=$2").bind(w).bind(candidate.body["id"].as_str().unwrap().parse::<uuid::Uuid>().unwrap()).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    adding
        .await
        .unwrap()
        .problem(StatusCode::NOT_FOUND, "not_found");
    assert_eq!(
        get(h, &c, &format!("{url}/progress")).await.body["eligible"],
        1
    );
}
