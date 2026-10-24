#![cfg(unix)]
#[allow(dead_code)]
#[path = "../../opentrack-app/tests/support/mod.rs"]
mod support;

use opentrack_app::worker::{Worker, WorkerOptions};
use opentrack_store as store;
use serde_json::{Value, json};
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::{
    fs,
    net::{SocketAddr, TcpListener},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::PathBuf,
    process::{Child, Command, ExitStatus, Stdio},
    time::{Duration, Instant},
};
use support::Harness;
use uuid::Uuid;

struct PrivateDirectory(PathBuf);
impl PrivateDirectory {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("opentrack-schema-contract-{}", Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        Self(path)
    }
}
impl Drop for PrivateDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
struct OwnedChild(Child);
impl Drop for OwnedChild {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
        }
        let _ = self.0.wait();
    }
}

async fn schema(h: &Harness) -> String {
    sqlx::query_scalar("SELECT current_schema()")
        .fetch_one(&h.owner)
        .await
        .unwrap()
}

fn scoped_url(variable: &str, schema: &str) -> String {
    let mut url =
        url::Url::parse(&std::env::var(variable).expect("Required isolated database URL")).unwrap();
    assert!(url.query_pairs().all(|(key, _)| key != "options"));
    url.query_pairs_mut()
        .append_pair("options", &format!("-csearch_path={schema}"));
    url.to_string()
}

async fn pool(url: &str) -> PgPool {
    PgPoolOptions::new()
        .max_connections(1)
        .connect(url)
        .await
        .expect("Connect the isolated restricted history reader")
}

async fn history_snapshot(h: &Harness) -> Value {
    let exists: bool = sqlx::query_scalar("SELECT to_regclass('_sqlx_migrations') IS NOT NULL")
        .fetch_one(&h.owner)
        .await
        .unwrap();
    if !exists {
        return Value::Null;
    }
    sqlx::query_scalar("SELECT jsonb_build_object(
        'rows',(SELECT jsonb_agg(to_jsonb(m) ORDER BY version) FROM _sqlx_migrations m),
        'table',(SELECT jsonb_build_array(c.oid,c.relowner,c.relacl,c.relkind,c.relrowsecurity,c.relforcerowsecurity)
          FROM pg_class c WHERE c.oid='_sqlx_migrations'::regclass),
        'columns',(SELECT jsonb_agg(jsonb_build_array(a.attname,a.atttypid,a.attnotnull,a.attacl,
          pg_get_expr(d.adbin,d.adrelid)) ORDER BY a.attnum)
          FROM pg_attribute a LEFT JOIN pg_attrdef d ON d.adrelid=a.attrelid AND d.adnum=a.attnum
          WHERE a.attrelid='_sqlx_migrations'::regclass AND a.attnum>0 AND NOT a.attisdropped))")
        .fetch_one(&h.owner).await.unwrap()
}

async fn effect_snapshot(h: &Harness, user: Uuid, workspace: Uuid) -> Value {
    let mut tx = h.context(user, workspace).await;
    let result = sqlx::query_scalar("SELECT jsonb_build_object(
        'jobs',(SELECT coalesce(jsonb_agg(to_jsonb(j) ORDER BY id),'[]'::jsonb) FROM jobs j),
        'checkpoints',(SELECT coalesce(jsonb_agg(to_jsonb(c) ORDER BY consumer_name,consumer_version),'[]'::jsonb) FROM consumer_checkpoints c),
        'receipts',(SELECT count(*) FROM consumer_receipts),
        'audit',(SELECT count(*) FROM audit_entries),
        'recovery_tokens',(SELECT count(*) FROM recovery_tokens WHERE user_id=$1),
        'recovery_events',(SELECT count(*) FROM account_security_events WHERE user_id=$1 AND action LIKE 'recovery.%'))")
        .bind(user).fetch_one(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    result
}

async fn future_history(h: &Harness) {
    let future = store::MIGRATOR.iter().map(|m| m.version).max().unwrap() + 1;
    sqlx::query(
        "INSERT INTO _sqlx_migrations(version,description,success,checksum,execution_time)
        VALUES($1,'Synthetic future history',true,decode(repeat('ab',48),'hex'),0)",
    )
    .bind(future)
    .execute(&h.owner)
    .await
    .unwrap();
}

async fn restricted_urls(h: &Harness) -> Vec<String> {
    vec![h.worker_url().await, h.operator_url().await]
}

#[tokio::test]
async fn current_history_is_readable_with_only_the_three_required_metadata_columns() {
    let h = Harness::new().await;
    let runner = h.clone();
    let outcome = tokio::spawn(async move {
        let h = &runner;
        let urls = restricted_urls(h).await;
        let before = history_snapshot(h).await;
        assert!(store::schema_is_current(&h.app.pool).await.unwrap());
        for url in &urls {
            let reader = pool(url).await;
            let bounded: bool = sqlx::query_scalar("SELECT
                NOT has_table_privilege(current_user,'_sqlx_migrations','SELECT,INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER')
                AND NOT has_any_column_privilege(current_user,'_sqlx_migrations','INSERT,UPDATE,REFERENCES')
                AND (SELECT bool_and(has_column_privilege(current_user,a.attrelid,a.attnum,'SELECT')
                  = (a.attname IN('version','success','checksum'))) FROM pg_attribute a
                  WHERE a.attrelid='_sqlx_migrations'::regclass AND a.attnum>0 AND NOT a.attisdropped)")
                .fetch_one(&reader).await.unwrap();
            assert!(bounded, "History readers must have exactly the required column capabilities");
            for statement in [
                "SELECT description FROM _sqlx_migrations LIMIT 0",
                "SELECT installed_on FROM _sqlx_migrations LIMIT 0",
                "SELECT execution_time FROM _sqlx_migrations LIMIT 0",
                "SELECT * FROM _sqlx_migrations LIMIT 0",
                "UPDATE _sqlx_migrations SET success=success WHERE false",
                "DELETE FROM _sqlx_migrations WHERE false",
                "INSERT INTO _sqlx_migrations(version,description,success,checksum,execution_time)
                  SELECT 0,'Denied metadata write',true,decode('00','hex'),0 WHERE false",
                "TRUNCATE _sqlx_migrations",
            ] {
                let mut tx = reader.begin().await.unwrap();
                let result = sqlx::query(statement).execute(&mut *tx).await;
                tx.rollback().await.unwrap();
                assert!(result.err().and_then(|e| e.as_database_error().and_then(|d| d.code()).map(|c| c.into_owned()))
                    .is_some_and(|code| code == "42501"), "Additional metadata access must be denied by privileges");
            }
            sqlx::query("SET default_transaction_read_only=on").execute(&reader).await.unwrap();
            assert!(store::schema_is_current(&reader).await.unwrap(), "Validation must work in a read-only session");
            reader.close().await;
        }
        let worker = Worker::connect(&urls[0], WorkerOptions::default()).await.unwrap();
        assert!(store::schema_is_current(&worker.pool).await.unwrap());
        worker.pool.close().await;
        let operator = opentrack_app::Operator::connect(&urls[1]).await.unwrap();
        assert!(operator.schema_is_current().await.unwrap());
        drop(operator);
        assert!(before == history_snapshot(h).await, "Validation and denied probes must preserve metadata rows, table and ACL");
    }).await;
    h.cleanup().await;
    if let Err(error) = outcome {
        std::panic::resume_unwind(error.into_panic());
    }
}

#[tokio::test]
async fn every_history_entry_and_checksum_is_required_without_repairing_metadata() {
    for corruption in ["future", "dirty", "checksum", "missing"] {
        let h = Harness::new().await;
        let runner = h.clone();
        let outcome = tokio::spawn(async move {
            let h = &runner;
            let urls = restricted_urls(h).await;
            match corruption {
                "future" => future_history(h).await,
                "dirty" => { sqlx::query("UPDATE _sqlx_migrations SET success=false WHERE version=(SELECT min(version) FROM _sqlx_migrations)")
                    .execute(&h.owner).await.unwrap(); }
                "checksum" => { sqlx::query("UPDATE _sqlx_migrations SET checksum=set_byte(checksum,0,get_byte(checksum,0)#1)
                    WHERE version=(SELECT min(version) FROM _sqlx_migrations)").execute(&h.owner).await.unwrap(); }
                "missing" => { sqlx::query("DELETE FROM _sqlx_migrations WHERE version=(SELECT min(version) FROM _sqlx_migrations)")
                    .execute(&h.owner).await.unwrap(); }
                _ => unreachable!(),
            }
            let before = history_snapshot(h).await;
            assert!(!store::schema_is_current(&h.app.pool).await.unwrap(), "Reject {corruption} history");
            for url in urls {
                let reader = pool(&url).await;
                assert!(!store::schema_is_current(&reader).await.unwrap(), "Restricted reader must reject {corruption} history");
                reader.close().await;
            }
            assert!(before == history_snapshot(h).await, "Incompatible history must not be repaired or modified");
        }).await;
        h.cleanup().await;
        if let Err(error) = outcome {
            std::panic::resume_unwind(error.into_panic());
        }
    }
}

#[tokio::test]
async fn absent_and_unreadable_metadata_return_errors_without_creating_or_granting_it() {
    for absent in [false, true] {
        let h = Harness::new().await;
        let runner = h.clone();
        let outcome = tokio::spawn(async move {
            let h = &runner;
            let mut urls = restricted_urls(h).await;
            urls.push(scoped_url("OPENTRACK_TEST_DATABASE_URL", &schema(h).await));
            if absent {
                sqlx::query("DROP TABLE _sqlx_migrations").execute(&h.owner).await.unwrap();
            } else {
                for url in &urls {
                    let role = url::Url::parse(url).unwrap().username().replace('"', "\"\"");
                    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
                        "REVOKE ALL ON _sqlx_migrations FROM \"{role}\";
                         REVOKE SELECT(version,success,checksum) ON _sqlx_migrations FROM \"{role}\";"
                    ))).execute(&h.owner).await.unwrap();
                }
            }
            let before = history_snapshot(h).await;
            for url in urls {
                let reader = pool(&url).await;
                assert!(store::schema_is_current(&reader).await.is_err(), "Unavailable metadata must fail closed");
                assert_eq!(sqlx::query_scalar::<_, i32>("SELECT 1").fetch_one(&reader).await.unwrap(), 1);
                reader.close().await;
            }
            assert!(before == history_snapshot(h).await, "Validation must not create metadata or restore its privileges");
        }).await;
        h.cleanup().await;
        if let Err(error) = outcome {
            std::panic::resume_unwind(error.into_panic());
        }
    }
}

async fn invoke(
    directory: &PrivateDirectory,
    url_key: &str,
    url: &str,
    setup: Option<&str>,
    address: SocketAddr,
    arguments: &[String],
) -> (ExitStatus, String) {
    let log_path = directory.0.join("command.log");
    let log = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&log_path)
        .unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_opentrack-server"));
    command
        .env_clear()
        .env("OPENTRACK_ENVIRONMENT", "test")
        .env("RUST_LOG", "error")
        .env(url_key, url)
        .env("OPENTRACK_LISTEN", address.to_string())
        .env("OPENTRACK_PUBLIC_URL", format!("http://{address}"))
        .env("OPENTRACK__STORAGE__LOCAL_ROOT", directory.0.join("blobs"))
        .current_dir(&directory.0)
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(log.try_clone().unwrap())
        .stderr(log);
    if let Some(setup) = setup {
        command.env("OPENTRACK_SETUP_TOKEN", setup);
    }
    let mut child = OwnedChild(command.spawn().expect("Start the owned schema-guard CLI"));
    let deadline = Instant::now() + Duration::from_secs(15);
    let status = loop {
        assert!(
            fs::metadata(&log_path).unwrap().len() <= 65536,
            "CLI output exceeded its bound"
        );
        if let Some(status) = child.0.try_wait().unwrap() {
            break status;
        }
        assert!(
            Instant::now() < deadline,
            "Owned CLI did not exit within its startup bound"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    };
    assert!(fs::metadata(&log_path).unwrap().len() <= 65536);
    let output = fs::read_to_string(log_path).unwrap();
    let parsed = url::Url::parse(url).unwrap();
    for private in [Some(url), parsed.password(), directory.0.to_str(), setup]
        .into_iter()
        .flatten()
    {
        assert!(
            !output.contains(private),
            "CLI output exposed a private connection, secret or path"
        );
    }
    assert!(
        !output.contains("otrec1."),
        "A schema refusal must not print a recovery bearer"
    );
    (status, output)
}

#[tokio::test]
async fn historical_harness_construction_and_explicit_migrate_remain_available() {
    let latest = store::MIGRATOR.iter().map(|m| m.version).max().unwrap();
    let h = Harness::with_version(3, latest - 1).await;
    let runner = h.clone();
    let outcome = tokio::spawn(async move {
        let h = &runner;
        assert_eq!(
            sqlx::query_scalar::<_, i32>("SELECT 1")
                .fetch_one(&h.app.pool)
                .await
                .unwrap(),
            1
        );
        let before = history_snapshot(h).await;
        assert!(!store::schema_is_current(&h.app.pool).await.unwrap());
        assert!(
            before == history_snapshot(h).await,
            "Strict validation cannot upgrade historical fixtures"
        );
        let directory = PrivateDirectory::new();
        let reserved = TcpListener::bind("127.0.0.1:0").unwrap();
        let owner = scoped_url("OPENTRACK_TEST_MIGRATION_DATABASE_URL", &schema(h).await);
        let (status, _) = invoke(
            &directory,
            "OPENTRACK_MIGRATION_DATABASE_URL",
            &owner,
            None,
            reserved.local_addr().unwrap(),
            &["migrate".into()],
        )
        .await;
        assert!(
            status.success(),
            "Explicit migrate must remain capable of upgrading supported historical history"
        );
        assert!(store::schema_is_current(&h.app.pool).await.unwrap());
    })
    .await;
    h.cleanup().await;
    if let Err(error) = outcome {
        std::panic::resume_unwind(error.into_panic());
    }
}

#[tokio::test]
async fn incompatible_history_stops_all_entry_points_before_binding_effects_or_output() {
    let h = Harness::new().await;
    let runner = h.clone();
    let outcome = tokio::spawn(async move {
        let h = &runner;
        let (session, _) = h.app.setup(serde_json::from_value(json!({
            "setup_token":h.setup_token,"email":"schema-owner@example.test",
            "password":"Schema startup contract passphrase 752!","display_name":"Example Owner",
            "workspace_name":"Schema startup","team_name":"Engineering","team_key":"ENG"
        })).unwrap()).await.unwrap();
        let urls = restricted_urls(h).await;
        let runtime = scoped_url("OPENTRACK_TEST_DATABASE_URL", &schema(h).await);
        future_history(h).await;
        let history = history_snapshot(h).await;
        let before = effect_snapshot(h, session.user.id, session.workspaces[0].id).await;
        for (action, key, url) in [
            ("serve", "OPENTRACK_DATABASE_URL", runtime.as_str()),
            ("worker", "OPENTRACK_WORKER_DATABASE_URL", urls[0].as_str()),
            (
                "operator-recovery-issue",
                "OPENTRACK_OPERATOR_DATABASE_URL",
                urls[1].as_str(),
            ),
        ] {
            let directory = PrivateDirectory::new();
            // An occupied owned port makes guard-before-bind ordering observable without a listener race.
            let reserved = TcpListener::bind("127.0.0.1:0").unwrap();
            let mut args = vec![action.to_owned()];
            if action == "worker" {
                args.push("--once".into());
            }
            if action == "operator-recovery-issue" {
                args.extend([
                    "--user".into(),
                    session.user.id.to_string(),
                    "--reason".into(),
                    "Synthetic schema guard rehearsal".into(),
                    "--output-file".into(),
                    directory
                        .0
                        .join("recovery-code")
                        .to_str()
                        .unwrap()
                        .to_owned(),
                ]);
            }
            let (status, output) = invoke(
                &directory,
                key,
                url,
                (action == "serve").then_some(h.setup_token.as_str()),
                reserved.local_addr().unwrap(),
                &args,
            )
            .await;
            assert!(
                !status.success(),
                "Incompatible history must refuse every executable entry point"
            );
            assert!(
                output.contains("migration history"),
                "History refusal must precede capability, bind or command failure"
            );
            assert!(
                !directory.0.join("blobs").exists(),
                "No storage or worker stage may start before validation"
            );
            assert!(
                !directory.0.join("recovery-code").exists(),
                "No recovery bearer may be created before validation"
            );
        }
        let check = PrivateDirectory::new();
        let reserved = TcpListener::bind("127.0.0.1:0").unwrap();
        let (status, output) = invoke(
            &check,
            "OPENTRACK_DATABASE_URL",
            &runtime,
            Some(&h.setup_token),
            reserved.local_addr().unwrap(),
            &["check-config".into(), "--for".into(), "serve".into()],
        )
        .await;
        assert!(status.success() && output.contains("no connection or listener was opened"));
        assert!(!check.0.join("blobs").exists());
        assert!(
            history == history_snapshot(h).await,
            "CLI guards and check-config cannot mutate migration history"
        );
        let after = effect_snapshot(h, session.user.id, session.workspaces[0].id).await;
        assert!(
            before == after,
            "Refused entry points must not perform worker or operator effects"
        );
    })
    .await;
    h.cleanup().await;
    if let Err(error) = outcome {
        std::panic::resume_unwind(error.into_panic());
    }
}
