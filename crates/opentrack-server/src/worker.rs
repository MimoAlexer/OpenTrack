use crate::config::{Cli, Settings};
use opentrack_app::worker::{Worker, WorkerOptions, WorkerTick};
use std::time::{Duration, Instant};

const JOB_QUANTUM: u8 = 4;

#[derive(Default)]
struct JobDiscovery {
    credits: u8,
    not_before: Option<Instant>,
    repeat_remaining: u8,
}
impl JobDiscovery {
    fn begin(&mut self, now: Instant) -> bool {
        if let Some(deadline) = self.not_before {
            if now < deadline {
                return false;
            }
            self.not_before = None;
            self.credits = 0;
        }
        self.credits += 1;
        true
    }

    fn repeat_workspace(&mut self, workspace: Option<uuid::Uuid>) -> Option<uuid::Uuid> {
        if self.repeat_remaining == 0 || workspace.is_none() {
            self.repeat_remaining = 0;
            return None;
        }
        self.repeat_remaining -= 1;
        workspace
    }

    fn finish(
        &mut self,
        now: Instant,
        poll_interval: Duration,
        result: &opentrack_domain::Result<WorkerTick>,
        repeated: bool,
    ) -> bool {
        match result {
            Ok(
                WorkerTick::Progress
                | WorkerTick::Completed
                | WorkerTick::Blocked
                | WorkerTick::Retry,
            ) => {
                self.credits = 0;
                self.not_before = None;
                if !repeated {
                    self.repeat_remaining = JOB_QUANTUM - 1;
                }
                true
            }
            Ok(WorkerTick::Idle | WorkerTick::Scanning | WorkerTick::LostLease) | Err(_) => {
                self.repeat_remaining = 0;
                if matches!(result, Ok(WorkerTick::Idle)) || self.credits >= 4 {
                    self.not_before = Some(now + poll_interval);
                }
                matches!(result, Ok(WorkerTick::Scanning)) && self.not_before.is_none()
            }
        }
    }

    fn idle_wait(&self, now: Instant, poll_interval: Duration) -> Duration {
        self.not_before.map_or(poll_interval, |deadline| {
            deadline.saturating_duration_since(now).min(poll_interval)
        })
    }
}

#[derive(Default)]
struct Cursors {
    capture: Option<uuid::Uuid>,
    job: Option<uuid::Uuid>,
    job_discovery: JobDiscovery,
    retention: Option<uuid::Uuid>,
    document_retention: Option<uuid::Uuid>,
    attachment_gc: Option<uuid::Uuid>,
}

#[derive(Clone, Copy, Debug)]
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
    poll_interval: Duration,
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
        Stage::Job => {
            if !cursors.job_discovery.begin(Instant::now()) {
                return Ok(false);
            }
            let repeated = cursors.job_discovery.repeat_workspace(cursors.job);
            let result = match repeated {
                Some(workspace) => match worker.claim(workspace).await {
                    Ok(Some(claim)) => worker.run_claim(&claim).await,
                    Ok(None) => Ok(WorkerTick::Scanning),
                    Err(error) => Err(error),
                },
                None => worker.tick(&mut cursors.job).await,
            };
            let active = cursors.job_discovery.finish(
                Instant::now(),
                poll_interval,
                &result,
                repeated.is_some(),
            );
            result.map(|_| active)
        }
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
    let history_current = opentrack_store::schema_is_current(&worker.pool)
        .await
        .map_err(|_| "Cannot verify worker migration history; apply matching worker grants")?;
    if !history_current {
        worker.pool.close().await;
        return Err("Database migration history is incompatible with this executable".into());
    }
    let current_schema: bool =
        sqlx::query_scalar("SELECT to_regclass('consumer_receipts') IS NOT NULL AND to_regclass('document_revisions') IS NOT NULL AND to_regclass('attachments') IS NOT NULL AND to_regclass('issue_templates') IS NOT NULL AND EXISTS(SELECT 1 FROM pg_attribute WHERE attrelid=to_regclass('issue_templates') AND attname='title_mode' AND NOT attisdropped) AND to_regclass('issue_template_sources') IS NOT NULL AND to_regclass('issue_subscriptions') IS NOT NULL AND to_regclass('document_mentions') IS NOT NULL AND to_regclass('mention_intents') IS NOT NULL AND to_regclass('subscription_events') IS NOT NULL AND to_regclass('discussion_sources') IS NOT NULL AND to_regclass('user_preferences') IS NOT NULL AND to_regclass('user_preference_events') IS NOT NULL AND to_regclass('projects') IS NOT NULL AND to_regclass('project_issues') IS NOT NULL AND to_regclass('project_updates') IS NOT NULL AND to_regclass('milestones') IS NOT NULL AND to_regclass('cycles') IS NOT NULL AND to_regclass('cycle_issues') IS NOT NULL AND to_regclass('cycle_snapshots') IS NOT NULL AND to_regclass('cycle_close_reports') IS NOT NULL AND to_regclass('cycle_carry_operations') IS NOT NULL AND to_regclass('cycle_reopen_reasons') IS NOT NULL AND EXISTS(SELECT 1 FROM pg_attribute WHERE attrelid=to_regclass('project_issues') AND attname='milestone_id' AND NOT attisdropped)")
            .fetch_one(&worker.pool)
            .await
            .map_err(|_| "Cannot inspect the worker schema")?;
    let admission_ready = opentrack_store::event_admission_ready(&worker.pool)
        .await
        .map_err(|_| "Cannot check event admission schema")?;
    if !current_schema || !admission_ready {
        worker.pool.close().await;
        return Err(
            "Apply current document and event-consumer migrations before starting this worker version".into(),
        );
    }
    let blobs = crate::storage::open(cfg).await?;
    let poll_interval = Duration::from_millis(cfg.worker.poll_interval_ms);
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
                poll_stage(&worker, &mut cursors, stage, &blobs, poll_interval).await?;
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
    tracing::info!(
        poll_interval_ms = cfg.worker.poll_interval_ms,
        max_connections = cfg.worker.max_connections,
        lease_seconds = cfg.worker.lease_seconds,
        "Worker started with isolated database authority"
    );
    let shutdown = crate::shutdown();
    tokio::pin!(shutdown);
    let mut stage = Stage::Capture;
    let mut round_active = false;
    loop {
        let stage_started = std::time::Instant::now();
        let result = {
            let started = std::sync::atomic::AtomicBool::new(false);
            let poll = async {
                started.store(true, std::sync::atomic::Ordering::Relaxed);
                poll_stage(&worker, &mut cursors, stage, &blobs, poll_interval).await
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
        tracing::debug!(
            stage = ?stage,
            elapsed_ms = stage_started.elapsed().as_secs_f64() * 1000.0,
            worked = matches!(&result, Ok(true)),
            failed = result.is_err(),
            "Worker stage completed"
        );
        match result {
            Ok(active) => round_active |= active,
            Err(_) => tracing::warn!("Worker stage failed; durable state remains recoverable"),
        }
        let end_of_round = matches!(stage, Stage::AttachmentGc);
        stage = stage.next();
        if end_of_round {
            if !round_active {
                let idle_for = cursors
                    .job_discovery
                    .idle_wait(Instant::now(), poll_interval);
                if idle_for.is_zero() {
                    tokio::task::yield_now().await;
                } else {
                    let idle_started = std::time::Instant::now();
                    tracing::debug!(
                        poll_interval_ms = cfg.worker.poll_interval_ms,
                        "Worker idle wait started"
                    );
                    tokio::select! { _=&mut shutdown=>break, _=tokio::time::sleep(idle_for)=>{} }
                    tracing::debug!(
                        elapsed_ms = idle_started.elapsed().as_secs_f64() * 1000.0,
                        "Worker idle wait completed"
                    );
                }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_handled_batches_never_refill_the_workspace_quantum() {
        let now = Instant::now();
        let poll = Duration::from_secs(2);
        let workspace = Some(uuid::Uuid::new_v4());
        for outcome in [
            WorkerTick::Progress,
            WorkerTick::Completed,
            WorkerTick::Blocked,
            WorkerTick::Retry,
        ] {
            let mut discovery = JobDiscovery::default();
            let result = Ok(outcome);
            assert!(discovery.begin(now));
            assert!(discovery.finish(now, poll, &result, false));
            for _ in 0..3 {
                assert!(discovery.begin(now));
                assert_eq!(discovery.repeat_workspace(workspace), workspace);
                assert!(discovery.finish(now, poll, &result, true));
            }
            assert_eq!(discovery.repeat_workspace(workspace), None);
        }
    }

    #[test]
    fn unhandled_repeat_clears_the_quantum_without_refunding_discovery_credit() {
        let now = Instant::now();
        let poll = Duration::from_secs(2);
        let workspace = Some(uuid::Uuid::new_v4());
        for result in [
            Ok(WorkerTick::Scanning),
            Ok(WorkerTick::Idle),
            Ok(WorkerTick::LostLease),
            Err(opentrack_domain::Error::Unavailable),
            Err(opentrack_domain::Error::Internal),
        ] {
            let mut discovery = JobDiscovery::default();
            assert!(discovery.begin(now));
            assert!(discovery.finish(now, poll, &Ok(WorkerTick::Progress), false));
            assert!(discovery.begin(now));
            assert_eq!(discovery.repeat_workspace(workspace), workspace);
            discovery.finish(now, poll, &result, true);
            assert_eq!(discovery.repeat_workspace(workspace), None);
            assert_eq!(discovery.credits, 1);
        }
    }

    #[test]
    fn missing_workspace_clears_the_unused_quantum() {
        let now = Instant::now();
        let mut discovery = JobDiscovery::default();
        assert!(discovery.begin(now));
        assert!(discovery.finish(
            now,
            Duration::from_secs(2),
            &Ok(WorkerTick::Progress),
            false
        ));
        assert_eq!(discovery.repeat_workspace(None), None);
        assert_eq!(discovery.repeat_workspace(Some(uuid::Uuid::new_v4())), None);
    }

    #[test]
    fn four_no_claim_pages_pause_until_the_job_deadline() {
        let now = Instant::now();
        let poll = Duration::from_secs(2);
        let mut discovery = JobDiscovery::default();
        for page in 0..4 {
            assert!(discovery.begin(now));
            assert_eq!(
                discovery.finish(now, poll, &Ok(WorkerTick::Scanning), false),
                page < 3
            );
        }
        assert!(!discovery.begin(now + poll - Duration::from_nanos(1)));
        assert_eq!(discovery.idle_wait(now + poll / 2, poll), poll / 2);
        assert!(discovery.begin(now + poll));
        assert!(discovery.finish(now + poll, poll, &Ok(WorkerTick::Scanning), false));
    }

    #[test]
    fn errors_and_lost_leases_cannot_refund_discovery_credit() {
        let now = Instant::now();
        let poll = Duration::from_secs(2);
        let mut discovery = JobDiscovery::default();
        for result in [
            Err(opentrack_domain::Error::Unavailable),
            Ok(WorkerTick::LostLease),
            Err(opentrack_domain::Error::Internal),
            Ok(WorkerTick::Scanning),
        ] {
            assert!(discovery.begin(now));
            assert!(!discovery.finish(now, poll, &result, false));
        }
        assert!(!discovery.begin(now));
        assert!(!discovery.begin(now + poll / 2));
        assert!(discovery.begin(now + poll));
    }

    #[test]
    fn handled_claims_restore_the_next_full_burst() {
        let now = Instant::now();
        let poll = Duration::from_secs(2);
        for outcome in [
            WorkerTick::Progress,
            WorkerTick::Completed,
            WorkerTick::Blocked,
            WorkerTick::Retry,
        ] {
            let mut discovery = JobDiscovery::default();
            for _ in 0..3 {
                assert!(discovery.begin(now));
                assert!(discovery.finish(now, poll, &Ok(WorkerTick::Scanning), false));
            }
            assert!(discovery.begin(now));
            assert!(discovery.finish(now, poll, &Ok(outcome), false));
            for page in 0..4 {
                assert!(discovery.begin(now));
                assert_eq!(
                    discovery.finish(now, poll, &Ok(WorkerTick::Scanning), false),
                    page < 3
                );
            }
            assert!(!discovery.begin(now));
        }
    }

    #[test]
    fn slow_fourth_call_still_receives_a_full_cooldown_after_completion() {
        let start = Instant::now();
        let poll = Duration::from_secs(2);
        let mut discovery = JobDiscovery::default();
        for _ in 0..3 {
            assert!(discovery.begin(start));
            assert!(discovery.finish(start, poll, &Ok(WorkerTick::Scanning), false));
        }
        assert!(discovery.begin(start));
        let completed = start + poll * 3;
        assert!(!discovery.finish(
            completed,
            poll,
            &Err(opentrack_domain::Error::Unavailable),
            false
        ));
        assert!(!discovery.begin(completed + poll - Duration::from_nanos(1)));
        assert!(discovery.begin(completed + poll));
    }

    #[test]
    fn exhausted_wrap_rearms_cooldown_and_bounds_the_outer_idle_wait() {
        let now = Instant::now();
        let poll = Duration::from_secs(2);
        let mut discovery = JobDiscovery::default();
        for _ in 0..4 {
            assert!(discovery.begin(now));
            discovery.finish(now, poll, &Ok(WorkerTick::Scanning), false);
        }
        assert!(discovery.begin(now + poll));
        assert!(!discovery.finish(now + poll, poll, &Ok(WorkerTick::Idle), false));
        assert!(!discovery.begin(now + poll + Duration::from_nanos(1)));
        assert_eq!(discovery.idle_wait(now + poll, poll), poll);
        let short_wait = Duration::from_millis(10);
        assert_eq!(discovery.idle_wait(now + poll, short_wait), short_wait);
        assert!(discovery.idle_wait(now + poll * 2, poll).is_zero());
        assert!(discovery.begin(now + poll * 2));
    }
}
