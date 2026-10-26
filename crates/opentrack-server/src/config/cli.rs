use super::{Result, safe_location};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Serve,
    Migrate,
    Worker,
    OperatorRecoveryIssue,
    OperatorRecoveryRevoke,
    OperatorAccountDisable,
    OperatorAccountEnable,
    OperatorConsumerStatus,
    OperatorConsumerDisposition,
    OperatorConsumerEnable,
    OperatorConsumerDisable,
    OperatorEventLimits,
    OperatorCleanupResume,
    OperatorPlanningRebuild,
    CheckConfig,
    PrintConfig,
    ConfigSchema,
    ConfigReference,
    Help,
}
impl Action {
    pub fn is_consumer_operator(self) -> bool {
        matches!(
            self,
            Self::OperatorConsumerStatus
                | Self::OperatorConsumerDisposition
                | Self::OperatorConsumerEnable
                | Self::OperatorConsumerDisable
                | Self::OperatorEventLimits
                | Self::OperatorCleanupResume
                | Self::OperatorPlanningRebuild
        )
    }
    pub fn is_operator(self) -> bool {
        self.is_consumer_operator()
            || matches!(
                self,
                Self::OperatorRecoveryIssue
                    | Self::OperatorRecoveryRevoke
                    | Self::OperatorAccountDisable
                    | Self::OperatorAccountEnable
            )
    }
}
pub struct Cli {
    pub action: Action,
    pub config: Option<PathBuf>,
    pub environment: Option<PathBuf>,
    pub check_target: Action,
    pub overrides: Vec<(String, String, String)>,
    pub operator_user: Option<uuid::Uuid>,
    pub operator_reason: Option<String>,
    pub operator_workspace: Option<uuid::Uuid>,
    pub operator_dead_letter: Option<uuid::Uuid>,
    pub operator_job: Option<uuid::Uuid>,
    pub operator_disposition: Option<String>,
    pub max_pending_events: Option<i64>,
    pub max_event_bytes: Option<i64>,
    pub output_file: Option<PathBuf>,
    pub once: bool,
}
impl Default for Cli {
    fn default() -> Self {
        Self {
            action: Action::Serve,
            config: None,
            environment: None,
            check_target: Action::Serve,
            overrides: Vec::new(),
            operator_user: None,
            operator_reason: None,
            operator_workspace: None,
            operator_dead_letter: None,
            operator_job: None,
            operator_disposition: None,
            max_pending_events: None,
            max_event_bytes: None,
            output_file: None,
            once: false,
        }
    }
}
impl Cli {
    pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Self> {
        let mut cli = Self::default();
        let mut action_seen = false;
        let mut args = args.into_iter();
        let mut seen = std::collections::BTreeSet::new();
        while let Some(arg) = args.next() {
            if arg == "--help" || arg == "-h" || arg == "help" {
                cli.action = Action::Help;
                return Ok(cli);
            }
            if !arg.starts_with('-') {
                if action_seen {
                    return Err("Only one command may be selected".into());
                }
                cli.action = match arg.as_str() {
                    "serve" => Action::Serve,
                    "migrate" => Action::Migrate,
                    "worker" => Action::Worker,
                    "operator-recovery-issue" => Action::OperatorRecoveryIssue,
                    "operator-recovery-revoke" => Action::OperatorRecoveryRevoke,
                    "operator-account-disable" => Action::OperatorAccountDisable,
                    "operator-account-enable" => Action::OperatorAccountEnable,
                    "operator-consumer-status" => Action::OperatorConsumerStatus,
                    "operator-consumer-disposition" => Action::OperatorConsumerDisposition,
                    "operator-consumer-enable" => Action::OperatorConsumerEnable,
                    "operator-consumer-disable" => Action::OperatorConsumerDisable,
                    "operator-event-limits" => Action::OperatorEventLimits,
                    "operator-cleanup-resume" => Action::OperatorCleanupResume,
                    "operator-planning-rebuild" => Action::OperatorPlanningRebuild,
                    "check-config" => Action::CheckConfig,
                    "print-config" => Action::PrintConfig,
                    "config-schema" => Action::ConfigSchema,
                    "config-reference" => Action::ConfigReference,
                    _ => return Err("Unknown command; use --help".into()),
                };
                action_seen = true;
                continue;
            }
            let (flag, inline) = arg
                .split_once('=')
                .map(|(a, b)| (a, Some(b.to_string())))
                .unwrap_or((&arg, None));
            if flag == "--once" {
                if inline.is_some() || !seen.insert(flag.to_string()) {
                    return Err("--once accepts no value and may occur only once".into());
                }
                cli.once = true;
                continue;
            }
            if !matches!(
                flag,
                "--config"
                    | "--environment"
                    | "--for"
                    | "--user"
                    | "--reason"
                    | "--output-file"
                    | "--workspace"
                    | "--dead-letter"
                    | "--job"
                    | "--disposition"
                    | "--max-pending-events"
                    | "--max-event-bytes"
            ) && !OVERRIDES.iter().any(|(name, _)| *name == flag)
            {
                return Err(format!(
                    "Unknown option {}; use --help",
                    safe_location(flag)
                ));
            }
            if !seen.insert(flag.to_string()) {
                return Err(format!(
                    "Option {} may be provided only once",
                    safe_location(flag)
                ));
            }
            let value = inline
                .or_else(|| args.next())
                .ok_or_else(|| format!("Option {flag} requires a value"))?;
            if value.starts_with("--") {
                return Err(format!("Option {flag} requires a value"));
            }
            match flag {
                "--config" => cli.config = Some(value.into()),
                "--environment" => cli.environment = Some(value.into()),
                "--user" => {
                    cli.operator_user =
                        Some(uuid::Uuid::parse_str(&value).map_err(|_| "--user requires a UUID")?)
                }
                "--workspace" => {
                    cli.operator_workspace = Some(
                        uuid::Uuid::parse_str(&value).map_err(|_| "--workspace requires a UUID")?,
                    )
                }
                "--dead-letter" => {
                    cli.operator_dead_letter = Some(
                        uuid::Uuid::parse_str(&value)
                            .map_err(|_| "--dead-letter requires a UUID")?,
                    )
                }
                "--job" => {
                    cli.operator_job =
                        Some(uuid::Uuid::parse_str(&value).map_err(|_| "--job requires a UUID")?)
                }
                "--disposition" => {
                    if !matches!(value.as_str(), "retry" | "reconcile") {
                        return Err("--disposition requires retry or reconcile".into());
                    }
                    cli.operator_disposition = Some(value);
                }
                "--max-pending-events" | "--max-event-bytes" => {
                    let limit: i64 = value
                        .parse()
                        .map_err(|_| "Event limits require a positive integer")?;
                    if limit <= 0 {
                        return Err("Event limits require a positive integer".into());
                    }
                    if flag == "--max-pending-events" {
                        cli.max_pending_events = Some(limit);
                    } else {
                        cli.max_event_bytes = Some(limit);
                    }
                }
                "--reason" => {
                    let reason = value.trim();
                    if reason.is_empty()
                        || reason.chars().count() > 500
                        || value.chars().any(char::is_control)
                    {
                        return Err(
                            "--reason requires 1 through 500 characters without control characters"
                                .into(),
                        );
                    }
                    cli.operator_reason = Some(reason.into());
                }
                "--output-file" => cli.output_file = Some(value.into()),
                "--for" => {
                    cli.check_target = match value.as_str() {
                        "serve" => Action::Serve,
                        "migrate" => Action::Migrate,
                        "worker" => Action::Worker,
                        "operator" => Action::OperatorRecoveryIssue,
                        _ => {
                            return Err("--for requires serve, migrate, worker, or operator".into());
                        }
                    }
                }
                _ => {
                    let path = OVERRIDES
                        .iter()
                        .find(|(name, _)| *name == flag)
                        .expect("validated flag")
                        .1;
                    cli.overrides.push((path.into(), value, flag.into()));
                }
            }
        }
        if cli.once && cli.action != Action::Worker {
            return Err("--once applies only to worker".into());
        }
        if seen.contains("--for") && cli.action != Action::CheckConfig {
            return Err("--for is supported only by check-config".into());
        }
        if cli.action == Action::OperatorEventLimits {
            if cli.max_pending_events.is_none() || cli.max_event_bytes.is_none() {
                return Err(
                    "operator-event-limits requires --max-pending-events and --max-event-bytes"
                        .into(),
                );
            }
        } else if cli.max_pending_events.is_some() || cli.max_event_bytes.is_some() {
            return Err("Event limits apply only to operator-event-limits".into());
        }
        if cli.action.is_consumer_operator() {
            if cli.operator_workspace.is_none() {
                return Err("Consumer operator commands require --workspace".into());
            }
            if cli.operator_user.is_some() || cli.output_file.is_some() {
                return Err(
                    "Consumer operator commands do not accept --user or --output-file".into(),
                );
            }
            if cli.action == Action::OperatorConsumerStatus {
                if cli.operator_reason.is_some() {
                    return Err("Read-only consumer status does not accept --reason".into());
                }
            } else if cli.operator_reason.is_none() {
                return Err("Consumer changes require --reason".into());
            }
            if cli.action == Action::OperatorConsumerDisposition {
                if cli.operator_dead_letter.is_none() || cli.operator_disposition.is_none() {
                    return Err(
                        "Consumer disposition requires --dead-letter and --disposition".into(),
                    );
                }
            } else if cli.operator_dead_letter.is_some() || cli.operator_disposition.is_some() {
                return Err(
                    "--dead-letter and --disposition apply only to operator-consumer-disposition"
                        .into(),
                );
            }
            if cli.action == Action::OperatorCleanupResume {
                if cli.operator_job.is_none() {
                    return Err("Cleanup resume requires --job".into());
                }
            } else if cli.operator_job.is_some() {
                return Err("--job applies only to operator-cleanup-resume".into());
            }
        } else if cli.action.is_operator() {
            if cli.operator_user.is_none() || cli.operator_reason.is_none() {
                return Err("Operator commands require --user and --reason".into());
            }
            if cli.action == Action::OperatorRecoveryIssue && cli.output_file.is_none() {
                return Err("operator-recovery-issue requires --output-file".into());
            }
            if cli.action != Action::OperatorRecoveryIssue && cli.output_file.is_some() {
                return Err("--output-file applies only to operator-recovery-issue".into());
            }
        } else if cli.operator_user.is_some()
            || cli.operator_reason.is_some()
            || cli.output_file.is_some()
        {
            return Err(
                "--user, --reason, and --output-file apply only to operator commands".into(),
            );
        }
        if !cli.action.is_consumer_operator()
            && (cli.operator_workspace.is_some()
                || cli.operator_dead_letter.is_some()
                || cli.operator_disposition.is_some()
                || cli.operator_job.is_some())
        {
            return Err("--workspace, --dead-letter, --disposition and --job apply only to consumer operator commands".into());
        }
        Ok(cli)
    }
    pub fn help() -> &'static str {
        r#"OpenTrack
  serve                   Start API and web client
  migrate                 Apply migrations with the separate migration role
  worker                  Run event capture, bounded maintenance and workflow batches
  worker --once           Run one bounded capture, job and retention poll, then exit
  check-config --for worker  Validate the separate worker secret without connecting
  operator-recovery-issue --user UUID --reason TEXT --output-file ABSOLUTE
                          Issue a one-use recovery code through the separate local operator role
  operator-recovery-revoke --user UUID --reason TEXT
                          Revoke the account's pending recovery codes
  operator-account-disable --user UUID --reason TEXT
                          Disable an account and revoke credentials, preserving enabled owners
  operator-account-enable --user UUID --reason TEXT
                          Re-enable an account without reviving revoked credentials
  operator-consumer-status --workspace UUID
                          Inspect bounded consumer lag, dead letters and failed cleanup jobs
  operator-consumer-disposition --workspace UUID --dead-letter UUID --disposition retry|reconcile --reason TEXT
                          Retry a poison event or durably capture a replacement reconciliation
  operator-consumer-disable --workspace UUID --reason TEXT
                          Disable a drained consumer while preserving its retention protection
  operator-consumer-enable --workspace UUID --reason TEXT
                          Re-enable the registered consumer
  operator-cleanup-resume --workspace UUID --job UUID --reason TEXT
                          Resume a failed assignment cleanup with a new retry budget
  operator-planning-rebuild --workspace UUID --reason TEXT
                          Reconcile planning facts without advancing their checkpoint before completion
  operator-event-limits --workspace UUID --max-pending-events INTEGER --max-event-bytes INTEGER --reason TEXT
                          Set durable event admission limits with an audited reason
  check-config            Validate settings and secrets for serving, without connecting
  check-config --for migrate  Validate settings and migration secret without connecting
  check-config --for operator Validate settings and operator secret without connecting
  print-config            Print effective TOML and per-field provenance; secrets redacted
  config-schema           Print the JSON Schema for implemented TOML settings
  config-reference        Print the generated reference for implemented settings

Explicit file selection (no working-directory configuration is read implicitly):
  --config PATH           Base TOML; each selected file requires schema_version = 1
  --environment PATH      Optional TOML overlay, merged by key; arrays replace

Named process overrides:
  --mode production|development|test
  --listen SOCKET         --public-url URL          --static-dir PATH
  --allowed-origins '["https://example.test"]'
  --api-max-connections N  --hash-concurrency N
  --database-url-file PATH --migration-database-url-file PATH --setup-token-file PATH
  --operator-database-url-file PATH --recovery-ttl-minutes N
  --worker-database-url-file PATH --worker-max-connections N
  --worker-lease-seconds N --worker-poll-interval-ms N

Environment: OPENTRACK__SECTION__KEY (strings are literal, numbers typed,
arrays use TOML syntax). Legacy OPENTRACK_* names remain compatible.
Precedence: defaults < base < overlay < legacy env < typed env < CLI.
Literal secrets and secret-file references are mutually exclusive.
"#
    }
}
const OVERRIDES: &[(&str, &str)] = &[
    ("--worker-database-url-file", "worker.database_url_file"),
    ("--worker-max-connections", "worker.max_connections"),
    ("--worker-lease-seconds", "worker.lease_seconds"),
    ("--worker-poll-interval-ms", "worker.poll_interval_ms"),
    ("--mode", "server.environment"),
    ("--listen", "server.listen"),
    ("--public-url", "server.public_url"),
    ("--static-dir", "server.static_dir"),
    ("--allowed-origins", "server.allowed_origins"),
    ("--api-max-connections", "database.api_max_connections"),
    ("--hash-concurrency", "auth.hash_concurrency"),
    ("--recovery-ttl-minutes", "auth.recovery_ttl_minutes"),
    ("--operator-database-url-file", "operator.database_url_file"),
    ("--database-url-file", "database.url_file"),
    (
        "--migration-database-url-file",
        "database.migration_url_file",
    ),
    ("--setup-token-file", "auth.setup_token_file"),
];
