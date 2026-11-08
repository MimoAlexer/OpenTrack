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
    // Catalog-only checks also run after a rejected App constructor, so a
    // reprovisioning regression reports the exact notification ACL boundary.
    let notification_grants = match sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(&runtime)
        .await
    {
        Ok(pool) => {
            let present =
                sqlx::query_scalar::<_, bool>("SELECT to_regclass('notifications') IS NOT NULL")
                    .fetch_one(&pool)
                    .await
                    .unwrap_or(false);
            let valid = if present {
                sqlx::query_scalar::<_, bool>(
                    "WITH expected(tab,readable,insert_columns,update_columns) AS (VALUES
                     ('notifications',true,ARRAY[]::text[],ARRAY['read_at','archived_at','snoozed_until','version']),
                     ('notification_heads',true,ARRAY[]::text[],ARRAY[]::text[]),
                     ('notification_preferences',true,ARRAY['workspace_id','user_id','assignments','mentions','subscribed_comments'],ARRAY['assignments','mentions','subscribed_comments','version','updated_at']),
                     ('notification_invalidations',false,ARRAY['workspace_id','user_id','notification_id','notification_version','event_kind'],ARRAY[]::text[]),
                     ('notification_sources',false,ARRAY[]::text[],ARRAY[]::text[]),
                     ('notification_backfills',false,ARRAY[]::text[],ARRAY[]::text[]))
                     SELECT count(DISTINCT expected.tab)=6 AND coalesce(bool_and(
                       c.relrowsecurity AND c.relforcerowsecurity
                       AND NOT has_table_privilege(current_user,c.oid,'DELETE,TRUNCATE,TRIGGER')
                       AND has_column_privilege(current_user,c.oid,a.attnum,'SELECT')=expected.readable
                       AND has_column_privilege(current_user,c.oid,a.attnum,'INSERT')=(a.attname::text=ANY(expected.insert_columns))
                       AND has_column_privilege(current_user,c.oid,a.attnum,'UPDATE')=(a.attname::text=ANY(expected.update_columns))
                       AND NOT has_column_privilege(current_user,c.oid,a.attnum,'REFERENCES')),false)
                     FROM expected JOIN pg_class c ON c.oid=to_regclass(expected.tab)
                     JOIN pg_attribute a ON a.attrelid=c.oid AND a.attnum>0 AND NOT a.attisdropped",
                ).fetch_one(&pool).await.unwrap_or(false)
            } else {
                true
            };
            pool.close().await;
            valid
        }
        Err(_) => false,
    };
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
        json!({"app_connect":app_ok,"worker_connect":worker_ok,"workflow_grants":workflow_grants,"notification_grants":notification_grants})
    );
    if !(app_ok && worker_ok && workflow_grants && notification_grants) {
        std::process::exit(1);
    }
}
