use super::*;

struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let root = env::temp_dir().join(format!("opentrack-config-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        Self(root)
    }
    fn write(&self, name: &str, text: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::write(&path, text).unwrap();
        path
    }
    #[cfg(unix)]
    fn secret(&self, name: &str, text: &str, mode: u32) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = self.write(name, text);
        fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
        path
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn environment() -> BTreeMap<String, String> {
    BTreeMap::from([
        (
            "OPENTRACK__STORAGE__LOCAL_ROOT".into(),
            std::env::temp_dir()
                .join("opentrack-config-blobs")
                .to_string_lossy()
                .into_owned(),
        ),
        ("OPENTRACK_ENVIRONMENT".into(), "test".into()),
        (
            "OPENTRACK_PUBLIC_URL".into(),
            "http://127.0.0.1:8080".into(),
        ),
    ])
}
#[tokio::test]
async fn local_storage_accepts_existing_relative_development_webroot() {
    let temp = Temp::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&temp.0, fs::Permissions::from_mode(0o700)).unwrap();
    }
    let mut values = environment();
    values.insert(
        "OPENTRACK__STORAGE__LOCAL_ROOT".into(),
        temp.0.to_str().unwrap().into(),
    );
    values.insert("OPENTRACK_STATIC_DIR".into(), ".".into());
    let settings = Settings::from_sources(&Cli::default(), &values).unwrap();
    assert!(settings.server.static_dir.is_relative());
    let (api, worker) = tokio::join!(
        crate::storage::open(&settings),
        crate::storage::open(&settings)
    );
    assert!(api.is_ok(), "{}", api.err().unwrap());
    assert!(worker.is_ok(), "{}", worker.err().unwrap());
    for child in ["keys", "staging", "objects"] {
        assert!(temp.0.join(child).is_dir());
    }
}
fn cli(args: &[&str]) -> Cli {
    Cli::parse(args.iter().map(|arg| (*arg).to_owned())).unwrap()
}
fn invalid(cli: &Cli, values: &BTreeMap<String, String>) -> String {
    match Settings::from_sources(cli, values) {
        Ok(_) => panic!("invalid configuration was accepted"),
        Err(message) => message,
    }
}
fn file_cli(path: PathBuf) -> Cli {
    Cli {
        config: Some(path),
        ..Cli::default()
    }
}

#[test]
fn precedence_merges_tables_replaces_arrays_and_reports_winning_sources() {
    let temp = Temp::new();
    let base = temp.write("base.toml", "schema_version=1\n[server]\nallowed_origins=['https://base.example']\n[database]\napi_max_connections=4\n[auth]\nhash_concurrency=3\n");
    let overlay = temp.write("environment.toml", "schema_version=1\n[server]\nallowed_origins=['https://overlay.example']\n[database]\napi_max_connections=5\n");
    let mut args = cli(&["--api-max-connections", "9"]);
    args.config = Some(base);
    args.environment = Some(overlay.clone());
    let mut values = environment();
    values.insert(
        "OPENTRACK__DATABASE__API_MAX_CONNECTIONS".into(),
        "7".into(),
    );
    values.insert(
        "OPENTRACK__SERVER__PUBLIC_URL".into(),
        "http://localhost:8081".into(),
    );
    let settings = Settings::from_sources(&args, &values).unwrap();
    assert_eq!(settings.database.api_max_connections, 9);
    assert_eq!(settings.auth.hash_concurrency, 3);
    assert_eq!(settings.server.public_url, "http://localhost:8081");
    assert_eq!(settings.server.allowed_origins, ["https://overlay.example"]);
    assert_eq!(
        settings.provenance["database.api_max_connections"].kind,
        "command_line"
    );
    assert_eq!(
        settings.provenance["server.public_url"].location,
        "OPENTRACK__SERVER__PUBLIC_URL"
    );
    assert_eq!(
        settings.provenance["server.allowed_origins"].location,
        overlay.to_string_lossy()
    );
    assert_eq!(settings.provenance["server.allowed_origins"].line, Some(3));
    assert_eq!(settings.provenance["server.listen"].kind, "default");
    let without_cli = Cli {
        overrides: Vec::new(),
        ..args
    };
    assert_eq!(
        Settings::from_sources(&without_cli, &values)
            .unwrap()
            .database
            .api_max_connections,
        7
    );
    values.remove("OPENTRACK__DATABASE__API_MAX_CONNECTIONS");
    assert_eq!(
        Settings::from_sources(&without_cli, &values)
            .unwrap()
            .database
            .api_max_connections,
        5
    );
}

#[test]
fn selected_files_each_require_supported_schema() {
    let temp = Temp::new();
    for text in [
        "[server]\nlisten='127.0.0.1:8080'",
        "schema_version=0",
        "schema_version=2",
        "schema_version='1'",
    ] {
        let path = temp.write("bad.toml", text);
        assert!(invalid(&file_cli(path.clone()), &environment()).contains("schema_version"));
        let args = Cli {
            environment: Some(path),
            ..Cli::default()
        };
        assert!(invalid(&args, &environment()).contains("schema_version"));
    }
}

#[test]
fn future_schema_rejection_never_rewrites_base_overlay_or_creates_backups() {
    let temp = Temp::new();
    let base_text = "schema_version=1\n[auth]\nhash_concurrency=3\n";
    let overlay_text = "schema_version=2\n[auth]\nhash_concurrency=4\n";
    let base = temp.write("base.toml", base_text);
    let overlay = temp.write("overlay.toml", overlay_text);
    let args = Cli {
        config: Some(base.clone()),
        environment: Some(overlay.clone()),
        ..Cli::default()
    };
    let mut values = environment();
    values.insert("OPENTRACK__SCHEMA_VERSION".into(), "1".into());
    let message = invalid(&args, &values);
    assert!(message.contains("schema_version"));
    assert!(message.contains("overlay.toml:1:1"));
    assert_eq!(fs::read_to_string(base).unwrap(), base_text);
    assert_eq!(fs::read_to_string(overlay).unwrap(), overlay_text);
    assert_eq!(fs::read_dir(&temp.0).unwrap().count(), 2);
}

#[test]
fn parser_spans_handle_dotted_quoted_and_inline_keys_without_snippets() {
    let temp = Temp::new();
    let file = temp.write(
        "locations.toml",
        "schema_version=1\nserver.\"listen\" = '127.0.0.1:8089'\nauth = { hash_concurrency = 3 }\n",
    );
    let settings = Settings::from_sources(&file_cli(file), &environment()).unwrap();
    assert_eq!(settings.provenance["server.listen"].line, Some(2));
    assert_eq!(settings.provenance["server.listen"].column, Some(8));
    assert_eq!(settings.provenance["auth.hash_concurrency"].line, Some(3));
    assert_eq!(
        settings.provenance["auth.hash_concurrency"].column,
        Some(10)
    );
    let file = temp.write(
        "unknown.toml",
        "schema_version=1\n[server]\nunknown = 'SECRET_CANARY'\n",
    );
    let error = invalid(&file_cli(file), &environment());
    assert!(error.contains("server.unknown") && error.contains(":3:1"));
    assert!(!error.contains("SECRET_CANARY"));
}

#[test]
fn malformed_toml_and_invalid_types_do_not_echo_values() {
    let temp = Temp::new();
    for text in [
        "schema_version=1\n[auth]\nsetup_token_file = 'SECRET_CANARY",
        "schema_version=1\n[auth]\nhash_concurrency = 'SECRET_CANARY'",
        "schema_version=1\n[auth]\nsetup_token = 'SECRET_CANARY'",
        "schema_version=1\nserver = 'SECRET_CANARY'",
    ] {
        let file = temp.write("invalid.toml", text);
        let error = invalid(&file_cli(file), &environment());
        assert!(!error.contains("SECRET_CANARY"));
        assert!(error.contains("invalid.toml:"));
    }
}

#[test]
fn typed_env_rejects_unknown_fields_bad_types_ranges_and_array_injection() {
    for (key, value, expected) in [
        ("OPENTRACK__AI__ENABLED", "false", "ai.enabled"),
        (
            "OPENTRACK__AUTH__HASH_CONCURRENCY",
            "SECRET_CANARY",
            "expected an integer",
        ),
        ("OPENTRACK__AUTH__HASH_CONCURRENCY", "0", "between 1 and 8"),
        (
            "OPENTRACK__DATABASE__API_MAX_CONNECTIONS",
            "101",
            "between 2 and 100",
        ),
        (
            "OPENTRACK__SERVER__ALLOWED_ORIGINS",
            "['https://example.test', 2]",
            "array of strings",
        ),
        (
            "OPENTRACK__SERVER__ALLOWED_ORIGINS",
            "[]\nextra='SECRET_CANARY'",
            "single TOML array",
        ),
        ("OPENTRACK__SCHEMA_VERSION", "2", "between 1 and 1"),
    ] {
        let mut values = environment();
        values.insert(key.into(), value.into());
        let error = invalid(&Cli::default(), &values);
        assert!(error.contains(expected), "{error}");
        assert!(!error.contains("SECRET_CANARY"));
    }
}

#[test]
fn legacy_aliases_remain_usable_and_typed_env_wins() {
    let mut values = environment();
    values.insert("OPENTRACK_LISTEN".into(), "127.0.0.1:8088".into());
    values.insert(
        "OPENTRACK_ALLOWED_ORIGINS".into(),
        "https://a.example, https://b.example".into(),
    );
    values.insert(
        "OPENTRACK_DATABASE_URL".into(),
        "postgres://role:SECRET_CANARY@localhost/app".into(),
    );
    values.insert(
        "OPENTRACK_MIGRATION_DATABASE_URL".into(),
        "postgres://owner:SECRET_CANARY@localhost/app".into(),
    );
    values.insert(
        "OPENTRACK_SETUP_TOKEN".into(),
        "setup-secret-canary-with-32-bytes-minimum".into(),
    );
    let settings = Settings::from_sources(&Cli::default(), &values).unwrap();
    settings.validate_secrets(Action::Serve).unwrap();
    settings.validate_secrets(Action::Migrate).unwrap();
    assert_eq!(settings.server.listen.port(), 8088);
    assert_eq!(settings.server.allowed_origins.len(), 2);
    values.insert("OPENTRACK__SERVER__LISTEN".into(), "127.0.0.1:8089".into());
    assert_eq!(
        Settings::from_sources(&Cli::default(), &values)
            .unwrap()
            .server
            .listen
            .port(),
        8089
    );
}

#[test]
fn only_explicit_files_are_loaded_and_cli_selection_wins_legacy_path() {
    let temp = Temp::new();
    let invalid_file = temp.write("opentrack.toml", "SECRET_CANARY isn't configuration");
    let valid_file = temp.write(
        "selected.toml",
        "schema_version=1\n[auth]\nhash_concurrency=4\n",
    );
    let mut values = environment();
    // The source pipeline has no working-directory search; only these explicit paths enter it.
    assert_eq!(
        Settings::from_sources(&Cli::default(), &values)
            .unwrap()
            .auth
            .hash_concurrency,
        2
    );
    values.insert(
        "OPENTRACK_CONFIG".into(),
        invalid_file.to_string_lossy().into_owned(),
    );
    assert!(invalid(&Cli::default(), &values).contains("TOML syntax"));
    assert_eq!(
        Settings::from_sources(&file_cli(valid_file), &values)
            .unwrap()
            .auth
            .hash_concurrency,
        4
    );
}

#[test]
fn production_origins_and_nonproduction_listener_are_validated() {
    for (key, value) in [
        ("OPENTRACK_PUBLIC_URL", "http://external.example"),
        (
            "OPENTRACK_PUBLIC_URL",
            "https://user:SECRET_CANARY@example.test",
        ),
        ("OPENTRACK_PUBLIC_URL", "https://example.test/path"),
        (
            "OPENTRACK_PUBLIC_URL",
            "https://example.test/?SECRET_CANARY",
        ),
        (
            "OPENTRACK_PUBLIC_URL",
            "https://example.test/#SECRET_CANARY",
        ),
        ("OPENTRACK_LISTEN", "0.0.0.0:8080"),
        ("OPENTRACK_ENVIRONMENT", "unknown"),
    ] {
        let mut values = environment();
        values.insert(key.into(), value.into());
        assert!(!invalid(&Cli::default(), &values).contains("SECRET_CANARY"));
    }
    let mut values = environment();
    values.insert("OPENTRACK_ENVIRONMENT".into(), "production".into());
    assert!(invalid(&Cli::default(), &values).contains("server.public_url"));
    values.insert("OPENTRACK_PUBLIC_URL".into(), "https://example.test".into());
    assert!(invalid(&Cli::default(), &values).contains("server.static_dir"));
    values.insert(
        "OPENTRACK_STATIC_DIR".into(),
        std::env::temp_dir()
            .join("opentrack-web-dist")
            .to_string_lossy()
            .into_owned(),
    );
    Settings::from_sources(&Cli::default(), &values).unwrap();
}

#[test]
fn migration_has_its_own_required_secrets_and_does_not_require_a_web_directory() {
    let mut values = BTreeMap::from([
        ("OPENTRACK_PUBLIC_URL".into(), "https://example.test".into()),
        (
            "OPENTRACK_MIGRATION_DATABASE_URL".into(),
            "postgres://owner:secret@localhost/app".into(),
        ),
    ]);
    let settings =
        Settings::from_sources(&cli(&["check-config", "--for", "migrate"]), &values).unwrap();
    settings.validate_secrets(Action::Migrate).unwrap();
    assert!(
        settings
            .validate_secrets(Action::Serve)
            .unwrap_err()
            .contains("database.url_file")
    );
    values.insert(
        "OPENTRACK_MIGRATION_DATABASE_URL".into(),
        "postgres://owner:SECRET_CANARY@localhost".into(),
    );
    let settings = Settings::from_sources(&cli(&["migrate"]), &values).unwrap();
    let error = settings.validate_secrets(Action::Migrate).unwrap_err();
    assert!(error.contains("PostgreSQL URL") && !error.contains("SECRET_CANARY"));
}

#[test]
fn raw_secret_and_reference_are_exclusive_even_across_precedence_layers() {
    let temp = Temp::new();
    let file = temp.write(
        "secrets.toml",
        "schema_version=1\n[auth]\nsetup_token_file='/not/read/SECRET_PATH_CANARY'\n",
    );
    let mut values = environment();
    values.insert("OPENTRACK_SETUP_TOKEN".into(), "SECRET_VALUE_CANARY".into());
    let error = invalid(&file_cli(file), &values);
    assert!(error.contains("choose one literal or auth.setup_token_file"));
    assert!(!error.contains("SECRET_VALUE_CANARY") && !error.contains("SECRET_PATH_CANARY"));
    values.insert("OPENTRACK_SETUP_TOKEN".into(), "  ".into());
    assert!(invalid(&Cli::default(), &values).contains("must not be empty"));
}

#[test]
fn effective_configuration_redacts_literal_secrets_and_preserves_provenance() {
    let mut values = environment();
    values.insert("OPENTRACK_SETUP_TOKEN".into(), "SECRET_CANARY".into());
    values.insert(
        "OPENTRACK_DATABASE_URL".into(),
        "postgres://user:SECRET_CANARY@localhost/app".into(),
    );
    let settings = Settings::from_sources(&Cli::default(), &values).unwrap();
    let printed = settings.effective_toml().unwrap();
    assert!(!printed.contains("SECRET_CANARY"));
    let result: Value = toml::from_str(&printed).unwrap();
    assert_eq!(
        result["settings"]["auth"]["setup_token"].as_str(),
        Some("<redacted>")
    );
    assert_eq!(
        result["provenance"]["auth.setup_token"]["location"].as_str(),
        Some("OPENTRACK_SETUP_TOKEN")
    );
    assert_eq!(
        result["provenance"]["auth.setup_token_file"]["source"].as_str(),
        Some("unset")
    );
    let error = settings.setup_token().unwrap_err();
    assert!(!error.contains("SECRET_CANARY"));
}

#[cfg(unix)]
#[test]
fn owner_only_secret_files_work_and_are_redacted_including_the_reference_path() {
    let temp = Temp::new();
    let secret = temp.secret(
        "SECRET_PATH_CANARY",
        "setup-secret-canary-with-32-bytes-minimum\n",
        0o600,
    );
    let mut values = environment();
    values.insert(
        "OPENTRACK_SETUP_TOKEN_FILE".into(),
        secret.to_string_lossy().into_owned(),
    );
    let settings = Settings::from_sources(&Cli::default(), &values).unwrap();
    assert_eq!(
        settings.setup_token().unwrap(),
        "setup-secret-canary-with-32-bytes-minimum"
    );
    let printed = settings.effective_toml().unwrap();
    assert!(!printed.contains("SECRET_PATH_CANARY") && !printed.contains("setup-secret-canary"));
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(secret, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(settings.setup_token().unwrap_err().contains("0400 or 0600"));
}

#[cfg(unix)]
#[test]
fn secret_references_reject_relative_symlink_executable_or_shared_files() {
    let temp = Temp::new();
    let mut values = environment();
    for mode in [0o644, 0o660, 0o000, 0o700, 0o4600] {
        let path = temp.secret(
            &format!("SECRET_PATH_CANARY-{mode:o}"),
            "SECRET_VALUE_CANARY",
            mode,
        );
        values.insert(
            "OPENTRACK_SETUP_TOKEN_FILE".into(),
            path.to_string_lossy().into_owned(),
        );
        let error = invalid(&Cli::default(), &values);
        assert!(error.contains("0400 or 0600"));
        assert!(!error.contains("SECRET_PATH_CANARY") && !error.contains("SECRET_VALUE_CANARY"));
    }
    let path = temp.secret("target", "SECRET_VALUE_CANARY", 0o400);
    let link = temp.0.join("link");
    std::os::unix::fs::symlink(path, &link).unwrap();
    values.insert(
        "OPENTRACK_SETUP_TOKEN_FILE".into(),
        link.to_string_lossy().into_owned(),
    );
    assert!(invalid(&Cli::default(), &values).contains("not a symlink"));
    values.insert(
        "OPENTRACK_SETUP_TOKEN_FILE".into(),
        "relative/secret".into(),
    );
    assert!(invalid(&Cli::default(), &values).contains("absolute"));
}

#[cfg(unix)]
#[test]
fn secret_content_has_a_size_limit_utf8_and_nonempty_requirements() {
    let temp = Temp::new();
    let path = temp.secret("secret", "", 0o600);
    assert!(read_secret_file(&path).unwrap_err().contains("empty"));
    fs::write(&path, [0xff]).unwrap();
    assert!(read_secret_file(&path).unwrap_err().contains("UTF-8"));
    fs::write(&path, vec![b'x'; 65537]).unwrap();
    assert!(read_secret_file(&path).unwrap_err().contains("64 KiB"));
}

#[cfg(unix)]
#[test]
fn opened_secret_metadata_rejects_special_files_and_directories() {
    let temp = Temp::new();
    assert!(
        validate_secret_metadata(&fs::metadata(&temp.0).unwrap())
            .unwrap_err()
            .contains("regular file")
    );
    let path = temp.0.join("pipe");
    let raw = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()).unwrap();
    // A replaced secret must be rejected even if its owner, mode and size are valid.
    assert_eq!(unsafe { libc::mkfifo(raw.as_ptr(), 0o600) }, 0);
    assert!(
        validate_secret_metadata(&fs::metadata(&path).unwrap())
            .unwrap_err()
            .contains("regular file")
    );
    assert!(
        read_secret_file(&path)
            .unwrap_err()
            .contains("regular file")
    );
}

#[test]
fn invalid_storage_root_reports_the_winning_source_without_its_value() {
    let temp = Temp::new();
    let file = temp.write(
        "storage.toml",
        "schema_version=1\n[storage]\nlocal_root='SECRET_PATH_CANARY'\n",
    );
    let mut values = environment();
    values.remove("OPENTRACK__STORAGE__LOCAL_ROOT");
    let message = invalid(&file_cli(file), &values);
    assert!(message.contains("storage.local_root"));
    assert!(message.contains("storage.toml:3:1"));
    assert!(!message.contains("SECRET_PATH_CANARY"));

    values.insert(
        "OPENTRACK__STORAGE__LOCAL_ROOT".into(),
        "SECRET_PATH_CANARY".into(),
    );
    let message = invalid(&Cli::default(), &values);
    assert!(message.contains("environment OPENTRACK__STORAGE__LOCAL_ROOT"));
    assert!(!message.contains("SECRET_PATH_CANARY"));
    values.remove("OPENTRACK__STORAGE__LOCAL_ROOT");
    assert!(invalid(&Cli::default(), &values).contains("default compiled defaults"));
}

#[test]
fn cli_accepts_explicit_commands_and_never_echoes_unknown_values() {
    assert!(cli(&["--config=explicit.toml", "print-config"]).action == Action::PrintConfig);
    assert!(cli(&["config-schema"]).action == Action::ConfigSchema);
    assert!(cli(&["--help"]).action == Action::Help);
    for args in [
        vec!["--setup-token=SECRET_CANARY"],
        vec!["serve", "migrate"],
        vec!["--for", "migrate"],
        vec!["--listen", "--public-url"],
        vec!["--listen=a", "--listen=b"],
    ] {
        let result = Cli::parse(args.into_iter().map(str::to_owned));
        match result {
            Ok(_) => panic!("invalid command line was accepted"),
            Err(error) => assert!(!error.contains("SECRET_CANARY")),
        }
    }
}

#[test]
fn operator_commands_require_explicit_target_reason_and_private_output() {
    let id = "00000000-0000-4000-8000-000000000001";
    let issue = cli(&[
        "operator-recovery-issue",
        "--user",
        id,
        "--reason",
        "Local recovery rehearsal",
        "--output-file",
        "/private/code",
    ]);
    assert_eq!(issue.action, Action::OperatorRecoveryIssue);
    assert_eq!(issue.operator_user.unwrap().to_string(), id);
    for (command, action) in [
        ("operator-account-disable", Action::OperatorAccountDisable),
        ("operator-account-enable", Action::OperatorAccountEnable),
    ] {
        let parsed = cli(&[
            command,
            "--user",
            id,
            "--reason",
            "  Account lifecycle rehearsal  ",
        ]);
        assert_eq!(parsed.action, action);
        assert!(parsed.action.is_operator());
        assert_eq!(
            parsed.operator_reason.as_deref(),
            Some("Account lifecycle rehearsal")
        );
        assert!(parsed.output_file.is_none());
        for invalid in [
            vec![command, "--user", id],
            vec![command, "--reason", "Reason"],
            vec![command, "--user", id, "--reason", "SECRET_CANARY\n"],
            vec![
                command,
                "--user",
                id,
                "--reason",
                "Reason",
                "--output-file",
                "/private/unused",
            ],
        ] {
            match Cli::parse(invalid.into_iter().map(str::to_owned)) {
                Ok(_) => panic!("invalid account operator command was accepted"),
                Err(message) => assert!(!message.contains("SECRET_CANARY")),
            }
        }
    }
    assert_eq!(
        cli(&["check-config", "--for", "operator"]).check_target,
        Action::OperatorRecoveryIssue
    );
    for args in [
        vec![
            "operator-recovery-issue",
            "--user",
            id,
            "--reason",
            "Recovery",
        ],
        vec!["operator-recovery-revoke", "--reason", "Recovery"],
        vec![
            "operator-recovery-revoke",
            "--user",
            id,
            "--reason",
            "Recovery",
            "--output-file",
            "/private/code",
        ],
        vec!["serve", "--user", id],
        vec![
            "operator-recovery-revoke",
            "--user",
            "SECRET_CANARY",
            "--reason",
            "Recovery",
        ],
        vec![
            "operator-recovery-revoke",
            "--user",
            id,
            "--reason",
            "SECRET_CANARY\nforged",
        ],
    ] {
        match Cli::parse(args.into_iter().map(str::to_owned)) {
            Ok(_) => panic!("invalid operator command was accepted"),
            Err(message) => assert!(!message.contains("SECRET_CANARY")),
        }
    }
}

#[test]
fn operator_configuration_needs_only_its_own_secret_and_bounds_lifetime() {
    let args = cli(&["check-config", "--for", "operator"]);
    let values = BTreeMap::from([(
        "OPENTRACK_OPERATOR_DATABASE_URL".into(),
        "postgres://operator:SECRET_CANARY@localhost/operator_test".into(),
    )]);
    let settings = Settings::from_sources(&args, &values).unwrap();
    settings.validate_secrets(args.check_target).unwrap();
    settings
        .validate_secrets(Action::OperatorAccountDisable)
        .unwrap();
    settings
        .validate_secrets(Action::OperatorAccountEnable)
        .unwrap();
    for action in [
        Action::OperatorConsumerStatus,
        Action::OperatorConsumerDisposition,
        Action::OperatorConsumerEnable,
        Action::OperatorConsumerDisable,
        Action::OperatorCleanupResume,
        Action::OperatorEventLimits,
    ] {
        settings.validate_secrets(action).unwrap();
    }
    assert!(settings.validate_secrets(Action::Serve).is_err());
    assert!(settings.validate_secrets(Action::Migrate).is_err());
    assert_eq!(settings.auth.recovery_ttl_minutes, 30);
    assert!(!settings.effective_toml().unwrap().contains("SECRET_CANARY"));
    for lifetime in ["4", "61", "-1", "SECRET_CANARY"] {
        let mut invalid_values = values.clone();
        invalid_values.insert(
            "OPENTRACK__AUTH__RECOVERY_TTL_MINUTES".into(),
            lifetime.into(),
        );
        let message = invalid(&args, &invalid_values);
        assert!(message.contains("auth.recovery_ttl_minutes"));
        assert!(!message.contains("SECRET_CANARY"));
    }
}

#[cfg(unix)]
#[test]
fn serving_does_not_read_operator_secret_contents() {
    let temp = Temp::new();
    let path = temp.secret("operator", "", 0o600);
    let mut values = environment();
    values.insert(
        "OPENTRACK_DATABASE_URL".into(),
        "postgres://runtime:password@localhost/app".into(),
    );
    values.insert("OPENTRACK_SETUP_TOKEN".into(), "x".repeat(32));
    values.insert(
        "OPENTRACK_OPERATOR_DATABASE_URL_FILE".into(),
        path.to_string_lossy().into(),
    );
    let settings = Settings::from_sources(&Cli::default(), &values).unwrap();
    settings.validate_secrets(Action::Serve).unwrap();
    assert!(
        settings
            .validate_secrets(Action::OperatorRecoveryIssue)
            .is_err()
    );
}

#[test]
fn generated_schema_matches_committed_file_and_rejects_future_catalog_fields() {
    let committed: serde_json::Value =
        serde_json::from_str(include_str!("../../../../config/schema.json")).unwrap();
    assert_eq!(
        json_schema(),
        committed,
        "Regenerate with cargo run -p opentrack-server -- config-schema > config/schema.json"
    );
    let schema = json_schema();
    assert_eq!(schema["additionalProperties"], false);
    assert_eq!(schema["required"], serde_json::json!(["schema_version"]));
    assert!(schema["properties"].get("ai").is_none());
    assert!(
        schema["properties"]["auth"]["properties"]
            .get("setup_token")
            .is_none()
    );
    assert_eq!(
        schema["properties"]["auth"]["properties"]["setup_token_file"]["writeOnly"],
        true
    );
}

#[test]
fn generated_reference_matches_committed_file() {
    assert_eq!(
        reference_markdown(),
        include_str!("../../../../config/REFERENCE.md").replace("\r\n", "\n"),
        "Regenerate with cargo run -p opentrack-server -- config-reference > config/REFERENCE.md"
    );
}

#[test]
fn worker_has_separate_command_secret_and_bounded_limits() {
    let values = BTreeMap::from([
        (
            "OPENTRACK__STORAGE__LOCAL_ROOT".into(),
            std::env::temp_dir()
                .join("opentrack-config-blobs")
                .to_string_lossy()
                .into_owned(),
        ),
        (
            "OPENTRACK_WORKER_DATABASE_URL".into(),
            "postgres://worker:private-canary@localhost/test".into(),
        ),
    ]);
    let args = cli(&["worker", "--once"]);
    let settings = Settings::from_sources(&args, &values).unwrap();
    settings.validate_secrets(Action::Worker).unwrap();
    assert!(settings.validate_secrets(Action::Serve).is_err());
    assert!(
        !settings
            .effective_toml()
            .unwrap()
            .contains("private-canary")
    );
    let check = cli(&["check-config", "--for", "worker"]);
    Settings::from_sources(&check, &values)
        .unwrap()
        .validate_secrets(check.check_target)
        .unwrap();
    assert!(Cli::parse(["serve", "--once"].map(str::to_owned)).is_err());
    assert!(Cli::parse(["worker", "--once=true"].map(str::to_owned)).is_err());
    assert!(
        invalid(&cli(&["worker", "--worker-lease-seconds", "9"]), &values)
            .contains("worker.lease_seconds")
    );
    assert!(
        invalid(&cli(&["worker", "--worker-poll-interval-ms", "0"]), &values)
            .contains("worker.poll_interval_ms")
    );
    assert_eq!(settings.worker.max_connections, 5);
}

#[test]
fn consumer_operator_commands_require_scoped_exact_arguments() {
    let id = "00000000-0000-4000-8000-000000000001";
    let status = cli(&["operator-consumer-status", "--workspace", id]);
    assert_eq!(status.action, Action::OperatorConsumerStatus);
    assert!(status.action.is_operator());
    assert!(status.operator_reason.is_none());
    for action in ["operator-consumer-enable", "operator-consumer-disable"] {
        let parsed = cli(&[action, "--workspace", id, "--reason", "Drain reviewed"]);
        assert!(parsed.action.is_consumer_operator());
    }
    for disposition in ["retry", "reconcile"] {
        let parsed = cli(&[
            "operator-consumer-disposition",
            "--workspace",
            id,
            "--dead-letter",
            id,
            "--disposition",
            disposition,
            "--reason",
            "Recovery reviewed",
        ]);
        assert_eq!(parsed.operator_disposition.as_deref(), Some(disposition));
        assert_eq!(parsed.operator_dead_letter.unwrap().to_string(), id);
    }
    assert_eq!(
        cli(&[
            "operator-cleanup-resume",
            "--workspace",
            id,
            "--job",
            id,
            "--reason",
            "Dependency restored"
        ])
        .action,
        Action::OperatorCleanupResume
    );
    for invalid in [
        vec!["operator-consumer-status"],
        vec!["operator-consumer-status", "--workspace", "SECRET_CANARY"],
        vec![
            "operator-consumer-status",
            "--workspace",
            id,
            "--reason",
            "Unexpected",
        ],
        vec!["operator-consumer-enable", "--workspace", id],
        vec![
            "operator-consumer-enable",
            "--workspace",
            id,
            "--reason",
            "Reason",
            "--user",
            id,
        ],
        vec![
            "operator-consumer-disable",
            "--workspace",
            id,
            "--reason",
            "Reason",
            "--job",
            id,
        ],
        vec![
            "operator-consumer-disposition",
            "--workspace",
            id,
            "--reason",
            "Reason",
        ],
        vec![
            "operator-consumer-disposition",
            "--workspace",
            id,
            "--dead-letter",
            id,
            "--disposition",
            "SECRET_CANARY",
            "--reason",
            "Reason",
        ],
        vec![
            "operator-cleanup-resume",
            "--workspace",
            id,
            "--reason",
            "Reason",
        ],
        vec![
            "operator-cleanup-resume",
            "--workspace",
            id,
            "--job",
            id,
            "--reason",
            "SECRET_CANARY\n",
        ],
        vec![
            "operator-account-enable",
            "--user",
            id,
            "--workspace",
            id,
            "--reason",
            "Reason",
        ],
        vec!["serve", "--workspace", id],
        vec!["worker", "--disposition", "retry"],
    ] {
        match Cli::parse(invalid.into_iter().map(str::to_owned)) {
            Ok(_) => panic!("invalid consumer operator command was accepted"),
            Err(message) => assert!(!message.contains("SECRET_CANARY")),
        }
    }
}

#[test]
fn event_admission_limits_require_both_positive_values_and_a_reason() {
    let base = [
        "operator-event-limits",
        "--workspace",
        "00000000-0000-4000-8000-000000000001",
        "--reason",
        "Capacity reviewed",
    ];
    let mut args = base.to_vec();
    args.extend([
        "--max-pending-events",
        "100000",
        "--max-event-bytes",
        "268435456",
    ]);
    let parsed = cli(&args);
    assert_eq!(parsed.action, Action::OperatorEventLimits);
    assert_eq!(parsed.max_pending_events, Some(100000));
    assert_eq!(parsed.max_event_bytes, Some(268435456));
    for value in ["0", "-1", "9223372036854775808", "SECRET_CANARY"] {
        let mut invalid = base.to_vec();
        invalid.extend([
            "--max-pending-events",
            value,
            "--max-event-bytes",
            "268435456",
        ]);
        let error = Cli::parse(invalid.into_iter().map(str::to_owned))
            .err()
            .unwrap();
        assert!(!error.contains("SECRET_CANARY"));
    }
    for invalid in [
        base.to_vec(),
        vec![
            "operator-event-limits",
            "--workspace",
            base[2],
            "--max-pending-events",
            "1",
            "--max-event-bytes",
            "1",
        ],
        vec![
            "operator-consumer-status",
            "--workspace",
            base[2],
            "--max-pending-events",
            "1",
        ],
        vec!["serve", "--max-event-bytes", "1"],
    ] {
        assert!(Cli::parse(invalid.into_iter().map(str::to_owned)).is_err());
    }
}

#[test]
fn attachment_storage_requires_explicit_root_and_consistent_scanner_policy() {
    let mut values = environment();
    values.remove("OPENTRACK__STORAGE__LOCAL_ROOT");
    assert!(invalid(&cli(&["serve"]), &values).contains("storage.local_root"));
    assert!(invalid(&cli(&["worker"]), &values).contains("storage.local_root"));
    // Migrations and the offline operator do not need blob access.
    assert!(Settings::from_sources(&cli(&["migrate"]), &values).is_ok());
    values.insert(
        "OPENTRACK__STORAGE__LOCAL_ROOT".into(),
        "relative/blobs".into(),
    );
    assert!(invalid(&cli(&["serve"]), &values).contains("storage.local_root"));
    values.insert(
        "OPENTRACK__STORAGE__LOCAL_ROOT".into(),
        std::env::temp_dir()
            .join("opentrack-config-blobs")
            .to_string_lossy()
            .into_owned(),
    );
    values.insert(
        "OPENTRACK__STORAGE__ANTIVIRUS_REQUIRED".into(),
        "true".into(),
    );
    assert!(invalid(&cli(&["serve"]), &values).contains("requires storage.scanner_socket"));
    values.insert(
        "OPENTRACK__STORAGE__SCANNER_SOCKET".into(),
        std::env::temp_dir()
            .join("clamd.ctl")
            .to_string_lossy()
            .into_owned(),
    );
    assert!(
        Settings::from_sources(&cli(&["serve"]), &values)
            .unwrap()
            .storage
            .antivirus_required
    );
    values.insert(
        "OPENTRACK__STORAGE__MAX_UPLOAD_BYTES".into(),
        "104857601".into(),
    );
    assert!(invalid(&cli(&["serve"]), &values).contains("storage.max_upload_bytes"));
    values.remove("OPENTRACK__STORAGE__MAX_UPLOAD_BYTES");
    values.insert(
        "OPENTRACK__STORAGE__ANTIVIRUS_REQUIRED".into(),
        "yes".into(),
    );
    assert!(invalid(&cli(&["serve"]), &values).contains("expected true or false"));
}

#[test]
fn local_container_listener_is_an_explicit_development_only_opt_in() {
    let mut values = environment();
    values.insert("OPENTRACK_ENVIRONMENT".into(), "development".into());
    values.insert("OPENTRACK_LISTEN".into(), "0.0.0.0:8080".into());
    assert!(invalid(&Cli::default(), &values).contains("loopback"));
    values.insert("OPENTRACK__SERVER__LOCAL_CONTAINER".into(), "true".into());
    assert!(
        Settings::from_sources(&Cli::default(), &values)
            .unwrap()
            .server
            .local_container
    );
    values.insert("OPENTRACK_LISTEN".into(), "[::]:8080".into());
    assert!(Settings::from_sources(&Cli::default(), &values).is_ok());
    values.insert("OPENTRACK_LISTEN".into(), "192.168.1.2:8080".into());
    assert!(invalid(&Cli::default(), &values).contains("loopback"));
    values.insert("OPENTRACK_LISTEN".into(), "0.0.0.0:8080".into());
    for mode in ["production", "test"] {
        values.insert("OPENTRACK_ENVIRONMENT".into(), mode.into());
        assert!(invalid(&Cli::default(), &values).contains("server.local_container"));
    }
}

#[test]
fn local_container_rejects_remote_https_and_extra_origins() {
    let mut values = environment();
    values.insert("OPENTRACK_ENVIRONMENT".into(), "development".into());
    values.insert("OPENTRACK__SERVER__LOCAL_CONTAINER".into(), "true".into());
    values.insert("OPENTRACK_LISTEN".into(), "0.0.0.0:8080".into());
    for origin in [
        "http://localhost:18970",
        "http://127.0.0.1:18970",
        "http://[::1]:18970",
    ] {
        values.insert("OPENTRACK_PUBLIC_URL".into(), origin.into());
        assert!(Settings::from_sources(&Cli::default(), &values).is_ok());
    }
    for origin in [
        "https://localhost",
        "https://tracker.example",
        "http://tracker.example",
        "http://localhost.evil.example",
        "http://127.0.0.1:18970/path",
        "http://127.0.0.1:18970?secret=x",
    ] {
        values.insert("OPENTRACK_PUBLIC_URL".into(), origin.into());
        assert!(invalid(&Cli::default(), &values).contains("server.public_url"));
    }
    values.insert(
        "OPENTRACK_PUBLIC_URL".into(),
        "http://localhost:18970".into(),
    );
    values.insert(
        "OPENTRACK__SERVER__ALLOWED_ORIGINS".into(),
        "[\"http://localhost:18970\",\"https://tracker.example\"]".into(),
    );
    assert!(invalid(&Cli::default(), &values).contains("server.allowed_origins"));
    values.insert(
        "OPENTRACK__SERVER__ALLOWED_ORIGINS".into(),
        "[\"http://localhost:18970\",\"http://127.0.0.1:18970\"]".into(),
    );
    assert!(Settings::from_sources(&Cli::default(), &values).is_ok());
}
#[test]
fn planning_rebuild_requires_workspace_and_reason_without_unrelated_targets() {
    let id = "11111111-1111-4111-8111-111111111111";
    let command = cli(&[
        "operator-planning-rebuild",
        "--workspace",
        id,
        "--reason",
        "Reconcile current planning facts",
    ]);
    assert_eq!(command.action, Action::OperatorPlanningRebuild);
    assert!(command.action.is_consumer_operator());
    assert!(command.action.is_operator());
    for args in [
        vec!["operator-planning-rebuild", "--workspace", id],
        vec!["operator-planning-rebuild", "--reason", "Reconcile"],
        vec![
            "operator-planning-rebuild",
            "--workspace",
            id,
            "--reason",
            "Reconcile",
            "--job",
            id,
        ],
    ] {
        assert!(Cli::parse(args.into_iter().map(str::to_owned)).is_err());
    }
}
