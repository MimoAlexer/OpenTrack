#!/usr/bin/env python3
"""Recovery boundary tests; no Docker, database or application is started."""
import argparse
from contextlib import redirect_stdout
import copy
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import stat
import tarfile
import tempfile
import unittest
from unittest.mock import MagicMock, patch


spec = importlib.util.spec_from_file_location("docker_smoke", Path(__file__).with_name("docker-smoke.py"))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


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
        failure = "snapshot"
        def seed(runner, reader, base):
            reader.cookies.set_cookie(module.http.cookiejar.Cookie(version=0, name="opentrack_session",
                value="synthetic-reader-session", port=None, port_specified=False, domain="localhost.local",
                domain_specified=False, domain_initial_dot=False, path="/", path_specified=True,
                secure=False, expires=None, discard=True, comment=None, comment_url=None, rest={}))
            readers.append(reader)
            return {}
        def dc(runner, *arguments, **kwargs):
            if arguments[0] == "config":
                config = copy.deepcopy(self.operator_config)
                config["name"] = runner.project
                for name, volume in config["volumes"].items():
                    volume["name"] = f"{runner.project}_{name}"
                for name, network in config["networks"].items():
                    network["name"] = f"{runner.project}_{name}"
                return argparse.Namespace(stdout=json.dumps(config))
            if arguments[0] == "up":
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
                patch.object(self.runner, "seed_operator_output"), \
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
                ready.reset_mock()
                output_cleanup.reset_mock()
                with self.subTest(failure=failure), self.assertRaisesRegex(RuntimeError, "cleanup failure") as raised:
                    self.runner.cold_recovery("fixture@example.test", "fixture", "/issues/fixture",
                                              "/attachments/" + identifier, {}, "fixture")
                self.assertEqual(str(raised.exception.__context__), reason)
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
        config["name"] = target
        for name, volume in config["volumes"].items():
            volume["name"] = f"{target}_{name}"
        for name, network in config["networks"].items():
            network["name"] = f"{target}_{name}"
        import json
        def result(value="", code=0):
            return argparse.Namespace(stdout=value, returncode=code)
        owned_backup = json.dumps([{"Name": self.project + "_recovery_backup",
                                    "Labels": {module.RECOVERY_LABEL: self.project}}])
        foreign_volume = json.dumps([{"Name": target + "_blobs", "Labels": {}}])
        responses = [result(json.dumps(config)), result(owned_backup), result(target + "_blobs\n"), result(foreign_volume)]
        with patch.object(self.runner, "cleanup_operator_output") as oneoffs, \
                patch.object(module.Qualification, "command", side_effect=responses) as command:
            with self.assertRaises(RuntimeError):
                self.runner.cleanup_recovery(remove_backup=True)
            self.assertFalse(any("rm" in call.args[0] or "down" in call.args[0] for call in command.call_args_list))
            self.assertEqual(oneoffs.call_count, 2)
            oneoffs.assert_any_call(initializer=True)
        owned_volume = json.dumps([{"Name": target + "_blobs", "Labels": {
            module.RECOVERY_LABEL: self.project, "com.docker.compose.project": target,
            "com.docker.compose.volume": "blobs"}}])
        responses = [result(json.dumps(config)), result(owned_backup), result(target + "_blobs\n"), result(owned_volume),
                     result(), result(), result(code=1), result(), result()]
        with patch.object(self.runner, "cleanup_operator_output") as oneoffs, \
                patch.object(module.Qualification, "command", side_effect=responses) as command:
            self.runner.cleanup_recovery(remove_backup=True)
            deletions = [call.args[0] for call in command.call_args_list if "rm" in call.args[0]]
            self.assertEqual(deletions, [["docker", "volume", "rm", target + "_blobs"],
                                        ["docker", "volume", "rm", self.project + "_recovery_backup"]])
            self.assertEqual(oneoffs.call_count, 2)
            oneoffs.assert_any_call(initializer=True)


if __name__ == "__main__":
    unittest.main()
