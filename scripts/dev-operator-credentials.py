#!/usr/bin/env python3
"""Add private operator credentials without replacing any existing development secret."""
import fcntl
import os
from pathlib import Path
import secrets
import shlex
import stat
import sys
from urllib.parse import urlsplit


def fail(message):
    sys.exit(message)


def check_file(path):
    metadata = path.lstat()
    if not stat.S_ISREG(metadata.st_mode) or metadata.st_uid != os.getuid() or stat.S_IMODE(metadata.st_mode) != 0o600:
        fail(f"{path.name} must be a regular, service-owned mode-0600 file; no existing secret was replaced.")


def validate(value, role, database, port):
    try:
        parsed = urlsplit(value)
        valid = (
            parsed.scheme == "postgres" and parsed.hostname == "127.0.0.1"
            and parsed.port == int(port) and parsed.path == "/" + database
            and parsed.username == role and not parsed.query and not parsed.fragment
            and parsed.password and len(parsed.password) == 64
            and all(char in "0123456789abcdef" for char in parsed.password)
        )
    except (ValueError, TypeError):
        valid = False
    if not valid:
        fail("An existing operator credential does not match this isolated development cluster; no values printed.")


def main():
    if len(sys.argv) != 4:
        fail("Usage: dev-operator-credentials.py ENV_FILE STATE PORT")
    env_path, state, port = Path(sys.argv[1]), Path(sys.argv[2]), sys.argv[3]
    check_file(env_path)
    os.umask(0o077)
    fd = os.open(env_path, os.O_RDWR | os.O_APPEND | os.O_NOFOLLOW)
    with os.fdopen(fd, "a+") as env_file:
        fcntl.flock(env_file, fcntl.LOCK_EX)
        env_file.seek(0)
        original = env_file.read()
        present = {}
        for line in original.splitlines():
            words = shlex.split(line, comments=True)
            if len(words) == 2 and words[0] == "export" and "=" in words[1]:
                key, value = words[1].split("=", 1)
                present[key] = value
        additions = []
        new_files = []
        for suffix, role, database in [
            ("OPERATOR_", "opentrack_operator", "opentrack"),
            ("TEST_OPERATOR_", "opentrack_test_operator", "opentrack_test"),
        ]:
            key = "OPENTRACK_" + suffix + "DATABASE_URL"
            path = state / (suffix.lower() + "database-url")
            value = present.get(key)
            if path.exists() or path.is_symlink():
                check_file(path)
                stored = path.read_text().strip()
                if value is not None and value != stored:
                    fail("Operator URL file and environment differ; no existing credentials were changed.")
                value = stored
            elif value is None:
                value = f"postgres://{role}:{secrets.token_hex(32)}@127.0.0.1:{port}/{database}"
            validate(value, role, database, port)
            if not path.exists():
                new_files.append((path, value))
            if key not in present:
                additions.append("export " + key + "=" + shlex.quote(value) + "\n")
        # Validate both identities before creating anything. A malformed second
        # credential must not leave a partially upgraded first credential behind.
        for path, value in new_files:
            with path.open("x") as output:
                output.write(value + "\n")
        if additions:
            if original and not original.endswith("\n"):
                env_file.write("\n")
            env_file.writelines(additions)
            env_file.flush()
            os.fsync(env_file.fileno())
    print("Separate development/test operator credentials are ready; existing credentials preserved; no values printed.")


if __name__ == "__main__":
    main()
