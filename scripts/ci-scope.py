#!/usr/bin/env python3
"""Classify a verified GitHub commit range without suppressing required jobs."""
import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import sys


class ScopeError(Exception):
    pass


def git(repository, *arguments):
    try:
        result = subprocess.run(["git", "-C", str(repository), *arguments],
                                capture_output=True, timeout=60, check=False)
    except (OSError, subprocess.TimeoutExpired) as error:
        raise ScopeError("Git inspection was unavailable") from error
    if result.returncode:
        raise ScopeError("Git could not verify the requested commit range")
    return result.stdout


def sha(value):
    if not isinstance(value, str) or not re.fullmatch(r"(?:[0-9a-fA-F]{40}|[0-9a-fA-F]{64})", value):
        raise ScopeError("Event commit identifiers must be complete hexadecimal SHAs")
    return value.lower()


def classify(paths):
    verify = docker = False
    for path in paths:
        # Source/package directories take precedence over Markdown suffixes:
        # config/REFERENCE.md and crate documentation may be compile-time inputs.
        if path.startswith(("crates/", "web/", "deploy/", "config/", "scripts/",
                            ".cargo/", ".github/workflows/")):
            verify = docker = True
        elif path.startswith("desktop/"):
            verify = True
        elif path.endswith(".md") and ("/" not in path or path.startswith(("docs/", ".github/"))):
            continue
        else:
            # Unknown files, generated API JSON, packaging metadata and future
            # product surfaces conservatively receive both complete suites.
            verify = docker = True
    return verify, docker


def scope(event_name, event, repository):
    if event_name == "workflow_dispatch":
        return {"verify": True, "docker": True, "reason": "manual_full_run", "changed_paths": None}
    if event_name not in ("push", "pull_request"):
        return {"verify": True, "docker": True, "reason": "unclassified_event_full_run", "changed_paths": None}
    try:
        if event_name == "pull_request":
            before, after = event["pull_request"]["base"]["sha"], event["pull_request"]["head"]["sha"]
        else:
            before, after = event["before"], event["after"]
    except (KeyError, TypeError) as error:
        raise ScopeError("Event data is missing its authoritative commit range") from error
    before, after = sha(before), sha(after)
    if not before.strip("0") or not after.strip("0"):
        return {"verify": True, "docker": True, "reason": "zero_sha_full_run", "changed_paths": None}
    for revision in (before, after):
        git(repository, "cat-file", "-e", revision + "^{commit}")
    if event_name == "pull_request":
        before = sha(git(repository, "merge-base", before, after).decode("ascii").strip())
    raw = git(repository, "diff", "--no-ext-diff", "--no-textconv", "--name-only", "-z",
              "--no-renames", before, after, "--")
    if raw and not raw.endswith(b"\0"):
        raise ScopeError("Git did not return a complete NUL-delimited path list")
    paths = [value.decode("utf-8", errors="surrogateescape") for value in raw.split(b"\0") if value]
    verify, docker = classify(paths)
    return {"verify": verify, "docker": docker,
            "reason": "changed_surfaces" if verify or docker else "prose_or_no_changes",
            "changed_paths": len(paths)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--event-name", default=os.environ.get("GITHUB_EVENT_NAME"))
    parser.add_argument("--event-path", type=Path, default=os.environ.get("GITHUB_EVENT_PATH"))
    parser.add_argument("--repository", type=Path, default=Path.cwd())
    parser.add_argument("--github-output", type=Path, default=os.environ.get("GITHUB_OUTPUT"))
    args = parser.parse_args()
    try:
        if not args.event_name:
            raise ScopeError("A GitHub event name is required")
        event = {}
        if args.event_name in ("push", "pull_request"):
            if args.event_path is None:
                raise ScopeError("A GitHub event payload is required")
            if args.event_path.stat().st_size > 32 * 1024 * 1024:
                raise ScopeError("The event payload exceeds the classification limit")
            event = json.loads(args.event_path.read_text(encoding="utf-8"))
        result = scope(args.event_name, event, args.repository)
        if args.github_output:
            with args.github_output.open("a", encoding="utf-8", newline="\n") as output:
                for key in ("verify", "docker"):
                    output.write(f"{key}={str(result[key]).lower()}\n")
        print(json.dumps(result, sort_keys=True))
        return 0
    except (ScopeError, OSError, ValueError, UnicodeError):
        # No false/skip outputs on errors. Required consumers explicitly fail if
        # this job fails or either output is absent rather than assuming no work.
        print("::error::CI change scope could not be verified. Inspect the commit range or dispatch the complete workflow.", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
