#!/usr/bin/env bash
set -Eeuo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."
if [[ -f work/shared/goal-coordination/with-lock.sh && "${OPENTRACK_STORAGE_QUALIFICATION_LOCKED:-}" != 1 ]]; then
  exec bash work/shared/goal-coordination/with-lock.sh verification env OPENTRACK_STORAGE_QUALIFICATION_LOCKED=1 bash scripts/qualify-attachment-storage.sh
fi
exec python3 - <<'PY'
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
from urllib.parse import urlsplit
sys.path.insert(0, "scripts")
from qualification_process import run_owned

def interrupted(_signal, _frame):
    raise SystemExit("Storage qualification interrupted; no acceptance recorded")

signal.signal(signal.SIGTERM, interrupted)

if sys.platform != "linux":
    raise SystemExit("Storage qualification requires Linux user and mount namespaces")
names = ["OPENTRACK_TEST_DATABASE_URL", "OPENTRACK_TEST_MIGRATION_DATABASE_URL",
         "OPENTRACK_TEST_OPERATOR_DATABASE_URL", "OPENTRACK_TEST_WORKER_DATABASE_URL"]
try:
    databases = [urlsplit(os.environ[name]) for name in names]
    valid = (all(url.scheme in {"postgres", "postgresql"} and url.hostname in {"localhost", "127.0.0.1", "::1"}
                 and url.path.endswith("_test") and url.username for url in databases)
             and len({(url.hostname, url.port, url.path) for url in databases}) == 1
             and len({url.username for url in databases}) == 4)
except (KeyError, ValueError):
    valid = False
if not valid:
    raise SystemExit("Storage qualification requires four distinct roles on one loopback isolated *_test database")

executables = []
for package, target in (("opentrack-adapters", "full_disk_qualification"),
                        ("opentrack-api", "attachment_storage_qualification")):
    try:
        build = run_owned(["cargo", "test", "--locked", "-p", package, "--test", target,
                                "--no-run", "--message-format=json"], capture_output=True, text=True, timeout=600)
    except subprocess.TimeoutExpired:
        raise SystemExit("Storage qualification compilation exceeded its deadline") from None
    if build.returncode:
        raise SystemExit("Storage qualification compilation failed; inspect the locked Cargo build")
    artifacts = [json.loads(line) for line in build.stdout.splitlines() if line.startswith("{")]
    matches = [item["executable"] for item in artifacts
               if item.get("reason") == "compiler-artifact" and item.get("executable")
               and item["target"]["name"] == target and item["profile"]["test"]]
    if len(matches) != 1:
        raise SystemExit("Expected exactly one compiled storage qualification executable")
    executables.append(matches[0])

with tempfile.TemporaryDirectory(prefix="opentrack-storage-refusal-", dir="/tmp") as directory:
    for executable in executables:
        for marker in (None, os.readlink("/proc/self/ns/mnt")):
            environment = {**os.environ, "OPENTRACK_TEST_FULL_DISK_ROOT": directory}
            environment.pop("OPENTRACK_QUALIFICATION_PARENT_MOUNT", None)
            if marker is not None:
                environment["OPENTRACK_QUALIFICATION_PARENT_MOUNT"] = marker
            result = run_owned([executable, "--ignored", "--test-threads=1"],
                                    env=environment, capture_output=True, timeout=20)
            if not result.returncode or b"A distinct launcher mount namespace is required" not in result.stdout + result.stderr:
                raise SystemExit("Direct invocation did not reject the missing or unchanged namespace marker")
            if list(Path(directory).iterdir()):
                raise SystemExit("Rejected direct invocation changed its target")
print("Both test executables reject missing and unchanged namespace markers before writing")

program = r'''
import os
from pathlib import Path
import subprocess
import sys

if os.readlink("/proc/self/ns/mnt") == os.environ["OPENTRACK_QUALIFICATION_PARENT_MOUNT"]:
    raise SystemExit("A private mount namespace is required")
subprocess.run(["mount", "--make-rprivate", "/"], check=True, capture_output=True)
subprocess.run(["mount", "-t", "tmpfs", "-o", "size=16m,mode=0700,nosuid,nodev,noexec",
                "tmpfs", "/tmp"], check=True, capture_output=True)
try:
    base = Path(sys.argv[1])
    base.mkdir(mode=0o700)
    runtime = base / "runtime"
    runtime.mkdir(mode=0o700)
    for index, executable in enumerate(sys.argv[2:]):
        target = base / str(index)
        target.mkdir(mode=0o700)
        subprocess.run(["mount", "-t", "tmpfs", "-o", "size=4m,mode=0700,nosuid,nodev,noexec",
                        "tmpfs", str(target)], check=True, capture_output=True)
        try:
            filesystem = subprocess.check_output(["findmnt", "-n", "-o", "FSTYPE", "--mountpoint", str(target)],
                                                 text=True).strip()
            capacity = os.statvfs(target).f_blocks * os.statvfs(target).f_frsize
            if filesystem != "tmpfs" or capacity != 4 * 1024 * 1024:
                raise SystemExit("Storage fixture is not a private 4 MiB tmpfs")
            environment = {**os.environ, "OPENTRACK_TEST_FULL_DISK_ROOT": str(target), "TMPDIR": str(runtime)}
            sentinel = target / "preserve"
            sentinel.write_bytes(b"preserve")
            alias = base / "alias"
            alias.symlink_to(target, target_is_directory=True)
            for refused in (target, alias):
                result = subprocess.run([executable, "--ignored", "--test-threads=1"],
                                        env={**environment, "OPENTRACK_TEST_FULL_DISK_ROOT": str(refused)},
                                        capture_output=True, timeout=20)
                if not result.returncode or b"A fresh nonsymlink empty storage mount is required" not in result.stdout + result.stderr:
                    raise SystemExit("Storage test accepted a linked or nonempty target")
                if sentinel.read_bytes() != b"preserve" or list(target.iterdir()) != [sentinel]:
                    raise SystemExit("Rejected storage target was changed")
            alias.unlink()
            sentinel.unlink()
            try:
                result = subprocess.run([executable, "--ignored", "--test-threads=1", "--nocapture"],
                                        env=environment, timeout=180)
            except subprocess.TimeoutExpired:
                raise SystemExit("Storage qualification exceeded its deadline") from None
            if result.returncode:
                raise SystemExit("Storage qualification test failed")
        finally:
            subprocess.run(["umount", str(target)], check=True, capture_output=True)
finally:
    subprocess.run(["umount", "/tmp"], check=True, capture_output=True)
print("Adapter and PostgreSQL/API ENOSPC qualification passed on separate private 4 MiB tmpfs mounts")
print("Both test executables refuse nonempty and linked roots without changing sentinel bytes")
'''
environment = {**os.environ, "OPENTRACK_QUALIFICATION_PARENT_MOUNT": os.readlink("/proc/self/ns/mnt")}
with tempfile.TemporaryDirectory(prefix="opentrack-storage-qualification-", dir="/tmp") as directory:
    try:
        result = run_owned(["unshare", "--user", "--map-root-user", "--mount", "--propagation", "private",
                            "python3", "-c", program, directory, *executables], env=environment, timeout=480)
    except subprocess.TimeoutExpired:
        raise SystemExit("Storage qualification exceeded its deadline") from None
    if result.returncode:
        raise SystemExit("Storage qualification failed; no acceptance recorded")
PY
