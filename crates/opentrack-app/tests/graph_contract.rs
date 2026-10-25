//! Real scoped graph invariants, historical cleanup, paging and atomic effects.
mod graph_support;
#[allow(dead_code)]
mod support;
use graph_support::*;
use opentrack_app::worker::{Worker, WorkerOptions, WorkerTick};
use opentrack_domain::*;
use serde_json::json;
use support::Harness;
use uuid::Uuid;
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_postgres_graph_contract() {
    let h = Harness::with_connections(6).await;
    let copy = h.clone();
    let run = tokio::spawn(async move { exercise(&copy).await }).await;
    if let Err(e) = run {
        std::panic::resume_unwind(e.into_panic());
    }
    h.cleanup().await;
}
async fn exercise(h: &Harness) {
    let (a, w, t) = setup(h).await;
    hierarchy(h, &a, w, t).await;
    relationships(h, &a, w, t).await;
    historical_repair(h, &a, w, t).await;
    paging_and_authority(h, &a, w, t).await;
    atomicity(h, &a, w, t).await;
    restricted_worker(h, &a, w).await;
    capacity(h, &a, w).await;
}
async fn hierarchy(h: &Harness, a: &Actor, w: Uuid, t: Uuid) {
    let root = issue(h, a, w, t, json!({"title":"Hierarchy root"})).await;
    let mut chain = vec![root.clone()];
    for level in 2..=8 {
        let p = fresh(h, a, w, chain.last().unwrap().id).await;
        let mut tx = h.context(a.user.id, w).await;
        let previous_seq: i64 =
            sqlx::query_scalar("SELECT last_seq FROM workspace_stream_heads WHERE workspace_id=$1")
                .bind(w)
                .fetch_one(&mut *tx)
                .await
                .unwrap();
        tx.commit().await.unwrap();
        let explicit_mutation = (level == 2).then(Uuid::new_v4);
        let mut body =
            json!({"title":format!("Level {level}"),"parent_id":p.id,"parent_etag":p.etag()});
        if let Some(mutation) = explicit_mutation {
            body["mutation_id"] = json!(mutation);
        }
        let child = issue(h, a, w, t, body).await;
        advanced(&p, &fresh(h, a, w, p.id).await);
        let mut tx = h.context(a.user.id, w).await;
        let events: Vec<(i64, Uuid, Uuid)> = sqlx::query_as("SELECT seq,entity_id,mutation_id FROM workspace_events WHERE workspace_id=$1 AND seq>$2 ORDER BY seq")
            .bind(w).bind(previous_seq).fetch_all(&mut *tx).await.unwrap();
        tx.commit().await.unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(
            (events[0].0, events[1].0),
            (previous_seq + 1, previous_seq + 2)
        );
        assert_eq!((events[0].1, events[1].1), (p.id, child.id));
        assert_eq!(
            events[0].2, events[1].2,
            "Parent and child share one command mutation"
        );
        if let Some(mutation) = explicit_mutation {
            assert_eq!(
                events[0].2, mutation,
                "Preserve the caller's explicit mutation UUID"
            );
        }
        chain.push(child);
    }
    let deepest = fresh(h, a, w, chain[7].id).await;
    let before = snapshot(h, a, w).await;
    assert!(matches!(h.app.create_issue(a,w,"ninth-level-refused",req(json!({"team_id":t,"title":"Level9","parent_id":deepest.id,"parent_etag":deepest.etag()}))).await,Err(Error::Conflict(_))));
    assert_eq!(snapshot(h, a, w).await, before);
    let root_now = fresh(h, a, w, root.id).await;
    assert!(matches!(
        h.app
            .set_issue_parent(
                a,
                w,
                root.id,
                &root_now.etag(),
                req(json!({"parent_id":deepest.id,"parent_etag":deepest.etag()}))
            )
            .await,
        Err(Error::Conflict(_))
    ));
    let outside = issue(h, a, w, t, json!({})).await;
    assert!(matches!(
        h.app
            .set_issue_parent(
                a,
                w,
                root.id,
                &root_now.etag(),
                req(json!({"parent_id":outside.id,"parent_etag":outside.etag()}))
            )
            .await,
        Err(Error::Conflict(_))
    ));
    let read = h.app.issue_parent(a, w, deepest.id).await.unwrap();
    assert_eq!(read.parent.unwrap().target.unwrap().id, chain[6].id);
    assert!(
        serde_json::to_value(&deepest)
            .unwrap()
            .get("parent_id")
            .is_none()
    );
    let old_parent = fresh(h, a, w, chain[6].id).await;
    let epoch_before = epoch(h, a, w, t).await;
    let detached = parent(h, a, w, deepest.id, None).await;
    advanced(&deepest, &detached);
    advanced(&old_parent, &fresh(h, a, w, old_parent.id).await);
    assert_eq!(epoch(h, a, w, t).await, epoch_before + 1);
    assert!(
        h.app
            .issue_parent(a, w, deepest.id)
            .await
            .unwrap()
            .parent
            .is_none()
    );
    let mut tx = h.context(a.user.id, w).await;
    assert!(
        sqlx::query("UPDATE issues SET parent_id=id WHERE id=$1")
            .bind(root.id)
            .execute(&mut *tx)
            .await
            .is_err()
    );
    tx.rollback().await.unwrap();
    let other = team(h, a, w, false).await;
    let foreign = issue(h, a, w, other, json!({})).await;
    let mut tx = h.context(a.user.id, w).await;
    let attempt = sqlx::query("UPDATE issues SET parent_id=$2 WHERE id=$1")
        .bind(detached.id)
        .bind(foreign.id)
        .execute(&mut *tx)
        .await;
    if attempt.is_ok() {
        assert!(tx.commit().await.is_err());
    } else {
        tx.rollback().await.unwrap();
    }
}
async fn relationships(h: &Harness, a: &Actor, w: Uuid, t: Uuid) {
    let x = issue(h, a, w, t, json!({})).await;
    let y = issue(h, a, w, t, json!({})).await;
    let z = issue(h, a, w, t, json!({})).await;
    let epoch_before = epoch(h, a, w, t).await;
    let edge = relate(h, a, w, x.id, y.id, "blocks").await;
    advanced(&x, &edge.issue);
    advanced(&y, &fresh(h, a, w, y.id).await);
    assert_eq!(epoch(h, a, w, t).await, epoch_before + 1);
    assert_eq!(edge.relation.kind, RelationKind::Blocks);
    assert_eq!(
        h.app
            .issue_relations(a, w, y.id, req(json!({})))
            .await
            .unwrap()
            .data[0]
            .kind,
        RelationKind::BlockedBy
    );
    relate(h, a, w, y.id, z.id, "blocks").await;
    let zz = fresh(h, a, w, z.id).await;
    let xx = fresh(h, a, w, x.id).await;
    assert!(matches!(
        h.app
            .create_issue_relation(
                a,
                w,
                z.id,
                "graph-cycle-refused",
                &zz.etag(),
                req(json!({"kind":"blocks","target_id":x.id,"target_etag":xx.etag()}))
            )
            .await,
        Err(Error::Conflict(_))
    ));
    let mut tx = h.context(a.user.id, w).await;
    assert!(sqlx::query("INSERT INTO issue_relations(id,workspace_id,team_id,source_id,target_id,kind,created_by) VALUES(gen_random_uuid(),$1,$2,$3,$4,'blocks',$5)").bind(w).bind(t).bind(z.id).bind(x.id).bind(a.user.id).execute(&mut *tx).await.is_err());
    tx.rollback().await.unwrap();
    let r = issue(h, a, w, t, json!({})).await;
    let s = issue(h, a, w, t, json!({})).await;
    let old = r.etag();
    let body = json!({"kind":"related","target_id":s.id,"target_etag":s.etag()});
    let one = h
        .app
        .create_issue_relation(a, w, r.id, "related-replay-key", &old, req(body.clone()))
        .await
        .unwrap();
    let before = snapshot(h, a, w).await;
    let replay = h
        .app
        .create_issue_relation(a, w, r.id, "related-replay-key", &old, req(body))
        .await
        .unwrap();
    assert_eq!(one.relation.id, replay.relation.id);
    assert_eq!(one.issue.etag(), replay.issue.etag());
    assert_eq!(snapshot(h, a, w).await, before);
    let mut tx = h.context(a.user.id, w).await;
    let pair: (Uuid, Uuid) =
        sqlx::query_as("SELECT source_id,target_id FROM issue_relations WHERE id=$1")
            .bind(one.relation.id)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    assert!(pair.0 < pair.1);
    tx.commit().await.unwrap();
    let source = fresh(h, a, w, r.id).await;
    let target_before = fresh(h, a, w, s.id).await;
    let removed = h
        .app
        .remove_issue_relation(a, w, r.id, one.relation.id, &source.etag())
        .await
        .unwrap();
    advanced(&source, &removed);
    advanced(&target_before, &fresh(h, a, w, s.id).await);
    let duplicate = relate(h, a, w, r.id, s.id, "duplicate_of").await;
    assert_eq!(duplicate.relation.kind, RelationKind::DuplicateOf);
    let ss = fresh(h, a, w, s.id).await;
    let zz = fresh(h, a, w, z.id).await;
    assert!(matches!(
        h.app
            .create_issue_relation(
                a,
                w,
                s.id,
                "duplicate-chain-refused",
                &ss.etag(),
                req(json!({"kind":"duplicate_of","target_id":z.id,"target_etag":zz.etag()}))
            )
            .await,
        Err(Error::Conflict(_))
    ));
    let rr = fresh(h, a, w, r.id).await;
    assert!(matches!(
        h.app
            .create_issue_relation(
                a,
                w,
                z.id,
                "duplicate-target-refused",
                &zz.etag(),
                req(json!({"kind":"duplicate_of","target_id":r.id,"target_etag":rr.etag()}))
            )
            .await,
        Err(Error::Conflict(_))
    ));
    // Fresh source, stale selected target must preserve every effect.
    let current = fresh(h, a, w, x.id).await;
    let target = fresh(h, a, w, z.id).await;
    let changed = h
        .app
        .update_issue(
            a,
            w,
            z.id,
            &target.etag(),
            json!({"title":"Target changed"}),
        )
        .await
        .unwrap();
    let before = snapshot(h, a, w).await;
    match h
        .app
        .create_issue_relation(
            a,
            w,
            x.id,
            "stale-target-refused",
            &current.etag(),
            req(json!({"kind":"related","target_id":z.id,"target_etag":target.etag()})),
        )
        .await
    {
        Err(Error::VersionConflict(v)) => assert_eq!(v["target"]["etag"], changed.etag()),
        other => panic!("Expected target conflict, got {other:?}"),
    };
    assert_eq!(snapshot(h, a, w).await, before);
}
async fn historical_repair(h: &Harness, a: &Actor, w: Uuid, t: Uuid) {
    for deleted in [false, true] {
        let source = issue(h, a, w, t, json!({})).await;
        let target = issue(h, a, w, t, json!({"title":"Historical target canary"})).await;
        let link = relate(h, a, w, source.id, target.id, "related").await;
        state(h, a, w, target.id, deleted).await;
        let projection = h
            .app
            .get_issue_relation(a, w, source.id, link.relation.id)
            .await
            .unwrap();
        assert!(projection.can_remove);
        if deleted {
            assert!(projection.target.is_none());
            let encoded = serde_json::to_string(&projection).unwrap();
            assert!(!encoded.contains(&target.id.to_string()));
            assert!(!encoded.contains("Historical target canary"));
        }
        let source = fresh(h, a, w, source.id).await;
        let mut tx = h.context(a.user.id, w).await;
        let version: i64 = sqlx::query_scalar("SELECT version FROM issues WHERE id=$1")
            .bind(target.id)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        h.app
            .remove_issue_relation(a, w, source.id, link.relation.id, &source.etag())
            .await
            .unwrap();
        assert!(
            h.app
                .issue_relations(a, w, source.id, req(json!({})))
                .await
                .unwrap()
                .data
                .is_empty()
        );
        let mut tx = h.context(a.user.id, w).await;
        let after: i64 = sqlx::query_scalar("SELECT version FROM issues WHERE id=$1")
            .bind(target.id)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        assert_eq!(after, version + 1);
        if deleted {
            let activity =
                serde_json::to_string(&h.app.issue_activity(a, w, source.id).await.unwrap())
                    .unwrap();
            assert!(!activity.contains("Historical target canary"));
            assert!(!activity.contains(&target.id.to_string()));
        }
        let parent_item = issue(h, a, w, t, json!({"title":"Hidden parent canary"})).await;
        let child = issue(
            h,
            a,
            w,
            t,
            json!({"parent_id":parent_item.id,"parent_etag":parent_item.etag()}),
        )
        .await;
        state(h, a, w, parent_item.id, deleted).await;
        let p = h.app.issue_parent(a, w, child.id).await.unwrap();
        assert!(p.parent.as_ref().unwrap().can_detach);
        if deleted {
            assert!(p.parent.as_ref().unwrap().target.is_none());
            assert!(
                !serde_json::to_string(&p)
                    .unwrap()
                    .contains(&parent_item.id.to_string())
            );
        }
        parent(h, a, w, child.id, None).await;
        assert!(
            h.app
                .issue_parent(a, w, child.id)
                .await
                .unwrap()
                .parent
                .is_none()
        );
    }
    let source = issue(h, a, w, t, json!({})).await;
    let target = issue(h, a, w, t, json!({})).await;
    let link = relate(h, a, w, source.id, target.id, "related").await;
    state(h, a, w, source.id, false).await;
    let current = fresh(h, a, w, source.id).await;
    assert!(matches!(
        h.app
            .remove_issue_relation(a, w, source.id, link.relation.id, &current.etag())
            .await,
        Err(Error::Conflict(_))
    ));
}
async fn paging_and_authority(h: &Harness, a: &Actor, w: Uuid, t: Uuid) {
    let root = issue(h, a, w, t, json!({"title":"Paging root"})).await;
    for n in 0..23 {
        let p = fresh(h, a, w, root.id).await;
        issue(
            h,
            a,
            w,
            t,
            json!({"title":format!("Paging child {n}"),"parent_id":p.id,"parent_etag":p.etag()}),
        )
        .await;
    }
    let first = h
        .app
        .issue_children(a, w, root.id, req(json!({"limit":20})))
        .await
        .unwrap();
    assert_eq!(first.data.len(), 20);
    assert!(first.has_more);
    let cursor = first.next_cursor.unwrap();
    let last = h
        .app
        .issue_children(a, w, root.id, req(json!({"limit":20,"cursor":cursor})))
        .await
        .unwrap();
    assert_eq!(last.data.len(), 3);
    let mut ids = first
        .data
        .iter()
        .chain(last.data.iter())
        .map(|v| v.id)
        .collect::<Vec<_>>();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), 23);
    let member = person(h, a, w, "member").await;
    let viewer = person(h, a, w, "viewer").await;
    assert!(matches!(
        h.app
            .issue_children(
                &member,
                w,
                root.id,
                req(json!({"limit":20,"cursor":cursor}))
            )
            .await,
        Err(Error::Invalid(_))
    ));
    let current = fresh(h, a, w, root.id).await;
    issue(
        h,
        a,
        w,
        t,
        json!({"parent_id":root.id,"parent_etag":current.etag()}),
    )
    .await;
    assert!(matches!(
        h.app
            .issue_children(a, w, root.id, req(json!({"limit":20,"cursor":cursor})))
            .await,
        Err(Error::Invalid(_))
    ));
    let lookup = h
        .app
        .lookup_issues(
            a,
            w,
            req(json!({"team_id":t,"q":"Paging child","limit":20})),
        )
        .await
        .unwrap();
    assert_eq!(lookup.data.len(), 20);
    assert!(lookup.has_more);
    assert!(!serde_json::to_string(&lookup).unwrap().contains("@"));
    let private = team(h, a, w, true).await;
    let hidden = issue(h, a, w, private, json!({"title":"Private graph canary"})).await;
    assert!(matches!(
        h.app
            .lookup_issues(&member, w, req(json!({"team_id":private})))
            .await,
        Err(Error::NotFound)
    ));
    assert!(matches!(
        h.app.issue_parent(&member, w, hidden.id).await,
        Err(Error::NotFound)
    ));
    let current = fresh(h, a, w, root.id).await;
    assert!(matches!(
        h.app
            .create_issue_relation(
                &member,
                w,
                root.id,
                "hidden-graph-target",
                &current.etag(),
                req(json!({"kind":"related","target_id":hidden.id,"target_etag":hidden.etag()}))
            )
            .await,
        Err(Error::NotFound)
    ));
    assert!(matches!(
        h.app
            .set_issue_parent(
                &viewer,
                w,
                root.id,
                &current.etag(),
                req(json!({"parent_id":null}))
            )
            .await,
        Err(Error::Forbidden)
    ));
    for table in ["issue_relations", "issue_key_aliases"] {
        let count: i64 =
            sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT count(*) FROM {table}")))
                .fetch_one(&h.app.pool)
                .await
                .unwrap();
        assert_eq!(count, 0);
    }
}
async fn atomicity(h: &Harness, a: &Actor, w: Uuid, t: Uuid) {
    let source = issue(h, a, w, t, json!({})).await;
    let target = issue(h, a, w, t, json!({})).await;
    for (table, event) in [
        ("issue_relations", "INSERT"),
        ("issues", "UPDATE"),
        ("teams", "UPDATE"),
        ("idempotency_records", "UPDATE"),
        ("activity_entries", "INSERT"),
        ("workspace_events", "INSERT"),
    ] {
        let before = snapshot(h, a, w).await;
        fault(h, table, event).await;
        let result = h
            .app
            .create_issue_relation(
                a,
                w,
                source.id,
                "graph-atomic-relation",
                &source.etag(),
                req(json!({"kind":"related","target_id":target.id,"target_etag":target.etag()})),
            )
            .await;
        clear(h, table).await;
        assert!(result.is_err(), "{table} {event}");
        assert_eq!(snapshot(h, a, w).await, before);
    }
    let link = relate(h, a, w, source.id, target.id, "related").await;
    let current = fresh(h, a, w, source.id).await;
    for (table, event) in [
        ("issue_relations", "DELETE"),
        ("issues", "UPDATE"),
        ("teams", "UPDATE"),
        ("activity_entries", "INSERT"),
        ("workspace_events", "INSERT"),
    ] {
        let before = snapshot(h, a, w).await;
        fault(h, table, event).await;
        let result = h
            .app
            .remove_issue_relation(a, w, source.id, link.relation.id, &current.etag())
            .await;
        clear(h, table).await;
        assert!(result.is_err());
        assert_eq!(snapshot(h, a, w).await, before);
    }
    let child = issue(h, a, w, t, json!({})).await;
    let target = fresh(h, a, w, target.id).await;
    for (table, event) in [
        ("issues", "UPDATE"),
        ("teams", "UPDATE"),
        ("activity_entries", "INSERT"),
        ("workspace_events", "INSERT"),
    ] {
        let before = snapshot(h, a, w).await;
        fault(h, table, event).await;
        let result = h
            .app
            .set_issue_parent(
                a,
                w,
                child.id,
                &child.etag(),
                req(json!({"parent_id":target.id,"parent_etag":target.etag()})),
            )
            .await;
        clear(h, table).await;
        assert!(result.is_err());
        assert_eq!(snapshot(h, a, w).await, before);
    }
    parent(h, a, w, child.id, Some(target.id)).await;
}
async fn restricted_worker(h: &Harness, a: &Actor, w: Uuid) {
    let t = team(h, a, w, false).await;
    let p = issue(h, a, w, t, json!({})).await;
    let child = issue(h, a, w, t, json!({"parent_id":p.id,"parent_etag":p.etag()})).await;
    let flow = h.app.workflow(a, w, t).await.unwrap();
    let target = flow
        .statuses
        .iter()
        .find(|s| s.id != p.status_id)
        .unwrap()
        .id;
    h.app
        .replace_status(
            a,
            w,
            t,
            p.status_id,
            "graph-worker-unchanged",
            &flow.etag(),
            req(json!({"replacement_status_id":target})),
        )
        .await
        .unwrap();
    let worker = Worker::connect(&h.worker_url().await, WorkerOptions::default())
        .await
        .unwrap();
    let claim = worker.claim(w).await.unwrap().unwrap();
    assert_eq!(
        worker.run_claim(&claim).await.unwrap(),
        WorkerTick::Completed
    );
    assert_eq!(fresh(h, a, w, child.id).await.status_id, target);
    assert_eq!(
        h.app
            .issue_parent(a, w, child.id)
            .await
            .unwrap()
            .parent
            .unwrap()
            .target
            .unwrap()
            .id,
        p.id
    );
    worker.pool.close().await;
}
async fn capacity(h: &Harness, a: &Actor, w: Uuid) {
    let t = team(h, a, w, false).await;
    let sentinel = issue(h, a, w, t, json!({})).await;
    let mut tx = h.context(a.user.id, w).await;
    sqlx::query("SET LOCAL statement_timeout='30s'")
        .execute(&mut *tx)
        .await
        .unwrap();
    let ids:Vec<Uuid>=sqlx::query_scalar("INSERT INTO issues(id,workspace_id,team_id,created_by_user_id,number,title,status_id,rank) SELECT gen_random_uuid(),$1,$2,$3,100+n,'Capacity '||n,$4,(100+n)::numeric*1024 FROM generate_series(0,10000)n ORDER BY n RETURNING id").bind(w).bind(t).bind(a.user.id).bind(sentinel.status_id).fetch_all(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO issue_relations(id,workspace_id,team_id,source_id,target_id,kind,created_by) SELECT gen_random_uuid(),$1,$2,ids[n],ids[n+1],'blocks',$3 FROM (SELECT $4::uuid[] AS ids)x CROSS JOIN generate_series(1,10000)n ORDER BY n").bind(w).bind(t).bind(a.user.id).bind(&ids).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    let target = fresh(h, a, w, ids[0]).await;
    let result = h
        .app
        .create_issue_relation(
            a,
            w,
            sentinel.id,
            "capacity-bound-refusal",
            &sentinel.etag(),
            req(json!({"kind":"blocks","target_id":target.id,"target_etag":target.etag()})),
        )
        .await;
    assert!(matches!(result, Err(Error::Conflict(_))), "{result:?}");
    let mut tx = h.context(a.user.id, w).await;
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM issue_relations WHERE team_id=$1")
            .bind(t)
            .fetch_one(&mut *tx)
            .await
            .unwrap(),
        10000
    );
    tx.commit().await.unwrap();
}
