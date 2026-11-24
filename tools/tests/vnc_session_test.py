"""Synthetic VNC transport and operator lifecycle; never contacts a VM or SSH host."""
import importlib.util
import io
import json
import os
from pathlib import Path
import shutil
import shlex
import socket
import subprocess
import sys
import tempfile
import time
import types
import unittest
from unittest import mock
import plistlib
import struct
import threading
import zlib

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("vnc_session", ROOT / "tools/vm/vnc-session.py")
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)
inspect_spec = importlib.util.spec_from_file_location("posture_inspect", ROOT / "tools/vm/sim-posture-inspect.py")
inspection = importlib.util.module_from_spec(inspect_spec)
inspect_spec.loader.exec_module(inspection)


class CaptureTests(unittest.TestCase):
    def test_rfb_loss_exits_while_fifo_idle_and_intentional_quit_calls_base(self):
        # Twisted calls the protocol instance, not the synchronous API proxy.
        # A separate process proves loss unblocks even an idle FIFO open.
        script = '''
import importlib.util, pathlib, sys, threading, types
class LibraryClient:
    def connectionLost(self, reason): print('base-loss', flush=True)
library = types.ModuleType('vncdotool.client')
library.VNCDoToolClient = LibraryClient
library.VNCDoToolFactory = type('Factory', (), {})
package = types.ModuleType('vncdotool')
package.api = types.SimpleNamespace()
sys.modules.update({'vncdotool': package, 'vncdotool.client': library})
spec = importlib.util.spec_from_file_location('holder', sys.argv[1])
holder = importlib.util.module_from_spec(spec)
spec.loader.exec_module(holder)
protocol = holder.ResizingVNCClient()
mode = sys.argv[4]
class Proxy:
    factory = types.SimpleNamespace(force_caps=False)
    def refreshScreen(self):
        if mode == 'idle':
            threading.Timer(.1, lambda: protocol.connectionLost('lost')).start()
    def disconnect(self): protocol.connectionLost('intentional')
package.api.connect = lambda *a, **k: Proxy()
package.api.shutdown = lambda: print('shutdown')
sys.argv = ['holder', sys.argv[2], sys.argv[3], sys.argv[4]]
holder.main()
'''
        with tempfile.TemporaryDirectory() as directory:
            fifo, pid = Path(directory) / "commands", Path(directory) / "holder.pid"
            os.mkfifo(fifo)
            for mode in ("idle", "quit"):
                if mode == "quit":
                    fifo.unlink()
                    fifo.write_text("quit\n")
                result = subprocess.run([sys.executable, "-c", script,
                    str(ROOT / "tools/vm/vnc-holder.py"), str(fifo), str(pid), mode],
                    env=dict(os.environ, DUO_FOLD_VNC_URL="vnc://:synthetic@127.0.0.1:12345"),
                    capture_output=True, timeout=3)
                self.assertEqual(result.returncode, 1 if mode == "idle" else 0, result.stderr.decode())
                self.assertIn(b"base-loss\n", result.stdout, result.stderr.decode())
                self.assertEqual(b"disconnected\n" in result.stdout, mode == "idle")
                self.assertEqual(b"shutdown\n" in result.stdout, mode == "quit")

    def test_capture_resyncs_request_bounds_and_preserves_library_call(self):
        calls = []
        result = object()

        class LibraryClient:
            def captureScreen(self, *args, **kwargs):
                # vncdotool's capture requests use protocol width/height, not
                # the current Pillow screen size. A full-sized PNG alone
                # therefore does not prove that its full area was refreshed.
                calls.append((self.width, self.height, args, kwargs))
                return result

        library = types.ModuleType("vncdotool.client")
        library.VNCDoToolClient = LibraryClient
        library.VNCDoToolFactory = type("LibraryFactory", (), {})
        package = types.ModuleType("vncdotool")
        package.api = mock.Mock()
        holder_spec = importlib.util.spec_from_file_location("vnc_holder", ROOT / "tools/vm/vnc-holder.py")
        holder = importlib.util.module_from_spec(holder_spec)
        with mock.patch.dict(sys.modules, {"vncdotool": package, "vncdotool.client": library}):
            holder_spec.loader.exec_module(holder)

        client = holder.ResizingVNCFactory.protocol()
        client.width, client.height = 1280, 720
        client.screen = None
        self.assertIs(client.captureScreen("boot.png"), result)
        self.assertEqual(calls[-1], (1280, 720, ("boot.png",), {}))
        for size in ((3840, 2400), (1280, 720), (3840, 2400)):
            client.screen = types.SimpleNamespace(size=size)
            with self.subTest(size=size):
                self.assertIs(client.captureScreen("frame.png", True), result)
                self.assertEqual(calls[-1], (*size, ("frame.png", True), {}))
                self.assertIs(client.captureScreen("frame.png", incremental=False), result)
                self.assertEqual(calls[-1], (*size, ("frame.png",), {"incremental": False}))

        # The existing command still calls the library with just its filename.
        holder.run(client, "shot final.png")
        self.assertEqual(calls[-1], (3840, 2400, ("final.png",), {}))
        with tempfile.TemporaryDirectory() as directory:
            fifo, pid = Path(directory) / "commands", Path(directory) / "holder.pid"
            fifo.write_text("quit\n")
            with mock.patch.object(sys, "argv", ["vnc-holder.py", str(fifo), str(pid)]), \
                    mock.patch.dict(os.environ, {"DUO_FOLD_VNC_URL": "vnc://:test-only@127.0.0.1:12345"}), \
                    mock.patch("builtins.print"):
                holder.main()
            package.api.connect.assert_called_once_with(
                "127.0.0.1::12345", password="test-only", factory_class=holder.ResizingVNCFactory)
            package.api.connect.return_value.disconnect.assert_called_once_with()


class SessionTests(unittest.TestCase):
    def setUp(self):
        # /var/folders on macOS can itself exhaust sockaddr_un's small limit.
        self.tmp = tempfile.TemporaryDirectory(dir="/tmp", prefix="vnc-")
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.state = self.root / "build/vm-fold"
        self.state.mkdir(parents=True)
        self.bin = self.root / "bin"
        self.bin.mkdir()
        self.lease = self.root / "scheduler/mac-vm.guest.lock"
        self.env = dict(os.environ, FAKE_ROOT=str(self.root),
                        MEGURI_MAC_VM_CONFIG=str(self.root / "config.json"),
                        MEGURI_HOST_LEASE_DIR=str(self.lease.parent / "host.lock"),
                        MEGURI_SIM_POOL_DIR=str(self.root / "pool"),
                        MEGURI_MAC_VM_LEASE_WAIT_SECONDS="0", MEGURI_MAC_VM_BOOT_SECONDS="0",
                        PATH=str(self.bin) + os.pathsep + os.environ["PATH"])
        for name in ("tools/vm/duo-fold.sh", "tools/vm/vnc-session.py",
                     "tools/scripts/mac-vm-lib.sh", "tools/scripts/host-lease-mutex.sh"):
            dest = self.root / name
            dest.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy(ROOT / name, dest)
        self.script("git", "print(__import__('os').environ['FAKE_ROOT'] + '/.git')\n")
        self.script("tart", '''import json, os, pathlib, sys, time
root = pathlib.Path(os.environ['FAKE_ROOT'])
assert (root / 'scheduler/mac-vm.guest.lock/holder.json').exists()
with (root / 'trace').open('a') as out: out.write(' '.join(sys.argv[1:]) + '\\n')
state = root / 'vm-state'
if sys.argv[1] == 'list':
    if (root / 'unknown-state').exists(): sys.exit(255)
    if state.read_text() == 'stopped' and (root / 'unknown-after-stop').exists():
        (root / 'unknown-after-stop').unlink()
        sys.exit(255)
    print(json.dumps([{'Name': 'guest', 'State': state.read_text()}]))
elif sys.argv[1] == 'stop':
    assert (root / 'pool/pool.lock/pid').exists(), 'shutdown without legacy pool mutex'
    if (root / 'stop-uncertain').exists(): sys.exit(255)
    if (root / 'stop-failed').exists(): sys.exit(1)
    state.write_text('stopped')
elif sys.argv[1] == 'run':
    state.write_text('running')
    (root / 'vm-mode').write_text('vnc' if '--vnc-experimental' in sys.argv else 'headless')
    if '--vnc-experimental' in sys.argv:
        if (root / 'lost-start').exists(): sys.exit(255)
        print('vnc://:fake-test-secret@' + ('0.0.0.0:54321' if (root / 'bad-uri').exists() else '127.0.0.1:54321'), flush=True)
        while state.read_text() != 'stopped': time.sleep(.01)
        if (root / 'lost-run').exists(): sys.exit(255)
''')
        self.script("vm-ssh", '''import os, pathlib, subprocess, sys
root = pathlib.Path(os.environ['FAKE_ROOT'])
assert (root / 'scheduler/mac-vm.guest.lock/holder.json').exists()
cmd = sys.argv[1]
if 'pgrep' in cmd:
    if not (root / 'busy-tests').exists(): print('idle')
elif 'stat -f' in cmd:
    if not ((root / 'no-guest').exists() and (root / 'vm-mode').read_text() == 'vnc'): print('guest-user')
else:
    sys.exit(subprocess.call(['/bin/sh', '-c', cmd]))
''')
        tunnel = self.script("ssh", '''import json, os, pathlib, socket, sys, time
root = pathlib.Path(os.environ['FAKE_ROOT'])
(root / 'tunnel-args').write_text(json.dumps(sys.argv[1:]))
if (root / 'bad-tunnel').exists(): sys.exit(255)
forward = sys.argv[sys.argv.index('-L') + 1].split(':')
s = socket.socket()
s.bind((forward[0], int(forward[1])))
s.listen()
(root / 'listener').write_text(json.dumps({'pid': os.getpid(), 'port': int(forward[1])}))
while True: time.sleep(.1)
''')
        # Linux/Docker does not provide macOS /usr/sbin/lsof. Substitute just
        # that OS observation, leaving the production parser and Popen checks.
        lsof = self.script("lsof", '''import json, os, pathlib, sys
root = pathlib.Path(os.environ['FAKE_ROOT'])
try: listener = json.loads((root / 'listener').read_text())
except FileNotFoundError: sys.exit(1)
if str(listener['pid']) != sys.argv[sys.argv.index('-p') + 1]: sys.exit(1)
if '-iTCP:' + str(listener['port']) not in sys.argv: sys.exit(1)
print('p' + str(listener['pid']))
print('n127.0.0.1:' + str(listener['port']))
''')
        session = self.root / "tools/vm/vnc-session.py"
        source = session.read_text()

        def replace_once(old, new):
            nonlocal source
            self.assertEqual(source.count(old), 1, "fixture source anchor missing or ambiguous: " + repr(old))
            source = source.replace(old, new, 1)

        replace_once('            "/usr/bin/ssh", "-N",',
                     '            ' + repr(str(tunnel)) + ', "-N",')
        replace_once('            "/usr/sbin/lsof", "-nP",',
                     '            ' + repr(str(lsof)) + ', "-nP",')
        # Fixture-only authenticated exit joins Popen-owned synthetic children
        # even when a test deliberately makes Tart return SSH's unknown code.
        # Match complete handler lines, not the CLI's separate stop dispatch.
        replace_once('''
                    if action == "stop":
                        join_transports(children, remote)
''', '''
                    if action == "fixture-stop":
                        join_transports(children, False)
                        done = True
                        result = {"ok": True}
                    elif action == "stop":
                        join_transports(children, remote)
''')
        session.write_text(source)
        compile(session.read_bytes(), str(session), "exec")
        holder = self.root / "tools/vm/vnc-holder.py"
        holder.write_text('''import os, pathlib, sys, time
root = pathlib.Path(os.environ['FAKE_ROOT'])
if (root / 'bad-holder').exists(): sys.exit(1)
(root / 'holder-url').write_text(os.environ['DUO_FOLD_VNC_URL'])
pathlib.Path(sys.argv[2]).write_text(str(os.getpid()))
print('connected', flush=True)
while True:
    with open(sys.argv[1]) as commands:
        for line in commands: pass
''')
        venv = self.state / "venv/bin"
        venv.mkdir(parents=True)
        self.script("holder-python", "import os, sys\nif '-c' not in sys.argv:\n    os.execv(sys.executable, [sys.executable, *sys.argv[1:]])\n")
        (venv / "python").symlink_to(self.bin / "holder-python")
        (self.root / "config.json").write_text(json.dumps({"tart": str(self.bin / "tart"),
            "ssh": str(self.bin / "vm-ssh"), "vms": ["guest"]}))
        (self.bin / "remote.json").write_text(json.dumps({"host": "fake-m3-alias", "runtime": "/private-kit"}))
        (self.root / "vm-state").write_text("running")
        self.addCleanup(self.kill_children)

    def script(self, name, body):
        path = self.bin / name
        path.write_text("#!" + sys.executable + "\n" + body)
        path.chmod(0o700)
        return path

    def call(self, command, expected=0, *args):
        result = subprocess.run(["bash", str(self.root / "tools/vm/duo-fold.sh"), command, *args],
                                env=self.env, capture_output=True, timeout=15)
        self.assertEqual(result.returncode, expected, result.stderr.decode())
        self.assertNotIn(b"fake-test-secret", result.stdout + result.stderr)
        return result

    def kill_children(self):
        # Synthetic-only cleanup: permit the owned Tart child to end normally.
        (self.root / 'vm-state').write_text('stopped')
        try:
            m.request(self.state, "fixture-stop", timeout=35)
        except (OSError, RuntimeError):
            pass

    def trace(self):
        return (self.root / "trace").read_text() if (self.root / "trace").exists() else ""

    def test_secondary_lifecycle_refused_before_touching_global_session(self):
        config = self.root / "config.json"
        data = json.loads(config.read_text())
        data["vms"].append("secondary")
        config.write_text(json.dumps(data))
        self.call("prepare")
        before = self.trace()
        owner = (self.state / "owner").read_bytes()
        for command in ("prepare", "start", "stop"):
            result = self.call(command, 1, "--vm", "secondary")
            self.assertIn(b"primary VM", result.stderr)
        self.assertEqual(self.trace(), before)
        self.assertEqual((self.state / "owner").read_bytes(), owner)
        self.assertFalse((self.lease.parent / "mac-vm.secondary.lock").exists())
        self.call("stop")
        # Also refuse earlier secondary state selected under the primary name.
        (self.state / "vm").write_text("secondary")
        before = self.trace()
        self.call("prepare", 1)
        self.call("stop", 1)
        self.assertEqual(self.trace(), before)

    def test_scheduler_metadata_failure_releases_only_own_lease(self):
        jq = shutil.which("jq")
        self.script("jq", '''import os, sys
if '--arg' in sys.argv and 'vm-fold-prepare' in sys.argv: sys.exit(1)
os.execv(''' + repr(jq) + ''', ['jq', *sys.argv[1:]])
''')
        self.call("prepare", 1)
        self.assertFalse(self.lease.exists())
        self.assertEqual(self.trace(), "")
        self.lease.mkdir(parents=True)
        (self.lease / "holder.json").write_text('{"pid": 1, "job": "foreign"}')
        before = (self.lease / "holder.json").read_bytes()
        self.call("prepare", 3)
        self.assertEqual((self.lease / "holder.json").read_bytes(), before)

    def test_control_socket_preflight_failure_never_stops_guest(self):
        # Existing socket evidence is refused without overwriting it.
        (self.state / "control.sock").write_text("foreign")
        self.call("prepare", 1)
        self.assertEqual(self.trace(), "")
        self.assertEqual((self.state / "control.sock").read_text(), "foreign")
        (self.state / "control.sock").unlink()
        # Keep the path in Linux's accepted range but over Darwin's 103 bytes.
        long_root = self.root / ("x" * (104 - len(os.fsencode(self.state / "control.sock")) - 1))
        long_state = long_root / "build/vm-fold"
        long_state.mkdir(parents=True)
        (long_state / "venv").symlink_to(self.state / "venv")
        self.script("git", "print(" + repr(str(long_root / ".git")) + ")\n")
        result = self.call("prepare", 1)
        self.assertIn(b"exceeds macOS limit", result.stderr)
        self.assertEqual(self.trace(), "")
        self.assertFalse((long_state / "owner").exists())

    def test_completed_shutdown_state_query_failure_still_restores(self):
        (self.root / "unknown-after-stop").touch()
        result = self.call("prepare", 1)
        self.assertIn(b"command completed but stopped state unconfirmed", result.stderr)
        self.assertIn(b"restored headless", result.stderr)
        self.assertEqual((self.root / "vm-state").read_text(), "running")
        self.assertFalse((self.state / "recovery-pending").exists())
        self.assertFalse((self.state / "shutdown-pending").exists())
        self.assertFalse(self.lease.exists())

    def test_unknown_and_failed_shutdown_exits_have_distinct_recovery(self):
        (self.root / "stop-uncertain").touch()
        result = self.call("prepare", 1)
        self.assertIn(b"remote shutdown completion unknown", result.stderr)
        self.assertIn(b"cannot be resolved by rerunning the wrapper", result.stderr)
        self.assertTrue((self.state / "shutdown-pending").exists())
        self.assertTrue((self.state / "recovery-pending").exists())
        self.assertNotIn("run --no-graphics", self.trace())
        (self.root / "stop-uncertain").unlink()
        (self.state / "shutdown-pending").unlink()  # Synthetic unknown command has joined.
        (self.state / "recovery-pending").unlink()
        (self.root / "stop-failed").touch()
        result = self.call("prepare", 1)
        self.assertIn(b"command exited unsuccessfully", result.stderr)
        self.assertFalse((self.state / "shutdown-pending").exists())
        self.assertNotIn("run --no-graphics", self.trace())

    def test_remote_prepares_once_private_forward_status_and_stop(self):
        # Exercise the actual MG integration without MG's host/container runner.
        result = subprocess.run(["bash", "-c", '''
mg_job() { :; }
mg_detail() { printf '%s\\n' "$2"; }
mg_message() { :; }
mg_failure() { echo failure >&2; }
mg_status() { [ "$1" = pass ]; }
source ''' + shlex.quote(str(ROOT / "tools/mg.d/sim.sh")) + '''
MG_TOOLS=''' + shlex.quote(str(self.root / "tools")) + '''
mg_sim_posture prepare
'''], env=self.env, capture_output=True, timeout=15)
        self.assertEqual(result.returncode, 0, result.stderr.decode())
        self.assertTrue(json.loads(result.stdout)["connected"])
        self.assertNotIn(b"fake-test-secret", result.stdout + result.stderr)
        before = self.trace()
        holder = (self.state / "holder.pid").read_text()
        self.call("prepare")
        self.call("start")
        self.assertEqual(before, self.trace())
        self.assertEqual(holder, (self.state / "holder.pid").read_text())
        self.assertTrue(json.loads(self.call("status").stdout)["connected"])
        args = json.loads((self.root / "tunnel-args").read_text())
        self.assertEqual(args[-2:], ["--", "fake-m3-alias"])
        for option in ("BatchMode=yes", "StrictHostKeyChecking=yes", "ExitOnForwardFailure=yes"):
            self.assertIn(option, args)
        forward = args[args.index("-L") + 1]
        self.assertRegex(forward, r"^127\.0\.0\.1:[0-9]+:127\.0\.0\.1:54321$")
        self.assertNotIn("fake-test-secret", " ".join(args))
        self.assertIn("@127.0.0.1:" + forward.split(":")[1], (self.root / "holder-url").read_text())
        self.assertEqual(self.state.stat().st_mode & 0o777, 0o700)
        self.call("stop")
        self.assertIn("run --no-graphics guest", self.trace())
        self.assertFalse(json.loads(self.call("status").stdout)["connected"])
        self.assertFalse(self.lease.exists())

    def test_restart_failures_restore_headless_without_secret_output(self):
        for failure in ("bad-uri", "bad-tunnel", "bad-holder", "no-guest"):
            with self.subTest(failure=failure):
                (self.root / failure).touch()
                result = self.call("prepare", 1)
                self.assertIn(b"restored headless", result.stderr)
                self.assertEqual((self.root / "vm-state").read_text(), "running")
                self.assertIn("run --no-graphics guest", self.trace())
                self.assertFalse((self.state / "vm").exists())
                self.assertFalse(self.lease.exists())
                (self.root / failure).unlink()
                (self.root / "tunnel-args").unlink(missing_ok=True)

    def test_missing_venv_fails_before_restart(self):
        (self.state / "venv/bin/python").unlink()
        result = self.call("prepare", 1)
        self.assertIn(b"holder venv", result.stderr)
        self.assertEqual(self.trace(), "")

    def test_busy_scheduler_guest_tests_and_unknown_inventory_do_not_stop(self):
        self.lease.mkdir(parents=True)
        self.call("prepare", 3)
        self.assertTrue(self.lease.exists())
        self.lease.rmdir()
        (self.root / "busy-tests").touch()
        self.call("prepare", 1)
        (self.root / "busy-tests").unlink()
        lease = self.root / "pool/leases/orphan/lease.json"
        lease.parent.mkdir(parents=True)
        for data in ({"expires_epoch": time.time() + 300}, {}, {"expires_epoch": "unknown"}):
            lease.write_text(json.dumps(data))
            self.call("prepare", 1)
            self.assertEqual(json.loads(lease.read_text()), data)
        self.assertEqual(self.trace(), "")
        lease.write_text(json.dumps({"expires_epoch": time.time() - 1}))
        self.call("prepare")
        self.assertTrue(lease.exists())
        # Stop may inspect VM state, but must drain before changing the holder or VM.
        lease.write_text(json.dumps({"expires_epoch": time.time() + 300}))
        active_lease = lease.read_bytes()
        before = self.trace()
        holder_pid = int((self.state / "holder.pid").read_text())
        self.call("stop", 1)
        self.assertEqual(self.trace(), before + "list --format json\n")
        self.assertEqual(int((self.state / "holder.pid").read_text()), holder_pid)
        self.assertTrue(m.status(self.state, 'guest')['connected'])
        self.assertEqual((self.root / "vm-state").read_text(), "running")
        self.assertEqual(lease.read_bytes(), active_lease)

    def test_partial_session_does_not_restart_and_local_manual_still_works(self):
        (self.state / "vm").write_text("guest")
        self.call("prepare", 1)
        self.assertEqual(self.trace(), "")
        (self.state / "vm").unlink()
        (self.bin / "remote.json").unlink()
        self.call("start")
        self.assertFalse((self.root / "tunnel-args").exists())
        self.assertTrue(json.loads(self.call("status").stdout)["connected"])
        self.call("stop")

    def test_uri_validation_rejects_unauthenticated_or_nonloopback_addresses(self):
        self.assertEqual(m.address("noise\nvnc://:test-secret@127.0.0.1:54321\n"), ("test-secret", 54321))
        for uri in ("vnc://127.0.0.1:1", "vnc://:@127.0.0.1:1", "vnc://:x@0.0.0.0:1",
                    "vnc://:x@localhost:1", "vnc://:x@127.0.0.1:65536", "vnc://:x@127.0.0.1:0"):
            with self.assertRaises(ValueError):
                m.address(uri)

    def test_inspect_holds_scheduler_releases_on_success_and_failure_without_lifecycle(self):
        helper = self.root / "tools/vm/sim-posture-inspect.py"
        helper.write_text('''import json, os, pathlib, sys
root = pathlib.Path(os.environ['FAKE_ROOT'])
assert (root / 'scheduler/mac-vm.guest.lock/holder.json').exists()
assert sys.argv[1:] == [str(root / 'build/vm-fold'), str(root / 'bin/vm-ssh'), 'guest', str(root), 'leased-udid', str(root / 'bin/tart')]
if (root / 'inspect-fail').exists(): sys.exit(1)
print(json.dumps({'ready': True}))
''')
        for expected in (0, 1):
            if expected:
                (self.root / "inspect-fail").touch()
            result = subprocess.run(["bash", str(self.root / "tools/vm/duo-fold.sh"), "inspect", "leased-udid"],
                                    env=self.env, capture_output=True, timeout=15)
            self.assertEqual(result.returncode, expected, result.stderr.decode())
            self.assertFalse(self.lease.exists())
            self.assertEqual(self.trace(), "")

    def test_pid_files_never_authorize_status_or_stop(self):
        # A recycled PID points at an unrelated live process: never signal it.
        child = subprocess.Popen([sys.executable, '-c', 'import time; time.sleep(60)'])
        try:
            (self.state / 'vm').write_text('guest')
            for name in ('tart', 'holder', 'tunnel'):
                (self.state / (name + '.pid')).write_text(str(child.pid))
            (self.state / 'holder.log').write_text('connected\n')
            before = {p.name: p.read_bytes() for p in self.state.iterdir() if p.is_file()}
            self.assertFalse(m.status(self.state, 'guest')['connected'])
            self.call('prepare', 1)
            self.call('stop', 1)
            self.assertIsNone(child.poll())
            self.assertEqual(self.trace(), '')
            self.assertEqual(before, {p.name: p.read_bytes() for p in self.state.iterdir() if p.is_file()})
        finally:
            child.terminate()
            child.wait()

    def test_owned_teardown_ignores_replaced_pid_file_and_joins(self):
        self.call('prepare')
        child = subprocess.Popen([sys.executable, '-c', 'import time; time.sleep(60)'])
        try:
            (self.state / 'holder.pid').write_text(str(child.pid))
            self.call('stop')
            self.assertIsNone(child.poll())
            self.assertFalse((self.state / 'owner').exists())
        finally:
            child.terminate()
            child.wait()

    def test_uncertain_shutdown_preserves_owner_and_no_headless_start(self):
        self.call('prepare')
        before = self.trace()
        (self.root / 'stop-uncertain').touch()
        self.call('stop', 1)
        self.assertTrue((self.state / 'owner').exists())
        self.assertTrue((self.state / 'vm').exists())
        self.assertTrue(m.status(self.state, 'guest')['connected'])
        self.assertNotIn('run --no-graphics guest', self.trace()[len(before):])
        self.assertFalse((self.root / 'pool/pool.lock').exists())
        (self.root / 'stop-uncertain').unlink()
        self.call('stop', 1)  # List/running alone cannot resolve lost stop completion.
        self.assertTrue((self.state / 'shutdown-pending').exists())

    def test_lost_remote_run_retains_evidence_without_headless(self):
        self.call('prepare')
        before = self.trace()
        (self.root / 'lost-run').touch()
        self.call('stop', 1)
        self.assertTrue((self.state / 'owner').exists())
        self.assertTrue((self.state / 'tart.pid').exists())
        self.assertNotIn('run --no-graphics guest', self.trace()[len(before):])
        self.assertFalse(self.lease.exists())

    def test_lost_remote_start_retains_evidence_without_headless(self):
        (self.root / 'lost-start').touch()
        self.call('prepare', 1)
        self.assertTrue((self.state / 'owner').exists())
        self.assertTrue((self.state / 'vm').exists())
        self.assertTrue((self.state / 'recovery-pending').exists())
        self.assertNotIn('run --no-graphics guest', self.trace())
        self.assertEqual(self.trace().count('--vnc-experimental'), 1)

    def test_unknown_remote_state_does_not_touch_transport(self):
        self.call('prepare')
        before = self.trace()
        (self.root / 'unknown-state').touch()
        self.call('stop', 1)
        self.assertNotIn('stop guest', self.trace()[len(before):])
        self.assertTrue(m.status(self.state, 'guest')['connected'])
        (self.root / 'unknown-state').unlink()
        self.call('stop')


class TransportTests(unittest.TestCase):
    def test_ssh_listener_requires_exact_owned_loopback_os_evidence(self):
        child = mock.Mock(pid=123)
        child.poll.return_value = None
        with mock.patch.object(socket, "socket") as probe:
            for output, expected in ((b"p123\nn127.0.0.1:2345\n", True),
                                     (b"p456\nn127.0.0.1:2345\n", False),
                                     (b"p123\nn*:2345\n", False),
                                     (b"p123\nn127.0.0.1:2346\n", False)):
                with mock.patch.object(subprocess, "check_output", return_value=output) as lsof:
                    self.assertEqual(m.ssh_listening(child, 2345), expected)
                    args = lsof.call_args.args[0]
                    self.assertEqual(args[:6], ["/usr/sbin/lsof", "-nP", "-a", "-p", "123", "-iTCP:2345"])
                    self.assertIn("-sTCP:LISTEN", args)
            with mock.patch.object(subprocess, "check_output", side_effect=subprocess.CalledProcessError(1, "lsof")):
                self.assertFalse(m.ssh_listening(child, 2345))
            child.poll.return_value = 255
            with self.assertRaises(RuntimeError):
                m.ssh_listening(child, 2345)
            probe.assert_not_called()

    def test_preflight_enforces_darwin_path_limit_on_linux_too(self):
        with tempfile.TemporaryDirectory(dir="/tmp", prefix="vnc-") as directory:
            root = Path(directory)
            for length in (103, 104):
                state = root / ("x" * (length - len(os.fsencode(root / "control.sock")) - 1))
                state.mkdir()
                self.assertEqual(len(os.fsencode(state / "control.sock")), length)
                if length == 103:
                    m.preflight(state)
                    self.assertEqual(list(state.iterdir()), [])
                else:
                    with self.assertRaisesRegex(RuntimeError, "macOS limit"):
                        m.preflight(state)
                    self.assertEqual(list(state.iterdir()), [])

    def test_prechild_bind_and_listen_failures_clean_only_owned_records(self):
        with tempfile.TemporaryDirectory(dir="/tmp", prefix="vnc-") as directory:
            state = Path(directory)
            path = state / "control.sock"
            path.write_text("foreign socket evidence")
            with mock.patch.object(m, "connect") as connect:
                with self.assertRaises(OSError):
                    m.serve(state, "unused", "guest")
                connect.assert_not_called()
            self.assertEqual(path.read_text(), "foreign socket evidence")
            self.assertEqual({p.name for p in state.iterdir()}, {"control.sock"})
            path.unlink()
            for replace in (False, True):
                with socket.socket(socket.AF_UNIX) as real:
                    listener = mock.Mock(wraps=real)
                    def fail_listen(*args):
                        if replace:
                            # Keep the original inode alive while replacing it.
                            for name in ("owner", "control.sock"):
                                (state / name).rename(state / (name + ".retained"))
                                (state / name).write_text("replacement")
                        raise OSError("synthetic listen failure")
                    listener.listen.side_effect = fail_listen
                    with mock.patch.object(socket, "socket", return_value=listener), \
                            mock.patch.object(m, "connect") as connect:
                        with self.assertRaises(OSError):
                            m.serve(state, "unused", "guest")
                        connect.assert_not_called()
                self.assertFalse((state / "owner.pid").exists())
                if replace:
                    self.assertEqual(path.read_text(), "replacement")
                    self.assertEqual((state / "owner").read_text(), "replacement")
                else:
                    self.assertEqual(list(state.iterdir()), [])

    def test_ready_fails_after_transport_exit_but_status_reports_disconnect(self):
        with tempfile.TemporaryDirectory(dir="/tmp", prefix="vnc-") as directory:
            state = Path(directory)
            child = mock.Mock()
            child.poll.return_value = None
            connected = threading.Event()
            def connect(state, children, *args):
                children['holder'] = child
                connected.set()
            with mock.patch.object(m, "remote_host", return_value="m3"), \
                    mock.patch.object(m, "connect", side_effect=connect), \
                    mock.patch.object(m, "join_transports"):
                thread = threading.Thread(target=m.serve, args=(state, "unused", "guest"), daemon=True)
                thread.start()
                try:
                    self.assertTrue(connected.wait(3))
                    self.assertTrue(m.request(state, "ready")["connected"])
                    child.poll.return_value = 1
                    self.assertFalse(m.request(state, "status")["connected"])
                    with self.assertRaises(RuntimeError):
                        m.request(state, "ready")
                finally:
                    m.request(state, "stop")
                    thread.join(timeout=3)
                self.assertFalse(thread.is_alive())

    def test_mg_post_prepare_disconnect_fails_but_readonly_status_passes(self):
        with tempfile.TemporaryDirectory() as directory:
            tools = Path(directory)
            (tools / "vm").mkdir()
            script = tools / "vm/duo-fold.sh"
            script.write_text('#!/bin/sh\n[ "$1" != status ] || echo \'{"connected":false}\'\nexit 0\n')
            script.chmod(0o700)
            for action, expected in (("prepare", "fail"), ("status", "pass")):
                result = subprocess.run(["bash", "-c", '''
mg_job() { :; }
mg_detail() { :; }
mg_message() { :; }
mg_failure() { :; }
mg_status() { echo "$1"; }
source ''' + shlex.quote(str(ROOT / "tools/mg.d/sim.sh")) + '''
MG_TOOLS=''' + shlex.quote(directory) + '''
mg_sim_posture ''' + action], capture_output=True, timeout=3)
                self.assertEqual(result.returncode, 0, result.stderr.decode())
                self.assertEqual(result.stdout.decode().strip(), expected)


class PreviousSessionTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.state = Path(self.tmp.name) / "private state"
        self.state.mkdir(mode=0o700)
        self.tart = self.state.parent / "tart"
        (self.state.parent / "remote.json").write_text(json.dumps({"host": "m3", "runtime": "/private kit"}))
        for name, content in {"vm": "guest", "holder.pid": "101", "tunnel.pid": "102",
                              "tart.pid": "103", "holder.log": "connected\n",
                              "tart.log": "vnc://:secret@127.0.0.1:54321\n"}.items():
            (self.state / name).write_text(content)
        os.mkfifo(self.state / "commands", 0o600)
        self.commands = {
            "101": f"/venv/bin/python -I /previous checkout/tools/vm/vnc-holder.py {self.state}/commands {self.state}/holder.pid",
            "102": "/usr/bin/ssh -N -T -o BatchMode=yes -o StrictHostKeyChecking=yes -o ExitOnForwardFailure=yes -L 127.0.0.1:54322:127.0.0.1:54321 -- m3",
            "103": "/usr/bin/ssh -T -o BatchMode=yes -o StrictHostKeyChecking=yes -- m3 if [ guard ]; then exit 255; fi; exec '/private kit/bin/tart' run --no-graphics --vnc-experimental guest",
        }
        self.sockets = {"101": b"p101\nn127.0.0.1:54323->127.0.0.1:54322\n",
                        "102": b"p102\nn127.0.0.1:54322\n"}
        self.output = mock.patch.object(subprocess, "check_output", side_effect=self.os_output).start()
        self.addCleanup(mock.patch.stopall)

    def os_output(self, args, **kwargs):
        self.assertEqual(kwargs["timeout"], 2)
        self.assertIn(args[0], ("/bin/ps", "/usr/sbin/lsof"))
        pid = args[args.index("-p") + 1]
        if args[0] == "/usr/sbin/lsof":
            return self.sockets[pid]
        return f"{os.getuid()} S Tue Nov 24 12:00:00 2026 {self.commands[pid]}\n".encode()

    def snapshot(self):
        return {p.name: (p.lstat().st_ino, p.lstat().st_mode, p.read_bytes() if p.is_file() else None)
                for p in self.state.iterdir()}

    def test_previous_format_status_and_inspect_preserve_session_without_lifecycle(self):
        before = self.snapshot()
        with mock.patch.object(os, "kill") as kill, mock.patch.object(m, "start") as start:
            self.assertTrue(m.status(self.state, "guest", self.tart)["connected"])
            for operation in (m.stop, m.owned):
                with self.assertRaises(RuntimeError):
                    operation(self.state)
            # Exercise the actual inspector's previous-format admission and
            # revalidation; guest transport/capture stay synthetic and private.
            def capture(state, connected):
                self.assertEqual(state, self.state)
                self.assertTrue(connected())
                return Path("/private/synthetic.png")
            with mock.patch.object(sys, "argv", ["inspect", str(self.state), "fake-ssh", "guest", str(ROOT),
                                                "11111111-2222-3333-4444-555555555555", str(self.tart)]), \
                    mock.patch.object(subprocess, "run", return_value=subprocess.CompletedProcess([], 0, b"ready", b"")) as guest, \
                    mock.patch.object(inspection, "capture", side_effect=capture), mock.patch("builtins.print"):
                self.assertEqual(inspection.main(), 0)
                guest.assert_called_once()
                self.assertEqual(guest.call_args.args[0][0], "fake-ssh")
            kill.assert_not_called()
            start.assert_not_called()
        self.assertEqual(before, self.snapshot())
        self.assertFalse((self.state / "owner").exists())

    def test_previous_format_rejects_unlinked_target_process_or_socket(self):
        for pid, original in tuple(self.commands.items()):
            for changed in ("/usr/bin/sleep 60", original.replace("guest", "foreign").replace("-- m3", "-- other")
                            .replace("/commands", "/foreign-commands")):
                with self.subTest(pid=pid, command=changed):
                    self.commands[pid] = changed
                    self.assertFalse(m.status(self.state, "guest", self.tart)["connected"])
            self.commands[pid] = original
        for pid, original in tuple(self.sockets.items()):
            self.sockets[pid] = b""
            self.assertFalse(m.status(self.state, "guest", self.tart)["connected"])
            self.sockets[pid] = original
        self.assertFalse(m.status(self.state, "foreign", self.tart)["connected"])
        self.assertFalse(m.status(self.state, "guest")["connected"])
        with mock.patch.object(subprocess, "check_output", side_effect=subprocess.TimeoutExpired("ps", 2)):
            self.assertFalse(m.status(self.state, "guest", self.tart)["connected"])

    def test_partial_public_symlinked_and_broken_owner_state_stays_conservative(self):
        self.state.chmod(0o755)
        self.assertFalse(m.status(self.state, "guest", self.tart)["connected"])
        self.state.chmod(0o700)
        (self.state / "holder.pid").unlink()
        (self.state / "holder.pid").symlink_to(self.state / "tart.pid")
        self.assertFalse(m.status(self.state, "guest", self.tart)["connected"])
        (self.state / "holder.pid").unlink()
        (self.state / "holder.pid").write_text("101")
        (self.state / "owner").write_text("unavailable-owner")
        self.output.reset_mock()
        self.assertFalse(m.status(self.state, "guest", self.tart)["connected"])
        self.output.assert_not_called()  # Never downgrade a broken new owner.


class InspectionTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.udid = "11111111-2222-3333-4444-555555555555"
        self.pin = json.loads((ROOT / "tools/sim-devices.json").read_text())["foldable"]["runtime"]
        self.request = dict(udid=self.udid, checkout="/task checkout", vm="guest", pin=self.pin,
                            pool=str(self.root / "pool"))
        self.lease = dict(udid=self.udid, kind="foldable", worktree="/task checkout", vm="guest",
                          expires_epoch=time.time() + 300)
        self.lease_path = self.root / "pool/leases" / self.udid / "lease.json"
        self.lease_path.parent.mkdir(parents=True)
        self.developer = self.root / "Selected Xcode.app/Contents/Developer"
        self.developer.mkdir(parents=True)
        self.hub = self.developer.parent / "Applications/DeviceHub.app"
        (self.hub / "Contents").mkdir(parents=True)
        (self.hub / "Contents/Info.plist").write_bytes(plistlib.dumps({"CFBundleExecutable": "DevicesTrampoline", "CFBundleIdentifier": "com.apple.dt.Devices"}))
        self.device = dict(udid=self.udid, name="Meguri-Pool-foldable-1", state="Booted",
                           isAvailable=True, deviceTypeIdentifier="duo-type")
        self.inventory = {self.pin["identifier"]: [self.device]}
        self.runtime = dict(identifier=self.pin["identifier"], version=self.pin["version"],
                            buildversion=self.pin["build"], isAvailable=True)
        self.device_type = dict(identifier="duo-type", name="iPhone Duo", productFamily="iPhone")
        self.calls = []
        self.opened = False

    def guest(self):
        self.lease_path.write_text(json.dumps(self.lease))
        original_stat = Path.stat
        def path_stat(path, *args, **kwargs):
            if str(path) == "/dev/console":
                return type("Console", (), {"st_uid": 501})()
            return original_stat(path, *args, **kwargs)
        def output(args, **kwargs):
            self.calls.append(args)
            if args[0] == "/usr/bin/xcode-select":
                return str(self.developer).encode()
            if args[0] == "/usr/bin/codesign":
                return b""
            if args[0] == "/usr/bin/open":
                self.opened = True
                return b""
            self.assertEqual(args[0], str(self.developer / "usr/bin/simctl"))
            self.assertEqual(args[1], "list")  # Any boot/reset/input command fails the test.
            if getattr(self, "failed_inventory", None) == args[2]:
                raise subprocess.CalledProcessError(1, args, stderr=b"private native label")
            inventories = {"devices": self.inventory, "runtimes": [self.runtime],
                           "devicetypes": [self.device_type]}
            return json.dumps({args[2]: inventories[args[2]]}).encode()
        with mock.patch.object(sys, "argv", ["guest", json.dumps(self.request)]), \
                mock.patch.object(os, "getuid", return_value=501), \
                mock.patch.object(Path, "stat", path_stat), \
                mock.patch.object(subprocess, "check_output", side_effect=output), \
                mock.patch("builtins.print") as self.guest_output:
            exec(inspection.GUEST, {})

    def test_guest_exact_selected_signed_hub_uid_and_udid(self):
        self.guest()
        self.assertIn(("/usr/bin/codesign", "--verify", "--strict", str(self.hub)), self.calls)
        self.assertIn(("/usr/bin/open", "-a", str(self.hub), "--args", "-CurrentDeviceUDID", self.udid), self.calls)
        self.assertEqual(json.loads(self.lease_path.read_text()), self.lease)

    def test_guest_refuses_invalid_lease_before_launch(self):
        for key, value in (("worktree", "/foreign"), ("vm", "foreign"), ("kind", "iphone"),
                           ("udid", "foreign"), ("expires_epoch", time.time() - 1),
                           ("expires_epoch", "unknown"), ("expires_epoch", float("nan"))):
            with self.subTest(key=key, value=value):
                original = self.lease[key]
                self.lease[key] = value
                with self.assertRaises(RuntimeError):
                    self.guest()
                self.assertFalse(self.opened)
                self.lease[key] = original

    def test_guest_refuses_nonbooted_wrong_type_runtime_duplicate_name(self):
        for target, key, value in ((self.device, "state", "Shutdown"), (self.device, "isAvailable", False),
                                   (self.device, "name", "Personal Duo"), (self.runtime, "buildversion", "wrong"),
                                   (self.device_type, "name", "iPhone 18 Pro")):
            with self.subTest(key=key):
                original = target[key]
                target[key] = value
                with self.assertRaises(RuntimeError):
                    self.guest()
                self.assertFalse(self.opened)
                target[key] = original
        self.inventory["foreign-runtime"] = [dict(self.device, udid="other")]
        with self.assertRaises(RuntimeError):
            self.guest()
        self.assertFalse(self.opened)

    def test_guest_refuses_wrong_bundle_identifier(self):
        for identifier, executable in (("foreign", "DevicesTrampoline"), ("com.apple.dt.Devices", "foreign")):
            (self.hub / "Contents/Info.plist").write_bytes(plistlib.dumps({
                "CFBundleIdentifier": identifier, "CFBundleExecutable": executable}))
            with self.assertRaises(RuntimeError):
                self.guest()
            self.assertFalse(self.opened)

    def test_guest_reports_fixed_simctl_failure_stage(self):
        for inventory in ("devices", "runtimes", "devicetypes"):
            with self.subTest(inventory=inventory):
                self.failed_inventory = inventory
                with self.assertRaises(RuntimeError):
                    self.guest()
                self.guest_output.assert_called_once_with("inspect-simctl-" + inventory, flush=True)
                self.assertFalse(self.opened)

    def test_capture_existing_holder_waits_for_complete_private_png(self):
        state = self.root / "state"
        state.mkdir()
        os.mkfifo(state / "commands")
        # Keep a synthetic reader present before the inspector's nonblocking open.
        fifo_fd = os.open(state / "commands", os.O_RDWR | os.O_NONBLOCK)
        self.addCleanup(os.close, fifo_fd)
        chunk = lambda name, body: struct.pack(">I", len(body)) + name + body + struct.pack(">I", zlib.crc32(name + body))
        png = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", 1, 1, 8, 2, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(b"\0\0\0\0")) + chunk(b"IEND", b"")
        def holder():
            with (state / "commands").open() as commands:
                command = commands.readline().strip()
            self.assertTrue(command.startswith("shot "))
            path = Path(command[5:])
            path.write_bytes(png[:-12])
            self.assertFalse(inspection.complete_png(path))
            path.write_bytes(png)
        thread = threading.Thread(target=holder, daemon=True)
        thread.start()
        with mock.patch.object(tempfile, "gettempdir", return_value=str(self.root)):
            path = inspection.capture(state, lambda: True)
        thread.join(timeout=2)
        self.assertFalse(thread.is_alive())
        self.assertTrue(inspection.complete_png(path))
        self.assertEqual(path.stat().st_mode & 0o777, 0o600)
        self.assertEqual(path.parent.stat().st_mode & 0o777, 0o700)

    def test_capture_refuses_checkout_output(self):
        (self.root / ".git").mkdir()
        with mock.patch.object(tempfile, "gettempdir", return_value=str(self.root)):
            with self.assertRaises(RuntimeError):
                inspection.capture(self.root, lambda: True)


class DrainAndJoinTests(unittest.TestCase):
    def test_shutdown_completion_markers_distinguish_lost_exit_from_completed_command(self):
        for code in (0, 1, 255, -15, "timeout"):
            with self.subTest(code=code), tempfile.TemporaryDirectory() as directory:
                state = Path(directory)
                guard = mock.Mock()
                guard.stdout = io.BytesIO(b"drained\n")
                guard.poll.return_value = None
                result = subprocess.CompletedProcess([], code)
                failure = subprocess.TimeoutExpired("synthetic", 75) if code == "timeout" else None
                with mock.patch.object(m, "clean_drain"), \
                        mock.patch.object(subprocess, "Popen", return_value=guard), \
                        mock.patch.object(m.select, "select", return_value=([guard.stdout], [], [])), \
                        mock.patch.object(subprocess, "run", return_value=result, side_effect=failure):
                    if code == 0:
                        m.shutdown(state, "fake-ssh", "fake-tart", "guest")
                    else:
                        message = "exited unsuccessfully" if code == 1 else "completion unknown"
                        with self.assertRaisesRegex(RuntimeError, message):
                            m.shutdown(state, "fake-ssh", "fake-tart", "guest")
                self.assertTrue((state / "recovery-pending").exists())
                self.assertEqual((state / "shutdown-pending").exists(), code in (255, -15, "timeout"))
                guard.stdin.close.assert_called_once()
                guard.wait.assert_called_once_with(timeout=5)
                guard.terminate.assert_not_called()

    def test_cleanup_after_hard_stop_matches_only_owned_guard(self):
        with tempfile.TemporaryDirectory() as directory:
            pool = Path(directory)
            lock = pool / 'pool.lock'
            lock.mkdir()
            (lock / 'pid').write_text('123')
            (lock / 'vnc-drain-owner').write_text('our-guard')
            env = dict(os.environ, MEGURI_SIM_POOL_DIR=directory)
            for token in ('foreign-guard', 'our-guard'):
                subprocess.run([sys.executable, '-c', m.CLEAN_DRAIN, token],
                               env=env, check=True, timeout=3)
                self.assertEqual(lock.exists(), token == 'foreign-guard')

    def test_guard_signal_cleans_lock(self):
        with tempfile.TemporaryDirectory() as directory:
            child = subprocess.Popen([sys.executable, '-c', m.DRAIN, 'test-guard'],
                                     env=dict(os.environ, MEGURI_SIM_POOL_DIR=directory),
                                     stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                     stderr=subprocess.DEVNULL)
            try:
                self.assertEqual(child.stdout.readline(), b'drained\n')
                child.terminate()
                child.wait(timeout=3)
                self.assertFalse((Path(directory) / 'pool.lock').exists())
            finally:
                child.stdin.close()
                child.stdout.close()
                if child.poll() is None:
                    child.kill()
                    child.wait()

    def test_rejected_inventory_cleans_only_its_guard(self):
        for data in ({'expires_epoch': 10**20}, {}, {'expires_epoch': 'unknown'}):
            with self.subTest(data=data), tempfile.TemporaryDirectory() as directory:
                pool = Path(directory)
                lease = pool / 'leases/orphan/lease.json'
                lease.parent.mkdir(parents=True)
                lease.write_text(json.dumps(data))
                result = subprocess.run([sys.executable, '-c', m.DRAIN, 'test-guard'],
                                        env=dict(os.environ, MEGURI_SIM_POOL_DIR=directory),
                                        capture_output=True, timeout=3)
                self.assertNotEqual(result.returncode, 0)
                self.assertFalse((pool / 'pool.lock').exists())
                self.assertEqual(json.loads(lease.read_text()), data)

    def test_final_drain_excludes_legacy_renew_and_eof_cleans_lock(self):
        with tempfile.TemporaryDirectory() as directory:
            pool = Path(directory)
            lease = pool / 'leases/expired/lease.json'
            lease.parent.mkdir(parents=True)
            lease.write_text('{"expires_epoch": 0}')
            child = subprocess.Popen([sys.executable, '-c', m.DRAIN, 'test-guard'],
                                     env=dict(os.environ, MEGURI_SIM_POOL_DIR=directory),
                                     stdin=subprocess.PIPE, stdout=subprocess.PIPE)
            try:
                self.assertEqual(child.stdout.readline(), b'drained\n')
                self.assertEqual((pool / 'pool.lock/pid').read_text(), str(child.pid))
                # This is the exact mkdir mutex used by legacy renew clients.
                with self.assertRaises(FileExistsError):
                    (pool / 'pool.lock').mkdir()
                self.assertEqual(lease.read_text(), '{"expires_epoch": 0}')
            finally:
                child.stdin.close()
                child.wait(timeout=3)
                child.stdout.close()
            self.assertFalse((pool / 'pool.lock').exists())

    def test_remote_run_must_join_and_confirm_command_exit(self):
        for code in (255, -15):
            with self.subTest(code=code):
                children = {name: mock.Mock() for name in ('holder', 'tunnel', 'tart')}
                children['tart'].wait.return_value = code
                with self.assertRaises(RuntimeError):
                    m.join_transports(children, True)
                for name in ('holder', 'tunnel'):
                    children[name].terminate.assert_called_once()
                    children[name].wait.assert_called_once_with(timeout=10)
                children['tart'].terminate.assert_not_called()
        child = mock.Mock()
        child.wait.side_effect = subprocess.TimeoutExpired('synthetic', 10)
        with self.assertRaises(subprocess.TimeoutExpired):
            m.join_transports({'tart': child}, True)
        child.terminate.assert_not_called()


if __name__ == "__main__":
    unittest.main()
