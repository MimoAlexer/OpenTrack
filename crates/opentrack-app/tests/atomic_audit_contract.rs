//! Silent trigger suppression cannot commit unaudited credentials or issue work.
#[allow(dead_code)]
mod support;

use opentrack_app::{Operator, generate_recovery_code};
use opentrack_domain::*;
use opentrack_store as store;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use sqlx::AssertSqlSafe;
use support::Harness;
use uuid::Uuid;

const EMAIL: &str = "atomic@audit.invalid";
const PASSWORD: &str = "atomic audit original password 519!";
const NEW_PASSWORD: &str = "atomic audit replacement password 826!";
const REASON: &str = "Verified atomic audit contract";
fn req<T: DeserializeOwned>(value: Value) -> T {
    serde_json::from_value(value).unwrap()
}
fn setup(h: &Harness) -> Setup {
    req(
        json!({"setup_token":h.setup_token,"email":EMAIL,"password":PASSWORD,"display_name":"Atomic audit owner","workspace_name":"Atomic audit workspace","team_name":"Engineering","team_key":"ENG"}),
    )
}
async fn fixture(h: &Harness, user: Uuid, w: Uuid) -> store::Tx {
    let mut tx = store::begin(&h.owner).await.unwrap();
    sqlx::query("SELECT set_config('opentrack.user_id',$1,true),set_config('opentrack.workspace_id',$2,true)").bind(user.to_string()).bind(w.to_string()).execute(&mut *tx).await.unwrap();
    tx
}
async fn login(h: &Harness, password: &str) -> (Actor, String) {
    let (_, cookie) = h
        .app
        .login(req(json!({"email":EMAIL,"password":password})))
        .await
        .unwrap();
    (h.app.authenticate(&cookie).await.unwrap(), cookie)
}
async fn account_state(h: &Harness, user: Uuid) -> Value {
    let mut tx = fixture(h, user, Uuid::nil()).await;
    let state:Value=sqlx::query_scalar("SELECT jsonb_build_object('user',(SELECT to_jsonb(u) FROM users u WHERE id=$1),'sessions',(SELECT coalesce(jsonb_agg(to_jsonb(s) ORDER BY s.id),'[]') FROM sessions s WHERE user_id=$1),'recovery',(SELECT coalesce(jsonb_agg(to_jsonb(r) ORDER BY r.id),'[]') FROM recovery_tokens r WHERE user_id=$1),'audit',(SELECT coalesce(jsonb_agg(to_jsonb(a) ORDER BY a.id),'[]') FROM account_security_events a WHERE user_id=$1))").bind(user).fetch_one(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    state
}
async fn workspace_state(h: &Harness, user: Uuid, w: Uuid) -> Value {
    let mut tx = fixture(h, user, w).await;
    let state:Value=sqlx::query_scalar("SELECT jsonb_build_object('issues',(SELECT coalesce(jsonb_agg(to_jsonb(i) ORDER BY i.id),'[]') FROM issues i WHERE workspace_id=$1),'teams',(SELECT coalesce(jsonb_agg(jsonb_build_array(id,next_issue_number) ORDER BY id),'[]') FROM teams WHERE workspace_id=$1),'retries',(SELECT coalesce(jsonb_agg(to_jsonb(r) ORDER BY r.principal_id,r.route,r.key_hash),'[]') FROM idempotency_records r WHERE workspace_id=$1),'activity',(SELECT coalesce(jsonb_agg(to_jsonb(a) ORDER BY a.id),'[]') FROM activity_entries a WHERE workspace_id=$1),'events',(SELECT coalesce(jsonb_agg(to_jsonb(e) ORDER BY e.seq),'[]') FROM workspace_events e WHERE workspace_id=$1),'head',(SELECT last_seq FROM workspace_stream_heads WHERE workspace_id=$1),'audit',(SELECT coalesce(jsonb_agg(to_jsonb(a) ORDER BY a.id),'[]') FROM audit_entries a WHERE workspace_id=$1),'epoch',(SELECT epoch FROM workspace_auth_guards WHERE workspace_id=$1))").bind(w).fetch_one(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    state
}
async fn suppress(h: &Harness, table: &str, operation: &str, field: &str, value: &str) {
    assert!(matches!(
        table,
        "audit_entries"
            | "account_security_events"
            | "activity_entries"
            | "workspace_events"
            | "workspace_stream_heads"
            | "sessions"
            | "users"
            | "recovery_tokens"
            | "invitations"
    ));
    assert!(matches!(operation, "INSERT" | "UPDATE"));
    assert!(matches!(
        field,
        "action" | "event_kind" | "event_type" | "id" | "workspace_id"
    ));
    assert!(
        value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
    );
    // Identifiers/actions are fixed test literals or generated UUIDs; no external
    // input enters this fixture-only SQL. Returning NULL is a successful trigger
    // outcome, so a database error alone cannot protect the production command.
    sqlx::query(AssertSqlSafe(format!("CREATE TRIGGER test_silent_suppression BEFORE {operation} ON {table} FOR EACH ROW EXECUTE FUNCTION test_silent_suppression('{field}','{value}')"))).execute(&h.owner).await.unwrap();
}
async fn restore(h: &Harness, table: &str) {
    sqlx::query(AssertSqlSafe(format!(
        "DROP TRIGGER test_silent_suppression ON {table}"
    )))
    .execute(&h.owner)
    .await
    .unwrap();
}
async fn assert_live(h: &Harness, cookies: &[&str]) {
    for cookie in cookies {
        assert!(h.app.authenticate(cookie).await.is_ok());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_postgres_mandatory_audit_atomicity_contracts() {
    let h = Harness::new().await;
    let runner = h.clone();
    let result = tokio::spawn(async move { exercise(&runner).await }).await;
    h.cleanup().await;
    if let Err(error) = result {
        std::panic::resume_unwind(error.into_panic());
    }
}
async fn exercise(h: &Harness) {
    sqlx::raw_sql("CREATE FUNCTION test_silent_suppression() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF to_jsonb(NEW)->>TG_ARGV[0]=TG_ARGV[1] THEN RETURN NULL; END IF; RETURN NEW; END $$").execute(&h.owner).await.unwrap();
    setup_rollback(h).await;
    let (session, cookie) = h.app.setup(setup(h)).await.unwrap();
    let actor = h.app.authenticate(&cookie).await.unwrap();
    let w = session.workspaces[0].id;
    let team = h.app.bootstrap(&actor, w).await.unwrap().teams[0].id;
    assert!(!h.app.setup_required().await.unwrap());
    assert!(matches!(
        h.app.setup(setup(h)).await,
        Err(Error::Conflict(_))
    ));
    bulk_session_rollback(h, &actor, &cookie).await;
    issue_rollback(h, &actor, w, team).await;
    let operator = Operator::connect(&h.operator_url().await).await.unwrap();
    lifecycle_write_rollback(h, &operator, &actor, w).await;
    let code = operator_rollback(h, &operator, &actor).await;
    recovery_rollback(h, &actor, &cookie, &code).await;
}

async fn setup_rollback(h: &Harness) {
    for (table, action) in [
        ("audit_entries", "instance.setup"),
        ("account_security_events", "session.created"),
    ] {
        suppress(h, table, "INSERT", "action", action).await;
        assert!(matches!(h.app.setup(setup(h)).await, Err(Error::Internal)));
        assert!(
            h.app.setup_required().await.unwrap(),
            "A silently missing setup audit cannot consume the one-time setup gate"
        );
        let rows:(i64,i64,i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM users),(SELECT count(*) FROM organizations),(SELECT count(*) FROM workspaces),(SELECT count(*) FROM sessions)").fetch_one(&h.owner).await.unwrap();
        assert_eq!(
            rows,
            (0, 0, 0, 0),
            "No credentials or ownership tree may survive failed setup"
        );
        restore(h, table).await;
    }
}

async fn bulk_session_rollback(h: &Harness, actor: &Actor, cookie: &str) {
    let (other_a, a) = login(h, PASSWORD).await;
    let (_, b) = login(h, PASSWORD).await;
    let mut tx = fixture(h, actor.user.id, Uuid::nil()).await;
    let selected: Uuid = sqlx::query_scalar("SELECT id FROM sessions WHERE token_hash=$1")
        .bind(&other_a.token_hash)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    for (table, operation, field, value) in [
        (
            "account_security_events",
            "INSERT",
            "action",
            "session.revoked".to_owned(),
        ),
        ("sessions", "UPDATE", "id", selected.to_string()),
    ] {
        let before = account_state(h, actor.user.id).await;
        suppress(h, table, operation, field, &value).await;
        assert!(matches!(
            h.app.revoke_other_sessions(actor).await,
            Err(Error::Internal)
        ));
        assert_eq!(
            account_state(h, actor.user.id).await,
            before,
            "Bulk revocation must compare eligible, changed and audited counts"
        );
        restore(h, table).await;
        assert_live(h, &[cookie, &a, &b]).await;
    }
    let before = account_state(h, actor.user.id).await;
    h.app.revoke_other_sessions(actor).await.unwrap();
    assert!(h.app.authenticate(cookie).await.is_ok());
    for old in [&a, &b] {
        assert!(matches!(
            h.app.authenticate(old).await,
            Err(Error::Unauthorized)
        ));
    }
    let after = account_state(h, actor.user.id).await;
    assert_eq!(
        after["audit"].as_array().unwrap().len(),
        before["audit"].as_array().unwrap().len() + 2
    );
    let before = account_state(h, actor.user.id).await;
    h.app.revoke_other_sessions(actor).await.unwrap();
    assert_eq!(
        account_state(h, actor.user.id).await,
        before,
        "An audited zero-work retry must not invent revocations"
    );
}

async fn issue_rollback(h: &Harness, actor: &Actor, w: Uuid, team: Uuid) {
    for (table, field, action) in [
        ("activity_entries", "event_kind", "issue.created".to_owned()),
        ("workspace_events", "event_type", "issue.created".to_owned()),
        ("workspace_stream_heads", "workspace_id", w.to_string()),
    ] {
        let key = Uuid::new_v4().to_string();
        let before = workspace_state(h, actor.user.id, w).await;
        suppress(
            h,
            table,
            if table == "workspace_stream_heads" {
                "UPDATE"
            } else {
                "INSERT"
            },
            field,
            &action,
        )
        .await;
        assert!(matches!(
            h.app
                .create_issue(
                    actor,
                    w,
                    &key,
                    req(json!({"team_id":team,"title":"Audited create"}))
                )
                .await,
            Err(Error::Internal)
        ));
        assert_eq!(
            workspace_state(h, actor.user.id, w).await,
            before,
            "Failed issue creation cannot consume a number, retry key, activity row or stream sequence"
        );
        restore(h, table).await;
        let issue = h
            .app
            .create_issue(
                actor,
                w,
                &key,
                req(json!({"team_id":team,"title":"Audited create"})),
            )
            .await
            .unwrap();
        let committed = workspace_state(h, actor.user.id, w).await;
        let replay = h
            .app
            .create_issue(
                actor,
                w,
                &key,
                req(json!({"team_id":team,"title":"Audited create"})),
            )
            .await
            .unwrap();
        assert_eq!(issue.id, replay.id);
        assert_eq!(workspace_state(h, actor.user.id, w).await, committed);
    }
    let mut issue = h
        .app
        .create_issue(
            actor,
            w,
            "atomic-update-target",
            req(json!({"team_id":team,"title":"Before audited update"})),
        )
        .await
        .unwrap();
    for (table, field, action) in [
        ("activity_entries", "event_kind", "issue.updated".to_owned()),
        ("workspace_events", "event_type", "issue.updated".to_owned()),
        ("workspace_stream_heads", "workspace_id", w.to_string()),
    ] {
        let before = workspace_state(h, actor.user.id, w).await;
        suppress(
            h,
            table,
            if table == "workspace_stream_heads" {
                "UPDATE"
            } else {
                "INSERT"
            },
            field,
            &action,
        )
        .await;
        assert!(matches!(
            h.app
                .update_issue(
                    actor,
                    w,
                    issue.id,
                    &issue.etag(),
                    json!({"title":"After audited update"})
                )
                .await,
            Err(Error::Internal)
        ));
        assert_eq!(
            workspace_state(h, actor.user.id, w).await,
            before,
            "Issue version, body, history and stream allocation must roll back together"
        );
        assert_eq!(
            h.app.get_issue(actor, w, issue.id).await.unwrap().etag(),
            issue.etag()
        );
        restore(h, table).await;
        let old_version = issue.version.parse::<u64>().unwrap();
        issue = h
            .app
            .update_issue(
                actor,
                w,
                issue.id,
                &issue.etag(),
                json!({"title":"After audited update"}),
            )
            .await
            .unwrap();
        assert_eq!(issue.version.parse::<u64>().unwrap(), old_version + 1);
    }
}

async fn operator_rollback(h: &Harness, operator: &Operator, actor: &Actor) -> String {
    let user = actor.user.id;
    let before = account_state(h, user).await;
    suppress(
        h,
        "account_security_events",
        "INSERT",
        "action",
        "recovery.revoked",
    )
    .await;
    assert!(operator.revoke_recovery(user, REASON).await.is_err());
    assert_eq!(
        account_state(h, user).await,
        before,
        "Even a zero-code operator command requires its mandatory identity/reason audit"
    );
    restore(h, "account_security_events").await;
    assert_eq!(operator.revoke_recovery(user, REASON).await.unwrap(), 0);
    let after = account_state(h, user).await;
    assert_eq!(
        after["audit"].as_array().unwrap().len(),
        before["audit"].as_array().unwrap().len() + 1
    );
    let first = generate_recovery_code(user);
    let first_id = Uuid::new_v4();
    operator
        .issue_recovery(user, first_id, &first, 30, REASON)
        .await
        .unwrap();
    for (table, operation, field, value) in [
        (
            "account_security_events",
            "INSERT",
            "action",
            "recovery.issued".to_owned(),
        ),
        (
            "recovery_tokens",
            "INSERT",
            "id",
            "generated-receipt".to_owned(),
        ),
        ("recovery_tokens", "UPDATE", "id", first_id.to_string()),
    ] {
        let code = generate_recovery_code(user);
        let receipt = Uuid::new_v4();
        let value = if value == "generated-receipt" {
            receipt.to_string()
        } else {
            value
        };
        let before = account_state(h, user).await;
        suppress(h, table, operation, field, &value).await;
        assert!(
            operator
                .issue_recovery(user, receipt, &code, 30, REASON)
                .await
                .is_err()
        );
        assert_eq!(
            account_state(h, user).await,
            before,
            "Failed issuance must preserve the previous code and leave no new usable code/audit"
        );
        restore(h, table).await;
        assert!(matches!(
            h.app
                .complete_recovery(req(json!({"token":code,"password":NEW_PASSWORD})))
                .await,
            Err(Error::InvalidRecovery)
        ));
    }
    let replacement = generate_recovery_code(user);
    let replacement_id = Uuid::new_v4();
    operator
        .issue_recovery(user, replacement_id, &replacement, 30, REASON)
        .await
        .unwrap();
    assert!(matches!(
        h.app
            .complete_recovery(req(json!({"token":first,"password":NEW_PASSWORD})))
            .await,
        Err(Error::InvalidRecovery)
    ));
    for (table, operation, field, value) in [
        (
            "account_security_events",
            "INSERT",
            "action",
            "recovery.revoked".to_owned(),
        ),
        (
            "recovery_tokens",
            "UPDATE",
            "id",
            replacement_id.to_string(),
        ),
    ] {
        let before = account_state(h, user).await;
        suppress(h, table, operation, field, &value).await;
        assert!(operator.revoke_recovery(user, REASON).await.is_err());
        assert_eq!(
            account_state(h, user).await,
            before,
            "A failed revocation cannot invalidate the handed-out code or pretend zero work succeeded"
        );
        restore(h, table).await;
    }
    replacement
}

async fn recovery_rollback(h: &Harness, actor: &Actor, cookie: &str, code: &str) {
    let (other, a) = login(h, PASSWORD).await;
    let (_, b) = login(h, PASSWORD).await;
    let mut tx = fixture(h, actor.user.id, Uuid::nil()).await;
    let selected: Uuid = sqlx::query_scalar("SELECT id FROM sessions WHERE token_hash=$1")
        .bind(&other.token_hash)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    let receipt:Uuid=sqlx::query_scalar("SELECT id FROM recovery_tokens WHERE user_id=$1 AND used_at IS NULL AND revoked_at IS NULL").bind(actor.user.id).fetch_one(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    for (table, operation, field, value) in [
        (
            "account_security_events",
            "INSERT",
            "action",
            "session.revoked".to_owned(),
        ),
        (
            "account_security_events",
            "INSERT",
            "action",
            "recovery.completed".to_owned(),
        ),
        ("sessions", "UPDATE", "id", selected.to_string()),
        ("users", "UPDATE", "id", actor.user.id.to_string()),
        ("recovery_tokens", "UPDATE", "id", receipt.to_string()),
    ] {
        let before = account_state(h, actor.user.id).await;
        suppress(h, table, operation, field, &value).await;
        assert!(matches!(
            h.app
                .complete_recovery(req(json!({"token":code,"password":NEW_PASSWORD})))
                .await,
            Err(Error::Internal)
        ));
        assert_eq!(
            account_state(h, actor.user.id).await,
            before,
            "Recovery must retain password, code, all sessions and audit on any suppressed mandatory write"
        );
        restore(h, table).await;
        assert_live(h, &[cookie, &a, &b]).await;
    }
    h.app
        .complete_recovery(req(json!({"token":code,"password":NEW_PASSWORD})))
        .await
        .unwrap();
    for cookie in [cookie, &a, &b] {
        assert!(matches!(
            h.app.authenticate(cookie).await,
            Err(Error::Unauthorized)
        ));
    }
    assert!(matches!(
        h.app
            .login(req(json!({"email":EMAIL,"password":PASSWORD})))
            .await,
        Err(Error::Unauthorized)
    ));
    assert!(matches!(
        h.app
            .complete_recovery(req(json!({"token":code,"password":NEW_PASSWORD})))
            .await,
        Err(Error::InvalidRecovery)
    ));
    let (_, fresh) = login(h, NEW_PASSWORD).await;
    assert!(h.app.authenticate(&fresh).await.is_ok());
}

struct LifecycleFixture {
    actor: Actor,
    cookies: Vec<String>,
    session: Uuid,
    recovery: Uuid,
    code: String,
    invitations: Vec<InvitationIssued>,
}
async fn lifecycle_fixture(
    h: &Harness,
    operator: &Operator,
    owner: &Actor,
    w: Uuid,
) -> LifecycleFixture {
    let user = Uuid::new_v4();
    let email = format!("{user}@atomic-lifecycle.invalid");
    let mut tx = fixture(h, owner.user.id, w).await;
    sqlx::query("INSERT INTO users(id,email,display_name,password_hash) SELECT $1,$2,'Atomic lifecycle target',password_hash FROM users WHERE id=$3").bind(user).bind(&email).bind(owner.user.id).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO workspace_members(workspace_id,user_id,role) VALUES($1,$2,'admin')")
        .bind(w)
        .bind(user)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let mut cookies = Vec::new();
    for _ in 0..2 {
        let (_, cookie) = h
            .app
            .login(req(json!({"email":email,"password":PASSWORD})))
            .await
            .unwrap();
        cookies.push(cookie);
    }
    let actor = h.app.authenticate(&cookies[0]).await.unwrap();
    let code = generate_recovery_code(user);
    let recovery = Uuid::new_v4();
    operator
        .issue_recovery(user, recovery, &code, 30, REASON)
        .await
        .unwrap();
    let mut invitations = Vec::new();
    for _ in 0..2 {
        invitations.push(h.app.create_invitation(&actor,w,&Uuid::new_v4().to_string(),req(json!({"email":format!("{}@atomic-invitation.invalid",Uuid::new_v4()),"role":"member","team_ids":[]}))).await.unwrap());
    }
    let mut tx = fixture(h, user, w).await;
    let session: Uuid = sqlx::query_scalar("SELECT id FROM sessions WHERE token_hash=$1")
        .bind(&actor.token_hash)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    LifecycleFixture {
        actor,
        cookies,
        session,
        recovery,
        code,
        invitations,
    }
}
async fn lifecycle_state(h: &Harness, user: Uuid, w: Uuid) -> Value {
    let account = account_state(h, user).await;
    let workspace = workspace_state(h, user, w).await;
    let mut tx = fixture(h, user, w).await;
    let related:Value=sqlx::query_scalar("SELECT jsonb_build_object('invitations',(SELECT coalesce(jsonb_agg(to_jsonb(i) ORDER BY id),'[]') FROM invitations i WHERE workspace_id=$1),'memberships',(SELECT coalesce(jsonb_agg(to_jsonb(m) ORDER BY user_id),'[]') FROM workspace_members m WHERE workspace_id=$1),'team_grants',(SELECT coalesce(jsonb_agg(to_jsonb(m) ORDER BY team_id,user_id),'[]') FROM team_members m WHERE workspace_id=$1))").bind(w).fetch_one(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    json!({"account":account,"workspace":workspace,"related":related})
}
async fn lifecycle_write_rollback(h: &Harness, operator: &Operator, owner: &Actor, w: Uuid) {
    for legacy_enable in [false, true] {
        let target = lifecycle_fixture(h, operator, owner, w).await;
        let user = target.actor.user.id;
        if legacy_enable {
            // Model a legitimately disabled account retained from before the
            // lifecycle migration: credentials and outgoing invites still exist.
            let mut tx = fixture(h, owner.user.id, w).await;
            sqlx::query("UPDATE users SET disabled_at=clock_timestamp() WHERE id=$1")
                .bind(user)
                .execute(&mut *tx)
                .await
                .unwrap();
            tx.commit().await.unwrap();
        }
        for (table, id) in [
            ("users", user),
            ("sessions", target.session),
            ("recovery_tokens", target.recovery),
            ("invitations", target.invitations[0].invitation.id),
        ] {
            let before = lifecycle_state(h, user, w).await;
            suppress(h, table, "UPDATE", "id", &id.to_string()).await;
            let result = if legacy_enable {
                operator.enable_account(user, REASON).await
            } else {
                operator.disable_account(user, REASON).await
            };
            assert!(
                matches!(result, Err(Error::Internal)),
                "Silently skipped {table} update cannot commit lifecycle transition; legacy_enable={legacy_enable}"
            );
            assert_eq!(
                lifecycle_state(h, user, w).await,
                before,
                "Account state, every credential/invite, mandatory audits and workspace projections roll back together"
            );
            restore(h, table).await;
            for cookie in &target.cookies {
                if legacy_enable {
                    assert!(matches!(
                        h.app.authenticate(cookie).await,
                        Err(Error::Unauthorized)
                    ));
                } else {
                    assert!(h.app.authenticate(cookie).await.is_ok());
                }
            }
        }
        let receipt = if legacy_enable {
            operator.enable_account(user, REASON).await.unwrap()
        } else {
            operator.disable_account(user, REASON).await.unwrap()
        };
        assert!(receipt.changed);
        assert_eq!(receipt.disabled, !legacy_enable);
        assert_eq!(
            (
                receipt.revoked_sessions,
                receipt.revoked_recovery_codes,
                receipt.revoked_invitations
            ),
            (2, 1, 2)
        );
        for cookie in &target.cookies {
            assert!(matches!(
                h.app.authenticate(cookie).await,
                Err(Error::Unauthorized)
            ));
        }
        assert!(matches!(
            h.app
                .complete_recovery(req(json!({"token":target.code,"password":NEW_PASSWORD})))
                .await,
            Err(Error::InvalidRecovery)
        ));
        if !legacy_enable {
            operator.enable_account(user, REASON).await.unwrap();
        }
        for invitation in &target.invitations {
            assert!(matches!(
                h.app
                    .invitation_preview(invitation.token.as_ref().unwrap())
                    .await,
                Err(Error::NotFound)
            ));
        }
        let (_, fresh) = h
            .app
            .login(req(
                json!({"email":target.actor.user.email,"password":PASSWORD}),
            ))
            .await
            .unwrap();
        assert!(h.app.authenticate(&fresh).await.is_ok());
    }
}
