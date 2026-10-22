#!/usr/bin/env python3
"""Destructive only to an explicitly named disposable opentrack-smoke-* project.

Run after Compose has built and started a fresh stack. Requires Python only for
this developer/CI qualification; ordinary installation uses Docker alone.
Never writes credentials into the public receipt or process arguments.
"""
import argparse
import hashlib
import http.cookiejar
from html.parser import HTMLParser
import json
import os
from pathlib import Path
import re
import secrets
import subprocess
import time
import urllib.error
import urllib.parse
import urllib.request


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


class FrontendAssets(HTMLParser):
    def __init__(self):
        super().__init__()
        self.assets = []

    def handle_starttag(self, tag, attributes):
        attrs = dict(attributes)
        if tag == "script" and attrs.get("src"):
            self.assets.append((attrs["src"], "script"))
        elif tag == "link" and "stylesheet" in (attrs.get("rel") or "").split() and attrs.get("href"):
            self.assets.append((attrs["href"], "stylesheet"))


class Qualification:
    def __init__(self, args):
        require(re.fullmatch(r"opentrack-smoke-[a-z0-9-]+", args.project),
                "Use an explicitly disposable opentrack-smoke-* project")
        origin = urllib.parse.urlsplit(args.url)
        require(origin.scheme == "http" and origin.hostname in ("localhost", "127.0.0.1")
                and not origin.username and not origin.password
                and origin.path in ("", "/") and not origin.query and not origin.fragment,
                "The smoke URL must be an HTTP loopback origin")
        self.url = args.url.rstrip("/")
        self.project = args.project
        self.port = origin.port or 80
        self.compose = ["docker", "compose", "--project-name", args.project, "--file", "compose.yaml"]
        self.cookies = http.cookiejar.CookieJar()
        self.http = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(self.cookies))
        self.csrf = None
        self.checks = []
        self.output = Path(args.output)
        self.expected_architecture = args.expected_architecture

    def command(self, arguments, *, timeout=90, allowed=(0,)):
        result = subprocess.run(arguments, capture_output=True, text=True, timeout=timeout)
        require(result.returncode in allowed,
                f"Container command failed ({result.returncode}); inspect the project logs")
        return result

    def dc(self, *arguments, **kwargs):
        return self.command(self.compose + list(arguments), **kwargs)

    def record(self, message):
        self.checks.append(message)
        print(f"PASS {message}", flush=True)

    def request(self, path, method="GET", body=None, *, raw=None, headers=None,
                expected=(200,), authenticated=True):
        fields = {"Origin": self.url}
        if body is not None:
            fields["Content-Type"] = "application/json"
            raw = json.dumps(body).encode()
        if self.csrf and authenticated and method not in ("GET", "HEAD"):
            fields["X-CSRF-Token"] = self.csrf
        fields.update(headers or {})
        request = urllib.request.Request(self.url + path, data=raw, method=method, headers=fields)
        opener = self.http if authenticated else urllib.request.build_opener()
        try:
            reply = opener.open(request, timeout=30)
        except urllib.error.HTTPError as error:
            reply = error
        with reply:
            status, received = reply.status, reply.read()
            response_headers = reply.headers
        require(status in expected, f"Unexpected HTTP {status} for {method} {path}")
        if "application/json" in response_headers.get("Content-Type", ""):
            return json.loads(received), response_headers
        return received, response_headers

    def sql(self, statement):
        return self.dc("exec", "-T", "database", "psql", "-X", "-v", "ON_ERROR_STOP=1",
                       "-U", "postgres", "-d", "opentrack", "-tAc", statement).stdout.strip()

    def ready(self):
        deadline = time.monotonic() + 120
        while time.monotonic() < deadline:
            try:
                self.request("/health/ready")
                return
            except (RuntimeError, OSError):
                time.sleep(1)
        raise RuntimeError("API never became ready")

    def setup_token(self):
        token = self.dc("run", "--rm", "--no-deps", "-T", "credentials", "setup-token",
                        "--app-secrets", "/run/secrets").stdout.strip()
        require(bool(re.fullmatch(r"[0-9a-f]{64}", token)), "Setup helper returned an invalid token")
        if os.environ.get("GITHUB_ACTIONS") == "true":
            print(f"::add-mask::{token}", flush=True)
        return token

    def container(self, service):
        identifier = self.dc("ps", "--all", "--quiet", service).stdout.strip()
        require(identifier and "\n" not in identifier, f"Expected one {service} container")
        return json.loads(self.command(["docker", "inspect", identifier]).stdout)[0]

    def run(self):
        version = json.loads(self.command(["docker", "version", "--format", "{{json .Server}}"]).stdout)
        engine_version = re.match(r"^(\d+)\.", version.get("Version", ""))
        require(engine_version and int(engine_version.group(1)) >= 28,
                "The loopback-only Docker path requires Engine server version 28 or newer")
        require(version.get("Os") == "linux" and version.get("Arch") == self.expected_architecture,
                "Docker engine OS/architecture does not match the requested native Linux qualification")
        published = self.dc("port", "app", "8080").stdout.strip()
        require(published == f"127.0.0.1:{self.port}", "Smoke URL must match the project's loopback port")
        self.ready()
        html, headers = self.request("/", headers={"Accept": "text/html"})
        require(b"OpenTrack" in html and b"/assets/" in html, "Container did not serve its built frontend")
        require("text/html" in headers.get("Content-Type", ""), "Frontend content type is incorrect")
        frontend = FrontendAssets()
        frontend.feed(html.decode("utf-8"))
        require({kind for _, kind in frontend.assets} == {"script", "stylesheet"}
                and any(kind == "script" and urllib.parse.urlsplit(path).path.startswith("/assets/") for path, kind in frontend.assets),
                "Frontend must reference its built JavaScript and stylesheet")
        for path, kind in frontend.assets:
            parsed = urllib.parse.urlsplit(path)
            require(not parsed.scheme and not parsed.netloc and not parsed.fragment
                    and (parsed.path.startswith("/assets/") or (kind == "script" and parsed.path == "/theme.js")),
                    "Frontend asset must use a local built-asset path or the theme bootstrap")
            asset, asset_headers = self.request(path)
            content_type = asset_headers.get("Content-Type", "").split(";", 1)[0].strip()
            expected_types = ("application/javascript", "text/javascript") if kind == "script" else ("text/css",)
            require(isinstance(asset, bytes) and asset.strip() and content_type in expected_types,
                    f"Built {kind} asset is missing, empty or has an incorrect content type")
        capabilities, _ = self.request("/api/v1/capabilities")
        require(capabilities["features"]["attachments"], "Attachment storage is unavailable")
        require(not capabilities["features"]["ai"], "Smoke must exercise the independent core")
        self.record("healthy API and fetched built JavaScript/CSS assets with AI disabled")

        app, worker = self.container("app"), self.container("worker")
        database = self.container("database")
        require(not any((database["HostConfig"].get("PortBindings") or {}).values())
                and not any((database["NetworkSettings"].get("Ports") or {}).values()),
                "PostgreSQL must not publish any host port")
        runtime_images = {}
        for name, container in (("app", app), ("worker", worker)):
            image = json.loads(self.command(["docker", "image", "inspect", container["Image"]]).stdout)[0]
            require(image.get("Os") == "linux" and image.get("Architecture") == self.expected_architecture,
                    f"{name} image is not native to the requested Linux architecture")
            runtime_images[name] = image["Id"]
            require(container["Config"]["User"] == "10001:10001", f"{name} must run as fixed nonroot UID")
            require(container["HostConfig"]["ReadonlyRootfs"], f"{name} root filesystem must be read-only")
            require(container["State"]["Running"], f"{name} must be running")
            secret_mounts = [mount for mount in container["Mounts"] if mount["Destination"] == "/run/secrets"]
            require(len(secret_mounts) == 1 and not secret_mounts[0]["RW"], f"{name} secret volume must be read-only")
            allowed = f"{self.project}_{'api' if name == 'app' else 'worker'}_secrets"
            require(secret_mounts[0].get("Name") == allowed, f"{name} must receive only its own secret volume")
        self.record(f"native Linux {self.expected_architecture} engine/runtime images and unpublished database port")
        require(self.sql("SELECT count(*)=4 AND bool_and(NOT rolsuper AND NOT rolcreatedb AND NOT rolcreaterole AND NOT rolbypassrls) FROM pg_roles WHERE rolname IN ('opentrack','opentrack_migrator','opentrack_worker','opentrack_operator')") == "t", "Database roles are not least privilege")
        require(self.sql("SELECT count(*) FROM pg_tables WHERE schemaname='public' AND tableowner IN ('opentrack','opentrack_worker')") == "0", "Runtime roles must not own tables")
        self.record("nonroot read-only services and separate least-privilege database credentials")

        status, _ = self.request("/api/v1/setup/status")
        require(status["setup_required"], "Qualification requires a fresh disposable installation")
        token = self.setup_token()
        token_hash = hashlib.sha256(token.encode()).hexdigest()
        password = secrets.token_urlsafe(32)
        account = "docker-qualification@example.invalid"
        setup_body = {
            "setup_token": token, "email": account, "password": password,
            "display_name": "Docker qualification", "workspace_name": "Container acceptance",
            "team_name": "Engineering", "team_key": "ENG",
        }
        session, _ = self.request("/api/v1/setup", "POST", setup_body, expected=(201,))
        self.csrf = session["csrf_token"]
        self.request("/api/v1/setup", "POST", setup_body, authenticated=False, expected=(409,))
        workspace = session["workspaces"][0]["id"]
        base = f"/api/v1/workspaces/{workspace}"
        bootstrap, _ = self.request(base + "/bootstrap")
        team = bootstrap["teams"][0]["id"]
        self.record("first-owner setup, repeated setup rejection and authenticated workspace bootstrap")

        content = b"Container persistence and checksum qualification\n" * 32769
        digest = hashlib.sha256(content).hexdigest()
        file, file_headers = self.request(base + "/attachments", "POST", {
            "team_id": team, "original_name": "container-qualification.txt",
            "size_bytes": len(content), "sha256": digest, "mime_hint": "text/plain",
        }, headers={"Idempotency-Key": "docker-smoke-file"}, expected=(201,))
        attachment = base + "/attachments/" + file["id"]
        uploaded, _ = self.request(attachment + "/content", "PUT", raw=content,
                                  headers={"If-Match": file_headers["ETag"], "Content-Type": "application/octet-stream"})
        require(uploaded["state"] == "ready" and uploaded["scan_status"] == "not_scanned", "Optional scanning must be reported honestly")
        body = {"type": "doc", "content": [
            {"type": "paragraph", "content": [{"type": "text", "text": "Persisted through Docker recreation."}]},
            {"type": "attachment", "attrs": {"id": file["id"]}},
        ]}
        create = {"team_id": team, "title": "Docker persistence acceptance", "document": {"schema_version": 1, "body": body}}
        issue, issue_headers = self.request(base + "/issues", "POST", create,
                                headers={"Idempotency-Key": "docker-smoke-issue"}, expected=(201,))
        replay, _ = self.request(base + "/issues", "POST", create,
                                 headers={"Idempotency-Key": "docker-smoke-issue"}, expected=(200, 201))
        require(replay["id"] == issue["id"], "Creation retry duplicated the issue")
        issue_path = base + "/issues/" + issue["id"]
        original_etag = issue_headers["ETag"]
        updated_title = "Docker updated persistence acceptance"
        updated, updated_headers = self.request(issue_path, "PATCH", {"title": updated_title, "priority": 2},
                                                 headers={"If-Match": original_etag})
        require(updated["id"] == issue["id"] and updated["title"] == updated_title
                and updated["priority"] == 2 and updated["version"] != issue["version"]
                and updated_headers["ETag"] != original_etag, "Conditional update did not persist a new issue version")
        self.request(issue_path, "PATCH", {"title": "Stale write must not persist"},
                     headers={"If-Match": original_etag}, expected=(412,))
        current, _ = self.request(issue_path)
        require(current["title"] == updated_title and current["version"] == updated["version"],
                "Rejected stale update changed the saved issue")
        issue = updated
        self.record("conditional issue update persists and stale ETag rejects without overwriting")
        document, document_headers = self.request(issue_path + "/document")
        require(document["body"] == body, "Saved structured document differs")
        downloaded, _ = self.request(attachment + "/content")
        require(hashlib.sha256(downloaded).hexdigest() == digest, "Downloaded bytes failed checksum")
        self.request(attachment + "/content", authenticated=False, expected=(401,))
        self.request(base + "/documents/" + document["id"], "PATCH",
                     {"schema_version": 1, "body": body},
                     headers={"X-CSRF-Token": "invalid", "If-Match": document_headers["ETag"]}, expected=(403,))
        self.record("real issue/document, streamed attachment, safe replay and unauthorized-request rejection")

        worker_query = "SELECT count(*)>0 AND bool_and(c.last_seq=h.last_seq AND c.backfill_job_id IS NOT NULL) FROM consumer_checkpoints c JOIN workspace_stream_heads h USING(workspace_id)"
        deadline = time.monotonic() + 90
        while self.sql(worker_query) != "t":
            require(time.monotonic() < deadline, "Restricted worker did not capture persisted events")
            time.sleep(2)
        self.record("restricted worker captured real durable workspace events")

        start = time.monotonic()
        self.dc("stop", "--timeout", "35", "app", "worker", timeout=55)
        elapsed = time.monotonic() - start
        require(elapsed < 45, "API/worker shutdown exceeded its bound")
        for service in ("app", "worker"):
            state = self.container(service)["State"]
            require(not state["Running"] and state["ExitCode"] == 0 and not state["OOMKilled"], f"{service} did not exit cleanly")
        self.record("bounded graceful API and worker shutdown without forced kill")

        self.dc("down", "--timeout", "35", timeout=70)
        self.dc("up", "--detach", "--wait", "--wait-timeout", "180", "--no-build", timeout=240)
        self.ready()
        require(hashlib.sha256(self.setup_token().encode()).hexdigest() == token_hash, "Recreation replaced setup credentials")
        status, _ = self.request("/api/v1/setup/status")
        require(not status["setup_required"], "Recreation lost owner setup")
        preserved, _ = self.request(issue_path)
        require(preserved["id"] == issue["id"] and preserved["title"] == updated_title and preserved["priority"] == 2,
                "Recreation lost the updated issue or the existing session")
        session, _ = self.request("/api/v1/auth/login", "POST", {"email": account, "password": password})
        self.csrf = session["csrf_token"]
        restored, _ = self.request(issue_path + "/document")
        restored_file, _ = self.request(attachment + "/content")
        require(restored["body"] == body and hashlib.sha256(restored_file).hexdigest() == digest,
                "Database or blob bytes did not survive container recreation")
        self.record("container recreation preserves login, session, issue, document, attachment bytes and secrets")

        self.dc("stop", "--timeout", "35", "app", "worker", timeout=55)
        rename = "from pathlib import Path; p=Path('/bootstrap/app/database-url'); p.rename(p.with_name('database-url.smoke-held'))"
        restore = "from pathlib import Path; p=Path('/bootstrap/app/database-url.smoke-held'); p.rename(p.with_name('database-url'))"
        self.dc("run", "--rm", "--no-deps", "-T", "--entrypoint", "python3", "initialize", "-c", rename)
        try:
            rejected = self.dc("run", "--rm", "--no-deps", "-T", "initialize", timeout=60, allowed=(0, 1))
            require(rejected.returncode == 1, "Missing credential was silently recreated")
            require("Missing or unexpected files in a secret volume; no credentials changed." in rejected.stderr,
                    "Failure did not come from the expected redacted bootstrap validation")
        finally:
            self.dc("run", "--rm", "--no-deps", "-T", "--entrypoint", "python3", "initialize", "-c", restore)
        self.dc("run", "--rm", "--no-deps", "-T", "initialize")
        self.dc("up", "--detach", "--wait", "--wait-timeout", "180", "--no-build", timeout=240)
        self.ready()
        require(hashlib.sha256(self.setup_token().encode()).hexdigest() == token_hash, "Credential failure test changed secrets")
        final_file, _ = self.request(attachment + "/content")
        require(hashlib.sha256(final_file).hexdigest() == digest, "Data changed during credential failure recovery")
        self.record("missing secret fails closed; restoring the original file recovers the same data")

        receipt = {"project": self.project, "docker": version.get("Version"),
                   "compose": self.command(["docker", "compose", "version", "--short"]).stdout.strip(),
                   "architecture": version.get("Arch"), "expected_architecture": self.expected_architecture,
                   "runtime_images": runtime_images, "checks": self.checks,
                   "attachment_bytes": len(content), "shutdown_seconds": round(elapsed, 2),
                   "limitations": ["not a native Docker Desktop host certification",
                                   "not a full backup/restore or production TLS qualification"]}
        self.output.parent.mkdir(parents=True, exist_ok=True)
        self.output.write_text(json.dumps(receipt, indent=2) + "\n", encoding="utf-8")
        print(f"Recorded {len(self.checks)} Docker runtime checks in {self.output}", flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--project", required=True)
    parser.add_argument("--url", default="http://localhost:18080")
    parser.add_argument("--expected-architecture", required=True, choices=("amd64", "arm64"))
    parser.add_argument("--output", default="work/docker-smoke/receipt.json")
    args = parser.parse_args()
    try:
        Qualification(args).run()
    except (RuntimeError, OSError, ValueError, subprocess.TimeoutExpired) as error:
        raise SystemExit(f"Docker qualification failed: {error}") from None


if __name__ == "__main__":
    main()
