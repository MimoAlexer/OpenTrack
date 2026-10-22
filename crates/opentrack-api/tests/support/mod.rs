use std::{path::PathBuf, sync::Arc};

use opentrack_api::{ApiState, router};
use opentrack_app::App;
use opentrack_store::{self as store, MIGRATOR};
use sqlx::{AssertSqlSafe, PgPool, postgres::PgPoolOptions};
use url::Url;
use uuid::Uuid;

pub const ORIGIN: &str = "https://opentrack.contract.invalid";
pub const INDEX: &str =
    "<!doctype html><html><body>OpenTrack contract application shell</body></html>";

#[derive(Clone)]
pub struct Harness {
    pub router: axum::Router,
    pub setup_token: String,
    app: App,
    owner: PgPool,
    schema: String,
    static_dir: PathBuf,
}

impl Harness {
    #[allow(dead_code)]
    pub fn serving_static(&self, directory: PathBuf) -> axum::Router {
        router(ApiState {
            app: self.app.clone(),
            attachments: None,
            origins: Arc::new(vec![ORIGIN.into()]),
            secure_cookie: true,
            static_dir: Arc::new(directory),
        })
    }

    #[allow(dead_code)]
    pub async fn close_runtime(&self) {
        self.app.pool.close().await;
    }

    #[allow(dead_code)] // Used by the real HTTP static-delivery contract only.
    pub async fn install_frontend_entrypoint(&self) {
        tokio::fs::write(
            self.static_dir.join("index.html"),
            include_str!("../../../../web/index.html"),
        )
        .await
        .unwrap();
        tokio::fs::write(
            self.static_dir.join("theme.js"),
            include_str!("../../../../web/public/theme.js"),
        )
        .await
        .unwrap();
    }

    #[allow(dead_code)] // Shared fixture is compiled separately by each contract target.
    pub fn with_attachments(mut self, service: opentrack_api::AttachmentService) -> Self {
        self.router = router(ApiState {
            app: self.app.clone(),
            attachments: Some(Arc::new(service)),
            origins: Arc::new(vec![ORIGIN.into()]),
            secure_cookie: true,
            static_dir: Arc::new(self.static_dir.clone()),
        });
        self
    }

    pub async fn lock_team(&self, workspace: Uuid, team: Uuid, actor: Uuid) -> opentrack_store::Tx {
        let mut tx = store::begin(&self.owner).await.unwrap();
        opentrack_store::context(&mut tx, actor, Some(workspace))
            .await
            .unwrap();
        sqlx::query("SELECT id FROM teams WHERE workspace_id=$1 AND id=$2 FOR UPDATE")
            .bind(workspace)
            .bind(team)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        tx
    }

    pub async fn new() -> Self {
        let runtime = test_url("OPENTRACK_TEST_DATABASE_URL");
        let owner = test_url("OPENTRACK_TEST_MIGRATION_DATABASE_URL");
        assert_eq!(runtime.path(), owner.path());
        assert_ne!(runtime.username(), owner.username());
        let owner_base = PgPoolOptions::new()
            .max_connections(1)
            .connect(owner.as_str())
            .await
            .unwrap();
        let schema = format!("http_contract_{}", Uuid::new_v4().simple());
        // This identifier consists only of a fixed prefix and generated UUID hex.
        sqlx::query(AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
            .execute(&owner_base)
            .await
            .unwrap();
        let owner = PgPoolOptions::new()
            .max_connections(1)
            .connect(scoped_url(owner, &schema).as_str())
            .await
            .unwrap();
        MIGRATOR.run(&owner).await.unwrap();
        // PostgreSQL identifiers cannot be bound; quote the role and double any quotes.
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
            // Match migration 0009 after the fixture's initial blanket grant.
            // Serving requests may enqueue/resume jobs, never administer leases.
            format!(
                "REVOKE ALL ON {schema}.jobs,{schema}.job_attempts,{schema}.worker_workspace_registry FROM \"{role}\""
            ),
            format!("GRANT SELECT,INSERT ON {schema}.jobs TO \"{role}\""),
            format!(
                "GRANT UPDATE(requested_by,state,retry_count,error_code,available_at,finished_at,version,updated_at) ON {schema}.jobs TO \"{role}\""
            ),
            format!("GRANT SELECT,INSERT ON {schema}.worker_workspace_registry TO \"{role}\""),
        ] {
            sqlx::query(AssertSqlSafe(statement))
                .execute(&owner_base)
                .await
                .unwrap();
        }
        let consumers: bool = sqlx::query_scalar("SELECT to_regclass($1) IS NOT NULL")
            .bind(format!("{schema}.consumer_receipts"))
            .fetch_one(&owner_base)
            .await
            .unwrap();
        if consumers {
            sqlx::query(AssertSqlSafe(format!("REVOKE ALL ON {schema}.assignment_reconciliations,{schema}.consumer_registrations,{schema}.consumer_checkpoints,{schema}.consumer_receipts,{schema}.consumer_dead_letters,{schema}.consumer_alerts,{schema}.event_retention_state,{schema}.consumer_operations FROM \"{role}\"")))
                .execute(&owner_base).await.unwrap();
        }
        let documents: bool = sqlx::query_scalar("SELECT to_regclass($1) IS NOT NULL")
            .bind(format!("{schema}.documents"))
            .fetch_one(&owner_base)
            .await
            .unwrap();
        if documents {
            sqlx::query(AssertSqlSafe(format!(
                "REVOKE ALL ON {schema}.document_retention_operations FROM \"{role}\""
            )))
            .execute(&owner_base)
            .await
            .unwrap();
            sqlx::query(AssertSqlSafe(format!(
                "REVOKE UPDATE,DELETE,TRUNCATE ON {schema}.document_revisions FROM \"{role}\""
            )))
            .execute(&owner_base)
            .await
            .unwrap();
        }
        let attachments: bool = sqlx::query_scalar("SELECT to_regclass($1) IS NOT NULL")
            .bind(format!("{schema}.attachments"))
            .fetch_one(&owner_base)
            .await
            .unwrap();
        if attachments {
            sqlx::query(AssertSqlSafe(format!(
                "REVOKE ALL ON {schema}.attachment_storage_operations FROM \"{role}\""
            )))
            .execute(&owner_base)
            .await
            .unwrap();
            sqlx::query(AssertSqlSafe(format!("REVOKE UPDATE,DELETE,TRUNCATE ON {schema}.document_attachments,{schema}.document_attachment_revisions FROM \"{role}\"")))
                .execute(&owner_base).await.unwrap();
        }
        let templates: bool = sqlx::query_scalar("SELECT to_regclass($1) IS NOT NULL")
            .bind(format!("{schema}.issue_templates"))
            .fetch_one(&owner_base)
            .await
            .unwrap();
        if templates {
            for statement in [
                format!("REVOKE DELETE,TRUNCATE ON {schema}.issue_templates FROM \"{role}\""),
                format!(
                    "REVOKE UPDATE,DELETE,TRUNCATE ON {schema}.issue_template_sources FROM \"{role}\""
                ),
            ] {
                sqlx::query(AssertSqlSafe(statement))
                    .execute(&owner_base)
                    .await
                    .unwrap();
            }
        }
        let mentions: bool = sqlx::query_scalar("SELECT to_regclass($1) IS NOT NULL")
            .bind(format!("{schema}.issue_subscriptions"))
            .fetch_one(&owner_base)
            .await
            .unwrap();
        if mentions {
            for statement in [
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
            ] {
                sqlx::query(AssertSqlSafe(statement))
                    .execute(&owner_base)
                    .await
                    .unwrap();
            }
        }
        let preferences: bool = sqlx::query_scalar("SELECT to_regclass($1) IS NOT NULL")
            .bind(format!("{schema}.user_preferences"))
            .fetch_one(&owner_base)
            .await
            .unwrap();
        if preferences {
            for statement in [
                format!(
                    "REVOKE ALL ON {schema}.user_preferences,{schema}.user_preference_events FROM \"{role}\""
                ),
                format!(
                    "GRANT SELECT,INSERT ON {schema}.user_preferences,{schema}.user_preference_events TO \"{role}\""
                ),
                format!(
                    "GRANT UPDATE(shortcut_mode,version,updated_at) ON {schema}.user_preferences TO \"{role}\""
                ),
            ] {
                sqlx::query(AssertSqlSafe(statement))
                    .execute(&owner_base)
                    .await
                    .unwrap();
            }
        }
        owner_base.close().await;
        let setup_token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        let app = App::connect(scoped_url(runtime, &schema).as_str(), &setup_token, 4, 2)
            .await
            .unwrap();
        let static_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../work/http-contract")
            .join(&schema);
        tokio::fs::create_dir_all(static_dir.join("assets"))
            .await
            .unwrap();
        tokio::fs::write(static_dir.join("index.html"), INDEX)
            .await
            .unwrap();
        tokio::fs::write(
            static_dir.join("assets/app-a1b2c3d4.js"),
            "console.log('contract asset');",
        )
        .await
        .unwrap();
        let router = router(ApiState {
            attachments: None,
            app: app.clone(),
            origins: Arc::new(vec![ORIGIN.into()]),
            secure_cookie: true,
            static_dir: Arc::new(static_dir.clone()),
        });
        Self {
            router,
            setup_token,
            app,
            owner,
            schema,
            static_dir,
        }
    }

    pub async fn cleanup(&self) {
        self.app.pool.close().await;
        // Only this harness's generated identifier is interpolated.
        sqlx::query(AssertSqlSafe(format!(
            "DROP SCHEMA {} CASCADE",
            self.schema
        )))
        .execute(&self.owner)
        .await
        .unwrap();
        self.owner.close().await;
        tokio::fs::remove_dir_all(&self.static_dir).await.unwrap();
    }
}

fn test_url(variable: &str) -> Url {
    let value = std::env::var(variable)
        .unwrap_or_else(|_| panic!("{variable} is required; run scripts/dev-db.sh test"));
    let url = Url::parse(&value).expect("Parse test database URL");
    assert!(matches!(url.scheme(), "postgres" | "postgresql"));
    assert!(
        url.path().trim_start_matches('/').ends_with("_test"),
        "HTTP tests refuse any database not ending in _test"
    );
    url
}

fn scoped_url(mut url: Url, schema: &str) -> Url {
    assert!(url.query_pairs().all(|(key, _)| key != "options"));
    url.query_pairs_mut()
        .append_pair("options", &format!("-csearch_path={schema}"));
    url
}
