#!/usr/bin/env bash
set -Eeuo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."
if [[ -f work/shared/goal-coordination/with-lock.sh && "${OPENTRACK_SCANNER_QUALIFICATION_LOCKED:-}" != 1 ]]; then
  exec bash work/shared/goal-coordination/with-lock.sh verification env OPENTRACK_SCANNER_QUALIFICATION_LOCKED=1 bash scripts/qualify-attachment-scanner.sh
fi
exec python3 - <<'PY'
import json
import os
from pathlib import Path
import re
import signal
import subprocess
import sys
import tempfile
from urllib.parse import urlsplit

def fail(message):
    raise SystemExit(message)

def interrupted(_signal, _frame):
    fail("Scanner qualification interrupted; no acceptance recorded")

signal.signal(signal.SIGTERM, interrupted)
if sys.platform != "linux":
    fail("Installed scanner qualification requires Linux")
binary = Path(os.environ.get("OPENTRACK_TEST_CLAMD_BIN", ""))
if not binary.is_absolute() or not binary.is_file() or not os.access(binary, os.X_OK):
    fail("Set OPENTRACK_TEST_CLAMD_BIN to an explicitly trusted installed clamd executable")
libraries = os.environ.get("OPENTRACK_TEST_CLAMD_LIBRARY_PATH")
if libraries is not None and any(not Path(path).is_absolute() or not Path(path).is_dir()
                                 for path in libraries.split(":")):
    fail("Scanner library directories must exist and be absolute")
# This custom-only profile uses CVDCertsDirectory, available in ClamAV 1.5+.
# https://github.com/Cisco-Talos/clamav/blob/clamav-1.5.4/etc/clamd.conf.sample
with tempfile.TemporaryDirectory(prefix="opentrack-scanner-version-", dir="/tmp") as directory:
    configuration = Path(directory) / "clamd.conf"
    configuration.write_text("Foreground yes\n")
    environment = {"LANG": "C", "TZ": "UTC"}
    if libraries is not None:
        environment["LD_LIBRARY_PATH"] = libraries
    try:
        version = subprocess.run([str(binary), "--foreground", "--version", "--config-file", str(configuration)],
                                 env=environment, capture_output=True, text=True, timeout=5)
    except (OSError, subprocess.TimeoutExpired):
        fail("Cannot verify the required ClamAV 1.5+ qualification profile")
    match = re.fullmatch(r"ClamAV (\d+)\.(\d+)\.(\d+)(?:[/\-][^\r\n]*)?", version.stdout.strip())
    if version.returncode or match is None or tuple(map(int, match.groups()[:2])) < (1, 5):
        fail("Scanner qualification requires ClamAV 1.5+ with CVDCertsDirectory support")
    print("Scanner qualification profile: ClamAV " + ".".join(match.groups()))
names = ["OPENTRACK_TEST_DATABASE_URL", "OPENTRACK_TEST_MIGRATION_DATABASE_URL",
         "OPENTRACK_TEST_OPERATOR_DATABASE_URL", "OPENTRACK_TEST_WORKER_DATABASE_URL"]
try:
    databases = [urlsplit(os.environ[name]) for name in names]
    valid = (all(url.scheme in {"postgres", "postgresql"} and url.path.endswith("_test") for url in databases)
             and len({(url.hostname, url.port, url.path) for url in databases}) == 1
             and len({url.username for url in databases}) == 4)
except (KeyError, ValueError):
    valid = False
if not valid:
    fail("Scanner qualification requires four distinct restricted roles on one isolated *_test database")
target = "attachment_scanner_qualification"
try:
    build = subprocess.run(["cargo", "test", "--locked", "-p", "opentrack-api", "--test", target,
                            "--no-run", "--message-format=json"], capture_output=True, text=True, timeout=600)
except subprocess.TimeoutExpired:
    fail("Scanner qualification compilation exceeded its deadline")
if build.returncode:
    fail("Scanner qualification compilation failed; inspect the locked Cargo build")
artifacts = [json.loads(line) for line in build.stdout.splitlines() if line.startswith("{")]
executables = [item["executable"] for item in artifacts
               if item.get("reason") == "compiler-artifact" and item.get("executable")
               and item["target"]["name"] == target and item["profile"]["test"]]
if len(executables) != 1:
    fail("Expected exactly one compiled scanner qualification executable")
with tempfile.TemporaryDirectory(prefix="opentrack-scanner-qualification-", dir="/tmp") as directory:
    environment = {**os.environ, "OPENTRACK_TEST_CLAMD_ROOT": directory}
    process = subprocess.Popen([executables[0], "--ignored", "--test-threads=1"],
                               env=environment, start_new_session=True)
    try:
        try:
            result = process.wait(timeout=120)
        except subprocess.TimeoutExpired:
            fail("Scanner qualification exceeded its deadline")
    finally:
        # Only the new session created for this test and its foreground daemon.
        try:
            os.killpg(process.pid, signal.SIGTERM)
        except ProcessLookupError:
            pass
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            pass
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        if process.poll() is None:
            process.wait(timeout=5)
    if result:
        fail("Installed scanner qualification failed; no acceptance recorded")
print("Installed clamd clean, private-signature detection and outage/restart API qualification passed")
print("Private fixture signatures do not qualify production signature coverage or malware detection breadth")
PY
