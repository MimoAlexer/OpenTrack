"""Structural installation only: synthetic bytes, private HOME/XDG, no app launch."""
import hashlib
import json
import os
from pathlib import Path
import shutil
import stat
import subprocess
import sys
import tempfile
import unittest

SCRIPT = Path(__file__).with_name("install-user-linux.py")
VALIDATOR = shutil.which("desktop-file-validate")
OLD = b"\x00opaque synthetic legacy artifact\xfe"
SELECTED = b"\x00opaque synthetic selected artifact\xff"
LICENSE = b"Synthetic project license fixture.\n"


class PrivateUserInstall(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="opentrack-installer-proposal-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.desktop = self.root / "desktop"
        self.license_source = self.root / "LICENSE"
        self.license_source.write_bytes(LICENSE)
        (self.desktop / "scripts").mkdir(parents=True)
        (self.desktop / "icons").mkdir()
        self.installer = self.desktop / "scripts/install-user-linux.py"
        shutil.copyfile(SCRIPT, self.installer)
        shutil.copyfile(SCRIPT.resolve().parents[1] / "icons/icon.png", self.desktop / "icons/icon.png")
        self.legacy = self.desktop / "target/release/opentrack-desktop"
        self.legacy.parent.mkdir(parents=True)
        self.legacy.write_bytes(OLD)
        self.selected = self.root / "separate build/release/opentrack-desktop"
        self.selected.parent.mkdir(parents=True)
        self.selected.write_bytes(SELECTED)
        self.data = self.root / 'private data % $installer "quoted"'
        self.data.mkdir(mode=0o700)
        self.home = self.root / "home"
        self.home.mkdir(mode=0o700)
        self.bin = self.root / "tools"
        self.bin.mkdir()
        if VALIDATOR:
            (self.bin / "desktop-file-validate").symlink_to(VALIDATOR)
        self.env = {"PATH":str(self.bin), "HOME":str(self.home), "XDG_DATA_HOME":str(self.data),
                    "XDG_CONFIG_HOME":str(self.root / "config"), "XDG_CACHE_HOME":str(self.root / "cache"),
                    "TMPDIR":str(self.root), "LANG":"C", "installer":"MUST_NOT_EXPAND",
                    "CARGO_TARGET_DIR":str(self.root / "unrelated-cargo-output")}
        self.destination = self.data / "opentrack/bin/opentrack-desktop"
        self.license = self.data / "opentrack/LICENSE"
        self.icon = self.data / "icons/hicolor/256x256/apps/app.opentrack.desktop.png"
        self.launcher = self.data / "applications/app.opentrack.desktop.desktop"

    def run_install(self, *arguments):
        result = subprocess.run([sys.executable, str(self.installer), *map(str, arguments)],
                                cwd=self.root, env=self.env, capture_output=True, text=True, timeout=15)
        self.assertNotIn(str(self.root), result.stdout + result.stderr, "Installer output disclosed private paths")
        self.assertNotIn("Traceback", result.stderr)
        self.assertFalse(list(self.data.rglob(".opentrack-*")), "Atomic-copy temporary files survived")
        self.assertFalse(list(self.root.glob("opentrack-launcher-*")), "Prepared launcher directory survived")
        return result

    def snapshot(self):
        return {str(path.relative_to(self.data)): (path.read_bytes(), stat.S_IMODE(path.stat().st_mode))
                for path in self.data.rglob("*") if path.is_file()}

    def control_program(self, name, text):
        path = self.bin / name
        path.unlink(missing_ok=True)
        path.write_text("#!/bin/sh\n" + text)
        path.chmod(0o700)

    def assert_identity(self, expected):
        self.assertEqual(hashlib.sha256(self.destination.read_bytes()).digest(), hashlib.sha256(expected).digest())
        self.assertEqual(self.icon.read_bytes(), (self.desktop / "icons/icon.png").read_bytes())
        self.assertEqual(self.license.read_bytes(), self.license_source.read_bytes())
        for path, mode in [(self.destination, 0o755), (self.icon, 0o644), (self.license, 0o644), (self.launcher, 0o644)]:
            self.assertEqual(stat.S_IMODE(path.stat().st_mode), mode)
        entry = self.launcher.read_text()
        for line in ["Name=OpenTrack", "StartupWMClass=opentrack-desktop",
                     "MimeType=x-scheme-handler/opentrack;", "X-OpenTrack-Managed=true"]:
            self.assertIn(line + "\n", entry)
        executable = next(line.removeprefix("Exec=") for line in entry.splitlines() if line.startswith("Exec="))
        # Decode Desktop Entry key escaping, then use the real shell's quoting parser without executing the artifact.
        command = "set -- " + executable.replace("\\\\", "\\") + '; printf "%s\\0" "$@"'
        parsed = subprocess.run(["/bin/sh", "-c", command], env=self.env, capture_output=True, timeout=5)
        self.assertEqual(parsed.returncode, 0)
        arguments = parsed.stdout.rstrip(b"\0").split(b"\0")
        self.assertEqual(arguments[0].replace(b"%%", b"%"), os.fsencode(self.destination))
        self.assertEqual(arguments[1:], [b"%u"])
        self.assertEqual(next(line for line in entry.splitlines() if line.startswith("Icon=")), "Icon=" + str(self.icon))
        if VALIDATOR:
            validation = subprocess.run([VALIDATOR, str(self.launcher)], capture_output=True, timeout=5)
            self.assertEqual(validation.returncode, 0, "Real desktop-file-validate rejected the installed launcher")

    def test_explicit_source_wins_and_preserves_identity_and_escaping(self):
        result = self.run_install("--binary", self.selected)
        self.assertEqual(result.returncode, 0)
        self.assert_identity(SELECTED)
        self.assertEqual(self.legacy.read_bytes(), OLD)

    def test_legacy_default_still_installs_without_cargo_discovery(self):
        self.assertEqual(self.run_install().returncode, 0)
        self.assert_identity(OLD)

    def test_managed_update_preserves_unrelated_private_data(self):
        self.assertEqual(self.run_install().returncode, 0)
        sentinel = self.data / "keep-private-data"
        sentinel.write_bytes(b"unrelated private fixture")
        self.license_source.write_bytes(b"Updated synthetic project license fixture.\n")
        self.assertEqual(self.run_install("--binary", self.selected).returncode, 0)
        self.assert_identity(SELECTED)
        self.assertEqual(sentinel.read_bytes(), b"unrelated private fixture")

    def test_unmanaged_launcher_refuses_without_changing_any_destination(self):
        for path, content in [(self.destination,b"unmanaged executable"), (self.icon,b"unmanaged icon"),
                              (self.launcher,b"\xffunmanaged launcher")]:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(content)
        before = self.snapshot()
        self.assertNotEqual(self.run_install("--binary", self.selected).returncode, 0)
        self.assertEqual(self.snapshot(), before)

    def test_executable_without_managed_launcher_is_not_replaced(self):
        for path in [self.destination, self.icon]:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(b"unmanaged sentinel")
        before = self.snapshot()
        self.assertNotEqual(self.run_install("--binary", self.selected).returncode, 0)
        self.assertEqual(self.snapshot(), before)

    def test_license_without_managed_launcher_is_not_replaced(self):
        self.license.parent.mkdir(parents=True)
        self.license.write_bytes(b"unmanaged license fixture")
        before = self.snapshot()
        self.assertNotEqual(self.run_install("--binary", self.selected).returncode, 0)
        self.assertEqual(self.snapshot(), before)
        self.license.unlink()
        self.license.symlink_to(self.root / "missing-license-target")
        self.assertNotEqual(self.run_install("--binary", self.selected).returncode, 0)
        self.assertTrue(self.license.is_symlink(), "An unmanaged dangling link must remain unchanged")
        self.assertFalse(self.destination.exists())

    def test_managed_license_update_replaces_a_leaf_link_without_following_it(self):
        self.assertEqual(self.run_install().returncode, 0)
        unrelated = self.root / "unrelated-license"
        unrelated.write_bytes(b"preserved unrelated fixture")
        self.license.unlink()
        self.license.symlink_to(unrelated)
        self.assertEqual(self.run_install("--binary", self.selected).returncode, 0)
        self.assert_identity(SELECTED)
        self.assertFalse(self.license.is_symlink())
        self.assertEqual(unrelated.read_bytes(), b"preserved unrelated fixture")

    def test_invalid_project_license_preserves_the_existing_install(self):
        self.assertEqual(self.run_install().returncode, 0)
        before = self.snapshot()
        for kind in ["missing", "directory", "symlink"]:
            with self.subTest(kind=kind):
                self.license_source.unlink()
                if kind == "directory":
                    self.license_source.mkdir()
                elif kind == "symlink":
                    self.license_source.symlink_to(self.selected)
                self.assertNotEqual(self.run_install("--binary", self.selected).returncode, 0)
                self.assertEqual(self.snapshot(), before)
                if kind == "directory":
                    self.license_source.rmdir()
                elif kind == "symlink":
                    self.license_source.unlink()
                self.license_source.write_bytes(LICENSE)

    def test_unreadable_project_license_fails_before_installation_writes(self):
        if os.getuid() == 0:
            self.skipTest("Read-denial control requires a non-root test user")
        self.assertEqual(self.run_install().returncode, 0)
        before = self.snapshot()
        self.license_source.chmod(0)
        try:
            result = self.run_install("--binary", self.selected)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("could not be completed", result.stderr)
        finally:
            self.license_source.chmod(0o644)
        self.assertEqual(self.snapshot(), before)

    def test_unreadable_existing_launcher_refuses_without_disclosing_paths(self):
        if os.getuid() == 0:
            self.skipTest("Read-denial control requires a non-root test user")
        self.launcher.parent.mkdir(parents=True)
        self.launcher.write_bytes(b"X-OpenTrack-Managed=true\n")
        before = self.snapshot()
        mode = stat.S_IMODE(self.launcher.stat().st_mode)
        self.launcher.chmod(0)
        try:
            result = self.run_install("--binary", self.selected)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("Existing user installation could not be inspected", result.stderr)
        finally:
            self.launcher.chmod(mode)
        self.assertEqual(self.snapshot(), before)

    def test_unsearchable_launcher_parent_refuses_before_changing_files(self):
        if os.getuid() == 0:
            self.skipTest("Read-denial control requires a non-root test user")
        self.launcher.parent.mkdir(parents=True)
        self.launcher.write_bytes(b"unmanaged launcher\n")
        self.icon.parent.mkdir(parents=True)
        self.icon.write_bytes(b"preserve existing icon")
        before = self.snapshot()
        mode = stat.S_IMODE(self.launcher.parent.stat().st_mode)
        self.launcher.parent.chmod(0)
        try:
            self.assertNotEqual(self.run_install("--binary", self.selected).returncode, 0)
        finally:
            self.launcher.parent.chmod(mode)
        self.assertEqual(self.snapshot(), before)

    def test_invalid_explicit_source_does_not_fall_back_or_disclose_paths(self):
        alias = self.root / "linked-artifact"
        alias.symlink_to(self.selected)
        values = ["relative/release/client", self.root / "missing-artifact", self.root, alias]
        for character in ["\n", "\r", "\t", "\x7f", "\x85"]:
            path = self.root / ("private" + character + "artifact")
            path.write_bytes(SELECTED)
            values.append(path)
        for value in values:
            with self.subTest(case="invalid explicit source"):
                before = self.snapshot()
                self.assertNotEqual(self.run_install("--binary", value).returncode, 0)
                self.assertEqual(self.snapshot(), before)
        for arguments in [("--binary",), ("--unknown", str(self.selected))]:
            self.assertNotEqual(self.run_install(*arguments).returncode, 0)
            self.assertEqual(self.snapshot(), {})

    def test_unsearchable_source_parent_refuses_without_disclosing_paths(self):
        if os.getuid() == 0:
            self.skipTest("Read-denial control requires a non-root test user")
        mode = stat.S_IMODE(self.selected.parent.stat().st_mode)
        self.selected.parent.chmod(0)
        try:
            self.assertNotEqual(self.run_install("--binary", self.selected).returncode, 0)
            self.assertEqual(self.snapshot(), {})
        finally:
            self.selected.parent.chmod(mode)

    def test_substituted_validator_failure_is_private_and_precedes_writes(self):
        self.control_program("desktop-file-validate", 'printf "private validator: %s\\n" "$1" >&2\nexit 7\n')
        result = self.run_install("--binary", self.selected)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Desktop launcher validation failed", result.stderr)
        self.assertEqual(self.snapshot(), {})

    def test_substituted_cache_failure_retains_complete_install(self):
        self.control_program("update-desktop-database", 'printf "private cache: %s\\n" "$1" >&2\nexit 7\n')
        result = self.run_install("--binary", self.selected)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Launcher cache update failed", result.stderr)
        self.assert_identity(SELECTED)

    def test_copy_read_failure_removes_temporary_file_and_reports_generically(self):
        if os.getuid() == 0:
            self.skipTest("Read-denial control requires a non-root test user")
        self.assertEqual(self.run_install().returncode, 0)
        before = self.snapshot()
        self.selected.chmod(0)
        result = self.run_install("--binary", self.selected)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("could not be completed", result.stderr)
        self.assertEqual(self.snapshot(), before)


if __name__ == "__main__":
    suite = unittest.defaultTestLoader.loadTestsFromTestCase(PrivateUserInstall)
    result = unittest.TextTestRunner().run(suite)
    receipt = {"status":"passed" if result.wasSuccessful() else "failed", "tests":result.testsRun,
               "python_version":sys.version.split()[0],
               "failures":len(result.failures), "errors":len(result.errors), "skipped":len(result.skipped),
               "structural_install_only":True, "artifact":"opaque synthetic bytes, never executed",
               "desktop_file_validate":"real tool exercised" if VALIDATOR else "not available; not qualified",
               "substituted_external_programs":"validator/cache failure controls only; explicitly synthetic",
               "global_associations_changed":False, "genuine_binary_or_native_runtime_qualified":False}
    if destination := os.environ.get("OPENTRACK_INSTALLER_TEST_RECEIPT"):
        Path(destination).write_text(json.dumps(receipt, indent=2) + "\n")
    print(json.dumps(receipt))
    raise SystemExit(0 if result.wasSuccessful() else 1)
