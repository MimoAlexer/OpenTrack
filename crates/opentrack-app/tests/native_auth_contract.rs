//! Browser-approved native credentials use the same guarded account lifecycle.
#[allow(dead_code)]
mod support;
use opentrack_domain::*;
use opentrack_store as store;
use serde_json::{Value, json};
use sqlx::AssertSqlSafe;
use support::Harness;
use url::Url;
use uuid::Uuid;

const PASSWORD: &str = "Native authentication contract password 791!";
const EMAIL: &str = "native-owner@example.invalid";
const VERIFIER: &str = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
fn req<T: serde::de::DeserializeOwned>(v: Value) -> T {
    serde_json::from_value(v).unwrap()
}
async fn setup(h: &Harness) -> (Actor, String, Uuid) {
    let (s,c)=h.app.setup(req(json!({"setup_token":h.setup_token,"email":EMAIL,"password":PASSWORD,"display_name":"Native owner","workspace_name":"Native workspace","team_name":"Engineering","team_key":"NAT"}))).await.unwrap();
    assert_eq!(s.audience, SessionAudience::Web);
    (h.app.authenticate(&c).await.unwrap(), c, s.workspaces[0].id)
}
async fn login(h: &Harness, email: &str) -> (Actor, String) {
    let (_, c) = h
        .app
        .login(req(json!({"email":email,"password":PASSWORD})))
        .await
        .unwrap();
    (h.app.authenticate(&c).await.unwrap(), c)
}
async fn start(h: &Harness, previous: Option<&Actor>) -> NativeAuthStartReceipt {
    h.app.start_native_auth(previous, req(json!({"code_challenge":native_code_challenge(VERIFIER).unwrap(),"callback_uri":"http://127.0.0.1:48523/opentrack/native-callback","state":"a".repeat(64),"device_name":"Contract desktop"}))).await.unwrap()
}
async fn approve(
    h: &Harness,
    actor: &Actor,
    receipt: &NativeAuthStartReceipt,
) -> NativeAuthExchange {
    let result = h
        .app
        .approve_native_auth(
            actor,
            receipt.id,
            req(json!({"confirmation_code":receipt.confirmation_code})),
        )
        .await
        .unwrap();
    let uri = Url::parse(&result.callback_uri).unwrap();
    assert_eq!(uri.host_str(), Some("127.0.0.1"));
    assert_eq!(uri.port(), Some(48523));
    let pairs = uri.query_pairs().collect::<Vec<_>>();
    assert_eq!(pairs.len(), 3);
    assert_eq!(
        pairs.iter().find(|(k, _)| k == "request_id").unwrap().1,
        receipt.id.to_string()
    );
    assert_eq!(
        pairs.iter().find(|(k, _)| k == "state").unwrap().1,
        "a".repeat(64)
    );
    NativeAuthExchange {
        request_id: receipt.id,
        code: pairs
            .iter()
            .find(|(k, _)| k == "code")
            .unwrap()
            .1
            .to_string(),
        code_verifier: VERIFIER.into(),
    }
}
fn copy_exchange(e: &NativeAuthExchange) -> NativeAuthExchange {
    req(serde_json::to_value(e).unwrap())
}
async fn grant(h: &Harness, browser: &Actor, previous: Option<&Actor>) -> NativeSessionGrant {
    let s = start(h, previous).await;
    let e = approve(h, browser, &s).await;
    h.app.exchange_native_auth(e).await.unwrap()
}
async fn public_id(h: &Harness, actor: &Actor) -> Uuid {
    sqlx::query_scalar("SELECT id FROM sessions WHERE user_id=$1 AND token_hash=$2")
        .bind(actor.user.id)
        .bind(&actor.token_hash)
        .fetch_one(&h.owner)
        .await
        .unwrap()
}
async fn fixture(h: &Harness, user: Uuid, workspace: Option<Uuid>) -> store::Tx {
    let mut tx = store::begin(&h.owner).await.unwrap();
    sqlx::query("SELECT set_config('opentrack.user_id',$1,true),set_config('opentrack.workspace_id',$2,true)").bind(user.to_string()).bind(workspace.map(|v|v.to_string()).unwrap_or_default()).execute(&mut *tx).await.unwrap();
    tx
}
async fn snapshot(h: &Harness, user: Uuid) -> Value {
    let mut tx = fixture(h, user, None).await;
    let s=sqlx::query_scalar("SELECT jsonb_build_object('requests',(SELECT coalesce(jsonb_agg(to_jsonb(r) ORDER BY id),'[]') FROM native_auth_requests r),'sessions',(SELECT coalesce(jsonb_agg(to_jsonb(s) ORDER BY id),'[]') FROM sessions s),'audit',(SELECT coalesce(jsonb_agg(to_jsonb(a) ORDER BY id),'[]') FROM account_security_events a))").fetch_one(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    s
}
async fn person(h: &Harness, owner: &Actor, w: Uuid) -> Actor {
    let id = Uuid::new_v4();
    let email = format!("{id}@native.invalid");
    let mut tx = h.context(owner.user.id, w).await;
    sqlx::query("INSERT INTO users(id,email,display_name,password_hash) SELECT $1,$2,'Native member',password_hash FROM users WHERE id=$3").bind(id).bind(&email).bind(owner.user.id).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO workspace_members(workspace_id,user_id,role) VALUES($1,$2,'member')")
        .bind(w)
        .bind(id)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    login(h, &email).await.0
}
async fn suppress(h: &Harness, table: &str, operation: &str, field: &str, value: &str) {
    assert!(matches!(
        table,
        "native_auth_requests" | "sessions" | "account_security_events"
    ));
    assert!(matches!(operation, "INSERT" | "UPDATE" | "DELETE"));
    assert!(matches!(field, "id" | "action" | "audience"));
    assert!(
        value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_'))
    );
    sqlx::query(AssertSqlSafe(format!("CREATE TRIGGER native_suppress BEFORE {operation} ON {table} FOR EACH ROW EXECUTE FUNCTION native_suppress('{field}','{value}')"))).execute(&h.owner).await.unwrap();
}
async fn clear(h: &Harness, table: &str) {
    sqlx::query(AssertSqlSafe(format!(
        "DROP TRIGGER native_suppress ON {table}"
    )))
    .execute(&h.owner)
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_postgres_native_authentication_contract() {
    let h = Harness::new().await;
    let runner = h.clone();
    let result = tokio::spawn(async move { exercise(&runner).await }).await;
    h.cleanup().await;
    if let Err(e) = result {
        std::panic::resume_unwind(e.into_panic());
    }
}
async fn exercise(h: &Harness) {
    let (browser, cookie, w) = setup(h).await;
    let other = person(h, &browser, w).await;
    let webid = public_id(h, &browser).await;
    sqlx::query("UPDATE sessions SET auth_time=clock_timestamp()-interval '9 minutes',expires_at=clock_timestamp()+interval '1 hour' WHERE id=$1").bind(webid).execute(&h.owner).await.unwrap();
    let times: (chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>) =
        sqlx::query_as("SELECT auth_time,expires_at FROM sessions WHERE id=$1")
            .bind(webid)
            .fetch_one(&h.owner)
            .await
            .unwrap();
    let s = start(h, None).await;
    assert!(
        s.authorization_path
            .starts_with("/auth/native/approve?request_id=")
    );
    assert!(matches!(h.app.start_native_auth(Some(&browser),req(json!({"code_challenge":native_code_challenge(VERIFIER).unwrap(),"callback_uri":"http://127.0.0.1:48523/opentrack/native-callback","state":"a".repeat(64),"device_name":"Device"}))).await,Err(Error::Forbidden)));
    let consent = h.app.native_auth_consent(&browser, s.id).await.unwrap();
    assert!(!consent.reauthentication);
    assert_eq!(consent.confirmation_code, s.confirmation_code);
    assert!(matches!(
        h.app
            .approve_native_auth(&browser, s.id, req(json!({"confirmation_code":format!("{}{}",if s.confirmation_code.starts_with('A'){'B'}else{'A'},&s.confirmation_code[1..])})))
            .await,
        Err(Error::InvalidNativeAuth)
    ));
    let e = approve(h, &browser, &s).await;
    assert!(matches!(
        h.app.native_auth_consent(&browser, s.id).await,
        Err(Error::NotFound)
    ));
    assert!(matches!(
        h.app
            .approve_native_auth(
                &browser,
                s.id,
                req(json!({"confirmation_code":s.confirmation_code}))
            )
            .await,
        Err(Error::InvalidNativeAuth)
    ));
    let stored: String =
        sqlx::query_scalar("SELECT code_hash FROM native_auth_requests WHERE id=$1")
            .bind(s.id)
            .fetch_one(&h.owner)
            .await
            .unwrap();
    assert_ne!(stored, e.code);
    let mut wrong = copy_exchange(&e);
    wrong.code_verifier = "A".repeat(43);
    assert!(matches!(
        h.app.exchange_native_auth(wrong).await,
        Err(Error::InvalidNativeAuth)
    ));
    let (a, b) = tokio::join!(
        h.app.exchange_native_auth(copy_exchange(&e)),
        h.app.exchange_native_auth(copy_exchange(&e))
    );
    let g = match (a, b) {
        (Ok(g), Err(Error::InvalidNativeAuth)) | (Err(Error::InvalidNativeAuth), Ok(g)) => g,
        other => panic!("Single-use exchange result: {other:?}"),
    };
    assert_eq!(g.session.audience, SessionAudience::Desktop);
    assert!(g.session.csrf_token.is_empty());
    assert_eq!(g.token_type, "Bearer");
    assert_eq!(g.session.user.id, browser.user.id);
    assert_eq!(g.expires_at, times.1);
    let native = h.app.authenticate_native(&g.access_token).await.unwrap();
    assert_eq!(native.audience, SessionAudience::Desktop);
    assert!(matches!(
        h.app.authenticate(&g.access_token).await,
        Err(Error::Unauthorized)
    ));
    assert!(matches!(
        h.app.authenticate_native(&cookie).await,
        Err(Error::Unauthorized)
    ));
    assert!(h.app.authenticate(&cookie).await.is_ok());
    let native_times: (chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>) =
        sqlx::query_as("SELECT auth_time,expires_at FROM sessions WHERE id=$1")
            .bind(g.session_id)
            .fetch_one(&h.owner)
            .await
            .unwrap();
    assert_eq!(native_times, times);
    let mut forged = native.clone();
    forged.audience = SessionAudience::Web;
    assert!(matches!(
        h.app.session(&forged).await,
        Err(Error::Unauthorized)
    ));
    assert!(matches!(
        h.app.native_auth_consent(&native, Uuid::new_v4()).await,
        Err(Error::Forbidden)
    ));
    assert!(matches!(
        h.app
            .accept_invitation(Some(&native), req(json!({"token":"invalid"})))
            .await,
        Err(Error::Forbidden)
    ));
    let directory = h
        .app
        .sessions(&native, SessionQuery::default())
        .await
        .unwrap();
    let n = directory
        .data
        .iter()
        .find(|n| n.id == g.session_id)
        .unwrap();
    assert!(n.current);
    assert_eq!(n.device_name.as_deref(), Some("Contract desktop"));
    assert_eq!(n.audience, SessionAudience::Desktop);
    assert!(
        directory
            .data
            .iter()
            .filter(|s| s.audience == SessionAudience::Web)
            .all(|s| s.device_name.is_none())
    );
    let readonly = serde_json::to_string(&directory).unwrap();
    assert!(!readonly.contains(&g.access_token));
    assert!(!format!("{g:?} {native:?}").contains(&g.access_token));
    let before = h
        .app
        .workspace_access(&browser, w)
        .await
        .unwrap()
        .auth_epoch;
    let bound = start(h, Some(&native)).await;
    assert!(
        h.app
            .native_auth_consent(&browser, bound.id)
            .await
            .unwrap()
            .reauthentication
    );
    assert!(matches!(
        h.app.native_auth_consent(&other, bound.id).await,
        Err(Error::NotFound)
    ));
    assert!(matches!(
        h.app
            .approve_native_auth(
                &other,
                bound.id,
                req(json!({"confirmation_code":bound.confirmation_code}))
            )
            .await,
        Err(Error::InvalidNativeAuth)
    ));
    let rotation = approve(h, &browser, &bound).await;
    let rotated = h.app.exchange_native_auth(rotation).await.unwrap();
    assert!(matches!(
        h.app.authenticate_native(&g.access_token).await,
        Err(Error::Unauthorized)
    ));
    assert!(h.app.authenticate(&cookie).await.is_ok());
    assert_eq!(
        h.app
            .workspace_access(&browser, w)
            .await
            .unwrap()
            .auth_epoch,
        before
    );
    let mut tx = fixture(h, browser.user.id, None).await;
    let actions: Vec<String> = sqlx::query_scalar(
        "SELECT action FROM account_security_events WHERE correlation_id=$1 ORDER BY action",
    )
    .bind(bound.id)
    .fetch_all(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(
        actions,
        vec![
            "session.created",
            "session.native_authorized",
            "session.native_exchanged",
            "session.native_rotated"
        ]
    );
    sqlx::query(
        "UPDATE sessions SET auth_time=clock_timestamp()-interval '11 minutes' WHERE id=$1",
    )
    .bind(webid)
    .execute(&h.owner)
    .await
    .unwrap();
    let stale = start(h, None).await;
    assert!(matches!(
        h.app
            .approve_native_auth(
                &browser,
                stale.id,
                req(json!({"confirmation_code":stale.confirmation_code}))
            )
            .await,
        Err(Error::ReauthenticationRequired)
    ));
    sqlx::query("UPDATE sessions SET auth_time=clock_timestamp() WHERE id=$1")
        .bind(webid)
        .execute(&h.owner)
        .await
        .unwrap();
    assert!(
        sqlx::query("UPDATE sessions SET audience='web',device_name=NULL WHERE id=$1")
            .bind(rotated.session_id)
            .execute(&h.owner)
            .await
            .is_err()
    );
    atomicity(h, &browser, &rotated).await;
    revoked_and_expired(h, &browser).await;
    expiry_crossing(h, &browser).await;
    lifecycle(h, &browser, w).await;
    revocation_races(h).await;
    admission_bounds(h, &browser).await;
}

async fn atomicity(h: &Harness, browser: &Actor, previous: &NativeSessionGrant) {
    sqlx::raw_sql("CREATE FUNCTION native_suppress() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF (CASE WHEN TG_OP='DELETE' THEN to_jsonb(OLD) ELSE to_jsonb(NEW) END)->>TG_ARGV[0]=TG_ARGV[1] THEN RETURN NULL; END IF; IF TG_OP='DELETE' THEN RETURN OLD; END IF; RETURN NEW; END $$").execute(&h.owner).await.unwrap();
    let s = start(h, None).await;
    for (table, field, value) in [
        ("native_auth_requests", "id", s.id.to_string()),
        (
            "account_security_events",
            "action",
            "session.native_authorized".into(),
        ),
    ] {
        suppress(
            h,
            table,
            if table == "native_auth_requests" {
                "UPDATE"
            } else {
                "INSERT"
            },
            field,
            &value,
        )
        .await;
        let before = snapshot(h, browser.user.id).await;
        assert!(
            h.app
                .approve_native_auth(
                    browser,
                    s.id,
                    req(json!({"confirmation_code":s.confirmation_code}))
                )
                .await
                .is_err()
        );
        assert_eq!(
            snapshot(h, browser.user.id).await,
            before,
            "Failed approval rolls back all durable state"
        );
        clear(h, table).await;
    }
    let e = approve(h, browser, &s).await;
    for (table, op, field, value) in [
        ("sessions", "INSERT", "audience", "desktop".into()),
        ("native_auth_requests", "UPDATE", "id", s.id.to_string()),
        (
            "account_security_events",
            "INSERT",
            "action",
            "session.created".into(),
        ),
        (
            "account_security_events",
            "INSERT",
            "action",
            "session.native_exchanged".into(),
        ),
    ] {
        suppress(h, table, op, field, &value).await;
        let before = snapshot(h, browser.user.id).await;
        assert!(h.app.exchange_native_auth(copy_exchange(&e)).await.is_err());
        assert_eq!(
            snapshot(h, browser.user.id).await,
            before,
            "Failed exchange rolls back and leaves its code unconsumed"
        );
        clear(h, table).await;
    }
    h.app.exchange_native_auth(e).await.unwrap();
    let actor = h
        .app
        .authenticate_native(&previous.access_token)
        .await
        .unwrap();
    let s = start(h, Some(&actor)).await;
    let e = approve(h, browser, &s).await;
    for (table, op, field, value) in [
        ("sessions", "UPDATE", "id", previous.session_id.to_string()),
        (
            "account_security_events",
            "INSERT",
            "action",
            "session.native_rotated".into(),
        ),
    ] {
        suppress(h, table, op, field, &value).await;
        let before = snapshot(h, browser.user.id).await;
        assert!(h.app.exchange_native_auth(copy_exchange(&e)).await.is_err());
        assert_eq!(snapshot(h, browser.user.id).await, before);
        clear(h, table).await;
    }
    h.app.exchange_native_auth(e).await.unwrap();
}
async fn revoked_and_expired(h: &Harness, browser: &Actor) {
    // Requests are immutable; the migration owner uses an explicit fixture-only
    // trigger pause to model clocks already near expiry without production hooks.
    let s = start(h, None).await;
    let e = approve(h, browser, &s).await;
    sqlx::raw_sql("ALTER TABLE native_auth_requests DISABLE TRIGGER native_auth_request_guard")
        .execute(&h.owner)
        .await
        .unwrap();
    sqlx::query("UPDATE native_auth_requests SET created_at=created_at-interval '20 minutes',expires_at=expires_at-interval '20 minutes',approved_at=approved_at-interval '20 minutes',code_expires_at=code_expires_at-interval '20 minutes' WHERE id=$1").bind(s.id).execute(&h.owner).await.unwrap();
    sqlx::raw_sql("ALTER TABLE native_auth_requests ENABLE TRIGGER native_auth_request_guard")
        .execute(&h.owner)
        .await
        .unwrap();
    assert!(matches!(
        h.app.exchange_native_auth(e).await,
        Err(Error::InvalidNativeAuth)
    ));
    let g = grant(h, browser, None).await;
    let native = h.app.authenticate_native(&g.access_token).await.unwrap();
    let s = start(h, Some(&native)).await;
    let e = approve(h, browser, &s).await;
    assert!(!h.app.revoke_session(browser, g.session_id).await.unwrap());
    assert!(matches!(
        h.app.exchange_native_auth(e).await,
        Err(Error::InvalidNativeAuth)
    ));
    assert!(matches!(
        h.app.session(&native).await,
        Err(Error::Unauthorized)
    ));
}
async fn lifecycle(h: &Harness, owner: &Actor, w: Uuid) {
    let operator = opentrack_app::Operator::connect(&h.operator_url().await)
        .await
        .unwrap();
    let disabled = person(h, owner, w).await;
    let g = grant(h, &disabled, None).await;
    operator
        .disable_account(disabled.user.id, "Native contract disable")
        .await
        .unwrap();
    assert!(matches!(
        h.app.authenticate_native(&g.access_token).await,
        Err(Error::Unauthorized)
    ));
    operator
        .enable_account(disabled.user.id, "Native contract reenable")
        .await
        .unwrap();
    assert!(matches!(
        h.app.authenticate_native(&g.access_token).await,
        Err(Error::Unauthorized)
    ));
    let recovered = person(h, owner, w).await;
    let g = grant(h, &recovered, None).await;
    let code = opentrack_app::generate_recovery_code(recovered.user.id);
    operator
        .issue_recovery(
            recovered.user.id,
            Uuid::new_v4(),
            &code,
            30,
            "Native contract recovery",
        )
        .await
        .unwrap();
    h.app
        .complete_recovery(req(
            json!({"token":code,"password":"Native replacement password 862!"}),
        ))
        .await
        .unwrap();
    assert!(matches!(
        h.app.authenticate_native(&g.access_token).await,
        Err(Error::Unauthorized)
    ));
    let elevated = person(h, owner, w).await;
    let g = grant(h, &elevated, None).await;
    let member = h
        .app
        .get_workspace_member(owner, w, elevated.user.id)
        .await
        .unwrap();
    h.app
        .update_member(
            owner,
            w,
            elevated.user.id,
            &member.etag(w),
            req(json!({"role":"admin"})),
        )
        .await
        .unwrap();
    assert!(matches!(
        h.app.authenticate_native(&g.access_token).await,
        Err(Error::Unauthorized)
    ));
    drop(operator);
}
async fn blocked_by(h: &Harness, blocker: i32) -> i32 {
    tokio::time::timeout(std::time::Duration::from_millis(800), async {
        loop {
            let pid: Option<i32> = sqlx::query_scalar(
                "SELECT pid FROM pg_stat_activity WHERE $1=ANY(pg_blocking_pids(pid)) LIMIT 1",
            )
            .bind(blocker)
            .fetch_optional(&h.owner)
            .await
            .unwrap();
            if let Some(pid) = pid {
                return pid;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("Expected native operation reached an observed PostgreSQL lock wait")
}
async fn revocation_races(h: &Harness) {
    let (controller, _) = login(h, EMAIL).await;
    let (source, _) = login(h, EMAIL).await;
    let s = start(h, None).await;
    let e = approve(h, &source, &s).await;
    let id = public_id(h, &source).await;
    let mut lock = fixture(h, source.user.id, None).await;
    let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *lock)
        .await
        .unwrap();
    sqlx::query("SELECT id FROM sessions WHERE id=$1 FOR UPDATE")
        .bind(id)
        .fetch_one(&mut *lock)
        .await
        .unwrap();
    let app = h.app.clone();
    let actor = controller.clone();
    let revoke = tokio::spawn(async move { app.revoke_session(&actor, id).await });
    let revoker = blocked_by(h, blocker).await;
    let app = h.app.clone();
    let exchange = tokio::spawn(async move { app.exchange_native_auth(e).await });
    blocked_by(h, revoker).await;
    lock.commit().await.unwrap();
    assert!(!revoke.await.unwrap().unwrap());
    assert!(matches!(
        exchange.await.unwrap(),
        Err(Error::InvalidNativeAuth)
    ));
    let (source, _) = login(h, EMAIL).await;
    let s = start(h, None).await;
    let e = approve(h, &source, &s).await;
    let mut lock = fixture(h, source.user.id, None).await;
    let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *lock)
        .await
        .unwrap();
    sqlx::query("SELECT id FROM native_auth_requests WHERE id=$1 FOR UPDATE")
        .bind(s.id)
        .fetch_one(&mut *lock)
        .await
        .unwrap();
    let app = h.app.clone();
    let exchange = tokio::spawn(async move { app.exchange_native_auth(e).await });
    let exchanger = blocked_by(h, blocker).await;
    let app = h.app.clone();
    let revoke = tokio::spawn(async move { app.revoke_other_sessions(&controller).await });
    blocked_by(h, exchanger).await;
    lock.commit().await.unwrap();
    let grant = exchange.await.unwrap().unwrap();
    revoke.await.unwrap().unwrap();
    assert!(matches!(
        h.app.authenticate_native(&grant.access_token).await,
        Err(Error::Unauthorized)
    ));
}
async fn admission_bounds(h: &Harness, browser: &Actor) {
    // Clear only expired rows through production cleanup; pending rows remain
    // immutable until their lifetime passes. Owner fixtures establish cap/age.
    sqlx::raw_sql("ALTER TABLE native_auth_requests DISABLE TRIGGER native_auth_request_guard")
        .execute(&h.owner)
        .await
        .unwrap();
    sqlx::query("DELETE FROM native_auth_requests")
        .execute(&h.owner)
        .await
        .unwrap();
    sqlx::raw_sql("ALTER TABLE native_auth_requests ENABLE TRIGGER native_auth_request_guard")
        .execute(&h.owner)
        .await
        .unwrap();
    sqlx::query("WITH at AS (SELECT clock_timestamp() AS now) INSERT INTO native_auth_requests(id,code_challenge,callback_uri,state,confirmation_code,device_name,created_at,expires_at) SELECT gen_random_uuid(),$1,'http://127.0.0.1:48523/opentrack/native-callback',$2,'ABCDEFGH','Bounded fixture',at.now,at.now+interval '10 minutes' FROM generate_series(1,2000),at")
        .bind(native_code_challenge(VERIFIER).unwrap()).bind("a".repeat(64)).execute(&h.owner).await.unwrap();
    let input = || {
        req(
            json!({"code_challenge":native_code_challenge(VERIFIER).unwrap(),"callback_uri":"http://127.0.0.1:48523/opentrack/native-callback","state":"a".repeat(64),"device_name":"Capacity"}),
        )
    };
    assert!(matches!(
        h.app.start_native_auth(None, input()).await,
        Err(Error::RateLimited)
    ));
    sqlx::raw_sql("ALTER TABLE native_auth_requests DISABLE TRIGGER native_auth_request_guard")
        .execute(&h.owner)
        .await
        .unwrap();
    sqlx::query("UPDATE native_auth_requests SET created_at=created_at-interval '20 minutes',expires_at=expires_at-interval '20 minutes'").execute(&h.owner).await.unwrap();
    sqlx::raw_sql("ALTER TABLE native_auth_requests ENABLE TRIGGER native_auth_request_guard")
        .execute(&h.owner)
        .await
        .unwrap();
    start(h, None).await;
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM native_auth_requests")
        .fetch_one(&h.owner)
        .await
        .unwrap();
    assert_eq!(
        count, 1901,
        "Each admission reaps no more than100 expired rows"
    );
    let id:Uuid=sqlx::query_scalar("SELECT id FROM native_auth_requests WHERE expires_at<=clock_timestamp() ORDER BY expires_at,id LIMIT 1").fetch_one(&h.owner).await.unwrap();
    suppress(h, "native_auth_requests", "DELETE", "id", &id.to_string()).await;
    let before = snapshot(h, browser.user.id).await;
    assert!(matches!(
        h.app.start_native_auth(None, input()).await,
        Err(Error::Internal)
    ));
    assert_eq!(snapshot(h, browser.user.id).await, before);
    clear(h, "native_auth_requests").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_postgres_native_session_upgrade() {
    let h = Harness::with_version(5, 12).await;
    let runner = h.clone();
    let result=tokio::spawn(async move {
        let h=&runner;
        // Setup persists the original web-only session columns before0013.
        let (s,c)=h.app.setup(req(json!({"setup_token":h.setup_token,"email":EMAIL,"password":PASSWORD,"display_name":"Legacy browser","workspace_name":"Legacy","team_name":"Legacy","team_key":"OLD"}))).await.unwrap();
        let before:Value=sqlx::query_scalar("SELECT to_jsonb(s) FROM sessions s").fetch_one(&h.owner).await.unwrap();
        assert!(before.get("audience").is_none());
        let worker_url=h.worker_url().await;
        let worker=opentrack_app::worker::Worker::connect(&worker_url,opentrack_app::worker::WorkerOptions::default()).await.unwrap();
        let operator_url=h.operator_url().await;let operator_role=Url::parse(&operator_url).unwrap().username().to_string();let operator=opentrack_app::Operator::connect(&operator_url).await.unwrap();
        store::MIGRATOR.run(&h.owner).await.unwrap();
        let after:Value=sqlx::query_scalar("SELECT to_jsonb(s) FROM sessions s").fetch_one(&h.owner).await.unwrap();
        let mut expected=before;expected["audience"]=json!("web");expected["device_name"]=Value::Null;assert_eq!(after,expected);
        let actor=h.app.authenticate(&c).await.unwrap();assert_eq!(actor.user.id,s.user.id);assert_eq!(actor.audience,SessionAudience::Web);
        assert!(matches!(h.app.authenticate_native(&c).await,Err(Error::Unauthorized)));
        for table in ["native_auth_requests","native_auth_admission"] {
            let allowed:bool=sqlx::query_scalar("SELECT bool_and(has_table_privilege(current_user,format('%I.%I',current_schema(),$1),p)) FROM unnest(ARRAY['SELECT','INSERT','UPDATE','DELETE']) p").bind(table).fetch_one(&h.app.pool).await.unwrap();assert!(allowed);
            let denied:bool=sqlx::query_scalar("SELECT NOT has_any_column_privilege(current_user,format('%I.%I',current_schema(),$1),'SELECT,INSERT,UPDATE,REFERENCES') AND NOT has_table_privilege(current_user,format('%I.%I',current_schema(),$1),'DELETE,TRUNCATE,TRIGGER')").bind(table).fetch_one(&worker.pool).await.unwrap();assert!(denied);
            let denied_operator:bool=sqlx::query_scalar("SELECT NOT has_any_column_privilege($1,format('%I.%I',current_schema(),$2),'SELECT,INSERT,UPDATE,REFERENCES') AND NOT has_table_privilege($1,format('%I.%I',current_schema(),$2),'DELETE,TRUNCATE,TRIGGER')").bind(&operator_role).bind(table).fetch_one(&h.owner).await.unwrap();assert!(denied_operator);
        }
        assert!(h.app.revoke_session(&actor,public_id(h,&actor).await).await.unwrap());
        assert!(matches!(h.app.authenticate(&c).await,Err(Error::Unauthorized)));
        drop(operator);worker.pool.close().await;
    }).await;
    h.cleanup().await;
    if let Err(e) = result {
        std::panic::resume_unwind(e.into_panic());
    }
}

async fn expiry_crossing(h: &Harness, browser: &Actor) {
    let s = start(h, None).await;
    let e = approve(h, browser, &s).await;
    let mut lock = fixture(h, browser.user.id, None).await;
    let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *lock)
        .await
        .unwrap();
    sqlx::query("SELECT id FROM users WHERE id=$1 FOR SHARE")
        .bind(browser.user.id)
        .fetch_one(&mut *lock)
        .await
        .unwrap();
    let app = h.app.clone();
    let exchange = tokio::spawn(async move { app.exchange_native_auth(e).await });
    blocked_by(h, blocker).await;
    // The initial lookup saw a valid grant; the guarded re-read must see expiry.
    sqlx::raw_sql("ALTER TABLE native_auth_requests DISABLE TRIGGER native_auth_request_guard")
        .execute(&mut *lock)
        .await
        .unwrap();
    sqlx::query("UPDATE native_auth_requests SET created_at=created_at-interval '20 minutes',expires_at=expires_at-interval '20 minutes',approved_at=approved_at-interval '20 minutes',code_expires_at=code_expires_at-interval '20 minutes' WHERE id=$1").bind(s.id).execute(&mut *lock).await.unwrap();
    sqlx::raw_sql("ALTER TABLE native_auth_requests ENABLE TRIGGER native_auth_request_guard")
        .execute(&mut *lock)
        .await
        .unwrap();
    lock.commit().await.unwrap();
    assert!(matches!(
        exchange.await.unwrap(),
        Err(Error::InvalidNativeAuth)
    ));
    let s = start(h, None).await;
    let e = approve(h, browser, &s).await;
    // Gate the session INSERT after grant validation. Expiry then crosses before
    // the final consume, which must roll back the inserted session and its audit.
    let mut lock = fixture(h, browser.user.id, None).await;
    sqlx::raw_sql("ALTER TABLE native_auth_requests DISABLE TRIGGER native_auth_request_guard")
        .execute(&mut *lock)
        .await
        .unwrap();
    sqlx::query("UPDATE native_auth_requests SET code_expires_at=clock_timestamp()+interval '350 milliseconds' WHERE id=$1").bind(s.id).execute(&mut *lock).await.unwrap();
    sqlx::raw_sql("ALTER TABLE native_auth_requests ENABLE TRIGGER native_auth_request_guard")
        .execute(&mut *lock)
        .await
        .unwrap();
    lock.commit().await.unwrap();
    sqlx::raw_sql("CREATE FUNCTION native_expiry_gate() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.audience='desktop' THEN PERFORM pg_advisory_xact_lock(18773627); END IF; RETURN NEW; END $$; CREATE TRIGGER native_expiry_gate BEFORE INSERT ON sessions FOR EACH ROW EXECUTE FUNCTION native_expiry_gate()").execute(&h.owner).await.unwrap();
    let before = snapshot(h, browser.user.id).await;
    let mut lock = fixture(h, browser.user.id, None).await;
    let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *lock)
        .await
        .unwrap();
    sqlx::query("SELECT pg_advisory_xact_lock(18773627)")
        .execute(&mut *lock)
        .await
        .unwrap();
    let app = h.app.clone();
    let exchange = tokio::spawn(async move { app.exchange_native_auth(e).await });
    blocked_by(h, blocker).await;
    loop {
        let expired: bool = sqlx::query_scalar(
            "SELECT clock_timestamp()>=code_expires_at FROM native_auth_requests WHERE id=$1",
        )
        .bind(s.id)
        .fetch_one(&mut *lock)
        .await
        .unwrap();
        if expired {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    lock.commit().await.unwrap();
    assert!(matches!(
        exchange.await.unwrap(),
        Err(Error::InvalidNativeAuth)
    ));
    assert_eq!(snapshot(h, browser.user.id).await, before);
    sqlx::raw_sql(
        "DROP TRIGGER native_expiry_gate ON sessions; DROP FUNCTION native_expiry_gate()",
    )
    .execute(&h.owner)
    .await
    .unwrap();
}
