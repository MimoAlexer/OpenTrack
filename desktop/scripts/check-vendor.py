#!/usr/bin/env python3
"""Verify pristine upstream file hashes after reversing only the approved fix."""
from pathlib import Path
import hashlib
import json
root = Path(__file__).resolve().parents[1] / "vendor"
manifest = json.loads((root / "glib-source-manifest.json").read_text())
source = root / "glib-0.18.5"
actual = {path.relative_to(source).as_posix() for path in source.rglob("*") if path.is_file()}
expected = set(manifest["original_files"])
if actual != expected:
    raise SystemExit("Vendored GLib file inventory differs from the official crate.")
for name, expected_hash in manifest["original_files"].items():
    # Git may check text files out with CRLF on Windows. The upstream crate
    # hashes use LF; normalize only that checkout transformation before hashing.
    data = (source / name).read_bytes().replace(b"\r\n", b"\n")
    if name == "src/variant_iter.rs":
        text = data.decode()
        before = "let mut p: *mut libc::c_char = std::ptr::null_mut();"
        after = "let p: *mut libc::c_char = std::ptr::null_mut();"
        if text.count(before) != 1 or text.count("                &mut p,") != 1:
            raise SystemExit("The approved GLib output-pointer repair is absent or changed.")
        data = text.replace(before, after).replace("                &mut p,", "                &p,").encode()
    if hashlib.sha256(data).hexdigest() != expected_hash:
        raise SystemExit(f"Unexpected vendored GLib modification: {name}")
print(f"Verified {len(expected)} upstream GLib files and the exact two-line repair.")
