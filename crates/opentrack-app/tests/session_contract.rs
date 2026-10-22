//! Account session contracts against a real, isolated PostgreSQL schema.
//! Fixture locks expose both revocation orders without guessed timing assertions.
#[allow(dead_code)]
mod support;

use opentrack_domain::*;
use opentrack_store as store;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use support::Harness;
use uuid::Uuid;

const PASSWORD: &str = "session contract password 184!";
const EMAIL: &str = "owner@sessions.invalid";

fn request<T: DeserializeOwned>(value: Value) -> T {
    serde_json::from_value(value).unwrap()
}

async fn login(h: &Harness) -> (Actor, String) {
    let (_, cookie) = h
        .app
        .login(request(json!({"email":EMAIL,"password":PASSWORD})))
        .await
        .unwrap();
    let actor = h.app.authenticate(&cookie).await.unwrap();
    (actor, cookie)
}

async fn current(h: &Harness, actor: &Actor) -> Uuid {
    h.app
        .sessions(actor, SessionQuery::default())
        .await
        .unwrap()
        .data
        .into_iter()
        .find(|s| s.current)
        .unwrap()
        .id
}

// Fixture setup deliberately avoids the application account guard, so it can
// block a specific SQL row after a real command has acquired that guard.
async fn fixture(h: &Harness, user: Uuid, workspace: Option<Uuid>) -> store::Tx {
    let mut tx = store::begin(&h.owner).await.unwrap();
    sqlx::query("SELECT set_config('opentrack.user_id',$1,true),set_config('opentrack.workspace_id',$2,true)")
        .bind(user.to_string()).bind(workspace.map(|v|v.to_string()).unwrap_or_default()).execute(&mut *tx).await.unwrap();
    tx
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_postgres_session_lifecycle_contracts() {
    let h = Harness::new().await;
    let runner = h.clone();
    let outcome = tokio::spawn(async move { exercise(&runner).await }).await;
    h.cleanup().await;
    if let Err(error) = outcome {
        std::panic::resume_unwind(error.into_panic());
    }
}

async fn exercise(h: &Harness) {
    let (session, original_cookie)=h.app.setup(request(json!({
        "setup_token":h.setup_token,"email":EMAIL,"password":PASSWORD,
        "display_name":"Session owner","workspace_name":"Session workspace","team_name":"Engineering","team_key":"ENG"
    }))).await.unwrap();
    let original = h.app.authenticate(&original_cookie).await.unwrap();
    let w = session.workspaces[0].id;
    let team = h.app.bootstrap(&original, w).await.unwrap().teams[0].id;
    let (second, second_cookie) = login(h).await;
    let (third, third_cookie) = login(h).await;
    let other = other_user(h, &original, w).await;
    pagination_and_projection(h, &original, &other).await;

    assert!(matches!(
        h.app
            .login_with_previous(
                request(json!({"email":EMAIL,"password":"wrong password"})),
                Some(&original_cookie)
            )
            .await,
        Err(Error::Unauthorized)
    ));
    assert!(
        h.app.authenticate(&original_cookie).await.is_ok(),
        "A failed login must not revoke the existing session"
    );
    let (rotated_session, rotated_cookie) = h
        .app
        .login_with_previous(
            request(json!({"email":EMAIL,"password":PASSWORD})),
            Some(&original_cookie),
        )
        .await
        .unwrap();
    assert_ne!(rotated_cookie, original_cookie);
    assert_ne!(rotated_session.csrf_token, original.csrf_token);
    assert!(matches!(
        h.app.authenticate(&original_cookie).await,
        Err(Error::Unauthorized)
    ));
    assert!(
        matches!(h.app.session(&original).await, Err(Error::Unauthorized)),
        "Cached actors must be rechecked after rotation"
    );
    assert!(
        h.app.authenticate(&second_cookie).await.is_ok(),
        "Rotation affects only the supplied prior session"
    );
    let rotated = h.app.authenticate(&rotated_cookie).await.unwrap();
    let third_id = current(h, &third).await;
    assert!(!h.app.revoke_session(&rotated, third_id).await.unwrap());
    assert!(
        !h.app.revoke_session(&rotated, third_id).await.unwrap(),
        "Repeated revocation of an owned session is idempotent"
    );
    assert!(matches!(
        h.app.authenticate(&third_cookie).await,
        Err(Error::Unauthorized)
    ));
    assert!(matches!(
        h.app.sessions(&third, SessionQuery::default()).await,
        Err(Error::Unauthorized)
    ));
    assert!(matches!(
        h.app
            .revoke_session(&rotated, current(h, &other).await)
            .await,
        Err(Error::NotFound)
    ));
    assert!(matches!(
        h.app.revoke_session(&rotated, Uuid::new_v4()).await,
        Err(Error::NotFound)
    ));

    freshness_and_revoke_others(h, &rotated, &second, &second_cookie).await;
    expiry(h).await;
    revocation_ordering(h, w, team).await;
    let other = invitation_preserves_authentication_time(h, &other).await;
    audit(
        h,
        &other,
        &[
            &original_cookie,
            &second_cookie,
            &third_cookie,
            &rotated_cookie,
            &original.token_hash,
            &original.csrf_token,
            PASSWORD,
        ],
    )
    .await;
}

async fn invitation_preserves_authentication_time(h: &Harness, other: &Actor) -> Actor {
    let (owner, _) = login(h).await;
    let organization = h.app.organizations(&owner).await.unwrap()[0].id;
    let workspace = h.app.create_workspace(&owner,"session-invite-workspace",request(json!({
        "organization_id":organization,"name":"Invitation destination","team_name":"Operations","team_key":"OPS"
    }))).await.unwrap();
    let invitation = h
        .app
        .create_invitation(
            &owner,
            workspace.id,
            "session-existing-invite",
            request(json!({
                "email":other.user.email,"role":"member"
            })),
        )
        .await
        .unwrap();
    let mut tx = fixture(h, other.user.id, None).await;
    let token = invitation.token.unwrap();
    sqlx::query("UPDATE sessions SET auth_time=now()-interval '11 minutes',expires_at=now()+interval '1 hour' WHERE token_hash=$1")
        .bind(&other.token_hash).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    assert!(matches!(
        h.app
            .accept_invitation(Some(other), request(json!({"token":token})))
            .await,
        Err(Error::ReauthenticationRequired)
    ));
    assert!(
        h.app.session(other).await.is_ok(),
        "Failed acceptance must leave the source session usable"
    );
    assert!(
        h.app.invitation_preview(&token).await.is_ok(),
        "Failed acceptance must not consume the invitation"
    );
    let mut tx = fixture(h, other.user.id, None).await;
    let original: (chrono::DateTime<chrono::Utc>,chrono::DateTime<chrono::Utc>) = sqlx::query_as("UPDATE sessions SET auth_time=now()-interval '9 minutes' WHERE token_hash=$1 RETURNING auth_time,expires_at")
        .bind(&other.token_hash).fetch_one(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    let (_, cookie) = h
        .app
        .accept_invitation(Some(other), request(json!({"token":token})))
        .await
        .unwrap();
    let replacement = h.app.authenticate(&cookie).await.unwrap();
    assert!(matches!(
        h.app.session(other).await,
        Err(Error::Unauthorized)
    ));
    let mut tx = fixture(h, replacement.user.id, None).await;
    let rotated: (chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>) =
        sqlx::query_as("SELECT auth_time,expires_at FROM sessions WHERE token_hash=$1")
            .bind(&replacement.token_hash)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(
        rotated, original,
        "Invitation cookie rotation must preserve exact password authentication and absolute expiration times"
    );
    let mut tx = fixture(h, replacement.user.id, None).await;
    sqlx::query("UPDATE sessions SET auth_time=now()-interval '11 minutes' WHERE token_hash=$1")
        .bind(&replacement.token_hash)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert!(
        matches!(
            h.app.revoke_other_sessions(&replacement).await,
            Err(Error::ReauthenticationRequired)
        ),
        "Accepting an invitation cannot bypass sensitive-action reauthentication"
    );
    replacement
}

async fn other_user(h: &Harness, owner: &Actor, w: Uuid) -> Actor {
    let issued = h
        .app
        .create_invitation(
            owner,
            w,
            "session-other-user",
            request(json!({"email":"other@sessions.invalid","role":"member"})),
        )
        .await
        .unwrap();
    let (_,cookie)=h.app.accept_invitation(None,request(json!({"token":issued.token.unwrap(),"display_name":"Other account","password":PASSWORD}))).await.unwrap();
    h.app.authenticate(&cookie).await.unwrap()
}

async fn pagination_and_projection(h: &Harness, actor: &Actor, other: &Actor) {
    let mut tx = fixture(h, actor.user.id, None).await;
    sqlx::query("UPDATE sessions SET created_at=now()-interval '1 hour' WHERE user_id=$1")
        .bind(actor.user.id)
        .execute(&mut *tx)
        .await
        .unwrap();
    let expected: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM sessions WHERE user_id=$1 ORDER BY created_at DESC,id DESC",
    )
    .bind(actor.user.id)
    .fetch_all(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    let all = h
        .app
        .sessions(actor, SessionQuery::default())
        .await
        .unwrap();
    assert_eq!(all.data.len(), 3);
    assert_eq!(all.data.iter().filter(|v| v.current).count(), 1);
    for session in &all.data {
        assert!(session.idle_expires_at <= session.expires_at);
        assert!(session.idle_expires_at <= session.last_seen_at + chrono::Duration::hours(12));
        let dto = serde_json::to_value(session).unwrap();
        assert_eq!(dto["audience"], "web");
        assert!(dto["device_name"].is_null());
        let mut keys: Vec<_> = dto
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec![
                "audience",
                "auth_time",
                "created_at",
                "current",
                "device_name",
                "expires_at",
                "id",
                "idle_expires_at",
                "last_seen_at"
            ]
        );
    }
    let first = h
        .app
        .sessions(actor, request(json!({"limit":1})))
        .await
        .unwrap();
    let cursor = first.next_cursor.clone().unwrap();
    let mut seen = vec![first.data[0].id];
    let mut next = Some(cursor.clone());
    while let Some(cursor) = next {
        let page = h
            .app
            .sessions(actor, request(json!({"limit":1,"cursor":cursor})))
            .await
            .unwrap();
        seen.extend(page.data.iter().map(|s| s.id));
        next = page.next_cursor;
        assert!(
            seen.len() <= expected.len(),
            "Pagination cannot repeat records indefinitely"
        );
    }
    assert_eq!(
        seen, expected,
        "Identical timestamps still paginate deterministically by ID"
    );
    for value in [
        json!({"limit":0}),
        json!({"limit":101}),
        json!({"cursor":"broken"}),
        json!({"limit":2,"cursor":cursor}),
    ] {
        assert!(matches!(
            h.app.sessions(actor, request(value)).await,
            Err(Error::Invalid(_))
        ));
    }
    assert!(matches!(
        h.app
            .sessions(other, request(json!({"limit":1,"cursor":cursor})))
            .await,
        Err(Error::Invalid(_))
    ));
}

async fn freshness_and_revoke_others(
    h: &Harness,
    actor: &Actor,
    second: &Actor,
    second_cookie: &str,
) {
    let own_id = current(h, actor).await;
    let second_id = current(h, second).await;
    let mut tx = fixture(h, actor.user.id, None).await;
    sqlx::query("UPDATE sessions SET auth_time=now()-interval '11 minutes' WHERE token_hash=$1")
        .bind(&actor.token_hash)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert!(matches!(
        h.app.revoke_session(actor, second_id).await,
        Err(Error::ReauthenticationRequired)
    ));
    assert!(matches!(
        h.app.revoke_other_sessions(actor).await,
        Err(Error::ReauthenticationRequired)
    ));
    assert!(h.app.authenticate(second_cookie).await.is_ok());
    assert!(
        h.app.revoke_session(actor, own_id).await.unwrap(),
        "Current-session sign out remains available after the fresh-auth window"
    );
    assert!(matches!(
        h.app.session(actor).await,
        Err(Error::Unauthorized)
    ));
    let (fresh, cookie) = login(h).await;
    h.app.revoke_other_sessions(&fresh).await.unwrap();
    assert!(h.app.authenticate(&cookie).await.is_ok());
    assert!(matches!(
        h.app.authenticate(second_cookie).await,
        Err(Error::Unauthorized)
    ));
    assert!(matches!(
        h.app.session(second).await,
        Err(Error::Unauthorized)
    ));
    h.app.revoke_other_sessions(&fresh).await.unwrap();
    let active = h
        .app
        .sessions(&fresh, SessionQuery::default())
        .await
        .unwrap();
    assert_eq!(active.data.len(), 1);
    assert!(active.data[0].current);
}

async fn expiry(h: &Harness) {
    let (absolute, absolute_cookie) = login(h).await;
    let (idle, idle_cookie) = login(h).await;
    let (valid, _) = login(h).await;
    let mut tx = fixture(h, valid.user.id, None).await;
    sqlx::query("UPDATE sessions SET expires_at=now()-interval '1 second' WHERE token_hash=$1")
        .bind(&absolute.token_hash)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("UPDATE sessions SET last_seen_at=now()-interval '13 hours' WHERE token_hash=$1")
        .bind(&idle.token_hash)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("UPDATE sessions SET expires_at=now()+interval '1 hour' WHERE token_hash=$1")
        .bind(&valid.token_hash)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    for (actor, cookie) in [(&absolute, &absolute_cookie), (&idle, &idle_cookie)] {
        assert!(matches!(
            h.app.authenticate(cookie).await,
            Err(Error::Unauthorized)
        ));
        assert!(matches!(
            h.app.session(actor).await,
            Err(Error::Unauthorized)
        ));
        assert!(matches!(
            h.app.revoke_other_sessions(actor).await,
            Err(Error::Unauthorized)
        ));
    }
    let list = h
        .app
        .sessions(&valid, SessionQuery::default())
        .await
        .unwrap();
    let current = list.data.iter().find(|s| s.current).unwrap();
    assert_eq!(
        current.expires_at, current.idle_expires_at,
        "Idle deadline is capped by absolute expiry"
    );
    let mut tx = fixture(h, valid.user.id, None).await;
    let expired_ids: Vec<Uuid> =
        sqlx::query_scalar("SELECT id FROM sessions WHERE token_hash IN ($1,$2)")
            .bind(&absolute.token_hash)
            .bind(&idle.token_hash)
            .fetch_all(&mut *tx)
            .await
            .unwrap();
    assert!(list.data.iter().all(|s| !expired_ids.contains(&s.id)));
    tx.commit().await.unwrap();
    h.app.revoke_other_sessions(&valid).await.unwrap();
    let mut tx = fixture(h, valid.user.id, None).await;
    let untouched: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM sessions WHERE token_hash IN ($1,$2) AND revoked_at IS NULL",
    )
    .bind(&absolute.token_hash)
    .bind(&idle.token_hash)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert_eq!(
        untouched, 2,
        "Bulk revocation leaves already expired history unchanged"
    );
    tx.commit().await.unwrap();
    assert!(!h.app.revoke_session(&valid, expired_ids[0]).await.unwrap());
    assert!(
        !h.app.revoke_session(&valid, expired_ids[0]).await.unwrap(),
        "An individually selected owned expired session remains idempotent"
    );
}

async fn blocked_by(h: &Harness, blocker: i32) -> i32 {
    tokio::time::timeout(std::time::Duration::from_millis(800), async {
        loop {
            let waiter: Option<i32> = sqlx::query_scalar(
                "SELECT pid FROM pg_stat_activity WHERE $1=ANY(pg_blocking_pids(pid)) LIMIT 1",
            )
            .bind(blocker)
            .fetch_optional(&h.owner)
            .await
            .unwrap();
            if let Some(pid) = waiter {
                return pid;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("Expected command reached the observed PostgreSQL lock wait")
}

async fn revocation_ordering(h: &Harness, w: Uuid, team: Uuid) {
    let (controller, _) = login(h).await;
    let (target, cookie) = login(h).await;
    let target_id = current(h, &target).await;
    let mut lock = fixture(h, target.user.id, Some(w)).await;
    let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *lock)
        .await
        .unwrap();
    sqlx::query("SELECT id FROM sessions WHERE id=$1 FOR UPDATE")
        .bind(target_id)
        .fetch_one(&mut *lock)
        .await
        .unwrap();
    let app = h.app.clone();
    let revoker = controller.clone();
    let revoke = tokio::spawn(async move { app.revoke_session(&revoker, target_id).await });
    let revoke_pid = blocked_by(h, blocker).await;
    let app = h.app.clone();
    let writer = target.clone();
    let write = tokio::spawn(async move {
        app.create_issue(
            &writer,
            w,
            "session-revoke-first",
            request(json!({"team_id":team,"title":"Revocation won"})),
        )
        .await
    });
    blocked_by(h, revoke_pid).await;
    lock.commit().await.unwrap();
    assert!(!revoke.await.unwrap().unwrap());
    assert!(
        matches!(write.await.unwrap(), Err(Error::Unauthorized)),
        "A queued cached actor must be checked after revocation"
    );
    assert!(matches!(
        h.app.authenticate(&cookie).await,
        Err(Error::Unauthorized)
    ));

    let (target, _) = login(h).await;
    let target_id = current(h, &target).await;
    let mut lock = fixture(h, target.user.id, Some(w)).await;
    let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *lock)
        .await
        .unwrap();
    sqlx::query("SELECT id FROM teams WHERE workspace_id=$1 AND id=$2 FOR UPDATE")
        .bind(w)
        .bind(team)
        .fetch_one(&mut *lock)
        .await
        .unwrap();
    let app = h.app.clone();
    let writer = target.clone();
    let write = tokio::spawn(async move {
        app.create_issue(
            &writer,
            w,
            "session-write-first",
            request(json!({"team_id":team,"title":"Write won"})),
        )
        .await
    });
    let write_pid = blocked_by(h, blocker).await;
    let app = h.app.clone();
    let revoker = controller.clone();
    let revoke = tokio::spawn(async move { app.revoke_session(&revoker, target_id).await });
    blocked_by(h, write_pid).await;
    lock.commit().await.unwrap();
    let issue = write
        .await
        .unwrap()
        .expect("A write already holding the shared account guard commits first");
    assert!(!revoke.await.unwrap().unwrap());
    assert!(h.app.get_issue(&controller, w, issue.id).await.is_ok());
    assert!(matches!(
        h.app
            .create_issue(
                &target,
                w,
                "session-after-revoke",
                request(json!({"team_id":team,"title":"Cannot write"}))
            )
            .await,
        Err(Error::Unauthorized)
    ));
    let mut tx = fixture(h, controller.user.id, Some(w)).await;
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM issues WHERE title IN ('Revocation won','Write won','Cannot write')",
    )
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert_eq!(count, 1, "Only the issue ordered before revocation exists");
    tx.commit().await.unwrap();
}

async fn audit(h: &Harness, other: &Actor, secrets: &[&str]) {
    let (owner, _) = login(h).await;
    let unscoped: i64 = sqlx::query_scalar("SELECT count(*) FROM account_security_events")
        .fetch_one(&h.app.pool)
        .await
        .unwrap();
    assert_eq!(
        unscoped, 0,
        "Personal security audit fails closed without a user context"
    );
    let mut tx = store::begin(&h.app.pool).await.unwrap();
    store::context(&mut tx, owner.user.id, None).await.unwrap();
    let rows: Vec<Value> = sqlx::query_scalar(
        "SELECT to_jsonb(e) FROM account_security_events e ORDER BY created_at,id",
    )
    .fetch_all(&mut *tx)
    .await
    .unwrap();
    assert!(!rows.is_empty());
    assert!(
        rows.iter()
            .all(|e| e["user_id"] == owner.user.id.to_string())
    );
    for row in &rows {
        let encoded = row.to_string();
        for secret in secrets {
            assert!(
                !encoded.contains(secret),
                "Account audit never contains authentication secrets"
            );
        }
        assert!(matches!(
            row["action"].as_str(),
            Some(
                "session.created"
                    | "session.revoked"
                    | "session.logged_out"
                    | "session.login_rotated"
                    | "session.invitation_rotated"
            )
        ));
    }
    assert!(rows.iter().any(|e| e["action"] == "session.login_rotated"));
    assert!(rows.iter().any(|e| e["action"] == "session.revoked"));
    tx.commit().await.unwrap();
    let mut tx = store::begin(&h.app.pool).await.unwrap();
    store::context(&mut tx, other.user.id, None).await.unwrap();
    let visible: Vec<Uuid> =
        sqlx::query_scalar("SELECT DISTINCT user_id FROM account_security_events")
            .fetch_all(&mut *tx)
            .await
            .unwrap();
    assert_eq!(visible, vec![other.user.id]);
    tx.commit().await.unwrap();
    let foreign_session = current(h, other).await;
    let own_session = current(h, &owner).await;
    for (user, session, related, code) in [
        (owner.user.id, foreign_session, None, "23503"),
        (owner.user.id, own_session, Some(foreign_session), "23503"),
        (other.user.id, foreign_session, None, "42501"),
    ] {
        let mut tx = store::begin(&h.app.pool).await.unwrap();
        store::context(&mut tx, owner.user.id, None).await.unwrap();
        let error = sqlx::query("INSERT INTO account_security_events(user_id,session_id,related_session_id,action) VALUES($1,$2,$3,'session.created')")
            .bind(user).bind(session).bind(related).execute(&mut *tx).await.unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some(code),
            "Neither foreign session IDs nor another account's audit scope can be forged"
        );
        tx.rollback().await.unwrap();
    }
    for statement in [
        "UPDATE account_security_events SET action='session.created'",
        "DELETE FROM account_security_events",
        "TRUNCATE account_security_events",
    ] {
        let mut tx = store::begin(&h.app.pool).await.unwrap();
        store::context(&mut tx, owner.user.id, None).await.unwrap();
        let error = sqlx::query(statement).execute(&mut *tx).await.unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("42501")
        );
        tx.rollback().await.unwrap();
    }
    // FORCE RLS applies to the migration owner too, and the trigger remains a
    // second append-only defense for that owner even though it has table rights.
    let owner_unscoped: i64 = sqlx::query_scalar("SELECT count(*) FROM account_security_events")
        .fetch_one(&h.owner)
        .await
        .unwrap();
    assert_eq!(owner_unscoped, 0);
    let mut tx = fixture(h, owner.user.id, None).await;
    assert!(
        sqlx::query("DELETE FROM account_security_events")
            .execute(&mut *tx)
            .await
            .is_err()
    );
    tx.rollback().await.unwrap();
}
