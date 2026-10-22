//! Cross-account owner and invitation races observe PostgreSQL lock dependencies.
#[allow(dead_code)]
mod support;

use opentrack_app::Operator;
use opentrack_domain::*;
use opentrack_store as store;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use sqlx::AssertSqlSafe;
use support::Harness;
use uuid::Uuid;

const PASSWORD: &str = "account lifecycle race password 519!";
const REASON: &str = "Verified lifecycle race fixture";

fn request<T: DeserializeOwned>(value: Value) -> T {
    serde_json::from_value(value).unwrap()
}
struct Account {
    id: Uuid,
    email: String,
}
async fn fixture(h: &Harness, user: Uuid, workspace: Uuid) -> store::Tx {
    let mut tx = store::begin(&h.owner).await.unwrap();
    sqlx::query("SELECT set_config('opentrack.user_id',$1,true),set_config('opentrack.workspace_id',$2,true)")
        .bind(user.to_string()).bind(workspace.to_string()).execute(&mut *tx).await.unwrap();
    tx
}
async fn account(h: &Harness, owner: &Actor, workspace: Uuid, role: &str) -> Account {
    let id = Uuid::new_v4();
    let email = format!("{id}@lifecycle-race.invalid");
    let mut tx = fixture(h, owner.user.id, workspace).await;
    sqlx::query("INSERT INTO users(id,email,display_name,password_hash) SELECT $1,$2,'Race account',password_hash FROM users WHERE id=$3")
        .bind(id).bind(&email).bind(owner.user.id).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO workspace_members(workspace_id,user_id,role) VALUES($1,$2,$3)")
        .bind(workspace)
        .bind(id)
        .bind(role)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    Account { id, email }
}
async fn login(h: &Harness, account: &Account) -> (Actor, String) {
    let (_, cookie) = h
        .app
        .login(request(json!({"email":account.email,"password":PASSWORD})))
        .await
        .unwrap();
    (h.app.authenticate(&cookie).await.unwrap(), cookie)
}
async fn workspace(h: &Harness, owner: &Actor, organization: Uuid, label: &str) -> Uuid {
    h.app.create_workspace(owner,label,request(json!({"organization_id":organization,"name":label,"team_name":"Race team","team_key":"RAC"}))).await.unwrap().id
}
async fn gate(h: &Harness, table: &str, action: &str) -> (store::Tx, i32) {
    // Identifiers/actions below are fixed test literals, never user input.
    assert!(matches!(table, "audit_entries" | "account_security_events"));
    assert!(matches!(
        action,
        "workspace.member.updated" | "account.disabled" | "invitation.accepted"
    ));
    let key = i64::from_be_bytes(Uuid::new_v4().as_bytes()[..8].try_into().unwrap());
    sqlx::query(AssertSqlSafe(format!("CREATE TRIGGER test_gate BEFORE INSERT ON {table} FOR EACH ROW EXECUTE FUNCTION test_lifecycle_race_gate('{key}','{action}')")))
        .execute(&h.owner).await.unwrap();
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
    assert!(matches!(table, "audit_entries" | "account_security_events"));
    sqlx::query(AssertSqlSafe(format!("DROP TRIGGER test_gate ON {table}")))
        .execute(&h.owner)
        .await
        .unwrap();
}
async fn blocked_by(h: &Harness, blocker: i32) -> i32 {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if let Some(pid) = sqlx::query_scalar(
                "SELECT pid FROM pg_stat_activity WHERE $1=ANY(pg_blocking_pids(pid)) LIMIT 1",
            )
            .bind(blocker)
            .fetch_optional(&h.owner)
            .await
            .unwrap()
            {
                return pid;
            }
            // Polling only observes the database wait graph; it never selects
            // which operation wins or assumes a scheduler delay establishes it.
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("Observe actual PostgreSQL lock dependency")
}
async fn enabled_owners(h: &Harness, owner: &Actor, workspace: Uuid) -> i64 {
    let mut tx = fixture(h, owner.user.id, workspace).await;
    let count = sqlx::query_scalar("SELECT count(*) FROM workspace_members m JOIN users u ON u.id=m.user_id WHERE m.workspace_id=$1 AND m.role='owner' AND m.active AND u.disabled_at IS NULL")
        .bind(workspace).fetch_one(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    count
}
async fn has_membership(h: &Harness, user: Uuid, workspace: Uuid) -> bool {
    let mut tx = fixture(h, user, workspace).await;
    let exists = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM workspace_members WHERE workspace_id=$1 AND user_id=$2 AND active)")
        .bind(workspace).bind(user).fetch_one(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    exists
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_postgres_account_owner_and_inviter_races() {
    let h = Harness::with_connections(6).await;
    let runner = h.clone();
    let result = tokio::spawn(async move { exercise(&runner).await }).await;
    h.cleanup().await;
    if let Err(error) = result {
        std::panic::resume_unwind(error.into_panic());
    }
}
async fn exercise(h: &Harness) {
    let (session,cookie)=h.app.setup(request(json!({"setup_token":h.setup_token,"email":"owner@lifecycle-race.invalid","password":PASSWORD,"display_name":"Race owner","workspace_name":"Race home","team_name":"Engineering","team_key":"ENG"}))).await.unwrap();
    let owner = h.app.authenticate(&cookie).await.unwrap();
    let home = session.workspaces[0].id;
    let organization = h.app.organizations(&owner).await.unwrap()[0].id;
    let operator = Operator::connect(&h.operator_url().await).await.unwrap();
    sqlx::raw_sql("CREATE FUNCTION test_lifecycle_race_gate() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.action=TG_ARGV[1] THEN PERFORM set_config('statement_timeout','15000',true); PERFORM set_config('lock_timeout','15000',true); PERFORM pg_advisory_xact_lock(TG_ARGV[0]::bigint); END IF; RETURN NEW; END $$;")
        .execute(&h.owner).await.unwrap();
    for disable_first in [true, false] {
        owner_change_race(h, &operator, &owner, organization, disable_first).await;
        invitation_race(h, &operator, &owner, home, organization, disable_first).await;
    }
    legacy_enable_does_not_resurrect_invitation(h, &operator, &owner, home, organization).await;
}

async fn owner_change_race(
    h: &Harness,
    operator: &Operator,
    owner: &Actor,
    organization: Uuid,
    disable_first: bool,
) {
    let w = workspace(
        h,
        owner,
        organization,
        if disable_first {
            "disable-before-demotion"
        } else {
            "demotion-before-disable"
        },
    )
    .await;
    let target = account(h, owner, w, "owner").await;
    // An active but disabled third owner cannot count as a usable fallback.
    let disabled_backup = account(h, owner, w, "owner").await;
    operator
        .disable_account(disabled_backup.id, REASON)
        .await
        .unwrap();
    let old = h
        .app
        .get_workspace_member(owner, w, owner.user.id)
        .await
        .unwrap();
    let etag = old.etag(w);
    let table = if disable_first {
        "account_security_events"
    } else {
        "audit_entries"
    };
    let action = if disable_first {
        "account.disabled"
    } else {
        "workspace.member.updated"
    };
    let (lock, pid) = gate(h, table, action).await;
    let app = h.app.clone();
    let initiator = owner.clone();
    let operator = operator.clone();
    if disable_first {
        let disable =
            tokio::spawn(async move { operator.disable_account(target.id, REASON).await });
        let disable_pid = blocked_by(h, pid).await;
        let demote = tokio::spawn(async move {
            app.update_member(
                &initiator,
                w,
                initiator.user.id,
                &etag,
                request(json!({"role":"admin"})),
            )
            .await
        });
        blocked_by(h, disable_pid).await;
        lock.commit().await.unwrap();
        assert!(disable.await.unwrap().unwrap().disabled);
        assert!(
            matches!(demote.await.unwrap(), Err(Error::Conflict(_))),
            "Demotion must see that the other owners are disabled"
        );
        assert_eq!(
            h.app
                .get_workspace_member(owner, w, owner.user.id)
                .await
                .unwrap()
                .role,
            "owner"
        );
    } else {
        let demote = tokio::spawn(async move {
            app.update_member(
                &initiator,
                w,
                initiator.user.id,
                &etag,
                request(json!({"role":"admin"})),
            )
            .await
        });
        let demotion_pid = blocked_by(h, pid).await;
        let disable =
            tokio::spawn(async move { operator.disable_account(target.id, REASON).await });
        blocked_by(h, demotion_pid).await;
        lock.commit().await.unwrap();
        let demoted = demote.await.unwrap().unwrap();
        assert_eq!(demoted.entity.role, "admin");
        assert!(!demoted.current_session_revoked);
        assert!(
            matches!(disable.await.unwrap(), Err(Error::Conflict(_))),
            "Disable must see the committed owner demotion"
        );
    }
    remove_gate(h, table).await;
    assert_eq!(enabled_owners(h, owner, w).await, 1);
    assert!(h.app.session(owner).await.is_ok());
}

async fn invitation_race(
    h: &Harness,
    operator: &Operator,
    owner: &Actor,
    home: Uuid,
    organization: Uuid,
    disable_first: bool,
) {
    let w = workspace(
        h,
        owner,
        organization,
        if disable_first {
            "disable-before-accept"
        } else {
            "accept-before-disable"
        },
    )
    .await;
    let inviter = account(h, owner, w, "admin").await;
    let recipient = account(h, owner, home, "member").await;
    let (inviter_actor, _) = login(h, &inviter).await;
    let (recipient_actor, recipient_cookie) = login(h, &recipient).await;
    let issued = h
        .app
        .create_invitation(
            &inviter_actor,
            w,
            &Uuid::new_v4().to_string(),
            request(json!({"email":recipient.email,"role":"member","team_ids":[]})),
        )
        .await
        .unwrap();
    let token = issued.token.unwrap();
    let acceptance: AcceptInvitation = request(json!({"token":token}));
    let table = if disable_first {
        "account_security_events"
    } else {
        "audit_entries"
    };
    let action = if disable_first {
        "account.disabled"
    } else {
        "invitation.accepted"
    };
    let (lock, pid) = gate(h, table, action).await;
    let app = h.app.clone();
    let actor = recipient_actor.clone();
    let op = operator.clone();
    if disable_first {
        let disable = tokio::spawn(async move { op.disable_account(inviter.id, REASON).await });
        let disable_pid = blocked_by(h, pid).await;
        let accept =
            tokio::spawn(async move { app.accept_invitation(Some(&actor), acceptance).await });
        blocked_by(h, disable_pid).await;
        lock.commit().await.unwrap();
        let outcome = disable.await.unwrap().unwrap();
        assert_eq!(outcome.revoked_invitations, 1);
        assert!(matches!(accept.await.unwrap(), Err(Error::NotFound)));
        assert!(!has_membership(h, recipient.id, w).await);
        assert!(h.app.authenticate(&recipient_cookie).await.is_ok());
        operator.enable_account(inviter.id, REASON).await.unwrap();
        assert!(matches!(
            h.app.invitation_preview(&token).await,
            Err(Error::NotFound)
        ));
        assert!(matches!(
            h.app
                .accept_invitation(Some(&recipient_actor), request(json!({"token":token})))
                .await,
            Err(Error::NotFound)
        ));
        assert!(!has_membership(h, recipient.id, w).await);
    } else {
        let accept =
            tokio::spawn(async move { app.accept_invitation(Some(&actor), acceptance).await });
        let acceptance_pid = blocked_by(h, pid).await;
        let disable = tokio::spawn(async move { op.disable_account(inviter.id, REASON).await });
        blocked_by(h, acceptance_pid).await;
        lock.commit().await.unwrap();
        let (_, new_cookie) = accept.await.unwrap().unwrap();
        assert_eq!(disable.await.unwrap().unwrap().revoked_invitations, 0);
        assert!(has_membership(h, recipient.id, w).await);
        let accepted = h.app.authenticate(&new_cookie).await.unwrap();
        assert!(h.app.bootstrap(&accepted, w).await.is_ok());
        assert!(matches!(
            h.app.authenticate(&recipient_cookie).await,
            Err(Error::Unauthorized)
        ));
        let invitation = h
            .app
            .get_invitation(owner, w, issued.invitation.id)
            .await
            .unwrap();
        assert!(invitation.accepted_at.is_some() && invitation.revoked_at.is_none());
        operator.enable_account(inviter.id, REASON).await.unwrap();
        assert!(matches!(
            h.app.invitation_preview(&token).await,
            Err(Error::NotFound)
        ));
        assert!(h.app.authenticate(&new_cookie).await.is_ok());
    }
    remove_gate(h, table).await;
}

async fn legacy_enable_does_not_resurrect_invitation(
    h: &Harness,
    operator: &Operator,
    owner: &Actor,
    home: Uuid,
    organization: Uuid,
) {
    let w = workspace(h, owner, organization, "legacy-enable-invitation").await;
    let inviter = account(h, owner, w, "admin").await;
    let recipient = account(h, owner, home, "member").await;
    let (actor, cookie) = login(h, &inviter).await;
    let (recipient_actor, _) = login(h, &recipient).await;
    let issued = h
        .app
        .create_invitation(
            &actor,
            w,
            &Uuid::new_v4().to_string(),
            request(json!({"email":recipient.email,"role":"member","team_ids":[]})),
        )
        .await
        .unwrap();
    // Represent a pre-0007 row: disabled_at was set without expiring bearers.
    // Only this fixture's migration owner can create that historical state now.
    let mut tx = fixture(h, inviter.id, w).await;
    sqlx::query("UPDATE users SET disabled_at=clock_timestamp() WHERE id=$1")
        .bind(inviter.id)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let enabled = operator.enable_account(inviter.id, REASON).await.unwrap();
    assert!(enabled.changed && !enabled.disabled);
    assert_eq!(
        (enabled.revoked_sessions, enabled.revoked_invitations),
        (1, 1)
    );
    assert!(matches!(
        h.app.authenticate(&cookie).await,
        Err(Error::Unauthorized)
    ));
    assert!(matches!(
        h.app
            .accept_invitation(
                Some(&recipient_actor),
                request(json!({"token":issued.token.unwrap()}))
            )
            .await,
        Err(Error::NotFound)
    ));
    assert!(!has_membership(h, recipient.id, w).await);
    assert!(login(h, &inviter).await.0.user.id == inviter.id);
}
