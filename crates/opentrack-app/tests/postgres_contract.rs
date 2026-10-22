//! Real PostgreSQL contract tests. Each run owns one schema in a database whose
//! name must end in `_test`; no existing application rows are deleted or reset.
use std::collections::HashSet;

use opentrack_app::App;
use opentrack_domain::{Actor, CreateIssue, Error, IssueFilter, Login, Setup};
use opentrack_store::{self as store, Tx};
use serde_json::json;
use sqlx::{AssertSqlSafe, PgPool, postgres::PgPoolOptions};
use url::Url;
use uuid::Uuid;

const PASSWORD: &str = "integration test passphrase 724!";

#[derive(Clone)]
struct Harness {
    app: App,
    owner: PgPool,
    schema: String,
    setup_token: String,
}

impl Harness {
    async fn new() -> Self {
        let runtime = test_url("OPENTRACK_TEST_DATABASE_URL");
        let owner = test_url("OPENTRACK_TEST_MIGRATION_DATABASE_URL");
        assert_eq!(
            runtime.path(),
            owner.path(),
            "Test URLs must use the same database"
        );
        assert_ne!(
            runtime.username(),
            owner.username(),
            "Runtime and migration roles must differ"
        );
        let base_owner = PgPoolOptions::new()
            .max_connections(1)
            .connect(owner.as_str())
            .await
            .expect("Connect to the isolated test database as migration owner");
        let schema = format!("contract_{}", Uuid::new_v4().simple());
        // Identifiers cannot be bound. `schema` is a fixed prefix plus UUID hex;
        // the role below is a quoted identifier with every quote doubled.
        sqlx::query(AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
            .execute(&base_owner)
            .await
            .expect("Create an isolated contract-test schema");
        let owner = PgPoolOptions::new()
            .max_connections(2)
            .connect(scoped_url(owner, &schema).as_str())
            .await
            .expect("Connect owner pool with isolated search path");
        store::MIGRATOR
            .run(&owner)
            .await
            .expect("Migrate the isolated schema");
        let role = runtime.username().replace('"', "\"\"");
        for statement in [
            format!("GRANT USAGE ON SCHEMA {schema} TO \"{role}\""),
            format!(
                "GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA {schema} TO \"{role}\""
            ),
            format!("GRANT USAGE, SELECT ON ALL SEQUENCES IN SCHEMA {schema} TO \"{role}\""),
            format!(
                "REVOKE INSERT,UPDATE,DELETE ON {schema}.workspace_stream_heads FROM \"{role}\""
            ),
            format!(
                "GRANT INSERT(workspace_id),UPDATE(last_seq) ON {schema}.workspace_stream_heads TO \"{role}\""
            ),
            format!("REVOKE UPDATE, DELETE, TRUNCATE ON {schema}.audit_entries FROM \"{role}\""),
            format!(
                "REVOKE UPDATE, DELETE, TRUNCATE ON {schema}.account_security_events FROM \"{role}\""
            ),
            format!("REVOKE DELETE, TRUNCATE ON {schema}.issue_templates FROM \"{role}\""),
            format!(
                "REVOKE ALL ON {schema}.document_mentions,{schema}.mention_intents,{schema}.discussion_sources,{schema}.subscription_events,{schema}.issue_subscriptions FROM \"{role}\""
            ),
            format!(
                "GRANT SELECT,INSERT ON {schema}.issue_subscriptions,{schema}.subscription_events TO \"{role}\""
            ),
            format!(
                "GRANT UPDATE(preference,version,updated_at) ON {schema}.issue_subscriptions TO \"{role}\""
            ),
            format!(
                "GRANT EXECUTE ON FUNCTION {schema}.mention_eligible(uuid,uuid,uuid,uuid) TO \"{role}\""
            ),
            // Match production: serving credentials cannot inspect or edit
            // required-consumer state after broad fixture table provisioning.
            format!(
                "REVOKE ALL ON {schema}.assignment_reconciliations,{schema}.consumer_registrations,{schema}.consumer_checkpoints,{schema}.consumer_receipts,{schema}.consumer_dead_letters,{schema}.consumer_alerts,{schema}.event_retention_state,{schema}.consumer_operations,{schema}.document_retention_operations,{schema}.attachment_storage_operations FROM \"{role}\""
            ),
            format!(
                "REVOKE UPDATE, DELETE, TRUNCATE ON {schema}.document_revisions,{schema}.document_attachments,{schema}.document_attachment_revisions,{schema}.issue_template_sources FROM \"{role}\""
            ),
        ] {
            sqlx::query(AssertSqlSafe(statement))
                .execute(&base_owner)
                .await
                .expect("Grant restricted runtime access");
        }
        base_owner.close().await;
        let setup_token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        let app = App::connect(scoped_url(runtime, &schema).as_str(), &setup_token, 5, 2)
            .await
            .expect("Connect the application using the restricted runtime role");
        Self {
            app,
            owner,
            schema,
            setup_token,
        }
    }

    async fn context(&self, actor: Uuid, workspace: Uuid) -> Tx {
        let mut tx = store::begin(&self.owner)
            .await
            .expect("Begin fixture transaction");
        store::context(&mut tx, actor, Some(workspace))
            .await
            .expect("Set explicit fixture tenant context");
        tx
    }

    async fn cleanup(&self) {
        self.app.pool.close().await;
        // This is the generated UUID schema owned exclusively by this harness.
        sqlx::query(AssertSqlSafe(format!(
            "DROP SCHEMA {} CASCADE",
            self.schema
        )))
        .execute(&self.owner)
        .await
        .expect("Remove only this test run's schema");
        self.owner.close().await;
    }
}

fn test_url(variable: &str) -> Url {
    let value = std::env::var(variable).unwrap_or_else(|_| {
        panic!(
            "{variable} is required; run scripts/dev-db.sh test for real PostgreSQL verification"
        )
    });
    let url = Url::parse(&value).expect("Test connection URL must parse");
    assert!(
        matches!(url.scheme(), "postgres" | "postgresql"),
        "PostgreSQL is required"
    );
    assert!(
        url.path().trim_start_matches('/').ends_with("_test"),
        "Refusing to create fixtures outside a database ending in _test"
    );
    url
}

fn scoped_url(mut url: Url, schema: &str) -> Url {
    assert!(
        url.query_pairs().all(|(key, _)| key != "options"),
        "Test URL must not already override PostgreSQL options"
    );
    url.query_pairs_mut()
        .append_pair("options", &format!("-csearch_path={schema}"));
    url
}

fn setup(token: &str) -> Setup {
    Setup {
        setup_token: token.into(),
        email: "owner@contract.invalid".into(),
        password: PASSWORD.into(),
        display_name: "Workspace owner".into(),
        workspace_name: "Contract workspace".into(),
        team_name: "Engineering".into(),
        team_key: "ENG".into(),
    }
}

fn issue(team: Uuid, title: &str) -> CreateIssue {
    CreateIssue {
        team_id: team,
        title: title.into(),
        label_ids: Vec::new(),
        parent_id: None,
        parent_etag: None,
        description: "Real database contract fixture".into(),
        document: None,
        template: None,
        status_id: None,
        priority: 2,
        assignee_id: None,
        estimate: Some(3),
        due_date: None,
        mutation_id: Some(Uuid::new_v4()),
    }
}

async fn sign_in(app: &App, email: &str) -> (Actor, String) {
    let (_, token) = app
        .login(Login {
            email: email.into(),
            password: PASSWORD.into(),
        })
        .await
        .expect("Sign in with the fixture's actual Argon2 password");
    let actor = app
        .authenticate(&token)
        .await
        .expect("Authenticate the new session");
    (actor, token)
}

struct AdditionalFixtures {
    viewer: Actor,
    private_member: Actor,
    private_team: Uuid,
    outsider: Actor,
    other_workspace: Uuid,
    other_team: Uuid,
}

async fn fixtures(h: &Harness, owner: &Actor, workspace: Uuid) -> AdditionalFixtures {
    let viewer_id = Uuid::new_v4();
    let member_id = Uuid::new_v4();
    let outsider_id = Uuid::new_v4();
    let mut tx = h.context(owner.user.id, workspace).await;
    for (id, email, name) in [
        (viewer_id, "viewer@contract.invalid", "Read only viewer"),
        (member_id, "private@contract.invalid", "Private team member"),
        (
            outsider_id,
            "outsider@contract.invalid",
            "Other workspace owner",
        ),
    ] {
        sqlx::query("INSERT INTO users(id,email,display_name,password_hash) SELECT $1,$2,$3,password_hash FROM users WHERE id=$4")
            .bind(id).bind(email).bind(name).bind(owner.user.id).execute(&mut *tx).await.unwrap();
    }
    for (id, role) in [(viewer_id, "viewer"), (member_id, "member")] {
        sqlx::query("INSERT INTO workspace_members(workspace_id,user_id,role) VALUES($1,$2,$3)")
            .bind(workspace)
            .bind(id)
            .bind(role)
            .execute(&mut *tx)
            .await
            .unwrap();
    }
    let private_team = Uuid::new_v4();
    sqlx::query("INSERT INTO teams(id,workspace_id,name,key,private) VALUES($1,$2,'Private research','SEC',true)")
        .bind(private_team).bind(workspace).execute(&mut *tx).await.unwrap();
    sqlx::query(
        "INSERT INTO team_members(workspace_id,team_id,user_id,role) VALUES($1,$2,$3,'maintainer')",
    )
    .bind(workspace)
    .bind(private_team)
    .bind(member_id)
    .execute(&mut *tx)
    .await
    .unwrap();
    add_status(&mut tx, workspace, private_team).await;
    tx.commit().await.unwrap();

    let other_workspace = Uuid::new_v4();
    let other_team = Uuid::new_v4();
    let organization = Uuid::new_v4();
    let mut tx = h.context(outsider_id, other_workspace).await;
    sqlx::query("INSERT INTO organizations(id,name,created_by) VALUES($1,'Other organization',$2)")
        .bind(organization)
        .bind(outsider_id)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("INSERT INTO workspaces(id,organization_id,name,slug) VALUES($1,$2,'Other workspace','other-contract-workspace')")
        .bind(other_workspace).bind(organization).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO workspace_members(workspace_id,user_id,role) VALUES($1,$2,'owner')")
        .bind(other_workspace)
        .bind(outsider_id)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("INSERT INTO workspace_auth_guards(workspace_id) VALUES($1)")
        .bind(other_workspace)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("INSERT INTO workspace_stream_heads(workspace_id) VALUES($1)")
        .bind(other_workspace)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("INSERT INTO teams(id,workspace_id,name,key) VALUES($1,$2,'Other team','OTH')")
        .bind(other_team)
        .bind(other_workspace)
        .execute(&mut *tx)
        .await
        .unwrap();
    add_status(&mut tx, other_workspace, other_team).await;
    tx.commit().await.unwrap();
    AdditionalFixtures {
        viewer: sign_in(&h.app, "viewer@contract.invalid").await.0,
        private_member: sign_in(&h.app, "private@contract.invalid").await.0,
        private_team,
        outsider: sign_in(&h.app, "outsider@contract.invalid").await.0,
        other_workspace,
        other_team,
    }
}

async fn add_status(tx: &mut Tx, workspace: Uuid, team: Uuid) {
    sqlx::query("INSERT INTO workflow_statuses(id,workspace_id,team_id,name,category,color,position,is_default) VALUES($1,$2,$3,'Backlog','backlog','gray',0,true)")
        .bind(Uuid::new_v4()).bind(workspace).bind(team).execute(&mut **tx).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_postgres_identity_isolation_and_issue_contracts() {
    let harness = Harness::new().await;
    let runner = harness.clone();
    let result = tokio::spawn(async move { exercise(&runner).await }).await;
    // Run cleanup even when an assertion in the contract suite panics.
    harness.cleanup().await;
    if let Err(error) = result {
        std::panic::resume_unwind(error.into_panic());
    }
}

async fn exercise(h: &Harness) {
    assert!(h.app.setup_required().await.unwrap());
    assert!(matches!(
        h.app.setup(setup("incorrect token")).await,
        Err(Error::Forbidden)
    ));
    let (session, bootstrap_token) = h
        .app
        .setup(setup(&h.setup_token))
        .await
        .expect("First setup succeeds");
    assert!(!h.app.setup_required().await.unwrap());
    assert!(matches!(
        h.app.setup(setup(&h.setup_token)).await,
        Err(Error::Conflict(_))
    ));
    assert_eq!(session.workspaces.len(), 1);
    let workspace = session.workspaces[0].id;
    let owner = h.app.authenticate(&bootstrap_token).await.unwrap();
    let bootstrap = h.app.bootstrap(&owner, workspace).await.unwrap();
    assert_eq!(bootstrap.teams.len(), 1);
    assert_eq!(bootstrap.statuses.len(), 5);
    let team = bootstrap.teams[0].id;
    let (login_owner, login_token) = sign_in(&h.app, &owner.user.email).await;
    assert_ne!(
        bootstrap_token, login_token,
        "Sign-in rotates session credentials"
    );
    assert_eq!(login_owner.user.id, owner.user.id);
    assert!(matches!(
        h.app
            .login(Login {
                email: owner.user.email.clone(),
                password: "definitely incorrect".into()
            })
            .await,
        Err(Error::Unauthorized)
    ));
    assert!(matches!(
        h.app.authenticate("not-a-session").await,
        Err(Error::Unauthorized)
    ));

    let fixture = fixtures(h, &owner, workspace).await;
    let mut request = issue(team, "Initial durable issue");
    request.assignee_id = Some(owner.user.id);
    let initial = h
        .app
        .create_issue(&owner, workspace, "create-same-request", request.clone())
        .await
        .unwrap();
    let retry = h
        .app
        .create_issue(&owner, workspace, "create-same-request", request.clone())
        .await
        .unwrap();
    assert_eq!(initial.id, retry.id);
    assert_eq!(initial.version, retry.version);
    assert_eq!(
        h.app
            .issue_activity(&owner, workspace, initial.id)
            .await
            .unwrap()
            .len(),
        1,
        "Retry cannot duplicate activity"
    );
    let mut different = request.clone();
    different.title = "Different payload with the same key".into();
    assert!(matches!(
        h.app
            .create_issue(&owner, workspace, "create-same-request", different)
            .await,
        Err(Error::Conflict(_))
    ));

    let simultaneous = issue(team, "Concurrent idempotent creation");
    let (left, right) = tokio::join!(
        h.app.create_issue(
            &owner,
            workspace,
            "concurrent-retry-key",
            simultaneous.clone()
        ),
        h.app.create_issue(
            &owner,
            workspace,
            "concurrent-retry-key",
            simultaneous.clone()
        )
    );
    assert_eq!(left.unwrap().id, right.unwrap().id);

    assert!(matches!(
        h.app
            .create_issue(
                &fixture.viewer,
                workspace,
                "viewer-create-denied",
                issue(team, "Not permitted")
            )
            .await,
        Err(Error::Forbidden)
    ));
    assert!(matches!(
        h.app
            .update_issue(
                &fixture.viewer,
                workspace,
                initial.id,
                &initial.etag(),
                json!({"title":"Not permitted"})
            )
            .await,
        Err(Error::Forbidden)
    ));
    assert_eq!(
        h.app
            .get_issue(&fixture.viewer, workspace, initial.id)
            .await
            .unwrap()
            .title,
        initial.title
    );

    let private_request = issue(fixture.private_team, "Secret research marker");
    let private = h
        .app
        .create_issue(
            &fixture.private_member,
            workspace,
            "private-issue-create",
            private_request.clone(),
        )
        .await
        .unwrap();
    let other = h
        .app
        .create_issue(
            &fixture.outsider,
            fixture.other_workspace,
            "other-workspace-create",
            issue(fixture.other_team, "Other tenant marker"),
        )
        .await
        .unwrap();
    assert!(
        matches!(
            h.app.get_issue(&owner, workspace, private.id).await,
            Err(Error::NotFound)
        ),
        "Workspace owner must not inherit private-team access"
    );
    assert!(matches!(
        h.app.issue_activity(&owner, workspace, private.id).await,
        Err(Error::NotFound)
    ));
    assert!(matches!(
        h.app
            .create_issue(
                &owner,
                workspace,
                "private-team-owner-denied",
                issue(fixture.private_team, "Owner cannot bypass privacy")
            )
            .await,
        Err(Error::NotFound)
    ));
    let owner_bootstrap = h.app.bootstrap(&owner, workspace).await.unwrap();
    assert!(
        !owner_bootstrap
            .teams
            .iter()
            .any(|t| t.id == fixture.private_team)
    );
    assert!(
        !owner_bootstrap
            .statuses
            .iter()
            .any(|s| s.team_id == fixture.private_team)
    );
    assert!(!owner_bootstrap.issues.iter().any(|i| i.id == private.id));
    assert!(
        h.app
            .list_issues(
                &owner,
                workspace,
                IssueFilter {
                    q: Some("Secret research".into()),
                    ..Default::default()
                }
            )
            .await
            .unwrap()
            .data
            .is_empty()
    );
    assert!(matches!(
        h.app
            .get_issue(&owner, fixture.other_workspace, other.id)
            .await,
        Err(Error::NotFound)
    ));
    assert!(matches!(
        h.app.get_issue(&owner, workspace, other.id).await,
        Err(Error::NotFound)
    ));
    assert!(matches!(
        h.app.bootstrap(&owner, fixture.other_workspace).await,
        Err(Error::NotFound)
    ));
    assert!(matches!(
        h.app
            .create_issue(
                &owner,
                workspace,
                "cross-team-create-denied",
                issue(fixture.other_team, "Invalid tenant reference")
            )
            .await,
        Err(Error::NotFound)
    ));
    assert_eq!(h.app.session(&owner).await.unwrap().workspaces.len(), 1);

    let unscoped_count: i64 = sqlx::query_scalar("SELECT count(*) FROM issues")
        .fetch_one(&h.app.pool)
        .await
        .unwrap();
    assert_eq!(
        unscoped_count, 0,
        "RLS must fail closed without transaction-local context"
    );
    let owner_unscoped_count: i64 = sqlx::query_scalar("SELECT count(*) FROM issues")
        .fetch_one(&h.owner)
        .await
        .unwrap();
    assert_eq!(
        owner_unscoped_count, 0,
        "FORCE RLS applies to the migration owner too"
    );
    let mut scoped = h.context(owner.user.id, workspace).await;
    let foreign_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM issues WHERE workspace_id=$1")
            .bind(fixture.other_workspace)
            .fetch_one(&mut *scoped)
            .await
            .unwrap();
    assert_eq!(foreign_count, 0);
    scoped.commit().await.unwrap();

    let updated = h
        .app
        .update_issue(
            &owner,
            workspace,
            initial.id,
            &initial.etag(),
            json!({"title":"Accepted current title","estimate":5}),
        )
        .await
        .unwrap();
    assert_eq!(updated.version, "2");
    match h
        .app
        .update_issue(
            &owner,
            workspace,
            initial.id,
            &initial.etag(),
            json!({"title":"Stale overwrite"}),
        )
        .await
    {
        Err(Error::Stale(current)) => assert_eq!(current.title, "Accepted current title"),
        other => panic!("Expected stale version conflict, received {other:?}"),
    }
    assert!(matches!(
        h.app
            .update_issue(
                &owner,
                workspace,
                initial.id,
                &updated.etag(),
                json!({"unexpected_field":true,"title":"Must not partly apply"})
            )
            .await,
        Err(Error::Invalid(_))
    ));
    let unchanged = h
        .app
        .get_issue(&owner, workspace, initial.id)
        .await
        .unwrap();
    assert_eq!(unchanged.title, updated.title);
    assert_eq!(unchanged.version, updated.version);
    assert_eq!(
        h.app
            .issue_activity(&owner, workspace, initial.id)
            .await
            .unwrap()
            .len(),
        2
    );
    let mut tx = h.context(owner.user.id, workspace).await;
    let events: i64 =
        sqlx::query_scalar("SELECT count(*) FROM workspace_events WHERE entity_id=$1")
            .bind(initial.id)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    assert_eq!(
        events, 2,
        "Rejected edits and retries cannot append durable events"
    );
    tx.commit().await.unwrap();

    for statement in [
        "UPDATE audit_entries SET action='rewritten' WHERE workspace_id=$1",
        "DELETE FROM audit_entries WHERE workspace_id=$1",
    ] {
        let mut runtime_tx = store::begin(&h.app.pool).await.unwrap();
        store::context(&mut runtime_tx, owner.user.id, Some(workspace))
            .await
            .unwrap();
        let error = sqlx::query(statement)
            .bind(workspace)
            .execute(&mut *runtime_tx)
            .await
            .expect_err("Runtime must not rewrite audit history");
        assert!(
            matches!(&error, sqlx::Error::Database(db) if db.code().as_deref() == Some("42501")),
            "Expected an audit privilege denial, received {error}"
        );
        runtime_tx.rollback().await.unwrap();
    }
    let error = sqlx::query("TRUNCATE audit_entries")
        .execute(&h.app.pool)
        .await
        .expect_err("Runtime must not truncate audit history");
    assert!(matches!(&error, sqlx::Error::Database(db) if db.code().as_deref() == Some("42501")));
    let mut tx = h.context(owner.user.id, workspace).await;
    let audit_action: String =
        sqlx::query_scalar("SELECT action FROM audit_entries WHERE workspace_id=$1")
            .bind(workspace)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    assert_eq!(audit_action, "instance.setup");
    tx.commit().await.unwrap();

    pagination(h, &owner, workspace, team).await;

    // Removing explicit private-team membership takes effect for existing actors
    // and for a retry whose response was already persisted before revocation.
    let mut tx = h.context(owner.user.id, workspace).await;
    sqlx::query("SELECT epoch FROM workspace_auth_guards WHERE workspace_id=$1 FOR UPDATE")
        .bind(workspace)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    sqlx::query("DELETE FROM team_members WHERE workspace_id=$1 AND team_id=$2 AND user_id=$3")
        .bind(workspace)
        .bind(fixture.private_team)
        .bind(fixture.private_member.user.id)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert!(matches!(
        h.app
            .get_issue(&fixture.private_member, workspace, private.id)
            .await,
        Err(Error::NotFound)
    ));
    assert!(
        matches!(
            h.app
                .create_issue(
                    &fixture.private_member,
                    workspace,
                    "private-issue-create",
                    private_request
                )
                .await,
            Err(Error::NotFound)
        ),
        "A persisted idempotency response must not bypass revoked visibility"
    );
    assert!(
        h.app
            .bootstrap(&fixture.private_member, workspace)
            .await
            .unwrap()
            .teams
            .iter()
            .all(|t| t.id != fixture.private_team)
    );

    h.app.logout(&login_owner).await.unwrap();
    assert!(matches!(
        h.app.authenticate(&login_token).await,
        Err(Error::Unauthorized)
    ));
    assert!(
        matches!(
            h.app.get_issue(&login_owner, workspace, initial.id).await,
            Err(Error::Unauthorized)
        ),
        "A cached actor cannot read after session revocation"
    );
    assert!(matches!(
        h.app
            .create_issue(
                &login_owner,
                workspace,
                "revoked-session-write",
                issue(team, "Cannot write after logout")
            )
            .await,
        Err(Error::Unauthorized)
    ));
    assert!(
        h.app.authenticate(&bootstrap_token).await.is_ok(),
        "Logout revokes only its own session"
    );
}

async fn pagination(h: &Harness, owner: &Actor, workspace: Uuid, team: Uuid) {
    let mut expected = HashSet::new();
    let mut status_id = None;
    for index in 0..7 {
        let mut request = issue(team, &format!("Page literal_% marker {index}"));
        request.assignee_id = Some(owner.user.id);
        let row = h
            .app
            .create_issue(
                owner,
                workspace,
                &format!("pagination-create-{index}"),
                request,
            )
            .await
            .unwrap();
        expected.insert(row.id);
        status_id = Some(row.status_id);
    }
    // Equal timestamps force pagination to use its UUID tiebreaker correctly.
    let mut tx = h.context(owner.user.id, workspace).await;
    sqlx::query("UPDATE issues SET created_at='2026-01-01T00:00:00Z' WHERE title LIKE 'Page literal%' AND workspace_id=$1")
        .bind(workspace).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    let filter = IssueFilter {
        team_id: Some(team),
        status_id,
        assignee_id: Some(owner.user.id),
        priority: Some(2),
        q: Some("literal_%".into()),
        limit: Some(2),
        ..Default::default()
    };
    let first = h
        .app
        .list_issues(owner, workspace, filter.clone())
        .await
        .unwrap();
    assert!(first.has_more);
    assert_eq!(first.data.len(), 2);
    let mut changed_query = filter.clone();
    changed_query.cursor = first.next_cursor.clone();
    changed_query.priority = Some(1);
    assert!(matches!(
        h.app.list_issues(owner, workspace, changed_query).await,
        Err(Error::Invalid(_))
    ));
    let mut seen = HashSet::new();
    let mut cursor = None;
    for page_number in 0..4 {
        let mut query = filter.clone();
        query.cursor = cursor;
        let page = h.app.list_issues(owner, workspace, query).await.unwrap();
        assert_eq!(page.has_more, page_number < 3);
        for row in page.data {
            assert!(seen.insert(row.id), "Pagination must not repeat a row");
        }
        cursor = page.next_cursor;
    }
    assert!(cursor.is_none());
    assert_eq!(seen, expected, "Pagination must not omit matching rows");
    let archived = first.data.first().unwrap();
    h.app
        .update_issue(
            owner,
            workspace,
            archived.id,
            &archived.etag(),
            json!({"archived":true}),
        )
        .await
        .unwrap();
    let archived_query = IssueFilter {
        archived: true,
        limit: Some(100),
        ..filter.clone()
    };
    let archived_page = h
        .app
        .list_issues(owner, workspace, archived_query)
        .await
        .unwrap();
    assert_eq!(archived_page.data.len(), 1);
    assert_eq!(archived_page.data[0].id, archived.id);
    let archived_current = &archived_page.data[0];
    assert!(matches!(
        h.app
            .update_issue(
                owner,
                workspace,
                archived_current.id,
                &archived_current.etag(),
                json!({"title":"Archived issue cannot be edited"})
            )
            .await,
        Err(Error::Conflict(_))
    ));
    let restored = h
        .app
        .update_issue(
            owner,
            workspace,
            archived_current.id,
            &archived_current.etag(),
            json!({"archived":false}),
        )
        .await
        .unwrap();
    assert!(restored.archived_at.is_none());
    assert_eq!(restored.title, archived_current.title);
    assert!(matches!(
        h.app
            .list_issues(
                owner,
                workspace,
                IssueFilter {
                    limit: Some(101),
                    ..Default::default()
                }
            )
            .await,
        Err(Error::Invalid(_))
    ));
}
