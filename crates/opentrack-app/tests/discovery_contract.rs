//! Saved definitions use current authority, stable favorites and repairable state.
mod graph_support;
#[allow(dead_code)]
mod support;
use graph_support::*;
use opentrack_domain::*;
use opentrack_store as store;
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
    let (a, w, t) = setup(h).await;
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
    let public = issue(h, &a, w, t, json!({"title":"Café public result"})).await;
    let permitted = h
        .app
        .execute_discovery(&a, w, created.id, None)
        .await
        .unwrap();
    assert_eq!(permitted.search.unwrap().data[0].id, public.id);
    let visible = h
        .app
        .search(
            &b,
            w,
            req(json!({"query":{"version":1,"filter":null,"q":"café"},"types":["issue"]})),
        )
        .await
        .unwrap();
    assert_eq!(visible.data[0].id, public.id);

    let mut raw = store::begin(&h.app.pool).await.unwrap();
    store::context(&mut raw, b.user.id, Some(w)).await.unwrap();
    let hidden: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM saved_discoveries WHERE workspace_id=$1 AND id=$2",
    )
    .bind(w)
    .bind(created.id)
    .fetch_one(&mut *raw)
    .await
    .unwrap();
    assert_eq!(hidden, 0);
    assert_eq!(sqlx::query("UPDATE saved_discoveries SET name='Unauthorized rename' WHERE workspace_id=$1 AND id=$2").bind(w).bind(created.id).execute(&mut *raw).await.unwrap().rows_affected(),0);
    raw.rollback().await.unwrap();

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
        let mut paged = query("nebula");
        paged.query.limit = 1;
        let first = h.app.search(&b, w, paged.clone()).await.unwrap();
        assert!(first.has_more);
        paged.query.cursor = first.next_cursor;
        let second = h.app.search(&b, w, paged.clone()).await.unwrap();
        assert_eq!(second.data.len(), 1);
        assert_ne!(first.data[0].id, second.data[0].id);
        assert!(matches!(
            h.app.search(&a, w, paged.clone()).await,
            Err(Error::Invalid(_))
        ));
        paged.query.q = Some("quasar".into());
        assert!(matches!(
            h.app.search(&b, w, paged).await,
            Err(Error::Invalid(_))
        ));

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
        let destination = team(h, &a, w, false).await;
        let preview = h
            .app
            .preview_issue_move(&a, w, changed.id, destination)
            .await
            .unwrap();
        let moved = h
            .app
            .move_issue(
                &a,
                w,
                changed.id,
                "search-historical-key",
                &preview.issue_etag,
                MoveIssue {
                    destination_team_id: destination,
                    status_id: preview.destination_default_status_id,
                    assignee_id: None,
                    label_ids: vec![],
                    preview_token: preview.preview_token,
                },
            )
            .await
            .unwrap();
        let historical = h.app.search(&b, w, query(&body.key)).await.unwrap();
        assert_eq!(historical.data[0].id, moved.id);
        assert_eq!(historical.data[0].key.as_deref(), Some(moved.key.as_str()));

        // Keyset continuations bind principal, query and current authority.
        let mut first = query("nebula");
        first.query.limit = 1;
        let first_page = h.app.search(&b, w, first.clone()).await.unwrap();
        // After the body edit only one public match remains.
        assert_eq!(first_page.data.len(), 1);
        h.app
            .delete_issue(&a, w, title.id, &title.etag())
            .await
            .unwrap();
        assert!(
            h.app
                .search(&b, w, query("nebula"))
                .await
                .unwrap()
                .data
                .is_empty()
        );

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
        let public = issue(h, &a, w, t, json!({"title":"Shared public issue"})).await;
        issue(
            h,
            &a,
            w,
            hidden,
            json!({"title":"Private source remains hidden"}),
        )
        .await;
        let run = h.app.execute_discovery(&b, w, view.id, None).await.unwrap();
        let rows = run.issues.unwrap().page.data;
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, public.id);

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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_saved_execution_does_not_hold_a_pool_connection_while_waiting_for_another() {
    let h = Harness::with_connections(2).await;
    let copy = h.clone();
    let result = tokio::spawn(async move {
        let (a, w, _) = setup(&copy).await;
        let saved = copy
            .app
            .create_discovery(&a, w, "execution-pool-1", definition("Run search"))
            .await
            .unwrap();
        let one = copy.app.execute_discovery(&a, w, saved.id, None);
        let two = copy.app.execute_discovery(&a, w, saved.id, None);
        let (one, two) = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            tokio::join!(one, two)
        })
        .await
        .expect("Execution must not starve the connection pool");
        one.unwrap();
        two.unwrap();
    })
    .await;
    h.cleanup().await;
    if let Err(error) = result {
        std::panic::resume_unwind(error.into_panic());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn project_search_uses_current_narrative_document() {
    let h = Harness::new().await;
    let copy = h.clone();
    let result=tokio::spawn(async move {
        let h=&copy;let (a,w,t)=setup(h).await;
        let p=h.app.create_project(&a,w,t,"search-project-1",req(json!({"name":"Project search corpus","narrative":DocumentWrite::from_plain_text("olduniqueprojectword")}))).await.unwrap();
        let query=|q:&str|req::<SearchRequest>(json!({"query":{"version":1,"filter":null,"q":q},"types":["project"]}));
        assert_eq!(h.app.search(&a,w,query("olduniqueprojectword")).await.unwrap().data[0].id,p.id);
        h.app.update_project(&a,w,t,p.id,&p.etag,req(json!({"narrative":DocumentWrite::from_plain_text("newuniqueprojectword")}))).await.unwrap();
        assert_eq!(h.app.search(&a,w,query("newuniqueprojectword")).await.unwrap().data[0].id,p.id);
        assert!(h.app.search(&a,w,query("olduniqueprojectword")).await.unwrap().data.is_empty());
    }).await;
    h.cleanup().await;
    if let Err(error) = result {
        std::panic::resume_unwind(error.into_panic());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn saved_definition_write_failure_rolls_back_receipt_and_quota_is_serialized() {
    let h = Harness::new().await;
    let copy = h.clone();
    let result=tokio::spawn(async move {
        let h=&copy;let(a,w,_)=setup(h).await;
        fault(h,"audit_entries","INSERT").await;
        assert!(h.app.create_discovery(&a,w,"rollback-search-1",definition("Rolled back")).await.is_err());
        clear(h,"audit_entries").await;
        assert!(h.app.discoveries(&a,w,DiscoveryListQuery::default()).await.unwrap().data.is_empty());
        let saved=h.app.create_discovery(&a,w,"rollback-search-1",definition("Retry after rollback")).await.unwrap();
        let mut tx=h.context(a.user.id,w).await;
        sqlx::query("INSERT INTO saved_discoveries(workspace_id,id,owner_id,kind,visibility,name,definition) SELECT workspace_id,gen_random_uuid(),owner_id,kind,visibility,'Quota seed '||n,definition FROM saved_discoveries CROSS JOIN generate_series(1,98) n WHERE workspace_id=$1 AND id=$2").bind(w).bind(saved.id).execute(&mut *tx).await.unwrap();tx.commit().await.unwrap();
        let one=h.app.create_discovery(&a,w,"quota-concurrent-1",definition("Last slot"));
        let two=h.app.create_discovery(&a,w,"quota-concurrent-2",definition("Beyond cap"));
        let(left,right)=tokio::join!(one,two);assert_eq!(usize::from(left.is_ok())+usize::from(right.is_ok()),1);
        assert!(matches!(left.as_ref().err().or(right.as_ref().err()),Some(Error::Conflict(_))));
        let page=h.app.discoveries(&a,w,DiscoveryListQuery{limit:Some(1),..Default::default()}).await.unwrap();assert!(page.has_more);
        let next=h.app.discoveries(&a,w,DiscoveryListQuery{limit:Some(1),cursor:page.next_cursor.clone(),..Default::default()}).await.unwrap();assert_ne!(page.data[0].id,next.data[0].id);
    }).await;
    h.cleanup().await;
    if let Err(error) = result {
        std::panic::resume_unwind(error.into_panic());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn discovery_upgrade_builds_live_search_from_existing_sources() {
    let h = Harness::with_version(5, 32).await;
    let copy = h.clone();
    let result=tokio::spawn(async move {
        let h=&copy;let(a,w,t)=setup(h).await;
        let existing=issue(h,&a,w,t,json!({"title":"Upgrade corpus","description":"migrationsearchtoken"})).await;
        assert!(!store::discovery_schema_ready(&h.app.pool).await.unwrap());
        store::MIGRATOR.run(&h.owner).await.unwrap();
        assert!(store::discovery_schema_ready(&h.app.pool).await.unwrap());
        let q=req(json!({"query":{"version":1,"filter":null,"q":"migrationsearchtoken"},"types":["issue"]}));
        assert_eq!(h.app.search(&a,w,q).await.unwrap().data[0].id,existing.id);
        let saved=h.app.create_discovery(&a,w,"upgraded-search",definition("Upgrade saved search")).await.unwrap();assert!(saved.definition.is_some());
    }).await;
    h.cleanup().await;
    if let Err(error) = result {
        std::panic::resume_unwind(error.into_panic());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn personal_mutation_events_are_private_durable_and_required() {
    let h = Harness::new().await;
    let copy = h.clone();
    let result=tokio::spawn(async move {
        let h=&copy;let(a,w,_)=setup(h).await;let b=person(h,&a,w,"member").await;
        let own=h.app.create_discovery(&a,w,"private-event-create",definition("Private event source")).await.unwrap();
        let mut tx=h.context(a.user.id,w).await;
        let count:i64=sqlx::query_scalar("SELECT count(*) FROM discovery_personal_events WHERE workspace_id=$1 AND user_id=$2 AND discovery_id=$3").bind(w).bind(a.user.id).bind(own.id).fetch_one(&mut *tx).await.unwrap();assert_eq!(count,1);tx.commit().await.unwrap();
        let mut tx=store::begin(&h.app.pool).await.unwrap();store::context(&mut tx,b.user.id,Some(w)).await.unwrap();
        let count:i64=sqlx::query_scalar("SELECT count(*) FROM discovery_personal_events WHERE workspace_id=$1 AND discovery_id=$2").bind(w).bind(own.id).fetch_one(&mut *tx).await.unwrap();assert_eq!(count,0);
        let count:i64=sqlx::query_scalar("SELECT count(*) FROM workspace_events WHERE workspace_id=$1 AND entity_id=$2").bind(w).bind(own.id).fetch_one(&mut *tx).await.unwrap();assert_eq!(count,0);tx.rollback().await.unwrap();
        fault(h,"discovery_personal_events","INSERT").await;
        assert!(h.app.update_discovery(&a,w,own.id,&own.etag,"private-event-update",definition("Must roll back")).await.is_err());
        clear(h,"discovery_personal_events").await;
        assert_eq!(h.app.discovery(&a,w,own.id).await.unwrap().name,"Private event source");
    }).await;
    h.cleanup().await;
    if let Err(error) = result {
        std::panic::resume_unwind(error.into_panic());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shared_view_events_keep_current_authority_after_deletion_and_roll_back_atomically() {
    let h = Harness::new().await;
    let copy = h.clone();
    let result=tokio::spawn(async move {
        let h=&copy;let(a,w,_)=setup(h).await;let b=person(h,&a,w,"member").await;
        let private=team(h,&a,w,true).await;
        let mut request=definition("Private team view");request.kind=DiscoveryKind::View;request.visibility=DiscoveryVisibility::Team;request.team_id=Some(private);
        let saved=h.app.create_discovery(&a,w,"shared-event-create",request).await.unwrap();
        let mut tx=store::begin(&h.app.pool).await.unwrap();store::context(&mut tx,b.user.id,Some(w)).await.unwrap();
        assert!(!store::discovery_event_visible(&mut tx,w,b.user.id,"view.created",saved.id).await.unwrap());tx.rollback().await.unwrap();
        grant(h,&a,w,private,b.user.id).await;
        fault(h,"workspace_events","INSERT").await;
        assert!(h.app.delete_discovery(&a,w,saved.id,&saved.etag,"shared-event-delete").await.is_err());
        clear(h,"workspace_events").await;
        assert!(h.app.discovery(&a,w,saved.id).await.is_ok());
        h.app.delete_discovery(&a,w,saved.id,&saved.etag,"shared-event-delete").await.unwrap();
        assert!(matches!(h.app.discovery(&a,w,saved.id).await,Err(Error::NotFound)));
        let mut tx=store::begin(&h.app.pool).await.unwrap();store::context(&mut tx,b.user.id,Some(w)).await.unwrap();
        assert!(store::discovery_event_visible(&mut tx,w,b.user.id,"view.deleted",saved.id).await.unwrap());
        let event:(i64,bool)=sqlx::query_as("SELECT version,deleted_at IS NOT NULL FROM discovery_view_sources WHERE workspace_id=$1 AND id=$2").bind(w).bind(saved.id).fetch_one(&mut *tx).await.unwrap();assert_eq!(event,(2,true));tx.rollback().await.unwrap();
        let mut tx=h.context(a.user.id,w).await;sqlx::query("DELETE FROM team_members WHERE workspace_id=$1 AND team_id=$2 AND user_id=$3").bind(w).bind(private).bind(b.user.id).execute(&mut *tx).await.unwrap();tx.commit().await.unwrap();
        let mut tx=store::begin(&h.app.pool).await.unwrap();store::context(&mut tx,b.user.id,Some(w)).await.unwrap();
        assert!(!store::discovery_event_visible(&mut tx,w,b.user.id,"view.deleted",saved.id).await.unwrap());tx.rollback().await.unwrap();
    }).await;
    h.cleanup().await;
    if let Err(error) = result {
        std::panic::resume_unwind(error.into_panic());
    }
}
