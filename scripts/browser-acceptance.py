#!/usr/bin/env python3
"""Run saved browser workflows only in the repository's disposable GitHub Docker job.

Credentials stay in captured pipes/process environment, never files or receipts.
No application runtime is started unless the CI identity and Compose project pass
the guards below. Cleanup can only address this run's distinct browser project.
"""
import argparse
import json
import os
from pathlib import Path
import re
import secrets
import subprocess
import sys
import tempfile
import time
import urllib.parse

from browser_order_fixtures import prepare, remaining, validate_manifest


SERVICE_MOUNTS = {
    "initialize": {("database", "/database-state", True), ("blobs", "/var/lib/opentrack", False),
                   ("database_secrets", "/bootstrap/database", False), ("api_secrets", "/bootstrap/app", False),
                   ("migration_secrets", "/bootstrap/migration", False), ("worker_secrets", "/bootstrap/worker", False),
                   ("operator_secrets", "/bootstrap/operator", False)},
    "database": {("database", "/var/lib/postgresql", False), ("database_secrets", "/run/secrets", True)},
    "migrate": {("migration_secrets", "/run/secrets", True)},
    "app": {("api_secrets", "/run/secrets", True), ("blobs", "/var/lib/opentrack", False)},
    "worker-provision": {("database_secrets", "/run/secrets", True)},
    "worker-grant": {("migration_secrets", "/run/secrets", True)},
    "worker": {("worker_secrets", "/run/secrets", True), ("blobs", "/var/lib/opentrack", False)},
    "credentials": {("api_secrets", "/run/secrets", True)},
    "operator-provision": {("database_secrets", "/run/secrets", True)},
    "operator-grant": {("migration_secrets", "/run/secrets", True)},
    "operator-output-init": {("operator_output", "/operator-output", False)},
    "operator": {("operator_secrets", "/run/secrets", True), ("operator_output", "/operator-output", False)},
}


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def validate_target(project, url, environment):
    require(environment.get("GITHUB_ACTIONS") == "true"
            and environment.get("RUNNER_OS") == "Linux",
            "Browser runtime acceptance is restricted to the configured Linux GitHub CI job")
    run = environment.get("GITHUB_RUN_ID", "")
    attempt = environment.get("GITHUB_RUN_ATTEMPT", "")
    require(re.fullmatch(r"[1-9][0-9]*", run) and re.fullmatch(r"[1-9][0-9]*", attempt),
            "A concrete GitHub run and attempt are required")
    require(project == f"opentrack-browser-{run}-{attempt}-amd64",
            "Use only this CI run's distinct disposable browser project")
    origin = urllib.parse.urlsplit(url)
    require(origin.scheme == "http" and origin.hostname in ("localhost", "127.0.0.1")
            and not origin.username and not origin.password and origin.port == 18081
            and origin.path in ("", "/") and not origin.query and not origin.fragment,
            "Browser acceptance requires an HTTP loopback origin on its dedicated port")
    return url.rstrip("/")


def validate_compose(config, project):
    require(config.get("name") == project, "Compose resolved a different project")
    for name, volume in config.get("volumes", {}).items():
        require(not volume.get("external") and volume.get("name") == project + "_" + name
                and volume.get("driver", "local") == "local" and not volume.get("driver_opts"),
                "Every test volume must belong only to the disposable browser project")
    require(set(config["services"]) == set(SERVICE_MOUNTS), "Browser Compose service roles changed")
    for name, service in config["services"].items():
        profiles = ["tools"] if name == "credentials" else ["operator"] if name.startswith("operator") else []
        require(service.get("profiles", []) == profiles, "Browser Compose service profiles changed")
        mounts = service.get("volumes", [])
        require(all(mount.get("type") == "volume" and mount.get("source") in config["volumes"] for mount in mounts)
                and {(mount["source"], mount["target"], mount.get("read_only", False)) for mount in mounts}
                == SERVICE_MOUNTS[name],
                "Browser acceptance requires the reviewed service volumes and read-only secret mounts")
    ports = config["services"]["app"]["ports"]
    require(len(ports) == 1 and ports[0].get("host_ip") == "127.0.0.1"
            and str(ports[0].get("published")) == "18081" and ports[0].get("target") == 8080,
            "The browser project must publish only its dedicated loopback application port")
    require(not config["services"]["database"].get("ports"), "The test database must not publish a port")


def browser_environment(environment, url, token, password, manifest):
    keys = ("PATH", "HOME", "LANG", "LC_ALL", "TZ", "XDG_CACHE_HOME", "PLAYWRIGHT_BROWSERS_PATH",
            "GITHUB_ACTIONS", "RUNNER_OS", "GITHUB_RUN_ID", "GITHUB_RUN_ATTEMPT")
    return {**{key: environment[key] for key in keys if key in environment}, "CI": "true",
            "OPENTRACK_BROWSER_URL": url, "OPENTRACK_BROWSER_SETUP_TOKEN": token,
            "OPENTRACK_BROWSER_PASSWORD": password, "OPENTRACK_BROWSER_ORDER_FIXTURES_FILE": str(manifest)}


def validate_container(actual, config, project, service, running=None):
    labels = actual["Config"].get("Labels") or {}
    mounts = {(config["volumes"][mount["source"]]["name"], mount["target"], not mount.get("read_only", False))
              for mount in config["services"][service]["volumes"]}
    require(labels.get("com.docker.compose.project") == project
            and labels.get("com.docker.compose.service") == service
            and actual["State"]["Running"] in (True, False)
            and (running is None or actual["State"]["Running"] is running)
            and actual.get("Mounts") and all(mount.get("Type") == "volume" for mount in actual["Mounts"])
            and {(mount.get("Name"), mount.get("Destination"), mount.get("RW"))
                 for mount in actual["Mounts"]} == mounts,
            "Browser fixture service or volume ownership did not match this project")
    if service == "worker":
        require(actual["Config"].get("Cmd") == ["worker"] and not any(
            item.startswith("OPENTRACK__WORKER__") for item in actual["Config"].get("Env", [])),
            "Browser fixture requires the unchanged default worker profile")


class BrowserAcceptance:
    def __init__(self, project, url):
        self.url = validate_target(project, url, os.environ)
        self.project = project
        self.root = Path(__file__).resolve().parent.parent
        self.environment = {**os.environ, "COMPOSE_PROJECT_NAME": project, "COMPOSE_PROFILES": "",
                            "OPENTRACK_HTTP_PORT": "18081"}
        self.compose = ["docker", "compose", "--project-name", project,
                        "--file", str(self.root / "compose.yaml")]

    def command(self, arguments, timeout=120):
        result = subprocess.run(arguments, cwd=self.root, env=self.environment,
                                capture_output=True, text=True, timeout=timeout)
        require(result.returncode == 0, "Browser acceptance container command failed; output withheld to protect credentials")
        return result.stdout.strip()

    def dc(self, *arguments, timeout=120):
        return self.command(self.compose + list(arguments), timeout)

    def preflight(self, timeout=120):
        config = json.loads(self.dc("config", "--format", "json", timeout=timeout))
        validate_compose(config, self.project)
        return config

    def owned_service(self, service, running=True, timeout=55):
        require(service in ("database", "worker"), "Browser fixture cannot control another service")
        deadline = time.monotonic() + timeout
        config = self.preflight(timeout=remaining(deadline, 55))
        container = self.dc("ps", "--all", "--quiet", service, timeout=remaining(deadline, 55))
        require(re.fullmatch(r"[0-9a-f]{12,64}", container),
                "Browser fixture requires exactly one owned service container")
        inspected = json.loads(self.command(["docker", "inspect", container], timeout=remaining(deadline, 55)))
        require(len(inspected) == 1, "Browser fixture container inspection was ambiguous")
        validate_container(inspected[0], config, self.project, service, running)
        return container

    def validate_live_resources(self, timeout=55):
        deadline = time.monotonic() + timeout
        config = self.preflight(timeout=remaining(deadline, 55))
        services = {name for name, service in config["services"].items() if not service.get("profiles")}
        containers = self.dc("ps", "--all", "--quiet", timeout=remaining(deadline, 55)).splitlines()
        require(len(containers) == len(services) and all(re.fullmatch(r"[0-9a-f]{12,64}", item) for item in containers),
                "Browser fixture requires only this fresh project's reviewed containers")
        inspected = json.loads(self.command(["docker", "inspect", *containers], timeout=remaining(deadline, 55)))
        seen = set()
        for actual in inspected:
            service = actual["Config"].get("Labels", {}).get("com.docker.compose.service")
            require(service in services and service not in seen, "Browser fixture has an unexpected service container")
            validate_container(actual, config, self.project, service)
            seen.add(service)
        require(seen == services, "Browser fixture is missing a reviewed service container")
        names = {source for service in services for source, _, _ in SERVICE_MOUNTS[service]}
        expected = {self.project + "_" + name: name for name in names}
        volumes = json.loads(self.command(["docker", "volume", "inspect", *sorted(expected)],
                                          timeout=remaining(deadline, 55)))
        require(len(volumes) == len(expected) and {item.get("Name") for item in volumes} == set(expected)
                and all(item.get("Driver") == "local" and not item.get("Options")
                        and (item.get("Labels") or {}).get("com.docker.compose.project") == self.project
                        and (item.get("Labels") or {}).get("com.docker.compose.volume") == expected[item["Name"]]
                        for item in volumes),
                "Browser fixture secret, blob or data volume belongs to a different project")

    def owned_sql(self, statement, timeout):
        deadline = time.monotonic() + timeout
        self.validate_live_resources(timeout=remaining(deadline, 20))
        self.owned_service("database", timeout=remaining(deadline, 20))
        return self.dc("exec", "-T", "database", "psql", "-X", "-q", "-v", "ON_ERROR_STOP=1",
                       "-U", "postgres", "-d", "opentrack", "-tAc", statement,
                       timeout=remaining(deadline, 20))

    def cleanup(self):
        self.preflight()
        self.dc("down", "--volumes", "--remove-orphans", "--timeout", "35")

    def run(self):
        self.preflight()
        version = json.loads(self.command(["docker", "version", "--format", "{{json .Server}}"] ))
        require(version.get("Os") == "linux" and version.get("Arch") == "amd64"
                and int(version["Version"].split(".")[0]) >= 28,
                "Browser CI needs a native Linux amd64 Docker Engine 28 or newer")
        for kind in ("container", "volume"):
            existing = self.command(["docker", kind, "ls", "--quiet", "--filter",
                                     f"label=com.docker.compose.project={self.project}"])
            require(not existing, "Browser acceptance requires a new project with no existing containers or volumes")
        started = False
        try:
            started = True
            self.dc("up", "--detach", "--wait", "--wait-timeout", "180", "--no-build", timeout=240)
            require(self.dc("port", "app", "8080") == "127.0.0.1:18081",
                    "The live app port does not match the guarded loopback target")
            self.validate_live_resources()
            # The installed credential helper emits only into this captured pipe.
            token = self.dc("run", "--rm", "--no-deps", "-T", "credentials", "setup-token",
                            "--app-secrets", "/run/secrets")
            require(re.fullmatch(r"[0-9a-f]{64}", token), "The credential helper returned an invalid setup token")
            password = secrets.token_urlsafe(36)
            suite_deadline = time.monotonic() + 900
            manifest = prepare(self, token, password, suite_deadline=suite_deadline)
            validate_manifest(manifest)
            with tempfile.TemporaryDirectory(prefix="opentrack-browser-order-") as directory:
                os.chmod(directory, 0o700)
                path = Path(directory) / "references.json"
                descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
                with os.fdopen(descriptor, "w", encoding="utf-8") as stream:
                    json.dump(manifest, stream)
                environment = browser_environment(self.environment, self.url, token, password, path)
                result = subprocess.run(["node", "node_modules/@playwright/test/cli.js", "test"],
                                        cwd=self.root / "web", env=environment, timeout=remaining(suite_deadline, 900))
                require(result.returncode == 0, "Browser acceptance failed; inspect the credential-free test receipt")
        finally:
            if started:
                self.cleanup()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--project", required=True)
    parser.add_argument("--url", default="http://localhost:18081")
    parser.add_argument("--cleanup", action="store_true")
    args = parser.parse_args()
    try:
        runner = BrowserAcceptance(args.project, args.url)
        runner.cleanup() if args.cleanup else runner.run()
        return 0
    except (RuntimeError, OSError, ValueError, KeyError, TypeError, subprocess.TimeoutExpired) as error:
        # Never include subprocess stdout/stderr, HTTP bodies or auth state.
        print(f"Browser acceptance stopped: {error if isinstance(error, RuntimeError) else type(error).__name__}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
