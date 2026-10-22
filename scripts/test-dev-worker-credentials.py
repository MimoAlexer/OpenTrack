#!/usr/bin/env python3
"""Credential upgrades must be additive, private, and silent on malformed input."""
from pathlib import Path
import os
import subprocess
import sys
import tempfile
import unittest

HELPER = Path(__file__).with_name("dev-worker-credentials.py")


class WorkerCredentialUpgrade(unittest.TestCase):
    def run_helper(self, root, port="55432"):
        return subprocess.run(
            [sys.executable, str(HELPER), str(root / "dev.env"), str(root), port],
            text=True, capture_output=True, check=False,
        )

    def test_additive_private_and_idempotent(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            original = b"export EXISTING_CREDENTIAL='preserved value'\n"
            (root / "dev.env").write_bytes(original)
            (root / "dev.env").chmod(0o600)
            first = self.run_helper(root)
            self.assertEqual(first.returncode, 0, first.stderr)
            self.assertTrue((root / "dev.env").read_bytes().startswith(original))
            before = {p.name: p.read_bytes() for p in root.iterdir()}
            second = self.run_helper(root)
            self.assertEqual(second.returncode, 0, second.stderr)
            self.assertEqual(before, {p.name: p.read_bytes() for p in root.iterdir()})
            self.assertEqual(len(before), 3)
            for path in root.iterdir():
                self.assertEqual(path.stat().st_mode & 0o777, 0o600)
                self.assertEqual(path.stat().st_uid, os.getuid())
            for value in before.values():
                self.assertNotIn(value.decode().strip(), first.stdout + first.stderr)

    def test_malformed_existing_urls_do_not_leak_or_partially_upgrade(self):
        canary = "private-canary-not-for-diagnostics"
        urls = [
            f"postgres://opentrack_test_worker:{'a'*64}@127.0.0.1:{canary}/opentrack_test",
            f"postgres://opentrack_test_worker:{canary}@[broken/opentrack_test",
        ]
        for url in urls:
            with self.subTest(url_kind=urls.index(url)), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                original = f"export OPENTRACK_TEST_WORKER_DATABASE_URL='{url}'\n".encode()
                (root / "dev.env").write_bytes(original)
                (root / "dev.env").chmod(0o600)
                result = self.run_helper(root)
                self.assertNotEqual(result.returncode, 0)
                self.assertNotIn(canary, result.stdout + result.stderr)
                self.assertNotIn("Traceback", result.stderr)
                self.assertEqual((root / "dev.env").read_bytes(), original)
                self.assertEqual([p.name for p in root.iterdir()], ["dev.env"])

    def test_malformed_port_argument_is_redacted(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "dev.env").write_bytes(b"")
            (root / "dev.env").chmod(0o600)
            result = self.run_helper(root, "port-canary-secret")
            self.assertNotEqual(result.returncode, 0)
            self.assertNotIn("port-canary-secret", result.stdout + result.stderr)
            self.assertNotIn("Traceback", result.stderr)
            self.assertEqual([p.name for p in root.iterdir()], ["dev.env"])


if __name__ == "__main__":
    unittest.main()
