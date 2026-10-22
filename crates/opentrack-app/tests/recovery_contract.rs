//! Real PostgreSQL recovery capabilities, rollback, and authorization ordering.
#[allow(dead_code)]
mod support;

use opentrack_app::{App, Operator, generate_recovery_code};
use opentrack_domain::*;
use opentrack_store as store;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{AssertSqlSafe, postgres::PgPoolOptions};
use support::Harness;
use uuid::Uuid;

const EMAIL: &str = "recovery@contract.invalid";
const PASSWORD: &str = "initial recovery password 309!";
const NEW_PASSWORD: &str = "replacement recovery password 713!";

fn req<T: DeserializeOwned>(value: Value) -> T {
    serde_json::from_value(value).unwrap()
}
fn complete(code: &str, password: &str) -> RecoveryComplete {
    req(json!({"token":code,"password":password}))
}
async fn login(h: &Harness, password: &str) -> (Actor, String) {
    let (_, cookie) = h
        .app
        .login(req(json!({"email":EMAIL,"password":password})))
        .await
        .unwrap();
    (h.app.authenticate(&cookie).await.unwrap(), cookie)
}
async fn fixture(h: &Harness, user: Uuid, w: Option<Uuid>) -> store::Tx {
    let mut tx = store::begin(&h.owner).await.unwrap();
    sqlx::query("SELECT set_config('opentrack.user_id',$1,true),set_config('opentrack.workspace_id',$2,true)")
        .bind(user.to_string()).bind(w.map(|v|v.to_string()).unwrap_or_default()).execute(&mut *tx).await.unwrap();
    tx
}
async fn issue(operator: &Operator, user: Uuid) -> (RecoveryReceipt, String) {
    let code = generate_recovery_code(user);
    let receipt = operator
        .issue_recovery(
            user,
            Uuid::new_v4(),
            &code,
            30,
            "Verified local recovery request",
        )
        .await
        .unwrap();
    (receipt, code)
}
fn invalid(result: Result<()>) {
    let error = result.unwrap_err();
    assert!(matches!(error, Error::InvalidRecovery));
    assert_eq!(error.to_string(), Error::InvalidRecovery.to_string());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_postgres_recovery_capability_and_atomicity_contracts() {
    let h = Harness::new().await;
    let runner = h.clone();
    let result = tokio::spawn(async move { exercise(&runner).await }).await;
    h.cleanup().await;
    if let Err(error) = result {
        std::panic::resume_unwind(error.into_panic());
    }
}

async fn exercise(h: &Harness) {
    let (session,cookie)=h.app.setup(req(json!({"setup_token":h.setup_token,"email":EMAIL,"password":PASSWORD,
        "display_name":"Recovery owner","workspace_name":"Recovery workspace","team_name":"Engineering","team_key":"ENG"}))).await.unwrap();
    let actor = h.app.authenticate(&cookie).await.unwrap();
    let user = actor.user.id;
    let w = session.workspaces[0].id;
    let team = h.app.bootstrap(&actor, w).await.unwrap().teams[0].id;
    let operator_url = h.operator_url().await;
    let operator = Operator::connect(&operator_url)
        .await
        .expect("The restricted capability-only operator connects");
    privilege_boundaries(h, &operator_url, user).await;
    invalid(
        h.app
            .complete_recovery(complete("invalid", NEW_PASSWORD))
            .await,
    );
    invalid(
        h.app
            .complete_recovery(complete(
                &generate_recovery_code(Uuid::new_v4()),
                NEW_PASSWORD,
            ))
            .await,
    );
    invalid(
        h.app
            .complete_recovery(complete(&generate_recovery_code(user), NEW_PASSWORD))
            .await,
    );
    assert!(matches!(
        operator
            .issue_recovery(
                Uuid::new_v4(),
                Uuid::new_v4(),
                &generate_recovery_code(Uuid::new_v4()),
                30,
                "missing"
            )
            .await,
        Err(Error::Invalid(_))
    ));
    let (first, first_code) = issue(&operator, user).await;
    let (replacement, replacement_code) = issue(&operator, user).await;
    invalid(
        h.app
            .complete_recovery(complete(&first_code, NEW_PASSWORD))
            .await,
    );
    let mut tx = fixture(h, user, None).await;
    let stored: (String, bool) =
        sqlx::query_as("SELECT token_hash,revoked_at IS NOT NULL FROM recovery_tokens WHERE id=$1")
            .bind(first.id)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    assert_eq!(
        stored.0,
        format!("{:x}", Sha256::digest(first_code.as_bytes()))
    );
    assert!(stored.1);
    sqlx::query("UPDATE recovery_tokens SET created_at=clock_timestamp()-interval '2 hours',expires_at=clock_timestamp()-interval '1 hour' WHERE id=$1")
        .bind(replacement.id).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    invalid(
        h.app
            .complete_recovery(complete(&replacement_code, NEW_PASSWORD))
            .await,
    );
    let (_, revoked) = issue(&operator, user).await;
    assert_eq!(
        operator
            .revoke_recovery(user, "Cancel the handed-out code")
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        operator
            .revoke_recovery(user, "Confirm no pending code remains")
            .await
            .unwrap(),
        0
    );
    invalid(
        h.app
            .complete_recovery(complete(&revoked, NEW_PASSWORD))
            .await,
    );
    disabled_account(h, &operator, user).await;
    let (_, code) = issue(&operator, user).await;
    runtime_cannot_issue_or_forge(h, user, &code).await;
    audit_failure_rollback(h, &operator, &actor, &cookie, &code).await;
    h.app
        .complete_recovery(complete(&code, NEW_PASSWORD))
        .await
        .unwrap();
    invalid(
        h.app
            .complete_recovery(complete(&code, "another replacement password 901!"))
            .await,
    );
    assert!(matches!(
        h.app.authenticate(&cookie).await,
        Err(Error::Unauthorized)
    ));
    assert!(matches!(
        h.app.session(&actor).await,
        Err(Error::Unauthorized)
    ));
    assert!(matches!(
        h.app
            .login(req(json!({"email":EMAIL,"password":PASSWORD})))
            .await,
        Err(Error::Unauthorized)
    ));
    let (fresh, _) = login(h, NEW_PASSWORD).await;
    assert_eq!(
        h.app
            .session(&fresh)
            .await
            .unwrap()
            .workspaces
            .iter()
            .map(|w| w.id)
            .collect::<Vec<_>>(),
        vec![w],
        "Recovery changes authentication, not membership"
    );
    search_path_is_fixed(h, &operator_url, user).await;
    recovery_races(h, &operator, user, w, team).await;
    audit_projection(
        h,
        user,
        &operator_url,
        &[
            &first_code,
            &replacement_code,
            &revoked,
            &code,
            PASSWORD,
            NEW_PASSWORD,
        ],
    )
    .await;
}

async fn privilege_boundaries(h: &Harness, operator_url: &str, user: Uuid) {
    let operator_pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(operator_url)
        .await
        .unwrap();
    for statement in [
        "SELECT * FROM users",
        "SELECT * FROM recovery_tokens",
        "UPDATE sessions SET revoked_at=clock_timestamp()",
        "CREATE TEMP TABLE recovery_probe(id integer)",
    ] {
        let error = sqlx::query(statement)
            .execute(&operator_pool)
            .await
            .unwrap_err();
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
    assert!(matches!(
        Operator::connect(runtime.as_str()).await,
        Err(Error::Forbidden)
    ));
    let role = runtime.username().replace('"', "\"\"");
    sqlx::query(AssertSqlSafe(format!("GRANT EXECUTE ON FUNCTION {schema}.operator_issue_recovery(uuid,uuid,text,integer,text) TO \"{role}\""))).execute(&h.owner).await.unwrap();
    assert!(
        App::connect(runtime.as_str(), &h.setup_token, 1, 1)
            .await
            .is_err(),
        "Serving preflight must refuse direct operator capability drift"
    );
    sqlx::query(AssertSqlSafe(format!("REVOKE EXECUTE ON FUNCTION {schema}.operator_issue_recovery(uuid,uuid,text,integer,text) FROM \"{role}\""))).execute(&h.owner).await.unwrap();
    let operator_role = url::Url::parse(operator_url)
        .unwrap()
        .username()
        .to_owned()
        .replace('"', "\"\"");
    sqlx::query(AssertSqlSafe(format!(
        "GRANT SELECT ON {schema}.users TO \"{operator_role}\""
    )))
    .execute(&h.owner)
    .await
    .unwrap();
    assert!(
        matches!(Operator::connect(operator_url).await, Err(Error::Forbidden)),
        "Operator must refuse even a read grant on application tables"
    );
    sqlx::query(AssertSqlSafe(format!(
        "REVOKE SELECT ON {schema}.users FROM \"{operator_role}\""
    )))
    .execute(&h.owner)
    .await
    .unwrap();
    let mut tx = store::begin(&h.app.pool).await.unwrap();
    store::context(&mut tx, user, None).await.unwrap();
    let error = sqlx::query("SELECT operator_revoke_recovery($1,'forbidden runtime request')")
        .bind(user)
        .execute(&mut *tx)
        .await
        .unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("42501")
    );
    tx.rollback().await.unwrap();
    operator_pool.close().await;
}

async fn disabled_account(h: &Harness, operator: &Operator, owner: Uuid) {
    // A disabled nonowner models a legacy disabled account without violating
    // the database's enabled-organization/workspace-owner invariant.
    let user = Uuid::new_v4();
    sqlx::query("INSERT INTO users(id,email,display_name,password_hash) SELECT $1,$2,'Disabled recovery target',password_hash FROM users WHERE id=$3")
        .bind(user).bind(format!("{user}@recovery.invalid")).bind(owner).execute(&h.owner).await.unwrap();
    let (_, code) = issue(operator, user).await;
    let mut tx = fixture(h, user, None).await;
    sqlx::query("UPDATE users SET disabled_at=clock_timestamp() WHERE id=$1")
        .bind(user)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    invalid(h.app.complete_recovery(complete(&code, NEW_PASSWORD)).await);
    assert!(matches!(
        operator
            .issue_recovery(
                user,
                Uuid::new_v4(),
                &generate_recovery_code(user),
                30,
                "disabled account"
            )
            .await,
        Err(Error::NotFound)
    ));
    let mut tx = fixture(h, user, None).await;
    sqlx::query("UPDATE users SET disabled_at=NULL WHERE id=$1")
        .bind(user)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
}

async fn runtime_cannot_issue_or_forge(h: &Harness, user: Uuid, code: &str) {
    let mut tx = fixture(h, user, None).await;
    let receipt:Uuid=sqlx::query_scalar("SELECT id FROM recovery_tokens WHERE user_id=$1 AND used_at IS NULL AND revoked_at IS NULL").bind(user).fetch_one(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    let mut tx = store::begin(&h.app.pool).await.unwrap();
    store::context(&mut tx, user, None).await.unwrap();
    let error=sqlx::query("INSERT INTO recovery_tokens(id,user_id,token_hash,expires_at) VALUES($1,$2,$3,clock_timestamp()+interval '30 minutes')")
        .bind(Uuid::new_v4()).bind(user).bind(format!("{:x}",Sha256::digest(b"fake issuance"))).execute(&mut *tx).await.unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("42501")
    );
    tx.rollback().await.unwrap();
    let mut tx = store::begin(&h.app.pool).await.unwrap();
    store::context(&mut tx, user, None).await.unwrap();
    let error=sqlx::query("INSERT INTO account_security_events(user_id,action,actor_kind,recovery_id,operator_identity,reason) VALUES($1,'recovery.issued','local_operator',$2,'forged operator','forged reason')")
        .bind(user).bind(receipt).execute(&mut *tx).await.unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("42501")
    );
    tx.rollback().await.unwrap();
    let mut tx = store::begin(&h.app.pool).await.unwrap();
    store::context(&mut tx, user, None).await.unwrap();
    let error = sqlx::query("UPDATE recovery_tokens SET token_hash=$2 WHERE id=$1")
        .bind(receipt)
        .bind(format!("{:x}", Sha256::digest(code.as_bytes())) + "0")
        .execute(&mut *tx)
        .await
        .unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("42501")
    );
    tx.rollback().await.unwrap();
}

async fn audit_failure_rollback(
    h: &Harness,
    operator: &Operator,
    actor: &Actor,
    cookie: &str,
    code: &str,
) {
    sqlx::raw_sql("CREATE FUNCTION test_reject_recovery_audit() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.action=TG_ARGV[0] THEN RAISE EXCEPTION 'Controlled audit failure'; END IF; RETURN NEW; END $$; CREATE TRIGGER test_recovery_audit BEFORE INSERT ON account_security_events FOR EACH ROW EXECUTE FUNCTION test_reject_recovery_audit('recovery.issued');").execute(&h.owner).await.unwrap();
    let failed_id = Uuid::new_v4();
    let failed_code = generate_recovery_code(actor.user.id);
    assert!(
        operator
            .issue_recovery(
                actor.user.id,
                failed_id,
                &failed_code,
                30,
                "audit unavailable"
            )
            .await
            .is_err()
    );
    let mut tx = fixture(h, actor.user.id, None).await;
    let failed: i64 = sqlx::query_scalar("SELECT count(*) FROM recovery_tokens WHERE id=$1")
        .bind(failed_id)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert_eq!(failed, 0);
    let pending:i64=sqlx::query_scalar("SELECT count(*) FROM recovery_tokens WHERE user_id=$1 AND token_hash=$2 AND used_at IS NULL AND revoked_at IS NULL").bind(actor.user.id).bind(format!("{:x}",Sha256::digest(code.as_bytes()))).fetch_one(&mut *tx).await.unwrap();
    assert_eq!(pending, 1);
    let password_before: String = sqlx::query_scalar("SELECT password_hash FROM users WHERE id=$1")
        .bind(actor.user.id)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    sqlx::raw_sql("DROP TRIGGER test_recovery_audit ON account_security_events; CREATE TRIGGER test_recovery_audit BEFORE INSERT ON account_security_events FOR EACH ROW EXECUTE FUNCTION test_reject_recovery_audit('recovery.completed');").execute(&h.owner).await.unwrap();
    assert!(
        h.app
            .complete_recovery(complete(code, NEW_PASSWORD))
            .await
            .is_err()
    );
    let mut tx = fixture(h, actor.user.id, None).await;
    let unchanged: bool = sqlx::query_scalar("SELECT password_hash=$2 FROM users WHERE id=$1")
        .bind(actor.user.id)
        .bind(password_before)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert!(
        unchanged,
        "Password update rolls back when mandatory audit fails"
    );
    let pending: bool = sqlx::query_scalar(
        "SELECT used_at IS NULL AND revoked_at IS NULL FROM recovery_tokens WHERE token_hash=$1",
    )
    .bind(format!("{:x}", Sha256::digest(code.as_bytes())))
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert!(pending);
    tx.commit().await.unwrap();
    assert!(
        h.app.authenticate(cookie).await.is_ok(),
        "Audit failure cannot leave sessions revoked"
    );
    sqlx::raw_sql("DROP TRIGGER test_recovery_audit ON account_security_events; DROP FUNCTION test_reject_recovery_audit();").execute(&h.owner).await.unwrap();
}

async fn search_path_is_fixed(h: &Harness, operator_url: &str, user: Uuid) {
    let schema: String = sqlx::query_scalar("SELECT current_schema()")
        .fetch_one(&h.owner)
        .await
        .unwrap();
    let config:Vec<Vec<String>>=sqlx::query_scalar("SELECT proconfig FROM pg_proc WHERE pronamespace=current_schema()::regnamespace AND proname IN ('operator_issue_recovery','operator_revoke_recovery')").fetch_all(&h.owner).await.unwrap();
    assert_eq!(config.len(), 2);
    for values in config {
        assert!(
            values
                .iter()
                .any(|v| v == &format!("search_path=pg_catalog, {schema}, pg_temp")),
            "Definer function path is frozen to its migration schema"
        );
    }
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(operator_url)
        .await
        .unwrap();
    let mut tx = store::begin(&pool).await.unwrap();
    sqlx::query(AssertSqlSafe(format!(
        "SET LOCAL search_path=pg_temp,pg_catalog,public,{schema}"
    )))
    .execute(&mut *tx)
    .await
    .unwrap();
    let code = generate_recovery_code(user);
    sqlx::query(AssertSqlSafe(format!(
        "SELECT {schema}.operator_issue_recovery($1,$2,$3,30,'Hostile caller search path')"
    )))
    .bind(user)
    .bind(Uuid::new_v4())
    .bind(format!("{:x}", Sha256::digest(code.as_bytes())))
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    pool.close().await;
}

// A fixture-only gate extends timeouts BEFORE the blocking statement, allowing
// real Argon2 work in the competing request. Production timeout behavior stays
// unchanged. Every ordering assertion observes a PostgreSQL blocking PID.
async fn install_gates(h: &Harness) {
    sqlx::raw_sql("CREATE FUNCTION test_extend_gate_timeout() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN PERFORM set_config('statement_timeout','15000',true); PERFORM set_config('lock_timeout','15000',true); IF TG_OP='DELETE' THEN RETURN OLD; ELSE RETURN NEW; END IF; END $$; CREATE TRIGGER test_recovery_wait_budget BEFORE UPDATE ON users FOR EACH ROW EXECUTE FUNCTION test_extend_gate_timeout(); CREATE TRIGGER test_issue_wait_budget BEFORE INSERT ON idempotency_records FOR EACH ROW EXECUTE FUNCTION test_extend_gate_timeout(); CREATE TRIGGER test_login_wait_budget BEFORE DELETE ON login_attempts FOR EACH ROW EXECUTE FUNCTION test_extend_gate_timeout(); CREATE FUNCTION test_wait_recovery_gate() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF TG_NARGS=1 OR to_jsonb(NEW)->>'action'=TG_ARGV[1] THEN PERFORM pg_advisory_xact_lock(TG_ARGV[0]::bigint); END IF; RETURN NEW; END $$;").execute(&h.owner).await.unwrap();
}
async fn gate(h: &Harness, table: &str, action: Option<&str>) -> (store::Tx, i32) {
    let key = i64::from_be_bytes(Uuid::new_v4().as_bytes()[..8].try_into().unwrap());
    let arguments = action
        .map(|a| format!("'{key}','{a}'"))
        .unwrap_or_else(|| format!("'{key}'"));
    // All identifiers/action strings come from fixed test call sites; key is i64.
    sqlx::query(AssertSqlSafe(format!("CREATE TRIGGER test_gate BEFORE INSERT ON {table} FOR EACH ROW EXECUTE FUNCTION test_wait_recovery_gate({arguments})"))).execute(&h.owner).await.unwrap();
    let mut lock = store::begin(&h.owner).await.unwrap();
    let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
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
            let found: Option<i32> = sqlx::query_scalar(
                "SELECT pid FROM pg_stat_activity WHERE $1=ANY(pg_blocking_pids(pid)) LIMIT 1",
            )
            .bind(blocker)
            .fetch_optional(&h.owner)
            .await
            .unwrap();
            if let Some(pid) = found {
                return pid;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("Competing operation reaches an observed database lock wait")
}
async fn recovery_races(h: &Harness, operator: &Operator, user: Uuid, w: Uuid, team: Uuid) {
    install_gates(h).await;
    // Password recovery wins against a login that verified the old committed hash.
    let (_, code) = issue(operator, user).await;
    let (lock, pid) = gate(h, "account_security_events", Some("recovery.completed")).await;
    let app = h.app.clone();
    let recovery = tokio::spawn(async move {
        app.complete_recovery(complete(&code, "race replacement password one 582!"))
            .await
    });
    let recovery_pid = blocked_by(h, pid).await;
    let app = h.app.clone();
    let login = tokio::spawn(async move {
        app.login(req(json!({"email":EMAIL,"password":NEW_PASSWORD})))
            .await
    });
    blocked_by(h, recovery_pid).await;
    lock.commit().await.unwrap();
    recovery.await.unwrap().unwrap();
    assert!(
        matches!(login.await.unwrap(), Err(Error::Unauthorized)),
        "Password must be rechecked after the recovery guard commits"
    );
    remove_gate(h, "account_security_events").await;
    // A login ordered first is immediately revoked by the later recovery.
    let (_, code) = issue(operator, user).await;
    let (lock, pid) = gate(h, "sessions", None).await;
    let app = h.app.clone();
    let login = tokio::spawn(async move {
        app.login(req(
            json!({"email":EMAIL,"password":"race replacement password one 582!"}),
        ))
        .await
    });
    let login_pid = blocked_by(h, pid).await;
    let app = h.app.clone();
    let recovery = tokio::spawn(async move {
        app.complete_recovery(complete(&code, "race replacement password two 583!"))
            .await
    });
    blocked_by(h, login_pid).await;
    lock.commit().await.unwrap();
    let (_, old_cookie) = login.await.unwrap().unwrap();
    recovery.await.unwrap().unwrap();
    assert!(matches!(
        h.app.authenticate(&old_cookie).await,
        Err(Error::Unauthorized)
    ));
    remove_gate(h, "sessions").await;
    // A workspace write that already holds the account guard commits first.
    let (actor, _) = login_user(h, "race replacement password two 583!").await;
    let (_, code) = issue(operator, user).await;
    let (lock, pid) = gate(h, "issues", None).await;
    let app = h.app.clone();
    let writer = actor.clone();
    let write = tokio::spawn(async move {
        app.create_issue(
            &writer,
            w,
            "recovery-write-first",
            req(json!({"team_id":team,"title":"Write before recovery"})),
        )
        .await
    });
    let writer_pid = blocked_by(h, pid).await;
    let app = h.app.clone();
    let recovery = tokio::spawn(async move {
        app.complete_recovery(complete(&code, "race replacement password three 584!"))
            .await
    });
    blocked_by(h, writer_pid).await;
    lock.commit().await.unwrap();
    let committed = write.await.unwrap().unwrap();
    recovery.await.unwrap().unwrap();
    remove_gate(h, "issues").await;
    assert!(matches!(
        h.app.get_issue(&actor, w, committed.id).await,
        Err(Error::Unauthorized)
    ));
    // Recovery ordered first rejects a queued cached actor without an issue row.
    let (actor, _) = login_user(h, "race replacement password three 584!").await;
    let (_, code) = issue(operator, user).await;
    let (lock, pid) = gate(h, "account_security_events", Some("recovery.completed")).await;
    let app = h.app.clone();
    let recovery = tokio::spawn(async move {
        app.complete_recovery(complete(&code, "race replacement password four 585!"))
            .await
    });
    let recovery_pid = blocked_by(h, pid).await;
    let app = h.app.clone();
    let write = tokio::spawn(async move {
        app.create_issue(
            &actor,
            w,
            "recovery-wins-write",
            req(json!({"team_id":team,"title":"Write after recovery"})),
        )
        .await
    });
    blocked_by(h, recovery_pid).await;
    lock.commit().await.unwrap();
    recovery.await.unwrap().unwrap();
    assert!(matches!(write.await.unwrap(), Err(Error::Unauthorized)));
    remove_gate(h, "account_security_events").await;
    let mut tx = fixture(h, user, Some(w)).await;
    let counts: (i64, i64) = sqlx::query_as(
        "SELECT count(*) FILTER (WHERE title='Write before recovery'),
                count(*) FILTER (WHERE title='Write after recovery') FROM issues",
    )
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert_eq!(counts, (1, 0));
    tx.commit().await.unwrap();
    simultaneous_code_consumption(h, operator, user).await;
}

async fn simultaneous_code_consumption(h: &Harness, operator: &Operator, user: Uuid) {
    let (receipt, code) = issue(operator, user).await;
    let key = i64::from_be_bytes(Uuid::new_v4().as_bytes()[..8].try_into().unwrap());
    // A restrictive policy only delays reads; the production ownership predicate
    // still applies. Hold both preliminary checks here before either can hash.
    sqlx::raw_sql(AssertSqlSafe(format!(
        "CREATE FUNCTION test_recovery_read_gate() RETURNS boolean LANGUAGE plpgsql AS $$
         BEGIN PERFORM pg_advisory_xact_lock_shared({key}); RETURN true; END $$;
         CREATE POLICY test_recovery_read_gate ON recovery_tokens AS RESTRICTIVE
         FOR SELECT USING(test_recovery_read_gate());"
    )))
    .execute(&h.owner)
    .await
    .unwrap();
    let mut lock = fixture(h, user, None).await;
    sqlx::query("SELECT id FROM users WHERE id=$1 FOR SHARE")
        .bind(user)
        .fetch_one(&mut *lock)
        .await
        .unwrap();
    let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *lock)
        .await
        .unwrap();
    // A session advisory lock can be released while retaining the user row lock.
    // Tx closes its connection on an uncommitted drop, also releasing this lock.
    sqlx::query("SELECT pg_advisory_lock($1)")
        .bind(key)
        .execute(&mut *lock)
        .await
        .unwrap();
    let passwords = [
        "simultaneous recovery winner candidate A 719!",
        "simultaneous recovery winner candidate B 719!",
    ];
    let app = h.app.clone();
    let first_code = code.clone();
    let first = tokio::spawn(async move {
        app.complete_recovery(complete(&first_code, passwords[0]))
            .await
    });
    let app = h.app.clone();
    let second =
        tokio::spawn(async move { app.complete_recovery(complete(&code, passwords[1])).await });
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let count: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM pg_stat_activity WHERE $1=ANY(pg_blocking_pids(pid))",
            )
            .bind(pid)
            .fetch_one(&h.owner)
            .await
            .unwrap();
            if count == 2 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("Both reset requests must reach their preliminary token read");
    let unlocked: bool = sqlx::query_scalar("SELECT pg_advisory_unlock($1)")
        .bind(key)
        .fetch_one(&mut *lock)
        .await
        .unwrap();
    assert!(unlocked);
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            // FOR SHARE would use ShareLock. These two ExclusiveLock tuple
            // waiters prove both requests passed precheck AND finished Argon2,
            // reaching the final FOR NO KEY UPDATE guard before either commits.
            let count: i64 = sqlx::query_scalar(
                "SELECT count(DISTINCT pid) FROM pg_locks
                 WHERE relation='users'::regclass AND locktype='tuple'
                 AND mode='ExclusiveLock' AND cardinality(pg_blocking_pids(pid))>0",
            )
            .fetch_one(&h.owner)
            .await
            .unwrap();
            if count == 2 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("Both reset requests must wait at the final exclusive account guard");
    lock.commit().await.unwrap();
    let results = [first.await.unwrap(), second.await.unwrap()];
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    let winner = results.iter().position(Result::is_ok).unwrap();
    assert!(matches!(results[1 - winner], Err(Error::InvalidRecovery)));
    sqlx::raw_sql(
        "DROP POLICY test_recovery_read_gate ON recovery_tokens;
         DROP FUNCTION test_recovery_read_gate();",
    )
    .execute(&h.owner)
    .await
    .unwrap();
    let mut tx = fixture(h, user, None).await;
    let completed: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM account_security_events
         WHERE recovery_id=$1 AND action='recovery.completed'",
    )
    .bind(receipt.id)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert_eq!(completed, 1);
    tx.commit().await.unwrap();
    assert!(matches!(
        h.app
            .login(req(json!({"email":EMAIL,"password":passwords[1 - winner]})))
            .await,
        Err(Error::Unauthorized)
    ));
    login(h, passwords[winner]).await;
}
async fn login_user(h: &Harness, password: &str) -> (Actor, String) {
    login(h, password).await
}

async fn audit_projection(h: &Harness, user: Uuid, operator_url: &str, secrets: &[&str]) {
    let identity = url::Url::parse(operator_url).unwrap().username().to_owned();
    let mut tx = fixture(h, user, None).await;
    let rows:Vec<Value>=sqlx::query_scalar("SELECT to_jsonb(e) FROM account_security_events e WHERE action LIKE 'recovery.%' ORDER BY created_at,id").fetch_all(&mut *tx).await.unwrap();
    assert!(!rows.is_empty());
    for row in &rows {
        let text = row.to_string();
        for secret in secrets {
            assert!(!text.contains(secret));
        }
        if row["actor_kind"] == "local_operator" {
            assert_eq!(row["operator_identity"], identity);
            assert!(row["reason"].is_string());
        }
        Uuid::parse_str(row["correlation_id"].as_str().unwrap()).unwrap();
    }
    assert!(rows.iter().any(|row| row["action"] == "recovery.completed"));
    tx.commit().await.unwrap();
    let hidden: i64 = sqlx::query_scalar("SELECT count(*) FROM recovery_tokens")
        .fetch_one(&h.app.pool)
        .await
        .unwrap();
    assert_eq!(hidden, 0);
}
