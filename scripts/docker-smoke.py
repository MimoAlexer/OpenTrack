#!/usr/bin/env python3
"""Destructive only to an explicitly named disposable opentrack-smoke-* project.

Run after Compose has built and started a fresh stack. Requires Python only for
this developer/CI qualification; ordinary installation uses Docker alone.
Never writes credentials into the public receipt or process arguments.
"""
import argparse
import copy
import hashlib
import http.cookiejar
from html.parser import HTMLParser
import importlib.util
import inspect
import json
import os
from pathlib import Path
import re
import secrets
import signal
import ssl
import stat
import subprocess
import tarfile
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request


_https_spec = importlib.util.spec_from_file_location("docker_https_smoke", Path(__file__).with_name("docker-https-smoke.py"))
https = importlib.util.module_from_spec(_https_spec)
_https_spec.loader.exec_module(https)


RECOVERY_VOLUMES = ("database", "blobs", "database_secrets", "api_secrets",
                    "migration_secrets", "worker_secrets", "operator_secrets")
OPTIONAL_RECOVERY_VOLUMES = ("operator_output",)
RECOVERY_LABEL = "org.opentrack.qualification.recovery"
RELAY_COMMAND = ["reverse-proxy", "--from", "http://:8080", "--to", "http://app:8080"]
RELAY_TMPFS = ["/data:uid=10001,gid=10001,mode=0700,size=4m",
               "/config:uid=10001,gid=10001,mode=0700,size=4m"]
DATABASE_FINGERPRINT = """
SELECT jsonb_build_object(
 'tables',(SELECT jsonb_agg(jsonb_build_array(c.relname,pg_get_userbyid(c.relowner),
   c.relacl,c.relrowsecurity,c.relforcerowsecurity,
   query_to_xml(format('SELECT md5(coalesce(string_agg(to_jsonb(t)::text,chr(10) ORDER BY to_jsonb(t)::text),'''')) AS digest FROM public.%I t',c.relname),false,true,'')::text)
   ORDER BY c.relname) FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
   WHERE n.nspname='public' AND c.relkind='r'),
 'sequences',(SELECT jsonb_agg(to_jsonb(s) ORDER BY sequencename) FROM pg_sequences s WHERE schemaname='public'),
 'functions',(SELECT jsonb_agg(jsonb_build_array(p.oid::regprocedure::text,
   pg_get_userbyid(p.proowner),p.proacl,p.proconfig,pg_get_functiondef(p.oid)) ORDER BY p.oid::regprocedure::text)
   FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace WHERE n.nspname IN ('public','opentrack_extensions') AND p.prokind='f'),
 'policies',(SELECT jsonb_agg(to_jsonb(p) ORDER BY tablename,policyname) FROM pg_policies p WHERE schemaname='public'),
 'schemas',(SELECT jsonb_agg(jsonb_build_array(nspname,pg_get_userbyid(nspowner),nspacl) ORDER BY nspname)
   FROM pg_namespace WHERE nspname IN ('public','opentrack_extensions')),
 'database',(SELECT jsonb_build_array(pg_get_userbyid(datdba),datacl) FROM pg_database WHERE datname=current_database()),
 'roles',(SELECT jsonb_agg(jsonb_build_array(rolname,rolsuper,rolcreatedb,rolcreaterole,rolinherit,
   rolcanlogin,rolreplication,rolbypassrls,rolconfig) ORDER BY rolname) FROM pg_roles
   WHERE rolname IN ('opentrack','opentrack_migrator','opentrack_worker','opentrack_operator'))
)::text
"""


def validate_project(project):
    require(isinstance(project, str) and re.fullmatch(r"opentrack-smoke-[a-z0-9-]+", project),
            "Use an explicitly disposable opentrack-smoke-* project")
    return project


def validate_compose(config, project, port):
    require(config.get("name") == validate_project(project), "Compose resolved a different project")
    volumes = config.get("volumes", {})
    require(set(RECOVERY_VOLUMES) <= set(volumes) <= set(RECOVERY_VOLUMES + OPTIONAL_RECOVERY_VOLUMES),
            "Recovery requires exactly the normal installation volume inventory")
    for name, volume in volumes.items():
        require(not volume.get("external") and not volume.get("driver_opts")
                and volume.get("driver") in (None, "local") and volume.get("name") == f"{project}_{name}",
                "Recovery refuses external or renamed installation volumes")
    networks = config.get("networks", {})
    require(set(networks) == {"database", "web", "offline_ingress"}
            and all(bool(network.get("internal")) == (name != "offline_ingress") and not network.get("external")
                             and network.get("name") == f"{project}_{name}"
                             for name, network in networks.items()),
            "Offline qualification requires internal application networks and one owned relay network")
    require("offline-relay" in config.get("services", {}), "Offline qualification requires its fixed HTTP relay")
    for name, service in config.get("services", {}).items():
        require(not service.get("privileged") and not service.get("volumes_from")
                and service.get("network_mode") in (None, "none"),
                "Recovery refuses privileged services or shared network/volume namespaces")
        require(set(service.get("networks", {})) <= set(networks), "Service references an unknown network")
        require(all(mount.get("type") == "volume" and mount.get("source") in volumes
                    for mount in service.get("volumes", [])), "Recovery refuses host-bound volumes")
        ports = service.get("ports", [])
        if name == "offline-relay":
            require(service.get("image") == "opentrack-proxy:local"
                    and service.get("entrypoint") == ["caddy"]
                    and service.get("command") == RELAY_COMMAND
                    and set(service.get("networks", {})) == {"web", "offline_ingress"}
                    and not service.get("volumes") and set(service.get("tmpfs", [])) == set(RELAY_TMPFS)
                    and service.get("user") == "10001:10001" and service.get("read_only")
                    and service.get("cap_drop") == ["ALL"] and not service.get("cap_add")
                    and service.get("security_opt") == ["no-new-privileges:true"],
                    "Offline relay must be the fixed nonroot proxy without persistent or secret mounts")
            require(len(ports) == 1 and ports[0].get("host_ip") == "127.0.0.1"
                    and str(ports[0].get("published")) == str(port)
                    and ports[0].get("target") == 8080, "Recovery requires the owned loopback relay port")
        else:
            require("offline_ingress" not in service.get("networks", {}),
                    "Application services cannot use the relay's outward network")
            require(not ports, "Recovery refuses additional published ports")


def validate_operator_output_profile(config):
    service = config.get("services", {}).get("operator-output-init", {})
    mounts = service.get("volumes") or []
    require("operator_output" in config.get("volumes", {})
            and service.get("image") == "opentrack-bootstrap:local"
            and service.get("command") == ["prepare-output", "--directory", "/operator-output"]
            and not service.get("entrypoint") and service.get("network_mode") == "none"
            and not service.get("networks") and service.get("user") == "0:0"
            and service.get("read_only") and service.get("cap_drop") == ["ALL"]
            and set(service.get("cap_add") or []) == {"CHOWN", "FOWNER", "DAC_OVERRIDE"}
            and service.get("security_opt") == ["no-new-privileges:true"]
            and len(mounts) == 1 and mounts[0].get("type") == "volume"
            and mounts[0].get("source") == "operator_output"
            and mounts[0].get("target") == "/operator-output" and not mounts[0].get("read_only"),
            "Selected operator output requires its fixed networkless private initializer")


def check_operator_output(root):
    info = root.lstat()
    require(stat.S_ISDIR(info.st_mode) and info.st_uid == info.st_gid == 10001
            and stat.S_IMODE(info.st_mode) == 0o700,
            "Operator output directory must retain private service ownership and permissions")
    require({path.name for path in root.iterdir()} == {"recovery-qualification.txt"},
            "Operator output must contain exactly its private qualification artifact")
    info = (root / "recovery-qualification.txt").lstat()
    require(stat.S_ISREG(info.st_mode) and info.st_uid == info.st_gid == 10001
            and stat.S_IMODE(info.st_mode) == 0o600 and info.st_size > 0,
            "Operator output artifact must be a nonempty private service-owned regular file")


def safe_members(archive):
    members = archive.getmembers()
    names = {}
    for member in members:
        path = Path(member.name)
        # Check POSIX names explicitly even when boundary tests run on Windows.
        parts = member.name.split("/")
        normalized = "/".join(part for part in parts if part and part != ".")
        require(not path.is_absolute() and not member.name.startswith("/")
                and ".." not in parts and "\\" not in member.name and ":" not in member.name
                and normalized not in names and (member.isdir() or member.isfile())
                and not (member.mode & 0o7000) and member.uid in (0, 999, 10001)
                and member.gid in (0, 999, 10001), "Unsafe recovery archive entry")
        require(normalized or member.isdir(), "Recovery archive root must be a directory")
        names[normalized] = member.isdir()
    require("" in names and members, "Recovery archive has no volume root")
    for name in set(names) - {""}:
        parent = name.rpartition("/")[0]
        require(names.get(parent), "Recovery archive omits a parent directory")
    return members


def checked_archive(path, checksum):
    require(path.is_file() and not path.is_symlink() and checksum.is_file()
            and not checksum.is_symlink(), "Recovery archive or checksum is missing")
    expected = checksum.read_text(encoding="ascii").strip()
    require(re.fullmatch(r"[0-9a-f]{64}", expected), "Recovery archive checksum is malformed")
    with path.open("rb") as source:
        actual = hashlib.file_digest(source, "sha256").hexdigest()
    require(actual == expected, "Recovery archive checksum differs")
    archive = tarfile.open(path, "r:")
    try:
        safe_members(archive)
    except BaseException:
        archive.close()
        raise
    return archive


def volume_operation(action, volume, storage_key=None):
    """Run only inside the isolated bootstrap helper with two named mounts."""
    require(volume in RECOVERY_VOLUMES + OPTIONAL_RECOVERY_VOLUMES, "Unknown recovery volume")
    root, backup = Path("/volume"), Path("/backup")
    if action == "check-output":
        require(volume == "operator_output", "Private output inspection requires the selected optional volume")
        check_operator_output(root)
        return
    archive_path, checksum = backup / f"{volume}.tar", backup / f"{volume}.sha256"
    if action == "snapshot":
        if volume == "operator_output":
            check_operator_output(root)
        os.chmod(backup, 0o700)
        with archive_path.open("xb") as output:
            os.chmod(archive_path, 0o600)
            with tarfile.open(fileobj=output, mode="w") as archive:
                archive.add(root, arcname=".", recursive=True)
            output.flush()
            os.fsync(output.fileno())
        with archive_path.open("rb") as source:
            digest = hashlib.file_digest(source, "sha256").hexdigest()
        with checksum.open("x", encoding="ascii") as output:
            os.chmod(checksum, 0o600)
            output.write(digest + "\n")
            output.flush()
            os.fsync(output.fileno())
        with checked_archive(archive_path, checksum):
            pass
        return
    if action in ("reject-missing", "reject-corrupt"):
        require(not any(root.iterdir()), "Failure fixture target is not empty")
        damaged = backup / f"{volume}.failure.tar"
        require(not damaged.exists(), "Failure fixture already exists")
        try:
            if action == "reject-corrupt":
                with archive_path.open("rb") as source, damaged.open("xb") as target:
                    os.chmod(damaged, 0o600)
                    target.write(bytes([source.read(1)[0] ^ 1]))
                    while chunk := source.read(1024 * 1024):
                        target.write(chunk)
            try:
                with checked_archive(damaged, checksum):
                    pass
            except (RuntimeError, OSError, tarfile.TarError):
                require(not any(root.iterdir()), "Rejected archive changed the target")
                return
            raise RuntimeError("Damaged recovery archive was accepted")
        finally:
            if damaged.exists():
                damaged.unlink()
    with checked_archive(archive_path, checksum) as archive:
        members = safe_members(archive)
        if volume == "operator_output":
            entries = {member.name.removeprefix("./"): member for member in members}
            require(set(entries) == {".", "recovery-qualification.txt"}
                    and entries["."].isdir() and entries["."].mode == 0o700
                    and entries["recovery-qualification.txt"].isfile()
                    and entries["recovery-qualification.txt"].mode == 0o600
                    and entries["recovery-qualification.txt"].size > 0
                    and all(member.uid == member.gid == 10001 for member in members),
                    "Operator output archive must contain only its privately owned directory and artifact")
        if action == "restore":
            require(not any(root.iterdir()), "Recovery refuses a nonempty target volume")
            options = {"numeric_owner": True}
            if hasattr(tarfile, "fully_trusted_filter"):
                options["filter"] = "fully_trusted"
            archive.extractall(root, members=members, **options)
        elif action in ("missing-blob", "corrupt-blob", "repair-blob"):
            require(volume == "blobs" and re.fullmatch(r"[0-9a-f-]{36}", storage_key or ""),
                    "Blob fault must address the owned attachment fixture")
            name = f"blobs/objects/{storage_key}"
            member = next((item for item in members if item.name.removeprefix("./") == name), None)
            require(member is not None and member.isfile(), "Attachment fixture is absent from the backup")
            target = root / name
            require(not target.is_symlink(), "Blob fault refuses a linked object")
            if action == "missing-blob":
                target.unlink()
            else:
                with archive.extractfile(member) as source:
                    content = source.read()
                if action == "corrupt-blob":
                    require(content, "Corruption fixture must contain bytes")
                    content = bytes([content[0] ^ 1]) + content[1:]
                if target.exists():
                    os.chmod(target, 0o600)
                with target.open("wb") as output:
                    output.write(content)
                    output.flush()
                    os.fsync(output.fileno())
                os.chown(target, member.uid, member.gid)
                os.chmod(target, member.mode)
            return
        else:
            require(action == "verify", "Unknown recovery operation")
        expected = {item.name.removeprefix("./").rstrip("/") for item in members}
        expected.discard(".")
        require({path.relative_to(root).as_posix() for path in root.rglob("*")} == expected,
                "Restored volume inventory differs from its snapshot")
        for member in members:
            target = root / member.name
            require(not target.is_symlink(), "Restored entry is linked")
            info = target.stat()
            require(info.st_uid == member.uid and info.st_gid == member.gid
                    and info.st_mode & 0o7777 == member.mode
                    and target.is_dir() == member.isdir(), "Restored ownership, permissions or type differ")
            if member.isfile():
                with target.open("rb") as current, archive.extractfile(member) as original:
                    require(hashlib.file_digest(current, "sha256").digest()
                            == hashlib.file_digest(original, "sha256").digest(),
                            "Restored volume bytes differ from their snapshot")
        if volume == "operator_output":
            check_operator_output(root)


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


def require_concealed(problem, headers):
    request_id = headers.get("X-Request-ID", "")
    require(bool(re.fullmatch(r"[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}", request_id))
            and problem == {"type": "/api/v1/problems/not_found", "title": "Not Found",
                            "status": 404, "detail": "This item could not be found",
                            "instance": "urn:uuid:" + request_id, "code": "not_found",
                            "request_id": request_id, "retryable": False, "field_errors": []},
            "Private resource denial must contain only the generic not-found problem")
    require(headers.get("Content-Type", "").partition(";")[0].strip().lower() == "application/problem+json"
            and headers.get("Cache-Control") == "no-store"
            and all(name not in headers for name in ("ETag", "Last-Modified", "Content-Disposition",
                                                       "Content-Range", "Accept-Ranges", "Location")),
            "Private resource denial exposed metadata or permitted caching")


class Qualification:
    def __init__(self, args):
        validate_project(args.project)
        origin = urllib.parse.urlsplit(args.url)
        require(origin.scheme == "http" and origin.hostname in ("localhost", "127.0.0.1")
                and not origin.username and not origin.password
                and origin.path in ("", "/") and not origin.query and not origin.fragment,
                "The smoke URL must be an HTTP loopback origin")
        self.url = args.url.rstrip("/")
        self.project = args.project
        self.port = origin.port or 80
        self.compose = ["docker", "compose", "--project-name", args.project, "--file", "compose.yaml"]
        self.compose_files = getattr(args, "compose_file", [])
        for file in self.compose_files:
            self.compose.extend(["--file", file])
        self.environment = {**os.environ, "OPENTRACK_HTTP_PORT": str(self.port)}
        self.cookies = http.cookiejar.CookieJar()
        self.http = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(self.cookies))
        self.csrf = None
        self.checks = []
        self.output = Path(args.output)
        self.expected_architecture = args.expected_architecture

    def command(self, arguments, *, timeout=90, allowed=(0,)):
        result = subprocess.run(arguments, env=self.environment, capture_output=True, text=True, timeout=timeout)
        require(result.returncode in allowed,
                f"Container command failed ({result.returncode}); inspect the project logs")
        return result

    def dc(self, *arguments, **kwargs):
        operation = arguments[0] if arguments else "unknown"
        if arguments and arguments[0] in ("up", "run"):
            arguments = (arguments[0], "--pull", "never", *arguments[1:])
        if arguments and arguments[0] == "up" and "--no-build" not in arguments:
            arguments = (arguments[0], "--no-build", *arguments[1:])
        try:
            return self.command(self.compose + list(arguments), **kwargs)
        except RuntimeError as error:
            raise RuntimeError(f"Compose {operation} failed; inspect the owned project logs") from error

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
        media_type = response_headers.get("Content-Type", "").partition(";")[0].strip().lower()
        if media_type in ("application/json", "application/problem+json"):
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

    def volume_info(self, name, *, missing=False, optional=False):
        result = self.command(["docker", "volume", "inspect", name], allowed=(0, 1))
        if result.returncode:
            require(missing or optional, "Owned installation volume is missing")
            return None
        require(not missing, "Recovery refuses a preexisting destination volume")
        information = json.loads(result.stdout)
        require(len(information) == 1 and information[0].get("Name") == name,
                "Docker resolved an unexpected volume")
        return information[0]

    def cleanup_recovery(self, *, remove_backup=False, include_https_metadata=False):
        if include_https_metadata:
            failed = []
            try:
                name = self.project + "-https-metadata"
                identifiers = self.command(["docker", "container", "ls", "--all", "--quiet",
                    "--filter", f"name=^/{name}$"], timeout=15).stdout.split()
                if identifiers:
                    require(len(identifiers) == 1, "HTTPS metadata cleanup unconfirmed")
                    initialize = self.container("initialize")
                    labels = initialize.get("Config", {}).get("Labels") or {}
                    image = initialize.get("Image")
                    require(type(image) is str and re.fullmatch(r"sha256:[0-9a-f]{64}", image)
                            and labels.get("com.docker.compose.project") == self.project
                            and labels.get("com.docker.compose.service") == "initialize"
                            and initialize.get("Config", {}).get("Image") == "opentrack-bootstrap:local"
                            and not initialize.get("State", {}).get("Running")
                            and not initialize.get("State", {}).get("Restarting")
                            and not initialize.get("State", {}).get("OOMKilled")
                            and initialize.get("State", {}).get("ExitCode") == 0,
                            "HTTPS metadata cleanup unconfirmed")
                    https.Qualification(argparse.Namespace(
                        project=self.project, output=str(self.output))).cleanup_metadata(image)
            except BaseException:
                failed.append("https_metadata")
            finally:
                try:
                    self.cleanup_recovery(remove_backup=remove_backup)
                except BaseException:
                    failed.append("http_recovery")
            if failed:
                raise RuntimeError("Recovery cleanup unconfirmed: " + ",".join(failed)) from None
            return
        self.cleanup_operator_output()
        self.cleanup_operator_output(initializer=True)
        target = self.project + "-restore"
        restored = Qualification(argparse.Namespace(project=target, url=self.url,
                                 output=str(self.output), expected_architecture=self.expected_architecture,
                                 compose_file=self.compose_files))
        config = json.loads(restored.dc("--profile", "operator", "config", "--format", "json").stdout)
        validate_compose(config, target, self.port)
        backup = self.project + "_recovery_backup"
        information = self.volume_info(backup, optional=True)
        if information:
            require((information.get("Labels") or {}).get(RECOVERY_LABEL) == self.project,
                    "Cleanup refuses a backup without its ownership label")
        volumes = self.command(["docker", "volume", "ls", "--quiet", "--filter",
                                f"label=com.docker.compose.project={target}"]).stdout.split()
        require(set(volumes) <= {f"{target}_{name}" for name in config["volumes"]},
                "Cleanup found volumes outside the exact restore inventory")
        for volume in volumes:
            labels = self.volume_info(volume).get("Labels") or {}
            require(labels.get(RECOVERY_LABEL) == self.project
                    and labels.get("com.docker.compose.project") == target
                    and labels.get("com.docker.compose.volume") == volume.removeprefix(target + "_"),
                    "Cleanup refuses a restore volume without its ownership labels")
        containers = self.command(["docker", "ps", "--all", "--quiet", "--filter",
                                   f"label=com.docker.compose.project={target}"]).stdout.split()
        if containers:
            require(information, "Cleanup refuses containers without the original recovery backup")
            for container in json.loads(self.command(["docker", "inspect", *containers]).stdout):
                labels = container["Config"].get("Labels") or {}
                require(labels.get("com.docker.compose.project") == target
                        and labels.get("com.docker.compose.service") in config["services"]
                        and all(mount["Type"] == "tmpfs" or (mount["Type"] == "volume"
                                and mount.get("Name") in volumes) for mount in container["Mounts"]),
                        "Cleanup refuses an unrelated container or mounted volume")
        networks = self.command(["docker", "network", "ls", "--quiet", "--filter",
                                 f"label=com.docker.compose.project={target}"]).stdout.split()
        if networks:
            require(information, "Cleanup refuses networks without the original recovery backup")
            for network in json.loads(self.command(["docker", "network", "inspect", *networks]).stdout):
                require(network.get("Name") in {f"{target}_{name}" for name in config["networks"]}
                        and (network.get("Labels") or {}).get("com.docker.compose.project") == target,
                        "Cleanup refuses an unrelated network")
        helper = self.command(["docker", "container", "inspect", self.project + "-recovery-helper"], allowed=(0, 1))
        if not helper.returncode:
            helper_info = json.loads(helper.stdout)[0]
            allowed = set(volumes) | {f"{self.project}_{name}"
                                      for name in RECOVERY_VOLUMES + OPTIONAL_RECOVERY_VOLUMES} | {backup}
            require((helper_info["Config"].get("Labels") or {}).get(RECOVERY_LABEL) == self.project
                    and helper_info["HostConfig"].get("NetworkMode") == "none"
                    and all(mount["Type"] == "volume" and mount.get("Name") in allowed for mount in helper_info["Mounts"]),
                    "Cleanup refuses an unrelated recovery helper")
            self.command(["docker", "container", "rm", "--force", self.project + "-recovery-helper"])
        if containers or networks:
            restored.dc("down", "--timeout", "35", timeout=90)
        for volume in volumes:
            self.command(["docker", "volume", "rm", volume])
        if remove_backup and information:
            self.command(["docker", "volume", "rm", backup])

    def volume_task(self, action, project, volume, backup, image, storage_key=None):
        require(project in (self.project, self.project + "-restore")
                and volume in RECOVERY_VOLUMES + OPTIONAL_RECOVERY_VOLUMES,
                "Recovery refuses a volume outside its exact source/target inventory")
        require(action != "check-output" or volume == "operator_output",
                "Private output inspection requires the selected optional volume")
        require(backup == self.project + "_recovery_backup", "Recovery backup is outside its owned inventory")
        information = self.volume_info(f"{project}_{volume}")
        labels = information.get("Labels") or {}
        require(labels.get("com.docker.compose.project") == project
                and labels.get("com.docker.compose.volume") == volume,
                "Recovery volume has unexpected ownership labels")
        backup_info = self.volume_info(backup)
        require((backup_info.get("Labels") or {}).get(RECOVERY_LABEL) == self.project,
                "Recovery backup has unexpected ownership labels")
        require(project != self.project or action in ("snapshot", "check-output"),
                "Recovery refuses to change a source volume")
        helper = "import hashlib, os, re, signal, stat, sys, tarfile\nfrom pathlib import Path\nsignal.alarm(150)\n"
        helper += "RECOVERY_VOLUMES = " + repr(RECOVERY_VOLUMES) + "\n"
        helper += "OPTIONAL_RECOVERY_VOLUMES = " + repr(OPTIONAL_RECOVERY_VOLUMES) + "\n"
        helper += "\n\n".join(inspect.getsource(function)
                                 for function in (require, check_operator_output, safe_members,
                                                  checked_archive, volume_operation))
        helper += "\ntry:\n    volume_operation(*sys.argv[1:])\nexcept RuntimeError as error:\n    raise SystemExit('Recovery validation rejected: ' + str(error))\nexcept OSError as error:\n    raise SystemExit('Recovery filesystem failure: errno=' + str(error.errno))\nexcept (ValueError, tarfile.TarError):\n    raise SystemExit('Recovery archive failure: invalid format')\n"
        readonly = action in ("snapshot", "verify", "reject-missing", "reject-corrupt", "check-output")
        mount = f"type=volume,src={project}_{volume},dst=/volume" + (",readonly" if readonly else "")
        result = self.command(["docker", "run", "--rm", "--pull", "never", "--network", "none",
                             "--name", self.project + "-recovery-helper", "--label", f"{RECOVERY_LABEL}={self.project}",
                             "--read-only", "--user", "0:0", "--cap-drop", "ALL",
                             "--cap-add", "DAC_OVERRIDE", "--cap-add", "CHOWN", "--cap-add", "FOWNER",
                             "--security-opt", "no-new-privileges:true", "--mount", mount,
                             "--mount", f"type=volume,src={backup},dst=/backup",
                             "--entrypoint", "python3", image, "-c", helper, action, volume,
                             *([storage_key] if storage_key else [])], timeout=180,
                            allowed=(0, 1))
        if result.returncode and action != "verify":
            reason = next((line for line in result.stderr.splitlines()
                           if line.startswith(("Recovery validation rejected:", "Recovery filesystem failure:",
                                               "Recovery archive failure:"))), "helper command failed")
            raise RuntimeError(f"Recovery {action} for {volume} rejected: {reason}")
        return result

    def database_fingerprint(self):
        return hashlib.sha256(self.sql(DATABASE_FINGERPRINT).encode()).digest()

    def verify_offline_runtime(self, config):
        containers = self.command(["docker", "ps", "--quiet", "--filter",
                                   f"label=com.docker.compose.project={self.project}"]).stdout.split()
        require(containers, "Offline qualification requires running project containers")
        for container in json.loads(self.command(["docker", "inspect", *containers]).stdout):
            labels = container["Config"].get("Labels") or {}
            service = labels.get("com.docker.compose.service")
            require(labels.get("com.docker.compose.project") == self.project and service in config["services"]
                    and container["HostConfig"].get("NetworkMode") not in ("host", "default", "bridge")
                    and not container["HostConfig"].get("NetworkMode", "").startswith("container:"),
                    "Offline qualification refuses a shared or foreign runtime network namespace")
            attached = container["NetworkSettings"].get("Networks") or {}
            expected = {config["networks"][name]["name"] for name in config["services"][service].get("networks", {})}
            require(set(attached) == expected, "Offline runtime has an unexpected network attachment")
            if service == "offline-relay":
                require(container["Config"].get("Image") == "opentrack-proxy:local"
                        and container["Config"].get("Entrypoint") == ["caddy"]
                        and container["Config"].get("Cmd") == RELAY_COMMAND
                        and container["Config"].get("User") == "10001:10001"
                        and container["HostConfig"].get("ReadonlyRootfs")
                        and container["HostConfig"].get("CapDrop") == ["ALL"]
                        and not container["HostConfig"].get("CapAdd")
                        and all(mount["Type"] == "tmpfs" for mount in container["Mounts"]),
                        "Running relay must be the fixed nonroot proxy without secret or persistent mounts")
            else:
                require(not any((container["HostConfig"].get("PortBindings") or {}).values()),
                        "Application services cannot publish ports in the offline fixture")
            for name, attachment in attached.items():
                network = json.loads(self.command(["docker", "network", "inspect", attachment["NetworkID"]]).stdout)[0]
                outward = service == "offline-relay" and name == f"{self.project}_offline_ingress"
                require(network.get("Name") == name and bool(network.get("Internal")) == (not outward)
                        and (network.get("Labels") or {}).get("com.docker.compose.project") == self.project,
                        "Offline runtime network is external or belongs to another project")

    def operator_output_container(self, *, initializer=False):
        project = self.project + "-restore" if initializer else self.project
        name = project + "-operator-output"
        identifiers = self.command(["docker", "container", "ls", "--all", "--quiet",
                                    "--filter", f"name=^/{name}$"], timeout=15).stdout.split()
        if not identifiers:
            return None
        require(len(identifiers) == 1, "Private output cleanup found an unexpected container inventory")
        items = json.loads(self.command(["docker", "container", "inspect", name], timeout=15).stdout)
        require(len(items) == 1, "Private output cleanup requires exactly its owned container")
        container = items[0]
        labels = container.get("Config", {}).get("Labels") or {}
        expected = {(project + "_operator_output", "/operator-output", True)}
        if not initializer:
            expected.add((project + "_operator_secrets", "/run/secrets", False))
        require(container.get("Name") == "/" + name
                and labels.get(RECOVERY_LABEL) == self.project
                and labels.get("com.docker.compose.project") == project
                and labels.get("com.docker.compose.service") == ("operator-output-init" if initializer else "operator")
                and container.get("HostConfig", {}).get("NetworkMode") == ("none" if initializer else project + "_database")
                and len(container.get("Mounts", [])) == len(expected)
                and all(mount.get("Type") == "volume" for mount in container.get("Mounts", []))
                and {(mount.get("Name"), mount.get("Destination"), mount.get("RW"))
                     for mount in container.get("Mounts", [])} == expected,
                "Private output cleanup refuses an unrelated container or volume")
        return container

    def cleanup_operator_output(self, *, initializer=False):
        try:
            if self.operator_output_container(initializer=initializer) is None:
                return
            project = self.project + "-restore" if initializer else self.project
            self.command(["docker", "container", "rm", "--force", project + "-operator-output"], timeout=30)
            require(self.operator_output_container(initializer=initializer) is None,
                    "Private output container remains after bounded cleanup")
        except (RuntimeError, OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError):
            raise RuntimeError("Private operator-output container cleanup failed; details withheld") from None

    def seed_operator_output(self, base):
        actor = Qualification(argparse.Namespace(project=self.project, url=self.url,
                              output=str(self.output), expected_architecture=self.expected_architecture,
                              compose_file=self.compose_files))
        reserved = False
        try:
            require(self.operator_output_container() is None,
                    "Private output issuance refuses a preexisting named container")
            reserved = True
            invitation, _ = self.request(base + "/invitations", "POST",
                {"email": "output-recovery@example.test", "role": "viewer", "team_ids": []},
                headers={"Idempotency-Key": "docker-operator-output-account"}, expected=(201,))
            session, _ = actor.request("/api/v1/invitations/accept", "POST",
                {"token": invitation["token"], "display_name": "Example Recovery Account",
                 "password": secrets.token_urlsafe(36) + "A1!"})
            user = session["user"]["id"]
            require(isinstance(user, str) and re.fullmatch(r"[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}", user),
                    "Operator output requires its dedicated synthetic account")
            result = self.dc("run", "--no-deps", "--name", self.project + "-operator-output",
                "--label", f"{RECOVERY_LABEL}={self.project}", "-T", "operator", "operator-recovery-issue",
                "--user", user, "--reason", "Synthetic cold-recovery output qualification",
                "--output-file", "/operator-output/recovery-qualification.txt")
            receipt = json.loads(result.stdout)
            require(isinstance(receipt, dict) and set(receipt) == {"id", "user_id", "expires_at", "output_file"}
                    and receipt["user_id"] == user and isinstance(receipt["id"], str)
                    and re.fullmatch(r"[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}", receipt["id"])
                    and isinstance(receipt["expires_at"], str) and receipt["expires_at"]
                    and receipt["output_file"] == "/operator-output/recovery-qualification.txt",
                    "Operator output requires a valid private issuance receipt")
        except (RuntimeError, OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError):
            raise RuntimeError("Private operator-output qualification failed; no receipt or bearer data printed") from None
        finally:
            try:
                if reserved:
                    self.cleanup_operator_output()
            finally:
                actor.cookies.clear()

    def seed_private_issue(self, base, team, key, content, title="Private cold restore fixture"):
        digest = hashlib.sha256(content).hexdigest()
        file, headers = self.request(base + "/attachments", "POST",
            {"team_id": team, "original_name": "private-cold-restore.txt",
             "size_bytes": len(content), "sha256": digest, "mime_hint": "text/plain"},
            headers={"Idempotency-Key": key + "-file"}, expected=(201,))
        attachment = base + "/attachments/" + file["id"]
        self.request(attachment + "/content", "PUT", raw=content,
            headers={"If-Match": headers["ETag"], "Content-Type": "application/octet-stream"})
        body = {"type": "doc", "content": [
            {"type": "paragraph", "content": [{"type": "text", "text": title}]},
            {"type": "attachment", "attrs": {"id": file["id"]}},
        ]}
        issue, _ = self.request(base + "/issues", "POST",
            {"team_id": team, "title": title,
             "document": {"schema_version": 1, "body": body}},
            headers={"Idempotency-Key": key + "-issue"}, expected=(201,))
        issue_path = base + "/issues/" + issue["id"]
        document, _ = self.request(issue_path + "/document")
        return {"team": team, "title": title, "issue": issue_path,
                "document": base + "/documents/" + document["id"],
                "attachment": attachment, "body": body, "digest": digest,
                "markers": (team, issue["id"], document["id"], file["id"], digest,
                            title, "private-cold-restore.txt", content.decode().strip())}

    def seed_private_recovery(self, reader, base):
        team, _ = self.request(base + "/teams", "POST",
            {"name": "Private restore team", "key": "RCV", "private": True},
            headers={"Idempotency-Key": "docker-private-team"}, expected=(201,))
        move_teams = []
        for name, key in (("Private move source", "MVA"), ("Private move destination", "MVB")):
            item, _ = self.request(base + "/teams", "POST", {"name": name, "key": key, "private": True},
                headers={"Idempotency-Key": "docker-private-team-" + key.lower()}, expected=(201,))
            move_teams.append(item["id"])
        invitation, _ = self.request(base + "/invitations", "POST",
            {"email": "restore-reader@example.test", "role": "viewer", "team_ids": [team["id"], move_teams[0]]},
            headers={"Idempotency-Key": "docker-private-reader"}, expected=(201,))
        session, _ = reader.request("/api/v1/invitations/accept", "POST",
            {"token": invitation["token"], "display_name": "Example Reader",
             "password": secrets.token_urlsafe(36) + "A1!"})
        reader.csrf = session["csrf_token"]
        fixture = self.seed_private_issue(base, team["id"], "docker-private",
                                          b"private-recovery-byte-marker-40192\n")
        fixture.update(reader=reader, reader_id=session["user"]["id"], workspace=base.rsplit("/", 1)[1])
        fixture["validators"] = reader.check_private_reads(fixture)
        fixture["cookies"] = tuple((c.name, c.value, c.domain, c.path) for c in reader.cookies)
        require(fixture["cookies"], "Private reader did not receive an independent session")
        fixture["move"] = self.seed_private_move(reader, base, *move_teams)
        membership = base + "/teams/" + team["id"] + "/members/" + fixture["reader_id"]
        _, headers = self.request(membership)
        self.request(membership, "DELETE", headers={"If-Match": headers["ETag"]}, expected=(204,))
        self.record("synthetic private-team viewer reads saved resources before team membership removal")
        return fixture

    def seed_private_move(self, reader, base, source, destination):
        control_body = {"type": "doc", "content": [
            {"type": "paragraph", "content": [{"type": "text", "text": "Unchanged private-A restore control"}]},
        ]}
        control, _ = self.request(base + "/issues", "POST",
            {"team_id": source, "title": "Unchanged private-A restore control",
             "document": {"schema_version": 1, "body": control_body}},
            headers={"Idempotency-Key": "docker-private-move-control"}, expected=(201,))
        control_path = base + "/issues/" + control["id"]
        control_document, _ = self.request(control_path + "/document")
        fixture = self.seed_private_issue(base, source, "docker-private-move",
            b"private-move-recovery-byte-marker-65018\n", title="Moved private cold restore fixture")
        fixture.update(reader=reader, source_team=source, control=control_path, control_body=control_body,
                       control_document=control_document["id"])
        fixture["validators"] = reader.check_private_reads(fixture)
        preview, _ = self.request(fixture["issue"] + "/move-preview?destination_team_id=" + destination)
        require(preview["issue_id"] == fixture["issue"].rsplit("/", 1)[1]
                and preview["source_team_id"] == source and preview["destination"]["id"] == destination
                and preview["destination"]["private"] and preview["destination"]["can_move_issue"]
                and preview["audience_change"] == "between_private_teams" and preview["blockers"] == [],
                "Private move preview must match the saved issue and both private teams without blockers")
        moved, _ = self.request(fixture["issue"] + "/move", "POST",
            {"destination_team_id": destination, "status_id": preview["destination_default_status_id"],
             "assignee_id": None, "label_ids": [], "preview_token": preview["preview_token"]},
            headers={"If-Match": preview["issue_etag"], "Idempotency-Key": "docker-private-move"})
        require(moved["id"] == preview["issue_id"] and moved["team_id"] == destination,
                "Private move must persist the same saved issue in the expected destination")
        fixture["team"] = destination
        fixture["markers"] += (destination,)
        self.record("synthetic reader reads saved private-A issue, documents and file before the actual move to private B")
        return fixture

    def check_private_reads(self, fixture):
        issue, issue_headers = self.request(fixture["issue"])
        document, document_headers = self.request(fixture["issue"] + "/document")
        direct, direct_headers = self.request(fixture["document"])
        metadata, headers = self.request(fixture["attachment"])
        content, _ = self.request(fixture["attachment"] + "/content")
        require(issue["id"] == fixture["issue"].rsplit("/", 1)[1]
                and issue["team_id"] == fixture["team"] and issue["title"] == fixture["title"]
                and document["id"] == direct["id"] == fixture["document"].rsplit("/", 1)[1]
                and document["body"] == direct["body"] == fixture["body"]
                and metadata["id"] == fixture["attachment"].rsplit("/", 1)[1]
                and metadata["sha256"] == fixture["digest"]
                and hashlib.sha256(content).hexdigest() == fixture["digest"],
                "Authorized private issue, document or attachment control differs")
        validators = {fixture["issue"]: issue_headers.get("ETag"),
                      fixture["issue"] + "/document": document_headers.get("ETag"),
                      fixture["document"]: direct_headers.get("ETag"),
                      fixture["attachment"]: headers.get("ETag"),
                      fixture["attachment"] + "/content": headers.get("ETag")}
        require(all(validators.values()), "Private resource control has no saved validator")
        return validators

    def check_private_denials(self, fixture):
        paths = (fixture["issue"], fixture["issue"] + "/document", fixture["document"],
                 fixture["attachment"], fixture["attachment"] + "/content")
        for path in paths:
            variants = ({}, {"If-None-Match": fixture["validators"][path]}, {"If-None-Match": "*"})
            if path.startswith(fixture["attachment"]):
                variants += ({"Range": "bytes=0-15", "If-None-Match": fixture["validators"][path]},)
            for fields in variants:
                problem, headers = self.request(path, headers=fields, expected=(404,))
                require_concealed(problem, headers)
                require(not any(marker in str(headers) for marker in fixture["markers"]),
                        "Private resource denial disclosed fixture metadata")

    def check_private_move(self, fixture):
        reader = fixture["reader"]
        control, _ = reader.request(fixture["control"])
        document, _ = reader.request(fixture["control"] + "/document")
        require(control["id"] == fixture["control"].rsplit("/", 1)[1]
                and control["team_id"] == fixture["source_team"]
                and control["title"] == "Unchanged private-A restore control"
                and document["id"] == fixture["control_document"] and document["body"] == fixture["control_body"],
                "The same reader must retain unchanged private-A issue and document access after the move")
        self.check_private_reads(fixture)
        reader.check_private_denials(fixture)

    def check_private_revocation(self, fixture, issue_path, attachment, body, digest):
        reader = fixture["reader"]
        session, _ = reader.request("/api/v1/auth/session")
        require(session["user"]["id"] == fixture["reader_id"]
                and any(w["id"] == fixture["workspace"] and w["role"] == "viewer" for w in session["workspaces"]),
                "Private-team removal must preserve the reader's workspace session")
        public, _ = reader.request(issue_path)
        document, _ = reader.request(issue_path + "/document")
        content, _ = reader.request(attachment + "/content")
        require(public["id"] == issue_path.rsplit("/", 1)[1] and document["body"] == body
                and hashlib.sha256(content).hexdigest() == digest,
                "Revoked private-team reader lost the public content control")
        self.check_private_reads(fixture)
        reader.check_private_denials(fixture)
        self.check_private_move(fixture["move"])
        require(tuple((c.name, c.value, c.domain, c.path) for c in reader.cookies) == fixture["cookies"],
                "Revocation checks must reuse the original reader session without replacement")

    def measure_https_source(self, account, password, private, issue_path, attachment, body, digest, bootstrap_image):
        stage, primary, interruption = "preflight", None, None
        cleanup = []
        mutated = established = False
        temporary = context = owner = secure = fingerprint = observed = owner_id = None
        secure_cookies = http.cookiejar.CookieJar()
        tls = https.Qualification(argparse.Namespace(project=self.project, output=str(self.output)))
        strict = {key: tls.environment[key] for key in (
            "OPENTRACK_DOMAIN", "OPENTRACK_PUBLIC_URL", "OPENTRACK_HTTPS_PORT", "OPENTRACK_HTTPS_BIND_ADDRESS")}
        tls.environment = {**self.environment, **strict}
        tls.compose = [*self.compose, "--file", "compose.https.yaml", "--profile", "operator", "--profile", "https"]

        def start(*services):
            tls.dc("up", "--detach", "--wait", "--wait-timeout", "180", "--no-build",
                   "--pull", "never", "--no-deps", *services, timeout=240)

        def stop_http_services():
            tls.dc("stop", "--timeout", "35", "worker", "offline-relay", timeout=70)
            for service in ("worker", "offline-relay"):
                state = tls.container(service)["State"]
                require(not state["Running"] and not state.get("Restarting")
                        and not state.get("OOMKilled") and state.get("ExitCode") == 0,
                        "HTTPS source HTTP services did not stop")

        def remove_proxy():
            identifiers = tls.dc("ps", "--all", "--quiet", "proxy", timeout=15).stdout.split()
            if not identifiers:
                return
            require(len(identifiers) == 1 and re.fullmatch(r"[0-9a-f]{12,64}", identifiers[0]),
                    "HTTPS source proxy cleanup rejected")
            items = json.loads(tls.command(["docker", "inspect", identifiers[0]], timeout=15).stdout)
            require(type(items) is list and len(items) == 1, "HTTPS source proxy cleanup rejected")
            item = items[0]
            labels = item["Config"].get("Labels") or {}
            host = item["HostConfig"]
            require(labels.get("com.docker.compose.project") == self.project
                    and labels.get("com.docker.compose.service") == "proxy" and item["Image"] == images["proxy"]
                    and item["Config"].get("Image") == "opentrack-proxy:local"
                    and item["Config"].get("User") == "10001:10001" and host.get("ReadonlyRootfs") is True
                    and not host.get("Privileged") and not host.get("CapAdd") and host.get("CapDrop") == ["ALL"]
                    and host.get("SecurityOpt") == ["no-new-privileges:true"]
                    and host.get("NetworkMode") in {self.project + "_web", self.project + "_offline_ingress"}
                    and set(item["NetworkSettings"].get("Networks") or {})
                    == {self.project + "_web", self.project + "_offline_ingress"}
                    and host.get("PortBindings") == {"8443/tcp": [{"HostIp": "127.0.0.1", "HostPort": "18443"}]}
                    and len(item["Mounts"]) == 2 and all(mount.get("Type") == "volume" for mount in item["Mounts"])
                    and {(mount.get("Name"), mount.get("Destination"), mount.get("RW")) for mount in item["Mounts"]}
                    == {(self.project + "_proxy_data", "/data", True),
                        (self.project + "_proxy_config", "/config", True)}
                    and not item["State"]["Running"] and not item["State"].get("Restarting")
                    and not item["State"].get("OOMKilled") and item["State"].get("ExitCode") == 0,
                    "HTTPS source proxy cleanup rejected")
            tls.command(["docker", "container", "rm", identifiers[0]], timeout=30)
            require(not tls.dc("ps", "--all", "--quiet", "proxy", timeout=15).stdout.strip(),
                    "HTTPS source proxy removal unconfirmed")

        def verify_runtime():
            identifiers = tls.command(["docker", "ps", "--quiet", "--filter",
                "label=com.docker.compose.project=" + self.project], timeout=15).stdout.split()
            require(identifiers, "HTTPS source runtime rejected")
            items = json.loads(tls.command(["docker", "inspect", *identifiers], timeout=15).stdout)
            services = set()
            for item in items:
                labels = item["Config"].get("Labels") or {}
                service = labels.get("com.docker.compose.service")
                require(labels.get("com.docker.compose.project") == self.project
                        and service in {"database", "app", "proxy"}
                        and service not in services and item["State"]["Running"]
                        and not item["State"].get("Restarting") and not item["State"].get("OOMKilled"),
                        "HTTPS source runtime rejected")
                services.add(service)
                require(item["Image"] == images[service], "HTTPS source image rejected")
                host = item["HostConfig"]
                require(host.get("NetworkMode") not in ("host", "default", "bridge")
                        and not host.get("NetworkMode", "").startswith("container:"),
                        "HTTPS source network rejected")
                attached = item["NetworkSettings"].get("Networks") or {}
                require(set(attached) == {config["networks"][name]["name"]
                    for name in config["services"][service].get("networks", {})}, "HTTPS source network rejected")
                for name, connection in attached.items():
                    network = json.loads(tls.command(["docker", "network", "inspect", connection["NetworkID"]], timeout=15).stdout)[0]
                    outward = service == "proxy" and name == self.project + "_offline_ingress"
                    labels = network.get("Labels") or {}
                    require(network.get("Name") == name and network.get("Internal") is (not outward)
                            and labels.get("com.docker.compose.project") == self.project
                            and labels.get("com.docker.compose.network") == name.removeprefix(self.project + "_"),
                            "HTTPS source network rejected")
                if service == "proxy":
                    bindings = {key: value for key, value in (host.get("PortBindings") or {}).items() if value}
                    actual = {key: value for key, value in (item["NetworkSettings"].get("Ports") or {}).items() if value}
                    expected = {"8443/tcp": [{"HostIp": "127.0.0.1", "HostPort": "18443"}]}
                    require(bindings == actual == expected
                            and host.get("NetworkMode") in {self.project + "_web", self.project + "_offline_ingress"}
                            and item["Config"].get("User") == "10001:10001" and host.get("ReadonlyRootfs") is True
                            and not host.get("Privileged")
                            and host.get("CapDrop") == ["ALL"] and not host.get("CapAdd")
                            and host.get("SecurityOpt") == ["no-new-privileges:true"], "HTTPS source proxy rejected")
                else:
                    require(not any((host.get("PortBindings") or {}).values())
                            and not any((item["NetworkSettings"].get("Ports") or {}).values()),
                            "HTTPS source published HTTP rejected")
                if service == "proxy":
                    require(len(item["Mounts"]) == 2 and all(mount["Type"] == "volume" for mount in item["Mounts"])
                            and {(mount["Name"], mount["Destination"], mount["RW"]) for mount in item["Mounts"]}
                            == {(self.project + "_proxy_data", "/data", True),
                                (self.project + "_proxy_config", "/config", True)}, "HTTPS source proxy mounts rejected")
                if service == "app":
                    environment = dict(value.split("=", 1) for value in item["Config"]["Env"])
                    require(environment.get("OPENTRACK_ENVIRONMENT") == "production"
                            and environment.get("OPENTRACK__SERVER__LOCAL_CONTAINER") == "false"
                            and environment.get("OPENTRACK_PUBLIC_URL") == https.ORIGIN,
                            "HTTPS source configuration rejected")
            require(services == {"database", "app", "proxy"},
                    "HTTPS source runtime rejected")

        def original_controls():
            nonlocal owner_id
            session, _ = owner.request("/api/v1/auth/session")
            require(session["user"]["email"] == account
                    and (owner_id is None or session["user"]["id"] == owner_id)
                    and any(workspace["id"] == private["workspace"] and workspace["role"] == "owner"
                            for workspace in session["workspaces"]), "HTTPS source owner rejected")
            owner_id = session["user"]["id"]
            owner.check_private_revocation(tls_private, issue_path, attachment, body, digest)
            issue, _ = owner.request(issue_path)
            document, _ = owner.request(issue_path + "/document")
            content, _ = owner.request(attachment + "/content")
            require(issue["id"] == issue_path.rsplit("/", 1)[1] and document["body"] == body
                    and hashlib.sha256(content).hexdigest() == digest, "HTTPS source data rejected")

        def secure_controls():
            session, _ = secure.request("/api/v1/auth/session")
            require(session["user"]["id"] == owner_id and session["user"]["email"] == account,
                    "HTTPS secure owner rejected")
            secure.check_private_revocation(tls_private, issue_path, attachment, body, digest)

        try:
            require(urllib.parse.urlsplit(self.url).hostname == "localhost", "HTTPS source hostname rejected")
            require(tls.metadata_container(bootstrap_image) is None, "HTTPS source preexisting helper rejected")
            http_config = json.loads(self.dc("--profile", "operator", "config", "--format", "json").stdout)
            validate_compose(http_config, self.project, self.port)
            validate_operator_output_profile(http_config)
            self.verify_offline_runtime(http_config)
            temporary = tempfile.TemporaryDirectory(prefix="opentrack-https-")
            tls.directory = Path(temporary.name)
            tls.certificate = tls.directory / "local-ca.crt"
            metadata = tls.directory.lstat()
            require(stat.S_ISDIR(metadata.st_mode) and stat.S_IMODE(metadata.st_mode) == 0o700
                    and metadata.st_uid == os.getuid(), "HTTPS source private storage rejected")
            override = tls.directory / "ingress.yaml"
            descriptor = os.open(override, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
            with os.fdopen(descriptor, "w", encoding="ascii") as stream:
                stream.write("services:\n  proxy:\n    networks:\n      web: {}\n      offline_ingress: {}\n")
            metadata = override.lstat()
            require(stat.S_ISREG(metadata.st_mode) and metadata.st_nlink == 1
                    and stat.S_IMODE(metadata.st_mode) == 0o600 and metadata.st_uid == os.getuid(),
                    "HTTPS source private override rejected")
            tls.compose = [*self.compose, "--file", "compose.https.yaml", "--file", str(override),
                           "--profile", "operator", "--profile", "https"]
            config = json.loads(tls.dc("config", "--format", "json").stdout)
            require(set(config["volumes"]) == set(RECOVERY_VOLUMES + OPTIONAL_RECOVERY_VOLUMES + ("proxy_data", "proxy_config")),
                    "HTTPS source volume inventory rejected")
            for name in ("proxy_data", "proxy_config"):
                volume = config["volumes"][name]
                require(not volume.get("external") and not volume.get("driver_opts")
                        and volume.get("driver") in (None, "local") and volume.get("name") == self.project + "_" + name,
                        "HTTPS source volume identity rejected")
            proxy = config["services"]["proxy"]
            require(proxy.get("image") == "opentrack-proxy:local" and proxy.get("user") == "10001:10001"
                    and proxy.get("read_only") is True and proxy.get("cap_drop") == ["ALL"]
                    and not proxy.get("cap_add") and not proxy.get("privileged") and not proxy.get("volumes_from")
                    and proxy.get("security_opt") == ["no-new-privileges:true"]
                    and not proxy.get("entrypoint") and not proxy.get("command")
                    and proxy.get("environment", {}).get("OPENTRACK_DOMAIN") == "localhost"
                    and proxy.get("tmpfs") == ["/tmp:size=16m,mode=1777"]
                    and set(proxy.get("networks", {})) == {"web", "offline_ingress"}
                    and all(connection in (None, {}) for connection in proxy["networks"].values())
                    and proxy.get("network_mode") in (None, "none")
                    and len(proxy.get("ports", [])) == 1 and proxy["ports"][0].get("host_ip") == "127.0.0.1"
                    and str(proxy["ports"][0].get("published")) == "18443" and proxy["ports"][0].get("target") == 8443
                    and len(proxy.get("volumes", [])) == 2
                    and all(mount.get("type") == "volume" and not mount.get("read_only") for mount in proxy["volumes"])
                    and {(mount.get("source"), mount.get("target")) for mount in proxy["volumes"]}
                    == {("proxy_data", "/data"), ("proxy_config", "/config")}, "HTTPS source proxy model rejected")
            require(all(config["services"]["app"]["environment"].get(key) == value for key, value in {
                "OPENTRACK_ENVIRONMENT": "production", "OPENTRACK__SERVER__LOCAL_CONTAINER": "false",
                "OPENTRACK_PUBLIC_URL": https.ORIGIN}.items()), "HTTPS source configuration rejected")
            projected = copy.deepcopy(config)
            del projected["services"]["proxy"]
            for name in ("proxy_data", "proxy_config"):
                del projected["volumes"][name]
            validate_compose(projected, self.project, self.port)
            validate_operator_output_profile(projected)
            for key in ("OPENTRACK_ENVIRONMENT", "OPENTRACK__SERVER__LOCAL_CONTAINER", "OPENTRACK_PUBLIC_URL"):
                projected["services"]["app"]["environment"][key] = http_config["services"]["app"]["environment"][key]
            require(projected == http_config, "HTTPS source HTTP model changed")
            images = {}
            for name in ("app", "worker", "database", "offline-relay"):
                item = self.container(name)
                state = item["State"]
                require(state.get("Running") is True and not state.get("Restarting")
                        and not state.get("OOMKilled")
                        and (state.get("Health") or {}).get("Status", "healthy") == "healthy",
                        "HTTPS source requires running HTTP services")
                images[name] = item["Image"]
            images["proxy"] = images["offline-relay"]
            for image in {bootstrap_image, *images.values()}:
                information = json.loads(self.command(["docker", "image", "inspect", image], timeout=15).stdout)
                require(len(information) == 1 and information[0].get("Id") == image
                        and information[0].get("Os") == "linux"
                        and information[0].get("Architecture") == self.expected_architecture, "HTTPS source image rejected")
            cached = json.loads(self.command(["docker", "image", "inspect", "opentrack-proxy:local"], timeout=15).stdout)
            require(len(cached) == 1 and cached[0].get("Id") == images["proxy"], "HTTPS source proxy image changed")
            stage, mutated = "tls_start", True
            stop_http_services()
            start("app", "proxy")
            verify_runtime()
            stage = "tls_trust"
            tls.export_certificate(tls.container("proxy"))
            tls.certificate.chmod(0o600)
            metadata = tls.certificate.lstat()
            require(stat.S_ISREG(metadata.st_mode) and metadata.st_uid == os.getuid()
                    and stat.S_IMODE(metadata.st_mode) == 0o600, "HTTPS source private certificate rejected")
            context = ssl.SSLContext(ssl.PROTOCOL_TLS_CLIENT)
            context.minimum_version = ssl.TLSVersion.TLSv1_2
            context.load_verify_locations(cafile=str(tls.certificate))
            require(context.check_hostname and context.verify_mode == ssl.CERT_REQUIRED
                    and context.cert_store_stats()["x509_ca"] == 1, "HTTPS source trust rejected")
            opener = tls.opener(context)
            tls.ready(opener)
            require(tls.rejects_untrusted(ssl.SSLContext(ssl.PROTOCOL_TLS_CLIENT))
                    and tls.rejects_untrusted(ssl.create_default_context()), "HTTPS source untrusted CA accepted")
            stage = "original_controls"
            owner = https.readonly_actor(self, opener)
            reader = https.readonly_actor(private["reader"], opener)
            tls_private = {**private, "reader": reader, "move": {**private["move"], "reader": reader}}
            original_controls()
            stage = "secure_login"
            secure_http = urllib.request.build_opener(urllib.request.ProxyHandler({}),
                urllib.request.HTTPSHandler(context=context), https.NoRedirects(),
                urllib.request.HTTPCookieProcessor(secure_cookies))
            target = https.ORIGIN + "/api/v1/auth/login"
            request = urllib.request.Request(target, method="POST", data=json.dumps({"email": account, "password": password}).encode(),
                headers={"Origin": https.ORIGIN, "Content-Type": "application/json", "Accept-Encoding": "identity"})
            with secure_http.open(request, timeout=30) as reply:
                require(reply.status == 200 and reply.geturl() == target
                        and reply.headers.get("Content-Type", "").partition(";")[0].strip().lower() == "application/json",
                        "HTTPS source login rejected")
                received = reply.read(https.MAX_RESPONSE + 1)
                require(len(received) <= https.MAX_RESPONSE, "HTTPS source login response rejected")
                session = json.loads(received)
            cookies = list(secure_cookies)
            require(len(cookies) == 1 and cookies[0].name == "opentrack_session" and cookies[0].secure
                    and cookies[0].has_nonstandard_attr("HttpOnly") and cookies[0].get_nonstandard_attr("SameSite") == "Lax"
                    and cookies[0].path == "/" and not cookies[0].domain_specified
                    and cookies[0].domain == http.cookiejar.eff_request_host(urllib.request.Request(https.ORIGIN))[1]
                    and cookies[0].value not in {cookie.value for cookie in self.cookies}
                    and session["user"]["id"] == owner_id, "HTTPS source secure cookie rejected")
            secure_actor = copy.copy(self)
            secure_actor.cookies, secure_actor.csrf = secure_cookies, session["csrf_token"]
            secure = https.readonly_actor(secure_actor, opener)
            secure_controls()
            established = True
            stage = "writer_fence"
            self.cleanup_operator_output()
            self.cleanup_operator_output(initializer=True)
            tls.dc("stop", "--timeout", "35", "proxy", "app", timeout=70)
            fingerprint = self.database_fingerprint()
            tls.dc("stop", "--timeout", "35", "database", timeout=55)
            identifiers = tls.command(["docker", "ps", "--all", "--quiet", "--filter",
                "label=com.docker.compose.project=" + self.project], timeout=15).stdout.split()
            require(identifiers, "HTTPS source writer fence rejected")
            items = json.loads(tls.command(["docker", "inspect", *identifiers], timeout=15).stdout)
            require(len(items) == len(identifiers) and all(
                (item["Config"].get("Labels") or {}).get("com.docker.compose.project") == self.project
                and (item["Config"].get("Labels") or {}).get("com.docker.compose.oneoff") not in ("True", "true")
                and not item["State"]["Running"] and not item["State"].get("Restarting")
                and not item["State"].get("OOMKilled") and item["State"].get("ExitCode") == 0
                for item in items), "HTTPS source writer fence rejected")
            for name in ("proxy_data", "proxy_config"):
                require(not tls.command(["docker", "ps", "--quiet", "--filter",
                    "volume=" + self.project + "_" + name], timeout=15).stdout.strip(),
                    "HTTPS source writer fence rejected")
            stage = "metadata"
            observed = tls.measure_metadata(bootstrap_image)
        except BaseException as error:
            primary = (stage, "interrupted" if isinstance(error, (KeyboardInterrupt, SystemExit)) else
                       "timeout" if isinstance(error, subprocess.TimeoutExpired) else "refused")
            if isinstance(error, (KeyboardInterrupt, SystemExit)):
                interruption = type(error)
        finally:
            if tls.metadata_reserved:
                try:
                    tls.cleanup_metadata(bootstrap_image)
                except BaseException:
                    cleanup.append("helper")
            if mutated:
                fenced = False
                try:
                    stop_http_services()
                    fenced = True
                except BaseException:
                    cleanup.append("tls_writer_stop")
                if fenced:
                    try:
                        start("database")
                        if fingerprint is not None:
                            require(self.database_fingerprint() == fingerprint, "HTTPS source database changed")
                    except BaseException:
                        cleanup.append("database_restart")
                    try:
                        start("app", "proxy")
                        verify_runtime()
                        require(established, "HTTPS source controls were not established")
                        tls.ready(opener)
                        original_controls()
                        secure_controls()
                        require(tls.rejects_untrusted(ssl.create_default_context()), "HTTPS source trust changed")
                    except BaseException:
                        cleanup.append("https_restart")
            try:
                secure_cookies.clear()
            except BaseException:
                cleanup.append("secure_session")
            if mutated:
                try:
                    tls.dc("stop", "--timeout", "35", "proxy", "app", timeout=70)
                except BaseException:
                    cleanup.append("tls_stop")
                try:
                    remove_proxy()
                except BaseException:
                    cleanup.append("proxy_removal")
                try:
                    self.dc("up", "--detach", "--wait", "--wait-timeout", "180", "--no-build", "--no-deps",
                            "database", "app", "worker", "offline-relay", timeout=240)
                    self.ready()
                    self.verify_offline_runtime(http_config)
                    session, _ = self.request("/api/v1/auth/session")
                    require(session["user"]["email"] == account
                            and (owner_id is None or session["user"]["id"] == owner_id), "HTTP source owner changed")
                    self.check_private_revocation(private, issue_path, attachment, body, digest)
                except BaseException:
                    cleanup.append("http_restore")
            try:
                if temporary is not None:
                    temporary.cleanup()
            except BaseException:
                cleanup.append("private_storage")
        if primary or cleanup:
            message = "HTTPS source metadata rejected"
            if primary:
                message += ": stage=" + primary[0] + "; primary=" + primary[1]
            if cleanup:
                message += "; cleanup=" + ",".join(cleanup)
            if interruption is not None and not cleanup:
                if interruption is KeyboardInterrupt:
                    raise KeyboardInterrupt from None
                raise SystemExit(message) from None
            raise RuntimeError(message) from None
        return {"metadata": observed, "source_https_restart_verified": True, "source_http_restore_verified": True,
                "source_proxy_removed": True,
                "original_sessions_verified": True, "secure_owner_session_verified": True,
                "system_trust_rejected_local_ca": True,
                "limitation": "class observation only; no archive policy or HTTPS cold restore qualified"}

    def cold_recovery(self, account, password, issue_path, attachment, body, digest):
        config = json.loads(self.dc("--profile", "operator", "config", "--format", "json").stdout)
        validate_compose(config, self.project, self.port)
        validate_operator_output_profile(config)
        selected_volumes = RECOVERY_VOLUMES + OPTIONAL_RECOVERY_VOLUMES
        restored = Qualification(argparse.Namespace(project=self.project + "-restore", url=self.url,
                                 output=str(self.output), expected_architecture=self.expected_architecture,
                                 compose_file=self.compose_files))
        restored_config = json.loads(restored.dc("--profile", "operator", "config", "--format", "json").stdout)
        validate_compose(restored_config, restored.project, self.port)
        validate_operator_output_profile(restored_config)
        for kind in ("container", "volume", "network"):
            arguments = ["docker", "ps", "--all", "--quiet"] if kind == "container" else ["docker", kind, "ls", "--quiet"]
            require(not self.command(arguments + ["--filter", f"label=com.docker.compose.project={restored.project}"]).stdout.strip(),
                    "Recovery requires a new disposable target project")
        for name in config["volumes"]:
            self.volume_info(f"{restored.project}_{name}", missing=True)
        backup = self.project + "_recovery_backup"
        self.volume_info(backup, missing=True)
        bootstrap_image = self.container("initialize")["Image"]
        source_image = self.container("app")["Image"]
        relay_image = self.container("offline-relay")["Image"]
        database_image = self.container("database")["Image"]
        self.command(["docker", "volume", "create", "--label", f"{RECOVERY_LABEL}={self.project}", backup])
        reader = Qualification(argparse.Namespace(project=self.project, url=self.url,
                               output=str(self.output), expected_architecture=self.expected_architecture,
                               compose_file=self.compose_files))
        source_stopped = False
        try:
            self.seed_operator_output(issue_path.rsplit("/issues/", 1)[0])
            self.volume_task("check-output", self.project, "operator_output", backup, bootstrap_image)
            self.record("restricted operator CLI populated the selected private output volume without exposing bearer data")
            private = self.seed_private_recovery(reader, issue_path.rsplit("/issues/", 1)[0])
            self.check_private_revocation(private, issue_path, attachment, body, digest)
            self.record("live revoked private-team reader keeps public access; saved private URLs conceal content and metadata")
            self.record("live private-A grant and unchanged control survive the move to private B; saved moved URLs conceal content and metadata")
            https_observation = self.measure_https_source(account, password, private, issue_path, attachment,
                                                         body, digest, bootstrap_image)
            source_stopped = True
            self.dc("stop", "--timeout", "35", "app", "worker", timeout=70)
            fingerprint = self.database_fingerprint()
            token_hash = hashlib.sha256(self.setup_token().encode()).digest()
            attachment_id = attachment.rsplit("/", 1)[1]
            require(re.fullmatch(r"[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}", attachment_id),
                    "Recovery attachment must have a canonical fixture ID")
            storage_key = self.sql("SELECT storage_key::text FROM attachments WHERE id='" + attachment_id + "'::uuid")
            require(re.fullmatch(r"[0-9a-f-]{36}", storage_key), "Recovery attachment fixture has no storage key")
            self.dc("stop", "--timeout", "35", timeout=90)
            containers = self.dc("ps", "--all", "--quiet").stdout.split()
            require(containers, "Recovery source has no containers")
            states = json.loads(self.command(["docker", "inspect", *containers]).stdout)
            require(all(not item["State"]["Running"] and not item["State"].get("Restarting")
                        and item["State"]["ExitCode"] == 0 for item in states),
                    "A coherent cold snapshot requires every source service stopped cleanly")
            for name in selected_volumes:
                self.volume_task("snapshot", self.project, name, backup, bootstrap_image)
            for name in config["volumes"]:
                volume = f"{restored.project}_{name}"
                self.command(["docker", "volume", "create", "--label", f"{RECOVERY_LABEL}={self.project}",
                              "--label", f"com.docker.compose.project={restored.project}",
                              "--label", f"com.docker.compose.volume={name}", volume])
            self.record("fully stopped database, blobs and five matching secret volumes archived privately")
            for action in ("reject-missing", "reject-corrupt"):
                self.volume_task(action, restored.project, "blobs", backup, bootstrap_image)
            self.record("missing and corrupt recovery archives rejected before target writes")
            for name in selected_volumes:
                self.volume_task("restore", restored.project, name, backup, bootstrap_image)
            self.record("all seven core and selected optional restored inventories, bytes, UID/GID and modes match their cold snapshots")
            require(self.operator_output_container(initializer=True) is None,
                    "Private output initialization refuses a preexisting named container")
            try:
                restored.dc("run", "--no-deps", "--name", restored.project + "-operator-output",
                            "--label", f"{RECOVERY_LABEL}={self.project}", "-T", "operator-output-init")
            except (RuntimeError, OSError, ValueError, subprocess.SubprocessError):
                raise RuntimeError("Private restored output initialization failed; details withheld") from None
            finally:
                self.cleanup_operator_output(initializer=True)
            require(self.volume_task("verify", restored.project, "operator_output", backup, bootstrap_image).returncode == 0,
                    "Private restored output changed during its networkless initializer")
            self.record("selected operator output restored with identical private bytes and metadata before its networkless initializer; populated contents remain unchanged")
            restored.dc("up", "--detach", "--wait", "--wait-timeout", "180", "database", timeout=240)
            require(restored.container("database")["Image"] == database_image,
                    "Recovery used a different database image")
            require(restored.database_fingerprint() == fingerprint,
                    "Recovered database data, sequences, migrations or authority differ")
            role_check = """
for pair in runtime-password:opentrack migration-password:opentrack_migrator worker-password:opentrack_worker operator-password:opentrack_operator; do
    PGPASSWORD=$(cat "/run/secrets/${pair%:*}")
    export PGPASSWORD
    psql -X -q -w -v ON_ERROR_STOP=1 -h 127.0.0.1 -U "${pair#*:}" -d opentrack -tAc 'SELECT NOT rolsuper AND NOT rolbypassrls AND NOT rolcreatedb AND NOT rolcreaterole FROM pg_roles WHERE rolname=current_user' | grep -qx t
done
"""
            restored.dc("exec", "-T", "database", "/bin/sh", "-eu", "-c", role_check)
            require(hashlib.sha256(restored.setup_token().encode()).digest() == token_hash,
                    "Recovered setup credential differs")
            self.record("restored data/sequence/migration and owner/ACL/RLS/function fingerprints match; four restricted roles authenticate")
            restored.cookies, restored.http, restored.csrf = self.cookies, self.http, self.csrf
            restored.dc("up", "--detach", "--wait", "--wait-timeout", "180", "offline-relay", timeout=240)
            restored.ready()
            restored.verify_offline_runtime(json.loads(restored.dc("config", "--format", "json").stdout))
            require(restored.container("app")["Image"] == source_image, "Recovery used a different application image")
            require(restored.container("offline-relay")["Image"] == relay_image, "Recovery used a different relay image")
            preserved, _ = restored.request(issue_path)
            require(preserved["title"] == "Docker updated persistence acceptance" and preserved["priority"] == 2,
                    "Restored session or issue state differs")
            session, _ = restored.request("/api/v1/auth/login", "POST", {"email": account, "password": password})
            restored.csrf = session["csrf_token"]
            document, _ = restored.request(issue_path + "/document")
            content, _ = restored.request(attachment + "/content")
            require(document["body"] == body and hashlib.sha256(content).hexdigest() == digest,
                    "Restored document or attachment checksum differs")
            restored.request(attachment + "/content", authenticated=False, expected=(401,))
            self.record("isolated offline restore preserves setup token, session, login, issue, document and populated attachment")
            restored.check_private_revocation(private, issue_path, attachment, body, digest)
            self.record("cold restore preserves the same reader session and current private-team denial with authorized owner and public controls")
            self.record("cold restore preserves the same reader and private-A control while moved private-B issue, documents and file remain concealed")
            for action in ("missing-blob", "corrupt-blob"):
                restored.dc("stop", "--timeout", "35", "app", timeout=55)
                self.volume_task(action, restored.project, "blobs", backup, bootstrap_image, storage_key)
                rejected = self.volume_task("verify", restored.project, "blobs", backup, bootstrap_image)
                require(rejected.returncode == 1, "Recovery preflight accepted a missing or corrupt blob")
                restored.dc("up", "--detach", "--wait", "--wait-timeout", "180", "--no-deps", "app", timeout=240)
                restored.ready()
                failure, headers = restored.request(attachment + "/content", expected=(503,))
                require(isinstance(failure, dict) and failure.get("status") == 503
                        and failure.get("code") == "dependency_unavailable"
                        and headers.get("Cache-Control") == "no-store"
                        and not headers.get("Content-Disposition"),
                        "Unavailable attachment must return a non-download dependency problem")
                restored.dc("stop", "--timeout", "35", "app", timeout=55)
                self.volume_task("repair-blob", restored.project, "blobs", backup, bootstrap_image, storage_key)
                require(self.volume_task("verify", restored.project, "blobs", backup, bootstrap_image).returncode == 0,
                        "Original backup did not repair the faulted restore fixture")
            self.record("missing/corrupt restored blobs fail preflight and authorized downloads without exposing bytes; original backup repairs both")
        finally:
            try:
                try:
                    self.cleanup_recovery()
                finally:
                    if source_stopped:
                        self.dc("up", "--detach", "--wait", "--wait-timeout", "180", timeout=240)
                        self.ready()
            finally:
                reader.cookies.clear()
        self.record("only the owned restore project removed; source installation and private recovery backup retained")
        return {"volumes": list(selected_volumes), "backup_volume": backup,
                "https_metadata": https_observation,
                "external_dispatch": "restored worker never started; application networks internal; fixed HTTP relay only",
                "limitations": ["cold same-host/same-architecture snapshot, not an encrypted scheduled backup profile",
                                "optional HTTPS volumes and recovery-code redemption are outside this fixture",
                                "provider encryption keys and future module data are not implemented fixture coverage",
                                "not a prior-release migration failure or different-host restore rehearsal"]}

    def run(self):
        config = json.loads(self.dc("config", "--format", "json").stdout)
        validate_compose(config, self.project, self.port)
        images = sorted({service["image"] for service in config["services"].values()})
        self.command(["docker", "image", "inspect", *images])
        self.verify_offline_runtime(config)
        self.record("fresh runtime uses cached images, internal application networks and a fixed loopback HTTP relay")
        version = json.loads(self.command(["docker", "version", "--format", "{{json .Server}}"]).stdout)
        engine_version = re.match(r"^(\d+)\.", version.get("Version", ""))
        require(engine_version and int(engine_version.group(1)) >= 28,
                "The loopback-only Docker path requires Engine server version 28 or newer")
        require(version.get("Os") == "linux" and version.get("Arch") == self.expected_architecture,
                "Docker engine OS/architecture does not match the requested native Linux qualification")
        published = self.dc("port", "offline-relay", "8080").stdout.strip()
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
        require(self.sql("SELECT to_regclass('public.cycles') IS NOT NULL AND EXISTS(SELECT 1 FROM pg_extension e JOIN pg_namespace n ON n.oid=e.extnamespace WHERE e.extname='btree_gist' AND n.nspname='opentrack_extensions' AND e.extowner='opentrack_migrator'::regrole AND n.nspowner=e.extowner) AND NOT has_schema_privilege('opentrack','opentrack_extensions','USAGE,CREATE') AND NOT has_schema_privilege('opentrack_worker','opentrack_extensions','USAGE,CREATE')") == "t", "Public application schema must isolate trusted extension authority from runtime and worker")
        self.record("public-schema worker captured events with migration-owned isolated cycle index extension")
        self.record("restricted worker captured real durable workspace events")

        event_spec = importlib.util.spec_from_file_location("event_admission_runtime", Path(__file__).with_name("event_admission_runtime.py"))
        event_admission = importlib.util.module_from_spec(event_spec)
        event_spec.loader.exec_module(event_admission)
        event_admission.qualify(self, base, issue_path, require)
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

        recovery = self.cold_recovery(account, password, issue_path, attachment, body, digest)

        receipt = {"project": self.project, "docker": version.get("Version"),
                   "compose": self.command(["docker", "compose", "version", "--short"]).stdout.strip(),
                   "architecture": version.get("Arch"), "expected_architecture": self.expected_architecture,
                   "runtime_images": runtime_images, "checks": self.checks,
                   "recovery": recovery,
                   "attachment_bytes": len(content), "shutdown_seconds": round(elapsed, 2),
                   "limitations": ["not a native Docker Desktop host certification",
                                   "not whole-product recovery, provider-key recovery or production TLS qualification"]}
        self.output.parent.mkdir(parents=True, exist_ok=True)
        self.output.write_text(json.dumps(receipt, indent=2) + "\n", encoding="utf-8")
        print(f"Recorded {len(self.checks)} Docker runtime checks in {self.output}", flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--project", required=True)
    parser.add_argument("--url", default="http://localhost:18080")
    parser.add_argument("--expected-architecture", required=True, choices=("amd64", "arm64"))
    parser.add_argument("--output", default="work/docker-smoke/receipt.json")
    parser.add_argument("--compose-file", action="append", default=[],
                        help="Additional Compose override, applied after compose.yaml")
    parser.add_argument("--cleanup-recovery", action="store_true",
                        help="Remove only this smoke project's labelled restore fixture")
    parser.add_argument("--cleanup-backup", action="store_true",
                        help="With --cleanup-recovery, also remove its labelled synthetic backup")
    args = parser.parse_args()
    def interrupted(_signal, _frame):
        raise SystemExit("Docker qualification interrupted")
    signal.signal(signal.SIGTERM, interrupted)
    try:
        require(not args.cleanup_backup or args.cleanup_recovery, "Backup cleanup requires explicit recovery cleanup")
        qualification = Qualification(args)
        if args.cleanup_recovery:
            qualification.cleanup_recovery(remove_backup=args.cleanup_backup, include_https_metadata=True)
            print("Only the exact owned recovery fixture was cleaned", flush=True)
        else:
            qualification.run()
    except (RuntimeError, OSError, ValueError, subprocess.TimeoutExpired) as error:
        raise SystemExit(f"Docker qualification failed: {error}") from None
    except KeyboardInterrupt:
        raise SystemExit("Docker qualification interrupted") from None


if __name__ == "__main__":
    main()
