#!/usr/bin/env python3
"""Verify exact approved GLib patches against the complete upstream inventory."""
from pathlib import Path
import hashlib
import json
import shutil
import subprocess
import tempfile


class VendorError(Exception):
    pass


def normalized(path):
    # Git for Windows may transform line endings during checkout.
    return path.read_bytes().replace(b"\r\n", b"\n")


def verify(root):
    manifest = json.loads((root / "glib-source-manifest.json").read_text())
    source = root / "glib-0.18.5"
    expected = manifest["original_files"]
    if any(path.is_symlink() for path in source.rglob("*")):
        raise VendorError("Vendored GLib sources must be regular files, not symbolic links.")
    actual = {path.relative_to(source).as_posix()
              for path in source.rglob("*") if path.is_file()}
    if actual != set(expected):
        raise VendorError("Vendored GLib file inventory differs from the official crate.")
    patch = normalized(root / "glib-approved.patch")
    if hashlib.sha256(patch).hexdigest() != manifest["approved_patch_sha256"]:
        raise VendorError("The approved GLib patch digest differs from the manifest.")

    # Reverse the complete, reviewable patch in a disposable copy. Every resulting
    # file must match the original crate, including files untouched by the patch.
    # The source checkout is never modified by verification.
    with tempfile.TemporaryDirectory(prefix="opentrack-vendor-check-") as directory:
        pristine = Path(directory) / "source"
        pristine.mkdir()
        for name in expected:
            target = pristine / name
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(normalized(source / name))
        patch_path = Path(directory) / "approved.patch"
        patch_path.write_bytes(patch)
        result = subprocess.run(
            ["git", "apply", "--reverse", "--whitespace=nowarn", str(patch_path)],
            cwd=pristine, capture_output=True, timeout=30, check=False,
        )
        if result.returncode:
            raise VendorError("The approved GLib patch cannot be reversed exactly.")
        restored = {path.relative_to(pristine).as_posix()
                    for path in pristine.rglob("*") if path.is_file()}
        if restored != set(expected):
            raise VendorError("The approved GLib patch changed the upstream file inventory.")
        for name, expected_hash in expected.items():
            if hashlib.sha256((pristine / name).read_bytes()).hexdigest() != expected_hash:
                raise VendorError(f"Unexpected vendored GLib modification: {name}")
    return len(expected)


def main():
    if shutil.which("git") is None:
        raise SystemExit("Vendor verification requires Git.")
    try:
        count = verify(Path(__file__).resolve().parents[1] / "vendor")
    except (VendorError, OSError, ValueError, subprocess.TimeoutExpired) as error:
        raise SystemExit(str(error)) from error
    print(f"Verified {count} upstream GLib files and the exact approved patch.")


if __name__ == "__main__":
    main()
