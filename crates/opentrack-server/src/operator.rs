//! Local operator authority is separate from the serving process and its credentials.
use crate::config::{Action, Cli, Settings};
use opentrack_app::{Operator, generate_recovery_code};
use std::path::Path;
#[cfg(unix)]
use std::{fs, io::Write};
use uuid::Uuid;

pub async fn run(cli: &Cli, cfg: &Settings) -> Result<(), String> {
    // Validate the restricted database identity before creating any bearer material.
    let operator = Operator::connect(&cfg.operator_database_url()?)
        .await
        .map_err(|_| "Cannot connect with a correctly restricted operator role; verify migrations and operator grants (connection details redacted)")?;
    if !operator
        .schema_is_current()
        .await
        .map_err(|_| "Cannot verify operator migration history; apply matching operator grants")?
    {
        return Err("Database migration history is incompatible with this executable".into());
    }
    if cli.action.is_consumer_operator() {
        return run_consumer(cli, &operator).await;
    }
    let user = cli
        .operator_user
        .ok_or("Operator command requires --user")?;
    let reason = cli
        .operator_reason
        .as_deref()
        .ok_or("Operator command requires --reason")?;
    if matches!(
        cli.action,
        Action::OperatorAccountDisable | Action::OperatorAccountEnable
    ) {
        let result = if cli.action == Action::OperatorAccountDisable {
            operator.disable_account(user, reason).await
        } else {
            operator.enable_account(user, reason).await
        };
        let receipt = result.map_err(|error| match error {
            opentrack_domain::Error::NotFound => "Account not found; verify the account UUID".to_string(),
            opentrack_domain::Error::Conflict(_) => "Account was not disabled: keep another enabled owner in every workspace and organization, including archived workspaces".to_string(),
            opentrack_domain::Error::Invalid(_) => "Invalid account lifecycle request; verify the account UUID and a reason of 1–500 characters without control characters".to_string(),
            opentrack_domain::Error::Forbidden => "Operator capability was denied; verify the restricted operator grants".to_string(),
            _ => "Account state change could not be confirmed. Retry the same command to confirm the desired state; no credential values were printed".to_string(),
        })?;
        println!("{}", serde_json::to_string(&receipt).map_err(|_| "Account state changed, but its receipt could not be encoded; retry the same command to confirm the desired state")?);
        return Ok(());
    }
    if cli.action == Action::OperatorRecoveryRevoke {
        let count = operator.revoke_recovery(user, reason).await
            .map_err(|_| "Recovery revocation could not be confirmed; retry the command before assuming any pending code is revoked")?;
        println!(
            "{}",
            serde_json::json!({"user_id":user,"revoked_count":count})
        );
        return Ok(());
    }
    let output = cli
        .output_file
        .as_deref()
        .ok_or("Issuance requires --output-file")?;
    let receipt = Uuid::new_v4();
    let code = generate_recovery_code(user);
    // Persist privately first: a file-write failure must never activate a lost code.
    write_code(output, &code)?;
    let issued = operator.issue_recovery(user, receipt, &code, cfg.auth.recovery_ttl_minutes, reason).await
        .map_err(|_| format!("Recovery issuance could not be confirmed (receipt {receipt}). The private output file is preserved. Do not assume its code is usable. Reissue to a new file to invalidate any pending code, or explicitly revoke recovery for this account."))?;
    println!(
        "{}",
        serde_json::json!({"id":issued.id,"user_id":issued.user_id,"expires_at":issued.expires_at,"output_file":output})
    );
    Ok(())
}

async fn run_consumer(cli: &Cli, operator: &Operator) -> Result<(), String> {
    let workspace = cli
        .operator_workspace
        .ok_or("Consumer operator command requires --workspace")?;
    let status = cli.action == Action::OperatorConsumerStatus;
    let result = if status {
        operator.consumer_status(workspace).await
    } else {
        let reason = cli
            .operator_reason
            .as_deref()
            .ok_or("Consumer changes require --reason")?;
        let (action, target, result) = match cli.action {
            Action::OperatorEventLimits => (
                "event_limits",
                None,
                operator
                    .set_event_admission_limits(
                        workspace,
                        cli.max_pending_events
                            .ok_or("Event limits require --max-pending-events")?,
                        cli.max_event_bytes
                            .ok_or("Event limits require --max-event-bytes")?,
                        reason,
                    )
                    .await,
            ),
            Action::OperatorConsumerDisposition => {
                let target = cli
                    .operator_dead_letter
                    .ok_or("Disposition requires --dead-letter")?;
                let action = cli
                    .operator_disposition
                    .as_deref()
                    .ok_or("Disposition requires --disposition")?;
                (
                    action,
                    Some(target),
                    operator
                        .consumer_disposition(workspace, target, action, reason)
                        .await,
                )
            }
            Action::OperatorConsumerEnable | Action::OperatorConsumerDisable => {
                let enabled = cli.action == Action::OperatorConsumerEnable;
                (
                    if enabled { "enable" } else { "disable" },
                    None,
                    operator
                        .set_consumer_enabled(workspace, enabled, reason)
                        .await,
                )
            }
            Action::OperatorPlanningRebuild => (
                "planning_rebuild",
                None,
                operator.rebuild_planning_counts(workspace, reason).await,
            ),
            Action::OperatorCleanupResume => {
                let target = cli.operator_job.ok_or("Cleanup resume requires --job")?;
                (
                    "resume_cleanup",
                    Some(target),
                    operator
                        .resume_assignment_cleanup(workspace, target, reason)
                        .await,
                )
            }
            _ => return Err("Unsupported consumer operator command".into()),
        };
        result.map(|correlation| serde_json::json!({"workspace_id":workspace,"target_id":target,"action":action,"correlation_id":correlation}))
    };
    let receipt = result.map_err(|error| match error {
        opentrack_domain::Error::NotFound => "Consumer, dead letter or job was not found in this workspace".to_owned(),
        opentrack_domain::Error::Invalid(_) | opentrack_domain::Error::Conflict(_) => "Consumer change was refused. Inspect consumer status and verify the target, disposition and drain state".to_owned(),
        opentrack_domain::Error::Forbidden => "Operator capability was denied; apply the matching migrations and restricted operator grants".to_owned(),
        _ if status => "Consumer status is unavailable; no state change was requested".to_owned(),
        _ => "Consumer change could not be confirmed. Inspect consumer status before retrying; no credential values were printed".to_owned(),
    })?;
    println!(
        "{}",
        serde_json::to_string(&receipt)
            .map_err(|_| "Consumer receipt could not be encoded; inspect consumer status")?
    );
    Ok(())
}

#[cfg(unix)]
fn write_code(path: &Path, code: &str) -> Result<(), String> {
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
    use std::path::Component;
    if !path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
    {
        return Err("Recovery output must be an absolute path without relative components".into());
    }
    let parent = path
        .parent()
        .ok_or("Recovery output needs a parent directory")?;
    let name = path
        .file_name()
        .ok_or("Recovery output needs a file name")?;
    // SAFETY: geteuid has no arguments, memory effects, or failure mode.
    let uid = unsafe { libc::geteuid() };
    let mut directory = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open("/")
        .map_err(|_| "Cannot open recovery output root")?;
    // Pin every directory before traversing onward. A different UID must not be
    // able to rename an ancestor after validation. Root-owned sticky directories
    // (e.g. /tmp) safely contain our own private, non-writable child directories.
    for part in parent.components() {
        let Component::Normal(component) = part else {
            continue;
        };
        directory = open_child(&directory, component,
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC, 0)
            .map_err(|_| "Recovery output ancestors must be accessible real directories, never symbolic links")?;
        let meta = directory
            .metadata()
            .map_err(|_| "Cannot inspect recovery output directory")?;
        let mode = meta.permissions().mode();
        let trusted_sticky = meta.uid() == 0 && mode & libc::S_ISVTX != 0;
        if !meta.is_dir()
            || (meta.uid() != 0 && meta.uid() != uid)
            || (mode & 0o022 != 0 && !trusted_sticky)
        {
            return Err("Recovery output ancestors must be owned by this user or root and protected from other users' writes".into());
        }
    }
    let meta = directory
        .metadata()
        .map_err(|_| "Cannot inspect recovery output directory")?;
    if meta.uid() != uid || meta.permissions().mode() & 0o777 != 0o700 {
        return Err(
            "Recovery output directory must be owned by the current user with mode 0700".into(),
        );
    }
    // Create relative to the validated handle. O_EXCL and O_NOFOLLOW reject an
    // existing file or final symlink without ever opening its target.
    let mut file = open_child(
        &directory,
        name,
        libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        0o600,
    )
    .map_err(|_| "Cannot create recovery output; it must be a new private file")?;
    let persisted = (|| -> std::io::Result<()> {
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
        writeln!(file, "{code}")?;
        file.sync_all()?;
        directory.sync_all()?;
        Ok(())
    })();
    persisted.map_err(|_| "Cannot persist recovery output; no code was issued. Remove the incomplete private file before retrying.".into())
}

#[cfg(unix)]
fn open_child(
    directory: &fs::File,
    name: &std::ffi::OsStr,
    flags: libc::c_int,
    mode: libc::mode_t,
) -> std::io::Result<fs::File> {
    use std::os::{
        fd::{AsRawFd, FromRawFd},
        unix::ffi::OsStrExt,
    };
    let name =
        std::ffi::CString::new(name.as_bytes()).map_err(|_| std::io::ErrorKind::InvalidInput)?;
    // SAFETY: directory remains alive; name is NUL-terminated; the mode argument
    // is supplied when O_CREAT requires it. Successful descriptors are owned.
    let fd = unsafe { libc::openat(directory.as_raw_fd(), name.as_ptr(), flags, mode) };
    if fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: openat returned a new owned descriptor, wrapped exactly once.
    Ok(unsafe { fs::File::from_raw_fd(fd) })
}

#[cfg(not(unix))]
fn write_code(_: &Path, _: &str) -> Result<(), String> {
    Err("Private recovery output is currently supported only on Unix".into())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn output_is_private_and_never_replaces_files_or_follows_symlinks() {
        let dir = std::env::temp_dir().join(format!("opentrack-recovery-{}", Uuid::new_v4()));
        fs::create_dir(&dir).unwrap();
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
        let path = dir.join("code");
        write_code(&path, "private-test-code").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "private-test-code\n");
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert!(write_code(&path, "replacement").is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "private-test-code\n");
        let link = dir.join("link");
        std::os::unix::fs::symlink(&path, &link).unwrap();
        assert!(write_code(&link, "replacement").is_err());
        assert!(write_code(Path::new("relative-output"), "code").is_err());
        assert!(write_code(&dir.join("../escaped"), "code").is_err());
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(write_code(&dir.join("readable-directory"), "code").is_err());
        assert!(!dir.join("readable-directory").exists());
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o777)).unwrap();
        assert!(write_code(&dir.join("unsafe"), "code").is_err());
        fs::remove_dir_all(dir).unwrap();
    }
}
