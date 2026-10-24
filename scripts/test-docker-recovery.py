#!/usr/bin/env python3
"""Recovery boundary tests; no Docker, database or application is started."""
import argparse
from contextlib import ExitStack, redirect_stderr, redirect_stdout
import copy
from email.message import Message
import hashlib
import http.cookiejar
import importlib.util
import io
import json
import os
from pathlib import Path
import shlex
import stat
import tarfile
import tempfile
import unittest
from unittest.mock import MagicMock, patch


spec = importlib.util.spec_from_file_location("docker_smoke", Path(__file__).with_name("docker-smoke.py"))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)

spec = importlib.util.spec_from_file_location("docker_https_smoke", Path(__file__).with_name("docker-https-smoke.py"))
https = importlib.util.module_from_spec(spec)
spec.loader.exec_module(https)


def archive_bytes(entries=(), *, root_owner=0, root_mode=0o700):
    output = io.BytesIO()
    with tarfile.open(fileobj=output, mode="w") as archive:
        root = tarfile.TarInfo(".")
        root.type = tarfile.DIRTYPE
        root.mode = root_mode
        root.uid = root.gid = root_owner
        archive.addfile(root)
        for entry in entries:
            archive.addfile(entry, io.BytesIO(b"x" * entry.size) if entry.isfile() else None)
    return output.getvalue()


def output_container(project, *, initializer=False):
    selected = project + "-restore" if initializer else project
    mounts = [{"Type": "volume", "Name": selected + "_operator_output", "Destination": "/operator-output", "RW": True}]
    if not initializer:
        mounts.append({"Type": "volume", "Name": selected + "_operator_secrets", "Destination": "/run/secrets", "RW": False})
    return {"Name": "/" + selected + "-operator-output", "Mounts": mounts,
            "Config": {"Labels": {module.RECOVERY_LABEL: project, "com.docker.compose.project": selected,
                                   "com.docker.compose.service": "operator-output-init" if initializer else "operator"}},
            "HostConfig": {"NetworkMode": "none" if initializer else selected + "_database"}}


class RecoveryBoundaries(unittest.TestCase):
    def setUp(self):
        self.project = "opentrack-smoke-123-amd64"
        self.runner = module.Qualification(argparse.Namespace(project=self.project,
            url="http://localhost:18080", output="work/docker-smoke/receipt.json",
            expected_architecture="amd64", compose_file=["work/docker-smoke/offline-compose.yaml"]))
        self.config = {"name": self.project,
            "volumes": {name: {"name": f"{self.project}_{name}"} for name in module.RECOVERY_VOLUMES},
            "networks": {name: {"internal": name != "offline_ingress", "name": f"{self.project}_{name}"}
                         for name in ("database", "web", "offline_ingress")},
            "services": {"app": {"networks": {"database": {}, "web": {}},
                                  "ports": []}, "database": {},
                "offline-relay": {"image": "opentrack-proxy:local", "entrypoint": ["caddy"],
                    "command": module.RELAY_COMMAND.copy(), "user": "10001:10001", "read_only": True,
                    "cap_drop": ["ALL"], "security_opt": ["no-new-privileges:true"],
                    "tmpfs": module.RELAY_TMPFS.copy(), "networks": {"web": {}, "offline_ingress": {}},
                    "ports": [{"host_ip": "127.0.0.1", "published": "18080", "target": 8080}]}}}
        self.operator_config = copy.deepcopy(self.config)
        self.operator_config["volumes"]["operator_output"] = {"name": self.project + "_operator_output"}
        self.operator_config["services"]["operator-output-init"] = {
            "profiles": ["operator"], "image": "opentrack-bootstrap:local",
            "command": ["prepare-output", "--directory", "/operator-output"],
            "user": "0:0", "restart": "no", "read_only": True, "network_mode": "none",
            "cap_drop": ["ALL"], "cap_add": ["CHOWN", "FOWNER", "DAC_OVERRIDE"],
            "security_opt": ["no-new-privileges:true"],
            "volumes": [{"type": "volume", "source": "operator_output", "target": "/operator-output",
                         "volume": {"nocopy": True}}]}
        request_id = "00000000-0000-0000-0000-000000000001"
        self.problem = {"type": "/api/v1/problems/not_found", "title": "Not Found", "status": 404,
            "detail": "This item could not be found", "instance": "urn:uuid:" + request_id,
            "code": "not_found", "request_id": request_id, "retryable": False, "field_errors": []}
        self.problem_headers = {"X-Request-ID": request_id, "Content-Type": "application/problem+json",
                                "Cache-Control": "no-store"}

    def test_compose_excludes_foreign_storage_egress_and_ports(self):
        module.validate_compose(self.config, self.project, 18080)
        optional = copy.deepcopy(self.config)
        optional["volumes"]["operator_output"] = {"name": self.project + "_operator_output"}
        module.validate_compose(optional, self.project, 18080)
        changes = [lambda c: c.update(name="opentrack"),
            lambda c: c["volumes"]["database"].update(name="opentrack_database"),
            lambda c: c["volumes"]["database"].update(external=True),
            lambda c: c["volumes"]["database"].update(driver_opts={"type": "none", "o": "bind", "device": "/tmp"}),
            lambda c: c["volumes"].pop("api_secrets"),
            lambda c: c["volumes"].update(other_output={"name": self.project + "_other_output"}),
            lambda c: c["networks"]["web"].update(internal=False),
            lambda c: c["networks"]["web"].update(external=True),
            lambda c: c["networks"]["web"].update(name="foreign_network"),
            lambda c: c["services"]["app"].update(network_mode="host"),
            lambda c: c["services"]["app"].update(privileged=True),
            lambda c: c["services"]["app"].update(volumes_from=["other"]),
            lambda c: c["services"]["app"].update(volumes=[{"type": "bind", "source": "/tmp"}]),
            lambda c: c["services"]["offline-relay"]["ports"][0].update(host_ip="0.0.0.0"),
            lambda c: c["services"]["offline-relay"].update(command=["file-server"]),
            lambda c: c["services"]["offline-relay"].update(cap_add=["SYS_ADMIN"]),
            lambda c: c["services"]["offline-relay"].update(volumes=[{"type":"volume","source":"api_secrets"}]),
            lambda c: c["services"]["offline-relay"].update(tmpfs=[]),
            lambda c: c["services"]["app"]["networks"].update(offline_ingress={}),
            lambda c: c["services"]["app"].update(ports=[{"host_ip":"127.0.0.1","published":"18080","target":8080}]),
            lambda c: c["services"]["database"].update(ports=[5432])]
        for change in changes:
            config = copy.deepcopy(self.config)
            change(config)
            with self.subTest(change=change), self.assertRaises(RuntimeError):
                module.validate_compose(config, self.project, 18080)

    def test_recreation_and_helpers_never_pull_or_build(self):
        self.assertNotIn("--profile", self.runner.compose)
        with patch.object(self.runner, "command") as command:
            self.runner.dc("up", "--detach", "database")
            arguments = command.call_args.args[0]
            self.assertIn("--no-build", arguments)
            self.assertEqual(arguments[arguments.index("--pull") + 1], "never")
            self.assertEqual(arguments[:len(self.runner.compose)], self.runner.compose)
            self.runner.dc("run", "--rm", "--no-deps", "initialize")
            arguments = command.call_args.args[0]
            self.assertEqual(arguments[arguments.index("--pull") + 1], "never")

    def test_optional_output_initializer_requires_its_private_networkless_profile(self):
        self.assertEqual(len(module.RECOVERY_VOLUMES), 7)
        self.assertNotIn("operator_output", module.RECOVERY_VOLUMES)
        self.assertEqual(module.OPTIONAL_RECOVERY_VOLUMES, ("operator_output",))
        config = copy.deepcopy(self.operator_config)
        module.validate_operator_output_profile(config)
        changes = [lambda c: c["volumes"].pop("operator_output"),
            lambda c: c["services"]["operator-output-init"].update(image="foreign-image"),
            lambda c: c["services"]["operator-output-init"].update(command=["initialize"]),
            lambda c: c["services"]["operator-output-init"].update(user="10001:10001"),
            lambda c: c["services"]["operator-output-init"].update(network_mode="host"),
            lambda c: c["services"]["operator-output-init"].update(networks={"web": {}}),
            lambda c: c["services"]["operator-output-init"].update(read_only=False),
            lambda c: c["services"]["operator-output-init"].update(cap_drop=[]),
            lambda c: c["services"]["operator-output-init"].update(cap_add=["SYS_ADMIN"]),
            lambda c: c["services"]["operator-output-init"].update(security_opt=[]),
            lambda c: c["services"]["operator-output-init"]["volumes"][0].update(source="api_secrets"),
            lambda c: c["services"]["operator-output-init"]["volumes"][0].update(target="/run/secrets")]
        for change in changes:
            current = copy.deepcopy(config)
            change(current)
            with self.subTest(change=change), self.assertRaises(RuntimeError):
                module.validate_operator_output_profile(current)

    def test_workflow_teardown_selects_profiles_for_every_populated_optional_volume(self):
        populated = set(module.OPTIONAL_RECOVERY_VOLUMES) & self.operator_config["volumes"].keys()
        required = {profile for service in self.operator_config["services"].values()
                    if any(mount.get("source") in populated for mount in service.get("volumes", []))
                    for profile in service.get("profiles", [])}
        self.assertTrue(required)
        lines = (Path(__file__).resolve().parents[1] / ".github/workflows/docker.yml").read_text(encoding="utf-8").splitlines()
        step = next(index for index, line in enumerate(lines)
                    if line.strip() == "- name: Remove only this job's disposable containers and volumes")
        command = next(line.split("run:", 1)[1].strip() for line in lines[step + 1:] if line.lstrip().startswith("run:"))
        project = next(line.split(":", 1)[1].strip() for line in lines if line.strip().startswith("COMPOSE_PROJECT_NAME:"))
        run_id, architecture = self.project.removeprefix("opentrack-smoke-").rsplit("-", 1)
        def values(arguments, flag):
            selected = set()
            for index, token in enumerate(arguments):
                if token == flag:
                    selected.add(arguments[index + 1])
                elif token.startswith(flag + "="):
                    selected.add(token.partition("=")[2])
            return selected
        def check(tokens):
            docker = tokens.index("docker")
            assignments = dict(token.split("=", 1) for token in tokens[:docker])
            arguments = tokens[docker:]
            self.assertEqual(arguments[:2], ["docker", "compose"])
            self.assertEqual(arguments.count("down"), 1)
            down = arguments.index("down")
            profiles = values(arguments[:down], "--profile")
            self.assertTrue((required | {"https"}) <= profiles, "Teardown omitted a populated optional fixture profile")
            self.assertEqual(values(arguments[:down], "-f"), {"compose.yaml", "compose.https.yaml"})
            self.assertTrue({"--volumes", "--remove-orphans"} <= set(arguments[down + 1:]))
            self.assertEqual(values(arguments[down + 1:], "--timeout"), {"35"})
            projects = values(arguments[:down], "--project-name") | values(arguments[:down], "-p")
            self.assertLessEqual(len(projects), 1)
            selected = next(iter(projects), assignments.get("COMPOSE_PROJECT_NAME", project))
            selected = selected.replace("${{ github.run_id }}", run_id).replace("${{ matrix.architecture }}", architecture)
            self.assertEqual(module.validate_project(selected), self.project)
        tokens = shlex.split(command)
        check(tokens)
        without_optional = tokens.copy()
        for index in reversed(range(len(without_optional))):
            token = without_optional[index]
            if token == "--profile" and index + 1 < len(without_optional) and without_optional[index + 1] in required:
                del without_optional[index:index + 2]
            elif token.startswith("--profile=") and token.partition("=")[2] in required:
                del without_optional[index]
        with self.assertRaisesRegex(AssertionError, "populated optional fixture profile"):
            check(without_optional)

    def test_optional_output_refuses_unsafe_private_metadata_without_reading_bearer_bytes(self):
        directory = {"st_mode": stat.S_IFDIR | 0o700, "st_uid": 10001, "st_gid": 10001}
        artifact = {"st_mode": stat.S_IFREG | 0o600, "st_uid": 10001, "st_gid": 10001, "st_size": 17}
        changes = [({}, {}, "valid"), ({"st_mode": stat.S_IFDIR | 0o755}, {}, "valid"),
            ({"st_mode": stat.S_IFLNK | 0o700}, {}, "valid"), ({"st_uid": 0}, {}, "valid"),
            ({"st_gid": 0}, {}, "valid"), ({}, {"st_mode": stat.S_IFREG | 0o644}, "valid"),
            ({}, {"st_mode": stat.S_IFLNK | 0o600}, "valid"), ({}, {"st_mode": stat.S_IFDIR | 0o600}, "valid"),
            ({}, {"st_uid": 0}, "valid"), ({}, {"st_gid": 0}, "valid"),
            ({}, {"st_size": 0}, "valid"), ({}, {}, "missing"), ({}, {}, "extra")]
        for index, (directory_change, file_change, inventory) in enumerate(changes):
            root = MagicMock(spec=Path)
            root.lstat.return_value = argparse.Namespace(**{**directory, **directory_change})
            file = root.__truediv__.return_value
            file.name = "recovery-qualification.txt"
            file.lstat.return_value = argparse.Namespace(**{**artifact, **file_change})
            root.iterdir.return_value = [] if inventory == "missing" else [file]
            if inventory == "extra":
                root.iterdir.return_value.append(Path("unexpected"))
            with self.subTest(directory=directory_change, file=file_change, inventory=inventory):
                if index == 0:
                    self.assertIsNone(module.check_operator_output(root))
                else:
                    with self.assertRaises(RuntimeError):
                        module.check_operator_output(root)
                file.open.assert_not_called()
                file.read_bytes.assert_not_called()

    def test_optional_output_seed_validates_private_cli_receipt_and_discards_actor(self):
        user = "00000000-0000-0000-0000-000000000001"
        receipt = {"id": "00000000-0000-0000-0000-000000000002", "user_id": user,
                   "expires_at": "2030-01-01T00:00:00Z", "output_file": "/operator-output/recovery-qualification.txt"}
        private = "synthetic-private-output-marker"
        outcomes = [receipt, {**receipt, "bearer": private}, {**receipt, "user_id": "other-account"},
            {**receipt, "id": private}, {**receipt, "expires_at": ""}, {**receipt, "output_file": private},
            private, "cleanup-failure", RuntimeError(private), OSError(private),
            module.subprocess.TimeoutExpired("qualification", 1, output=private, stderr=private), SystemExit("interrupted")]
        for outcome in outcomes:
            actors = []
            state = {"exists": False}
            def request(runner, path, method="GET", body=None, **kwargs):
                if path.endswith("/invitations"):
                    self.assertEqual(body["role"], "viewer")
                    self.assertEqual(body["team_ids"], [])
                    return {"token": "synthetic-invitation"}, {}
                self.assertEqual(path, "/api/v1/invitations/accept")
                runner.cookies.set_cookie(module.http.cookiejar.Cookie(version=0, name="opentrack_session",
                    value="synthetic-output-account-session", port=None, port_specified=False, domain="localhost.local",
                    domain_specified=False, domain_initial_dot=False, path="/", path_specified=True,
                    secure=False, expires=None, discard=True, comment=None, comment_url=None, rest={}))
                actors.append(runner)
                return {"user": {"id": user}}, {}
            data = receipt if outcome == "cleanup-failure" else outcome
            response = argparse.Namespace(stdout=json.dumps(data) if isinstance(data, dict) else private)
            def cli(*arguments, **kwargs):
                state["exists"] = True
                if isinstance(outcome, BaseException):
                    raise outcome
                return response
            def docker(arguments, **kwargs):
                if arguments[2] == "ls":
                    return argparse.Namespace(stdout="owned\n" if state["exists"] else "")
                if arguments[2] == "inspect":
                    return argparse.Namespace(stdout=json.dumps([output_container(self.project)]))
                self.assertEqual(arguments, ["docker", "container", "rm", "--force", self.project + "-operator-output"])
                self.assertEqual(kwargs["timeout"], 30)
                if outcome == "cleanup-failure":
                    raise OSError(private)
                state["exists"] = False
                return argparse.Namespace(stdout="")
            with self.subTest(outcome=type(outcome).__name__), redirect_stdout(io.StringIO()) as output, \
                    patch.object(module.Qualification, "request", new=request), \
                    patch.object(self.runner, "command", side_effect=docker) as command, \
                    patch.object(self.runner, "dc", side_effect=cli) as issuance:
                if outcome == receipt:
                    self.assertIsNone(self.runner.seed_operator_output("/workspaces/fixture"))
                elif isinstance(outcome, SystemExit):
                    with self.assertRaises(SystemExit):
                        self.runner.seed_operator_output("/workspaces/fixture")
                else:
                    with self.assertRaisesRegex(RuntimeError, "Private operator-output .*failed") as raised:
                        self.runner.seed_operator_output("/workspaces/fixture")
                    self.assertTrue(raised.exception.__suppress_context__)
                    self.assertIsNone(raised.exception.__cause__)
                    self.assertNotIn(private, str(raised.exception))
                arguments = issuance.call_args.args
                self.assertEqual(arguments[arguments.index("--user") + 1], user)
                self.assertEqual(arguments[arguments.index("--output-file") + 1], receipt["output_file"])
                self.assertIn("--no-deps", arguments)
                self.assertNotIn("--rm", arguments)
                self.assertEqual(arguments[arguments.index("--name") + 1], self.project + "-operator-output")
                self.assertEqual(arguments[arguments.index("--label") + 1], module.RECOVERY_LABEL + "=" + self.project)
                self.assertEqual(sum(call.args[0][2] == "rm" for call in command.call_args_list), 1)
                self.assertEqual(state["exists"], outcome == "cleanup-failure")
                self.assertEqual(len(actors), 1)
                self.assertEqual(list(actors[0].cookies), [])
                self.assertEqual(output.getvalue(), "")

    def test_main_interrupt_handler_unwinds_without_publishing_private_diagnostics(self):
        for interrupt in ("term", "keyboard"):
            cleaned = []
            def run(runner):
                try:
                    if interrupt == "term":
                        registration.call_args.args[1](module.signal.SIGTERM, None)
                    else:
                        raise KeyboardInterrupt("synthetic-private-diagnostic")
                finally:
                    cleaned.append(True)
            with self.subTest(interrupt=interrupt), redirect_stdout(io.StringIO()) as output, \
                    patch("sys.argv", ["qualification", "--project", self.project, "--expected-architecture", "amd64"]), \
                    patch.object(module.signal, "signal") as registration, \
                    patch.object(module.Qualification, "run", new=run):
                with self.assertRaises(SystemExit) as raised:
                    module.main()
                self.assertEqual(str(raised.exception), "Docker qualification interrupted")
                self.assertEqual(registration.call_args.args[0], module.signal.SIGTERM)
                self.assertEqual(cleaned, [True])
                self.assertEqual(output.getvalue(), "")

    def test_optional_output_seed_refuses_preexisting_names_before_account_or_cli_mutation(self):
        for foreign in (False, True):
            container = output_container(self.project)
            if foreign:
                container["Config"]["Labels"][module.RECOVERY_LABEL] = "foreign"
            def docker(arguments, **kwargs):
                if arguments[2] == "ls":
                    return argparse.Namespace(stdout="existing\n")
                self.assertEqual(arguments, ["docker", "container", "inspect", self.project + "-operator-output"])
                return argparse.Namespace(stdout=json.dumps([container]))
            with self.subTest(foreign=foreign), patch.object(self.runner, "command", side_effect=docker) as command, \
                    patch.object(module.Qualification, "request") as request, patch.object(self.runner, "dc") as issuance:
                with self.assertRaisesRegex(RuntimeError, "Private operator-output qualification failed"):
                    self.runner.seed_operator_output("/workspaces/fixture")
                request.assert_not_called()
                issuance.assert_not_called()
                self.assertFalse(any(call.args[0][2] == "rm" for call in command.call_args_list))

    def test_optional_output_cleanup_requires_exact_container_authority(self):
        for initializer in (False, True):
            changes = [None, lambda c: c.update(Name="/foreign"),
                lambda c: c["Config"]["Labels"].update({module.RECOVERY_LABEL: "foreign"}),
                lambda c: c["Config"]["Labels"].update({"com.docker.compose.project": "foreign"}),
                lambda c: c["Config"]["Labels"].update({"com.docker.compose.service": "foreign"}),
                lambda c: c["HostConfig"].update(NetworkMode="host"),
                lambda c: c["Mounts"][0].update(Type="bind"),
                lambda c: c["Mounts"][0].update(Name="foreign-volume"),
                lambda c: c["Mounts"][0].update(Destination="/foreign"),
                lambda c: c["Mounts"][0].update(RW=False)]
            if not initializer:
                changes.append(lambda c: c["Mounts"][1].update(RW=True))
            for change in changes:
                container = output_container(self.project, initializer=initializer)
                if change:
                    change(container)
                state = {"exists": True}
                name = (self.project + "-restore" if initializer else self.project) + "-operator-output"
                def docker(arguments, **kwargs):
                    if arguments[2] == "ls":
                        self.assertEqual(arguments[-1], "name=^/" + name + "$")
                        return argparse.Namespace(stdout="owned\n" if state["exists"] else "")
                    if arguments[2] == "inspect":
                        self.assertEqual(arguments[-1], name)
                        return argparse.Namespace(stdout=json.dumps([container]))
                    self.assertEqual(arguments, ["docker", "container", "rm", "--force", name])
                    self.assertEqual(kwargs["timeout"], 30)
                    state["exists"] = False
                    return argparse.Namespace(stdout="")
                with self.subTest(initializer=initializer, change=change), \
                        patch.object(self.runner, "command", side_effect=docker) as command:
                    if change is None:
                        self.assertIsNone(self.runner.cleanup_operator_output(initializer=initializer))
                        self.assertFalse(state["exists"])
                        self.assertEqual(command.call_args.args[0][2], "ls")
                    else:
                        with self.assertRaisesRegex(RuntimeError, "cleanup failed; details withheld"):
                            self.runner.cleanup_operator_output(initializer=initializer)
                        self.assertTrue(state["exists"])
                        self.assertFalse(any(call.args[0][2] == "rm" for call in command.call_args_list))

    def test_optional_output_cleanup_refuses_unconfirmed_absence_and_sanitizes_failures(self):
        private = "synthetic-private-docker-diagnostic"
        for initializer in (False, True):
            for failure in ("listing", "remove", "remains"):
                def docker(arguments, **kwargs):
                    if arguments[2] == "ls":
                        if failure == "listing":
                            raise RuntimeError(private)
                        return argparse.Namespace(stdout="owned\n")
                    if arguments[2] == "inspect":
                        return argparse.Namespace(stdout=json.dumps([output_container(self.project, initializer=initializer)]))
                    self.assertEqual(arguments[2], "rm")
                    if failure == "remove":
                        raise module.subprocess.TimeoutExpired("docker", 30, output=private, stderr=private)
                    return argparse.Namespace(stdout="")
                with self.subTest(initializer=initializer, failure=failure), \
                        patch.object(self.runner, "command", side_effect=docker):
                    with self.assertRaisesRegex(RuntimeError, "cleanup failed; details withheld") as raised:
                        self.runner.cleanup_operator_output(initializer=initializer)
                    self.assertTrue(raised.exception.__suppress_context__)
                    self.assertIsNone(raised.exception.__cause__)
                    self.assertNotIn(private, str(raised.exception))

    def test_http_problem_is_decoded_without_treating_file_bytes_as_json(self):
        problem = {"status": 503, "code": "dependency_unavailable"}
        for content_type in ("application/problem+json", "application/problem+json; charset=utf-8"):
            reply = module.urllib.error.HTTPError(self.runner.url, 503, "Unavailable",
                {"Content-Type": content_type}, io.BytesIO(json.dumps(problem).encode()))
            with self.subTest(content_type=content_type), patch.object(self.runner.http, "open", side_effect=reply):
                body, _ = self.runner.request("/fixture/content", expected=(503,))
                self.assertEqual(body, problem)
        reply = module.urllib.error.HTTPError(self.runner.url, 503, "Unavailable",
            {"Content-Type": "application/octet-stream"}, io.BytesIO(b"file bytes"))
        with patch.object(self.runner.http, "open", side_effect=reply):
            body, _ = self.runner.request("/fixture/content", expected=(503,))
            self.assertEqual(body, b"file bytes")

    def test_concealed_response_accepts_only_the_generic_not_found_envelope(self):
        for content_type in ("application/problem+json", "application/problem+json; charset=utf-8"):
            headers = {**self.problem_headers, "Content-Type": content_type, "Content-Length": "256"}
            with self.subTest(content_type=content_type):
                module.require_concealed(self.problem, headers)
        changes = [{"status": 401, "code": "unauthenticated", "detail": "Sign in to continue"},
            {"status": 200}, {"status": 206}, {"status": 304},
            {"type": "/api/v1/problems/private_resource"}, {"title": "Private fixture"},
            {"detail": "Private fixture"}, {"original_name": "private-fixture.txt"},
            {"storage_key": "private-storage-marker"}, {"retryable": True},
            {"field_errors": [{"field": "private-fixture"}]},
            {"request_id": "00000000-0000-0000-0000-000000000002"},
            {"instance": "urn:uuid:00000000-0000-0000-0000-000000000002"}]
        for change in changes:
            with self.subTest(change=change), self.assertRaises(RuntimeError):
                module.require_concealed({**self.problem, **change}, self.problem_headers)
        for field in self.problem:
            problem = {key: value for key, value in self.problem.items() if key != field}
            with self.subTest(missing=field), self.assertRaises(RuntimeError):
                module.require_concealed(problem, self.problem_headers)
        for problem in (b"private file bytes", None, []):
            with self.subTest(problem=problem), self.assertRaises(RuntimeError):
                module.require_concealed(problem, self.problem_headers)

    def test_concealed_response_rejects_cacheable_or_metadata_bearing_headers(self):
        changes = [{"Cache-Control": "public, max-age=3600"}, {"Cache-Control": "no-cache"},
            {"Content-Type": "application/json"}, {"Content-Type": "application/octet-stream"},
            {"X-Request-ID": "not-a-request-id"}]
        changes.extend({name: value} for name in
            ("ETag", "Last-Modified", "Content-Disposition", "Content-Range", "Accept-Ranges", "Location")
            for value in ("private-fixture-marker", ""))
        for change in changes:
            with self.subTest(change=change), self.assertRaises(RuntimeError):
                module.require_concealed(self.problem, {**self.problem_headers, **change})
        for field in self.problem_headers:
            headers = {key: value for key, value in self.problem_headers.items() if key != field}
            with self.subTest(missing=field), self.assertRaises(RuntimeError):
                module.require_concealed(self.problem, headers)

    def test_private_move_validates_preview_and_preserves_saved_resources(self):
        base, source, destination = "/api/v1/workspaces/fixture", "private-a", "private-b"
        issue, document, attachment = (base + "/issues/moved-id", base + "/documents/moved-document",
                                       base + "/attachments/file-id")
        preview = {"issue_id": "moved-id", "source_team_id": source,
            "destination": {"id": destination, "private": True, "can_move_issue": True},
            "audience_change": "between_private_teams", "blockers": [],
            "destination_default_status_id": "preview-status", "preview_token": "actual-preview-token",
            "issue_etag": '"actual-preview-etag:7"'}
        invalid = [dict(preview, issue_id="other-issue"), dict(preview, source_team_id="other-team"),
            dict(preview, destination={**preview["destination"], "id": "other-team"}),
            dict(preview, destination={**preview["destination"], "private": False}),
            dict(preview, destination={**preview["destination"], "can_move_issue": False}),
            dict(preview, audience_change="private_to_public"), dict(preview, blockers=["unresolved"])]
        for candidate in [preview, *invalid]:
            state = {}
            reader = module.Qualification(argparse.Namespace(project=self.project, url=self.runner.url,
                output=str(self.runner.output), expected_architecture="amd64", compose_file=self.runner.compose_files))
            def owner_request(path, method="GET", body=None, **kwargs):
                if path == base + "/issues" and method == "POST":
                    identity = "moved-id" if "control" in state else "control-id"
                    state["control" if identity == "control-id" else "issue"] = copy.deepcopy(body)
                    return {"id": identity}, {}
                if path == base + "/issues/control-id/document":
                    return {"id": "control-document"}, {}
                if path == base + "/attachments" and method == "POST":
                    state["file"] = copy.deepcopy(body)
                    return {"id": "file-id"}, {"ETag": '"reservation:1"'}
                if path == attachment + "/content" and method == "PUT":
                    state["bytes"] = kwargs["raw"]
                    return {}, {}
                if path == issue + "/document":
                    return {"id": "moved-document"}, {}
                if path == issue + "/move-preview?destination_team_id=" + destination:
                    return copy.deepcopy(candidate), {}
                if path == issue + "/move" and method == "POST":
                    return {"id": "moved-id", "team_id": destination}, {}
                raise AssertionError("Unexpected fixture request")
            def reader_request(path, **kwargs):
                if path == issue:
                    return {"id": "moved-id", "team_id": source, "title": state["issue"]["title"]}, {"ETag": '"saved-issue:4"'}
                if path in (issue + "/document", document):
                    return {"id": "moved-document", "body": state["issue"]["document"]["body"]}, {"ETag": '"saved-document:2"'}
                if path == attachment:
                    return {"id": "file-id", "sha256": state["file"]["sha256"]}, {"ETag": '"saved-file:3"'}
                if path == attachment + "/content":
                    return state["bytes"], {}
                raise AssertionError("Unexpected reader request")
            with self.subTest(preview=candidate), redirect_stdout(io.StringIO()), \
                    patch.object(self.runner, "request", side_effect=owner_request) as owner, \
                    patch.object(reader, "request", side_effect=reader_request):
                if candidate != preview:
                    with self.assertRaisesRegex(RuntimeError, "Private move preview must match"):
                        self.runner.seed_private_move(reader, base, source, destination)
                    self.assertFalse(any(call.args[:2] == (issue + "/move", "POST")
                                         for call in owner.call_args_list))
                    continue
                fixture = self.runner.seed_private_move(reader, base, source, destination)
                moves = [call for call in owner.call_args_list if call.args[:2] == (issue + "/move", "POST")]
                self.assertEqual(len(moves), 1)
                self.assertEqual(moves[0].args[2], {"destination_team_id": destination,
                    "status_id": preview["destination_default_status_id"], "assignee_id": None,
                    "label_ids": [], "preview_token": preview["preview_token"]})
                self.assertEqual(moves[0].kwargs["headers"], {"If-Match": preview["issue_etag"],
                                                          "Idempotency-Key": "docker-private-move"})
                self.assertEqual((fixture["issue"], fixture["document"], fixture["attachment"]),
                                 (issue, document, attachment))
                self.assertEqual(fixture["validators"], {issue: '"saved-issue:4"',
                    issue + "/document": '"saved-document:2"', document: '"saved-document:2"',
                    attachment: '"saved-file:3"', attachment + "/content": '"saved-file:3"'})
                self.assertEqual(fixture["team"], destination)
                self.assertEqual(fixture["source_team"], source)

    def test_private_move_rejects_changed_source_control_before_owner_or_denial_reads(self):
        reader = module.Qualification(argparse.Namespace(project=self.project, url=self.runner.url,
            output=str(self.runner.output), expected_architecture="amd64", compose_file=self.runner.compose_files))
        control = {"id": "control-id", "team_id": "private-a", "title": "Unchanged private-A restore control"}
        document = {"id": "control-document", "body": {"type": "doc", "content": []}}
        fixture = {"reader": reader, "control": "/issues/control-id", "source_team": "private-a",
                   "control_document": document["id"], "control_body": document["body"]}
        changed = [(dict(control, id="other-issue"), document),
            (dict(control, team_id="private-b"), document), (dict(control, title="Changed control"), document),
            (control, dict(document, id="other-document")),
            (control, dict(document, body={"type": "doc", "content": [{"type": "paragraph"}]}))]
        for issue_reply, document_reply in changed:
            with self.subTest(issue=issue_reply, document=document_reply), \
                    patch.object(reader, "request", side_effect=[(issue_reply, {}), (document_reply, {})]) as reads, \
                    patch.object(self.runner, "request", side_effect=AssertionError("Owner read preceded control validation")) as owner:
                with self.assertRaisesRegex(RuntimeError, "reader must retain unchanged private-A"):
                    self.runner.check_private_move(fixture)
                self.assertEqual([call.args[0] for call in reads.call_args_list],
                                 [fixture["control"], fixture["control"] + "/document"])
                owner.assert_not_called()

    def test_private_denials_bind_conditional_requests_to_every_saved_resource(self):
        fixture = {"issue": "/issues/saved-issue", "document": "/documents/saved-document",
                   "attachment": "/attachments/saved-file", "markers": ("private-byte-marker",)}
        paths = (fixture["issue"], fixture["issue"] + "/document", fixture["document"],
                 fixture["attachment"], fixture["attachment"] + "/content")
        fixture["validators"] = {path: f'"saved-validator:{index}"' for index, path in enumerate(paths)}
        with patch.object(self.runner, "request", return_value=(self.problem, self.problem_headers)) as reads:
            self.runner.check_private_denials(fixture)
        for path in paths:
            calls = [call for call in reads.call_args_list if call.args == (path,)]
            self.assertTrue(all(call.kwargs["expected"] == (404,) for call in calls))
            fields = [call.kwargs["headers"] for call in calls]
            self.assertIn({}, fields)
            self.assertIn({"If-None-Match": fixture["validators"][path]}, fields)
            self.assertIn({"If-None-Match": "*"}, fields)
            if path.startswith(fixture["attachment"]):
                self.assertIn({"Range": "bytes=0-15", "If-None-Match": fixture["validators"][path]}, fields)

    def test_actual_network_state_must_match_internal_project_inventory(self):
        config = copy.deepcopy(self.config)
        config["services"]["app"]["networks"] = {"web": {}}
        container = {"Config": {"Labels": {"com.docker.compose.project": self.project,
                                              "com.docker.compose.service": "app"}},
                     "HostConfig": {"NetworkMode": self.project + "_web"},
                     "NetworkSettings": {"Networks": {self.project + "_web": {"NetworkID": "network"}}}}
        network = {"Name": self.project + "_web", "Internal": True,
                   "Labels": {"com.docker.compose.project": self.project}}
        def result(value):
            return argparse.Namespace(stdout=json.dumps(value))
        with patch.object(self.runner, "command", side_effect=[argparse.Namespace(stdout="container"),
                result([container]), result([network])]):
            self.runner.verify_offline_runtime(config)
        for change in (lambda c, n: n.update(Internal=False),
                       lambda c, n: n["Labels"].update({"com.docker.compose.project": "foreign"}),
                       lambda c, n: c["HostConfig"].update(NetworkMode="host"),
                       lambda c, n: c["NetworkSettings"]["Networks"].update(foreign={"NetworkID": "other"})):
            current, actual = copy.deepcopy(container), copy.deepcopy(network)
            change(current, actual)
            with patch.object(self.runner, "command", side_effect=[argparse.Namespace(stdout="container"),
                    result([current]), result([actual])]), self.assertRaises(RuntimeError):
                self.runner.verify_offline_runtime(config)

    def test_actual_relay_keeps_fixed_upstream_and_no_persistent_mounts(self):
        relay = self.config["services"]["offline-relay"]
        container = {"Config": {"Labels": {"com.docker.compose.project": self.project,
                                              "com.docker.compose.service": "offline-relay"},
                                 "Image": relay["image"], "Entrypoint": ["caddy"],
                                 "Cmd": module.RELAY_COMMAND.copy(), "User": "10001:10001"},
                     "HostConfig": {"NetworkMode": self.project + "_web", "ReadonlyRootfs": True,
                                    "CapDrop": ["ALL"], "CapAdd": None},
                     "Mounts": [{"Type":"tmpfs"},{"Type":"tmpfs"}],
                     "NetworkSettings": {"Networks": {f"{self.project}_{name}": {"NetworkID":name}
                                                        for name in ("web","offline_ingress")}}}
        def command(arguments, **kwargs):
            if arguments[1] == "ps":
                return argparse.Namespace(stdout="container")
            if arguments[1] == "inspect":
                return argparse.Namespace(stdout=json.dumps([container]))
            name = arguments[-1]
            return argparse.Namespace(stdout=json.dumps([{"Name": f"{self.project}_{name}",
                "Internal":name != "offline_ingress", "Labels":{"com.docker.compose.project":self.project}}]))
        with patch.object(self.runner, "command", side_effect=command):
            self.runner.verify_offline_runtime(self.config)
            container["Mounts"].append({"Type":"volume","Name":self.project + "_api_secrets"})
            with self.assertRaises(RuntimeError):
                self.runner.verify_offline_runtime(self.config)

    def test_source_restart_is_attempted_even_when_cleanup_fails(self):
        starts = []
        readers = []
        operations = []
        config_reads = []
        failure = "snapshot"
        def seed(runner, reader, base):
            reader.cookies.set_cookie(module.http.cookiejar.Cookie(version=0, name="opentrack_session",
                value="synthetic-reader-session", port=None, port_specified=False, domain="localhost.local",
                domain_specified=False, domain_initial_dot=False, path="/", path_specified=True,
                secure=False, expires=None, discard=True, comment=None, comment_url=None, rest={}))
            readers.append(reader)
            return {}
        def seed_output(base):
            self.assertEqual(config_reads, [(self.project, True), (self.project + "-restore", True)])
        def dc(runner, *arguments, **kwargs):
            selected = arguments[:3] == ("--profile", "operator", "config")
            if arguments[0] == "config" or selected:
                config_reads.append((runner.project, selected))
                config = copy.deepcopy(self.operator_config if selected else self.config)
                config["name"] = runner.project
                for name, volume in config["volumes"].items():
                    volume["name"] = f"{runner.project}_{name}"
                for name, network in config["networks"].items():
                    network["name"] = f"{runner.project}_{name}"
                return argparse.Namespace(stdout=json.dumps(config))
            if arguments[0] == "up":
                self.assertNotIn("--profile", arguments)
                starts.append(runner.project)
            if arguments[0] == "run" and "operator-output-init" in arguments:
                self.assertIn("--no-deps", arguments)
                self.assertNotIn("--rm", arguments)
                self.assertEqual(arguments[arguments.index("--name") + 1], runner.project + "-operator-output")
                self.assertEqual(arguments[arguments.index("--label") + 1], module.RECOVERY_LABEL + "=" + self.project)
                self.assertEqual([volume for action, volume in operations if action == "restore"],
                                 list(module.RECOVERY_VOLUMES + module.OPTIONAL_RECOVERY_VOLUMES))
                operations.append(("initialize", "operator_output"))
                if failure == "initializer-timeout":
                    raise module.subprocess.TimeoutExpired("initializer", 1, output="private fixture diagnostic")
            return argparse.Namespace(stdout="container" if arguments[0] == "ps" else "")
        def volume_task(action, project, volume, *args):
            operations.append((action, volume))
            if failure == "snapshot" and action == "snapshot":
                raise RuntimeError("snapshot failure")
            return argparse.Namespace(returncode=1 if action == "verify" and volume == "operator_output" else 0)
        def command(arguments, **kwargs):
            states = [{"State": {"Running": False, "ExitCode": 0}}]
            return argparse.Namespace(stdout=json.dumps(states) if arguments[1] == "inspect" else "")
        identifier = "00000000-0000-0000-0000-000000000001"
        with patch.object(module.Qualification, "dc", new=dc), \
                patch.object(module.Qualification, "seed_private_recovery", new=seed), \
                patch.object(module.Qualification, "check_private_revocation"), \
                patch.object(self.runner, "measure_https_source", return_value={}), \
                patch.object(self.runner, "seed_operator_output", side_effect=seed_output) as output_seed, \
                patch.object(self.runner, "operator_output_container", return_value=None), \
                patch.object(self.runner, "cleanup_operator_output") as output_cleanup, \
                patch.object(self.runner, "record"), \
                patch.object(self.runner, "command", side_effect=command), \
                patch.object(self.runner, "volume_info", return_value=None), \
                patch.object(self.runner, "container", return_value={"Image": "fixture"}), \
                patch.object(self.runner, "database_fingerprint", return_value=b"fixture"), \
                patch.object(self.runner, "setup_token", return_value="0" * 64), \
                patch.object(self.runner, "sql", return_value=identifier), \
                patch.object(self.runner, "volume_task", side_effect=volume_task), \
                patch.object(self.runner, "cleanup_recovery", side_effect=RuntimeError("cleanup failure")), \
                patch.object(self.runner, "ready") as ready:
            for failure, reason in (("snapshot", "snapshot failure"),
                    ("initializer", "Private restored output changed during its networkless initializer"),
                    ("initializer-timeout", "Private restored output initialization failed; details withheld")):
                starts.clear()
                readers.clear()
                operations.clear()
                config_reads.clear()
                ready.reset_mock()
                output_cleanup.reset_mock()
                output_seed.reset_mock()
                with self.subTest(failure=failure), self.assertRaisesRegex(RuntimeError, "cleanup failure") as raised:
                    self.runner.cold_recovery("fixture@example.test", "fixture", "/issues/fixture",
                                              "/attachments/" + identifier, {}, "fixture")
                self.assertEqual(str(raised.exception.__context__), reason)
                output_seed.assert_called_once()
                self.assertEqual(starts, [self.project])
                ready.assert_called_once()
                self.assertEqual(len(readers), 1)
                self.assertEqual(list(readers[0].cookies), [])
                if failure == "initializer":
                    self.assertEqual(operations[-2:], [("initialize", "operator_output"), ("verify", "operator_output")])
                if failure == "snapshot":
                    output_cleanup.assert_not_called()
                else:
                    output_cleanup.assert_called_once_with(initializer=True)
                if failure == "initializer-timeout":
                    self.assertNotIn(("verify", "operator_output"), operations)

    def test_archive_rejects_paths_links_special_files_and_authority(self):
        bad = []
        for name in ("../escape", "/escape", "C:/escape", "a\\escape", "missing/child"):
            bad.append(tarfile.TarInfo(name))
        for kind in (tarfile.SYMTYPE, tarfile.LNKTYPE, tarfile.FIFOTYPE, tarfile.CHRTYPE):
            entry = tarfile.TarInfo("linked")
            entry.type, entry.linkname = kind, "outside"
            bad.append(entry)
        entry = tarfile.TarInfo("setuid")
        entry.mode = 0o4700
        bad.append(entry)
        entry = tarfile.TarInfo("foreign-owner")
        entry.uid = 1234
        bad.append(entry)
        for entry in bad:
            with self.subTest(name=entry.name, kind=entry.type), self.assertRaises(RuntimeError):
                with tarfile.open(fileobj=io.BytesIO(archive_bytes([entry]))) as archive:
                    module.safe_members(archive)
        for entries in ([tarfile.TarInfo("same"), tarfile.TarInfo("./same")],
                        [tarfile.TarInfo("file"), tarfile.TarInfo("file/child")]):
            with self.assertRaises(RuntimeError):
                with tarfile.open(fileobj=io.BytesIO(archive_bytes(entries))) as archive:
                    module.safe_members(archive)

    def test_missing_corrupt_and_malformed_backup_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            archive, checksum = root / "snapshot.tar", root / "snapshot.sha256"
            data = archive_bytes([tarfile.TarInfo("object")])
            archive.write_bytes(data)
            checksum.write_text(hashlib.sha256(data).hexdigest() + "\n", encoding="ascii")
            with module.checked_archive(archive, checksum):
                pass
            archive.write_bytes(bytes([data[0] ^ 1]) + data[1:])
            with self.assertRaises(RuntimeError):
                module.checked_archive(archive, checksum)
            archive.unlink()
            with self.assertRaises(RuntimeError):
                module.checked_archive(archive, checksum)
            archive.write_bytes(data)
            checksum.write_text("invalid\n", encoding="ascii")
            with self.assertRaises(RuntimeError):
                module.checked_archive(archive, checksum)

    def test_optional_output_archive_refuses_private_metadata_before_extraction(self):
        file = tarfile.TarInfo("recovery-qualification.txt")
        file.uid = file.gid = 10001
        file.mode, file.size = 0o600, 3
        changes = [(10001, 0o700, []), (0, 0o700, [file]), (10001, 0o755, [file])]
        for attribute, value in (("mode", 0o644), ("uid", 0), ("gid", 0), ("size", 0),
                                 ("name", "other-output.txt"), ("type", tarfile.SYMTYPE)):
            changed = copy.copy(file)
            setattr(changed, attribute, value)
            changes.append((10001, 0o700, [changed]))
        extra = copy.copy(file)
        extra.name = "unexpected-output.txt"
        changes.append((10001, 0o700, [file, extra]))
        for owner, mode, entries in changes:
            with self.subTest(owner=owner, mode=mode, entries=entries), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                target, backup = root / "volume", root / "backup"
                target.mkdir()
                backup.mkdir()
                data = archive_bytes(entries, root_owner=owner, root_mode=mode)
                (backup / "operator_output.tar").write_bytes(data)
                (backup / "operator_output.sha256").write_text(hashlib.sha256(data).hexdigest(), encoding="ascii")
                def mapped_path(value):
                    return {"/volume": target, "/backup": backup}.get(value, Path(value))
                with patch.object(module, "Path", side_effect=mapped_path), \
                        patch.object(tarfile.TarFile, "extractall") as extract, self.assertRaises(RuntimeError):
                    module.volume_operation("restore", "operator_output")
                extract.assert_not_called()
                self.assertEqual(list(target.iterdir()), [])

    def test_valid_archive_cannot_overwrite_nonempty_target(self):
        file = tarfile.TarInfo("recovery-qualification.txt")
        file.uid = file.gid = 10001
        file.mode, file.size = 0o600, 3
        for volume, data in (("blobs", archive_bytes()),
                ("operator_output", archive_bytes([file], root_owner=10001))):
            with self.subTest(volume=volume), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                target, backup = root / "volume", root / "backup"
                target.mkdir()
                backup.mkdir()
                existing = target / "keep"
                existing.write_bytes(b"preserve")
                (backup / f"{volume}.tar").write_bytes(data)
                (backup / f"{volume}.sha256").write_text(hashlib.sha256(data).hexdigest(), encoding="ascii")
                def mapped_path(value):
                    return {"/volume": target, "/backup": backup}.get(value, Path(value))
                with patch.object(module, "Path", side_effect=mapped_path), \
                        self.assertRaisesRegex(RuntimeError, "nonempty target volume"):
                    module.volume_operation("restore", volume)
                self.assertEqual(existing.read_bytes(), b"preserve")

    def test_volume_task_refuses_other_projects_and_source_mutations(self):
        with patch.object(self.runner, "command") as command:
            for project, volume, backup in (("opentrack", "blobs", self.project + "_recovery_backup"),
                    (self.project + "-restore", "foreign", self.project + "_recovery_backup"),
                    (self.project + "-restore", "blobs", "foreign_backup")):
                with self.assertRaises(RuntimeError):
                    self.runner.volume_task("restore", project, volume, backup, "image")
            command.assert_not_called()

    def test_pinned_helper_has_no_network_and_readonly_source(self):
        def information(name):
            if name.endswith("_recovery_backup"):
                return {"Labels": {module.RECOVERY_LABEL: self.project}}
            return {"Labels": {"com.docker.compose.project": self.project,
                               "com.docker.compose.volume": "blobs"}}
        with patch.object(self.runner, "volume_info", side_effect=information), patch.object(self.runner, "command") as command:
            command.return_value = argparse.Namespace(returncode=0, stderr="")
            with self.assertRaises(RuntimeError):
                self.runner.volume_task("restore", self.project, "blobs", self.project + "_recovery_backup", "sha256:fixture")
            command.assert_not_called()
            self.runner.volume_task("snapshot", self.project, "blobs", self.project + "_recovery_backup", "sha256:fixture")
            arguments = command.call_args.args[0]
            self.assertEqual(arguments[arguments.index("--pull") + 1], "never")
            self.assertEqual(arguments[arguments.index("--network") + 1], "none")
            self.assertIn(f"type=volume,src={self.project}_blobs,dst=/volume,readonly", arguments)
            self.assertIn("sha256:fixture", arguments)
            compile(arguments[arguments.index("-c") + 1], "recovery-helper", "exec")

    def test_optional_output_check_is_readonly_and_cannot_select_other_volumes(self):
        backup = self.project + "_recovery_backup"
        def information(name):
            if name == backup:
                return {"Labels": {module.RECOVERY_LABEL: self.project}}
            project = self.project + "-restore" if name.startswith(self.project + "-restore_") else self.project
            return {"Labels": {"com.docker.compose.project": project,
                               "com.docker.compose.volume": name.removeprefix(project + "_")}}
        with patch.object(self.runner, "volume_info", side_effect=information), \
                patch.object(self.runner, "command", return_value=argparse.Namespace(returncode=0, stderr="")) as command:
            self.runner.volume_task("check-output", self.project, "operator_output", backup, "sha256:fixture")
            arguments = command.call_args.args[0]
            self.assertIn(f"type=volume,src={self.project}_operator_output,dst=/volume,readonly", arguments)
            self.assertEqual(arguments[arguments.index("--network") + 1], "none")
            command.reset_mock()
            for project in (self.project, self.project + "-restore"):
                for volume in (*module.RECOVERY_VOLUMES, "other_output"):
                    with self.subTest(project=project, volume=volume), self.assertRaises(RuntimeError):
                        self.runner.volume_task("check-output", project, volume, backup, "sha256:fixture")
            command.assert_not_called()

    def test_helper_failure_identifies_action_and_volume_without_raw_stderr(self):
        def information(name):
            if name.endswith("_recovery_backup"):
                return {"Labels": {module.RECOVERY_LABEL: self.project}}
            return {"Labels": {"com.docker.compose.project": self.project + "-restore",
                               "com.docker.compose.volume": "database"}}
        with patch.object(self.runner, "volume_info", side_effect=information), \
                patch.object(self.runner, "command", return_value=argparse.Namespace(returncode=1,
                    stderr="untrusted diagnostic\nRecovery filesystem failure: errno=1\n")):
            with self.assertRaisesRegex(RuntimeError, "restore for database rejected: Recovery filesystem failure: errno=1"):
                self.runner.volume_task("restore", self.project + "-restore", "database",
                                        self.project + "_recovery_backup", "sha256:fixture")
        with patch.object(self.runner, "volume_info", side_effect=information), \
                patch.object(self.runner, "command", return_value=argparse.Namespace(returncode=1,
                    stderr="untrusted diagnostic")):
            with self.assertRaisesRegex(RuntimeError, "helper command failed") as raised:
                self.runner.volume_task("restore", self.project + "-restore", "database",
                                        self.project + "_recovery_backup", "sha256:fixture")
            self.assertNotIn("untrusted diagnostic", str(raised.exception))

    def test_cleanup_requires_exact_ownership_before_deletion(self):
        target = self.project + "-restore"
        config = copy.deepcopy(self.config)
        selected_config = copy.deepcopy(self.operator_config)
        for model in (config, selected_config):
            model["name"] = target
            for name, volume in model["volumes"].items():
                volume["name"] = f"{target}_{name}"
            for name, network in model["networks"].items():
                network["name"] = f"{target}_{name}"
        import json
        def result(value="", code=0):
            return argparse.Namespace(stdout=value, returncode=code)
        profile_config = ["--profile", "operator", "config", "--format", "json"]
        def execute(arguments, **kwargs):
            if arguments[-3:] == ["config", "--format", "json"]:
                return result(json.dumps(selected_config if arguments[-5:] == profile_config else config))
            return next(responses)
        owned_backup = json.dumps([{"Name": self.project + "_recovery_backup",
                                    "Labels": {module.RECOVERY_LABEL: self.project}}])
        foreign_volume = json.dumps([{"Name": target + "_blobs", "Labels": {}}])
        responses = iter([result(owned_backup), result(target + "_blobs\n"), result(foreign_volume)])
        with patch.object(self.runner, "cleanup_operator_output") as oneoffs, \
                patch.object(module.Qualification, "command", side_effect=execute) as command:
            with self.assertRaises(RuntimeError):
                self.runner.cleanup_recovery(remove_backup=True)
            self.assertFalse(any("rm" in call.args[0] or "down" in call.args[0] for call in command.call_args_list))
            self.assertEqual(command.call_args_list[0].args[0][-5:], profile_config)
            self.assertEqual(oneoffs.call_count, 2)
            oneoffs.assert_any_call()
            oneoffs.assert_any_call(initializer=True)
        for volume in ("blobs", "operator_output"):
            owned_volume = json.dumps([{"Name": target + "_" + volume, "Labels": {
                module.RECOVERY_LABEL: self.project, "com.docker.compose.project": target,
                "com.docker.compose.volume": volume}}])
            responses = iter([result(owned_backup), result(target + "_" + volume + "\n"), result(owned_volume),
                              result(), result(), result(code=1), result(), result()])
            with self.subTest(volume=volume), patch.object(self.runner, "cleanup_operator_output") as oneoffs, \
                    patch.object(module.Qualification, "command", side_effect=execute) as command:
                self.runner.cleanup_recovery(remove_backup=True)
                deletions = [call.args[0] for call in command.call_args_list if "rm" in call.args[0]]
                self.assertEqual(deletions, [["docker", "volume", "rm", target + "_" + volume],
                                            ["docker", "volume", "rm", self.project + "_recovery_backup"]])
                self.assertEqual(command.call_args_list[0].args[0][-5:], profile_config)
                self.assertEqual(oneoffs.call_count, 2)
                oneoffs.assert_any_call()
                oneoffs.assert_any_call(initializer=True)


class MetadataDocker:
    def __init__(self, project, image):
        self.project, self.image = project, image
        self.name = project + "-https-metadata"
        self.calls, self.unexpected = [], []
        self.sources = {character * 64: {
            "Image": image, "Config": {"Labels": {"com.docker.compose.project": project,
                                                  "com.docker.compose.service": service}},
            "State": {"Running": False, "Restarting": False, "OOMKilled": False, "ExitCode": 0}}
            for character, service in zip("abcde", ("initialize", "database", "app", "worker", "proxy"))}
        self.initialize_id = "a" * 64
        self.source_ids = list(self.sources)
        self.inspected_sources = None
        self.volumes = {volume: {"Name": project + "_" + volume, "Driver": "local", "Options": None,
            "CreatedAt": "2026-01-01T00:00:00Z", "Labels": {"com.docker.compose.project": project,
                "com.docker.compose.volume": volume}} for volume in ("proxy_data", "proxy_config")}
        self.volume_reads = {volume: 0 for volume in self.volumes}
        self.active_consumers = set()
        self.changed_volume = None
        self.template = {"Name": "/" + self.name, "Image": image,
            "Config": {"Image": image, "User": "10001:10001", "Labels": {
                https.RECOVERY_LABEL: project, "com.docker.compose.project": project,
                https.METADATA_PURPOSE_LABEL: "https-metadata"}},
            "HostConfig": {"NetworkMode": "none", "ReadonlyRootfs": True, "Privileged": False,
                "CapAdd": None, "CapDrop": ["ALL"], "SecurityOpt": ["no-new-privileges:true"]},
            "Mounts": [{"Type": "volume", "Name": project + "_" + volume,
                        "Destination": "/" + volume.replace("_", "-"), "RW": False}
                       for volume in self.volumes],
            "State": {"Running": False, "Restarting": False, "OOMKilled": False, "ExitCode": 0}}
        directory = {"kind": "directory", "uid": 10001, "gid": 10001, "mode": "0700", "count": 1}
        self.observed = {"proxy_data": [directory, {"kind": "file", "uid": 10001,
            "gid": 10001, "mode": "0600", "count": 2}], "proxy_config": [directory.copy()]}
        self.output = json.dumps(self.observed)
        self.helper = self.helper_items = None
        self.helper_ids = None
        self.run_failure = self.remove_failure = self.confirmation_failure = None
        self.completed_change = None
        self.remove_effective, self.removed = True, False

    def command(self, arguments, **options):
        self.calls.append((arguments.copy(), options.copy()))
        result = lambda value="": argparse.Namespace(stdout=value, returncode=0)
        if arguments[:3] == ["docker", "container", "ls"]:
            if self.removed and self.confirmation_failure:
                raise self.confirmation_failure
            identifiers = self.helper_ids if self.helper_ids is not None else (["f" * 64] if self.helper is not None else [])
            return result("\n".join(identifiers))
        if arguments[:3] == ["docker", "container", "inspect"]:
            return result(json.dumps(self.helper_items if self.helper_items is not None else [self.helper]))
        if arguments[:3] == ["docker", "volume", "inspect"]:
            volume = arguments[-1].removeprefix(self.project + "_")
            self.volume_reads[volume] += 1
            item = copy.deepcopy(self.volumes[volume])
            if volume == self.changed_volume and self.volume_reads[volume] > 1:
                item["CreatedAt"] += "-changed"
            return result(json.dumps([item]))
        if arguments[:2] == ["docker", "compose"] and arguments[-4:] == ["ps", "--all", "--quiet", "initialize"]:
            return result(self.initialize_id)
        if arguments[:2] == ["docker", "inspect"]:
            if arguments[2:] == [self.initialize_id]:
                items = [self.sources[self.initialize_id]]
            else:
                items = self.inspected_sources if self.inspected_sources is not None else [self.sources[key] for key in arguments[2:]]
            return result(json.dumps(items))
        if arguments[:2] == ["docker", "ps"]:
            selector = arguments[-1]
            if selector.startswith("volume="):
                return result("e" * 64 if selector.removeprefix("volume=") in self.active_consumers else "")
            if selector == "label=com.docker.compose.project=" + self.project:
                return result("\n".join(self.source_ids))
        if arguments[:2] == ["docker", "run"]:
            self.helper = copy.deepcopy(self.template)
            if self.completed_change:
                self.completed_change(self.helper)
            if self.run_failure:
                raise self.run_failure
            return result(self.output)
        if arguments[:3] == ["docker", "container", "rm"]:
            if self.remove_failure:
                raise self.remove_failure
            self.removed = True
            if self.remove_effective:
                self.helper = None
            return result()
        self.unexpected.append(arguments)
        raise AssertionError("Unexpected fake Docker operation")


class ProxyMetadataHelperBoundaries(unittest.TestCase):
    def setUp(self):
        self.project = "opentrack-smoke-123-amd64"
        self.image = "sha256:" + "1" * 64
        self.runner = https.Qualification(argparse.Namespace(project=self.project, output="work/docker-smoke/https-receipt.json"))

    def mutations(self, fake):
        return [arguments for arguments, _ in fake.calls
                if arguments[:2] == ["docker", "run"] or arguments[:3] == ["docker", "container", "rm"]]

    def assert_cleaned(self, fake):
        self.assertEqual(fake.unexpected, [])
        self.assertIsNone(fake.helper)
        self.assertFalse(self.runner.metadata_reserved)
        removals = [(arguments, options) for arguments, options in fake.calls
                    if arguments[:3] == ["docker", "container", "rm"]]
        self.assertEqual(removals, [(["docker", "container", "rm", "--force", fake.name], {"timeout": 30})])
        self.assertEqual(fake.calls[-1][0], ["docker", "container", "ls", "--all", "--quiet",
                                           "--filter", "name=^/" + fake.name + "$"])

    def test_metadata_helper_uses_only_cached_owned_image_and_readonly_proxy_volumes(self):
        fake = MetadataDocker(self.project, self.image)
        with patch.object(self.runner, "command", side_effect=fake.command):
            self.assertEqual(self.runner.measure_metadata(self.image), fake.observed)
            self.runner.cleanup_metadata(self.image)
        self.assert_cleaned(fake)
        runs = [(arguments, options) for arguments, options in fake.calls if arguments[:2] == ["docker", "run"]]
        self.assertEqual(len(runs), 1)
        arguments, options = runs[0]
        self.assertEqual(options, {"timeout": 30})
        self.assertEqual(arguments[-3:-1], [self.image, "-c"])
        compile(arguments[-1], "<proxy-metadata-helper>", "exec")
        for flag, value in [("--pull", "never"), ("--name", fake.name), ("--network", "none"),
                            ("--user", "10001:10001"), ("--cap-drop", "ALL"),
                            ("--security-opt", "no-new-privileges:true"), ("--entrypoint", "python3")]:
            self.assertEqual(arguments[arguments.index(flag) + 1], value)
        self.assertIn("--read-only", arguments)
        self.assertFalse({"--cap-add", "--privileged", "--volumes-from", "--rm", "--build"} & set(arguments))
        self.assertEqual({arguments[index + 1] for index, value in enumerate(arguments) if value == "--label"},
            {https.RECOVERY_LABEL + "=" + self.project, "com.docker.compose.project=" + self.project,
             https.METADATA_PURPOSE_LABEL + "=https-metadata"})
        mounts = [dict(field.split("=", 1) if "=" in field else (field, True)
                       for field in arguments[index + 1].split(","))
                  for index, argument in enumerate(arguments) if argument == "--mount"]
        self.assertEqual(mounts, [{"type": "volume", "src": self.project + "_" + volume,
            "dst": "/" + volume.replace("_", "-"), "readonly": True, "volume-nocopy": True}
            for volume in ("proxy_data", "proxy_config")])
        self.assertTrue(all(count == 2 for count in fake.volume_reads.values()))
        self.assertTrue(all(arguments[-1] == fake.name for arguments, _ in fake.calls
                            if arguments[:3] == ["docker", "container", "inspect"]))

    def test_metadata_preexisting_helper_is_never_created_or_removed(self):
        for foreign in (False, True):
            fake = MetadataDocker(self.project, self.image)
            fake.helper = copy.deepcopy(fake.template)
            if foreign:
                fake.helper["Config"]["Labels"][https.RECOVERY_LABEL] = "foreign"
            with self.subTest(foreign=foreign), patch.object(self.runner, "command", side_effect=fake.command):
                with self.assertRaises(RuntimeError):
                    self.runner.measure_metadata(self.image)
            self.assertEqual(fake.unexpected, [])
            self.assertEqual(self.mutations(fake), [])
            self.assertTrue(all(count == 0 for count in fake.volume_reads.values()))
            self.assertIsNotNone(fake.helper)

    def test_metadata_preflight_refuses_wrong_image_volume_or_active_source_before_creation(self):
        changes = [lambda f: f.sources[f.initialize_id].update(Image="sha256:" + "2" * 64),
            lambda f: f.sources[f.initialize_id]["Config"]["Labels"].update({"com.docker.compose.project": "foreign"}),
            lambda f: f.sources[f.initialize_id]["Config"]["Labels"].update({"com.docker.compose.service": "app"}),
            lambda f: f.sources[f.initialize_id]["State"].update(Running=True),
            lambda f: f.sources[f.initialize_id]["State"].update(ExitCode=1),
            lambda f: f.volumes["proxy_data"].update(Name="foreign"),
            lambda f: f.volumes["proxy_data"].update(Driver="foreign"),
            lambda f: f.volumes["proxy_data"].update(Options={"device": "private-canary"}),
            lambda f: f.volumes["proxy_data"].update(CreatedAt=""),
            lambda f: f.volumes["proxy_data"]["Labels"].update({"com.docker.compose.project": "foreign"}),
            lambda f: f.volumes["proxy_data"]["Labels"].update({"com.docker.compose.volume": "proxy_config"}),
            lambda f: f.active_consumers.add(self.project + "_proxy_config"),
            lambda f: f.source_ids.clear(), lambda f: setattr(f, "inspected_sources", []),
            lambda f: f.sources["e" * 64]["Config"]["Labels"].update({"com.docker.compose.project": "foreign"})]
        for key, value in [("Running", True), ("Restarting", True), ("OOMKilled", True), ("ExitCode", 1)]:
            changes.append(lambda f, key=key, value=value: f.sources["e" * 64]["State"].update({key: value}))
        for index, change in enumerate(changes):
            fake = MetadataDocker(self.project, self.image)
            change(fake)
            with self.subTest(case=index), patch.object(self.runner, "command", side_effect=fake.command):
                with self.assertRaises(RuntimeError):
                    self.runner.measure_metadata(self.image)
            self.assertEqual(fake.unexpected, [])
            self.assertEqual(self.mutations(fake), [])
        for image in ("opentrack-bootstrap:local", "sha256:invalid", None):
            fake = MetadataDocker(self.project, self.image)
            with self.subTest(image=image), patch.object(self.runner, "command", side_effect=fake.command):
                with self.assertRaises(RuntimeError):
                    self.runner.measure_metadata(image)
            self.assertEqual(fake.calls, [])

    def test_metadata_preflight_timeouts_are_private_and_never_reserve_or_clean_helper(self):
        private = "private-preflight-argv-canary"
        for stage in ("preexisting_helper", "volume", "source_inspect"):
            fake = MetadataDocker(self.project, self.image)
            if stage == "preexisting_helper":
                fake.helper = copy.deepcopy(fake.template)
            before = copy.deepcopy(fake.helper)
            faults, printed = [], io.StringIO()

            def command(arguments, **options):
                selected = (stage == "preexisting_helper" and arguments[:3] == ["docker", "container", "inspect"]
                    or stage == "volume" and arguments[:3] == ["docker", "volume", "inspect"]
                    or stage == "source_inspect" and arguments[:2] == ["docker", "inspect"] and len(arguments) > 3)
                if selected:
                    fake.calls.append((arguments.copy(), options.copy()))
                    failure = https.subprocess.TimeoutExpired(arguments + [private], 15, output=private, stderr=private)
                    faults.append(failure)
                    raise failure
                return fake.command(arguments, **options)

            with self.subTest(stage=stage), redirect_stdout(printed), redirect_stderr(printed), \
                    patch.object(self.runner, "command", side_effect=command), \
                    patch.object(self.runner, "cleanup_metadata", wraps=self.runner.cleanup_metadata) as cleanup:
                with self.assertRaisesRegex(RuntimeError, "^Proxy metadata rejected: timeout$"):
                    self.runner.measure_metadata(self.image)
            self.assertEqual(len(faults), 1)
            if stage == "source_inspect":
                self.assertTrue(set(fake.source_ids) <= set(faults[0].cmd))
            self.assertEqual(fake.unexpected, [])
            self.assertEqual(fake.helper, before)
            self.assertEqual(self.mutations(fake), [])
            cleanup.assert_not_called()
            self.assertEqual(printed.getvalue(), "")

    def test_metadata_cleanup_refuses_foreign_or_unsafe_helper_authority(self):
        changes = [lambda c: c.update(Name="/foreign"), lambda c: c.update(Image="sha256:" + "2" * 64),
            lambda c: c["Config"].update(Image="opentrack-bootstrap:local"), lambda c: c["Config"].update(User="0:0"),
            lambda c: c["Config"]["Labels"].update({https.RECOVERY_LABEL: "foreign"}),
            lambda c: c["Config"]["Labels"].update({"com.docker.compose.project": "foreign"}),
            lambda c: c["Config"]["Labels"].update({https.METADATA_PURPOSE_LABEL: "foreign"}),
            lambda c: c["HostConfig"].update(NetworkMode="host"), lambda c: c["HostConfig"].update(ReadonlyRootfs=False),
            lambda c: c["HostConfig"].update(Privileged=True), lambda c: c["HostConfig"].update(CapAdd=["SYS_ADMIN"]),
            lambda c: c["HostConfig"].update(CapDrop=[]), lambda c: c["HostConfig"].update(SecurityOpt=[]),
            lambda c: c["Mounts"][0].update(RW=True), lambda c: c["Mounts"][0].update(Type="bind"),
            lambda c: c["Mounts"][0].update(Name="foreign"), lambda c: c["Mounts"][0].update(Destination="/backup"),
            lambda c: c["Mounts"].append({"Type": "volume", "Name": "foreign", "Destination": "/extra", "RW": True}),
            lambda c: c["Mounts"].__setitem__(1, c["Mounts"][0].copy()),
            lambda c: c.update(Config=None), lambda c: c.update(HostConfig=[]), lambda c: c.update(Mounts=[None]),
            lambda c: c["Config"].update(Labels="private-label-canary")]
        for index, change in enumerate(changes):
            fake = MetadataDocker(self.project, self.image)
            fake.helper = copy.deepcopy(fake.template)
            change(fake.helper)
            with self.subTest(case=index), patch.object(self.runner, "command", side_effect=fake.command):
                with self.assertRaisesRegex(RuntimeError, "^Proxy metadata cleanup unconfirmed$"):
                    self.runner.cleanup_metadata(self.image)
            self.assertEqual(fake.unexpected, [])
            self.assertEqual(self.mutations(fake), [])
            self.assertIsNotNone(fake.helper)

    def test_metadata_helper_inspection_requires_exact_cardinality_and_typed_object(self):
        for identifiers, items in [(["f" * 64, "e" * 64], None), (["f" * 64], []),
                                   (["f" * 64], [None]), (["f" * 64], {}), (["f" * 64], "private-canary")]:
            fake = MetadataDocker(self.project, self.image)
            fake.helper = copy.deepcopy(fake.template)
            fake.helper_ids, fake.helper_items = identifiers, items
            with self.subTest(items=type(items)), patch.object(self.runner, "command", side_effect=fake.command):
                with self.assertRaisesRegex(RuntimeError, "^Proxy metadata cleanup unconfirmed$"):
                    self.runner.cleanup_metadata(self.image)
            self.assertEqual(fake.unexpected, [])
            self.assertEqual(self.mutations(fake), [])

    def test_metadata_completed_helper_must_be_stopped_cleanly_before_acceptance(self):
        for key, value in [("Running", True), ("Restarting", True), ("OOMKilled", True), ("ExitCode", 1)]:
            fake = MetadataDocker(self.project, self.image)
            fake.completed_change = lambda c, key=key, value=value: c["State"].update({key: value})
            with self.subTest(state=key), patch.object(self.runner, "command", side_effect=fake.command):
                with self.assertRaisesRegex(RuntimeError, "^Proxy metadata rejected: helper$"):
                    self.runner.measure_metadata(self.image)
            self.assert_cleaned(fake)

    def test_metadata_failures_and_interruptions_remove_helper_and_confirm_absence(self):
        private = "private-helper-error-canary"
        failures = [RuntimeError(private), OSError(private),
            https.subprocess.TimeoutExpired("synthetic", 30, output=private, stderr=private), KeyboardInterrupt(), SystemExit(130)]
        for failure in failures:
            fake = MetadataDocker(self.project, self.image)
            fake.run_failure = failure
            if isinstance(failure, (https.subprocess.TimeoutExpired, KeyboardInterrupt, SystemExit)):
                fake.completed_change = lambda c: c["State"].update(Running=True)
            printed = io.StringIO()
            interrupted = isinstance(failure, (KeyboardInterrupt, SystemExit))
            with self.subTest(failure=type(failure)), redirect_stdout(printed), redirect_stderr(printed), \
                    patch.object(self.runner, "command", side_effect=fake.command):
                with self.assertRaises(type(failure) if interrupted else RuntimeError) as error:
                    self.runner.measure_metadata(self.image)
            self.assert_cleaned(fake)
            if interrupted:
                self.assertIs(error.exception, failure)
            else:
                category = "timeout" if isinstance(failure, https.subprocess.TimeoutExpired) else "helper"
                self.assertEqual(str(error.exception), "Proxy metadata rejected: " + category)
            self.assertEqual(printed.getvalue(), "")

    def test_metadata_invalid_private_output_or_changed_source_volume_never_returns_receipt(self):
        for outcome in ("malformed", "extra_private_field", "changed_volume"):
            fake = MetadataDocker(self.project, self.image)
            if outcome == "malformed":
                fake.output = "private-output-canary"
            elif outcome == "extra_private_field":
                fake.output = json.dumps({**fake.observed, "certificate": "private-output-canary"})
            else:
                fake.changed_volume = "proxy_config"
            printed = io.StringIO()
            with self.subTest(outcome=outcome), redirect_stdout(printed), redirect_stderr(printed), \
                    patch.object(self.runner, "command", side_effect=fake.command):
                with self.assertRaisesRegex(RuntimeError, "^Proxy metadata rejected: helper$"):
                    self.runner.measure_metadata(self.image)
            self.assert_cleaned(fake)
            self.assertEqual(printed.getvalue(), "")
            if outcome == "changed_volume":
                self.assertEqual(fake.volume_reads["proxy_config"], 2)

    def test_metadata_cleanup_failure_preserves_sanitized_primary_category(self):
        private = "private-cleanup-canary"
        failures = [(None, None), (RuntimeError(private), "helper"),
            (https.subprocess.TimeoutExpired("synthetic", 30, output=private, stderr=private), "timeout"),
            (KeyboardInterrupt(private), "interrupted"), (SystemExit(private), "interrupted")]
        for mode in ("remove_error", "still_present", "absence_check_error"):
            for failure, category in failures:
                fake = MetadataDocker(self.project, self.image)
                fake.run_failure = failure
                if mode == "remove_error":
                    fake.remove_failure = OSError(private)
                elif mode == "still_present":
                    fake.remove_effective = False
                else:
                    fake.confirmation_failure = OSError(private)
                printed = io.StringIO()
                with self.subTest(mode=mode, primary=category), redirect_stdout(printed), redirect_stderr(printed), \
                        patch.object(self.runner, "command", side_effect=fake.command):
                    with self.assertRaises(RuntimeError) as error:
                        self.runner.measure_metadata(self.image)
                self.assertEqual(fake.unexpected, [])
                reason = "primary=" + category + "; " if category else ""
                self.assertEqual(str(error.exception), "Proxy metadata rejected: " + reason + "cleanup=unconfirmed")
                self.assertEqual(printed.getvalue(), "")
                self.assertTrue(any(arguments[:3] == ["docker", "container", "rm"] for arguments, _ in fake.calls))
                self.assertTrue(self.runner.metadata_reserved)
                previous_calls = len(fake.calls)
                with patch.object(self.runner, "command", side_effect=fake.command):
                    with self.assertRaisesRegex(RuntimeError, "^Proxy metadata rejected: helper$"):
                        self.runner.measure_metadata(self.image)
                self.assertEqual(len(fake.calls), previous_calls)
                fake.remove_failure = fake.confirmation_failure = None
                fake.remove_effective = True
                with patch.object(self.runner, "command", side_effect=fake.command):
                    self.runner.cleanup_metadata(self.image)
                self.assertFalse(self.runner.metadata_reserved)
                self.assertIsNone(fake.helper)
                self.assertEqual(fake.unexpected, [])


class ReadonlyActorBoundaries(unittest.TestCase):
    def setUp(self):
        self.jar_type = http.cookiejar.CookieJar
        self.scratch = []

    def actor(self, name="owner", *, clocks=True, blocked=False):
        actor = object.__new__(module.Qualification)
        actor.url = "http://localhost:18080"
        actor.http = MagicMock(spec=["open"])
        actor.http.open.side_effect = AssertionError("Original HTTP opener called")
        actor.request = MagicMock(side_effect=AssertionError("Original HTTP request called"))
        actor.csrf = "synthetic-" + name + "-csrf"
        actor.fixture = {"path": "/api/v1/auth/session", "headers": {
            "If-None-Match": '"saved-validator"', "Range": "bytes=0-0"}}
        policy = http.cookiejar.DefaultCookiePolicy(blocked_domains=("localhost.local",) if blocked else ())
        policy.synthetic_state = {"unchanged": [name]}
        actor.cookies = self.jar_type(policy=policy)
        for cookie_name, domain, path, expires, secure in [
                ("opentrack_session", "localhost.local", "/", 4102444800, False),
                ("expired_sentinel", "localhost.local", "/", 1, False),
                ("foreign_sentinel", "other.example.test", "/", 4102444800, False),
                ("path_sentinel", "localhost.local", "/different", 4102444800, False),
                ("secure_sentinel", "localhost.local", "/", 4102444800, True)]:
            actor.cookies.set_cookie(http.cookiejar.Cookie(version=0, name=cookie_name, value=name + "-" + cookie_name,
                port=None, port_specified=False, domain=domain, domain_specified=False, domain_initial_dot=False,
                path=path, path_specified=True, secure=secure, expires=expires, discard=False,
                comment="synthetic-cookie", comment_url=None,
                rest={"HttpOnly": None, "SameSite": "Lax", "Synthetic": {"unchanged": [name]}}, rfc2109=False))
        if clocks:
            actor.cookies._now, policy._now = 123, 456
        return actor

    def snapshot(self, actor):
        return {"attributes": actor.__dict__.copy(), "fixture": copy.deepcopy(actor.fixture),
            "cookies": [(id(cookie), copy.deepcopy(cookie.__dict__)) for cookie in actor.cookies],
            "policy": actor.cookies._policy, "policy_state": copy.deepcopy(actor.cookies._policy.__dict__),
            "jar_clock": ("_now" in actor.cookies.__dict__, actor.cookies.__dict__.get("_now"))}

    def assert_unchanged(self, actor, before):
        self.assertEqual(set(actor.__dict__), set(before["attributes"]))
        for name, value in before["attributes"].items():
            self.assertIs(actor.__dict__[name], value)
        self.assertEqual(actor.fixture, before["fixture"])
        self.assertEqual([(id(cookie), cookie.__dict__) for cookie in actor.cookies], before["cookies"])
        self.assertIs(actor.cookies._policy, before["policy"])
        self.assertEqual(actor.cookies._policy.__dict__, before["policy_state"])
        self.assertEqual(("_now" in actor.cookies.__dict__, actor.cookies.__dict__.get("_now")), before["jar_clock"])
        actor.request.assert_not_called()
        actor.http.open.assert_not_called()
        self.assertTrue(all(not list(jar) and jar is not actor.cookies for jar in self.scratch))

    def selection(self, *arguments, **options):
        jar = self.jar_type(*arguments, **options)
        self.scratch.append(jar)
        return jar

    def opener(self, *, status=200, body=b'{"status":"saved"}', media_type="application/json", final_url=None, failure=None):
        opener = MagicMock(spec=["open"])
        replies = []

        def opened(outgoing, timeout):
            if failure is not None:
                raise failure
            reply = MagicMock()
            reply.status = status
            reply.headers = Message()
            reply.headers["Content-Type"] = media_type
            reply.headers["Set-Cookie"] = "opentrack_session=injected-private-canary; Path=/; Secure"
            reply.geturl.return_value = final_url or outgoing.full_url
            reply.read.side_effect = lambda limit: body[:limit]
            reply.__enter__.return_value = reply
            replies.append(reply)
            return reply

        opener.open.side_effect = opened
        return opener, replies

    def test_readonly_view_preserves_actors_fixtures_cookie_records_policy_and_clocks(self):
        for clocks in (False, True):
            actor = self.actor(clocks=clocks)
            before = self.snapshot(actor)
            opener, replies = self.opener()
            with self.subTest(clocks=clocks), patch.object(http.cookiejar, "CookieJar", side_effect=self.selection):
                view = https.readonly_actor(actor, opener)
                value, headers = view.request(actor.fixture["path"], headers=actor.fixture["headers"])
            self.assertIsNot(view, actor)
            self.assertIsNot(view.request, actor.request)
            for name in ("url", "http", "cookies", "csrf", "fixture"):
                self.assertIs(getattr(view, name), getattr(actor, name))
            self.assertEqual(value, {"status": "saved"})
            self.assertIs(headers, replies[0].headers)
            outgoing = opener.open.call_args.args[0]
            fields = {name.lower(): value for name, value in outgoing.header_items()}
            self.assertEqual(outgoing.full_url, https.ORIGIN + actor.fixture["path"])
            self.assertEqual(outgoing.get_method(), "GET")
            self.assertEqual(opener.open.call_args.kwargs, {"timeout": 30})
            self.assertEqual(fields["origin"], https.ORIGIN)
            self.assertEqual(fields["accept-encoding"], "identity")
            self.assertEqual(fields["if-none-match"], actor.fixture["headers"]["If-None-Match"])
            self.assertEqual(fields["range"], actor.fixture["headers"]["Range"])
            self.assertNotIn("x-csrf-token", fields)
            replies[0].read.assert_called_once_with(https.MAX_RESPONSE + 1)
            replies[0].__exit__.assert_called_once()
            self.assert_unchanged(actor, before)

    def test_readonly_actor_headers_stay_separate_and_scratch_jars_are_fresh_and_discarded(self):
        owner, reader = self.actor(), self.actor("reader")
        before = [self.snapshot(actor) for actor in (owner, reader)]
        opener, _ = self.opener()
        with patch.object(http.cookiejar, "CookieJar", side_effect=self.selection):
            owner_view, reader_view = [https.readonly_actor(actor, opener) for actor in (owner, reader)]
            owner_view.request("/api/v1/auth/session")
            reader_view.request("/api/v1/auth/session")
            owner_view.request("/api/v1/auth/session")
            owner_view.request("/api/v1/auth/session", authenticated=False)
        cookies = [call.args[0].get_header("Cookie") for call in opener.open.call_args_list]
        for actual, name in zip(cookies[:3], ("owner", "reader", "owner")):
            self.assertEqual(set(actual.split("; ")), {"opentrack_session=" + name + "-opentrack_session",
                "secure_sentinel=" + name + "-secure_sentinel"})
        self.assertIsNone(cookies[3])
        self.assertEqual(len(self.scratch), 4)
        self.assertEqual(len({id(jar) for jar in self.scratch}), 4)
        for actor, snapshot in zip((owner, reader), before):
            self.assert_unchanged(actor, snapshot)

    def test_readonly_cookie_selection_preserves_the_real_default_policy_restrictions(self):
        actor = self.actor(blocked=True)
        before = self.snapshot(actor)
        opener, _ = self.opener()
        with patch.object(http.cookiejar, "CookieJar", side_effect=self.selection):
            https.readonly_actor(actor, opener).request("/api/v1/auth/session")
        self.assertIsNone(opener.open.call_args.args[0].get_header("Cookie"))
        self.assert_unchanged(actor, before)

    def test_readonly_expected_http_error_decodes_problem_and_binary_reply_remains_exact(self):
        actor = self.actor()
        before = self.snapshot(actor)
        path = "/api/v1/saved-resource"
        problem = {"status": 404, "code": "not_found", "detail": "This item could not be found"}
        headers = Message()
        headers["Content-Type"] = "application/problem+json; charset=utf-8"
        headers["Cache-Control"] = "no-store"
        headers["Set-Cookie"] = "opentrack_session=injected-private-canary; Path=/"
        stream = io.BytesIO(json.dumps(problem).encode())
        failure = https.urllib.error.HTTPError(https.ORIGIN + path, 404, "private-reason-canary", headers, stream)
        opener, _ = self.opener(failure=failure)
        with patch.object(http.cookiejar, "CookieJar", side_effect=self.selection):
            received, metadata = https.readonly_actor(actor, opener).request(path, expected=(404,))
        self.assertEqual(received, problem)
        self.assertIs(metadata, headers)
        self.assertTrue(stream.closed)
        self.assert_unchanged(actor, before)
        body = b"\x00\xffexact attachment bytes"
        opener, replies = self.opener(body=body, media_type="application/octet-stream")
        with patch.object(http.cookiejar, "CookieJar", side_effect=self.selection):
            received, metadata = https.readonly_actor(actor, opener).request(path)
        self.assertEqual(received, body)
        self.assertIs(metadata, replies[0].headers)
        replies[0].read.assert_called_once_with(https.MAX_RESPONSE + 1)
        self.assert_unchanged(actor, before)

    def test_readonly_invalid_paths_writes_arguments_and_headers_never_reach_opener(self):
        actor = self.actor()
        before = self.snapshot(actor)
        cases = [(value, {}) for value in (None, "", "https://private-canary.invalid/", "//private-canary.invalid/",
            "/private#canary", "/private\\canary", "/private canary", "/private\ncanary", "/private\x7fcanary")]
        path = "/api/v1/auth/session"
        cases += [(path, {"method": method}) for method in ("POST", "DELETE", "HEAD", "get")]
        cases += [(path, arguments) for arguments in [{"body": {}}, {"raw": b""}, {"authenticated": 1},
            {"expected": []}, {"expected": ()}, {"expected": (True,)}, {"expected": (99,)}, {"expected": (600,)},
            {"headers": []}, {"headers": {"Origin": "private-canary"}},
            {"headers": {"Cookie": "private-canary"}}, {"headers": {"Authorization": "private-canary"}},
            {"headers": {"X-CSRF-Token": "private-canary"}}, {"headers": {False: "private-canary"}},
            {"headers": {"Range": 1}}, {"headers": {"Range": ""}}, {"headers": {"Range": "bytes=0-0\r\nprivate-canary"}},
            {"headers": {"If-None-Match": "private\x00canary"}},
            {"headers": {"If-None-Match": '"first"', "if-none-match": '"second"'}}]]
        for index, (path, arguments) in enumerate(cases):
            opener, _ = self.opener()
            with self.subTest(case=index), patch.object(http.cookiejar, "CookieJar", side_effect=self.selection):
                with self.assertRaisesRegex(RuntimeError, "^Read-only HTTPS transport rejected$"):
                    https.readonly_actor(actor, opener).request(path, **arguments)
            opener.open.assert_not_called()
            self.assert_unchanged(actor, before)

    def test_readonly_transport_and_response_failures_are_generic_without_http_fallback(self):
        actor = self.actor()
        before = self.snapshot(actor)
        cases = [({"failure": https.ssl.SSLError("private-tls-canary")}, {}),
            ({"failure": RuntimeError("private-opener-canary")}, {}),
            ({"status": 500}, {}), ({"status": 302}, {"expected": (302,)}),
            ({"final_url": "https://private-canary.invalid/"}, {}),
            ({"final_url": https.ORIGIN + "/private-canary"}, {}),
            ({"body": b"canary!"}, {}), ({"body": b"\xff"}, {}),
            ({"body": b"123456789", "media_type": "application/octet-stream"}, {})]
        for index, (response, arguments) in enumerate(cases):
            opener, replies = self.opener(**response)
            printed = io.StringIO()
            with self.subTest(case=index), redirect_stdout(printed), redirect_stderr(printed), \
                    patch.object(http.cookiejar, "CookieJar", side_effect=self.selection), patch.object(https, "MAX_RESPONSE", 8):
                with self.assertRaisesRegex(RuntimeError, "^Read-only HTTPS transport rejected$") as error:
                    https.readonly_actor(actor, opener).request("/api/v1/auth/session", **arguments)
            self.assertTrue(error.exception.__suppress_context__)
            opener.open.assert_called_once()
            self.assertEqual(printed.getvalue(), "")
            if replies:
                replies[0].__exit__.assert_called_once()
                if index >= 6:
                    replies[0].read.assert_called_once_with(9)
                else:
                    replies[0].read.assert_not_called()
            self.assert_unchanged(actor, before)

    def test_readonly_interruptions_preserve_original_actors_and_discard_selected_cookies(self):
        actor = self.actor()
        before = self.snapshot(actor)
        for failure in (KeyboardInterrupt(), SystemExit(130)):
            opener, _ = self.opener(failure=failure)
            with self.subTest(interruption=type(failure)), patch.object(http.cookiejar, "CookieJar", side_effect=self.selection):
                with self.assertRaises(type(failure)) as error:
                    https.readonly_actor(actor, opener).request("/api/v1/auth/session")
            self.assertIs(error.exception, failure)
            opener.open.assert_called_once()
            self.assert_unchanged(actor, before)


class HttpsSourcePhaseBoundaries(unittest.TestCase):
    def setUp(self):
        with patch.object(module.urllib.request, "build_opener", return_value=MagicMock()):
            RecoveryBoundaries.setUp(self)
        self.account = "owner@example.test"
        self.workspace, self.owner_id, self.reader_id = "workspace", "original-owner", "original-reader"
        self.issue, self.attachment = "/issues/saved", "/attachments/saved"
        self.body, self.content = {"saved": True}, b"synthetic attachment"
        self.digest = hashlib.sha256(self.content).hexdigest()
        self.bootstrap = "sha256:" + "9" * 64
        self.reader = copy.copy(self.runner)
        self.reader.cookies = http.cookiejar.CookieJar()
        self.reader.http = MagicMock()
        for actor, value in ((self.runner, "original-owner-cookie"), (self.reader, "original-reader-cookie")):
            actor.cookies.set_cookie(http.cookiejar.Cookie(version=0, name="opentrack_session", value=value,
                port=None, port_specified=False, domain="localhost.local", domain_specified=False,
                domain_initial_dot=False, path="/", path_specified=True, secure=False, expires=None,
                discard=True, comment=None, comment_url=None, rest={"HttpOnly": None, "SameSite": "Lax"}))
        self.private = {"reader": self.reader, "workspace": self.workspace,
                        "move": {"reader": self.reader}, "validators": {"saved": '"original"'}}
        self.operator_config["services"]["app"]["environment"] = {"OPENTRACK_ENVIRONMENT": "development",
            "OPENTRACK__SERVER__LOCAL_CONTAINER": "true", "OPENTRACK_PUBLIC_URL": self.runner.url}
        self.operator_config["services"]["database"]["networks"] = {"database": {}}
        self.operator_config["services"]["worker"] = {"networks": {"database": {}}}
        self.original_fields = [{name: getattr(actor, name) for name in ("url", "http", "csrf", "cookies", "compose", "environment")}
                                for actor in (self.runner, self.reader)]
        self.original_compose, self.original_environment = self.runner.compose.copy(), self.runner.environment.copy()

    def fixture(self):
        fake = argparse.Namespace(events=[], unexpected=[], forbidden=[], mutations=[], secure_jar=None,
            observation={"proxy_data": [{"kind": "directory", "uid": 10001, "gid": 10001, "mode": "0700", "count": 1}],
                         "proxy_config": [{"kind": "directory", "uid": 10001, "gid": 10001, "mode": "0700", "count": 1}]},
            cookie="opentrack_session=secure-owner-cookie; Path=/; Secure; HttpOnly; SameSite=Lax",
            wrong_owner=False, writer_fault=False, metadata_failure=None, cleanup_failure=False,
            restart_failure=False, proxy_fault=None, preexisting=False, private_checks=[],
            runtime_fault=None, proxy_present=False, ready_calls=0, fingerprint_calls=0,
            override_observations=[], override_contents=set(), reads=[])
        fake.images = {name: "sha256:" + str(index) * 64 for index, name in enumerate(
            ("database", "app", "worker", "offline-relay"), 1)}
        fake.images["proxy"] = fake.images["offline-relay"]
        fake.identifiers = {name: character * 64 for name, character in zip(
            ("database", "app", "worker", "offline-relay", "proxy"), "abcde")}
        fake.services = {name: True for name in fake.identifiers}
        fake.services["proxy"] = False
        fake.config = copy.deepcopy(self.operator_config)
        fake.config["volumes"].update({name: {"name": self.project + "_" + name}
                                       for name in ("proxy_data", "proxy_config")})
        fake.config["services"]["app"]["environment"] = {"OPENTRACK_ENVIRONMENT": "production",
            "OPENTRACK__SERVER__LOCAL_CONTAINER": "false", "OPENTRACK_PUBLIC_URL": module.https.ORIGIN}
        fake.config["services"]["proxy"] = {"image": "opentrack-proxy:local", "user": "10001:10001",
            "read_only": True, "cap_drop": ["ALL"], "security_opt": ["no-new-privileges:true"],
            "environment": {"OPENTRACK_DOMAIN": "localhost"}, "tmpfs": ["/tmp:size=16m,mode=1777"],
            "networks": {"web": {}, "offline_ingress": {}},
            "ports": [{"host_ip": "127.0.0.1", "published": "18443", "target": 8443}],
            "volumes": [{"type": "volume", "source": name, "target": target}
                        for name, target in (("proxy_data", "/data"), ("proxy_config", "/config"))]}
        tls = fake.tls = MagicMock()
        tls.environment = {"OPENTRACK_DOMAIN": "localhost", "OPENTRACK_PUBLIC_URL": module.https.ORIGIN,
                           "OPENTRACK_HTTPS_PORT": "18443", "OPENTRACK_HTTPS_BIND_ADDRESS": "127.0.0.1"}
        tls.metadata_reserved = False

        def unexpected(arguments):
            fake.unexpected.append(arguments)
            raise AssertionError("Unexpected synthetic phase operation")

        def override_alive():
            files = [Path(tls.compose[index + 1]) for index, value in enumerate(tls.compose)
                     if value == "--file" and Path(tls.compose[index + 1]).name == "ingress.yaml"]
            fake.override_observations.append(bool(files) and files[0].is_file())
            if files:
                fake.override_path = files[0]
                fake.override_contents.add(files[0].read_text(encoding="ascii"))

        def item(service):
            networks = fake.config["services"].get(service, {}).get("networks", {})
            bindings = {"8443/tcp": [{"HostIp": "127.0.0.1", "HostPort": "18443"}]} if service == "proxy" else {}
            return {"Image": fake.images[service], "Config": {"Image": "opentrack-proxy:local" if service == "proxy" else "synthetic:local",
                "User": "10001:10001", "Labels": {"com.docker.compose.project": self.project,
                    "com.docker.compose.service": service}, "Env": [key + "=" + value for key, value in
                        fake.config["services"].get(service, {}).get("environment", {}).items()]},
                "State": {"Running": fake.services[service], "Restarting": False, "OOMKilled": False, "ExitCode": 0},
                "HostConfig": {"NetworkMode": self.project + "_" + next(iter(networks), "database"),
                    "PortBindings": bindings, "ReadonlyRootfs": True, "CapDrop": ["ALL"],
                    "SecurityOpt": ["no-new-privileges:true"]},
                "NetworkSettings": {"Networks": {self.project + "_" + name: {"NetworkID": name} for name in networks},
                                    "Ports": copy.deepcopy(bindings)},
                "Mounts": [{"Type": "volume", "Name": self.project + "_" + name, "Destination": target, "RW": True}
                           for name, target in (("proxy_data", "/data"), ("proxy_config", "/config"))] if service == "proxy" else []}

        def dc(*arguments, **options):
            override_alive()
            if arguments[0] == "config":
                return argparse.Namespace(stdout=json.dumps(fake.config))
            if arguments[0] in ("up", "stop"):
                selected = [value for value in arguments[1:] if value in fake.services]
                fake.events.append(("tls_" + arguments[0], tuple(selected)))
                fake.mutations.append(arguments)
                for service in selected:
                    fake.services[service] = arguments[0] == "up"
                    if service == "proxy" and arguments[0] == "up":
                        fake.proxy_present = True
                return argparse.Namespace(stdout="")
            if arguments == ("ps", "--all", "--quiet", "proxy"):
                fake.events.append(("proxy_presence", fake.proxy_present))
                return argparse.Namespace(stdout=fake.identifiers["proxy"] if fake.proxy_present else "")
            return unexpected(arguments)

        def command(arguments, **options):
            if arguments[:3] == ["docker", "image", "inspect"]:
                image = fake.images["proxy"] if arguments[3] == "opentrack-proxy:local" else arguments[3]
                return argparse.Namespace(stdout=json.dumps([{"Id": image, "Os": "linux", "Architecture": "amd64"}]))
            if arguments[:3] == ["docker", "network", "inspect"]:
                name = arguments[3]
                return argparse.Namespace(stdout=json.dumps([{"Name": self.project + "_" + name,
                    "Internal": name != "offline_ingress", "Labels": {"com.docker.compose.project": self.project,
                                                           "com.docker.compose.network": name}}]))
            if arguments[:2] == ["docker", "ps"]:
                if arguments[-1].startswith("volume="):
                    fake.events.append(("volume_consumers", any(fake.services.values()),
                        arguments[-1].removeprefix("volume=" + self.project + "_")))
                    return argparse.Namespace(stdout="")
                names = [name for name in fake.identifiers if arguments[2] == "--all" or fake.services[name]]
                return argparse.Namespace(stdout="\n".join(fake.identifiers[name] for name in names))
            if arguments[:2] == ["docker", "inspect"]:
                names = [next(name for name, identifier in fake.identifiers.items() if identifier == value)
                         for value in arguments[2:]]
                items = [item(name) for name in names]
                if fake.runtime_fault and "proxy" in names and fake.services["proxy"]:
                    fake.runtime_fault(items[names.index("proxy")])
                if not any(fake.services.values()) and len(names) > 1 and fake.writer_fault:
                    items[0]["State"]["Running"] = True
                    fake.writer_fault = False
                if names == ["proxy"] and not fake.services["proxy"] and fake.proxy_fault:
                    fake.proxy_fault(items[0])
                return argparse.Namespace(stdout=json.dumps(items))
            if arguments[:3] == ["docker", "container", "rm"]:
                fake.events.append(("proxy_remove", arguments[3:]))
                fake.mutations.append(tuple(arguments))
                fake.proxy_present = False
                return argparse.Namespace(stdout="")
            return unexpected(arguments)

        def http_dc(*arguments, **options):
            if arguments[:3] == ("--profile", "operator", "config"):
                return argparse.Namespace(stdout=json.dumps(self.operator_config))
            if arguments[0] == "up":
                override_alive()
                fake.events.append(("http_restore", not fake.proxy_present))
                selected = [value for value in arguments[1:] if value in fake.services]
                fake.events.append(("http_start_services", tuple(selected), "--no-deps" in arguments, "--no-build" in arguments))
                for service in selected:
                    fake.services[service] = True
                fake.mutations.append(arguments)
                return argparse.Namespace(stdout="")
            return unexpected(arguments)

        def session(reader=False):
            return {"user": {"id": self.reader_id if reader else self.owner_id,
                             "email": "reader@example.test" if reader else self.account},
                    "workspaces": [{"id": self.workspace, "role": "viewer" if reader else "owner"}],
                    "csrf_token": "synthetic-secure-csrf"}

        def response(target, value, headers=None):
            reply = MagicMock()
            reply.status = 200
            reply.geturl.return_value = target
            reply.headers = headers or Message()
            if headers is None:
                reply.headers["Content-Type"] = "application/json"
            payload = json.dumps(value).encode() if not isinstance(value, bytes) else value
            reply.read.side_effect = lambda limit=None: payload if limit is None else payload[:limit]
            reply.__enter__.return_value = reply
            reply.info.return_value = reply.headers
            return reply

        def opened(request, timeout):
            cookie = request.get_header("Cookie") or ""
            role = next((role for value, role in (("original-reader-cookie", "reader"),
                ("secure-owner-cookie", "secure"), ("original-owner-cookie", "owner")) if value in cookie), None)
            if role is None:
                reply = response(request.full_url, {})
                reply.status = 401
                return reply
            fake.events.append(("https_read", role))
            fake.reads.append((request.full_url, request.get_method(), cookie))
            if request.full_url.endswith("/api/v1/auth/session"):
                value = session(role == "reader")
                if fake.wrong_owner and role == "owner":
                    value["user"]["email"] = "private-canary"
            elif request.full_url.endswith(self.issue + "/document"):
                value = {"body": self.body}
            elif request.full_url.endswith(self.attachment + "/content"):
                value = self.content
            elif request.full_url.endswith(self.issue):
                value = {"id": "saved"}
            else:
                return unexpected(request.full_url)
            headers = Message()
            headers["Content-Type"] = "application/octet-stream" if isinstance(value, bytes) else "application/json"
            return response(request.full_url, value, headers)

        fake.opener = MagicMock(spec=["open"])
        fake.opener.open.side_effect = opened
        fake.login_opener = MagicMock(spec=["open"])

        def login(request, timeout):
            fake.cookies_before_login = len(list(fake.secure_jar))
            fake.secure_jar.add_cookie_header(request)
            fake.events.append(("secure_login", request.get_header("Cookie")))
            headers = Message()
            headers["Content-Type"] = "application/json"
            headers["Set-Cookie"] = fake.cookie
            reply = response(request.full_url, session(), headers)
            fake.secure_jar.extract_cookies(reply, request)
            fake.issued = [(cookie.secure, cookie.has_nonstandard_attr("HttpOnly"),
                cookie.get_nonstandard_attr("SameSite"), cookie.path, cookie.domain_specified) for cookie in fake.secure_jar]
            return reply

        fake.login_opener.open.side_effect = login

        def build_opener(*handlers):
            jars = [handler.cookiejar for handler in handlers if isinstance(handler, module.urllib.request.HTTPCookieProcessor)]
            if len(jars) != 1:
                return unexpected("unexpected opener")
            fake.secure_jar = jars[0]
            return fake.login_opener

        def private_controls(actor, fixture, *arguments):
            role = "http" if actor is self.runner else "original" if actor.cookies is self.runner.cookies else "secure"
            fake.events.append(("private_controls", role))
            reader_session, _ = fixture["reader"].request("/api/v1/auth/session")
            fake.private_checks.append((role, fixture is self.private,
                fixture["reader"] is fixture["move"]["reader"], reader_session["user"]["id"], arguments))

        def measure(image):
            fake.events.append(("metadata", not any(fake.services.values())))
            tls.metadata_reserved = True
            if fake.metadata_failure:
                if not fake.cleanup_failure:
                    cleanup(image)
                raise fake.metadata_failure
            cleanup(image)
            return fake.observation

        def cleanup(image):
            fake.events.append(("helper_cleanup", image == self.bootstrap))
            if fake.cleanup_failure:
                raise RuntimeError("private-cleanup-canary")
            tls.metadata_reserved = False

        def ready(opener):
            fake.ready_calls += 1
            fake.events.append(("tls_ready", fake.ready_calls))
            if fake.restart_failure and fake.ready_calls > 1:
                raise RuntimeError("private-restart-canary")

        def fingerprint():
            fake.fingerprint_calls += 1
            fake.events.append(("fingerprint", fake.fingerprint_calls))
            return "matching-fingerprint"

        def output_cleanup(*arguments, **options):
            fake.events.append(("operator_cleanup", options.get("initializer", False)))

        def exported(proxy):
            tls.certificate.write_bytes(b"synthetic public fixture")

        def context(*arguments, **options):
            return argparse.Namespace(check_hostname=True, verify_mode=module.ssl.CERT_REQUIRED,
                load_verify_locations=MagicMock(), cert_store_stats=lambda: {"x509_ca": 1})

        def forbidden(*arguments, **options):
            fake.forbidden.append(True)
            raise AssertionError("Real process or URL access forbidden")

        def http_open(request, timeout, *, reader=False):
            fake.events.append(("http_read", "reader" if reader else "owner"))
            return response(request.full_url, session(reader))

        tls.dc.side_effect, tls.command.side_effect = dc, command
        tls.container.side_effect = item
        tls.metadata_container.side_effect = lambda image: {} if fake.preexisting else None
        tls.measure_metadata.side_effect, tls.cleanup_metadata.side_effect = measure, cleanup
        tls.export_certificate.side_effect, tls.ready.side_effect = exported, ready
        tls.opener.return_value, tls.rejects_untrusted.return_value = fake.opener, True
        fake.patches = [patch.object(module.https, "Qualification", return_value=tls),
            patch.object(module.urllib.request, "build_opener", side_effect=build_opener),
            patch.object(module.urllib.request, "urlopen", side_effect=forbidden),
            patch.object(module.subprocess, "run", side_effect=forbidden),
            patch.object(module.ssl, "SSLContext", side_effect=context),
            patch.object(module.ssl, "create_default_context", side_effect=context),
            patch.object(self.runner, "dc", side_effect=http_dc), patch.object(self.runner, "command", side_effect=command),
            patch.object(self.runner, "container", side_effect=item), patch.object(self.runner, "ready"),
            patch.object(self.runner, "verify_offline_runtime"), patch.object(self.runner, "database_fingerprint", side_effect=fingerprint),
            patch.object(self.runner, "cleanup_operator_output", side_effect=output_cleanup),
            patch.object(module.Qualification, "check_private_revocation", new=private_controls),
            patch.object(self.runner.http, "open", side_effect=http_open),
            patch.object(self.reader.http, "open", side_effect=lambda request, timeout: http_open(request, timeout, reader=True))]
        return fake

    def run_phase(self, fake):
        with ExitStack() as stack:
            for operation in fake.patches:
                stack.enter_context(operation)
            return self.runner.measure_https_source(self.account, "synthetic owner password", self.private,
                self.issue, self.attachment, self.body, self.digest, self.bootstrap)

    def assert_isolated(self, fake, original):
        self.assertEqual(fake.unexpected, [])
        self.assertEqual(fake.forbidden, [])
        self.assertEqual([[cookie.__dict__ for cookie in actor.cookies] for actor in (self.runner, self.reader)], original)
        for actor, fields in zip((self.runner, self.reader), self.original_fields):
            for name, value in fields.items():
                self.assertIs(getattr(actor, name), value)
            self.assertNotIn("request", actor.__dict__)
        self.assertEqual(self.runner.compose, self.original_compose)
        self.assertEqual(self.runner.environment, self.original_environment)
        self.assertIs(self.private["reader"], self.reader)
        self.assertIs(self.private["move"]["reader"], self.reader)
        self.assertEqual(self.private["validators"], {"saved": '"original"'})
        if fake.secure_jar is not None:
            self.assertIsNot(fake.secure_jar, self.runner.cookies)
            self.assertIsNot(fake.secure_jar, self.reader.cookies)
            self.assertEqual(list(fake.secure_jar), [])
        self.assertTrue(all(fake.override_observations))
        if hasattr(fake, "override_path"):
            self.assertFalse(fake.override_path.exists())

    def test_https_phase_keeps_original_sessions_then_secure_login_and_fences_before_observation(self):
        fake = self.fixture()
        original = copy.deepcopy([[cookie.__dict__ for cookie in actor.cookies] for actor in (self.runner, self.reader)])
        result = self.run_phase(fake)
        self.assert_isolated(fake, original)
        self.assertEqual(result["metadata"], fake.observation)
        self.assertTrue(all(result[key] for key in ("source_https_restart_verified", "source_http_restore_verified",
            "source_proxy_removed", "original_sessions_verified", "secure_owner_session_verified")))
        login = next(index for index, event in enumerate(fake.events) if event[0] == "secure_login")
        self.assertIn(("https_read", "owner"), fake.events[:login])
        self.assertIn(("https_read", "reader"), fake.events[:login])
        self.assertEqual(fake.events[login], ("secure_login", None))
        self.assertEqual(fake.cookies_before_login, 0)
        self.assertEqual(fake.issued, [(True, True, "Lax", "/", False)])
        self.assertEqual([event for event in fake.events if event[0] == "metadata"], [("metadata", True)])
        self.assertEqual({event for event in fake.events if event[0] == "volume_consumers"},
            {("volume_consumers", False, "proxy_data"), ("volume_consumers", False, "proxy_config")})
        metadata = next(index for index, event in enumerate(fake.events) if event[0] == "metadata")
        self.assertEqual([event for event in fake.events[:metadata] if event[0] == "operator_cleanup"],
                         [("operator_cleanup", False), ("operator_cleanup", True)])
        self.assertTrue(all(url.startswith(module.https.ORIGIN + "/") and method == "GET" for url, method, _ in fake.reads))
        self.assertEqual([check[0] for check in fake.private_checks], ["original", "secure", "original", "secure", "http"])
        self.assertTrue(all(same_reader and identity == self.reader_id
                            and arguments == (self.issue, self.attachment, self.body, self.digest)
                            for _, _, same_reader, identity, arguments in fake.private_checks))
        self.assertEqual([event for event in fake.events if event[0] == "http_restore"], [("http_restore", True)])
        self.assertLess(next(i for i, event in enumerate(fake.events) if event[0] == "proxy_remove"),
                        next(i for i, event in enumerate(fake.events) if event[0] == "http_restore"))
        self.assertEqual(fake.fingerprint_calls, 2)
        self.assertEqual(fake.services, {"database": True, "app": True, "worker": True, "offline-relay": True, "proxy": False})
        self.assertFalse(fake.proxy_present)
        self.assertFalse(fake.tls.metadata_reserved)
        self.assertIn(("http_start_services", ("database", "app", "worker", "offline-relay"), True, True), fake.events)
        self.assertTrue(all(set(event[1]) <= {"app", "database", "proxy"} for event in fake.events if event[0] == "tls_up"))
        for call in fake.tls.dc.call_args_list:
            if call.args[0] == "up":
                self.assertIn("--no-deps", call.args)
                self.assertIn("--no-build", call.args)
                self.assertEqual(call.args[call.args.index("--pull") + 1], "never")
        self.assertEqual([event[1] for event in fake.events if event[0] == "proxy_remove"], [[fake.identifiers["proxy"]]])
        self.assertEqual(fake.override_contents,
            {"services:\n  proxy:\n    networks:\n      web: {}\n      offline_ingress: {}\n"})

    def test_https_phase_refuses_preexisting_helper_without_source_mutation(self):
        fake = self.fixture()
        fake.preexisting = True
        original = copy.deepcopy([[cookie.__dict__ for cookie in actor.cookies] for actor in (self.runner, self.reader)])
        with self.assertRaisesRegex(RuntimeError, "stage=preflight; primary=refused"):
            self.run_phase(fake)
        self.assert_isolated(fake, original)
        self.assertEqual(fake.mutations, [])
        fake.tls.cleanup_metadata.assert_not_called()
        fake.tls.measure_metadata.assert_not_called()

    def test_https_phase_refuses_already_stopped_http_worker_before_any_tls_mutation(self):
        fake = self.fixture()
        fake.services["worker"] = False
        original = copy.deepcopy([[cookie.__dict__ for cookie in actor.cookies] for actor in (self.runner, self.reader)])
        with self.assertRaisesRegex(RuntimeError, "stage=preflight; primary=refused"):
            self.run_phase(fake)
        self.assert_isolated(fake, original)
        self.assertEqual(fake.mutations, [])
        fake.login_opener.open.assert_not_called()
        fake.tls.measure_metadata.assert_not_called()

    def test_https_phase_requires_original_owner_and_genuine_secure_cookie_before_metadata(self):
        changes = [lambda f: setattr(f, "wrong_owner", True)]
        for old, new in [("; Secure", ""), ("; HttpOnly", ""), ("SameSite=Lax", "SameSite=None"),
                         ("Path=/;", "Path=/different;"), ("secure-owner-cookie", "original-owner-cookie")]:
            changes.append(lambda f, old=old, new=new: setattr(f, "cookie", f.cookie.replace(old, new)))
        for index, change in enumerate(changes):
            fake = self.fixture()
            change(fake)
            original = copy.deepcopy([[cookie.__dict__ for cookie in actor.cookies] for actor in (self.runner, self.reader)])
            with self.subTest(case=index), self.assertRaises(RuntimeError) as error:
                self.run_phase(fake)
            self.assert_isolated(fake, original)
            self.assertNotIn("canary", str(error.exception))
            fake.tls.measure_metadata.assert_not_called()
            self.assertTrue(any(event[0] == "http_restore" for event in fake.events))
            if index == 0:
                fake.login_opener.open.assert_not_called()

    def test_https_phase_rejects_unfenced_writer_before_metadata_and_restores_http(self):
        fake = self.fixture()
        fake.writer_fault = True
        original = copy.deepcopy([[cookie.__dict__ for cookie in actor.cookies] for actor in (self.runner, self.reader)])
        with self.assertRaisesRegex(RuntimeError, "stage=writer_fence; primary=refused"):
            self.run_phase(fake)
        self.assert_isolated(fake, original)
        fake.tls.measure_metadata.assert_not_called()
        self.assertIn(("http_restore", True), fake.events)

    def test_https_phase_rejects_wrong_actual_port_or_extra_runtime_network_before_login(self):
        for change in (lambda item: item["NetworkSettings"]["Ports"]["8443/tcp"][0].update(HostIp="0.0.0.0"),
                       lambda item: item["NetworkSettings"]["Networks"].update(foreign={"NetworkID": "foreign"})):
            fake = self.fixture()
            fake.runtime_fault = change
            original = copy.deepcopy([[cookie.__dict__ for cookie in actor.cookies] for actor in (self.runner, self.reader)])
            with self.subTest(change=change), self.assertRaises(RuntimeError) as error:
                self.run_phase(fake)
            self.assert_isolated(fake, original)
            self.assertIn("stage=tls_start; primary=refused", str(error.exception))
            fake.login_opener.open.assert_not_called()
            fake.tls.measure_metadata.assert_not_called()
            self.assertTrue(any(event[0] == "http_restore" for event in fake.events))

    def test_https_phase_preserves_primary_failure_and_independently_attempts_cleanup_and_restart(self):
        for failure, secondary in [(module.subprocess.TimeoutExpired(["docker", "private-argv-canary"], 1), False),
                (KeyboardInterrupt(), False), (SystemExit("private-interruption-canary"), False),
                (module.subprocess.TimeoutExpired(["docker", "private-argv-canary"], 1), True)]:
            fake = self.fixture()
            fake.metadata_failure = failure
            fake.cleanup_failure = fake.restart_failure = secondary
            original = copy.deepcopy([[cookie.__dict__ for cookie in actor.cookies] for actor in (self.runner, self.reader)])
            printed = io.StringIO()
            exception = type(failure) if isinstance(failure, (KeyboardInterrupt, SystemExit)) and not secondary else RuntimeError
            with self.subTest(failure=type(failure), secondary=secondary), redirect_stdout(printed), redirect_stderr(printed):
                with self.assertRaises(exception) as error:
                    self.run_phase(fake)
            self.assert_isolated(fake, original)
            self.assertEqual(printed.getvalue(), "")
            self.assertNotIn("canary", str(error.exception))
            self.assertIn(("helper_cleanup", True), fake.events)
            self.assertIn(("http_restore", True), fake.events)
            self.assertEqual(fake.fingerprint_calls, 2)
            if secondary:
                self.assertIn("stage=metadata; primary=timeout", str(error.exception))
                self.assertIn("cleanup=helper,https_restart", str(error.exception))

    def test_https_phase_never_force_removes_running_or_foreign_proxy_during_cleanup(self):
        for change in (lambda item: item["State"].update(Running=True),
                       lambda item: item["Config"]["Labels"].update({"com.docker.compose.service": "foreign"})):
            fake = self.fixture()
            fake.proxy_fault = change
            original = copy.deepcopy([[cookie.__dict__ for cookie in actor.cookies] for actor in (self.runner, self.reader)])
            with self.subTest(change=change), self.assertRaisesRegex(RuntimeError, "cleanup=proxy_removal"):
                self.run_phase(fake)
            self.assert_isolated(fake, original)
            self.assertFalse(any(event[0] == "proxy_remove" for event in fake.events))
            self.assertTrue(any(event[0] == "http_restore" for event in fake.events))

    def test_explicit_metadata_cleanup_binds_owned_initialize_and_still_attempts_http_cleanup(self):
        changes = [None, lambda item: item.update(Image="opentrack-bootstrap:local"),
            lambda item: item["Config"].update(Image="foreign:local"),
            lambda item: item["Config"]["Labels"].update({"com.docker.compose.project": "foreign"}),
            lambda item: item["State"].update(Running=True)]
        for index, change in enumerate(changes):
            initialize = {"Image": self.bootstrap, "Config": {"Image": "opentrack-bootstrap:local", "Labels": {
                "com.docker.compose.project": self.project, "com.docker.compose.service": "initialize"}},
                "State": {"Running": False, "Restarting": False, "OOMKilled": False, "ExitCode": 0}}
            if change:
                change(initialize)
            helper = MagicMock()
            forbidden = []

            def no_process(*arguments, **options):
                forbidden.append(True)
                raise AssertionError("Unexpected real process")

            with self.subTest(case=index), patch.object(module.subprocess, "run", side_effect=no_process), \
                    patch.object(self.runner, "command", return_value=argparse.Namespace(stdout="f" * 64)) as command, \
                    patch.object(self.runner, "container", return_value=initialize), \
                    patch.object(module.https, "Qualification", return_value=helper) as factory, \
                    patch.object(self.runner, "cleanup_operator_output", side_effect=RuntimeError("private-cleanup-canary")) as legacy:
                with self.assertRaises(RuntimeError) as error:
                    self.runner.cleanup_recovery(remove_backup=True, include_https_metadata=True)
            self.assertEqual(forbidden, [])
            legacy.assert_called_once_with()
            self.assertEqual(command.call_args_list[0].args[0], ["docker", "container", "ls", "--all", "--quiet",
                "--filter", "name=^/" + self.project + "-https-metadata$"])
            if change:
                factory.assert_not_called()
                self.assertEqual(str(error.exception), "Recovery cleanup unconfirmed: https_metadata,http_recovery")
            else:
                helper.cleanup_metadata.assert_called_once_with(self.bootstrap)
                self.assertEqual(str(error.exception), "Recovery cleanup unconfirmed: http_recovery")

    def test_cli_only_selects_metadata_cleanup_through_explicit_recovery_command(self):
        for cleanup in (False, True):
            runner = MagicMock()
            arguments = ["docker-smoke.py", "--project", self.project, "--expected-architecture", "amd64"]
            if cleanup:
                arguments += ["--cleanup-recovery", "--cleanup-backup"]
            forbidden = []

            def no_process(*arguments, **options):
                forbidden.append(True)
                raise AssertionError("Unexpected real process")

            with self.subTest(cleanup=cleanup), patch("sys.argv", arguments), \
                    patch.object(module.signal, "signal"), patch.object(module, "Qualification", return_value=runner), \
                    patch.object(module.subprocess, "run", side_effect=no_process), redirect_stdout(io.StringIO()):
                module.main()
            self.assertEqual(forbidden, [])
            if cleanup:
                runner.cleanup_recovery.assert_called_once_with(remove_backup=True, include_https_metadata=True)
                runner.run.assert_not_called()
            else:
                runner.run.assert_called_once_with()
                runner.cleanup_recovery.assert_not_called()


class ProxyMetadataBoundaries(unittest.TestCase):
    def test_observer_aggregates_metadata_without_reading_private_contents(self):
        with tempfile.TemporaryDirectory() as temporary:
            roots = {name: Path(temporary) / name for name in ("proxy_data", "proxy_config")}
            for root in roots.values():
                root.mkdir(mode=0o700)
            nested = roots["proxy_data"] / "private-name-canary"
            nested.mkdir(mode=0o750)
            files = [roots["proxy_data"] / "private-name-canary-a",
                     nested / "private-name-canary-b", nested / "private-name-canary-c",
                     roots["proxy_config"] / "private-name-canary-d"]
            for index, path in enumerate(files):
                path.write_bytes(b"private-content-canary")
                path.chmod(0o640 if index == 2 else 0o600)
            before = [(path.read_bytes(), path.stat().st_mode, path.stat().st_mtime_ns) for path in files]
            descriptor_count = len(os.listdir("/proc/self/fd"))
            original_open = os.open

            def directory_open(path, flags, *args, **kwargs):
                self.assertTrue(flags & os.O_DIRECTORY)
                self.assertTrue(flags & os.O_NOFOLLOW)
                return original_open(path, flags, *args, **kwargs)

            with patch("builtins.open", side_effect=AssertionError("File body opened")), \
                    patch.object(Path, "open", side_effect=AssertionError("File body opened")), \
                    patch.object(os, "open", side_effect=directory_open), \
                    patch.object(https.hashlib, "sha256", side_effect=AssertionError("Content hashed")):
                observed = https.proxy_metadata(roots)
            self.assertEqual(len(os.listdir("/proc/self/fd")), descriptor_count)
            self.assertEqual([sum(row["count"] for row in observed[name]) for name in roots], [5, 2])
            self.assertEqual(sum(row["count"] for row in observed["proxy_data"]
                                 if row["kind"] == "file" and row["mode"] == "0600"), 2)
            for rows in observed.values():
                self.assertTrue(all(set(row) == {"kind", "uid", "gid", "mode", "count"} for row in rows))
            self.assertNotIn("canary", json.dumps(observed))
            self.assertEqual(https.parse_proxy_metadata(json.dumps(observed)), observed)
            self.assertEqual([(path.read_bytes(), path.stat().st_mode, path.stat().st_mtime_ns)
                              for path in files], before)

    def test_observer_refuses_links_special_entries_and_metadata_failures_privately(self):
        with tempfile.TemporaryDirectory() as temporary:
            roots = {name: Path(temporary) / name for name in ("proxy_data", "proxy_config")}
            for root in roots.values():
                root.mkdir(mode=0o700)
            sentinel = roots["proxy_data"] / "private-name-canary"
            sentinel.write_bytes(b"unchanged-private-canary")
            sentinel.chmod(0o600)
            bad = roots["proxy_data"] / "private-rejected-canary"
            descriptor_count = len(os.listdir("/proc/self/fd"))
            for kind in ("symlink", "hardlink", "fifo", "special_bits"):
                with self.subTest(kind=kind):
                    if kind == "symlink":
                        bad.symlink_to(sentinel)
                    elif kind == "hardlink":
                        os.link(sentinel, bad)
                    elif kind == "fifo":
                        os.mkfifo(bad)
                    else:
                        bad.write_bytes(b"synthetic")
                        bad.chmod(0o4600)
                    try:
                        with self.assertRaises(RuntimeError) as error:
                            https.proxy_metadata(roots)
                        self.assertNotIn("canary", str(error.exception))
                    finally:
                        bad.unlink()
                    self.assertEqual(len(os.listdir("/proc/self/fd")), descriptor_count)
            linked_root = Path(temporary) / "private-root-canary"
            linked_root.symlink_to(roots["proxy_data"], target_is_directory=True)
            with self.assertRaises(RuntimeError) as error:
                https.proxy_metadata({**roots, "proxy_data": linked_root})
            self.assertNotIn("canary", str(error.exception))
            original_lstat = os.lstat

            def other_device(path, *args, **kwargs):
                information = original_lstat(path, *args, **kwargs)
                if os.fspath(path).endswith("private-name-canary"):
                    fields = list(information)
                    fields[2] += 1
                    return os.stat_result(fields)
                return information

            for operation, failure in (("lstat", other_device),
                                       ("scandir", PermissionError("private-error-canary"))):
                with self.subTest(operation=operation), patch.object(os, operation, side_effect=failure):
                    with self.assertRaises(RuntimeError) as error:
                        https.proxy_metadata(roots)
                    self.assertNotIn("canary", str(error.exception))
                self.assertEqual(len(os.listdir("/proc/self/fd")), descriptor_count)
            self.assertEqual(sentinel.read_bytes(), b"unchanged-private-canary")

    def test_observer_stops_at_each_traversal_bound(self):
        with tempfile.TemporaryDirectory() as temporary:
            roots = {name: Path(temporary) / name for name in ("proxy_data", "proxy_config")}
            for root in roots.values():
                root.mkdir(mode=0o700)
            (roots["proxy_data"] / "private-limit-canary").write_bytes(b"synthetic")
            for name, limit in (("ENTRIES", 1), ("DEPTH", 0), ("CLASSES", 1)):
                with self.subTest(bound=name), patch.object(https, "MAX_PROXY_METADATA_" + name, limit):
                    with self.assertRaises(RuntimeError) as error:
                        https.proxy_metadata(roots)
                    self.assertNotIn("canary", str(error.exception))

    def test_decoder_rejects_untrusted_fields_types_duplicates_and_bounds(self):
        directory = {"kind": "directory", "uid": 10001, "gid": 10001, "mode": "0700", "count": 1}
        regular = {"kind": "file", "uid": 10001, "gid": 10001, "mode": "0600", "count": 2}
        valid = {"proxy_data": [directory, regular], "proxy_config": [directory]}
        decoded = https.parse_proxy_metadata(json.dumps(valid))
        self.assertEqual(decoded, valid)
        self.assertIsNot(decoded["proxy_data"][0], directory)
        changes = [lambda value: value.update(private_canary=[]),
            lambda value: value.pop("proxy_config"),
            lambda value: value.update(proxy_data={}),
            lambda value: value.update(proxy_data=[]),
            lambda value: value["proxy_data"][0].update(extra="private-canary"),
            lambda value: value["proxy_data"][0].update(uid=True),
            lambda value: value["proxy_data"][0].update(gid=-1),
            lambda value: value["proxy_data"][0].update(uid=4294967295),
            lambda value: value["proxy_data"][0].update(mode="700"),
            lambda value: value["proxy_data"][0].update(mode="4700"),
            lambda value: value["proxy_data"][0].update(kind="private-canary"),
            lambda value: value["proxy_data"][0].update(count=True),
            lambda value: value["proxy_data"][0].update(count=0),
            lambda value: value["proxy_data"][1].update(count=4096),
            lambda value: value["proxy_data"].append({**directory, "count": 2}),
            lambda value: value.update(proxy_data=[{**directory, "uid": number} for number in range(65)])]
        for change in changes:
            value = copy.deepcopy(valid)
            change(value)
            with self.subTest(change=change), self.assertRaises(RuntimeError) as error:
                https.parse_proxy_metadata(json.dumps(value))
            self.assertNotIn("canary", str(error.exception))
        encoded = json.dumps(valid)
        duplicate_key = encoded.replace('"uid": 10001', '"uid": 10001, "uid": 10001', 1)
        for rejected in (None, {}, b"{}", "\ud800", duplicate_key, encoded + "{}", " " * 16385 + encoded):
            with self.subTest(output_type=type(rejected)), self.assertRaises(RuntimeError) as error:
                https.parse_proxy_metadata(rejected)
            self.assertNotIn("canary", str(error.exception))


if __name__ == "__main__":
    unittest.main()
