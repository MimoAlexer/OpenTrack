//! Invitation replacement and expiry must commit exactly their audited effects.
#[allow(dead_code)]
mod support;

use opentrack_domain::*;
use opentrack_store as store;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use sqlx::AssertSqlSafe;
use support::Harness;
use uuid::Uuid;

const PASSWORD: &str = "invitation integrity password 708!";
fn req<T: DeserializeOwned>(value: Value) -> T {
    serde_json::from_value(value).unwrap()
}
async fn fixture(h: &Harness, user: Uuid, w: Uuid) -> store::Tx {
    let mut tx = store::begin(&h.owner).await.unwrap();
    sqlx::query("SELECT set_config('opentrack.user_id',$1,true),set_config('opentrack.workspace_id',$2,true)").bind(user.to_string()).bind(w.to_string()).execute(&mut *tx).await.unwrap();
    tx
}
async fn account(h: &Harness, source: Uuid, w: Uuid, role: Option<&str>) -> (Actor, String) {
    let id = Uuid::new_v4();
    let email = format!("{id}@invitation.invalid");
    let mut tx = fixture(h, source, w).await;
    sqlx::query("INSERT INTO users(id,email,display_name,password_hash) SELECT $1,$2,'Invitation fixture',password_hash FROM users WHERE id=$3").bind(id).bind(&email).bind(source).execute(&mut *tx).await.unwrap();
    if let Some(role) = role {
        sqlx::query("INSERT INTO workspace_members(workspace_id,user_id,role) VALUES($1,$2,$3)")
            .bind(w)
            .bind(id)
            .bind(role)
            .execute(&mut *tx)
            .await
            .unwrap();
    }
    tx.commit().await.unwrap();
    let (_, cookie) = h
        .app
        .login(req(json!({"email":email,"password":PASSWORD})))
        .await
        .unwrap();
    (h.app.authenticate(&cookie).await.unwrap(), cookie)
}
fn body(email: &str, teams: &[Uuid]) -> CreateInvitation {
    req(json!({"email":email,"role":"member","team_ids":teams}))
}
async fn create(
    h: &Harness,
    actor: &Actor,
    w: Uuid,
    email: &str,
    teams: &[Uuid],
) -> InvitationIssued {
    h.app
        .create_invitation(actor, w, &Uuid::new_v4().to_string(), body(email, teams))
        .await
        .unwrap()
}
async fn expired(h: &Harness, actor: &Actor, w: Uuid, teams: &[Uuid]) -> InvitationIssued {
    let email = format!("{}@expired.invalid", Uuid::new_v4());
    let issued = create(h, actor, w, &email, teams).await;
    let mut tx = fixture(h, actor.user.id, w).await;
    sqlx::query(
        "UPDATE invitations SET expires_at=clock_timestamp()-interval '1 second' WHERE id=$1",
    )
    .bind(issued.invitation.id)
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    issued
}
async fn snapshot(h: &Harness, user: Uuid, w: Uuid) -> Value {
    let mut tx = fixture(h, user, w).await;
    let state:Value=sqlx::query_scalar("SELECT jsonb_build_object('invitations',(SELECT coalesce(jsonb_agg(to_jsonb(i) ORDER BY i.id),'[]') FROM invitations i WHERE workspace_id=$1),'grants',(SELECT coalesce(jsonb_agg(to_jsonb(i) ORDER BY i.invitation_id,i.team_id),'[]') FROM invitation_teams i WHERE workspace_id=$1),'audit',(SELECT count(*) FROM audit_entries WHERE workspace_id=$1),'events',(SELECT count(*) FROM workspace_events WHERE workspace_id=$1),'head',(SELECT last_seq FROM workspace_stream_heads WHERE workspace_id=$1),'epoch',(SELECT epoch FROM workspace_auth_guards WHERE workspace_id=$1),'retries',(SELECT coalesce(jsonb_agg(to_jsonb(i) ORDER BY i.principal_id,i.route,i.key_hash),'[]') FROM idempotency_records i WHERE workspace_id=$1))").bind(w).fetch_one(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    state
}
async fn blocked_by(h: &Harness, blocker: i32) -> i32 {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
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
    .expect("Observe the real PostgreSQL lock wait")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_postgres_invitation_integrity_contracts() {
    let h = Harness::with_connections(6).await;
    let runner = h.clone();
    let result = tokio::spawn(async move { exercise(&runner).await }).await;
    h.cleanup().await;
    if let Err(error) = result {
        std::panic::resume_unwind(error.into_panic());
    }
}
async fn exercise(h: &Harness) {
    let (session,cookie)=h.app.setup(req(json!({"setup_token":h.setup_token,"email":"owner@invitation.invalid","password":PASSWORD,"display_name":"Invitation owner","workspace_name":"Invitation integrity","team_name":"Engineering","team_key":"ENG"}))).await.unwrap();
    let owner = h.app.authenticate(&cookie).await.unwrap();
    let w = session.workspaces[0].id;
    let private = h
        .app
        .create_team(
            &owner,
            w,
            "invitation-private-team",
            req(json!({"name":"Private invitation audience","key":"PVT","private":true})),
        )
        .await
        .unwrap()
        .id;
    replacement(h, &owner, w).await;
    hidden_replacement(h, &owner, w, private).await;
    replacement_failures(h, &owner, w).await;
    concurrent_replacements(h, &owner, w, true).await;
    concurrent_replacements(h, &owner, w, false).await;
    acceptance_crosses_expiry(h, &owner, w, private).await;
    acceptance_crosses_consumption_expiry(h, &owner, w, private).await;
    acceptance_audit_failures(h, &owner, w, private).await;
}

async fn assert_replacement(
    h: &Harness,
    actor: &Actor,
    w: Uuid,
    old: &InvitationIssued,
    new: &InvitationIssued,
    before: &Value,
) {
    assert_ne!(old.invitation.id, new.invitation.id);
    assert!(new.token.is_some());
    let old_now = h
        .app
        .get_invitation(actor, w, old.invitation.id)
        .await
        .unwrap();
    assert!(old_now.revoked_at.is_some());
    assert!(old_now.accepted_at.is_none());
    assert_eq!(
        old_now.version.parse::<i64>().unwrap(),
        old.invitation.version.parse::<i64>().unwrap() + 1
    );
    assert!(matches!(
        h.app.invitation_preview(old.token.as_ref().unwrap()).await,
        Err(Error::NotFound)
    ));
    assert!(
        h.app
            .invitation_preview(new.token.as_ref().unwrap())
            .await
            .is_ok()
    );
    let after = snapshot(h, actor.user.id, w).await;
    assert_eq!(
        after["audit"].as_i64().unwrap(),
        before["audit"].as_i64().unwrap() + 2
    );
    assert_eq!(
        after["events"].as_i64().unwrap(),
        before["events"].as_i64().unwrap() + 1
    );
    assert_eq!(
        after["head"].as_i64().unwrap(),
        before["head"].as_i64().unwrap() + 1
    );
    assert_eq!(
        after["epoch"], before["epoch"],
        "Retiring an already expired invitation grants no access"
    );
    let mut tx = fixture(h, actor.user.id, w).await;
    let retired:(i64,Uuid)=sqlx::query_as("SELECT count(*) OVER(),actor_id FROM audit_entries WHERE workspace_id=$1 AND target_id=$2 AND action='invitation.expired_replaced'").bind(w).bind(old.invitation.id).fetch_one(&mut *tx).await.unwrap();
    assert_eq!(retired, (1, actor.user.id));
    let linked:i64=sqlx::query_scalar("SELECT count(*) FROM audit_entries a JOIN workspace_events e ON e.mutation_id=a.id WHERE a.workspace_id=$1 AND a.action='invitation.created' AND a.target_id=$2 AND e.entity_id=$2 AND e.event_type='invitation.created'").bind(w).bind(new.invitation.id).fetch_one(&mut *tx).await.unwrap();
    assert_eq!(linked, 1);
    let old_events: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workspace_events WHERE workspace_id=$1 AND entity_id=$2 AND seq>$3",
    )
    .bind(w)
    .bind(old.invitation.id)
    .bind(before["head"].as_i64().unwrap())
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert_eq!(
        old_events, 0,
        "Retirement cannot disclose a formerly hidden invitation ID"
    );
    let retry:String=sqlx::query_scalar("SELECT coalesce(jsonb_agg(response_body),'[]')::text FROM idempotency_records WHERE workspace_id=$1").bind(w).fetch_one(&mut *tx).await.unwrap();
    for token in [old.token.as_ref().unwrap(), new.token.as_ref().unwrap()] {
        assert!(!retry.contains(token));
    }
    tx.commit().await.unwrap();
}
async fn replacement(h: &Harness, owner: &Actor, w: Uuid) {
    let old = expired(h, owner, w, &[]).await;
    let before = snapshot(h, owner.user.id, w).await;
    let key = Uuid::new_v4().to_string();
    let new = h
        .app
        .create_invitation(owner, w, &key, body(&old.invitation.email, &[]))
        .await
        .unwrap();
    assert_replacement(h, owner, w, &old, &new, &before).await;
    let before_retry = snapshot(h, owner.user.id, w).await;
    let retry = h
        .app
        .create_invitation(owner, w, &key, body(&old.invitation.email, &[]))
        .await
        .unwrap();
    assert_eq!(retry.invitation.id, new.invitation.id);
    assert!(retry.token.is_none());
    assert_eq!(snapshot(h, owner.user.id, w).await, before_retry);
    assert!(matches!(
        h.app
            .create_invitation(
                owner,
                w,
                &key,
                req(json!({"email":old.invitation.email,"role":"viewer","team_ids":[]}))
            )
            .await,
        Err(Error::Conflict(_))
    ));
    assert_eq!(snapshot(h, owner.user.id, w).await, before_retry);
}
async fn hidden_replacement(h: &Harness, owner: &Actor, w: Uuid, private: Uuid) {
    let old = expired(h, owner, w, &[private]).await;
    let (admin, _) = account(h, owner.user.id, w, Some("admin")).await;
    assert!(matches!(
        h.app.get_invitation(&admin, w, old.invitation.id).await,
        Err(Error::NotFound)
    ));
    let before = snapshot(h, admin.user.id, w).await;
    let new = create(h, &admin, w, &old.invitation.email, &[]).await;
    assert!(matches!(
        h.app.get_invitation(&admin, w, old.invitation.id).await,
        Err(Error::NotFound)
    ));
    let response = serde_json::to_string(&new).unwrap();
    assert!(!response.contains(&old.invitation.id.to_string()));
    assert!(!response.contains(&private.to_string()));
    let mut tx = fixture(h, admin.user.id, w).await;
    let events: Vec<Value> = sqlx::query_scalar(
        "SELECT to_jsonb(e) FROM workspace_events e WHERE workspace_id=$1 AND seq>$2 ORDER BY seq",
    )
    .bind(w)
    .bind(before["head"].as_i64().unwrap())
    .fetch_all(&mut *tx)
    .await
    .unwrap();
    assert_eq!(events.len(), 1);
    let serialized = serde_json::to_string(&events).unwrap();
    for hidden in [
        old.invitation.id.to_string(),
        private.to_string(),
        old.token.unwrap(),
        old.invitation.email,
    ] {
        assert!(!serialized.contains(&hidden));
    }
    assert_eq!(events[0]["entity_id"], new.invitation.id.to_string());
    assert!(events[0]["team_id"].is_null());
    let retired: bool = sqlx::query_scalar(
        "SELECT revoked_at IS NOT NULL AND version=$2 FROM invitations WHERE id=$1",
    )
    .bind(old.invitation.id)
    .bind(old.invitation.version.parse::<i64>().unwrap() + 1)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert!(retired);
    let retirement_audit:i64=sqlx::query_scalar("SELECT count(*) FROM audit_entries WHERE workspace_id=$1 AND target_id=$2 AND actor_id=$3 AND action='invitation.expired_replaced'").bind(w).bind(old.invitation.id).bind(admin.user.id).fetch_one(&mut *tx).await.unwrap();
    assert_eq!(retirement_audit, 1);
    tx.commit().await.unwrap();
    assert_eq!(
        snapshot(h, admin.user.id, w).await["epoch"],
        before["epoch"]
    );
}

async fn install_failure(h: &Harness, table: &str, field: &str, action: &str, silent: bool) {
    assert!(matches!(
        table,
        "audit_entries" | "workspace_events" | "account_security_events"
    ));
    assert!(matches!(field, "action" | "event_type"));
    // All identifiers and actions are fixed internal test literals.
    sqlx::query(AssertSqlSafe(format!("CREATE TRIGGER test_invitation_failure BEFORE INSERT ON {table} FOR EACH ROW EXECUTE FUNCTION test_invitation_failure('{field}','{action}','{silent}')"))).execute(&h.owner).await.unwrap();
}
async fn remove_failure(h: &Harness, table: &str) {
    sqlx::query(AssertSqlSafe(format!(
        "DROP TRIGGER test_invitation_failure ON {table}"
    )))
    .execute(&h.owner)
    .await
    .unwrap();
}
async fn replacement_failures(h: &Harness, owner: &Actor, w: Uuid) {
    sqlx::raw_sql("CREATE FUNCTION test_invitation_failure() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF to_jsonb(NEW)->>TG_ARGV[0]=TG_ARGV[1] THEN IF TG_ARGV[2]::boolean THEN RETURN NULL; ELSE RAISE EXCEPTION 'Controlled invitation durability failure'; END IF; END IF; RETURN NEW; END $$").execute(&h.owner).await.unwrap();
    for silent in [false, true] {
        for (table, field, action) in [
            ("audit_entries", "action", "invitation.expired_replaced"),
            ("audit_entries", "action", "invitation.created"),
            ("workspace_events", "event_type", "invitation.created"),
        ] {
            let old = expired(h, owner, w, &[]).await;
            let before = snapshot(h, owner.user.id, w).await;
            let key = Uuid::new_v4().to_string();
            install_failure(h, table, field, action, silent).await;
            assert!(
                h.app
                    .create_invitation(owner, w, &key, body(&old.invitation.email, &[]))
                    .await
                    .is_err(),
                "Mandatory {table} {action} cannot be skipped; silent={silent}"
            );
            assert_eq!(
                snapshot(h, owner.user.id, w).await,
                before,
                "Retirement, replacement, retry, audit and stream head roll back together"
            );
            remove_failure(h, table).await;
            let new = h
                .app
                .create_invitation(owner, w, &key, body(&old.invitation.email, &[]))
                .await
                .unwrap();
            assert_replacement(h, owner, w, &old, &new, &before).await;
        }
    }
}

async fn concurrent_replacements(h: &Harness, owner: &Actor, w: Uuid, same_key: bool) {
    let old = expired(h, owner, w, &[]).await;
    let before = snapshot(h, owner.user.id, w).await;
    let key = Uuid::new_v4().to_string();
    let other_key = if same_key {
        key.clone()
    } else {
        Uuid::new_v4().to_string()
    };
    let lock_key = i64::from_be_bytes(Uuid::new_v4().as_bytes()[..8].try_into().unwrap());
    sqlx::raw_sql("CREATE OR REPLACE FUNCTION test_invitation_gate() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.action='invitation.expired_replaced' THEN PERFORM set_config('lock_timeout','5000',true); PERFORM pg_advisory_xact_lock(TG_ARGV[0]::bigint); END IF; RETURN NEW; END $$").execute(&h.owner).await.unwrap();
    sqlx::query(AssertSqlSafe(format!("CREATE TRIGGER test_invitation_gate BEFORE INSERT ON audit_entries FOR EACH ROW EXECUTE FUNCTION test_invitation_gate('{lock_key}')"))).execute(&h.owner).await.unwrap();
    let mut gate = store::begin(&h.owner).await.unwrap();
    let pid = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *gate)
        .await
        .unwrap();
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(lock_key)
        .execute(&mut *gate)
        .await
        .unwrap();
    let app = h.app.clone();
    let actor = owner.clone();
    let email = old.invitation.email.clone();
    let first = tokio::spawn(async move {
        app.create_invitation(&actor, w, &key, body(&email, &[]))
            .await
    });
    let first_pid = blocked_by(h, pid).await;
    let app = h.app.clone();
    let actor = owner.clone();
    let email = old.invitation.email.clone();
    let second = tokio::spawn(async move {
        app.create_invitation(&actor, w, &other_key, body(&email, &[]))
            .await
    });
    blocked_by(h, first_pid).await;
    gate.commit().await.unwrap();
    let new = first.await.unwrap().unwrap();
    if same_key {
        let replay = second.await.unwrap().unwrap();
        assert_eq!(replay.invitation.id, new.invitation.id);
        assert!(replay.token.is_none());
    } else {
        assert!(matches!(second.await.unwrap(), Err(Error::Conflict(_))));
    }
    sqlx::query("DROP TRIGGER test_invitation_gate ON audit_entries")
        .execute(&h.owner)
        .await
        .unwrap();
    assert_replacement(h, owner, w, &old, &new, &before).await;
    let after = snapshot(h, owner.user.id, w).await;
    assert_eq!(
        after["retries"].as_array().unwrap().len(),
        before["retries"].as_array().unwrap().len() + 1
    );
}

async fn acceptance_state(h: &Harness, user: Uuid, w: Uuid) -> Value {
    let mut tx = fixture(h, user, w).await;
    let state:Value=sqlx::query_scalar("SELECT jsonb_build_object('memberships',(SELECT coalesce(jsonb_agg(to_jsonb(m)),'[]') FROM workspace_members m WHERE workspace_id=$1 AND user_id=$2),'team_grants',(SELECT coalesce(jsonb_agg(to_jsonb(m) ORDER BY team_id),'[]') FROM team_members m WHERE workspace_id=$1 AND user_id=$2),'sessions',(SELECT coalesce(jsonb_agg(jsonb_build_array(id,token_hash,revoked_at,auth_time,expires_at) ORDER BY id),'[]') FROM sessions WHERE user_id=$2),'account_audit',(SELECT count(*) FROM account_security_events WHERE user_id=$2),'workspace_audit',(SELECT count(*) FROM audit_entries WHERE workspace_id=$1),'events',(SELECT count(*) FROM workspace_events WHERE workspace_id=$1),'head',(SELECT last_seq FROM workspace_stream_heads WHERE workspace_id=$1),'epoch',(SELECT epoch FROM workspace_auth_guards WHERE workspace_id=$1))").bind(w).bind(user).fetch_one(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    state
}
async fn acceptance_crosses_expiry(h: &Harness, owner: &Actor, w: Uuid, private: Uuid) {
    let (recipient, cookie) = account(h, owner.user.id, w, None).await;
    let invitation = create(h, owner, w, &recipient.user.email, &[private]).await;
    let before = acceptance_state(h, recipient.user.id, w).await;
    let mut guard = fixture(h, recipient.user.id, w).await;
    sqlx::query("SELECT id FROM users WHERE id=$1 FOR SHARE")
        .bind(recipient.user.id)
        .fetch_one(&mut *guard)
        .await
        .unwrap();
    let pid = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *guard)
        .await
        .unwrap();
    let app = h.app.clone();
    let actor = recipient.clone();
    let token = invitation.token.clone().unwrap();
    let accept = tokio::spawn(async move {
        app.accept_invitation(Some(&actor), req(json!({"token":token})))
            .await
    });
    blocked_by(h, pid).await;
    // The acceptance transaction already started and passed the preview. Set
    // expiry after that start, then cross actual DB time before releasing its
    // account guard. A stale now() check would incorrectly accept this token.
    let expires:chrono::DateTime<chrono::Utc>=sqlx::query_scalar("UPDATE invitations SET expires_at=clock_timestamp()+interval '100 milliseconds' WHERE id=$1 RETURNING expires_at").bind(invitation.invitation.id).fetch_one(&mut *guard).await.unwrap();
    tokio::time::timeout(std::time::Duration::from_millis(700), async {
        loop {
            let crossed: bool = sqlx::query_scalar("SELECT clock_timestamp()>$1")
                .bind(expires)
                .fetch_one(&mut *guard)
                .await
                .unwrap();
            if crossed {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        }
    })
    .await
    .expect("Fixture crosses real expiration before the production lock deadline");
    guard.commit().await.unwrap();
    assert!(matches!(accept.await.unwrap(), Err(Error::NotFound)));
    assert_eq!(
        acceptance_state(h, recipient.user.id, w).await,
        before,
        "Expiry cannot create membership/grants, rotate sessions, or emit audit/events"
    );
    assert!(h.app.authenticate(&cookie).await.is_ok());
    let mut tx = fixture(h, owner.user.id, w).await;
    let unchanged: (bool, bool, String) = sqlx::query_as(
        "SELECT accepted_at IS NULL,revoked_at IS NULL,version::text FROM invitations WHERE id=$1",
    )
    .bind(invitation.invitation.id)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert_eq!(unchanged, (true, true, invitation.invitation.version));
    tx.commit().await.unwrap();
}

async fn acceptance_audit_failures(h: &Harness, owner: &Actor, w: Uuid, private: Uuid) {
    let (recipient, cookie) = account(h, owner.user.id, w, None).await;
    let invitation = create(h, owner, w, &recipient.user.email, &[private]).await;
    let token = invitation.token.as_ref().unwrap();
    for silent in [false, true] {
        for (table, field, event) in [
            ("audit_entries", "action", "invitation.accepted"),
            ("workspace_events", "event_type", "access.changed"),
            ("account_security_events", "action", "session.created"),
            (
                "account_security_events",
                "action",
                "session.invitation_rotated",
            ),
        ] {
            let before = acceptance_state(h, recipient.user.id, w).await;
            let invitations_before = snapshot(h, recipient.user.id, w).await;
            install_failure(h, table, field, event, silent).await;
            assert!(
                h.app
                    .accept_invitation(Some(&recipient), req(json!({"token":token})))
                    .await
                    .is_err()
            );
            assert_eq!(acceptance_state(h, recipient.user.id, w).await, before);
            assert_eq!(snapshot(h, recipient.user.id, w).await, invitations_before);
            assert!(h.app.authenticate(&cookie).await.is_ok());
            remove_failure(h, table).await;
        }
    }
    let (_, replacement) = h
        .app
        .accept_invitation(Some(&recipient), req(json!({"token":token})))
        .await
        .unwrap();
    assert!(matches!(
        h.app.authenticate(&cookie).await,
        Err(Error::Unauthorized)
    ));
    assert!(h.app.authenticate(&replacement).await.is_ok());
}

async fn acceptance_crosses_consumption_expiry(h: &Harness, owner: &Actor, w: Uuid, private: Uuid) {
    let (recipient, cookie) = account(h, owner.user.id, w, None).await;
    let invitation = create(h, owner, w, &recipient.user.email, &[private]).await;
    let before = acceptance_state(h, recipient.user.id, w).await;
    let key = i64::from_be_bytes(Uuid::new_v4().as_bytes()[..8].try_into().unwrap());
    sqlx::raw_sql("CREATE FUNCTION test_acceptance_gate() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN PERFORM pg_advisory_xact_lock(TG_ARGV[0]::bigint); RETURN NEW; END $$").execute(&h.owner).await.unwrap();
    sqlx::query(AssertSqlSafe(format!("CREATE TRIGGER test_acceptance_gate BEFORE INSERT ON team_members FOR EACH ROW EXECUTE FUNCTION test_acceptance_gate('{key}')"))).execute(&h.owner).await.unwrap();
    let mut gate = fixture(h, recipient.user.id, w).await;
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(key)
        .execute(&mut *gate)
        .await
        .unwrap();
    let pid = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *gate)
        .await
        .unwrap();
    let app = h.app.clone();
    let actor = recipient.clone();
    let token = invitation.token.clone().unwrap();
    let accept = tokio::spawn(async move {
        app.accept_invitation(Some(&actor), req(json!({"token":token})))
            .await
    });
    blocked_by(h, pid).await;
    // This gate is after token/authority checks and after workspace membership
    // insertion, but before the final guarded token consumption. Only its fresh
    // expiry predicate can reject and roll back the already attempted writes.
    let expires:chrono::DateTime<chrono::Utc>=sqlx::query_scalar("UPDATE invitations SET expires_at=clock_timestamp()+interval '100 milliseconds' WHERE id=$1 RETURNING expires_at").bind(invitation.invitation.id).fetch_one(&mut *gate).await.unwrap();
    tokio::time::timeout(std::time::Duration::from_millis(700), async {
        loop {
            let crossed: bool = sqlx::query_scalar("SELECT clock_timestamp()>$1")
                .bind(expires)
                .fetch_one(&mut *gate)
                .await
                .unwrap();
            if crossed {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        }
    })
    .await
    .expect("Cross actual expiry before the fixture lock deadline");
    gate.commit().await.unwrap();
    assert!(matches!(accept.await.unwrap(), Err(Error::NotFound)));
    sqlx::raw_sql(
        "DROP TRIGGER test_acceptance_gate ON team_members; DROP FUNCTION test_acceptance_gate()",
    )
    .execute(&h.owner)
    .await
    .unwrap();
    assert_eq!(
        acceptance_state(h, recipient.user.id, w).await,
        before,
        "Final consumption must roll back membership/grants, session rotation, audit and epoch"
    );
    assert!(h.app.authenticate(&cookie).await.is_ok());
    let mut tx = fixture(h, owner.user.id, w).await;
    let consumed: bool =
        sqlx::query_scalar("SELECT accepted_at IS NOT NULL FROM invitations WHERE id=$1")
            .bind(invitation.invitation.id)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    assert!(!consumed);
    tx.commit().await.unwrap();
}
