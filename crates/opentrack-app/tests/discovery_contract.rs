//! Saved definitions use current authority, stable favorites and repairable state.
mod graph_support;
#[allow(dead_code)]
mod support;
use graph_support::*;
use opentrack_domain::*;
use serde_json::json;
use support::Harness;

fn definition(name: &str) -> SaveDiscovery {
    req(
        json!({"name":name,"kind":"search","visibility":"personal","team_id":null,"definition":{"query":{"version":1,"filter":null,"q":"café","timezone":"UTC"},"layout":"list","group_by":"status","columns":["key","title","status"]}}),
    )
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn saved_discovery_authority_versions_and_repair() {
    let h = Harness::new().await;
    let copy = h.clone();
    let result = tokio::spawn(async move { exercise(&copy).await }).await;
    h.cleanup().await;
    if let Err(error) = result {
        std::panic::resume_unwind(error.into_panic());
    }
}
async fn exercise(h: &Harness) {
    let (a, w, _) = setup(h).await;
    let b = person(h, &a, w, "member").await;
    let created = h
        .app
        .create_discovery(&a, w, "save-search-1", definition("Café work"))
        .await
        .unwrap();
    let replay = h
        .app
        .create_discovery(&a, w, "save-search-1", definition("Café work"))
        .await
        .unwrap();
    assert_eq!(created.id, replay.id);
    assert_eq!(
        created.definition.as_ref().unwrap().query.q.as_deref(),
        Some("café")
    );
    assert!(
        h.app
            .discoveries(&b, w, DiscoveryListQuery::default())
            .await
            .unwrap()
            .data
            .is_empty()
    );
    assert!(matches!(
        h.app.discovery(&b, w, created.id).await,
        Err(Error::NotFound)
    ));
    assert!(matches!(
        h.app.execute_discovery(&b, w, created.id, None).await,
        Err(Error::NotFound)
    ));
    assert!(matches!(
        h.app
            .update_discovery(
                &b,
                w,
                created.id,
                &created.etag,
                "rename-other-1",
                definition("Other")
            )
            .await,
        Err(Error::NotFound)
    ));
    assert!(matches!(
        h.app
            .delete_discovery(&b, w, created.id, &created.etag, "delete-other-1")
            .await,
        Err(Error::NotFound)
    ));
    h.app
        .favorite_discovery(&a, w, created.id, "favorite-search-1", true)
        .await
        .unwrap();
    let renamed = h
        .app
        .update_discovery(
            &a,
            w,
            created.id,
            &created.etag,
            "rename-search-1",
            definition("Renamed"),
        )
        .await
        .unwrap();
    assert_eq!(renamed.id, created.id);
    assert!(renamed.favorite);
    assert!(matches!(
        h.app
            .update_discovery(
                &a,
                w,
                created.id,
                &created.etag,
                "rename-search-2",
                definition("Stale")
            )
            .await,
        Err(Error::VersionConflict(_))
    ));
    assert!(matches!(
        h.app
            .create_discovery(&a, w, "save-search-1", definition("Changed"))
            .await,
        Err(Error::Conflict(_))
    ));
    let mut tx = h.context(a.user.id, w).await;
    sqlx::query("UPDATE saved_discoveries SET definition=jsonb_set(definition,'{query,version}','99') WHERE workspace_id=$1 AND id=$2").bind(w).bind(created.id).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    let invalid = h.app.discovery(&a, w, created.id).await.unwrap();
    assert!(invalid.definition.is_none());
    assert!(invalid.repair_required.is_some());
    assert!(matches!(
        h.app.execute_discovery(&a, w, created.id, None).await,
        Err(Error::Invalid(_))
    ));
    let repaired = h
        .app
        .update_discovery(
            &a,
            w,
            created.id,
            &invalid.etag,
            "repair-search-1",
            definition("Repaired"),
        )
        .await
        .unwrap();
    assert!(repaired.repair_required.is_none());
    assert!(repaired.favorite);
    h.app
        .delete_discovery(&a, w, created.id, &repaired.etag, "delete-search-1")
        .await
        .unwrap();
    h.app
        .delete_discovery(&a, w, created.id, &repaired.etag, "delete-search-1")
        .await
        .unwrap();
    assert!(
        h.app
            .discoveries(&a, w, DiscoveryListQuery::default())
            .await
            .unwrap()
            .data
            .is_empty()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn search_relevance_live_projection_and_acl() {
    let h = Harness::new().await;
    let copy = h.clone();
    let result = tokio::spawn(async move {
        let h = &copy;
        let (a, w, t) = setup(h).await;
        let b = person(h, &a, w, "member").await;
        let title = issue(h, &a, w, t, json!({"title":"Nebula migration"})).await;
        let body = issue(
            h,
            &a,
            w,
            t,
            json!({"title":"Database work","description":"nebula indexing"}),
        )
        .await;
        let private = team(h, &a, w, true).await;
        issue(
            h,
            &a,
            w,
            private,
            json!({"title":"Nebula hidden","description":"secret nebula"}),
        )
        .await;
        let query = |q: &str| {
            req::<SearchRequest>(
                json!({"query":{"version":1,"filter":null,"q":q},"types":["issue"]}),
            )
        };
        let page = h.app.search(&b, w, query("nebula")).await.unwrap();
        assert_eq!(page.data.len(), 2);
        assert_eq!(page.data[0].id, title.id);
        assert!(page.fuzzy_degraded);
        let exact = h.app.search(&b, w, query(&body.key)).await.unwrap();
        assert_eq!(exact.data[0].id, body.id);
        let changed = h
            .app
            .update_issue(
                &a,
                w,
                body.id,
                &body.etag(),
                req(json!({"description":"quasar committed"})),
            )
            .await
            .unwrap();
        let updated = h.app.search(&b, w, query("quasar")).await.unwrap();
        assert_eq!(updated.data[0].id, changed.id);
        assert!(h.app.search(&b, w, query(&"x".repeat(257))).await.is_err());
        assert!(
            h.app
                .search(
                    &b,
                    w,
                    query(&(0..33).map(|_| "term").collect::<Vec<_>>().join(" "))
                )
                .await
                .is_err()
        );
    })
    .await;
    h.cleanup().await;
    if let Err(error) = result {
        std::panic::resume_unwind(error.into_panic());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shared_definition_rechecks_targets_and_redacts_unavailable_references() {
    let h = Harness::new().await;
    let copy = h.clone();
    let result = tokio::spawn(async move {
        let h = &copy;
        let (a, w, t) = setup(h).await;
        let b = person(h, &a, w, "member").await;
        let hidden = team(h, &a, w, true).await;
        let mut request = definition("Shared work");
        request.kind = DiscoveryKind::View;
        request.visibility = DiscoveryVisibility::Workspace;
        request.definition.query.q = None;
        let view = h
            .app
            .create_discovery(&a, w, "shared-view-1", request.clone())
            .await
            .unwrap();
        assert!(
            h.app
                .discovery(&b, w, view.id)
                .await
                .unwrap()
                .definition
                .is_some()
        );
        request.definition.query.filter = Some(req(
            json!({"kind":"predicate","field":"team","op":"not_in","values":[hidden]}),
        ));
        h.app
            .update_discovery(&a, w, view.id, &view.etag, "shared-view-2", request)
            .await
            .unwrap();
        let recipient = h.app.discovery(&b, w, view.id).await.unwrap();
        assert!(recipient.definition.is_none());
        assert!(recipient.repair_required.is_some());
        let page = h
            .app
            .discoveries(&b, w, DiscoveryListQuery::default())
            .await
            .unwrap();
        assert!(page.data[0].definition.is_none());
        assert!(
            !serde_json::to_string(&page)
                .unwrap()
                .contains(&hidden.to_string())
        );
        assert!(matches!(
            h.app.execute_discovery(&b, w, view.id, None).await,
            Err(Error::Invalid(_))
        ));
        assert!(matches!(
            h.app
                .update_discovery(&b, w, view.id, &recipient.etag, "shared-view-denied", {
                    let mut d = definition("Forbidden");
                    d.kind = DiscoveryKind::View;
                    d.visibility = DiscoveryVisibility::Workspace;
                    d
                })
                .await,
            Err(Error::Forbidden)
        ));
        let mut personal = definition("Personal view");
        personal.kind = DiscoveryKind::View;
        let own = h
            .app
            .create_discovery(&b, w, "member-personal-1", personal)
            .await
            .unwrap();
        let mut tx = h.context(a.user.id, w).await;
        sqlx::query(
            "UPDATE workspace_members SET active=false WHERE workspace_id=$1 AND user_id=$2",
        )
        .bind(w)
        .bind(b.user.id)
        .execute(&mut *tx)
        .await
        .unwrap();
        tx.commit().await.unwrap();
        assert!(matches!(
            h.app.discovery(&b, w, own.id).await,
            Err(Error::NotFound)
        ));
        let _ = t;
    })
    .await;
    h.cleanup().await;
    if let Err(error) = result {
        std::panic::resume_unwind(error.into_panic());
    }
}
