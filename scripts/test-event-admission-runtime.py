#!/usr/bin/env python3
"""Boundary and cleanup regressions; no Docker, database or HTTP acceptance."""
import importlib.util
from pathlib import Path
from types import SimpleNamespace
import unittest
from unittest.mock import Mock


spec = importlib.util.spec_from_file_location(
    "event_admission_runtime", Path(__file__).with_name("event_admission_runtime.py"))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)

BASE = "/api/v1/workspaces/12345678-abcd-4123-8123-123456789abc"
ISSUE = BASE + "/issues/87654321-dcba-4321-8765-cba987654321"


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


class SourceBoundaries(unittest.TestCase):
    def assert_rejected_without_runtime_access(self, *, project="opentrack-smoke-boundary",
                                               url="http://127.0.0.1:18081", base=BASE,
                                               issue_path=ISSUE):
        runtime = SimpleNamespace(project=project, url=url)
        for method in ("dc", "sql", "request", "command", "container", "record"):
            setattr(runtime, method, Mock(side_effect=AssertionError("Runtime accessed before validation")))
        with self.assertRaises(RuntimeError):
            module.qualify(runtime, base, issue_path, require)
        for method in ("dc", "sql", "request", "command", "container", "record"):
            getattr(runtime, method).assert_not_called()

    def test_project_must_be_explicitly_disposable(self):
        for project in ("opentrack", "opentrack-smoke-", "opentrack-smoke-Boundary",
                        "opentrack-smoke-boundary\n"):
            with self.subTest(project=project):
                self.assert_rejected_without_runtime_access(project=project)

    def test_origin_must_be_local_and_unambiguous(self):
        for url in ("http://example.test:18081", "http://0.0.0.0:18081",
                    "http://127.0.0.1.example.test:18081", "https://localhost:18081",
                    "http://operator@localhost:18081", "http://@localhost:18081",
                    "http://localhost:18081/path", "http://localhost:18081?target=other",
                    "http://localhost:18081#other", "http://localhost:18081\n",
                    "http://localhost:invalid", "http://localhost:0"):
            with self.subTest(url=url):
                self.assert_rejected_without_runtime_access(url=url)

    def test_workspace_source_must_be_canonical(self):
        for base in (BASE.replace("abcd", "ABCD"), BASE.replace("-", ""), BASE + "/",
                     BASE + "?other=true", BASE + "'; SELECT 1; --"):
            with self.subTest(base=base):
                self.assert_rejected_without_runtime_access(base=base)

    def test_issue_source_must_be_canonical_and_inside_workspace(self):
        for issue_path in (ISSUE.replace("dcba", "DCBA"), ISSUE.replace("-", ""), ISSUE + "/",
                           ISSUE + "?other=true", ISSUE.replace("12345678", "12345679", 1),
                           ISSUE.replace("/issues/", "/issues/%2e%2e/")):
            with self.subTest(issue_path=issue_path):
                self.assert_rejected_without_runtime_access(issue_path=issue_path)


class WorkerCleanup(unittest.TestCase):
    def assert_worker_restart_attempted(self, runtime):
        self.assertIn(("start", "worker"), [call.args for call in runtime.dc.call_args_list])

    def test_stop_failure_attempts_restart_without_running_body(self):
        stop_error = RuntimeError("Stop failed")
        runtime = SimpleNamespace(dc=Mock(side_effect=[stop_error, None]))
        body = Mock()
        with self.assertRaises(RuntimeError) as caught:
            with module._stopped_worker(runtime):
                body()
        self.assertIs(caught.exception, stop_error)
        body.assert_not_called()
        self.assert_worker_restart_attempted(runtime)

    def test_body_failure_attempts_restart(self):
        primary = RuntimeError("Qualification failed")
        runtime = SimpleNamespace(dc=Mock())
        with self.assertRaises(RuntimeError) as caught:
            with module._stopped_worker(runtime):
                raise primary
        self.assertIs(caught.exception, primary)
        self.assert_worker_restart_attempted(runtime)

    def test_restart_failure_preserves_primary_and_reports_cleanup(self):
        for fail_stop in (False, True):
            with self.subTest(fail_stop=fail_stop):
                primary = RuntimeError("Primary qualification failure")
                restart_error = RuntimeError("Restart failed")
                runtime = SimpleNamespace(dc=Mock(side_effect=[primary if fail_stop else None,
                                                             restart_error]))
                with self.assertRaises(RuntimeError) as caught:
                    with module._stopped_worker(runtime):
                        raise primary
                self.assertIs(caught.exception, primary)
                self.assertTrue(getattr(primary, "__notes__", []), "Cleanup failure needs an exception note")
                self.assert_worker_restart_attempted(runtime)

    def test_restart_failure_surfaces_after_successful_body(self):
        restart_error = RuntimeError("Restart failed")
        runtime = SimpleNamespace(dc=Mock(side_effect=[None, restart_error]))
        body = Mock()
        with self.assertRaises(RuntimeError) as caught:
            with module._stopped_worker(runtime):
                body()
        self.assertIs(caught.exception, restart_error)
        body.assert_called_once_with()
        self.assert_worker_restart_attempted(runtime)


if __name__ == "__main__":
    unittest.main()
