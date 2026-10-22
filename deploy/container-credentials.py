#!/usr/bin/env python3
"""Generate and validate role-separated named-volume secrets inside Docker only."""
import argparse
from contextlib import ExitStack
import fcntl
import hashlib
import json
import os
from pathlib import Path
import secrets
import stat
import sys

SERVICE_UID = 10001
DATABASE_UID = 999
MARKER = ".opentrack-secrets.json"
INCOMPLETE = ".initializing"
LOCK = ".bootstrap.lock"
FILES = {
    "database": ("postgres-password", "runtime-password", "migration-password", "operator-password", "worker-password"),
    "app": ("database-url", "setup-token"),
    "migration": ("migration-database-url",),
    "worker": ("worker-database-url",),
    "operator": ("operator-database-url",),
}


class UnsafeState(Exception):
    """Messages contain operational facts only, never file contents."""


def require(condition, message):
    if not condition:
        raise UnsafeState(message)


def owner(kind):
    return DATABASE_UID if kind == "database" else SERVICE_UID


class Directory:
    def __init__(self, path):
        self.fd = os.open(path, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC)

    def __enter__(self):
        return self

    def __exit__(self, *_):
        os.close(self.fd)

    def entries(self):
        return set(os.listdir(self.fd))

    def prepare(self, uid):
        # Called only after all volumes have been checked empty and the durable
        # partial-initialization marker exists. Existing installations are never
        # repaired by changing their ownership or permissions automatically.
        os.fchown(self.fd, uid, uid)
        os.fchmod(self.fd, 0o700)
        os.fsync(self.fd)

    def check(self, uid):
        info = os.fstat(self.fd)
        require(info.st_uid == uid and info.st_gid == uid and stat.S_IMODE(info.st_mode) == 0o700,
                "A secret/storage volume has unsafe ownership or permissions; no credentials changed.")

    def read(self, name, uid, maximum=4096):
        fd = os.open(name, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=self.fd)
        try:
            info = os.fstat(fd)
            require(stat.S_ISREG(info.st_mode) and info.st_nlink == 1
                    and info.st_uid == uid and info.st_gid == uid and stat.S_IMODE(info.st_mode) == 0o600,
                    "A secret or state file is linked, shared, or incorrectly owned; no credentials changed.")
            require(info.st_size <= maximum, "An unexpected secret/state file size requires explicit reconciliation.")
            with os.fdopen(fd, "rb", closefd=False) as source:
                data = source.read(maximum + 1)
            require(len(data) <= maximum, "A secret/state file exceeded its size limit.")
            return data
        finally:
            os.close(fd)

    def write_new(self, name, data, uid):
        fd = os.open(name, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC,
                     0o600, dir_fd=self.fd)
        try:
            with os.fdopen(fd, "wb", closefd=False) as output:
                output.write(data)
                output.flush()
                os.fchown(fd, uid, uid)
                os.fsync(fd)
        finally:
            os.close(fd)
        os.fsync(self.fd)

    def lock(self, create=True):
        fd = os.open(LOCK, os.O_RDWR | (os.O_CREAT if create else 0) | os.O_NOFOLLOW | os.O_CLOEXEC,
                     0o600, dir_fd=self.fd)
        try:
            info = os.fstat(fd)
            require(stat.S_ISREG(info.st_mode) and info.st_nlink == 1 and info.st_uid == 0
                    and info.st_gid == 0 and stat.S_IMODE(info.st_mode) == 0o600,
                    "Unsafe bootstrap lock; initialization refused.")
            fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
            os.fsync(fd)
            os.fsync(self.fd)
            return os.fdopen(fd, "rb")
        except BaseException:
            os.close(fd)
            raise


def url(role, password):
    return f"postgres://{role}:{password}@database:5432/opentrack\n".encode()


def generated_values():
    passwords = {name: secrets.token_hex(32) for name in FILES["database"]}
    result = {("database", name): (value + "\n").encode() for name, value in passwords.items()}
    result.update({
        ("app", "database-url"): url("opentrack", passwords["runtime-password"]),
        ("app", "setup-token"): (secrets.token_hex(32) + "\n").encode(),
        ("migration", "migration-database-url"): url("opentrack_migrator", passwords["migration-password"]),
        ("worker", "worker-database-url"): url("opentrack_worker", passwords["worker-password"]),
        ("operator", "operator-database-url"): url("opentrack_operator", passwords["operator-password"]),
    })
    return result


def check_values(values):
    passwords = {}
    for name in FILES["database"]:
        value = values[("database", name)]
        require(len(value) == 65 and value[-1:] == b"\n"
                and all(byte in b"0123456789abcdef" for byte in value[:-1]),
                "A generated database password is malformed; no credentials changed.")
        passwords[name] = value[:-1].decode("ascii")
    token = values[("app", "setup-token")]
    require(len(token) == 65 and token[-1:] == b"\n"
            and all(byte in b"0123456789abcdef" for byte in token[:-1]),
            "The owner setup token is malformed; no credentials changed.")
    for kind, filename, role, password in [
        ("app", "database-url", "opentrack", "runtime-password"),
        ("migration", "migration-database-url", "opentrack_migrator", "migration-password"),
        ("worker", "worker-database-url", "opentrack_worker", "worker-password"),
        ("operator", "operator-database-url", "opentrack_operator", "operator-password"),
    ]:
        require(values[(kind, filename)] == url(role, passwords[password]),
                "Secret volumes contain mismatched database credentials; no credentials changed.")


def validate(directories, blobs):
    require(INCOMPLETE not in directories["database"].entries(),
            "Interrupted initialization requires explicit reconciliation; no credentials changed.")
    manifest = json.loads(directories["database"].read(MARKER, DATABASE_UID, 16384))
    require(set(manifest) == {"schema", "service_uid", "database_uid", "files"}
            and manifest["schema"] == 1 and manifest["service_uid"] == SERVICE_UID
            and manifest["database_uid"] == DATABASE_UID,
            "Unsupported bootstrap marker; no credentials changed.")
    values = {}
    for kind, names in FILES.items():
        directory = directories[kind]
        directory.check(owner(kind))
        expected = set(names) | ({MARKER, LOCK} if kind == "database" else set())
        require(directory.entries() == expected, "Missing or unexpected files in a secret volume; no credentials changed.")
        for name in names:
            values[(kind, name)] = directory.read(name, owner(kind))
    hashes = {kind + "/" + name: hashlib.sha256(value).hexdigest() for (kind, name), value in values.items()}
    require(manifest["files"] == hashes, "A secret volume differs from the completed installation; no credentials changed.")
    check_values(values)
    blobs.check(SERVICE_UID)
    require(blobs.entries() == {"blobs"}, "Unexpected entries in the blob volume require explicit reconciliation.")
    with Directory(f"/proc/self/fd/{blobs.fd}/blobs") as child:
        child.check(SERVICE_UID)


def initialize(root, blob_root, database_state, check_only=False):
    require(os.getuid() == 0, "Named-volume bootstrap/check must run as the initializer's root user.")
    with ExitStack() as stack:
        directories = {kind: stack.enter_context(Directory(root / kind)) for kind in FILES}
        blobs = stack.enter_context(Directory(blob_root))
        existing_database = stack.enter_context(Directory(database_state))
        # One persistent lock serializes init attempts across containers. Its
        # descriptor and every opened volume remain pinned for this invocation.
        stack.enter_context(directories["database"].lock(create=not check_only))
        if MARKER in directories["database"].entries():
            validate(directories, blobs)
            print("Existing role-separated secrets and blob permissions verified; all credentials preserved.")
            return
        require(not check_only, "No completed bootstrap marker exists; validation refused.")
        require(not existing_database.entries(),
                "Existing PostgreSQL data has no matching secret marker; restore its original secret volumes. No credentials generated.")
        for kind, directory in directories.items():
            require(directory.entries() == ({LOCK} if kind == "database" else set()),
                    "Partial or nonempty uninitialized secret volumes require explicit reconciliation. No credentials generated.")
            info = os.fstat(directory.fd)
            require(info.st_uid == 0 and info.st_gid == 0,
                    "Uninitialized secret volumes must be empty root-owned named volumes; no ownership repaired.")
        require(not blobs.entries(), "Existing blob data has no matching secret marker; no credentials generated.")
        info = os.fstat(blobs.fd)
        require(info.st_uid == 0 and info.st_gid == 0,
                "An uninitialized blob volume must be empty and root-owned; no ownership repaired.")
        directories["database"].write_new(INCOMPLETE, b"OpenTrack bootstrap schema 1\n", 0)
        for kind, directory in directories.items():
            directory.prepare(owner(kind))
        blobs.prepare(SERVICE_UID)
        os.mkdir("blobs", mode=0o700, dir_fd=blobs.fd)
        with Directory(f"/proc/self/fd/{blobs.fd}/blobs") as child:
            child.prepare(SERVICE_UID)
        os.fsync(blobs.fd)
        values = generated_values()
        for (kind, name), value in values.items():
            directories[kind].write_new(name, value, owner(kind))
        marker = {"schema": 1, "service_uid": SERVICE_UID, "database_uid": DATABASE_UID,
                  "files": {kind + "/" + name: hashlib.sha256(value).hexdigest() for (kind, name), value in values.items()}}
        # The sentinel is removed only after all durable files exist. A crash
        # before the final marker leaves nonempty volumes and is refused, never
        # repaired by rotating credentials or resetting PostgreSQL passwords.
        os.unlink(INCOMPLETE, dir_fd=directories["database"].fd)
        os.fsync(directories["database"].fd)
        directories["database"].write_new(MARKER, (json.dumps(marker, sort_keys=True) + "\n").encode(), DATABASE_UID)
        validate(directories, blobs)
        print("Created role-separated private secrets and blob storage; no credential values printed.")


def setup_token(path):
    require(os.getuid() == SERVICE_UID, "Read the setup token as the fixed application UID only.")
    with Directory(path) as directory:
        directory.check(SERVICE_UID)
        token = directory.read("setup-token", SERVICE_UID)
    require(len(token) == 65 and token[-1:] == b"\n" and all(byte in b"0123456789abcdef" for byte in token[:-1]),
            "Invalid owner setup token; no value printed.")
    # This explicit user-run command is the sole secret-printing operation.
    sys.stdout.buffer.write(token)


def prepare_output(path):
    require(os.getuid() == 0, "Operator-output initialization must run as its dedicated root helper.")
    with Directory(path) as directory:
        info = os.fstat(directory.fd)
        if info.st_uid == SERVICE_UID and info.st_gid == SERVICE_UID and stat.S_IMODE(info.st_mode) == 0o700:
            print("Existing private operator-output volume preserved.")
            return
        require(info.st_uid == 0 and info.st_gid == 0 and not directory.entries(),
                "Operator-output volume is nonempty or has unexpected ownership/permissions; no files changed.")
        directory.prepare(SERVICE_UID)
        directory.check(SERVICE_UID)
        print("Prepared a private operator-output volume for UID 10001.")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=["initialize", "check", "setup-token", "prepare-output"])
    parser.add_argument("--root", type=Path, default=Path("/bootstrap"))
    parser.add_argument("--blob-root", type=Path, default=Path("/var/lib/opentrack"))
    parser.add_argument("--database-state", type=Path, default=Path("/database-state"))
    parser.add_argument("--app-secrets", type=Path, default=Path("/run/secrets"))
    parser.add_argument("--directory", type=Path, default=Path("/operator-output"))
    args = parser.parse_args()
    try:
        if args.action == "setup-token":
            setup_token(args.app_secrets)
        elif args.action == "prepare-output":
            prepare_output(args.directory)
        else:
            initialize(args.root, args.blob_root, args.database_state, args.action == "check")
    except (UnsafeState, OSError, ValueError, KeyError, TypeError) as error:
        message = str(error) if isinstance(error, UnsafeState) else "Bootstrap filesystem/state validation failed; preserve all volumes and reconcile explicitly. No credential values printed."
        print(message, file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
