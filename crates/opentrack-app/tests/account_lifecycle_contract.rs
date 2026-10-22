//! Global account state changes use real PostgreSQL authority and lock ordering.
#[allow(dead_code)]
mod support;

use opentrack_app::{AccountLifecycleOutcome, App, Operator, generate_recovery_code};
use opentrack_domain::*;
use opentrack_store as store;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use sqlx::{AssertSqlSafe, postgres::PgPoolOptions};
use support::Harness;
use uuid::Uuid;

const PASSWORD: &str = "account lifecycle contract password 615!";
const REASON: &str = "Verified installation operator request";

fn req<T: DeserializeOwned>(value: Value) -> T {
    serde_json::from_value(value).unwrap()
}
#[derive(Clone)]
struct Account {
    id: Uuid,
    email: String,
}
async fn fixture(h: &Harness, user: Uuid, w: Uuid) -> store::Tx {
    let mut tx = store::begin(&h.owner).await.unwrap();
    sqlx::query("SELECT set_config('opentrack.user_id',$1,true),set_config('opentrack.workspace_id',$2,true)")
        .bind(user.to_string()).bind(w.to_string()).execute(&mut *tx).await.unwrap();
    tx
}
async fn account(h: &Harness, source: Uuid, w: Uuid, role: &str) -> Account {
    let id = Uuid::new_v4();
    let email = format!("{id}@lifecycle.invalid");
    let mut tx = fixture(h, source, w).await;
    // Fixture users share a known valid hash; all cookies are minted by the real
    // password verification path rather than manually inserting sessions.
    sqlx::query("INSERT INTO users(id,email,display_name,password_hash) SELECT $1,$2,'Lifecycle target',password_hash FROM users WHERE id=$3")
        .bind(id).bind(&email).bind(source).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO workspace_members(workspace_id,user_id,role) VALUES($1,$2,$3)")
        .bind(w)
        .bind(id)
        .bind(role)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    Account { id, email }
}
async fn login(h: &Harness, target: &Account) -> (Actor, String) {
    let (_, cookie) = h
        .app
        .login(req(json!({"email":target.email,"password":PASSWORD})))
        .await
        .unwrap();
    (h.app.authenticate(&cookie).await.unwrap(), cookie)
}
async fn revoked(h: &Harness, actor: &Actor, cookie: &str) {
    assert!(matches!(
        h.app.authenticate(cookie).await,
        Err(Error::Unauthorized)
    ));
    assert!(matches!(
        h.app.session(actor).await,
        Err(Error::Unauthorized)
    ));
}
async fn workspace(h: &Harness, actor: &Actor, organization: Uuid, key: &str) -> Uuid {
    h.app.create_workspace(actor, key, req(json!({"organization_id":organization,"name":key,"team_name":"Lifecycle team","team_key":"LIF"}))).await.unwrap().id
}
async fn membership(h: &Harness, actor: Uuid, w: Uuid, target: Uuid, role: &str, active: bool) {
    let mut tx = fixture(h, actor, w).await;
    sqlx::query("INSERT INTO workspace_members(workspace_id,user_id,role,active) VALUES($1,$2,$3,$4) ON CONFLICT(workspace_id,user_id) DO UPDATE SET role=EXCLUDED.role,active=EXCLUDED.active")
        .bind(w).bind(target).bind(role).bind(active).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
}
async fn organization_owner(h: &Harness, organization: Uuid, target: Uuid) {
    let mut tx = fixture(h, target, Uuid::nil()).await;
    sqlx::query("INSERT INTO organization_members(organization_id,user_id,role) VALUES($1,$2,'owner') ON CONFLICT(organization_id,user_id) DO UPDATE SET role='owner'")
        .bind(organization).bind(target).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
}
async fn invitation(h: &Harness, actor: &Actor, w: Uuid) -> InvitationIssued {
    h.app.create_invitation(actor,w,&Uuid::new_v4().to_string(),req(json!({"email":format!("{}@invite.invalid",Uuid::new_v4()),"role":"member","team_ids":[]}))).await.unwrap()
}
async fn issue_recovery(operator: &Operator, target: Uuid) -> (Uuid, String) {
    let code = generate_recovery_code(target);
    let receipt = operator
        .issue_recovery(target, Uuid::new_v4(), &code, 30, REASON)
        .await
        .unwrap();
    (receipt.id, code)
}
async fn snapshot(h: &Harness, target: Uuid, w: Uuid) -> Value {
    let mut tx = fixture(h, target, w).await;
    let state:Value=sqlx::query_scalar("SELECT jsonb_build_object('disabled_at',(SELECT disabled_at FROM users WHERE id=$1),'sessions',(SELECT coalesce(jsonb_agg(jsonb_build_array(id,revoked_at) ORDER BY id),'[]') FROM sessions WHERE user_id=$1),'recovery',(SELECT coalesce(jsonb_agg(jsonb_build_array(id,revoked_at,used_at) ORDER BY id),'[]') FROM recovery_tokens WHERE user_id=$1),'invitations',(SELECT coalesce(jsonb_agg(jsonb_build_array(id,revoked_at,version) ORDER BY id),'[]') FROM invitations WHERE workspace_id=$2 AND invited_by=$1),'account_audit',(SELECT count(*) FROM account_security_events WHERE user_id=$1),'workspace_audit',(SELECT count(*) FROM audit_entries WHERE workspace_id=$2),'events',(SELECT count(*) FROM workspace_events WHERE workspace_id=$2),'epoch',(SELECT epoch FROM workspace_auth_guards WHERE workspace_id=$2))")
        .bind(target).bind(w).fetch_one(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    state
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_postgres_account_lifecycle_contracts() {
    let h = Harness::with_connections(6).await;
    let runner = h.clone();
    let result = tokio::spawn(async move { exercise(&runner).await }).await;
    h.cleanup().await;
    if let Err(error) = result {
        std::panic::resume_unwind(error.into_panic());
    }
}

async fn exercise(h: &Harness) {
    let (session,cookie)=h.app.setup(req(json!({"setup_token":h.setup_token,"email":"owner@lifecycle.invalid","password":PASSWORD,"display_name":"Lifecycle owner","workspace_name":"Lifecycle workspace","team_name":"Engineering","team_key":"ENG"}))).await.unwrap();
    let owner = h.app.authenticate(&cookie).await.unwrap();
    let w = session.workspaces[0].id;
    let team = h.app.bootstrap(&owner, w).await.unwrap().teams[0].id;
    let organization = h.app.organizations(&owner).await.unwrap()[0].id;
    let operator_url = h.operator_url().await;
    let operator = Operator::connect(&operator_url).await.unwrap();
    boundaries(h, &operator, &operator_url, owner.user.id, w).await;
    owner_invariants(h, &operator, &owner, organization, w).await;

    let target = account(h, owner.user.id, w, "admin").await;
    let (actor, first) = login(h, &target).await;
    let (second_actor, second) = login(h, &target).await;
    let other_w = workspace(h, &owner, organization, "inactive-inviter-workspace").await;
    membership(h, owner.user.id, other_w, target.id, "admin", true).await;
    let first_invite = invitation(h, &actor, w).await;
    let inactive_invite = invitation(h, &actor, other_w).await;
    let accepted_invite = invitation(h, &actor, w).await;
    let mut tx = fixture(h, target.id, w).await;
    sqlx::query("UPDATE invitations SET accepted_at=clock_timestamp() WHERE id=$1")
        .bind(accepted_invite.invitation.id)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    membership(h, owner.user.id, other_w, target.id, "admin", false).await;
    let (recovery, code) = issue_recovery(&operator, target.id).await;
    let issue = h
        .app
        .create_issue(
            &actor,
            w,
            "authorship-survives-disable",
            req(json!({"team_id":team,"title":"Authorship survives disable"})),
        )
        .await
        .unwrap();
    let before_main = snapshot(h, target.id, w).await;
    let before_other = snapshot(h, target.id, other_w).await;
    let disabled = operator
        .disable_account(target.id, &format!("  {REASON}  "))
        .await
        .unwrap();
    assert!(disabled.disabled && disabled.changed && disabled.disabled_at.is_some());
    assert_eq!(disabled.user_id, target.id);
    assert_eq!(
        (
            disabled.revoked_sessions,
            disabled.revoked_recovery_codes,
            disabled.revoked_invitations
        ),
        (2, 1, 2)
    );
    revoked(h, &actor, &first).await;
    revoked(h, &second_actor, &second).await;
    assert!(matches!(
        h.app
            .login(req(json!({"email":target.email,"password":PASSWORD})))
            .await,
        Err(Error::Unauthorized)
    ));
    assert!(matches!(
        h.app
            .complete_recovery(req(
                json!({"token":code,"password":"new lifecycle password 901!"})
            ))
            .await,
        Err(Error::InvalidRecovery)
    ));
    for token in [
        first_invite.token.as_ref().unwrap(),
        inactive_invite.token.as_ref().unwrap(),
    ] {
        assert!(matches!(
            h.app.invitation_preview(token).await,
            Err(Error::NotFound)
        ));
    }
    assert!(h.app.get_issue(&owner, w, issue.id).await.is_ok());
    let mut tx = fixture(h, owner.user.id, w).await;
    let author: Uuid = sqlx::query_scalar("SELECT created_by_user_id FROM issues WHERE id=$1")
        .bind(issue.id)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert_eq!(author, target.id);
    tx.commit().await.unwrap();
    assert!(h.app.authenticate(&cookie).await.is_ok());
    let mut tx = fixture(h, owner.user.id, w).await;
    let terminal_unchanged: bool = sqlx::query_scalar(
        "SELECT accepted_at IS NOT NULL AND revoked_at IS NULL FROM invitations WHERE id=$1",
    )
    .bind(accepted_invite.invitation.id)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert!(
        terminal_unchanged,
        "Accepted invitations retain their terminal history"
    );
    tx.commit().await.unwrap();
    let after_main = snapshot(h, target.id, w).await;
    let after_other = snapshot(h, target.id, other_w).await;
    assert!(after_main["epoch"].as_i64().unwrap() > before_main["epoch"].as_i64().unwrap());
    assert!(after_other["epoch"].as_i64().unwrap() > before_other["epoch"].as_i64().unwrap());
    correlated_audit(
        h,
        target.id,
        &[w, other_w],
        &disabled,
        &operator_url,
        &[
            &first,
            &second,
            &code,
            first_invite.token.as_ref().unwrap(),
            inactive_invite.token.as_ref().unwrap(),
        ],
    )
    .await;
    let noop = operator.disable_account(target.id, REASON).await.unwrap();
    assert!(noop.disabled && !noop.changed);
    assert_eq!(noop.disabled_at, disabled.disabled_at);
    assert_ne!(noop.correlation_id, disabled.correlation_id);
    assert_eq!(
        (
            noop.revoked_sessions,
            noop.revoked_recovery_codes,
            noop.revoked_invitations
        ),
        (0, 0, 0)
    );
    let after_noop = snapshot(h, target.id, w).await;
    for key in [
        "epoch",
        "events",
        "workspace_audit",
        "sessions",
        "recovery",
        "invitations",
    ] {
        assert_eq!(after_noop[key], after_main[key]);
    }
    assert_eq!(
        after_noop["account_audit"].as_i64().unwrap(),
        after_main["account_audit"].as_i64().unwrap() + 1
    );
    let enabled = operator.enable_account(target.id, REASON).await.unwrap();
    assert!(!enabled.disabled && enabled.changed && enabled.disabled_at.is_none());
    assert_eq!(
        (
            enabled.revoked_sessions,
            enabled.revoked_recovery_codes,
            enabled.revoked_invitations
        ),
        (0, 0, 0)
    );
    revoked(h, &actor, &first).await;
    revoked(h, &second_actor, &second).await;
    assert!(matches!(
        h.app
            .complete_recovery(req(
                json!({"token":code,"password":"new lifecycle password 902!"})
            ))
            .await,
        Err(Error::InvalidRecovery)
    ));
    // Restore inviter authority as well: the old invitation must remain revoked,
    // rather than merely being hidden by the inactive-membership check.
    membership(h, owner.user.id, other_w, target.id, "admin", true).await;
    for token in [
        first_invite.token.as_ref().unwrap(),
        inactive_invite.token.as_ref().unwrap(),
    ] {
        assert!(matches!(
            h.app.invitation_preview(token).await,
            Err(Error::NotFound)
        ));
    }
    let (fresh, fresh_cookie) = login(h, &target).await;
    assert!(h.app.bootstrap(&fresh, w).await.is_ok());
    assert!(h.app.bootstrap(&fresh, other_w).await.is_ok());
    let before_noop = snapshot(h, target.id, w).await;
    let noop = operator.enable_account(target.id, REASON).await.unwrap();
    assert!(!noop.disabled && !noop.changed && noop.disabled_at.is_none());
    let after_noop = snapshot(h, target.id, w).await;
    for key in [
        "epoch",
        "events",
        "workspace_audit",
        "sessions",
        "recovery",
        "invitations",
    ] {
        assert_eq!(after_noop[key], before_noop[key]);
    }
    let mut tx = fixture(h, target.id, w).await;
    let actions:Vec<String>=sqlx::query_scalar("SELECT action FROM account_security_events WHERE user_id=$1 AND action LIKE 'account.%' ORDER BY created_at,id").bind(target.id).fetch_all(&mut *tx).await.unwrap();
    assert_eq!(
        actions,
        vec![
            "account.disabled",
            "account.disable_noop",
            "account.enabled",
            "account.enable_noop"
        ]
    );
    let revoked_code: bool =
        sqlx::query_scalar("SELECT revoked_at IS NOT NULL FROM recovery_tokens WHERE id=$1")
            .bind(recovery)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    assert!(revoked_code);
    tx.commit().await.unwrap();
    assert!(h.app.authenticate(&fresh_cookie).await.is_ok());
    audit_rollback(h, &operator, &target, &fresh, w).await;
    lock_orderings(h, &operator, &owner, w, team).await;
    concurrent_last_owners(h, &operator, &owner, organization).await;
}

async fn boundaries(h: &Harness, operator: &Operator, operator_url: &str, user: Uuid, w: Uuid) {
    for disabled in [true, false] {
        let result = if disabled {
            operator.disable_account(Uuid::new_v4(), REASON).await
        } else {
            operator.enable_account(Uuid::new_v4(), REASON).await
        };
        assert!(matches!(result, Err(Error::NotFound)));
    }
    for reason in ["".to_owned(), " \n ".to_owned(), "x".repeat(501)] {
        assert!(matches!(
            operator.disable_account(user, &reason).await,
            Err(Error::Invalid(_))
        ));
    }
    for statement in [
        "SELECT * FROM operator_disable_account($1,'forged')",
        "SELECT * FROM operator_enable_account($1,'forged')",
        "UPDATE users SET disabled_at=clock_timestamp() WHERE id=$1",
    ] {
        let error = sqlx::query(statement)
            .bind(user)
            .execute(&h.app.pool)
            .await
            .unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("42501")
        );
    }
    let runtime_visible: i64 = sqlx::query_scalar("SELECT count(*) FROM organization_members")
        .fetch_one(&h.app.pool)
        .await
        .unwrap();
    assert_eq!(
        runtime_visible, 0,
        "Owner-only inspection must not broaden runtime RLS"
    );
    for statement in [
        "INSERT INTO audit_entries(id,workspace_id,actor_id,actor_kind,operator_identity,action,target_id,correlation_id) VALUES($3,$2,NULL,'local_operator','forged operator','account.disabled',$1,$3)",
        "INSERT INTO workspace_events(workspace_id,seq,event_id,event_type,entity_id,entity_version,actor_id,actor_kind,mutation_id) SELECT $2,999999,$3,'access.changed',$2,1,NULL,'local_operator',$3 FROM users WHERE id=$1",
        "INSERT INTO account_security_events(user_id,action,actor_kind,operator_identity,reason,correlation_id) SELECT $1,'account.disabled','local_operator','forged operator','forged reason',$3 WHERE $2::uuid IS NOT NULL",
    ] {
        let mut tx = store::begin(&h.app.pool).await.unwrap();
        store::context(&mut tx, user, Some(w)).await.unwrap();
        let error = sqlx::query(statement)
            .bind(user)
            .bind(w)
            .bind(Uuid::new_v4())
            .execute(&mut *tx)
            .await
            .unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("42501"),
            "Runtime cannot forge operator attribution in audit or events"
        );
        tx.rollback().await.unwrap();
    }
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(operator_url)
        .await
        .unwrap();
    for statement in [
        "SELECT * FROM users",
        "UPDATE users SET disabled_at=NULL",
        "SELECT * FROM audit_entries",
    ] {
        let error = sqlx::query(statement).execute(&pool).await.unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("42501")
        );
    }
    let schema: String = sqlx::query_scalar("SELECT current_schema()")
        .fetch_one(&h.owner)
        .await
        .unwrap();
    let mut runtime =
        url::Url::parse(&std::env::var("OPENTRACK_TEST_DATABASE_URL").unwrap()).unwrap();
    runtime
        .query_pairs_mut()
        .append_pair("options", &format!("-csearch_path={schema}"));
    let role = runtime.username().replace('"', "\"\"");
    for function in ["operator_disable_account", "operator_enable_account"] {
        // Function names are fixed literals; the role is identifier-escaped and
        // the schema is generated exclusively by this test harness.
        sqlx::query(AssertSqlSafe(format!(
            "GRANT EXECUTE ON FUNCTION {schema}.{function}(uuid,text) TO \"{role}\""
        )))
        .execute(&h.owner)
        .await
        .unwrap();
        assert!(matches!(
            App::connect(runtime.as_str(), &h.setup_token, 1, 1).await,
            Err(Error::Internal)
        ));
        sqlx::query(AssertSqlSafe(format!(
            "REVOKE EXECUTE ON FUNCTION {schema}.{function}(uuid,text) FROM \"{role}\""
        )))
        .execute(&h.owner)
        .await
        .unwrap();
    }
    for statement in [
        "SELECT assert_workspace_enabled_owner($1,NULL)",
        "SELECT assert_organization_enabled_owner($1,NULL)",
        "SELECT * FROM operator_change_account_state($1,true,'forged')",
    ] {
        for pool in [&h.app.pool, &pool] {
            let error = sqlx::query(statement)
                .bind(user)
                .execute(pool)
                .await
                .unwrap_err();
            assert_eq!(
                error.as_database_error().unwrap().code().as_deref(),
                Some("42501")
            );
        }
    }
    let paths:Vec<Vec<String>>=sqlx::query_scalar("SELECT proconfig FROM pg_proc WHERE pronamespace=current_schema()::regnamespace AND proname IN ('operator_disable_account','operator_enable_account')").fetch_all(&h.owner).await.unwrap();
    assert_eq!(paths.len(), 2);
    for config in paths {
        assert!(
            config
                .iter()
                .any(|v| v == &format!("search_path=pg_catalog, {schema}, pg_temp"))
        );
    }
    pool.close().await;
}

async fn owner_invariants(
    h: &Harness,
    operator: &Operator,
    owner: &Actor,
    organization: Uuid,
    w: Uuid,
) {
    let before = snapshot(h, owner.user.id, w).await;
    assert!(matches!(
        operator.disable_account(owner.user.id, REASON).await,
        Err(Error::Conflict(_))
    ));
    assert_eq!(snapshot(h, owner.user.id, w).await, before);
    let backup = account(h, owner.user.id, w, "owner").await;
    // Workspace ownership alone cannot make the sole organization owner safe.
    assert!(matches!(
        operator.disable_account(owner.user.id, REASON).await,
        Err(Error::Conflict(_))
    ));
    organization_owner(h, organization, backup.id).await;
    let archived = workspace(h, owner, organization, "archived-owner-guard").await;
    let mut tx = fixture(h, owner.user.id, archived).await;
    sqlx::query("UPDATE workspaces SET archived_at=clock_timestamp() WHERE id=$1")
        .bind(archived)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert!(matches!(
        operator.disable_account(owner.user.id, REASON).await,
        Err(Error::Conflict(_))
    ));
    membership(h, owner.user.id, archived, backup.id, "owner", false).await;
    assert!(matches!(
        operator.disable_account(owner.user.id, REASON).await,
        Err(Error::Conflict(_))
    ));
    membership(h, owner.user.id, archived, backup.id, "owner", true).await;
    // A second organization with no workspace still has its own owner invariant.
    let second_org = Uuid::new_v4();
    let mut tx = fixture(h, owner.user.id, w).await;
    sqlx::query(
        "INSERT INTO organizations(id,name,created_by) VALUES($1,'Separate organization',$2)",
    )
    .bind(second_org)
    .bind(owner.user.id)
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO organization_members(organization_id,user_id,role) VALUES($1,$2,'owner')",
    )
    .bind(second_org)
    .bind(owner.user.id)
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    assert!(matches!(
        operator.disable_account(owner.user.id, REASON).await,
        Err(Error::Conflict(_))
    ));
    organization_owner(h, second_org, backup.id).await;
    // Leave the test administrator signed in; disable its backup instead. The
    // now-disabled backup cannot satisfy either owner invariant on a later call.
    operator.disable_account(backup.id, REASON).await.unwrap();
    assert!(matches!(
        operator.disable_account(owner.user.id, REASON).await,
        Err(Error::Conflict(_))
    ));
    operator.enable_account(backup.id, REASON).await.unwrap();
}

async fn correlated_audit(
    h: &Harness,
    user: Uuid,
    workspaces: &[Uuid],
    receipt: &AccountLifecycleOutcome,
    operator_url: &str,
    secrets: &[&str],
) {
    let operator_name = url::Url::parse(operator_url).unwrap().username().to_owned();
    for w in workspaces {
        let mut tx = fixture(h, user, *w).await;
        let account:(String,String,String)=sqlx::query_as("SELECT actor_kind,operator_identity,reason FROM account_security_events WHERE user_id=$1 AND action='account.disabled' AND correlation_id=$2").bind(user).bind(receipt.correlation_id).fetch_one(&mut *tx).await.unwrap();
        assert_eq!(
            account,
            (
                "local_operator".into(),
                operator_name.clone(),
                REASON.into()
            )
        );
        let audit:(bool,String,String)=sqlx::query_as("SELECT actor_id IS NULL,actor_kind,operator_identity FROM audit_entries WHERE workspace_id=$1 AND correlation_id=$2").bind(w).bind(receipt.correlation_id).fetch_one(&mut *tx).await.unwrap();
        assert_eq!(
            audit,
            (true, "local_operator".into(), operator_name.clone())
        );
        let event: Value = sqlx::query_scalar(
            "SELECT to_jsonb(e) FROM workspace_events e WHERE workspace_id=$1 AND mutation_id=$2",
        )
        .bind(w)
        .bind(receipt.correlation_id)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
        assert_eq!(event["actor_kind"], "local_operator");
        assert!(event["actor_id"].is_null());
        assert_eq!(event["event_type"], "access.changed");
        assert_eq!(event["entity_id"], w.to_string());
        assert!(event["team_id"].is_null());
        let session_audit:i64=sqlx::query_scalar("SELECT count(*) FROM account_security_events WHERE user_id=$1 AND correlation_id=$2 AND action='session.revoked'").bind(user).bind(receipt.correlation_id).fetch_one(&mut *tx).await.unwrap();
        assert_eq!(session_audit, receipt.revoked_sessions as i64);
        let records: Vec<Value> = sqlx::query_scalar(
            "SELECT to_jsonb(s) FROM account_security_events s WHERE user_id=$1",
        )
        .bind(user)
        .fetch_all(&mut *tx)
        .await
        .unwrap();
        let serialized = serde_json::to_string(&records).unwrap();
        for secret in
            secrets
                .iter()
                .copied()
                .chain([PASSWORD, "password_hash", "token_hash", "csrf_token"])
        {
            assert!(!serialized.contains(secret));
        }
        assert!(!event.to_string().contains(&operator_name));
        assert!(!event.to_string().contains(REASON));
        tx.commit().await.unwrap();
    }
}

async fn audit_rollback(
    h: &Harness,
    operator: &Operator,
    target: &Account,
    actor: &Actor,
    w: Uuid,
) {
    let (_, code) = issue_recovery(operator, target.id).await;
    invitation(h, actor, w).await;
    sqlx::raw_sql("CREATE FUNCTION test_reject_account_audit() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.action=TG_ARGV[0] THEN RAISE EXCEPTION 'Controlled mandatory audit failure'; END IF; RETURN NEW; END $$").execute(&h.owner).await.unwrap();
    for table in ["account_security_events", "audit_entries"] {
        // The table/action identifiers are fixed test literals, never user input.
        sqlx::query(AssertSqlSafe(format!("CREATE TRIGGER test_audit_failure BEFORE INSERT ON {table} FOR EACH ROW EXECUTE FUNCTION test_reject_account_audit('account.disabled')"))).execute(&h.owner).await.unwrap();
        let before = snapshot(h, target.id, w).await;
        assert!(operator.disable_account(target.id, REASON).await.is_err());
        assert_eq!(
            snapshot(h, target.id, w).await,
            before,
            "Every durable change rolls back with mandatory audit"
        );
        assert!(h.app.session(actor).await.is_ok());
        sqlx::query(AssertSqlSafe(format!(
            "DROP TRIGGER test_audit_failure ON {table}"
        )))
        .execute(&h.owner)
        .await
        .unwrap();
    }
    sqlx::raw_sql("CREATE FUNCTION test_drop_account_audit() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF to_jsonb(NEW)->>TG_ARGV[0]=TG_ARGV[1] THEN RETURN NULL; END IF; RETURN NEW; END $$").execute(&h.owner).await.unwrap();
    for (table, field, action) in [
        ("account_security_events", "action", "session.revoked"),
        ("account_security_events", "action", "account.disabled"),
        ("audit_entries", "action", "account.disabled"),
        ("workspace_events", "event_type", "access.changed"),
    ] {
        sqlx::query(AssertSqlSafe(format!("CREATE TRIGGER test_audit_drop BEFORE INSERT ON {table} FOR EACH ROW EXECUTE FUNCTION test_drop_account_audit('{field}','{action}')"))).execute(&h.owner).await.unwrap();
        let before = snapshot(h, target.id, w).await;
        assert!(
            operator.disable_account(target.id, REASON).await.is_err(),
            "Suppressed mandatory record must fail: {table} {action}"
        );
        assert_eq!(
            snapshot(h, target.id, w).await,
            before,
            "Suppressed audit/event cannot silently commit a partial disable"
        );
        sqlx::query(AssertSqlSafe(format!(
            "DROP TRIGGER test_audit_drop ON {table}"
        )))
        .execute(&h.owner)
        .await
        .unwrap();
    }
    operator.disable_account(target.id, REASON).await.unwrap();
    sqlx::raw_sql("CREATE TRIGGER test_audit_failure BEFORE INSERT ON account_security_events FOR EACH ROW EXECUTE FUNCTION test_reject_account_audit('account.enabled')").execute(&h.owner).await.unwrap();
    let before = snapshot(h, target.id, w).await;
    assert!(operator.enable_account(target.id, REASON).await.is_err());
    assert_eq!(snapshot(h, target.id, w).await, before);
    sqlx::raw_sql("DROP TRIGGER test_audit_failure ON account_security_events; DROP FUNCTION test_reject_account_audit()").execute(&h.owner).await.unwrap();
    for (table, field, action) in [
        ("account_security_events", "action", "account.enabled"),
        ("audit_entries", "action", "account.enabled"),
        ("workspace_events", "event_type", "access.changed"),
    ] {
        sqlx::query(AssertSqlSafe(format!("CREATE TRIGGER test_audit_drop BEFORE INSERT ON {table} FOR EACH ROW EXECUTE FUNCTION test_drop_account_audit('{field}','{action}')"))).execute(&h.owner).await.unwrap();
        let before = snapshot(h, target.id, w).await;
        assert!(operator.enable_account(target.id, REASON).await.is_err());
        assert_eq!(snapshot(h, target.id, w).await, before);
        sqlx::query(AssertSqlSafe(format!(
            "DROP TRIGGER test_audit_drop ON {table}"
        )))
        .execute(&h.owner)
        .await
        .unwrap();
    }
    sqlx::query("DROP FUNCTION test_drop_account_audit()")
        .execute(&h.owner)
        .await
        .unwrap();
    operator.enable_account(target.id, REASON).await.unwrap();
    assert!(matches!(
        h.app
            .complete_recovery(req(
                json!({"token":code,"password":"replacement lifecycle password 413!"})
            ))
            .await,
        Err(Error::InvalidRecovery)
    ));
}

async fn install_gates(h: &Harness) {
    sqlx::raw_sql("CREATE FUNCTION test_lifecycle_wait_budget() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN PERFORM set_config('statement_timeout','15000',true); PERFORM set_config('lock_timeout','15000',true); IF TG_OP='DELETE' THEN RETURN OLD; ELSE RETURN NEW; END IF; END $$; CREATE TRIGGER test_login_budget BEFORE DELETE ON login_attempts FOR EACH ROW EXECUTE FUNCTION test_lifecycle_wait_budget(); CREATE TRIGGER test_issue_budget BEFORE INSERT ON idempotency_records FOR EACH ROW EXECUTE FUNCTION test_lifecycle_wait_budget(); CREATE FUNCTION test_lifecycle_gate() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF TG_NARGS=1 OR to_jsonb(NEW)->>'action'=TG_ARGV[1] THEN PERFORM set_config('lock_timeout','15000',true); PERFORM pg_advisory_xact_lock(TG_ARGV[0]::bigint); END IF; RETURN NEW; END $$").execute(&h.owner).await.unwrap();
}
async fn gate(h: &Harness, table: &str, action: Option<&str>) -> (store::Tx, i32) {
    let key = i64::from_be_bytes(Uuid::new_v4().as_bytes()[..8].try_into().unwrap());
    let arguments = action
        .map(|action| format!("'{key}','{action}'"))
        .unwrap_or_else(|| format!("'{key}'"));
    // Only fixed identifiers/action names and an internally generated integer.
    sqlx::query(AssertSqlSafe(format!("CREATE TRIGGER test_gate BEFORE INSERT ON {table} FOR EACH ROW EXECUTE FUNCTION test_lifecycle_gate({arguments})"))).execute(&h.owner).await.unwrap();
    let mut lock = store::begin(&h.owner).await.unwrap();
    let pid = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *lock)
        .await
        .unwrap();
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(key)
        .execute(&mut *lock)
        .await
        .unwrap();
    (lock, pid)
}
async fn remove_gate(h: &Harness, table: &str) {
    sqlx::query(AssertSqlSafe(format!("DROP TRIGGER test_gate ON {table}")))
        .execute(&h.owner)
        .await
        .unwrap();
}
async fn blocked_by(h: &Harness, blocker: i32) -> i32 {
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
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
    .expect("The second operation must reach a real PostgreSQL lock wait")
}
fn disable(
    operator: &Operator,
    user: Uuid,
) -> tokio::task::JoinHandle<Result<AccountLifecycleOutcome>> {
    let operator = operator.clone();
    tokio::spawn(async move { operator.disable_account(user, REASON).await })
}
fn signing_in(h: &Harness, target: &Account) -> tokio::task::JoinHandle<Result<(Session, String)>> {
    let app = h.app.clone();
    let email = target.email.clone();
    tokio::spawn(async move {
        app.login(req(json!({"email":email,"password":PASSWORD})))
            .await
    })
}
fn write(
    h: &Harness,
    actor: &Actor,
    w: Uuid,
    team: Uuid,
    title: &'static str,
) -> tokio::task::JoinHandle<Result<Issue>> {
    let app = h.app.clone();
    let actor = actor.clone();
    tokio::spawn(async move {
        app.create_issue(&actor, w, title, req(json!({"team_id":team,"title":title})))
            .await
    })
}
async fn lock_orderings(h: &Harness, operator: &Operator, owner: &Actor, w: Uuid, team: Uuid) {
    install_gates(h).await;
    let target = account(h, owner.user.id, w, "member").await;
    let (actor, cookie) = login(h, &target).await;
    // The operator has already changed the account under its exclusive guard;
    // a password login reaches that guard and must recheck the disabled state.
    let (lock, pid) = gate(h, "account_security_events", Some("account.disabled")).await;
    let changing = disable(operator, target.id);
    let disabling_pid = blocked_by(h, pid).await;
    let attempt = signing_in(h, &target);
    blocked_by(h, disabling_pid).await;
    lock.commit().await.unwrap();
    changing.await.unwrap().unwrap();
    assert!(matches!(attempt.await.unwrap(), Err(Error::Unauthorized)));
    revoked(h, &actor, &cookie).await;
    remove_gate(h, "account_security_events").await;
    operator.enable_account(target.id, REASON).await.unwrap();

    // A password login that gets the guard first is allowed to finish, but its
    // newly minted cookie is revoked by the waiting disable command.
    let (lock, pid) = gate(h, "sessions", None).await;
    let attempt = signing_in(h, &target);
    let login_pid = blocked_by(h, pid).await;
    let changing = disable(operator, target.id);
    blocked_by(h, login_pid).await;
    lock.commit().await.unwrap();
    let (_, cookie) = attempt.await.unwrap().unwrap();
    changing.await.unwrap().unwrap();
    assert!(matches!(
        h.app.authenticate(&cookie).await,
        Err(Error::Unauthorized)
    ));
    remove_gate(h, "sessions").await;
    operator.enable_account(target.id, REASON).await.unwrap();

    // The ordinary write's shared account guard commits before disablement.
    let (actor, cookie) = login(h, &target).await;
    let (lock, pid) = gate(h, "issues", None).await;
    let writing = write(h, &actor, w, team, "Write before account disable");
    let writer_pid = blocked_by(h, pid).await;
    let changing = disable(operator, target.id);
    blocked_by(h, writer_pid).await;
    lock.commit().await.unwrap();
    let issue = writing.await.unwrap().unwrap();
    changing.await.unwrap().unwrap();
    assert!(h.app.get_issue(owner, w, issue.id).await.is_ok());
    revoked(h, &actor, &cookie).await;
    remove_gate(h, "issues").await;
    operator.enable_account(target.id, REASON).await.unwrap();

    // The opposite order rejects the cached actor before an issue can exist.
    let (actor, _) = login(h, &target).await;
    let (lock, pid) = gate(h, "account_security_events", Some("account.disabled")).await;
    let changing = disable(operator, target.id);
    let disabling_pid = blocked_by(h, pid).await;
    let writing = write(h, &actor, w, team, "Write after account disable");
    blocked_by(h, disabling_pid).await;
    lock.commit().await.unwrap();
    changing.await.unwrap().unwrap();
    assert!(matches!(writing.await.unwrap(), Err(Error::Unauthorized)));
    remove_gate(h, "account_security_events").await;
    let mut tx = fixture(h, owner.user.id, w).await;
    let counts:(i64,i64)=sqlx::query_as("SELECT count(*) FILTER(WHERE title='Write before account disable'),count(*) FILTER(WHERE title='Write after account disable') FROM issues WHERE workspace_id=$1")
        .bind(w).fetch_one(&mut *tx).await.unwrap();
    assert_eq!(counts, (1, 0));
    tx.commit().await.unwrap();
}

async fn concurrent_last_owners(
    h: &Harness,
    operator: &Operator,
    owner: &Actor,
    organization: Uuid,
) {
    let w = workspace(h, owner, organization, "concurrent-owner-guard").await;
    let a = account(h, owner.user.id, w, "owner").await;
    let b = account(h, owner.user.id, w, "owner").await;
    membership(h, owner.user.id, w, owner.user.id, "admin", true).await;
    // Independent pools are essential: both commands must really reach the DB,
    // rather than being serialized by Operator's one-connection pool.
    let second = Operator::connect(&h.operator_url().await).await.unwrap();
    let (lock, pid) = gate(h, "account_security_events", Some("account.disabled")).await;
    let first = disable(operator, a.id);
    let first_pid = blocked_by(h, pid).await;
    let other = disable(&second, b.id);
    blocked_by(h, first_pid).await;
    lock.commit().await.unwrap();
    assert!(first.await.unwrap().unwrap().changed);
    assert!(matches!(other.await.unwrap(), Err(Error::Conflict(_))));
    remove_gate(h, "account_security_events").await;
    let mut tx = fixture(h, owner.user.id, w).await;
    let enabled:i64=sqlx::query_scalar("SELECT count(*) FROM workspace_members m JOIN users u ON u.id=m.user_id WHERE m.workspace_id=$1 AND m.role='owner' AND m.active AND u.disabled_at IS NULL").bind(w).fetch_one(&mut *tx).await.unwrap();
    assert_eq!(enabled, 1);
    tx.commit().await.unwrap();

    // Repeat with an organization having only these two owners. Organization
    // guards precede workspace guards, so its last-enabled-owner invariant must
    // also survive two independently connected operator commands.
    operator.enable_account(a.id, REASON).await.unwrap();
    let organization = Uuid::new_v4();
    let mut tx = fixture(h, a.id, w).await;
    sqlx::query("INSERT INTO organizations(id,name,created_by) VALUES($1,'Concurrent organization owners',$2)").bind(organization).bind(a.id).execute(&mut *tx).await.unwrap();
    sqlx::query(
        "INSERT INTO organization_members(organization_id,user_id,role) VALUES($1,$2,'owner')",
    )
    .bind(organization)
    .bind(a.id)
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    organization_owner(h, organization, b.id).await;
    let (lock, pid) = gate(h, "account_security_events", Some("account.disabled")).await;
    let first = disable(operator, a.id);
    let first_pid = blocked_by(h, pid).await;
    let other = disable(&second, b.id);
    blocked_by(h, first_pid).await;
    lock.commit().await.unwrap();
    first.await.unwrap().unwrap();
    assert!(matches!(other.await.unwrap(), Err(Error::Conflict(_))));
    remove_gate(h, "account_security_events").await;
    let enabled:i64=sqlx::query_scalar("SELECT count(*) FROM organization_members m JOIN users u ON u.id=m.user_id WHERE m.organization_id=$1 AND m.role='owner' AND u.disabled_at IS NULL").bind(organization).fetch_one(&h.owner).await.unwrap();
    assert_eq!(enabled, 1);
}
