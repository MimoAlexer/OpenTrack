//! Real PostgreSQL current-authority references and atomic insertion validation.
mod graph_support;
#[allow(dead_code)]
mod support;
use graph_support::*;
use opentrack_domain::*;
use serde_json::{Value, json};
use support::Harness;
use uuid::Uuid;

fn body(text: &str, ids: &[Uuid]) -> DocumentWrite {
    let mut content = Vec::new();
    if !text.is_empty() {
        content.push(json!({"type":"text","text":text}));
    }
    content.extend(
        ids.iter()
            .map(|id| json!({"type":"issueReference","attrs":{"id":id}})),
    );
    req(
        json!({"schema_version":1,"body":{"type":"doc","content":[{"type":"paragraph","content":content}]}}),
    )
}
fn reference(document: &Document, id: Uuid) -> Value {
    serde_json::to_value(
        document
            .issue_references
            .iter()
            .find(|v| v.id == id)
            .unwrap(),
    )
    .unwrap()
}
async fn candidates(
    h: &Harness,
    a: &Actor,
    w: Uuid,
    query: Value,
) -> Page<IssueReferenceCandidate> {
    h.app
        .issue_reference_candidates(a, w, req(query))
        .await
        .unwrap()
}
async fn stored(h: &Harness, a: &Actor, w: Uuid) -> Value {
    let tracking = snapshot(h, a, w).await;
    let mut tx = h.context(a.user.id, w).await;
    let docs: Value = sqlx::query_scalar("SELECT jsonb_build_object('documents',(SELECT coalesce(jsonb_agg(to_jsonb(d) ORDER BY id),'[]') FROM documents d),'comments',(SELECT coalesce(jsonb_agg(to_jsonb(c) ORDER BY id),'[]') FROM comments c),'revisions',(SELECT coalesce(jsonb_agg(to_jsonb(r) ORDER BY document_id,revision),'[]') FROM document_revisions r))")
        .fetch_one(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    json!({"tracking":tracking,"documents":docs})
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_postgres_issue_reference_visibility_rename_move_delete_and_revisions() {
    let h = Harness::new().await;
    let copy = h.clone();
    let run = tokio::spawn(async move { visibility(&copy).await }).await;
    h.cleanup().await;
    if let Err(error) = run {
        std::panic::resume_unwind(error.into_panic());
    }
}
async fn visibility(h: &Harness) {
    let (a, w, public) = setup(h).await;
    let private = team(h, &a, w, true).await;
    let reader = person(h, &a, w, "member").await;
    let hidden = issue(h, &a, w, private, json!({"title":"Private target title"})).await;
    let target = issue(h, &a, w, public, json!({"title":"Public target title"})).await;
    let source = issue(
        h,
        &a,
        w,
        public,
        json!({"document":body("See ", &[target.id,hidden.id])}),
    )
    .await;
    let doc = h.app.issue_document(&a, w, source.id).await.unwrap();
    assert_eq!(reference(&doc, hidden.id)["title"], hidden.title);
    let redacted = h.app.issue_document(&reader, w, source.id).await.unwrap();
    assert_eq!(
        reference(&redacted, hidden.id),
        json!({"id":hidden.id,"available":false,"key":null,"title":null})
    );
    assert_eq!(redacted.body, doc.body);
    assert!(!redacted.plain_text.contains("target title"));
    assert!(!source.description.contains("target title"));
    assert_ne!(redacted.etag, doc.etag);
    let found = candidates(h, &reader, w, json!({"document_id":doc.id,"q":"target"})).await;
    assert_eq!(found.data.len(), 1);
    assert_eq!(found.data[0].id, target.id);
    let from_private = h
        .app
        .issue_reference_candidates(&reader, w, req(json!({"team_id":private})))
        .await;
    assert!(matches!(from_private, Err(Error::NotFound)));
    // Retaining an inaccessible ID does not require its title or a new grant.
    let saved = h
        .app
        .update_document(
            &reader,
            w,
            doc.id,
            &redacted.etag,
            body("Still see ", &[target.id, hidden.id]),
        )
        .await
        .unwrap();
    assert!(!reference(&saved, hidden.id)["available"].as_bool().unwrap());
    let comment_body = body("", &[target.id]);
    let comment = h
        .app
        .create_comment(
            &reader,
            w,
            source.id,
            "reference-comment",
            comment_body.clone(),
        )
        .await
        .unwrap();
    let mut tx = h.context(a.user.id, w).await;
    let receipt: Value = sqlx::query_scalar("SELECT response_body->'document' FROM idempotency_records WHERE workspace_id=$1 AND response_body->>'id'=$2")
        .bind(w).bind(comment.id.to_string()).fetch_one(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    assert!(
        receipt.get("issue_references").is_none(),
        "Idempotent receipts retain IDs without private target labels"
    );
    assert!(!receipt.to_string().contains(&target.title));
    assert!(!receipt.to_string().contains(&target.key));
    let updated_target = h
        .app
        .update_issue(
            &a,
            w,
            target.id,
            &target.etag(),
            json!({"title":"Renamed target"}),
        )
        .await
        .unwrap();
    let renamed = h.app.get_document(&reader, w, doc.id).await.unwrap();
    assert_eq!(reference(&renamed, target.id)["title"], "Renamed target");
    assert_ne!(renamed.etag, saved.etag);
    assert_eq!(renamed.version, saved.version);
    let renamed_comment = h.app.get_comment(&reader, w, comment.id).await.unwrap();
    assert_ne!(comment.etag, renamed_comment.etag);
    assert_eq!(comment.version, renamed_comment.version);
    let moved = move_to(h, &a, w, updated_target.id, private).await;
    let moved_doc = h.app.get_document(&reader, w, doc.id).await.unwrap();
    assert_eq!(
        reference(&moved_doc, target.id),
        json!({"id":target.id,"available":false,"key":null,"title":null})
    );
    let owner_doc = h.app.get_document(&a, w, doc.id).await.unwrap();
    assert_eq!(reference(&owner_doc, target.id)["key"], moved.key);
    let history = h
        .app
        .document_revision(&reader, w, doc.id, 1)
        .await
        .unwrap();
    assert!(
        history
            .issue_references
            .iter()
            .all(|r| !r.available && r.key.is_none() && r.title.is_none())
    );
    assert!(!history.plain_text.contains("target"));
    let replay = h
        .app
        .create_comment(&reader, w, source.id, "reference-comment", comment_body)
        .await
        .unwrap();
    assert!(
        !replay.document.unwrap().issue_references[0].available,
        "Retries return current-authority metadata"
    );
    let retained = h
        .app
        .update_comment(
            &reader,
            w,
            comment.id,
            &h.app
                .get_comment(&reader, w, comment.id)
                .await
                .unwrap()
                .etag,
            body("Retained ", &[target.id]),
        )
        .await
        .unwrap();
    assert!(!retained.document.unwrap().issue_references[0].available);
    // Membership grant and revocation change representations without source edits.
    grant(h, &a, w, private, reader.user.id).await;
    let granted = h.app.get_document(&reader, w, doc.id).await.unwrap();
    assert!(granted.issue_references.iter().all(|r| r.available));
    assert_ne!(granted.etag, moved_doc.etag);
    let membership = h
        .app
        .get_team_member(&a, w, private, reader.user.id)
        .await
        .unwrap();
    h.app
        .remove_team_member(&a, w, private, reader.user.id, &membership.etag(private))
        .await
        .unwrap();
    let revoked = h.app.get_document(&reader, w, doc.id).await.unwrap();
    assert_eq!(revoked.etag, moved_doc.etag);
    h.app
        .delete_issue(&a, w, moved.id, &moved.etag())
        .await
        .unwrap();
    let deleted = h.app.get_document(&a, w, doc.id).await.unwrap();
    assert!(
        !reference(&deleted, target.id)["available"]
            .as_bool()
            .unwrap()
    );
    let removed = h
        .app
        .update_document(&reader, w, doc.id, &revoked.etag, body("Removed ", &[]))
        .await
        .unwrap();
    assert!(matches!(
        h.app
            .update_document(
                &reader,
                w,
                doc.id,
                &removed.etag,
                body("Reintroduced ", &[hidden.id])
            )
            .await,
        Err(Error::Invalid(_))
    ));
    assert!(matches!(
        h.app
            .update_document(
                &a,
                w,
                doc.id,
                &h.app.get_document(&a, w, doc.id).await.unwrap().etag,
                body("Deleted ", &[target.id])
            )
            .await,
        Err(Error::Invalid(_))
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_postgres_issue_reference_scope_validation_and_atomicity() {
    let h = Harness::new().await;
    let copy = h.clone();
    let run = tokio::spawn(async move { validation(&copy).await }).await;
    h.cleanup().await;
    if let Err(error) = run {
        std::panic::resume_unwind(error.into_panic());
    }
}
async fn validation(h: &Harness) {
    let (a, w, t) = setup(h).await;
    let reader = person(h, &a, w, "member").await;
    let private = team(h, &a, w, true).await;
    let hidden = issue(h, &a, w, private, json!({})).await;
    let source = issue(h, &a, w, t, json!({})).await;
    let target = issue(h, &a, w, t, json!({"title":"Literal %_ search"})).await;
    let doc = h.app.issue_document(&reader, w, source.id).await.unwrap();
    let organization = h.app.organizations(&a).await.unwrap()[0].id;
    let other = h.app.create_workspace(&a,"reference-other-workspace",req(json!({"organization_id":organization,"name":"Other","team_name":"Other","team_key":"OTH"}))).await.unwrap();
    let other_team = h.app.bootstrap(&a, other.id).await.unwrap().teams[0].id;
    let foreign = issue(h, &a, other.id, other_team, json!({})).await;
    for id in [Uuid::new_v4(), hidden.id, foreign.id] {
        let before = stored(h, &a, w).await;
        assert!(matches!(
            h.app
                .update_document(&reader, w, doc.id, &doc.etag, body("", &[id]))
                .await,
            Err(Error::Invalid(_))
        ));
        assert!(matches!(
            h.app
                .create_comment(
                    &reader,
                    w,
                    source.id,
                    &Uuid::new_v4().to_string(),
                    body("", &[id])
                )
                .await,
            Err(Error::Invalid(_))
        ));
        assert!(matches!(
            h.app
                .create_issue(
                    &reader,
                    w,
                    &Uuid::new_v4().to_string(),
                    req(json!({"team_id":t,"title":"Rejected","document":body("",&[id])}))
                )
                .await,
            Err(Error::Invalid(_))
        ));
        assert_eq!(
            stored(h, &a, w).await,
            before,
            "Invalid references roll back documents, comments, receipts and events"
        );
    }
    let comment = h
        .app
        .create_comment(
            &reader,
            w,
            source.id,
            "reference-empty-comment",
            body("", &[target.id]),
        )
        .await
        .unwrap();
    let before = stored(h, &a, w).await;
    assert!(matches!(
        h.app
            .update_comment(
                &reader,
                w,
                comment.id,
                &comment.etag,
                body("", &[hidden.id])
            )
            .await,
        Err(Error::Invalid(_))
    ));
    assert_eq!(stored(h, &a, w).await, before);
    let literal = candidates(h, &reader, w, json!({"issue_id":source.id,"q":"%_"})).await;
    assert_eq!(literal.data.len(), 1);
    assert_eq!(literal.data[0].id, target.id);
    for query in [
        json!({}),
        json!({"team_id":t,"issue_id":source.id}),
        json!({"team_id":t,"limit":0}),
        json!({"team_id":t,"limit":21}),
        json!({"team_id":t,"q":"a".repeat(101)}),
        json!({"team_id":t,"cursor":"a".repeat(2049)}),
    ] {
        assert!(matches!(
            h.app
                .issue_reference_candidates(&reader, w, req(query))
                .await,
            Err(Error::Invalid(_))
        ));
    }
    let first = candidates(h, &reader, w, json!({"team_id":t,"limit":1})).await;
    assert!(first.has_more);
    let next = candidates(
        h,
        &reader,
        w,
        json!({"team_id":t,"limit":1,"cursor":first.next_cursor}),
    )
    .await;
    assert_ne!(first.data[0].id, next.data[0].id);
    assert!(matches!(
        h.app
            .issue_reference_candidates(
                &a,
                w,
                req(json!({"team_id":t,"limit":1,"cursor":first.next_cursor}))
            )
            .await,
        Err(Error::Invalid(_))
    ));
    let template = req(
        json!({"name":"No reference template","title":"Template","defaults":{"status_id":null,"priority":0,"label_ids":[]},"document":body("",&[target.id])}),
    );
    assert!(matches!(
        h.app
            .create_template(&a, w, t, "reference-template", template)
            .await,
        Err(Error::Invalid(_))
    ));
}

async fn head(h: &Harness, a: &Actor, w: Uuid) -> (opentrack_store::Tx, i32) {
    let mut tx = h.context(a.user.id, w).await;
    sqlx::query("SELECT workspace_id FROM workspace_stream_heads WHERE workspace_id=$1 FOR UPDATE")
        .bind(w)
        .execute(&mut *tx)
        .await
        .unwrap();
    let pid = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    (tx, pid)
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_postgres_reference_insertion_serializes_with_target_move_and_delete() {
    let h = Harness::with_connections(8).await;
    let copy = h.clone();
    let run = tokio::spawn(async move { races(&copy).await }).await;
    h.cleanup().await;
    if let Err(error) = run {
        std::panic::resume_unwind(error.into_panic());
    }
}
async fn races(h: &Harness) {
    let (a, w, t) = setup(h).await;
    let reader = person(h, &a, w, "member").await;
    let private = team(h, &a, w, true).await;
    for deleting in [false, true] {
        for target_first in [false, true] {
            let source = issue(h, &a, w, t, json!({})).await;
            let target_team = team(h, &a, w, false).await;
            let target = issue(h, &a, w, target_team, json!({})).await;
            let target_id = target.id;
            let doc = h.app.issue_document(&reader, w, source.id).await.unwrap();
            let preview = h
                .app
                .preview_issue_move(&a, w, target.id, private)
                .await
                .unwrap();
            let (holder, pid) = head(h, &a, w).await;
            let app = h.app.clone();
            let actor = a.clone();
            let transition = async move {
                if deleting {
                    app.delete_issue(&actor, w, target.id, &target.etag()).await
                } else {
                    app.move_issue(
                        &actor,
                        w,
                        target.id,
                        &Uuid::new_v4().to_string(),
                        &preview.issue_etag,
                        move_body(&preview),
                    )
                    .await
                    .map(|_| ())
                }
            };
            let app = h.app.clone();
            let actor = reader.clone();
            let doc_before = doc.clone();
            let write = async move {
                app.update_document(
                    &actor,
                    w,
                    doc_before.id,
                    &doc_before.etag,
                    body("", &[target_id]),
                )
                .await
            };
            let written = if target_first {
                let first = tokio::spawn(transition);
                let first_pid = blocked(h, pid).await;
                let second = tokio::spawn(write);
                blocked(h, first_pid).await;
                holder.commit().await.unwrap();
                first.await.unwrap().unwrap();
                second.await.unwrap()
            } else {
                let first = tokio::spawn(write);
                let first_pid = blocked(h, pid).await;
                let second = tokio::spawn(transition);
                blocked(h, first_pid).await;
                holder.commit().await.unwrap();
                let result = first.await.unwrap();
                second.await.unwrap().unwrap();
                result
            };
            if target_first {
                assert!(
                    matches!(written, Err(Error::Conflict(_))),
                    "Target mutation committed first: {written:?}"
                );
                assert!(
                    h.app
                        .get_document(&reader, w, doc.id)
                        .await
                        .unwrap()
                        .issue_references
                        .is_empty()
                );
            } else {
                assert!(written.unwrap().issue_references[0].available);
                assert!(
                    !h.app
                        .get_document(&reader, w, doc.id)
                        .await
                        .unwrap()
                        .issue_references[0]
                        .available
                );
            }
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_postgres_reference_source_move_and_reciprocal_team_lock_order() {
    let h = Harness::with_connections(8).await;
    let copy = h.clone();
    let run = tokio::spawn(async move { source_races(&copy).await }).await;
    h.cleanup().await;
    if let Err(error) = run {
        std::panic::resume_unwind(error.into_panic());
    }
}
async fn source_races(h: &Harness) {
    let (a, w, t) = setup(h).await;
    let destination = team(h, &a, w, false).await;
    let source = issue(h, &a, w, t, json!({})).await;
    let target = issue(h, &a, w, t, json!({})).await;
    let doc = h.app.issue_document(&a, w, source.id).await.unwrap();
    let preview = h
        .app
        .preview_issue_move(&a, w, source.id, destination)
        .await
        .unwrap();
    let (holder, pid) = head(h, &a, w).await;
    let app = h.app.clone();
    let actor = a.clone();
    let moved = tokio::spawn(async move {
        app.move_issue(
            &actor,
            w,
            source.id,
            "reference-source-move",
            &preview.issue_etag,
            move_body(&preview),
        )
        .await
    });
    let moving_pid = blocked(h, pid).await;
    let app = h.app.clone();
    let actor = a.clone();
    let before = doc.clone();
    let written = tokio::spawn(async move {
        app.update_document(&actor, w, before.id, &before.etag, body("", &[target.id]))
            .await
    });
    blocked(h, moving_pid).await;
    holder.commit().await.unwrap();
    moved.await.unwrap().unwrap();
    assert!(matches!(written.await.unwrap(), Err(Error::Conflict(_))));
    assert!(
        h.app
            .get_document(&a, w, doc.id)
            .await
            .unwrap()
            .issue_references
            .is_empty()
    );

    let left = issue(h, &a, w, t, json!({})).await;
    let right = issue(h, &a, w, destination, json!({})).await;
    let left_doc = h.app.issue_document(&a, w, left.id).await.unwrap();
    let right_doc = h.app.issue_document(&a, w, right.id).await.unwrap();
    let (holder, pid) = gate(h, &a, w, t.min(destination), None).await;
    let app = h.app.clone();
    let actor = a.clone();
    let first = tokio::spawn(async move {
        app.update_document(
            &actor,
            w,
            left_doc.id,
            &left_doc.etag,
            body("", &[right.id]),
        )
        .await
    });
    blocked(h, pid).await;
    let app = h.app.clone();
    let actor = a.clone();
    let second = tokio::spawn(async move {
        app.update_document(
            &actor,
            w,
            right_doc.id,
            &right_doc.etag,
            body("", &[left.id]),
        )
        .await
    });
    holder.commit().await.unwrap();
    assert!(first.await.unwrap().unwrap().issue_references[0].available);
    assert!(second.await.unwrap().unwrap().issue_references[0].available);
}
