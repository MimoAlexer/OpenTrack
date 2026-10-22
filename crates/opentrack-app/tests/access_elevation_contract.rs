//! Privilege increases invalidate old credentials in the same authorization order
//! as ordinary work. All waits below observe real PostgreSQL lock dependencies.
#[allow(dead_code)]
mod support;

use opentrack_app::AccessMutation;
use opentrack_domain::*;
use opentrack_store as store;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use sqlx::AssertSqlSafe;
use support::Harness;
use uuid::Uuid;

const PASSWORD: &str = "access elevation contract password 818!";
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
async fn account(h: &Harness, owner: &Actor, w: Uuid, role: &str) -> Account {
    let id = Uuid::new_v4();
    let email = format!("{id}@elevation.invalid");
    let mut tx = fixture(h, owner.user.id, w).await;
    // Reuse this fixture's valid Argon2 hash. Password verification still runs
    // through the real login implementation for every resulting session.
    sqlx::query("INSERT INTO users(id,email,display_name,password_hash) SELECT $1,$2,'Access target',password_hash FROM users WHERE id=$3")
        .bind(id).bind(&email).bind(owner.user.id).execute(&mut *tx).await.unwrap();
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
async fn login(h: &Harness, user: &Account) -> (Actor, String) {
    let (_, cookie) = h
        .app
        .login(req(json!({"email":user.email,"password":PASSWORD})))
        .await
        .unwrap();
    (h.app.authenticate(&cookie).await.unwrap(), cookie)
}
async fn member(
    h: &Harness,
    owner: &Actor,
    w: Uuid,
    target: Uuid,
    patch: Value,
) -> AccessMutation<WorkspaceMember> {
    let old = h.app.get_workspace_member(owner, w, target).await.unwrap();
    h.app
        .update_member(owner, w, target, &old.etag(w), req(patch))
        .await
        .unwrap()
}
async fn grant(
    h: &Harness,
    owner: &Actor,
    w: Uuid,
    t: Uuid,
    target: Uuid,
    role: &str,
) -> AccessMutation<TeamMember> {
    let old = h
        .app
        .team_members(owner, w, t, DirectoryQuery::default())
        .await
        .unwrap()
        .data
        .into_iter()
        .find(|m| m.user.id == target);
    let etag = old.map(|m| m.etag(t));
    h.app
        .put_team_member(
            owner,
            w,
            t,
            target,
            etag.as_deref(),
            req(json!({"role":role})),
        )
        .await
        .unwrap()
}
async fn team(h: &Harness, owner: &Actor, w: Uuid, private: bool) -> TeamDetail {
    let key = format!("T{}", &Uuid::new_v4().simple().to_string()[..5]).to_uppercase();
    h.app
        .create_team(
            owner,
            w,
            &Uuid::new_v4().to_string(),
            req(json!({"name":"Access test team","key":key,"private":private})),
        )
        .await
        .unwrap()
}
async fn revoked(h: &Harness, actor: &Actor, cookie: &str) {
    assert!(matches!(
        h.app.authenticate(cookie).await,
        Err(Error::Unauthorized)
    ));
    assert!(
        matches!(h.app.session(actor).await, Err(Error::Unauthorized)),
        "Cached actors must also lose authority"
    );
}
async fn alive(h: &Harness, actor: &Actor, cookie: &str) {
    assert!(h.app.authenticate(cookie).await.is_ok());
    assert!(h.app.session(actor).await.is_ok());
}
async fn snapshot(h: &Harness, target: Uuid, w: Uuid) -> (i64, i64, i64, i64) {
    let mut tx = fixture(h, target, w).await;
    let value=sqlx::query_as("SELECT (SELECT count(*) FROM sessions WHERE user_id=$1 AND revoked_at IS NOT NULL),(SELECT count(*) FROM account_security_events WHERE user_id=$1),(SELECT count(*) FROM audit_entries WHERE workspace_id=$2),(SELECT epoch FROM workspace_auth_guards WHERE workspace_id=$2)")
        .bind(target).bind(w).fetch_one(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    value
}
async fn linked_audit(h: &Harness, actor: Uuid, target: Uuid, w: Uuid, action: &str, minimum: i64) {
    let mut tx = fixture(h, target, w).await;
    let linked:i64=sqlx::query_scalar("SELECT count(*) FROM account_security_events s JOIN audit_entries a ON a.id=s.correlation_id JOIN workspace_events e ON e.mutation_id=a.id WHERE s.user_id=$1 AND s.action='session.revoked' AND a.workspace_id=$2 AND a.actor_id=$3 AND a.action=$4 AND e.actor_id=$3 AND e.event_type='access.changed'")
        .bind(target).bind(w).bind(actor).bind(action).fetch_one(&mut *tx).await.unwrap();
    assert!(
        linked >= minimum,
        "Revocation events must link to the actual actor's workspace audit and access event"
    );
    let records: Vec<Value> =
        sqlx::query_scalar("SELECT to_jsonb(s) FROM account_security_events s WHERE user_id=$1")
            .bind(target)
            .fetch_all(&mut *tx)
            .await
            .unwrap();
    let serialized = serde_json::to_string(&records).unwrap();
    for secret in [PASSWORD, "password_hash", "token_hash", "csrf_token"] {
        assert!(!serialized.contains(secret));
    }
    tx.commit().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_postgres_access_elevation_contracts() {
    let h = Harness::with_connections(6).await;
    let runner = h.clone();
    let result = tokio::spawn(async move { exercise(&runner).await }).await;
    h.cleanup().await;
    if let Err(error) = result {
        std::panic::resume_unwind(error.into_panic());
    }
}
async fn exercise(h: &Harness) {
    let (session,cookie)=h.app.setup(req(json!({"setup_token":h.setup_token,"email":"owner@elevation.invalid","password":PASSWORD,
        "display_name":"Access owner","workspace_name":"Access workspace","team_name":"Engineering","team_key":"ENG"}))).await.unwrap();
    let owner = h.app.authenticate(&cookie).await.unwrap();
    let w = session.workspaces[0].id;
    let public = h.app.bootstrap(&owner, w).await.unwrap().teams[0].id;
    let organization = h.app.organizations(&owner).await.unwrap()[0].id;
    let other_workspace=h.app.create_workspace(&owner,"access-other-workspace",req(json!({"organization_id":organization,"name":"Unrelated workspace","team_name":"Other","team_key":"OTH"}))).await.unwrap().id;
    let target = account(h, &owner, w, "viewer").await;
    let other = account(h, &owner, w, "member").await;
    let (other_actor, other_cookie) = login(h, &other).await;
    let mut tx = fixture(h, owner.user.id, other_workspace).await;
    sqlx::query("INSERT INTO workspace_members(workspace_id,user_id,role) VALUES($1,$2,'member')")
        .bind(other_workspace)
        .bind(target.id)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let (first, first_cookie) = login(h, &target).await;
    let (second, second_cookie) = login(h, &target).await;
    let changed = member(h, &owner, w, target.id, json!({"role":"member"})).await;
    assert_eq!(changed.entity.role, "member");
    assert!(!changed.current_session_revoked);
    revoked(h, &first, &first_cookie).await;
    revoked(h, &second, &second_cookie).await;
    assert!(
        matches!(
            h.app.bootstrap(&first, other_workspace).await,
            Err(Error::Unauthorized)
        ),
        "An old global cookie cannot inherit access in any workspace"
    );
    alive(h, &owner, &cookie).await;
    alive(h, &other_actor, &other_cookie).await;
    linked_audit(
        h,
        owner.user.id,
        target.id,
        w,
        "workspace.member.updated",
        2,
    )
    .await;

    let (fresh, fresh_cookie) = login(h, &target).await;
    let before = snapshot(h, target.id, w).await;
    member(
        h,
        &owner,
        w,
        target.id,
        json!({"role":"member","active":true}),
    )
    .await;
    alive(h, &fresh, &fresh_cookie).await;
    assert_eq!(snapshot(h, target.id, w).await.0, before.0);
    let lower = member(h, &owner, w, target.id, json!({"role":"viewer"})).await;
    assert!(!lower.current_session_revoked);
    alive(h, &fresh, &fresh_cookie).await;
    member(h, &owner, w, target.id, json!({"active":false})).await;
    alive(h, &fresh, &fresh_cookie).await;
    assert!(h.app.bootstrap(&fresh, other_workspace).await.is_ok());
    member(h, &owner, w, target.id, json!({"active":true})).await;
    revoked(h, &fresh, &fresh_cookie).await;

    team_capabilities(h, &owner, w, public).await;
    failure_atomicity(h, &owner, w, &target, &other_actor).await;
    invitation_rotates_every_cookie(h, &owner, w, other_workspace).await;
    lock_orderings(h, &owner, w, public).await;
    reciprocal_changes(h, &owner, w).await;

    // Self-elevation is a committed success that deliberately expires the current
    // cookie; the HTTP layer is responsible for its explicit success signal.
    let self_team = team(h, &owner, w, false).await;
    let mut tx = fixture(h, owner.user.id, w).await;
    sqlx::query("DELETE FROM team_members WHERE workspace_id=$1 AND team_id=$2 AND user_id=$3")
        .bind(w)
        .bind(self_team.id)
        .bind(owner.user.id)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let result = grant(h, &owner, w, self_team.id, owner.user.id, "maintainer").await;
    assert!(result.current_session_revoked);
    assert_eq!(result.entity.role, "maintainer");
    revoked(h, &owner, &cookie).await;
    // Runtime connections do not retain the target GUC after pooled reuse.
    let unscoped: i64 = sqlx::query_scalar("SELECT count(*) FROM account_security_events")
        .fetch_one(&h.app.pool)
        .await
        .unwrap();
    assert_eq!(unscoped, 0);
}

async fn team_capabilities(h: &Harness, owner: &Actor, w: Uuid, public: Uuid) {
    let target = account(h, owner, w, "member").await;
    let (actor, cookie) = login(h, &target).await;
    grant(h, owner, w, public, target.id, "contributor").await;
    alive(h, &actor, &cookie).await;
    let private = team(h, owner, w, true).await;
    assert!(
        !grant(h, owner, w, private.id, target.id, "contributor")
            .await
            .current_session_revoked
    );
    revoked(h, &actor, &cookie).await;
    let (actor, cookie) = login(h, &target).await;
    grant(h, owner, w, private.id, target.id, "contributor").await;
    alive(h, &actor, &cookie).await;
    grant(h, owner, w, public, target.id, "maintainer").await;
    revoked(h, &actor, &cookie).await;
    let (actor, cookie) = login(h, &target).await;
    grant(h, owner, w, public, target.id, "maintainer").await;
    grant(h, owner, w, public, target.id, "contributor").await;
    alive(h, &actor, &cookie).await;
    let second_public = team(h, owner, w, false).await;
    grant(h, owner, w, second_public.id, target.id, "maintainer").await;
    revoked(h, &actor, &cookie).await;
    let viewer = account(h, owner, w, "viewer").await;
    let (actor, cookie) = login(h, &viewer).await;
    grant(h, owner, w, private.id, viewer.id, "contributor").await;
    revoked(h, &actor, &cookie).await;
    linked_audit(h, owner.user.id, target.id, w, "team.member.updated", 3).await;
}

async fn failure_atomicity(
    h: &Harness,
    owner: &Actor,
    w: Uuid,
    target: &Account,
    unauthorized: &Actor,
) {
    let (actor, cookie) = login(h, target).await;
    let old = h
        .app
        .get_workspace_member(owner, w, target.id)
        .await
        .unwrap();
    let before = snapshot(h, target.id, w).await;
    assert!(matches!(
        h.app
            .update_member(
                unauthorized,
                w,
                target.id,
                &old.etag(w),
                req(json!({"role":"member"}))
            )
            .await,
        Err(Error::Forbidden)
    ));
    assert!(matches!(
        h.app
            .update_member(
                owner,
                w,
                target.id,
                "\"member:stale\"",
                req(json!({"role":"member"}))
            )
            .await,
        Err(Error::VersionConflict(_))
    ));
    let mut tx = fixture(h, owner.user.id, w).await;
    sqlx::query("UPDATE users SET disabled_at=clock_timestamp() WHERE id=$1")
        .bind(target.id)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert!(
        h.app
            .update_member(
                owner,
                w,
                target.id,
                &old.etag(w),
                req(json!({"role":"member"}))
            )
            .await
            .is_err()
    );
    let mut tx = fixture(h, owner.user.id, w).await;
    sqlx::query("UPDATE users SET disabled_at=NULL WHERE id=$1")
        .bind(target.id)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE sessions SET auth_time=clock_timestamp()-interval '11 minutes' WHERE token_hash=$1",
    )
    .bind(&owner.token_hash)
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    assert!(matches!(
        h.app
            .update_member(
                owner,
                w,
                target.id,
                &old.etag(w),
                req(json!({"role":"member"}))
            )
            .await,
        Err(Error::ReauthenticationRequired)
    ));
    let mut tx = fixture(h, owner.user.id, w).await;
    sqlx::query("UPDATE sessions SET auth_time=clock_timestamp() WHERE token_hash=$1")
        .bind(&owner.token_hash)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(snapshot(h, target.id, w).await, before);
    for table in ["account_security_events", "audit_entries"] {
        sqlx::raw_sql("CREATE FUNCTION test_reject_access_audit() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'Injected mandatory access audit failure'; END $$;").execute(&h.owner).await.unwrap();
        // Fixed table identifiers from this test, never caller SQL.
        sqlx::query(AssertSqlSafe(format!("CREATE TRIGGER test_access_audit BEFORE INSERT ON {table} FOR EACH ROW EXECUTE FUNCTION test_reject_access_audit()"))).execute(&h.owner).await.unwrap();
        assert!(
            h.app
                .update_member(
                    owner,
                    w,
                    target.id,
                    &old.etag(w),
                    req(json!({"role":"member"}))
                )
                .await
                .is_err()
        );
        sqlx::raw_sql(AssertSqlSafe(format!(
            "DROP TRIGGER test_access_audit ON {table}; DROP FUNCTION test_reject_access_audit();"
        )))
        .execute(&h.owner)
        .await
        .unwrap();
        assert_eq!(
            h.app
                .get_workspace_member(owner, w, target.id)
                .await
                .unwrap()
                .etag(w),
            old.etag(w)
        );
        assert_eq!(
            snapshot(h, target.id, w).await,
            before,
            "Membership, revoked sessions, security events, and access epoch are atomic"
        );
        alive(h, &actor, &cookie).await;
    }
}

async fn invitation_rotates_every_cookie(h: &Harness, owner: &Actor, w: Uuid, other_w: Uuid) {
    let target = account(h, owner, other_w, "member").await;
    let (first, first_cookie) = login(h, &target).await;
    let (second, second_cookie) = login(h, &target).await;
    let (third, third_cookie) = login(h, &target).await;
    let mut tx = fixture(h, target.id, other_w).await;
    let before:(chrono::DateTime<chrono::Utc>,chrono::DateTime<chrono::Utc>)=sqlx::query_as("UPDATE sessions SET auth_time=clock_timestamp()-interval '5 minutes',expires_at=clock_timestamp()+interval '2 hours' WHERE token_hash=$1 RETURNING auth_time,expires_at").bind(&first.token_hash).fetch_one(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    let invitation = h
        .app
        .create_invitation(
            owner,
            w,
            "elevation-existing-invite",
            req(json!({"email":target.email,"role":"member"})),
        )
        .await
        .unwrap();
    let (session, replacement) = h
        .app
        .accept_invitation(
            Some(&first),
            req(json!({"token":invitation.token.unwrap()})),
        )
        .await
        .unwrap();
    revoked(h, &first, &first_cookie).await;
    revoked(h, &second, &second_cookie).await;
    revoked(h, &third, &third_cookie).await;
    let fresh = h.app.authenticate(&replacement).await.unwrap();
    assert_ne!(session.csrf_token, first.csrf_token);
    let current = h
        .app
        .sessions(&fresh, SessionQuery::default())
        .await
        .unwrap();
    assert_eq!(current.data.len(), 1);
    assert!(current.data[0].current);
    assert_eq!(
        (current.data[0].auth_time, current.data[0].expires_at),
        before
    );
    linked_audit(h, target.id, target.id, w, "invitation.accepted", 2).await;
}

async fn install_gates(h: &Harness) {
    sqlx::raw_sql("CREATE FUNCTION test_extend_access_timeout() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN PERFORM set_config('statement_timeout','15000',true); PERFORM set_config('lock_timeout','15000',true); IF TG_OP='DELETE' THEN RETURN OLD; ELSE RETURN NEW; END IF; END $$; CREATE TRIGGER test_access_wait_budget BEFORE UPDATE ON workspace_members FOR EACH ROW EXECUTE FUNCTION test_extend_access_timeout(); CREATE TRIGGER test_issue_wait_budget BEFORE INSERT ON idempotency_records FOR EACH ROW EXECUTE FUNCTION test_extend_access_timeout(); CREATE TRIGGER test_login_wait_budget BEFORE DELETE ON login_attempts FOR EACH ROW EXECUTE FUNCTION test_extend_access_timeout(); CREATE FUNCTION test_wait_access_gate() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF TG_NARGS=1 OR to_jsonb(NEW)->>'action'=TG_ARGV[1] THEN PERFORM pg_advisory_xact_lock(TG_ARGV[0]::bigint); END IF; RETURN NEW; END $$;")
        .execute(&h.owner).await.unwrap();
}
async fn gate(h: &Harness, table: &str, action: Option<&str>) -> (store::Tx, i32) {
    let key = i64::from_be_bytes(Uuid::new_v4().as_bytes()[..8].try_into().unwrap());
    let arguments = action
        .map(|a| format!("'{key}','{a}'"))
        .unwrap_or_else(|| format!("'{key}'"));
    // Identifiers/actions are fixed test call sites; the generated key is i64.
    sqlx::query(AssertSqlSafe(format!("CREATE TRIGGER test_gate BEFORE INSERT ON {table} FOR EACH ROW EXECUTE FUNCTION test_wait_access_gate({arguments})"))).execute(&h.owner).await.unwrap();
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
    .expect("Competing request reached an observed PostgreSQL lock wait")
}
fn promote(
    h: &Harness,
    owner: &Actor,
    w: Uuid,
    target: Uuid,
    etag: String,
) -> tokio::task::JoinHandle<Result<AccessMutation<WorkspaceMember>>> {
    let app = h.app.clone();
    let actor = owner.clone();
    tokio::spawn(async move {
        app.update_member(&actor, w, target, &etag, req(json!({"role":"admin"})))
            .await
    })
}
async fn lock_orderings(h: &Harness, owner: &Actor, w: Uuid, team: Uuid) {
    install_gates(h).await;
    let target = account(h, owner, w, "member").await;
    let (original, original_cookie) = login(h, &target).await;
    let tag = h
        .app
        .get_workspace_member(owner, w, target.id)
        .await
        .unwrap()
        .etag(w);
    // Elevation commits before a waiting password login. That newly verified
    // login is permitted to mint a new session with the new access.
    let (lock, pid) = gate(h, "audit_entries", Some("workspace.member.updated")).await;
    let change = promote(h, owner, w, target.id, tag);
    let changing = blocked_by(h, pid).await;
    let app = h.app.clone();
    let email = target.email.clone();
    let signing_in = tokio::spawn(async move {
        app.login(req(json!({"email":email,"password":PASSWORD})))
            .await
    });
    blocked_by(h, changing).await;
    lock.commit().await.unwrap();
    change.await.unwrap().unwrap();
    let (session, new_cookie) = signing_in.await.unwrap().unwrap();
    assert_eq!(
        session.workspaces.iter().find(|x| x.id == w).unwrap().role,
        "admin"
    );
    assert!(h.app.authenticate(&new_cookie).await.is_ok());
    revoked(h, &original, &original_cookie).await;
    remove_gate(h, "audit_entries").await;

    // A login holding the account guard first is revoked by the later grant.
    let tag = member(h, owner, w, target.id, json!({"role":"member"}))
        .await
        .entity
        .etag(w);
    let (lock, pid) = gate(h, "sessions", None).await;
    let app = h.app.clone();
    let email = target.email.clone();
    let signing_in = tokio::spawn(async move {
        app.login(req(json!({"email":email,"password":PASSWORD})))
            .await
    });
    let signing = blocked_by(h, pid).await;
    let change = promote(h, owner, w, target.id, tag);
    blocked_by(h, signing).await;
    lock.commit().await.unwrap();
    let (_, old_cookie) = signing_in.await.unwrap().unwrap();
    change.await.unwrap().unwrap();
    assert!(matches!(
        h.app.authenticate(&old_cookie).await,
        Err(Error::Unauthorized)
    ));
    remove_gate(h, "sessions").await;

    // A write already authorized under the old session may commit first.
    let tag = member(h, owner, w, target.id, json!({"role":"member"}))
        .await
        .entity
        .etag(w);
    let (actor, cookie) = login(h, &target).await;
    let (lock, pid) = gate(h, "issues", None).await;
    let app = h.app.clone();
    let writer = actor.clone();
    let write = tokio::spawn(async move {
        app.create_issue(
            &writer,
            w,
            "before-elevation",
            req(json!({"team_id":team,"title":"Before elevation"})),
        )
        .await
    });
    let writing = blocked_by(h, pid).await;
    let change = promote(h, owner, w, target.id, tag);
    blocked_by(h, writing).await;
    lock.commit().await.unwrap();
    let issue = write.await.unwrap().unwrap();
    change.await.unwrap().unwrap();
    assert!(h.app.get_issue(owner, w, issue.id).await.is_ok());
    revoked(h, &actor, &cookie).await;
    remove_gate(h, "issues").await;

    // The opposite order rejects a queued cached actor before any issue row.
    let tag = member(h, owner, w, target.id, json!({"role":"member"}))
        .await
        .entity
        .etag(w);
    let (actor, _) = login(h, &target).await;
    let (lock, pid) = gate(h, "audit_entries", Some("workspace.member.updated")).await;
    let change = promote(h, owner, w, target.id, tag);
    let changing = blocked_by(h, pid).await;
    let app = h.app.clone();
    let write = tokio::spawn(async move {
        app.create_issue(
            &actor,
            w,
            "after-elevation",
            req(json!({"team_id":team,"title":"After elevation"})),
        )
        .await
    });
    blocked_by(h, changing).await;
    lock.commit().await.unwrap();
    change.await.unwrap().unwrap();
    assert!(matches!(write.await.unwrap(), Err(Error::Unauthorized)));
    remove_gate(h, "audit_entries").await;
    let mut tx = fixture(h, owner.user.id, w).await;
    let counts:(i64,i64)=sqlx::query_as("SELECT count(*) FILTER(WHERE title='Before elevation'),count(*) FILTER(WHERE title='After elevation') FROM issues WHERE workspace_id=$1")
        .bind(w).fetch_one(&mut *tx).await.unwrap();
    assert_eq!(counts, (1, 0));
    tx.commit().await.unwrap();
}

async fn reciprocal_changes(h: &Harness, owner: &Actor, w: Uuid) {
    let a = account(h, owner, w, "owner").await;
    let b = account(h, owner, w, "owner").await;
    let (actor_a, _) = login(h, &a).await;
    let (actor_b, _) = login(h, &b).await;
    let team = team(h, owner, w, false).await.id;
    let barrier = std::sync::Arc::new(tokio::sync::Barrier::new(3));
    let mut tasks = Vec::new();
    for (actor, target) in [(actor_a, b.id), (actor_b, a.id)] {
        let app = h.app.clone();
        let ready = barrier.clone();
        tasks.push(tokio::spawn(async move {
            ready.wait().await;
            app.put_team_member(
                &actor,
                w,
                team,
                target,
                None,
                req(json!({"role":"maintainer"})),
            )
            .await
        }));
    }
    barrier.wait().await;
    let mut successes = 0;
    let mut unauthorized = 0;
    for task in tasks {
        match tokio::time::timeout(std::time::Duration::from_secs(5), task)
            .await
            .expect("Reciprocal access changes must not deadlock")
            .unwrap()
        {
            Ok(result) => {
                assert!(!result.current_session_revoked);
                successes += 1;
            }
            Err(Error::Unauthorized) => unauthorized += 1,
            Err(error) => {
                panic!("Reciprocal grant failed outside the expected authorization order: {error}")
            }
        }
    }
    assert_eq!((successes, unauthorized), (1, 1));
}
