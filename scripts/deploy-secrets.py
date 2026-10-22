#!/usr/bin/env python3
"""Provision Compose secrets, validate them, or add a separate capability login."""
import argparse
import os
from pathlib import Path
import secrets
import stat
import sys
from urllib.parse import urlsplit

ROOT = Path(__file__).resolve().parent.parent
APP_SECRETS = ("database-url", "migration-database-url", "setup-token")
DB_SECRETS = ("postgres-password", "runtime-password", "migration-password")
OPERATOR_SECRETS = ("operator-password", "operator-database-url")
WORKER_SECRETS = ("worker-password", "worker-database-url")


def fail(message):
    sys.exit(message)


def owned_file(path, uid, allowed_modes):
    try:
        metadata = path.lstat()
    except FileNotFoundError:
        fail(f"Missing {path}; no existing credentials were changed.")
    if (
        not stat.S_ISREG(metadata.st_mode)
        or metadata.st_uid != uid
        or stat.S_IMODE(metadata.st_mode) not in allowed_modes
    ):
        modes = "/".join(f"0{mode:o}" for mode in sorted(allowed_modes))
        fail(f"{path.name} must be a regular, non-symlink file owned by UID {uid} with mode {modes}; no files changed.")


def environment(destination, uid, gid):
    # Docker Compose dotenv single-quoted strings are literal. Reject ambiguous
    # newline/quote paths rather than risking an incorrect bind source.
    if any(char in str(destination) for char in "\n\r'"):
        fail("Choose a secret directory path without quotes or line breaks.")
    return (
        f"OPENTRACK_SERVICE_UID={uid}\nOPENTRACK_SERVICE_GID={gid}\n"
        f"OPENTRACK_SECRETS_DIR='{destination}'\n"
    )


def check_base(destination, uid, gid):
    metadata = destination.lstat()
    if not stat.S_ISDIR(metadata.st_mode) or metadata.st_uid != uid or stat.S_IMODE(metadata.st_mode) != 0o700:
        fail("Secret directory must be a real directory owned by the service UID with mode 0700.")
    for name in APP_SECRETS:
        owned_file(destination / name, uid, {0o400, 0o600})
    # These three files are used only by PostgreSQL's different container UID.
    # Their host parent remains 0700; they are never mounted into the app.
    for name in DB_SECRETS:
        owned_file(destination / name, uid, {0o444})
    env_path = destination / "compose.env"
    owned_file(env_path, uid, {0o400, 0o600})
    if env_path.read_text() != environment(destination, uid, gid):
        fail("compose.env must match the provisioned service UID/GID and secret directory; no files changed.")


def check_pair(destination, uid, kind):
    owned_file(destination / f"{kind}-password", uid, {0o444})
    owned_file(destination / f"{kind}-database-url", uid, {0o400, 0o600})
    try:
        password = (destination / f"{kind}-password").read_text().strip()
        url = urlsplit((destination / f"{kind}-database-url").read_text().strip())
        matches = (
            len(password) == 64
            and all(char in "0123456789abcdef" for char in password)
            and url.scheme == "postgres"
            and url.username == f"opentrack_{kind}"
            and url.password == password
            and url.hostname == "database"
            and url.port == 5432
            and url.path == "/opentrack"
            and not url.query
            and not url.fragment
        )
    except (OSError, UnicodeError, ValueError):
        matches = False
    if not matches:
        fail(f"{kind.capitalize()} password and URL must be a matching generated pair; no existing credentials were changed.")


def check(destination, uid, gid):
    check_base(destination, uid, gid)
    check_pair(destination, uid, "operator")
    check_pair(destination, uid, "worker")
    print(f"Validated secret permissions and nonroot service UID/GID {uid}:{gid}; no values printed.")


def create_pair(destination, kind):
    password = secrets.token_hex(32)
    values = {
        f"{kind}-password": password,
        f"{kind}-database-url": f"postgres://opentrack_{kind}:{password}@database:5432/opentrack",
    }
    created = []
    try:
        for name, value in values.items():
            path = destination / name
            with path.open("x") as file:
                created.append(path)
                file.write(value + "\n")
                file.flush()
                os.fsync(file.fileno())
            path.chmod(0o444 if name.endswith("-password") else 0o600)
    except OSError:
        # Only remove files this invocation just created, never a preexisting one.
        for path in created:
            path.unlink()
        fail(f"Could not create the {kind} credential pair; existing credentials were not changed.")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--directory", type=Path, default=ROOT / "deploy" / "secrets")
    action = parser.add_mutually_exclusive_group()
    action.add_argument("--check", action="store_true", help="validate only; never change existing files")
    action.add_argument("--add-operator", action="store_true", help="add only the operator pair to a valid older directory; preserve every existing file")
    action.add_argument("--add-worker", action="store_true", help="add only the worker pair to a valid older directory; preserve every existing file")
    args = parser.parse_args()
    uid, gid = os.getuid(), os.getgid()
    if uid == 0 or gid == 0:
        fail("Run this helper as the intended nonroot service account, with a nonroot group.")
    destination = args.directory.absolute()
    if destination.is_symlink():
        fail("The secret directory must not be a symbolic link.")
    env_content = environment(destination, uid, gid)
    if args.add_operator or args.add_worker:
        kind = "operator" if args.add_operator else "worker"
        if not destination.exists():
            fail(f"Existing secret directory required for --add-{kind}; no files changed.")
        check_base(destination, uid, gid)
        # Validate any existing capability pair before creating another. Missing
        # pairs are allowed during an explicit upgrade from older versions.
        for other in ("operator", "worker"):
            found = [os.path.lexists(destination / f"{other}-{suffix}") for suffix in ("password", "database-url")]
            if any(found) and not all(found):
                fail(f"Partial {other} credentials found; reconcile them explicitly. No files changed.")
            if all(found):
                check_pair(destination, uid, other)
        pair = OPERATOR_SECRETS if kind == "operator" else WORKER_SECRETS
        present = [os.path.lexists(destination / name) for name in pair]
        if any(present) and not all(present):
            fail(f"Partial {kind} credentials found; reconcile them explicitly. No files changed.")
        if all(present):
            check_pair(destination, uid, kind)
            print(f"{kind.capitalize()} credentials already match; no files changed.")
            return
        os.umask(0o077)
        create_pair(destination, kind)
        check_pair(destination, uid, kind)
        print(f"Added only the {kind} password and URL. All existing files and credential values were preserved; provision the separate database role explicitly.")
        return
    if args.check:
        if not destination.exists():
            fail("Secret directory does not exist; no files changed.")
        check(destination, uid, gid)
        return
    if destination.exists():
        fail("Secret directory already exists; refusing to change or replace existing credentials. Use --check for validation.")
    os.umask(0o077)
    destination.mkdir(mode=0o700)
    runtime, migration = secrets.token_hex(32), secrets.token_hex(32)
    values = {
        "postgres-password": secrets.token_hex(32),
        "runtime-password": runtime,
        "migration-password": migration,
        "database-url": f"postgres://opentrack:{runtime}@database:5432/opentrack",
        "migration-database-url": f"postgres://opentrack_migrator:{migration}@database:5432/opentrack",
        "setup-token": secrets.token_hex(32),
    }
    for name, value in values.items():
        path = destination / name
        with path.open("x") as file:
            file.write(value + "\n")
        path.chmod(0o444 if name in DB_SECRETS else 0o600)
    with (destination / "compose.env").open("x") as file:
        file.write(env_content)
    create_pair(destination, "operator")
    create_pair(destination, "worker")
    check(destination, uid, gid)
    print(f"Created ten secret files and compose.env in {destination}; existing credentials were untouched.")


if __name__ == "__main__":
    main()
