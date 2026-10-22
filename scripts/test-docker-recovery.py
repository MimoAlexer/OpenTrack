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
            "networks": {name: {"internal": True, "name": f"{self.project}_{name}"} for name in ("database", "web")},
            "services": {"app": {"networks": {"database": {}, "web": {}},
                "ports": [{"host_ip": "127.0.0.1", "published": "18080", "target": 8080}]}, "database": {}}}

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
            lambda c: c["services"]["app"]["ports"][0].update(host_ip="0.0.0.0"),
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

    def test_actual_network_state_must_match_internal_project_inventory(self):
        container = {"Config": {"Labels": {"com.docker.compose.project": self.project}},
                     "HostConfig": {"NetworkMode": self.project + "_web"},
                     "NetworkSettings": {"Networks": {self.project + "_web": {"NetworkID": "network"}}}}
        network = {"Name": self.project + "_web", "Internal": True,
                   "Labels": {"com.docker.compose.project": self.project}}
        def result(value):
            return argparse.Namespace(stdout=json.dumps(value))
        with patch.object(self.runner, "command", side_effect=[argparse.Namespace(stdout="container"),
                result([container]), result([network])]):
            self.runner.verify_offline_runtime(self.config)
        for change in (lambda c, n: n.update(Internal=False),
                       lambda c, n: n["Labels"].update({"com.docker.compose.project": "foreign"}),
                       lambda c, n: c["HostConfig"].update(NetworkMode="host"),
                       lambda c, n: c["NetworkSettings"]["Networks"].update(foreign={"NetworkID": "other"})):
            current, actual = copy.deepcopy(container), copy.deepcopy(network)
            change(current, actual)
            with patch.object(self.runner, "command", side_effect=[argparse.Namespace(stdout="container"),
                    result([current]), result([actual])]), self.assertRaises(RuntimeError):
                self.runner.verify_offline_runtime(self.config)

    def test_source_restart_is_attempted_even_when_cleanup_fails(self):
        starts = []
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
