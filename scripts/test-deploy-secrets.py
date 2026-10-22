#!/usr/bin/env python3
"""Credential upgrade regression tests in disposable private directories."""
from pathlib import Path
import hashlib
import os
import stat
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parent.parent
HELPER = ROOT / "scripts/deploy-secrets.py"


def snapshot(directory):
    return {p.name: (stat.S_IMODE(p.lstat().st_mode),
                     p.readlink().as_posix() if p.is_symlink() else hashlib.sha256(p.read_bytes()).hexdigest())
            for p in directory.iterdir()}


class DeploymentSecrets(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="opentrack-deploy-secrets-")
        self.addCleanup(self.temp.cleanup)
        self.directory = Path(self.temp.name) / "secrets"

    def run_helper(self, *flags, success=True):
        result = subprocess.run([sys.executable, str(HELPER), "--directory", str(self.directory), *flags],
                                capture_output=True, text=True)
        output = result.stdout + result.stderr
        if self.directory.exists():
            for name in ("worker-password", "operator-password", "runtime-password", "migration-password", "setup-token"):
                path = self.directory / name
                if path.is_file():
                    value = path.read_text().strip()
                    if value:
                        self.assertNotIn(value, output, "A credential appeared in helper output")
        self.assertEqual(result.returncode == 0, success, output)
        return result

    def test_fresh_layout_and_refusal_to_replace(self):
        self.run_helper()
        self.assertEqual(len(list(self.directory.iterdir())), 11)
        self.assertEqual(stat.S_IMODE(self.directory.stat().st_mode), 0o700)
        self.assertEqual(stat.S_IMODE((self.directory / "worker-password").stat().st_mode), 0o444)
        self.assertEqual(stat.S_IMODE((self.directory / "worker-database-url").stat().st_mode), 0o600)
        before = snapshot(self.directory)
        self.run_helper("--check")
        self.run_helper(success=False)
        self.assertEqual(snapshot(self.directory), before)

    def test_upgrade_adds_only_worker_and_is_idempotent(self):
        self.run_helper()
        for name in ("worker-password", "worker-database-url"):
            (self.directory / name).unlink()
        before = snapshot(self.directory)
        self.run_helper("--add-worker")
        after = snapshot(self.directory)
        self.assertEqual({key: after[key] for key in before}, before)
        self.assertEqual(set(after) - set(before), {"worker-password", "worker-database-url"})
        self.run_helper("--add-worker")
        self.run_helper("--check")
        self.assertEqual(snapshot(self.directory), after)

    def test_old_base_can_add_both_capabilities_without_rotation(self):
        self.run_helper()
        for kind in ("operator", "worker"):
            for suffix in ("password", "database-url"):
                (self.directory / f"{kind}-{suffix}").unlink()
        before = snapshot(self.directory)
        self.run_helper("--add-operator")
        self.run_helper("--add-worker")
        self.run_helper("--check")
        after = snapshot(self.directory)
        self.assertEqual({key: after[key] for key in before}, before)

    def test_partial_mismatched_or_unsafe_pair_never_rewritten(self):
        self.run_helper()
        url = self.directory / "worker-database-url"
        original = url.read_text()
        url.unlink()
        before = snapshot(self.directory)
        self.run_helper("--add-worker", success=False)
        self.assertEqual(snapshot(self.directory), before)
        url.write_text(original.replace("opentrack_worker", "opentrack_operator"))
        url.chmod(0o600)
        before = snapshot(self.directory)
        self.run_helper("--check", success=False)
        self.run_helper("--add-worker", success=False)
        self.assertEqual(snapshot(self.directory), before)
        url.write_text(original)
        url.chmod(0o644)
        before = snapshot(self.directory)
        self.run_helper("--add-worker", success=False)
        self.assertEqual(snapshot(self.directory), before)
        url.unlink()
        url.symlink_to(self.directory / "operator-database-url")
        before = snapshot(self.directory)
        self.run_helper("--add-worker", success=False)
        self.assertEqual(snapshot(self.directory), before)


if __name__ == "__main__":
    if os.getuid() == 0:
        sys.exit("Run credential tests as the intended nonroot service user")
    unittest.main()
