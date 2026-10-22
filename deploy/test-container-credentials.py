#!/usr/bin/env python3
"""Run as root inside the bootstrap image; fixtures are disposable directories."""
import fcntl
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import stat
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

SCRIPT = Path(__file__).with_name("container-credentials.py")
spec = importlib.util.spec_from_file_location("credentials", SCRIPT)
credentials = importlib.util.module_from_spec(spec)
spec.loader.exec_module(credentials)


class BootstrapContracts(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="opentrack-bootstrap-test-")
        self.base = Path(self.temp.name)
        self.root = self.base / "secrets"
        self.root.mkdir()
        for name in credentials.FILES:
            (self.root / name).mkdir()
        self.blobs = self.base / "blobs"
        self.blobs.mkdir()
        self.database = self.base / "database"
        self.database.mkdir()

    def tearDown(self):
        self.temp.cleanup()

    def invoke(self, action="initialize"):
        return subprocess.run([sys.executable, str(SCRIPT), action, "--root", str(self.root),
                               "--blob-root", str(self.blobs), "--database-state", str(self.database)],
                              capture_output=True, text=True)

    def setup_complete(self):
        result = self.invoke()
        self.assertTrue(result.returncode == 0, "Fresh initialization must succeed.")
        self.assertFalse("postgres://" in result.stdout + result.stderr)

    def snapshot(self):
        result = {}
        for path in sorted(self.base.rglob("*")):
            info = path.lstat()
            digest = hashlib.sha256(path.read_bytes()).hexdigest() if stat.S_ISREG(info.st_mode) else None
            result[str(path.relative_to(self.base))] = (info.st_uid, info.st_gid, stat.S_IMODE(info.st_mode), digest)
        return result

    def refuses_unchanged(self):
        before = self.snapshot()
        result = self.invoke()
        self.assertNotEqual(result.returncode, 0)
        self.assertTrue(before == self.snapshot(), "Failure must preserve every preexisting byte and permission.")
        for kind, names in credentials.FILES.items():
            for name in names:
                path = self.root / kind / name
                if path.is_file() and not path.is_symlink():
                    value = path.read_text().strip()
                    if len(value) >= 32:
                        self.assertFalse(value in result.stdout + result.stderr, "Failure output must not disclose credentials.")

    def test_fresh_complete_permissions_urls_and_restart_preservation(self):
        self.setup_complete()
        for kind, names in credentials.FILES.items():
            directory = self.root / kind
            self.assertEqual(directory.stat().st_uid, credentials.owner(kind))
            self.assertEqual(stat.S_IMODE(directory.stat().st_mode), 0o700)
            for name in names:
                info = (directory / name).stat()
                self.assertEqual((info.st_uid, info.st_gid, stat.S_IMODE(info.st_mode)),
                                 (credentials.owner(kind), credentials.owner(kind), 0o600))
        (self.database / "existing-database").write_text("persistent database fixture")
        (self.blobs / "blobs/accepted-object").write_text("persistent blob fixture")
        before = self.snapshot()
        result = self.invoke()
        self.assertEqual(result.returncode, 0)
        self.assertTrue(before == self.snapshot(), "Restart must not rotate credentials or rewrite existing data.")

    def test_read_only_check_does_not_repair_credentials(self):
        self.setup_complete()
        before = self.snapshot()
        self.assertEqual(self.invoke("check").returncode, 0)
        self.assertTrue(before == self.snapshot())

    def test_missing_secret_refuses_without_regeneration(self):
        self.setup_complete()
        (self.root / "worker/worker-database-url").unlink()
        self.refuses_unchanged()

    def test_corrupt_secret_refuses_without_reset(self):
        self.setup_complete()
        (self.root / "app/database-url").write_text("malformed secret fixture\n")
        self.refuses_unchanged()

    def test_shared_file_permissions_refuse_without_chmod(self):
        self.setup_complete()
        (self.root / "migration/migration-database-url").chmod(0o644)
        self.refuses_unchanged()

    def test_wrong_owner_refuses_without_chown(self):
        self.setup_complete()
        os.chown(self.root / "app/setup-token", 0, 0)
        self.refuses_unchanged()

    def test_linked_secret_does_not_read_or_modify_the_target(self):
        self.setup_complete()
        secret = self.root / "app/setup-token"
        secret.unlink()
        external = self.base / "external-canary"
        external.write_text("external state must stay intact")
        secret.symlink_to(external)
        self.refuses_unchanged()

    def test_hardlinked_secret_refuses(self):
        self.setup_complete()
        os.link(self.root / "app/setup-token", self.base / "second-link")
        self.refuses_unchanged()

    def test_partial_volume_refuses_without_generating_any_secret(self):
        (self.root / "app/unexpected").write_text("partial initialization fixture")
        result = self.invoke()
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual((self.root / "app/unexpected").read_text(), "partial initialization fixture")
        for kind, names in credentials.FILES.items():
            for name in names:
                self.assertFalse((self.root / kind / name).exists())

    def test_existing_database_with_lost_secrets_refuses_new_credentials(self):
        (self.database / "PG_VERSION").write_text("18")
        result = self.invoke()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("restore its original secret volumes", result.stderr)
        self.assertEqual((self.database / "PG_VERSION").read_text(), "18")
        self.assertFalse((self.root / "database/postgres-password").exists())

    def test_existing_blobs_without_marker_refuse(self):
        (self.blobs / "retained-data").write_text("keep this blob")
        self.assertNotEqual(self.invoke().returncode, 0)
        self.assertEqual((self.blobs / "retained-data").read_text(), "keep this blob")
        self.assertFalse((self.root / "database/postgres-password").exists())

    def test_interrupted_initialization_sentinel_refuses(self):
        (self.root / "database" / credentials.INCOMPLETE).write_text("interrupted")
        self.assertNotEqual(self.invoke().returncode, 0)
        self.assertFalse((self.root / "database/postgres-password").exists())

    def test_actual_write_failure_leaves_a_durable_partial_marker_and_refuses_retry(self):
        original = credentials.Directory.write_new
        def failing_write(directory, name, data, uid):
            if name == "worker-password":
                raise OSError("injected filesystem write failure")
            return original(directory, name, data, uid)
        with patch.object(credentials.Directory, "write_new", failing_write):
            with self.assertRaises(OSError):
                credentials.initialize(self.root, self.blobs, self.database)
        self.assertTrue((self.root / "database" / credentials.INCOMPLETE).exists())
        self.assertFalse((self.root / "database" / credentials.MARKER).exists())
        self.assertTrue((self.root / "database/postgres-password").exists())
        self.refuses_unchanged()

    def test_marker_corruption_refuses(self):
        self.setup_complete()
        (self.root / "database" / credentials.MARKER).write_text('{"schema":')
        self.refuses_unchanged()

    def test_inconsistent_url_refuses_even_with_matching_file_hash(self):
        self.setup_complete()
        wrong = b"postgres://opentrack:0000000000000000000000000000000000000000000000000000000000000000@database:5432/opentrack\n"
        (self.root / "app/database-url").write_bytes(wrong)
        marker_path = self.root / "database" / credentials.MARKER
        marker = json.loads(marker_path.read_text())
        marker["files"]["app/database-url"] = hashlib.sha256(wrong).hexdigest()
        marker_path.write_text(json.dumps(marker))
        self.refuses_unchanged()

    def test_concurrent_initializer_cannot_cross_the_lock(self):
        self.setup_complete()
        with (self.root / "database" / credentials.LOCK).open("rb") as lock:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            self.refuses_unchanged()

    def test_only_explicit_service_user_token_command_discloses_token(self):
        self.setup_complete()
        self.base.chmod(0o755)
        expected = (self.root / "app/setup-token").read_bytes()
        command = [sys.executable, str(SCRIPT), "setup-token", "--app-secrets", str(self.root / "app")]
        denied = subprocess.run(command, capture_output=True)
        self.assertNotEqual(denied.returncode, 0)
        self.assertFalse(expected.strip() in denied.stdout + denied.stderr)
        def service_user():
            os.setgroups([])
            os.setgid(credentials.SERVICE_UID)
            os.setuid(credentials.SERVICE_UID)
        accepted = subprocess.run(command, capture_output=True, preexec_fn=service_user)
        self.assertTrue(accepted.returncode == 0 and accepted.stdout == expected and not accepted.stderr,
                        "Explicit token command must expose only the validated token to its service user.")

    def output_command(self, path):
        return subprocess.run([sys.executable, str(SCRIPT), "prepare-output", "--directory", str(path)], capture_output=True, text=True)

    def test_new_operator_output_becomes_private_and_existing_files_are_preserved(self):
        output = self.base / "operator-output"
        output.mkdir()
        self.assertEqual(self.output_command(output).returncode, 0)
        info = output.stat()
        self.assertEqual((info.st_uid, info.st_gid, stat.S_IMODE(info.st_mode)), (10001, 10001, 0o700))
        (output / "existing-code").write_text("retained code fixture")
        before = self.snapshot()
        self.assertEqual(self.output_command(output).returncode, 0)
        self.assertTrue(before == self.snapshot(), "Existing operator recovery files must not be changed.")

    def test_nonempty_uninitialized_operator_output_refuses(self):
        output = self.base / "operator-output"
        output.mkdir()
        (output / "foreign-file").write_text("keep this file")
        before = self.snapshot()
        self.assertNotEqual(self.output_command(output).returncode, 0)
        self.assertTrue(before == self.snapshot())

    def test_unsafe_existing_operator_output_is_not_silently_repaired(self):
        output = self.base / "operator-output"
        output.mkdir()
        os.chown(output, 10001, 10001)
        output.chmod(0o755)
        before = self.snapshot()
        self.assertNotEqual(self.output_command(output).returncode, 0)
        self.assertTrue(before == self.snapshot())


if __name__ == "__main__":
    if os.geteuid() != 0:
        raise SystemExit("Run these ownership/volume tests as root inside the bootstrap image.")
    unittest.main()
