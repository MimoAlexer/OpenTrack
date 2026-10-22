use std::{
    env, fs,
    net::TcpListener,
    path::PathBuf,
    process::{Command, Output, Stdio},
    thread,
    time::{Duration, Instant},
};

struct Fixture {
    root: PathBuf,
    occupied: TcpListener,
}

impl Fixture {
    fn new() -> Self {
        let root = env::temp_dir().join(format!("opentrack-startup-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        Self {
            root,
            occupied: TcpListener::bind("127.0.0.1:0").unwrap(),
        }
    }

    fn run(&self, action: &str, overrides: &[(&str, &str)], args: &[&str]) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_opentrack-server"));
        for (key, _) in
            env::vars_os().filter(|(key, _)| key.to_string_lossy().starts_with("OPENTRACK"))
        {
            command.env_remove(key);
        }
        command
            .arg(action)
            .args(args)
            .env("OPENTRACK_ENVIRONMENT", "test")
            .env("OPENTRACK_PUBLIC_URL", "http://127.0.0.1:8080")
            .env(
                "OPENTRACK_LISTEN",
                self.occupied.local_addr().unwrap().to_string(),
            )
            .env("OPENTRACK__STORAGE__LOCAL_ROOT", self.root.join("blobs"))
            .env(
                "OPENTRACK_DATABASE_URL",
                "postgres://role:synthetic-secret@127.0.0.1:1/config_test?connect_timeout=1",
            )
            .env(
                "OPENTRACK_SETUP_TOKEN",
                "synthetic-setup-token-at-least-32-bytes",
            )
            .envs(overrides.iter().copied())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn().unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while child.try_wait().unwrap().is_none() {
            if Instant::now() >= deadline {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("configuration command exceeded its connection-free deadline");
            }
            thread::sleep(Duration::from_millis(10));
        }
        child.wait_with_output().unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[test]
fn invalid_fields_and_secrets_fail_before_listener_or_database_startup() {
    let fixture = Fixture::new();
    for (key, value, field) in [
        (
            "OPENTRACK__AUTH__HASH_CONCURRENCY",
            "SECRET_CANARY",
            "auth.hash_concurrency",
        ),
        ("OPENTRACK__SCHEMA_VERSION", "2", "schema_version"),
        (
            "OPENTRACK__STORAGE__LOCAL_ROOT",
            "SECRET_CANARY",
            "storage.local_root",
        ),
        ("OPENTRACK_DATABASE_URL", "SECRET_CANARY", "database.url"),
        ("OPENTRACK_SETUP_TOKEN", "SECRET_CANARY", "auth.setup_token"),
    ] {
        let output = fixture.run("serve", &[(key, value)], &[]);
        assert!(
            !output.status.success(),
            "invalid field was accepted: {field}"
        );
        let message = text(&output);
        assert!(message.contains(field), "missing exact field: {field}");
        assert!(!message.contains("SECRET_CANARY"));
        assert!(!message.contains("Cannot bind listener"));
        assert!(!message.contains("Cannot connect"));
        assert!(fixture.occupied.local_addr().is_ok());
    }
}

#[test]
fn check_config_validates_each_command_without_connecting_or_listening() {
    let fixture = Fixture::new();
    for (target, variable) in [
        ("serve", "OPENTRACK_DATABASE_URL"),
        ("migrate", "OPENTRACK_MIGRATION_DATABASE_URL"),
        ("worker", "OPENTRACK_WORKER_DATABASE_URL"),
        ("operator", "OPENTRACK_OPERATOR_DATABASE_URL"),
    ] {
        let output = fixture.run(
            "check-config",
            &[(
                variable,
                "postgres://role:synthetic-secret@127.0.0.1:1/config_test",
            )],
            &["--for", target],
        );
        assert!(
            output.status.success(),
            "connection-free validation failed: {target}"
        );
        assert!(text(&output).contains("no connection or listener was opened"));
        assert!(!text(&output).contains("synthetic-secret"));
    }
}

#[test]
fn print_config_redacts_secrets_and_preserves_winning_sources() {
    let fixture = Fixture::new();
    let output = fixture.run(
        "print-config",
        &[
            ("OPENTRACK__DATABASE__API_MAX_CONNECTIONS", "6"),
            (
                "OPENTRACK_MIGRATION_DATABASE_URL",
                "postgres://role:synthetic-secret-migration@127.0.0.1:1/config_test",
            ),
            (
                "OPENTRACK_OPERATOR_DATABASE_URL",
                "postgres://role:synthetic-secret-operator@127.0.0.1:1/config_test",
            ),
            (
                "OPENTRACK_WORKER_DATABASE_URL",
                "postgres://role:synthetic-secret-worker@127.0.0.1:1/config_test",
            ),
        ],
        &["--api-max-connections", "7"],
    );
    assert!(output.status.success());
    let report = text(&output);
    assert!(!report.contains("synthetic-secret"));
    assert!(!report.contains("synthetic-setup-token"));
    let report: toml::Value = toml::from_str(&report).unwrap();
    assert_eq!(
        report["settings"]["database"]["api_max_connections"].as_integer(),
        Some(7)
    );
    assert_eq!(
        report["provenance"]["database.api_max_connections"]["location"].as_str(),
        Some("--api-max-connections")
    );
    assert_eq!(
        report["settings"]["database"]["url"].as_str(),
        Some("<redacted>")
    );
    for (section, key) in [
        ("database", "migration_url"),
        ("operator", "database_url"),
        ("worker", "database_url"),
        ("auth", "setup_token"),
    ] {
        assert_eq!(
            report["settings"][section][key].as_str(),
            Some("<redacted>")
        );
    }
}
