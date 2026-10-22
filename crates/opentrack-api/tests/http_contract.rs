mod support;

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{HeaderMap, Request, StatusCode},
};
use serde_json::{Value, json};
use support::{Harness, INDEX, ORIGIN};
use tower::ServiceExt;
use uuid::Uuid;

struct Reply {
    status: StatusCode,
    headers: HeaderMap,
    body: Vec<u8>,
}
impl Reply {
    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).expect("Expected JSON response")
    }
    fn text(&self) -> &str {
        std::str::from_utf8(&self.body).expect("Expected UTF-8 response")
    }
    fn header(&self, name: &str) -> &str {
        self.headers
            .get(name)
            .unwrap_or_else(|| panic!("Missing {name} response header"))
            .to_str()
            .unwrap()
    }
    fn api_error(&self, status: StatusCode, code: &str) {
        assert_eq!(self.status, status, "{}", self.text());
        assert_eq!(self.header("cache-control"), "no-store");
        assert_eq!(self.header("content-type"), "application/problem+json");
        let body = self.json();
        assert!(
            body.get("error").is_none(),
            "Problem fields must be top-level"
        );
        assert_eq!(body["code"], code);
        assert_eq!(body["status"], status.as_u16());
        for field in ["title", "detail"] {
            assert!(
                body[field].as_str().is_some_and(|v| !v.is_empty()),
                "Problem lacks {field}"
            );
        }
        assert_eq!(body["type"], format!("/api/v1/problems/{code}"));
        let request_id =
            Uuid::parse_str(body["request_id"].as_str().unwrap()).expect("Errors have request IDs");
        assert_eq!(body["instance"], format!("urn:uuid:{request_id}"));
        assert_eq!(body["request_id"], self.header("x-request-id"));
        assert_eq!(
            body["retryable"],
            matches!(
                status,
                StatusCode::TOO_MANY_REQUESTS | StatusCode::SERVICE_UNAVAILABLE
            )
        );
        for field in body["field_errors"]
            .as_array()
            .expect("field_errors is always an array")
        {
            for key in ["path", "code", "message"] {
                assert!(field[key].is_string(), "Field error lacks {key}");
            }
        }
    }
}

async fn request(
    router: &Router,
    method: &str,
    path: &str,
    body: Option<Value>,
    headers: &[(&str, &str)],
) -> Reply {
    let mut headers = headers.to_vec();
    if body.is_some()
        && !headers
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case("content-type"))
    {
        headers.push(("content-type", "application/json"));
    }
    let payload = body
        .map(|body| serde_json::to_vec(&body).unwrap())
        .unwrap_or_default();
    raw_request(router, method, path, payload, &headers).await
}

async fn raw_request(
    router: &Router,
    method: &str,
    path: &str,
    payload: Vec<u8>,
    headers: &[(&str, &str)],
) -> Reply {
    let mut builder = Request::builder().method(method).uri(path);
    for (key, value) in headers {
        builder = builder.header(*key, *value);
    }
    let caller_request_id = headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("x-request-id"))
        .map(|(_, value)| *value);
    let response = router
        .clone()
        .oneshot(builder.body(Body::from(payload)).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let request_id = headers["x-request-id"].to_str().unwrap();
    Uuid::parse_str(request_id).expect("Every response has a generated request UUID");
    assert_ne!(
        caller_request_id,
        Some(request_id),
        "Callers cannot spoof server correlation IDs"
    );
    assert_eq!(headers["x-content-type-options"], "nosniff");
    assert_eq!(headers["referrer-policy"], "same-origin");
    let csp = headers["content-security-policy"].to_str().unwrap();
    assert!(
        csp.contains("frame-ancestors 'none'")
            && csp.contains("script-src 'self'")
            && csp.contains("base-uri 'self'")
    );
    let body = to_bytes(response.into_body(), 2 * 1024 * 1024)
        .await
        .unwrap()
        .to_vec();
    Reply {
        status,
        headers,
        body,
    }
}

fn cookie(reply: &Reply) -> String {
    let set_cookie = reply.header("set-cookie");
    for attribute in ["HttpOnly", "SameSite=Lax", "Path=/", "Secure"] {
        assert!(
            set_cookie.contains(attribute),
            "Session cookie lacks {attribute}"
        );
    }
    set_cookie.split(';').next().unwrap().to_owned()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_http_authentication_mutation_and_delivery_contracts() {
    let harness = Harness::new().await;
    let runner = harness.clone();
    let result = tokio::spawn(async move { exercise(&runner).await }).await;
    harness.cleanup().await;
    if let Err(error) = result {
        std::panic::resume_unwind(error.into_panic());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_http_identity_management_contracts() {
    // Separate router state keeps authentication admission limits independent of
    // the deliberately abusive rate-limit cases in the original HTTP suite.
    let harness = Harness::new().await;
    let runner = harness.clone();
    let result = tokio::spawn(async move { identity_management(&runner).await }).await;
    harness.cleanup().await;
    if let Err(error) = result {
        std::panic::resume_unwind(error.into_panic());
    }
}

async fn identity_management(h: &Harness) {
    let router = &h.router;
    let setup = request(
        router,
        "POST",
        "/api/v1/setup",
        Some(json!({
            "setup_token":h.setup_token,"email":"identity-http@contract.invalid",
            "password":"HTTP identity passphrase 492!","display_name":"HTTP identity owner",
            "workspace_name":"HTTP identity","team_name":"Engineering","team_key":"ENG"
        })),
        &[("origin", ORIGIN)],
    )
    .await;
    assert_eq!(setup.status, StatusCode::CREATED, "{}", setup.text());
    let owner_cookie = cookie(&setup);
    let session = setup.json();
    let csrf = session["csrf_token"].as_str().unwrap();
    let w = session["workspaces"][0]["id"].as_str().unwrap();
    let headers = [
        ("origin", ORIGIN),
        ("cookie", owner_cookie.as_str()),
        ("x-csrf-token", csrf),
    ];
    request(router, "GET", "/api/v1/organizations", None, &[])
        .await
        .api_error(StatusCode::UNAUTHORIZED, "unauthenticated");
    let organizations = request(router, "GET", "/api/v1/organizations", None, &headers).await;
    assert_eq!(organizations.status, StatusCode::OK);
    let organization = organizations.json()[0]["id"].as_str().unwrap().to_owned();
    let mut create_headers = headers.to_vec();
    create_headers.push(("idempotency-key", "http-identity-workspace"));
    let created=request(router,"POST","/api/v1/workspaces",Some(json!({"organization_id":organization,"name":"Second HTTP workspace","team_name":"Operations","team_key":"OPS"})),&create_headers).await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.text());
    let workspace_path = created.header("location").to_owned();
    let second = created.json()["id"].as_str().unwrap().to_owned();
    assert_eq!(workspace_path, format!("/api/v1/workspaces/{second}"));
    assert_eq!(created.header("etag"), format!("\"workspace:{second}:1\""));
    request(
        router,
        "PATCH",
        &workspace_path,
        Some(json!({"name":"Missing precondition"})),
        &headers,
    )
    .await
    .api_error(StatusCode::PRECONDITION_REQUIRED, "precondition_required");
    let mut change_headers = headers.to_vec();
    change_headers.push(("if-match", created.header("etag")));
    let renamed = request(
        router,
        "PATCH",
        &workspace_path,
        Some(json!({"name":"Renamed HTTP workspace"})),
        &change_headers,
    )
    .await;
    assert_eq!(renamed.status, StatusCode::OK, "{}", renamed.text());
    assert_eq!(renamed.json()["id"], second);
    let stale = request(
        router,
        "PATCH",
        &workspace_path,
        Some(json!({"name":"Stale overwrite"})),
        &change_headers,
    )
    .await;
    stale.api_error(StatusCode::PRECONDITION_FAILED, "version_conflict");
    assert_eq!(
        stale.json()["details"]["current"]["name"],
        "Renamed HTTP workspace"
    );

    create_headers.pop();
    create_headers.push(("idempotency-key", "http-identity-team"));
    let teams_path = format!("/api/v1/workspaces/{w}/teams");
    let team = request(
        router,
        "POST",
        &teams_path,
        Some(json!({"name":"Delivery","key":"SHIP","private":false})),
        &create_headers,
    )
    .await;
    assert_eq!(team.status, StatusCode::CREATED, "{}", team.text());
    let team_id = team.json()["id"].as_str().unwrap().to_owned();
    assert_eq!(team.header("location"), format!("{teams_path}/{team_id}"));
    assert_eq!(team.header("etag"), format!("\"team:{team_id}:1\""));
    let fetched_team = request(router, "GET", team.header("location"), None, &headers).await;
    assert_eq!(
        fetched_team.status,
        StatusCode::OK,
        "{}",
        fetched_team.text()
    );
    assert_eq!(fetched_team.header("etag"), team.header("etag"));
    assert_eq!(fetched_team.json()["id"], team_id);
    let listed = request(router, "GET", &teams_path, None, &headers).await;
    assert_eq!(listed.status, StatusCode::OK);
    assert!(
        listed.json()["data"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["id"] == team_id)
    );

    let invites_path = format!("/api/v1/workspaces/{w}/invitations");
    create_headers.pop();
    create_headers.push(("idempotency-key", "http-identity-invite"));
    let invite_body = json!({"email":"new-http@contract.invalid","role":"member"});
    let invitation = request(
        router,
        "POST",
        &invites_path,
        Some(invite_body.clone()),
        &create_headers,
    )
    .await;
    assert_eq!(
        invitation.status,
        StatusCode::CREATED,
        "{}",
        invitation.text()
    );
    let invite_id = invitation.json()["invitation"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let token = invitation.json()["token"].as_str().unwrap().to_owned();
    assert_eq!(
        invitation.header("location"),
        format!("{invites_path}/{invite_id}")
    );
    assert_eq!(
        invitation.header("etag"),
        format!("\"invitation:{invite_id}:1\"")
    );
    let fetched_invitation =
        request(router, "GET", invitation.header("location"), None, &headers).await;
    assert_eq!(
        fetched_invitation.status,
        StatusCode::OK,
        "{}",
        fetched_invitation.text()
    );
    assert_eq!(fetched_invitation.header("etag"), invitation.header("etag"));
    assert_eq!(fetched_invitation.json()["id"], invite_id);
    assert!(fetched_invitation.json().get("token").is_none());
    assert!(!fetched_invitation.text().contains(&token));
    let replay = request(
        router,
        "POST",
        &invites_path,
        Some(invite_body),
        &create_headers,
    )
    .await;
    assert_eq!(replay.status, StatusCode::CREATED);
    assert_eq!(replay.json()["invitation"]["id"], invite_id);
    assert!(replay.json()["token"].is_null());
    for origin_headers in [vec![], vec![("origin", "https://attacker.invalid")]] {
        request(
            router,
            "POST",
            "/api/v1/invitations/preview",
            Some(json!({"token":token})),
            &origin_headers,
        )
        .await
        .api_error(StatusCode::FORBIDDEN, "forbidden");
    }
    let preview = request(
        router,
        "POST",
        "/api/v1/invitations/preview",
        Some(json!({"token":token})),
        &[("origin", ORIGIN)],
    )
    .await;
    assert_eq!(preview.status, StatusCode::OK, "{}", preview.text());
    assert_eq!(preview.json()["workspace_id"], w);
    assert_eq!(preview.json()["email"], "new-http@contract.invalid");
    let acceptance = json!({"token":token,"display_name":"New HTTP member","password":"HTTP member passphrase 935!"});
    request(
        router,
        "POST",
        "/api/v1/invitations/accept",
        Some(acceptance.clone()),
        &[("origin", "https://attacker.invalid")],
    )
    .await
    .api_error(StatusCode::FORBIDDEN, "forbidden");
    let accepted = request(
        router,
        "POST",
        "/api/v1/invitations/accept",
        Some(acceptance),
        &[("origin", ORIGIN)],
    )
    .await;
    assert_eq!(accepted.status, StatusCode::OK, "{}", accepted.text());
    let member_cookie = cookie(&accepted);
    let member_session = accepted.json();
    let member_id = member_session["user"]["id"].as_str().unwrap();
    let member_csrf = member_session["csrf_token"].as_str().unwrap();
    let member_path = format!("/api/v1/workspaces/{w}/members/{member_id}");
    request(
        router,
        "PATCH",
        &member_path,
        Some(json!({"role":"viewer"})),
        &headers,
    )
    .await
    .api_error(StatusCode::PRECONDITION_REQUIRED, "precondition_required");
    request(
        router,
        "PATCH",
        &member_path,
        Some(json!({"role":"viewer"})),
        &[
            ("origin", ORIGIN),
            ("cookie", &owner_cookie),
            ("if-match", &format!("\"member:{w}:{member_id}:1\"")),
        ],
    )
    .await
    .api_error(StatusCode::FORBIDDEN, "forbidden");
    let team_member_path = format!("{teams_path}/{team_id}/members/{member_id}");
    request(
        router,
        "PUT",
        &team_member_path,
        Some(json!({"role":"contributor"})),
        &headers,
    )
    .await
    .api_error(StatusCode::PRECONDITION_REQUIRED, "precondition_required");
    let mut grant_headers = headers.to_vec();
    grant_headers.push(("if-none-match", "*"));
    let grant = request(
        router,
        "PUT",
        &team_member_path,
        Some(json!({"role":"contributor"})),
        &grant_headers,
    )
    .await;
    assert_eq!(grant.status, StatusCode::CREATED, "{}", grant.text());
    assert_eq!(grant.header("location"), team_member_path);
    assert_eq!(
        grant.header("etag"),
        format!(
            "\"team-member:{team_id}:{member_id}:{}\"",
            grant.json()["version"].as_str().unwrap()
        )
    );
    let fetched_grant = request(router, "GET", grant.header("location"), None, &headers).await;
    assert_eq!(
        fetched_grant.status,
        StatusCode::OK,
        "{}",
        fetched_grant.text()
    );
    assert_eq!(fetched_grant.header("etag"), grant.header("etag"));
    assert_eq!(fetched_grant.json()["user"]["id"], member_id);
    grant_headers.push(("if-match", grant.header("etag")));
    request(
        router,
        "PUT",
        &team_member_path,
        Some(json!({"role":"maintainer"})),
        &grant_headers,
    )
    .await
    .api_error(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");

    create_headers.pop();
    create_headers.push(("idempotency-key", "http-identity-existing"));
    let existing = request(
        router,
        "POST",
        &format!("/api/v1/workspaces/{second}/invitations"),
        Some(json!({"email":"new-http@contract.invalid","role":"member"})),
        &create_headers,
    )
    .await;
    assert_eq!(existing.status, StatusCode::CREATED, "{}", existing.text());
    let existing_token = existing.json()["token"].as_str().unwrap().to_owned();
    request(
        router,
        "POST",
        "/api/v1/invitations/accept",
        Some(json!({"token":existing_token})),
        &[("origin", ORIGIN)],
    )
    .await
    .api_error(StatusCode::CONFLICT, "invite_sign_in_required");
    request(
        router,
        "POST",
        "/api/v1/invitations/accept",
        Some(json!({"token":existing_token})),
        &[("origin", ORIGIN), ("cookie", &member_cookie)],
    )
    .await
    .api_error(StatusCode::FORBIDDEN, "forbidden");
    let accepted_existing = request(
        router,
        "POST",
        "/api/v1/invitations/accept",
        Some(json!({"token":existing_token})),
        &[
            ("origin", ORIGIN),
            ("cookie", &member_cookie),
            ("x-csrf-token", member_csrf),
        ],
    )
    .await;
    assert_eq!(
        accepted_existing.status,
        StatusCode::OK,
        "{}",
        accepted_existing.text()
    );
    let current_cookie = cookie(&accepted_existing);
    assert_eq!(accepted_existing.json()["user"]["id"], member_id);
    assert_eq!(
        accepted_existing.json()["workspaces"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let bootstrap_path = format!("/api/v1/workspaces/{w}/bootstrap");
    let before = request(router, "GET", &bootstrap_path, None, &headers).await;
    let membership = request(router, "GET", &member_path, None, &headers).await;
    assert_eq!(membership.status, StatusCode::OK, "{}", membership.text());
    let mut revoke_headers = headers.to_vec();
    revoke_headers.push(("if-match", membership.header("etag")));
    let revoked = request(
        router,
        "PATCH",
        &member_path,
        Some(json!({"active":false})),
        &revoke_headers,
    )
    .await;
    assert_eq!(revoked.status, StatusCode::OK, "{}", revoked.text());
    let after = request(router, "GET", &bootstrap_path, None, &headers).await;
    assert_ne!(before.json()["auth_epoch"], after.json()["auth_epoch"]);
    let inaccessible = request(
        router,
        "GET",
        team.header("location"),
        None,
        &[("cookie", &current_cookie)],
    )
    .await;
    match inaccessible.status {
        StatusCode::FORBIDDEN => inaccessible.api_error(StatusCode::FORBIDDEN, "forbidden"),
        _ => inaccessible.api_error(StatusCode::NOT_FOUND, "not_found"),
    }
    request(router, "GET", &team_member_path, None, &headers)
        .await
        .api_error(StatusCode::NOT_FOUND, "not_found");
    let other_workspace = request(
        router,
        "GET",
        &workspace_path,
        None,
        &[("cookie", &current_cookie)],
    )
    .await;
    assert_eq!(
        other_workspace.status,
        StatusCode::OK,
        "Workspace deactivation must preserve access elsewhere"
    );
}

async fn exercise(h: &Harness) {
    let router = &h.router;
    for route in ["/health/live", "/health/ready"] {
        assert_eq!(
            request(router, "GET", route, None, &[]).await.status,
            StatusCode::OK
        );
    }
    request(
        router,
        "GET",
        "/api/v1/auth/session",
        None,
        &[("x-request-id", "00000000-0000-4000-8000-000000000000")],
    )
    .await
    .api_error(StatusCode::UNAUTHORIZED, "unauthenticated");
    let status = request(router, "GET", "/api/v1/setup/status", None, &[]).await;
    assert_eq!(status.json()["setup_required"], true);
    assert_eq!(status.header("cache-control"), "no-store");
    let setup = json!({
        "setup_token":h.setup_token, "email":"http-owner@contract.invalid", "password":"HTTP test passphrase 724!",
        "display_name":"HTTP owner", "workspace_name":"HTTP workspace", "team_name":"Engineering", "team_key":"ENG"
    });
    request(
        router,
        "POST",
        "/api/v1/setup",
        Some(setup.clone()),
        &[("origin", "https://attacker.invalid")],
    )
    .await
    .api_error(StatusCode::FORBIDDEN, "forbidden");
    request(router, "POST", "/api/v1/setup", Some(setup.clone()), &[])
        .await
        .api_error(StatusCode::FORBIDDEN, "forbidden");
    let mut unknown_setup = setup.clone();
    unknown_setup["make_me_admin"] = json!(true);
    request(
        router,
        "POST",
        "/api/v1/setup",
        Some(unknown_setup),
        &[("origin", ORIGIN)],
    )
    .await
    .api_error(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    let created = request(
        router,
        "POST",
        "/api/v1/setup",
        Some(setup.clone()),
        &[("origin", ORIGIN)],
    )
    .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.text());
    let setup_cookie = cookie(&created);
    let session = created.json();
    let workspace = session["workspaces"][0]["id"].as_str().unwrap();
    request(
        router,
        "POST",
        "/api/v1/setup",
        Some(setup),
        &[("origin", ORIGIN)],
    )
    .await
    .api_error(StatusCode::CONFLICT, "conflict");
    let setup_status = request(router, "GET", "/api/v1/setup/status", None, &[]).await;
    assert_eq!(setup_status.json()["setup_required"], false);

    let login =
        json!({"email":"http-owner@contract.invalid", "password":"HTTP test passphrase 724!"});
    request(
        router,
        "POST",
        "/api/v1/auth/login",
        Some(login.clone()),
        &[("origin", "https://attacker.invalid")],
    )
    .await
    .api_error(StatusCode::FORBIDDEN, "forbidden");
    let signed_in = request(
        router,
        "POST",
        "/api/v1/auth/login",
        Some(login),
        &[("origin", ORIGIN)],
    )
    .await;
    assert_eq!(signed_in.status, StatusCode::OK, "{}", signed_in.text());
    let login_cookie = cookie(&signed_in);
    assert_ne!(setup_cookie, login_cookie);
    let signed_in_body = signed_in.json();
    let csrf = signed_in_body["csrf_token"].as_str().unwrap();
    let cookies = format!("unrelated_cookie=value; {login_cookie}");
    let auth_headers = [
        ("origin", ORIGIN),
        ("cookie", cookies.as_str()),
        ("x-csrf-token", csrf),
    ];
    let session_reply = request(
        router,
        "GET",
        "/api/v1/auth/session",
        None,
        &[("cookie", &cookies)],
    )
    .await;
    assert_eq!(session_reply.status, StatusCode::OK);
    assert_eq!(session_reply.json()["user"]["id"], session["user"]["id"]);
    let bootstrap = request(
        router,
        "GET",
        &format!("/api/v1/workspaces/{workspace}/bootstrap"),
        None,
        &[("cookie", &cookies)],
    )
    .await
    .json();
    let team = bootstrap["teams"][0]["id"].as_str().unwrap();
    let collection = format!("/api/v1/workspaces/{workspace}/issues");
    let create = json!({"team_id":team, "title":"HTTP durable issue", "priority":2});
    request(
        router,
        "POST",
        &collection,
        Some(create.clone()),
        &[("origin", ORIGIN)],
    )
    .await
    .api_error(StatusCode::UNAUTHORIZED, "unauthenticated");
    request(
        router,
        "POST",
        &collection,
        Some(create.clone()),
        &[
            ("origin", ORIGIN),
            ("cookie", &cookies),
            ("idempotency-key", "missing-csrf-test"),
        ],
    )
    .await
    .api_error(StatusCode::FORBIDDEN, "forbidden");
    request(
        router,
        "POST",
        &collection,
        Some(create.clone()),
        &[
            ("origin", ORIGIN),
            ("cookie", &cookies),
            ("x-csrf-token", "wrong"),
            ("idempotency-key", "wrong-csrf-test"),
        ],
    )
    .await
    .api_error(StatusCode::FORBIDDEN, "forbidden");
    request(
        router,
        "POST",
        &collection,
        Some(create.clone()),
        &[
            ("origin", "https://attacker.invalid"),
            ("cookie", &cookies),
            ("x-csrf-token", csrf),
            ("idempotency-key", "wrong-origin-test"),
        ],
    )
    .await
    .api_error(StatusCode::FORBIDDEN, "forbidden");
    request(
        router,
        "POST",
        &collection,
        Some(create.clone()),
        &auth_headers,
    )
    .await
    .api_error(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    let mut creation_headers = auth_headers.to_vec();
    creation_headers.push(("idempotency-key", "http-creation-contract"));
    framework_rejections(
        router,
        workspace,
        &collection,
        &creation_headers,
        &cookies,
        &create,
    )
    .await;
    let mut unknown_create = create.clone();
    unknown_create["unexpected"] = json!(true);
    request(
        router,
        "POST",
        &collection,
        Some(unknown_create),
        &creation_headers,
    )
    .await
    .api_error(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    let issue = request(
        router,
        "POST",
        &collection,
        Some(create.clone()),
        &creation_headers,
    )
    .await;
    assert_eq!(issue.status, StatusCode::CREATED, "{}", issue.text());
    assert_eq!(issue.header("cache-control"), "no-store");
    let issue_body = issue.json();
    let id = issue_body["id"].as_str().unwrap();
    let item = format!("{collection}/{id}");
    assert_eq!(issue.header("location"), item);
    assert_eq!(issue_body["etag"], issue.header("etag"));
    let reference_digest = issue
        .header("etag")
        .strip_prefix(&format!("\"issue:{id}:1:"))
        .and_then(|value| value.strip_suffix('"'))
        .expect("Strong issue validator includes its reference digest");
    assert_eq!(reference_digest.len(), 64);
    assert!(
        reference_digest
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    );
    let replay = request(
        router,
        "POST",
        &collection,
        Some(create.clone()),
        &creation_headers,
    )
    .await;
    assert_eq!(replay.json(), issue_body);
    create_changed_key_conflict(router, &collection, create, &creation_headers).await;
    request(
        router,
        "PATCH",
        &item,
        Some(json!({"title":"Missing precondition"})),
        &auth_headers,
    )
    .await
    .api_error(StatusCode::PRECONDITION_REQUIRED, "precondition_required");
    let mut patch_headers = auth_headers.to_vec();
    patch_headers.push(("if-match", issue.header("etag")));
    request(
        router,
        "PATCH",
        &item,
        Some(json!({"unexpected":true,"title":"Must not partially apply"})),
        &patch_headers,
    )
    .await
    .api_error(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    let updated = request(
        router,
        "PATCH",
        &item,
        Some(json!({"title":"HTTP accepted title","estimate":5})),
        &patch_headers,
    )
    .await;
    assert_eq!(updated.status, StatusCode::OK, "{}", updated.text());
    assert_eq!(updated.json()["etag"], updated.header("etag"));
    assert_eq!(
        updated.header("etag"),
        format!("\"issue:{id}:2:{reference_digest}\"")
    );
    let stale = request(
        router,
        "PATCH",
        &item,
        Some(json!({"title":"Lost update attempt"})),
        &patch_headers,
    )
    .await;
    stale.api_error(StatusCode::PRECONDITION_FAILED, "version_conflict");
    assert_eq!(
        stale.json()["details"]["current"]["title"],
        "HTTP accepted title"
    );
    let read = request(router, "GET", &item, None, &[("cookie", &cookies)]).await;
    assert_eq!(read.json(), updated.json());
    assert_eq!(read.header("etag"), updated.header("etag"));
    let activity = request(
        router,
        "GET",
        &format!("{item}/activity"),
        None,
        &[("cookie", &cookies)],
    )
    .await;
    assert_eq!(activity.json()["data"].as_array().unwrap().len(), 2);

    let blocked_request =
        json!({"team_id":team,"title":"Request blocked by a concurrent transaction"});
    let mut blocked_headers = auth_headers.to_vec();
    blocked_headers.push(("idempotency-key", "http-lock-timeout-contract"));
    let lock = h
        .lock_team(
            Uuid::parse_str(workspace).unwrap(),
            Uuid::parse_str(team).unwrap(),
            Uuid::parse_str(session["user"]["id"].as_str().unwrap()).unwrap(),
        )
        .await;
    request(
        router,
        "POST",
        &collection,
        Some(blocked_request.clone()),
        &blocked_headers,
    )
    .await
    .api_error(StatusCode::SERVICE_UNAVAILABLE, "dependency_unavailable");
    lock.rollback().await.unwrap();
    assert_eq!(
        request(
            router,
            "POST",
            &collection,
            Some(blocked_request),
            &blocked_headers
        )
        .await
        .status,
        StatusCode::CREATED,
        "Timed-out writes roll back and can safely retry once the dependency recovers"
    );

    request(
        router,
        "POST",
        "/api/v1/auth/logout",
        None,
        &[("origin", ORIGIN), ("cookie", &cookies)],
    )
    .await
    .api_error(StatusCode::FORBIDDEN, "forbidden");
    let logout = request(router, "POST", "/api/v1/auth/logout", None, &auth_headers).await;
    assert_eq!(logout.status, StatusCode::NO_CONTENT);
    assert!(logout.body.is_empty());
    assert!(logout.header("set-cookie").contains("Max-Age=0"));
    request(
        router,
        "GET",
        "/api/v1/auth/session",
        None,
        &[("cookie", &cookies)],
    )
    .await
    .api_error(StatusCode::UNAUTHORIZED, "unauthenticated");
    request(
        router,
        "PATCH",
        &item,
        Some(json!({"title":"Revoked cookie"})),
        &patch_headers,
    )
    .await
    .api_error(StatusCode::UNAUTHORIZED, "unauthenticated");
    assert_eq!(
        request(
            router,
            "GET",
            "/api/v1/auth/session",
            None,
            &[("cookie", &setup_cookie)]
        )
        .await
        .status,
        StatusCode::OK
    );

    delivery(router).await;
    let mut limited = false;
    for _ in 0..25 {
        let response = request(
            router,
            "POST",
            "/api/v1/auth/login",
            Some(json!({})),
            &[("origin", "https://attacker.invalid")],
        )
        .await;
        if response.status == StatusCode::TOO_MANY_REQUESTS {
            response.api_error(StatusCode::TOO_MANY_REQUESTS, "rate_limited");
            assert!(response.header("retry-after").parse::<u64>().unwrap() > 0);
            limited = true;
            break;
        }
        response.api_error(StatusCode::FORBIDDEN, "forbidden");
    }
    assert!(
        limited,
        "Bounded auth admission must reject the repeated requests"
    );
}

async fn framework_rejections(
    router: &Router,
    workspace: &str,
    collection: &str,
    headers: &[(&str, &str)],
    cookie: &str,
    create: &Value,
) {
    for path in [
        "/api/v1/workspaces/not-a-uuid/issues".to_owned(),
        format!("/api/v1/workspaces/{workspace}/issues/not-a-uuid"),
        format!("{collection}?limit=not-a-number"),
        format!("{collection}?unknown_option=true"),
    ] {
        request(router, "GET", &path, None, &[("cookie", cookie)])
            .await
            .api_error(StatusCode::BAD_REQUEST, "bad_request");
    }
    request(
        router,
        "GET",
        &format!("{collection}?limit=101"),
        None,
        &[("cookie", cookie)],
    )
    .await
    .api_error(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    let mut json_headers = headers.to_vec();
    json_headers.push(("content-type", "application/json"));
    raw_request(
        router,
        "POST",
        collection,
        b"{ invalid JSON".to_vec(),
        &json_headers,
    )
    .await
    .api_error(StatusCode::BAD_REQUEST, "bad_request");
    raw_request(router, "POST", collection, Vec::new(), &json_headers)
        .await
        .api_error(StatusCode::BAD_REQUEST, "bad_request");
    let mut unsupported_headers = headers.to_vec();
    unsupported_headers.push(("content-type", "text/plain"));
    request(
        router,
        "POST",
        collection,
        Some(create.clone()),
        &unsupported_headers,
    )
    .await
    .api_error(StatusCode::UNSUPPORTED_MEDIA_TYPE, "unsupported_media_type");
    let mut oversized = create.clone();
    oversized["description"] = json!("x".repeat(1024 * 1024));
    request(router, "POST", collection, Some(oversized), headers)
        .await
        .api_error(StatusCode::PAYLOAD_TOO_LARGE, "payload_too_large");
    let method = request(router, "PUT", collection, Some(create.clone()), headers).await;
    method.api_error(StatusCode::METHOD_NOT_ALLOWED, "method_not_allowed");
    assert!(method.header("allow").contains("GET") && method.header("allow").contains("POST"));
}

async fn create_changed_key_conflict(
    router: &Router,
    collection: &str,
    mut create: Value,
    headers: &[(&str, &str)],
) {
    create["title"] = json!("Different body with the same retry key");
    request(router, "POST", collection, Some(create), headers)
        .await
        .api_error(StatusCode::CONFLICT, "conflict");
}

async fn delivery(router: &Router) {
    for route in ["/", "/workspace/issues/ENG-42"] {
        let page = request(router, "GET", route, None, &[("accept", "text/html")]).await;
        assert_eq!(page.status, StatusCode::OK);
        assert_eq!(page.text(), INDEX);
        assert_eq!(page.header("cache-control"), "no-cache");
        assert!(page.header("content-type").starts_with("text/html"));
    }
    for route in [
        "/api/v1/nonexistent",
        "/api/nonexistent",
        "/missing.js",
        "/assets/missing.js",
    ] {
        let missing = request(router, "GET", route, None, &[("accept", "text/html")]).await;
        assert_eq!(missing.status, StatusCode::NOT_FOUND, "{route}");
        if route.starts_with("/api/") {
            missing.api_error(StatusCode::NOT_FOUND, "not_found");
        }
        assert_ne!(
            missing.text(),
            INDEX,
            "Missing API/assets must not return the SPA shell"
        );
    }
    let asset = request(router, "GET", "/assets/app-a1b2c3d4.js", None, &[]).await;
    assert_eq!(asset.status, StatusCode::OK);
    assert_eq!(asset.text(), "console.log('contract asset');");
    let cache = asset.header("cache-control");
    assert!(
        cache.contains("immutable") && cache.contains("max-age=31536000"),
        "Content-hashed assets should be cached immutably: {cache}"
    );
    let non_navigation = request(
        router,
        "GET",
        "/unrecognized-resource",
        None,
        &[("accept", "application/json")],
    )
    .await;
    assert_eq!(
        non_navigation.status,
        StatusCode::NOT_FOUND,
        "SPA fallback is reserved for HTML navigation"
    );
    let post = request(
        router,
        "POST",
        "/workspace/issues/ENG-42",
        None,
        &[("accept", "text/html")],
    )
    .await;
    assert!(
        matches!(
            post.status,
            StatusCode::NOT_FOUND | StatusCode::METHOD_NOT_ALLOWED
        ),
        "SPA fallback must not turn unknown mutations into successful pages"
    );
}
