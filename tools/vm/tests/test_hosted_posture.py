"""Docker-only resident peer fixtures; no native tools, SSH host or real pixels.

Run: tools/mg image-tools-selftest -k hosted
"""
import base64
import hashlib
import io
import types
import importlib.util
import json
import os
from pathlib import Path
import shutil
import socket
import subprocess
import sys
import tempfile
import time
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[3]
spec = importlib.util.spec_from_file_location("hosted", ROOT / "tools/vm/vnc-hosted.py")
h = importlib.util.module_from_spec(spec)
spec.loader.exec_module(h)

# Only the vncdotool boundary is synthetic. The actual shipped supervisor,
# FIFO holder, capture barrier, callback, RPC and detached launch run unchanged.
PYTHON = r'''#!PYTHON
import os, pathlib, runpy, socket, sys, time, types
from PIL import Image
root = pathlib.Path(__file__).resolve().parents[2]
class Client:
    def __init__(self):
        self.screen = Image.new('RGB', (200, 100), 'blue')
        self.width, self.height = self.screen.size
        self.factory = types.SimpleNamespace(force_caps=False)
    def refreshScreen(self): return self
    def captureScreen(self, path):
        (root / 'capturing').touch()
        while (root / 'block-capture').exists(): time.sleep(.01)
        self.screen.save(path)
        return self
    def mouseMove(self, x, y):
        (root / 'point').write_text('%s %s' % (x, y))
        return self
    def mouseDown(self, button):
        with (root / 'buttons').open('a') as log: log.write('down\n')
        while (root / 'block-click').exists(): time.sleep(.01)
    def mouseUp(self, button):
        with (root / 'buttons').open('a') as log: log.write('up\n')
    def connectionLost(self, reason): pass
library = types.ModuleType('vncdotool.client')
library.VNCDoToolClient = Client
library.VNCDoToolFactory = type('Factory', (), {})
package = types.ModuleType('vncdotool')
def connect(address, password, factory_class):
    instance = factory_class.protocol()
    instance.connection = socket.create_connection(('127.0.0.1', int(address.split('::')[1])))
    return instance
package.api = types.SimpleNamespace(connect=connect)
sys.modules.update({'vncdotool':package, 'vncdotool.client':library})
sys.argv = sys.argv[1:]
if sys.argv[0] == '-I': sys.argv.pop(0)
runpy.run_path(sys.argv[0], run_name='__main__')
'''
TART = r'''#!PYTHON
import pathlib, socket, sys
root = pathlib.Path(__file__).resolve().parents[1] / 'vnc-owner-v1'
assert sys.argv[1:4] == ['run', '--no-graphics', '--vnc-experimental']
with socket.socket() as listener:
    listener.bind(('127.0.0.1', 0)); listener.listen(1)
    with (root / 'runs').open('a') as log: log.write('run\n')
    print('vnc://:synthetic@127.0.0.1:%d' % listener.getsockname()[1], flush=True)
    peer, _ = listener.accept()
    with (root / 'connections').open('a') as log: log.write('connection\n')
    with peer:
        while peer.recv(4096): pass
'''


class HostedPeerTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="hp-")
        self.kit = Path(self.temp.name)
        self.root = self.kit / h.VERSION
        self.root.mkdir(mode=0o700)
        for name in h.FILES:
            (self.root / name).write_bytes((ROOT / "tools/vm" / name).read_bytes())
            (self.root / name).chmod(0o600)
        binary = self.root / "venv/bin/python"
        binary.parent.mkdir(parents=True)
        binary.write_text(PYTHON.replace("#!PYTHON", "#!" + sys.executable))
        binary.chmod(0o700)
        tart = self.kit / "bin/tart"
        tart.parent.mkdir()
        tart.write_text(TART.replace("#!PYTHON", "#!" + sys.executable))
        tart.chmod(0o700)
        self.token = "a" * 64
        (self.root / "drains").mkdir(mode=0o700)
        drain = self.root / "drains" / self.token
        drain.mkdir(parents=True, mode=0o700)
        (drain / "intent.json").write_text(json.dumps(dict(vm="guest", drain=self.token)))
        (drain / "receipt.json").write_text(json.dumps(dict(joined=True, code=0, stopped=True)))
        for path in drain.iterdir(): path.chmod(0o600)
        self.controller = None

    def tearDown(self):
        for name in ("block-capture", "block-click"):
            (self.root / name).unlink(missing_ok=True)
        if (self.root / "state/control.sock").exists():
            with __import__('contextlib').suppress(Exception):
                h.session.request(self.root / "state", "stop", timeout=15)
        if self.controller is not None:
            if self.controller.poll() is None:
                self.controller.terminate()
            self.controller.communicate(timeout=5)
        self.temp.cleanup()

    def wait(self, probe):
        return h.session.wait_until(probe, 10, "synthetic resident fixture timed out")

    def start(self):
        # This disposable process represents the SSH command. Killing it must
        # neither signal nor close the resident owner's inherited descriptors.
        script = '''import importlib.util, pathlib, sys, time
from unittest import mock
spec = importlib.util.spec_from_file_location('hosted', sys.argv[1])
h = importlib.util.module_from_spec(spec); spec.loader.exec_module(h)
with mock.patch.object(h, 'preflight'), mock.patch.object(h, 'tart_state', return_value='stopped'):
    h.detached_start(pathlib.Path(sys.argv[2]), 'guest', sys.argv[3])
print('detached', flush=True)
time.sleep(30)
'''
        self.controller = subprocess.Popen([sys.executable, "-c", script,
            str(ROOT / "tools/vm/vnc-hosted.py"), str(self.root), self.token],
            stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        self.assertEqual(self.controller.stdout.readline(), b"detached\n")
        self.wait(lambda: h.owner_status(self.root, "guest")["connected"])
        self.sid = h.owner_status(self.root, "guest")["session"]

    def operation(self, op, **values):
        return h.resident_rpc(self.root, dict(op="operation", vm="guest",
            operation=dict(op=op, session=self.sid, **values)))

    def test_hosted_detached_peer_capture_survives_controller_and_capture_disconnect(self):
        self.start()
        self.controller.terminate()
        self.controller.communicate(timeout=5)
        self.assertTrue(h.owner_status(self.root, "guest")["connected"])
        self.assertEqual(h.owner_status(self.root, "guest")["phase"], "connected")
        first = self.operation("move-capture", point=[20, 40])["png"]
        self.assertEqual((self.root / "point").read_text(), "20 40")
        (self.root / "capturing").unlink()
        (self.root / "block-capture").touch()
        # Lose the local relay socket while the resident is performing capture.
        with socket.socket(socket.AF_UNIX) as peer:
            peer.connect(str(self.root / "state/control.sock"))
            owner = (self.root / "state/owner").read_text()
            peer.sendall((json.dumps([owner, dict(op="capture", session=self.sid)]) + "\n").encode())
            self.wait(lambda: (self.root / "capturing").exists())
        (self.root / "block-capture").unlink()
        self.assertEqual(self.operation("capture")["png"], first)
        with mock.patch.object(h, "preflight"), mock.patch.object(h, "tart_state") as state:
            self.assertTrue(h.detached_start(self.root, "guest", self.token)["connected"])
            state.assert_not_called()
        self.assertEqual((self.root / "connections").read_text(), "connection\n")
        self.assertEqual((self.root / "runs").read_text(), "run\n")

    def test_hosted_one_click_receipt_recovers_after_disconnect_without_retry(self):
        self.start()
        self.operation("capture")
        rid, guard = "b" * 64, "c" * 32
        self.operation("reserve", request=rid, guard=guard)
        ticket = json.loads((self.root / "operations" / rid / "ticket.json").read_text())
        self.assertLessEqual(ticket["deadline"], time.monotonic() + 5)
        (self.root / "block-click").touch()
        with socket.socket(socket.AF_UNIX) as peer:
            peer.connect(str(self.root / "state/control.sock"))
            peer.sendall((json.dumps([(self.root / "state/owner").read_text(),
                dict(op="click", session=self.sid, request=rid, guard=guard)]) + "\n").encode())
            self.wait(lambda: (self.root / "buttons").exists())
        self.assertIsNone(self.operation("receipt", request=rid, guard=guard)["receipt"])
        with self.assertRaises(RuntimeError):
            self.operation("click", request=rid, guard=guard)
        self.assertEqual((self.root / "buttons").read_text(), "down\n")
        (self.root / "block-click").unlink()
        receipt = self.wait(lambda: self.operation("receipt", request=rid, guard=guard)["receipt"])
        self.assertEqual(receipt, dict(joined=True, executed=True))
        self.assertEqual(self.operation("receipt", request=rid, guard=guard)["receipt"], receipt)
        self.assertEqual((self.root / "buttons").read_text(), "down\nup\n")
        for invalid in (dict(op="receipt", session=self.sid, request=rid, guard="d" * 32),
                        dict(op="capture", session="e" * 64),
                        dict(op="capture", session=self.sid, path="/tmp/arbitrary"), "stop"):
            with self.assertRaises(RuntimeError):
                h.resident_rpc(self.root, dict(op="operation", vm="guest", operation=invalid))
        self.assertTrue(h.owner_status(self.root, "guest")["connected"])

    def test_hosted_expired_ticket_never_clicks_and_can_be_queried(self):
        self.start()
        rid, guard = "b" * 64, "c" * 32
        self.operation("reserve", request=rid, guard=guard)
        path = self.root / "operations" / rid / "ticket.json"
        record = json.loads(path.read_text()); record["deadline"] = 0
        path.write_text(json.dumps(record))
        receipt = self.operation("receipt", request=rid, guard=guard)["receipt"]
        self.assertEqual(receipt, dict(joined=True, executed=False, error="expired"))
        with self.assertRaises(RuntimeError):
            self.operation("click", request=rid, guard=guard)
        self.assertFalse((self.root / "buttons").exists())
        self.operation("capture")


class HostedAdmissionTests(unittest.TestCase):
    def test_hosted_route_reuses_validated_ownerless_session_only_when_resident_vacant(self):
        spec = importlib.util.spec_from_file_location("previous_fixture", ROOT / "tools/tests/vnc_session_test.py")
        fixtures = importlib.util.module_from_spec(spec); spec.loader.exec_module(fixtures)
        fixture = fixtures.PreviousSessionTests(methodName="runTest")
        fixture.setUp()
        self.addCleanup(fixture.doCleanups)
        original = fixture.state
        fixture.state = original.with_name("vm-fold")
        original.rename(fixture.state)
        fixture.commands = {pid: command.replace(str(original), str(fixture.state))
                            for pid, command in fixture.commands.items()}
        canonical = fixture.state
        hosted = canonical.with_name("vmh3")
        before = fixture.snapshot()
        vacancy = dict(connected=False, conclusive=True, vacant=True)
        resident = mock.Mock(return_value=vacancy)
        with mock.patch.object(h.session, "hosted_module", return_value=types.SimpleNamespace(hosted_status=resident)), \
                mock.patch.object(h.session, "start") as start, \
                mock.patch.object(h.session, "stop") as stop:
            self.assertFalse((canonical / "owner").exists())
            self.assertFalse(hosted.exists())
            self.assertEqual(h.session.route_state(canonical, "guest", fixture.tart), canonical)
            resident.assert_called_once_with(hosted, "guest", fixture.tart)
            self.assertTrue(fixture.output.called)  # Actual legacy process/socket validation ran.
            for result in (dict(vacancy, conclusive=False), dict(vacancy, vacant=False),
                           dict(connected=True, conclusive=True)):
                with self.subTest(resident=result):
                    resident.return_value = result
                    fixture.output.reset_mock()
                    self.assertEqual(h.session.route_state(canonical, "guest", fixture.tart), hosted)
                    fixture.output.assert_not_called()
            resident.return_value = vacancy
            for root in (canonical, hosted):
                if root == hosted: root.mkdir(mode=0o700)
                for name in ("shutdown-pending", "recovery-pending", "drain-token",
                             "drain-precommit", "sim-posture-pending"):
                    with self.subTest(root=root.name, fence=name):
                        marker = root / name
                        marker.symlink_to(root / "absent-fence-target")
                        self.assertEqual(h.session.route_state(canonical, "guest", fixture.tart), hosted)
                        marker.unlink()
                if root == hosted: root.rmdir()
            holder_socket = fixture.sockets["101"]
            fixture.sockets["101"] = b""
            self.assertEqual(h.session.route_state(canonical, "guest", fixture.tart), hosted)
            fixture.sockets["101"] = holder_socket
            with mock.patch.object(subprocess, "check_output", side_effect=subprocess.TimeoutExpired("ps", 2)):
                self.assertEqual(h.session.route_state(canonical, "guest", fixture.tart), hosted)
            # An unavailable supervisor must still never downgrade to the old format.
            (canonical / "owner").write_text("unavailable-owner")
            self.assertEqual(h.session.route_state(canonical, "guest", fixture.tart), hosted)
            (canonical / "owner").unlink()
            self.assertEqual(h.session.route_state(canonical, "guest", fixture.tart), canonical)
            start.assert_not_called()
            stop.assert_not_called()
        self.assertEqual(fixture.snapshot(), before)
        self.assertFalse(hosted.exists())

    def test_hosted_status_never_turns_an_uncertain_owner_into_vacancy(self):
        with tempfile.TemporaryDirectory(prefix="ha-") as directory:
            root = Path(directory)
            (root / "state").mkdir(mode=0o700)
            self.assertEqual(h.owner_status(root, "guest")["phase"], "vacant")
            (root / "launch.json").write_text(json.dumps(dict(vm="guest", drain="a" * 64)))
            self.assertEqual(h.owner_status(root, "guest")["phase"], "starting")
            (root / "state/owner").write_text("private")
            with mock.patch.object(h.session, "request", side_effect=TimeoutError()):
                with self.assertRaises(TimeoutError): h.owner_status(root, "guest")
            with mock.patch.object(h, "endpoint", return_value=("wrapper", "guest")), \
                    mock.patch.object(h, "rpc", side_effect=RuntimeError()):
                result = h.hosted_status(root, "guest", "wrapper")
            self.assertFalse(result["connected"])
            self.assertFalse(result["conclusive"])
            self.assertFalse(result["vacant"])
            self.assertTrue(result["remote"])

    def test_hosted_stop_is_detached_token_bound_once_and_needs_positive_receipt(self):
        with tempfile.TemporaryDirectory(prefix="ha-") as directory:
            root = Path(directory)
            token = "a" * 64
            with mock.patch.object(h, "preflight"), \
                    mock.patch.object(h, "owner_status", return_value=dict(vacant=True)), \
                    mock.patch.object(h.subprocess, "Popen") as launch:
                h.reserve_stop(root, "guest", token)
                h.reserve_stop(root, "guest", token)
                self.assertEqual(launch.call_count, 1)
                self.assertTrue(launch.call_args.kwargs["start_new_session"])
                self.assertEqual(launch.call_args.kwargs["stdin"], subprocess.DEVNULL)
                with self.assertRaises(RuntimeError): h.reserve_stop(root, "guest", "b" * 64)
            with mock.patch.object(h.subprocess, "run", return_value=mock.Mock(returncode=0)), \
                    mock.patch.object(h, "tart_state", return_value="stopped"):
                h.stop_peer(root, "guest", token)
                with self.assertRaises(FileExistsError): h.stop_peer(root, "guest", token)
            self.assertEqual(h.stop_receipt(root, "guest", token), dict(joined=True, stopped=True, code=0))
            with self.assertRaises(RuntimeError): h.stop_receipt(root, "foreign", token)

    def test_hosted_proven_spawn_failures_recover_only_with_original_token(self):
        for kind in ("owner", "stop"):
            with self.subTest(kind=kind), tempfile.TemporaryDirectory(prefix="ha-") as directory:
                root = Path(directory)
                token = "a" * 64
                if kind == "owner":
                    path = root / "drains" / token
                    path.parent.mkdir(mode=0o700)
                    path.mkdir(mode=0o700)
                    h.atomic_json(path / "intent.json", dict(vm="guest", drain=token))
                    h.atomic_json(path / "receipt.json", dict(joined=True, stopped=True, code=0))
                    call = h.detached_start
                    failure = root / "owner-spawn-failed.json"
                else:
                    call = h.reserve_stop
                    failure = root / "drains" / token / "stop-spawn-failed.json"
                with mock.patch.object(h, "preflight"), mock.patch.object(h, "tart_state", return_value="stopped"), \
                        mock.patch.object(h.subprocess, "Popen", side_effect=OSError(11, "process pressure")):
                    with self.assertRaises(OSError): call(root, "guest", token)
                self.assertEqual(json.loads(failure.read_text()), dict(vm="guest", drain=token))
                with mock.patch.object(h, "preflight"), mock.patch.object(h, "tart_state", return_value="stopped"), \
                        mock.patch.object(h.subprocess, "Popen") as spawn:
                    with self.assertRaises(RuntimeError): call(root, "guest", "b" * 64)
                    spawn.assert_not_called()
                    call(root, "guest", token)
                    call(root, "guest", token)
                    self.assertEqual(spawn.call_count, 1)
                    self.assertFalse(failure.exists())

    def test_hosted_log_open_failure_is_proven_but_interrupted_spawn_is_uncertain(self):
        for error in (OSError("log unavailable"), KeyboardInterrupt()):
            with tempfile.TemporaryDirectory(prefix="ha-") as directory:
                root = Path(directory)
                token = "a" * 64
                open_file = os.open
                def open_log(path, *args, **kwargs):
                    if str(path).endswith("stop.log"): raise error
                    return open_file(path, *args, **kwargs)
                with mock.patch.object(h, "preflight"), mock.patch.object(h.os, "open", side_effect=open_log):
                    with self.assertRaises(type(error)): h.reserve_stop(root, "guest", token)
                failure = root / "drains" / token / "stop-spawn-failed.json"
                self.assertEqual(failure.exists(), isinstance(error, OSError))
                if not failure.exists():
                    with mock.patch.object(h, "preflight"), mock.patch.object(h.subprocess, "Popen") as spawn:
                        h.reserve_stop(root, "guest", token)
                        spawn.assert_not_called()

    def test_hosted_disconnected_vmf2_migration_preserves_all_old_evidence(self):
        with tempfile.TemporaryDirectory(prefix="ha-") as directory:
            canonical = Path(directory) / "vm-fold"
            canonical.mkdir(mode=0o700)
            old = canonical.with_name("vmf2"); old.mkdir(mode=0o700)
            for name, value in dict(owner="private", vm="guest", **{
                    "owner.pid":"99", "tart.pid":"100", "tunnel.pid":"101", "holder.pid":"102"}).items():
                (old / name).write_text(value)
            listener = socket.socket(socket.AF_UNIX)
            self.addCleanup(listener.close)
            listener.bind(str(old / "control.sock"))
            (old / "control.sock").chmod(0o700)
            before = {p.name:p.read_bytes() for p in old.iterdir() if p.is_file()}
            def probe(argv, **kwargs):
                if argv[0] == "/usr/sbin/lsof":
                    return mock.Mock(returncode=0, stdout=("p99\nn" + str(old / "control.sock") + "\n").encode(), stderr=b"")
                self.assertEqual(argv[-1], "ppid=,state=,comm=")
                return mock.Mock(returncode=1, stdout=b"", stderr=b"")
            with mock.patch.object(h.session, "route_state", return_value=old), \
                    mock.patch.object(h.session, "legacy_disconnected"), \
                    mock.patch.object(h.session, "process_identity", return_value=("uid", ["stable"],
                        "/python /source/vnc-session.py serve " + str(old) + " wrapper guest /python")), \
                    mock.patch.object(h.session, "request", return_value=dict(connected=False)) as owner, \
                    mock.patch.object(h.subprocess, "run", side_effect=probe), \
                    mock.patch.object(h, "rpc", return_value=dict(vacant=True)):
                h.migration(canonical, "guest", "wrapper")
                self.assertEqual(owner.call_count, 2)
                self.assertEqual(before, {p.name:p.read_bytes() for p in old.iterdir() if p.is_file()})
                def zombies(argv, **kwargs):
                    if argv[0] == "/usr/sbin/lsof" or argv[2] == "100":
                        return probe(argv, **kwargs)
                    return mock.Mock(returncode=0, stdout=b"  99 Z (ssh)\n", stderr=b"")
                with mock.patch.object(h.subprocess, "run", side_effect=zombies):
                    h.migration(canonical, "guest", "wrapper")
                for output in (b"98 Z (ssh)\n", b"99 S ssh\n", b"99 R ssh\n", b"99 Z\n", b"99 Z ssh\nextra"):
                    def unsafe(argv, **kwargs):
                        if argv[0] == "/usr/sbin/lsof": return probe(argv, **kwargs)
                        return mock.Mock(returncode=0, stdout=output, stderr=b"")
                    with self.subTest(output=output), mock.patch.object(h.subprocess, "run", side_effect=unsafe):
                        with self.assertRaises(RuntimeError): h.migration(canonical, "guest", "wrapper")
                samples = iter([b"99 Z (ssh)\n", b"99 Z (holder)\n", b"99 Z changed\n", b"99 Z (holder)\n"])
                def changing(argv, **kwargs):
                    if argv[0] == "/usr/sbin/lsof" or argv[2] == "100": return probe(argv, **kwargs)
                    return mock.Mock(returncode=0, stdout=next(samples), stderr=b"")
                with mock.patch.object(h.subprocess, "run", side_effect=changing):
                    with self.assertRaises(RuntimeError): h.migration(canonical, "guest", "wrapper")
                with mock.patch.object(h.session, "process_identity", side_effect=ValueError()):
                    with self.assertRaises(ValueError): h.migration(canonical, "guest", "wrapper")
                with mock.patch.object(h.subprocess, "run", return_value=mock.Mock(returncode=1, stdout=b"", stderr=b"")):
                    with self.assertRaises(RuntimeError): h.migration(canonical, "guest", "wrapper")
                for name in ("shutdown-pending", "drain-token", "drain-precommit", "recovery-pending", "sim-posture-pending"):
                    (old / name).touch()
                    with self.assertRaises(RuntimeError): h.migration(canonical, "guest", "wrapper")
                    (old / name).unlink()
                owner.side_effect = TimeoutError()
                with self.assertRaises(TimeoutError): h.migration(canonical, "guest", "wrapper")
                owner.side_effect = None; owner.return_value = dict(connected=True)
                with self.assertRaises(RuntimeError): h.migration(canonical, "guest", "wrapper")

    def routing_fixture(self, *, legacy=False):
        spec = importlib.util.spec_from_file_location("legacy_fixture", ROOT / "tools/tests/vnc_session_test.py")
        fixtures = importlib.util.module_from_spec(spec); spec.loader.exec_module(fixtures)
        fixture = fixtures.SessionTests(methodName="runTest")
        fixture.setUp()
        self.addCleanup(fixture.doCleanups)
        if legacy:
            fixture.call('prepare')
        for name in h.FILES:
            target = fixture.root / "tools/vm" / name
            if not target.exists(): shutil.copy(ROOT / "tools/vm" / name, target)
        helper = fixture.root / "tools/vm/vnc-session.py"
        helper.write_text(helper.read_text().replace(
            'def route_state(canonical, vm, tart=None):\n    tart = None\n',
            'def route_state(canonical, vm, tart=None):\n').replace(
            '    token = metadata_text(state / "owner")\n',
            '    if (Path(os.environ["FAKE_ROOT"]) / "legacy-timeout").exists():\n'
            '        raise OSError("synthetic transient legacy timeout")\n'
            '    token = metadata_text(state / "owner")\n'))
        # Replace only the SSH wire boundary, keeping the production shell,
        # scheduler, pool guard, shutdown fencing, routing and cleanup intact.
        hosted = fixture.root / "tools/vm/vnc-hosted.py"
        wire = r'''def rpc(tart, message, *, install=False):
    root = Path(os.environ['FAKE_ROOT'])
    op = message['op']
    online = root / 'resident-online'
    if op == 'preflight':
        if (root / 'preflight-fail').exists(): raise RuntimeError()
        return dict(ready=True, digest=digest({n: Path(__file__).with_name(n).read_text() for n in FILES}))
    if op == 'status':
        if (root / 'resident-unknown').exists(): raise RuntimeError()
        if (root / 'resident-reserved').exists():
            return dict(vm='guest', connected=False, vacant=False, phase='stopping')
        return dict(vm='guest', connected=online.exists(), vacant=not online.exists(),
                    phase='connected' if online.exists() else 'vacant', session='d'*64,
                    drain=online.read_text() if online.exists() else None)
    if op == 'drained-stop':
        with (root / 'one-stop').open('x') as output: output.write(message['drain'])
        with (root / 'wire-stop.log').open('wb') as log:
            result = subprocess.run([str(root/'bin/tart'), 'stop', 'guest', '--timeout', '60'], stdout=log, stderr=log)
        require(result.returncode == 0 and (root/'vm-state').read_text() == 'stopped')
        (root / 'stop-joined').touch()
        return dict(reserved=True)
    if op == 'stop-receipt':
        require((root / 'one-stop').read_text() == message['drain'])
        return dict(receipt=dict(joined=True, stopped=True, code=0) if (root/'stop-joined').exists() else None)
    if op == 'start':
        require((root/'one-stop').read_text() == message['drain'])
        if not online.exists():
            with (root/'wire-start.log').open('wb') as log:
                subprocess.Popen([str(root/'bin/tart'), 'run', '--no-graphics', '--vnc-experimental', 'guest'],
                    stdin=subprocess.DEVNULL, stdout=log, stderr=log, start_new_session=True)
            session.wait_until(lambda: (root/'vm-state').read_text() == 'running', 3, 'fixture start')
            online.write_text(message['drain'])
        return dict(connected=True)
    raise RuntimeError()

'''
        content = hosted.read_text().replace('def rpc(tart, message, *, install=False):',
                                            'def unused_ssh_rpc(tart, message, *, install=False):')
        hosted.write_text(content.replace('def deployed(tart, vm, install=False):', wire + 'def deployed(tart, vm, install=False):'))
        return fixture

    def test_cancel_before_hosted_start_publication_cannot_publish_after_pause(self):
        fixture = self.routing_fixture()
        fixture.cancel_at_start(fixture.root / "tools/vm/vnc-hosted.py",
                                "        require((root/'one-stop').read_text() == message['drain'])\n",
                                published=False)
        self.assertFalse((fixture.root / "resident-online").exists())
        self.assertEqual((fixture.root / "vm-state").read_text(), "stopped")

    def test_cancel_after_hosted_start_reconciles_without_second_start_or_stop(self):
        fixture = self.routing_fixture()
        fixture.cancel_at_start(fixture.root / "tools/vm/vnc-hosted.py",
                                '        return dict(connected=True)\n', published=True)
        self.assertTrue((fixture.root / "resident-online").exists())
        fixture.call("prepare")
        self.assertEqual(fixture.trace().count("stop guest"), 1)
        self.assertEqual(fixture.trace().count("--vnc-experimental"), 1)
        self.assertFalse((fixture.root / "build/vmh3/recovery-pending").exists())

    def test_hosted_route_recovers_legacy_after_transient_prepare_failure_without_restart(self):
        fixture = self.routing_fixture(legacy=True)
        trace = fixture.trace()
        (fixture.root / 'legacy-timeout').touch()
        fixture.call('prepare', 1)
        state = fixture.root / 'build/vmh3'
        self.assertEqual({p.name for p in state.iterdir()}, {'vm', 'hosted.json'})
        metadata = {p.name:p.read_bytes() for p in state.iterdir()}
        self.assertTrue(json.loads(fixture.call('status').stdout)['hosted'])
        (fixture.root / 'legacy-timeout').unlink()
        fixture.call('prepare')
        status = json.loads(fixture.call('status').stdout)
        self.assertTrue(status['connected'] and status['conclusive'])
        self.assertNotIn('hosted', status)
        # The same healthy legacy endpoint cannot override either side's fences,
        # a reserved M3 stop, or unavailable resident evidence.
        for root in (state, fixture.state):
            for name in ('shutdown-pending', 'recovery-pending', 'drain-token',
                         'drain-precommit', 'sim-posture-pending'):
                with self.subTest(root=root.name, fence=name):
                    marker = root / name
                    marker.symlink_to(root / 'absent-fence-target')
                    self.assertTrue(json.loads(fixture.call('status').stdout)['hosted'])
                    marker.unlink()
        for name in ('resident-reserved', 'resident-unknown'):
            with self.subTest(remote=name):
                marker = fixture.root / name
                marker.touch()
                fixture.call('prepare', 1)
                self.assertTrue(json.loads(fixture.call('status').stdout)['hosted'])
                marker.unlink()
        (fixture.root / 'resident-online').write_text('d' * 64)
        status = json.loads(fixture.call('status').stdout)
        self.assertTrue(status['hosted'] and status['connected'])
        (fixture.root / 'resident-online').unlink()
        self.assertEqual(fixture.trace(), trace)
        self.assertEqual({p.name:p.read_bytes() for p in state.iterdir()}, metadata)

    def test_hosted_prepare_drains_once_reuses_and_refuses_active_or_unknown_leases(self):
        fixture = self.routing_fixture()
        (fixture.root / 'preflight-fail').touch()
        fixture.call('prepare', 1)
        self.assertEqual(fixture.trace(), '')
        self.assertFalse((fixture.root/'pool/pool.lock').exists())
        (fixture.root / 'preflight-fail').unlink()
        lease = fixture.root / 'pool/leases/orphan/lease.json'
        lease.parent.mkdir(parents=True)
        for data in ({'expires_epoch':time.time()+600}, {}, {'expires_epoch':'unknown'}):
            lease.write_text(json.dumps(data))
            fixture.call('prepare', 1)
            self.assertNotIn('stop guest', fixture.trace())
            self.assertEqual(json.loads(lease.read_text()), data)
            self.assertFalse((fixture.root/'pool/pool.lock').exists())
        lease.write_text(json.dumps({'expires_epoch':0}))
        fixture.call('prepare')
        state = fixture.root / 'build/vmh3'
        self.assertFalse((state/'recovery-pending').exists())
        self.assertFalse((fixture.root/'pool/pool.lock').exists())
        self.assertFalse(fixture.lease.exists())
        trace = fixture.trace()
        self.assertEqual(trace.count('stop guest --timeout 60'), 1)
        self.assertEqual(trace.count('run --no-graphics --vnc-experimental guest'), 1)
        self.assertNotIn('run --no-graphics guest', trace)
        self.assertFalse((fixture.root/'tunnel-args').exists())
        fixture.call('prepare')
        status = json.loads(fixture.call('status').stdout)
        self.assertTrue(status['hosted'] and status['connected'] and status['conclusive'])
        self.assertEqual(fixture.trace(), trace)


class HostedCommissioningTests(unittest.TestCase):
    def bootstrap(self, runtime, *, broken=False, packages=True):
        root = runtime / h.VERSION
        state = dict(usable=not broken, packages=packages)
        calls = []
        def run(argv, **kwargs):
            calls.append(argv)
            if argv[1:3] == ["-m", "venv"]:
                (root / "venv/bin").mkdir(parents=True, mode=0o700)
                (root / "venv/bin/python").write_text("synthetic interpreter")
                state.update(usable=True, packages=False)
            elif argv[-2:] == ["pip", "--version"]:
                if not (root / "venv/bin/python").exists(): raise FileNotFoundError()
                return mock.Mock(returncode=0 if state["usable"] else 1)
            elif "install" in argv:
                state["packages"] = True
            elif "-c" in argv:
                return mock.Mock(returncode=0 if state["packages"] else 1)
            else:
                self.assertIn("rpc", argv)
                self.assertEqual(json.loads(kwargs["input"]), dict(op="preflight", vm="guest"))
            return mock.Mock(returncode=0, stdout=b'{"ok":true}')
        payload = dict(files={name:"new component" for name in h.FILES}, vm="guest")
        namespace = {}
        previous_umask = os.umask(0o077)
        output = io.TextIOWrapper(io.BytesIO())
        try:
            with mock.patch.object(sys, "argv", ["bootstrap", str(runtime)]), \
                    mock.patch.object(sys, "stdin", types.SimpleNamespace(buffer=io.BytesIO(json.dumps(payload).encode()))), \
                    mock.patch.object(sys, "stdout", output), mock.patch.object(subprocess, "run", side_effect=run):
                try:
                    exec(compile(h.BOOTSTRAP, "bootstrap", "exec"), namespace)
                    result = 0
                except SystemExit as error:
                    result = error.code
        finally:
            if "fd" in namespace: os.close(namespace["fd"])
            os.umask(previous_umask)
        return result, calls

    def test_hosted_bootstrap_repairs_partial_components_and_incomplete_venv(self):
        for broken in (False, True):
            with self.subTest(broken=broken), tempfile.TemporaryDirectory(prefix="hc-") as directory:
                runtime = Path(directory)
                root = runtime / h.VERSION
                root.mkdir(mode=0o700)
                (root / h.FILES[0]).write_text("partial")
                (root / h.FILES[0]).chmod(0o600)
                (root / "venv").mkdir(mode=0o700)
                if broken:
                    (root / "venv/bin").mkdir(mode=0o700)
                    (root / "venv/bin/python").write_text("incomplete pip")
                # Missing interpreter and broken pip both rebuild while vacant.
                code, calls = self.bootstrap(runtime, broken=broken)
                self.assertEqual(code, 0)
                self.assertTrue(any(argv[1:3] == ["-m", "venv"] for argv in calls))
                self.assertTrue(any("install" in argv for argv in calls))
                for name in h.FILES:
                    self.assertEqual((root / name).read_text(), "new component")
                    self.assertEqual((root / name).stat().st_mode & 0o777, 0o600)
                # Identical ready deployment neither rebuilds nor installs.
                code, calls = self.bootstrap(runtime)
                self.assertEqual(code, 0)
                self.assertFalse(any("install" in argv or argv[1:3] == ["-m", "venv"] for argv in calls))

    def test_hosted_bootstrap_protects_every_prior_runtime_reservation(self):
        for evidence in ("launch.json", "drains", "state/owner", "operations/ticket"):
            with self.subTest(evidence=evidence), tempfile.TemporaryDirectory(prefix="hc-") as directory:
                runtime = Path(directory)
                root = runtime / h.VERSION
                root.mkdir(mode=0o700)
                path = root / evidence
                path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
                path.write_text("private evidence")
                code, calls = self.bootstrap(runtime)
                self.assertEqual(code, 1)
                self.assertEqual(calls, [])
                self.assertEqual(path.read_text(), "private evidence")
                self.assertFalse((root / h.FILES[0]).exists())

    def test_hosted_bootstrap_refuses_venv_repair_after_launch_even_with_matching_code(self):
        with tempfile.TemporaryDirectory(prefix="hc-") as directory:
            runtime = Path(directory)
            self.assertEqual(self.bootstrap(runtime)[0], 0)
            # A functional interpreter/pip can repair only the packages.
            code, calls = self.bootstrap(runtime, packages=False)
            self.assertEqual(code, 0)
            self.assertTrue(any("install" in argv for argv in calls))
            self.assertFalse(any(argv[1:3] == ["-m", "venv"] for argv in calls))
            root = runtime / h.VERSION
            (root / "launch.json").write_text("private")
            for broken, packages in ((True, True), (False, False)):
                code, calls = self.bootstrap(runtime, broken=broken, packages=packages)
                self.assertEqual(code, 1)
                self.assertFalse(any("install" in argv or argv[1:3] == ["-m", "venv"] for argv in calls))

    def test_hosted_preflight_exercises_sidecar_storage_socket_and_inventory_before_stop(self):
        with tempfile.TemporaryDirectory(prefix="hc-") as directory:
            runtime = Path(directory)
            root = runtime / h.VERSION
            root.mkdir(mode=0o700)
            for name in h.FILES: (root / name).write_text("component")
            tart = runtime / "bin/tart"
            tart.parent.mkdir()
            tart.write_text("#!" + sys.executable + "\nimport json, sys\n"
                            "assert sys.argv[1:] == ['list', '--format', 'json']\n"
                            "print(json.dumps([{'Name':'guest', 'State':'running'}]))\n")
            tart.chmod(0o700)
            with mock.patch.dict(sys.modules, {"vncdotool":types.SimpleNamespace(api=object())}), \
                    mock.patch("importlib.metadata.version", side_effect=lambda name: "1.4.2" if name == "vncdotool" else "11.3.0"):
                self.assertTrue(h.preflight(root, "guest")["ready"])
                self.assertEqual(list((root / "state").iterdir()), [])
                self.assertEqual(list((root / "operations").iterdir()), [])
                with mock.patch.object(h, "sibling", side_effect=ImportError()), mock.patch.object(h.subprocess, "Popen") as spawn:
                    with self.assertRaises(ImportError): h.reserve_stop(root, "guest", "a" * 64)
                    spawn.assert_not_called()
                for folder in ("state", "operations"):
                    # A private regular file is invalid operation storage.
                    (root / folder).rmdir(); (root / folder).write_text("foreign")
                    with self.assertRaises(RuntimeError): h.preflight(root, "guest")
                    (root / folder).unlink(); (root / folder).mkdir(mode=0o700)
                mkstemp = h.tempfile.mkstemp
                for folder in ("state", "operations"):
                    def unwritable(*args, **kwargs):
                        if Path(kwargs["dir"]).parent.name == folder: raise PermissionError()
                        return mkstemp(*args, **kwargs)
                    with self.subTest(unwritable=folder), mock.patch.object(h.tempfile, "mkstemp", side_effect=unwritable):
                        with self.assertRaises(PermissionError): h.preflight(root, "guest")
                    self.assertEqual(list((root / folder).iterdir()), [])
                for inventory in (b'{}', b'[]', b'[{"Name":"guest","State":"paused"}]',
                                  b'[{"name":"guest","state":"running"}]',
                                  b'[{"Name":"guest","State":"running"},{"Name":"guest","State":"stopped"}]'):
                    with self.subTest(inventory=inventory), mock.patch.object(h.subprocess, "run", return_value=mock.Mock(returncode=0, stdout=inventory)), \
                            mock.patch.object(h.subprocess, "Popen") as spawn:
                        with self.assertRaises(RuntimeError): h.reserve_stop(root, "guest", "a" * 64)
                        spawn.assert_not_called()
                        self.assertFalse((root / "drains").exists())
                with mock.patch.object(h.socket.socket, "listen", side_effect=OSError()):
                    with self.assertRaises(OSError): h.preflight(root, "guest")
                self.assertEqual(list((root / "state").iterdir()), [])

    def test_hosted_publication_requires_full_ticket_plus_reservation_round_trip(self):
        with tempfile.TemporaryDirectory(prefix="hc-") as directory:
            root = Path(directory)
            guest = types.SimpleNamespace(request={"guard":"c" * 32})
            with mock.patch.object(h, "endpoint", return_value=("wrapper", "guest")), \
                    mock.patch.object(h, "rpc", return_value=dict(connected=True, session="a" * 64)):
                holder = h.RemoteHolder(root, lambda:True, guest, root)
            for latest, allowed in ((107.1, True), (107, False), (106, False)):
                with self.subTest(latest=latest), mock.patch.object(holder, "operation", return_value=dict(reserved=True)) as operation, \
                        mock.patch.object(h.time, "monotonic", side_effect=[100, 101]):
                    if allowed:
                        self.assertEqual(holder.publication(None, latest, root / "receipt"), (105, b"remote-click"))
                    else:
                        with self.assertRaises(RuntimeError): holder.publication(None, latest, root / "receipt")
                    self.assertEqual(operation.call_args.args, ("reserve",))


if __name__ == "__main__":
    unittest.main()
