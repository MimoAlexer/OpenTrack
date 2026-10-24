#[allow(dead_code)]
#[path = "support/mod.rs"]
mod support;

use opentrack_app::worker::{Worker, WorkerOptions, WorkerTick};
use opentrack_domain::{Actor, CreateIssue, Issue, ReplaceStatus, WorkflowReplacement};
use serde_json::json;
use support::Harness;
use uuid::Uuid;

async fn setup(h: &Harness) -> (Actor, Uuid, Vec<Uuid>) {
    let (session, cookie) = h
        .app
        .setup(
            serde_json::from_value(json!({
                "setup_token":h.setup_token,"email":"discovery-owner@example.test",
                "password":"Discovery contract password 752!","display_name":"Example Owner",
                "workspace_name":"Discovery contracts","team_name":"Engineering","team_key":"ENG"
            }))
            .unwrap(),
        )
        .await
        .unwrap();
    let actor = h.app.authenticate(&cookie).await.unwrap();
    let organization = h.app.organizations(&actor).await.unwrap()[0].id;
    (actor, organization, vec![session.workspaces[0].id])
}

async fn grow(
    h: &Harness,
    actor: &Actor,
    organization: Uuid,
    scopes: &mut Vec<Uuid>,
    count: usize,
) {
    while scopes.len() < count {
        let index = scopes.len();
        let workspace = h.app.create_workspace(actor, &format!("discovery-workspace-{index}"),
            serde_json::from_value(json!({"organization_id":organization,
                "name":format!("Discovery workspace {index}"),"team_name":"Engineering","team_key":"ENG"})).unwrap()
        ).await.unwrap();
        scopes.push(workspace.id);
    }
    scopes.sort();
}

async fn assert_empty_sweep(worker: &Worker, cursor: &mut Option<Uuid>, remaining: &[Uuid]) {
    for page in remaining.chunks(20) {
        assert_eq!(worker.tick(cursor).await.unwrap(), WorkerTick::Scanning);
        assert_eq!(*cursor, page.last().copied());
    }
    let expected = if cursor.is_some() {
        WorkerTick::Scanning
    } else {
        WorkerTick::Idle
    };
    assert_eq!(worker.tick(cursor).await.unwrap(), expected);
    assert_eq!(*cursor, None);
}

#[tokio::test]
async fn empty_registry_backs_off_after_resetting_a_stale_cursor() {
    let h = Harness::new().await;
    let worker = Worker::connect(&h.worker_url().await, WorkerOptions::default())
        .await
        .unwrap();
    let mut cursor = None;
    assert_eq!(worker.tick(&mut cursor).await.unwrap(), WorkerTick::Idle);
    cursor = Some(Uuid::new_v4());
    assert_eq!(
        worker.tick(&mut cursor).await.unwrap(),
        WorkerTick::Scanning
    );
    assert_eq!(cursor, None);
    assert_eq!(worker.tick(&mut cursor).await.unwrap(), WorkerTick::Idle);
    worker.pool.close().await;
    h.cleanup().await;
}

async fn enqueue(h: &Harness, actor: &Actor, workspace: Uuid) -> (Issue, WorkflowReplacement) {
    let team = h.app.bootstrap(actor, workspace).await.unwrap().teams[0].id;
    let flow = h.app.workflow(actor, workspace, team).await.unwrap();
    let source = flow
        .statuses
        .iter()
        .find(|status| status.is_default)
        .unwrap()
        .id;
    let target = flow
        .statuses
        .iter()
        .find(|status| status.id != source)
        .unwrap()
        .id;
    let issue = h
        .app
        .create_issue(
            actor,
            workspace,
            &Uuid::new_v4().to_string(),
            serde_json::from_value::<CreateIssue>(
                json!({"team_id":team,"title":"Discovery queued issue"}),
            )
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(issue.status_id, source);
    let job = h
        .app
        .replace_status(
            actor,
            workspace,
            team,
            source,
            &Uuid::new_v4().to_string(),
            &flow.etag(),
            ReplaceStatus {
                replacement_status_id: target,
            },
        )
        .await
        .unwrap();
    assert_eq!(job.matched_count, "1");
    assert_eq!(job.state, "queued");
    (issue, job)
}

async fn assert_job(
    h: &Harness,
    actor: &Actor,
    workspace: Uuid,
    issue: &Issue,
    job: &WorkflowReplacement,
    completed: bool,
) {
    let current = h
        .app
        .get_workflow_replacement(actor, workspace, job.team_id, job.id)
        .await
        .unwrap();
    assert_eq!(
        current.state,
        if completed { "succeeded" } else { "queued" }
    );
    assert_eq!(current.processed_count, if completed { "1" } else { "0" });
    assert_eq!(current.remaining_count, if completed { "0" } else { "1" });
    let saved = h.app.get_issue(actor, workspace, issue.id).await.unwrap();
    assert_eq!(
        saved.status_id,
        if completed {
            job.replacement_status_id
        } else {
            issue.status_id
        }
    );
    assert_eq!(
        saved.version.parse::<u64>().unwrap(),
        issue.version.parse::<u64>().unwrap() + u64::from(completed)
    );
    let mut tx = h.context(actor.user.id, workspace).await;
    let counters: (i32, i32, bool) = sqlx::query_as(
        "SELECT attempt_count,retry_count,requested_by=$3 FROM jobs WHERE workspace_id=$1 AND id=$2",
    ).bind(workspace).bind(job.id).bind(actor.user.id).fetch_one(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    assert_eq!(counters, (i32::from(completed), 0, true));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn discovery_distinguishes_bounded_pages_from_exhausted_idle() {
    let h = Harness::new().await;
    let runner = h.clone();
    let result = tokio::spawn(async move {
        let worker = Worker::connect(&runner.worker_url().await, WorkerOptions::default())
            .await
            .unwrap();
        let mut cursor = None;
        assert_empty_sweep(&worker, &mut cursor, &[]).await;
        assert_empty_sweep(&worker, &mut cursor, &[]).await;
        let (actor, organization, mut scopes) = setup(&runner).await;
        for count in [19, 20, 21, 34, 79, 80, 81] {
            grow(&runner, &actor, organization, &mut scopes, count).await;
            let registered: Vec<Uuid> =
                sqlx::query_scalar("SELECT workspace_id FROM worker_ready_workspaces(NULL,100)")
                    .fetch_all(&worker.pool)
                    .await
                    .unwrap();
            assert_eq!(registered, scopes);
            assert_empty_sweep(&worker, &mut cursor, &scopes).await;
            assert_empty_sweep(&worker, &mut cursor, &scopes).await;
        }
        worker.pool.close().await;
    })
    .await;
    h.cleanup().await;
    if let Err(error) = result {
        std::panic::resume_unwind(error.into_panic());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn discovery_keeps_ahead_and_behind_jobs_through_finite_growth() {
    let h = Harness::new().await;
    let runner = h.clone();
    let result = tokio::spawn(async move {
        let (actor, organization, mut scopes) = setup(&runner).await;
        grow(&runner, &actor, organization, &mut scopes, 34).await;
        let worker = Worker::connect(&runner.worker_url().await, WorkerOptions::default())
            .await
            .unwrap();
        let mut cursor = None;
        assert_eq!(
            worker.tick(&mut cursor).await.unwrap(),
            WorkerTick::Scanning
        );
        assert_eq!(cursor, Some(scopes[19]));
        let ahead_scope = scopes[20];
        let behind_scope = scopes[0];
        let (ahead_issue, ahead_job) = enqueue(&runner, &actor, ahead_scope).await;
        let (behind_issue, behind_job) = enqueue(&runner, &actor, behind_scope).await;
        assert_job(
            &runner,
            &actor,
            ahead_scope,
            &ahead_issue,
            &ahead_job,
            false,
        )
        .await;
        assert_job(
            &runner,
            &actor,
            behind_scope,
            &behind_issue,
            &behind_job,
            false,
        )
        .await;
        assert_eq!(
            worker.tick(&mut cursor).await.unwrap(),
            WorkerTick::Completed
        );
        assert_eq!(cursor, Some(ahead_scope));
        assert_job(&runner, &actor, ahead_scope, &ahead_issue, &ahead_job, true).await;
        assert_job(
            &runner,
            &actor,
            behind_scope,
            &behind_issue,
            &behind_job,
            false,
        )
        .await;
        assert_empty_sweep(&worker, &mut cursor, &scopes[21..]).await;
        assert_job(
            &runner,
            &actor,
            behind_scope,
            &behind_issue,
            &behind_job,
            false,
        )
        .await;
        assert_eq!(
            worker.tick(&mut cursor).await.unwrap(),
            WorkerTick::Completed
        );
        assert_eq!(cursor, Some(behind_scope));
        assert_job(
            &runner,
            &actor,
            behind_scope,
            &behind_issue,
            &behind_job,
            true,
        )
        .await;
        assert_empty_sweep(&worker, &mut cursor, &scopes[1..]).await;

        assert_eq!(
            worker.tick(&mut cursor).await.unwrap(),
            WorkerTick::Scanning
        );
        assert_eq!(cursor, Some(scopes[19]));
        let boundary = cursor.unwrap();
        let waiting_scope = scopes[1];
        let (waiting_issue, waiting_job) = enqueue(&runner, &actor, waiting_scope).await;
        grow(&runner, &actor, organization, &mut scopes, 37).await;
        assert_eq!(cursor, Some(boundary));
        let remaining: Vec<Uuid> = scopes
            .iter()
            .copied()
            .filter(|scope| *scope > boundary)
            .collect();
        assert_empty_sweep(&worker, &mut cursor, &remaining).await;
        assert_job(
            &runner,
            &actor,
            waiting_scope,
            &waiting_issue,
            &waiting_job,
            false,
        )
        .await;
        assert_eq!(
            worker.tick(&mut cursor).await.unwrap(),
            WorkerTick::Completed
        );
        assert_eq!(cursor, Some(waiting_scope));
        assert_job(
            &runner,
            &actor,
            waiting_scope,
            &waiting_issue,
            &waiting_job,
            true,
        )
        .await;
        let remaining: Vec<Uuid> = scopes
            .iter()
            .copied()
            .filter(|scope| *scope > waiting_scope)
            .collect();
        assert_empty_sweep(&worker, &mut cursor, &remaining).await;
        assert_job(&runner, &actor, ahead_scope, &ahead_issue, &ahead_job, true).await;
        assert_job(
            &runner,
            &actor,
            behind_scope,
            &behind_issue,
            &behind_job,
            true,
        )
        .await;
        worker.pool.close().await;
    })
    .await;
    h.cleanup().await;
    if let Err(error) = result {
        std::panic::resume_unwind(error.into_panic());
    }
}
