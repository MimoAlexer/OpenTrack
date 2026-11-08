#!/usr/bin/env python3
"""Challenge the two-client HTTPS qualification without starting containers."""
import importlib.util
import json
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch


spec = importlib.util.spec_from_file_location("https_smoke", Path(__file__).with_name("docker-https-smoke.py"))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class AdmissionQualification(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.qual = module.Qualification(SimpleNamespace(project="opentrack-smoke-test", output="unused.json"))
        self.qual.certificate = Path(directory.name) / "root.crt"
        self.qual.certificate.write_text("PUBLIC TEST CA\n", encoding="ascii")
        self.network = "opentrack-smoke-test_proxy_ingress"
        self.app = {"Image": "sha256:" + "c" * 64,
                    "Config": {"Env": ['OPENTRACK__SERVER__TRUSTED_PROXY_IPS=["192.0.2.2"]']}}
        self.proxy = {"NetworkSettings": {"Networks": {self.network: {"IPAddress": "192.0.2.2"}}}}
        self.clients = []
        self.delivered = []
        self.removed = []
        self.counts = {}
        self.mode = "separate"

    def command(self, arguments, **options):
        if arguments[:2] == ["docker", "run"]:
            self.assertIn("--read-only", arguments)
            self.assertEqual(arguments[arguments.index("--user") + 1], "10001:10001")
            self.assertEqual(arguments[arguments.index("--cap-drop") + 1], "ALL")
            self.assertIn("no-new-privileges:true", arguments)
            self.assertNotIn("--mount", arguments)
            client = ("a" if not self.clients else "b") * 64
            self.clients.append(client)
            return SimpleNamespace(stdout=client + "\n")
        if arguments[:3] == ["docker", "exec", "--interactive"]:
            self.assertEqual(arguments[4:], ["sh", "-c", "umask 077; cat > /tmp/root.crt"])
            self.assertEqual(options.get("input"), "PUBLIC TEST CA\n")
            self.delivered.append(arguments[3])
            return SimpleNamespace(stdout="")
        if arguments[:2] == ["docker", "inspect"]:
            return SimpleNamespace(stdout=json.dumps([
                {"NetworkSettings": {"Networks": {self.network: {
                    "IPAddress": "192.0.2.128" if self.mode == "same_address" else f"192.0.2.{128 + index}"
                }}}} for index, _ in enumerate(self.clients)
            ]))
        if arguments[:2] == ["docker", "exec"] and arguments[3] == "curl":
            client = arguments[2]
            self.assertIn(client, self.delivered)
            self.assertNotIn("--insecure", arguments)
            self.assertEqual(arguments[arguments.index("--cacert") + 1], "/tmp/root.crt")
            headers = [arguments[i + 1] for i, value in enumerate(arguments) if value == "--header"]
            forged = next(value for value in headers if value.startswith("X-OpenTrack-Client-IP:"))
            key = "all" if self.mode == "shared" else forged if self.mode == "spoofed" else client
            self.counts[key] = self.counts.get(key, 0) + 1
            return SimpleNamespace(stdout="422" if self.counts[key] <= 20 else "429")
        if arguments[:3] == ["docker", "rm", "--force"]:
            self.removed.append(arguments[3])
            return SimpleNamespace(stdout="")
        raise AssertionError(f"Unexpected fixture command: {arguments[:3]}")

    def run_probe(self):
        with patch.object(self.qual, "command", side_effect=self.command):
            self.qual.authentication_admission(self.app, self.proxy)

    def test_verified_ca_reaches_two_nonroot_clients_and_isolation_passes(self):
        self.run_probe()
        self.assertEqual(self.delivered, self.clients)
        self.assertEqual(self.removed, self.clients)
        self.assertEqual(sorted(self.counts.values()), [1, 22])
        self.assertEqual(len(self.qual.checks), 1)

    def test_shared_proxy_allowance_is_rejected_and_both_clients_are_removed(self):
        self.mode = "shared"
        with self.assertRaisesRegex(RuntimeError, "different client's"):
            self.run_probe()
        self.assertEqual(self.removed, self.clients)

    def test_trusting_forged_client_headers_cannot_pass_qualification(self):
        self.mode = "spoofed"
        with self.assertRaisesRegex(RuntimeError, "forging client headers"):
            self.run_probe()
        self.assertEqual(self.removed, self.clients)

    def test_same_source_address_cannot_masquerade_as_two_clients(self):
        self.mode = "same_address"
        with self.assertRaisesRegex(RuntimeError, "distinct proxy-visible"):
            self.run_probe()
        self.assertEqual(self.counts, {})
        self.assertEqual(self.removed, self.clients)

    def test_command_failure_names_the_stage_without_printing_input_or_output(self):
        result = SimpleNamespace(returncode=1, stdout="private stdout", stderr="private stderr")
        with patch.object(module.subprocess, "run", return_value=result):
            with self.assertRaisesRegex(RuntimeError, "certificate delivery failed") as raised:
                self.qual.command(["docker", "exec"], input="private input", purpose="certificate delivery")
        self.assertNotIn("private", str(raised.exception))


if __name__ == "__main__":
    unittest.main()
