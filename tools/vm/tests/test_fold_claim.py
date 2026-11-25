"""Synthetic guest claim loss/reboot; never reaches a host, VM or VNC server."""
import importlib.util
import json
import os
from pathlib import Path
import signal
import socket
import subprocess
import sys
import tempfile
import time
import types
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[3]
spec = importlib.util.spec_from_file_location("fold_claim", ROOT / "tools/vm/fold-claim.py")
boundary = importlib.util.module_from_spec(spec)
spec.loader.exec_module(boundary)


class FoldClaimTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.lease = self.root / "lease"
        self.lease.mkdir()
        (self.lease / "holder.json").write_text(json.dumps(dict(pid=os.getpid())))
        self.state = Path(str(self.lease) + ".claim")
        self.lock = self.root / "guest.lock"
        self.ssh = self.root / "ssh"
        # The synthetic transport owns/joins its Perl child. A reboot marker
        # ends that session and prevents reconnection until the test allows it.
        self.ssh.write_text("#!" + sys.executable + "\n" + '''
import os, pathlib, subprocess, sys, time
root = pathlib.Path(__file__).parent
if (root / 'reboot').exists(): sys.exit(255)
child = subprocess.Popen(['/bin/sh', '-c', sys.argv[1]])
while child.poll() is None:
    if (root / 'reboot').exists():
        child.terminate()
        child.wait(timeout=3)
        sys.exit(255)
    time.sleep(.01)
sys.exit(child.returncode)
''')
        self.ssh.chmod(0o700)
        self.old_path = os.environ.get("MEGURI_MAC_VM_CLAIM_PATH")
        os.environ["MEGURI_MAC_VM_CLAIM_PATH"] = str(self.lock)
        self.addCleanup(self.restore_environment)
        self.keeper = subprocess.Popen([sys.executable, str(ROOT / "tools/scripts/vm-claim.py"),
            "--ssh", str(self.ssh), "--lease", str(self.lease), "--state", str(self.state),
            "--token", "test-fold", "--holder", '{"job":"fold","run_id":"synthetic"}',
            "--path", str(self.lock), "--beat", "0.05", "--answer", "0.2"],
            stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        self.addCleanup(self.stop_keeper)
        self.wait_state("taken")
        self.claim = boundary.Claim(self.lease, str(self.ssh))

    def restore_environment(self):
        if self.old_path is None:
            os.environ.pop("MEGURI_MAC_VM_CLAIM_PATH", None)
        else:
            os.environ["MEGURI_MAC_VM_CLAIM_PATH"] = self.old_path

    def stop_keeper(self):
        # Release the exact synthetic lease/fences so production cleanup joins
        # its own session. No unrelated PID or private host is involved.
        for path in self.lease.glob("*/**/*"):
            if path.is_file():
                path.unlink()
        for path in self.lease.glob("*"):
            if path.is_dir():
                path.rmdir()
            else:
                path.unlink()
        self.lease.rmdir()
        self.keeper.wait(timeout=5)

    def wait_state(self, state):
        until = time.monotonic() + 5
        while time.monotonic() < until:
            try:
                if json.loads(self.state.read_text())["state"] == state:
                    return
            except (OSError, ValueError):
                pass
            time.sleep(.01)
        self.fail("keeper never reached " + state)

    def test_fold_claim_reboot_foreign_handoff_and_admission_again(self):
        import fcntl
        self.claim.check()
        load = boundary.guarded_modules(self.claim, str(self.ssh))
        hosted = load("vnc-hosted")
        with mock.patch.object(hosted, "endpoint", return_value=("unused-tart", "guest")), \
                mock.patch.object(hosted, "rpc", return_value=dict(connected=True, session="a" * 64)):
            remote = hosted.RemoteHolder(self.root, lambda: True, mock.Mock(), self.root, self.lease)
        (self.root / "reboot").touch()
        self.wait_state("lost")
        with self.lock.open("a+") as foreign:
            fcntl.flock(foreign, fcntl.LOCK_EX)
            foreign.seek(0)
            foreign.truncate()
            foreign.write('{"job":"mac-ui","machine":"foreign"}\n')
            foreign.flush()
            (self.root / "reboot").unlink()
            with self.assertRaises(RuntimeError):
                self.claim.check()
            session = load("vnc-session")
            fence = session.ShutdownFence(self.lease, self.root, "synthetic")
            with self.assertRaises(RuntimeError):
                fence.arm()
            self.assertFalse((self.lease / "vm-shutdown-pending").exists())
            with self.assertRaises(RuntimeError):
                hosted.rpc("unused-tart", dict(op="drained-stop"))
            with mock.patch.object(hosted, "rpc") as rpc:
                self.assertEqual(remote.publish(None, b"remote-click"), 0)
                rpc.assert_not_called()
            pose = load("sim-posture")
            read_fd, write_fd = os.pipe()
            try:
                holder = pose.Holder.__new__(pose.Holder)
                self.assertEqual(holder.publish(write_fd, b"posture-click 9999999999 receipt\n"), 0)
                os.set_blocking(read_fd, False)
                with self.assertRaises(BlockingIOError):
                    os.read(read_fd, 100)
            finally:
                os.close(read_fd)
                os.close(write_fd)
        self.wait_state("taken")
        self.claim.check()

    def test_fold_claim_lease_handoff_reuses_provable_keeper_but_yields_old_keeper(self):
        pid = json.loads(self.state.read_text())["pid"]
        (self.lease / "holder.json").write_text(json.dumps(dict(pid=os.getpid(), job="next-job")))
        next_claim = boundary.Claim(self.lease, str(self.ssh))
        next_claim.check()
        record = json.loads(self.state.read_text())
        record.pop("holder")
        self.state.write_text(json.dumps(record))
        next_claim.check()  # Ownership does not depend on holder text.
        record.pop("proof")
        record.pop("acquisition")
        self.state.write_text(json.dumps(record))
        with self.assertRaisesRegex(RuntimeError, "natural release"):
            next_claim.check()
        self.assertEqual(json.loads(self.state.read_text())["pid"], pid)
        self.assertIsNone(self.keeper.poll())  # No replacement or forced release.

    def test_suspended_keeper_and_foreign_lock_with_identical_stale_text_never_admit(self):
        import fcntl
        self.claim.check()
        stale = self.state.read_bytes()
        text = self.lock.read_bytes()
        os.kill(self.keeper.pid, signal.SIGSTOP)
        try:
            # Even before disconnection, a suspended keeper cannot freshly
            # acknowledge input/shutdown authority.
            with self.assertRaises((OSError, ValueError, RuntimeError)):
                self.claim.check()
            (self.root / "reboot").touch()
            with self.lock.open("a+") as foreign:
                until = time.monotonic() + 3
                while True:
                    try:
                        fcntl.flock(foreign, fcntl.LOCK_EX | fcntl.LOCK_NB)
                        break
                    except BlockingIOError:
                        if time.monotonic() >= until:
                            self.fail("synthetic disconnected guest did not release")
                        time.sleep(.01)
                # New holder has acquired but has not replaced the old text.
                self.assertEqual(self.lock.read_bytes(), text)
                self.assertEqual(self.state.read_bytes(), stale)
                os.kill(self.keeper.pid, signal.SIGCONT)
                with self.assertRaises((OSError, ValueError, RuntimeError)):
                    self.claim.check()
                session = boundary.guarded_modules(self.claim, str(self.ssh))("vnc-session")
                with self.assertRaises((OSError, ValueError, RuntimeError)):
                    session.ShutdownFence(self.lease, self.root, "synthetic").arm()
                self.assertFalse((self.lease / "vm-shutdown-pending").exists())
                pose = boundary.guarded_modules(self.claim, str(self.ssh))("sim-posture")
                read_fd, write_fd = os.pipe()
                try:
                    holder = pose.Holder.__new__(pose.Holder)
                    self.assertEqual(holder.publish(write_fd, b"posture-click 9999999999 receipt\n"), 0)
                    os.set_blocking(read_fd, False)
                    with self.assertRaises(BlockingIOError): os.read(read_fd, 100)
                finally:
                    os.close(read_fd)
                    os.close(write_fd)
        finally:
            os.kill(self.keeper.pid, signal.SIGCONT)

    def test_proof_is_bound_to_token_acquisition_and_fresh_challenge(self):
        self.claim.check()
        record = json.loads(self.state.read_text())
        for change in (dict(token="other"), dict(acquisition="0" * 64)):
            with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as channel:
                channel.settimeout(2)
                channel.connect(record["proof"])
                request = dict(token=record["token"], acquisition=record["acquisition"], challenge="c" * 64)
                request.update(change)
                channel.sendall((json.dumps(request) + "\n").encode())
                self.assertEqual(channel.recv(1024), b"")
        with mock.patch.object(boundary.secrets, "token_hex", side_effect=["a" * 64, "b" * 64]) as nonce:
            self.claim.check()
            self.claim.check()
            self.assertEqual(nonce.call_count, 2)

    def test_fold_claim_lease_replacement_and_stale_taken_never_admit(self):
        self.claim.check()
        holder = self.lease / "holder.json"
        original = holder.read_bytes()
        holder.write_text('{"pid":1,"job":"replacement"}')
        with self.assertRaises(RuntimeError):
            self.claim.check()
        holder.write_bytes(original)
        # A stale taken record cannot hide loss of the actual guest lock.
        stale = self.state.read_bytes()
        (self.root / "reboot").touch()
        self.wait_state("lost")
        self.state.write_bytes(stale)
        with self.assertRaises(RuntimeError):
            self.claim.check()

    def test_fold_claim_retained_fence_keeps_keeper_through_cancel(self):
        guard = self.lease / "sim-posture-pending"
        guard.mkdir()
        (self.lease / "holder.json").unlink()
        time.sleep(.2)
        self.assertIsNone(self.keeper.poll())
        self.assertEqual(json.loads(self.state.read_text())["state"], "taken")


class StartBoundaryTests(unittest.TestCase):
    def test_stopped_owned_start_uses_scheduler_without_guest_proof(self):
        session = boundary.guarded_modules(mock.Mock(), "unused")("vnc-session")
        with tempfile.TemporaryDirectory() as directory:
            state = Path(directory)
            for name in ("recovery-pending", "drain-token"):
                (state / name).write_text("a" * 64)
            for script, arguments in (
                    ("vnc-session.py", ["start", str(state), "tart", "guest", "python"]),
                    ("vnc-hosted.py", ["prepare", str(state), "tart", "guest", "canonical", "start"])):
                with self.subTest(script=script):
                    claim = mock.Mock()
                    claim.check.side_effect = AssertionError("stopped guest cannot prove anything")
                    module = types.SimpleNamespace(session=session, main=mock.Mock(return_value=0))
                    spec = types.SimpleNamespace(loader=mock.Mock())
                    with mock.patch.object(boundary.importlib.util, "spec_from_file_location", return_value=spec), \
                            mock.patch.object(boundary.importlib.util, "module_from_spec", return_value=module), \
                            mock.patch.object(boundary, "stopped_reservation") as reservation, \
                            mock.patch.object(boundary.subprocess, "check_output", return_value=b'[{"Name":"guest","State":"stopped"}]'), \
                            mock.patch.object(boundary.sys, "argv", []):
                        self.assertEqual(boundary.lifecycle_start(claim, script, arguments), 0)
                        module.main.assert_called_once()
                        claim.check.assert_not_called()
                        self.assertEqual(claim.scheduler_check.call_count, 2)
                        self.assertEqual(reservation.call_count, int(script == "vnc-hosted.py"))
                        for invalid in ("running", "unknown"):
                            module.main.reset_mock()
                            boundary.subprocess.check_output.return_value = json.dumps([dict(Name="guest", State=invalid)]).encode()
                            with self.assertRaises(RuntimeError):
                                boundary.lifecycle_start(claim, script, arguments)
                            module.main.assert_not_called()
                        boundary.subprocess.check_output.return_value = b'[{"Name":"guest","State":"stopped"}]'
                        for marker in ("shutdown-pending", "drain-precommit"):
                            (state / marker).touch()
                            with self.assertRaises(RuntimeError):
                                boundary.lifecycle_start(claim, script, arguments)
                            module.main.assert_not_called()
                            (state / marker).unlink()
                        (state / "drain-token").write_text("b" * 64)
                        with self.assertRaises(RuntimeError):
                            boundary.lifecycle_start(claim, script, arguments)
                        module.main.assert_not_called()
                        (state / "drain-token").write_text("a" * 64)


if __name__ == "__main__":
    unittest.main()
