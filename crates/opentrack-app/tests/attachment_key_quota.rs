//! Lifetime allocation admission includes the filesystem retirement records GC retains.
mod graph_support;
#[allow(dead_code)]
mod support;

use graph_support::{blocked, person, req, setup};
use opentrack_app::{
    ScanOutcome, UploadFailure,
    worker::{Worker, WorkerOptions},
};
use opentrack_domain::{Actor, AttachmentLimits, CreateAttachment, Error};
use serde_json::json;
use support::Harness;
use uuid::Uuid;

fn reservation(team: Uuid) -> CreateAttachment {
    req(
        json!({"team_id":team,"original_name":"one-byte.txt","size_bytes":1,"sha256":"a".repeat(64)}),
    )
}

async fn counts(h: &Harness, actor: &Actor, workspace: Uuid) -> (i64, i64) {
    let mut tx = h.context(actor.user.id, workspace).await;
    let result = sqlx::query_as(
        "SELECT (SELECT count(*) FROM attachments WHERE workspace_id=$1),charged_bytes FROM workspace_storage_usage WHERE workspace_id=$1",
    )
    .bind(workspace)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    result
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn retired_and_never_uploaded_keys_keep_their_allocation_charge() {
    let h = Harness::with_connections(8).await;
    let copy = h.clone();
    let result = tokio::spawn(async move { lifetime_allocations(&copy).await }).await;
    h.cleanup().await;
    if let Err(error) = result {
        std::panic::resume_unwind(error.into_panic());
    }
}

async fn lifetime_allocations(h: &Harness) {
    let (owner, workspace, team) = setup(h).await;
    let contributor = person(h, &owner, workspace, "member").await;
    let limits = AttachmentLimits {
        workspace_key_quota: 2,
        ..AttachmentLimits::default()
    };
    let worker = Worker::connect(&h.worker_url().await, WorkerOptions::default())
        .await
        .unwrap();

    // This preexisting reservation must count even when the limit is lowered.
    let first = h
        .app
        .reserve_attachment(
            &contributor,
            workspace,
            "historical-reservation",
            reservation(team),
            &AttachmentLimits::default(),
        )
        .await
        .unwrap();
    h.app
        .delete_attachment(
            &contributor,
            workspace,
            first.id,
            &first.etag,
            req(json!({})),
        )
        .await
        .unwrap();
    let claims = worker.claim_attachment_gc(workspace, 10).await.unwrap();
    assert_eq!(claims.len(), 1);
    assert!(worker.acknowledge_attachment_gc(&claims[0]).await.unwrap());
    assert_eq!(counts(h, &owner, workspace).await, (1, 0));

    let second = h
        .app
        .reserve_attachment(
            &contributor,
            workspace,
            "rejected-reservation",
            reservation(team),
            &limits,
        )
        .await
        .unwrap();
    let permit = h
        .app
        .begin_upload(&contributor, workspace, second.id, &second.etag, &limits)
        .await
        .unwrap();
    h.app
        .fail_attachment_upload(&contributor, &permit, UploadFailure::ChecksumMismatch)
        .await
        .unwrap();
    let claims = worker.claim_attachment_gc(workspace, 10).await.unwrap();
    assert_eq!(claims.len(), 1);
    assert!(worker.acknowledge_attachment_gc(&claims[0]).await.unwrap());
    assert_eq!(counts(h, &owner, workspace).await, (2, 0));

    assert!(matches!(
        h.app
            .reserve_attachment(
                &owner,
                workspace,
                "another-principal-same-budget",
                reservation(team),
                &limits,
            )
            .await,
        Err(Error::Conflict(_))
    ));

    let rejected = h
        .app
        .reserve_attachment(
            &contributor,
            workspace,
            "third-fresh-key",
            reservation(team),
            &limits,
        )
        .await;
    assert!(
        matches!(rejected, Err(Error::Conflict(message)) if message.contains("key allocation limit"))
    );
    assert_eq!(counts(h, &owner, workspace).await, (2, 0));

    let replay = h
        .app
        .reserve_attachment(
            &contributor,
            workspace,
            "historical-reservation",
            reservation(team),
            &limits,
        )
        .await
        .unwrap();
    assert_eq!(replay.id, first.id);
    assert_eq!(counts(h, &owner, workspace).await, (2, 0));

    // A different workspace retains its independent allocation allowance.
    let organization = h
        .app
        .workspace(&owner, workspace)
        .await
        .unwrap()
        .organization_id;
    let other = h
        .app
        .create_workspace(
            &owner,
            "other-key-budget",
            req(json!({"organization_id":organization,"name":"Other workspace","team_name":"Other team","team_key":"OTH"})),
        )
        .await
        .unwrap();
    let other_team = h.app.bootstrap(&owner, other.id).await.unwrap().teams[0].id;
    h.app
        .reserve_attachment(
            &owner,
            other.id,
            "other-workspace-file",
            reservation(other_team),
            &limits,
        )
        .await
        .unwrap();
    assert_eq!(counts(h, &owner, other.id).await, (1, 1));

    // An explicit operator increase permits new allocations; deletion did not.
    h.app
        .reserve_attachment(
            &contributor,
            workspace,
            "third-fresh-key",
            reservation(team),
            &AttachmentLimits {
                workspace_key_quota: 3,
                ..limits
            },
        )
        .await
        .unwrap();
    assert_eq!(counts(h, &owner, workspace).await, (3, 1));
    worker.pool.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_reservations_cannot_share_the_final_key_allocation() {
    let h = Harness::with_connections(8).await;
    let copy = h.clone();
    let result = tokio::spawn(async move { final_allocation(&copy).await }).await;
    h.cleanup().await;
    if let Err(error) = result {
        std::panic::resume_unwind(error.into_panic());
    }
}

async fn final_allocation(h: &Harness) {
    let (actor, workspace, team) = setup(h).await;
    let limits = AttachmentLimits {
        workspace_key_quota: 2,
        ..AttachmentLimits::default()
    };
    let historical = h
        .app
        .reserve_attachment(
            &actor,
            workspace,
            "existing-key",
            reservation(team),
            &limits,
        )
        .await
        .unwrap();
    let mut lock = h.context(actor.user.id, workspace).await;
    sqlx::query(
        "SELECT workspace_id FROM workspace_storage_usage WHERE workspace_id=$1 FOR UPDATE",
    )
    .bind(workspace)
    .execute(&mut *lock)
    .await
    .unwrap();
    let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *lock)
        .await
        .unwrap();
    let first_app = h.app.clone();
    let first_actor = actor.clone();
    let first = tokio::spawn(async move {
        first_app
            .reserve_attachment(
                &first_actor,
                workspace,
                "final-key-a",
                reservation(team),
                &limits,
            )
            .await
    });
    let first_waiter = blocked(h, pid).await;
    let second_app = h.app.clone();
    let second_actor = actor.clone();
    let second = tokio::spawn(async move {
        second_app
            .reserve_attachment(
                &second_actor,
                workspace,
                "final-key-b",
                reservation(team),
                &limits,
            )
            .await
    });
    blocked(h, first_waiter).await;
    lock.commit().await.unwrap();
    let results = [first.await.unwrap(), second.await.unwrap()];
    assert_eq!(results.iter().filter(|item| item.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|item| matches!(item, Err(Error::Conflict(_))))
            .count(),
        1
    );
    assert_eq!(counts(h, &actor, workspace).await, (2, 2));
    let item = results.into_iter().find_map(Result::ok).unwrap();
    let permit = h
        .app
        .begin_upload(&actor, workspace, item.id, &item.etag, &limits)
        .await
        .unwrap();
    h.app
        .quarantine_attachment(&actor, &permit, 1, &permit.sha256, "text/plain")
        .await
        .unwrap();
    let ready = h
        .app
        .finalize_attachment(&actor, &permit, ScanOutcome::NotScanned)
        .await
        .unwrap();
    h.app
        .attachment_download(&actor, workspace, ready.id)
        .await
        .unwrap();
    assert_eq!(
        h.app
            .reserve_attachment(
                &actor,
                workspace,
                "existing-key",
                reservation(team),
                &limits
            )
            .await
            .unwrap()
            .id,
        historical.id
    );
    assert_eq!(counts(h, &actor, workspace).await, (2, 2));
}
