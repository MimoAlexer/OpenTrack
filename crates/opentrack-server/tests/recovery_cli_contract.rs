//! Real operator CLI calls use a restricted third PostgreSQL identity and fresh schema.
//! Code files live beneath a private /tmp directory because repository ancestors
//! may be group-writable and correctly fail the production output-path policy.
#![cfg(unix)]
#[allow(dead_code)]
#[path = "../../opentrack-app/tests/support/mod.rs"]
mod support;

use opentrack_store as store;
use serde_json::{Value, json};
use std::{
    fs,
    io::Write,
    os::unix::fs::{OpenOptionsExt, PermissionsExt, symlink},
    path::{Path, PathBuf},
    process::{Command, Output},
};
use support::Harness;
use uuid::Uuid;

const PASSWORD: &str = "CLI recovery contract password 415!";
const REASON: &str = "Identity verified in person";

struct PrivateDirectory(PathBuf);
impl PrivateDirectory {
    fn new() -> Self {
        let path = Path::new("/tmp").join(format!("opentrack-recovery-cli-{}", Uuid::new_v4()));
        fs::DirBuilder::new().create(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        Self(path)
    }
    fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}
impl Drop for PrivateDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

async fn invoke(
    directory: &Path,
    credentials: &str,
    credential_file: bool,
    action: &str,
    user: Uuid,
    output_file: Option<&Path>,
) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_opentrack-server"));
    command
        .env_clear()
        .env("OPENTRACK_ENVIRONMENT", "test")
        .env("RUST_LOG", "error")
        .env(
            if credential_file {
                "OPENTRACK_OPERATOR_DATABASE_URL_FILE"
            } else {
                "OPENTRACK_OPERATOR_DATABASE_URL"
            },
            credentials,
        )
        .current_dir(directory)
        .args([action, "--user", &user.to_string(), "--reason", REASON]);
    if let Some(path) = output_file {
        command.arg("--output-file").arg(path);
    }
    tokio::task::spawn_blocking(move || command.output().expect("Run the recovery CLI"))
        .await
        .unwrap()
}

fn assert_redacted(output: &Output, url: &str, codes: &[&str]) {
    let text = output_text(output);
    assert!(!text.contains(url), "Connection URL must not be printed");
    let parsed = url::Url::parse(url).unwrap();
    if let Some(password) = parsed.password() {
        assert!(
            !text.contains(password),
            "Database password must not be printed"
        );
    }
    assert!(
        !text.contains(PASSWORD),
        "Account password must not be printed"
    );
    assert!(
        !text.contains("otrec1."),
        "No recovery code may enter command output"
    );
    assert!(!text.contains(REASON), "The operator reason is audit-only");
    for code in codes {
        assert!(!text.contains(code), "Recovery secret must not be printed");
    }
}
fn output_text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}
fn receipt(output: &Output) -> Value {
    assert!(
        output.status.success(),
        "CLI expected success; details withheld to avoid leaking credentials"
    );
    serde_json::from_slice(&output.stdout).expect("Success prints one JSON receipt")
}
fn assert_failed_without_receipt(output: &Output) {
    assert!(!output.status.success(), "CLI must reject the operation");
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        if let Ok(value) = serde_json::from_str::<Value>(line) {
            assert!(
                value.get("expires_at").is_none()
                    && value.get("correlation_id").is_none()
                    && value.get("revoked_count").is_none(),
                "A failure must not print a successful receipt"
            );
        }
    }
}
async fn recovery_state(h: &Harness, user: Uuid) -> (Vec<Uuid>, i64) {
    let mut tx = store::begin(&h.owner).await.unwrap();
    store::context(&mut tx, user, None).await.unwrap();
    let pending = sqlx::query_scalar("SELECT id FROM recovery_tokens WHERE user_id=$1 AND used_at IS NULL AND revoked_at IS NULL ORDER BY id")
        .bind(user).fetch_all(&mut *tx).await.unwrap();
    let events = sqlx::query_scalar("SELECT count(*) FROM account_security_events WHERE user_id=$1 AND action LIKE 'recovery.%'")
        .bind(user).fetch_one(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    (pending, events)
}
async fn invalid_code(h: &Harness, code: &str) {
    let result = h
        .app
        .complete_recovery(
            serde_json::from_value(json!({
                "token": code, "password": "An unused replacement passphrase 415!"
            }))
            .unwrap(),
        )
        .await;
    assert!(
        result.is_err(),
        "An inactive code must not reset the account"
    );
    assert_eq!(
        result.unwrap_err().to_string(),
        "This recovery code is invalid or no longer available"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_cli_operator_recovery_contract() {
    let harness = Harness::new().await;
    let runner = harness.clone();
    let outcome = tokio::spawn(async move { exercise(&runner).await }).await;
    harness.cleanup().await;
    if let Err(error) = outcome {
        std::panic::resume_unwind(error.into_panic());
    }
}

async fn exercise(h: &Harness) {
    let directory = PrivateDirectory::new();
    let operator_url = h.operator_url().await;
    let (session, _) = h.app.setup(serde_json::from_value(json!({
        "setup_token": h.setup_token, "email":"recovery-cli@contract.invalid", "password":PASSWORD,
        "display_name":"CLI recovery owner", "workspace_name":"CLI recovery", "team_name":"Engineering", "team_key":"ENG"
    })).unwrap()).await.unwrap();
    let user = session.user.id;
    let first_path = directory.join("first-code");
    let issued = invoke(
        &directory.0,
        &operator_url,
        false,
        "operator-recovery-issue",
        user,
        Some(&first_path),
    )
    .await;
    assert_redacted(&issued, &operator_url, &[]);
    let first_receipt = receipt(&issued);
    let first_id = Uuid::parse_str(first_receipt["id"].as_str().unwrap()).unwrap();
    assert_eq!(first_receipt["user_id"], user.to_string());
    assert_eq!(first_receipt["output_file"], first_path.to_str().unwrap());
    assert!(first_receipt["expires_at"].is_string());
    let first_file = fs::read_to_string(&first_path).unwrap();
    let first_code = first_file.trim();
    assert!(first_code.starts_with(&format!("otrec1.{user}.")));
    assert_eq!(first_code.len(), 108);
    assert_eq!(
        fs::metadata(&first_path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_redacted(&issued, &operator_url, &[first_code]);
    let state = recovery_state(h, user).await;
    assert_eq!(state, (vec![first_id], 1));

    // Every path refusal occurs before issuance, preserving the previous code.
    let link = directory.join("file-link");
    symlink(&first_path, &link).unwrap();
    let linked_parent = directory.join("linked-parent");
    symlink(&directory.0, &linked_parent).unwrap();
    let unsafe_parent = directory.join("unsafe-parent");
    fs::create_dir(&unsafe_parent).unwrap();
    fs::set_permissions(&unsafe_parent, fs::Permissions::from_mode(0o770)).unwrap();
    let unsafe_intermediate = directory.join("unsafe-intermediate");
    fs::create_dir(&unsafe_intermediate).unwrap();
    fs::set_permissions(&unsafe_intermediate, fs::Permissions::from_mode(0o777)).unwrap();
    let private_child = unsafe_intermediate.join("private-child");
    fs::create_dir(&private_child).unwrap();
    fs::set_permissions(&private_child, fs::Permissions::from_mode(0o700)).unwrap();
    let real_parent = directory.join("real-parent");
    fs::create_dir(&real_parent).unwrap();
    fs::set_permissions(&real_parent, fs::Permissions::from_mode(0o700)).unwrap();
    let child = real_parent.join("child");
    fs::create_dir(&child).unwrap();
    fs::set_permissions(&child, fs::Permissions::from_mode(0o700)).unwrap();
    let linked_ancestor = directory.join("linked-ancestor");
    symlink(&real_parent, &linked_ancestor).unwrap();
    for path in [
        first_path.clone(),
        link,
        PathBuf::from("relative-code"),
        linked_parent.join("not-created"),
        unsafe_parent.join("not-created"),
        private_child.join("not-created"),
        linked_ancestor.join("child/not-created"),
        directory.join("../escape-code"),
    ] {
        let output = invoke(
            &directory.0,
            &operator_url,
            false,
            "operator-recovery-issue",
            user,
            Some(&path),
        )
        .await;
        assert_redacted(&output, &operator_url, &[first_code]);
        assert_failed_without_receipt(&output);
        assert_eq!(recovery_state(h, user).await, state);
        assert!(
            fs::read_to_string(&first_path).unwrap() == first_file,
            "An existing code file must never be replaced"
        );
    }
    for path in [
        directory.join("relative-code"),
        directory.join("not-created"),
        unsafe_parent.join("not-created"),
        private_child.join("not-created"),
        child.join("not-created"),
    ] {
        assert!(!path.exists(), "Rejected paths must not create files");
    }

    // A normal runtime credential fails authority preflight before any code file exists.
    let schema: String = sqlx::query_scalar("SELECT current_schema()")
        .fetch_one(&h.owner)
        .await
        .unwrap();
    let mut runtime_url =
        url::Url::parse(&std::env::var("OPENTRACK_TEST_DATABASE_URL").unwrap()).unwrap();
    runtime_url
        .query_pairs_mut()
        .append_pair("options", &format!("-csearch_path={schema}"));
    let denied_path = directory.join("runtime-must-not-create");
    let denied = invoke(
        &directory.0,
        runtime_url.as_str(),
        false,
        "operator-recovery-issue",
        user,
        Some(&denied_path),
    )
    .await;
    assert_redacted(&denied, runtime_url.as_str(), &[first_code]);
    assert_failed_without_receipt(&denied);
    assert!(!denied_path.exists());
    assert_eq!(recovery_state(h, user).await, state);

    // A mandatory audit failure rolls back issuance, but the prewritten file is
    // retained privately with an explicit uncertain result and no success receipt.
    sqlx::raw_sql("CREATE FUNCTION reject_cli_issue() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.action='recovery.issued' THEN RAISE EXCEPTION 'Contract audit rejection' USING ERRCODE='42501'; END IF; RETURN NEW; END $$; CREATE TRIGGER zz_contract_reject_issue BEFORE INSERT ON account_security_events FOR EACH ROW EXECUTE FUNCTION reject_cli_issue();")
        .execute(&h.owner).await.unwrap();
    let failed_path = directory.join("unconfirmed-code");
    let failed = invoke(
        &directory.0,
        &operator_url,
        false,
        "operator-recovery-issue",
        user,
        Some(&failed_path),
    )
    .await;
    let failed_file = fs::read_to_string(&failed_path).unwrap();
    let failed_code = failed_file.trim();
    assert_redacted(&failed, &operator_url, &[first_code, failed_code]);
    assert_failed_without_receipt(&failed);
    let message = format!(
        "{}{}",
        String::from_utf8_lossy(&failed.stdout),
        String::from_utf8_lossy(&failed.stderr)
    );
    assert!(message.contains("could not be confirmed"));
    assert!(message.contains("private output file is preserved"));
    assert_eq!(
        fs::metadata(&failed_path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(recovery_state(h, user).await, state);
    invalid_code(h, failed_code).await;
    sqlx::raw_sql("DROP TRIGGER zz_contract_reject_issue ON account_security_events; DROP FUNCTION reject_cli_issue();")
        .execute(&h.owner).await.unwrap();

    // Returning NULL is not a SQL error: the mandatory-count check must reject
    // this omission and leave both the old credential and the private file intact.
    sqlx::raw_sql("CREATE FUNCTION suppress_cli_issue() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.action='recovery.issued' THEN RETURN NULL; END IF; RETURN NEW; END $$; CREATE TRIGGER zz_contract_suppress_issue BEFORE INSERT ON account_security_events FOR EACH ROW EXECUTE FUNCTION suppress_cli_issue();")
        .execute(&h.owner).await.unwrap();
    let suppressed_path = directory.join("silently-unconfirmed-code");
    let suppressed = invoke(
        &directory.0,
        &operator_url,
        false,
        "operator-recovery-issue",
        user,
        Some(&suppressed_path),
    )
    .await;
    let suppressed_file = fs::read_to_string(&suppressed_path).unwrap();
    let suppressed_code = suppressed_file.trim();
    assert_redacted(&suppressed, &operator_url, &[first_code, suppressed_code]);
    assert_failed_without_receipt(&suppressed);
    assert!(output_text(&suppressed).contains("could not be confirmed"));
    assert!(output_text(&suppressed).contains("private output file is preserved"));
    assert_eq!(suppressed_file.len(), 109);
    assert!(suppressed_file.ends_with('\n'));
    assert!(suppressed_code.starts_with(&format!("otrec1.{user}.")));
    assert_eq!(
        fs::metadata(&suppressed_path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(recovery_state(h, user).await, state);
    invalid_code(h, suppressed_code).await;
    sqlx::raw_sql("DROP TRIGGER zz_contract_suppress_issue ON account_security_events; DROP FUNCTION suppress_cli_issue();")
        .execute(&h.owner).await.unwrap();
    // Even after the fault is removed, retrying an existing output path cannot
    // replace its secret or issue a new database credential.
    let retained = invoke(
        &directory.0,
        &operator_url,
        false,
        "operator-recovery-issue",
        user,
        Some(&suppressed_path),
    )
    .await;
    assert_redacted(&retained, &operator_url, &[first_code, suppressed_code]);
    assert_failed_without_receipt(&retained);
    assert!(fs::read_to_string(&suppressed_path).unwrap() == suppressed_file);
    assert_eq!(recovery_state(h, user).await, state);

    // A replacement supersedes the first code and a credential-file invocation
    // revokes the replacement without receiving either code in command arguments.
    let second_path = directory.join("second-code");
    let second = invoke(
        &directory.0,
        &operator_url,
        false,
        "operator-recovery-issue",
        user,
        Some(&second_path),
    )
    .await;
    assert_redacted(&second, &operator_url, &[first_code, failed_code]);
    let second_receipt = receipt(&second);
    let second_id = Uuid::parse_str(second_receipt["id"].as_str().unwrap()).unwrap();
    assert_ne!(first_id, second_id);
    assert_eq!(recovery_state(h, user).await, (vec![second_id], 2));
    invalid_code(h, first_code).await;
    let second_file = fs::read_to_string(&second_path).unwrap();
    let second_code = second_file.trim();
    let credentials_path = directory.join("operator-credentials");
    let mut credentials = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&credentials_path)
        .unwrap();
    writeln!(credentials, "{operator_url}").unwrap();
    credentials.sync_all().unwrap();
    sqlx::raw_sql("CREATE FUNCTION suppress_cli_revoke() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.action='recovery.revoked' THEN RETURN NULL; END IF; RETURN NEW; END $$; CREATE TRIGGER zz_contract_suppress_revoke BEFORE INSERT ON account_security_events FOR EACH ROW EXECUTE FUNCTION suppress_cli_revoke();")
        .execute(&h.owner).await.unwrap();
    let revoke_failed = invoke(
        &directory.0,
        credentials_path.to_str().unwrap(),
        true,
        "operator-recovery-revoke",
        user,
        None,
    )
    .await;
    assert_redacted(&revoke_failed, &operator_url, &[first_code, second_code]);
    assert_failed_without_receipt(&revoke_failed);
    assert!(output_text(&revoke_failed).contains("could not be confirmed"));
    assert_eq!(recovery_state(h, user).await, (vec![second_id], 2));
    assert!(fs::read_to_string(&second_path).unwrap() == second_file);
    sqlx::raw_sql("DROP TRIGGER zz_contract_suppress_revoke ON account_security_events; DROP FUNCTION suppress_cli_revoke();")
        .execute(&h.owner).await.unwrap();
    let revoked = invoke(
        &directory.0,
        credentials_path.to_str().unwrap(),
        true,
        "operator-recovery-revoke",
        user,
        None,
    )
    .await;
    assert_redacted(
        &revoked,
        &operator_url,
        &[first_code, failed_code, second_code],
    );
    let revoked_receipt = receipt(&revoked);
    assert_eq!(revoked_receipt["revoked_count"], 1);
    assert_eq!(recovery_state(h, user).await, (vec![], 3));
    invalid_code(h, second_code).await;
    // A zero-candidate request is still an audited operator action. Missing its
    // audit must not become a misleading successful revoked_count:0 receipt.
    sqlx::raw_sql("CREATE FUNCTION suppress_cli_revoke() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.action='recovery.revoked' THEN RETURN NULL; END IF; RETURN NEW; END $$; CREATE TRIGGER zz_contract_suppress_revoke BEFORE INSERT ON account_security_events FOR EACH ROW EXECUTE FUNCTION suppress_cli_revoke();")
        .execute(&h.owner).await.unwrap();
    let noop_failed = invoke(
        &directory.0,
        &operator_url,
        false,
        "operator-recovery-revoke",
        user,
        None,
    )
    .await;
    assert_redacted(&noop_failed, &operator_url, &[first_code, second_code]);
    assert_failed_without_receipt(&noop_failed);
    assert!(output_text(&noop_failed).contains("could not be confirmed"));
    assert_eq!(recovery_state(h, user).await, (vec![], 3));
    sqlx::raw_sql("DROP TRIGGER zz_contract_suppress_revoke ON account_security_events; DROP FUNCTION suppress_cli_revoke();")
        .execute(&h.owner).await.unwrap();
    let repeat = invoke(
        &directory.0,
        &operator_url,
        false,
        "operator-recovery-revoke",
        user,
        None,
    )
    .await;
    assert_redacted(
        &repeat,
        &operator_url,
        &[first_code, failed_code, second_code],
    );
    assert_eq!(receipt(&repeat)["revoked_count"], 0);
    assert_eq!(recovery_state(h, user).await, (vec![], 4));
}

fn lifecycle_receipt(output: &Output, user: Uuid, disabled: bool, changed: bool) -> Value {
    let value = receipt(output);
    let keys: Vec<_> = value
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        keys,
        vec![
            "changed",
            "correlation_id",
            "disabled",
            "disabled_at",
            "revoked_invitations",
            "revoked_recovery_codes",
            "revoked_sessions",
            "user_id"
        ]
    );
    assert_eq!(value["user_id"], user.to_string());
    assert_eq!(value["disabled"], disabled);
    assert_eq!(value["changed"], changed);
    Uuid::parse_str(value["correlation_id"].as_str().unwrap()).unwrap();
    assert_eq!(value["disabled_at"].is_string(), disabled);
    if !disabled {
        assert!(value["disabled_at"].is_null());
    }
    for field in [
        "revoked_sessions",
        "revoked_recovery_codes",
        "revoked_invitations",
    ] {
        assert!(value[field].as_i64().unwrap() >= 0);
    }
    value
}

async fn lifecycle_state(h: &Harness, user: Uuid, workspace: Uuid) -> Value {
    let mut tx = h.context(user, workspace).await;
    let value = sqlx::query_scalar("SELECT jsonb_build_object('disabled_at',u.disabled_at,'sessions',(SELECT count(*) FROM sessions WHERE user_id=u.id AND revoked_at IS NOT NULL),'codes',(SELECT count(*) FROM recovery_tokens WHERE user_id=u.id AND revoked_at IS NOT NULL),'invitations',(SELECT count(*) FROM invitations WHERE workspace_id=$2 AND invited_by=u.id AND revoked_at IS NOT NULL),'account_audit',(SELECT count(*) FROM account_security_events WHERE user_id=u.id),'workspace_audit',(SELECT count(*) FROM audit_entries WHERE workspace_id=$2),'events',(SELECT count(*) FROM workspace_events WHERE workspace_id=$2),'epoch',(SELECT epoch FROM workspace_auth_guards WHERE workspace_id=$2)) FROM users u WHERE u.id=$1")
        .bind(user).bind(workspace).fetch_one(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    value
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_cli_operator_account_lifecycle_contract() {
    let harness = Harness::new().await;
    let runner = harness.clone();
    let outcome = tokio::spawn(async move { exercise_lifecycle(&runner).await }).await;
    harness.cleanup().await;
    if let Err(error) = outcome {
        std::panic::resume_unwind(error.into_panic());
    }
}

async fn exercise_lifecycle(h: &Harness) {
    let directory = PrivateDirectory::new();
    let operator_url = h.operator_url().await;
    let (session, owner_cookie) = h.app.setup(serde_json::from_value(json!({
        "setup_token":h.setup_token,"email":"lifecycle-owner@contract.invalid","password":PASSWORD,
        "display_name":"CLI lifecycle owner","workspace_name":"CLI lifecycle","team_name":"Engineering","team_key":"ENG"
    })).unwrap()).await.unwrap();
    let owner = h.app.authenticate(&owner_cookie).await.unwrap();
    let workspace = session.workspaces[0].id;

    // Last enabled owner refusal is actionable and has no durable side effects.
    let before = lifecycle_state(h, owner.user.id, workspace).await;
    let refused = invoke(
        &directory.0,
        &operator_url,
        false,
        "operator-account-disable",
        owner.user.id,
        None,
    )
    .await;
    assert_redacted(&refused, &operator_url, &[]);
    assert_failed_without_receipt(&refused);
    assert!(output_text(&refused).contains("enabled owner"));
    assert_eq!(lifecycle_state(h, owner.user.id, workspace).await, before);
    assert!(h.app.authenticate(&owner_cookie).await.is_ok());

    let issued = h
        .app
        .create_invitation(
            &owner,
            workspace,
            &Uuid::new_v4().to_string(),
            serde_json::from_value(
                json!({"email":"lifecycle-target@contract.invalid","role":"admin","team_ids":[]}),
            )
            .unwrap(),
        )
        .await
        .unwrap();
    let (target_session, first_cookie) = h.app.accept_invitation(None,
        serde_json::from_value(json!({"token":issued.token.unwrap(),"display_name":"Lifecycle target","password":PASSWORD})).unwrap()
    ).await.unwrap();
    let target = target_session.user.id;
    let (_, second_cookie) = h
        .app
        .login(
            serde_json::from_value(json!({"email":target_session.user.email,"password":PASSWORD}))
                .unwrap(),
        )
        .await
        .unwrap();
    let actor = h.app.authenticate(&first_cookie).await.unwrap();
    let outgoing = h
        .app
        .create_invitation(
            &actor,
            workspace,
            &Uuid::new_v4().to_string(),
            serde_json::from_value(
                json!({"email":"outgoing@contract.invalid","role":"member","team_ids":[]}),
            )
            .unwrap(),
        )
        .await
        .unwrap();
    let outgoing_code = outgoing.token.unwrap();
    let code_path = directory.join("target-recovery");
    let issued = invoke(
        &directory.0,
        &operator_url,
        false,
        "operator-recovery-issue",
        target,
        Some(&code_path),
    )
    .await;
    receipt(&issued);
    let code_file = fs::read_to_string(&code_path).unwrap();
    let code = code_file.trim();
    assert_redacted(&issued, &operator_url, &[code, &outgoing_code]);

    // These state-changing commands never accept a bearer output path.
    let before = lifecycle_state(h, target, workspace).await;
    let forbidden_output = directory.join("lifecycle-must-not-create");
    for action in ["operator-account-disable", "operator-account-enable"] {
        let output = invoke(
            &directory.0,
            &operator_url,
            false,
            action,
            target,
            Some(&forbidden_output),
        )
        .await;
        assert_redacted(&output, &operator_url, &[code, &outgoing_code]);
        assert_failed_without_receipt(&output);
        assert!(!forbidden_output.exists());
        assert_eq!(lifecycle_state(h, target, workspace).await, before);
    }
    let schema: String = sqlx::query_scalar("SELECT current_schema()")
        .fetch_one(&h.owner)
        .await
        .unwrap();
    let mut runtime_url =
        url::Url::parse(&std::env::var("OPENTRACK_TEST_DATABASE_URL").unwrap()).unwrap();
    runtime_url
        .query_pairs_mut()
        .append_pair("options", &format!("-csearch_path={schema}"));
    for action in ["operator-account-disable", "operator-account-enable"] {
        let output = invoke(
            &directory.0,
            runtime_url.as_str(),
            false,
            action,
            target,
            None,
        )
        .await;
        assert_redacted(&output, runtime_url.as_str(), &[code, &outgoing_code]);
        assert_failed_without_receipt(&output);
        assert_eq!(lifecycle_state(h, target, workspace).await, before);
    }
    let missing = invoke(
        &directory.0,
        &operator_url,
        false,
        "operator-account-disable",
        Uuid::new_v4(),
        None,
    )
    .await;
    assert_redacted(&missing, &operator_url, &[code, &outgoing_code]);
    assert_failed_without_receipt(&missing);
    assert!(output_text(&missing).contains("Account not found"));
    assert_eq!(lifecycle_state(h, target, workspace).await, before);

    // A required audit failure must roll back user state and every revocation.
    sqlx::raw_sql("CREATE FUNCTION reject_cli_lifecycle() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.action='account.disabled' THEN RAISE EXCEPTION 'Contract audit rejection' USING ERRCODE='P0001'; END IF; RETURN NEW; END $$; CREATE TRIGGER zz_contract_reject_lifecycle BEFORE INSERT ON account_security_events FOR EACH ROW EXECUTE FUNCTION reject_cli_lifecycle();")
        .execute(&h.owner).await.unwrap();
    let failed = invoke(
        &directory.0,
        &operator_url,
        false,
        "operator-account-disable",
        target,
        None,
    )
    .await;
    assert_redacted(&failed, &operator_url, &[code, &outgoing_code]);
    assert_failed_without_receipt(&failed);
    let error = output_text(&failed);
    assert!(error.contains("could not be confirmed"));
    assert!(error.contains("Retry the same command"));
    assert_eq!(lifecycle_state(h, target, workspace).await, before);
    assert!(h.app.authenticate(&first_cookie).await.is_ok());
    assert!(h.app.invitation_preview(&outgoing_code).await.is_ok());
    sqlx::raw_sql("DROP TRIGGER zz_contract_reject_lifecycle ON account_security_events; DROP FUNCTION reject_cli_lifecycle();")
        .execute(&h.owner).await.unwrap();

    let disabled = invoke(
        &directory.0,
        &operator_url,
        false,
        "operator-account-disable",
        target,
        None,
    )
    .await;
    assert_redacted(&disabled, &operator_url, &[code, &outgoing_code]);
    let disabled_receipt = lifecycle_receipt(&disabled, target, true, true);
    assert_eq!(disabled_receipt["revoked_sessions"], 2);
    assert_eq!(disabled_receipt["revoked_recovery_codes"], 1);
    assert_eq!(disabled_receipt["revoked_invitations"], 1);
    for cookie in [&first_cookie, &second_cookie] {
        assert!(h.app.authenticate(cookie).await.is_err());
    }
    assert!(h.app.session(&actor).await.is_err());
    assert!(
        h.app
            .login(
                serde_json::from_value(
                    json!({"email":target_session.user.email,"password":PASSWORD})
                )
                .unwrap()
            )
            .await
            .is_err()
    );
    assert!(h.app.authenticate(&owner_cookie).await.is_ok());
    invalid_code(h, code).await;
    assert!(h.app.invitation_preview(&outgoing_code).await.is_err());

    let repeat = invoke(
        &directory.0,
        &operator_url,
        false,
        "operator-account-disable",
        target,
        None,
    )
    .await;
    assert_redacted(&repeat, &operator_url, &[code, &outgoing_code]);
    let repeat_receipt = lifecycle_receipt(&repeat, target, true, false);
    assert_eq!(
        repeat_receipt["disabled_at"],
        disabled_receipt["disabled_at"]
    );
    assert_ne!(
        repeat_receipt["correlation_id"],
        disabled_receipt["correlation_id"]
    );
    for field in [
        "revoked_sessions",
        "revoked_recovery_codes",
        "revoked_invitations",
    ] {
        assert_eq!(repeat_receipt[field], 0);
    }

    let enabled = invoke(
        &directory.0,
        &operator_url,
        false,
        "operator-account-enable",
        target,
        None,
    )
    .await;
    assert_redacted(&enabled, &operator_url, &[code, &outgoing_code]);
    let enabled_receipt = lifecycle_receipt(&enabled, target, false, true);
    let repeat = invoke(
        &directory.0,
        &operator_url,
        false,
        "operator-account-enable",
        target,
        None,
    )
    .await;
    assert_redacted(&repeat, &operator_url, &[code, &outgoing_code]);
    let repeat_receipt = lifecycle_receipt(&repeat, target, false, false);
    assert_ne!(
        repeat_receipt["correlation_id"],
        enabled_receipt["correlation_id"]
    );
    for field in [
        "revoked_sessions",
        "revoked_recovery_codes",
        "revoked_invitations",
    ] {
        assert_eq!(enabled_receipt[field], 0);
        assert_eq!(repeat_receipt[field], 0);
    }
    for cookie in [&first_cookie, &second_cookie] {
        assert!(
            h.app.authenticate(cookie).await.is_err(),
            "Enable never revives old cookies"
        );
    }
    invalid_code(h, code).await;
    assert!(
        h.app.invitation_preview(&outgoing_code).await.is_err(),
        "Enable never revives outgoing invitations"
    );
    let (_, fresh_cookie) = h
        .app
        .login(
            serde_json::from_value(json!({"email":target_session.user.email,"password":PASSWORD}))
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(h.app.authenticate(&fresh_cookie).await.is_ok());
    assert!(h.app.authenticate(&owner_cookie).await.is_ok());

    let mut tx = h.context(target, workspace).await;
    let actions: Vec<String> = sqlx::query_scalar("SELECT action FROM account_security_events WHERE user_id=$1 AND action LIKE 'account.%' ORDER BY created_at,id")
        .bind(target).fetch_all(&mut *tx).await.unwrap();
    assert_eq!(
        actions,
        vec![
            "account.disabled",
            "account.disable_noop",
            "account.enabled",
            "account.enable_noop"
        ]
    );
    let operator_identity = url::Url::parse(&operator_url)
        .unwrap()
        .username()
        .to_owned();
    let audited: i64 = sqlx::query_scalar("SELECT count(*) FROM account_security_events WHERE user_id=$1 AND action LIKE 'account.%' AND actor_kind='local_operator' AND operator_identity=$2 AND reason=$3")
        .bind(target).bind(operator_identity).bind(REASON).fetch_one(&mut *tx).await.unwrap();
    assert_eq!(
        audited, 4,
        "Every invocation records the real operator and reason"
    );
    tx.commit().await.unwrap();

    // Before this migration, disabled_at could be set without revoking bearer
    // credentials. Enabling such a retained account must not revive that state.
    let fresh_actor = h.app.authenticate(&fresh_cookie).await.unwrap();
    let legacy_invite = h
        .app
        .create_invitation(
            &fresh_actor,
            workspace,
            &Uuid::new_v4().to_string(),
            serde_json::from_value(
                json!({"email":"legacy-outgoing@contract.invalid","role":"member","team_ids":[]}),
            )
            .unwrap(),
        )
        .await
        .unwrap()
        .token
        .unwrap();
    let legacy_path = directory.join("legacy-recovery");
    let issued = invoke(
        &directory.0,
        &operator_url,
        false,
        "operator-recovery-issue",
        target,
        Some(&legacy_path),
    )
    .await;
    receipt(&issued);
    let legacy_file = fs::read_to_string(&legacy_path).unwrap();
    let legacy_code = legacy_file.trim();
    assert_redacted(&issued, &operator_url, &[legacy_code, &legacy_invite]);
    let mut tx = h.context(target, workspace).await;
    // Migration-owner fixture only: normal runtime credentials cannot perform
    // this update. The enabled owner of this workspace remains untouched.
    sqlx::query("UPDATE users SET disabled_at=clock_timestamp() WHERE id=$1")
        .bind(target)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert!(h.app.authenticate(&fresh_cookie).await.is_err());
    let enabled = invoke(
        &directory.0,
        &operator_url,
        false,
        "operator-account-enable",
        target,
        None,
    )
    .await;
    assert_redacted(&enabled, &operator_url, &[legacy_code, &legacy_invite]);
    let enabled = lifecycle_receipt(&enabled, target, false, true);
    assert_eq!(enabled["revoked_sessions"], 1);
    assert_eq!(enabled["revoked_recovery_codes"], 1);
    assert_eq!(enabled["revoked_invitations"], 1);
    assert!(
        h.app.authenticate(&fresh_cookie).await.is_err(),
        "Legacy cookies stay invalid after enable"
    );
    invalid_code(h, legacy_code).await;
    assert!(
        h.app.invitation_preview(&legacy_invite).await.is_err(),
        "Legacy outgoing invitations stay invalid after enable"
    );
}
