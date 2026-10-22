#!/usr/bin/env python3
"""Recovery boundary tests; no Docker, database or application is started."""
import argparse
import copy
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import tarfile
import tempfile
import unittest
from unittest.mock import patch


spec = importlib.util.spec_from_file_location("docker_smoke", Path(__file__).with_name("docker-smoke.py"))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


def archive_bytes(entries=()):
    output = io.BytesIO()
    with tarfile.open(fileobj=output, mode="w") as archive:
        root = tarfile.TarInfo(".")
        root.type = tarfile.DIRTYPE
        root.mode = 0o700
        archive.addfile(root)
        for entry in entries:
            archive.addfile(entry, io.BytesIO(b"x" * entry.size) if entry.isfile() else None)
    return output.getvalue()


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
        request_id = "00000000-0000-0000-0000-000000000001"
        self.problem = {"type": "/api/v1/problems/not_found", "title": "Not Found", "status": 404,
            "detail": "This item could not be found", "instance": "urn:uuid:" + request_id,
            "code": "not_found", "request_id": request_id, "retryable": False, "field_errors": []}
        self.problem_headers = {"X-Request-ID": request_id, "Content-Type": "application/problem+json",
                                "Cache-Control": "no-store"}

    def test_compose_excludes_foreign_storage_egress_and_ports(self):
        module.validate_compose(self.config, self.project, 18080)
        changes = [lambda c: c.update(name="opentrack"),
            lambda c: c["volumes"]["database"].update(name="opentrack_database"),
            lambda c: c["volumes"]["database"].update(external=True),
            lambda c: c["volumes"]["database"].update(driver_opts={"type": "none", "o": "bind", "device": "/tmp"}),
            lambda c: c["volumes"].pop("api_secrets"),
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
        def seed(runner, reader, base):
            reader.cookies.set_cookie(module.http.cookiejar.Cookie(version=0, name="opentrack_session",
                value="synthetic-reader-session", port=None, port_specified=False, domain="localhost.local",
                domain_specified=False, domain_initial_dot=False, path="/", path_specified=True,
                secure=False, expires=None, discard=True, comment=None, comment_url=None, rest={}))
            readers.append(reader)
            return {}
        def dc(runner, *arguments, **kwargs):
            if arguments[0] == "config":
                config = copy.deepcopy(self.config)
                config["name"] = runner.project
                for name, volume in config["volumes"].items():
                    volume["name"] = f"{runner.project}_{name}"
                for name, network in config["networks"].items():
                    network["name"] = f"{runner.project}_{name}"
                return argparse.Namespace(stdout=json.dumps(config))
            if arguments[0] == "up":
                starts.append(runner.project)
            return argparse.Namespace(stdout="container" if arguments[0] == "ps" else "")
        def command(arguments, **kwargs):
            states = [{"State": {"Running": False, "ExitCode": 0}}]
            return argparse.Namespace(stdout=json.dumps(states) if arguments[1] == "inspect" else "")
        identifier = "00000000-0000-0000-0000-000000000001"
        with patch.object(module.Qualification, "dc", new=dc), \
                patch.object(module.Qualification, "seed_private_recovery", new=seed), \
                patch.object(module.Qualification, "check_private_revocation"), \
                patch.object(self.runner, "record"), \
                patch.object(self.runner, "command", side_effect=command), \
                patch.object(self.runner, "volume_info", return_value=None), \
                patch.object(self.runner, "container", return_value={"Image": "fixture"}), \
                patch.object(self.runner, "database_fingerprint", return_value=b"fixture"), \
                patch.object(self.runner, "setup_token", return_value="0" * 64), \
                patch.object(self.runner, "sql", return_value=identifier), \
                patch.object(self.runner, "volume_task", side_effect=RuntimeError("snapshot failure")), \
                patch.object(self.runner, "cleanup_recovery", side_effect=RuntimeError("cleanup failure")), \
                patch.object(self.runner, "ready") as ready:
            with self.assertRaisesRegex(RuntimeError, "cleanup failure") as raised:
                self.runner.cold_recovery("fixture@example.test", "fixture", "/issues/fixture",
                                          "/attachments/" + identifier, {}, "fixture")
            self.assertEqual(str(raised.exception.__context__), "snapshot failure")
            self.assertEqual(starts, [self.project])
            ready.assert_called_once()
            self.assertEqual(len(readers), 1)
            self.assertEqual(list(readers[0].cookies), [])

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

    def test_valid_archive_cannot_overwrite_nonempty_target(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            target, backup = root / "volume", root / "backup"
            target.mkdir()
            backup.mkdir()
            existing = target / "keep"
            existing.write_bytes(b"preserve")
            data = archive_bytes()
            (backup / "blobs.tar").write_bytes(data)
            (backup / "blobs.sha256").write_text(hashlib.sha256(data).hexdigest(), encoding="ascii")
            def mapped_path(value):
                return {"/volume": target, "/backup": backup}.get(value, Path(value))
            with patch.object(module, "Path", side_effect=mapped_path), self.assertRaises(RuntimeError):
                module.volume_operation("restore", "blobs")
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
        with patch.object(module.Qualification, "command", side_effect=responses) as command:
            with self.assertRaises(RuntimeError):
                self.runner.cleanup_recovery(remove_backup=True)
            self.assertFalse(any("rm" in call.args[0] or "down" in call.args[0] for call in command.call_args_list))
        owned_volume = json.dumps([{"Name": target + "_blobs", "Labels": {
            module.RECOVERY_LABEL: self.project, "com.docker.compose.project": target,
            "com.docker.compose.volume": "blobs"}}])
        responses = [result(json.dumps(config)), result(owned_backup), result(target + "_blobs\n"), result(owned_volume),
                     result(), result(), result(code=1), result(), result()]
        with patch.object(module.Qualification, "command", side_effect=responses) as command:
            self.runner.cleanup_recovery(remove_backup=True)
            deletions = [call.args[0] for call in command.call_args_list if "rm" in call.args[0]]
            self.assertEqual(deletions, [["docker", "volume", "rm", target + "_blobs"],
                                        ["docker", "volume", "rm", self.project + "_recovery_backup"]])


if __name__ == "__main__":
    unittest.main()
