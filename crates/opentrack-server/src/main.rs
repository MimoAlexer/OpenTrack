mod config;
mod operator;
mod storage;
mod worker;
use config::{Action, Cli, Settings};
use opentrack_api::ApiState;
use opentrack_app::App;
use std::sync::Arc;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .json()
        .init();
    if let Err(e) = run().await {
        tracing::error!(message=%e,"Startup failed");
        std::process::exit(1);
    }
}
async fn run() -> Result<(), String> {
    let cli = Cli::parse(std::env::args().skip(1))?;
    if cli.action == Action::Help {
        print!("{}", Cli::help());
        return Ok(());
    }
    if cli.action == Action::ConfigSchema {
        println!(
            "{}",
            serde_json::to_string_pretty(&config::json_schema())
                .map_err(|_| "Cannot render configuration schema")?
        );
        return Ok(());
    }
    if cli.action == Action::ConfigReference {
        print!("{}", config::reference_markdown());
        return Ok(());
    }
    let cfg = Settings::load(&cli)?;
    if cli.action == Action::PrintConfig {
        print!("{}", cfg.effective_toml()?);
        return Ok(());
    }
    if cli.action == Action::CheckConfig {
        cfg.validate_secrets(cli.check_target)?;
        println!(
            "Configuration and required secrets are valid; no connection or listener was opened"
        );
        return Ok(());
    }
    if cli.action == Action::Worker {
        return worker::run(&cli, &cfg).await;
    }
    if cli.action.is_operator() {
        return operator::run(&cli, &cfg).await;
    }
    if cli.action == Action::Migrate {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&cfg.database_url(true)?)
            .await
            .map_err(|_| "Cannot connect with migration role (connection details redacted)")?;
        opentrack_store::MIGRATOR
            .run(&pool)
            .await
            .map_err(|e| format!("Migration failed: {e}"))?;
        pool.close().await;
        println!("Migrations applied successfully");
        return Ok(());
    }
    let app = App::connect(
        &cfg.database_url(false)?,
        &cfg.setup_token()?,
        cfg.database.api_max_connections,
        cfg.auth.hash_concurrency,
    )
    .await
    .map_err(|e| e.to_string())?;
    let missing: bool = sqlx::query_scalar("SELECT to_regclass('public.issues') IS NULL OR to_regclass('public.documents') IS NULL OR to_regclass('public.document_revisions') IS NULL OR to_regclass('public.attachments') IS NULL OR to_regclass('public.issue_templates') IS NULL OR NOT EXISTS(SELECT 1 FROM pg_attribute WHERE attrelid=to_regclass('public.issue_templates') AND attname='title_mode' AND NOT attisdropped) OR to_regclass('public.issue_template_sources') IS NULL OR to_regclass('public.issue_subscriptions') IS NULL OR to_regclass('public.document_mentions') IS NULL OR to_regclass('public.mention_intents') IS NULL OR to_regclass('public.subscription_events') IS NULL OR to_regclass('public.discussion_sources') IS NULL OR to_regclass('public.user_preferences') IS NULL OR to_regclass('public.user_preference_events') IS NULL OR to_regclass('public.projects') IS NULL OR to_regclass('public.project_issues') IS NULL OR to_regclass('public.project_updates') IS NULL")
        .fetch_one(&app.pool)
        .await
        .map_err(|_| "Cannot check schema")?;
    if missing {
        app.pool.close().await;
        return Err("Apply the current migrations before starting this server version".into());
    }
    let attachments = storage::open(&cfg).await?;
    let state = ApiState {
        attachments: Some(Arc::new(opentrack_api::AttachmentService::new(
            attachments,
            storage::limits(&cfg),
        ))),
        app: app.clone(),
        origins: Arc::new(cfg.origins()),
        secure_cookie: cfg.server.public_url.starts_with("https:"),
        static_dir: Arc::new(cfg.server.static_dir.clone()),
    };
    let listener = tokio::net::TcpListener::bind(cfg.server.listen)
        .await
        .map_err(|_| "Cannot bind listener")?;
    tracing::info!(listen=%cfg.server.listen,environment=%cfg.server.environment,local_container=cfg.server.local_container,schema_version=cfg.schema_version,"OpenTrack listening");
    axum_serve(listener, state).await?;
    app.pool.close().await;
    Ok(())
}
async fn axum_serve(listener: tokio::net::TcpListener, state: ApiState) -> Result<(), String> {
    opentrack_api::serve(listener, state, shutdown())
        .await
        .map_err(|_| "HTTP server failed".into())
}
async fn shutdown() {
    let interrupt = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut s) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            s.recv().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {_=interrupt=>{},_=terminate=>{}}
    tracing::info!("Graceful shutdown requested");
}
