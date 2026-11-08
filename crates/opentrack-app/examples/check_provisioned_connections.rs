//! Read-only local provisioning check: no listener, sessions or job execution.
use opentrack_app::{
    App,
    worker::{Worker, WorkerOptions},
};
use serde_json::json;
use url::Url;

#[tokio::main]
async fn main() {
    if std::env::args().skip(1).collect::<Vec<_>>() != ["--read-only-local-role-check"] {
        eprintln!(
            "Use --read-only-local-role-check with explicit runtime/worker database environment."
        );
        std::process::exit(2);
    }
    let load = |name| {
        std::env::var(name).ok().and_then(|raw| {
            let parsed = Url::parse(&raw).ok()?;
            (matches!(parsed.scheme(), "postgres" | "postgresql")
                && matches!(parsed.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"))
                && parsed.query().is_none()
                && parsed.fragment().is_none())
            .then_some((raw, parsed))
        })
    };
    let (Some((runtime, api_target)), Some((worker, worker_target))) = (
        load("OPENTRACK_DATABASE_URL"),
        load("OPENTRACK_WORKER_DATABASE_URL"),
    ) else {
        eprintln!(
            "Provide explicit local PostgreSQL runtime and worker credentials; no values printed."
        );
        std::process::exit(2);
    };
    if (api_target.host_str(), api_target.port(), api_target.path())
        != (
            worker_target.host_str(),
            worker_target.port(),
            worker_target.path(),
        )
        || api_target.username() == worker_target.username()
    {
        eprintln!("Runtime and worker must use the same local database with distinct identities.");
        std::process::exit(2);
    }
    let app = App::connect(
        &runtime,
        "Synthetic provisioning check; not an authentication request",
        1,
        1,
    )
    .await;
    let app_ok = app.is_ok();
    let mut workflow_grants = false;
    if let Ok(app) = app {
        workflow_grants = sqlx::query_scalar::<_, bool>(
            "SELECT bool_and(to_regclass(tab) IS NOT NULL AND coalesce(has_table_privilege(current_user,to_regclass(tab),priv),false)) FROM (VALUES
             ('issue_subscriptions','SELECT'),('issue_subscriptions','INSERT'),
             ('subscription_events','SELECT'),('subscription_events','INSERT'),
             ('issue_templates','SELECT'),('issue_templates','INSERT'),('issue_templates','UPDATE'),
             ('issue_template_sources','SELECT'),('issue_template_sources','INSERT')) expected(tab,priv)",
        ).fetch_one(&app.pool).await.unwrap_or(false);
        workflow_grants &= sqlx::query_scalar::<_, bool>(
            "SELECT has_column_privilege(current_user,'issue_subscriptions','preference','UPDATE')
             AND has_column_privilege(current_user,'issue_subscriptions','version','UPDATE')
             AND has_column_privilege(current_user,'issue_subscriptions','updated_at','UPDATE')
             AND has_table_privilege(current_user,'planning_issue_facts','SELECT')
             AND NOT has_any_column_privilege(current_user,'planning_issue_facts','INSERT,UPDATE,REFERENCES')
             AND NOT has_table_privilege(current_user,'planning_issue_facts','DELETE,TRUNCATE,TRIGGER')",
        ).fetch_one(&app.pool).await.unwrap_or(false);
        app.pool.close().await;
    }
    let worker = Worker::connect(
        &worker,
        WorkerOptions {
            max_connections: 1,
            ..Default::default()
        },
    )
    .await;
    let worker_ok = worker.is_ok();
    if let Ok(worker) = worker {
        worker.pool.close().await;
    }
    println!(
        "{}",
        json!({"app_connect":app_ok,"worker_connect":worker_ok,"workflow_grants":workflow_grants})
    );
    if !(app_ok && worker_ok && workflow_grants) {
        std::process::exit(1);
    }
}
