//! Metadata, document ownership, quota and fenced cleanup against real PostgreSQL.
mod graph_support;
#[allow(dead_code)]
mod support;
use graph_support::*;
use opentrack_app::{
    ScanOutcome, UploadFailure,
    worker::{Worker, WorkerOptions},
};
use opentrack_domain::*;
use opentrack_store as store;
use serde_json::{Value, json};
use support::Harness;
use uuid::Uuid;

fn reservation(team: Uuid, size: i64) -> CreateAttachment {
    req(
        json!({"original_name":"support.txt","size_bytes":size,"sha256":"a".repeat(64),"team_id":team}),
    )
}
fn body(ids: &[Uuid]) -> DocumentWrite {
    req(
        json!({"schema_version":1,"body":{"type":"doc","content":ids.iter().map(|id|json!({"type":"attachment","attrs":{"id":id}})).collect::<Vec<_>>()}}),
    )
}
async fn reserve(h: &Harness, a: &Actor, w: Uuid, t: Uuid) -> Attachment {
    h.app
        .reserve_attachment(
            a,
            w,
            &Uuid::new_v4().to_string(),
            reservation(t, 16),
            &AttachmentLimits::default(),
        )
        .await
        .unwrap()
}
async fn uploaded(h: &Harness, a: &Actor, w: Uuid, item: &Attachment) -> Attachment {
    let permit = h
        .app
        .begin_upload(a, w, item.id, &item.etag, &AttachmentLimits::default())
        .await
        .unwrap();
    h.app
        .quarantine_attachment(a, &permit, permit.size_bytes, &permit.sha256, "text/plain")
        .await
        .unwrap();
    h.app
        .finalize_attachment(a, &permit, ScanOutcome::NotScanned)
        .await
        .unwrap()
}
async fn charged(h: &Harness, a: &Actor, w: Uuid) -> i64 {
    let mut tx = h.context(a.user.id, w).await;
    let count=sqlx::query_scalar("SELECT coalesce((SELECT charged_bytes FROM workspace_storage_usage WHERE workspace_id=$1),0)").bind(w).fetch_one(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    count
}
async fn all(h: &Harness, a: &Actor, w: Uuid) -> Value {
    let tracking = snapshot(h, a, w).await;
    let mut tx = h.context(a.user.id, w).await;
    let files:Value=sqlx::query_scalar("SELECT jsonb_build_object('attachments',(SELECT coalesce(jsonb_agg(to_jsonb(a) ORDER BY id),'[]') FROM attachments a),'quota',(SELECT coalesce(jsonb_agg(to_jsonb(q)),'[]') FROM workspace_storage_usage q),'links',(SELECT coalesce(jsonb_agg(to_jsonb(l) ORDER BY to_jsonb(l)::text),'[]') FROM document_attachments l),'references',(SELECT coalesce(jsonb_agg(to_jsonb(r) ORDER BY to_jsonb(r)::text),'[]') FROM document_attachment_revisions r),'docs',(SELECT coalesce(jsonb_agg(to_jsonb(d) ORDER BY id),'[]') FROM documents d),'revisions',(SELECT coalesce(jsonb_agg(to_jsonb(r) ORDER BY to_jsonb(r)::text),'[]') FROM document_revisions r),'operations',(SELECT coalesce(jsonb_agg(to_jsonb(o) ORDER BY id),'[]') FROM attachment_storage_operations o))").fetch_one(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    json!({"tracking":tracking,"files":files})
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_postgres_attachment_lifecycle_and_document_ownership() {
    let h = Harness::with_connections(8).await;
    let copy = h.clone();
    let run = tokio::spawn(async move { lifecycle(&copy).await }).await;
    h.cleanup().await;
    if let Err(e) = run {
        std::panic::resume_unwind(e.into_panic());
    }
}
async fn lifecycle(h: &Harness) {
    let (a, w, t) = setup(h).await;
    let member = person(h, &a, w, "member").await;
    let viewer = person(h, &a, w, "viewer").await;
    let key = "reserved-once-key";
    let request = reservation(t, 16);
    let pending = h
        .app
        .reserve_attachment(&a, w, key, request.clone(), &AttachmentLimits::default())
        .await
        .unwrap();
    assert_eq!(pending.state, AttachmentState::Pending);
    assert!(pending.document_id.is_none() && pending.expires_at.is_some());
    let serialized = serde_json::to_string(&pending).unwrap();
    for private in ["storage_key", "lease_token", "uploader_id", "generation"] {
        assert!(!serialized.contains(private));
    }
    assert_eq!(
        h.app
            .reserve_attachment(&a, w, key, request.clone(), &AttachmentLimits::default())
            .await
            .unwrap()
            .id,
        pending.id
    );
    assert_eq!(charged(h, &a, w).await, 16);
    let mut changed = request.clone();
    changed.size_bytes = 17;
    assert!(matches!(
        h.app
            .reserve_attachment(&a, w, key, changed, &AttachmentLimits::default())
            .await,
        Err(Error::Conflict(_))
    ));
    assert!(matches!(
        h.app.attachment(&member, w, pending.id).await,
        Err(Error::NotFound)
    ));
    assert!(
        h.app
            .attachments(&member, w, req(json!({"team_id":t})))
            .await
            .unwrap()
            .data
            .is_empty()
    );
    assert!(matches!(
        h.app
            .reserve_attachment(
                &viewer,
                w,
                "viewer-reservation",
                request,
                &AttachmentLimits::default()
            )
            .await,
        Err(Error::Forbidden)
    ));
    assert!(matches!(
        h.app
            .begin_upload(&a, w, pending.id, "stale", &AttachmentLimits::default())
            .await,
        Err(Error::VersionConflict(_))
    ));
    let first = h
        .app
        .begin_upload(
            &a,
            w,
            pending.id,
            &pending.etag,
            &AttachmentLimits::default(),
        )
        .await
        .unwrap();
    assert!(matches!(
        h.app
            .quarantine_attachment(&a, &first, 17, &first.sha256, "text/plain")
            .await,
        Err(Error::Invalid(_))
    ));
    let reset = h
        .app
        .fail_attachment_upload(&a, &first, UploadFailure::StorageUnavailable)
        .await
        .unwrap();
    assert_eq!(reset.state, AttachmentState::Pending);
    assert!(matches!(
        h.app
            .quarantine_attachment(&a, &first, 16, &first.sha256, "text/plain")
            .await,
        Err(Error::Conflict(_))
    ));
    let limits = AttachmentLimits {
        antivirus_required: true,
        ..AttachmentLimits::default()
    };
    let permit = h
        .app
        .begin_upload(&a, w, pending.id, &reset.etag, &limits)
        .await
        .unwrap();
    assert!(permit.generation > first.generation);
    h.app
        .quarantine_attachment(&a, &permit, 16, &permit.sha256, "text/plain")
        .await
        .unwrap();
    let quarantined = h
        .app
        .finalize_attachment(&a, &permit, ScanOutcome::NotScanned)
        .await
        .unwrap();
    assert_eq!(quarantined.state, AttachmentState::Quarantined);
    assert_eq!(quarantined.scan_status, AttachmentScanStatus::Unavailable);
    assert!(matches!(
        h.app.attachment_download(&a, w, pending.id).await,
        Err(Error::NotFound)
    ));
    let finish = h
        .app
        .begin_finalize(&a, w, pending.id, &limits)
        .await
        .unwrap();
    let ready = h
        .app
        .finalize_attachment(&a, &finish, ScanOutcome::Clean)
        .await
        .unwrap();
    assert_eq!(ready.state, AttachmentState::Ready);
    let source = issue(h, &a, w, t, json!({"document":body(&[ready.id])})).await;
    assert!(source.description.is_empty());
    let doc = h.app.issue_document(&a, w, source.id).await.unwrap();
    assert_eq!(doc.body, body(&[ready.id]).body);
    assert_eq!(
        h.app
            .attachment(&member, w, ready.id)
            .await
            .unwrap()
            .document_id,
        Some(doc.id)
    );
    assert!(
        h.app
            .attachment(&member, w, ready.id)
            .await
            .unwrap()
            .expires_at
            .is_none()
    );
    assert_eq!(
        h.app
            .attachment_download(&member, w, ready.id)
            .await
            .unwrap()
            .size_bytes,
        16
    );
    let before = all(h, &a, w).await;
    assert!(matches!(h.app.create_issue(&a,w,"duplicate-document",req(json!({"team_id":t,"title":"Forbidden shared attachment","document":body(&[ready.id])}))).await,Err(Error::Invalid(_))));
    assert_eq!(all(h, &a, w).await, before);
    let private = team(h, &a, w, true).await;
    move_to(h, &a, w, source.id, private).await;
    assert!(matches!(
        h.app.attachment_download(&member, w, ready.id).await,
        Err(Error::NotFound)
    ));
    // Existing links travel with their current issue; upload-team equality only
    // constrains first binding, never invalidates an authorized move.
    let moved = h.app.get_document(&a, w, doc.id).await.unwrap();
    h.app
        .update_document(&a, w, doc.id, &moved.etag, body(&[ready.id]))
        .await
        .unwrap();
    let other = reserve(h, &a, w, private).await;
    let other = uploaded(h, &a, w, &other).await;
    let comment = h
        .app
        .create_comment(&a, w, source.id, "attachment-comment", body(&[other.id]))
        .await
        .unwrap();
    assert!(comment.document.as_ref().unwrap().plain_text.is_empty());
    let listed = h
        .app
        .attachments(
            &a,
            w,
            req(json!({"document_id":comment.document.as_ref().unwrap().id})),
        )
        .await
        .unwrap();
    assert_eq!(listed.data.len(), 1);
    // A document reservation is uploader-only until the body actually links it.
    let mut existing = reservation(private, 16);
    existing.team_id = None;
    existing.document_id = Some(doc.id);
    let reserved = h
        .app
        .reserve_attachment(&a, w, "existing-doc-file", existing, &limits)
        .await
        .unwrap();
    assert!(reserved.document_id.is_none());
    grant(h, &a, w, private, member.user.id).await;
    assert!(matches!(
        h.app.attachment(&member, w, reserved.id).await,
        Err(Error::NotFound)
    ));
    let mut invalid = body(&[ready.id]);
    invalid.body["content"][0]["attrs"]["url"] = json!("file:///secret");
    assert!(invalid.validate().is_err());
    assert!(body(&[ready.id, ready.id]).validate().is_err());
    // Historical private bytes do not become available through a public issue
    // when their reference was removed before the move.
    let outsider = person(h, &a, w, "member").await;
    let private_file = reserve(h, &a, w, private).await;
    let private_file = uploaded(h, &a, w, &private_file).await;
    let private_issue = issue(
        h,
        &a,
        w,
        private,
        json!({"document":body(&[private_file.id])}),
    )
    .await;
    let private_doc = h.app.issue_document(&a, w, private_issue.id).await.unwrap();
    h.app
        .update_document(
            &a,
            w,
            private_doc.id,
            &private_doc.etag,
            DocumentWrite::from_plain_text("Public description"),
        )
        .await
        .unwrap();
    move_to(h, &a, w, private_issue.id, t).await;
    assert!(matches!(
        h.app
            .attachment_download(&outsider, w, private_file.id)
            .await,
        Err(Error::NotFound)
    ));
    let visible = h
        .app
        .get_document(&outsider, w, private_doc.id)
        .await
        .unwrap();
    assert!(matches!(
        h.app
            .update_document(
                &outsider,
                w,
                private_doc.id,
                &visible.etag,
                body(&[private_file.id])
            )
            .await,
        Err(Error::Invalid(_))
    ));
    let owner_doc = h.app.get_document(&a, w, private_doc.id).await.unwrap();
    h.app
        .update_document(
            &a,
            w,
            private_doc.id,
            &owner_doc.etag,
            body(&[private_file.id]),
        )
        .await
        .unwrap();
    assert!(
        h.app
            .attachment_download(&outsider, w, private_file.id)
            .await
            .is_ok()
    );
    // Linked private comments keep their historical audience after an issue move.
    move_to(h, &a, w, source.id, t).await;
    assert!(matches!(
        h.app.attachment_download(&outsider, w, other.id).await,
        Err(Error::NotFound)
    ));
    let marker_file = reserve(h, &a, w, t).await;
    let marker_file = uploaded(h, &a, w, &marker_file).await;
    let marker_issue = issue(h, &a, w, t, json!({"document":body(&[marker_file.id])})).await;
    let linked = h.app.attachment(&a, w, marker_file.id).await.unwrap();
    h.app
        .delete_attachment(&a, w, linked.id, &linked.etag, CommentDelete::default())
        .await
        .unwrap();
    let deleted = h.app.attachment(&a, w, linked.id).await.unwrap();
    assert_eq!(deleted.state, AttachmentState::Deleted);
    assert!(!deleted.can_delete);
    assert!(matches!(
        h.app.attachment_download(&a, w, linked.id).await,
        Err(Error::NotFound)
    ));
    let document = h.app.issue_document(&a, w, marker_issue.id).await.unwrap();
    let mut preserved = body(&[linked.id]);
    preserved.body["content"].as_array_mut().unwrap().push(json!({"type":"paragraph","content":[{"type":"text","text":"Text beside an unavailable attachment"}]}));
    let changed = h
        .app
        .update_document(&a, w, document.id, &document.etag, preserved)
        .await
        .unwrap();
    let removed = h
        .app
        .update_document(
            &a,
            w,
            document.id,
            &changed.etag,
            DocumentWrite::from_plain_text("Removed marker"),
        )
        .await
        .unwrap();
    assert!(matches!(
        h.app
            .update_document(&a, w, document.id, &removed.etag, body(&[linked.id]))
            .await,
        Err(Error::Invalid(_))
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_postgres_attachment_atomic_quota_and_expired_fences() {
    let h = Harness::with_connections(8).await;
    let copy = h.clone();
    let run = tokio::spawn(async move { quota(&copy).await }).await;
    h.cleanup().await;
    if let Err(e) = run {
        std::panic::resume_unwind(e.into_panic());
    }
}
async fn quota(h: &Harness) {
    let (a, w, t) = setup(h).await;
    let limits = AttachmentLimits {
        max_upload_bytes: 10,
        workspace_quota_bytes: 10,
        ..AttachmentLimits::default()
    };
    let mut lock = h.context(a.user.id, w).await;
    sqlx::query("INSERT INTO workspace_storage_usage(workspace_id) VALUES($1)")
        .bind(w)
        .execute(&mut *lock)
        .await
        .unwrap();
    lock.commit().await.unwrap();
    let mut lock = h.context(a.user.id, w).await;
    sqlx::query(
        "SELECT workspace_id FROM workspace_storage_usage WHERE workspace_id=$1 FOR UPDATE",
    )
    .bind(w)
    .execute(&mut *lock)
    .await
    .unwrap();
    let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *lock)
        .await
        .unwrap();
    let first_app = h.app.clone();
    let first_actor = a.clone();
    let first = tokio::spawn(async move {
        first_app
            .reserve_attachment(
                &first_actor,
                w,
                "quota-first-file",
                reservation(t, 6),
                &limits,
            )
            .await
    });
    let first_waiter = blocked(h, pid).await;
    let second_app = h.app.clone();
    let second_actor = a.clone();
    let second = tokio::spawn(async move {
        second_app
            .reserve_attachment(
                &second_actor,
                w,
                "quota-second-file",
                reservation(t, 6),
                &limits,
            )
            .await
    });
    tokio::time::timeout(std::time::Duration::from_secs(2),async{loop {
        let count:i64=sqlx::query_scalar("SELECT count(*) FROM pg_stat_activity WHERE $1=ANY(pg_blocking_pids(pid)) OR $2=ANY(pg_blocking_pids(pid))").bind(pid).bind(first_waiter).fetch_one(&h.owner).await.unwrap();
        if count>=2 {break;}tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }}).await.unwrap();
    lock.commit().await.unwrap();
    let results = [first.await.unwrap(), second.await.unwrap()];
    assert_eq!(results.iter().filter(|v| v.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|v| matches!(v, Err(Error::Conflict(_))))
            .count(),
        1
    );
    assert_eq!(charged(h, &a, w).await, 6);
    let item = results.into_iter().find_map(|v| v.ok()).unwrap();
    let p = h
        .app
        .begin_upload(&a, w, item.id, &item.etag, &limits)
        .await
        .unwrap();
    let mut lock = h.context(a.user.id, w).await;
    sqlx::query("UPDATE attachments SET lease_expires_at=clock_timestamp()+interval '200 milliseconds',version=version+1 WHERE workspace_id=$1 AND id=$2").bind(w).bind(item.id).execute(&mut *lock).await.unwrap();
    lock.commit().await.unwrap();
    let (lock, pid) = gate(h, &a, w, t, None).await;
    let copy = h.app.clone();
    let actor = a.clone();
    let id = item.id;
    let pending = tokio::spawn(async move {
        copy.quarantine_attachment(&actor, &p, p.size_bytes, &p.sha256, "text/plain")
            .await
    });
    blocked(h, pid).await;
    loop {
        let mut tx = h.context(a.user.id, w).await;
        let expired:bool=sqlx::query_scalar("SELECT lease_expires_at<=clock_timestamp() FROM attachments WHERE workspace_id=$1 AND id=$2").bind(w).bind(id).fetch_one(&mut *tx).await.unwrap();
        tx.commit().await.unwrap();
        if expired {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    lock.commit().await.unwrap();
    assert!(matches!(pending.await.unwrap(), Err(Error::Conflict(_))));
    let current = h.app.attachment(&a, w, id).await.unwrap();
    assert_eq!(current.state, AttachmentState::Pending);
    let restarted = h
        .app
        .begin_upload(&a, w, id, &current.etag, &limits)
        .await
        .unwrap();
    let rejected = h
        .app
        .fail_attachment_upload(&a, &restarted, UploadFailure::ChecksumMismatch)
        .await
        .unwrap();
    assert_eq!(rejected.state, AttachmentState::Rejected);
    assert_eq!(
        charged(h, &a, w).await,
        6,
        "Rejected files remain charged until physical deletion is acknowledged"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_postgres_attachment_required_effects_roll_back() {
    let h = Harness::with_connections(6).await;
    let copy = h.clone();
    let run = tokio::spawn(async move { atomicity(&copy).await }).await;
    h.cleanup().await;
    if let Err(e) = run {
        std::panic::resume_unwind(e.into_panic());
    }
}
async fn atomicity(h: &Harness) {
    let (a, w, t) = setup(h).await;
    for (table, event) in [
        ("attachments", "INSERT"),
        ("workspace_storage_usage", "UPDATE"),
        ("idempotency_records", "UPDATE"),
        ("audit_entries", "INSERT"),
        ("workspace_events", "INSERT"),
        ("workspace_stream_heads", "UPDATE"),
    ] {
        let before = all(h, &a, w).await;
        fault(h, table, event).await;
        assert!(
            h.app
                .reserve_attachment(
                    &a,
                    w,
                    "faulted-reservation",
                    reservation(t, 16),
                    &AttachmentLimits::default()
                )
                .await
                .is_err(),
            "Missing reservation {table} effect must fail"
        );
        clear(h, table).await;
        assert_eq!(
            all(h, &a, w).await,
            before,
            "Failed {table} must not retain quota, receipt or file"
        );
    }
    let file = reserve(h, &a, w, t).await;
    let file = uploaded(h, &a, w, &file).await;
    for (table, event) in [
        ("document_attachments", "INSERT"),
        ("document_attachment_revisions", "INSERT"),
        ("attachments", "UPDATE"),
        ("document_revisions", "INSERT"),
        ("audit_entries", "INSERT"),
        ("workspace_events", "INSERT"),
    ] {
        let before = all(h, &a, w).await;
        fault(h, table, event).await;
        assert!(
            h.app
                .create_issue(
                    &a,
                    w,
                    "linked-create-fault",
                    req(
                        json!({"team_id":t,"title":"Atomic attachment","document":body(&[file.id])})
                    )
                )
                .await
                .is_err(),
            "Missing link {table} effect must fail"
        );
        clear(h, table).await;
        assert_eq!(
            all(h, &a, w).await,
            before,
            "Failed link {table} must roll back every effect"
        );
    }
    let issue = issue(h, &a, w, t, json!({"document":body(&[file.id])})).await;
    let doc = h.app.issue_document(&a, w, issue.id).await.unwrap();
    let fresh = reserve(h, &a, w, t).await;
    let fresh = uploaded(h, &a, w, &fresh).await;
    let before = all(h, &a, w).await;
    fault(h, "document_attachment_revisions", "INSERT").await;
    assert!(
        h.app
            .update_document(&a, w, doc.id, &doc.etag, body(&[file.id, fresh.id]))
            .await
            .is_err()
    );
    clear(h, "document_attachment_revisions").await;
    assert_eq!(all(h, &a, w).await, before);
}

async fn expire(h: &Harness, a: &Actor, w: Uuid, id: Uuid) {
    let mut tx = h.context(a.user.id, w).await;
    sqlx::query("ALTER TABLE attachments DISABLE TRIGGER attachments_identity")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("UPDATE attachments SET expires_at=clock_timestamp()-interval '1 day' WHERE workspace_id=$1 AND id=$2").bind(w).bind(id).execute(&mut *tx).await.unwrap();
    sqlx::query("ALTER TABLE attachments ENABLE TRIGGER attachments_identity")
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_postgres_attachment_retention_gc_and_restricted_roles() {
    let h = Harness::with_connections(6).await;
    let copy = h.clone();
    let run = tokio::spawn(async move { cleanup(&copy).await }).await;
    h.cleanup().await;
    if let Err(e) = run {
        std::panic::resume_unwind(e.into_panic());
    }
}
async fn cleanup(h: &Harness) {
    let (a, w, t) = setup(h).await;
    let worker = Worker::connect(&h.worker_url().await, WorkerOptions::default())
        .await
        .unwrap();
    let file = reserve(h, &a, w, t).await;
    let file = uploaded(h, &a, w, &file).await;
    let issue = issue(h, &a, w, t, json!({"document":body(&[file.id])})).await;
    let doc = h.app.issue_document(&a, w, issue.id).await.unwrap();
    expire(h, &a, w, file.id).await;
    assert!(
        worker.claim_attachment_gc(w, 25).await.unwrap().is_empty(),
        "Current references survive TTL"
    );
    h.app
        .update_document(
            &a,
            w,
            doc.id,
            &doc.etag,
            DocumentWrite::from_plain_text("Removed attachment"),
        )
        .await
        .unwrap();
    assert!(
        worker.claim_attachment_gc(w, 25).await.unwrap().is_empty(),
        "Retained revisions preserve their blob"
    );
    let mut tx = h.context(a.user.id, w).await;
    sqlx::query("ALTER TABLE document_revisions DISABLE TRIGGER document_revisions_immutable")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("UPDATE document_revisions SET created_at=clock_timestamp()-interval '91 days' WHERE workspace_id=$1 AND document_id=$2 AND revision=1").bind(w).bind(doc.id).execute(&mut *tx).await.unwrap();
    sqlx::query("ALTER TABLE document_revisions ENABLE TRIGGER document_revisions_immutable")
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(worker.prune_document_revisions(w, 100).await.unwrap(), 1);
    let claims = worker.claim_attachment_gc(w, 25).await.unwrap();
    assert_eq!(claims.len(), 1);
    let claim = &claims[0];
    assert_eq!(claim.id, file.id);
    assert_eq!(charged(h, &a, w).await, 16);
    assert!(worker.claim_attachment_gc(w, 25).await.unwrap().is_empty());
    for table in [
        "workspace_storage_usage",
        "attachments",
        "attachment_storage_operations",
    ] {
        let before = all(h, &a, w).await;
        fault(
            h,
            table,
            if table == "attachment_storage_operations" {
                "INSERT"
            } else {
                "UPDATE"
            },
        )
        .await;
        assert!(worker.acknowledge_attachment_gc(claim).await.is_err());
        clear(h, table).await;
        assert_eq!(all(h, &a, w).await, before);
    }
    assert!(worker.acknowledge_attachment_gc(claim).await.unwrap());
    assert_eq!(charged(h, &a, w).await, 0);
    assert!(!worker.acknowledge_attachment_gc(claim).await.unwrap());
    let abandoned = reserve(h, &a, w, t).await;
    let permit = h
        .app
        .begin_upload(
            &a,
            w,
            abandoned.id,
            &abandoned.etag,
            &AttachmentLimits::default(),
        )
        .await
        .unwrap();
    expire(h, &a, w, abandoned.id).await;
    let pending_claims = worker.claim_attachment_gc(w, 25).await.unwrap();
    assert_eq!(pending_claims.len(), 1);
    assert!(
        h.app
            .quarantine_attachment(&a, &permit, 16, &permit.sha256, "text/plain")
            .await
            .is_err()
    );
    assert_eq!(charged(h, &a, w).await, 16);
    assert!(
        worker
            .acknowledge_attachment_gc(&pending_claims[0])
            .await
            .unwrap()
    );
    assert!(
        sqlx::query("SELECT * FROM worker_claim_attachment_gc($1,1)")
            .bind(w)
            .execute(&h.app.pool)
            .await
            .is_err()
    );
    assert!(
        sqlx::query("SELECT * FROM attachments")
            .execute(&worker.pool)
            .await
            .is_err()
    );
    assert!(
        sqlx::query("SELECT * FROM attachment_storage_operations")
            .execute(&h.app.pool)
            .await
            .is_err()
    );
    worker.pool.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_postgres_attachment_0016_upgrade_preserves_documents() {
    let h = Harness::with_version(5, 16).await;
    let copy = h.clone();
    let run = tokio::spawn(async move {
        let (a, w, t) = setup(&copy).await;
        let issue = issue(&copy, &a, w, t, json!({"description":"Preserved document"})).await;
        let before = serde_json::to_value(&issue).unwrap();
        let document = copy.app.issue_document(&a, w, issue.id).await.unwrap();
        store::MIGRATOR.run(&copy.owner).await.unwrap();
        assert_eq!(
            serde_json::to_value(copy.app.get_issue(&a, w, issue.id).await.unwrap()).unwrap(),
            before
        );
        assert_eq!(
            serde_json::to_value(copy.app.get_document(&a, w, document.id).await.unwrap()).unwrap(),
            serde_json::to_value(document).unwrap()
        );
        let item = reserve(&copy, &a, w, t).await;
        assert_eq!(charged(&copy, &a, w).await, 16);
        assert_eq!(item.state, AttachmentState::Pending);
        let worker = Worker::connect(&copy.worker_url().await, WorkerOptions::default())
            .await
            .unwrap();
        worker.pool.close().await;
    })
    .await;
    h.cleanup().await;
    if let Err(e) = run {
        std::panic::resume_unwind(e.into_panic());
    }
}
