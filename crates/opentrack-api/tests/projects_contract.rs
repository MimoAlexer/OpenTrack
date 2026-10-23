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
fn document(text: &str) -> Value {
    json!({"schema_version":1,"body":{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":text}]}]}})
}

async fn replay_state(
    h: &Harness,
    workspace: uuid::Uuid,
    team: uuid::Uuid,
    actor: uuid::Uuid,
) -> Value {
    let mut tx = h.lock_team(workspace, team, actor).await;
    let state = sqlx::query_scalar("SELECT jsonb_build_object('projects',(SELECT coalesce(jsonb_agg(to_jsonb(p) ORDER BY id),'[]') FROM projects p),'documents',(SELECT coalesce(jsonb_agg(to_jsonb(d) ORDER BY id),'[]') FROM documents d),'revisions',(SELECT coalesce(jsonb_agg(to_jsonb(r) ORDER BY document_id,revision),'[]') FROM document_revisions r),'receipts',(SELECT coalesce(jsonb_agg(to_jsonb(i) ORDER BY principal_id,route,key_hash),'[]') FROM idempotency_records i),'audit',(SELECT coalesce(jsonb_agg(to_jsonb(a) ORDER BY id),'[]') FROM audit_entries a),'events',(SELECT coalesce(jsonb_agg(to_jsonb(e) ORDER BY seq),'[]') FROM workspace_events e),'stream',(SELECT coalesce(jsonb_agg(to_jsonb(s)),'[]') FROM workspace_stream_heads s),'mentions',(SELECT coalesce(jsonb_agg(to_jsonb(m) ORDER BY id),'[]') FROM mention_intents m))")
        .fetch_one(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    state
}

#[tokio::test]
async fn project_create_replay_keeps_original_brief_snapshot() {
    let h = Harness::new().await;
    let copy = h.clone();
    let run = tokio::spawn(async move {
        let (c, base, t, boot) = setup(&copy).await;
        let workspace = base.rsplit('/').next().unwrap().parse().unwrap();
        let team = t.parse().unwrap();
        let actor = boot["members"][0]["id"].as_str().unwrap().parse().unwrap();
        let directory = format!("{base}/teams/{t}/projects");
        let key = "project-original-receipt";
        let body = json!({"name":"Original create outcome","summary":"Original metadata","narrative":document("Original brief")});
        let created = request(&copy.router, Some(&c), "POST", &directory, Some(body.clone()), &[("idempotency-key", key)]).await;
        created.expect(StatusCode::CREATED);
        assert_eq!(created.body["narrative_version"], "1");
        let url = created.headers["location"].to_str().unwrap();
        let initial = get(&copy, &c, &format!("{url}/document")).await;
        let doc_url = format!("{base}/documents/{}", initial.body["id"].as_str().unwrap());
        let saved = request(&copy.router, Some(&c), "PATCH", &doc_url, Some(document("Later independently saved brief")), &[("if-match", initial.etag())]).await;
        saved.expect(StatusCode::OK);
        let metadata = get(&copy, &c, &format!("{url}/metadata")).await;
        request(&copy.router, Some(&c), "PATCH", &format!("{url}/metadata"), Some(json!({"summary":"Later independently saved metadata"})), &[("if-match", metadata.etag())]).await.expect(StatusCode::OK);
        let current = get(&copy, &c, url).await;
        assert_eq!(current.body["narrative_version"], "2");
        assert_ne!(current.body["version"], created.body["version"]);
        let before = replay_state(&copy, workspace, team, actor).await;
        let replay = request(&copy.router, Some(&c), "POST", &directory, Some(body), &[("idempotency-key", key)]).await;
        replay.expect(StatusCode::CREATED);
        assert_eq!(replay_state(&copy, workspace, team, actor).await, before,
            "Receipt replay must not write another document revision, audit, event or receipt outcome");
        assert_eq!(replay.body["narrative"], created.body["narrative"],
            "Create replay must retain its original brief instead of the current document");
        assert_eq!(replay.body["narrative_version"], created.body["narrative_version"]);
        assert_eq!(replay.body, created.body);
        assert_eq!(replay.etag(), created.etag());
        assert_eq!(replay.headers["location"], created.headers["location"]);
        request(&copy.router, Some(&c), "POST", &directory, Some(json!({"name":"Changed original request"})), &[("idempotency-key", key)]).await.problem(StatusCode::CONFLICT, "conflict");
        assert_eq!(replay_state(&copy, workspace, team, actor).await, before);
        assert_eq!(get(&copy, &c, url).await.body, current.body);
        assert_eq!(get(&copy, &c, &doc_url).await.body, saved.body);
    }).await;
    h.cleanup().await;
    if let Err(error) = run {
        std::panic::resume_unwind(error.into_panic());
    }
}

#[tokio::test]
async fn project_create_replay_preserves_legacy_receipts_and_current_authority() {
    let h = Harness::new().await;
    let copy = h.clone();
    let run = tokio::spawn(async move {
        let (owner, base, t, boot) = setup(&copy).await;
        let member = invite(&copy, &owner, &base, "member").await;
        let workspace = base.rsplit('/').next().unwrap().parse().unwrap();
        let team = t.parse().unwrap();
        let actor = boot["members"][0]["id"].as_str().unwrap().parse().unwrap();
        let directory = format!("{base}/teams/{t}/projects");
        let key = "project-legacy-receipt";
        let body = json!({"name":"Legacy create outcome","summary":"Original historical metadata","narrative":document("Original historical brief")});
        let made = request(&copy.router, Some(&member), "POST", &directory, Some(body.clone()), &[("idempotency-key", key)]).await;
        made.expect(StatusCode::CREATED);
        let url = made.headers["location"].to_str().unwrap();
        let initial = get(&copy, &member, &format!("{url}/document")).await;
        let doc_url = format!("{base}/documents/{}", initial.body["id"].as_str().unwrap());
        let project: uuid::Uuid = made.body["id"].as_str().unwrap().parse().unwrap();
        let mut legacy = made.body.clone();
        legacy.as_object_mut().unwrap().remove("narrative_version");
        legacy.as_object_mut().unwrap().remove("metadata_etag");
        let mut tx = copy.lock_team(workspace, team, actor).await;
        let principal: uuid::Uuid = sqlx::query_scalar("UPDATE idempotency_records SET response_body=$2 WHERE response_body->>'id'=$1 AND route LIKE 'projects.create:%' RETURNING principal_id")
            .bind(project.to_string()).bind(&legacy).fetch_one(&mut *tx).await.unwrap();
        tx.commit().await.unwrap();
        let saved = request(&copy.router, Some(&member), "PATCH", &doc_url, Some(document("Current brief after legacy receipt")), &[("if-match", initial.etag())]).await;
        saved.expect(StatusCode::OK);
        let metadata = get(&copy, &member, &format!("{url}/metadata")).await;
        request(&copy.router, Some(&member), "PATCH", &format!("{url}/metadata"), Some(json!({"summary":"Current metadata after legacy receipt"})), &[("if-match", metadata.etag())]).await.expect(StatusCode::OK);
        let current = get(&copy, &member, url).await;
        assert_eq!(current.body["narrative_version"], "2");
        let before = replay_state(&copy, workspace, team, actor).await;
        let replay = request(&copy.router, Some(&member), "POST", &directory, Some(body.clone()), &[("idempotency-key", key)]).await;
        replay.expect(StatusCode::CREATED);
        assert_eq!(replay.body.get("narrative_version"), Some(&Value::Null),
            "A legacy create receipt cannot claim a document revision it never recorded");
        for field in ["id", "workspace_id", "team_id", "name", "summary", "narrative", "lead_id", "start_date", "target_date", "state", "health", "version", "created_at", "updated_at", "archived_at"] {
            assert_eq!(replay.body[field], legacy[field], "Legacy outcome field {field}");
        }
        assert!(!replay.body["metadata_etag"].as_str().unwrap().is_empty());
        assert_ne!(replay.body["metadata_etag"], current.body["metadata_etag"]);
        assert_eq!(replay.body["etag"], replay.etag());
        assert_ne!(replay.etag(), current.etag());
        assert_eq!(replay.headers["location"], made.headers["location"]);
        assert_eq!(replay_state(&copy, workspace, team, actor).await, before,
            "Legacy hydration must not rewrite the original stored receipt or emit effects");
        let repeated = request(&copy.router, Some(&member), "POST", &directory, Some(body.clone()), &[("idempotency-key", key)]).await;
        repeated.expect(StatusCode::CREATED);
        assert_eq!(repeated.body, replay.body);
        assert_eq!(repeated.etag(), replay.etag());
        request(&copy.router, Some(&member), "PATCH", url,
            Some(json!({"summary":"Must not use a historical receipt as the current validator","narrative":document("Must not replace the current brief")})),
            &[("if-match", replay.etag())]).await.problem(StatusCode::PRECONDITION_FAILED, "version_conflict");
        assert_eq!(replay_state(&copy, workspace, team, actor).await, before);

        let member_url = format!("{base}/members/{principal}");
        let membership = get(&copy, &owner, &member_url).await;
        request(&copy.router, Some(&owner), "PATCH", &member_url, Some(json!({"role":"viewer"})), &[("if-match", membership.etag())]).await.expect(StatusCode::OK);
        let metadata = get(&copy, &owner, &format!("{url}/metadata")).await;
        request(&copy.router, Some(&owner), "PATCH", &format!("{url}/metadata"), Some(json!({"archived":true})), &[("if-match", metadata.etag())]).await.expect(StatusCode::OK);
        let before = replay_state(&copy, workspace, team, actor).await;
        let replay = request(&copy.router, Some(&member), "POST", &directory, Some(body.clone()), &[("idempotency-key", key)]).await;
        replay.expect(StatusCode::CREATED);
        assert_eq!(replay.body["can_edit"], false);
        assert_eq!(replay.body["can_manage"], false);
        assert_eq!(replay.body["narrative"], legacy["narrative"]);
        assert_eq!(replay.body["version"], legacy["version"]);
        assert_eq!(replay.body.get("narrative_version"), Some(&Value::Null));
        request(&copy.router, Some(&member), "POST", &directory, Some(body.clone()), &[("idempotency-key", "viewer-cannot-create-another")]).await.problem(StatusCode::FORBIDDEN, "forbidden");
        assert_eq!(replay_state(&copy, workspace, team, actor).await, before);
        let seen = get(&copy, &member, &doc_url).await;
        assert_eq!(seen.body["body"], saved.body["body"]);
        assert_eq!(seen.body["version"], saved.body["version"]);
        assert_eq!(seen.body["can_edit"], false);

        let membership = get(&copy, &owner, &member_url).await;
        request(&copy.router, Some(&owner), "PATCH", &member_url, Some(json!({"active":false})), &[("if-match", membership.etag())]).await.expect(StatusCode::OK);
        let before = replay_state(&copy, workspace, team, actor).await;
        request(&copy.router, Some(&member), "POST", &directory, Some(body), &[("idempotency-key", key)]).await.problem(StatusCode::NOT_FOUND, "not_found");
        request(&copy.router, Some(&member), "GET", url, None, &[]).await.problem(StatusCode::NOT_FOUND, "not_found");
        assert_eq!(replay_state(&copy, workspace, team, actor).await, before,
            "A known receipt must not bypass current source access or acquire write effects");
    }).await;
    h.cleanup().await;
    if let Err(error) = run {
        std::panic::resume_unwind(error.into_panic());
    }
}

#[tokio::test]
async fn project_narrative_route_mismatch_does_not_wait_for_unrelated_team_lock() {
    let h = Harness::new().await;
    let copy = h.clone();
    let run = tokio::spawn(async move {
        let (client, base, team, boot) = setup(&copy).await;
        let made = request(
            &copy.router,
            Some(&client),
            "POST",
            &format!("{base}/teams/{team}/projects"),
            Some(json!({"name":"Route scoped brief","narrative":document("Original brief")})),
            &[("idempotency-key", "route-scoped-project")],
        )
        .await;
        made.expect(StatusCode::CREATED);
        let other = request(
            &copy.router,
            Some(&client),
            "POST",
            &format!("{base}/teams"),
            Some(json!({"name":"Other route team","key":"ALT","private":false})),
            &[("idempotency-key", "route-other-team")],
        )
        .await;
        other.expect(StatusCode::CREATED);
        let workspace = base.rsplit('/').next().unwrap().parse().unwrap();
        let actor = boot["members"][0]["id"].as_str().unwrap().parse().unwrap();
        let other_team = other.body["id"].as_str().unwrap().parse().unwrap();
        let held = copy.lock_team(workspace, other_team, actor).await;
        let path = format!(
            "{base}/teams/{}/projects/{}",
            other.body["id"].as_str().unwrap(),
            made.body["id"].as_str().unwrap()
        );
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            request(
                &copy.router,
                Some(&client),
                "PATCH",
                &path,
                Some(json!({"narrative":document("Rejected brief")})),
                &[("if-match", made.etag())],
            ),
        )
        .await;
        held.rollback().await.unwrap();
        result
            .expect("A wrong project route must fail before taking its unrelated team lock")
            .problem(StatusCode::NOT_FOUND, "not_found");
        assert_eq!(
            get(&copy, &client, made.headers["location"].to_str().unwrap())
                .await
                .body,
            made.body
        );
    })
    .await;
    h.cleanup().await;
    if let Err(error) = run {
        std::panic::resume_unwind(error.into_panic());
    }
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
    let mut project=request(&h.router,Some(&c),"PATCH",&url,Some(json!({"summary":"A real deliverable","start_date":"2026-02-01","target_date":"2026-03-01"})),&[("if-match",created.etag())]).await;
    project.expect(StatusCode::OK);
    let brief = get(h, &c, &format!("{url}/document")).await;
    request(
        &h.router,
        Some(&c),
        "PATCH",
        &format!("{base}/documents/{}", brief.body["id"].as_str().unwrap()),
        Some(document("The launch plan")),
        &[("if-match", brief.etag())],
    )
    .await
    .expect(StatusCode::OK);
    project = get(h, &c, &url).await;
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

#[tokio::test]
async fn project_documents_have_independent_versions_history_and_atomic_effects() {
    let h = Harness::new().await;
    let copy = h.clone();
    let run = tokio::spawn(async move { document_versions(&copy).await }).await;
    h.cleanup().await;
    if let Err(error) = run {
        std::panic::resume_unwind(error.into_panic());
    }
}
async fn document_versions(h: &Harness) {
    let (c, base, t, boot) = setup(h).await;
    let made = create(
        h,
        &c,
        &format!("{base}/teams/{t}/projects"),
        "Independent project brief",
        "independent-project-brief",
    )
    .await;
    let url = made.headers["location"].to_str().unwrap();
    let metadata_url = format!("{url}/metadata");
    let initial_metadata = get(h, &c, &metadata_url).await;
    assert!(initial_metadata.body.get("narrative").is_none());
    assert_eq!(made.body["metadata_etag"], initial_metadata.etag());
    let initial = get(h, &c, &format!("{url}/document")).await;
    assert_eq!(initial.body["owner_type"], "project");
    assert_eq!(initial.body["owner_id"], made.body["id"]);
    let doc_url = format!("{base}/documents/{}", initial.body["id"].as_str().unwrap());
    request(
        &h.router,
        Some(&c),
        "PATCH",
        &doc_url,
        Some(document("Requires a document version")),
        &[],
    )
    .await
    .problem(StatusCode::PRECONDITION_REQUIRED, "precondition_required");
    let first = request(
        &h.router,
        Some(&c),
        "PATCH",
        &doc_url,
        Some(document("A saved project brief")),
        &[("if-match", initial.etag())],
    )
    .await;
    first.expect(StatusCode::OK);
    let full_after_body = get(h, &c, url).await;
    let metadata_after_body = get(h, &c, &metadata_url).await;
    assert_ne!(full_after_body.etag(), made.etag());
    assert_eq!(full_after_body.body["version"], made.body["version"]);
    assert_eq!(
        full_after_body.body["narrative"]["body"],
        first.body["body"]
    );
    assert_eq!(metadata_after_body.body, initial_metadata.body);
    assert_eq!(metadata_after_body.etag(), initial_metadata.etag());
    assert_eq!(
        full_after_body.body["metadata_etag"],
        initial_metadata.etag()
    );
    request(
        &h.router,
        Some(&c),
        "PATCH",
        url,
        Some(json!({"summary":"Stale full representation"})),
        &[("if-match", made.etag())],
    )
    .await
    .problem(StatusCode::PRECONDITION_FAILED, "version_conflict");
    let metadata_headers = [("if-match", initial_metadata.etag())];
    let body_headers = [("if-match", first.etag())];
    let (metadata_save, body_save) = tokio::join!(
        request(
            &h.router,
            Some(&c),
            "PATCH",
            &metadata_url,
            Some(json!({"summary":"A concurrent metadata edit"})),
            &metadata_headers,
        ),
        request(
            &h.router,
            Some(&c),
            "PATCH",
            &doc_url,
            Some(document("A concurrent brief edit")),
            &body_headers,
        )
    );
    metadata_save.expect(StatusCode::OK);
    body_save.expect(StatusCode::OK);
    assert!(metadata_save.body.get("narrative").is_none());
    let full = get(h, &c, url).await;
    assert_eq!(full.body["summary"], "A concurrent metadata edit");
    assert_eq!(full.body["narrative"]["body"], body_save.body["body"]);
    assert_eq!(full.body["version"], metadata_save.body["version"]);
    assert_eq!(full.body["metadata_etag"], metadata_save.etag());
    assert_eq!(get(h, &c, &doc_url).await.etag(), body_save.etag());
    for (path, body, etag) in [
        (
            metadata_url.as_str(),
            json!({"summary":"Stale metadata"}),
            initial_metadata.etag(),
        ),
        (doc_url.as_str(), document("Stale brief"), first.etag()),
    ] {
        request(
            &h.router,
            Some(&c),
            "PATCH",
            path,
            Some(body),
            &[("if-match", etag)],
        )
        .await
        .problem(StatusCode::PRECONDITION_FAILED, "version_conflict");
    }
    request(
        &h.router,
        Some(&c),
        "PATCH",
        &metadata_url,
        Some(json!({"summary":"Must not partially save","narrative":document("Wrong resource")})),
        &[("if-match", metadata_save.etag())],
    )
    .await
    .problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    assert_eq!(get(h, &c, url).await.body, full.body);
    let legacy = request(
        &h.router,
        Some(&c),
        "PATCH",
        url,
        Some(json!({"summary":"An atomic legacy edit","narrative":document("A legacy client brief")})),
        &[("if-match", full.etag())],
    )
    .await;
    legacy.expect(StatusCode::OK);
    assert_eq!(legacy.body["summary"], "An atomic legacy edit");
    assert_eq!(
        legacy.body["narrative"]["plain_text"],
        "A legacy client brief"
    );
    request(
        &h.router,
        Some(&c),
        "PATCH",
        url,
        Some(json!({"summary":"Stale legacy metadata","narrative":document("Stale legacy brief")})),
        &[("if-match", full.etag())],
    )
    .await
    .problem(StatusCode::PRECONDITION_FAILED, "version_conflict");
    request(
        &h.router,
        Some(&c),
        "PATCH",
        &doc_url,
        Some(document("Stale after the legacy save")),
        &[("if-match", body_save.etag())],
    )
    .await
    .problem(StatusCode::PRECONDITION_FAILED, "version_conflict");
    let full = legacy;
    let body_save = get(h, &c, &doc_url).await;
    let metadata_save = get(h, &c, &metadata_url).await;
    assert_eq!(body_save.body["body"], full.body["narrative"]["body"]);
    assert_eq!(full.body["metadata_etag"], metadata_save.etag());
    let mut forged = document("Forged provenance");
    forged["migrated"] = json!(true);
    forged["author"] = Value::Null;
    request(
        &h.router,
        Some(&c),
        "PATCH",
        &doc_url,
        Some(forged),
        &[("if-match", body_save.etag())],
    )
    .await
    .problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    assert_eq!(get(h, &c, url).await.body, full.body);
    let history = get(h, &c, &format!("{doc_url}/revisions")).await;
    let revisions = history.body["data"].as_array().unwrap();
    assert_eq!(revisions.len(), 4);
    for revision in revisions {
        assert_eq!(revision["migrated"], false);
        assert_eq!(revision["author"]["id"], boot["members"][0]["id"]);
        assert_eq!(revision["author"]["display_name"], "Example Owner");
    }
    let saved_revision = get(
        h,
        &c,
        &format!(
            "{doc_url}/revisions/{}",
            body_save.body["version"].as_str().unwrap()
        ),
    )
    .await;
    assert_eq!(saved_revision.body["body"], body_save.body["body"]);
    assert_eq!(saved_revision.body["migrated"], false);
    let w = base.rsplit('/').next().unwrap().parse().unwrap();
    let actor = boot["members"][0]["id"].as_str().unwrap().parse().unwrap();
    let mut tx = h.lock_team(w, t.parse().unwrap(), actor).await;
    let frozen: Value = sqlx::query_scalar("SELECT jsonb_build_object('schema_version',narrative_schema_version,'body',narrative_body,'plain_text',narrative_plain_text) FROM projects WHERE workspace_id=$1 AND id=$2")
        .bind(w).bind(made.body["id"].as_str().unwrap().parse::<uuid::Uuid>().unwrap()).fetch_one(&mut *tx).await.unwrap();
    assert_eq!(frozen["schema_version"], initial.body["schema_version"]);
    assert_eq!(frozen["body"], initial.body["body"]);
    assert_eq!(frozen["plain_text"], initial.body["plain_text"]);
    sqlx::query("CREATE FUNCTION suppress_project_document_revision() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RETURN NULL; END $$").execute(&mut *tx).await.unwrap();
    sqlx::query("CREATE TRIGGER suppress_project_document_revision BEFORE INSERT ON document_revisions FOR EACH ROW EXECUTE FUNCTION suppress_project_document_revision()").execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    for (path, body, etag) in [
        (
            doc_url.as_str(),
            document("Must roll back without its revision"),
            body_save.etag(),
        ),
        (
            url,
            json!({"summary":"Must roll back with its brief","narrative":document("Missing legacy revision")}),
            full.etag(),
        ),
    ] {
        request(
            &h.router,
            Some(&c),
            "PATCH",
            path,
            Some(body),
            &[("if-match", etag)],
        )
        .await
        .problem(StatusCode::INTERNAL_SERVER_ERROR, "internal_error");
    }
    assert_eq!(get(h, &c, &doc_url).await.body, body_save.body);
    assert_eq!(
        get(h, &c, &format!("{doc_url}/revisions")).await.body,
        history.body
    );
    assert_eq!(get(h, &c, &metadata_url).await.body, metadata_save.body);
}

#[tokio::test]
async fn project_document_references_follow_current_source_and_recipient_access() {
    let h = Harness::new().await;
    let copy = h.clone();
    let run = tokio::spawn(async move { document_access(&copy).await }).await;
    h.cleanup().await;
    if let Err(error) = run {
        std::panic::resume_unwind(error.into_panic());
    }
}
async fn document_access(h: &Harness) {
    let (owner, base, t, boot) = setup(h).await;
    let member = invite(h, &owner, &base, "member").await;
    let viewer = invite(h, &owner, &base, "viewer").await;
    let members = get(h, &owner, &format!("{base}/bootstrap")).await;
    let recipient = members.body["members"]
        .as_array()
        .unwrap()
        .iter()
        .find(|user| user["display_name"] == "Example member")
        .unwrap()["id"]
        .as_str()
        .unwrap();
    let made = create(
        h,
        &owner,
        &format!("{base}/teams/{t}/projects"),
        "Reference planning",
        "project-reference-planning",
    )
    .await;
    let url = made.headers["location"].to_str().unwrap();
    let initial = get(h, &owner, &format!("{url}/document")).await;
    let doc_id = initial.body["id"].as_str().unwrap();
    let doc_url = format!("{base}/documents/{doc_id}");
    let viewer_doc = get(h, &viewer, &doc_url).await;
    assert_eq!(viewer_doc.body["can_edit"], false);
    let viewer_metadata = get(h, &viewer, &format!("{url}/metadata")).await;
    assert_eq!(viewer_metadata.body["can_edit"], false);
    for (path, body, etag) in [
        (doc_url.clone(), document("Viewer edit"), viewer_doc.etag()),
        (
            format!("{url}/metadata"),
            json!({"summary":"Viewer edit"}),
            viewer_metadata.etag(),
        ),
    ] {
        request(
            &h.router,
            Some(&viewer),
            "PATCH",
            &path,
            Some(body),
            &[("if-match", etag)],
        )
        .await
        .problem(StatusCode::FORBIDDEN, "forbidden");
    }
    let public_target = request(
        &h.router,
        Some(&owner),
        "POST",
        &format!("{base}/issues"),
        Some(json!({"team_id":t,"title":"Public project reference"})),
        &[("idempotency-key", "public-project-reference")],
    )
    .await;
    public_target.expect(StatusCode::CREATED);
    let public_id = public_target.body["id"].as_str().unwrap();
    let private = request(
        &h.router,
        Some(&owner),
        "POST",
        &format!("{base}/teams"),
        Some(json!({"name":"Private document planning","key":"DOC","private":true})),
        &[("idempotency-key", "private-document-planning")],
    )
    .await;
    private.expect(StatusCode::CREATED);
    let private_team = private.body["id"].as_str().unwrap();
    let hidden_target = request(
        &h.router,
        Some(&owner),
        "POST",
        &format!("{base}/issues"),
        Some(json!({"team_id":private_team,"title":"Confidential project reference"})),
        &[("idempotency-key", "hidden-project-reference")],
    )
    .await;
    hidden_target.expect(StatusCode::CREATED);
    let hidden_id = hidden_target.body["id"].as_str().unwrap();
    let secret = create(
        h,
        &owner,
        &format!("{base}/teams/{private_team}/projects"),
        "Private source plan",
        "private-document-source",
    )
    .await;
    let secret_url = secret.headers["location"].to_str().unwrap();
    let secret_doc = get(h, &owner, &format!("{secret_url}/document")).await;
    let secret_doc_id = secret_doc.body["id"].as_str().unwrap();
    let secret_doc_url = format!("{base}/documents/{secret_doc_id}");
    let private_endpoints = [
        format!("{secret_url}/metadata"),
        format!("{secret_url}/document"),
        secret_doc_url.clone(),
        format!("{secret_doc_url}/revisions"),
        format!("{secret_doc_url}/revisions/1"),
        format!("{base}/mention-candidates?document_id={secret_doc_id}"),
        format!("{base}/issue-reference-candidates?document_id={secret_doc_id}"),
    ];
    for path in &private_endpoints {
        request(
            &h.router,
            Some(&member),
            "GET",
            path,
            None,
            &[("if-none-match", secret_doc.etag())],
        )
        .await
        .problem(StatusCode::NOT_FOUND, "not_found");
    }
    let candidates = get(
        h,
        &owner,
        &format!("{base}/mention-candidates?document_id={doc_id}"),
    )
    .await;
    assert!(
        candidates.body["data"]
            .as_array()
            .unwrap()
            .iter()
            .any(|user| user["id"] == recipient)
    );
    let reference_candidates = get(
        h,
        &member,
        &format!("{base}/issue-reference-candidates?document_id={doc_id}&q=reference"),
    )
    .await;
    assert!(
        reference_candidates.body["data"]
            .as_array()
            .unwrap()
            .iter()
            .any(|target| target["id"] == public_id)
    );
    assert!(
        !reference_candidates.body["data"]
            .as_array()
            .unwrap()
            .iter()
            .any(|target| target["id"] == hidden_id)
    );
    let content = json!({"schema_version":1,"body":{"type":"doc","content":[{"type":"paragraph","content":[
        {"type":"userMention","attrs":{"id":recipient}},
        {"type":"issueReference","attrs":{"id":public_id}},
        {"type":"issueReference","attrs":{"id":hidden_id}}
    ]}]}});
    let member_doc = get(h, &member, &doc_url).await;
    request(
        &h.router,
        Some(&member),
        "PATCH",
        &doc_url,
        Some(content.clone()),
        &[("if-match", member_doc.etag())],
    )
    .await
    .problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    let saved = request(
        &h.router,
        Some(&owner),
        "PATCH",
        &doc_url,
        Some(content.clone()),
        &[("if-match", initial.etag())],
    )
    .await;
    saved.expect(StatusCode::OK);
    assert_eq!(saved.body["body"], content["body"]);
    assert_eq!(saved.body["plain_text"], "@someone[issue][issue]");
    let read = get(h, &member, &doc_url).await;
    assert_eq!(read.body["mentions"][0]["id"], recipient);
    assert_eq!(read.body["mentions"][0]["available"], true);
    let redacted = read.body["issue_references"]
        .as_array()
        .unwrap()
        .iter()
        .find(|target| target["id"] == hidden_id)
        .unwrap();
    assert_eq!(
        *redacted,
        json!({"id":hidden_id,"available":false,"key":null,"title":null})
    );
    let revision_url = format!(
        "{doc_url}/revisions/{}",
        saved.body["version"].as_str().unwrap()
    );
    assert_eq!(
        get(h, &member, &revision_url).await.body["issue_references"],
        read.body["issue_references"]
    );
    let mut forged = content.clone();
    forged["body"]["content"][0]["content"][0]["attrs"]["label"] = json!("Forged person label");
    request(
        &h.router,
        Some(&owner),
        "PATCH",
        &doc_url,
        Some(forged),
        &[("if-match", saved.etag())],
    )
    .await
    .problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    let mut follow_up = content.clone();
    follow_up["body"]["content"].as_array_mut().unwrap().push(json!({"type":"paragraph","content":[{"type":"text","text":"Additional planning context"}]}));
    let saved = request(
        &h.router,
        Some(&owner),
        "PATCH",
        &doc_url,
        Some(follow_up),
        &[("if-match", saved.etag())],
    )
    .await;
    saved.expect(StatusCode::OK);
    let subscription = get(
        h,
        &member,
        &format!("{base}/issues/{public_id}/subscription"),
    )
    .await;
    assert_eq!(subscription.body["subscribed"], false);
    assert_eq!(subscription.body["sources"], json!([]));
    let w = base.rsplit('/').next().unwrap().parse().unwrap();
    let actor = boot["members"][0]["id"].as_str().unwrap().parse().unwrap();
    let mut tx = h.lock_team(w, t.parse().unwrap(), actor).await;
    let intents: i64 = sqlx::query_scalar("SELECT count(*) FROM mention_intents WHERE workspace_id=$1 AND project_id=$2 AND document_id=$3 AND recipient_id=$4 AND issue_id IS NULL")
        .bind(w).bind(made.body["id"].as_str().unwrap().parse::<uuid::Uuid>().unwrap()).bind(doc_id.parse::<uuid::Uuid>().unwrap()).bind(recipient.parse::<uuid::Uuid>().unwrap()).fetch_one(&mut *tx).await.unwrap();
    assert_eq!(
        intents, 1,
        "Retained project mentions must not capture duplicate sources"
    );
    let subscriptions: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM issue_subscriptions WHERE workspace_id=$1 AND user_id=$2",
    )
    .bind(w)
    .bind(recipient.parse::<uuid::Uuid>().unwrap())
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert_eq!(
        subscriptions, 0,
        "Project mentions must not follow any issue"
    );
    tx.rollback().await.unwrap();
    let metadata = get(h, &owner, &format!("{url}/metadata")).await;
    let archived = request(
        &h.router,
        Some(&owner),
        "PATCH",
        &format!("{url}/metadata"),
        Some(json!({"archived":true})),
        &[("if-match", metadata.etag())],
    )
    .await;
    archived.expect(StatusCode::OK);
    let archived_doc = get(h, &owner, &doc_url).await;
    assert_eq!(archived_doc.body["can_edit"], false);
    assert_eq!(archived_doc.body["body"], saved.body["body"]);
    get(h, &viewer, &format!("{doc_url}/revisions")).await;
    request(
        &h.router,
        Some(&owner),
        "PATCH",
        &doc_url,
        Some(document("Archived overwrite")),
        &[("if-match", archived_doc.etag())],
    )
    .await
    .problem(StatusCode::CONFLICT, "conflict");
    request(
        &h.router,
        Some(&owner),
        "PATCH",
        &format!("{url}/metadata"),
        Some(json!({"archived":false})),
        &[("if-match", archived.etag())],
    )
    .await
    .expect(StatusCode::OK);
    let grant_url = format!("{base}/teams/{private_team}/members/{recipient}");
    let grant = request(
        &h.router,
        Some(&owner),
        "PUT",
        &grant_url,
        Some(json!({"role":"contributor"})),
        &[("if-none-match", "*")],
    )
    .await;
    grant.expect(StatusCode::CREATED);
    let logged_in = request(&h.router, None, "POST", "/api/v1/auth/login", Some(json!({"email":"member@projects.example.test","password":"Project member verification password 2026!"})), &[]).await;
    logged_in.expect(StatusCode::OK);
    let member = client(&logged_in);
    let permitted = get(h, &member, &secret_doc_url).await;
    for path in &private_endpoints {
        get(h, &member, path).await;
    }
    let visible = get(h, &member, &doc_url).await;
    assert!(
        visible.body["issue_references"]
            .as_array()
            .unwrap()
            .iter()
            .any(|target| target["id"] == hidden_id && target["available"] == true)
    );
    request(
        &h.router,
        Some(&owner),
        "DELETE",
        &grant_url,
        None,
        &[("if-match", grant.etag())],
    )
    .await
    .expect(StatusCode::NO_CONTENT);
    for path in &private_endpoints {
        request(
            &h.router,
            Some(&member),
            "GET",
            path,
            None,
            &[("if-none-match", permitted.etag())],
        )
        .await
        .problem(StatusCode::NOT_FOUND, "not_found");
    }
    request(
        &h.router,
        Some(&member),
        "PATCH",
        &secret_doc_url,
        Some(content),
        &[("if-match", permitted.etag())],
    )
    .await
    .problem(StatusCode::NOT_FOUND, "not_found");
    let hidden_again = get(h, &member, &doc_url).await;
    assert_ne!(hidden_again.etag(), visible.etag());
    assert_eq!(hidden_again.body["version"], visible.body["version"]);
    assert!(
        hidden_again.body["issue_references"]
            .as_array()
            .unwrap()
            .iter()
            .any(|target| target == redacted)
    );
    assert_eq!(
        get(h, &member, &revision_url).await.body["issue_references"],
        read.body["issue_references"]
    );
}
