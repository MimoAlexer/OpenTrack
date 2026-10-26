#![allow(dead_code)]
use crate::support::Harness;
use opentrack_domain::*;
use opentrack_store as store;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::AssertSqlSafe;
use uuid::Uuid;
pub const PASSWORD: &str = "Graph contract password 482!";
pub fn req<T: serde::de::DeserializeOwned>(v: Value) -> T {
    serde_json::from_value(v).unwrap()
}
pub async fn setup(h: &Harness) -> (Actor, Uuid, Uuid) {
    let (s,c)=h.app.setup(req(json!({"setup_token":h.setup_token,"email":"owner@graph.invalid","password":PASSWORD,"display_name":"Graph owner","workspace_name":"Graph","team_name":"Initial","team_key":"GRA"}))).await.unwrap();
    let a = h.app.authenticate(&c).await.unwrap();
    let w = s.workspaces[0].id;
    let t = h.app.bootstrap(&a, w).await.unwrap().teams[0].id;
    (a, w, t)
}
pub async fn person(h: &Harness, a: &Actor, w: Uuid, role: &str) -> Actor {
    let id = Uuid::new_v4();
    let email = format!("{id}@graph.invalid");
    let mut tx = h.context(a.user.id, w).await;
    sqlx::query("INSERT INTO users(id,email,display_name,password_hash) SELECT $1,$2,'Graph member',password_hash FROM users WHERE id=$3").bind(id).bind(&email).bind(a.user.id).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO workspace_members(workspace_id,user_id,role) VALUES($1,$2,$3)")
        .bind(w)
        .bind(id)
        .bind(role)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let (_, cookie) = h
        .app
        .login(req(json!({"email":email,"password":PASSWORD})))
        .await
        .unwrap();
    h.app.authenticate(&cookie).await.unwrap()
}
pub async fn grant(h: &Harness, a: &Actor, w: Uuid, t: Uuid, user: Uuid) {
    let mut tx = h.context(a.user.id, w).await;
    sqlx::query("INSERT INTO team_members(workspace_id,team_id,user_id,role) VALUES($1,$2,$3,'contributor')").bind(w).bind(t).bind(user).execute(&mut *tx).await.unwrap();
    sqlx::query("UPDATE workspace_auth_guards SET epoch=epoch+1 WHERE workspace_id=$1")
        .bind(w)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
}
pub async fn team(h: &Harness, a: &Actor, w: Uuid, private: bool) -> Uuid {
    let key = Uuid::new_v4().simple().to_string();
    h.app
        .create_team(
            a,
            w,
            &key,
            req(json!({"name":key,"key":format!("T{}",&key[..6]),"private":private})),
        )
        .await
        .unwrap()
        .id
}
pub async fn issue(h: &Harness, a: &Actor, w: Uuid, t: Uuid, extra: Value) -> Issue {
    let mut body = json!({"team_id":t,"title":format!("Graph issue {}",Uuid::new_v4())});
    body.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    h.app
        .create_issue(a, w, &Uuid::new_v4().to_string(), req(body))
        .await
        .unwrap()
}
pub async fn historical_issue(
    h: &Harness,
    a: &Actor,
    w: Uuid,
    t: Uuid,
    extra: Value,
    key: Option<&str>,
) -> Issue {
    let mut body = json!({"team_id":t,"title":format!("Graph issue {}",Uuid::new_v4())});
    body.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    let mut request: CreateIssue = req(body);
    request.validate().unwrap();
    assert_eq!(request.team_id, t);
    assert!(request.template.is_none());
    let key = key
        .map(str::to_owned)
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    let key_hash = format!("{:x}", Sha256::digest(key.as_bytes()));
    let request_hash = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&request).unwrap())
    );
    let document = request
        .document
        .clone()
        .unwrap_or_else(|| DocumentWrite::from_plain_text(&request.description));
    let plain = document.validate().unwrap();
    let mut tx = h.context(a.user.id, w).await;
    let schema: i64 = sqlx::query_scalar("SELECT max(version) FROM _sqlx_migrations")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert!(
        matches!(schema, 13 | 16 | 17 | 18 | 19 | 22 | 24 | 25),
        "Historical seeding is limited to its explicit upgrade drivers"
    );
    let status = match request.status_id {
        Some(status) => status,
        None => sqlx::query_scalar(
            "SELECT id FROM workflow_statuses WHERE workspace_id=$1 AND team_id=$2 AND is_default",
        )
        .bind(w)
        .bind(t)
        .fetch_one(&mut *tx)
        .await
        .unwrap(),
    };
    let number: i64 = sqlx::query_scalar("UPDATE teams SET next_issue_number=next_issue_number+1 WHERE workspace_id=$1 AND id=$2 RETURNING next_issue_number-1")
        .bind(w).bind(t).fetch_one(&mut *tx).await.unwrap();
    sqlx::query("SELECT set_config('opentrack.initial_document',$1,true)")
        .bind(document.body.to_string())
        .execute(&mut *tx)
        .await
        .unwrap();
    let id = Uuid::new_v4();
    assert_eq!(sqlx::query("INSERT INTO issues(id,workspace_id,team_id,created_by_user_id,number,title,description,status_id,priority,assignee_id,estimate,due_date,parent_id) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13)")
        .bind(id).bind(w).bind(t).bind(a.user.id).bind(number).bind(&request.title).bind(plain).bind(status).bind(request.priority).bind(request.assignee_id).bind(request.estimate).bind(request.due_date).bind(request.parent_id)
        .execute(&mut *tx).await.unwrap().rows_affected(),1);
    sqlx::query("SELECT set_config('opentrack.initial_document','',true)")
        .execute(&mut *tx)
        .await
        .unwrap();
    store::set_issue_labels(&mut tx, w, id, &request.label_ids)
        .await
        .unwrap();
    let issue = store::issue(&mut tx, w, a.user.id, id, false)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(sqlx::query("INSERT INTO idempotency_records(workspace_id,principal_id,route,key_hash,request_hash,response_body) VALUES($1,$2,'issues.create',$3,$4,$5)")
        .bind(w).bind(a.user.id).bind(key_hash).bind(request_hash).bind(serde_json::to_value(&issue).unwrap()).execute(&mut *tx).await.unwrap().rows_affected(),1);
    let mutation = request.mutation_id.unwrap_or_else(Uuid::new_v4);
    let mut events = Vec::new();
    if let Some(parent) = request.parent_id {
        let version: i64 = sqlx::query_scalar("UPDATE issues SET version=version+1,updated_at=clock_timestamp() WHERE workspace_id=$1 AND team_id=$2 AND id=$3 RETURNING version")
            .bind(w).bind(t).bind(parent).fetch_one(&mut *tx).await.unwrap();
        assert_eq!(
            sqlx::query(
                "UPDATE teams SET graph_version=graph_version+1 WHERE workspace_id=$1 AND id=$2"
            )
            .bind(w)
            .bind(t)
            .execute(&mut *tx)
            .await
            .unwrap()
            .rows_affected(),
            1
        );
        events.push(store::Event {
            workspace: w,
            team: t,
            entity: parent,
            version,
            actor: a.user.id,
            kind: "issue.parent_changed",
            mutation,
            changes: json!({"source":"manual","action":"child_created"}),
        });
    }
    if schema >= 16 {
        let document: Uuid =
            sqlx::query_scalar("SELECT id FROM documents WHERE workspace_id=$1 AND issue_id=$2")
                .bind(w)
                .bind(id)
                .fetch_one(&mut *tx)
                .await
                .unwrap();
        assert_eq!(sqlx::query("INSERT INTO audit_entries(id,workspace_id,actor_id,action,target_id) VALUES($1,$2,$3,'document.created',$4)")
            .bind(Uuid::new_v4()).bind(w).bind(a.user.id).bind(document).execute(&mut *tx).await.unwrap().rows_affected(),1);
    }
    events.push(store::Event {workspace:w,team:t,entity:id,version:1,actor:a.user.id,kind:"issue.created",mutation,
        changes:json!({"label_ids":{"before":[],"after":request.label_ids},"assignee_id":{"before":null,"after":request.assignee_id},"source":"manual"})});
    for event in &events {
        store::append_activity(&mut tx, event).await.unwrap();
    }
    store::append_stream_events(&mut tx, &events).await.unwrap();
    tx.commit().await.unwrap();
    issue
}
pub async fn fresh(h: &Harness, a: &Actor, w: Uuid, id: Uuid) -> Issue {
    h.app.get_issue(a, w, id).await.unwrap()
}
pub async fn relate(
    h: &Harness,
    a: &Actor,
    w: Uuid,
    source: Uuid,
    target: Uuid,
    kind: &str,
) -> RelationMutation {
    let s = fresh(h, a, w, source).await;
    let t = fresh(h, a, w, target).await;
    h.app
        .create_issue_relation(
            a,
            w,
            source,
            &Uuid::new_v4().to_string(),
            &s.etag(),
            req(json!({"kind":kind,"target_id":target,"target_etag":t.etag()})),
        )
        .await
        .unwrap()
}
pub async fn parent(h: &Harness, a: &Actor, w: Uuid, child: Uuid, target: Option<Uuid>) -> Issue {
    let c = fresh(h, a, w, child).await;
    let body = if let Some(p) = target {
        json!({"parent_id":p,"parent_etag":fresh(h,a,w,p).await.etag()})
    } else {
        json!({"parent_id":null})
    };
    h.app
        .set_issue_parent(a, w, child, &c.etag(), req(body))
        .await
        .unwrap()
}
pub async fn snapshot(h: &Harness, a: &Actor, w: Uuid) -> Value {
    let mut tx = h.context(a.user.id, w).await;
    let v=sqlx::query_scalar("SELECT jsonb_build_object('issues',(SELECT coalesce(jsonb_agg(to_jsonb(x) ORDER BY id),'[]') FROM issues x),'relations',(SELECT coalesce(jsonb_agg(to_jsonb(x) ORDER BY id),'[]') FROM issue_relations x),'aliases',(SELECT coalesce(jsonb_agg(to_jsonb(x) ORDER BY team_prefix,number),'[]') FROM issue_key_aliases x),'teams',(SELECT jsonb_agg(to_jsonb(x) ORDER BY id) FROM teams x),'links',(SELECT coalesce(jsonb_agg(to_jsonb(x) ORDER BY issue_id,label_id),'[]') FROM issue_labels x),'retry',(SELECT coalesce(jsonb_agg(to_jsonb(x) ORDER BY principal_id,route,key_hash),'[]') FROM idempotency_records x),'activity',(SELECT coalesce(jsonb_agg(to_jsonb(x) ORDER BY id),'[]') FROM activity_entries x),'events',(SELECT coalesce(jsonb_agg(to_jsonb(x) ORDER BY seq),'[]') FROM workspace_events x),'audit',(SELECT count(*) FROM audit_entries),'head',(SELECT last_seq FROM workspace_stream_heads WHERE workspace_id=$1))").bind(w).fetch_one(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    v
}
pub async fn fault(h: &Harness, table: &str, event: &str) {
    sqlx::raw_sql(AssertSqlSafe(format!("CREATE FUNCTION graph_suppress() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RETURN NULL; END $$; CREATE TRIGGER graph_suppress BEFORE {event} ON {table} FOR EACH ROW EXECUTE FUNCTION graph_suppress()"))).execute(&h.owner).await.unwrap();
}
pub async fn clear(h: &Harness, table: &str) {
    sqlx::raw_sql(AssertSqlSafe(format!(
        "DROP TRIGGER graph_suppress ON {table}; DROP FUNCTION graph_suppress()"
    )))
    .execute(&h.owner)
    .await
    .unwrap();
}
pub async fn state(h: &Harness, a: &Actor, w: Uuid, id: Uuid, deleted: bool) {
    let mut tx = h.context(a.user.id, w).await;
    // Model legacy pre-0014 historical endpoints for remove-only repair tests.
    // New deletion commands must detach their edges and cannot create this state.
    let guard: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_trigger WHERE tgrelid='issues'::regclass AND tgname='issues_deletion_guard')").fetch_one(&mut *tx).await.unwrap();
    if deleted && guard {
        sqlx::query("ALTER TABLE issues DISABLE TRIGGER issues_deletion_guard")
            .execute(&mut *tx)
            .await
            .unwrap();
    }

    sqlx::query("UPDATE issues SET archived_at=CASE WHEN $2 THEN archived_at ELSE clock_timestamp() END,deleted_at=CASE WHEN $2 THEN clock_timestamp() ELSE deleted_at END,version=version+1 WHERE id=$1").bind(id).bind(deleted).execute(&mut *tx).await.unwrap();
    if deleted && guard {
        sqlx::query("SET CONSTRAINTS ALL IMMEDIATE")
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("ALTER TABLE issues ENABLE TRIGGER issues_deletion_guard")
            .execute(&mut *tx)
            .await
            .unwrap();
    }

    tx.commit().await.unwrap();
}
pub async fn epoch(h: &Harness, a: &Actor, w: Uuid, t: Uuid) -> i64 {
    let mut tx = h.context(a.user.id, w).await;
    let v = sqlx::query_scalar("SELECT graph_version FROM teams WHERE id=$1")
        .bind(t)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    v
}
pub fn advanced(old: &Issue, new: &Issue) {
    assert_eq!(
        new.version.parse::<i64>().unwrap(),
        old.version.parse::<i64>().unwrap() + 1
    );
    assert_ne!(old.etag(), new.etag());
}
pub async fn gate(
    h: &Harness,
    a: &Actor,
    w: Uuid,
    t: Uuid,
    issue: Option<Uuid>,
) -> (store::Tx, i32) {
    let mut tx = h.context(a.user.id, w).await;
    if let Some(id) = issue {
        sqlx::query("SELECT id FROM issues WHERE id=$1 FOR UPDATE")
            .bind(id)
            .execute(&mut *tx)
            .await
            .unwrap();
    } else {
        sqlx::query("SELECT id FROM teams WHERE id=$1 FOR UPDATE")
            .bind(t)
            .execute(&mut *tx)
            .await
            .unwrap();
    }
    let pid = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    (tx, pid)
}
pub async fn blocked(h: &Harness, pid: i32) -> i32 {
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            if let Some(v) = sqlx::query_scalar(
                "SELECT pid FROM pg_stat_activity WHERE $1=ANY(pg_blocking_pids(pid)) LIMIT 1",
            )
            .bind(pid)
            .fetch_optional(&h.owner)
            .await
            .unwrap()
            {
                return v;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("Observe actual PostgreSQL blocking")
}
pub fn move_body(preview: &IssueMovePreview) -> MoveIssue {
    req(
        json!({"destination_team_id":preview.destination.id,"status_id":preview.destination_default_status_id,"assignee_id":null,"label_ids":preview.retained_label_ids,"preview_token":preview.preview_token}),
    )
}
pub async fn move_to(h: &Harness, a: &Actor, w: Uuid, id: Uuid, destination: Uuid) -> Issue {
    let p = h
        .app
        .preview_issue_move(a, w, id, destination)
        .await
        .unwrap();
    h.app
        .move_issue(
            a,
            w,
            id,
            &Uuid::new_v4().to_string(),
            &p.issue_etag,
            move_body(&p),
        )
        .await
        .unwrap()
}
