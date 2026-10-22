//! Real PostgreSQL mention deltas, private audiences and durable subscription choices.
mod graph_support;
#[allow(dead_code)]
mod support;
use graph_support::*;
use opentrack_app::Operator;
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
            .map(|id| json!({"type":"userMention","attrs":{"id":id}})),
    );
    req(
        json!({"schema_version":1,"body":{"type":"doc","content":[{"type":"paragraph","content":content}]}}),
    )
}
fn preference(value: &str) -> UpdateIssueSubscription {
    req(json!({"preference":value}))
}
fn has_source(value: &IssueSubscription, source: &str) -> bool {
    serde_json::to_value(&value.sources)
        .unwrap()
        .as_array()
        .unwrap()
        .contains(&json!(source))
}
async fn own(h: &Harness, a: &Actor, w: Uuid, issue: Uuid) -> IssueSubscription {
    h.app.issue_subscription(a, w, issue).await.unwrap()
}
async fn choose(h: &Harness, a: &Actor, w: Uuid, issue: Uuid, value: &str) -> IssueSubscription {
    let current = own(h, a, w, issue).await;
    h.app
        .update_issue_subscription(a, w, issue, &current.etag, preference(value))
        .await
        .unwrap()
}
async fn count(h: &Harness, a: &Actor, w: Uuid, issue: Uuid, recipient: Uuid) -> i64 {
    let mut tx = h.context(a.user.id, w).await;
    let value = sqlx::query_scalar("SELECT count(*) FROM mention_intents WHERE workspace_id=$1 AND issue_id=$2 AND recipient_id=$3")
        .bind(w).bind(issue).bind(recipient).fetch_one(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    value
}
async fn all(h: &Harness, a: &Actor, w: Uuid) -> Value {
    let tracking = snapshot(h, a, w).await;
    let mut tx = h.context(a.user.id, w).await;
    let details:Value=sqlx::query_scalar("SELECT jsonb_build_object('documents',(SELECT coalesce(jsonb_agg(to_jsonb(x) ORDER BY id),'[]') FROM documents x),'revisions',(SELECT coalesce(jsonb_agg(to_jsonb(x) ORDER BY document_id,revision),'[]') FROM document_revisions x),'comments',(SELECT coalesce(jsonb_agg(to_jsonb(x) ORDER BY id),'[]') FROM comments x),'mentions',(SELECT coalesce(jsonb_agg(to_jsonb(x) ORDER BY document_id,user_id),'[]') FROM document_mentions x),'intents',(SELECT coalesce(jsonb_agg(to_jsonb(x) ORDER BY id),'[]') FROM mention_intents x),'subscriptions',(SELECT coalesce(jsonb_agg(to_jsonb(x) ORDER BY issue_id,user_id),'[]') FROM issue_subscriptions x),'subscription_events',(SELECT coalesce(jsonb_agg(to_jsonb(x) ORDER BY to_jsonb(x)::text),'[]') FROM subscription_events x),'discussion_sources',(SELECT coalesce(jsonb_agg(to_jsonb(x) ORDER BY to_jsonb(x)::text),'[]') FROM discussion_sources x),'audit',(SELECT coalesce(jsonb_agg(to_jsonb(x) ORDER BY id),'[]') FROM audit_entries x))")
        .fetch_one(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    json!({"tracking":tracking,"details":details})
}
fn reference(document: &Document, id: Uuid) -> Value {
    let serialized = serde_json::to_value(document).unwrap();
    serialized["mentions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["id"] == json!(id))
        .unwrap()
        .clone()
}
async fn candidates(h: &Harness, a: &Actor, w: Uuid, query: Value) -> Vec<Uuid> {
    h.app
        .mention_candidates(a, w, req(query))
        .await
        .unwrap()
        .data
        .into_iter()
        .map(|v| v.id)
        .collect()
}
async fn remove_private(h: &Harness, a: &Actor, w: Uuid, t: Uuid, user: Uuid) {
    let member = h.app.get_team_member(a, w, t, user).await.unwrap();
    h.app
        .remove_team_member(a, w, t, user, &member.etag(t))
        .await
        .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_postgres_added_only_mentions_and_explicit_subscription_override() {
    let h = Harness::new().await;
    let copy = h.clone();
    let run = tokio::spawn(async move { deltas_and_preferences(&copy).await }).await;
    h.cleanup().await;
    if let Err(error) = run {
        std::panic::resume_unwind(error.into_panic());
    }
}
async fn deltas_and_preferences(h: &Harness) {
    let (a, w, t) = setup(h).await;
    let member = person(h, &a, w, "member").await;
    let viewer = person(h, &a, w, "viewer").await;
    let peer = person(h, &a, w, "member").await;
    let initial = body(
        "Initial body ",
        &[member.user.id, member.user.id, a.user.id],
    );
    let key = Uuid::new_v4().to_string();
    let command: CreateIssue = req(
        json!({"team_id":t,"title":"Mention contract","assignee_id":member.user.id,"document":initial}),
    );
    let item = h
        .app
        .create_issue(&a, w, &key, command.clone())
        .await
        .unwrap();
    let document = h.app.issue_document(&a, w, item.id).await.unwrap();
    assert_eq!(
        count(h, &a, w, item.id, member.user.id).await,
        1,
        "Repeated nodes have one recipient effect"
    );
    assert_eq!(
        count(h, &a, w, item.id, a.user.id).await,
        0,
        "A self mention is not a delivery intent"
    );
    assert!(document.plain_text.contains("@someone"));
    assert!(!document.plain_text.contains(&member.user.display_name));
    assert_eq!(
        document.body, initial.body,
        "Resolved names must not be written into canonical nodes"
    );
    let creator = own(h, &a, w, item.id).await;
    assert!(creator.subscribed && has_source(&creator, "creator"));
    let assigned = own(h, &member, w, item.id).await;
    assert!(
        assigned.subscribed
            && has_source(&assigned, "assignment")
            && has_source(&assigned, "mention")
    );
    let unchanged = all(h, &a, w).await;
    assert_eq!(
        h.app.create_issue(&a, w, &key, command).await.unwrap().id,
        item.id
    );
    assert_eq!(
        all(h, &a, w).await,
        unchanged,
        "Issue retry does not repeat mention or subscription effects"
    );

    let before = fresh(h, &a, w, item.id).await;
    let unsubscribed = choose(h, &member, w, item.id, "unsubscribed").await;
    assert!(
        !unsubscribed.subscribed
            && has_source(&unsubscribed, "assignment")
            && has_source(&unsubscribed, "mention")
    );
    assert_eq!(fresh(h, &a, w, item.id).await.etag(), before.etag());
    assert_eq!(
        h.app.get_document(&a, w, document.id).await.unwrap().etag,
        document.etag
    );
    assert!(
        matches!(
            h.app
                .update_issue_subscription(
                    &member,
                    w,
                    item.id,
                    &assigned.etag,
                    preference("subscribed")
                )
                .await,
            Err(Error::VersionConflict(_))
        ),
        "A stale preference cannot silently reverse explicit unsubscribe"
    );
    assert!(
        !choose(h, &peer, w, item.id, "unsubscribed")
            .await
            .subscribed
    );
    let before_assignment = fresh(h, &a, w, item.id).await;
    h.app
        .update_issue(
            &a,
            w,
            item.id,
            &before_assignment.etag(),
            json!({"assignee_id":peer.user.id}),
        )
        .await
        .unwrap();
    let assigned_after_opt_out = own(h, &peer, w, item.id).await;
    assert!(
        !assigned_after_opt_out.subscribed && has_source(&assigned_after_opt_out, "assignment"),
        "A first automatic assignment source does not override an earlier explicit opt-out"
    );

    let next = h
        .app
        .update_document(
            &a,
            w,
            document.id,
            &document.etag,
            body(
                "New viewer ",
                &[member.user.id, viewer.user.id, viewer.user.id, a.user.id],
            ),
        )
        .await
        .unwrap();
    assert_eq!(
        count(h, &a, w, item.id, member.user.id).await,
        1,
        "Retained mentions are not new recipients"
    );
    assert_eq!(
        count(h, &a, w, item.id, viewer.user.id).await,
        1,
        "A reader may be mentioned even when not assignable"
    );
    assert!(!own(h, &member, w, item.id).await.subscribed);
    assert!(own(h, &viewer, w, item.id).await.subscribed);
    let repeated = h
        .app
        .update_document(
            &a,
            w,
            document.id,
            &next.etag,
            body(
                "Reworded only ",
                &[viewer.user.id, member.user.id, a.user.id],
            ),
        )
        .await
        .unwrap();
    assert_eq!(count(h, &a, w, item.id, member.user.id).await, 1);
    assert_eq!(count(h, &a, w, item.id, viewer.user.id).await, 1);
    let before_stale = all(h, &a, w).await;
    assert!(matches!(
        h.app
            .update_document(
                &a,
                w,
                document.id,
                &next.etag,
                body("Stale ", &[peer.user.id])
            )
            .await,
        Err(Error::VersionConflict(_))
    ));
    assert_eq!(all(h, &a, w).await, before_stale);
    let removed = h
        .app
        .update_document(
            &a,
            w,
            document.id,
            &repeated.etag,
            body("Removed mention", &[]),
        )
        .await
        .unwrap();
    h.app
        .update_document(
            &a,
            w,
            document.id,
            &removed.etag,
            body("Explicitly re-added ", &[member.user.id]),
        )
        .await
        .unwrap();
    assert_eq!(count(h, &a, w, item.id, member.user.id).await, 2);
    assert!(
        !own(h, &member, w, item.id).await.subscribed,
        "New mention does not override opt-out"
    );

    let comment_key = Uuid::new_v4().to_string();
    let comment_body = body("", &[member.user.id, a.user.id]);
    let comment = h
        .app
        .create_comment(&a, w, item.id, &comment_key, comment_body.clone())
        .await
        .unwrap();
    assert_eq!(count(h, &a, w, item.id, member.user.id).await, 3);
    assert!(!own(h, &member, w, item.id).await.subscribed);
    assert_eq!(count(h, &a, w, item.id, a.user.id).await, 0);
    let current = all(h, &a, w).await;
    assert_eq!(
        h.app
            .create_comment(&a, w, item.id, &comment_key, comment_body.clone())
            .await
            .unwrap()
            .id,
        comment.id
    );
    assert_eq!(all(h, &a, w).await, current, "Comment retry is effect-free");
    let comment_doc = comment.document.unwrap();
    h.app
        .update_document(
            &a,
            w,
            comment_doc.id,
            &comment_doc.etag,
            body("Generic edit ", &[member.user.id, peer.user.id]),
        )
        .await
        .unwrap();
    assert_eq!(count(h, &a, w, item.id, member.user.id).await, 3);
    assert_eq!(count(h, &a, w, item.id, peer.user.id).await, 1);
    assert!(!own(h, &peer, w, item.id).await.subscribed);
    let mut tx = h.context(a.user.id, w).await;
    let sources: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM discussion_sources WHERE workspace_id=$1 AND issue_id=$2",
    )
    .bind(w)
    .bind(item.id)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    let leaked:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM workspace_events e WHERE workspace_id=$1 AND event_type IN('document.updated','comment.created','comment.updated') AND to_jsonb(e)::text LIKE $2)").bind(w).bind(format!("%{}%",peer.user.id)).fetch_one(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    assert_eq!(
        sources, 1,
        "Only comment creation, not edits or replay, creates a subscribed-discussion source"
    );
    assert!(
        !leaked,
        "Recipient IDs do not belong in the general workspace replay payload"
    );
    let now = fresh(h, &a, w, item.id).await;
    h.app
        .update_issue(&a, w, item.id, &now.etag(), json!({"assignee_id":null}))
        .await
        .unwrap();
    let automatic = choose(h, &member, w, item.id, "automatic").await;
    assert!(
        automatic.subscribed
            && has_source(&automatic, "assignment")
            && has_source(&automatic, "mention"),
        "Automatic reset retains the recorded follow reasons"
    );
    assert!(
        !choose(h, &viewer, w, item.id, "unsubscribed")
            .await
            .subscribed
    );
    assert!(
        choose(h, &viewer, w, item.id, "subscribed")
            .await
            .subscribed,
        "Read-only members can choose their own following preference"
    );
    let current_comment = h.app.get_comment(&a, w, comment.id).await.unwrap();
    let peer_membership = h
        .app
        .get_workspace_member(&a, w, peer.user.id)
        .await
        .unwrap();
    h.app
        .update_member(
            &a,
            w,
            peer.user.id,
            &peer_membership.etag(w),
            req(json!({"active":false})),
        )
        .await
        .unwrap();
    let after_revoke = all(h, &a, w).await;
    let replay = h
        .app
        .create_comment(&a, w, item.id, &comment_key, comment_body)
        .await
        .unwrap();
    assert_ne!(replay.etag, current_comment.etag);
    assert_eq!(
        reference(replay.document.as_ref().unwrap(), peer.user.id)["display_name"],
        Value::Null,
        "A create retry projects the current comment instead of its old serialized recipient name"
    );
    assert_eq!(
        all(h, &a, w).await,
        after_revoke,
        "Current-authority replay has no repeated source effects"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_postgres_mention_current_and_historical_authority() {
    let h = Harness::new().await;
    let copy = h.clone();
    let run = tokio::spawn(async move { authority(&copy).await }).await;
    h.cleanup().await;
    if let Err(error) = run {
        std::panic::resume_unwind(error.into_panic());
    }
}
async fn authority(h: &Harness) {
    let (a, w, public) = setup(h).await;
    let private = team(h, &a, w, true).await;
    let member = person(h, &a, w, "member").await;
    let viewer = person(h, &a, w, "viewer").await;
    let outside = person(h, &a, w, "admin").await;
    grant(h, &a, w, private, member.user.id).await;
    grant(h, &a, w, private, viewer.user.id).await;
    let item = issue(
        h,
        &a,
        w,
        private,
        json!({"document":body("Private source ",&[member.user.id])}),
    )
    .await;
    let document = h.app.issue_document(&a, w, item.id).await.unwrap();
    let found = candidates(h, &a, w, json!({"issue_id":item.id})).await;
    for table in ["document_mentions", "mention_intents", "discussion_sources"] {
        let mut restricted = opentrack_store::begin(&h.app.pool).await.unwrap();
        opentrack_store::context(&mut restricted, a.user.id, Some(w))
            .await
            .unwrap();
        let read = sqlx::query(sqlx::AssertSqlSafe(format!("SELECT count(*) FROM {table}")))
            .fetch_one(&mut *restricted)
            .await;
        assert!(
            read.is_err(),
            "The serving role cannot read private durable source table {table}"
        );
        restricted.rollback().await.unwrap();
    }
    assert!(found.contains(&member.user.id) && found.contains(&viewer.user.id));
    assert!(!found.contains(&outside.user.id));
    assert!(matches!(
        h.app
            .mention_candidates(&outside, w, req(json!({"issue_id":item.id})))
            .await,
        Err(Error::NotFound)
    ));
    assert!(matches!(
        h.app.issue_subscription(&outside, w, item.id).await,
        Err(Error::NotFound)
    ));
    assert!(matches!(
        h.app
            .update_issue_subscription(&outside, w, item.id, "*", preference("subscribed"))
            .await,
        Err(Error::NotFound)
    ));
    assert!(matches!(
        h.app
            .update_document(
                &viewer,
                w,
                document.id,
                &document.etag,
                body("Viewer attempt ", &[member.user.id])
            )
            .await,
        Err(Error::Forbidden)
    ));
    let comment_key = Uuid::new_v4().to_string();
    let comment_body = body("Private comment ", &[member.user.id]);
    let comment = h
        .app
        .create_comment(&member, w, item.id, &comment_key, comment_body.clone())
        .await
        .unwrap();
    let comment_doc = comment.document.unwrap();
    move_to(h, &a, w, item.id, public).await;
    assert!(
        candidates(h, &a, w, json!({"document_id":document.id}))
            .await
            .contains(&outside.user.id)
    );
    assert!(
        !candidates(h, &a, w, json!({"document_id":comment_doc.id}))
            .await
            .contains(&outside.user.id),
        "Comment audience remains restricted by its original private team"
    );
    assert!(matches!(
        h.app.get_comment(&outside, w, comment.id).await,
        Err(Error::NotFound)
    ));
    let current = h
        .app
        .get_document(&member, w, comment_doc.id)
        .await
        .unwrap();
    assert!(matches!(
        h.app
            .update_document(
                &member,
                w,
                comment_doc.id,
                &current.etag,
                body(
                    "Cannot invite public readers ",
                    &[member.user.id, outside.user.id]
                )
            )
            .await,
        Err(Error::Invalid(_))
    ));
    let moved_revision = h
        .app
        .update_document(
            &member,
            w,
            current.id,
            &current.etag,
            body(
                "Edited while public, retaining private comment audience ",
                &[member.user.id],
            ),
        )
        .await
        .unwrap();
    remove_private(h, &a, w, private, member.user.id).await;
    let historical = h.app.get_document(&a, w, comment_doc.id).await.unwrap();
    assert_eq!(reference(&historical, member.user.id)["available"], false);
    assert_eq!(
        reference(&historical, member.user.id)["display_name"],
        Value::Null
    );
    assert_eq!(historical.body, moved_revision.body);
    assert_ne!(
        historical.etag, comment_doc.etag,
        "Strong document ETag changes with authorized mention projection"
    );
    let moved_history = h
        .app
        .document_revision(
            &a,
            w,
            comment_doc.id,
            moved_revision.version.parse().unwrap(),
        )
        .await
        .unwrap();
    let moved_history = serde_json::to_value(moved_history).unwrap();
    assert_eq!(
        moved_history["mentions"][0]["display_name"],
        Value::Null,
        "A public revision origin cannot replace the comment's original private audience"
    );
    assert_eq!(moved_history["mentions"][0]["available"], false);
    assert!(
        matches!(
            h.app
                .create_comment(&member, w, item.id, &comment_key, comment_body)
                .await,
            Err(Error::NotFound)
        ),
        "Replay must retain the original comment audience"
    );
    let revision = h
        .app
        .document_revision(&a, w, document.id, 1)
        .await
        .unwrap();
    let revision = serde_json::to_value(revision).unwrap();
    assert_eq!(
        revision["mentions"][0]["display_name"],
        Value::Null,
        "Private historical labels are resolved under the original audience"
    );
    assert_eq!(revision["body"], document.body);
    move_to(h, &a, w, item.id, private).await;
    let current = h.app.get_document(&a, w, document.id).await.unwrap();
    assert!(
        !reference(&current, member.user.id)["available"]
            .as_bool()
            .unwrap()
    );
    let saved = h
        .app
        .update_document(
            &a,
            w,
            document.id,
            &current.etag,
            body("Retain unavailable reference ", &[member.user.id]),
        )
        .await
        .unwrap();
    assert_eq!(
        reference(&saved, member.user.id)["display_name"],
        Value::Null
    );
    assert_eq!(
        count(h, &a, w, item.id, member.user.id).await,
        1,
        "Retaining unavailable historical reference generates no fresh intent"
    );
    assert!(matches!(
        h.app
            .update_document(
                &a,
                w,
                document.id,
                &saved.etag,
                body("Forged ", &[member.user.id, Uuid::new_v4()])
            )
            .await,
        Err(Error::Invalid(_))
    ));
    let removed = h
        .app
        .update_document(&a, w, document.id, &saved.etag, body("Removed", &[]))
        .await
        .unwrap();
    assert!(matches!(
        h.app
            .update_document(
                &a,
                w,
                document.id,
                &removed.etag,
                body("New inaccessible reference ", &[member.user.id])
            )
            .await,
        Err(Error::Invalid(_))
    ));

    // Disable an otherwise eligible recipient without bypassing the account guard.
    let public_item = issue(
        h,
        &a,
        w,
        public,
        json!({"document":body("Current enabled ",&[outside.user.id])}),
    )
    .await;
    let old = h.app.issue_document(&a, w, public_item.id).await.unwrap();
    let operator = Operator::connect(&h.operator_url().await).await.unwrap();
    operator
        .disable_account(outside.user.id, "Verify disabled mention recipient")
        .await
        .unwrap();
    let disabled = h.app.get_document(&a, w, old.id).await.unwrap();
    assert_eq!(reference(&disabled, outside.user.id)["available"], false);
    assert_ne!(disabled.etag, old.etag);
    assert!(
        !candidates(h, &a, w, json!({"issue_id":public_item.id}))
            .await
            .contains(&outside.user.id)
    );
    h.app
        .update_document(
            &a,
            w,
            old.id,
            &disabled.etag,
            body("Keep disabled history ", &[outside.user.id]),
        )
        .await
        .unwrap();
    assert!(matches!(
        h.app
            .create_comment(
                &a,
                w,
                public_item.id,
                "disabled-new-mention",
                body("New disabled ", &[outside.user.id])
            )
            .await,
        Err(Error::Invalid(_))
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_postgres_mention_shape_bounds_scope_and_template_exclusion() {
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
    let peer = person(h, &a, w, "member").await;
    let item = issue(h, &a, w, t, json!({})).await;
    for node in [
        json!({"type":"userMention","attrs":{"id":peer.user.id,"label":"forged private name"}}),
        json!({"type":"userMention","attrs":{"id":"../not-a-uuid"}}),
        json!({"type":"userMention","attrs":{"id":peer.user.id},"content":[{"type":"text","text":"forged"}]}),
        json!({"type":"userMention","attrs":{"id":peer.user.id},"text":"forged"}),
    ] {
        let candidate: DocumentWrite = req(
            json!({"schema_version":1,"body":{"type":"doc","content":[{"type":"paragraph","content":[node]}]}}),
        );
        assert!(candidate.validate().is_err());
    }
    let fifty = (0..50).map(|_| Uuid::new_v4()).collect::<Vec<_>>();
    assert!(body("", &fifty).validate().is_ok());
    let mut fifty_one = fifty.clone();
    fifty_one.push(Uuid::new_v4());
    assert!(body("", &fifty_one).validate().is_err());
    assert!(
        body("", &vec![peer.user.id; 51]).validate().is_ok(),
        "The bound is distinct recipients, not occurrences"
    );
    for query in [
        json!({}),
        json!({"issue_id":item.id,"team_id":t}),
        json!({"issue_id":item.id,"limit":0}),
        json!({"issue_id":item.id,"limit":51}),
        json!({"issue_id":item.id,"q":"x".repeat(101)}),
    ] {
        assert!(matches!(
            h.app.mention_candidates(&a, w, req(query)).await,
            Err(Error::Invalid(_))
        ));
    }
    let page = h
        .app
        .mention_candidates(&a, w, req(json!({"issue_id":item.id,"limit":1})))
        .await
        .unwrap();
    assert!(page.has_more && page.next_cursor.is_some());
    assert_eq!(page.data.len(), 1);
    assert!(
        h.app
            .mention_candidates(
                &a,
                w,
                req(json!({"team_id":t,"limit":1,"cursor":page.next_cursor}))
            )
            .await
            .is_err(),
        "Candidate cursors cannot transfer between scopes"
    );
    let template: CreateIssueTemplate = req(
        json!({"name":"No mention template","title":"No mention defaults","defaults":{"status_id":null,"priority":0,"label_ids":[]},"document":body("Future template mention ",&[peer.user.id])}),
    );
    assert!(
        matches!(
            h.app
                .create_template(&a, w, t, "mention-template-excluded", template)
                .await,
            Err(Error::Invalid(_))
        ),
        "Extending the common document validator must not enable template mentions"
    );
    let before = all(h, &a, w).await;
    assert!(matches!(h.app.create_issue(&a,w,"unknown-initial-mention",req(json!({"team_id":t,"title":"No leaked recipient","document":body("",&[Uuid::new_v4()])}))).await,Err(Error::Invalid(_))));
    assert_eq!(all(h, &a, w).await, before);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_postgres_required_mention_subscription_and_discussion_effects_roll_back() {
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
    let peer = person(h, &a, w, "member").await;
    for table in [
        "document_mentions",
        "mention_intents",
        "issue_subscriptions",
    ] {
        let before = all(h, &a, w).await;
        fault(h, table, "INSERT").await;
        let result=h.app.create_issue(&a,w,&Uuid::new_v4().to_string(),req(json!({"team_id":t,"title":"Atomic initial mention","document":body("",&[peer.user.id])}))).await;
        clear(h, table).await;
        assert!(
            result.is_err(),
            "Suppressed {table} must prevent issue creation"
        );
        assert_eq!(
            all(h, &a, w).await,
            before,
            "Initial body and all other effects roll back for {table}"
        );
    }
    let item = issue(h, &a, w, t, json!({})).await;
    for table in [
        "document_mentions",
        "mention_intents",
        "issue_subscriptions",
        "discussion_sources",
        "audit_entries",
        "activity_entries",
        "workspace_events",
    ] {
        let before = all(h, &a, w).await;
        fault(h, table, "INSERT").await;
        let result = h
            .app
            .create_comment(
                &a,
                w,
                item.id,
                &Uuid::new_v4().to_string(),
                body("Atomic comment ", &[peer.user.id]),
            )
            .await;
        clear(h, table).await;
        assert!(
            result.is_err(),
            "Suppressed {table} must prevent comment creation"
        );
        assert_eq!(
            all(h, &a, w).await,
            before,
            "Comment, retry, sources and effects roll back for {table}"
        );
    }
    let document = h.app.issue_document(&a, w, item.id).await.unwrap();
    for table in [
        "document_mentions",
        "mention_intents",
        "issue_subscriptions",
    ] {
        let before = all(h, &a, w).await;
        fault(h, table, "INSERT").await;
        let result = h
            .app
            .update_document(
                &a,
                w,
                document.id,
                &document.etag,
                body("Atomic edit ", &[peer.user.id]),
            )
            .await;
        clear(h, table).await;
        assert!(result.is_err(), "Suppressed {table} must prevent body save");
        assert_eq!(
            all(h, &a, w).await,
            before,
            "Body revision and all effects roll back for {table}"
        );
    }
    let original = own(h, &a, w, item.id).await;
    for (table, event) in [
        ("issue_subscriptions", "UPDATE"),
        ("subscription_events", "INSERT"),
        ("audit_entries", "INSERT"),
    ] {
        let before = all(h, &a, w).await;
        fault(h, table, event).await;
        let result = h
            .app
            .update_issue_subscription(&a, w, item.id, &original.etag, preference("unsubscribed"))
            .await;
        clear(h, table).await;
        assert!(
            result.is_err(),
            "Suppressed {table} must prevent preference success"
        );
        assert_eq!(all(h, &a, w).await, before);
    }
    let saved = h
        .app
        .update_document(
            &a,
            w,
            document.id,
            &document.etag,
            body("Retry after faults ", &[peer.user.id]),
        )
        .await
        .unwrap();
    assert_eq!(count(h, &a, w, item.id, peer.user.id).await, 1);
    let before_remove = all(h, &a, w).await;
    fault(h, "document_mentions", "DELETE").await;
    let result = h
        .app
        .update_document(&a, w, saved.id, &saved.etag, body("Remove", &[]))
        .await;
    clear(h, "document_mentions").await;
    assert!(
        result.is_err(),
        "Suppressed removal cannot leave a stale mention projection"
    );
    assert_eq!(all(h, &a, w).await, before_remove);
}
