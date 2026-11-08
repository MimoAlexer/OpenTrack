#!/usr/bin/env python3
"""Verify provenance failures cannot silently approve changed native sources."""
import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest


spec = importlib.util.spec_from_file_location("check_vendor", Path(__file__).with_name("check-vendor.py"))
checker = importlib.util.module_from_spec(spec)
spec.loader.exec_module(checker)

PATCH = b"--- a/src/example.rs\n+++ b/src/example.rs\n@@ -1 +1 @@\n-old\n+repaired\n"


class VendorProvenance(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.source = self.root / "glib-0.18.5"
        (self.source / "src").mkdir(parents=True)
        (self.source / "src/example.rs").write_bytes(b"repaired\n")
        (self.source / "LICENSE").write_bytes(b"license\n")
        self.manifest = {
            "original_files": {
                "src/example.rs": hashlib.sha256(b"old\n").hexdigest(),
                "LICENSE": hashlib.sha256(b"license\n").hexdigest(),
            },
            "approved_patch_sha256": hashlib.sha256(PATCH).hexdigest(),
        }
        self.save_manifest()
        (self.root / "glib-approved.patch").write_bytes(PATCH)

    def save_manifest(self):
        (self.root / "glib-source-manifest.json").write_text(json.dumps(self.manifest))

    def test_exact_patch_restores_every_original_hash_without_changing_checkout(self):
        before = {p: p.read_bytes() for p in self.source.rglob("*") if p.is_file()}
        self.assertEqual(checker.verify(self.root), 2)
        self.assertEqual(before, {p: p.read_bytes() for p in before})

    def test_windows_checkout_line_endings_remain_supported(self):
        for path in (self.source / "src/example.rs", self.source / "LICENSE", self.root / "glib-approved.patch"):
            path.write_bytes(path.read_bytes().replace(b"\n", b"\r\n"))
        self.assertEqual(checker.verify(self.root), 2)

    def test_missing_required_repair_fails(self):
        (self.source / "src/example.rs").write_bytes(b"old\n")
        with self.assertRaises(checker.VendorError):
            checker.verify(self.root)

    def test_extra_source_and_missing_original_both_fail(self):
        extra = self.source / "src/extra.rs"
        extra.write_bytes(b"extra\n")
        with self.assertRaises(checker.VendorError):
            checker.verify(self.root)
        extra.unlink()
        (self.source / "LICENSE").unlink()
        with self.assertRaises(checker.VendorError):
            checker.verify(self.root)

    def test_unrelated_modification_cannot_hide_behind_an_approved_patch(self):
        (self.source / "LICENSE").write_bytes(b"changed\n")
        with self.assertRaisesRegex(checker.VendorError, "LICENSE"):
            checker.verify(self.root)

    def test_changed_patch_requires_explicit_manifest_review(self):
        (self.root / "glib-approved.patch").write_bytes(PATCH + b"\n")
        with self.assertRaisesRegex(checker.VendorError, "digest"):
            checker.verify(self.root)

    def test_external_symlink_cannot_substitute_even_identical_source_bytes(self):
        outside = self.root / "external.rs"
        outside.write_bytes(b"repaired\n")
        path = self.source / "src/example.rs"
        path.unlink()
        try:
            path.symlink_to(outside)
        except OSError as error:
            self.skipTest(f"Symbolic links unavailable: {error}")
        with self.assertRaisesRegex(checker.VendorError, "symbolic links"):
            checker.verify(self.root)

    def test_self_consistent_patch_still_must_restore_the_official_crate(self):
        patch = PATCH.replace(b"-old", b"-invented")
        (self.root / "glib-approved.patch").write_bytes(patch)
        self.manifest["approved_patch_sha256"] = hashlib.sha256(patch).hexdigest()
        self.save_manifest()
        with self.assertRaisesRegex(checker.VendorError, "example.rs"):
            checker.verify(self.root)


if __name__ == "__main__":
    unittest.main()
