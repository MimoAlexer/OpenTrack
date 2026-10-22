//! Single-issue move repair, historical aliases and receipt/activity privacy.
mod graph_support;
#[allow(dead_code)]
mod support;
use graph_support::*;
use opentrack_domain::*;
use serde_json::json;
use support::Harness;
use uuid::Uuid;
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_postgres_move_contract() {
    let h = Harness::with_connections(6).await;
    let copy = h.clone();
    let run = tokio::spawn(async move { exercise(&copy).await }).await;
    h.cleanup().await;
    if let Err(e) = run {
        std::panic::resume_unwind(e.into_panic());
    }
}
async fn exercise(h: &Harness) {
    let (a, w, t) = setup(h).await;
    repairs_and_aliases(h, &a, w, t).await;
    blockers(h, &a, w, t).await;
    receipt_privacy(h, &a, w, t).await;
    historical_privacy(h, &a, w, t).await;
    atomicity(h, &a, w, t).await;
}
async fn repairs_and_aliases(h: &Harness, a: &Actor, w: Uuid, t: Uuid) {
    let member = person(h, a, w, "member").await;
    let private = team(h, a, w, true).await;
    let shared = h
        .app
        .create_label(
            a,
            w,
            "shared-move-label",
            req(json!({"name":"Shared move"})),
        )
        .await
        .unwrap();
    let local = h
        .app
        .create_label(
            a,
            w,
            "local-move-label",
            req(json!({"team_id":t,"name":"Local move"})),
        )
        .await
        .unwrap();
    let item = issue(
        h,
        a,
        w,
        t,
        json!({"assignee_id":member.user.id,"label_ids":[local.id,shared.id]}),
    )
    .await;
    let preview = h
        .app
        .preview_issue_move(a, w, item.id, private)
        .await
        .unwrap();
    assert!(preview.blockers.is_empty());
    assert_eq!(preview.retained_label_ids, [shared.id]);
    assert_eq!(preview.removed_label_ids, [local.id]);
    assert!(!preview.current_assignee.as_ref().unwrap().assignable);
    assert_eq!(preview.audience_change, MoveAudienceChange::PublicToPrivate);
    assert!(preview.destination.can_move_issue);
    let encoded = serde_json::to_value(&preview).unwrap();
    assert!(encoded.get("new_key").is_none());
    assert!(encoded.get("number").is_none());
    let before = snapshot(h, a, w).await;
    let mut bad = move_body(&preview);
    bad.label_ids = vec![local.id, shared.id];
    assert!(matches!(
        h.app
            .move_issue(a, w, item.id, "bad-label-move", &preview.issue_etag, bad)
            .await,
        Err(Error::Invalid(_))
    ));
    let mut bad = move_body(&preview);
    bad.assignee_id = Some(member.user.id);
    assert!(matches!(
        h.app
            .move_issue(a, w, item.id, "bad-assignee-move", &preview.issue_etag, bad)
            .await,
        Err(Error::Invalid(_))
    ));
    let mut bad = move_body(&preview);
    bad.preview_token.push('x');
    assert!(matches!(
        h.app
            .move_issue(a, w, item.id, "bad-preview-move", &preview.issue_etag, bad)
            .await,
        Err(Error::Conflict(_))
    ));
    assert_eq!(snapshot(h, a, w).await, before);
    let destinations = h
        .app
        .issue_move_destinations(&member, w, item.id, req(json!({})))
        .await
        .unwrap();
    assert!(
        !destinations
            .data
            .iter()
            .any(|v| v.id == private || v.id == t)
    );
    assert!(matches!(
        h.app.preview_issue_move(&member, w, item.id, private).await,
        Err(Error::NotFound)
    ));
    let moved = h
        .app
        .move_issue(
            a,
            w,
            item.id,
            "successful-move-retry",
            &preview.issue_etag,
            move_body(&preview),
        )
        .await
        .unwrap();
    assert_eq!(moved.id, item.id);
    assert_eq!(moved.team_id, private);
    assert_ne!(moved.key, item.key);
    advanced(&item, &moved);
    assert_eq!(moved.label_ids, [shared.id]);
    assert!(moved.assignee.is_none());
    let after = snapshot(h, a, w).await;
    let replay = h
        .app
        .move_issue(
            a,
            w,
            item.id,
            "successful-move-retry",
            &preview.issue_etag,
            move_body(&preview),
        )
        .await
        .unwrap();
    assert_eq!(replay.etag(), moved.etag());
    assert_eq!(snapshot(h, a, w).await, after);
    assert_eq!(
        h.app.resolve_issue_key(a, w, &item.key).await.unwrap().id,
        item.id
    );
    assert_eq!(
        h.app.resolve_issue_key(a, w, &moved.key).await.unwrap().id,
        item.id
    );
    assert!(matches!(
        h.app.resolve_issue_key(&member, w, &item.key).await,
        Err(Error::NotFound)
    ));
    let lookup = h
        .app
        .lookup_issues(a, w, req(json!({"team_id":private,"q":item.key})))
        .await
        .unwrap();
    assert_eq!(lookup.data.len(), 1);
    assert_eq!(lookup.data[0].id, item.id);
    let again = move_to(h, a, w, item.id, t).await;
    assert_ne!(again.key, item.key);
    assert_eq!(
        h.app.resolve_issue_key(a, w, &item.key).await.unwrap().key,
        again.key
    );
    assert_eq!(
        h.app.resolve_issue_key(a, w, &moved.key).await.unwrap().key,
        again.key
    );
    let mut tx = h.context(a.user.id, w).await;
    assert!(
        sqlx::query("UPDATE issue_key_aliases SET issue_id=gen_random_uuid() WHERE issue_id=$1")
            .bind(item.id)
            .execute(&mut *tx)
            .await
            .is_err()
    );
    tx.rollback().await.unwrap();
    // An old preview cannot commit after a destination workflow change.
    let stale = h
        .app
        .preview_issue_move(a, w, item.id, private)
        .await
        .unwrap();
    let flow = h.app.workflow(a, w, private).await.unwrap();
    h.app
        .update_status(
            a,
            w,
            private,
            flow.statuses[0].id,
            &flow.etag(),
            req(json!({"name":"Changed after preview"})),
        )
        .await
        .unwrap();
    assert!(matches!(
        h.app
            .move_issue(
                a,
                w,
                item.id,
                "stale-workflow-preview",
                &stale.issue_etag,
                move_body(&stale)
            )
            .await,
        Err(Error::Conflict(_))
    ));
}
async fn blockers(h: &Harness, a: &Actor, w: Uuid, t: Uuid) {
    let destination = team(h, a, w, false).await;
    for (kind, expected, inverse) in [
        ("blocks", MoveBlocker::Blocks, false),
        ("blocks", MoveBlocker::BlockedBy, true),
        ("related", MoveBlocker::Related, false),
        ("duplicate_of", MoveBlocker::DuplicateOf, false),
        ("duplicate_of", MoveBlocker::HasDuplicate, true),
    ] {
        let source = issue(h, a, w, t, json!({})).await;
        let target = issue(h, a, w, t, json!({})).await;
        if inverse {
            relate(h, a, w, target.id, source.id, kind).await;
        } else {
            relate(h, a, w, source.id, target.id, kind).await;
        }
        state(h, a, w, target.id, true).await;
        let preview = h
            .app
            .preview_issue_move(a, w, source.id, destination)
            .await
            .unwrap();
        assert!(preview.blockers.contains(&expected));
        let before = snapshot(h, a, w).await;
        assert!(matches!(
            h.app
                .move_issue(
                    a,
                    w,
                    source.id,
                    &Uuid::new_v4().to_string(),
                    &preview.issue_etag,
                    move_body(&preview)
                )
                .await,
            Err(Error::Conflict(_))
        ));
        assert_eq!(snapshot(h, a, w).await, before);
    }
    let p = issue(h, a, w, t, json!({})).await;
    let child = issue(h, a, w, t, json!({"parent_id":p.id,"parent_etag":p.etag()})).await;
    assert!(
        h.app
            .preview_issue_move(a, w, p.id, destination)
            .await
            .unwrap()
            .blockers
            .contains(&MoveBlocker::Children)
    );
    assert!(
        h.app
            .preview_issue_move(a, w, child.id, destination)
            .await
            .unwrap()
            .blockers
            .contains(&MoveBlocker::Parent)
    );
    parent(h, a, w, child.id, None).await;
    assert!(
        h.app
            .preview_issue_move(a, w, p.id, destination)
            .await
            .unwrap()
            .blockers
            .is_empty()
    );
    assert!(
        h.app
            .preview_issue_move(a, w, child.id, destination)
            .await
            .unwrap()
            .blockers
            .is_empty()
    );
}
async fn receipt_privacy(h: &Harness, a: &Actor, w: Uuid, t: Uuid) {
    let member = person(h, a, w, "member").await;
    let private = team(h, a, w, true).await;
    let public = team(h, a, w, false).await;
    let body = json!({"team_id":t,"title":"Original create receipt"});
    let created = h
        .app
        .create_issue(&member, w, "private-create-receipt", req(body.clone()))
        .await
        .unwrap();
    move_to(h, a, w, created.id, private).await;
    assert!(matches!(
        h.app
            .create_issue(&member, w, "private-create-receipt", req(body))
            .await,
        Err(Error::NotFound)
    ));
    let source = issue(h, &member, w, t, json!({})).await;
    let target = issue(h, &member, w, t, json!({})).await;
    let body = json!({"kind":"related","target_id":target.id,"target_etag":target.etag()});
    let related = h
        .app
        .create_issue_relation(
            &member,
            w,
            source.id,
            "private-relation-receipt",
            &source.etag(),
            req(body.clone()),
        )
        .await
        .unwrap();
    h.app
        .remove_issue_relation(a, w, source.id, related.relation.id, &related.issue.etag())
        .await
        .unwrap();
    move_to(h, a, w, target.id, private).await;
    assert!(matches!(
        h.app
            .create_issue_relation(
                &member,
                w,
                source.id,
                "private-relation-receipt",
                &source.etag(),
                req(body)
            )
            .await,
        Err(Error::NotFound)
    ));
    let source = issue(h, &member, w, t, json!({})).await;
    let preview = h
        .app
        .preview_issue_move(&member, w, source.id, public)
        .await
        .unwrap();
    h.app
        .move_issue(
            &member,
            w,
            source.id,
            "private-move-receipt",
            &preview.issue_etag,
            move_body(&preview),
        )
        .await
        .unwrap();
    move_to(h, a, w, source.id, private).await;
    assert!(matches!(
        h.app
            .move_issue(
                &member,
                w,
                source.id,
                "private-move-receipt",
                &preview.issue_etag,
                move_body(&preview)
            )
            .await,
        Err(Error::NotFound)
    ));
}
async fn historical_privacy(h: &Harness, a: &Actor, w: Uuid, t: Uuid) {
    let source = team(h, a, w, true).await;
    let mover = person(h, a, w, "member").await;
    let destination_reader = person(h, a, w, "member").await;
    grant(h, a, w, source, mover.user.id).await;
    let body = json!({"team_id":source,"title":"Private historical text canary"});
    let original = h
        .app
        .create_issue(&mover, w, "recorded-team-create", req(body.clone()))
        .await
        .unwrap();
    let target = issue(h, a, w, source, json!({"title":"Private target canary"})).await;
    let relation = relate(h, &mover, w, original.id, target.id, "related").await;
    h.app
        .remove_issue_relation(
            &mover,
            w,
            original.id,
            relation.relation.id,
            &relation.issue.etag(),
        )
        .await
        .unwrap();
    let current = fresh(h, &mover, w, original.id).await;
    h.app
        .update_issue(
            &mover,
            w,
            original.id,
            &current.etag(),
            json!({"title":"Public current title"}),
        )
        .await
        .unwrap();
    let preview = h
        .app
        .preview_issue_move(&mover, w, original.id, t)
        .await
        .unwrap();
    let moved = h
        .app
        .move_issue(
            &mover,
            w,
            original.id,
            "recorded-team-move",
            &preview.issue_etag,
            move_body(&preview),
        )
        .await
        .unwrap();
    let full = serde_json::to_string(&h.app.issue_activity(a, w, moved.id).await.unwrap()).unwrap();
    assert!(!full.contains("Private historical text canary"));
    assert!(full.contains("issue.created"));
    let visible = serde_json::to_string(
        &h.app
            .issue_activity(&destination_reader, w, moved.id)
            .await
            .unwrap(),
    )
    .unwrap();
    assert!(!visible.contains("Private historical text canary"));
    assert!(!visible.contains("Private target canary"));
    assert!(!visible.contains(&target.id.to_string()));
    assert!(!visible.contains(&source.to_string()));
    assert_eq!(
        h.app
            .resolve_issue_key(&destination_reader, w, &original.key)
            .await
            .unwrap()
            .id,
        moved.id
    );
    let membership = h
        .app
        .get_team_member(a, w, source, mover.user.id)
        .await
        .unwrap();
    h.app
        .remove_team_member(a, w, source, mover.user.id, &membership.etag(source))
        .await
        .unwrap();
    assert_eq!(fresh(h, &mover, w, moved.id).await.id, moved.id);
    assert!(matches!(
        h.app
            .create_issue(&mover, w, "recorded-team-create", req(body))
            .await,
        Err(Error::NotFound)
    ));
    assert!(matches!(
        h.app
            .move_issue(
                &mover,
                w,
                moved.id,
                "recorded-team-move",
                &preview.issue_etag,
                move_body(&preview)
            )
            .await,
        Err(Error::NotFound)
    ));
}
async fn atomicity(h: &Harness, a: &Actor, w: Uuid, t: Uuid) {
    let destination = team(h, a, w, false).await;
    let local = h
        .app
        .create_label(
            a,
            w,
            "atomic-move-label",
            req(json!({"team_id":t,"name":"Atomic move local"})),
        )
        .await
        .unwrap();
    let item = issue(h, a, w, t, json!({"label_ids":[local.id]})).await;
    let preview = h
        .app
        .preview_issue_move(a, w, item.id, destination)
        .await
        .unwrap();
    for (table, event) in [
        ("issue_key_aliases", "INSERT"),
        ("issues", "UPDATE"),
        ("teams", "UPDATE"),
        ("issue_labels", "DELETE"),
        ("idempotency_records", "UPDATE"),
        ("activity_entries", "INSERT"),
        ("workspace_events", "INSERT"),
    ] {
        let before = snapshot(h, a, w).await;
        fault(h, table, event).await;
        let result = h
            .app
            .move_issue(
                a,
                w,
                item.id,
                "atomic-move-retry",
                &preview.issue_etag,
                move_body(&preview),
            )
            .await;
        clear(h, table).await;
        assert!(result.is_err(), "{table} {event} suppression committed");
        assert_eq!(snapshot(h, a, w).await, before);
    }
    let moved = h
        .app
        .move_issue(
            a,
            w,
            item.id,
            "atomic-move-retry",
            &preview.issue_etag,
            move_body(&preview),
        )
        .await
        .unwrap();
    assert_eq!(moved.team_id, destination);
    assert_eq!(
        h.app.resolve_issue_key(a, w, &item.key).await.unwrap().id,
        item.id
    );
}
