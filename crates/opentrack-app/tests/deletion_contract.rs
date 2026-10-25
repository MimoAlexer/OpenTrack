//! Real PostgreSQL atomic tombstones, repair, retained history and bounded effects.
mod graph_support;
#[allow(dead_code)]
mod support;
use graph_support::*;
use opentrack_domain::*;
use opentrack_store as store;
use serde_json::json;
use sqlx::AssertSqlSafe;
use support::Harness;
use uuid::Uuid;

fn repair(issue: &Issue) -> RestoreIssue {
    RestoreIssue {
        status_id: issue.status_id,
        assignee_id: issue.assignee_id,
        label_ids: issue.label_ids.clone(),
    }
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_postgres_deletion_atomicity_and_upgrade() {
    let h = Harness::with_version(8, 13).await;
    let copy = h.clone();
    let run = tokio::spawn(async move { exercise(&copy).await }).await;
    if let Err(e) = run {
        std::panic::resume_unwind(e.into_panic());
    }
    h.cleanup().await;
}
async fn exercise(h: &Harness) {
    let (a, w, t) = setup(h).await;
    let body = json!({"team_id":t,"title":"Retained create receipt"});
    let source = historical_issue(h, &a, w, t, body.clone(), Some("before-deletion-upgrade")).await;
    let parent_issue = historical_issue(h, &a, w, t, json!({}), None).await;
    parent(h, &a, w, source.id, Some(parent_issue.id)).await;
    let child = historical_issue(
        h,
        &a,
        w,
        t,
        json!({"parent_id":source.id,"parent_etag":fresh(h,&a,w,source.id).await.etag()}),
        None,
    )
    .await;
    let historical = historical_issue(
        h,
        &a,
        w,
        t,
        json!({"parent_id":source.id,"parent_etag":fresh(h,&a,w,source.id).await.etag()}),
        None,
    )
    .await;
    let peer = historical_issue(h, &a, w, t, json!({}), None).await;
    let incoming = historical_issue(h, &a, w, t, json!({}), None).await;
    relate(h, &a, w, source.id, peer.id, "related").await;
    relate(h, &a, w, incoming.id, source.id, "blocks").await;
    // Seed a pre-0014 tombstone still linked to its parent; new commands must repair
    // this historical endpoint without reviving it or cascading into descendants.
    state(h, &a, w, historical.id, true).await;
    let before_upgrade = snapshot(h, &a, w).await;
    // Prove the deletion migration itself preserves every retained row before
    // applying later migrations required by the current application.
    sqlx::migrate::Migrator {
        migrations: store::MIGRATOR
            .iter()
            .filter(|migration| migration.version <= 14)
            .cloned()
            .collect::<Vec<_>>()
            .into(),
        ..sqlx::migrate::Migrator::DEFAULT
    }
    .run(&h.owner)
    .await
    .unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT max(version) FROM _sqlx_migrations")
            .fetch_one(&h.owner)
            .await
            .unwrap(),
        14
    );
    assert_eq!(snapshot(h, &a, w).await, before_upgrade);
    let mut tx = h.context(a.user.id, w).await;
    let ranks: Vec<(Uuid,i64)> = sqlx::query_as("SELECT id,row_number() OVER(PARTITION BY team_id,status_id ORDER BY created_at DESC,id DESC)*1024 FROM issues WHERE workspace_id=$1")
        .bind(w).fetch_all(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    store::MIGRATOR.run(&h.owner).await.unwrap();
    // 0015 adds explicit attribution/schema defaults without rewriting history.
    // Compare the complete new snapshot, not a projection that would conceal
    // unrelated changes. Subsequent rollback checks retain all current fields.
    let mut expected_current = before_upgrade.clone();
    for issue in expected_current["issues"].as_array_mut().unwrap() {
        issue["body_version"] = json!(1);
        let rank = ranks
            .iter()
            .find(|(id, _)| issue["id"] == id.to_string())
            .unwrap()
            .1;
        issue["rank"] = serde_json::from_str(&format!("{rank}.000000000000000000")).unwrap();
    }
    for team in expected_current["teams"].as_array_mut().unwrap() {
        team["ordering_version"] = json!(1);
    }
    for activity in expected_current["activity"].as_array_mut().unwrap() {
        activity["actor_kind"] = json!("account");
        activity["system_job_id"] = json!(null);
    }
    for event in expected_current["events"].as_array_mut().unwrap() {
        event["schema_version"] = json!(1);
        event["system_job_id"] = json!(null);
        event["storage_bytes"] = json!(
            192 + event["event_type"].as_str().unwrap().len()
                + event["actor_kind"].as_str().unwrap().len()
        );
    }
    assert_eq!(snapshot(h, &a, w).await, expected_current);
    let mut tx = h.context(a.user.id, w).await;
    assert!(sqlx::query_scalar::<_, bool>("SELECT bool_and(actor_kind='account' AND actor_id IS NOT NULL AND system_job_id IS NULL) FROM audit_entries WHERE workspace_id=$1")
        .bind(w).fetch_one(&mut *tx).await.unwrap());
    tx.commit().await.unwrap();
    assert_eq!(
        h.app
            .create_issue(&a, w, "before-deletion-upgrade", req(body.clone()))
            .await
            .unwrap()
            .id,
        source.id
    );
    let preview = h
        .app
        .issue_deletion_preview(&a, w, source.id)
        .await
        .unwrap();
    assert!(preview.can_delete && preview.has_parent);
    assert_eq!((preview.child_count, preview.relationship_count), (2, 2));
    let before = snapshot(h, &a, w).await;
    for (table, event) in [
        ("issues", "UPDATE"),
        ("issue_relations", "DELETE"),
        ("teams", "UPDATE"),
        ("activity_entries", "INSERT"),
        ("audit_entries", "INSERT"),
        ("workspace_events", "INSERT"),
    ] {
        fault(h, table, event).await;
        assert!(
            matches!(
                h.app
                    .delete_issue(&a, w, source.id, &preview.issue.etag())
                    .await,
                Err(Error::Internal)
            ),
            "{table} suppression must fail"
        );
        clear(h, table).await;
        assert_eq!(
            snapshot(h, &a, w).await,
            before,
            "{table} deletion rollback"
        );
    }
    // Suppressing only the source tombstone must not commit earlier child/edge work.
    sqlx::raw_sql("CREATE FUNCTION delete_source_fault() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.deleted_at IS NOT NULL AND OLD.deleted_at IS NULL THEN RETURN NULL; END IF; RETURN NEW; END $$; CREATE TRIGGER delete_source_fault BEFORE UPDATE ON issues FOR EACH ROW EXECUTE FUNCTION delete_source_fault()").execute(&h.owner).await.unwrap();
    assert!(matches!(
        h.app
            .delete_issue(&a, w, source.id, &preview.issue.etag())
            .await,
        Err(Error::Internal)
    ));
    sqlx::raw_sql(
        "DROP TRIGGER delete_source_fault ON issues; DROP FUNCTION delete_source_fault()",
    )
    .execute(&h.owner)
    .await
    .unwrap();
    assert_eq!(snapshot(h, &a, w).await, before);
    let version_before = epoch(h, &a, w, t).await;
    h.app
        .delete_issue(&a, w, source.id, &preview.issue.etag())
        .await
        .unwrap();
    assert_eq!(epoch(h, &a, w, t).await, version_before + 1);
    let tombstone = h.app.get_deleted_issue(&a, w, source.id).await.unwrap();
    advanced(&preview.issue, &tombstone.issue);
    assert!(tombstone.can_restore);
    assert_eq!(
        tombstone.restore_until - tombstone.deleted_at,
        chrono::Duration::days(30)
    );
    assert!(matches!(
        h.app.get_issue(&a, w, source.id).await,
        Err(Error::NotFound)
    ));
    assert!(matches!(
        h.app.issue_activity(&a, w, source.id).await,
        Err(Error::NotFound)
    ));
    assert!(matches!(
        h.app.resolve_issue_key(&a, w, &source.key).await,
        Err(Error::NotFound)
    ));
    assert!(matches!(
        h.app
            .create_issue(&a, w, "before-deletion-upgrade", req(body))
            .await,
        Err(Error::NotFound)
    ));
    let after = snapshot(h, &a, w).await;
    for id in [
        source.id,
        parent_issue.id,
        child.id,
        historical.id,
        peer.id,
        incoming.id,
    ] {
        let old = before["issues"]
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["id"] == id.to_string())
            .unwrap();
        let new = after["issues"]
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["id"] == id.to_string())
            .unwrap();
        assert_eq!(
            new["version"].as_i64().unwrap(),
            old["version"].as_i64().unwrap() + 1
        );
        if [source.id, child.id, historical.id].contains(&id) {
            assert!(new["parent_id"].is_null());
        }
    }
    assert!(after["relations"].as_array().unwrap().is_empty());
    assert!(h.app.get_issue(&a, w, child.id).await.is_ok());
    assert!(h.app.get_deleted_issue(&a, w, historical.id).await.is_ok());
    let mut tx = h.context(a.user.id, w).await;
    let linked:bool=sqlx::query_scalar("SELECT count(DISTINCT e.mutation_id)=1 AND count(*)=6 AND min(e.seq)+5=max(e.seq) FROM workspace_events e JOIN audit_entries a ON a.id=e.mutation_id WHERE a.action='issue.deleted' AND a.target_id=$1").bind(source.id).fetch_one(&mut *tx).await.unwrap();
    assert!(
        linked,
        "one delete correlation and contiguous events for every endpoint"
    );
    tx.commit().await.unwrap();
    for (table, event) in [
        ("issues", "UPDATE"),
        ("teams", "UPDATE"),
        ("activity_entries", "INSERT"),
        ("audit_entries", "INSERT"),
        ("workspace_events", "INSERT"),
    ] {
        let before = snapshot(h, &a, w).await;
        fault(h, table, event).await;
        assert!(
            matches!(
                h.app
                    .restore_issue(
                        &a,
                        w,
                        source.id,
                        &tombstone.issue.etag(),
                        repair(&tombstone.issue)
                    )
                    .await,
                Err(Error::Internal)
            ),
            "{table} restore suppression"
        );
        clear(h, table).await;
        assert_eq!(snapshot(h, &a, w).await, before);
    }
    let restored = h
        .app
        .restore_issue(
            &a,
            w,
            source.id,
            &tombstone.issue.etag(),
            repair(&tombstone.issue),
        )
        .await
        .unwrap();
    advanced(&tombstone.issue, &restored);
    assert_eq!(restored.key, source.key);
    assert!(
        h.app
            .issue_parent(&a, w, source.id)
            .await
            .unwrap()
            .parent
            .is_none()
    );
    assert!(
        h.app
            .issue_children(&a, w, source.id, req(json!({})))
            .await
            .unwrap()
            .data
            .is_empty()
    );
    assert!(
        h.app
            .issue_relations(&a, w, source.id, req(json!({})))
            .await
            .unwrap()
            .data
            .is_empty()
    );
    direct_guards(h, &a, w, t).await;
    fanout_chunks(h, &a, w, t).await;
    limit(h, &a, w, t).await;
    worker_still_status_only(h, &a, w, t).await;
}
async fn direct_guards(h: &Harness, a: &Actor, w: Uuid, t: Uuid) {
    let deleted = issue(h, a, w, t, json!({})).await;
    h.app
        .delete_issue(a, w, deleted.id, &deleted.etag())
        .await
        .unwrap();
    let active = issue(h, a, w, t, json!({})).await;
    for statement in [
        "UPDATE issues SET parent_id=$2 WHERE id=$1",
        "INSERT INTO issue_relations(id,workspace_id,team_id,source_id,target_id,kind,created_by) SELECT gen_random_uuid(),workspace_id,team_id,id,$2,'blocks',created_by_user_id FROM issues WHERE id=$1",
    ] {
        let mut tx = store::begin(&h.app.pool).await.unwrap();
        store::context(&mut tx, a.user.id, Some(w)).await.unwrap();
        let result = sqlx::query(AssertSqlSafe(statement))
            .bind(active.id)
            .bind(deleted.id)
            .execute(&mut *tx)
            .await;
        assert!(result.is_err());
        tx.rollback().await.unwrap();
    }
    let mut tx = h.context(a.user.id, w).await;
    sqlx::query("UPDATE issues SET deleted_at=clock_timestamp()-interval '31 days' WHERE id=$1")
        .bind(deleted.id)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let mut tx = store::begin(&h.app.pool).await.unwrap();
    store::context(&mut tx, a.user.id, Some(w)).await.unwrap();
    assert!(
        sqlx::query("UPDATE issues SET deleted_at=NULL WHERE id=$1")
            .bind(deleted.id)
            .execute(&mut *tx)
            .await
            .is_err()
    );
    tx.rollback().await.unwrap();
    assert!(
        !h.app
            .get_deleted_issue(a, w, deleted.id)
            .await
            .unwrap()
            .can_restore
    );
}
async fn fanout_chunks(h: &Harness, a: &Actor, w: Uuid, t: Uuid) {
    let source = issue(h, a, w, t, json!({})).await;
    let mut tx = h.context(a.user.id, w).await;
    let number:i64=sqlx::query_scalar("UPDATE teams SET next_issue_number=next_issue_number+105 WHERE id=$1 RETURNING next_issue_number-105").bind(t).fetch_one(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO issues(id,workspace_id,team_id,created_by_user_id,number,title,status_id,parent_id,rank) SELECT gen_random_uuid(),$1,$2,$3,$4+n,'Deletion fanout',$5,$6,($4+n)::numeric*1024 FROM generate_series(0,104) n").bind(w).bind(t).bind(a.user.id).bind(number).bind(source.status_id).bind(source.id).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    let preview = h.app.issue_deletion_preview(a, w, source.id).await.unwrap();
    assert_eq!(preview.child_count, 105);
    h.app
        .delete_issue(a, w, source.id, &preview.issue.etag())
        .await
        .unwrap();
    let mut tx = h.context(a.user.id, w).await;
    let result:(i64,i64,i64)=sqlx::query_as("SELECT count(*),min(e.seq),max(e.seq) FROM workspace_events e JOIN audit_entries a ON a.id=e.mutation_id WHERE a.action='issue.deleted' AND a.target_id=$1").bind(source.id).fetch_one(&mut *tx).await.unwrap();
    assert_eq!(result.0, 106);
    assert_eq!(result.2 - result.1, 105);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM issues WHERE parent_id=$1")
            .bind(source.id)
            .fetch_one(&mut *tx)
            .await
            .unwrap(),
        0
    );
    tx.commit().await.unwrap();
}
async fn limit(h: &Harness, a: &Actor, w: Uuid, t: Uuid) {
    let source = issue(h, a, w, t, json!({})).await;
    let mut tx = h.context(a.user.id, w).await;
    // A legal related star is larger than the bounded synchronous delete command.
    // Parentless fixture rows do not need 10,001 redundant recursive traversals.
    sqlx::query("ALTER TABLE issues DISABLE TRIGGER issues_parent_guard")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("SET LOCAL statement_timeout='30s'")
        .execute(&mut *tx)
        .await
        .unwrap();
    let number:i64=sqlx::query_scalar("UPDATE teams SET next_issue_number=next_issue_number+10001 WHERE id=$1 RETURNING next_issue_number-10001").bind(t).fetch_one(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO issues(id,workspace_id,team_id,created_by_user_id,number,title,status_id,rank) SELECT gen_random_uuid(),$1,$2,$3,$4+n,'Large incident graph',$5,($4+n)::numeric*1024 FROM generate_series(0,10000) n").bind(w).bind(t).bind(a.user.id).bind(number).bind(source.status_id).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO issue_relations(id,workspace_id,team_id,source_id,target_id,kind,created_by) SELECT gen_random_uuid(),$1,$2,least(id,$3),greatest(id,$3),'related',$4 FROM issues WHERE workspace_id=$1 AND team_id=$2 AND number>=$5 AND number<$5+10001").bind(w).bind(t).bind(source.id).bind(a.user.id).bind(number).execute(&mut *tx).await.unwrap();
    sqlx::query("SET CONSTRAINTS ALL IMMEDIATE")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("ALTER TABLE issues ENABLE TRIGGER issues_parent_guard")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("ANALYZE issues")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("ANALYZE issue_relations")
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let p = h.app.issue_deletion_preview(a, w, source.id).await.unwrap();
    assert_eq!(p.relationship_count, 10001);
    assert!(matches!(
        h.app.delete_issue(a, w, source.id, &p.issue.etag()).await,
        Err(Error::Conflict(_))
    ));
    assert_eq!(
        h.app.get_issue(a, w, source.id).await.unwrap().etag(),
        p.issue.etag()
    );
}
async fn worker_still_status_only(h: &Harness, a: &Actor, w: Uuid, t: Uuid) {
    let pool = sqlx::PgPool::connect(&h.worker_url().await).await.unwrap();
    let item = issue(h, a, w, t, json!({})).await;
    let mut tx = store::begin(&pool).await.unwrap();
    sqlx::query("SELECT set_config('opentrack.workspace_id',$1,true)")
        .bind(w.to_string())
        .execute(&mut *tx)
        .await
        .unwrap();
    assert_eq!(sqlx::query("UPDATE issues SET status_id=$2,version=version+1,updated_at=clock_timestamp() WHERE id=$1").bind(item.id).bind(item.status_id).execute(&mut *tx).await.unwrap().rows_affected(),1);
    tx.rollback().await.unwrap();
    let mut tx = store::begin(&pool).await.unwrap();
    sqlx::query("SELECT set_config('opentrack.workspace_id',$1,true)")
        .bind(w.to_string())
        .execute(&mut *tx)
        .await
        .unwrap();
    assert!(
        sqlx::query("UPDATE issues SET deleted_at=clock_timestamp() WHERE id=$1")
            .bind(item.id)
            .execute(&mut *tx)
            .await
            .is_err()
    );
    tx.rollback().await.unwrap();
    pool.close().await;
}
