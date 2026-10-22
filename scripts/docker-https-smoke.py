#!/usr/bin/env python3
"""Qualify the local-CA HTTPS profile on an existing disposable Docker stack.

Only opentrack-smoke-* projects are accepted. Run after docker-smoke.py has
completed first-owner setup and the proxy image has been built. No account
credentials, private CA keys or disabled TLS verification are used.
"""
import argparse
import hashlib
from html.parser import HTMLParser
import json
import os
from pathlib import Path
import re
import ssl
import subprocess
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request


ORIGIN = "https://localhost:18443"
MAX_RESPONSE = 8 * 1024 * 1024


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


class NoRedirects(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, request, response, code, message, headers, url):
        raise RuntimeError("HTTPS qualification refuses redirects")


class Assets(HTMLParser):
    def __init__(self):
        super().__init__()
        self.entries = set()

    def handle_starttag(self, tag, attributes):
        attributes = dict(attributes)
        if tag == "script" and attributes.get("src"):
            self.entries.add((attributes["src"], "javascript"))
        if tag == "link" and "stylesheet" in attributes.get("rel", "").split() and attributes.get("href"):
            self.entries.add((attributes["href"], "css"))


class Qualification:
    def __init__(self, args):
        require(re.fullmatch(r"opentrack-smoke-[a-z0-9-]+", args.project),
                "Use an explicitly disposable opentrack-smoke-* project")
        self.project = args.project
        self.environment = dict(os.environ)
        self.environment.update({
            "OPENTRACK_DOMAIN": "localhost",
            "OPENTRACK_PUBLIC_URL": ORIGIN,
            "OPENTRACK_HTTPS_PORT": "18443",
            "OPENTRACK_HTTPS_BIND_ADDRESS": "127.0.0.1",
        })
        self.compose = ["docker", "compose", "--project-name", self.project,
                        "--file", "compose.yaml", "--file", "compose.https.yaml", "--profile", "https"]
        self.directory = Path("work/docker-smoke")
        self.certificate = self.directory / "local-ca.crt"
        self.output = Path(args.output)
        self.checks = []

    def command(self, arguments, *, timeout=90, allowed=(0,)):
        result = subprocess.run(arguments, env=self.environment, capture_output=True,
                                text=True, timeout=timeout)
        require(result.returncode in allowed,
                f"Container command failed ({result.returncode}); inspect the disposable project logs")
        return result

    def dc(self, *arguments, **kwargs):
        return self.command(self.compose + list(arguments), **kwargs)

    def record(self, message):
        self.checks.append(message)
        print(f"PASS {message}", flush=True)

    def container(self, service):
        identifier = self.dc("ps", "--all", "--quiet", service).stdout.strip()
        require(bool(re.fullmatch(r"[0-9a-f]{12,64}", identifier)),
                f"The existing disposable project needs exactly one {service} container")
        containers = json.loads(self.command(["docker", "inspect", identifier]).stdout)
        require(len(containers) == 1, f"Expected one inspected {service} container")
        item = containers[0]
        labels = item["Config"].get("Labels") or {}
        require(labels.get("com.docker.compose.project") == self.project
                and labels.get("com.docker.compose.service") == service,
                "Container identity does not match the disposable project")
        return item

    @staticmethod
    def opener(context):
        return urllib.request.build_opener(
            urllib.request.ProxyHandler({}),
            urllib.request.HTTPSHandler(context=context),
            NoRedirects(),
        )

    @staticmethod
    def get(opener, path, *, timeout=15, accept="*/*"):
        require(path.startswith("/") and not path.startswith("//"),
                "Only this qualification origin may be requested")
        request = urllib.request.Request(ORIGIN + path, headers={"Origin": ORIGIN, "Accept-Encoding": "identity", "Accept": accept})
        with opener.open(request, timeout=timeout) as reply:
            require(reply.status == 200 and reply.geturl() == ORIGIN + path,
                    "HTTPS response did not match its requested resource")
            body = reply.read(MAX_RESPONSE + 1)
            require(len(body) <= MAX_RESPONSE, "HTTPS response exceeded the qualification bound")
            return body, reply.headers

    def export_certificate(self, proxy):
        self.directory.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(prefix="ca-copy-", dir=self.directory) as temporary:
            destination = Path(temporary) / "root.crt"
            deadline = time.monotonic() + 90
            while True:
                copied = self.command(
                    ["docker", "cp", f"{proxy['Id']}:/data/caddy/pki/authorities/local/root.crt", str(destination)],
                    timeout=15, allowed=(0, 1),
                )
                if copied.returncode == 0:
                    break
                require(time.monotonic() < deadline, "The test proxy did not create its local CA certificate")
                time.sleep(1)
            require(destination.is_file() and destination.stat().st_size <= 65536,
                    "Unexpected exported CA certificate size")
            pem = destination.read_text(encoding="ascii")
            require(pem.count("-----BEGIN CERTIFICATE-----") == 1
                    and pem.count("-----END CERTIFICATE-----") == 1
                    and "PRIVATE KEY" not in pem,
                    "Export must contain only one public CA certificate")
            fingerprint = hashlib.sha256(ssl.PEM_cert_to_DER_cert(pem)).hexdigest()
            os.replace(destination, self.certificate)
        return fingerprint

    def ready(self, opener):
        deadline = time.monotonic() + 90
        while True:
            try:
                body, _ = self.get(opener, "/health/ready", timeout=5)
                require(json.loads(body).get("status") == "ready", "Unexpected HTTPS readiness response")
                return
            except (OSError, RuntimeError, ValueError):
                require(time.monotonic() < deadline,
                        "The proxy did not become ready with verified hostname and test-CA trust")
                time.sleep(1)

    def rejects_untrusted(self, context):
        try:
            self.get(self.opener(context), "/health/ready")
        except urllib.error.URLError as error:
            if isinstance(error.reason, ssl.SSLCertVerificationError):
                return True
            raise RuntimeError("Untrusted-CA check failed before certificate verification") from None
        except ssl.SSLCertVerificationError:
            return True
        return False

    def run(self):
        # Check existing identities before up could create a fresh installation.
        for service in ("app", "database", "worker"):
            require(self.container(service)["State"]["Running"],
                    "HTTPS qualification requires the existing running core smoke stack")
        self.dc("up", "--detach", "--wait", "--wait-timeout", "180", "--no-build", timeout=240)
        app, proxy = self.container("app"), self.container("proxy")
        for name, container in (("app", app), ("proxy", proxy)):
            require(container["State"]["Running"] and not container["State"]["OOMKilled"],
                    f"{name} is not running normally")
            require(container["Config"]["User"] == "10001:10001"
                    and container["HostConfig"]["ReadonlyRootfs"],
                    f"{name} must use the fixed nonroot identity and a read-only root filesystem")
        require(not any((app["HostConfig"].get("PortBindings") or {}).values()),
                "HTTPS profile still publishes an app HTTP port")
        proxy_ports = {key: value for key, value in
                       (proxy["HostConfig"].get("PortBindings") or {}).items() if value}
        require(proxy_ports == {"8443/tcp": [{"HostIp": "127.0.0.1", "HostPort": "18443"}]},
                "The HTTPS proxy must publish only loopback port 18443")
        environment = dict(value.split("=", 1) for value in app["Config"]["Env"])
        require(environment.get("OPENTRACK_ENVIRONMENT") == "production"
                and environment.get("OPENTRACK__SERVER__LOCAL_CONTAINER") == "false"
                and environment.get("OPENTRACK_PUBLIC_URL") == ORIGIN,
                "HTTPS profile did not restore strict production configuration")
        self.record("strict production HTTPS profile, no published app HTTP port, and nonroot read-only services")

        fingerprint = self.export_certificate(proxy)
        context = ssl.SSLContext(ssl.PROTOCOL_TLS_CLIENT)
        context.minimum_version = ssl.TLSVersion.TLSv1_2
        context.load_verify_locations(cafile=str(self.certificate))
        require(context.check_hostname and context.verify_mode == ssl.CERT_REQUIRED
                and context.cert_store_stats()["x509_ca"] == 1,
                "HTTPS trust must contain only the exported test CA and require hostname verification")
        http = self.opener(context)
        self.ready(http)
        self.record("HTTPS readiness verified using only the current proxy's exported public CA")

        # An empty trust store is deterministic, even on a developer host where
        # the same local CA was explicitly installed previously. Also observe
        # the ordinary system trust store without modifying it.
        require(self.rejects_untrusted(ssl.SSLContext(ssl.PROTOCOL_TLS_CLIENT)),
                "TLS unexpectedly succeeded without trusting the test CA")
        system_rejected = self.rejects_untrusted(ssl.create_default_context())
        self.record("untrusted TLS certificate rejected without the test CA")

        html, headers = self.get(http, "/", accept="text/html")
        require("text/html" in headers.get("Content-Type", "") and b"OpenTrack" in html,
                "HTTPS did not serve the built app HTML")
        parser = Assets()
        parser.feed(html.decode("utf-8"))
        assets = []
        for reference, kind in sorted(parser.entries):
            target = urllib.parse.urlsplit(urllib.parse.urljoin(ORIGIN + "/", reference))
            require(target.scheme == "https" and target.netloc == "localhost:18443"
                    and not target.fragment and not target.username and not target.password,
                    "App assets must resolve to the same verified HTTPS origin")
            path = target.path + ("?" + target.query if target.query else "")
            data, metadata = self.get(http, path)
            media_type = metadata.get("Content-Type", "").split(";", 1)[0].strip().lower()
            expected = {"text/css"} if kind == "css" else {"text/javascript", "application/javascript"}
            require(bool(data) and media_type in expected,
                    "An HTTPS app asset was missing or returned an HTML fallback")
            assets.append({"path": path, "kind": kind, "bytes": len(data)})
        require(any(item["kind"] == "javascript" and item["path"].startswith("/assets/") for item in assets)
                and any(item["kind"] == "css" and item["path"].startswith("/assets/") for item in assets),
                "The HTML must reference and serve built JavaScript and CSS assets")
        status, _ = self.get(http, "/api/v1/setup/status")
        require(json.loads(status).get("setup_required") is False,
                "HTTPS redeployment lost the existing first-owner database state")
        self.record("verified HTTPS app HTML, JavaScript, CSS and retained completed owner setup")

        started = time.monotonic()
        self.dc("stop", "--timeout", "35", "proxy", timeout=50)
        elapsed = time.monotonic() - started
        stopped = self.container("proxy")["State"]
        require(elapsed < 45 and not stopped["Running"] and stopped["ExitCode"] == 0
                and not stopped["OOMKilled"], "HTTPS proxy did not shut down cleanly within its bound")
        self.dc("up", "--detach", "--wait", "--wait-timeout", "120", "--no-build", "proxy", timeout=180)
        self.ready(http)
        self.record("bounded graceful proxy shutdown and verified HTTPS after restart using the same trusted CA")

        limitations = ["no authenticated HTTPS session flow tested",
                       "no publicly trusted certificate or external network route qualified",
                       "not a native Docker Desktop host certification or full backup/restore"]
        if not system_rejected:
            limitations.append("this host's existing system trust already accepted the proxy CA")
        receipt = {"project": self.project, "origin": ORIGIN, "checks": self.checks,
                   "ca_sha256_der": fingerprint, "ca_file": self.certificate.as_posix(),
                   "system_trust_rejected_local_ca": system_rejected,
                   "proxy_shutdown_seconds": round(elapsed, 2), "assets": assets,
                   "limitations": limitations}
        self.output.parent.mkdir(parents=True, exist_ok=True)
        self.output.write_text(json.dumps(receipt, indent=2) + "\n", encoding="utf-8")
        print(f"Recorded {len(self.checks)} HTTPS checks in {self.output}", flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--project", required=True)
    parser.add_argument("--output", default="work/docker-smoke/https-receipt.json")
    args = parser.parse_args()
    try:
        Qualification(args).run()
    except subprocess.TimeoutExpired:
        raise SystemExit("HTTPS qualification failed: a bounded container command timed out") from None
    except (RuntimeError, OSError, ValueError, KeyError, TypeError) as error:
        message = str(error) if isinstance(error, RuntimeError) else "unexpected filesystem, TLS or container response"
        raise SystemExit(f"HTTPS qualification failed: {message}") from None


if __name__ == "__main__":
    main()
