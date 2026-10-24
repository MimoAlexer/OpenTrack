#!/usr/bin/env python3
"""Qualify the local-CA HTTPS profile on an existing disposable Docker stack.

Only opentrack-smoke-* projects are accepted. Run after docker-smoke.py has
completed first-owner setup and the proxy image has been built. The standalone
command needs no account credentials; recovery checks retain existing actors in
memory. Private CA keys are never exported and TLS verification stays enabled.
"""
import argparse
import copy
import http.cookiejar
import hashlib
from html.parser import HTMLParser
import inspect
import json
import os
from pathlib import Path
import re
import ssl
import stat
import subprocess
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request


ORIGIN = "https://localhost:18443"
MAX_RESPONSE = 8 * 1024 * 1024
MAX_PROXY_METADATA_ENTRIES = 4096
MAX_PROXY_METADATA_DEPTH = 32
MAX_PROXY_METADATA_CLASSES = 64
MAX_PROXY_METADATA_BYTES = 16384
RECOVERY_LABEL = "org.opentrack.qualification.recovery"
METADATA_PURPOSE_LABEL = "org.opentrack.qualification.purpose"


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def proxy_metadata(roots):
    require(type(roots) is dict and set(roots) == {"proxy_data", "proxy_config"}
            and all(isinstance(root, Path) for root in roots.values()),
            "Proxy metadata roots rejected")
    require(hasattr(os, "O_DIRECTORY") and hasattr(os, "O_NOFOLLOW"),
            "Proxy metadata filesystem rejected")
    flags = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW
    result = {}
    try:
        for volume in ("proxy_data", "proxy_config"):
            root = roots[volume]
            root_info = root.lstat()
            require(stat.S_ISDIR(root_info.st_mode), "Proxy metadata entry rejected")
            device, entries_seen, classes = root_info.st_dev, 0, {}

            def visit(name, info, depth, parent=None):
                nonlocal entries_seen
                require(depth <= MAX_PROXY_METADATA_DEPTH, "Proxy metadata bounds exceeded")
                require(info.st_dev == device and not info.st_mode & 0o7000,
                        "Proxy metadata entry rejected")
                if stat.S_ISDIR(info.st_mode):
                    kind = "directory"
                else:
                    require(stat.S_ISREG(info.st_mode) and info.st_nlink == 1,
                            "Proxy metadata entry rejected")
                    kind = "file"
                entries_seen += 1
                require(entries_seen <= MAX_PROXY_METADATA_ENTRIES, "Proxy metadata bounds exceeded")
                key = (kind, info.st_uid, info.st_gid, f"{stat.S_IMODE(info.st_mode):04o}")
                classes[key] = classes.get(key, 0) + 1
                require(len(classes) <= MAX_PROXY_METADATA_CLASSES, "Proxy metadata bounds exceeded")
                if kind == "directory":
                    descriptor = os.open(name, flags, dir_fd=parent)
                    try:
                        opened = os.fstat(descriptor)
                        require((opened.st_dev, opened.st_ino) == (info.st_dev, info.st_ino),
                                "Proxy metadata entry rejected")
                        with os.scandir(descriptor) as children:
                            for child in children:
                                visit(child.name, os.lstat(child.name, dir_fd=descriptor),
                                      depth + 1, descriptor)
                    finally:
                        os.close(descriptor)

            visit(root, root_info, 0)
            result[volume] = [{"kind": kind, "uid": uid, "gid": gid, "mode": mode, "count": count}
                              for (kind, uid, gid, mode), count in sorted(classes.items())]
    except (OSError, NotImplementedError):
        raise RuntimeError("Proxy metadata filesystem rejected") from None
    return parse_proxy_metadata(json.dumps(result, separators=(",", ":")))


def parse_proxy_metadata(output):
    require(type(output) is str, "Proxy metadata JSON rejected")
    require(len(output) <= MAX_PROXY_METADATA_BYTES, "Proxy metadata bounds exceeded")

    def unique_object(pairs):
        result = {}
        for key, value in pairs:
            require(key not in result, "Proxy metadata JSON rejected")
            result[key] = value
        return result

    def invalid_constant(_value):
        raise RuntimeError("Proxy metadata JSON rejected")

    try:
        require(len(output.encode("utf-8")) <= MAX_PROXY_METADATA_BYTES,
                "Proxy metadata bounds exceeded")
        value = json.loads(output, object_pairs_hook=unique_object, parse_constant=invalid_constant)
    except (ValueError, RecursionError):
        raise RuntimeError("Proxy metadata JSON rejected") from None
    require(type(value) is dict and set(value) == {"proxy_data", "proxy_config"},
            "Proxy metadata schema rejected")
    normalized = {}
    for volume in ("proxy_data", "proxy_config"):
        records = value[volume]
        require(type(records) is list and 1 <= len(records) <= MAX_PROXY_METADATA_CLASSES,
                "Proxy metadata schema rejected")
        seen, total, entries = set(), 0, []
        for record in records:
            require(type(record) is dict and set(record) == {"kind", "uid", "gid", "mode", "count"},
                    "Proxy metadata schema rejected")
            require(type(record["kind"]) is str and record["kind"] in ("directory", "file")
                    and all(type(record[key]) is int and 0 <= record[key] <= 4294967294
                            for key in ("uid", "gid"))
                    and type(record["mode"]) is str and re.fullmatch(r"0[0-7]{3}", record["mode"])
                    and type(record["count"]) is int and 0 < record["count"] <= MAX_PROXY_METADATA_ENTRIES,
                    "Proxy metadata schema rejected")
            key = (record["kind"], record["uid"], record["gid"], record["mode"])
            require(key not in seen, "Proxy metadata schema rejected")
            seen.add(key)
            total += record["count"]
            require(total <= MAX_PROXY_METADATA_ENTRIES, "Proxy metadata bounds exceeded")
            entries.append({"kind": key[0], "uid": key[1], "gid": key[2],
                            "mode": key[3], "count": record["count"]})
        require(any(record["kind"] == "directory" for record in entries),
                "Proxy metadata schema rejected")
        normalized[volume] = sorted(entries, key=lambda record: (
            record["kind"], record["uid"], record["gid"], record["mode"]))
    return normalized


def readonly_actor(actor, opener):
    view = copy.copy(actor)

    def request(path, method="GET", body=None, *, raw=None, headers=None,
                expected=(200,), authenticated=True):
        selection = None
        try:
            if not (type(path) is str and path.startswith("/") and not path.startswith("//")
                    and all(ord(character) > 32 and ord(character) != 127
                            and character not in "#\\" for character in path)
                    and method == "GET" and body is None and raw is None
                    and type(authenticated) is bool and type(expected) is tuple and expected
                    and all(type(status) is int and 100 <= status <= 599 for status in expected)
                    and (headers is None or type(headers) is dict)):
                raise RuntimeError("Read-only HTTPS transport rejected")
            target = ORIGIN + path
            parsed = urllib.parse.urlsplit(target)
            if (parsed.scheme != "https" or parsed.netloc != "localhost:18443"
                    or parsed.fragment or parsed.username or parsed.password):
                raise RuntimeError("Read-only HTTPS transport rejected")
            fields = {"Origin": ORIGIN, "Accept-Encoding": "identity"}
            permitted = {"if-none-match": "If-None-Match", "range": "Range"}
            for name, value in (headers or {}).items():
                if (type(name) is not str or name.lower() not in permitted
                        or type(value) is not str or not value
                        or any(character in value for character in "\r\n\x00")):
                    raise RuntimeError("Read-only HTTPS transport rejected")
                canonical = permitted[name.lower()]
                if canonical in fields:
                    raise RuntimeError("Read-only HTTPS transport rejected")
                fields[canonical] = value
            outgoing = urllib.request.Request(target, method="GET", headers=fields)
            # CookieJar has no public policy getter; preserve its policy without sharing state.
            selection = http.cookiejar.CookieJar(policy=copy.deepcopy(actor.cookies._policy))
            if authenticated:
                for cookie in actor.cookies:
                    selection.set_cookie(copy.deepcopy(cookie))
            selection.add_cookie_header(outgoing)
            try:
                reply = opener.open(outgoing, timeout=30)
            except urllib.error.HTTPError as error:
                reply = error
            with reply:
                if (reply.status not in expected or 300 <= reply.status < 400
                        or reply.geturl() != target):
                    raise RuntimeError("Read-only HTTPS transport rejected")
                received = reply.read(MAX_RESPONSE + 1)
                if len(received) > MAX_RESPONSE:
                    raise RuntimeError("Read-only HTTPS transport rejected")
                response_headers = reply.headers
            media_type = response_headers.get("Content-Type", "").partition(";")[0].strip().lower()
            if media_type in ("application/json", "application/problem+json"):
                return json.loads(received), response_headers
            return received, response_headers
        except Exception:
            raise RuntimeError("Read-only HTTPS transport rejected") from None
        finally:
            if selection is not None:
                selection.clear()

    view.request = request
    return view


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
        self.metadata_reserved = False

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

    def metadata_container(self, image):
        name = self.project + "-https-metadata"
        identifiers = self.command(["docker", "container", "ls", "--all", "--quiet",
                                    "--filter", f"name=^/{name}$"], timeout=15).stdout.split()
        if not identifiers:
            return None
        require(len(identifiers) == 1, "Proxy metadata identity rejected")
        items = json.loads(self.command(["docker", "container", "inspect", name], timeout=15).stdout)
        require(type(items) is list and len(items) == 1 and type(items[0]) is dict,
                "Proxy metadata identity rejected")
        item = items[0]
        configuration, host = item.get("Config", {}), item.get("HostConfig", {})
        require(type(configuration) is dict and type(host) is dict, "Proxy metadata identity rejected")
        labels = configuration.get("Labels") or {}
        mounts = item.get("Mounts", [])
        require(type(labels) is dict and type(mounts) is list
                and all(type(mount) is dict for mount in mounts), "Proxy metadata identity rejected")
        require(item.get("Name") == "/" + name and item.get("Image") == image
                and configuration.get("Image") == image and configuration.get("User") == "10001:10001"
                and labels.get(RECOVERY_LABEL) == self.project
                and labels.get("com.docker.compose.project") == self.project
                and labels.get(METADATA_PURPOSE_LABEL) == "https-metadata"
                and host.get("NetworkMode") == "none" and host.get("ReadonlyRootfs") is True
                and not host.get("Privileged") and not host.get("CapAdd")
                and host.get("CapDrop") == ["ALL"]
                and host.get("SecurityOpt") == ["no-new-privileges:true"]
                and len(mounts) == 2 and all(mount.get("Type") == "volume"
                                           and mount.get("RW") is False for mount in mounts)
                and {(mount.get("Name"), mount.get("Destination"), mount.get("RW")) for mount in mounts}
                == {(self.project + "_proxy_data", "/proxy-data", False),
                    (self.project + "_proxy_config", "/proxy-config", False)},
                "Proxy metadata identity rejected")
        return item

    def cleanup_metadata(self, image):
        try:
            if self.metadata_container(image) is None:
                self.metadata_reserved = False
                return
            self.command(["docker", "container", "rm", "--force",
                          self.project + "-https-metadata"], timeout=30)
            require(self.metadata_container(image) is None, "Proxy metadata cleanup unconfirmed")
            self.metadata_reserved = False
        except (RuntimeError, OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError):
            raise RuntimeError("Proxy metadata cleanup unconfirmed") from None

    def measure_metadata(self, image):
        primary = None
        reserved = False
        try:
            require(not self.metadata_reserved, "Proxy metadata prior cleanup is unconfirmed")
            require(type(image) is str and re.fullmatch(r"sha256:[0-9a-f]{64}", image),
                    "Proxy metadata image rejected")
            require(self.metadata_container(image) is None, "Proxy metadata refuses a preexisting helper")
            initialize = self.container("initialize")
            require(initialize.get("Image") == image and not initialize.get("State", {}).get("Running")
                    and initialize.get("State", {}).get("ExitCode") == 0,
                    "Proxy metadata image rejected")
            volumes = {}
            for volume in ("proxy_data", "proxy_config"):
                name = self.project + "_" + volume
                items = json.loads(self.command(["docker", "volume", "inspect", name], timeout=15).stdout)
                require(type(items) is list and len(items) == 1 and type(items[0]) is dict,
                        "Proxy metadata volume identity rejected")
                item = items[0]
                labels = item.get("Labels") or {}
                require(type(labels) is dict and item.get("Name") == name
                        and item.get("Driver") == "local" and not item.get("Options")
                        and labels.get("com.docker.compose.project") == self.project
                        and labels.get("com.docker.compose.volume") == volume,
                        "Proxy metadata volume identity rejected")
                require(type(item.get("CreatedAt")) is str and item["CreatedAt"],
                        "Proxy metadata volume identity rejected")
                volumes[volume] = item
                require(not self.command(["docker", "ps", "--quiet", "--filter", "volume=" + name],
                                         timeout=15).stdout.strip(), "Proxy metadata writer is active")
            identifiers = self.command(["docker", "ps", "--all", "--quiet", "--filter",
                                        "label=com.docker.compose.project=" + self.project], timeout=15).stdout.split()
            require(identifiers, "Proxy metadata source inventory rejected")
            containers = json.loads(self.command(["docker", "inspect", *identifiers], timeout=15).stdout)
            require(type(containers) is list and len(containers) == len(identifiers) and all(
                    type(item) is dict and
                    (item.get("Config", {}).get("Labels") or {}).get("com.docker.compose.project") == self.project
                    and not item.get("State", {}).get("Running") and not item.get("State", {}).get("Restarting")
                    and not item.get("State", {}).get("OOMKilled") and item.get("State", {}).get("ExitCode") == 0
                    for item in containers), "Proxy metadata writer is active")
            helper = "import json, os, re, signal, stat\nfrom pathlib import Path\n"
            for bound in ("ENTRIES", "DEPTH", "CLASSES", "BYTES"):
                key = "MAX_PROXY_METADATA_" + bound
                helper += key + " = " + str(globals()[key]) + "\n"
            helper += "\n".join(inspect.getsource(function) for function in
                                 (require, proxy_metadata, parse_proxy_metadata))
            helper += ("\ndef expired(_signal, _frame):\n    raise TimeoutError()\n"
                       "signal.signal(signal.SIGALRM, expired)\nsignal.alarm(15)\n"
                       "try:\n    value = proxy_metadata({'proxy_data':Path('/proxy-data'),"
                       "'proxy_config':Path('/proxy-config')})\n"
                       "    print(json.dumps(value,separators=(',',':')),flush=True)\n"
                       "except (RuntimeError,OSError,ValueError,TimeoutError):\n"
                       "    raise SystemExit('Proxy metadata helper rejected') from None\n")
            reserved = True
            self.metadata_reserved = True
            result = self.command(["docker", "run", "--pull", "never", "--name", self.project + "-https-metadata",
                "--label", RECOVERY_LABEL + "=" + self.project,
                "--label", "com.docker.compose.project=" + self.project,
                "--label", METADATA_PURPOSE_LABEL + "=https-metadata",
                "--network", "none", "--user", "10001:10001", "--read-only", "--cap-drop", "ALL",
                "--security-opt", "no-new-privileges:true",
                "--mount", "type=volume,src=" + self.project + "_proxy_data,dst=/proxy-data,readonly,volume-nocopy",
                "--mount", "type=volume,src=" + self.project + "_proxy_config,dst=/proxy-config,readonly,volume-nocopy",
                "--entrypoint", "python3", image, "-c", helper], timeout=30)
            completed = self.metadata_container(image)
            require(completed is not None and not completed.get("State", {}).get("Running")
                    and not completed.get("State", {}).get("Restarting")
                    and not completed.get("State", {}).get("OOMKilled")
                    and completed.get("State", {}).get("ExitCode") == 0,
                    "Proxy metadata helper did not complete")
            observed = parse_proxy_metadata(result.stdout)
            for volume, original in volumes.items():
                current = json.loads(self.command(["docker", "volume", "inspect", self.project + "_" + volume],
                                                 timeout=15).stdout)
                require(current == [original], "Proxy metadata source volume changed")
            return observed
        except BaseException as error:
            primary = ("interrupted" if isinstance(error, (KeyboardInterrupt, SystemExit)) else
                       "timeout" if isinstance(error, subprocess.TimeoutExpired) else "helper")
            if isinstance(error, (KeyboardInterrupt, SystemExit)):
                raise
            raise RuntimeError("Proxy metadata rejected: " + primary) from None
        finally:
            if reserved:
                try:
                    self.cleanup_metadata(image)
                except RuntimeError:
                    reason = ("primary=" + primary + "; ") if primary else ""
                    raise RuntimeError("Proxy metadata rejected: " + reason + "cleanup=unconfirmed") from None

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
