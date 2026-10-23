"""Real descendant cleanup before an operating-system lock becomes available."""
import fcntl
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time
import unittest

ROOT = Path(__file__).resolve().parent
WORKER = r'''
import json, os, signal, subprocess, sys, time
from pathlib import Path
root = Path(sys.argv[1])
signal.signal(signal.SIGTERM, signal.SIG_IGN)
print('{"reason":"compiler-artifact","executable":"synthetic"}', flush=True)
print("private synthetic diagnostic", file=sys.stderr, flush=True)
child = subprocess.Popen([sys.executable, "-c", "import signal,sys,time; from pathlib import Path; signal.signal(signal.SIGTERM,signal.SIG_IGN); Path(sys.argv[1]).touch(); time.sleep(60)", str(root / "ready")], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, process_group=0 if sys.argv[3] == "split" else None)
(root / "pids").write_text(json.dumps([os.getpid(), child.pid]))
while not (root / "ready").exists(): time.sleep(0.01)
if sys.argv[2] in {"normal", "error"}: raise SystemExit(0 if sys.argv[2] == "normal" else 7)
time.sleep(60)
'''
DRIVER = r'''
import fcntl, os, subprocess, sys
from pathlib import Path
sys.path.insert(0, sys.argv[1])
from qualification_process import run_owned
root = Path(sys.argv[2])
with (root / "lock").open("w") as lock:
    fcntl.flock(lock, fcntl.LOCK_EX)
    (root / "locked").touch()
    try:
        result = run_owned([sys.executable, str(root / "worker.py"), str(root), sys.argv[3], sys.argv[4]], timeout=2 if sys.argv[3] == "timeout" else 30, capture_output=True, text=True)
        assert result.stdout == '{"reason":"compiler-artifact","executable":"synthetic"}\n'
        assert result.stderr == "private synthetic diagnostic\n"
        if sys.argv[3] == "normal": assert result.returncode == 0
        elif sys.argv[3] == "error": assert result.returncode == 7
        else: raise AssertionError("Expected an interruption")
    except subprocess.TimeoutExpired:
        assert sys.argv[3] == "timeout"
    finally:
        fcntl.flock(lock, fcntl.LOCK_UN)
'''


def live(pid):
    try:
        value = Path(f"/proc/{pid}/stat").read_text()
        return value[value.rfind(")") + 2:].split()[0] not in {"Z", "X"}
    except FileNotFoundError:
        return False


@unittest.skipUnless(sys.platform == "linux", "Linux process sessions are required")
class OwnedProcessTests(unittest.TestCase):
    def exercise(self, mode, split):
        unrelated = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(60)"], start_new_session=True)
        driver = None
        descendants = []
        try:
            with tempfile.TemporaryDirectory(prefix="opentrack-process-qualification-") as folder:
                root = Path(folder)
                (root / "worker.py").write_text(WORKER)
                driver = subprocess.Popen([sys.executable, "-c", DRIVER, str(ROOT), str(root), mode,
                                           "split" if split else "same"],
                                          stdout=subprocess.PIPE, stderr=subprocess.PIPE, start_new_session=True)
                deadline = time.monotonic() + 5
                while not ((root / "locked").exists() and (root / "ready").exists()):
                    self.assertIsNone(driver.poll(), "Supervisor exited before descendants were ready")
                    self.assertLess(time.monotonic(), deadline, "Fixture readiness exceeded its deadline")
                    time.sleep(0.01)
                descendants = json.loads((root / "pids").read_text())
                self.assertEqual(os.getsid(descendants[1]), descendants[0])
                self.assertEqual(os.getpgid(descendants[1]), descendants[1] if split else descendants[0])
                if mode == "signal":
                    os.kill(driver.pid, signal.SIGTERM)
                deadline = time.monotonic() + 9
                with (root / "lock").open("r") as lock:
                    while True:
                        try:
                            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
                            break
                        except BlockingIOError:
                            self.assertLess(time.monotonic(), deadline, "Supervisor did not release its lock")
                            time.sleep(0.01)
                    self.assertTrue(all(not live(pid) for pid in descendants),
                                    "A compiler or linker descendant survived lock release")
                    self.assertIsNone(unrelated.poll(), "Cleanup touched an unrelated process")
                driver.communicate(timeout=5)
                self.assertEqual(driver.returncode, 143 if mode == "signal" else 0)
        finally:
            if driver is not None:
                if driver.poll() is None:
                    os.killpg(driver.pid, signal.SIGKILL)
                driver.communicate(timeout=5)
            for pid in descendants:
                if live(pid):
                    os.kill(pid, signal.SIGKILL)
            unrelated.kill()
            unrelated.wait(timeout=5)

    def test_timeout_cleans_descendants_before_lock_release(self):
        for split in (False, True):
            with self.subTest(split_group=split):
                self.exercise("timeout", split)

    def test_parent_sigterm_cleans_descendants_before_lock_release(self):
        for split in (False, True):
            with self.subTest(split_group=split):
                self.exercise("signal", split)

    def test_success_cleans_orphaned_descendants_before_lock_release(self):
        for split in (False, True):
            with self.subTest(split_group=split):
                self.exercise("normal", split)

    def test_error_cleans_orphaned_descendants_before_lock_release(self):
        for split in (False, True):
            with self.subTest(split_group=split):
                self.exercise("error", split)


if __name__ == "__main__":
    unittest.main()
