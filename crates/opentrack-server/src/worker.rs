use crate::config::{Cli, Settings};
use opentrack_app::worker::{Worker, WorkerOptions, WorkerTick};
use std::time::Duration;

#[derive(Default)]
struct Cursors {
    capture: Option<uuid::Uuid>,
    job: Option<uuid::Uuid>,
    retention: Option<uuid::Uuid>,
    document_retention: Option<uuid::Uuid>,
    attachment_gc: Option<uuid::Uuid>,
}

#[derive(Clone, Copy)]
enum Stage {
    Capture,
    Job,
    Retention,
    DocumentRetention,
    AttachmentGc,
}
impl Stage {
    fn next(self) -> Self {
        match self {
            Self::Capture => Self::Job,
            Self::Job => Self::Retention,
            Self::Retention => Self::DocumentRetention,
            Self::DocumentRetention => Self::AttachmentGc,
            Self::AttachmentGc => Self::Capture,
        }
    }
}

// Separate cursors and a fixed rotation ensure a busy projection cannot starve
// accepted jobs or retention. Each stage finishes its own bounded transaction.
async fn poll_stage(
    worker: &Worker,
    cursors: &mut Cursors,
    stage: Stage,
    blobs: &opentrack_adapters::local_blob::LocalBlobStore,
) -> opentrack_domain::Result<bool> {
    match stage {
        Stage::AttachmentGc => {
            use opentrack_adapters::local_blob::{CleanupRequest, CleanupState};
            let Some((_, claims)) = worker
                .claim_attachment_gc_next(&mut cursors.attachment_gc, 25)
                .await?
            else {
                return Ok(false);
            };
            let mut removed = false;
            let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
            for claim in claims {
                let key = claim
                    .storage_key
                    .parse()
                    .map_err(|_| opentrack_domain::Error::Internal)?;
                let result = tokio::time::timeout_at(
                    deadline,
                    blobs.cleanup(vec![CleanupRequest {
                        key,
                        older_than: std::time::SystemTime::now(),
                    }]),
                )
                .await
                .map_err(|_| opentrack_domain::Error::Unavailable)?
                .map_err(|_| opentrack_domain::Error::Unavailable)?;
                if result.first().is_some_and(|v| {
                    matches!(v.state, CleanupState::Removed | CleanupState::Missing)
                }) {
                    removed |= worker.acknowledge_attachment_gc(&claim).await?;
                }
            }
            Ok(removed)
        }
        Stage::Capture => Ok(worker
            .capture_next(&mut cursors.capture)
            .await?
            .is_some_and(|(_, result)| {
                result.processed > 0 || result.captured > 0 || result.backfilled
            })),
        Stage::Job => Ok(worker.tick(&mut cursors.job).await? != WorkerTick::Idle),
        Stage::Retention => Ok(worker
            .retain_next(&mut cursors.retention, 100)
            .await?
            .is_some_and(|(_, result)| result.deleted > 0)),
        Stage::DocumentRetention => Ok(worker
            .prune_document_revisions_next(&mut cursors.document_retention, 100)
            .await?
            .is_some_and(|(_, deleted)| deleted > 0)),
    }
}

pub async fn run(cli: &Cli, cfg: &Settings) -> Result<(), String> {
    let worker = Worker::connect(
        &cfg.worker_database_url()?,
        WorkerOptions {
            max_connections: cfg.worker.max_connections,
            lease_seconds: cfg.worker.lease_seconds,
        },
    )
    .await
    .map_err(|_| "Worker startup failed; check migrations and the restricted worker credential")?;
    let current_schema: bool =
        sqlx::query_scalar("SELECT to_regclass('consumer_receipts') IS NOT NULL AND to_regclass('document_revisions') IS NOT NULL AND to_regclass('attachments') IS NOT NULL AND to_regclass('issue_templates') IS NOT NULL AND EXISTS(SELECT 1 FROM pg_attribute WHERE attrelid=to_regclass('issue_templates') AND attname='title_mode' AND NOT attisdropped) AND to_regclass('issue_template_sources') IS NOT NULL AND to_regclass('issue_subscriptions') IS NOT NULL AND to_regclass('document_mentions') IS NOT NULL AND to_regclass('mention_intents') IS NOT NULL AND to_regclass('subscription_events') IS NOT NULL AND to_regclass('discussion_sources') IS NOT NULL AND to_regclass('user_preferences') IS NOT NULL AND to_regclass('user_preference_events') IS NOT NULL AND to_regclass('projects') IS NOT NULL AND to_regclass('project_issues') IS NOT NULL AND to_regclass('project_updates') IS NOT NULL AND to_regclass('milestones') IS NOT NULL AND EXISTS(SELECT 1 FROM pg_attribute WHERE attrelid=to_regclass('project_issues') AND attname='milestone_id' AND NOT attisdropped)")
            .fetch_one(&worker.pool)
            .await
            .map_err(|_| "Cannot inspect the worker schema")?;
    if !current_schema {
        worker.pool.close().await;
        return Err(
            "Apply current document and event-consumer migrations before starting this worker version".into(),
        );
    }
    let blobs = crate::storage::open(cfg).await?;
    let mut cursors = Cursors::default();
    if cli.once {
        let result = tokio::time::timeout(Duration::from_secs(30), async {
            for stage in [
                Stage::Capture,
                Stage::Job,
                Stage::Retention,
                Stage::DocumentRetention,
                Stage::AttachmentGc,
            ] {
                poll_stage(&worker, &mut cursors, stage, &blobs).await?;
            }
            opentrack_domain::Result::Ok(())
        })
        .await;
        worker.pool.close().await;
        result.map_err(|_| "Worker deadline reached; uncommitted work rolls back and unfinished claims recover after lease expiry")?
            .map_err(|_| "Worker poll failed; inspect consumer status and durable jobs")?;
        tracing::info!("Worker completed one bounded event, job and retention poll");
        return Ok(());
    }
    tracing::info!("Worker started with isolated database authority");
    let shutdown = crate::shutdown();
    tokio::pin!(shutdown);
    let mut stage = Stage::Capture;
    let mut round_active = false;
    loop {
        let result = {
            let started = std::sync::atomic::AtomicBool::new(false);
            let poll = async {
                started.store(true, std::sync::atomic::Ordering::Relaxed);
                poll_stage(&worker, &mut cursors, stage, &blobs).await
            };
            tokio::pin!(poll);
            tokio::select! {
                biased;
                _ = &mut shutdown => {
                    // Finish only the in-flight bounded stage. No later stage is started.
                    if started.load(std::sync::atomic::Ordering::Relaxed) {
                        let _ = tokio::time::timeout(Duration::from_secs(30), &mut poll).await;
                    }
                    break;
                }
                result = &mut poll => result,
            }
        };
        match result {
            Ok(active) => round_active |= active,
            Err(_) => tracing::warn!("Worker stage failed; durable state remains recoverable"),
        }
        let end_of_round = matches!(stage, Stage::AttachmentGc);
        stage = stage.next();
        if end_of_round {
            if !round_active {
                tokio::select! { _=&mut shutdown=>break, _=tokio::time::sleep(Duration::from_millis(cfg.worker.poll_interval_ms))=>{} }
            } else {
                tokio::task::yield_now().await;
            }
            round_active = false;
        }
    }
    worker.pool.close().await;
    tracing::info!("Worker stopped; uncaptured events and unfinished leases remain recoverable");
    Ok(())
}
