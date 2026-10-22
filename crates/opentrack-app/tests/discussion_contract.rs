//! Real PostgreSQL comment permissions, current-authority replay and tombstones.
mod graph_support;
#[allow(dead_code)]
mod support;
use graph_support::*;
use opentrack_domain::*;
use serde_json::{Value, json};
use support::Harness;
use uuid::Uuid;

fn body(text: &str) -> DocumentWrite {
    req(
        json!({"schema_version":1,"body":{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":text}]}]}}),
    )
}
fn moderated(text: &str, reason: &str) -> DocumentWrite {
    let mut write = body(text);
    write.moderation_reason = Some(reason.into());
    write
}
fn deletion(reason: Option<&str>) -> CommentDelete {
    req(json!({"moderation_reason":reason}))
}
async fn state_snapshot(h: &Harness, a: &Actor, w: Uuid) -> Value {
    let tracking = snapshot(h, a, w).await;
    let mut tx = h.context(a.user.id, w).await;
    let discussion:Value=sqlx::query_scalar("SELECT jsonb_build_object('documents',(SELECT coalesce(jsonb_agg(to_jsonb(d) ORDER BY to_jsonb(d)::text),'[]') FROM documents d),'revisions',(SELECT coalesce(jsonb_agg(to_jsonb(r) ORDER BY to_jsonb(r)::text),'[]') FROM document_revisions r),'comments',(SELECT coalesce(jsonb_agg(to_jsonb(c) ORDER BY to_jsonb(c)::text),'[]') FROM comments c),'audit',(SELECT coalesce(jsonb_agg(to_jsonb(a) ORDER BY to_jsonb(a)::text),'[]') FROM audit_entries a))").fetch_one(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    json!({"tracking":tracking,"discussion":discussion})
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_postgres_comment_author_moderator_and_document_endpoint_policy() {
    let h = Harness::new().await;
    let copy = h.clone();
    let run = tokio::spawn(async move { moderation(&copy).await }).await;
    h.cleanup().await;
    if let Err(error) = run {
        std::panic::resume_unwind(error.into_panic());
    }
}
async fn moderation(h: &Harness) {
    let (owner, w, t) = setup(h).await;
    let author = person(h, &owner, w, "member").await;
    let peer = person(h, &owner, w, "member").await;
    let viewer = person(h, &owner, w, "viewer").await;
    let source = issue(h, &owner, w, t, json!({})).await;
    let initial = h
        .app
        .create_comment(
            &author,
            w,
            source.id,
            "comment-author",
            body("Author's original comment"),
        )
        .await
        .unwrap();
    assert_eq!(initial.author.id, author.user.id);
    assert!(initial.can_edit && initial.can_delete);
    let document = initial.document.clone().unwrap();
    assert_eq!(
        (document.owner_type.as_str(), document.owner_id),
        ("comment", initial.id)
    );
    for actor in [&peer, &viewer] {
        let read = h.app.get_comment(actor, w, initial.id).await.unwrap();
        assert!(!read.can_edit && !read.can_delete);
        assert_ne!(read.etag, initial.etag);
        assert!(matches!(
            h.app
                .update_comment(actor, w, initial.id, &read.etag, body("Nonauthor edit"))
                .await,
            Err(Error::Forbidden)
        ));
        assert!(matches!(
            h.app
                .delete_comment(actor, w, initial.id, &read.etag, deletion(None))
                .await,
            Err(Error::Forbidden)
        ));
        let read_doc = h.app.get_document(actor, w, document.id).await.unwrap();
        assert!(!read_doc.can_edit);
        assert!(matches!(
            h.app
                .update_document(
                    actor,
                    w,
                    document.id,
                    &read_doc.etag,
                    body("Generic bypass")
                )
                .await,
            Err(Error::Forbidden)
        ));
    }
    assert!(matches!(
        h.app
            .create_comment(&viewer, w, source.id, "viewer-create", body("Cannot post"))
            .await,
        Err(Error::Forbidden)
    ));
    let edited = h
        .app
        .update_comment(
            &author,
            w,
            initial.id,
            &initial.etag,
            body("Author revised the comment"),
        )
        .await
        .unwrap();
    assert!(edited.edited_at.is_some());
    assert_ne!(edited.version, initial.version);
    assert_ne!(edited.document.as_ref().unwrap().version, document.version);
    assert_ne!(edited.etag, initial.etag);
    match h
        .app
        .update_comment(
            &author,
            w,
            initial.id,
            &initial.etag,
            body("Stale comment draft"),
        )
        .await
        .unwrap_err()
    {
        Error::VersionConflict(current) => {
            assert_eq!(current, serde_json::to_value(&edited).unwrap())
        }
        other => panic!("Expected current comment conflict, got {other:?}"),
    }
    let own_doc = edited.document.as_ref().unwrap();
    assert!(matches!(h.app.update_document(&author,w,own_doc.id,&own_doc.etag,req(json!({"schema_version":1,"body":{"type":"doc","content":[{"type":"paragraph"}]}}))).await,Err(Error::Invalid(_))),"The generic endpoint cannot bypass the comment's nonempty rule");
    let direct = h
        .app
        .update_document(
            &author,
            w,
            own_doc.id,
            &own_doc.etag,
            body("Direct author edit"),
        )
        .await
        .unwrap();
    let after_direct = h.app.get_comment(&author, w, initial.id).await.unwrap();
    assert_eq!(after_direct.document.as_ref().unwrap().etag, direct.etag);
    assert_ne!(
        after_direct.version, edited.version,
        "Generic document editing must maintain the owning comment"
    );
    assert_ne!(after_direct.etag, edited.etag);
    assert_eq!(
        fresh(h, &owner, w, source.id).await.version,
        source.version,
        "Discussion has independent metadata"
    );
    let moderator = h.app.get_comment(&owner, w, initial.id).await.unwrap();
    for reason in [None, Some("   ")] {
        let mut write = body("Moderation without a reason");
        write.moderation_reason = reason.map(str::to_string);
        assert!(matches!(
            h.app
                .update_comment(&owner, w, initial.id, &moderator.etag, write.clone())
                .await,
            Err(Error::Invalid(_))
        ));
        assert!(matches!(
            h.app
                .update_document(
                    &owner,
                    w,
                    document.id,
                    &moderator.document.as_ref().unwrap().etag,
                    write
                )
                .await,
            Err(Error::Invalid(_))
        ));
        assert!(matches!(
            h.app
                .delete_comment(&owner, w, initial.id, &moderator.etag, deletion(reason))
                .await,
            Err(Error::Invalid(_))
        ));
    }
    let reason = "Removed an accidentally pasted credential";
    let redacted = h
        .app
        .update_document(
            &owner,
            w,
            document.id,
            &moderator.document.as_ref().unwrap().etag,
            moderated("[redacted]", reason),
        )
        .await
        .unwrap();
    assert_eq!(redacted.plain_text, "[redacted]");
    let mut tx = h.context(owner.user.id, w).await;
    let audited:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM audit_entries WHERE actor_id=$1 AND metadata->>'moderation_reason'=$2)").bind(owner.user.id).bind(reason).fetch_one(&mut *tx).await.unwrap();
    assert!(
        audited,
        "The moderation reason must be durable in the same transaction"
    );
    tx.commit().await.unwrap();
    // A real team maintainer may moderate, but a viewer's team role never grants writes.
    grant(h, &owner, w, t, peer.user.id).await;
    grant(h, &owner, w, t, viewer.user.id).await;
    let mut tx = h.context(owner.user.id, w).await;
    sqlx::query("UPDATE team_members SET role='maintainer',version=version+1 WHERE team_id=$1 AND user_id=ANY($2)").bind(t).bind(vec![peer.user.id,viewer.user.id]).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    let maintained = h.app.get_comment(&peer, w, initial.id).await.unwrap();
    assert!(maintained.can_edit && maintained.can_delete);
    let viewer_comment = h.app.get_comment(&viewer, w, initial.id).await.unwrap();
    assert!(!viewer_comment.can_edit && !viewer_comment.can_delete);
    assert!(matches!(
        h.app
            .update_comment(
                &viewer,
                w,
                initial.id,
                &viewer_comment.etag,
                moderated("Viewer maintainer attempt", "Test")
            )
            .await,
        Err(Error::Forbidden)
    ));
    h.app
        .delete_comment(
            &peer,
            w,
            initial.id,
            &maintained.etag,
            deletion(Some("Remove duplicated private detail")),
        )
        .await
        .unwrap();
    let tombstone = h.app.get_comment(&author, w, initial.id).await.unwrap();
    assert!(tombstone.deleted_at.is_some() && tombstone.document.is_none());
    assert!(!tombstone.can_edit && !tombstone.can_delete);
    assert_ne!(tombstone.etag, maintained.etag);
    assert!(matches!(
        h.app.get_document(&author, w, document.id).await,
        Err(Error::NotFound)
    ));
    assert!(matches!(
        h.app.document_revision(&author, w, document.id, 1).await,
        Err(Error::NotFound)
    ));
    assert!(matches!(
        h.app
            .document_revisions(&author, w, document.id, DiscussionQuery::default())
            .await,
        Err(Error::NotFound)
    ));
    let mut tx = h.context(owner.user.id, w).await;
    let retained:Value=sqlx::query_scalar("SELECT jsonb_build_object('body',body,'plain_text',plain_text,'deleted',deleted_at IS NOT NULL) FROM documents WHERE id=$1").bind(document.id).fetch_one(&mut *tx).await.unwrap();
    assert_eq!(
        retained,
        json!({"body":{"type":"doc","content":[]},"plain_text":"","deleted":true}),
        "A tombstone must not retain deleted text as its permanent current snapshot"
    );
    assert!(sqlx::query_scalar::<_,bool>("SELECT EXISTS(SELECT 1 FROM document_revisions WHERE document_id=$1 AND plain_text='[redacted]')").bind(document.id).fetch_one(&mut *tx).await.unwrap(),"Previously saved text remains in policy-bound revisions");
    tx.commit().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_postgres_comment_idempotency_pages_and_historical_audience() {
    let h = Harness::new().await;
    let copy = h.clone();
    let run = tokio::spawn(async move { pages_and_access(&copy).await }).await;
    h.cleanup().await;
    if let Err(error) = run {
        std::panic::resume_unwind(error.into_panic());
    }
}
async fn pages_and_access(h: &Harness) {
    let (owner, w, public) = setup(h).await;
    let private = team(h, &owner, w, true).await;
    let author = person(h, &owner, w, "member").await;
    let outside = person(h, &owner, w, "member").await;
    grant(h, &owner, w, private, author.user.id).await;
    let source = issue(h, &owner, w, private, json!({})).await;
    let first = h
        .app
        .create_comment(
            &author,
            w,
            source.id,
            "retry-comment",
            body("Private discussion"),
        )
        .await
        .unwrap();
    let before = state_snapshot(h, &owner, w).await;
    let replay = h
        .app
        .create_comment(
            &author,
            w,
            source.id,
            "retry-comment",
            body("Private discussion"),
        )
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(&replay).unwrap(),
        serde_json::to_value(&first).unwrap()
    );
    assert_eq!(
        state_snapshot(h, &owner, w).await,
        before,
        "Replay does not duplicate activity or bodies"
    );
    assert!(matches!(
        h.app
            .create_comment(
                &author,
                w,
                source.id,
                "retry-comment",
                body("Different request")
            )
            .await,
        Err(Error::Conflict(_))
    ));
    let mut ids = vec![first.id];
    for index in 0..6 {
        ids.push(
            h.app
                .create_comment(
                    &author,
                    w,
                    source.id,
                    &format!("comment-page-{index}"),
                    body(&format!("Comment {index}")),
                )
                .await
                .unwrap()
                .id,
        );
    }
    let remove = h.app.get_comment(&author, w, ids[3]).await.unwrap();
    h.app
        .delete_comment(&author, w, remove.id, &remove.etag, deletion(None))
        .await
        .unwrap();
    let page = h
        .app
        .list_comments(&author, w, source.id, req(json!({"limit":2})))
        .await
        .unwrap();
    assert_eq!(page.data.len(), 2);
    assert!(page.has_more && page.next_cursor.is_some());
    let other = issue(h, &owner, w, private, json!({})).await;
    assert!(matches!(
        h.app
            .list_comments(
                &author,
                w,
                other.id,
                req(json!({"limit":2,"cursor":page.next_cursor}))
            )
            .await,
        Err(Error::Invalid(_))
    ));
    let mut collected = page.data;
    let mut cursor = page.next_cursor;
    while let Some(next) = cursor {
        let page = h
            .app
            .list_comments(&author, w, source.id, req(json!({"limit":2,"cursor":next})))
            .await
            .unwrap();
        assert!(page.data.len() <= 2);
        assert_eq!(page.has_more, page.next_cursor.is_some());
        collected.extend(page.data);
        cursor = page.next_cursor;
    }
    let actual: std::collections::BTreeSet<_> = collected.iter().map(|c| c.id).collect();
    assert_eq!(actual, ids.iter().copied().collect());
    assert_eq!(collected.len(), ids.len());
    assert!(
        collected
            .iter()
            .find(|c| c.id == remove.id)
            .unwrap()
            .document
            .is_none()
    );
    for invalid in [0, 101] {
        assert!(matches!(
            h.app
                .list_comments(&author, w, source.id, req(json!({"limit":invalid})))
                .await,
            Err(Error::Invalid(_))
        ));
    }
    let activity = h
        .app
        .issue_activity_page(&author, w, source.id, req(json!({"limit":2})))
        .await
        .unwrap();
    assert_eq!(activity.data.len(), 2);
    assert!(activity.has_more);
    assert!(
        matches!(
            h.app
                .list_comments(
                    &author,
                    w,
                    source.id,
                    req(json!({"limit":2,"cursor":activity.next_cursor}))
                )
                .await,
            Err(Error::Invalid(_))
        ),
        "Activity and comment cursors are not interchangeable"
    );
    move_to(h, &owner, w, source.id, public).await;
    let historical = h.app.get_comment(&owner, w, first.id).await.unwrap();
    h.app
        .update_comment(
            &owner,
            w,
            first.id,
            &historical.etag,
            moderated(
                "Still private after moderation",
                "Correct private discussion after the move",
            ),
        )
        .await
        .unwrap();
    assert!(
        h.app
            .list_comments(&outside, w, source.id, DiscussionQuery::default())
            .await
            .unwrap()
            .data
            .is_empty()
    );
    assert!(matches!(
        h.app.get_comment(&outside, w, first.id).await,
        Err(Error::NotFound)
    ));
    assert!(matches!(
        h.app
            .get_document(&outside, w, first.document.as_ref().unwrap().id)
            .await,
        Err(Error::NotFound)
    ));
    let public_comment = h
        .app
        .create_comment(
            &outside,
            w,
            source.id,
            "public-after-move",
            body("New public discussion"),
        )
        .await
        .unwrap();
    let visible_activity = h
        .app
        .issue_activity_page(&outside, w, source.id, DiscussionQuery::default())
        .await
        .unwrap();
    assert!(
        visible_activity
            .data
            .iter()
            .all(|event| event["changes"]["comment_id"].is_null()
                || event["changes"]["comment_id"] == public_comment.id.to_string()),
        "Current public access must not reveal old private discussion activity"
    );
    assert_eq!(
        h.app
            .list_comments(&outside, w, source.id, DiscussionQuery::default())
            .await
            .unwrap()
            .data
            .iter()
            .map(|c| c.id)
            .collect::<Vec<_>>(),
        vec![public_comment.id]
    );
    // Losing historical-team access hides private comments even while the issue is public.
    let member = h
        .app
        .get_team_member(&owner, w, private, author.user.id)
        .await
        .unwrap();
    h.app
        .remove_team_member(&owner, w, private, author.user.id, &member.etag(private))
        .await
        .unwrap();
    assert!(matches!(
        h.app.get_comment(&author, w, first.id).await,
        Err(Error::NotFound)
    ));
    assert!(
        matches!(
            h.app
                .create_comment(
                    &author,
                    w,
                    source.id,
                    "retry-comment",
                    body("Private discussion")
                )
                .await,
            Err(Error::NotFound)
        ),
        "A receipt must not bypass the comment's current historical audience"
    );
    move_to(h, &owner, w, source.id, private).await;
    assert!(matches!(
        h.app.get_comment(&outside, w, public_comment.id).await,
        Err(Error::NotFound)
    ));
    assert!(
        matches!(
            h.app
                .create_comment(
                    &outside,
                    w,
                    source.id,
                    "public-after-move",
                    body("New public discussion")
                )
                .await,
            Err(Error::NotFound)
        ),
        "Current issue access is required before replay"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_postgres_comment_required_effects_rollback() {
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
    let issue = issue(h, &a, w, t, json!({})).await;
    let before = state_snapshot(h, &a, w).await;
    for table in [
        "comments",
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
            .create_comment(&a, w, issue.id, "atomic-comment", body("Create comment"))
            .await;
        clear(h, table).await;
        assert!(
            matches!(result, Err(Error::Internal)),
            "Missing comment create effect {table}: {result:?}"
        );
        assert_eq!(
            state_snapshot(h, &a, w).await,
            before,
            "Create rollback: {table}"
        );
    }
    let comment = h
        .app
        .create_comment(&a, w, issue.id, "atomic-comment", body("Create comment"))
        .await
        .unwrap();
    let before = state_snapshot(h, &a, w).await;
    for (table, event) in [
        ("comments", "UPDATE"),
        ("documents", "UPDATE"),
        ("document_revisions", "INSERT"),
        ("activity_entries", "INSERT"),
        ("audit_entries", "INSERT"),
        ("workspace_events", "INSERT"),
        ("workspace_stream_heads", "UPDATE"),
    ] {
        fault(h, table, event).await;
        let result = h
            .app
            .update_comment(&a, w, comment.id, &comment.etag, body("Edited comment"))
            .await;
        clear(h, table).await;
        assert!(
            matches!(result, Err(Error::Internal)),
            "Missing comment edit effect {table}: {result:?}"
        );
        assert_eq!(
            state_snapshot(h, &a, w).await,
            before,
            "Edit rollback: {table}"
        );
    }
    for (table, event) in [
        ("comments", "UPDATE"),
        ("documents", "UPDATE"),
        ("activity_entries", "INSERT"),
        ("audit_entries", "INSERT"),
        ("workspace_events", "INSERT"),
        ("workspace_stream_heads", "UPDATE"),
    ] {
        fault(h, table, event).await;
        let result = h
            .app
            .delete_comment(&a, w, comment.id, &comment.etag, deletion(None))
            .await;
        clear(h, table).await;
        assert!(
            matches!(result, Err(Error::Internal)),
            "Missing comment delete effect {table}: {result:?}"
        );
        assert_eq!(
            state_snapshot(h, &a, w).await,
            before,
            "Delete rollback: {table}"
        );
    }
    h.app
        .delete_comment(&a, w, comment.id, &comment.etag, deletion(None))
        .await
        .unwrap();
    assert!(
        h.app
            .get_comment(&a, w, comment.id)
            .await
            .unwrap()
            .document
            .is_none()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_postgres_large_comment_pages_are_bounded_and_lossless() {
    let h = Harness::new().await;
    let copy = h.clone();
    let run = tokio::spawn(async move { large_comment_pages(&copy).await }).await;
    h.cleanup().await;
    if let Err(error) = run {
        std::panic::resume_unwind(error.into_panic());
    }
}
async fn large_comment_pages(h: &Harness) {
    let (a, w, t) = setup(h).await;
    let source = issue(h, &a, w, t, json!({})).await;
    let mut expected = std::collections::BTreeMap::new();
    // Escapes and multibyte text make raw text length an incorrect size bound.
    // The largest item is close to the allowed encoded-document limit itself.
    for index in 0..9 {
        let text = if index == 8 {
            format!("Large {index}: {}", "\"".repeat(125_000))
        } else {
            format!("Large {index}: {}", "é\\\"\n".repeat(20_000 + index * 500))
        };
        let request = body(&text);
        assert!(serde_json::to_vec(&request.body).unwrap().len() <= DOCUMENT_MAX_BYTES);
        let created = h
            .app
            .create_comment(
                &a,
                w,
                source.id,
                &format!("large-comment-{index}"),
                request.clone(),
            )
            .await
            .unwrap();
        expected.insert(created.id, (request.body, text));
    }
    for limit in [25, 100] {
        let mut cursor: Option<String> = None;
        let mut seen = std::collections::BTreeSet::new();
        let mut pages = 0;
        loop {
            let page = h
                .app
                .list_comments(
                    &a,
                    w,
                    source.id,
                    DiscussionQuery {
                        limit: Some(limit),
                        cursor: cursor.clone(),
                    },
                )
                .await
                .unwrap();
            pages += 1;
            assert!(
                pages <= expected.len(),
                "Pagination must always make progress"
            );
            assert!(
                !page.data.is_empty(),
                "A continuation may not return an empty page"
            );
            assert!(
                serde_json::to_vec(&page).unwrap().len() <= 768 * 1024,
                "The complete encoded page must fit the native response budget"
            );
            for comment in &page.data {
                assert!(seen.insert(comment.id), "Each comment appears exactly once");
                let (body, text) = expected.get(&comment.id).unwrap();
                let document = comment.document.as_ref().unwrap();
                assert_eq!(
                    &document.body, body,
                    "Structured content must not be truncated"
                );
                assert_eq!(
                    &document.plain_text, text,
                    "Plain-text projection must not be truncated"
                );
            }
            assert_eq!(page.has_more, page.next_cursor.is_some());
            if !page.has_more {
                break;
            }
            assert_ne!(page.next_cursor, cursor, "The continuation must advance");
            cursor = page.next_cursor;
        }
        assert!(
            pages > 1,
            "Byte size, not the larger requested item count, must split this feed"
        );
        assert_eq!(seen, expected.keys().copied().collect());
    }
}
