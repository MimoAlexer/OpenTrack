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
import urllib.parse
import urllib.request


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
    for volume in config.get("volumes", {}).values():
        require(not volume.get("external") and volume.get("name", "").startswith(project + "_"),
                "Every test volume must belong only to the disposable browser project")
    for service in config["services"].values():
        require(all(mount.get("type") == "volume" for mount in service.get("volumes", [])),
                "Browser acceptance cannot mount host directories")
    ports = config["services"]["app"]["ports"]
    require(len(ports) == 1 and ports[0].get("host_ip") == "127.0.0.1"
            and str(ports[0].get("published")) == "18081" and ports[0].get("target") == 8080,
            "The browser project must publish only its dedicated loopback application port")
    require(not config["services"]["database"].get("ports"), "The test database must not publish a port")


class BrowserAcceptance:
    def __init__(self, project, url):
        self.url = validate_target(project, url, os.environ)
        self.project = project
        self.root = Path(__file__).resolve().parent.parent
        self.environment = {**os.environ, "COMPOSE_PROJECT_NAME": project, "OPENTRACK_HTTP_PORT": "18081"}
        self.compose = ["docker", "compose", "--project-name", project,
                        "--file", str(self.root / "compose.yaml")]

    def command(self, arguments, timeout=120):
        result = subprocess.run(arguments, cwd=self.root, env=self.environment,
                                capture_output=True, text=True, timeout=timeout)
        require(result.returncode == 0, "Browser acceptance container command failed; output withheld to protect credentials")
        return result.stdout.strip()

    def dc(self, *arguments, timeout=120):
        return self.command(self.compose + list(arguments), timeout)

    def preflight(self):
        validate_compose(json.loads(self.dc("config", "--format", "json")), self.project)

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
            # The installed credential helper emits only into this captured pipe.
            token = self.dc("run", "--rm", "--no-deps", "-T", "credentials", "setup-token",
                            "--app-secrets", "/run/secrets")
            require(re.fullmatch(r"[0-9a-f]{64}", token), "The credential helper returned an invalid setup token")
            with urllib.request.urlopen(self.url + "/api/v1/setup/status", timeout=15) as response:
                require(json.load(response)["setup_required"], "Browser setup must target a fresh database")
            environment = {**self.environment, "OPENTRACK_BROWSER_URL": self.url,
                           "OPENTRACK_BROWSER_SETUP_TOKEN": token,
                           "OPENTRACK_BROWSER_PASSWORD": secrets.token_urlsafe(36)}
            result = subprocess.run(["node", "node_modules/@playwright/test/cli.js", "test"],
                                    cwd=self.root / "web", env=environment, timeout=600)
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
    except (RuntimeError, OSError, ValueError, subprocess.TimeoutExpired) as error:
        # Never include subprocess stdout/stderr, HTTP bodies or auth state.
        print(f"Browser acceptance stopped: {error if isinstance(error, RuntimeError) else type(error).__name__}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
