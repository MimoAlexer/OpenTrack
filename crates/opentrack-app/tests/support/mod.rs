//! A restricted runtime role and fresh schema in an explicitly isolated test DB.
use opentrack_app::App;
use opentrack_store::{self as store, Tx};
use sqlx::{AssertSqlSafe, PgPool, postgres::PgPoolOptions};
use url::Url;
use uuid::Uuid;

#[derive(Clone)]
pub struct Harness {
    pub app: App,
    pub owner: PgPool,
    schema: String,
    pub setup_token: String,
}

impl Harness {
    pub async fn new() -> Self {
        Self::with_connections(5).await
    }

    pub async fn with_connections(connections: u32) -> Self {
        Self::with_version(connections, i64::MAX).await
    }

    #[allow(dead_code)]
    pub async fn with_version(connections: u32, version: i64) -> Self {
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
        let schema = format!("identity_contract_{}", Uuid::new_v4().simple());
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
        let role = runtime.username().replace('"', "\"\"");
        // Match deployment defaults before applying migrations; upgrade tests
        // deliberately rely on these grants for tables created by the next migration.
        sqlx::query(AssertSqlSafe(format!("ALTER DEFAULT PRIVILEGES IN SCHEMA {schema} GRANT SELECT,INSERT,UPDATE,DELETE ON TABLES TO \"{role}\""))).execute(&owner).await.unwrap();
        let migrations = store::MIGRATOR
            .iter()
            .filter(|migration| migration.version <= version)
            .cloned()
            .collect::<Vec<_>>();
        sqlx::migrate::Migrator {
            migrations: migrations.into(),
            ..sqlx::migrate::Migrator::DEFAULT
        }
        .run(&owner)
        .await
        .expect("Migrate the isolated schema to its requested version");
        for statement in [
            format!("GRANT USAGE ON SCHEMA {schema} TO \"{role}\""),
            format!(
                "GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA {schema} TO \"{role}\""
            ),
            format!("GRANT USAGE, SELECT ON ALL SEQUENCES IN SCHEMA {schema} TO \"{role}\""),
            format!("REVOKE UPDATE, DELETE, TRUNCATE ON {schema}.audit_entries FROM \"{role}\""),
            format!(
                "REVOKE UPDATE, DELETE, TRUNCATE ON {schema}.account_security_events FROM \"{role}\""
            ),
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
                .execute(&base_owner)
                .await
                .expect("Grant restricted runtime access");
        }
        let consumers: bool = sqlx::query_scalar("SELECT to_regclass($1) IS NOT NULL")
            .bind(format!("{schema}.consumer_receipts"))
            .fetch_one(&base_owner)
            .await
            .unwrap();
        if consumers {
            sqlx::query(AssertSqlSafe(format!("REVOKE ALL ON {schema}.assignment_reconciliations,{schema}.consumer_registrations,{schema}.consumer_checkpoints,{schema}.consumer_receipts,{schema}.consumer_dead_letters,{schema}.consumer_alerts,{schema}.event_retention_state,{schema}.consumer_operations FROM \"{role}\"")))
                .execute(&base_owner).await.unwrap();
        }
        let documents: bool = sqlx::query_scalar("SELECT to_regclass($1) IS NOT NULL")
            .bind(format!("{schema}.documents"))
            .fetch_one(&base_owner)
            .await
            .unwrap();
        if documents {
            sqlx::query(AssertSqlSafe(format!(
                "REVOKE ALL ON {schema}.document_retention_operations FROM \"{role}\""
            )))
            .execute(&base_owner)
            .await
            .unwrap();
            sqlx::query(AssertSqlSafe(format!(
                "REVOKE UPDATE,DELETE,TRUNCATE ON {schema}.document_revisions FROM \"{role}\""
            )))
            .execute(&base_owner)
            .await
            .unwrap();
        }
        let attachments: bool = sqlx::query_scalar("SELECT to_regclass($1) IS NOT NULL")
            .bind(format!("{schema}.attachments"))
            .fetch_one(&base_owner)
            .await
            .unwrap();
        if attachments {
            sqlx::query(AssertSqlSafe(format!(
                "REVOKE ALL ON {schema}.attachment_storage_operations FROM \"{role}\""
            )))
            .execute(&base_owner)
            .await
            .unwrap();
            sqlx::query(AssertSqlSafe(format!("REVOKE UPDATE,DELETE,TRUNCATE ON {schema}.document_attachments,{schema}.document_attachment_revisions FROM \"{role}\"")))
                .execute(&base_owner).await.unwrap();
        }
        let templates: bool = sqlx::query_scalar("SELECT to_regclass($1) IS NOT NULL")
            .bind(format!("{schema}.issue_templates"))
            .fetch_one(&base_owner)
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
                    .execute(&base_owner)
                    .await
                    .unwrap();
            }
        }
        let mentions: bool = sqlx::query_scalar("SELECT to_regclass($1) IS NOT NULL")
            .bind(format!("{schema}.issue_subscriptions"))
            .fetch_one(&base_owner)
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
                    .execute(&base_owner)
                    .await
                    .unwrap();
            }
        }
        let preferences: bool = sqlx::query_scalar("SELECT to_regclass($1) IS NOT NULL")
            .bind(format!("{schema}.user_preferences"))
            .fetch_one(&base_owner)
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
                    .execute(&base_owner)
                    .await
                    .unwrap();
            }
        }
        base_owner.close().await;
        let setup_token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        let app = App::connect(
            scoped_url(runtime, &schema).as_str(),
            &setup_token,
            connections,
            2,
        )
        .await
        .expect("Connect the application using the restricted runtime role");
        Self {
            app,
            owner,
            schema,
            setup_token,
        }
    }

    pub async fn context(&self, actor: Uuid, workspace: Uuid) -> Tx {
        let mut tx = store::begin(&self.owner)
            .await
            .expect("Begin fixture transaction");
        store::context(&mut tx, actor, Some(workspace))
            .await
            .expect("Set explicit fixture tenant context");
        tx
    }

    /// Provision a separate least-privileged worker only inside this isolated schema.
    #[allow(dead_code)]
    pub async fn worker_url(&self) -> String {
        let worker = test_url("OPENTRACK_TEST_WORKER_DATABASE_URL");
        let owner = test_url("OPENTRACK_TEST_MIGRATION_DATABASE_URL");
        let runtime = test_url("OPENTRACK_TEST_DATABASE_URL");
        assert_eq!(
            (worker.host_str(), worker.port(), worker.path()),
            (owner.host_str(), owner.port(), owner.path())
        );
        assert_ne!(worker.username(), owner.username());
        assert_ne!(worker.username(), runtime.username());
        let script = include_str!("../../../../deploy/grant-worker.sql")
            .lines()
            .filter(|line| !line.starts_with('\\'))
            .collect::<Vec<_>>()
            .join("\n")
            .replace(
                ":'worker_role'",
                &format!("'{}'", worker.username().replace('\'', "''")),
            )
            .replace(":'worker_schema'", &format!("'{}'", self.schema));
        sqlx::raw_sql(AssertSqlSafe(script))
            .execute(&self.owner)
            .await
            .expect("Grant reviewed worker capabilities");
        scoped_url(worker, &self.schema).to_string()
    }

    /// Only operator test binaries require the third database identity.
    #[allow(dead_code)]
    pub async fn operator_url(&self) -> String {
        let operator = test_url("OPENTRACK_TEST_OPERATOR_DATABASE_URL");
        let runtime = test_url("OPENTRACK_TEST_DATABASE_URL");
        let owner = test_url("OPENTRACK_TEST_MIGRATION_DATABASE_URL");
        assert_eq!(
            (operator.host_str(), operator.port(), operator.path()),
            (owner.host_str(), owner.port(), owner.path())
        );
        assert_ne!(operator.username(), runtime.username());
        assert_ne!(operator.username(), owner.username());
        let restricted: bool=sqlx::query_scalar("SELECT rolcanlogin AND NOT (rolsuper OR rolbypassrls OR rolcreatedb OR rolcreaterole OR rolinherit OR rolreplication) AND NOT EXISTS(SELECT 1 FROM pg_auth_members WHERE member=r.oid OR roleid=r.oid) FROM pg_roles r WHERE rolname=$1")
            .bind(operator.username()).fetch_one(&self.owner).await.unwrap();
        assert!(
            restricted,
            "Operator tests require a restricted operator login without inherited roles"
        );
        let role = operator.username().replace('"', "\"\"");
        for statement in [
            format!("GRANT USAGE ON SCHEMA {} TO \"{role}\"", self.schema),
            format!(
                "REVOKE ALL ON ALL TABLES IN SCHEMA {} FROM \"{role}\"",
                self.schema
            ),
            format!(
                "REVOKE ALL ON ALL SEQUENCES IN SCHEMA {} FROM \"{role}\"",
                self.schema
            ),
            format!(
                "REVOKE ALL ON ALL FUNCTIONS IN SCHEMA {} FROM \"{role}\"",
                self.schema
            ),
            format!(
                "GRANT EXECUTE ON FUNCTION {}.operator_issue_recovery(uuid,uuid,text,integer,text),{}.operator_revoke_recovery(uuid,text),{}.operator_disable_account(uuid,text),{}.operator_enable_account(uuid,text) TO \"{role}\"",
                self.schema, self.schema, self.schema, self.schema
            ),
        ] {
            sqlx::query(AssertSqlSafe(statement))
                .execute(&self.owner)
                .await
                .unwrap();
        }
        let consumers: bool =
            sqlx::query_scalar("SELECT to_regclass('consumer_receipts') IS NOT NULL")
                .fetch_one(&self.owner)
                .await
                .unwrap();
        if consumers {
            sqlx::query(AssertSqlSafe(format!("GRANT EXECUTE ON FUNCTION {}.operator_consumer_status(uuid),{}.operator_consumer_disposition(uuid,uuid,text,text),{}.operator_consumer_set_enabled(uuid,boolean,text),{}.operator_resume_assignment_cleanup(uuid,uuid,text) TO \"{role}\"",self.schema,self.schema,self.schema,self.schema))).execute(&self.owner).await.unwrap();
        }
        scoped_url(operator, &self.schema).to_string()
    }

    pub async fn cleanup(&self) {
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
