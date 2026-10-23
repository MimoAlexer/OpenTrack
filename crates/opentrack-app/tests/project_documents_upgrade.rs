//! Exact PostgreSQL 0025 -> 0026 upgrade and restricted-runtime invariants.
#[allow(dead_code)]
mod support;

use opentrack_domain::Actor;
use opentrack_store as store;
use serde_json::{Value, json};
use sqlx::AssertSqlSafe;
use support::Harness;
use uuid::Uuid;

fn req<T: serde::de::DeserializeOwned>(value: Value) -> T {
    serde_json::from_value(value).unwrap()
}

fn body(text: &str) -> Value {
    json!({"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":text}]}]})
}

async fn migrate(h: &Harness) -> Result<(), sqlx::migrate::MigrateError> {
    sqlx::migrate::Migrator {
        migrations: store::MIGRATOR
            .iter()
            .filter(|migration| migration.version <= 26)
            .cloned()
            .collect::<Vec<_>>()
            .into(),
        ..sqlx::migrate::Migrator::DEFAULT
    }
    .run(&h.owner)
    .await
}

async fn version(h: &Harness) -> i64 {
    sqlx::query_scalar("SELECT max(version) FROM _sqlx_migrations")
        .fetch_one(&h.owner)
        .await
        .unwrap()
}

async fn setup(h: &Harness) -> (Actor, Uuid, Uuid, Uuid) {
    let (session, token) = h
        .app
        .setup(req(json!({
            "setup_token":h.setup_token,
            "email":"owner@project-upgrade.example.test",
            "display_name":"Example Owner",
            "password":"Synthetic project upgrade password 2026!",
            "workspace_name":"Project upgrade",
            "team_name":"Planning",
            "team_key":"PLN"
        })))
        .await
        .unwrap();
    let actor = h.app.authenticate(&token).await.unwrap();
    let workspace = session.workspaces[0].id;
    let mut tx = h.context(actor.user.id, workspace).await;
    let (team, organization): (Uuid, Uuid) = sqlx::query_as(
        "SELECT t.id,w.organization_id FROM teams t JOIN workspaces w ON w.id=t.workspace_id WHERE w.id=$1",
    )
    .bind(workspace)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    (actor, workspace, team, organization)
}

struct LegacyProject {
    id: Uuid,
    workspace: Uuid,
    team: Uuid,
    schema: i32,
    body: Value,
    plain: String,
    archived: bool,
}

struct Fixture {
    user: Uuid,
    projects: Vec<LegacyProject>,
    issue: Uuid,
}

async fn insert_project(tx: &mut store::Tx, user: Uuid, project: &LegacyProject) {
    sqlx::query("INSERT INTO projects(id,workspace_id,team_id,name,summary,narrative_schema_version,narrative_body,narrative_plain_text,lead_id,state,health,version,start_date,target_date,archived_at,created_at,updated_at) VALUES($1,$2,$3,$4,'Existing summary',$5,$6,$7,$8,'active','at_risk',7,'2026-10-01','2026-12-15',CASE WHEN $9 THEN clock_timestamp()-interval '7 days' END,clock_timestamp()-interval '21 days',clock_timestamp()-interval '12 days')")
        .bind(project.id)
        .bind(project.workspace)
        .bind(project.team)
        .bind(format!("Existing project {}", project.id))
        .bind(project.schema)
        .bind(&project.body)
        .bind(&project.plain)
        .bind(user)
        .bind(project.archived)
        .execute(&mut **tx)
        .await
        .unwrap();
}

async fn seed_legacy(h: &Harness) -> Fixture {
    assert_eq!(version(h).await, 25);
    let (actor, workspace, team, organization) = setup(h).await;
    let private = h
        .app
        .create_team(
            &actor,
            workspace,
            "project-upgrade-private-team",
            req(json!({"name":"Private planning","key":"PRV","private":true})),
        )
        .await
        .unwrap();
    let other = h
        .app
        .create_workspace(
            &actor,
            "project-upgrade-other-workspace",
            req(json!({"organization_id":organization,"name":"Other planning","team_name":"Other team","team_key":"OTH"})),
        )
        .await
        .unwrap();
    let mut tx = h.context(actor.user.id, other.id).await;
    let other_team = sqlx::query_scalar("SELECT id FROM teams WHERE workspace_id=$1")
        .bind(other.id)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let projects = vec![
        LegacyProject {
            id: Uuid::new_v4(),
            workspace,
            team,
            schema: 1,
            body: body("Existing résumé\r\n保留 🧭"),
            plain: "Existing résumé\r\n保留 🧭".into(),
            archived: false,
        },
        LegacyProject {
            id: Uuid::new_v4(),
            workspace,
            team: private.id,
            schema: 99,
            body: json!({"type":"doc","content":[
                {"type":"attachment","attrs":{"id":"legacy-not-a-uuid"}},
                {"type":"paragraph","content":[{"type":"userMention","attrs":{"id":"legacy-not-a-uuid","label":"Preserved legacy label"}}]}
            ],"legacy":{"unrecognized":true}}),
            plain: "Exact legacy plain text\r\nwith an independent representation".into(),
            archived: true,
        },
        LegacyProject {
            id: Uuid::new_v4(),
            workspace: other.id,
            team: other_team,
            schema: 1,
            body: json!({"type":"doc","content":[]}),
            plain: String::new(),
            archived: false,
        },
    ];
    // Current project/document App methods require 0026; seed actual 0025 rows.
    for project in &projects {
        let mut tx = h.context(actor.user.id, project.workspace).await;
        insert_project(&mut tx, actor.user.id, project).await;
        tx.commit().await.unwrap();
    }
    let issue = Uuid::new_v4();
    let comment = Uuid::new_v4();
    let milestone = Uuid::new_v4();
    let mut tx = h.context(actor.user.id, workspace).await;
    sqlx::query("INSERT INTO issues(id,workspace_id,team_id,created_by_user_id,number,title,description,status_id,priority,estimate) SELECT $1,$2,$3,$4,1,'Existing issue','Existing issue body',id,2,5 FROM workflow_statuses WHERE workspace_id=$2 AND team_id=$3 AND is_default")
        .bind(issue).bind(workspace).bind(team).bind(actor.user.id)
        .execute(&mut *tx).await.unwrap();
    sqlx::query("UPDATE teams SET next_issue_number=2 WHERE workspace_id=$1 AND id=$2")
        .bind(workspace)
        .bind(team)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("INSERT INTO comments(id,workspace_id,issue_id,origin_team_id,author_id) VALUES($1,$2,$3,$4,$5)")
        .bind(comment).bind(workspace).bind(issue).bind(team).bind(actor.user.id)
        .execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO documents(id,workspace_id,comment_id,schema_version,body,plain_text,updated_by_user_id) VALUES($1,$2,$3,1,$4,'Existing comment body',$5)")
        .bind(Uuid::new_v4()).bind(workspace).bind(comment).bind(body("Existing comment body")).bind(actor.user.id)
        .execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO milestones(id,workspace_id,team_id,project_id,title,rank) VALUES($1,$2,$3,$4,'Existing outcome',1)")
        .bind(milestone).bind(workspace).bind(team).bind(projects[0].id)
        .execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO project_issues(workspace_id,team_id,project_id,issue_id,milestone_id) VALUES($1,$2,$3,$4,$5)")
        .bind(workspace).bind(team).bind(projects[0].id).bind(issue).bind(milestone)
        .execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO project_updates(id,workspace_id,team_id,project_id,author_id,body,state,health,completed,eligible) VALUES($1,$2,$3,$4,$5,'Existing immutable update','active','at_risk',0,1)")
        .bind(Uuid::new_v4()).bind(workspace).bind(team).bind(projects[0].id).bind(actor.user.id)
        .execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    Fixture {
        user: actor.user.id,
        projects,
        issue,
    }
}

async fn legacy_snapshot(h: &Harness, user: Uuid, workspace: Uuid) -> Value {
    let mut tx = h.context(user, workspace).await;
    let mut snapshot = serde_json::Map::new();
    for table in [
        "projects",
        "project_issues",
        "project_updates",
        "milestones",
        "issues",
        "comments",
        "teams",
        "issue_subscriptions",
        "subscription_events",
        "document_mentions",
        "discussion_sources",
        "attachments",
        "document_attachments",
        "document_attachment_revisions",
        "audit_entries",
        "activity_entries",
        "workspace_events",
        "workspace_stream_heads",
    ] {
        // Names are from this fixed list, never fixture or runtime input.
        let rows: Value = sqlx::query_scalar(AssertSqlSafe(format!(
            "SELECT coalesce(jsonb_agg(to_jsonb(x) ORDER BY to_jsonb(x)::text),'[]'::jsonb) FROM {table} x"
        )))
        .fetch_one(&mut *tx)
        .await
        .unwrap();
        snapshot.insert(table.into(), rows);
    }
    for (key, query) in [
        (
            "documents",
            "SELECT coalesce(jsonb_agg(to_jsonb(d)-'project_id'-'migrated_initial' ORDER BY d.id),'[]'::jsonb) FROM documents d WHERE d.issue_id IS NOT NULL OR d.comment_id IS NOT NULL",
        ),
        (
            "revisions",
            "SELECT coalesce(jsonb_agg(to_jsonb(r) ORDER BY r.document_id,r.revision),'[]'::jsonb) FROM document_revisions r JOIN documents d ON d.workspace_id=r.workspace_id AND d.id=r.document_id WHERE d.issue_id IS NOT NULL OR d.comment_id IS NOT NULL",
        ),
        (
            "mention_intents",
            "SELECT coalesce(jsonb_agg(to_jsonb(m)-'project_id' ORDER BY m.id),'[]'::jsonb) FROM mention_intents m",
        ),
    ] {
        snapshot.insert(
            key.into(),
            sqlx::query_scalar(query).fetch_one(&mut *tx).await.unwrap(),
        );
    }
    tx.commit().await.unwrap();
    Value::Object(snapshot)
}

async fn assert_backfill(h: &Harness, fixture: &Fixture) {
    let mut captured_at = None;
    for project in &fixture.projects {
        let mut tx = h.context(fixture.user, project.workspace).await;
        let result: Value = sqlx::query_scalar("SELECT jsonb_build_object('schema',d.schema_version,'body',d.body,'plain',d.plain_text,'version',d.version,'migrated',d.migrated_initial,'editor',d.updated_by_user_id,'issue',d.issue_id,'comment',d.comment_id,'created',d.created_at,'updated',d.updated_at,'after_legacy',d.created_at>p.updated_at,'origin',r.origin_team_id,'revision',r.revision,'revision_schema',r.schema_version,'revision_body',r.body,'revision_plain',r.plain_text,'revision_author',r.author_id,'revision_created',r.created_at,'revision_count',(SELECT count(*) FROM document_revisions all_r WHERE all_r.workspace_id=d.workspace_id AND all_r.document_id=d.id)) FROM projects p JOIN documents d ON d.workspace_id=p.workspace_id AND d.project_id=p.id JOIN document_revisions r ON r.workspace_id=d.workspace_id AND r.document_id=d.id AND r.revision=1 WHERE p.workspace_id=$1 AND p.id=$2 AND d.deleted_at IS NULL")
            .bind(project.workspace).bind(project.id).fetch_one(&mut *tx).await.unwrap();
        assert_eq!(result["schema"], project.schema);
        assert_eq!(result["body"], project.body);
        assert_eq!(result["plain"], project.plain);
        assert_eq!(result["version"], 1);
        assert_eq!(result["migrated"], true);
        assert!(result["editor"].is_null());
        assert!(result["issue"].is_null() && result["comment"].is_null());
        assert_eq!(result["after_legacy"], true);
        assert_eq!(result["origin"], json!(project.team));
        assert_eq!(result["revision"], 1);
        assert_eq!(result["revision_schema"], project.schema);
        assert_eq!(result["revision_body"], project.body);
        assert_eq!(result["revision_plain"], project.plain);
        assert!(result["revision_author"].is_null());
        assert_eq!(result["revision_created"], result["created"]);
        assert_eq!(result["updated"], result["created"]);
        assert_eq!(result["revision_count"], 1);
        if let Some(previous) = &captured_at {
            assert_eq!(&result["created"], previous);
        } else {
            captured_at = Some(result["created"].clone());
        }
        let count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM documents WHERE workspace_id=$1 AND project_id IS NOT NULL",
        )
        .bind(project.workspace)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
        let expected = fixture
            .projects
            .iter()
            .filter(|p| p.workspace == project.workspace)
            .count();
        assert_eq!(count, expected as i64);
        tx.commit().await.unwrap();
    }
}

async fn runtime_context(h: &Harness, user: Uuid, workspace: Uuid) -> store::Tx {
    let mut tx = store::begin(&h.app.pool).await.unwrap();
    store::context(&mut tx, user, Some(workspace))
        .await
        .unwrap();
    tx
}

async fn denied(
    h: &Harness,
    user: Uuid,
    workspace: Uuid,
    sql: &'static str,
    ids: &[Uuid],
    code: &str,
) {
    let mut tx = runtime_context(h, user, workspace).await;
    let mut query = sqlx::query(sql);
    for id in ids {
        query = query.bind(*id);
    }
    let error = query.execute(&mut *tx).await.unwrap_err();
    tx.rollback().await.unwrap();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some(code)
    );
}

#[tokio::test]
async fn populated_25_to_26_preserves_exact_snapshots_and_issue_comment_state() {
    let h = Harness::with_version(5, 25).await;
    let copy = h.clone();
    let run = tokio::spawn(async move { populated_upgrade(&copy).await }).await;
    h.cleanup().await;
    if let Err(error) = run {
        std::panic::resume_unwind(error.into_panic());
    }
}

async fn populated_upgrade(h: &Harness) {
    let fixture = seed_legacy(h).await;
    let primary = &fixture.projects[0];
    let other = &fixture.projects[2];
    let before = legacy_snapshot(h, fixture.user, primary.workspace).await;
    let other_before = legacy_snapshot(h, fixture.user, other.workspace).await;
    migrate(h).await.unwrap();
    assert_eq!(version(h).await, 26);
    assert_backfill(h, &fixture).await;
    assert_eq!(
        legacy_snapshot(h, fixture.user, primary.workspace).await,
        before
    );
    assert_eq!(
        legacy_snapshot(h, fixture.user, other.workspace).await,
        other_before
    );
    // Re-running the migration ledger must not add snapshots or authored effects.
    migrate(h).await.unwrap();
    assert_backfill(h, &fixture).await;
    assert_eq!(
        legacy_snapshot(h, fixture.user, primary.workspace).await,
        before
    );

    let mut tx = runtime_context(h, fixture.user, primary.workspace).await;
    let restricted: bool = sqlx::query_scalar("SELECT NOT (r.rolsuper OR r.rolbypassrls) AND current_user<>pg_get_userbyid(c.relowner) FROM pg_roles r CROSS JOIN pg_class c WHERE r.rolname=current_user AND c.oid='documents'::regclass")
        .fetch_one(&mut *tx).await.unwrap();
    assert!(restricted);
    let document: Uuid = sqlx::query_scalar("SELECT id FROM documents WHERE project_id=$1")
        .bind(primary.id)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    let concealed: i64 = sqlx::query_scalar("SELECT count(*) FROM documents WHERE workspace_id=$1")
        .bind(other.workspace)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert_eq!(concealed, 0, "The runtime workspace policy remains active");
    let existing_authorship: bool = sqlx::query_scalar("SELECT bool_and(NOT d.migrated_initial AND d.project_id IS NULL AND d.updated_by_user_id IS NOT NULL AND r.author_id IS NOT NULL) FROM documents d JOIN document_revisions r ON r.workspace_id=d.workspace_id AND r.document_id=d.id WHERE d.issue_id IS NOT NULL OR d.comment_id IS NOT NULL")
        .fetch_one(&mut *tx).await.unwrap();
    assert!(
        existing_authorship,
        "The migration exception must not affect issue/comment authorship"
    );
    tx.commit().await.unwrap();

    for (sql, ids, code) in [
        (
            "INSERT INTO documents(id,workspace_id,project_id,schema_version,body,plain_text,migrated_initial) VALUES(gen_random_uuid(),$1,$2,1,'{}','',true)",
            vec![primary.workspace, primary.id],
            "42501",
        ),
        (
            "INSERT INTO documents(id,workspace_id,project_id,schema_version,body,plain_text,updated_by_user_id,migrated_initial) VALUES(gen_random_uuid(),$1,$2,1,'{}','',$3,true)",
            vec![primary.workspace, primary.id, fixture.user],
            "42501",
        ),
        (
            "INSERT INTO documents(id,workspace_id,project_id,schema_version,body,plain_text) VALUES(gen_random_uuid(),$1,$2,1,'{}','')",
            vec![primary.workspace, primary.id],
            "23514",
        ),
        (
            "INSERT INTO documents(id,workspace_id,project_id,issue_id,schema_version,body,plain_text,updated_by_user_id) VALUES(gen_random_uuid(),$1,$2,$3,1,'{}','',$4)",
            vec![primary.workspace, primary.id, fixture.issue, fixture.user],
            "23514",
        ),
        (
            "INSERT INTO document_revisions(workspace_id,document_id,revision,origin_team_id,schema_version,body,plain_text,author_id,created_at) SELECT d.workspace_id,d.id,1,p.team_id,d.schema_version,d.body,d.plain_text,NULL,d.updated_at FROM documents d JOIN projects p ON p.workspace_id=d.workspace_id AND p.id=d.project_id WHERE d.id=$1",
            vec![document],
            "42501",
        ),
        (
            "INSERT INTO document_revisions(workspace_id,document_id,revision,origin_team_id,schema_version,body,plain_text,author_id,created_at) SELECT d.workspace_id,d.id,2,p.team_id,d.schema_version,d.body,d.plain_text,NULL,d.updated_at FROM documents d JOIN projects p ON p.workspace_id=d.workspace_id AND p.id=d.project_id WHERE d.id=$1",
            vec![document],
            "42501",
        ),
        (
            "UPDATE documents SET migrated_initial=false,updated_by_user_id=$2,version=version+1 WHERE id=$1",
            vec![document, fixture.user],
            "23514",
        ),
        (
            "UPDATE documents SET project_id=$2,updated_by_user_id=$3,version=version+1 WHERE id=$1",
            vec![document, fixture.projects[1].id, fixture.user],
            "23514",
        ),
        (
            "UPDATE documents SET updated_by_user_id=NULL,version=version+1 WHERE id=$1",
            vec![document],
            "23514",
        ),
        (
            "UPDATE documents SET updated_by_user_id=$2,version=version+1 WHERE id=$1",
            vec![document, Uuid::new_v4()],
            "23503",
        ),
        (
            "UPDATE document_revisions SET author_id=$2 WHERE document_id=$1 AND revision=1",
            vec![document, fixture.user],
            "42501",
        ),
        (
            "UPDATE projects SET narrative_schema_version=narrative_schema_version+1,version=version+1 WHERE id=$1",
            vec![primary.id],
            "23514",
        ),
        (
            "UPDATE projects SET narrative_body='{}',version=version+1 WHERE id=$1",
            vec![primary.id],
            "23514",
        ),
        (
            "UPDATE projects SET narrative_plain_text='Forged legacy snapshot',version=version+1 WHERE id=$1",
            vec![primary.id],
            "23514",
        ),
    ] {
        denied(h, fixture.user, primary.workspace, sql, &ids, code).await;
    }
    denied(h, fixture.user, other.workspace,
        "INSERT INTO documents(id,workspace_id,project_id,schema_version,body,plain_text,updated_by_user_id) VALUES(gen_random_uuid(),$1,$2,1,'{}','',$3)",
        &[other.workspace, primary.id, fixture.user], "23503").await;
    assert_backfill(h, &fixture).await;
    assert_eq!(
        legacy_snapshot(h, fixture.user, primary.workspace).await,
        before
    );

    let mut tx = runtime_context(h, fixture.user, primary.workspace).await;
    sqlx::query("UPDATE documents SET schema_version=1,body=$2,plain_text='Authored after upgrade',updated_by_user_id=$3,updated_at=clock_timestamp(),version=version+1 WHERE id=$1")
        .bind(document).bind(body("Authored after upgrade")).bind(fixture.user)
        .execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    let mut tx = h.context(fixture.user, primary.workspace).await;
    let authored: Value = sqlx::query_scalar("SELECT jsonb_build_object('version',d.version,'migrated',d.migrated_initial,'editor',d.updated_by_user_id,'body',d.body,'authors',(SELECT jsonb_agg(r.author_id ORDER BY r.revision) FROM document_revisions r WHERE r.workspace_id=d.workspace_id AND r.document_id=d.id),'bodies',(SELECT jsonb_agg(r.body ORDER BY r.revision) FROM document_revisions r WHERE r.workspace_id=d.workspace_id AND r.document_id=d.id)) FROM documents d WHERE d.id=$1")
        .bind(document).fetch_one(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    assert_eq!(authored["version"], 2);
    assert_eq!(
        authored["migrated"], true,
        "Provenance remains immutable after editing"
    );
    assert_eq!(authored["editor"], json!(fixture.user));
    assert_eq!(authored["body"], body("Authored after upgrade"));
    assert_eq!(authored["authors"], json!([null, fixture.user]));
    assert_eq!(
        authored["bodies"],
        json!([primary.body, body("Authored after upgrade")])
    );
    denied(
        h,
        fixture.user,
        primary.workspace,
        "UPDATE documents SET updated_by_user_id=NULL,version=version+1 WHERE id=$1",
        &[document],
        "23514",
    )
    .await;
    assert_eq!(
        legacy_snapshot(h, fixture.user, primary.workspace).await,
        before,
        "An authored project brief must not overwrite legacy snapshots or issue/comment state"
    );
}

#[tokio::test]
async fn fresh_26_requires_one_live_scoped_owner_and_real_authorship() {
    let h = Harness::with_version(5, 26).await;
    let copy = h.clone();
    let run = tokio::spawn(async move { fresh_schema(&copy).await }).await;
    h.cleanup().await;
    if let Err(error) = run {
        std::panic::resume_unwind(error.into_panic());
    }
}

async fn fresh_schema(h: &Harness) {
    assert_eq!(version(h).await, 26);
    let (actor, workspace, team, _) = setup(h).await;
    let project = LegacyProject {
        id: Uuid::new_v4(),
        workspace,
        team,
        schema: 1,
        body: body("Initial authored brief"),
        plain: "Initial authored brief".into(),
        archived: false,
    };
    let document = Uuid::new_v4();
    let mut tx = runtime_context(h, actor.user.id, workspace).await;
    insert_project(&mut tx, actor.user.id, &project).await;
    sqlx::query("INSERT INTO documents(id,workspace_id,project_id,schema_version,body,plain_text,updated_by_user_id) VALUES($1,$2,$3,1,$4,$5,$6)")
        .bind(document).bind(workspace).bind(project.id).bind(&project.body).bind(&project.plain).bind(actor.user.id)
        .execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    let mut tx = runtime_context(h, actor.user.id, workspace).await;
    let authored: bool = sqlx::query_scalar("SELECT NOT d.migrated_initial AND d.version=1 AND d.updated_by_user_id=$2 AND r.author_id=$2 AND r.body=d.body AND r.plain_text=d.plain_text AND r.created_at=d.updated_at AND r.origin_team_id=$3 FROM documents d JOIN document_revisions r ON r.workspace_id=d.workspace_id AND r.document_id=d.id AND r.revision=1 WHERE d.id=$1")
        .bind(document).bind(actor.user.id).bind(team).fetch_one(&mut *tx).await.unwrap();
    assert!(authored);
    tx.commit().await.unwrap();
    for table in ["documents", "document_revisions", "mention_intents"] {
        let rls: bool = sqlx::query_scalar(
            "SELECT relrowsecurity AND relforcerowsecurity FROM pg_class WHERE oid=to_regclass($1)",
        )
        .bind(table)
        .fetch_one(&h.owner)
        .await
        .unwrap();
        assert!(rls, "Workspace isolation must remain forced on {table}");
    }
    let rights: bool = sqlx::query_scalar("SELECT (SELECT bool_and(has_table_privilege(current_user,'documents',p)) FROM unnest(ARRAY['SELECT','INSERT','UPDATE']) p) AND (SELECT bool_and(has_table_privilege(current_user,'document_revisions',p)) FROM unnest(ARRAY['SELECT','INSERT']) p) AND NOT has_table_privilege(current_user,'document_revisions','UPDATE,DELETE,TRUNCATE')")
        .fetch_one(&h.app.pool).await.unwrap();
    assert!(rights);
    for (sql, ids, code) in [
        (
            "INSERT INTO documents(id,workspace_id,project_id,schema_version,body,plain_text,updated_by_user_id) VALUES(gen_random_uuid(),$1,$2,1,'{}','',$3)",
            vec![workspace, project.id, actor.user.id],
            "23505",
        ),
        (
            "INSERT INTO documents(id,workspace_id,schema_version,body,plain_text,updated_by_user_id) VALUES(gen_random_uuid(),$1,1,'{}','',$2)",
            vec![workspace, actor.user.id],
            "23514",
        ),
        (
            "UPDATE documents SET migrated_initial=true,version=version+1 WHERE id=$1",
            vec![document],
            "23514",
        ),
        (
            "UPDATE documents SET updated_by_user_id=NULL,version=version+1 WHERE id=$1",
            vec![document],
            "23514",
        ),
    ] {
        denied(h, actor.user.id, workspace, sql, &ids, code).await;
    }
    let mut tx = runtime_context(h, actor.user.id, workspace).await;
    let orphan = LegacyProject {
        id: Uuid::new_v4(),
        ..project
    };
    insert_project(&mut tx, actor.user.id, &orphan).await;
    let error = sqlx::query("SET CONSTRAINTS ALL IMMEDIATE")
        .execute(&mut *tx)
        .await
        .unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("23514")
    );
    tx.rollback().await.unwrap();
    let mut tx = runtime_context(h, actor.user.id, workspace).await;
    sqlx::query("UPDATE documents SET deleted_at=clock_timestamp(),version=version+1 WHERE id=$1")
        .bind(document)
        .execute(&mut *tx)
        .await
        .unwrap();
    let error = sqlx::query("SET CONSTRAINTS ALL IMMEDIATE")
        .execute(&mut *tx)
        .await
        .unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("23514")
    );
    tx.rollback().await.unwrap();
    let mut tx = runtime_context(h, actor.user.id, workspace).await;
    let counts: (i64, i64, i64) = sqlx::query_as("SELECT (SELECT count(*) FROM projects),(SELECT count(*) FROM documents WHERE project_id IS NOT NULL AND deleted_at IS NULL),(SELECT count(*) FROM document_revisions)")
        .fetch_one(&mut *tx).await.unwrap();
    assert_eq!(
        counts,
        (1, 1, 1),
        "Rejected owner mutations must roll back their revision effects"
    );
    tx.commit().await.unwrap();
}

async fn schema_snapshot(h: &Harness) -> Value {
    sqlx::query_scalar("SELECT jsonb_build_object('migrations',(SELECT jsonb_agg(to_jsonb(m) ORDER BY version) FROM _sqlx_migrations m),'columns',(SELECT jsonb_agg(jsonb_build_array(a.attrelid::regclass::text,a.attname,a.atttypid,a.attnotnull) ORDER BY a.attrelid::regclass::text,a.attnum) FROM pg_attribute a WHERE a.attrelid IN('projects'::regclass,'documents'::regclass,'document_revisions'::regclass,'mention_intents'::regclass) AND a.attnum>0 AND NOT a.attisdropped),'functions',(SELECT jsonb_agg(pg_get_functiondef(p.oid) ORDER BY p.proname) FROM pg_proc p WHERE p.pronamespace=current_schema()::regnamespace AND p.proname IN('project_guard','document_identity_guard','document_revision_source','document_effects','document_required_owner','document_attachment_sync','document_mention_sync')),'constraints',(SELECT jsonb_agg(jsonb_build_array(c.conrelid::regclass::text,c.conname,pg_get_constraintdef(c.oid)) ORDER BY c.conrelid::regclass::text,c.conname) FROM pg_constraint c WHERE c.conrelid IN('projects'::regclass,'documents'::regclass,'document_revisions'::regclass,'mention_intents'::regclass)),'triggers',(SELECT jsonb_agg(pg_get_triggerdef(t.oid) ORDER BY t.tgrelid::regclass::text,t.tgname) FROM pg_trigger t WHERE t.tgrelid IN('projects'::regclass,'documents'::regclass) AND NOT t.tgisinternal))")
        .fetch_one(&h.owner).await.unwrap()
}

#[tokio::test]
async fn failed_26_backfill_rolls_back_schema_data_and_migration_ledger_before_retry() {
    let h = Harness::with_version(5, 25).await;
    let copy = h.clone();
    let run = tokio::spawn(async move { failed_upgrade(&copy).await }).await;
    h.cleanup().await;
    if let Err(error) = run {
        std::panic::resume_unwind(error.into_panic());
    }
}

async fn failed_upgrade(h: &Harness) {
    let fixture = seed_legacy(h).await;
    // Suppress one required row after the real migration has changed its schema.
    // This tests failed-upgrade atomicity, not a down-migration or backup restore.
    sqlx::raw_sql("CREATE FUNCTION test_project_backfill_failure() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF to_jsonb(NEW)->>'project_id'=TG_ARGV[0] THEN RETURN NULL; END IF; RETURN NEW; END $$")
        .execute(&h.owner).await.unwrap();
    sqlx::query(AssertSqlSafe(format!(
        "CREATE TRIGGER test_project_backfill_failure BEFORE INSERT ON documents FOR EACH ROW EXECUTE FUNCTION test_project_backfill_failure('{}')",
        fixture.projects[0].id
    )))
    .execute(&h.owner).await.unwrap();
    let schema_before = schema_snapshot(h).await;
    let before = legacy_snapshot(h, fixture.user, fixture.projects[0].workspace).await;
    let other_before = legacy_snapshot(h, fixture.user, fixture.projects[2].workspace).await;
    let error = migrate(h).await.unwrap_err();
    assert!(
        error
            .to_string()
            .contains("Project document backfill incomplete")
    );
    assert_eq!(version(h).await, 25);
    assert_eq!(schema_snapshot(h).await, schema_before);
    assert_eq!(
        legacy_snapshot(h, fixture.user, fixture.projects[0].workspace).await,
        before
    );
    assert_eq!(
        legacy_snapshot(h, fixture.user, fixture.projects[2].workspace).await,
        other_before
    );
    sqlx::raw_sql("DROP TRIGGER test_project_backfill_failure ON documents; DROP FUNCTION test_project_backfill_failure()")
        .execute(&h.owner).await.unwrap();
    migrate(h).await.unwrap();
    assert_eq!(version(h).await, 26);
    assert_backfill(h, &fixture).await;
    assert_eq!(
        legacy_snapshot(h, fixture.user, fixture.projects[0].workspace).await,
        before
    );
    assert_eq!(
        legacy_snapshot(h, fixture.user, fixture.projects[2].workspace).await,
        other_before
    );
}
