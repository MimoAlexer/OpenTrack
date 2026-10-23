"""Bound Linux qualification commands and clean their owned process sessions."""
import os
from pathlib import Path
import signal
import subprocess
import time


def _stat(pid):
    try:
        value = Path(f"/proc/{pid}/stat").read_text()
        return value[value.rfind(")") + 2:].split()
    except (OSError, ProcessLookupError):
        return None


def _live_session(session, started):
    leader = _stat(session)
    if started is not None and leader is not None and leader[19] != started:
        return {}
    members = {}
    for entry in Path("/proc").iterdir():
        if entry.name.isdigit():
            state = _stat(entry.name)
            if state and state[3] == str(session) and state[0] not in {"Z", "X"}:
                members[int(entry.name)] = state[19]
    return members


def _signal_members(session, members, operation):
    for pid, started in members.items():
        try:
            descriptor = os.pidfd_open(pid)
        except ProcessLookupError:
            continue
        try:
            current = _stat(pid)
            if current and current[3] == str(session) and current[19] == started:
                signal.pidfd_send_signal(descriptor, operation)
        except ProcessLookupError:
            pass
        finally:
            os.close(descriptor)


def _stop(process, started):
    for operation, grace in [(signal.SIGTERM, 1), (signal.SIGKILL, 3)]:
        _signal_members(process.pid, _live_session(process.pid, started), operation)
        deadline = time.monotonic() + grace
        while (members := _live_session(process.pid, started)) and time.monotonic() < deadline:
            process.poll()
            if operation == signal.SIGKILL:
                _signal_members(process.pid, members, operation)
            time.sleep(0.01)
    if _live_session(process.pid, started):
        raise SystemExit("Owned qualification processes did not stop; no acceptance recorded")
    try:
        process.wait(timeout=3)
    except subprocess.TimeoutExpired:
        raise SystemExit("Owned qualification process was not reaped; no acceptance recorded") from None


def run_owned(arguments, *, timeout, env=None, capture_output=False, text=False):
    """Preserve command output while cleaning descendants before returning or raising."""
    process = None
    started = None
    pending_signal = None
    cleaning = False

    def interrupted(number, _frame):
        nonlocal pending_signal
        pending_signal = number
        if process is not None and not cleaning:
            raise SystemExit(128 + number)

    previous = {number: signal.getsignal(number) for number in (signal.SIGTERM, signal.SIGINT)}
    for number in previous:
        signal.signal(number, interrupted)
    try:
        process = subprocess.Popen(arguments, env=env, start_new_session=True, text=text,
                                   stdout=subprocess.PIPE if capture_output else None,
                                   stderr=subprocess.PIPE if capture_output else None)
        state = _stat(process.pid)
        started = state[19] if state else None
        if pending_signal is not None:
            raise SystemExit(128 + pending_signal)
        deadline = time.monotonic() + timeout
        while True:
            remaining = deadline - time.monotonic()
            try:
                stdout, stderr = process.communicate(timeout=max(0, min(0.1, remaining)))
                break
            except subprocess.TimeoutExpired as error:
                if time.monotonic() >= deadline:
                    raise subprocess.TimeoutExpired(arguments, timeout, output=error.output,
                                                    stderr=error.stderr) from None
                if process.poll() is not None:
                    _stop(process, started)
                    stdout, stderr = process.communicate(timeout=3)
                    break
        result = subprocess.CompletedProcess(arguments, process.returncode, stdout, stderr)
    finally:
        cleaning = True
        try:
            if process is not None:
                _stop(process, started)
                for stream in (process.stdout, process.stderr):
                    if stream is not None:
                        stream.close()
        finally:
            for number, handler in previous.items():
                signal.signal(number, handler)
    if pending_signal is not None:
        raise SystemExit(128 + pending_signal)
    return result
