"""Real Compose event-admission qualification for the disposable smoke project."""
from contextlib import contextmanager
import json
import re
import urllib.parse
from uuid import UUID, uuid4


@contextmanager
def _stopped_worker(q):
    failure = None
    try:
        q.dc("stop", "--timeout", "35", "worker", timeout=55)
        yield
    except BaseException as error:
        failure = error
        raise
    finally:
        try:
            q.dc("start", "worker", timeout=55)
        except BaseException:
            if failure is None:
                raise
            failure.add_note("Owned worker restart also failed; inspect the disposable project.")


def qualify(q, base, issue_path, require):
    uuid_pattern = r"[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}"
    require(isinstance(q.project, str)
            and re.fullmatch(r"opentrack-smoke-[a-z0-9-]+", q.project),
            "Event qualification requires an explicitly disposable smoke project")
    require(isinstance(q.url, str) and not any(ord(char) <= 32 or ord(char) == 127 for char in q.url),
            "Event qualification requires an HTTP loopback origin")
    try:
        origin = urllib.parse.urlsplit(q.url)
        port = 80 if origin.port is None else origin.port
    except ValueError:
        raise RuntimeError("Event qualification requires an HTTP loopback origin") from None
    require(origin.scheme == "http" and origin.hostname in ("localhost", "127.0.0.1") and 1 <= port <= 65535
            and origin.username is None and origin.password is None and origin.path in ("", "/")
            and not origin.query and not origin.fragment,
            "Event qualification requires an HTTP loopback origin")
    require(isinstance(base, str) and re.fullmatch(r"/api/v1/workspaces/" + uuid_pattern, base),
            "Event qualification requires a canonical workspace path")
    require(isinstance(issue_path, str)
            and re.fullmatch(re.escape(base) + r"/issues/" + uuid_pattern, issue_path),
            "Event qualification requires a canonical issue in the same workspace")
    workspace = base.rsplit("/", 1)[1]
    require(q.dc("port", "offline-relay", "8080").stdout.strip() == f"127.0.0.1:{port}",
            "Event qualification origin must match the owned project's published port")
    require(q.sql("SELECT count(*)=1 AND bool_and(id='" + workspace + "'::uuid) FROM workspaces") == "t",
            "Event qualification requires the single disposable fixture workspace")
    source, _ = q.request(issue_path)
    team = source.get("team_id", "")
    require(isinstance(team, str) and re.fullmatch(uuid_pattern, team),
            "Event qualification requires the fixture's canonical team")
    _, workspace_headers = q.request(base)

    for service in ("operator-provision", "operator-grant", "operator-output-init"):
        q.dc("run", "--rm", "--no-deps", "-T", service)

    def operator(action, *arguments):
        result = q.dc("run", "--rm", "--no-deps", "-T", "operator", action,
                      "--workspace", workspace, *arguments)
        receipt = json.loads(result.stdout)
        require(isinstance(receipt, dict) and receipt.get("workspace_id") == workspace,
                "Operator CLI returned an invalid scoped receipt")
        return receipt

    def status():
        result = operator("operator-consumer-status")
        require(set(result) == {"workspace_id", "head_seq", "purged_through_seq", "registrations",
                                "dead_letters", "failed_jobs", "oldest_pending_at", "event_admission"}
                and set(result["event_admission"]) == {
                    "max_pending_events", "max_retained_event_bytes", "retained_event_bytes",
                    "remaining_event_bytes", "minimum_event_bytes", "pending_events", "admission_blocked",
                    "admission_reason", "accounting"}
                and result["event_admission"]["accounting"] == "logical_v1",
                "Operator CLI status did not preserve its redacted logical-accounting contract")
        for collection, fields in (
                ("registrations", {"consumer_name", "consumer_version", "required", "enabled", "last_seq", "backfill_job_id", "lag"}),
                ("dead_letters", {"id", "seq", "event_type", "schema_version", "attempts", "state", "error_code", "created_at", "available_at"}),
                ("failed_jobs", {"id", "state", "error_code", "retry_count", "created_at"})):
            require(isinstance(result[collection], list)
                    and all(isinstance(row, dict) and set(row) == fields for row in result[collection]),
                    "Operator CLI status exposed fields outside its redacted maintenance contract")
        return result

    def metrics():
        result = json.loads(q.sql(f"""SELECT jsonb_build_object(
          'head',h.last_seq,'bytes',h.retained_event_bytes,'sum',coalesce(sum(e.storage_bytes),0),
          'exact',coalesce(bool_and(e.storage_bytes=192+octet_length(e.event_type)::bigint+octet_length(e.actor_kind)::bigint),true),
          'count',count(e.seq),'aged',count(e.seq) FILTER(WHERE e.occurred_at<clock_timestamp()-interval '7 days'),
          'events',coalesce(jsonb_agg(jsonb_build_object('id',e.event_id,'seq',e.seq,'bytes',e.storage_bytes) ORDER BY e.seq) FILTER(WHERE e.seq IS NOT NULL),'[]'),
          'floor',(SELECT purged_through_seq FROM event_retention_state WHERE workspace_id=h.workspace_id),
          'checkpoint',(SELECT last_seq FROM consumer_checkpoints WHERE workspace_id=h.workspace_id AND consumer_name='assignment_cleanup' AND consumer_version=1),
          'backfill',(SELECT backfill_job_id IS NOT NULL FROM consumer_checkpoints WHERE workspace_id=h.workspace_id AND consumer_name='assignment_cleanup' AND consumer_version=1),
          'jobs',(SELECT count(*) FROM jobs WHERE workspace_id=h.workspace_id AND kind='assignment.reconcile' AND state<>'succeeded'))
          FROM workspace_stream_heads h LEFT JOIN workspace_events e USING(workspace_id)
          WHERE h.workspace_id='{workspace}' GROUP BY h.workspace_id"""))
        require(result["exact"] and result["bytes"] == result["sum"],
                "Retained logical charges differ from the exact durable event total")
        return result

    def snapshot():
        tables = ("teams", "issues", "issue_labels", "issue_key_aliases", "documents",
                  "document_revisions", "document_mentions", "mention_intents", "discussion_sources",
                  "issue_subscriptions", "subscription_events", "issue_template_sources",
                  "idempotency_records", "activity_entries", "audit_entries", "workspace_events",
                  "workspace_stream_heads", "workspace_auth_guards", "consumer_operations")
        fields = [f"'workspace',(SELECT to_jsonb(t) FROM workspaces t WHERE id='{workspace}')"]
        fields += [f"'{table}',(SELECT coalesce(jsonb_agg(to_jsonb(t) ORDER BY to_jsonb(t)::text),'[]') FROM {table} t WHERE workspace_id='{workspace}')"
                   for table in tables]
        return q.sql("SELECT jsonb_build_object(" + ",".join(fields) + ")")

    def limits(pending, byte_cap, reason):
        before = status()["event_admission"]
        receipt = operator("operator-event-limits", "--max-pending-events", str(pending),
                           "--max-event-bytes", str(byte_cap), "--reason", reason)
        correlation = receipt.get("correlation_id", "")
        require(isinstance(correlation, str) and re.fullmatch(uuid_pattern, correlation)
                and receipt == {"workspace_id": workspace, "target_id": None,
                                "action": "event_limits", "correlation_id": correlation},
                "Limit CLI did not return a canonical correlation receipt")
        expected = {"before": {"max_pending_events": before["max_pending_events"],
                               "max_retained_event_bytes": before["max_retained_event_bytes"]},
                    "after": {"max_pending_events": str(pending), "max_retained_event_bytes": str(byte_cap)},
                    "reason": reason}
        audit = json.loads(q.sql(f"""SELECT jsonb_build_object('metadata',a.metadata,
          'valid',a.id=a.correlation_id AND a.actor_kind='local_operator' AND a.operator_identity='opentrack_operator'
          AND a.action='events.admission_limits_changed' AND a.target_id=a.workspace_id
          AND o.action=a.action AND o.affected_count=1 AND o.through_seq=h.last_seq
          AND o.reason=a.metadata->>'reason' AND o.operator_identity=a.operator_identity)
          FROM audit_entries a JOIN consumer_operations o ON o.workspace_id=a.workspace_id AND o.id=a.id
          JOIN workspace_stream_heads h ON h.workspace_id=a.workspace_id
          WHERE a.workspace_id='{workspace}' AND a.id='{correlation}'"""))
        require(audit == {"metadata": expected, "valid": True},
                "Reasoned limits did not commit matching scoped audit and operation records")
        current = status()["event_admission"]
        require(current["max_pending_events"] == str(pending)
                and current["max_retained_event_bytes"] == str(byte_cap),
                "Operator CLI limits did not persist")

    def enabled(value, reason):
        action = "enable" if value else "disable"
        operator("operator-consumer-" + action, "--reason", reason)
        registration = next(item for item in status()["registrations"]
                            if item["consumer_name"] == "assignment_cleanup" and item["consumer_version"] == 1)
        require(registration["enabled"] is value and registration["required"],
                "Operator CLI did not preserve the required consumer's enabled state")

    def once():
        q.dc("run", "--rm", "--no-deps", "-T", "worker", "worker", "--once", timeout=45)

    def drain():
        for _ in range(20):
            once()
            result = metrics()
            if result["checkpoint"] == result["head"] and result["backfill"] and result["jobs"] == 0:
                return result
        raise RuntimeError("Bounded actual worker polls did not drain the fixture")

    def age(events):
        require(events and len(events) <= 100
                and all(re.fullmatch(uuid_pattern, item["id"]) for item in events),
                "Only a bounded snapshot of canonical fixture events may be aged")
        identifiers = ",".join("'" + item["id"] + "'::uuid" for item in events)
        q.sql(f"""BEGIN; DO $$ DECLARE changed bigint; BEGIN
          IF NOT worker_lock_maintenance_context('{workspace}') THEN RAISE EXCEPTION 'Fixture workspace unavailable';END IF;
          UPDATE workspace_events SET occurred_at=clock_timestamp()-interval '9 days'
          WHERE workspace_id='{workspace}' AND event_id IN({identifiers});
          GET DIAGNOSTICS changed=ROW_COUNT;
          IF changed<>{len(events)} THEN RAISE EXCEPTION 'Fixture event snapshot changed';END IF;
          END $$; COMMIT;""")

    def unavailable(path, method, body, headers):
        problem, received = q.request(path, method, body, headers=headers, expected=(503,))
        if isinstance(problem, bytes):
            problem = json.loads(problem)
        request_id = problem.get("request_id", "")
        require(isinstance(request_id, str) and str(UUID(request_id)) == request_id,
                "Admission failure lacks a canonical request identifier")
        require(problem == {"type": "/api/v1/problems/dependency_unavailable", "title": "Service Unavailable",
                            "status": 503, "detail": "Service temporarily unavailable", "instance": "urn:uuid:" + request_id,
                            "code": "dependency_unavailable", "request_id": request_id, "retryable": True, "field_errors": []}
                and received.get("Content-Type") == "application/problem+json"
                and received.get("Cache-Control") == "no-store" and received.get("X-Request-ID") == request_id
                and all(received.get(name) is None for name in
                        ("ETag", "Location", "Set-Cookie", "X-OpenTrack-Session-Invalidated")),
                "Admission failure did not use the complete safe generic retry envelope")

    def candidate(label):
        return ("event-runtime-" + uuid4().hex,
                {"team_id": team, "title": "Event runtime " + label, "mutation_id": str(uuid4())})

    def blocked(key, body):
        before = snapshot()
        unavailable(base + "/issues", "POST", body, {"Idempotency-Key": key})
        unavailable(base, "PATCH", {"name": "Rejected event runtime update"}, {"If-Match": workspace_headers["ETag"]})
        require(snapshot() == before,
                "503 changed an entity, issue counter, document, history, receipt, authorization epoch, sequence or charge")

    def recover(key, body):
        created, _ = q.request(base + "/issues", "POST", body, headers={"Idempotency-Key": key}, expected=(201,))
        committed = snapshot()
        replay, _ = q.request(base + "/issues", "POST", body, headers={"Idempotency-Key": key}, expected=(200, 201))
        require(created == replay and snapshot() == committed,
                "Same-key recovery/replay duplicated or changed committed effects")
        metrics()

    original = status()
    required = [item for item in original["registrations"] if item["required"]]
    require(len(required) == 1 and required[0]["consumer_name"] == "assignment_cleanup"
            and required[0]["consumer_version"] == 1,
            "Qualification requires the actual assignment consumer without synthetic registrations")
    original_enabled = required[0]["enabled"]
    original_pending = int(original["event_admission"]["max_pending_events"])
    original_bytes = int(original["event_admission"]["max_retained_event_bytes"])
    pending_cap = max(original_pending, 100000)
    byte_cap = max(original_bytes, metrics()["bytes"] + 1048576)
    q.record("actual Compose operator provisioning, grants, private output initialization and redacted CLI status")

    with _stopped_worker(q):
        failure = None
        try:
            stopped = json.loads(q.dc("ps", "--all", "--format", "json", "worker").stdout)
            if isinstance(stopped, list):
                require(len(stopped) == 1, "Expected exactly one owned worker container")
                stopped = stopped[0]
            require(stopped.get("State") == "exited" and stopped.get("ExitCode") == 0,
                    "Owned worker did not stop cleanly before event qualification")
            enabled(True, "Enable actual runtime qualification capture")
            drain()
            limits(1, byte_cap, "Qualify pending event admission")
            seed_key, seed_body = candidate("pending seed")
            recover(seed_key, seed_body)
            admission = status()["event_admission"]
            require(admission["pending_events"] == "1" and admission["admission_blocked"]
                    and admission["admission_reason"] == "pending_event_limit",
                    "One committed event did not fill the pending cap")
            key, body = candidate("pending retry")
            blocked(key, body)
            retained = metrics()["bytes"]
            require(drain()["bytes"] == retained, "Capture changed recent retained event charges")
            recover(key, body)
            q.record("pending cap returns safe HTTP503 with complete rollback and same-key recovery/replay")

            retained = metrics()["bytes"]
            limits(pending_cap, retained, "Qualify logical retained byte admission")
            admission = status()["event_admission"]
            require(admission["admission_blocked"] and admission["admission_reason"] == "retained_event_byte_limit",
                    "Exact retained-byte cap did not report admission refusal")
            key, body = candidate("byte retry")
            blocked(key, body)
            limits(pending_cap, byte_cap, "Restore runtime byte capacity")
            recover(key, body)
            q.record("logical byte cap returns safe HTTP503 with complete rollback and same-key recovery/replay")

            captured = drain()
            age(captured["events"])
            require(metrics()["bytes"] == captured["bytes"], "Fixture aging altered logical charges")
            once()
            reclaimed = metrics()
            require(reclaimed["count"] == 0 and reclaimed["bytes"] == 0
                    and reclaimed["floor"] == captured["head"],
                    "Actual bounded retention did not reclaim exactly the captured fixture prefix")
            enabled(False, "Pause drained actual runtime consumer")
            limits(1, byte_cap, "Keep one disabled required event")
            seed_key, seed_body = candidate("disabled backlog")
            recover(seed_key, seed_body)
            retained = metrics()
            require(retained["count"] == 1 and retained["head"] == retained["checkpoint"] + 1,
                    "Disabled required fixture did not leave one uncaptured event")
            age(retained["events"])
            pinned = metrics()
            require(pinned["aged"] == 1, "Fixture event did not exceed the replay window")
            once()
            require(metrics() == pinned, "Disabled required capture or retention discarded uncaptured work")
            key, body = candidate("disabled retry")
            blocked(key, body)
            limits(1, pinned["bytes"], "Require exact retained byte reclamation")
            enabled(True, "Resume actual required runtime capture")
            once()
            reclaimed = metrics()
            event = pinned["events"][0]
            require(reclaimed["checkpoint"] == pinned["head"] and reclaimed["floor"] == pinned["head"]
                    and reclaimed["count"] == 0 and reclaimed["bytes"] == 0,
                    "Re-enabled actual worker did not capture and reclaim the exact aged charge")
            require(q.sql(f"SELECT count(*)=1 FROM consumer_receipts WHERE workspace_id='{workspace}' AND event_id='{event['id']}' AND seq={event['seq']} AND state='ignored' AND job_id IS NULL") == "t",
                    "Actual assignment consumer did not persist its independent capture receipt")
            recover(key, body)
            recent = drain()
            require(recent["count"] == 1 and recent["bytes"] == pinned["bytes"]
                    and recent["floor"] == pinned["head"],
                    "Recent recovered event lost its replay window or exact logical charge")
            q.record("bounded actual worker drain; disabled required event survives nine-day aging; re-enable captures and reclaims exact bytes before same-key retry")
        except BaseException as error:
            failure = error
        for cleanup in (
                lambda: limits(original_pending, original_bytes, "Restore original runtime admission limits"),
                lambda: enabled(True, "Restore runtime capture for cleanup"),
                drain,
                lambda: enabled(original_enabled, "Restore original runtime consumer state")):
            try:
                cleanup()
            except BaseException as error:
                if failure is None:
                    failure = error
                else:
                    failure.add_note("Additional event qualification cleanup failed; inspect the disposable project.")
        if failure is not None:
            raise failure
    require(bool(q.dc("ps", "--status", "running", "--quiet", "worker").stdout.strip()),
            "Owned worker did not resume after qualification")
    q.record("original event limits and required-consumer state restored; owned worker restarted")
