//! Team serialization prevents historical issue moves from inverting head locks.
mod graph_support;
#[allow(dead_code)]
mod support;
use graph_support::{blocked, gate, issue, move_body, req, setup, team};
use opentrack_domain::*;
use serde_json::json;
use support::Harness;
use uuid::Uuid;
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn historical_readdition_and_team_move_serialize_in_both_observed_orders() {
    let h = Harness::with_connections(8).await;
    let copy = h.clone();
    let run = tokio::spawn(async move { exercise(&copy).await }).await;
    h.cleanup().await;
    if let Err(error) = run {
        std::panic::resume_unwind(error.into_panic())
    }
}
async fn exercise(h: &Harness) {
    let (a, w, t) = setup(h).await;
    for move_first in [true, false] {
        let destination = team(h, &a, w, true).await;
        let p = h
            .app
            .create_project(
                &a,
                w,
                t,
                &Uuid::new_v4().to_string(),
                req(json!({"name":"Historical move race"})),
            )
            .await
            .unwrap();
        let i = issue(h, &a, w, t, json!({})).await;
        let p = h
            .app
            .set_project_issue(
                &a,
                w,
                t,
                p.id,
                i.id,
                &p.etag,
                ProjectMembership { included: true },
            )
            .await
            .unwrap();
        h.app
            .set_project_issue(
                &a,
                w,
                t,
                p.id,
                i.id,
                &p.etag,
                ProjectMembership { included: false },
            )
            .await
            .unwrap();
        let view = h
            .app
            .project_scope_history(&a, w, t, p.id, ProjectScopeQuery::default())
            .await
            .unwrap();
        let preview = h
            .app
            .preview_issue_move(&a, w, i.id, destination)
            .await
            .unwrap();
        let movement = move_body(&preview);
        let issue_etag = preview.issue_etag;
        let (held, pid) = gate(h, &a, w, t, None).await;
        let add = || {
            let h = h.clone();
            let a = a.clone();
            let token = view.scope_etag.clone();
            tokio::spawn(async move {
                h.app
                    .change_project_scope(
                        &a,
                        w,
                        t,
                        p.id,
                        &token,
                        &Uuid::new_v4().to_string(),
                        ChangeProjectScope {
                            issue_id: i.id,
                            action: ProjectScopeAction::Added,
                            reason: "Resume reviewed work".into(),
                        },
                    )
                    .await
                    .map(|_| ())
            })
        };
        let moving = || {
            let h = h.clone();
            let a = a.clone();
            let etag = issue_etag.clone();
            let movement = movement.clone();
            tokio::spawn(async move {
                h.app
                    .move_issue(&a, w, i.id, &Uuid::new_v4().to_string(), &etag, movement)
                    .await
                    .map(|_| ())
            })
        };
        let first = if move_first { moving() } else { add() };
        let first_pid = blocked(h, pid).await;
        let second = if move_first { add() } else { moving() };
        tokio::time::timeout(std::time::Duration::from_secs(3),async{
   loop{let seen:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE pid<>$2 AND ($1=ANY(pg_blocking_pids(pid)) OR $2=ANY(pg_blocking_pids(pid))))").bind(pid).bind(first_pid).fetch_one(&h.owner).await.unwrap();if seen{break}tokio::time::sleep(std::time::Duration::from_millis(5)).await}
  }).await.expect("Observe both actual operations queued on the source team");
        held.commit().await.unwrap();
        let (first, second) = tokio::time::timeout(std::time::Duration::from_secs(3), async {
            tokio::join!(first, second)
        })
        .await
        .expect("No issue/head deadlock");
        assert!(first.unwrap().is_ok());
        let loser = second.unwrap();
        if move_first {
            assert!(matches!(loser, Err(Error::NotFound)));
            assert_eq!(
                h.app.get_issue(&a, w, i.id).await.unwrap().team_id,
                destination
            )
        } else {
            assert!(matches!(loser, Err(Error::Conflict(_))));
            assert_eq!(h.app.get_issue(&a, w, i.id).await.unwrap().team_id, t)
        }
    }
}
