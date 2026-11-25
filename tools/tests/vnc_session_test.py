"""Synthetic VNC transport and operator lifecycle; never contacts a VM or SSH host."""
import importlib.util
import io
import json
import os
from pathlib import Path
import shutil
import shlex
import signal
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
        self.root = Path(self.tmp.name).resolve()
        self.state = self.root / "build/vm-fold"
        self.state.mkdir(parents=True, mode=0o700)
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
        # The synthetic guest and transport each own an authenticated self-exit
        # endpoint. No PID file authorizes a fixture signal or host operation.
        self.script("fixture_control.py", '''import json, os, pathlib, secrets, socket, threading
root = pathlib.Path(os.environ['FAKE_ROOT'])
def rpc(name, action):
    record = json.loads((root / ('fixture-' + name + '.json')).read_text())
    with socket.create_connection(('127.0.0.1', record['port']), timeout=2) as channel:
        channel.sendall((record['token'] + ' ' + action + '\\n').encode())
        return channel.recv(100)
def serve(name):
    listener = socket.socket()
    listener.bind(('127.0.0.1', 0))
    listener.listen()
    token = secrets.token_hex(32)
    record = root / ('fixture-' + name + '.json')
    temporary = record.with_suffix('.new')
    temporary.write_text(json.dumps(dict(port=listener.getsockname()[1], token=token)))
    temporary.replace(record)
    def run():
        while True:
            with listener.accept()[0] as channel:
                line = channel.makefile('rb').readline(256)
                if line not in [(token + ' ' + action + '\\n').encode() for action in ('ping', 'exit')]:
                    continue
                channel.sendall(b'alive\\n')
                if line == (token + ' exit\\n').encode():
                    os._exit(0)
    threading.Thread(target=run, daemon=True).start()
''')
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
    assert (root / 'scheduler/mac-vm.guest.lock/vm-shutdown-pending/intent.json').exists(), 'stop without scheduler fence'
    if (root / 'stop-guard-loss').exists():
        from fixture_control import rpc
        assert rpc('transport', 'exit') == b'alive\\n'
        assert rpc('guest', 'ping') == b'alive\\n'
        (root / 'stop-waiting').touch()
        while not (root / 'stop-release').exists(): time.sleep(.01)
        sys.exit(255)
    if (root / 'stop-uncertain').exists(): sys.exit(255)
    if (root / 'stop-failed').exists(): sys.exit(1)
    from fixture_control import rpc
    assert rpc('guest', 'exit') == b'alive\\n'
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
        self.script("vm-ssh", '''import os, pathlib, shlex, subprocess, sys
root = pathlib.Path(os.environ['FAKE_ROOT'])
assert (root / 'scheduler/mac-vm.guest.lock/holder.json').exists()
cmd = sys.argv[1]
if 'pgrep' in cmd:
    if not (root / 'busy-tests').exists(): print('idle')
elif 'stat -f' in cmd:
    if not ((root / 'no-guest').exists() and (root / 'vm-mode').read_text() == 'vnc'):
        print('guest-user')
        if (root / 'vm-mode').read_text() == 'vnc': (root / 'guest-login').touch()
else:
    if 'remove_drain(sys.argv[1])' in cmd:
        with (root / 'cleanup-attempts').open('a') as out: out.write('cleanup\\n')
        if (root / 'fail-cleanup').exists():
            print('fake-test-secret', file=sys.stderr)
            sys.exit(255)
        if (root / 'replace-recovery').exists():
            recovery = root / 'build/vm-fold/recovery-pending'
            recovery.rename(recovery.with_suffix('.retained'))
            recovery.write_text('replacement')
    if 'drain guard expired' in cmd:
        from fixture_control import serve
        serve('transport')
        words = shlex.split(cmd)
        index = words.index('-c') + 1
        words[index] = ('import sys; sys.path.insert(0, ' + repr(str(root / 'bin')) + '); '
                        "from fixture_control import serve; serve('guest')\\n" + words[index])
        cmd = shlex.join(words)
    code = subprocess.call(['/bin/sh', '-c', 'exec ' + cmd])
    if 'remove_drain(sys.argv[1])' in cmd and (root / 'lose-cleanup-response').exists():
        (root / 'lose-cleanup-response').unlink()
        sys.exit(255)
    sys.exit(code)
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
        # This suite deliberately retains coverage of the legacy M5-forwarded
        # route. New resident routing/peers are covered in test_hosted_posture.
        replace_once('def route_state(canonical, vm, tart=None):\n',
                     'def route_state(canonical, vm, tart=None):\n    tart = None\n')
        # Lose a selected authenticated status reply after guest login. Count
        # real RPCs rather than using sleeps or killing supervisor processes.
        replace_once('    return result\n', '''    root = Path(os.environ['FAKE_ROOT'])
    if action == 'status' and (root / 'guest-login').exists() and (root / 'fail-status').exists():
        counter = root / 'status-count'
        count = int(counter.read_text()) + 1 if counter.exists() else 1
        counter.write_text(str(count))
        if count == int((root / 'fail-status').read_text()):
            if (root / 'disconnected-status').exists():
                return dict(result, connected=False)
            raise OSError('synthetic lost status response')
    return result
''')
        replace_once('                (state / "drain-token").unlink()\n', '''                (state / "drain-token").unlink()
                interruption = Path(os.environ['FAKE_ROOT']) / 'interrupt-cleanup'
                if interruption.exists():
                    interruption.unlink()
                    os._exit(1)
''')
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
        self.script("holder-python", "import os, sys, pathlib\npathlib.Path(os.environ['FAKE_ROOT'], 'holder-interpreter').write_text(sys.argv[0])\nif '-c' not in sys.argv:\n    os.execv(sys.executable, [sys.executable, *sys.argv[1:]])\n")
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

    def fixture_rpc(self, name, action):
        record = json.loads((self.root / ('fixture-' + name + '.json')).read_text())
        with socket.create_connection(('127.0.0.1', record['port']), timeout=2) as channel:
            channel.sendall((record['token'] + ' ' + action + '\n').encode())
            return channel.recv(100)

    def kill_children(self):
        for name in ('guest', 'transport'):
            try:
                self.fixture_rpc(name, 'exit')
            except (OSError, ValueError):
                pass
        # Synthetic-only cleanup: permit the owned Tart child to end normally.
        (self.root / 'vm-state').write_text('stopped')
        try:
            m.request(self.state, "fixture-stop", timeout=35)
        except (OSError, RuntimeError):
            pass

    def trace(self):
        return (self.root / "trace").read_text() if (self.root / "trace").exists() else ""

    def test_disconnected_route_reuses_canonical_venv_across_commands_and_worktrees(self):
        canonical = self.state
        # Synthetic OS observations only; use the real routing/lifecycle code.
        for name in ("tart", "vm-ssh"):
            path = self.bin / name
            path.write_text(path.read_text().replace("guest", "meguri-mac"))
        config = self.root / "config.json"
        config.write_text(config.read_text().replace('"guest"', '"meguri-mac"'))
        self.lease = self.lease.with_name("mac-vm.meguri-mac.lock")
        for name, value in {"vm": "meguri-mac", "holder.pid": "101", "tunnel.pid": "102",
                            "tart.pid": "103", "holder.log": "connected\n",
                            "tart.log": "vnc://:synthetic@127.0.0.1:54321\n"}.items():
            (canonical / name).write_text(value)
        os.mkfifo(canonical / "commands", 0o600)
        ps = self.script("old-ps", '''import os, pathlib, sys
root = pathlib.Path(os.environ['FAKE_ROOT'])
if (root / 'old-live').exists(): sys.exit(2)
if sys.argv[sys.argv.index('-p') + 1] != '101': sys.exit(1)
if sys.argv[-1] == 'pid=': print('101')
else:
    state = root / 'build/vm-fold'
    print(str(os.getuid()) + ' S Tue Nov 24 12:00:00 2026 /venv/bin/python -I /old/tools/vm/vnc-holder.py ' + str(state / 'commands') + ' ' + str(state / 'holder.pid'))
''')
        lsof = self.script("old-lsof", "import sys\nsys.exit(1)\n")
        helper = self.root / "tools/vm/vnc-session.py"
        helper.write_text(helper.read_text().replace('"/bin/ps"', repr(str(ps)))
                          .replace('"/usr/sbin/lsof"', repr(str(lsof))))
        def old_snapshot():
            return {p.name: (p.lstat(), p.read_bytes() if p.is_file() else None)
                    for p in canonical.iterdir()}
        before = old_snapshot()
        self.state = canonical.with_name("vmf2")
        # A failed read-only proof neither creates the managed root nor changes
        # the roster/guest. Original legacy lifecycle remains refused.
        (self.root / 'old-live').touch()
        self.call("prepare", 1)
        self.call("stop", 1)
        self.assertFalse(self.state.exists())
        self.assertEqual(self.trace(), "")
        (self.root / 'old-live').unlink()
        self.call("prepare")
        self.assertEqual((self.root / "holder-interpreter").read_text(), str(canonical / "venv/bin/python"))
        self.assertFalse((self.state / "venv").exists())
        owner = (self.state / "owner").read_bytes()
        trace = self.trace()
        self.call("prepare")
        self.assertTrue(json.loads(self.call("status").stdout)["connected"])
        worktree = self.root / "another-worktree"
        shutil.copytree(self.root / "tools", worktree / "tools")
        for command in ("status", "prepare"):
            result = subprocess.run(["bash", str(worktree / "tools/vm/duo-fold.sh"), command],
                                    env=self.env, capture_output=True, timeout=15)
            self.assertEqual(result.returncode, 0, result.stderr.decode())
        self.assertEqual((self.state / "owner").read_bytes(), owner)
        self.assertEqual(self.trace(), trace)
        # Validate the selected managed VM with the same bounded nofollow read
        # as the untouched canonical root, before any lifecycle operation.
        managed_vm = self.state / 'vm'
        original_vm = managed_vm.read_bytes()
        managed_vm.unlink()
        managed_vm.symlink_to(canonical / 'vm')
        for command in ('prepare', 'stop', 'status'):
            self.call(command, 1)
        self.assertTrue(managed_vm.is_symlink())
        self.assertEqual(self.trace(), trace)
        managed_vm.unlink()
        managed_vm.write_bytes(original_vm)
        jq = shutil.which('jq')
        self.script('jq', '''import os, pathlib, sys
if '--arg' in sys.argv and any(job in sys.argv for job in ('vm-fold-prepare', 'vm-fold-stop')):
    path = pathlib.Path(os.environ['FAKE_ROOT'], 'build/vmf2/vm')
    path.unlink()
    path.symlink_to('/missing-vm')
os.execv(''' + repr(jq) + ''', ['jq', *sys.argv[1:]])
''')
        for command in ('prepare', 'stop'):
            self.call(command, 1)
            self.assertTrue(managed_vm.is_symlink())
            self.assertFalse(self.lease.exists())
            self.assertEqual(self.trace(), trace)
            managed_vm.unlink()
            managed_vm.write_bytes(original_vm)
        (self.bin / 'jq').unlink()
        # Both roots with live/unknown evidence refuse before input or lifecycle.
        (self.root / 'old-live').touch()
        for command, args in (("prepare", ()), ("stop", ()), ("click", ("1", "2")),
                              ("hub", ("synthetic",)), ("status", ())):
            self.call(command, 1, *args)
        self.assertEqual(self.trace(), trace)
        self.assertFalse(self.lease.exists())
        (self.root / 'old-live').unlink()
        self.call("stop")
        self.call("prepare")
        self.call("stop")
        # atime may change during reads; inode, contents, modes and mtimes don't.
        after = old_snapshot()
        self.assertEqual(before.keys(), after.keys())
        for name in before:
            for field in ("st_ino", "st_mode", "st_mtime_ns"):
                self.assertEqual(getattr(before[name][0], field), getattr(after[name][0], field))
            self.assertEqual(before[name][1], after[name][1])

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

    def test_primary_metadata_refuses_fifo_symlinks_and_oversized_without_lifecycle(self):
        path = self.state / "vm"
        target = self.root / "target-vm"
        target.write_text("guest")
        for kind in ("fifo", "symlink", "dangling", "oversized"):
            with self.subTest(kind=kind):
                if kind == "fifo":
                    os.mkfifo(path)
                elif kind in ("symlink", "dangling"):
                    path.symlink_to(target if kind == "symlink" else self.root / "missing")
                else:
                    path.write_text("guest" + " " * 65536)
                fields = lambda info: (info.st_ino, info.st_mode, info.st_size, info.st_mtime_ns)
                before = fields(path.lstat())
                for command in ("prepare", "start", "stop"):
                    self.call(command, 1)  # Bounded shell timeout catches FIFO opens.
                self.assertEqual(fields(path.lstat()), before)
                self.assertEqual(self.trace(), "")
                self.assertFalse(self.lease.exists())
                path.unlink()

    def test_primary_metadata_is_rechecked_under_start_and_stop_lease(self):
        jq = shutil.which("jq")
        self.script("jq", '''import os, pathlib, sys
if '--arg' in sys.argv and any(job in sys.argv for job in ('vm-fold-prepare', 'vm-fold-stop')):
    pathlib.Path(os.environ['FAKE_ROOT'], 'build/vm-fold/vm').symlink_to('/missing-vm')
os.execv(''' + repr(jq) + ''', ['jq', *sys.argv[1:]])
''')
        for command in ("prepare", "stop"):
            self.call(command, 1)
            self.assertTrue((self.state / "vm").is_symlink())
            self.assertFalse(self.lease.exists())
            self.assertEqual(self.trace(), "")
            (self.state / "vm").unlink()

    def test_missing_stop_wrapper_does_not_restore_a_running_guest(self):
        (self.bin / "tart").unlink()
        result = self.call("prepare", 1)
        self.assertIn(b"could not launch; no stop issued", result.stderr)
        self.assertNotIn(b"restoration", result.stderr)
        self.assertEqual((self.root / "vm-state").read_text(), "running")
        self.assertEqual(self.trace(), "")
        for name in ("recovery-pending", "shutdown-pending", "drain-token"):
            self.assertFalse((self.state / name).exists())
        self.assertFalse((self.root / "pool/pool.lock").exists())
        self.assertFalse(self.lease.exists())

    def mac_ui_scheduler_client(self, blocked, secondary=False):
        result = subprocess.run(["bash", "-c", '''
mg_job() { :; }
mg_detail() { :; }
mg_failure() { echo "$1"; }
mg_message() { :; }
mg_status() { :; }
MG_TOOLS=''' + shlex.quote(str(ROOT / "tools")) + '''
MG_ROOT=''' + shlex.quote(str(self.root / "other-worktree")) + '''
MG_JOB=mac-ui MG_SIM_RUNNER="" MG_RUN_ID=second-client
source "$MG_TOOLS/mg.d/host.sh"
source "$MG_TOOLS/mg.d/mac-vm.sh"
MG_MAC_RUNNER=vm
MG_MAC_VMS='[{"name":"guest"}]'
[ -z "$SECOND_VM" ] || MG_MAC_VMS='[{"name":"guest"},{"name":"other-guest"}]'
# No VM execution: admission is the exact boundary this regression exercises.
mg__mac_vm_boot() { echo admitted; return 1; }
mg__mac_vm_stop_orphans() { :; }
mg_mac_ui_take
'''], env=dict(self.env, MEGURI_MAC_VM_LEASE_WAIT_SECONDS="1",
              MEGURI_MAC_VM_POLL_SECONDS=".05", SECOND_VM="1" if secondary else ""),
                                capture_output=True, timeout=5)
        if blocked:
            self.assertIn(b"VM lease: waiting for the VM", result.stderr)
            self.assertIn(b"mac-vm-busy", result.stdout, result.stderr.decode())
            self.assertNotIn(b"admitted", result.stdout)
        else:
            self.assertIn(b"admitted", result.stdout, result.stderr.decode())
            self.assertNotIn(b"mac-vm-busy", result.stdout)

    def test_uncertain_stop_fences_second_worktree_mac_ui_scheduler(self):
        (self.root / "stop-uncertain").touch()
        self.call("prepare", 1)
        self.assertTrue((self.lease / "vm-shutdown-pending").is_dir())
        holder = (self.lease / "holder.json").read_bytes()
        intent = json.loads((self.lease / "vm-shutdown-pending/intent.json").read_text())
        self.assertEqual(intent["token"], (self.state / "shutdown-pending").read_text())
        self.assertEqual(intent["token"], (self.state / "drain-token").read_text())
        self.mac_ui_scheduler_client(blocked=True)
        self.mac_ui_scheduler_client(blocked=False, secondary=True)
        self.assertEqual((self.lease / "holder.json").read_bytes(), holder)

    def test_proven_no_stop_launch_and_missing_ssh_allow_second_scheduler_client(self):
        for missing in ("tart", "vm-ssh"):
            with self.subTest(missing=missing):
                wrapper = self.bin / missing
                saved = wrapper.read_bytes()
                wrapper.unlink()
                try:
                    self.call("prepare", 1)
                    self.assertFalse(self.lease.exists())
                    self.mac_ui_scheduler_client(blocked=False)
                finally:
                    wrapper.write_bytes(saved)
                    wrapper.chmod(0o700)

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
        long_state.mkdir(parents=True, mode=0o700)
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
        (self.lease / "vm-shutdown-pending/intent.json").unlink()
        (self.lease / "vm-shutdown-pending").rmdir()
        self.lease.joinpath("holder.json").unlink()
        self.lease.rmdir()
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

    def pending_poststart(self):
        (self.root / "fail-cleanup").touch()
        result = self.call("prepare", 1)
        self.assertIn(b"guest drain cleanup unconfirmed", result.stderr)
        self.assertNotIn(b"restoration", result.stderr)
        self.assertTrue(m.status(self.state, "guest")["connected"])
        self.assertFalse(self.lease.exists())
        self.assertFalse((self.state / "shutdown-pending").exists())
        self.assertFalse((self.state / "drain-precommit").exists())
        self.assertEqual((self.state / "drain-token").read_bytes(),
                         (self.state / "recovery-pending").read_bytes())
        return self.trace(), {name: (self.state / name).read_bytes() for name in
                             ("owner", "vm", "holder.pid", "drain-token", "recovery-pending")}

    def test_poststart_cleanup_retries_across_processes_without_restart_or_reconnect(self):
        before, records = self.pending_poststart()
        # Retrying uses the same authenticated owner and token binding.
        result = self.call("prepare", 1)
        self.assertIn(b"guest drain cleanup unconfirmed", result.stderr)
        self.assertNotIn(b"restoration", result.stderr)
        self.assertEqual(records, {name: (self.state / name).read_bytes() for name in records})
        self.assertTrue(m.status(self.state, "guest")["connected"])
        (self.root / "fail-cleanup").unlink()
        self.call("prepare")
        self.assertEqual(self.trace(), before)
        self.assertEqual(self.trace().count("stop guest"), 1)
        self.assertEqual(self.trace().count("--vnc-experimental"), 1)
        for name in ("owner", "vm", "holder.pid"):
            self.assertEqual((self.state / name).read_bytes(), records[name])
        for name in ("drain-token", "recovery-pending"):
            self.assertFalse((self.state / name).exists())
        self.assertFalse((self.root / "pool/pool.lock").exists())
        self.assertFalse(self.lease.exists())
        cleanups = (self.root / "cleanup-attempts").read_bytes()
        self.call("prepare")
        self.assertEqual((self.root / "cleanup-attempts").read_bytes(), cleanups)

    def test_poststart_cleanup_unreachable_guest_preserves_connection_and_evidence(self):
        before, records = self.pending_poststart()
        (self.root / "no-guest").touch()
        result = self.call("prepare", 1)
        self.assertIn(b"retry prepare when SSH is available", result.stderr)
        self.assertNotIn(b"restoration", result.stderr)
        self.assertEqual(self.trace(), before)
        self.assertEqual(records, {name: (self.state / name).read_bytes() for name in records})
        self.assertTrue(m.status(self.state, "guest")["connected"])
        self.assertFalse(self.lease.exists())

    def test_poststart_cleanup_resumes_after_guest_success_and_lost_response(self):
        (self.root / "lose-cleanup-response").touch()
        result = self.call("prepare", 1)
        self.assertIn(b"guest drain cleanup unconfirmed", result.stderr)
        self.assertNotIn(b"restoration", result.stderr)
        self.assertFalse((self.root / "pool/pool.lock").exists())
        self.assertTrue((self.state / "drain-token").exists())
        self.assert_cleanup_resumes()

    def test_poststart_cleanup_resumes_after_interruption_between_local_unlinks(self):
        (self.root / "interrupt-cleanup").touch()
        result = self.call("prepare", 1)
        self.assertNotIn(b"restoration", result.stderr)
        self.assertFalse((self.state / "drain-token").exists())
        self.assertEqual(len((self.state / "recovery-pending").read_text()), 64)
        before = self.trace()
        cleanups = (self.root / "cleanup-attempts").read_bytes()
        for name in ("shutdown-pending", "drain-precommit"):
            with self.subTest(name=name):
                marker = self.state / name
                marker.write_bytes((self.state / "recovery-pending").read_bytes())
                result = self.call("prepare", 1)
                self.assertIn(b"stop or admission uncertain", result.stderr)
                self.assertTrue(marker.exists())
                self.assertEqual(self.trace(), before)
                self.assertEqual((self.root / "cleanup-attempts").read_bytes(), cleanups)
                marker.unlink()
        self.assert_cleanup_resumes()

    def assert_cleanup_resumes(self):
        before = self.trace()
        owner = (self.state / "owner").read_bytes()
        self.assertTrue(m.status(self.state, "guest")["connected"])
        self.call("prepare")
        self.assertEqual(self.trace(), before)
        self.assertEqual((self.state / "owner").read_bytes(), owner)
        self.assertEqual(before.count("stop guest"), 1)
        self.assertEqual(before.count("--vnc-experimental"), 1)
        for name in ("drain-token", "recovery-pending"):
            self.assertFalse((self.state / name).exists())
        self.assertFalse(self.lease.exists())

    def test_poststart_transient_status_after_login_retains_session_and_can_resume(self):
        # First read after login, and the read after successful guest cleanup.
        for nth in (1, 3):
            with self.subTest(nth=nth):
                (self.root / "fail-status").write_text(str(nth))
                result = self.call("prepare", 1)
                self.assertNotIn(b"restoration", result.stderr)
                self.assertTrue((self.state / "recovery-pending").exists())
                self.assertTrue((self.state / "drain-token").exists())
                self.assert_cleanup_resumes()
                (self.root / "fail-status").unlink()
                (self.root / "status-count").unlink()
                self.call("stop")
                (self.root / "trace").unlink()

    def test_poststart_proven_disconnect_still_restores_headless(self):
        (self.root / "fail-status").write_text("1")
        (self.root / "disconnected-status").touch()
        result = self.call("prepare", 1)
        self.assertIn(b"restored headless", result.stderr)
        self.assertFalse((self.state / "recovery-pending").exists())
        self.assertEqual((self.root / "vm-mode").read_text(), "headless")

    def test_poststart_partial_cleanup_refuses_forged_lone_recovery(self):
        self.call("prepare")
        before = self.trace()
        recovery = self.state / "recovery-pending"
        recovery.write_text("f" * 64)
        result = self.call("prepare", 1)
        self.assertIn(b"does not predate the VNC supervisor", result.stderr)
        self.assertEqual(recovery.read_text(), "f" * 64)
        self.assertEqual(self.trace(), before)
        self.assertFalse((self.root / "pool/pool.lock").exists())

    def test_poststart_cleanup_refuses_uncertain_markers_and_competing_fence(self):
        before, records = self.pending_poststart()
        cleanups = (self.root / "cleanup-attempts").read_bytes()
        for name in ("shutdown-pending", "drain-precommit"):
            with self.subTest(name=name):
                (self.state / name).write_bytes(records["drain-token"])
                result = self.call("prepare", 1)
                self.assertIn(b"stop or admission uncertain", result.stderr)
                self.assertTrue((self.state / name).exists())
                (self.state / name).unlink()
        # Inject a competing fence after our scheduler holder is published.
        jq = shutil.which("jq")
        self.script("jq", '''import os, pathlib, sys
if '--arg' in sys.argv and 'vm-fold-prepare' in sys.argv:
    pathlib.Path(os.environ['FAKE_ROOT'], 'scheduler/mac-vm.guest.lock/sim-posture-pending').mkdir()
os.execv(''' + repr(jq) + ''', ['jq', *sys.argv[1:]])
''')
        result = self.call("prepare", 1)
        self.assertIn(b"competing scheduler fence", result.stderr)
        self.assertTrue((self.lease / "sim-posture-pending").exists())
        self.assertEqual(self.trace(), before)
        self.assertEqual(records, {name: (self.state / name).read_bytes() for name in records})
        self.assertEqual((self.root / "cleanup-attempts").read_bytes(), cleanups)

    def test_poststart_cleanup_refuses_mismatched_recovery(self):
        before, records = self.pending_poststart()
        (self.state / "recovery-pending").write_text("0" * 64)
        result = self.call("prepare", 1)
        self.assertIn(b"records do not match", result.stderr)
        self.assertEqual((self.state / "drain-token").read_bytes(), records["drain-token"])
        self.assertEqual(self.trace(), before)
        self.assertTrue(m.status(self.state, "guest")["connected"])

    def test_poststart_cleanup_refuses_new_stop_on_old_connected_supervisor(self):
        self.call("prepare")
        before = self.trace()
        token = "a" * 64
        for name in ("drain-token", "recovery-pending"):
            (self.state / name).write_text(token)
        result = self.call("prepare", 1)
        self.assertIn(b"does not predate the VNC supervisor", result.stderr)
        self.assertEqual(self.trace(), before)
        self.assertTrue(m.status(self.state, "guest")["connected"])
        for name in ("drain-token", "recovery-pending"):
            self.assertEqual((self.state / name).read_text(), token)

    def test_poststart_cleanup_preserves_replacement_recovery_after_successful_ssh(self):
        before, records = self.pending_poststart()
        (self.root / "fail-cleanup").unlink()
        (self.root / "replace-recovery").touch()
        result = self.call("prepare", 1)
        self.assertIn(b"cleanup metadata changed", result.stderr)
        self.assertEqual((self.state / "recovery-pending").read_text(), "replacement")
        self.assertEqual((self.state / "recovery-pending.retained").read_bytes(), records["recovery-pending"])
        self.assertTrue(m.status(self.state, "guest")["connected"])
        self.assertEqual(self.trace(), before)
        self.assertNotIn(b"restoration", result.stderr)

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
            for name in ("recovery-pending", "shutdown-pending", "drain-token"):
                self.assertFalse((self.state / name).exists())
            self.assertFalse((self.root / "pool/pool.lock").exists())
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
        self.assertTrue((self.root / 'pool/pool.lock/vnc-drain-owner').exists())
        (self.root / 'stop-uncertain').unlink()
        self.call('stop', 3)  # The unresolved stop retains the primary scheduler lease.
        self.assertTrue((self.state / 'shutdown-pending').exists())

    def test_guard_ssh_loss_while_tart_stop_waits_keeps_exclusion(self):
        (self.root / "stop-guard-loss").touch()
        attempt = subprocess.Popen(["bash", str(self.root / "tools/vm/duo-fold.sh"), "prepare"],
                                   env=self.env, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        try:
            m.wait_until(lambda: (self.root / "stop-waiting").exists(), 8, "stop never started")
            self.assertIsNone(attempt.poll())
            lock = self.root / "pool/pool.lock"
            self.assertTrue((lock / "vnc-drain-owner").exists())
            self.assertEqual(self.fixture_rpc('guest', 'ping'), b'alive\n')
            with self.assertRaises(FileExistsError):
                lock.mkdir()
            # Even an explicit cleanup request must refuse while Tart can stop.
            cleanup = subprocess.run([sys.executable, str(self.root / "tools/vm/vnc-session.py"),
                                      "clean-drain", str(self.state), str(self.bin / "vm-ssh")],
                                     env=self.env, capture_output=True, timeout=3)
            self.assertEqual(cleanup.returncode, 1)
            self.assertTrue(lock.exists())
            (self.root / "stop-release").touch()
            _, error = attempt.communicate(timeout=10)
            self.assertEqual(attempt.returncode, 1, error.decode())
            self.assertTrue(lock.exists())
            self.call("stop", 3)
            self.assertEqual(self.trace().count("stop guest"), 1)
            self.assertNotIn("run --no-graphics", self.trace())
            self.assertTrue((self.state / "shutdown-pending").exists())
            self.assertTrue((self.state / "recovery-pending").exists())
        finally:
            (self.root / "stop-release").touch()
            if attempt.poll() is None:
                attempt.kill()
            attempt.communicate(timeout=10)

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

    def test_posture_scheduler_machine_and_execution_failures_are_distinct(self):
        with tempfile.TemporaryDirectory() as directory:
            folder = Path(directory)
            (folder / "vm").mkdir()
            script = folder / "vm/duo-fold.sh"
            for code, expected in ((3, "queued:scheduler"), (5, "queued:transport"), (1, "fail")):
                script.write_text("#!/bin/sh\nexit " + str(code) + "\n")
                script.chmod(0o700)
                result = subprocess.run(["bash", "-c", '''
mg_job() { :; }
mg_failure() { :; }
mg_status() { echo "$1"; }
mg__mac_vm_unreachable() {
  case "$1" in *scheduler*) echo queued:scheduler;; *transport*) echo queued:transport;; esac
}
source ''' + shlex.quote(str(ROOT / "tools/mg.d/sim.sh")) + '''
MG_TOOLS=''' + shlex.quote(directory) + '''
mg_sim_posture open 11111111-2222-3333-4444-555555555555
'''], capture_output=True, timeout=3)
                self.assertEqual(result.returncode, 0, result.stderr.decode())
                self.assertEqual(result.stdout.decode().strip(), expected)


class PreviousSessionTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.state = Path(self.tmp.name).resolve() / "private state"
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

    def disconnected_probe(self, args, **kwargs):
        self.assertEqual(kwargs, {"capture_output": True, "timeout": 3})
        self.assertIn(args[0], ("/bin/ps", "/usr/sbin/lsof"))
        pid = args[args.index("-p") + 1]
        if args[0] == "/usr/sbin/lsof":
            self.assertIn("-iTCP", args)
            self.assertNotIn("-sTCP:ESTABLISHED", args)
            return subprocess.CompletedProcess(args, 1, b"", b"")
        return subprocess.CompletedProcess(args, 0 if pid == "101" else 1,
                                           b"101\n" if pid == "101" else b"", b"")

    def test_disconnected_two_observations_route_without_writes_and_refuse_changed_old(self):
        (self.state / "vm").write_text("meguri-mac")
        before = self.snapshot()
        managed = self.state.with_name("vmf2")
        with mock.patch.object(subprocess, "run", side_effect=self.disconnected_probe) as probe:
            self.assertEqual(m.route_state(self.state, "meguri-mac"), managed)
            self.assertEqual(probe.call_count, 8)  # two ps/ps/ps/lsof observations
            self.assertLess(len(os.fsencode(managed / "control.sock")),
                            len(os.fsencode(self.state / "control.sock")))
            self.assertFalse(managed.exists())
            managed.mkdir(mode=0o700)
            (managed / "owner").write_text("synthetic")
            self.assertEqual(m.route_state(self.state, "meguri-mac"), managed)
            for marker in ("shutdown-pending", "recovery-pending", "drain-token", "owner.pid"):
                (self.state / marker).touch()
                with self.assertRaises(RuntimeError):
                    m.route_state(self.state, "meguri-mac")
                (self.state / marker).unlink()
            with mock.patch.object(m, "process_identity", return_value=("0", [], "/usr/bin/sleep 60")):
                with self.assertRaises(RuntimeError):
                    m.route_state(self.state, "meguri-mac")
        self.assertEqual(self.snapshot(), before)

    def test_disconnected_requires_empty_exit_one_and_stable_exact_holder(self):
        (self.state / "vm").write_text("meguri-mac")
        for target in ("/bin/ps", "/usr/sbin/lsof"):
            for code, stdout, stderr in ((0, b"", b""), (1, b" ", b""), (1, b"", b" "), (2, b"", b"")):
                def bad_probe(args, **kwargs):
                    if args[0] == target and (target != "/bin/ps" or "102" in args):
                        return subprocess.CompletedProcess(args, code, stdout, stderr)
                    return self.disconnected_probe(args, **kwargs)
                with self.subTest(target=target, result=(code, stdout, stderr)), \
                        mock.patch.object(subprocess, "run", side_effect=bad_probe):
                    with self.assertRaises(RuntimeError):
                        m.legacy_disconnected(self.state, "meguri-mac")
        with mock.patch.object(subprocess, "run", side_effect=self.disconnected_probe), \
                mock.patch.object(m, "process_identity", side_effect=[("u", [], self.commands["101"]),
                                                                   ("u", ["changed"], self.commands["101"])]):
            with self.assertRaisesRegex(RuntimeError, "observation changed"):
                m.legacy_disconnected(self.state, "meguri-mac")

    def test_fresh_and_owned_v1_keep_the_canonical_root(self):
        fresh = self.state.parent / "fresh"
        self.assertEqual(m.route_state(fresh, "guest"), fresh)
        self.assertFalse(fresh.exists())
        (self.state / "owner").write_text("synthetic-owner")
        with mock.patch.object(m, "legacy_disconnected") as old_probe:
            self.assertEqual(m.route_state(self.state, "guest"), self.state)
            old_probe.assert_not_called()

    def test_route_guards_private_directories_and_symlinks_before_writes(self):
        managed = self.state.with_name("vmf2")
        for mode in (0o755, 0o777):
            self.state.chmod(mode)
            with self.assertRaises(RuntimeError):
                m.route_state(self.state, "guest")
        self.state.chmod(0o700)
        managed.symlink_to(self.state, target_is_directory=True)
        with self.assertRaises(RuntimeError):
            m.route_state(self.state, "guest")
        managed.unlink()
        linked = self.state.parent / "linked"
        linked.symlink_to(self.state.parent, target_is_directory=True)
        with self.assertRaises(RuntimeError):
            m.init_state(linked / "new-state")
        self.assertFalse((self.state.parent / "new-state").exists())

    def test_previous_format_status_and_inspect_preserve_session_without_lifecycle(self):
        before = self.snapshot()
        self.assertEqual(m.route_state(self.state, "guest"), self.state)
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
        self.root = Path(self.tmp.name).resolve()
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
        (self.hub / "Contents/MacOS").mkdir(parents=True)
        (self.hub / "Contents/MacOS/DevicesTrampoline").write_bytes(b"synthetic")
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
        for target in (self.hub, self.hub / "Contents/MacOS/DevicesTrampoline"):
            self.assertIn(("/usr/bin/codesign", "--verify", "--strict", "-R",
                           '=anchor apple and identifier "com.apple.dt.Devices"', str(target)), self.calls)
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
        for code in (0, 1, 255, -15, "timeout", "wait-error"):
            with self.subTest(code=code), tempfile.TemporaryDirectory() as directory:
                state = Path(directory)
                guard = mock.Mock()
                guard.stdout = io.BytesIO(b"drained\ncommitted\n")
                guard.poll.return_value = None
                child = mock.Mock()
                child.wait.return_value = code
                if code == "timeout":
                    child.wait.side_effect = subprocess.TimeoutExpired("synthetic", 75)
                elif code == "wait-error":
                    child.wait.side_effect = OSError("synthetic wait failed")
                with mock.patch.object(m, "clean_drain") as cleanup, \
                        mock.patch.object(subprocess, "Popen", side_effect=[guard, child]), \
                        mock.patch.object(m.select, "select", return_value=([guard.stdout], [], [])):
                    if code == 0:
                        m.shutdown(state, "fake-ssh", "fake-tart", "guest", state)
                    else:
                        message = "exited unsuccessfully" if code == 1 else "completion unknown"
                        with self.assertRaisesRegex(RuntimeError, message):
                            m.shutdown(state, "fake-ssh", "fake-tart", "guest", state)
                self.assertTrue((state / "recovery-pending").exists())
                self.assertEqual((state / "shutdown-pending").exists(), code in (255, -15, "timeout", "wait-error"))
                pending = code in (255, -15, "timeout", "wait-error")
                self.assertEqual((state / "vm-shutdown-pending").exists(), pending)
                if pending:
                    intent = json.loads((state / "vm-shutdown-pending/intent.json").read_text())
                    self.assertEqual(intent["token"], (state / "drain-token").read_text())
                    self.assertEqual(intent["token"], (state / "shutdown-pending").read_text())
                self.assertEqual(cleanup.call_count, 2 if code == 1 else 1)
                child.wait.assert_called_once_with(timeout=75)
                guard.stdin.close.assert_called_once()
                guard.wait.assert_called_once_with(timeout=5)
                guard.terminate.assert_not_called()

    def test_exec_enoent_rolls_back_only_attempt_owned_evidence(self):
        for earlier_recovery in (False, True):
            with self.subTest(earlier_recovery=earlier_recovery), tempfile.TemporaryDirectory() as directory:
                state = Path(directory)
                if earlier_recovery:
                    (state / "recovery-pending").write_text("earlier")
                (state / "sim-posture-pending").mkdir()
                guard = mock.Mock()
                guard.stdout = io.BytesIO(b"drained\ncommitted\n")
                guard.poll.return_value = None
                with mock.patch.object(subprocess, "Popen", side_effect=[guard, FileNotFoundError()]) as launch, \
                        mock.patch.object(m.select, "select", return_value=([guard.stdout], [], [])), \
                        mock.patch.object(subprocess, "run", return_value=subprocess.CompletedProcess([], 0)) as cleanup:
                    with self.assertRaisesRegex(RuntimeError, "could not launch; no stop issued"):
                        m.shutdown(state, "fake-ssh", "missing-tart", "guest", state)
                self.assertEqual(launch.call_count, 2)
                self.assertFalse((state / "vm-shutdown-pending").exists())
                self.assertTrue((state / "sim-posture-pending").is_dir())
                cleanup.assert_called_once()
                self.assertFalse((state / "shutdown-pending").exists())
                self.assertFalse((state / "drain-token").exists())
                self.assertEqual((state / "recovery-pending").exists(), earlier_recovery)
                if earlier_recovery:
                    self.assertEqual((state / "recovery-pending").read_text(), "earlier")

    def test_denied_drain_cannot_launch_stop_and_releases_no_stop_token(self):
        with tempfile.TemporaryDirectory() as directory:
            state = Path(directory)
            guard = mock.Mock()
            guard.stdout = io.BytesIO(b"")
            guard.poll.return_value = 1
            with mock.patch.object(subprocess, "Popen", return_value=guard) as launch, \
                    mock.patch.object(m.select, "select", return_value=([guard.stdout], [], [])), \
                    mock.patch.object(subprocess, "run", return_value=subprocess.CompletedProcess([], 0)):
                with self.assertRaisesRegex(RuntimeError, "shutdown refused"):
                    m.shutdown(state, "fake-ssh", "fake-tart", "guest", state)
            launch.assert_called_once()
            for name in ("recovery-pending", "shutdown-pending", "drain-token"):
                self.assertFalse((state / name).exists())

    def test_uncertain_stop_blocks_cleanup_and_duplicate_stop_before_transport(self):
        with tempfile.TemporaryDirectory() as directory:
            state = Path(directory)
            (state / "shutdown-pending").touch()
            (state / "drain-token").write_text("owned-token")
            with mock.patch.object(subprocess, "Popen") as launch, \
                    mock.patch.object(subprocess, "run") as cleanup:
                for operation, args in ((m.clean_drain, (state, "fake-ssh")),
                                        (m.shutdown, (state, "fake-ssh", "fake-tart", "guest", state))):
                    with self.assertRaisesRegex(RuntimeError, "completion unknown"):
                        operation(*args)
                launch.assert_not_called()
                cleanup.assert_not_called()
            self.assertEqual((state / "drain-token").read_text(), "owned-token")

    def test_cleanup_timeout_has_distinct_sanitized_reason_and_preserves_token(self):
        with tempfile.TemporaryDirectory() as directory:
            state = Path(directory)
            (state / "drain-token").write_text("owned-token")
            error = subprocess.TimeoutExpired(["fake-ssh", "private-control-data"], 120)
            with mock.patch.object(subprocess, "run", side_effect=error) as cleanup:
                with self.assertRaisesRegex(RuntimeError, "guest drain cleanup unconfirmed") as raised:
                    m.clean_drain(state, "fake-ssh", "owned-token")
            self.assertNotIn("private-control-data", str(raised.exception))
            self.assertEqual(cleanup.call_args.kwargs["timeout"], 120)
            self.assertEqual((state / "drain-token").read_text(), "owned-token")

    def test_cleanup_does_not_remove_replaced_token_after_successful_ssh(self):
        with tempfile.TemporaryDirectory() as directory:
            state = Path(directory)
            token = state / "drain-token"
            token.write_text("owned-token")
            def replace(*args, **kwargs):
                token.rename(state / "retained-token")
                token.write_text("replacement")
                return subprocess.CompletedProcess([], 0)
            with mock.patch.object(subprocess, "run", side_effect=replace):
                with self.assertRaisesRegex(RuntimeError, "metadata changed"):
                    m.clean_drain(state, "fake-ssh", "owned-token")
            self.assertEqual(token.read_text(), "replacement")

    def test_delayed_guard_cannot_publish_after_confirmed_no_stop_cancellation(self):
        # Pause the real guard before token installation, or after accepting
        # commit but before its ack. Keep stdin OPEN: delayed EOF is not proof.
        for phase in ("installation", "ack"):
            with self.subTest(phase=phase), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                state, pool = root / "state", root / "pool"
                state.mkdir()
                token = "delayed-guard"
                (state / "drain-token").write_text(token)
                (state / "drain-precommit").write_text(token)
                ssh = root / "ssh"
                ssh.write_text("#!" + sys.executable + "\nimport subprocess, sys\n"
                               "sys.exit(subprocess.call(['/bin/sh', '-c', sys.argv[1]]))\n")
                ssh.chmod(0o700)
                gate_read, gate_write = os.pipe()
                ready_read, ready_write = os.pipe()
                pause = ("    os.write(" + str(ready_write) + ", b'ready'); "
                         "os.read(" + str(gate_read) + ", 1)\n")
                anchor = ("    deadline = time.monotonic() + 10\n"
                          "    while True:\n"
                          "        check_cancelled()" if phase == "installation"
                          else '    try:\n        print("committed", flush=True)')
                self.assertEqual(m.DRAIN.count(anchor), 1)
                script = m.DRAIN.replace(anchor, pause + anchor)
                env = dict(os.environ, MEGURI_SIM_POOL_DIR=str(pool))
                child = subprocess.Popen([sys.executable, '-c', script, token], env=env,
                                         stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                         stderr=subprocess.DEVNULL, pass_fds=(gate_read, ready_write))
                os.close(gate_read)
                os.close(ready_write)
                try:
                    if phase == "ack":
                        self.assertEqual(child.stdout.readline(), b'drained\n')
                        child.stdin.write(('commit ' + token + '\n').encode())
                        child.stdin.flush()
                    self.assertTrue(m.select.select([ready_read], [], [], 3)[0])
                    self.assertEqual(os.read(ready_read, 5), b'ready')
                    with mock.patch.dict(os.environ, MEGURI_SIM_POOL_DIR=str(pool)):
                        m.clean_drain(state, str(ssh), token)
                    self.assertFalse((state / 'drain-token').exists())
                    self.assertTrue((pool / 'vnc-drain-cancelled' / token).exists())
                    os.write(gate_write, b'go')
                    child.wait(timeout=3)
                    self.assertFalse((pool / 'pool.lock').exists())
                    self.assertFalse((pool / 'vnc-drain-cancelled' / token).exists())
                    # A legacy client can acquire immediately after the guard joins.
                    (pool / 'pool.lock').mkdir()
                finally:
                    child.stdin.close()
                    child.stdout.close()
                    os.close(gate_write)
                    os.close(ready_read)
                    if child.poll() is None:
                        child.kill()
                        child.wait()

    def test_denied_precommit_eof_wrong_instruction_and_timeout_release_pool(self):
        for denied in ('eof', 'wrong', 'timeout'):
            with self.subTest(denied=denied), tempfile.TemporaryDirectory() as directory:
                pool = Path(directory)
                child = subprocess.Popen([sys.executable, '-c', m.DRAIN, 'test-guard'],
                                         env=dict(os.environ, MEGURI_SIM_POOL_DIR=directory),
                                         stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                         stderr=subprocess.DEVNULL)
                try:
                    self.assertEqual(child.stdout.readline(), b'drained\n')
                    if denied == 'timeout':
                        child.send_signal(signal.SIGALRM)  # Our own subprocess; same bounded-alarm handler.
                    else:
                        if denied == 'wrong':
                            child.stdin.write(b'commit foreign-token\n')
                            child.stdin.flush()
                        child.stdin.close()
                    self.assertNotEqual(child.wait(timeout=3), 0)
                    self.assertNotIn(b'committed', child.stdout.read())
                    self.assertFalse((pool / 'pool.lock').exists())
                    (pool / 'pool.lock').mkdir()
                finally:
                    child.stdin.close()
                    child.stdout.close()
                    if child.poll() is None:
                        child.kill()
                        child.wait()

    def test_lost_commit_ack_requires_confirmed_cancellation_before_discarding_token(self):
        with tempfile.TemporaryDirectory() as directory:
            state = Path(directory)
            guard = mock.Mock()
            guard.stdout = io.BytesIO(b'drained\n')  # Commit sent, ack lost.
            guard.poll.return_value = None
            with mock.patch.object(subprocess, 'Popen', return_value=guard) as launch, \
                    mock.patch.object(m.select, 'select', return_value=([guard.stdout], [], [])), \
                    mock.patch.object(subprocess, 'run', return_value=subprocess.CompletedProcess([], 255)):
                with self.assertRaisesRegex(RuntimeError, 'cleanup unconfirmed'):
                    m.shutdown(state, 'fake-ssh', 'fake-tart', 'guest', state)
            launch.assert_called_once()  # Stop cannot launch without committed ack.
            guard.stdin.write.assert_called_once()
            token = (state / 'drain-token').read_text()
            self.assertEqual((state / 'drain-precommit').read_text(), token)
            for name in ('recovery-pending', 'shutdown-pending'):
                self.assertFalse((state / name).exists())
            with mock.patch.object(subprocess, 'run', return_value=subprocess.CompletedProcess([], 0)) as cleanup:
                m.clean_drain(state, 'fake-ssh')
            self.assertIn(m.CANCEL_DRAIN, cleanup.call_args.args[0][1])
            self.assertFalse((state / 'drain-token').exists())
            self.assertFalse((state / 'drain-precommit').exists())

    def test_unpublished_guard_with_closed_caller_cleans_its_lock(self):
        with tempfile.TemporaryDirectory() as directory:
            result = subprocess.run([sys.executable, '-c', m.DRAIN, 'test-guard'],
                                    env=dict(os.environ, MEGURI_SIM_POOL_DIR=directory),
                                    stdin=subprocess.DEVNULL, capture_output=True, timeout=3)
            self.assertNotEqual(result.returncode, 0)
            self.assertNotIn(b"committed", result.stdout)
            self.assertFalse((Path(directory) / "pool.lock").exists())

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

    def test_overlapping_paused_cleanups_exclude_replacement_guard(self):
        with tempfile.TemporaryDirectory() as directory:
            pool = Path(directory)
            lock = pool / "pool.lock"
            lock.mkdir()
            (lock / "pid").write_text("old")
            (lock / "vnc-drain-owner").write_text("A")
            env = dict(os.environ, MEGURI_SIM_POOL_DIR=directory)
            gate_read, gate_write = os.pipe()
            ready_read, ready_write = os.pipe()
            # Pause after checking A, precisely where another timed-out SSH
            # cleanup used to remove A and let B reuse the pathname.
            anchor = '            os.unlink("vnc-drain-owner", dir_fd=fd)'
            self.assertEqual(m.CLEAN_DRAIN.count(anchor), 1)
            pause = ("            os.write(" + str(ready_write) + ", b'ready'); "
                     "os.read(" + str(gate_read) + ", 1)\n")
            first = subprocess.Popen([sys.executable, "-c", m.CLEAN_DRAIN.replace(anchor, pause + anchor), "A"],
                                     env=env, pass_fds=(gate_read, ready_write), stderr=subprocess.PIPE)
            os.close(gate_read)
            os.close(ready_write)
            second = guard = None
            try:
                self.assertTrue(m.select.select([ready_read], [], [], 3)[0])
                self.assertEqual(os.read(ready_read, 5), b"ready")
                # Announce the second cleanup's failed nonblocking flock, so
                # the assertion proves serialization rather than scheduling.
                anchor = "            except BlockingIOError:\n"
                announce = (anchor + "                if not globals().get('_blocked'):\n"
                            "                    print('blocked', flush=True)\n"
                            "                    globals()['_blocked'] = True\n")
                second = subprocess.Popen([sys.executable, "-c", m.CLEAN_DRAIN.replace(anchor, announce), "A"],
                                          env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
                self.assertTrue(m.select.select([second.stdout], [], [], 3)[0])
                self.assertEqual(second.stdout.readline(), b"blocked\n")
                guard = subprocess.Popen([sys.executable, "-c", m.DRAIN.replace(anchor, announce), "B"], env=env,
                                         stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
                self.assertTrue(m.select.select([guard.stdout], [], [], 3)[0])
                self.assertEqual(guard.stdout.readline(), b"blocked\n")
                self.assertEqual((lock / "vnc-drain-owner").read_text(), "A")
                os.write(gate_write, b"go")
                self.assertEqual(first.wait(timeout=3), 0, first.stderr.read())
                # Either mutex order after A's release is legal. Even if the
                # stale cleanup gets it after B, it must preserve B's inode.
                self.assertEqual(guard.stdout.readline(), b"drained\n")
                guard.stdin.write(b"commit B\n")
                guard.stdin.flush()
                self.assertEqual(guard.stdout.readline(), b"committed\n")
                self.assertEqual(second.wait(timeout=3), 0, second.stderr.read())
                inode = lock.stat().st_ino
                subprocess.run([sys.executable, "-c", m.CLEAN_DRAIN, "A"], env=env, check=True, timeout=3)
                self.assertEqual(lock.stat().st_ino, inode)
                self.assertEqual((lock / "vnc-drain-owner").read_text(), "B")
                subprocess.run([sys.executable, "-c", m.CLEAN_DRAIN, "B"], env=env, check=True, timeout=3)
                guard.wait(timeout=3)
            finally:
                os.close(gate_write)
                os.close(ready_read)
                for child in (first, second, guard):
                    if child is None:
                        continue
                    if child.poll() is None:
                        child.kill()  # Only a subprocess owned by this test.
                        child.wait()
                    for stream in (child.stdin, child.stdout, child.stderr):
                        if stream:
                            stream.close()

    def test_cleanup_preserves_pending_and_foreign_pool_entries(self):
        for entry in ("sim-posture-pending", "posture-owner", "foreign-token"):
            with self.subTest(entry=entry), tempfile.TemporaryDirectory() as directory:
                lock = Path(directory) / "pool.lock"
                lock.mkdir()
                (lock / "pid").write_text("old")
                (lock / "vnc-drain-owner").write_text("A")
                (lock / entry).mkdir()
                result = subprocess.run([sys.executable, "-c", m.CLEAN_DRAIN, "A"],
                                        env=dict(os.environ, MEGURI_SIM_POOL_DIR=directory),
                                        capture_output=True, timeout=3)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(set(p.name for p in lock.iterdir()), {"pid", "vnc-drain-owner", entry})
                self.assertEqual((lock / "vnc-drain-owner").read_text(), "A")

    def test_published_guard_survives_transport_loss_and_legacy_stale_rules(self):
        for lost in ("eof", signal.SIGHUP, signal.SIGTERM, signal.SIGALRM):
            with self.subTest(lost=lost), tempfile.TemporaryDirectory() as directory:
                pool = Path(directory)
                lock = pool / "pool.lock"
                env = dict(os.environ, MEGURI_SIM_POOL_DIR=directory)
                child = subprocess.Popen([sys.executable, '-c', m.DRAIN, 'test-guard'],
                                         env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                         stderr=subprocess.DEVNULL)
                try:
                    self.assertEqual(child.stdout.readline(), b'drained\n')
                    child.stdin.write(b'commit test-guard\n')
                    child.stdin.flush()
                    self.assertEqual(child.stdout.readline(), b'committed\n')
                    if lost == "eof":
                        child.stdin.close()
                    else:
                        child.send_signal(lost)
                    # Legacy clients reclaim either a dead PID or an old directory.
                    old = time.time() - 1200
                    os.utime(lock, (old, old))
                    m.wait_until(lambda: lock.stat().st_mtime > old, 3, "fence not refreshed")
                    self.assertIsNone(child.poll())
                    with self.assertRaises(FileExistsError):
                        lock.mkdir()
                    subprocess.run([sys.executable, '-c', m.CLEAN_DRAIN, 'foreign'],
                                   env=env, check=True, timeout=3)
                    self.assertTrue(lock.exists())
                    subprocess.run([sys.executable, '-c', m.CLEAN_DRAIN, 'test-guard'],
                                   env=env, check=True, timeout=3)
                    child.wait(timeout=3)
                    self.assertFalse(lock.exists())
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

    def test_final_drain_excludes_legacy_renew_until_explicit_cleanup(self):
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
                child.stdin.write(b'commit test-guard\n')
                child.stdin.flush()
                self.assertEqual(child.stdout.readline(), b'committed\n')
                self.assertEqual((pool / 'pool.lock/pid').read_text(), str(child.pid))
                # This is the exact mkdir mutex used by legacy renew clients.
                with self.assertRaises(FileExistsError):
                    (pool / 'pool.lock').mkdir()
                self.assertEqual(lease.read_text(), '{"expires_epoch": 0}')
            finally:
                child.stdin.close()
                subprocess.run([sys.executable, '-c', m.CLEAN_DRAIN, 'test-guard'],
                               env=dict(os.environ, MEGURI_SIM_POOL_DIR=directory), check=True, timeout=3)
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
