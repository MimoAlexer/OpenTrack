//! Authored project context remains immutable, honest about legacy data, and bounded on the wire.
#[allow(dead_code)]
mod support;
use opentrack_app::worker::{Worker, WorkerOptions};
use opentrack_domain::*;
use opentrack_store as store;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use support::Harness;
use uuid::Uuid;

const PASSWORD: &str = "Synthetic update context password 2026!";
fn req<T: serde::de::DeserializeOwned>(value: Value) -> T {
    serde_json::from_value(value).unwrap()
}
fn digest(value: &[u8]) -> String {
    format!("{:x}", Sha256::digest(value))
}

#[tokio::test]
async fn project_note_validation_preserves_api_inserts_without_worker_callable_authority() {
    let h = Harness::new().await;
    let copy = h.clone();
    let run = tokio::spawn(async move { validation_capabilities(&copy).await }).await;
    h.cleanup().await;
    if let Err(error) = run {
        std::panic::resume_unwind(error.into_panic());
    }
}
async fn validation_capabilities(h: &Harness) {
    let (a, w, t, p) = setup(h).await;
    // This must exercise the unchanged complete worker admission check, not a
    // hand-maintained subset of its capability predicates.
    let worker_url = h.worker_url().await;
    let worker = Worker::connect(
        &worker_url,
        WorkerOptions {
            max_connections: 2,
            ..WorkerOptions::default()
        },
    )
    .await
    .unwrap();
    assert!(
        !sqlx::query_scalar::<_, bool>(
            "SELECT has_function_privilege(current_user,'project_update_notes_guard()','EXECUTE')"
        )
        .fetch_one(&worker.pool)
        .await
        .unwrap()
    );
    assert!(
        !sqlx::query_scalar::<_, bool>(
            "SELECT has_function_privilege(current_user,'project_update_notes_guard()','EXECUTE')"
        )
        .fetch_one(&h.app.pool)
        .await
        .unwrap()
    );
    let private:bool=sqlx::query_scalar("SELECT NOT p.prosecdef AND p.prorettype='trigger'::regtype AND p.proconfig=ARRAY[format('search_path=pg_catalog, %I, pg_temp',current_schema())] AND NOT EXISTS(SELECT 1 FROM aclexplode(coalesce(p.proacl,acldefault('f',p.proowner))) a WHERE a.grantee=0 AND a.privilege_type='EXECUTE') FROM pg_proc p WHERE p.oid='project_update_notes_guard()'::regprocedure").fetch_one(&h.owner).await.unwrap();
    assert!(private);
    for key in ["private-validator-first", "private-validator-second"] {
        let update = h
            .app
            .create_project_update(
                &a,
                w,
                t,
                p.id,
                key,
                req(json!({"body":"Reviewed with private validation","notes":notes()})),
            )
            .await
            .unwrap();
        assert_eq!(
            update.context.unwrap().notes.unwrap().risks[0].title,
            "Delivery risk"
        );
        // Reapplying the deployment grant script must not make this helper
        // callable or make a restricted worker fail its admission check.
        let url = h.worker_url().await;
        let repeated = Worker::connect(
            &url,
            WorkerOptions {
                max_connections: 2,
                ..WorkerOptions::default()
            },
        )
        .await
        .unwrap();
        assert!(!sqlx::query_scalar::<_,bool>("SELECT has_function_privilege(current_user,'project_update_notes_guard()','EXECUTE')").fetch_one(&repeated.pool).await.unwrap());
        repeated.pool.close().await;
    }
    worker.pool.close().await;
}

async fn setup(h: &Harness) -> (Actor, Uuid, Uuid, Project) {
    let (session, token) = h.app.setup(req(json!({"setup_token":h.setup_token,"email":"owner@update-context.example.test","display_name":"Example Owner","password":PASSWORD,"workspace_name":"Update context","team_name":"Engineering","team_key":"ENG"}))).await.unwrap();
    let owner = h.app.authenticate(&token).await.unwrap();
    let w = session.workspaces[0].id;
    let t = h.app.bootstrap(&owner, w).await.unwrap().teams[0].id;
    let project = h.app.create_project(&owner,w,t,"context-project",req(json!({"name":"Context project","start_date":"2026-11-01","target_date":"2026-12-01"}))).await.unwrap();
    (owner, w, t, project)
}
fn notes() -> Value {
    json!({"risks":[{"id":"00000000-0000-4000-8000-000000000011","title":" Delivery risk ","detail":" Review supplier timing ","status":"open"}],"decisions":[{"id":"00000000-0000-4000-8000-000000000012","title":"Choose rollout","detail":"Review both options","status":"needed"}]})
}
async fn counts(h: &Harness, a: &Actor, w: Uuid) -> Value {
    let mut tx = h.context(a.user.id, w).await;
    let value=sqlx::query_scalar("SELECT jsonb_build_object('updates',(SELECT count(*) FROM project_updates),'receipts',(SELECT count(*) FROM idempotency_records),'events',(SELECT count(*) FROM workspace_events),'audit',(SELECT count(*) FROM audit_entries))").fetch_one(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    value
}

#[tokio::test]
async fn immutable_snapshots_preconditions_replays_and_page_boundary_comparisons() {
    let h = Harness::new().await;
    let copy = h.clone();
    let run = tokio::spawn(async move { snapshots(&copy).await }).await;
    h.cleanup().await;
    if let Err(e) = run {
        std::panic::resume_unwind(e.into_panic());
    }
}
async fn snapshots(h: &Harness) {
    let (a, w, t, p) = setup(h).await;
    let first = h
        .app
        .create_project_update(
            &a,
            w,
            t,
            p.id,
            "body-only-update",
            req(json!({"body":"Original dates, context not reviewed"})),
        )
        .await
        .unwrap();
    assert!(first.context.as_ref().unwrap().notes.is_none());
    assert!(first.previous.is_none());
    assert_eq!(
        first
            .context
            .as_ref()
            .unwrap()
            .target_date
            .unwrap()
            .to_string(),
        "2026-12-01"
    );
    let changed = h
        .app
        .update_project_metadata(
            &a,
            w,
            t,
            p.id,
            &p.metadata_etag,
            req(json!({"target_date":"2026-12-15","health":"at_risk"})),
        )
        .await
        .unwrap();
    let before = counts(h, &a, w).await;
    assert!(matches!(
        h.app
            .create_project_update_checked(
                &a,
                w,
                t,
                p.id,
                "stale-context-update",
                Some(&p.metadata_etag),
                req(json!({"body":"Stale draft","notes":notes()}))
            )
            .await,
        Err(Error::VersionConflict(_))
    ));
    assert_eq!(counts(h, &a, w).await, before);
    let second = h
        .app
        .create_project_update_checked(
            &a,
            w,
            t,
            p.id,
            "reviewed-context-update",
            Some(&changed.etag),
            req(json!({"body":"Reviewed plan","notes":notes()})),
        )
        .await
        .unwrap();
    let context = second.context.as_ref().unwrap();
    assert_eq!(context.target_date.unwrap().to_string(), "2026-12-15");
    assert_eq!(
        context.notes.as_ref().unwrap().risks[0].title,
        "Delivery risk"
    );
    assert_eq!(second.previous.as_ref().unwrap().id, first.id);
    assert!(
        second
            .previous
            .as_ref()
            .unwrap()
            .context
            .as_ref()
            .unwrap()
            .notes
            .is_none()
    );
    let newest = h
        .app
        .update_project_metadata(
            &a,
            w,
            t,
            p.id,
            &changed.etag,
            req(json!({"target_date":null,"health":"on_track"})),
        )
        .await
        .unwrap();
    let replay = h
        .app
        .create_project_update_checked(
            &a,
            w,
            t,
            p.id,
            "reviewed-context-update",
            Some(&changed.etag),
            req(json!({"body":"Reviewed plan","notes":notes()})),
        )
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(&replay).unwrap(),
        serde_json::to_value(&second).unwrap()
    );
    let mut different = notes();
    different["risks"][0]["status"] = json!("closed");
    assert!(matches!(
        h.app
            .create_project_update(
                &a,
                w,
                t,
                p.id,
                "reviewed-context-update",
                req(json!({"body":"Reviewed plan","notes":different}))
            )
            .await,
        Err(Error::Conflict(_))
    ));
    let page = h
        .app
        .project_updates(&a, w, t, p.id, req(json!({"limit":1})))
        .await
        .unwrap();
    assert_eq!(page.data[0].id, second.id);
    assert_eq!(page.data[0].previous.as_ref().unwrap().id, first.id);
    assert!(page.has_more);
    let next = h
        .app
        .project_updates(
            &a,
            w,
            t,
            p.id,
            req(json!({"limit":1,"cursor":page.next_cursor})),
        )
        .await
        .unwrap();
    assert_eq!(next.data[0].id, first.id);
    assert!(!next.has_more);
    assert!(next.data[0].previous.is_none());
    assert_eq!(
        serde_json::to_value(
            h.app
                .get_project_update(&a, w, t, p.id, second.id)
                .await
                .unwrap()
        )
        .unwrap(),
        serde_json::to_value(&second).unwrap()
    );
    assert!(matches!(
        h.app
            .project_updates(&a, w, t, p.id, req(json!({"limit":26})))
            .await,
        Err(Error::Invalid(_))
    ));
    for statement in [
        "UPDATE project_updates SET notes=NULL WHERE id=$1",
        "DELETE FROM project_updates WHERE id=$1",
    ] {
        let mut tx = h.context(a.user.id, w).await;
        assert!(
            sqlx::query(statement)
                .bind(second.id)
                .execute(&mut *tx)
                .await
                .is_err()
        );
        tx.rollback().await.unwrap();
    }
    let reviewed = h
        .app
        .create_project_update_checked(
            &a,
            w,
            t,
            p.id,
            "empty-reviewed-update",
            Some(&newest.etag),
            req(json!({"body":"No notes after review","notes":{"risks":[],"decisions":[]}})),
        )
        .await
        .unwrap();
    assert!(
        reviewed
            .context
            .as_ref()
            .unwrap()
            .notes
            .as_ref()
            .unwrap()
            .risks
            .is_empty()
    );
    assert!(reviewed.context.as_ref().unwrap().target_date.is_none());
    let mut escaped = notes();
    escaped["risks"][0]["title"] = json!("Quoted \"risk\" \\");
    escaped["risks"][0]["detail"] = json!("First\tline\nSecond \\ line \"quoted\"");
    let escaping = h
        .app
        .create_project_update(
            &a,
            w,
            t,
            p.id,
            "escaped-update-context",
            req(json!({"body":"Quotes \" and slash \\ with\nnew line","notes":escaped})),
        )
        .await
        .unwrap();
    let stored = h
        .app
        .get_project_update(&a, w, t, p.id, escaping.id)
        .await
        .unwrap();
    assert_eq!(
        stored
            .context
            .as_ref()
            .unwrap()
            .notes
            .as_ref()
            .unwrap()
            .risks[0]
            .detail,
        "First\tline\nSecond \\ line \"quoted\""
    );
    assert_eq!(stored.body, escaping.body);
    let before = counts(h, &a, w).await;
    let mut tx = h.context(a.user.id, w).await;
    sqlx::raw_sql("CREATE FUNCTION reject_update_event() RETURNS trigger LANGUAGE plpgsql AS $$BEGIN RAISE EXCEPTION 'Synthetic event rejection'; END$$; CREATE TRIGGER reject_update_event BEFORE INSERT ON workspace_events FOR EACH ROW EXECUTE FUNCTION reject_update_event();").execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    assert!(
        h.app
            .create_project_update(
                &a,
                w,
                t,
                p.id,
                "rolled-back-context",
                req(json!({"body":"Atomic note publication","notes":notes()}))
            )
            .await
            .is_err()
    );
    assert_eq!(counts(h, &a, w).await, before);
}

#[tokio::test]
async fn schema_29_upgrade_retains_unknown_history_and_old_body_only_receipts() {
    let h = Harness::with_version(5, 29).await;
    let copy = h.clone();
    let run = tokio::spawn(async move { upgrade(&copy).await }).await;
    h.cleanup().await;
    if let Err(e) = run {
        std::panic::resume_unwind(e.into_panic());
    }
}
async fn upgrade(h: &Harness) {
    let (a, w, t, p) = setup(h).await;
    let id = Uuid::new_v4();
    let legacy = json!({"id":id,"author_id":a.user.id,"author_name":a.user.display_name,"body":"Legacy publication","state":"planned","health":"on_track","completed":0,"eligible":0,"created_at":"2026-01-01T00:00:00Z"});
    let key = "legacy-context-retry";
    let route = format!("projects.update.create:{}", p.id);
    let request_hash = digest(br#"{"body":"Legacy publication"}"#);
    let mut tx = h.context(a.user.id, w).await;
    sqlx::query("INSERT INTO project_updates(id,workspace_id,team_id,project_id,author_id,body,state,health,completed,eligible,created_at) VALUES($1,$2,$3,$4,$5,'Legacy publication','planned','on_track',0,0,'2026-01-01')").bind(id).bind(w).bind(t).bind(p.id).bind(a.user.id).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO idempotency_records(workspace_id,principal_id,route,key_hash,request_hash,response_body) VALUES($1,$2,$3,$4,$5,$6)").bind(w).bind(a.user.id).bind(&route).bind(digest(key.as_bytes())).bind(&request_hash).bind(&legacy).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    store::MIGRATOR.run(&h.owner).await.unwrap();
    let old = h.app.get_project_update(&a, w, t, p.id, id).await.unwrap();
    assert!(old.context.is_none());
    assert!(old.previous.is_none());
    let replay = h
        .app
        .create_project_update(
            &a,
            w,
            t,
            p.id,
            key,
            req(json!({"body":"Legacy publication"})),
        )
        .await
        .unwrap();
    assert_eq!(replay.id, id);
    assert!(replay.context.is_none());
    let mut tx = h.context(a.user.id, w).await;
    let receipt: (String, Value) = sqlx::query_as(
        "SELECT request_hash,response_body FROM idempotency_records WHERE route=$1 AND key_hash=$2",
    )
    .bind(&route)
    .bind(digest(key.as_bytes()))
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert_eq!(receipt, (request_hash, legacy));
    tx.commit().await.unwrap();
    let new = h
        .app
        .create_project_update(
            &a,
            w,
            t,
            p.id,
            "new-unknown-notes",
            req(json!({"body":"Dates now captured"})),
        )
        .await
        .unwrap();
    assert!(new.context.is_some());
    assert!(new.context.as_ref().unwrap().notes.is_none());
    assert_eq!(new.previous.as_ref().unwrap().id, id);
    let mut duplicate = notes();
    duplicate["decisions"][0]["id"] = duplicate["risks"][0]["id"].clone();
    for invalid in [
        json!(null),
        json!({"risks":null,"decisions":[]}),
        json!({"risks":[],"decisions":[],"extra":true}),
        duplicate,
        json!({"risks":[{"id":Uuid::new_v4(),"title":"Risk","detail":"","status":"arbitrary"}],"decisions":[]}),
    ] {
        let mut tx = h.context(a.user.id, w).await;
        assert!(sqlx::query("INSERT INTO project_updates(id,workspace_id,team_id,project_id,author_id,body,state,health,completed,eligible,context_captured,notes) VALUES($1,$2,$3,$4,$5,'Invalid notes','planned','on_track',0,0,true,$6)").bind(Uuid::new_v4()).bind(w).bind(t).bind(p.id).bind(a.user.id).bind(invalid).execute(&mut *tx).await.is_err());
        tx.rollback().await.unwrap();
    }
}

#[tokio::test]
async fn maximum_unicode_snapshots_use_lossless_byte_bounded_keyset_pages() {
    let h = Harness::new().await;
    let copy = h.clone();
    let run = tokio::spawn(async move { bounded_pages(&copy).await }).await;
    h.cleanup().await;
    if let Err(e) = run {
        std::panic::resume_unwind(e.into_panic());
    }
}
async fn bounded_pages(h: &Harness) {
    let (a, w, t, p) = setup(h).await;
    let risks=(0..6).map(|n|json!({"id":Uuid::from_u128(100+n),"title":"🧭".repeat(160),"detail":"🧭".repeat(1000),"status":"mitigated"})).collect::<Vec<_>>();
    let decisions=(0..6).map(|n|json!({"id":Uuid::from_u128(200+n),"title":"🧭".repeat(160),"detail":"🧭".repeat(1000),"status":"deferred"})).collect::<Vec<_>>();
    let body = json!({"body":"🧭".repeat(8000),"notes":{"risks":risks,"decisions":decisions}});
    let encoded = serde_json::to_vec(&body).unwrap();
    assert!(encoded.len() > 65536);
    assert!(encoded.len() < 128 * 1024);
    let mut published = vec![Uuid::from_u128(1), Uuid::from_u128(2)];
    let mut tx = h.context(a.user.id, w).await;
    for id in &published {
        sqlx::query("INSERT INTO project_updates(id,workspace_id,team_id,project_id,author_id,body,state,health,completed,eligible,created_at) VALUES($1,$2,$3,$4,$5,'Same timestamp history','planned','on_track',0,0,'2026-01-01')").bind(id).bind(w).bind(t).bind(p.id).bind(a.user.id).execute(&mut *tx).await.unwrap();
    }
    tx.commit().await.unwrap();
    for n in 0..30 {
        published.push(
            h.app
                .create_project_update(
                    &a,
                    w,
                    t,
                    p.id,
                    &format!("maximum-context-{n:03}"),
                    req(body.clone()),
                )
                .await
                .unwrap()
                .id,
        );
    }
    let mut seen = Vec::new();
    let mut cursor = None;
    let mut pages = 0;
    loop {
        let page = h
            .app
            .project_updates(
                &a,
                w,
                t,
                p.id,
                ProjectPageQuery {
                    limit: Some(25),
                    cursor,
                },
            )
            .await
            .unwrap();
        assert!(!page.data.is_empty());
        assert!(serde_json::to_vec(&page).unwrap().len() < 3 * 1024 * 1024);
        for update in &page.data {
            assert!(serde_json::to_vec(update).unwrap().len() < 1024 * 1024);
            if let Some(previous) = &update.previous {
                let position = published.iter().position(|id| *id == update.id).unwrap();
                assert_eq!(previous.id, published[position - 1]);
            }
            seen.push(update.id);
        }
        pages += 1;
        if !page.has_more {
            break;
        }
        assert!(page.data.len() < 25 || pages > 1);
        cursor = page.next_cursor;
    }
    published.reverse();
    assert_eq!(seen, published);
    assert!(pages >= 2);
}

#[tokio::test]
async fn private_project_context_and_receipts_follow_current_access() {
    let h = Harness::new().await;
    let copy = h.clone();
    let run = tokio::spawn(async move { permissions(&copy).await }).await;
    h.cleanup().await;
    if let Err(e) = run {
        std::panic::resume_unwind(e.into_panic());
    }
}
async fn permissions(h: &Harness) {
    let (owner, w, public, p) = setup(h).await;
    let invited = h
        .app
        .create_invitation(
            &owner,
            w,
            "context-member-invite",
            req(json!({"email":"member@update-context.example.test","role":"member"})),
        )
        .await
        .unwrap();
    let (_, token) = h
        .app
        .accept_invitation(
            None,
            req(json!({"token":invited.token,"display_name":"Example Member","password":PASSWORD})),
        )
        .await
        .unwrap();
    let member = h.app.authenticate(&token).await.unwrap();
    let private = h
        .app
        .create_team(
            &owner,
            w,
            "private-context-team",
            req(json!({"name":"Private planning","key":"PRV","private":true})),
        )
        .await
        .unwrap();
    let granted = h
        .app
        .put_team_member(
            &owner,
            w,
            private.id,
            member.user.id,
            None,
            req(json!({"role":"contributor"})),
        )
        .await
        .unwrap();
    let (_, token) = h
        .app
        .login(req(json!({"email":member.user.email,"password":PASSWORD})))
        .await
        .unwrap();
    let member = h.app.authenticate(&token).await.unwrap();
    let project = h
        .app
        .create_project(
            &owner,
            w,
            private.id,
            "private-context-project",
            req(json!({"name":"Private context"})),
        )
        .await
        .unwrap();
    let first = h
        .app
        .create_project_update(
            &owner,
            w,
            private.id,
            project.id,
            "private-owner-update",
            req(json!({"body":"Private baseline","notes":notes()})),
        )
        .await
        .unwrap();
    let second = h
        .app
        .create_project_update(
            &member,
            w,
            private.id,
            project.id,
            "private-member-update",
            req(json!({"body":"Private follow-up","notes":notes()})),
        )
        .await
        .unwrap();
    assert_eq!(second.previous.as_ref().unwrap().id, first.id);
    assert!(matches!(
        h.app
            .get_project_update(&owner, w, public, project.id, second.id)
            .await,
        Err(Error::NotFound)
    ));
    let membership = h
        .app
        .get_workspace_member(&owner, w, member.user.id)
        .await
        .unwrap();
    h.app
        .update_member(
            &owner,
            w,
            member.user.id,
            &membership.etag(w),
            req(json!({"role":"viewer"})),
        )
        .await
        .unwrap();
    assert_eq!(
        h.app
            .get_project_update(&member, w, private.id, project.id, second.id)
            .await
            .unwrap()
            .id,
        second.id
    );
    assert!(matches!(
        h.app
            .create_project_update(
                &member,
                w,
                private.id,
                project.id,
                "private-member-update",
                req(json!({"body":"Private follow-up","notes":notes()}))
            )
            .await,
        Err(Error::Forbidden)
    ));
    h.app
        .remove_team_member(
            &owner,
            w,
            private.id,
            member.user.id,
            &granted.entity.etag(private.id),
        )
        .await
        .unwrap();
    assert!(matches!(
        h.app
            .project_updates(
                &member,
                w,
                private.id,
                project.id,
                ProjectPageQuery::default()
            )
            .await,
        Err(Error::NotFound)
    ));
    assert!(matches!(
        h.app
            .get_project_update(&member, w, private.id, project.id, second.id)
            .await,
        Err(Error::NotFound)
    ));
    assert!(matches!(
        h.app
            .create_project_update(
                &member,
                w,
                private.id,
                project.id,
                "private-member-update",
                req(json!({"body":"Private follow-up","notes":notes()}))
            )
            .await,
        Err(Error::NotFound)
    ));
    h.app.logout(&member).await.unwrap();
    assert!(matches!(
        h.app
            .get_project_update(&member, w, public, p.id, first.id)
            .await,
        Err(Error::Unauthorized)
    ));
    assert!(matches!(
        h.app.authenticate(&token).await,
        Err(Error::Unauthorized)
    ));
}

async fn blocked_by(h: &Harness, blocker: i32) -> i32 {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if let Some(pid) = sqlx::query_scalar::<_, i32>(
                "SELECT pid FROM pg_stat_activity WHERE $1=ANY(pg_blocking_pids(pid)) LIMIT 1",
            )
            .bind(blocker)
            .fetch_optional(&h.owner)
            .await
            .unwrap()
            {
                return pid;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("Observe the real PostgreSQL lock dependency")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn publication_and_member_revocation_have_one_observed_commit_order() {
    let h = Harness::new().await;
    let copy = h.clone();
    let run = tokio::spawn(async move { revocation_order(&copy).await }).await;
    h.cleanup().await;
    if let Err(e) = run {
        std::panic::resume_unwind(e.into_panic());
    }
}
async fn revocation_order(h: &Harness) {
    let (owner, w, t, p) = setup(h).await;
    let invited = h
        .app
        .create_invitation(
            &owner,
            w,
            "context-race-invitation",
            req(json!({"email":"race@update-context.example.test","role":"member"})),
        )
        .await
        .unwrap();
    let (_, token) = h
        .app
        .accept_invitation(
            None,
            req(json!({"token":invited.token,"display_name":"Example Member","password":PASSWORD})),
        )
        .await
        .unwrap();
    let member = h.app.authenticate(&token).await.unwrap();
    let membership = h
        .app
        .get_workspace_member(&owner, w, member.user.id)
        .await
        .unwrap();
    let mut gate = h.context(owner.user.id, w).await;
    sqlx::query("SELECT id FROM teams WHERE id=$1 FOR UPDATE")
        .bind(t)
        .fetch_one(&mut *gate)
        .await
        .unwrap();
    let gate_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *gate)
        .await
        .unwrap();
    let app = h.app.clone();
    let actor = member.clone();
    let publication = tokio::spawn(async move {
        app.create_project_update(
            &actor,
            w,
            t,
            p.id,
            "revocation-race-update",
            req(json!({"body":"Publication before access revocation","notes":notes()})),
        )
        .await
    });
    let publication_pid = blocked_by(h, gate_pid).await;
    let app = h.app.clone();
    let administrator = owner.clone();
    let target = member.user.id;
    let revocation = tokio::spawn(async move {
        app.update_member(
            &administrator,
            w,
            target,
            &membership.etag(w),
            req(json!({"active":false})),
        )
        .await
    });
    blocked_by(h, publication_pid).await;
    gate.commit().await.unwrap();
    let published = publication.await.unwrap().unwrap();
    assert!(!revocation.await.unwrap().unwrap().entity.active);
    assert_eq!(
        h.app
            .get_project_update(&owner, w, t, p.id, published.id)
            .await
            .unwrap()
            .id,
        published.id
    );
    assert!(matches!(
        h.app
            .get_project_update(&member, w, t, p.id, published.id)
            .await,
        Err(Error::NotFound | Error::Forbidden | Error::Unauthorized)
    ));
    assert!(matches!(
        h.app
            .create_project_update(
                &member,
                w,
                t,
                p.id,
                "revocation-race-update",
                req(json!({"body":"Publication before access revocation","notes":notes()}))
            )
            .await,
        Err(Error::NotFound | Error::Forbidden | Error::Unauthorized)
    ));
}
