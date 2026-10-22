#!/usr/bin/env python3
"""Exercise scope decisions against disposable real Git histories."""
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import textwrap
import unittest
from unittest.mock import patch


SCRIPT = Path(__file__).with_name("ci-scope.py")
spec = importlib.util.spec_from_file_location("ci_scope", SCRIPT)
ci_scope = importlib.util.module_from_spec(spec)
spec.loader.exec_module(ci_scope)


class ScopeContracts(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="opentrack-ci-scope-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.repository = self.root / "repository"
        self.repository.mkdir()
        self.git("init", "--initial-branch=main")
        self.git("config", "user.name", "CI scope fixture")
        self.git("config", "user.email", "scope@example.invalid")
        self.git("config", "core.autocrlf", "false")
        self.base = self.commit({"README.md": "Initial documentation\n"})

    def git(self, *arguments):
        return subprocess.run(["git", "-C", str(self.repository), *arguments],
                              capture_output=True, check=True).stdout.decode().strip()

    def commit(self, files):
        for name, content in files.items():
            path = self.repository / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(content, encoding="utf-8")
        self.git("add", "--all")
        self.git("commit", "--quiet", "-m", "Fixture change")
        return self.git("rev-parse", "HEAD")

    def push(self, after, before=None):
        return ci_scope.scope("push", {"before": before or self.base, "after": after}, self.repository)

    def test_markdown_only_push_and_pr_emit_explicit_false(self):
        head = self.commit({"README.md": "Updated\n", "docs/nested/guide.md": "Guide\n"})
        self.assertEqual((self.push(head)["verify"], self.push(head)["docker"]), (False, False))
        pr = {"pull_request": {"base": {"sha": self.base}, "head": {"sha": head}}}
        result = ci_scope.scope("pull_request", pr, self.repository)
        self.assertFalse(result["verify"] or result["docker"])

    def test_source_deployment_workflows_and_scope_changes_require_both_suites(self):
        for path in ("crates/core/src/main.rs", "web/package-lock.json", "deploy/Dockerfile",
                     "compose.yaml", ".github/workflows/ci.yml", ".github/workflows/docker.yml",
                     "scripts/ci-scope.py", "scripts/test-ci-scope.py", ".dockerignore",
                     "config/REFERENCE.md", "crates/core/README.md", "docs/api/openapi.json"):
            with self.subTest(path=path):
                before = self.git("rev-parse", "HEAD")
                after = self.commit({path: "Changed\n"})
                result = self.push(after, before)
                self.assertTrue(result["verify"] and result["docker"])

    def test_desktop_changes_preserve_implementation_checks_without_docker_rebuild(self):
        for path in ("desktop/src/protocol.rs", "desktop/Cargo.lock", "desktop/tests/native_boundary.rs",
                     "desktop/scripts/check.sh", "desktop/vendor/glib-0.18.5/src/variant_iter.rs"):
            with self.subTest(path=path):
                before = self.git("rev-parse", "HEAD")
                result = self.push(self.commit({path: "Changed\n"}), before)
                self.assertTrue(result["verify"])
                self.assertFalse(result["docker"])

    def test_native_inputs_remain_in_cumulative_pr_scope_after_documentation(self):
        for path in ("desktop/src/protocol.rs", "docs/api/openapi.json"):
            with self.subTest(path=path):
                before = self.git("rev-parse", "HEAD")
                self.commit({path: "Changed\n"})
                head = self.commit({"README.md": f"Documentation after {path}\n"})
                result = ci_scope.scope("pull_request", {"pull_request": {
                    "base": {"sha": before}, "head": {"sha": head}}}, self.repository)
                self.assertTrue(result["verify"])

    def test_unknown_surface_conservatively_runs_everything(self):
        result = self.push(self.commit({"future-module/config.bin": "new product surface"}))
        self.assertTrue(result["verify"] and result["docker"])

    def test_manual_and_zero_sha_events_force_full_without_git_history(self):
        missing = self.root / "not-a-repository"
        for event, data in (("workflow_dispatch", {}), ("merge_group", {}),
                            ("push", {"before": "0" * 40, "after": self.base}),
                            ("push", {"before": self.base, "after": "0" * 40})):
            result = ci_scope.scope(event, data, missing)
            self.assertTrue(result["verify"] and result["docker"])

    def test_pr_scope_is_cumulative_not_only_the_last_documentation_commit(self):
        code = self.commit({"crates/core/src/lib.rs": "pub fn changed() {}\n"})
        head = self.commit({"README.md": "Docs after code\n"})
        self.assertFalse(self.push(head, code)["docker"])
        result = ci_scope.scope("pull_request", {"pull_request": {"base": {"sha": self.base}, "head": {"sha": head}}}, self.repository)
        self.assertTrue(result["verify"] and result["docker"])

    def test_pr_merge_base_does_not_treat_unrelated_base_changes_as_pr_changes(self):
        self.git("checkout", "-b", "feature")
        head = self.commit({"docs/change.md": "PR docs\n"})
        self.git("checkout", "main")
        base = self.commit({"crates/core/src/lib.rs": "Base-only change\n"})
        result = ci_scope.scope("pull_request", {"pull_request": {"base": {"sha": base}, "head": {"sha": head}}}, self.repository)
        self.assertFalse(result["verify"] or result["docker"])

    def test_renaming_source_into_documentation_does_not_hide_the_deletion(self):
        base = self.commit({"crates/core/fixture.txt": "Retained content\n"})
        self.git("mv", "crates/core/fixture.txt", "renamed.md")
        self.git("commit", "--quiet", "-m", "Rename code out of source")
        result = self.push(self.git("rev-parse", "HEAD"), base)
        self.assertTrue(result["verify"] and result["docker"])

    def test_nul_path_framing_handles_newline_and_non_utf8_names(self):
        raw = b"docs/line\ncrates/pretend.rs.md\0docs/unusual-\xff.md\0"
        with patch.object(ci_scope, "git", side_effect=[b"", b"", raw]):
            result = self.push(self.base)
        self.assertFalse(result["verify"] or result["docker"])
        self.assertEqual(result["changed_paths"], 2)

    def test_cli_failure_never_writes_skip_outputs(self):
        for event in ({"before": "--output=unsafe", "after": self.base},
                      {"before": "f" * 40, "after": self.base}, {}, []):
            with self.subTest(event=event):
                payload = self.root / "event.json"
                payload.write_text(json.dumps(event), encoding="utf-8")
                output = self.root / "outputs"
                result = subprocess.run([sys.executable, str(SCRIPT), "--event-name", "push",
                                         "--event-path", str(payload), "--repository", str(self.repository),
                                         "--github-output", str(output)], capture_output=True)
                self.assertNotEqual(result.returncode, 0)
                self.assertFalse(output.exists())

    def test_diff_error_and_unterminated_output_fail_closed(self):
        for fault in (ci_scope.ScopeError("diff failed"), b"README.md"):
            with self.subTest(fault=fault), patch.object(ci_scope, "git", side_effect=[b"", b"", fault]):
                with self.assertRaises(ci_scope.ScopeError):
                    self.push(self.base)

    def test_cli_success_writes_both_explicit_boolean_outputs(self):
        head = self.commit({"docs/guide.md": "Documentation\n"})
        payload, output = self.root / "event.json", self.root / "outputs"
        payload.write_text(json.dumps({"before": self.base, "after": head}), encoding="utf-8")
        result = subprocess.run([sys.executable, str(SCRIPT), "--event-name", "push", "--event-path", str(payload),
                                 "--repository", str(self.repository), "--github-output", str(output)], capture_output=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(output.read_text(), "verify=false\ndocker=false\n")

    def test_required_jobs_report_and_their_actual_shell_guards_fail_closed(self):
        bash = shutil.which("bash")
        if os.name == "nt":
            candidate = Path(shutil.which("git")).parents[1] / "bin/bash.exe"
            bash = str(candidate) if candidate.is_file() else bash
        self.assertIsNotNone(bash, "Workflow guard tests require Bash (included with Git for Windows)")
        for filename, job in (("ci.yml", "verify"), ("ci.yml", "native-contracts"),
                              ("docker.yml", "runtime")):
            with self.subTest(workflow=filename, job=job):
                workflow = (SCRIPT.parent.parent / ".github/workflows" / filename).read_text()
                triggers = workflow.split("permissions:", 1)[0]
                self.assertNotRegex(triggers, r"(?m)^\s+(?:paths|paths-ignore):")
                self.assertIn("  push:\n    branches: [main]\n  pull_request:\n", triggers)
                block = workflow.split(f"  {job}:\n", 1)[1]
                self.assertRegex(block, r"(?m)^    needs: scope$")
                self.assertRegex(block, r"(?m)^    if: \$\{\{ always\(\) \}\}$")
                guard = block.split("      - name: Require a successful explicit scope decision\n", 1)[1]
                guard = textwrap.dedent(guard.split("        run: |\n", 1)[1].split("      - name:", 1)[0])
                for result, decision, expected in (("success", "true", 0), ("success", "false", 0),
                                                   ("failure", "false", 1), ("cancelled", "true", 1),
                                                   ("skipped", "false", 1), ("success", "", 1),
                                                   ("success", "unknown", 1)):
                    environment = dict(os.environ, SCOPE_RESULT=result, RUN_FULL=decision)
                    executed = subprocess.run([bash, "-c", guard], env=environment,
                                              capture_output=True, timeout=10)
                    self.assertEqual(executed.returncode, expected,
                                     f"{filename}/{job}: {result}/{decision}: {executed.stderr!r}")


if __name__ == "__main__":
    unittest.main()
