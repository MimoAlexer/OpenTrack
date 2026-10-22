#!/usr/bin/env python3
"""Enforce the implemented Rust layers, including optional and target dependencies."""
import json
from pathlib import Path
import subprocess
import sys
import tomllib


ROOT = Path(__file__).resolve().parents[1]
LAYERS = {
    "opentrack-domain": set(),
    "opentrack-store": {"opentrack-domain"},
    "opentrack-app": {"opentrack-domain", "opentrack-store"},
    "opentrack-adapters": {"opentrack-domain", "opentrack-app"},
    "opentrack-api": {"opentrack-domain", "opentrack-app", "opentrack-adapters"},
    "opentrack-server": {"opentrack-domain", "opentrack-store", "opentrack-app",
                         "opentrack-adapters", "opentrack-api"},
    "opentrack-desktop": set(),
}
TEST_EDGES = {"opentrack-api": {"opentrack-store"}}
DOMAIN_LIBRARIES = {"serde", "serde_json", "uuid", "chrono", "thiserror", "sha2",
                    "base64", "url", "utoipa"}
DATABASE_LIBRARIES = {"sqlx", "postgres", "tokio-postgres", "diesel", "rusqlite"}
TRANSPORT_LIBRARIES = {"axum", "tower", "tower-http", "hyper"}


class BoundaryError(Exception):
    pass


def metadata(root, manifest):
    try:
        result = subprocess.run(
            ["cargo", "metadata", "--no-deps", "--locked", "--format-version", "1",
             "--manifest-path", str(root / manifest)], cwd=root,
            capture_output=True, text=True, timeout=120, check=False)
        if result.returncode:
            raise BoundaryError(f"Cargo metadata failed for {manifest}")
        return json.loads(result.stdout)
    except (OSError, subprocess.TimeoutExpired, ValueError) as error:
        raise BoundaryError(f"Cargo metadata unavailable for {manifest}") from error


def violations(root, inventories):
    errors = []
    packages = []
    for inventory in inventories:
        members = set(inventory["workspace_members"])
        packages.extend(p for p in inventory["packages"] if p["id"] in members)
    names = [package["name"] for package in packages]
    if len(names) != len(set(names)) or set(names) != LAYERS.keys():
        errors.append("Owned Cargo packages changed; review and update the boundary policy")
    for package in packages:
        name = package["name"]
        if name not in LAYERS:
            continue
        if package["edition"] != "2024":
            errors.append(f"{name}: Rust 2024 edition is required")
        for dependency in package["dependencies"]:
            target = dependency["name"]  # Cargo retains the package name for renamed imports.
            kind = dependency["kind"] or "normal"
            allowed = LAYERS[name] | (TEST_EDGES.get(name, set()) if kind == "dev" else set())
            if target in LAYERS or target.startswith("opentrack-") or dependency.get("path"):
                # The audited native GLib patch is a vendor dependency, not a product layer.
                vendor = (name == "opentrack-desktop" and target == "glib"
                          and dependency.get("path")
                          and Path(dependency["path"]).resolve()
                          == (root / "desktop/vendor/glib-0.18.5").resolve())
                expected = (root / ("desktop" if target == "opentrack-desktop"
                                    else f"crates/{target}")).resolve()
                local = dependency.get("path") and Path(dependency["path"]).resolve() == expected
                if not vendor and (target not in allowed or not local or kind == "build"):
                    errors.append(f"{name}: forbidden {kind} dependency on {target}")
            if name == "opentrack-domain" and target not in DOMAIN_LIBRARIES:
                errors.append(f"{name}: {target} is outside the pure domain library allowlist")
            if kind != "dev":
                forbidden = set()
                if name in {"opentrack-api", "opentrack-adapters", "opentrack-desktop"}:
                    forbidden |= DATABASE_LIBRARIES
                if name in {"opentrack-app", "opentrack-store", "opentrack-adapters", "opentrack-desktop"}:
                    forbidden |= TRANSPORT_LIBRARIES
                if target in forbidden:
                    errors.append(f"{name}: {target} crosses its I/O responsibility")
    return errors


def check(root):
    toolchain = tomllib.loads((root / "rust-toolchain.toml").read_text())["toolchain"]
    if not {"clippy", "rustfmt"} <= set(toolchain.get("components", [])):
        raise BoundaryError("The pinned toolchain must include clippy and rustfmt")
    errors = violations(root, [metadata(root, "Cargo.toml"), metadata(root, "desktop/Cargo.toml")])
    if errors:
        raise BoundaryError("\n".join(errors))


def main():
    try:
        check(ROOT)
    except (BoundaryError, OSError, KeyError, TypeError, tomllib.TOMLDecodeError) as error:
        # Cargo diagnostics contain host paths; report only controlled policy messages.
        message = str(error) if isinstance(error, BoundaryError) else "Boundary inputs are invalid"
        print(message, file=sys.stderr)
        return 1
    print("Rust dependency directions, native isolation and Rust 2024 boundaries passed.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
