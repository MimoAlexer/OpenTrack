//! Real PostgreSQL document versions, current/original access and atomic effects.
mod graph_support;
#[allow(dead_code)]
mod support;
use graph_support::*;
use opentrack_app::worker::{Worker, WorkerOptions};
use opentrack_domain::*;
use opentrack_store as store;
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use support::Harness;
use uuid::Uuid;

fn body(text: &str) -> DocumentWrite {
    req(
        json!({"schema_version":1,"body":{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":text}]}]}}),
    )
}
fn rich(text: &str) -> DocumentWrite {
    req(
        json!({"schema_version":1,"body":{"type":"doc","content":[{"type":"heading","attrs":{"level":2},"content":[{"type":"text","text":text,"marks":[{"type":"bold"}]}]}]}}),
    )
}
async fn state_snapshot(h: &Harness, a: &Actor, w: Uuid) -> Value {
    let tracking = snapshot(h, a, w).await;
    let mut tx = h.context(a.user.id, w).await;
    let documents: Value = sqlx::query_scalar("SELECT jsonb_build_object('documents',(SELECT coalesce(jsonb_agg(to_jsonb(d) ORDER BY to_jsonb(d)::text),'[]') FROM documents d),'revisions',(SELECT coalesce(jsonb_agg(to_jsonb(r) ORDER BY to_jsonb(r)::text),'[]') FROM document_revisions r),'comments',(SELECT coalesce(jsonb_agg(to_jsonb(c) ORDER BY to_jsonb(c)::text),'[]') FROM comments c),'audit',(SELECT coalesce(jsonb_agg(to_jsonb(a) ORDER BY to_jsonb(a)::text),'[]') FROM audit_entries a))")
        .fetch_one(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    json!({"tracking":tracking,"documents":documents})
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_postgres_document_validation_versions_and_conflicts() {
    let h = Harness::new().await;
    let copy = h.clone();
    let run = tokio::spawn(async move { validation_versions(&copy).await }).await;
    h.cleanup().await;
    if let Err(error) = run {
        std::panic::resume_unwind(error.into_panic());
    }
}
async fn validation_versions(h: &Harness) {
    let (a, w, t) = setup(h).await;
    let before = state_snapshot(h, &a, w).await;
    let mut invalid = vec![
        json!({"type":"doc","script":"alert(1)","content":[]}),
        json!({"type":"doc","content":[{"type":"image","attrs":{"src":"https://example.invalid/x"}}]}),
        json!({"type":"doc","content":[{"type":"paragraph","content":[{"type":"mention","attrs":{"id":Uuid::new_v4()}}]}]}),
        json!({"type":"doc","content":[{"type":"heading","attrs":{"level":4},"content":[]}]}),
        json!({"type":"doc","content":[{"type":"paragraph","attrs":{"style":"position:fixed"},"content":[]}]}),
        json!({"type":"doc","content":[{"type":"text","text":"wrong parent"}]}),
        body(&"é".repeat(140_000)).body,
        json!({"type":"doc","content":vec![json!({"type":"paragraph"});10_001]}),
    ];
    let mut deep = json!({"type":"paragraph"});
    for _ in 0..33 {
        deep = json!({"type":"blockquote","content":[deep]});
    }
    invalid.push(json!({"type":"doc","content":[deep]}));
    for href in [
        "javascript:alert(1)",
        "data:text/html,<script>1</script>",
        "ftp://example.invalid/x",
        "https://user:password@example.invalid",
        "https://example.invalid/\nattack",
        "https:\\example.invalid",
        "mailto:person@example.invalid?body=secret",
    ] {
        invalid.push(json!({"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"unsafe link","marks":[{"type":"link","attrs":{"href":href}}]}]}]}));
    }
    for (index, candidate) in invalid.into_iter().enumerate() {
        let result = h.app.create_issue(&a,w,&Uuid::new_v4().to_string(),req(json!({"team_id":t,"title":"Invalid document","document":{"schema_version":1,"body":candidate}}))).await;
        assert!(
            matches!(result, Err(Error::Invalid(_))),
            "Invalid document case {index}: {result:?}"
        );
        assert_eq!(
            state_snapshot(h, &a, w).await,
            before,
            "Invalid body must have no persisted effects"
        );
    }
    assert!(matches!(h.app.create_issue(&a,w,&Uuid::new_v4().to_string(),req(json!({"team_id":t,"title":"Future schema","document":{"schema_version":2,"body":{"type":"doc","content":[]}}}))).await,Err(Error::UnsupportedDocumentSchema)));
    assert!(
        serde_json::from_value::<DocumentWrite>(
            json!({"schema_version":1,"body":{"type":"doc"},"untrusted":true})
        )
        .is_err()
    );
    assert!(matches!(h.app.create_issue(&a,w,&Uuid::new_v4().to_string(),req(json!({"team_id":t,"title":"Ambiguous body","description":"legacy","document":body("rich")}))).await,Err(Error::Invalid(_))));

    let first = issue(h, &a, w, t, json!({"document":body("First body")})).await;
    let document = h.app.issue_document(&a, w, first.id).await.unwrap();
    assert_eq!(
        (document.owner_type.as_str(), document.owner_id),
        ("issue", first.id)
    );
    assert_eq!(document.plain_text, "First body");
    assert_eq!(document.version, "1");
    assert!(document.can_edit && document.schema_supported);
    let next = h
        .app
        .update_document(&a, w, document.id, &document.etag, rich("Second body"))
        .await
        .unwrap();
    let second = fresh(h, &a, w, first.id).await;
    assert_eq!(
        second.version, first.version,
        "Body saves have an independent version"
    );
    assert_eq!(second.description, "Second body");
    assert_ne!(
        second.etag(),
        first.etag(),
        "The issue representation includes the changed body cache"
    );
    assert_ne!(next.etag, document.etag);
    assert_eq!(next.version, "2");
    let conflict = h
        .app
        .update_document(&a, w, document.id, &document.etag, body("Stale draft"))
        .await
        .unwrap_err();
    match conflict {
        Error::VersionConflict(current) => {
            assert_eq!(current, serde_json::to_value(&next).unwrap())
        }
        other => panic!("Expected current document conflict, got {other:?}"),
    }
    assert_eq!(
        h.app.get_document(&a, w, document.id).await.unwrap().body,
        next.body
    );
    let revisions = h
        .app
        .document_revisions(&a, w, document.id, DiscussionQuery::default())
        .await
        .unwrap();
    assert!(revisions.data.iter().any(|r| r.revision == "1"));
    let old = h
        .app
        .document_revision(&a, w, document.id, 1)
        .await
        .unwrap();
    assert_eq!(old.body, document.body);
    assert_eq!(old.plain_text, "First body");
    assert_eq!(old.summary.author.as_ref().unwrap().id, a.user.id);
    assert!(!old.summary.migrated);
    assert_eq!(
        old.summary.expires_at - old.summary.created_at,
        chrono::Duration::days(90)
    );
    assert!(matches!(
        h.app
            .update_issue(
                &a,
                w,
                first.id,
                &first.etag(),
                json!({"title":"stale metadata"})
            )
            .await,
        Err(Error::Stale(_))
    ));
    assert!(matches!(
        h.app
            .update_issue(
                &a,
                w,
                first.id,
                &second.etag(),
                json!({"description":"must not flatten rich document"})
            )
            .await,
        Err(Error::Invalid(_))
    ));
    let updated = h
        .app
        .update_issue(
            &a,
            w,
            first.id,
            &second.etag(),
            json!({"title":"Metadata only"}),
        )
        .await
        .unwrap();
    assert_eq!(
        h.app.get_document(&a, w, document.id).await.unwrap().etag,
        next.etag,
        "Metadata writes must not bump body version"
    );
    assert_ne!(updated.version, second.version);
    let plain = issue(h, &a, w, t, json!({"description":"Plain compatibility"})).await;
    let plain_doc = h.app.issue_document(&a, w, plain.id).await.unwrap();
    let plain = h
        .app
        .update_issue(
            &a,
            w,
            plain.id,
            &plain.etag(),
            json!({"description":"Plain update"}),
        )
        .await
        .unwrap();
    let changed_plain = h.app.issue_document(&a, w, plain.id).await.unwrap();
    assert_eq!(changed_plain.plain_text, "Plain update");
    assert_ne!(changed_plain.etag, plain_doc.etag);

    let mut tx = h.context(a.user.id, w).await;
    sqlx::query("UPDATE documents SET schema_version=99,version=version+1 WHERE id=$1")
        .bind(document.id)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let future = h.app.get_document(&a, w, document.id).await.unwrap();
    assert!(!future.schema_supported && !future.can_edit);
    assert_eq!(future.plain_text, "Second body");
    assert!(matches!(
        h.app
            .update_document(&a, w, document.id, &future.etag, body("Cannot downgrade"))
            .await,
        Err(Error::UnsupportedDocumentSchema)
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_postgres_document_required_effects_are_atomic() {
    let h = Harness::new().await;
    let copy = h.clone();
    let run = tokio::spawn(async move { atomicity(&copy).await }).await;
    h.cleanup().await;
    if let Err(error) = run {
        std::panic::resume_unwind(error.into_panic());
    }
}
async fn atomicity(h: &Harness) {
    let (a, w, t) = setup(h).await;
    let create = json!({"team_id":t,"title":"Atomic initial document","document":rich("Original")});
    let before = state_snapshot(h, &a, w).await;
    for table in [
        "documents",
        "document_revisions",
        "activity_entries",
        "audit_entries",
        "workspace_events",
        "idempotency_records",
    ] {
        fault(h, table, "INSERT").await;
        let result = h
            .app
            .create_issue(&a, w, "atomic-document-create", req(create.clone()))
            .await;
        clear(h, table).await;
        assert!(
            matches!(result, Err(Error::Internal)),
            "Missing {table} initial effect: {result:?}"
        );
        assert_eq!(
            state_snapshot(h, &a, w).await,
            before,
            "Initial document rollback: {table}"
        );
    }
    let issue = h
        .app
        .create_issue(&a, w, "atomic-document-create", req(create))
        .await
        .unwrap();
    let document = h.app.issue_document(&a, w, issue.id).await.unwrap();
    let before = state_snapshot(h, &a, w).await;
    for (table, event) in [
        ("documents", "UPDATE"),
        ("document_revisions", "INSERT"),
        ("issues", "UPDATE"),
        ("activity_entries", "INSERT"),
        ("audit_entries", "INSERT"),
        ("workspace_events", "INSERT"),
        ("workspace_stream_heads", "UPDATE"),
    ] {
        fault(h, table, event).await;
        let result = h
            .app
            .update_document(&a, w, document.id, &document.etag, body("Pending edit"))
            .await;
        clear(h, table).await;
        assert!(
            matches!(result, Err(Error::Internal)),
            "Missing {table} save effect: {result:?}"
        );
        assert_eq!(
            state_snapshot(h, &a, w).await,
            before,
            "Document save rollback: {table}"
        );
    }
    assert_eq!(
        h.app
            .update_document(&a, w, document.id, &document.etag, body("Committed edit"))
            .await
            .unwrap()
            .version,
        "2"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_postgres_document_current_and_historical_access_after_move() {
    let h = Harness::new().await;
    let copy = h.clone();
    let run = tokio::spawn(async move { access_after_move(&copy).await }).await;
    h.cleanup().await;
    if let Err(error) = run {
        std::panic::resume_unwind(error.into_panic());
    }
}
async fn access_after_move(h: &Harness) {
    let (a, w, public) = setup(h).await;
    let private = team(h, &a, w, true).await;
    let outside = person(h, &a, w, "member").await;
    let viewer = person(h, &a, w, "viewer").await;
    let source = issue(
        h,
        &a,
        w,
        private,
        json!({"document":body("Private original")}),
    )
    .await;
    let document = h.app.issue_document(&a, w, source.id).await.unwrap();
    assert!(matches!(
        h.app.get_document(&outside, w, document.id).await,
        Err(Error::NotFound)
    ));
    assert!(matches!(
        h.app
            .update_document(
                &outside,
                w,
                document.id,
                &document.etag,
                body("Forged edit")
            )
            .await,
        Err(Error::NotFound)
    ));
    let preview = h
        .app
        .preview_issue_move(&a, w, source.id, public)
        .await
        .unwrap();
    let current = h
        .app
        .update_document(
            &a,
            w,
            document.id,
            &document.etag,
            body("Reviewed current body"),
        )
        .await
        .unwrap();
    assert!(matches!(
        h.app
            .move_issue(
                &a,
                w,
                source.id,
                "stale-body-move",
                &preview.issue_etag,
                move_body(&preview)
            )
            .await,
        Err(Error::Stale(_))
    ));
    let moved = move_to(h, &a, w, source.id, public).await;
    assert_eq!(
        moved.version.parse::<i64>().unwrap(),
        source.version.parse::<i64>().unwrap() + 1
    );
    assert_eq!(
        h.app
            .get_document(&outside, w, document.id)
            .await
            .unwrap()
            .body,
        current.body,
        "The current body follows its current issue"
    );
    assert!(
        h.app
            .document_revisions(&outside, w, document.id, DiscussionQuery::default())
            .await
            .unwrap()
            .data
            .is_empty(),
        "Original private history must not be exposed by a later public move"
    );
    assert!(matches!(
        h.app.document_revision(&outside, w, document.id, 1).await,
        Err(Error::NotFound)
    ));
    let read_only = h.app.get_document(&viewer, w, document.id).await.unwrap();
    assert!(!read_only.can_edit);
    assert_ne!(
        read_only.etag, current.etag,
        "Permission-bearing representations need distinct tags"
    );
    assert!(matches!(
        h.app
            .update_document(
                &viewer,
                w,
                document.id,
                &read_only.etag,
                body("Viewer write")
            )
            .await,
        Err(Error::Forbidden)
    ));
    let public_doc = h.app.get_document(&outside, w, document.id).await.unwrap();
    let public_revision = h
        .app
        .update_document(
            &outside,
            w,
            document.id,
            &public_doc.etag,
            body("Public revision"),
        )
        .await
        .unwrap();
    assert_eq!(
        h.app
            .document_revision(
                &outside,
                w,
                document.id,
                public_revision.version.parse().unwrap()
            )
            .await
            .unwrap()
            .plain_text,
        "Public revision"
    );
    move_to(h, &a, w, source.id, private).await;
    assert!(matches!(
        h.app.get_document(&outside, w, document.id).await,
        Err(Error::NotFound)
    ));
    assert!(
        matches!(
            h.app
                .document_revision(
                    &outside,
                    w,
                    document.id,
                    public_revision.version.parse().unwrap()
                )
                .await,
            Err(Error::NotFound)
        ),
        "Old public origin alone cannot bypass current private issue access"
    );
    grant(h, &a, w, private, outside.user.id).await;
    assert!(h.app.get_document(&outside, w, document.id).await.is_ok());
    let member = h
        .app
        .get_team_member(&a, w, private, outside.user.id)
        .await
        .unwrap();
    h.app
        .remove_team_member(&a, w, private, outside.user.id, &member.etag(private))
        .await
        .unwrap();
    assert!(matches!(
        h.app.get_document(&outside, w, document.id).await,
        Err(Error::NotFound)
    ));
    assert!(matches!(
        h.app
            .document_revisions(&outside, w, document.id, DiscussionQuery::default())
            .await,
        Err(Error::NotFound)
    ));
}

#[derive(Serialize)]
struct LegacyCreate {
    team_id: Uuid,
    title: String,
    description: String,
    status_id: Option<Uuid>,
    priority: i16,
    assignee_id: Option<Uuid>,
    estimate: Option<i32>,
    due_date: Option<chrono::NaiveDate>,
    mutation_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    label_ids: Vec<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    parent_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    parent_etag: Option<String>,
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_postgres_document_genuine_15_to_16_upgrade() {
    let h = Harness::with_version(5, 15).await;
    let copy = h.clone();
    let run = tokio::spawn(async move { upgrade(&copy).await }).await;
    h.cleanup().await;
    if let Err(error) = run {
        std::panic::resume_unwind(error.into_panic());
    }
}
async fn upgrade(h: &Harness) {
    let (a, w, t) = setup(h).await;
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT max(version) FROM _sqlx_migrations")
            .fetch_one(&h.owner)
            .await
            .unwrap(),
        15
    );
    let old_text = "\"\\\n".repeat(87_381);
    assert!(old_text.len() <= 262_144);
    assert!(serde_json::to_vec(&body(&old_text).body).unwrap().len() > 262_144);
    let id = Uuid::new_v4();
    let mut tx = h.context(a.user.id, w).await;
    let status: Uuid =
        sqlx::query_scalar("SELECT id FROM workflow_statuses WHERE team_id=$1 AND is_default")
            .bind(t)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    sqlx::query("INSERT INTO issues(id,workspace_id,team_id,created_by_user_id,number,title,description,status_id) VALUES($1,$2,$3,$4,1,'Legacy escaped body',$5,$6)").bind(id).bind(w).bind(t).bind(a.user.id).bind(&old_text).bind(status).execute(&mut *tx).await.unwrap();
    sqlx::query("UPDATE teams SET next_issue_number=2 WHERE id=$1")
        .bind(t)
        .execute(&mut *tx)
        .await
        .unwrap();
    let raw: Value = sqlx::query_scalar("SELECT to_jsonb(i) FROM issues i WHERE id=$1")
        .bind(id)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    let old_etag = format!(
        "\"issue:{id}:1:{}\"",
        digest(json!({"assignee":null,"labels":[]}).to_string().as_bytes())
    );
    let old_receipt = json!({"id":id,"workspace_id":w,"team_id":t,"key":"GRA-1","title":"Legacy escaped body","description":old_text,"status_id":status,"priority":0,"assignee_id":null,"assignee":null,"label_ids":[],"labels":[],"etag":old_etag,"estimate":null,"due_date":null,"version":"1","created_at":raw["created_at"],"updated_at":raw["updated_at"],"archived_at":null});
    let old_request = LegacyCreate {
        team_id: t,
        title: "Legacy escaped body".into(),
        description: old_text.clone(),
        status_id: None,
        priority: 0,
        assignee_id: None,
        estimate: None,
        due_date: None,
        mutation_id: None,
        label_ids: vec![],
        parent_id: None,
        parent_etag: None,
    };
    let encoded = serde_json::to_vec(&old_request).unwrap();
    let key = "pre-document-create";
    sqlx::query("INSERT INTO idempotency_records(workspace_id,principal_id,route,key_hash,request_hash,response_body) VALUES($1,$2,'issues.create',$3,$4,$5)").bind(w).bind(a.user.id).bind(digest(key.as_bytes())).bind(digest(&encoded)).bind(&old_receipt).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    store::MIGRATOR.run(&h.owner).await.unwrap();
    let doc = h.app.issue_document(&a, w, id).await.unwrap();
    assert_eq!(doc.body, body(&old_text).body);
    assert_eq!(doc.plain_text, old_text);
    assert_eq!(doc.version, "1");
    let revision = h.app.document_revision(&a, w, doc.id, 1).await.unwrap();
    assert_eq!(revision.plain_text, old_text);
    assert_eq!(revision.summary.author.as_ref().unwrap().id, a.user.id);
    assert!(!revision.summary.migrated);
    let mut modern: CreateIssue = serde_json::from_slice(&encoded).unwrap();
    modern.validate().unwrap();
    assert_eq!(
        digest(&serde_json::to_vec(&modern).unwrap()),
        digest(&encoded)
    );
    let replay = h.app.create_issue(&a, w, key, modern).await.unwrap();
    assert_eq!(replay.id, id);
    assert_eq!(replay.description, old_text);
    assert_eq!(replay.version, "1");
    let mut tx = h.context(a.user.id, w).await;
    let receipt: Value =
        sqlx::query_scalar("SELECT response_body FROM idempotency_records WHERE key_hash=$1")
            .bind(digest(key.as_bytes()))
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    assert_eq!(
        receipt, old_receipt,
        "Upgrade and replay must not rewrite historical business outcomes"
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM documents")
            .fetch_one(&mut *tx)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT next_issue_number FROM teams WHERE id=$1")
            .bind(t)
            .fetch_one(&mut *tx)
            .await
            .unwrap(),
        2
    );
    tx.commit().await.unwrap();
    let issue = fresh(h, &a, w, id).await;
    assert!(
        h.app
            .update_issue(
                &a,
                w,
                id,
                &issue.etag(),
                json!({"title":"Unrelated metadata remains editable"})
            )
            .await
            .is_ok()
    );
    assert!(
        matches!(
            h.app
                .update_document(&a, w, doc.id, &doc.etag, body(&old_text))
                .await,
            Err(Error::Invalid(_))
        ),
        "Grandfathered data does not bypass bounds on a new write"
    );
    assert!(
        h.app
            .update_document(&a, w, doc.id, &doc.etag, body("Explicitly shortened"))
            .await
            .is_ok()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_postgres_document_retention_is_bounded_audited_and_restricted() {
    let h = Harness::new().await;
    let copy = h.clone();
    let run = tokio::spawn(async move { retention(&copy).await }).await;
    h.cleanup().await;
    if let Err(error) = run {
        std::panic::resume_unwind(error.into_panic());
    }
}
async fn retention(h: &Harness) {
    let (a, w, t) = setup(h).await;
    let issue = issue(
        h,
        &a,
        w,
        t,
        json!({"document":body("Initial retained body")}),
    )
    .await;
    let first = h.app.issue_document(&a, w, issue.id).await.unwrap();
    let second = h
        .app
        .update_document(&a, w, first.id, &first.etag, body("Second retained body"))
        .await
        .unwrap();
    let current = h
        .app
        .update_document(&a, w, first.id, &second.etag, body("Current retained body"))
        .await
        .unwrap();
    let mut tx = h.context(a.user.id, w).await;
    sqlx::query("ALTER TABLE document_revisions DISABLE TRIGGER document_revisions_immutable")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("UPDATE document_revisions SET created_at=clock_timestamp()-interval '91 days' WHERE document_id=$1").bind(first.id).execute(&mut *tx).await.unwrap();
    sqlx::query("ALTER TABLE document_revisions ENABLE TRIGGER document_revisions_immutable")
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(
        h.app.get_document(&a, w, first.id).await.unwrap().body,
        current.body,
        "Current content is not history and remains readable"
    );
    assert!(
        h.app
            .document_revisions(&a, w, first.id, DiscussionQuery::default())
            .await
            .unwrap()
            .data
            .is_empty()
    );
    assert!(matches!(
        h.app.document_revision(&a, w, first.id, 1).await,
        Err(Error::NotFound)
    ));
    let worker = Worker::connect(&h.worker_url().await, WorkerOptions::default())
        .await
        .unwrap();
    assert!(
        sqlx::query("SELECT body FROM documents")
            .execute(&worker.pool)
            .await
            .is_err()
    );
    assert!(
        sqlx::query("SELECT body FROM document_revisions")
            .execute(&worker.pool)
            .await
            .is_err()
    );
    assert!(
        sqlx::query("SELECT worker_prune_document_revisions($1,1)")
            .bind(w)
            .execute(&h.app.pool)
            .await
            .is_err(),
        "Runtime cannot invoke a maintenance capability"
    );
    assert!(
        sqlx::query("SELECT * FROM document_retention_operations")
            .execute(&h.app.pool)
            .await
            .is_err()
    );
    for table in ["documents", "document_revisions"] {
        let mut tx = store::begin(&h.app.pool).await.unwrap();
        store::context(&mut tx, a.user.id, Some(w)).await.unwrap();
        let statement = if table == "documents" {
            "DELETE FROM documents WHERE id=$1"
        } else {
            "DELETE FROM document_revisions WHERE document_id=$1"
        };
        assert!(
            sqlx::query(statement)
                .bind(first.id)
                .execute(&mut *tx)
                .await
                .is_err(),
            "Runtime hard-deletion denied for {table}"
        );
        tx.rollback().await.unwrap();
    }
    let before = state_snapshot(h, &a, w).await;
    fault(h, "document_retention_operations", "INSERT").await;
    let result = worker.prune_document_revisions(w, 1).await;
    clear(h, "document_retention_operations").await;
    assert!(
        result.is_err(),
        "Retention without its mandatory audit must fail"
    );
    assert_eq!(state_snapshot(h, &a, w).await, before);
    assert_eq!(worker.prune_document_revisions(w, 1).await.unwrap(), 1);
    assert_eq!(worker.prune_document_revisions(w, 1000).await.unwrap(), 1);
    assert_eq!(worker.prune_document_revisions(w, 1000).await.unwrap(), 0);
    let mut tx = h.context(a.user.id, w).await;
    let remaining: Vec<i64> = sqlx::query_scalar(
        "SELECT revision FROM document_revisions WHERE document_id=$1 ORDER BY revision",
    )
    .bind(first.id)
    .fetch_all(&mut *tx)
    .await
    .unwrap();
    assert_eq!(remaining, vec![3], "Only superseded snapshots are pruned");
    assert_eq!(sqlx::query_scalar::<_,i64>("SELECT sum(deleted_count)::bigint FROM document_retention_operations WHERE workspace_id=$1").bind(w).fetch_one(&mut *tx).await.unwrap(),2);
    tx.commit().await.unwrap();
    assert_eq!(
        h.app.get_document(&a, w, first.id).await.unwrap().body,
        current.body
    );
    worker.pool.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_postgres_competing_document_saves_preserve_winner_and_revision() {
    let h = Harness::with_connections(8).await;
    let copy = h.clone();
    let run = tokio::spawn(async move { competing_saves(&copy).await }).await;
    h.cleanup().await;
    if let Err(error) = run {
        std::panic::resume_unwind(error.into_panic());
    }
}
async fn competing_saves(h: &Harness) {
    let (a, w, t) = setup(h).await;
    let issue = issue(h, &a, w, t, json!({"document":body("Before both editors")})).await;
    let document = h.app.issue_document(&a, w, issue.id).await.unwrap();
    let (gate_tx, gate_pid) = gate(h, &a, w, t, Some(issue.id)).await;
    let app = h.app.clone();
    let actor = a.clone();
    let first = document.clone();
    let winner = tokio::spawn(async move {
        app.update_document(
            &actor,
            w,
            first.id,
            &first.etag,
            body("First editor committed"),
        )
        .await
    });
    let first_pid = blocked(h, gate_pid).await;
    let app = h.app.clone();
    let actor = a.clone();
    let second = document.clone();
    let stale = tokio::spawn(async move {
        app.update_document(
            &actor,
            w,
            second.id,
            &second.etag,
            body("Second editor's unsaved draft"),
        )
        .await
    });
    let second_pid = blocked(h, first_pid).await;
    assert_ne!(
        first_pid, second_pid,
        "Both editors must be observed waiting in PostgreSQL"
    );
    gate_tx.commit().await.unwrap();
    let saved = tokio::time::timeout(std::time::Duration::from_secs(5), winner)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    match tokio::time::timeout(std::time::Duration::from_secs(5), stale)
        .await
        .unwrap()
        .unwrap()
        .unwrap_err()
    {
        Error::VersionConflict(current) => {
            assert_eq!(current, serde_json::to_value(&saved).unwrap())
        }
        other => panic!("Expected the second editor's current-document conflict, got {other:?}"),
    }
    assert_eq!(saved.version, "2");
    assert_eq!(saved.plain_text, "First editor committed");
    assert_eq!(
        h.app
            .document_revision(&a, w, document.id, 1)
            .await
            .unwrap()
            .plain_text,
        "Before both editors"
    );
    let history = h
        .app
        .document_revisions(&a, w, document.id, DiscussionQuery::default())
        .await
        .unwrap();
    assert_eq!(
        history
            .data
            .iter()
            .map(|r| r.revision.as_str())
            .collect::<Vec<_>>(),
        vec!["2", "1"]
    );
    assert_eq!(
        h.app.get_document(&a, w, document.id).await.unwrap().body,
        saved.body
    );
    assert_eq!(fresh(h, &a, w, issue.id).await.version, issue.version);
}
