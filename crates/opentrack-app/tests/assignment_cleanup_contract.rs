//! Real restricted-worker capture, current eligibility and bounded assignment effects.
#[allow(dead_code)]
mod support;

use opentrack_app::{
    Operator,
    worker::{Worker, WorkerOptions, WorkerTick},
};
use opentrack_domain::*;
use opentrack_store as store;
use serde_json::{Value, json};
use sqlx::AssertSqlSafe;
use support::Harness;
use uuid::Uuid;

const PASSWORD: &str = "Assignment cleanup contract 876!";

fn req<T: serde::de::DeserializeOwned>(value: Value) -> T {
    serde_json::from_value(value).unwrap()
}

async fn setup(h: &Harness) -> (Actor, Uuid, Uuid) {
    let (session, cookie) = h.app.setup(req(json!({
        "setup_token":h.setup_token,"email":"owner@cleanup.invalid","password":PASSWORD,
        "display_name":"Cleanup owner","workspace_name":"Cleanup","team_name":"Public","team_key":"PUB"
    }))).await.unwrap();
    let actor = h.app.authenticate(&cookie).await.unwrap();
    let workspace = session.workspaces[0].id;
    let team = h.app.bootstrap(&actor, workspace).await.unwrap().teams[0].id;
    (actor, workspace, team)
}

async fn person(h: &Harness, actor: &Actor, workspace: Uuid) -> Actor {
    let id = Uuid::new_v4();
    let email = format!("{id}@cleanup.invalid");
    let mut tx = h.context(actor.user.id, workspace).await;
    sqlx::query("INSERT INTO users(id,email,display_name,password_hash) SELECT $1,$2,'Cleanup member',password_hash FROM users WHERE id=$3")
        .bind(id).bind(&email).bind(actor.user.id).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO workspace_members(workspace_id,user_id,role) VALUES($1,$2,'member')")
        .bind(workspace)
        .bind(id)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    login(h, &email).await
}

async fn login(h: &Harness, email: &str) -> Actor {
    let (_, cookie) = h
        .app
        .login(req(json!({"email":email,"password":PASSWORD})))
        .await
        .unwrap();
    h.app.authenticate(&cookie).await.unwrap()
}

async fn team(h: &Harness, actor: &Actor, workspace: Uuid, private: bool) -> Uuid {
    let key = Uuid::new_v4().simple().to_string();
    h.app
        .create_team(
            actor,
            workspace,
            &key,
            req(json!({
                "name":format!("Cleanup {key}"),"key":format!("C{}",&key[..6]),"private":private
            })),
        )
        .await
        .unwrap()
        .id
}

async fn grant(h: &Harness, actor: &Actor, workspace: Uuid, team: Uuid, user: Uuid) {
    h.app
        .put_team_member(
            actor,
            workspace,
            team,
            user,
            None,
            req(json!({"role":"contributor"})),
        )
        .await
        .unwrap();
}

async fn revoke(h: &Harness, actor: &Actor, workspace: Uuid, team: Uuid, user: Uuid) {
    let member = h
        .app
        .get_team_member(actor, workspace, team, user)
        .await
        .unwrap();
    h.app
        .remove_team_member(actor, workspace, team, user, &member.etag(team))
        .await
        .unwrap();
}

async fn deactivate(h: &Harness, actor: &Actor, workspace: Uuid, user: Uuid) {
    let member = h
        .app
        .get_workspace_member(actor, workspace, user)
        .await
        .unwrap();
    h.app
        .update_member(
            actor,
            workspace,
            user,
            &member.etag(workspace),
            req(json!({"active":false})),
        )
        .await
        .unwrap();
}

async fn issue(h: &Harness, actor: &Actor, workspace: Uuid, team: Uuid, extra: Value) -> Issue {
    let mut body = json!({"team_id":team,"title":format!("Cleanup issue {}",Uuid::new_v4())});
    body.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    h.app
        .create_issue(actor, workspace, &Uuid::new_v4().to_string(), req(body))
        .await
        .unwrap()
}

async fn patch(h: &Harness, actor: &Actor, workspace: Uuid, id: Uuid, body: Value) -> Issue {
    let current = h.app.get_issue(actor, workspace, id).await.unwrap();
    h.app
        .update_issue(actor, workspace, id, &current.etag(), req(body))
        .await
        .unwrap()
}

async fn status(h: &Harness, actor: &Actor, workspace: Uuid, team: Uuid, category: &str) -> Uuid {
    h.app
        .workflow(actor, workspace, team)
        .await
        .unwrap()
        .statuses
        .into_iter()
        .find(|status| status.category == category)
        .unwrap()
        .id
}

async fn seed(
    h: &Harness,
    actor: &Actor,
    workspace: Uuid,
    team: Uuid,
    user: Uuid,
    count: i32,
) -> Vec<Uuid> {
    let mut tx = h.context(actor.user.id, workspace).await;
    let start: i64 = sqlx::query_scalar(
        "SELECT next_issue_number FROM teams WHERE workspace_id=$1 AND id=$2 FOR UPDATE",
    )
    .bind(workspace)
    .bind(team)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    let ids: Vec<Uuid> = sqlx::query_scalar("INSERT INTO issues(id,workspace_id,team_id,created_by_user_id,number,title,status_id,assignee_id) SELECT gen_random_uuid(),$1,$2,$3,$5+n,'Bounded cleanup fixture',s.id,$4 FROM generate_series(0,$6-1) n CROSS JOIN workflow_statuses s WHERE s.workspace_id=$1 AND s.team_id=$2 AND s.is_default RETURNING id")
        .bind(workspace).bind(team).bind(actor.user.id).bind(user).bind(start).bind(count)
        .fetch_all(&mut *tx).await.unwrap();
    sqlx::query(
        "UPDATE teams SET next_issue_number=next_issue_number+$3 WHERE workspace_id=$1 AND id=$2",
    )
    .bind(workspace)
    .bind(team)
    .bind(i64::from(count))
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(ids.len(), count as usize);
    ids
}

async fn rows(h: &Harness, actor: &Actor, workspace: Uuid, ids: &[Uuid]) -> Vec<Value> {
    let mut tx = h.context(actor.user.id, workspace).await;
    let rows = sqlx::query_scalar(
        "SELECT to_jsonb(i) FROM issues i WHERE workspace_id=$1 AND id=ANY($2) ORDER BY id",
    )
    .bind(workspace)
    .bind(ids)
    .fetch_all(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    rows
}

async fn assigned(h: &Harness, actor: &Actor, workspace: Uuid, ids: &[Uuid]) -> usize {
    rows(h, actor, workspace, ids)
        .await
        .iter()
        .filter(|row| !row["assignee_id"].is_null())
        .count()
}

async fn worker(h: &Harness) -> Worker {
    Worker::connect(&h.worker_url().await, WorkerOptions::default())
        .await
        .unwrap()
}

async fn settle(worker: &Worker, workspace: Uuid) -> usize {
    let mut batches = 0;
    for _ in 0..100 {
        let capture = worker.capture(workspace).await.unwrap();
        assert!(!capture.dead_lettered, "Unexpected consumer dead letter");
        let Some(claim) = worker.claim(workspace).await.unwrap() else {
            if capture.processed == 100 {
                continue;
            }
            return batches;
        };
        let result = worker.run_claim(&claim).await.unwrap();
        assert!(
            matches!(result, WorkerTick::Progress | WorkerTick::Completed),
            "Unexpected cleanup outcome: {result:?}"
        );
        batches += 1;
    }
    panic!("Bounded fixture failed to drain after 100 batches");
}

async fn fault(h: &Harness, table: &str, event: &str, body: &str) {
    // Every SQL fragment is a fixed test constant; no user input reaches DDL.
    sqlx::raw_sql(AssertSqlSafe(format!("CREATE FUNCTION cleanup_fault() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN {body}; RETURN NEW; END $$; CREATE TRIGGER cleanup_fault BEFORE {event} ON {table} FOR EACH ROW EXECUTE FUNCTION cleanup_fault()")))
        .execute(&h.owner).await.unwrap();
}

async fn clear_fault(h: &Harness, table: &str) {
    sqlx::raw_sql(AssertSqlSafe(format!(
        "DROP TRIGGER cleanup_fault ON {table}; DROP FUNCTION cleanup_fault()"
    )))
    .execute(&h.owner)
    .await
    .unwrap();
}

async fn effect_snapshot(h: &Harness, owner: &Actor, workspace: Uuid) -> Value {
    let mut tx = h.context(owner.user.id, workspace).await;
    let snapshot = sqlx::query_scalar("SELECT jsonb_build_object(
        'issues',(SELECT coalesce(jsonb_agg(to_jsonb(i) ORDER BY id),'[]') FROM issues i WHERE workspace_id=$1),
        'progress',(SELECT coalesce(jsonb_agg(to_jsonb(p) ORDER BY id),'[]') FROM assignment_reconciliations p WHERE workspace_id=$1),
        'activity',(SELECT coalesce(jsonb_agg(to_jsonb(a) ORDER BY id),'[]') FROM activity_entries a WHERE workspace_id=$1),
        'audit',(SELECT coalesce(jsonb_agg(to_jsonb(a) ORDER BY id),'[]') FROM audit_entries a WHERE workspace_id=$1),
        'events',(SELECT coalesce(jsonb_agg(to_jsonb(e) ORDER BY seq),'[]') FROM workspace_events e WHERE workspace_id=$1),
        'head',(SELECT last_seq FROM workspace_stream_heads WHERE workspace_id=$1))")
        .bind(workspace).fetch_one(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    snapshot
}

async fn system_effects(h: &Harness, owner: &Actor, workspace: Uuid, ids: &[Uuid], previous: Uuid) {
    let mut tx = h.context(owner.user.id, workspace).await;
    let result: Value = sqlx::query_scalar("SELECT jsonb_build_object(
        'activity',(SELECT count(*) FROM activity_entries WHERE workspace_id=$1 AND entity_id=ANY($2) AND actor_kind='system'),
        'audit',(SELECT count(*) FROM audit_entries WHERE workspace_id=$1 AND target_id=ANY($2) AND actor_kind='system'),
        'events',(SELECT count(*) FROM workspace_events WHERE workspace_id=$1 AND entity_id=ANY($2) AND actor_kind='system'),
        'joined',(SELECT count(*) FROM activity_entries a
          JOIN audit_entries au ON au.workspace_id=a.workspace_id AND au.target_id=a.entity_id AND au.system_job_id=a.system_job_id
          JOIN workspace_events e ON e.workspace_id=a.workspace_id AND e.entity_id=a.entity_id AND e.system_job_id=a.system_job_id
          JOIN jobs j ON j.workspace_id=a.workspace_id AND j.id=a.system_job_id
          WHERE a.workspace_id=$1 AND a.entity_id=ANY($2) AND a.actor_kind='system' AND a.actor_id IS NULL
          AND a.event_kind='issue.updated' AND a.changes->>'source'='access_cleanup'
          AND a.changes->'assignee_id'->>'before'=$3::text AND a.changes->'assignee_id'->'after'='null'::jsonb
          AND a.changes->'fields'='[\"assignee_id\"]'::jsonb AND (a.changes->>'job_id')::uuid=j.id
          AND au.actor_kind='system' AND au.actor_id IS NULL AND au.action='issue.assignment_cleared' AND au.correlation_id=j.id
          AND e.actor_kind='system' AND e.actor_id IS NULL AND e.event_type='issue.updated' AND e.mutation_id=j.id
          AND j.kind='assignment.reconcile' AND j.requested_by IS NULL))")
        .bind(workspace).bind(ids).bind(previous).fetch_one(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    for key in ["activity", "audit", "events", "joined"] {
        assert_eq!(
            result[key],
            ids.len(),
            "Missing, duplicated or falsely attributed {key}"
        );
    }
    let history = h
        .app
        .issue_activity(owner, workspace, ids[0])
        .await
        .unwrap();
    let effect = history
        .iter()
        .find(|entry| entry["changes"]["source"] == "access_cleanup")
        .unwrap();
    assert_eq!(effect["actor"], "OpenTrack");
}

async fn wait_for_blocker(h: &Harness, blocker: i32) {
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            let observed: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE $1=ANY(pg_blocking_pids(pid)))",
            )
            .bind(blocker)
            .fetch_one(&h.owner)
            .await
            .unwrap();
            if observed {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("Observe cleanup waiting on the actual PostgreSQL row lock");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_postgres_assignment_cleanup_contract() {
    let h = Harness::with_connections(8).await;
    let copy = h.clone();
    let run = tokio::spawn(async move { exercise(&copy).await }).await;
    h.cleanup().await;
    if let Err(error) = run {
        std::panic::resume_unwind(error.into_panic());
    }
}

async fn exercise(h: &Harness) {
    let (owner, workspace, public) = setup(h).await;
    let recipient = person(h, &owner, workspace).await;
    let private = team(h, &owner, workspace, true).await;
    grant(h, &owner, workspace, private, recipient.user.id).await;
    let recipient = login(h, &recipient.user.email).await;
    let worker = worker(h).await;
    let competitor = Worker::connect(&h.worker_url().await, WorkerOptions::default())
        .await
        .unwrap();
    settle(&worker, workspace).await;

    let done = status(h, &owner, workspace, private, "completed").await;
    let canceled = status(h, &owner, workspace, private, "canceled").await;
    let open = status(h, &owner, workspace, private, "unstarted").await;
    let completed = issue(
        h,
        &owner,
        workspace,
        private,
        json!({"assignee_id":recipient.user.id,"status_id":done}),
    )
    .await;
    let canceled_issue = issue(
        h,
        &owner,
        workspace,
        private,
        json!({"assignee_id":recipient.user.id,"status_id":canceled}),
    )
    .await;
    let archived = issue(
        h,
        &owner,
        workspace,
        private,
        json!({"assignee_id":recipient.user.id}),
    )
    .await;
    let archived = patch(h, &owner, workspace, archived.id, json!({"archived":true})).await;
    let deleted = issue(
        h,
        &owner,
        workspace,
        private,
        json!({"assignee_id":recipient.user.id}),
    )
    .await;
    h.app
        .delete_issue(&owner, workspace, deleted.id, &deleted.etag())
        .await
        .unwrap();
    let historical = [completed.id, canceled_issue.id, archived.id, deleted.id];
    let historical_before = rows(h, &owner, workspace, &historical).await;
    let active = seed(h, &owner, workspace, private, recipient.user.id, 205).await;
    settle(&worker, workspace).await;
    assert_eq!(assigned(h, &owner, workspace, &active).await, 205);

    revoke(h, &owner, workspace, private, recipient.user.id).await;
    assert!(matches!(
        h.app.get_issue(&recipient, workspace, active[0]).await,
        Err(Error::NotFound)
    ));
    assert_eq!(
        assigned(h, &owner, workspace, &active).await,
        205,
        "Access denial must precede asynchronous cleanup"
    );
    let (left, right) = tokio::join!(worker.capture(workspace), competitor.capture(workspace));
    left.unwrap();
    right.unwrap();
    let mut previous = 205;
    let mut changed_batches = 0;
    for _ in 0..20 {
        let Some(claim) = worker.claim(workspace).await.unwrap() else {
            break;
        };
        assert_eq!(
            claim.requested_by, None,
            "Maintenance must not impersonate a revoked member"
        );
        assert!(competitor.claim(workspace).await.unwrap().is_none());
        let result = worker.run_claim(&claim).await.unwrap();
        assert!(matches!(
            result,
            WorkerTick::Progress | WorkerTick::Completed
        ));
        let remaining = assigned(h, &owner, workspace, &active).await;
        assert!(
            previous - remaining <= 100,
            "One commit exceeded its 100-issue bound"
        );
        if remaining < previous {
            changed_batches += 1;
        }
        previous = remaining;
        let after = rows(h, &owner, workspace, &active).await;
        assert_eq!(
            worker.run_claim(&claim).await.unwrap(),
            WorkerTick::LostLease
        );
        assert_eq!(
            rows(h, &owner, workspace, &active).await,
            after,
            "Old fence replay changed assignments"
        );
    }
    assert_eq!(previous, 0);
    assert!(changed_batches >= 3);
    assert_eq!(
        rows(h, &owner, workspace, &historical).await,
        historical_before,
        "Historical assignments must remain intact"
    );
    for row in rows(h, &owner, workspace, &active).await {
        assert_eq!(row["version"], 2);
    }
    system_effects(h, &owner, workspace, &active, recipient.user.id).await;
    let after = rows(h, &owner, workspace, &active).await;
    settle(&worker, workspace).await;
    settle(&competitor, workspace).await;
    assert_eq!(
        rows(h, &owner, workspace, &active).await,
        after,
        "Repeated capture redelivered an effect"
    );

    // An excluded closed assignment can be edited, but becoming open requires repair.
    let closed = patch(
        h,
        &owner,
        workspace,
        completed.id,
        json!({"title":"Historical assignment retained"}),
    )
    .await;
    assert_eq!(closed.assignee_id, Some(recipient.user.id));
    assert!(matches!(
        h.app
            .update_issue(
                &owner,
                workspace,
                closed.id,
                &closed.etag(),
                req(json!({"status_id":open}))
            )
            .await,
        Err(Error::Invalid(_))
    ));
    let repaired = patch(
        h,
        &owner,
        workspace,
        closed.id,
        json!({"status_id":open,"assignee_id":null}),
    )
    .await;
    assert_eq!(repaired.assignee_id, None);
    let archived = h
        .app
        .get_issue(&owner, workspace, archived.id)
        .await
        .unwrap();
    assert!(matches!(
        h.app
            .update_issue(
                &owner,
                workspace,
                archived.id,
                &archived.etag(),
                req(json!({"archived":false}))
            )
            .await,
        Err(Error::Invalid(_))
    ));
    assert_eq!(
        patch(
            h,
            &owner,
            workspace,
            archived.id,
            json!({"archived":false,"assignee_id":null})
        )
        .await
        .assignee_id,
        None
    );
    let tombstone = h
        .app
        .get_deleted_issue(&owner, workspace, deleted.id)
        .await
        .unwrap();
    assert!(matches!(
        h.app
            .restore_issue(
                &owner,
                workspace,
                deleted.id,
                &tombstone.issue.etag(),
                req(json!({"status_id":open,"assignee_id":recipient.user.id,"label_ids":[]}))
            )
            .await,
        Err(Error::Invalid(_))
    ));
    assert_eq!(
        h.app
            .restore_issue(
                &owner,
                workspace,
                deleted.id,
                &tombstone.issue.etag(),
                req(json!({"status_id":open,"assignee_id":null,"label_ids":[]}))
            )
            .await
            .unwrap()
            .assignee_id,
        None
    );

    // A queued loss is only a reason to inspect: fresh eligibility wins at effect time.
    grant(h, &owner, workspace, private, recipient.user.id).await;
    let regranted = issue(
        h,
        &owner,
        workspace,
        private,
        json!({"assignee_id":recipient.user.id}),
    )
    .await;
    settle(&worker, workspace).await;
    revoke(h, &owner, workspace, private, recipient.user.id).await;
    worker.capture(workspace).await.unwrap();
    grant(h, &owner, workspace, private, recipient.user.id).await;
    settle(&worker, workspace).await;
    let latest = h
        .app
        .get_issue(&owner, workspace, regranted.id)
        .await
        .unwrap();
    assert_eq!(
        (latest.assignee_id, &latest.version),
        (Some(recipient.user.id), &regranted.version)
    );

    revoke(h, &owner, workspace, private, recipient.user.id).await;
    worker.capture(workspace).await.unwrap();
    let reassigned = patch(
        h,
        &owner,
        workspace,
        regranted.id,
        json!({"assignee_id":owner.user.id}),
    )
    .await;
    settle(&worker, workspace).await;
    let latest = h
        .app
        .get_issue(&owner, workspace, reassigned.id)
        .await
        .unwrap();
    assert_eq!(
        (latest.assignee_id, latest.version),
        (Some(owner.user.id), reassigned.version)
    );

    // A global operator event has no human requester; cleanup still executes.
    let disabled = person(h, &owner, workspace).await;
    let disabled_issue = issue(
        h,
        &owner,
        workspace,
        public,
        json!({"assignee_id":disabled.user.id}),
    )
    .await;
    settle(&worker, workspace).await;
    let operator = Operator::connect(&h.operator_url().await).await.unwrap();
    operator
        .disable_account(disabled.user.id, "Assignment contract disable")
        .await
        .unwrap();
    assert!(matches!(
        h.app
            .get_issue(&disabled, workspace, disabled_issue.id)
            .await,
        Err(Error::Unauthorized)
    ));
    assert_eq!(
        h.app
            .get_issue(&owner, workspace, disabled_issue.id)
            .await
            .unwrap()
            .assignee_id,
        Some(disabled.user.id)
    );
    settle(&worker, workspace).await;
    assert_eq!(
        h.app
            .get_issue(&owner, workspace, disabled_issue.id)
            .await
            .unwrap()
            .assignee_id,
        None
    );
    system_effects(h, &owner, workspace, &[disabled_issue.id], disabled.user.id).await;

    // A workflow batch can reopen a historical assignment without a browser draft.
    // Its committed signal must therefore cause a new current-eligibility sweep.
    let flow = h.app.workflow(&owner, workspace, private).await.unwrap();
    h.app
        .replace_status(
            &owner,
            workspace,
            private,
            canceled,
            "cleanup-workflow-reopen",
            &flow.etag(),
            req(json!({"replacement_status_id":open})),
        )
        .await
        .unwrap();
    settle(&worker, workspace).await;
    let reopened = h
        .app
        .get_issue(&owner, workspace, canceled_issue.id)
        .await
        .unwrap();
    assert_eq!((reopened.status_id, reopened.assignee_id), (open, None));

    // An archived team retains history. Restoring it emits a fresh access event,
    // so a sweep that already skipped the team cannot leave stale assignments.
    let archived_team = team(h, &owner, workspace, false).await;
    let team_issue = issue(
        h,
        &owner,
        workspace,
        archived_team,
        json!({"assignee_id":recipient.user.id}),
    )
    .await;
    let team_before = h
        .app
        .get_team(&owner, workspace, archived_team)
        .await
        .unwrap();
    h.app
        .update_team(
            &owner,
            workspace,
            archived_team,
            &team_before.etag(),
            req(json!({"archived":true})),
        )
        .await
        .unwrap();
    deactivate(h, &owner, workspace, recipient.user.id).await;
    settle(&worker, workspace).await;
    assert_eq!(
        h.app
            .get_issue(&owner, workspace, team_issue.id)
            .await
            .unwrap()
            .assignee_id,
        Some(recipient.user.id)
    );
    let team_before = h
        .app
        .get_team(&owner, workspace, archived_team)
        .await
        .unwrap();
    h.app
        .update_team(
            &owner,
            workspace,
            archived_team,
            &team_before.etag(),
            req(json!({"archived":false})),
        )
        .await
        .unwrap();
    settle(&worker, workspace).await;
    assert_eq!(
        h.app
            .get_issue(&owner, workspace, team_issue.id)
            .await
            .unwrap()
            .assignee_id,
        None
    );

    let workspace_member = person(h, &owner, workspace).await;
    let workspace_issue = issue(
        h,
        &owner,
        workspace,
        public,
        json!({"assignee_id":workspace_member.user.id}),
    )
    .await;
    let current = h.app.workspace(&owner, workspace).await.unwrap();
    h.app
        .update_workspace(
            &owner,
            workspace,
            &current.etag(),
            req(json!({"archived":true})),
        )
        .await
        .unwrap();
    operator
        .disable_account(
            workspace_member.user.id,
            "Archived workspace cleanup qualification",
        )
        .await
        .unwrap();
    settle(&worker, workspace).await;
    assert_eq!(
        h.app
            .get_issue(&owner, workspace, workspace_issue.id)
            .await
            .unwrap()
            .assignee_id,
        Some(workspace_member.user.id)
    );
    let current = h.app.workspace(&owner, workspace).await.unwrap();
    h.app
        .update_workspace(
            &owner,
            workspace,
            &current.etag(),
            req(json!({"archived":false})),
        )
        .await
        .unwrap();
    settle(&worker, workspace).await;
    assert_eq!(
        h.app
            .get_issue(&owner, workspace, workspace_issue.id)
            .await
            .unwrap()
            .assignee_id,
        None
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_postgres_assignment_effect_rollback() {
    let h = Harness::with_connections(6).await;
    let copy = h.clone();
    let run = tokio::spawn(async move { rollback(&copy).await }).await;
    h.cleanup().await;
    if let Err(error) = run {
        std::panic::resume_unwind(error.into_panic());
    }
}

async fn rollback(h: &Harness) {
    let (owner, workspace, public) = setup(h).await;
    let worker = worker(h).await;
    let operator = Operator::connect(&h.operator_url().await).await.unwrap();
    settle(&worker, workspace).await;
    let recipient = person(h, &owner, workspace).await;
    let ids = seed(h, &owner, workspace, public, recipient.user.id, 2).await;
    deactivate(h, &owner, workspace, recipient.user.id).await;
    worker.capture(workspace).await.unwrap();
    let first = worker.claim(workspace).await.unwrap().unwrap();
    let job = first.id;
    let partial = format!(
        "IF OLD.id='{}'::uuid AND OLD.assignee_id IS NOT NULL AND NEW.assignee_id IS NULL THEN RETURN NULL; END IF",
        ids[0]
    );
    let cases = [
        ("issues", "UPDATE", partial.as_str()),
        (
            "activity_entries",
            "INSERT",
            "IF NEW.actor_kind='system' THEN RETURN NULL; END IF",
        ),
        (
            "audit_entries",
            "INSERT",
            "IF NEW.actor_kind='system' THEN RETURN NULL; END IF",
        ),
        (
            "workspace_events",
            "INSERT",
            "IF NEW.actor_kind='system' THEN RETURN NULL; END IF",
        ),
        ("workspace_stream_heads", "UPDATE", "RETURN NULL"),
        ("assignment_reconciliations", "UPDATE", "RETURN NULL"),
        (
            "jobs",
            "UPDATE",
            "IF OLD.state='running' AND NEW.state IN('queued','succeeded') THEN RETURN NULL; END IF",
        ),
    ];
    let mut claim = first;
    for (table, event, body) in cases {
        let before = effect_snapshot(h, &owner, workspace).await;
        fault(h, table, event, body).await;
        let result = worker.run_claim(&claim).await;
        clear_fault(h, table).await;
        assert!(
            result.is_err() || matches!(result.unwrap(), WorkerTick::Retry | WorkerTick::LostLease),
            "{table} suppression was reported successful"
        );
        assert_eq!(
            effect_snapshot(h, &owner, workspace).await,
            before,
            "{table} suppression committed partial assignment effects"
        );
        let mut tx = h.context(owner.user.id, workspace).await;
        let state: String =
            sqlx::query_scalar("SELECT state FROM jobs WHERE workspace_id=$1 AND id=$2")
                .bind(workspace)
                .bind(job)
                .fetch_one(&mut *tx)
                .await
                .unwrap();
        tx.commit().await.unwrap();
        if state == "running" {
            // A silently suppressed final lease update cannot commit effects;
            // the original still-live claim remains valid after removing the fault.
            assert_eq!(table, "jobs");
        } else {
            operator
                .resume_assignment_cleanup(workspace, job, "Retry after isolated suppression probe")
                .await
                .unwrap();
            claim = worker.claim(workspace).await.unwrap().unwrap();
            assert_eq!(claim.id, job);
        }
    }
    // Cancel an actual in-flight effect after its issue UPDATE but before its
    // first required activity INSERT. The dropped transaction must roll back;
    // a later lease takeover must not duplicate any partially attempted effect.
    let advisory = i64::from_be_bytes(Uuid::new_v4().as_bytes()[..8].try_into().unwrap());
    let gate_body = format!(
        "IF NEW.actor_kind='system' AND NEW.entity_id='{}'::uuid THEN PERFORM pg_advisory_xact_lock({advisory}); END IF",
        ids[0]
    );
    fault(h, "activity_entries", "INSERT", &gate_body).await;
    let mut held = h.context(owner.user.id, workspace).await;
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(advisory)
        .execute(&mut *held)
        .await
        .unwrap();
    let pid = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *held)
        .await
        .unwrap();
    let before = effect_snapshot(h, &owner, workspace).await;
    let runner = worker.clone();
    let abandoned = claim.clone();
    let running = tokio::spawn(async move { runner.run_claim(&abandoned).await });
    wait_for_blocker(h, pid).await;
    running.abort();
    assert!(running.await.unwrap_err().is_cancelled());
    held.commit().await.unwrap();
    clear_fault(h, "activity_entries").await;
    assert_eq!(
        effect_snapshot(h, &owner, workspace).await,
        before,
        "Canceled effect leaked uncommitted assignments or history"
    );
    let mut tx = h.context(owner.user.id, workspace).await;
    sqlx::query("UPDATE jobs SET lease_expires_at=clock_timestamp()-interval '1 second' WHERE workspace_id=$1 AND id=$2")
        .bind(workspace).bind(job).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    let takeover = worker.claim(workspace).await.unwrap().unwrap();
    assert!(takeover.lease_token > claim.lease_token);
    assert!(takeover.attempt_count > claim.attempt_count);
    assert_eq!(
        worker.run_claim(&claim).await.unwrap(),
        WorkerTick::LostLease
    );
    claim = takeover;

    // Let a real effect cross its lease deadline while waiting on its team.
    // The final fence must roll back every assignment and side effect.
    let mut tx = h.context(owner.user.id, workspace).await;
    sqlx::query("UPDATE jobs SET lease_expires_at=clock_timestamp()+interval '400 milliseconds' WHERE workspace_id=$1 AND id=$2")
        .bind(workspace).bind(job).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    let mut held = h.context(owner.user.id, workspace).await;
    sqlx::query("SELECT id FROM teams WHERE workspace_id=$1 AND id=$2 FOR UPDATE")
        .bind(workspace)
        .bind(public)
        .execute(&mut *held)
        .await
        .unwrap();
    let pid = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *held)
        .await
        .unwrap();
    let before = effect_snapshot(h, &owner, workspace).await;
    let runner = worker.clone();
    let expiring = claim.clone();
    let running = tokio::spawn(async move { runner.run_claim(&expiring).await });
    wait_for_blocker(h, pid).await;
    loop {
        let mut check = h.context(owner.user.id, workspace).await;
        let expired: bool = sqlx::query_scalar(
            "SELECT lease_expires_at<=clock_timestamp() FROM jobs WHERE workspace_id=$1 AND id=$2",
        )
        .bind(workspace)
        .bind(job)
        .fetch_one(&mut *check)
        .await
        .unwrap();
        check.commit().await.unwrap();
        if expired {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    held.commit().await.unwrap();
    assert_eq!(running.await.unwrap().unwrap(), WorkerTick::LostLease);
    assert_eq!(effect_snapshot(h, &owner, workspace).await, before);
    let recovered = worker.claim(workspace).await.unwrap().unwrap();
    assert!(recovered.lease_token > claim.lease_token);
    assert!(recovered.attempt_count > claim.attempt_count);
    assert_eq!(
        worker.run_claim(&recovered).await.unwrap(),
        WorkerTick::Progress
    );
    settle(&worker, workspace).await;
    assert_eq!(assigned(h, &owner, workspace, &ids).await, 0);
    for row in rows(h, &owner, workspace, &ids).await {
        assert_eq!(row["version"], 2);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_postgres_assignment_0014_upgrade_backfill() {
    let h = Harness::with_version(6, 14).await;
    let copy = h.clone();
    let run = tokio::spawn(async move { upgrade(&copy).await }).await;
    h.cleanup().await;
    if let Err(error) = run {
        std::panic::resume_unwind(error.into_panic());
    }
}

async fn upgrade(h: &Harness) {
    let (owner, workspace, public) = setup(h).await;
    let recipient = person(h, &owner, workspace).await;
    let ids = seed(h, &owner, workspace, public, recipient.user.id, 205).await;
    // This is genuine pre-migration persisted debt, not an event emitted after
    // the consumer was registered. The backfill must discover it independently.
    deactivate(h, &owner, workspace, recipient.user.id).await;
    let old = rows(h, &owner, workspace, &ids).await;
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT max(version) FROM _sqlx_migrations")
            .fetch_one(&h.owner)
            .await
            .unwrap(),
        14
    );
    // Qualify the actual 0014 -> 0015 transition independently of migrations
    // required by the current application and worker.
    sqlx::migrate::Migrator {
        migrations: store::MIGRATOR
            .iter()
            .filter(|migration| migration.version <= 15)
            .cloned()
            .collect::<Vec<_>>()
            .into(),
        ..sqlx::migrate::Migrator::DEFAULT
    }
    .run(&h.owner)
    .await
    .unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT max(version) FROM _sqlx_migrations")
            .fetch_one(&h.owner)
            .await
            .unwrap(),
        15
    );
    assert_eq!(
        rows(h, &owner, workspace, &ids).await,
        old,
        "Migration must not perform unbounded assignment updates"
    );
    store::MIGRATOR.run(&h.owner).await.unwrap();
    // 0016 adds the canonical initial document projection. Preserve a full-row
    // comparison so no unrelated assignment or issue changes are hidden.
    let mut expected_current = old.clone();
    for issue in &mut expected_current {
        issue["body_version"] = json!(1);
    }
    assert_eq!(rows(h, &owner, workspace, &ids).await, expected_current);
    let worker = worker(h).await;
    let initial = worker.capture(workspace).await.unwrap();
    assert!(initial.backfilled);
    assert_eq!(initial.captured, 1);
    assert!(!initial.dead_lettered);
    let replay = worker.capture(workspace).await.unwrap();
    assert!(!replay.backfilled);
    assert_eq!(replay.captured, 0);
    assert!(settle(&worker, workspace).await >= 3);
    assert_eq!(assigned(h, &owner, workspace, &ids).await, 0);
    for row in rows(h, &owner, workspace, &ids).await {
        assert_eq!(row["version"], 2);
    }
    let mut tx = h.context(owner.user.id, workspace).await;
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM jobs WHERE workspace_id=$1 AND kind='assignment.reconcile' AND dedupe_key='assignment:backfill:v1'")
        .bind(workspace).fetch_one(&mut *tx).await.unwrap();
    assert_eq!(count, 1);
    tx.commit().await.unwrap();
}
