#!/usr/bin/env python3
"""Mutation regressions for declared Rust dependency boundaries."""
from copy import deepcopy
import importlib.util
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch


SPEC = importlib.util.spec_from_file_location(
    "boundaries", Path(__file__).with_name("check-workspace-boundaries.py"))
boundaries = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(boundaries)
ROOT = Path("fixture").resolve()


def dependency(name, kind=None, **extra):
    return dict(name=name, kind=kind, path=None, **extra)


def local(name, kind=None, **extra):
    value = dependency(name, kind, **extra)
    value["path"] = str(ROOT / "crates" / name)
    return value


def inventory():
    packages = [dict(id=name, name=name, edition="2024", dependencies=[])
                for name in boundaries.LAYERS]
    return dict(packages=packages, workspace_members=[p["id"] for p in packages])


class BoundaryContracts(unittest.TestCase):
    def setUp(self):
        self.graph = inventory()
        self.packages = {p["name"]: p for p in self.graph["packages"]}

    def errors(self, source=None, edge=None):
        if edge:
            self.packages[source]["dependencies"].append(edge)
        return boundaries.violations(ROOT, [self.graph])

    def test_all_allowed_directions_and_test_only_api_store_edge(self):
        for source, targets in boundaries.LAYERS.items():
            for target in targets:
                self.packages[source]["dependencies"].append(local(target))
        self.packages["opentrack-api"]["dependencies"].append(local("opentrack-store", "dev"))
        self.assertEqual(self.errors(), [])

    def test_rejects_every_inverted_or_native_first_party_edge(self):
        for source in boundaries.LAYERS:
            for target in boundaries.LAYERS:
                if target in boundaries.LAYERS[source]:
                    continue
                graph = deepcopy(self.graph)
                next(p for p in graph["packages"] if p["name"] == source)["dependencies"] = [local(target)]
                with self.subTest(source=source, target=target):
                    self.assertTrue(boundaries.violations(ROOT, [graph]))

    def test_optional_target_and_renamed_edges_do_not_bypass_policy(self):
        for options in ({"optional": True}, {"target": "cfg(windows)"}, {"rename": "harmless"}):
            with self.subTest(options=options):
                self.assertTrue(self.errors("opentrack-domain", local("opentrack-store", **options)))
                self.packages["opentrack-domain"]["dependencies"].clear()

    def test_dev_exception_never_applies_to_normal_or_build_dependencies(self):
        for kind in (None, "build"):
            self.assertTrue(self.errors("opentrack-api", local("opentrack-store", kind)))
            self.packages["opentrack-api"]["dependencies"].clear()
        self.assertTrue(self.errors("opentrack-app", local("opentrack-domain", "build")))

    def test_missing_duplicate_or_new_product_packages_fail_closed(self):
        for operation in ("remove", "duplicate", "add"):
            graph = deepcopy(self.graph)
            if operation == "remove":
                graph["packages"].pop()
            elif operation == "duplicate":
                graph["packages"].append(deepcopy(graph["packages"][0]))
            else:
                graph["packages"].append(dict(id="new", name="new", edition="2024", dependencies=[]))
                graph["workspace_members"].append("new")
            with self.subTest(operation=operation):
                self.assertTrue(boundaries.violations(ROOT, [graph]))

    def test_unknown_local_packages_and_registry_layer_impostors_are_rejected(self):
        unknown = dependency("new-provider")
        unknown["path"] = str(ROOT / "other")
        self.assertTrue(self.errors("opentrack-adapters", unknown))
        self.assertTrue(self.errors("opentrack-app", dependency("opentrack-domain")))

    def test_pure_domain_and_transport_database_ownership(self):
        for source, target in (("opentrack-domain", "tokio"), ("opentrack-api", "sqlx"),
                               ("opentrack-app", "axum"), ("opentrack-store", "tower"),
                               ("opentrack-adapters", "axum"), ("opentrack-adapters", "sqlx"),
                               ("opentrack-desktop", "sqlx")):
            with self.subTest(source=source, target=target):
                self.assertTrue(self.errors(source, dependency(target)))
                self.packages[source]["dependencies"].clear()

    def test_owned_packages_require_rust_2024(self):
        for package in self.graph["packages"]:
            package["edition"] = "2021"
        self.assertEqual(len(self.errors()), len(boundaries.LAYERS))

    def test_metadata_is_locked_includes_all_declared_edges_and_sanitizes_failures(self):
        with patch.object(boundaries.subprocess, "run") as run:
            run.return_value = subprocess.CompletedProcess([], 0, '{"packages":[]}', "")
            self.assertEqual(boundaries.metadata(ROOT, "Cargo.toml"), {"packages": []})
            arguments = run.call_args.args[0]
            self.assertIn("--locked", arguments)
            self.assertIn("--no-deps", arguments)
            self.assertNotIn("--filter-platform", arguments)
            run.return_value = subprocess.CompletedProcess([], 1, "", "private host diagnostics")
            with self.assertRaisesRegex(boundaries.BoundaryError, "^Cargo metadata failed for Cargo.toml$"):
                boundaries.metadata(ROOT, "Cargo.toml")

    def test_toolchain_requires_format_and_lint_components(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "rust-toolchain.toml").write_text('[toolchain]\ncomponents=["rustfmt"]\n')
            with self.assertRaisesRegex(boundaries.BoundaryError, "clippy and rustfmt"):
                boundaries.check(root)


if __name__ == "__main__":
    unittest.main()
