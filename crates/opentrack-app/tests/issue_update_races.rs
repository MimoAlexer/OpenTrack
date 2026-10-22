mod graph_support;
#[allow(dead_code)]
mod support;

use graph_support::*;
use opentrack_domain::Error;
use serde_json::json;
use support::Harness;
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_postgres_issue_updates_commit_one_observed_winner() {
    let h = Harness::with_connections(8).await;
    let copy = h.clone();
    let run = tokio::spawn(async move { exercise(&copy).await }).await;
    h.cleanup().await;
    if let Err(error) = run {
        std::panic::resume_unwind(error.into_panic());
    }
}

async fn exercise(h: &Harness) {
    let (owner, workspace, team) = setup(h).await;
    let member = person(h, &owner, workspace, "member").await;
    let candidates = [
        (&owner, json!({"title":"Owner update","priority":1})),
        (&member, json!({"title":"Member update","estimate":8})),
    ];
    for first_index in [0, 1] {
        let original = issue(h, &owner, workspace, team, json!({})).await;
        let (holder, holder_pid) = gate(h, &owner, workspace, team, Some(original.id)).await;
        let update = |index: usize| {
            let app = h.app.clone();
            let actor = candidates[index].0.clone();
            let patch = candidates[index].1.clone();
            let original = original.clone();
            async move {
                app.update_issue(&actor, workspace, original.id, &original.etag(), patch)
                    .await
            }
        };
        let first = tokio::spawn(update(first_index));
        let first_pid = blocked(h, holder_pid).await;
        let second = tokio::spawn(update(1 - first_index));
        blocked(h, first_pid).await;
        holder.commit().await.unwrap();

        let winner = first.await.unwrap().unwrap();
        let stale = match second.await.unwrap() {
            Err(Error::Stale(current)) => current,
            other => panic!("Expected a stale concurrent update, received {other:?}"),
        };
        advanced(&original, &winner);
        assert_eq!(json!(stale), json!(winner));
        assert_eq!(
            winner.title,
            candidates[first_index].1["title"].as_str().unwrap()
        );
        assert_eq!(winner.priority, if first_index == 0 { 1 } else { 0 });
        assert_eq!(
            winner.estimate,
            if first_index == 1 { Some(8) } else { None }
        );
        let canonical = fresh(h, &owner, workspace, original.id).await;
        assert_eq!(json!(canonical), json!(winner));

        let activity = h
            .app
            .issue_activity(&owner, workspace, original.id)
            .await
            .unwrap();
        assert_eq!(activity.len(), 2);
        let updates: Vec<_> = activity
            .iter()
            .filter(|entry| entry["event_kind"] == "issue.updated")
            .collect();
        assert_eq!(updates.len(), 1);
        assert_eq!(
            updates[0]["actor_id"],
            json!(candidates[first_index].0.user.id)
        );
        assert_eq!(
            updates[0]["changes"],
            if first_index == 0 {
                json!({"fields":["title","priority"],"priority":{"before":0,"after":1},"source":"manual"})
            } else {
                json!({"fields":["title","estimate"],"estimate":{"before":null,"after":8},"source":"manual"})
            }
        );
        let mut tx = h.context(owner.user.id, workspace).await;
        let events: Vec<(String, i64, Uuid)> = sqlx::query_as(
            "SELECT event_type,entity_version,actor_id FROM workspace_events WHERE workspace_id=$1 AND entity_id=$2 ORDER BY seq",
        )
        .bind(workspace)
        .bind(original.id)
        .fetch_all(&mut *tx)
        .await
        .unwrap();
        tx.commit().await.unwrap();
        assert_eq!(
            events,
            vec![
                ("issue.created".into(), 1, owner.user.id),
                ("issue.updated".into(), 2, candidates[first_index].0.user.id),
            ]
        );
    }
}
