#!/usr/bin/env python3
"""Exercise check.sh modes and failure propagation without compiling Rust."""
import os
from pathlib import Path
import shutil
import subprocess
import unittest


SCRIPT = Path(__file__).with_name("check.sh")
HARNESS = r'''
python3() {
  case "$1" in
    */prepare-linux-sysroot.py) echo overlay ;;
    */check-vendor.py) echo vendor ;;
    *) return 99 ;;
  esac
  [ "${FAIL_COMMAND:-}" != python3 ]
}
pkg-config() {
  echo "pkg-config $*"
  [ "${FAIL_COMMAND:-}" != pkg-config ]
}
cargo() {
  echo "cargo $*"
  [ "${FAIL_COMMAND:-}" != "cargo $1" ]
}
source "$CHECK_SCRIPT" "$@"
'''


class CheckModes(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.bash = shutil.which("bash")
        if os.name == "nt":
            candidate = Path(shutil.which("git")).parents[1] / "bin/bash.exe"
            if candidate.is_file():
                cls.bash = str(candidate)
        if cls.bash is None:
            raise RuntimeError("Check mode tests require Bash (included with Git for Windows)")

    def run_check(self, *arguments, fail=""):
        result = subprocess.run(
            [self.bash, "-c", HARNESS, "check-mode-test", *arguments],
            env=dict(os.environ, CHECK_SCRIPT=SCRIPT.as_posix(), FAIL_COMMAND=fail),
            capture_output=True, text=True, timeout=10,
        )
        return result, result.stdout.splitlines()

    def test_default_retains_overlay_and_full_shell_clippy(self):
        result, calls = self.run_check()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(calls[:2], ["overlay", "vendor"])
        clippy = next(call for call in calls if call.startswith("cargo clippy "))
        self.assertIn("--all-targets", clippy)
        self.assertNotIn("--no-default-features", clippy)
        self.assert_complete_locked_tests(calls)

    def test_core_checks_host_glib_and_vendor_before_complete_core_suite(self):
        result, calls = self.run_check("--core")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(calls[:2], [
            "pkg-config --print-errors --exists glib-2.0 gobject-2.0 gio-2.0", "vendor"])
        self.assertNotIn("overlay", calls)
        self.assertTrue(calls[2].startswith("cargo fmt "))
        self.assertTrue(calls[2].endswith(" --check"))
        clippy = next(call for call in calls if call.startswith("cargo clippy "))
        self.assertIn("--all-targets --no-default-features --locked", clippy)
        self.assert_complete_locked_tests(calls)

    def assert_complete_locked_tests(self, calls):
        # Exact trailing arguments prevent a misspelled name filter from making
        # the request-policy or integration suites silently run zero tests.
        self.assertRegex(calls[-1], r"^cargo test --manifest-path .+/Cargo.toml --no-default-features --locked -j 2$")

    def test_unknown_or_extra_arguments_fail_before_any_check(self):
        for arguments in (("--unknown",), ("--core", "--skip-tests")):
            with self.subTest(arguments=arguments):
                result, calls = self.run_check(*arguments)
                self.assertEqual(result.returncode, 2)
                self.assertFalse(calls)

    def test_missing_core_libraries_fail_before_vendor_or_build(self):
        result, calls = self.run_check("--core", fail="pkg-config")
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(len(calls), 1)

    def test_failed_vendor_verification_never_compiles(self):
        result, calls = self.run_check("--core", fail="python3")
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(calls[-1], "vendor")
        self.assertFalse(any(call.startswith("cargo ") for call in calls))

    def test_each_cargo_failure_stops_and_fails_the_entrypoint(self):
        for command in ("fmt", "clippy", "test"):
            with self.subTest(command=command):
                result, calls = self.run_check("--core", fail=f"cargo {command}")
                self.assertNotEqual(result.returncode, 0)
                self.assertTrue(calls[-1].startswith(f"cargo {command} "))


if __name__ == "__main__":
    unittest.main()
