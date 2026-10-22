//! Observe both actual PostgreSQL lock orders, never infer order from sleeps.
#[allow(dead_code)]
mod support;
use opentrack_app::Operator;
use opentrack_domain::*;
use opentrack_store as store;
use serde_json::{Value, json};
use support::Harness;
use uuid::Uuid;
fn req<T: serde::de::DeserializeOwned>(v: Value) -> T {
    serde_json::from_value(v).unwrap()
}
async fn gate(h: &Harness, a: &Actor, w: Uuid, issue: Option<Uuid>) -> (store::Tx, i32) {
    let mut tx = h.context(a.user.id, w).await;
    match issue {
        Some(id) => {
            sqlx::query("SELECT id FROM issues WHERE workspace_id=$1 AND id=$2 FOR UPDATE")
                .bind(w)
                .bind(id)
                .execute(&mut *tx)
                .await
                .unwrap();
        }
        None => {
            sqlx::query(
                "SELECT workspace_id FROM workspace_auth_guards WHERE workspace_id=$1 FOR UPDATE",
            )
            .bind(w)
            .execute(&mut *tx)
            .await
            .unwrap();
        }
    }
    let pid = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    (tx, pid)
}
async fn blocked(h: &Harness, pid: i32) -> i32 {
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            let result: Option<i32> = sqlx::query_scalar(
                "SELECT pid FROM pg_stat_activity WHERE $1=ANY(pg_blocking_pids(pid)) LIMIT 1",
            )
            .bind(pid)
            .fetch_optional(&h.owner)
            .await
            .unwrap();
            if let Some(waiter) = result {
                return waiter;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("Observe the real queued PostgreSQL lock")
}
async fn team(h: &Harness, a: &Actor, w: Uuid, private: bool) -> Uuid {
    let key = Uuid::new_v4().simple().to_string();
    h.app
        .create_team(
            a,
            w,
            &key,
            req(json!({"name":key,"key":format!("R{}",&key[..6]),"private":private})),
        )
        .await
        .unwrap()
        .id
}
async fn issue(h: &Harness, a: &Actor, w: Uuid, t: Uuid) -> Issue {
    h.app
        .create_issue(
            a,
            w,
            &Uuid::new_v4().to_string(),
            req(json!({"team_id":t,"title":"Racing assignment"})),
        )
        .await
        .unwrap()
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_postgres_taxonomy_observed_races() {
    let h = Harness::with_connections(8).await;
    let copy = h.clone();
    let run = tokio::spawn(async move { exercise(&copy).await }).await;
    h.cleanup().await;
    if let Err(e) = run {
        std::panic::resume_unwind(e.into_panic());
    }
}
async fn exercise(h: &Harness) {
    let (session,cookie)=h.app.setup(req(json!({"setup_token":h.setup_token,"email":"races@taxonomy.invalid","password":"races contract password 694!","display_name":"Race owner","workspace_name":"Races","team_name":"Race","team_key":"RACE"}))).await.unwrap();
    let a = h.app.authenticate(&cookie).await.unwrap();
    let w = session.workspaces[0].id;
    let t = h.app.bootstrap(&a, w).await.unwrap().teams[0].id;
    for shared in [true, false] {
        for archive_first in [true, false] {
            archive_race(h, &a, w, t, shared, archive_first).await;
        }
    }
    let operator = Operator::connect(&h.operator_url().await).await.unwrap();
    for loss in [Loss::Membership, Loss::PrivateGrant, Loss::Disabled] {
        for revoke_first in [true, false] {
            assignment_race(h, &a, w, &operator, loss, revoke_first).await;
        }
    }
}
async fn archive_race(h: &Harness, a: &Actor, w: Uuid, t: Uuid, shared: bool, archive_first: bool) {
    let label=h.app.create_label(a,w,&Uuid::new_v4().to_string(),req(json!({"team_id":if shared{None}else{Some(t)},"name":format!("Race {}",Uuid::new_v4())}))).await.unwrap();
    let item = issue(h, a, w, t).await;
    // Shared labels serialize at the workspace guard. Team labels serialize at
    // the team, so their archive-first fixture holds that team instead.
    eprintln!("archive order: shared={shared}, archive_first={archive_first}");
    let (holder, pid) = if archive_first && shared {
        let mut tx = h.context(a.user.id, w).await;
        sqlx::query("SELECT id FROM labels WHERE workspace_id=$1 AND id=$2 FOR UPDATE")
            .bind(w)
            .bind(label.id)
            .execute(&mut *tx)
            .await
            .unwrap();
        let pid = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        (tx, pid)
    } else if archive_first {
        let mut tx = h.context(a.user.id, w).await;
        sqlx::query("SELECT id FROM teams WHERE workspace_id=$1 AND id=$2 FOR UPDATE")
            .bind(w)
            .bind(t)
            .execute(&mut *tx)
            .await
            .unwrap();
        let pid = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        (tx, pid)
    } else {
        gate(h, a, w, if archive_first { None } else { Some(item.id) }).await
    };
    let app = h.app.clone();
    let actor = a.clone();
    let l = label.clone();
    let archive = async move {
        app.update_label(&actor, w, l.id, &l.etag(), req(json!({"archived":true})))
            .await
    };
    let app = h.app.clone();
    let actor = a.clone();
    let old = item.clone();
    let assign = async move {
        app.update_issue(
            &actor,
            w,
            old.id,
            &old.etag(),
            json!({"label_ids":[label.id]}),
        )
        .await
    };
    let (archived, applied) = if archive_first {
        let first = tokio::spawn(archive);
        let first_pid = blocked(h, pid).await;
        let second = tokio::spawn(assign);
        blocked(h, first_pid).await;
        holder.commit().await.unwrap();
        (first.await.unwrap(), second.await.unwrap())
    } else {
        let first = tokio::spawn(assign);
        let first_pid = blocked(h, pid).await;
        let second = tokio::spawn(archive);
        blocked(h, first_pid).await;
        holder.commit().await.unwrap();
        (second.await.unwrap(), first.await.unwrap())
    };
    assert!(archived.unwrap().archived_at.is_some());
    let current = h.app.get_issue(a, w, item.id).await.unwrap();
    if archive_first {
        assert!(matches!(applied, Err(Error::Invalid(_))));
        assert!(current.labels.is_empty());
        assert_eq!(current.version, item.version);
    } else {
        applied.unwrap();
        assert_eq!(current.label_ids, [label.id]);
        assert!(current.labels[0].archived_at.is_some());
    }
}
#[derive(Clone, Copy)]
enum Loss {
    Membership,
    PrivateGrant,
    Disabled,
}
async fn assignment_race(
    h: &Harness,
    a: &Actor,
    w: Uuid,
    operator: &Operator,
    loss: Loss,
    revoke_first: bool,
) {
    eprintln!(
        "assignment order: loss={}, revoke_first={revoke_first}",
        match loss {
            Loss::Membership => "membership",
            Loss::PrivateGrant => "private",
            Loss::Disabled => "disabled",
        }
    );
    let t = team(h, a, w, matches!(loss, Loss::PrivateGrant)).await;
    let target = Uuid::new_v4();
    let mut tx = h.context(a.user.id, w).await;
    sqlx::query("INSERT INTO users(id,email,display_name,password_hash) SELECT $1,$2,'Historical race assignee',password_hash FROM users WHERE id=$3").bind(target).bind(format!("{target}@races.invalid")).bind(a.user.id).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO workspace_members(workspace_id,user_id,role) VALUES($1,$2,'member')")
        .bind(w)
        .bind(target)
        .execute(&mut *tx)
        .await
        .unwrap();
    if matches!(loss, Loss::PrivateGrant) {
        sqlx::query("INSERT INTO team_members(workspace_id,team_id,user_id,role) VALUES($1,$2,$3,'contributor')").bind(w).bind(t).bind(target).execute(&mut *tx).await.unwrap();
    }
    tx.commit().await.unwrap();
    let item = issue(h, a, w, t).await;
    let member = h.app.get_workspace_member(a, w, target).await.unwrap();
    let team_member = if matches!(loss, Loss::PrivateGrant) {
        Some(h.app.get_team_member(a, w, t, target).await.unwrap())
    } else {
        None
    };
    let (holder, pid) = gate(h, a, w, if revoke_first { None } else { Some(item.id) }).await;
    let app = h.app.clone();
    let actor = a.clone();
    let op = operator.clone();
    let revoke = async move {
        match loss {
            Loss::Membership => app
                .update_member(
                    &actor,
                    w,
                    target,
                    &member.etag(w),
                    req(json!({"active":false})),
                )
                .await
                .map(|_| ()),
            Loss::PrivateGrant => {
                app.remove_team_member(&actor, w, t, target, &team_member.unwrap().etag(t))
                    .await
            }
            Loss::Disabled => op
                .disable_account(target, "Assignment race qualification")
                .await
                .map(|_| ()),
        }
    };
    let app = h.app.clone();
    let actor = a.clone();
    let old = item.clone();
    let assign = async move {
        app.update_issue(
            &actor,
            w,
            old.id,
            &old.etag(),
            json!({"assignee_id":target}),
        )
        .await
    };
    let (revoked, assigned) = if revoke_first {
        let first = tokio::spawn(revoke);
        let first_pid = blocked(h, pid).await;
        let second = tokio::spawn(assign);
        blocked(h, first_pid).await;
        holder.commit().await.unwrap();
        (first.await.unwrap(), second.await.unwrap())
    } else {
        let first = tokio::spawn(assign);
        let first_pid = blocked(h, pid).await;
        let second = tokio::spawn(revoke);
        blocked(h, first_pid).await;
        holder.commit().await.unwrap();
        (second.await.unwrap(), first.await.unwrap())
    };
    revoked.unwrap();
    let current = h.app.get_issue(a, w, item.id).await.unwrap();
    if revoke_first {
        assert!(matches!(assigned, Err(Error::Invalid(_))));
        assert!(current.assignee.is_none());
        assert_eq!(current.version, item.version);
    } else {
        assigned.unwrap();
        assert_eq!(current.assignee_id, Some(target));
        assert_eq!(
            current.assignee.as_ref().unwrap().display_name,
            "Historical race assignee"
        );
        assert!(!current.assignee.as_ref().unwrap().assignable);
        let retained = h
            .app
            .update_issue(
                a,
                w,
                current.id,
                &current.etag(),
                json!({"title":"Unrelated edit keeps history"}),
            )
            .await
            .unwrap();
        assert_eq!(retained.assignee_id, Some(target));
    }
    let available = h
        .app
        .eligible_assignees(a, w, t, req(json!({"q":"Historical race assignee"})))
        .await
        .unwrap();
    assert!(!available.data.iter().any(|u| u.id == target));
    assert!(matches!(
        h.app
            .create_issue(
                a,
                w,
                &Uuid::new_v4().to_string(),
                req(json!({"team_id":t,"title":"New unavailable assignment","assignee_id":target}))
            )
            .await,
        Err(Error::Invalid(_))
    ));
}
