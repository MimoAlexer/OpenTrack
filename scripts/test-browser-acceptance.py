#!/usr/bin/env python3
"""Pure boundary tests; no containers, app server, browser or network are started."""
import copy
import importlib.util
import io
import json
import os
from pathlib import Path
import subprocess
import time
from types import SimpleNamespace
import unittest
from unittest.mock import Mock, patch
from uuid import UUID

import browser_order_fixtures as fixtures

spec = importlib.util.spec_from_file_location("browser_acceptance", Path(__file__).with_name("browser-acceptance.py"))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


def uid(number):
    return str(UUID(int=number))


def manifest():
    return {"COMPLETE": True, "workspace_id": uid(1), "maintenance": {
        browser: {"team_id": uid(10 + index * 4), "status_id": uid(11 + index * 4),
                  "job_id": uid(12 + index * 4), "requester_id": uid(13 + index * 4),
                  "staged_count": "300", "total_count": None}
        for index, browser in enumerate(fixtures.BROWSERS)}}


class PreparationHarness:
    """API/Compose double: progress can change only through the worker command."""
    def __init__(self):
        self.url = "http://localhost:18081"
        self.fresh, self.running, self.staged = True, True, "300"
        self.operations, self.groups = [], []
        self.fail_sql, self.fail_stop, self.displace_baseline = False, False, False
        self.fail_start, self.changed_worker, self.bad_block = False, False, False
        self.sql = []

    def owned_service(self, service, running=True, timeout=55):
        self.operations.append(("inspect", service))
        if running is not None:
            assert self.running is running
        return ("b" if self.changed_worker and not self.running else "a") * 64

    def owned_sql(self, statement, timeout):
        self.operations.append(("sql",))
        self.sql.append(statement)
        if self.fail_sql:
            raise RuntimeError("Mock fixture input failure")

    def dc(self, *arguments, timeout):
        self.operations.append(arguments)
        if arguments[0] == "stop":
            self.running = False
            if self.fail_stop:
                raise RuntimeError("Mock partial stop failure")
        elif arguments == ("start", "worker"):
            if self.fail_start:
                raise RuntimeError("Mock worker restoration failure")
            self.running = True
        else:
            assert arguments == ("run", "--rm", "--no-deps", "-T", "worker", "worker", "--once")
            assert not self.running
            for group in self.groups:
                if group["state"] == "queued":
                    if group["revoked"]:
                        group.update(state="blocked", error_code="access_lost", can_resume=not self.bad_block)
                    else:
                        group["staged_count"] = self.staged
                    break

    def client(self, origin, deadline):
        harness = self

        class FakeClient:
            group = None

            def authenticate(self, path, body, expected=(200,)):
                harness.operations.append(("http", "POST", path))
                if path == "/api/v1/setup":
                    assert body == {"setup_token": "synthetic-token", "email": "owner@example.test",
                                    "password": "synthetic-password", "display_name": "Example Owner",
                                    "workspace_name": "Browser acceptance", "team_name": "Example Team", "team_key": "EXA"}
                    return {"user": {"id": uid(2)}, "workspaces": [{"id": uid(1), "name": "Browser acceptance"}]}
                assert path == "/api/v1/invitations/accept" and body["token"] == "synthetic-invitation"
                self.group = harness.groups[-1]
                return {"user": {"id": self.group["requester_id"]}}

            def request(self, path, method="GET", body=None, headers=None, expected=(200,)):
                harness.operations.append(("http", method, path))
                if path == "/api/v1/setup/status":
                    return {"setup_required": harness.fresh}
                if path.endswith("/bootstrap"):
                    first = {"id": uid(3), "name": "Example Team", "key": "EXA"}
                    if harness.displace_baseline and harness.groups:
                        first["id"] = harness.groups[0]["team_id"]
                    return {"teams": [first]}
                if path.endswith("/teams"):
                    assert method == "POST" and body["private"] is True
                    index = len(harness.groups)
                    group = {**manifest()["maintenance"][fixtures.BROWSERS[index]], "state": "queued",
                             "phase": "staging", "staged_count": "0", "applied_count": "0", "revoked": False}
                    harness.groups.append(group)
                    return {"id": group["team_id"]}
                if path.endswith("/workflow"):
                    return {"statuses": [{"id": harness.groups[-1]["status_id"], "is_default": True}]}
                if path.endswith("/invitations"):
                    assert body["role"] == "member" and body["team_ids"] == [harness.groups[-1]["team_id"]]
                    return {"token": "synthetic-invitation"}
                if path.endswith("/issues"):
                    assert method == "POST" and expected == (409,)
                    return {"code": "issue_order_maintenance", "details": {"applied": False, "maintenance": [{"id": self.group["job_id"]}]}}
                if "/members/" in path:
                    group = harness.groups[-1]
                    if method == "DELETE":
                        assert headers["If-Match"] == f'"team-member:{group["team_id"]}:{group["requester_id"]}:1"'
                        group["revoked"] = True
                        return None
                    return {"version": "1"}
                if "/issue-order-maintenance/" in path:
                    group = next(group for group in harness.groups if path.endswith(group["job_id"]))
                    if self.group:
                        assert group["revoked"] and expected == (404,)
                        return {"code": "not_found"}
                    return {**group, "id": group["job_id"]}
                raise AssertionError("Unexpected mocked API route")

        return FakeClient()


class Boundaries(unittest.TestCase):
    def setUp(self):
        self.env = {"GITHUB_ACTIONS": "true", "RUNNER_OS": "Linux", "GITHUB_RUN_ID": "123", "GITHUB_RUN_ATTEMPT": "2"}
        self.project = "opentrack-browser-123-2-amd64"
        names = {source for mounts in module.SERVICE_MOUNTS.values() for source, _, _ in mounts}
        self.config = {"name": self.project, "volumes": {name: {"name": self.project + "_" + name} for name in names},
                       "services": {name: {"volumes": [{"type": "volume", "source": source, "target": target, "read_only": readonly}
                                                         for source, target, readonly in sorted(mounts)]}
                                    for name, mounts in module.SERVICE_MOUNTS.items()}}
        self.config["services"]["app"]["ports"] = [{"host_ip": "127.0.0.1", "published": "18081", "target": 8080}]
        for name, service in self.config["services"].items():
            if name == "credentials":
                service["profiles"] = ["tools"]
            elif name.startswith("operator"):
                service["profiles"] = ["operator"]

    def test_only_current_disposable_project(self):
        self.assertEqual(module.validate_target(self.project, "http://localhost:18081/", self.env), "http://localhost:18081")
        for project in ("opentrack", "opentrack-smoke-123-amd64", "opentrack-browser-123-1-amd64", "opentrack-browser-124-2-amd64", ""):
            with self.subTest(project=project), self.assertRaises(RuntimeError):
                module.validate_target(project, "http://localhost:18081", self.env)

    def test_no_local_or_non_loopback_runtime(self):
        for url in ("http://example.test:18081", "http://0.0.0.0:18081", "http://localhost:8080", "https://localhost:18081", "http://localhost:18081/path", "http://user@localhost:18081", "http://localhost:18081?target=elsewhere"):
            with self.subTest(url=url), self.assertRaises(RuntimeError):
                module.validate_target(self.project, url, self.env)
        for key in self.env:
            with self.subTest(key=key), self.assertRaises(RuntimeError):
                module.validate_target(self.project, "http://localhost:18081", {**self.env, key: ""})

    def test_compose_cannot_reach_user_volumes_or_ports(self):
        module.validate_compose(self.config, self.project)
        changes = [lambda c: c.update(name="opentrack"),
                   lambda c: c["volumes"]["database"].update(name="opentrack_database"),
                   lambda c: c["volumes"]["database"].update(external=True),
                   lambda c: c["volumes"]["database"].update(driver_opts={"type": "none", "o": "bind", "device": "/outside"}),
                   lambda c: c["volumes"]["api_secrets"].update(name=self.project + "_other"),
                   lambda c: c["services"]["credentials"]["volumes"][0].update(read_only=False),
                   lambda c: c["services"]["worker"]["volumes"][1].update(source="migration_secrets"),
                   lambda c: c["services"]["credentials"].update(profiles=[]),
                   lambda c: c["services"]["app"]["ports"][0].update(host_ip="0.0.0.0"),
                   lambda c: c["services"]["database"].update(ports=[5432]),
                   lambda c: c["services"]["app"].update(volumes=[{"type": "bind", "source": "/tmp"}])]
        for change in changes:
            config = copy.deepcopy(self.config)
            change(config)
            with self.assertRaises(RuntimeError):
                module.validate_compose(config, self.project)

    def runner(self):
        with patch.dict(os.environ, self.env, clear=True):
            runner = module.BrowserAcceptance(self.project, "http://localhost:18081")
        runner.preflight = Mock(return_value=self.config)
        runner.validate_live_resources = Mock()
        runner.dc = Mock(return_value="a" * 64)
        self.container = self.container_for("worker")
        runner.command = Mock(side_effect=lambda *args, **kwargs: json.dumps([self.container]))
        return runner

    def container_for(self, service):
        return {"Config": {"Labels": {"com.docker.compose.project": self.project,
                           "com.docker.compose.service": service}, "Cmd": [service], "Env": []},
                "State": {"Running": True}, "Mounts": [
                    {"Type": "volume", "Name": self.project + "_" + mount["source"],
                     "Destination": mount["target"], "RW": not mount["read_only"]}
                    for mount in self.config["services"][service]["volumes"]]}

    def test_service_scope_and_default_profile_reject_before_control(self):
        changes = [lambda c: c["Config"]["Labels"].update({"com.docker.compose.project": "other"}),
                   lambda c: c["Config"]["Labels"].update({"com.docker.compose.service": "database"}),
                   lambda c: c["Mounts"][0].update(Name="other_database"),
                   lambda c: c["Mounts"][0].update(Type="bind"),
                   lambda c: c["Mounts"][0].update(RW=not c["Mounts"][0]["RW"]),
                   lambda c: c["Mounts"][0].update(Destination="/elsewhere"),
                   lambda c: c["Config"].update(Cmd=["worker", "--once"]),
                   lambda c: c["Config"].update(Env=["OPENTRACK__WORKER__POLL_INTERVAL_MS=100"]),
                   lambda c: c["State"].update(Running=False)]
        for change in changes:
            runner = self.runner()
            change(self.container)
            with self.assertRaises(RuntimeError):
                runner.owned_service("worker")
            self.assertEqual(runner.dc.call_count, 1)
            self.assertEqual(runner.dc.call_args.args, ("ps", "--all", "--quiet", "worker"))

    def test_service_cannot_control_another_role_or_multiple_containers(self):
        runner = self.runner()
        self.assertEqual(runner.environment["COMPOSE_PROFILES"], "")
        with self.assertRaises(RuntimeError):
            runner.owned_service("app")
        runner.dc.assert_not_called()
        runner.dc.return_value = "a" * 64 + "\n" + "b" * 64
        with self.assertRaises(RuntimeError):
            runner.owned_service("worker")
        runner.command.assert_not_called()

    def test_database_exec_is_blocked_by_live_resource_mismatch(self):
        runner = self.runner()
        with self.assertRaises(RuntimeError):
            runner.owned_sql("SELECT 1", timeout=20)
        self.assertEqual(runner.dc.call_count, 1)
        self.assertEqual(runner.dc.call_args.args[0], "ps")

    def test_all_secret_blob_and_data_volume_labels_are_checked(self):
        for changed in (None, "api_secrets", "database_secrets", "migration_secrets", "worker_secrets", "operator_secrets", "blobs", "database"):
            runner = self.runner()
            services = [name for name, service in self.config["services"].items() if not service.get("profiles")]
            runner.dc.return_value = "\n".join(f"{index:064x}" for index in range(1, len(services) + 1))
            containers = [self.container_for(service) for service in services]
            names = {source for service in services for source, _, _ in module.SERVICE_MOUNTS[service]}
            volumes = [{"Name": self.project + "_" + name, "Driver": "local", "Options": None,
                        "Labels": {"com.docker.compose.project": self.project, "com.docker.compose.volume": name}}
                       for name in names]
            if changed:
                next(item for item in volumes if item["Labels"]["com.docker.compose.volume"] == changed)["Labels"]["com.docker.compose.project"] = "other"
            runner.command.side_effect = [json.dumps(containers), json.dumps(volumes)]
            with self.subTest(volume=changed):
                if changed:
                    with self.assertRaises(RuntimeError):
                        module.BrowserAcceptance.validate_live_resources(runner)
                else:
                    module.BrowserAcceptance.validate_live_resources(runner)

    def test_live_readonly_secret_mounts_cannot_change_after_config_validation(self):
        for service in ("app", "database", "migrate", "worker", "worker-grant", "worker-provision", "initialize"):
            container = self.container_for(service)
            mount = next(item for item in container["Mounts"] if "secrets" in item["Name"])
            mount["RW"] = not mount["RW"]
            with self.subTest(service=service), self.assertRaises(RuntimeError):
                module.validate_container(container, self.config, self.project, service)

    def test_all_resource_failure_precedes_credential_reads_and_preparation(self):
        runner = self.runner()
        runner.command = Mock(side_effect=[json.dumps({"Os": "linux", "Arch": "amd64", "Version": "28.0"}), "", ""])
        runner.dc.side_effect = lambda *args, **kwargs: "127.0.0.1:18081" if args[0] == "port" else ""
        runner.validate_live_resources.side_effect = RuntimeError("Mock secret volume ownership mismatch")
        with patch.object(module, "prepare") as preparation, patch.object(module.subprocess, "run") as node:
            with self.assertRaises(RuntimeError):
                runner.run()
        self.assertFalse(any("credentials" in call.args for call in runner.dc.call_args_list))
        preparation.assert_not_called()
        node.assert_not_called()
        self.assertEqual(runner.dc.call_args.args[0], "down")

    def test_existing_project_rejected_before_start_setup_or_cleanup(self):
        runner = self.runner()
        runner.command = Mock(side_effect=[json.dumps({"Os": "linux", "Arch": "amd64", "Version": "28.0"}), "existing"])
        with patch.object(module, "prepare") as preparation, patch.object(module.subprocess, "run") as child:
            with self.assertRaises(RuntimeError):
                runner.run()
        runner.dc.assert_not_called()
        preparation.assert_not_called()
        child.assert_not_called()

    def test_browser_environment_excludes_parent_secrets_and_controls(self):
        parent = {**self.env, "PATH": "tooling", "HOME": "cache", "DOCKER_HOST": "forbidden",
                  "COMPOSE_PROJECT_NAME": "forbidden", "PGPASSWORD": "forbidden", "GITHUB_TOKEN": "forbidden",
                  "OPENTRACK_MIGRATION_DATABASE_URL": "forbidden", "OPENTRACK_WORKER_DATABASE_URL_FILE": "forbidden",
                  "OPENTRACK_OPERATOR_DATABASE_URL": "forbidden", "HTTP_PROXY": "forbidden", "NODE_OPTIONS": "forbidden"}
        child = module.browser_environment(parent, "http://localhost:18081", "setup", "password", "references")
        self.assertNotIn("forbidden", child.values())
        self.assertEqual(child["PATH"], "tooling")
        self.assertEqual(child["OPENTRACK_BROWSER_PASSWORD"], "password")

    def test_completed_private_manifest_lifetime_cleanup_and_suite_watchdog(self):
        for outcome in (0, 1, subprocess.TimeoutExpired("node", 900)):
            with self.subTest(outcome=type(outcome).__name__):
                runner = self.runner()
                runner.command = Mock(side_effect=[json.dumps({"Os": "linux", "Arch": "amd64", "Version": "28.0"}), "", ""])
                runner.dc.side_effect = lambda *args, **kwargs: "127.0.0.1:18081" if args[0] == "port" else "a" * 64
                observed = []

                def node(arguments, *, cwd, env, timeout):
                    path = Path(env["OPENTRACK_BROWSER_ORDER_FIXTURES_FILE"])
                    observed.append(path)
                    self.assertEqual(json.loads(path.read_bytes()), manifest())
                    self.assertEqual(timeout, 900)
                    self.assertNotIn("COMPOSE_PROJECT_NAME", env)
                    if isinstance(outcome, Exception):
                        raise outcome
                    return SimpleNamespace(returncode=outcome)

                with patch.object(module, "prepare", return_value=manifest()), patch.object(module.subprocess, "run", side_effect=node), \
                        patch.object(module.os, "open", wraps=os.open) as opened, patch.object(module.os, "chmod", wraps=os.chmod) as mode, \
                        patch.object(module.time, "monotonic", return_value=1000):
                    if outcome == 0:
                        runner.run()
                    else:
                        with self.assertRaises((RuntimeError, subprocess.TimeoutExpired)):
                            runner.run()
                    self.assertTrue(any(call.args[-1] == 0o600 for call in opened.call_args_list))
                    self.assertTrue(any(call.args[-1] == 0o700 for call in mode.call_args_list))
                self.assertEqual(len(observed), 1)
                self.assertFalse(observed[0].exists())
                self.assertEqual(runner.dc.call_args.args, ("down", "--volumes", "--remove-orphans", "--timeout", "35"))

    def test_incomplete_preparation_cannot_launch_node_and_still_cleans_owned_project(self):
        runner = self.runner()
        runner.command = Mock(side_effect=[json.dumps({"Os": "linux", "Arch": "amd64", "Version": "28.0"}), "", ""])
        runner.dc.side_effect = lambda *args, **kwargs: "127.0.0.1:18081" if args[0] == "port" else "a" * 64
        incomplete = manifest()
        del incomplete["maintenance"]["webkit"]
        with patch.object(module, "prepare", return_value=incomplete), patch.object(module.subprocess, "run") as node:
            with self.assertRaises(RuntimeError):
                runner.run()
        node.assert_not_called()
        self.assertEqual(runner.dc.call_args.args[0], "down")

    def test_preparation_or_restore_failure_cannot_launch_node_and_still_cleans_project(self):
        runner = self.runner()
        runner.command = Mock(side_effect=[json.dumps({"Os": "linux", "Arch": "amd64", "Version": "28.0"}), "", ""])
        runner.dc.side_effect = lambda *args, **kwargs: "127.0.0.1:18081" if args[0] == "port" else "a" * 64
        with patch.object(module, "prepare", side_effect=RuntimeError("Worker restoration failed")), \
                patch.object(module.subprocess, "run") as node:
            with self.assertRaisesRegex(RuntimeError, "Worker restoration failed"):
                runner.run()
        node.assert_not_called()
        self.assertEqual(runner.dc.call_args.args[0], "down")

    def test_command_failure_withholds_all_captured_credentials(self):
        with patch.dict(os.environ, self.env, clear=True):
            runner = module.BrowserAcceptance(self.project, "http://localhost:18081")
        with patch.object(module.subprocess, "run", return_value=SimpleNamespace(
                returncode=1, stdout="synthetic-password", stderr="synthetic-token")):
            with self.assertRaises(RuntimeError) as raised:
                runner.command(["docker", "version"])
        self.assertNotIn("synthetic", str(raised.exception))

    def test_preparation_consumes_the_single_suite_budget_before_node(self):
        runner = self.runner()
        runner.command = Mock(side_effect=[json.dumps({"Os": "linux", "Arch": "amd64", "Version": "28.0"}), "", ""])
        clock = [1000]

        def compose(*args, **kwargs):
            if args[0] == "up":
                clock[0] += 240
            return "127.0.0.1:18081" if args[0] == "port" else "a" * 64

        runner.dc.side_effect = compose

        def preparation(*args, suite_deadline):
            self.assertEqual(suite_deadline, 2140)
            clock[0] += 137
            return manifest()

        with patch.object(module.time, "monotonic", side_effect=lambda: clock[0]), \
                patch.object(module, "prepare", side_effect=preparation), \
                patch.object(module.subprocess, "run", return_value=SimpleNamespace(returncode=0)) as node:
            runner.run()
        node.assert_called_once()
        self.assertEqual(node.call_args.kwargs["timeout"], 763)
        self.assertEqual(runner.dc.call_args.args[0], "down")

    def test_exhausted_suite_budget_never_starts_node_and_still_cleans_up(self):
        runner = self.runner()
        runner.command = Mock(side_effect=[json.dumps({"Os": "linux", "Arch": "amd64", "Version": "28.0"}), "", ""])
        runner.dc.side_effect = lambda *args, **kwargs: "127.0.0.1:18081" if args[0] == "port" else "a" * 64
        clock = [1000]

        def preparation(*args, suite_deadline):
            clock[0] = suite_deadline
            return manifest()

        with patch.object(module.time, "monotonic", side_effect=lambda: clock[0]), \
                patch.object(module, "prepare", side_effect=preparation), patch.object(module.subprocess, "run") as node:
            with self.assertRaisesRegex(RuntimeError, "bounded deadline"):
                runner.run()
        node.assert_not_called()
        self.assertEqual(runner.dc.call_args.args, ("down", "--volumes", "--remove-orphans", "--timeout", "35"))


class FixturePreparation(unittest.TestCase):
    def test_three_real_lifecycle_sequences_and_variable_partial_count(self):
        harness = PreparationHarness()
        with patch.object(fixtures, "Client", side_effect=harness.client):
            result = fixtures.prepare(harness, "synthetic-token", "synthetic-password", suite_deadline=time.monotonic() + 900)
        self.assertEqual(result, manifest())
        self.assertTrue(harness.running)
        self.assertEqual(harness.operations[-2:], [("start", "worker"), ("inspect", "worker")])
        self.assertEqual(len(harness.sql), 3)
        for statement in harness.sql:
            self.assertIn("SET LOCAL ROLE opentrack_migrator", statement)
            self.assertLess(statement.index("database authority mismatch"), statement.index("INSERT INTO issues"))
            self.assertIn("SET LOCAL opentrack.workspace_id='" + uid(1), statement)
            self.assertIn("generate_series(1,501)", statement)
            self.assertNotIn("UPDATE jobs", statement)
            self.assertNotIn("UPDATE issue_order_", statement)
        self.assertEqual(sum(op[:2] == ("http", "DELETE") for op in harness.operations), 3)
        self.assertEqual(sum(op[0] == "run" for op in harness.operations), 6)
        self.assertNotIn("synthetic", json.dumps(result))

    def test_fresh_database_required_before_any_mutation_or_worker_control(self):
        harness = PreparationHarness()
        harness.fresh = False
        with patch.object(fixtures, "Client", side_effect=harness.client), self.assertRaises(RuntimeError):
            fixtures.prepare(harness, "synthetic-token", "synthetic-password", suite_deadline=time.monotonic() + 900)
        self.assertEqual(harness.operations, [("http", "GET", "/api/v1/setup/status")])

    def test_input_failure_partial_stop_and_missed_checkpoint_restore_worker(self):
        for failure in ("fail_sql", "fail_stop", "completed_staging", "displace_baseline", "bad_block"):
            harness = PreparationHarness()
            if failure == "completed_staging":
                harness.staged = "501"
            else:
                setattr(harness, failure, True)
            with self.subTest(failure=failure), patch.object(fixtures, "Client", side_effect=harness.client), self.assertRaises(RuntimeError):
                fixtures.prepare(harness, "synthetic-token", "synthetic-password", suite_deadline=time.monotonic() + 900)
            self.assertTrue(harness.running)
            self.assertEqual(harness.operations[-2:], [("start", "worker"), ("inspect", "worker")])

    def test_restoration_failure_cannot_return_a_completed_manifest(self):
        harness = PreparationHarness()
        harness.fail_start = True
        with patch.object(fixtures, "Client", side_effect=harness.client), self.assertRaisesRegex(RuntimeError, "restoration failure"):
            fixtures.prepare(harness, "synthetic-token", "synthetic-password", suite_deadline=time.monotonic() + 900)
        self.assertFalse(harness.running)

    def test_replaced_worker_is_never_started(self):
        harness = PreparationHarness()
        harness.changed_worker = True
        with patch.object(fixtures, "Client", side_effect=harness.client), self.assertRaisesRegex(RuntimeError, "different worker"):
            fixtures.prepare(harness, "synthetic-token", "synthetic-password", suite_deadline=time.monotonic() + 900)
        self.assertFalse(any(op[0] == "start" for op in harness.operations))
        self.assertEqual(harness.sql, [])

    def test_live_resource_failure_precedes_setup_and_all_mutations(self):
        harness = PreparationHarness()
        harness.owned_service = Mock(side_effect=RuntimeError("Mock foreign resource"))
        with patch.object(fixtures, "Client", side_effect=harness.client), self.assertRaises(RuntimeError):
            fixtures.prepare(harness, "synthetic-token", "synthetic-password", suite_deadline=time.monotonic() + 900)
        self.assertEqual(harness.operations, [("http", "GET", "/api/v1/setup/status")])

    def test_deadline_prevents_worker_control_and_sql(self):
        harness = PreparationHarness()
        with patch.object(fixtures, "Client", side_effect=harness.client), patch.object(fixtures.time, "monotonic", side_effect=[0, 301]), self.assertRaises(RuntimeError):
            fixtures.prepare(harness, "synthetic-token", "synthetic-password", suite_deadline=900)
        self.assertFalse(any(op[0] in ("stop", "run", "sql") for op in harness.operations))

    def test_preparation_cap_is_clipped_to_remaining_suite_time_and_cleanup_is_not(self):
        harness = PreparationHarness()
        with patch.object(fixtures.time, "monotonic", return_value=1000), \
                patch.object(fixtures, "Client", side_effect=harness.client) as clients, \
                patch.object(harness, "dc", wraps=harness.dc) as commands:
            result = fixtures.prepare(harness, "synthetic-token", "synthetic-password", suite_deadline=1040)
        self.assertEqual(result, manifest())
        self.assertTrue(all(call.args[1] == 1040 for call in clients.call_args_list))
        self.assertTrue(all(call.kwargs["timeout"] == 40 for call in commands.call_args_list if call.args[0] != "start"))
        self.assertEqual(commands.call_args.kwargs["timeout"], 55)

    def test_expiry_after_worker_stop_still_restores_that_owned_worker(self):
        harness = PreparationHarness()
        clock = [1000]
        command = harness.dc

        def expire_after_stop(*args, **kwargs):
            command(*args, **kwargs)
            if args[0] == "stop":
                clock[0] = 1901

        with patch.object(fixtures.time, "monotonic", side_effect=lambda: clock[0]), \
                patch.object(fixtures, "Client", side_effect=harness.client), \
                patch.object(harness, "dc", side_effect=expire_after_stop) as commands:
            with self.assertRaisesRegex(RuntimeError, "bounded deadline"):
                fixtures.prepare(harness, "synthetic-token", "synthetic-password", suite_deadline=1900)
        self.assertTrue(harness.running)
        self.assertEqual(harness.sql, [])
        self.assertEqual(commands.call_args.args, ("start", "worker"))
        self.assertEqual(commands.call_args.kwargs["timeout"], 55)

    def test_sql_identifiers_and_team_scope_reject_before_execution(self):
        for ids in (("not-an-id", uid(2), uid(3), uid(4)), (uid(1), uid(2), "x'; SELECT 1", uid(4))):
            with self.assertRaises((RuntimeError, ValueError)):
                fixtures.seed_statement(*ids, "ZBO1", "ZZ Browser order chromium")
        with self.assertRaises(RuntimeError):
            fixtures.seed_statement(uid(1), uid(2), uid(3), uid(4), "EXA", "Example Team")

    def test_manifest_rejects_incomplete_secret_extra_duplicate_and_invalid_counts(self):
        changes = [lambda value: value.update(COMPLETE=False), lambda value: value.update(password="forbidden"),
                   lambda value: value["maintenance"].pop("webkit"),
                   lambda value: value["maintenance"]["chromium"].update(cookie="forbidden"),
                   lambda value: value["maintenance"]["chromium"].update(staged_count="0"),
                   lambda value: value["maintenance"]["chromium"].update(staged_count="501"),
                   lambda value: value["maintenance"]["chromium"].update(staged_count=100),
                   lambda value: value["maintenance"]["chromium"].update(total_count="501"),
                   lambda value: value["maintenance"]["chromium"].update(job_id=value["maintenance"]["firefox"]["job_id"])]
        for change in changes:
            value = manifest()
            change(value)
            with self.assertRaises(RuntimeError):
                fixtures.validate_manifest(value)

    def test_parent_http_rejects_off_origin_paths_and_redirects_without_forwarding_auth(self):
        for origin in ("http://example.test:18081", "http://localhost:18081@elsewhere", "http://localhost:18081/"):
            with self.assertRaises(RuntimeError):
                fixtures.Client(origin, time.monotonic() + 20)
        client = fixtures.Client("http://localhost:18081", time.monotonic() + 20)
        client.http = Mock()
        for path in ("https://example.test/api/v1/setup", "//example.test/api/v1/setup", "/api/v1/\r\nCookie: secret"):
            with self.assertRaises(RuntimeError):
                client.request(path)
        client.http.open.assert_not_called()
        response = io.BytesIO(b'{"private":"synthetic-secret"}')
        response.status = 302
        client.http.open.return_value = response
        client.csrf = "synthetic-csrf"
        with self.assertRaisesRegex(RuntimeError, "response withheld") as raised:
            client.request("/api/v1/setup", "POST", {})
        self.assertNotIn("synthetic", str(raised.exception))
        self.assertEqual(client.http.open.call_count, 1)
        self.assertIsNone(fixtures.NoRedirect().redirect_request(None, None, 302, "", {}, "http://example.test"))

    def test_parent_http_disables_environment_proxies(self):
        with patch.object(fixtures.urllib.request, "build_opener"), \
                patch.object(fixtures.urllib.request, "ProxyHandler", wraps=fixtures.urllib.request.ProxyHandler) as proxy:
            fixtures.Client("http://localhost:18081", time.monotonic() + 20)
        proxy.assert_called_once_with({})


if __name__ == "__main__":
    unittest.main()
