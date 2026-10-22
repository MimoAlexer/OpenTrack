//! Real blocking establishes command order; no sleep is used to assume a winner.
mod graph_support;
#[allow(dead_code)]
mod support;
use graph_support::*;
use opentrack_domain::*;
use opentrack_store as store;
use serde_json::json;
use support::Harness;
use uuid::Uuid;
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_postgres_graph_move_races() {
    let h = Harness::with_connections(8).await;
    let copy = h.clone();
    let run = tokio::spawn(async move { exercise(&copy).await }).await;
    h.cleanup().await;
    if let Err(e) = run {
        std::panic::resume_unwind(e.into_panic());
    }
}
async fn exercise(h: &Harness) {
    let (a, w, t) = setup(h).await;
    for parenting in [false, true] {
        opposite(h, &a, w, t, parenting).await;
    }
    direct_block_cycle(h, &a, w, t).await;
    related_and_duplicate_races(h, &a, w, t).await;
    opposite_moves(h, &a, w).await;
    for revoke_first in [false, true] {
        move_access_race(h, &a, w, revoke_first).await;
    }
}
async fn opposite(h: &Harness, a: &Actor, w: Uuid, t: Uuid, parenting: bool) {
    let x = issue(h, a, w, t, json!({})).await;
    let y = issue(h, a, w, t, json!({})).await;
    let (holder, pid) = gate(h, a, w, t, Some(x.id)).await;
    let app = h.app.clone();
    let actor = a.clone();
    let s = x.clone();
    let target = y.clone();
    let first = tokio::spawn(async move {
        if parenting {
            app.set_issue_parent(
                &actor,
                w,
                s.id,
                &s.etag(),
                req(json!({"parent_id":target.id,"parent_etag":target.etag()})),
            )
            .await
            .map(|_| ())
        } else {
            app.create_issue_relation(
                &actor,
                w,
                s.id,
                "opposite-first-block",
                &s.etag(),
                req(json!({"kind":"blocks","target_id":target.id,"target_etag":target.etag()})),
            )
            .await
            .map(|_| ())
        }
    });
    let first_pid = blocked(h, pid).await;
    let app = h.app.clone();
    let actor = a.clone();
    let s = y.clone();
    let target = x.clone();
    let second = tokio::spawn(async move {
        if parenting {
            app.set_issue_parent(
                &actor,
                w,
                s.id,
                &s.etag(),
                req(json!({"parent_id":target.id,"parent_etag":target.etag()})),
            )
            .await
            .map(|_| ())
        } else {
            app.create_issue_relation(
                &actor,
                w,
                s.id,
                "opposite-second-block",
                &s.etag(),
                req(json!({"kind":"blocks","target_id":target.id,"target_etag":target.etag()})),
            )
            .await
            .map(|_| ())
        }
    });
    blocked(h, first_pid).await;
    holder.commit().await.unwrap();
    first.await.unwrap().unwrap();
    assert!(matches!(
        second.await.unwrap(),
        Err(Error::Stale(_) | Error::VersionConflict(_) | Error::Conflict(_))
    ));
    if parenting {
        assert_eq!(
            h.app
                .issue_parent(a, w, x.id)
                .await
                .unwrap()
                .parent
                .unwrap()
                .target
                .unwrap()
                .id,
            y.id
        );
        assert!(
            h.app
                .issue_parent(a, w, y.id)
                .await
                .unwrap()
                .parent
                .is_none()
        );
    } else {
        assert_eq!(
            h.app
                .issue_relations(a, w, x.id, req(json!({})))
                .await
                .unwrap()
                .data
                .len(),
            1
        );
    }
}
async fn direct_block_cycle(h: &Harness, a: &Actor, w: Uuid, t: Uuid) {
    let x = issue(h, a, w, t, json!({})).await;
    let y = issue(h, a, w, t, json!({})).await;
    let (holder, pid) = gate(h, a, w, t, None).await;
    let pool = h.app.pool.clone();
    let user = a.user.id;
    let run = move |source: Uuid, target: Uuid| {
        let pool = pool.clone();
        async move {
            let mut tx = store::begin(&pool).await.unwrap();
            store::context(&mut tx, user, Some(w)).await.unwrap();
            let result=sqlx::query("INSERT INTO issue_relations(id,workspace_id,team_id,source_id,target_id,kind,created_by) VALUES(gen_random_uuid(),$1,$2,$3,$4,'blocks',$5)").bind(w).bind(t).bind(source).bind(target).bind(user).execute(&mut *tx).await;
            match result {
                Ok(_) => tx.commit().await,
                Err(e) => Err(e),
            }
        }
    };
    let first = tokio::spawn(run(x.id, y.id));
    let first_pid = blocked(h, pid).await;
    let second = tokio::spawn(run(y.id, x.id));
    blocked(h, first_pid).await;
    holder.commit().await.unwrap();
    first.await.unwrap().unwrap();
    assert!(second.await.unwrap().is_err());
    let mut tx = h.context(a.user.id, w).await;
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM issue_relations WHERE source_id=ANY($1)"
        )
        .bind([x.id, y.id].as_slice())
        .fetch_one(&mut *tx)
        .await
        .unwrap(),
        1
    );
    tx.commit().await.unwrap();
}
async fn opposite_moves(h: &Harness, a: &Actor, w: Uuid) {
    let left = team(h, a, w, false).await;
    let right = team(h, a, w, false).await;
    let x = issue(h, a, w, left, json!({})).await;
    let y = issue(h, a, w, right, json!({})).await;
    let xp = h.app.preview_issue_move(a, w, x.id, right).await.unwrap();
    let yp = h.app.preview_issue_move(a, w, y.id, left).await.unwrap();
    let (holder, pid) = gate(h, a, w, left, Some(x.id)).await;
    let app = h.app.clone();
    let actor = a.clone();
    let first = tokio::spawn(async move {
        app.move_issue(
            &actor,
            w,
            x.id,
            "opposite-move-first",
            &xp.issue_etag,
            move_body(&xp),
        )
        .await
    });
    let first_pid = blocked(h, pid).await;
    let app = h.app.clone();
    let actor = a.clone();
    let second = tokio::spawn(async move {
        app.move_issue(
            &actor,
            w,
            y.id,
            "opposite-move-second",
            &yp.issue_etag,
            move_body(&yp),
        )
        .await
    });
    blocked(h, first_pid).await;
    holder.commit().await.unwrap();
    assert_eq!(first.await.unwrap().unwrap().team_id, right);
    match second.await.unwrap() {
        Ok(result) => assert_eq!(result.team_id, left),
        Err(Error::Conflict(_) | Error::Stale(_)) => {
            assert_eq!(move_to(h, a, w, y.id, left).await.team_id, left);
        }
        other => panic!("Opposite moves must not deadlock: {other:?}"),
    }
}
async fn move_access_race(h: &Harness, a: &Actor, w: Uuid, revoke_first: bool) {
    let source = team(h, a, w, false).await;
    let destination = team(h, a, w, true).await;
    let mover = person(h, a, w, "member").await;
    grant(h, a, w, destination, mover.user.id).await;
    let item = issue(h, &mover, w, source, json!({})).await;
    let preview = h
        .app
        .preview_issue_move(&mover, w, item.id, destination)
        .await
        .unwrap();
    let membership = h
        .app
        .get_team_member(a, w, destination, mover.user.id)
        .await
        .unwrap();
    let (holder, pid) = if revoke_first {
        let mut tx = h.context(a.user.id, w).await;
        sqlx::query(
            "SELECT workspace_id FROM workspace_auth_guards WHERE workspace_id=$1 FOR UPDATE",
        )
        .bind(w)
        .execute(&mut *tx)
        .await
        .unwrap();
        let pid = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        (tx, pid)
    } else {
        gate(h, a, w, source, Some(item.id)).await
    };
    let app = h.app.clone();
    let actor = mover.clone();
    let movement = async move {
        app.move_issue(
            &actor,
            w,
            item.id,
            &Uuid::new_v4().to_string(),
            &preview.issue_etag,
            move_body(&preview),
        )
        .await
    };
    let app = h.app.clone();
    let actor = a.clone();
    let removal = async move {
        app.remove_team_member(
            &actor,
            w,
            destination,
            mover.user.id,
            &membership.etag(destination),
        )
        .await
    };
    let result = if revoke_first {
        let first = tokio::spawn(removal);
        let first_pid = blocked(h, pid).await;
        let second = tokio::spawn(movement);
        blocked(h, first_pid).await;
        holder.commit().await.unwrap();
        first.await.unwrap().unwrap();
        second.await.unwrap()
    } else {
        let first = tokio::spawn(movement);
        let first_pid = blocked(h, pid).await;
        let second = tokio::spawn(removal);
        blocked(h, first_pid).await;
        holder.commit().await.unwrap();
        let result = first.await.unwrap();
        second.await.unwrap().unwrap();
        result
    };
    let stored = fresh(h, a, w, item.id).await;
    if revoke_first {
        assert!(matches!(result, Err(Error::NotFound | Error::Forbidden)));
        assert_eq!(stored.team_id, source);
    } else {
        assert_eq!(result.unwrap().team_id, destination);
        assert_eq!(stored.team_id, destination);
        assert!(matches!(
            h.app.resolve_issue_key(&mover, w, &item.key).await,
            Err(Error::NotFound)
        ));
    }
}

async fn related_and_duplicate_races(h: &Harness, a: &Actor, w: Uuid, t: Uuid) {
    // Related is submitted from opposite endpoints; duplicate-root edits are
    // submitted in each observed order. Both callers begin with current ETags.
    for (kind, reverse) in [
        ("related", false),
        ("duplicate_of", false),
        ("duplicate_of", true),
    ] {
        let x = issue(h, a, w, t, json!({})).await;
        let y = issue(h, a, w, t, json!({})).await;
        let z = issue(h, a, w, t, json!({})).await;
        let ((first_source, first_target), (second_source, second_target)) = if kind == "related" {
            ((x.clone(), y.clone()), (y.clone(), x.clone()))
        } else if reverse {
            ((y.clone(), z.clone()), (x.clone(), y.clone()))
        } else {
            ((x.clone(), y.clone()), (y.clone(), z.clone()))
        };
        let (holder, pid) = gate(h, a, w, t, Some(first_source.id)).await;
        let app = h.app.clone();
        let actor = a.clone();
        let first_pair = (first_source.clone(), first_target.clone());
        let first = tokio::spawn(async move {
            app.create_issue_relation(&actor,w,first_pair.0.id,&Uuid::new_v4().to_string(),&first_pair.0.etag(),req(json!({"kind":kind,"target_id":first_pair.1.id,"target_etag":first_pair.1.etag()}))).await
        });
        let first_pid = blocked(h, pid).await;
        let app = h.app.clone();
        let actor = a.clone();
        let second_pair = (second_source.clone(), second_target.clone());
        let second = tokio::spawn(async move {
            app.create_issue_relation(&actor,w,second_pair.0.id,&Uuid::new_v4().to_string(),&second_pair.0.etag(),req(json!({"kind":kind,"target_id":second_pair.1.id,"target_etag":second_pair.1.etag()}))).await
        });
        blocked(h, first_pid).await;
        holder.commit().await.unwrap();
        first.await.unwrap().unwrap();
        let loser = second.await.unwrap();
        assert!(
            matches!(
                loser,
                Err(Error::Stale(_) | Error::VersionConflict(_) | Error::Conflict(_))
            ),
            "Concurrent {kind} loser: {loser:?}"
        );
        let second_source = fresh(h, a, w, second_source.id).await;
        let second_target = fresh(h, a, w, second_target.id).await;
        let before = snapshot(h, a, w).await;
        let repaired=h.app.create_issue_relation(a,w,second_source.id,&Uuid::new_v4().to_string(),&second_source.etag(),req(json!({"kind":kind,"target_id":second_target.id,"target_etag":second_target.etag()}))).await;
        assert!(
            matches!(repaired, Err(Error::Conflict(_))),
            "Current ETags cannot bypass {kind} invariant: {repaired:?}"
        );
        assert_eq!(snapshot(h, a, w).await, before);
        let mut tx = h.context(a.user.id, w).await;
        let rows:Vec<(Uuid,Uuid,String)>=sqlx::query_as("SELECT source_id,target_id,kind FROM issue_relations WHERE workspace_id=$1 AND source_id=ANY($2) ORDER BY id")
            .bind(w).bind([x.id,y.id,z.id]).fetch_all(&mut *tx).await.unwrap();
        tx.commit().await.unwrap();
        assert_eq!(rows.len(), 1);
        if kind == "related" {
            assert_eq!(rows[0], (x.id.min(y.id), x.id.max(y.id), "related".into()));
        } else {
            assert_eq!(
                rows[0],
                (first_source.id, first_target.id, "duplicate".into())
            );
        }
    }
}
