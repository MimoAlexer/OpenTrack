"""Parent-only fixture preparation for the guarded fresh browser Compose project."""
import http.cookiejar
import json
import re
import time
import urllib.error
import urllib.request
from uuid import UUID, uuid4


BROWSERS = ("chromium", "firefox", "webkit")
PREPARATION_SECONDS = 300


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def identifier(value):
    require(isinstance(value, str) and str(UUID(value)) == value,
            "Browser fixture requires canonical identifiers")
    return value


def remaining(deadline, maximum):
    seconds = deadline - time.monotonic()
    require(seconds > 0, "Browser acceptance exceeded its bounded deadline")
    return min(seconds, maximum)


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, request, response, code, message, headers, url):
        return None


class Client:
    def __init__(self, origin, deadline):
        require(isinstance(origin, str) and re.fullmatch(r"http://(?:localhost|127\.0\.0\.1):18081", origin),
                "Browser fixture HTTP requires the validated dedicated loopback origin")
        self.origin, self.deadline, self.csrf = origin, deadline, None
        self.http = urllib.request.build_opener(urllib.request.ProxyHandler({}), NoRedirect(),
                                               urllib.request.HTTPCookieProcessor(http.cookiejar.CookieJar()))

    def request(self, path, method="GET", body=None, headers=None, expected=(200,)):
        require(isinstance(path, str) and path.startswith("/api/v1/")
                and not any(char in path for char in ("\r", "\n", "#", "\\")),
                "Browser fixture requires a fixed local API path")
        fields = {"Origin": self.origin}
        if self.csrf and method != "GET":
            fields["X-CSRF-Token"] = self.csrf
        if body is not None:
            fields["Content-Type"] = "application/json"
        fields.update(headers or {})
        request = urllib.request.Request(self.origin + path, method=method, headers=fields,
                                         data=None if body is None else json.dumps(body).encode())
        try:
            response = self.http.open(request, timeout=remaining(self.deadline, 15))
        except urllib.error.HTTPError as error:
            response = error
        with response:
            require(response.status in expected, "Browser fixture API request failed; response withheld")
            payload = response.read(1048577)
            require(len(payload) <= 1048576, "Browser fixture API response exceeded its bound")
        return json.loads(payload) if payload else None

    def authenticate(self, path, body, expected=(200,)):
        session = self.request(path, "POST", body, expected=expected)
        require(isinstance(session.get("csrf_token"), str) and session["csrf_token"],
                "Browser fixture authentication did not return a session")
        self.csrf = session["csrf_token"]
        identifier(session["user"]["id"])
        return session


def validate_manifest(manifest):
    require(isinstance(manifest, dict) and set(manifest) == {"COMPLETE", "workspace_id", "maintenance"}
            and manifest["COMPLETE"] is True and isinstance(manifest["maintenance"], dict)
            and set(manifest["maintenance"]) == set(BROWSERS),
            "Browser fixture manifest must be complete and contain only references")
    identifier(manifest["workspace_id"])
    identifiers = set()
    for item in manifest["maintenance"].values():
        require(isinstance(item, dict) and set(item) == {
            "team_id", "status_id", "job_id", "requester_id", "staged_count", "total_count"},
            "Browser fixture manifest must contain only identifiers and counts")
        for key in ("team_id", "status_id", "job_id", "requester_id"):
            value = identifier(item[key])
            require(value not in identifiers and value != manifest["workspace_id"],
                    "Browser fixture references must address distinct resources")
            identifiers.add(value)
        count = item["staged_count"]
        require(isinstance(count, str) and re.fullmatch(r"[1-9][0-9]{0,2}", count)
                and int(count) < 501 and item["total_count"] is None,
                "Browser fixture must retain partial staging with an unknown total")


def seed_statement(workspace, owner, team, status, key, name):
    workspace, owner, team, status = map(identifier, (workspace, owner, team, status))
    require((key, name) in {(f"ZBO{index}", f"ZZ Browser order {browser}")
                           for index, browser in enumerate(BROWSERS, 1)},
            "Browser fixture SQL may address only its synthetic teams")
    return f"""BEGIN;
SET LOCAL statement_timeout='10s'; SET LOCAL lock_timeout='1s';
DO $$ BEGIN
 IF current_database()<>'opentrack' OR session_user<>'postgres'
 OR (SELECT count(*) FROM workspaces)<>1
 OR NOT EXISTS(SELECT 1 FROM workspaces WHERE id='{workspace}' AND name='Browser acceptance')
 OR NOT EXISTS(SELECT 1 FROM pg_roles WHERE rolname='opentrack_migrator'
  AND NOT rolsuper AND NOT rolbypassrls AND NOT rolcreaterole AND NOT rolcreatedb)
 OR EXISTS(SELECT 1 FROM pg_class WHERE oid IN('public.issues'::regclass,'public.teams'::regclass,
  'public.workflow_statuses'::regclass) AND pg_get_userbyid(relowner)<>'opentrack_migrator')
 THEN RAISE EXCEPTION 'Browser fixture database authority mismatch'; END IF;
END $$;
SET LOCAL ROLE opentrack_migrator;
SET LOCAL search_path=pg_catalog,public;
SET LOCAL opentrack.workspace_id='{workspace}'; SET LOCAL opentrack.user_id='{owner}';
DO $$ DECLARE affected bigint; BEGIN
 IF NOT EXISTS(SELECT 1 FROM workspace_members WHERE workspace_id='{workspace}'
  AND user_id='{owner}' AND role='owner' AND active)
 OR NOT EXISTS(SELECT 1 FROM teams WHERE workspace_id='{workspace}' AND id='{team}'
  AND name='{name}' AND key='{key}' AND private AND archived_at IS NULL AND next_issue_number=1)
 OR NOT EXISTS(SELECT 1 FROM workflow_statuses WHERE workspace_id='{workspace}' AND team_id='{team}'
  AND id='{status}' AND is_default AND archived_at IS NULL AND retiring_at IS NULL)
 OR EXISTS(SELECT 1 FROM issues WHERE workspace_id='{workspace}' AND team_id='{team}')
 THEN RAISE EXCEPTION 'Browser fixture input scope mismatch'; END IF;
 PERFORM 1 FROM teams WHERE workspace_id='{workspace}' AND id='{team}' FOR UPDATE;
 INSERT INTO issues(id,workspace_id,team_id,created_by_user_id,number,title,status_id,rank)
 SELECT gen_random_uuid(),'{workspace}','{team}','{owner}',n,'Browser order input '||n,'{status}',
  CASE WHEN n=501 THEN 99999999999999999999.999999999999999999::numeric ELSE n::numeric*1024 END
 FROM generate_series(1,501) n;
 GET DIAGNOSTICS affected=ROW_COUNT;
 IF affected<>501 THEN RAISE EXCEPTION 'Browser fixture input count mismatch'; END IF;
 UPDATE teams SET next_issue_number=502 WHERE workspace_id='{workspace}' AND id='{team}' AND next_issue_number=1;
 GET DIAGNOSTICS affected=ROW_COUNT;
 IF affected<>1 THEN RAISE EXCEPTION 'Browser fixture counter mismatch'; END IF;
END $$; COMMIT;"""


def progress(owner, path, team, status, job):
    current = owner.request(path)
    require(current.get("id") == job and current.get("team_id") == team
            and current.get("status_id") == status and current.get("phase") == "staging"
            and current.get("applied_count") == "0" and current.get("total_count") is None
            and isinstance(current.get("staged_count"), str)
            and re.fullmatch(r"0|[1-9][0-9]{0,2}", current["staged_count"])
            and int(current["staged_count"]) < 501,
            "Browser fixture lost its partial staging boundary")
    return current


def prepare(runner, token, password, *, suite_deadline):
    deadline = min(suite_deadline, time.monotonic() + PREPARATION_SECONDS)
    owner = Client(runner.url, deadline)
    require(owner.request("/api/v1/setup/status").get("setup_required") is True,
            "Browser setup must target a fresh database")
    worker = runner.owned_service("worker", timeout=remaining(deadline, 55))
    runner.owned_service("database", timeout=remaining(deadline, 55))
    session = owner.authenticate("/api/v1/setup", {
        "setup_token": token, "email": "owner@example.test", "password": password,
        "display_name": "Example Owner", "workspace_name": "Browser acceptance",
        "team_name": "Example Team", "team_key": "EXA"}, expected=(201,))
    require(len(session["workspaces"]) == 1 and session["workspaces"][0]["name"] == "Browser acceptance",
            "Browser fixture must use the sole fresh synthetic workspace")
    workspace = identifier(session["workspaces"][0]["id"])
    owner_id = identifier(session["user"]["id"])
    base = f"/api/v1/workspaces/{workspace}"
    baseline = owner.request(base + "/bootstrap")["teams"]
    require(len(baseline) == 1 and baseline[0]["name"] == "Example Team" and baseline[0]["key"] == "EXA",
            "Browser fixture baseline team is not the fresh setup team")
    baseline_id = identifier(baseline[0]["id"])
    manifest = {"COMPLETE": True, "workspace_id": workspace, "maintenance": {}}
    try:
        runner.dc("stop", "--timeout", "35", "worker", timeout=remaining(deadline, 55))
        require(runner.owned_service("worker", running=False, timeout=remaining(deadline, 55)) == worker,
                "Browser fixture worker changed during preparation")
        for index, browser in enumerate(BROWSERS, 1):
            name, key = f"ZZ Browser order {browser}", f"ZBO{index}"
            team = owner.request(base + "/teams", "POST", {"name": name, "key": key, "private": True},
                                 {"Idempotency-Key": str(uuid4())}, expected=(201,))
            team_id = identifier(team["id"])
            workflow = owner.request(base + f"/teams/{team_id}/workflow")
            defaults = [item for item in workflow["statuses"] if item["is_default"]
                        and not item.get("archived_at") and not item.get("retiring_at")]
            require(len(defaults) == 1, "Browser fixture requires one live default status")
            status_id = identifier(defaults[0]["id"])
            invitation = owner.request(base + "/invitations", "POST", {
                "email": f"browser-order-{browser}@example.test", "role": "member", "team_ids": [team_id]},
                {"Idempotency-Key": str(uuid4())}, expected=(201,))
            member = Client(runner.url, deadline)
            requester = member.authenticate("/api/v1/invitations/accept", {
                "token": invitation["token"], "display_name": "Example Member", "password": password})
            requester_id = identifier(requester["user"]["id"])
            runner.owned_sql(seed_statement(workspace, owner_id, team_id, status_id, key, name),
                             timeout=remaining(deadline, 20))
            problem = member.request(base + "/issues", "POST", {
                "team_id": team_id, "status_id": status_id, "title": "Unapplied browser order input"},
                {"Idempotency-Key": str(uuid4())}, expected=(409,))
            details = problem.get("details", {})
            require(problem.get("code") == "issue_order_maintenance" and details.get("applied") is False
                    and len(details.get("maintenance", [])) == 1,
                    "Browser fixture did not enqueue a real unapplied maintenance intent")
            queued = details["maintenance"][0]
            job = identifier(queued["id"])
            path = base + f"/teams/{team_id}/issue-order-maintenance/{job}"
            current = progress(owner, path, team_id, status_id, job)
            require(current["state"] == "queued" and current["staged_count"] == "0",
                    "Browser fixture must begin with a real queued job")
            for _ in range(12):
                runner.dc("run", "--rm", "--no-deps", "-T", "worker", "worker", "--once",
                          timeout=remaining(deadline, 45))
                current = progress(owner, path, team_id, status_id, job)
                require(current["state"] == "queued", "Browser fixture staging worker did not remain queued")
                if int(current["staged_count"]) > 0:
                    break
            require(int(current["staged_count"]) > 0, "Browser fixture worker did not reach partial staging")
            staged = current["staged_count"]
            membership_path = base + f"/teams/{team_id}/members/{requester_id}"
            membership = owner.request(membership_path)
            version = membership.get("version")
            require(isinstance(version, str) and re.fullmatch(r"[1-9][0-9]*", version),
                    "Browser fixture membership lacks a version")
            owner.request(membership_path, "DELETE", headers={
                "If-Match": f'"team-member:{team_id}:{requester_id}:{version}"'}, expected=(204,))
            member.request(path, expected=(404,))
            for _ in range(12):
                runner.dc("run", "--rm", "--no-deps", "-T", "worker", "worker", "--once",
                          timeout=remaining(deadline, 45))
                current = progress(owner, path, team_id, status_id, job)
                require(current["staged_count"] == staged,
                        "Browser fixture progressed after requester access revocation")
                if current["state"] == "blocked":
                    break
                require(current["state"] == "queued", "Browser fixture access-loss transition failed")
            require(current["state"] == "blocked" and current.get("error_code") == "access_lost"
                    and current.get("can_resume") is True,
                    "Browser fixture did not reach resumable real access loss")
            manifest["maintenance"][browser] = {
                "team_id": team_id, "status_id": status_id, "job_id": job,
                "requester_id": requester_id, "staged_count": staged, "total_count": None}
        require(owner.request(base + "/bootstrap")["teams"][0]["id"] == baseline_id,
                "Browser fixture teams displaced the baseline saved team")
        validate_manifest(manifest)
    finally:
        require(runner.owned_service("worker", running=None) == worker,
                "Browser fixture cannot restore a different worker")
        runner.dc("start", "worker", timeout=55)
        require(runner.owned_service("worker") == worker, "Browser fixture worker restoration failed")
    return manifest
