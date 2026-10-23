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
import inspect
import json
import os
from pathlib import Path
import re
import secrets
import subprocess
import tarfile
import time
import urllib.error
import urllib.parse
import urllib.request
from event_admission_runtime import qualify as qualify_event_admission


RECOVERY_VOLUMES = ("database", "blobs", "database_secrets", "api_secrets",
                    "migration_secrets", "worker_secrets", "operator_secrets")
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
    require(set(RECOVERY_VOLUMES) <= set(volumes) <= set(RECOVERY_VOLUMES) | {"operator_output"},
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
    require(volume in RECOVERY_VOLUMES, "Unknown recovery volume")
    root, backup = Path("/volume"), Path("/backup")
    archive_path, checksum = backup / f"{volume}.tar", backup / f"{volume}.sha256"
    if action == "snapshot":
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

    def cleanup_recovery(self, *, remove_backup=False):
        target = self.project + "-restore"
        restored = Qualification(argparse.Namespace(project=target, url=self.url,
                                 output=str(self.output), expected_architecture=self.expected_architecture,
                                 compose_file=self.compose_files))
        config = json.loads(restored.dc("config", "--format", "json").stdout)
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
            allowed = set(volumes) | {f"{self.project}_{name}" for name in RECOVERY_VOLUMES} | {backup}
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
        require(project in (self.project, self.project + "-restore") and volume in RECOVERY_VOLUMES,
                "Recovery refuses a volume outside its exact source/target inventory")
        require(backup == self.project + "_recovery_backup", "Recovery backup is outside its owned inventory")
        information = self.volume_info(f"{project}_{volume}")
        labels = information.get("Labels") or {}
        require(labels.get("com.docker.compose.project") == project
                and labels.get("com.docker.compose.volume") == volume,
                "Recovery volume has unexpected ownership labels")
        backup_info = self.volume_info(backup)
        require((backup_info.get("Labels") or {}).get(RECOVERY_LABEL) == self.project,
                "Recovery backup has unexpected ownership labels")
        require(project != self.project or action == "snapshot", "Recovery refuses to change a source volume")
        helper = "import hashlib, os, re, signal, sys, tarfile\nfrom pathlib import Path\nsignal.alarm(150)\n"
        helper += "RECOVERY_VOLUMES = " + repr(RECOVERY_VOLUMES) + "\n"
        helper += "\n\n".join(inspect.getsource(function)
                                 for function in (require, safe_members, checked_archive, volume_operation))
        helper += "\ntry:\n    volume_operation(*sys.argv[1:])\nexcept RuntimeError as error:\n    raise SystemExit('Recovery validation rejected: ' + str(error))\nexcept OSError as error:\n    raise SystemExit('Recovery filesystem failure: errno=' + str(error.errno))\nexcept (ValueError, tarfile.TarError):\n    raise SystemExit('Recovery archive failure: invalid format')\n"
        readonly = action in ("snapshot", "verify", "reject-missing", "reject-corrupt")
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

    def seed_private_recovery(self, reader, base):
        team, _ = self.request(base + "/teams", "POST",
            {"name": "Private restore team", "key": "RCV", "private": True},
            headers={"Idempotency-Key": "docker-private-team"}, expected=(201,))
        invitation, _ = self.request(base + "/invitations", "POST",
            {"email": "restore-reader@example.test", "role": "viewer", "team_ids": [team["id"]]},
            headers={"Idempotency-Key": "docker-private-reader"}, expected=(201,))
        session, _ = reader.request("/api/v1/invitations/accept", "POST",
            {"token": invitation["token"], "display_name": "Example Reader",
             "password": secrets.token_urlsafe(36) + "A1!"})
        reader.csrf = session["csrf_token"]
        content = b"private-recovery-byte-marker-40192\n"
        digest = hashlib.sha256(content).hexdigest()
        file, headers = self.request(base + "/attachments", "POST",
            {"team_id": team["id"], "original_name": "private-cold-restore.txt",
             "size_bytes": len(content), "sha256": digest, "mime_hint": "text/plain"},
            headers={"Idempotency-Key": "docker-private-file"}, expected=(201,))
        attachment = base + "/attachments/" + file["id"]
        self.request(attachment + "/content", "PUT", raw=content,
            headers={"If-Match": headers["ETag"], "Content-Type": "application/octet-stream"})
        body = {"type": "doc", "content": [
            {"type": "paragraph", "content": [{"type": "text", "text": "Private cold restore fixture"}]},
            {"type": "attachment", "attrs": {"id": file["id"]}},
        ]}
        issue, _ = self.request(base + "/issues", "POST",
            {"team_id": team["id"], "title": "Private cold restore fixture",
             "document": {"schema_version": 1, "body": body}},
            headers={"Idempotency-Key": "docker-private-issue"}, expected=(201,))
        issue_path = base + "/issues/" + issue["id"]
        document, _ = self.request(issue_path + "/document")
        fixture = {"reader": reader, "reader_id": session["user"]["id"],
                   "workspace": base.rsplit("/", 1)[1], "team": team["id"],
                   "issue": issue_path, "document": base + "/documents/" + document["id"],
                   "attachment": attachment, "body": body, "digest": digest,
                   "markers": (team["id"], issue["id"], document["id"], file["id"], digest,
                               "Private cold restore fixture", "private-cold-restore.txt", content.decode().strip())}
        fixture["etag"] = reader.check_private_reads(fixture)
        fixture["cookies"] = tuple((c.name, c.value, c.domain, c.path) for c in reader.cookies)
        require(fixture["cookies"], "Private reader did not receive an independent session")
        membership = base + "/teams/" + team["id"] + "/members/" + fixture["reader_id"]
        _, headers = self.request(membership)
        self.request(membership, "DELETE", headers={"If-Match": headers["ETag"]}, expected=(204,))
        self.record("synthetic private-team viewer reads saved resources before team membership removal")
        return fixture

    def check_private_reads(self, fixture):
        issue, _ = self.request(fixture["issue"])
        document, _ = self.request(fixture["issue"] + "/document")
        direct, _ = self.request(fixture["document"])
        metadata, headers = self.request(fixture["attachment"])
        content, _ = self.request(fixture["attachment"] + "/content")
        require(issue["id"] == fixture["issue"].rsplit("/", 1)[1]
                and issue["team_id"] == fixture["team"] and issue["title"] == "Private cold restore fixture"
                and document["id"] == direct["id"] == fixture["document"].rsplit("/", 1)[1]
                and document["body"] == direct["body"] == fixture["body"]
                and metadata["id"] == fixture["attachment"].rsplit("/", 1)[1]
                and metadata["sha256"] == fixture["digest"]
                and hashlib.sha256(content).hexdigest() == fixture["digest"],
                "Authorized private issue, document or attachment control differs")
        require(bool(headers.get("ETag")), "Private attachment control has no saved validator")
        return headers["ETag"]

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
        paths = (fixture["issue"], fixture["issue"] + "/document", fixture["document"],
                 fixture["attachment"], fixture["attachment"] + "/content")
        for path in paths:
            variants = ({}, {"Range": "bytes=0-15", "If-None-Match": fixture["etag"]},
                        {"If-None-Match": "*"}) if path.startswith(fixture["attachment"]) else ({},)
            for fields in variants:
                problem, headers = reader.request(path, headers=fields, expected=(404,))
                require_concealed(problem, headers)
                require(not any(marker in str(headers) for marker in fixture["markers"]),
                        "Private resource denial disclosed fixture metadata")
        require(tuple((c.name, c.value, c.domain, c.path) for c in reader.cookies) == fixture["cookies"],
                "Revocation checks must reuse the original reader session without replacement")

    def cold_recovery(self, account, password, issue_path, attachment, body, digest):
        config = json.loads(self.dc("config", "--format", "json").stdout)
        validate_compose(config, self.project, self.port)
        restored = Qualification(argparse.Namespace(project=self.project + "-restore", url=self.url,
                                 output=str(self.output), expected_architecture=self.expected_architecture,
                                 compose_file=self.compose_files))
        validate_compose(json.loads(restored.dc("config", "--format", "json").stdout), restored.project, self.port)
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
            private = self.seed_private_recovery(reader, issue_path.rsplit("/issues/", 1)[0])
            self.check_private_revocation(private, issue_path, attachment, body, digest)
            self.record("live revoked private-team reader keeps public access; saved private URLs conceal content and metadata")
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
            for name in RECOVERY_VOLUMES:
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
            for name in RECOVERY_VOLUMES:
                self.volume_task("restore", restored.project, name, backup, bootstrap_image)
            self.record("all seven restored volume inventories, bytes, UID/GID and modes match their cold snapshots")
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
        return {"volumes": list(RECOVERY_VOLUMES), "backup_volume": backup,
                "external_dispatch": "restored worker never started; application networks internal; fixed HTTP relay only",
                "limitations": ["cold same-host/same-architecture snapshot, not an encrypted scheduled backup profile",
                                "operator_output and optional HTTPS volumes are outside the normal runtime fixture",
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
        qualify_event_admission(self, base, issue_path, require)

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
    try:
        require(not args.cleanup_backup or args.cleanup_recovery, "Backup cleanup requires explicit recovery cleanup")
        qualification = Qualification(args)
        if args.cleanup_recovery:
            qualification.cleanup_recovery(remove_backup=args.cleanup_backup)
            print("Only the exact owned recovery fixture was cleaned", flush=True)
        else:
            qualification.run()
    except (RuntimeError, OSError, ValueError, subprocess.TimeoutExpired) as error:
        raise SystemExit(f"Docker qualification failed: {error}") from None


if __name__ == "__main__":
    main()
