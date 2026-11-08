//! Real PostgreSQL durable fanout, authority, replay and admission boundaries.
mod graph_support;
#[allow(dead_code)]
mod support;
use graph_support::*;
use opentrack_app::worker::{Worker, WorkerOptions};
use opentrack_domain::*;
use opentrack_store as store;
use serde_json::{Value, json};
use sqlx::AssertSqlSafe;
use support::Harness;
use uuid::Uuid;

fn body(ids: &[Uuid]) -> DocumentWrite {
    req(
        json!({"schema_version":1,"body":{"type":"doc","content":[{"type":"paragraph","content":ids.iter().map(|id|json!({"type":"userMention","attrs":{"id":id}})).collect::<Vec<_>>()}]}}),
    )
}
async fn runner(h: &Harness) -> Worker {
    Worker::connect(
        &h.worker_url().await,
        WorkerOptions {
            max_connections: 2,
            ..WorkerOptions::default()
        },
    )
    .await
    .unwrap()
}
async fn production_operator(h: &Harness) -> opentrack_app::Operator {
    let url = h.operator_url().await;
    let parsed = url::Url::parse(&url).unwrap();
    let role = parsed.username().replace('\'', "''");
    let schema: String = sqlx::query_scalar("SELECT current_schema()")
        .fetch_one(&h.owner)
        .await
        .unwrap();
    let script = include_str!("../../../deploy/grant-operator.sql")
        .lines()
        .filter(|line| !line.starts_with('\\'))
        .collect::<Vec<_>>()
        .join("\n");
    let script = script
        .replace(":'operator_role'", &format!("'{role}'"))
        .replace(":'operator_schema'", &format!("'{schema}'"));
    let script = script
        .trim()
        .strip_prefix("BEGIN;")
        .unwrap()
        .strip_suffix("COMMIT;")
        .unwrap();
    let mut tx = store::begin(&h.owner).await.unwrap();
    sqlx::query("SELECT pg_advisory_xact_lock(1869636978,1)")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::raw_sql(AssertSqlSafe(script))
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    opentrack_app::Operator::connect(&url).await.unwrap()
}
async fn drain(worker: &Worker, w: Uuid) {
    for _ in 0..40 {
        let tick = worker.apply_notification_events(w).await.unwrap();
        assert!(!tick.dead_lettered);
        let flushed = worker.flush_notification_events(w).await.unwrap();
        if tick.processed == 0 && tick.captured == 0 && !tick.backfilled && flushed == 0 {
            return;
        }
    }
    panic!("Notification fixture did not drain");
}
async fn state(h: &Harness, a: &Actor, w: Uuid) -> Value {
    let mut tx = h.context(a.user.id, w).await;
    let result=sqlx::query_scalar("SELECT jsonb_build_object('notifications',(SELECT coalesce(jsonb_agg(to_jsonb(n) ORDER BY user_id,inbox_seq),'[]') FROM notifications n WHERE workspace_id=$1),'checkpoint',(SELECT to_jsonb(c) FROM consumer_checkpoints c WHERE workspace_id=$1 AND consumer_name='notifications'),'head',(SELECT last_seq FROM workspace_stream_heads WHERE workspace_id=$1),'sources',(SELECT coalesce(jsonb_agg(to_jsonb(s) ORDER BY id),'[]') FROM notification_sources s WHERE workspace_id=$1),'letters',(SELECT coalesce(jsonb_agg(to_jsonb(d) ORDER BY seq),'[]') FROM consumer_dead_letters d WHERE workspace_id=$1 AND consumer_name='notifications'),'outbox',(SELECT coalesce(jsonb_agg(to_jsonb(o) ORDER BY notification_id,notification_version),'[]') FROM notification_invalidations o WHERE workspace_id=$1))").bind(w).fetch_one(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    result
}
async fn count(h: &Harness, a: &Actor, w: Uuid, user: Uuid, reason: &str) -> i64 {
    let mut tx = h.context(a.user.id, w).await;
    let n = sqlx::query_scalar(
        "SELECT count(*) FROM notifications WHERE workspace_id=$1 AND user_id=$2 AND reason=$3",
    )
    .bind(w)
    .bind(user)
    .bind(reason)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    n
}
#[tokio::test]
async fn restricted_app_sources_dedupe_preferences_and_unsubscribe() {
    let h = Harness::new().await;
    let r = h.clone();
    let result=tokio::spawn(async move {
 let (a,w,t)=setup(&r).await;let b=person(&r,&a,w,"member").await;let c=person(&r,&a,w,"member").await;
 let worker=runner(&r).await;drain(&worker,w).await;
 let item=issue(&r,&a,w,t,json!({"assignee_id":b.user.id,"document":body(&[b.user.id,c.user.id,a.user.id])})).await;
 drain(&worker,w).await;
 assert_eq!(count(&r,&a,w,b.user.id,"assignment").await,1);assert_eq!(count(&r,&a,w,b.user.id,"mention").await,1);assert_eq!(count(&r,&a,w,c.user.id,"mention").await,1);
 assert_eq!(count(&r,&a,w,a.user.id,"mention").await,0);
 let before=state(&r,&a,w).await;
 r.app.update_issue(&a,w,item.id,&item.etag(),json!({"title":"Unrelated title"})).await.unwrap();drain(&worker,w).await;
 assert_eq!(state(&r,&a,w).await["notifications"],before["notifications"]);
 let sub=r.app.issue_subscription(&b,w,item.id).await.unwrap();r.app.update_issue_subscription(&b,w,item.id,&sub.etag,req(json!({"preference":"unsubscribed"}))).await.unwrap();
 r.app.create_comment(&a,w,item.id,"new-comment",body(&[b.user.id])).await.unwrap();drain(&worker,w).await;
 assert_eq!(count(&r,&a,w,b.user.id,"mention").await,2);assert_eq!(count(&r,&a,w,b.user.id,"subscribed_comment").await,0);assert_eq!(count(&r,&a,w,c.user.id,"subscribed_comment").await,1);
 let mut tx=store::begin(&r.app.pool).await.unwrap();store::context(&mut tx,b.user.id,Some(w)).await.unwrap();sqlx::query("UPDATE notification_preferences SET assignments=false,mentions=false,version=version+1,updated_at=clock_timestamp() WHERE workspace_id=$1 AND user_id=$2").bind(w).bind(b.user.id).execute(&mut *tx).await.unwrap();tx.commit().await.unwrap();
 issue(&r,&a,w,t,json!({"assignee_id":b.user.id,"document":body(&[b.user.id])})).await;drain(&worker,w).await;
 assert_eq!(count(&r,&a,w,b.user.id,"assignment").await,1);assert_eq!(count(&r,&a,w,b.user.id,"mention").await,2);
 let project=r.app.create_project(&a,w,t,"mentioned-project",req(json!({"name":"Mentioned project"}))).await.unwrap();
 let document=r.app.project_document(&a,w,t,project.id).await.unwrap();r.app.update_document(&a,w,document.id,&document.etag,body(&[c.user.id])).await.unwrap();drain(&worker,w).await;
 let final_state=state(&r,&a,w).await;assert!(final_state["notifications"].as_array().unwrap().iter().any(|n|n["entity_kind"]=="project"&&n["user_id"]==json!(c.user.id)));
 let saved=final_state["notifications"].clone();worker.pool.close().await;let restarted=runner(&r).await;drain(&restarted,w).await;assert_eq!(state(&r,&a,w).await["notifications"],saved);
 let mut runtime=store::begin(&r.app.pool).await.unwrap();store::context(&mut runtime,b.user.id,Some(w)).await.unwrap();let own_count:i64=sqlx::query_scalar("SELECT count(*) FROM notifications WHERE workspace_id=$1").bind(w).fetch_one(&mut *runtime).await.unwrap();assert_eq!(own_count,3);runtime.rollback().await.unwrap();
 for statement in ["SELECT * FROM notification_sources","UPDATE notification_heads SET last_seq=last_seq+1","UPDATE notifications SET user_id=gen_random_uuid(),version=version+1","INSERT INTO notification_sources(workspace_id,id) VALUES(gen_random_uuid(),gen_random_uuid())"] {let mut tx=store::begin(&r.app.pool).await.unwrap();store::context(&mut tx,b.user.id,Some(w)).await.unwrap();assert!(sqlx::query(AssertSqlSafe(statement)).execute(&mut *tx).await.is_err());}
 restarted.pool.close().await;
 }).await;
    h.cleanup().await;
    if let Err(e) = result {
        std::panic::resume_unwind(e.into_panic());
    }
}

#[tokio::test]
async fn current_and_origin_acl_and_disabled_recipient_are_rechecked() {
    let h = Harness::new().await;
    let r = h.clone();
    let result=tokio::spawn(async move {
 let(a,w,t)=setup(&r).await;let b=person(&r,&a,w,"member").await;let private=team(&r,&a,w,true).await;grant(&r,&a,w,private,b.user.id).await;
 let worker=runner(&r).await;drain(&worker,w).await;
 let item=issue(&r,&a,w,private,json!({"assignee_id":b.user.id,"document":body(&[b.user.id])})).await;
 let public_status=r.app.workflow(&a,w,t).await.unwrap().statuses.into_iter().find(|s|s.is_default).unwrap().id;
 let mut tx=r.context(a.user.id,w).await;sqlx::query("UPDATE issues SET team_id=$3,status_id=$4,number=999,version=version+1 WHERE workspace_id=$1 AND id=$2").bind(w).bind(item.id).bind(t).bind(public_status).execute(&mut *tx).await.unwrap();sqlx::query("DELETE FROM team_members WHERE workspace_id=$1 AND team_id=$2 AND user_id=$3").bind(w).bind(private).bind(b.user.id).execute(&mut *tx).await.unwrap();tx.commit().await.unwrap();
 drain(&worker,w).await;assert_eq!(count(&r,&a,w,b.user.id,"mention").await,0);assert_eq!(count(&r,&a,w,b.user.id,"assignment").await,0);
 issue(&r,&a,w,t,json!({"document":body(&[b.user.id])})).await;
 let operator=opentrack_app::Operator::connect(&r.operator_url().await).await.unwrap();operator.disable_account(b.user.id,"Check pending recipient revocation").await.unwrap();drain(&worker,w).await;
 assert_eq!(count(&r,&a,w,b.user.id,"mention").await,0);worker.pool.close().await;
 }).await;
    h.cleanup().await;
    if let Err(e) = result {
        std::panic::resume_unwind(e.into_panic());
    }
}

#[tokio::test]
async fn fanout_is_bounded_persisted_and_restart_safe() {
    let h = Harness::new().await;
    let r = h.clone();
    let result=tokio::spawn(async move {
 let(a,w,t)=setup(&r).await;let worker=runner(&r).await;drain(&worker,w).await;let item=issue(&r,&a,w,t,json!({})).await;
 let mut tx=r.context(a.user.id,w).await;
 sqlx::query("WITH inserted AS (INSERT INTO users(id,email,display_name,password_hash) SELECT gen_random_uuid(),'fanout-'||g||'@example.test','Example Recipient',password_hash FROM users CROSS JOIN generate_series(1,205) g WHERE id=$1 RETURNING id) INSERT INTO workspace_members(workspace_id,user_id,role) SELECT $2,id,'member' FROM inserted").bind(a.user.id).bind(w).execute(&mut *tx).await.unwrap();
 sqlx::query("INSERT INTO issue_subscriptions(workspace_id,issue_id,user_id,preference) SELECT $1,$2,user_id,'subscribed' FROM workspace_members WHERE workspace_id=$1 AND user_id<>$3").bind(w).bind(item.id).bind(a.user.id).execute(&mut *tx).await.unwrap();tx.commit().await.unwrap();
 r.app.create_comment(&a,w,item.id,"fanout-comment",DocumentWrite::from_plain_text("Subscribed comment")).await.unwrap();
 let first=worker.apply_notification_events(w).await.unwrap();assert!(first.captured>0&&first.captured<=100);let partial=state(&r,&a,w).await;assert!(partial["checkpoint"]["last_seq"].as_i64().unwrap()<partial["head"].as_i64().unwrap());assert!(partial["sources"].as_array().unwrap().iter().any(|s|!s["completed"].as_bool().unwrap()&&s["after_user_id"].is_string()));
 worker.pool.close().await;let restarted=runner(&r).await;
 for _ in 0..10{let tick=restarted.apply_notification_events(w).await.unwrap();assert!(tick.captured<=100);if tick.processed==0&&tick.captured==0{break;}}
 let complete=state(&r,&a,w).await;assert_eq!(complete["notifications"].as_array().unwrap().len(),205);assert!(complete["sources"].as_array().unwrap().iter().all(|s|s["completed"]==true));
 restarted.flush_notification_events(w).await.unwrap();drain(&restarted,w).await;assert_eq!(state(&r,&a,w).await["notifications"],complete["notifications"]);restarted.pool.close().await;
 }).await;
    h.cleanup().await;
    if let Err(e) = result {
        std::panic::resume_unwind(e.into_panic());
    }
}

#[tokio::test]
async fn suppressed_receipt_rolls_back_heads_fanout_and_retry_is_durable() {
    let h = Harness::new().await;
    let r = h.clone();
    let result=tokio::spawn(async move {
 let(a,w,t)=setup(&r).await;let b=person(&r,&a,w,"member").await;let worker=runner(&r).await;drain(&worker,w).await;
 issue(&r,&a,w,t,json!({"assignee_id":b.user.id})).await;
 sqlx::raw_sql("CREATE FUNCTION test_notification_suppress() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.consumer_name='notifications' THEN RETURN NULL;END IF;RETURN NEW;END $$;CREATE TRIGGER test_notification_suppress BEFORE INSERT ON consumer_receipts FOR EACH ROW EXECUTE FUNCTION test_notification_suppress();").execute(&r.owner).await.unwrap();
 let before=state(&r,&a,w).await;let failed=worker.apply_notification_events(w).await.unwrap();assert_eq!((failed.processed,failed.captured),(0,0));let after=state(&r,&a,w).await;assert_eq!(after["checkpoint"],before["checkpoint"]);assert_eq!(after["notifications"],before["notifications"]);assert_eq!(after["outbox"],before["outbox"]);
 sqlx::raw_sql("DROP TRIGGER test_notification_suppress ON consumer_receipts;DROP FUNCTION test_notification_suppress();").execute(&r.owner).await.unwrap();
 let letter=Uuid::parse_str(after["letters"][0]["id"].as_str().unwrap()).unwrap();let mut tx=r.context(a.user.id,w).await;sqlx::query("SELECT operator_notification_retry($1,$2,'Retry after repaired receipt trigger')").bind(w).bind(letter).execute(&mut *tx).await.unwrap();tx.commit().await.unwrap();
 drain(&worker,w).await;let repaired=state(&r,&a,w).await;assert_eq!(repaired["notifications"].as_array().unwrap().len(),1);assert_eq!(repaired["notifications"][0]["inbox_seq"],1);assert_eq!(repaired["letters"][0]["state"],"resolved");
 worker.pool.close().await;
 }).await;
    h.cleanup().await;
    if let Err(e) = result {
        std::panic::resume_unwind(e.into_panic());
    }
}

#[tokio::test]
async fn saturated_admission_preserves_inbox_then_publishes_private_invalidation() {
    let h = Harness::new().await;
    let r = h.clone();
    let result=tokio::spawn(async move {
 let(a,w,t)=setup(&r).await;let b=person(&r,&a,w,"member").await;let worker=runner(&r).await;drain(&worker,w).await;
 issue(&r,&a,w,t,json!({"assignee_id":b.user.id})).await;
 let mut tx=r.context(a.user.id,w).await;sqlx::query("UPDATE workspace_stream_heads SET max_pending_events=1 WHERE workspace_id=$1").bind(w).execute(&mut *tx).await.unwrap();tx.commit().await.unwrap();
 assert_eq!(worker.apply_notification_events(w).await.unwrap().captured,1);assert_eq!(worker.flush_notification_events(w).await.unwrap(),0);
 let pending=state(&r,&a,w).await;assert_eq!(pending["notifications"].as_array().unwrap().len(),1);assert!(pending["outbox"][0]["published_seq"].is_null());assert_eq!(pending["outbox"][0]["attempts"],0);
 let mut tx=r.context(a.user.id,w).await;sqlx::query("UPDATE consumer_checkpoints SET last_seq=(SELECT last_seq FROM workspace_stream_heads WHERE workspace_id=$1) WHERE workspace_id=$1").bind(w).execute(&mut *tx).await.unwrap();tx.commit().await.unwrap();
 assert_eq!(worker.flush_notification_events(w).await.unwrap(),1);let published=state(&r,&a,w).await;assert!(published["outbox"][0]["published_seq"].is_number());
 let mut tx=store::begin(&r.app.pool).await.unwrap();store::context(&mut tx,b.user.id,Some(w)).await.unwrap();let id=Uuid::parse_str(published["notifications"][0]["id"].as_str().unwrap()).unwrap();let err=sqlx::query("INSERT INTO workspace_events(workspace_id,seq,event_id,event_type,entity_id,entity_version,actor_kind,actor_id,mutation_id) SELECT $1,last_seq+1,gen_random_uuid(),'notification.created',$2,1,'system',NULL,gen_random_uuid() FROM workspace_stream_heads WHERE workspace_id=$1").bind(w).bind(id).execute(&mut *tx).await.unwrap_err();assert_eq!(err.as_database_error().unwrap().code().as_deref(),Some("42501"));
 worker.pool.close().await;
 }).await;
    h.cleanup().await;
    if let Err(e) = result {
        std::panic::resume_unwind(e.into_panic());
    }
}

#[tokio::test]
async fn upgrade_backfills_durable_sources_after_replay_purge_without_invented_assignments() {
    let h = Harness::with_version(5, 31).await;
    let r = h.clone();
    let result = tokio::spawn(async move {
        let (a, w, t) = setup(&r).await;
        let b = person(&r, &a, w, "member").await;
        let item = issue(
            &r,
            &a,
            w,
            t,
            json!({"assignee_id":b.user.id,"document":body(&[b.user.id])}),
        )
        .await;
        r.app
            .create_comment(&a, w, item.id, "historical-comment", body(&[b.user.id]))
            .await
            .unwrap();
        // Real pre-upgrade mutations produce more than one historical scan batch.
        for _ in 0..100 {
            issue(&r, &a, w, t, json!({"document":body(&[b.user.id])})).await;
        }
        let mut tx = r.context(a.user.id, w).await;
        let floor: i64 =
            sqlx::query_scalar("SELECT last_seq FROM workspace_stream_heads WHERE workspace_id=$1")
                .bind(w)
                .fetch_one(&mut *tx)
                .await
                .unwrap();
        sqlx::query("DELETE FROM workspace_events WHERE workspace_id=$1")
            .bind(w)
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("UPDATE event_retention_state SET purged_through_seq=$2 WHERE workspace_id=$1")
            .bind(w)
            .bind(floor)
            .execute(&mut *tx)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        store::MIGRATOR.run(&r.owner).await.unwrap();
        let initial = state(&r, &a, w).await;
        assert_eq!(initial["checkpoint"]["last_seq"], floor);
        assert!(initial["checkpoint"]["backfill_job_id"].is_null());
        let first_worker = runner(&r).await;
        let first = first_worker.apply_notification_events(w).await.unwrap();
        assert_eq!(first.captured, 100);
        let partial = state(&r, &a, w).await;
        assert!(partial["checkpoint"]["backfill_job_id"].is_null());
        assert_eq!(partial["notifications"].as_array().unwrap().len(), 100);
        first_worker.pool.close().await;
        let worker = runner(&r).await;
        drain(&worker, w).await;
        assert_eq!(count(&r, &a, w, b.user.id, "assignment").await, 0);
        assert_eq!(count(&r, &a, w, b.user.id, "mention").await, 102);
        assert_eq!(count(&r, &a, w, b.user.id, "subscribed_comment").await, 1);
        let completed = state(&r, &a, w).await;
        assert!(completed["checkpoint"]["backfill_job_id"].is_string());
        assert_eq!(completed["checkpoint"]["last_seq"], completed["head"]);
        assert!(
            completed["notifications"]
                .as_array()
                .unwrap()
                .iter()
                .all(|n| n["event_seq"].is_null())
        );
        worker.pool.close().await;
    })
    .await;
    h.cleanup().await;
    if let Err(e) = result {
        std::panic::resume_unwind(e.into_panic());
    }
}

#[tokio::test]
async fn assignment_delta_requires_exact_actor_and_version_event() {
    let h = Harness::new().await;
    let r = h.clone();
    let result=tokio::spawn(async move {
        let(a,w,t)=setup(&r).await;let b=person(&r,&a,w,"member").await;let item=issue(&r,&a,w,t,json!({})).await;
        for mismatch in ["version","actor"] {
            let mut tx=r.context(a.user.id,w).await;
            sqlx::query("UPDATE issues SET assignee_id=$3,version=version+1 WHERE workspace_id=$1 AND id=$2").bind(w).bind(item.id).bind(b.user.id).execute(&mut *tx).await.unwrap();
            let seq:i64=sqlx::query_scalar("UPDATE workspace_stream_heads SET last_seq=last_seq+1 WHERE workspace_id=$1 RETURNING last_seq").bind(w).fetch_one(&mut *tx).await.unwrap();
            sqlx::query("INSERT INTO workspace_events(workspace_id,seq,event_id,event_type,team_id,entity_id,entity_version,actor_id,mutation_id) VALUES($1,$2,gen_random_uuid(),'issue.updated',$3,$4,$5,$6,gen_random_uuid())").bind(w).bind(seq).bind(t).bind(item.id).bind(if mismatch=="version"{3_i64}else{2_i64}).bind(if mismatch=="actor"{b.user.id}else{a.user.id}).execute(&mut *tx).await.unwrap();
            let err=tx.commit().await.unwrap_err();assert_eq!(err.as_database_error().unwrap().code().as_deref(),Some("23514"));
        }
        let current=r.app.get_issue(&a,w,item.id).await.unwrap();assert_eq!(current.assignee_id,None);assert_eq!(current.version,"1");
    }).await;
    h.cleanup().await;
    if let Err(e) = result {
        std::panic::resume_unwind(e.into_panic());
    }
}

#[tokio::test]
async fn invalidation_failures_block_and_operator_retry_preserves_inbox() {
    let h = Harness::new().await;
    let r = h.clone();
    let result=tokio::spawn(async move {
  let(a,w,t)=setup(&r).await;let b=person(&r,&a,w,"member").await;let worker=runner(&r).await;drain(&worker,w).await;
  issue(&r,&a,w,t,json!({"assignee_id":b.user.id})).await;assert_eq!(worker.apply_notification_events(w).await.unwrap().captured,1);
  sqlx::raw_sql("CREATE FUNCTION test_notification_event_suppress() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.event_type LIKE 'notification.%' THEN RETURN NULL;END IF;RETURN NEW;END $$;CREATE TRIGGER test_notification_event_suppress BEFORE INSERT ON workspace_events FOR EACH ROW EXECUTE FUNCTION test_notification_event_suppress();").execute(&r.owner).await.unwrap();
  let before=state(&r,&a,w).await;
  for attempt in 1..=5 {
   let mut tx=r.context(a.user.id,w).await;sqlx::query("UPDATE notification_invalidations SET available_at=clock_timestamp() WHERE workspace_id=$1 AND published_seq IS NULL").bind(w).execute(&mut *tx).await.unwrap();tx.commit().await.unwrap();
   assert_eq!(worker.flush_notification_events(w).await.unwrap(),0);let current=state(&r,&a,w).await;assert_eq!(current["notifications"],before["notifications"]);assert_eq!(current["head"],before["head"]);assert_eq!(current["outbox"][0]["attempts"],attempt);assert_eq!(current["outbox"][0]["blocked"],attempt==5);
  }
  let operator=production_operator(&r).await;let status=operator.consumer_status(w).await.unwrap();assert_eq!(status["notification_invalidations"]["blocked"].as_array().unwrap().len(),1);
  sqlx::raw_sql("DROP TRIGGER test_notification_event_suppress ON workspace_events;DROP FUNCTION test_notification_event_suppress();").execute(&r.owner).await.unwrap();
  let id=Uuid::parse_str(before["notifications"][0]["id"].as_str().unwrap()).unwrap();operator.retry_notification_invalidation(w,id,"Retry after repaired event trigger").await.unwrap();assert_eq!(worker.flush_notification_events(w).await.unwrap(),1);assert_eq!(state(&r,&a,w).await["notifications"],before["notifications"]);worker.pool.close().await;
 }).await;
    h.cleanup().await;
    if let Err(e) = result {
        std::panic::resume_unwind(e.into_panic());
    }
}

#[tokio::test]
async fn later_poison_event_does_not_permanently_rollback_an_earlier_valid_source() {
    let h = Harness::new().await;
    let r = h.clone();
    let result=tokio::spawn(async move {
  let(a,w,t)=setup(&r).await;let b=person(&r,&a,w,"member").await;let c=person(&r,&a,w,"member").await;let worker=runner(&r).await;drain(&worker,w).await;
  issue(&r,&a,w,t,json!({"assignee_id":b.user.id})).await;issue(&r,&a,w,t,json!({"assignee_id":c.user.id})).await;
  let mut tx=r.context(a.user.id,w).await;sqlx::query("UPDATE workspace_events SET schema_version=2 WHERE workspace_id=$1 AND seq=(SELECT last_seq FROM workspace_stream_heads WHERE workspace_id=$1)").bind(w).execute(&mut *tx).await.unwrap();tx.commit().await.unwrap();
  let failed=worker.apply_notification_events(w).await.unwrap();assert_eq!((failed.processed,failed.captured),(0,0));
  let progressed=worker.apply_notification_events(w).await.unwrap();assert_eq!((progressed.processed,progressed.captured),(1,1));assert_eq!(count(&r,&a,w,b.user.id,"assignment").await,1);assert_eq!(count(&r,&a,w,c.user.id,"assignment").await,0);
  for _ in 1..5 {let mut tx=r.context(a.user.id,w).await;sqlx::query("UPDATE consumer_dead_letters SET available_at=clock_timestamp() WHERE workspace_id=$1 AND consumer_name='notifications' AND state='retry_wait'").bind(w).execute(&mut *tx).await.unwrap();tx.commit().await.unwrap();worker.apply_notification_events(w).await.unwrap();}
  let blocked=state(&r,&a,w).await;assert_eq!(blocked["letters"][0]["state"],"blocked");assert_eq!(blocked["letters"][0]["attempts"],5);assert_eq!(blocked["letters"][0]["error_code"],"unsupported_event_schema");
  let mut tx=r.context(a.user.id,w).await;sqlx::query("UPDATE workspace_events SET schema_version=1 WHERE workspace_id=$1 AND schema_version=2").bind(w).execute(&mut *tx).await.unwrap();tx.commit().await.unwrap();
  let operator=opentrack_app::Operator::connect(&r.operator_url().await).await.unwrap();operator.retry_notification_consumer(w,Uuid::parse_str(blocked["letters"][0]["id"].as_str().unwrap()).unwrap(),"Retry supported source after repair").await.unwrap();drain(&worker,w).await;assert_eq!(count(&r,&a,w,b.user.id,"assignment").await,1);assert_eq!(count(&r,&a,w,c.user.id,"assignment").await,1);worker.pool.close().await;
 }).await;
    h.cleanup().await;
    if let Err(e) = result {
        std::panic::resume_unwind(e.into_panic());
    }
}

#[tokio::test]
async fn missing_known_required_consumer_blocks_admission_and_retention_until_repaired() {
    let h = Harness::new().await;
    let r = h.clone();
    let result=tokio::spawn(async move {
  let(a,w,t)=setup(&r).await;let worker=runner(&r).await;
  for name in ["notifications","planning_counts"] {
   issue(&r,&a,w,t,json!({})).await;
   for _ in 0..10 {worker.capture(w).await.unwrap();worker.apply_planning_events(w).await.unwrap();worker.apply_notification_events(w).await.unwrap();if let Some(claim)=worker.claim(w).await.unwrap(){worker.run_claim(&claim).await.unwrap();}}
   let mut tx=r.context(a.user.id,w).await;
   let cp:(i64,Option<Uuid>)=sqlx::query_as("SELECT last_seq,backfill_job_id FROM consumer_checkpoints WHERE workspace_id=$1 AND consumer_name=$2 AND consumer_version=1").bind(w).bind(name).fetch_one(&mut *tx).await.unwrap();
   sqlx::query("UPDATE workspace_events SET occurred_at=clock_timestamp()-interval '9 days' WHERE workspace_id=$1").bind(w).execute(&mut *tx).await.unwrap();
   sqlx::query("DELETE FROM consumer_receipts WHERE workspace_id=$1 AND consumer_name=$2").bind(w).bind(name).execute(&mut *tx).await.unwrap();
   sqlx::query("DELETE FROM consumer_checkpoints WHERE workspace_id=$1 AND consumer_name=$2").bind(w).bind(name).execute(&mut *tx).await.unwrap();
   sqlx::query("DELETE FROM consumer_registrations WHERE workspace_id=$1 AND consumer_name=$2").bind(w).bind(name).execute(&mut *tx).await.unwrap();tx.commit().await.unwrap();
   assert_eq!(worker.retain(w,1000).await.unwrap().deleted,0);
   let operator=opentrack_app::Operator::connect(&r.operator_url().await).await.unwrap();let status=operator.consumer_status(w).await.unwrap();assert_eq!(status["event_admission"]["admission_reason"],"required_consumer_unavailable");
   assert!(matches!(r.app.create_issue(&a,w,&Uuid::new_v4().to_string(),req(json!({"team_id":t,"title":"Refused while required registration missing"}))).await,Err(Error::Unavailable)));
   let mut tx=r.context(a.user.id,w).await;
   sqlx::query("INSERT INTO consumer_registrations(workspace_id,consumer_name,consumer_version) VALUES($1,$2,1)").bind(w).bind(name).execute(&mut *tx).await.unwrap();
   sqlx::query("INSERT INTO consumer_checkpoints(workspace_id,consumer_name,consumer_version,last_seq,backfill_job_id) VALUES($1,$2,1,$3,$4)").bind(w).bind(name).bind(cp.0).bind(cp.1).execute(&mut *tx).await.unwrap();tx.commit().await.unwrap();
   assert!(worker.retain(w,1000).await.unwrap().deleted>0);issue(&r,&a,w,t,json!({})).await;
  }
  worker.pool.close().await;
 }).await;
    h.cleanup().await;
    if let Err(e) = result {
        std::panic::resume_unwind(e.into_panic());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn committed_preference_and_unsubscribe_writes_win_a_waiting_dispatch() {
    let h = Harness::new().await;
    let r = h.clone();
    let result=tokio::spawn(async move {
  let(a,w,t)=setup(&r).await;let b=person(&r,&a,w,"member").await;let worker=runner(&r).await;drain(&worker,w).await;
  for choice in ["preference","unsubscribe"] {
   let item=issue(&r,&a,w,t,json!({})).await;
   if choice=="preference" {
    let mut tx=r.context(a.user.id,w).await;sqlx::query("INSERT INTO notification_preferences(workspace_id,user_id) VALUES($1,$2)").bind(w).bind(b.user.id).execute(&mut *tx).await.unwrap();tx.commit().await.unwrap();
    let document=r.app.issue_document(&a,w,item.id).await.unwrap();r.app.update_document(&a,w,document.id,&document.etag,body(&[b.user.id])).await.unwrap();
   } else {
    let own=r.app.issue_subscription(&b,w,item.id).await.unwrap();r.app.update_issue_subscription(&b,w,item.id,&own.etag,req(json!({"preference":"subscribed"}))).await.unwrap();
    r.app.create_comment(&a,w,item.id,"unsubscribe-race-comment",DocumentWrite::from_plain_text("Concurrent unsubscribe")).await.unwrap();
   }
   let mut holder=r.context(a.user.id,w).await;let pid:i32=sqlx::query_scalar("SELECT pg_backend_pid()").fetch_one(&mut *holder).await.unwrap();
   if choice=="preference" {sqlx::query("UPDATE notification_preferences SET mentions=false,version=version+1 WHERE workspace_id=$1 AND user_id=$2").bind(w).bind(b.user.id).execute(&mut *holder).await.unwrap();}
   else {sqlx::query("UPDATE issue_subscriptions SET preference='unsubscribed',version=version+1 WHERE workspace_id=$1 AND issue_id=$2 AND user_id=$3").bind(w).bind(item.id).bind(b.user.id).execute(&mut *holder).await.unwrap();}
   let pending_worker=worker.clone();let pending=tokio::spawn(async move {pending_worker.apply_notification_events(w).await});
   let mut observed=false;
   for _ in 0..100 {
    observed=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE $1=ANY(pg_blocking_pids(pid)))").bind(pid).fetch_one(&r.owner).await.unwrap();
    if observed{break;}tokio::time::sleep(std::time::Duration::from_millis(10)).await;
   }
   assert!(observed,"Dispatch must actually wait for the earlier preference write");holder.commit().await.unwrap();let tick=pending.await.unwrap().unwrap();assert!(!tick.dead_lettered);drain(&worker,w).await;
   assert_eq!(count(&r,&a,w,b.user.id,if choice=="preference"{"mention"}else{"subscribed_comment"}).await,0);
  }
  worker.pool.close().await;
 }).await;
    h.cleanup().await;
    if let Err(e) = result {
        std::panic::resume_unwind(e.into_panic());
    }
}

#[tokio::test]
async fn restoring_with_a_new_assignee_preserves_the_authored_assignment_source() {
    let h = Harness::new().await;
    let r = h.clone();
    let result = tokio::spawn(async move {
        let (a, w, t) = setup(&r).await;
        let b = person(&r, &a, w, "member").await;
        let worker = runner(&r).await;
        drain(&worker, w).await;
        let item = issue(&r, &a, w, t, json!({})).await;
        r.app
            .delete_issue(&a, w, item.id, &item.etag())
            .await
            .unwrap();
        let deleted = r.app.get_deleted_issue(&a, w, item.id).await.unwrap();
        let restored = r
            .app
            .restore_issue(
                &a,
                w,
                item.id,
                &deleted.issue.etag(),
                RestoreIssue {
                    status_id: item.status_id,
                    assignee_id: Some(b.user.id),
                    label_ids: vec![],
                },
            )
            .await
            .unwrap();
        assert_eq!(restored.assignee_id, Some(b.user.id));
        drain(&worker, w).await;
        assert_eq!(count(&r, &a, w, b.user.id, "assignment").await, 1);
        worker.pool.close().await;
    })
    .await;
    h.cleanup().await;
    if let Err(e) = result {
        std::panic::resume_unwind(e.into_panic());
    }
}
