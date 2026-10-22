//! Real PostgreSQL template authority, independent instances and atomic effects.
mod graph_support;
#[allow(dead_code)]
mod support;
use graph_support::*;
use opentrack_domain::*;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use support::Harness;
use uuid::Uuid;

fn document(text: &str) -> DocumentWrite {
    req(json!({"schema_version":1,"body":{"type":"doc","content":[
        {"type":"paragraph","content":[{"type":"text","text":text}]},
        {"type":"taskList","content":[{"type":"taskItem","attrs":{"checked":false},"content":[
            {"type":"paragraph","content":[{"type":"text","text":"Review before shipping"}]}]}]}
    ]}}))
}
fn definition(name: &str) -> CreateIssueTemplate {
    req(
        json!({"name":name,"title":"Investigate release issue","defaults":{"status_id":null,"priority":2,"label_ids":[]},"document":document("Steps to reproduce")}),
    )
}
async fn create(h: &Harness, a: &Actor, w: Uuid, t: Uuid, name: &str) -> IssueTemplate {
    h.app
        .create_template(a, w, t, &Uuid::new_v4().to_string(), definition(name))
        .await
        .unwrap()
}
async fn instantiate(
    h: &Harness,
    a: &Actor,
    w: Uuid,
    t: Uuid,
    template: &IssueTemplate,
) -> CreateIssue {
    let flow = h.app.workflow(a, w, t).await.unwrap();
    let status = template
        .defaults
        .status_id
        .unwrap_or_else(|| flow.statuses.iter().find(|s| s.is_default).unwrap().id);
    req(
        json!({"team_id":t,"title":template.title,"status_id":status,"priority":template.defaults.priority,
        "label_ids":template.defaults.label_ids,"document":{"schema_version":template.document.schema_version,"body":template.document.body},
        "template":{"id":template.summary.id,"version":template.summary.version}}),
    )
}
async fn all(h: &Harness, a: &Actor, w: Uuid) -> Value {
    let tracking = snapshot(h, a, w).await;
    let mut tx = h.context(a.user.id, w).await;
    let details: Value = sqlx::query_scalar("SELECT jsonb_build_object('templates',(SELECT coalesce(jsonb_agg(to_jsonb(x) ORDER BY id),'[]') FROM issue_templates x),'sources',(SELECT coalesce(jsonb_agg(to_jsonb(x) ORDER BY issue_id),'[]') FROM issue_template_sources x),'documents',(SELECT coalesce(jsonb_agg(to_jsonb(x) ORDER BY id),'[]') FROM documents x),'revisions',(SELECT coalesce(jsonb_agg(to_jsonb(x) ORDER BY document_id,revision),'[]') FROM document_revisions x),'audit',(SELECT coalesce(jsonb_agg(to_jsonb(x) ORDER BY id),'[]') FROM audit_entries x))").fetch_one(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    json!({"tracking":tracking,"details":details})
}
async fn maintainer(h: &Harness, a: &Actor, w: Uuid, t: Uuid, user: Uuid) {
    let mut tx = h.context(a.user.id, w).await;
    sqlx::query("INSERT INTO team_members(workspace_id,team_id,user_id,role) VALUES($1,$2,$3,'maintainer') ON CONFLICT(workspace_id,team_id,user_id) DO UPDATE SET role='maintainer',version=team_members.version+1")
        .bind(w).bind(t).bind(user).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_postgres_template_lifecycle_and_independent_instances() {
    let h = Harness::new().await;
    let copy = h.clone();
    let run = tokio::spawn(async move { lifecycle(&copy).await }).await;
    h.cleanup().await;
    if let Err(error) = run {
        std::panic::resume_unwind(error.into_panic());
    }
}
async fn lifecycle(h: &Harness) {
    let (a, w, t) = setup(h).await;
    let create_key = Uuid::new_v4().to_string();
    let command = definition("Release review");
    let original = h
        .app
        .create_template(&a, w, t, &create_key, command.clone())
        .await
        .unwrap();
    let mut different = command.clone();
    different.title = "Different request with the same retry key".into();
    assert!(matches!(
        h.app
            .create_template(&a, w, t, &create_key, different)
            .await,
        Err(Error::Conflict(_))
    ));
    assert_eq!(original.summary.version, "1");
    assert!(
        original.summary.can_manage
            && original.summary.can_use
            && original.document.schema_supported
    );
    assert!(
        original
            .document
            .plain_text
            .contains("Review before shipping")
    );
    let preview = h
        .app
        .preview_template(&a, w, t, original.summary.id)
        .await
        .unwrap();
    assert!(preview.can_create && preview.diagnostics.is_empty());
    assert!(preview.resolved_status.as_ref().unwrap().is_default);
    let payload = instantiate(h, &a, w, t, &original).await;
    let issue_key = Uuid::new_v4().to_string();
    let one = h
        .app
        .create_issue(&a, w, &issue_key, payload.clone())
        .await
        .unwrap();
    let two = h
        .app
        .create_issue(&a, w, &Uuid::new_v4().to_string(), payload.clone())
        .await
        .unwrap();
    let one_document = h.app.issue_document(&a, w, one.id).await.unwrap();
    let two_document = h.app.issue_document(&a, w, two.id).await.unwrap();
    assert_ne!(one_document.id, two_document.id);
    assert_eq!(one_document.body, original.document.body);
    assert_eq!(two_document.body, original.document.body);
    let mut tx = h.context(a.user.id, w).await;
    let sources: Vec<(Uuid,i64,String)> = sqlx::query_as("SELECT issue_id,template_version,definition_sha256 FROM issue_template_sources WHERE template_id=$1 ORDER BY issue_id")
        .bind(original.summary.id).fetch_all(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    assert_eq!(sources.len(), 2);
    assert!(
        sources
            .iter()
            .all(|(_, version, digest)| *version == 1 && digest.len() == 64)
    );
    let edited = h.app.update_template(&a,w,t,original.summary.id,&original.summary.etag,
        req(json!({"name":"Release review revised","title":"Revised title","document":document("Revised template body")}))).await.unwrap();
    assert_eq!(edited.summary.version, "2");
    assert_ne!(edited.summary.etag, original.summary.etag);
    assert_eq!(
        h.app.issue_document(&a, w, one.id).await.unwrap().body,
        one_document.body
    );
    h.app
        .update_document(
            &a,
            w,
            one_document.id,
            &one_document.etag,
            document("Issue-specific instructions"),
        )
        .await
        .unwrap();
    assert_eq!(
        h.app.issue_document(&a, w, two.id).await.unwrap().body,
        two_document.body
    );
    assert_eq!(
        h.app
            .get_template(&a, w, t, original.summary.id)
            .await
            .unwrap()
            .document
            .body,
        edited.document.body
    );
    match h
        .app
        .update_template(
            &a,
            w,
            t,
            original.summary.id,
            &original.summary.etag,
            req(json!({"title":"stale draft"})),
        )
        .await
        .unwrap_err()
    {
        Error::VersionConflict(current) => {
            assert_eq!(current, serde_json::to_value(&edited).unwrap())
        }
        other => panic!("Expected current template conflict, got {other:?}"),
    }
    assert!(matches!(
        h.app
            .create_issue(&a, w, &Uuid::new_v4().to_string(), payload.clone())
            .await,
        Err(Error::VersionConflict(_))
    ));
    let archived = h
        .app
        .update_template(
            &a,
            w,
            t,
            edited.summary.id,
            &edited.summary.etag,
            req(json!({"archived":true})),
        )
        .await
        .unwrap();
    assert!(archived.summary.archived_at.is_some() && !archived.summary.can_use);
    assert!(
        h.app
            .list_templates(&a, w, t, TemplateQuery::default())
            .await
            .unwrap()
            .data
            .is_empty()
    );
    let archive_page = h
        .app
        .list_templates(&a, w, t, req(json!({"archived":true})))
        .await
        .unwrap();
    assert_eq!(archive_page.data.len(), 1);
    assert_eq!(archive_page.data[0].id, archived.summary.id);
    // Original outcome wins over later mutable template state, without another issue/counter/effect.
    let before = all(h, &a, w).await;
    assert_eq!(
        serde_json::to_value(
            h.app
                .create_template(&a, w, t, &create_key, command)
                .await
                .unwrap()
        )
        .unwrap(),
        serde_json::to_value(&archived).unwrap()
    );
    assert_eq!(
        h.app
            .create_issue(&a, w, &issue_key, payload)
            .await
            .unwrap()
            .id,
        one.id
    );
    assert_eq!(all(h, &a, w).await, before);
    let current_payload = instantiate(h, &a, w, t, &archived).await;
    assert!(matches!(
        h.app
            .create_issue(&a, w, &Uuid::new_v4().to_string(), current_payload)
            .await,
        Err(Error::Conflict(_))
    ));
    let restored = h
        .app
        .update_template(
            &a,
            w,
            t,
            archived.summary.id,
            &archived.summary.etag,
            req(json!({"archived":false})),
        )
        .await
        .unwrap();
    assert!(restored.summary.archived_at.is_none() && restored.summary.can_use);
    for name in ["Alpha checklist", "Zulu checklist"] {
        create(h, &a, w, t, name).await;
    }
    let first = h
        .app
        .list_templates(&a, w, t, req(json!({"limit":1,"q":"checklist"})))
        .await
        .unwrap();
    assert_eq!(first.data.len(), 1);
    assert!(first.has_more);
    let second = h
        .app
        .list_templates(
            &a,
            w,
            t,
            req(json!({"limit":1,"q":"checklist","cursor":first.next_cursor})),
        )
        .await
        .unwrap();
    assert_eq!(second.data.len(), 1);
    assert_ne!(first.data[0].id, second.data[0].id);
    assert!(!second.has_more);
    let listing = serde_json::to_value(first).unwrap();
    assert!(
        listing["data"][0].get("document").is_none(),
        "Lists must not copy rich bodies"
    );
    assert!(matches!(
        h.app
            .create_template(
                &a,
                w,
                t,
                "duplicate-template-name",
                definition("  release   REVIEW revised  ")
            )
            .await,
        Err(Error::Conflict(_))
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_postgres_template_current_authority_and_private_source() {
    let h = Harness::new().await;
    let copy = h.clone();
    let run = tokio::spawn(async move { authority(&copy).await }).await;
    h.cleanup().await;
    if let Err(error) = run {
        std::panic::resume_unwind(error.into_panic());
    }
}
async fn authority(h: &Harness) {
    let (a, w, t) = setup(h).await;
    let member = person(h, &a, w, "member").await;
    let viewer = person(h, &a, w, "viewer").await;
    let admin = person(h, &a, w, "admin").await;
    let original = create(h, &a, w, t, "Public checklist").await;
    for reader in [&member, &viewer] {
        let read = h
            .app
            .get_template(reader, w, t, original.summary.id)
            .await
            .unwrap();
        assert!(!read.summary.can_manage);
        assert_ne!(
            read.summary.etag, original.summary.etag,
            "Permission-bearing representation needs a distinct ETag"
        );
        assert!(matches!(
            h.app
                .create_template(
                    reader,
                    w,
                    t,
                    "forbidden-template-create",
                    definition("Denied")
                )
                .await,
            Err(Error::Forbidden)
        ));
        assert!(matches!(
            h.app
                .update_template(
                    reader,
                    w,
                    t,
                    original.summary.id,
                    &read.summary.etag,
                    req(json!({"title":"Denied"}))
                )
                .await,
            Err(Error::Forbidden)
        ));
    }
    assert!(
        h.app
            .preview_template(&member, w, t, original.summary.id)
            .await
            .unwrap()
            .can_create
    );
    assert!(
        !h.app
            .preview_template(&viewer, w, t, original.summary.id)
            .await
            .unwrap()
            .can_create
    );
    let member_payload = instantiate(h, &member, w, t, &original).await;
    h.app
        .create_issue(&member, w, "member-template-issue", member_payload.clone())
        .await
        .unwrap();
    assert!(matches!(
        h.app
            .create_issue(&viewer, w, "viewer-template-issue", member_payload)
            .await,
        Err(Error::Forbidden)
    ));
    maintainer(h, &a, w, t, member.user.id).await;
    let managed = create(h, &member, w, t, "Maintainer checklist").await;
    assert!(managed.summary.can_manage);
    maintainer(h, &a, w, t, viewer.user.id).await;
    let viewer_maintainer = h
        .app
        .get_template(&viewer, w, t, managed.summary.id)
        .await
        .unwrap();
    assert!(!viewer_maintainer.summary.can_manage && !viewer_maintainer.summary.can_use);
    assert!(matches!(
        h.app
            .create_template(
                &viewer,
                w,
                t,
                "viewer-maintainer-denied",
                definition("Denied")
            )
            .await,
        Err(Error::Forbidden)
    ));
    create(h, &admin, w, t, "Admin checklist").await;
    let private = team(h, &a, w, true).await;
    let hidden = create(h, &a, w, private, "Private source name").await;
    for outsider in [&member, &viewer, &admin] {
        assert!(matches!(
            h.app
                .list_templates(outsider, w, private, TemplateQuery::default())
                .await,
            Err(Error::NotFound)
        ));
        assert!(matches!(
            h.app
                .get_template(outsider, w, private, hidden.summary.id)
                .await,
            Err(Error::NotFound)
        ));
        assert!(matches!(
            h.app
                .preview_template(outsider, w, private, hidden.summary.id)
                .await,
            Err(Error::NotFound)
        ));
        assert!(matches!(
            h.app
                .update_template(
                    outsider,
                    w,
                    private,
                    hidden.summary.id,
                    &hidden.summary.etag,
                    req(json!({"title":"private probe"}))
                )
                .await,
            Err(Error::NotFound)
        ));
    }
    assert!(matches!(
        h.app.get_template(&a, w, t, hidden.summary.id).await,
        Err(Error::NotFound)
    ));
    let mut tx = h.context(a.user.id, w).await;
    sqlx::query("DELETE FROM team_members WHERE workspace_id=$1 AND team_id=$2 AND user_id=$3")
        .bind(w)
        .bind(private)
        .bind(a.user.id)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert!(
        matches!(
            h.app.get_template(&a, w, private, hidden.summary.id).await,
            Err(Error::NotFound)
        ),
        "Workspace ownership does not bypass private membership"
    );
    grant(h, &a, w, private, a.user.id).await;
    grant(h, &a, w, private, member.user.id).await;
    let contributor = h
        .app
        .get_template(&member, w, private, hidden.summary.id)
        .await
        .unwrap();
    assert!(contributor.summary.can_use && !contributor.summary.can_manage);
    assert!(matches!(
        h.app
            .create_template(
                &member,
                w,
                private,
                "private-contributor-denied",
                definition("Denied")
            )
            .await,
        Err(Error::Forbidden)
    ));
    maintainer(h, &a, w, private, member.user.id).await;
    let key = Uuid::new_v4().to_string();
    let definition = definition("Private maintainer");
    let own = h
        .app
        .create_template(&member, w, private, &key, definition.clone())
        .await
        .unwrap();
    let payload = instantiate(h, &member, w, private, &own).await;
    let issue_key = Uuid::new_v4().to_string();
    let item = h
        .app
        .create_issue(&member, w, &issue_key, payload.clone())
        .await
        .unwrap();
    let moved = move_to(h, &a, w, item.id, t).await;
    let serialized =
        serde_json::to_string(&h.app.get_issue(&viewer, w, moved.id).await.unwrap()).unwrap();
    assert!(
        !serialized.contains(&own.summary.id.to_string())
            && !serialized.contains("Private maintainer")
    );
    let mut tx = h.context(a.user.id, w).await;
    sqlx::query("DELETE FROM team_members WHERE team_id=$1 AND user_id=$2")
        .bind(private)
        .bind(member.user.id)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert!(matches!(
        h.app
            .create_template(&member, w, private, &key, definition)
            .await,
        Err(Error::NotFound)
    ));
    assert!(
        matches!(
            h.app.create_issue(&member, w, &issue_key, payload).await,
            Err(Error::NotFound)
        ),
        "Historical receipt source authority must still apply after a move"
    );
    assert!(
        h.app.get_issue(&member, w, moved.id).await.is_ok(),
        "The moved issue itself is still visible"
    );
    let own_now = h
        .app
        .get_template(&a, w, private, own.summary.id)
        .await
        .unwrap();
    let before = all(h, &a, w).await;
    assert!(matches!(
        h.app
            .update_template(
                &member,
                w,
                private,
                own.summary.id,
                &own_now.summary.etag,
                req(json!({"archived":true}))
            )
            .await,
        Err(Error::NotFound)
    ));
    assert_eq!(all(h, &a, w).await, before);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_postgres_template_validation_and_stale_reference_diagnostics() {
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
    let before = all(h, &a, w).await;
    for body in [
        json!({"type":"doc","content":[{"type":"attachment","attrs":{"id":Uuid::new_v4()}}]}),
        json!({"type":"doc","content":[{"type":"paragraph","content":[{"type":"mention","attrs":{"id":Uuid::new_v4()}}]}]}),
        json!({"type":"doc","content":[{"type":"image","attrs":{"src":"https://example.invalid/image.png"}}]}),
        json!({"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"x".repeat(270_000)}]}]}),
    ] {
        let mut command = definition("Invalid template");
        command.document.body = body;
        assert!(matches!(
            h.app
                .create_template(&a, w, t, &Uuid::new_v4().to_string(), command)
                .await,
            Err(Error::Invalid(_))
        ));
        assert_eq!(all(h, &a, w).await, before);
    }
    let mut moderated = definition("Moderation not applicable");
    moderated.document.moderation_reason = Some("No comment authority here".into());
    assert!(matches!(
        h.app
            .create_template(&a, w, t, "invalid-template-moderation", moderated)
            .await,
        Err(Error::Invalid(_))
    ));
    let mut unsupported = definition("Unsupported schema");
    unsupported.document.schema_version = 99;
    assert!(matches!(
        h.app
            .create_template(&a, w, t, "invalid-template-schema", unsupported)
            .await,
        Err(Error::UnsupportedDocumentSchema)
    ));
    for patch in [
        json!({"name":null}),
        json!({"document":null}),
        json!({"defaults":null}),
        json!({"archived":null}),
        json!({"team_id":t}),
    ] {
        assert!(serde_json::from_value::<UpdateIssueTemplate>(patch).is_err());
    }
    let flow = h.app.workflow(&a, w, t).await.unwrap();
    let (status, _) = h
        .app
        .create_status(
            &a,
            w,
            t,
            "template-extra-status",
            &flow.etag(),
            req(json!({"name":"Template only","category":"unstarted","color":"blue"})),
        )
        .await
        .unwrap();
    let label = h
        .app
        .create_label(
            &a,
            w,
            "template-extra-label",
            req(json!({"team_id":t,"name":"Template default"})),
        )
        .await
        .unwrap();
    let mut command = definition("Reference diagnostics");
    command.defaults.status_id = Some(status);
    command.defaults.label_ids = vec![label.id];
    let template = h
        .app
        .create_template(&a, w, t, "template-reference-create", command)
        .await
        .unwrap();
    h.app
        .update_label(
            &a,
            w,
            label.id,
            &label.etag(),
            req(json!({"archived":true})),
        )
        .await
        .unwrap();
    let flow = h.app.workflow(&a, w, t).await.unwrap();
    h.app
        .archive_status(&a, w, t, status, &flow.etag())
        .await
        .unwrap();
    let preview = h
        .app
        .preview_template(&a, w, t, template.summary.id)
        .await
        .unwrap();
    assert!(!preview.can_create);
    assert_eq!(preview.resolved_status.as_ref().unwrap().id, status);
    assert!(
        preview
            .resolved_status
            .as_ref()
            .unwrap()
            .archived_at
            .is_some()
    );
    assert!(
        preview
            .diagnostics
            .iter()
            .any(|d| d.reference_id == Some(status))
    );
    assert!(
        preview
            .diagnostics
            .iter()
            .any(|d| d.reference_id == Some(label.id))
    );
    let payload = instantiate(h, &a, w, t, &template).await;
    let before = all(h, &a, w).await;
    assert!(matches!(
        h.app
            .update_template(
                &a,
                w,
                t,
                template.summary.id,
                &template.summary.etag,
                req(json!({"defaults":template.defaults}))
            )
            .await,
        Err(Error::Invalid(_))
    ));
    assert!(matches!(
        h.app
            .create_issue(&a, w, "template-stale-reference", payload)
            .await,
        Err(Error::Invalid(_))
    ));
    assert_eq!(all(h, &a, w).await, before);
    // Explicit repairs are a user decision; template source attribution still pins its reviewed version.
    let mut repaired = instantiate(h, &a, w, t, &template).await;
    repaired.status_id = Some(
        h.app
            .workflow(&a, w, t)
            .await
            .unwrap()
            .statuses
            .iter()
            .find(|s| s.is_default)
            .unwrap()
            .id,
    );
    repaired.label_ids.clear();
    let item = h
        .app
        .create_issue(&a, w, "template-repaired-reference", repaired.clone())
        .await
        .unwrap();
    assert!(item.label_ids.is_empty());
    let mut omitted = repaired.clone();
    omitted.status_id = None;
    assert!(matches!(
        h.app
            .create_issue(&a, w, "template-missing-status", omitted)
            .await,
        Err(Error::Invalid(_))
    ));
    let mut omitted = repaired;
    omitted.document = None;
    assert!(matches!(
        h.app
            .create_issue(&a, w, "template-missing-document", omitted)
            .await,
        Err(Error::Invalid(_))
    ));
    let other = team(h, &a, w, false).await;
    let mut wrong = instantiate(h, &a, w, other, &template).await;
    wrong.team_id = other;
    assert!(matches!(
        h.app
            .create_issue(&a, w, "template-wrong-team", wrong)
            .await,
        Err(Error::NotFound)
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_postgres_template_required_effects_roll_back() {
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
    for (table, event) in [
        ("issue_templates", "INSERT"),
        ("audit_entries", "INSERT"),
        ("workspace_events", "INSERT"),
        ("workspace_stream_heads", "UPDATE"),
        ("idempotency_records", "INSERT"),
        ("idempotency_records", "UPDATE"),
    ] {
        let before = all(h, &a, w).await;
        fault(h, table, event).await;
        let result = h
            .app
            .create_template(
                &a,
                w,
                t,
                &Uuid::new_v4().to_string(),
                definition("Atomic create"),
            )
            .await;
        clear(h, table).await;
        assert!(
            result.is_err(),
            "Suppressed {table} must fail template creation"
        );
        assert_eq!(all(h, &a, w).await, before, "Complete rollback for {table}");
    }
    let template = create(h, &a, w, t, "Atomic update").await;
    for (table, event) in [
        ("issue_templates", "UPDATE"),
        ("audit_entries", "INSERT"),
        ("workspace_events", "INSERT"),
        ("workspace_stream_heads", "UPDATE"),
    ] {
        let before = all(h, &a, w).await;
        fault(h, table, event).await;
        let result = h
            .app
            .update_template(
                &a,
                w,
                t,
                template.summary.id,
                &template.summary.etag,
                req(json!({"title":"Uncommitted title"})),
            )
            .await;
        clear(h, table).await;
        assert!(
            result.is_err(),
            "Suppressed {table} must fail template update"
        );
        assert_eq!(all(h, &a, w).await, before, "Complete rollback for {table}");
    }
    let payload = instantiate(h, &a, w, t, &template).await;
    let key = Uuid::new_v4().to_string();
    let before = all(h, &a, w).await;
    fault(h, "issue_template_sources", "INSERT").await;
    let result = h.app.create_issue(&a, w, &key, payload.clone()).await;
    clear(h, "issue_template_sources").await;
    assert!(
        result.is_err(),
        "Issue attribution cannot silently disappear"
    );
    assert_eq!(
        all(h, &a, w).await,
        before,
        "Issue, body, counter, receipt and stream all roll back"
    );
    let item = h
        .app
        .create_issue(&a, w, &key, payload.clone())
        .await
        .unwrap();
    assert_eq!(
        h.app.create_issue(&a, w, &key, payload).await.unwrap().id,
        item.id
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_postgres_pattern_titles_preview_and_explicit_draft_authority() {
    let h = Harness::new().await;
    let copy = h.clone();
    let run = tokio::spawn(async move { pattern_titles(&copy).await }).await;
    h.cleanup().await;
    if let Err(error) = run {
        std::panic::resume_unwind(error.into_panic());
    }
}
async fn pattern_titles(h: &Harness) {
    let (a, w, t) = setup(h).await;
    let mut command = definition("Daily review");
    command.title = "{{team.key}} {{team.name}} {{date}}".into();
    command.title_mode = TemplateTitleMode::Pattern;
    let template = h
        .app
        .create_template(&a, w, t, "daily-pattern-create", command.clone())
        .await
        .unwrap();
    assert_eq!(template.title_mode, TemplateTitleMode::Pattern);
    let preview = h
        .app
        .preview_template(&a, w, t, template.summary.id)
        .await
        .unwrap();
    let reviewed = format!(
        "GRA Initial {}",
        preview.title_context_at.format("%Y-%m-%d")
    );
    assert_eq!(
        preview.materialized_title.as_deref(),
        Some(reviewed.as_str())
    );
    assert!(preview.can_create && preview.diagnostics.is_empty());
    let mut draft = instantiate(h, &a, w, t, &template).await;
    // The explicit reviewed draft owns its calendar date, even if submission happens later.
    draft.title = "GRA Initial 2028-02-29 - reviewed correction".into();
    let item = h
        .app
        .create_issue(&a, w, "reviewed-pattern-issue", draft.clone())
        .await
        .unwrap();
    assert_eq!(item.title, draft.title);
    let current_team = h.app.get_team(&a, w, t).await.unwrap();
    h.app
        .update_team(
            &a,
            w,
            t,
            &current_team.etag(),
            req(json!({"name":"Renamed example team"})),
        )
        .await
        .unwrap();
    let updated_preview = h
        .app
        .preview_template(&a, w, t, template.summary.id)
        .await
        .unwrap();
    assert!(
        updated_preview
            .materialized_title
            .unwrap()
            .starts_with("GRA Renamed example team ")
    );
    assert_eq!(updated_preview.template.summary.version, "1");
    assert_eq!(
        h.app.get_issue(&a, w, item.id).await.unwrap().title,
        draft.title
    );

    let before = all(h, &a, w).await;
    for invalid in ["{{unknown}}", "{{date+1}}", "{{date}"] {
        let mut invalid_command = command.clone();
        invalid_command.title = invalid.into();
        assert!(matches!(
            h.app
                .create_template(&a, w, t, "invalid-pattern-create", invalid_command)
                .await,
            Err(Error::Invalid(_))
        ));
        assert!(matches!(
            h.app
                .update_template(
                    &a,
                    w,
                    t,
                    template.summary.id,
                    &template.summary.etag,
                    req(json!({"title":invalid}))
                )
                .await,
            Err(Error::Invalid(_))
        ));
    }
    assert_eq!(all(h, &a, w).await, before);
    let oversized = h
        .app
        .update_template(
            &a,
            w,
            t,
            template.summary.id,
            &template.summary.etag,
            req(json!({"title":"{{team.name}}".repeat(19)})),
        )
        .await
        .unwrap();
    let bad_preview = h
        .app
        .preview_template(&a, w, t, template.summary.id)
        .await
        .unwrap();
    assert_eq!(bad_preview.materialized_title, None);
    assert!(!bad_preview.can_create);
    assert!(
        bad_preview
            .diagnostics
            .iter()
            .any(|d| d.field == "title" && d.code == "title_too_long")
    );
    assert!(matches!(
        h.app
            .create_issue(&a, w, "stale-pattern-issue", draft.clone())
            .await,
        Err(Error::VersionConflict(_))
    ));
    assert_eq!(
        h.app
            .create_issue(&a, w, "reviewed-pattern-issue", draft.clone())
            .await
            .unwrap()
            .id,
        item.id
    );
    draft.template = Some(TemplateSelection {
        id: oversized.summary.id,
        version: oversized.summary.version.clone(),
    });
    draft.title = "Explicit repair of oversized default".into();
    assert_eq!(
        h.app
            .create_issue(&a, w, "repaired-pattern-issue", draft.clone())
            .await
            .unwrap()
            .title,
        draft.title
    );
    let literal = h
        .app
        .update_template(
            &a,
            w,
            t,
            oversized.summary.id,
            &oversized.summary.etag,
            req(json!({"title_mode":"literal","title":"{{unknown}} {literal}"})),
        )
        .await
        .unwrap();
    assert_eq!(
        h.app
            .preview_template(&a, w, t, literal.summary.id)
            .await
            .unwrap()
            .materialized_title
            .as_deref(),
        Some("{{unknown}} {literal}")
    );
    assert!(matches!(
        h.app
            .update_template(
                &a,
                w,
                t,
                literal.summary.id,
                &literal.summary.etag,
                req(json!({"title_mode":"pattern"}))
            )
            .await,
        Err(Error::Invalid(_))
    ));

    let hidden_team = team(h, &a, w, true).await;
    command.name = "Private pattern".into();
    let hidden = h
        .app
        .create_template(&a, w, hidden_team, "private-pattern-create", command)
        .await
        .unwrap();
    let outsider = person(h, &a, w, "admin").await;
    assert!(matches!(
        h.app
            .preview_template(&outsider, w, hidden_team, hidden.summary.id)
            .await,
        Err(Error::NotFound)
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_postgres_title_mode_upgrade_preserves_literal_digests_and_legacy_retries() {
    let h = Harness::with_version(5, 19).await;
    let copy = h.clone();
    let run = tokio::spawn(async move { title_mode_upgrade(&copy).await }).await;
    h.cleanup().await;
    if let Err(error) = run {
        std::panic::resume_unwind(error.into_panic());
    }
}
async fn title_mode_upgrade(h: &Harness) {
    let (a, w, t) = setup(h).await;
    let mut command = definition("Legacy literal braces");
    command.title = "{{unknown}} {literal}".into();
    let plain = command.validate().unwrap();
    let hash = |bytes: &[u8]| format!("{:x}", Sha256::digest(bytes));
    let old_definition = hash(&serde_json::to_vec(&json!({"name":command.name,"title":command.title,"defaults":command.defaults,"schema_version":command.document.schema_version,"body":command.document.body})).unwrap());
    let id = Uuid::new_v4();
    let old_response = json!({"id":id,"team_id":t,"name":command.name,"version":"1","etag":format!("\"template:{id}:1:1:1\""),"archived_at":null,"updated_at":"2026-01-01T00:00:00Z","can_manage":true,"can_use":true,"title":command.title,"defaults":command.defaults,"document":{"schema_version":1,"body":command.document.body,"plain_text":plain,"schema_supported":true}});
    let mut tx = h.context(a.user.id, w).await;
    sqlx::query("INSERT INTO issue_templates(id,workspace_id,team_id,name,title,default_priority,document_schema_version,document_body,document_plain_text,definition_sha256) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)")
        .bind(id).bind(w).bind(t).bind(&command.name).bind(&command.title).bind(command.defaults.priority).bind(command.document.schema_version).bind(&command.document.body).bind(&plain).bind(&old_definition).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO idempotency_records(workspace_id,principal_id,route,key_hash,request_hash,response_body) VALUES($1,$2,$3,$4,$5,$6)")
        .bind(w).bind(a.user.id).bind(format!("templates.create:{t}")).bind(hash(b"legacy-template-retry")).bind(hash(&serde_json::to_vec(&command).unwrap())).bind(&old_response).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    let old_issue = h
        .app
        .create_issue(
            &a,
            w,
            "legacy-template-instance",
            req(json!({"team_id":t,"title":command.title,"document":command.document})),
        )
        .await
        .unwrap();
    let mut tx = h.context(a.user.id, w).await;
    sqlx::query("INSERT INTO issue_template_sources(workspace_id,issue_id,source_team_id,template_id,template_version,definition_sha256) VALUES($1,$2,$3,$4,1,$5)")
        .bind(w).bind(old_issue.id).bind(t).bind(id).bind(&old_definition).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    opentrack_store::MIGRATOR.run(&h.owner).await.unwrap();
    let migrated = h.app.get_template(&a, w, t, id).await.unwrap();
    assert_eq!(migrated.title_mode, TemplateTitleMode::Literal);
    assert_eq!(migrated.summary.version, "1");
    assert_eq!(
        h.app
            .preview_template(&a, w, t, id)
            .await
            .unwrap()
            .materialized_title
            .as_deref(),
        Some("{{unknown}} {literal}")
    );
    assert_eq!(
        h.app
            .create_template(&a, w, t, "legacy-template-retry", command)
            .await
            .unwrap()
            .summary
            .id,
        id
    );
    let same = h
        .app
        .update_template(
            &a,
            w,
            t,
            id,
            &migrated.summary.etag,
            req(json!({"title_mode":"literal"})),
        )
        .await
        .unwrap();
    let mut tx = h.context(a.user.id, w).await;
    let current_hash: String =
        sqlx::query_scalar("SELECT definition_sha256 FROM issue_templates WHERE id=$1")
            .bind(id)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    let original_hash: String = sqlx::query_scalar(
        "SELECT definition_sha256 FROM issue_template_sources WHERE issue_id=$1",
    )
    .bind(old_issue.id)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(current_hash, old_definition);
    assert_eq!(original_hash, old_definition);
    assert_eq!(same.summary.version, "2");
    assert_eq!(
        h.app.get_issue(&a, w, old_issue.id).await.unwrap().title,
        "{{unknown}} {literal}"
    );
}
