"""M3-resident VNC owner. SSH carries bounded typed RPC, never an RFB forward.

The M5 owns all scheduling/guest guards. This service only owns the one local
Tart child, holder, captures and at-most-once click receipts in its private kit.
"""
import base64
import contextlib
import fcntl
import hashlib
import importlib.util
import io
import json
import math
import os
from pathlib import Path
import re
import secrets
import shlex
import stat
import socket
import tempfile
import subprocess
import sys
import time

VERSION = "vnc-owner-v1"
FILES = ("vnc-hosted.py", "vnc-session.py", "vnc-holder.py", "sim-posture.py",
         "sim-posture-inspect.py", "remote-wrapper.py")
PACKAGES = ("vncdotool==1.4.2", "Pillow==11.3.0")
LIMIT = 24 * 1024 * 1024


def sibling(name):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(name + ".py"))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


session = sibling("vnc-session")
wrapper = sibling("remote-wrapper")


def require(ok):
    if not ok:
        raise RuntimeError("hosted VNC evidence unavailable; retain private state")


def identifier(value):
    require(type(value) is str and re.fullmatch(r"[0-9a-f]{64}", value) is not None)
    return value


def vm_name(vm):
    require(type(vm) is str and re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_.-]{0,79}", vm) is not None)
    return vm


# Bootstrap is the sole install entry point. No archive extraction, supplied
# path, shell instruction, system settings or user credential is accepted.
BOOTSTRAP = r'''
import hashlib, json, os, pathlib, stat, subprocess, sys
os.umask(0o077)
runtime = pathlib.Path(sys.argv[1])
files = ("vnc-hosted.py", "vnc-session.py", "vnc-holder.py", "sim-posture.py", "sim-posture-inspect.py", "remote-wrapper.py")
def owned(path, directory=False):
    info = path.lstat()
    if (info.st_uid != os.getuid() or info.st_mode & 0o077
            or not (stat.S_ISDIR(info.st_mode) if directory else stat.S_ISREG(info.st_mode))
            or (not directory and info.st_nlink != 1)):
        raise ValueError()
def ancestors(path):
    for parent in (*reversed(path.parents), path):
        info = parent.lstat()
        if (not stat.S_ISDIR(info.st_mode) or info.st_uid not in (0, os.getuid())
                or (info.st_mode & 0o022 and not info.st_mode & stat.S_ISVTX)):
            raise ValueError()
try:
    ancestors(runtime)
    root = runtime / "vnc-owner-v1"
    root.mkdir(mode=0o700, exist_ok=True)
    owned(root, True)
    # Same permanent lock as preflight/start. Never overwrite running code.
    import fcntl
    fd = os.open(root / "install.lock", os.O_CREAT | os.O_RDWR | os.O_NOFOLLOW, 0o600)
    owned(root / "install.lock")
    fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
    data = json.loads(sys.stdin.buffer.read(2097153))
    if set(data) != {"files", "vm"}: raise ValueError()
    import re, shutil, tempfile
    vm = data["vm"]
    if type(vm) is not str or re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_.-]{0,79}", vm) is None: raise ValueError()
    data = data["files"]
    if set(data) != set(files): raise ValueError()
    def vacant():
        # Commissioning only. No prior drain, launch or operation can be bypassed.
        for name in ("launch.json", "drains", "owner.log", "owner-spawn-failed.json"):
            if os.path.lexists(root / name): raise ValueError()
        for name in ("state", "operations"):
            path = root / name
            if os.path.lexists(path):
                owned(path, True)
                if list(path.iterdir()): raise ValueError()
    replacements = {}
    for name in files:
        payload = data[name].encode()
        if len(payload) > 524288: raise ValueError()
        target = root / name
        if os.path.lexists(target):
            owned(target)
            if target.read_bytes() == payload: continue
        replacements[name] = payload
    if replacements: vacant()
    for name, payload in replacements.items():
        out, temporary = tempfile.mkstemp(prefix=".component-", dir=root)
        try:
            with os.fdopen(out, "wb") as stream:
                stream.write(payload)
                stream.flush()
                os.fsync(stream.fileno())
            os.replace(temporary, root / name)
        finally:
            if os.path.lexists(temporary): os.unlink(temporary)
    directory_fd = os.open(root, os.O_RDONLY)
    try: os.fsync(directory_fd)
    finally: os.close(directory_fd)
    venv = root / "venv"
    logfd = os.open(root / "install.log", os.O_WRONLY | os.O_APPEND | os.O_CREAT | os.O_NOFOLLOW, 0o600)
    owned(root / "install.log")
    with os.fdopen(logfd, "ab") as log:
        def probe(arguments):
            try:
                return subprocess.run([str(venv / "bin/python"), "-I", *arguments], stdout=log, stderr=log, timeout=20).returncode == 0
            except OSError:
                return False
        if os.path.lexists(venv): owned(venv, True)
        if not probe(["-m", "pip", "--version"]):
            vacant()
            if os.path.lexists(venv): shutil.rmtree(venv)
            subprocess.run([sys.executable, "-m", "venv", str(venv)], check=True, stdout=log, stderr=log, timeout=120)
            owned(venv, True)
            if not probe(["-m", "pip", "--version"]): raise ValueError()
        versions = "from importlib.metadata import version; assert version('vncdotool') == '1.4.2' and version('Pillow') == '11.3.0'; from vncdotool import api; from PIL import Image"
        if not probe(["-c", versions]):
            vacant()
            subprocess.run([str(venv / "bin/python"), "-I", "-m", "pip", "--isolated", "--disable-pip-version-check", "install", "vncdotool==1.4.2", "Pillow==11.3.0"], check=True, stdout=log, stderr=log, timeout=240)
            if not probe(["-c", versions]): raise ValueError()
    owned(venv, True)
    fcntl.flock(fd, fcntl.LOCK_UN)
    check = subprocess.run([str(venv / "bin/python"), "-I", str(root / "vnc-hosted.py"), "rpc", str(runtime)],
        input=json.dumps({"op":"preflight", "vm":vm}).encode(), stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, timeout=20)
    if check.returncode: raise ValueError()
    sys.stdout.buffer.write(check.stdout)
except Exception:
    print('{"ok":false}')
    sys.exit(1)
'''


def rpc(tart, message, *, install=False):
    """Every call can be replaced by a fresh SSH connection. No automatic replay."""
    try:
        host, runtime = wrapper.configuration(tart)
        root = runtime + "/" + VERSION
        argv = (["/usr/bin/python3", "-c", BOOTSTRAP, runtime] if install else
                [root + "/venv/bin/python", "-I", root + "/vnc-hosted.py", "rpc", runtime])
        operation = message.get("operation", {}).get("op") if not install else None
        timeout = 380 if install else 145 if operation in ("capture", "move-capture") else 18
        result = subprocess.run([*wrapper.ssh_arguments(host, dedicated=True), shlex.join(argv)],
            input=json.dumps(message).encode(), stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL, timeout=timeout)
        require(result.returncode == 0 and len(result.stdout) <= 2 * LIMIT)
        reply = json.loads(result.stdout)
        require(type(reply) is dict and reply.pop("ok", None) is True)
        return reply
    except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError):
        raise RuntimeError("hosted VNC RPC unconfirmed; query existing receipts, never resend input") from None


def deployed(tart, vm, install=False):
    sources = {name: Path(__file__).with_name(name).read_text() for name in FILES}
    reply = rpc(tart, dict(files=sources, vm=vm) if install else {"op": "preflight", "vm": vm}, install=install)
    require(reply == {"ready": True, "digest": digest(sources)})


def digest(sources):
    return hashlib.sha256(json.dumps(sources, sort_keys=True).encode()).hexdigest()


def endpoint(state, tart=None, vm=None):
    if os.path.lexists(state / "hosted.json"):
        record = json.loads(session.metadata_text(state / "hosted.json"))
        require(set(record) == {"tart", "vm", "endpoint"})
        if tart is not None:
            require(str(tart) == record["tart"] and vm == record["vm"])
        tart, vm = record["tart"], record["vm"]
        require(record["endpoint"] == list(wrapper.configuration(tart)))
    return str(tart), vm_name(vm)


def capture_into(tart, vm, sid, folder, number, point=None):
    operation = dict(op="capture" if point is None else "move-capture", session=sid)
    if point is not None:
        operation["point"] = point
    result = rpc(tart, dict(op="operation", vm=vm, operation=operation))
    pixels = base64.b64decode(result["png"], validate=True)
    require(len(pixels) <= LIMIT)
    path = folder / ("remote-frame-%d.png" % number)
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
    with os.fdopen(fd, "wb") as output:
        output.write(pixels)
    return path


def hosted_status(state, vm, tart):
    try:
        tart, vm = endpoint(state, tart, vm)
        reply = rpc(tart, {"op": "status", "vm": vm_name(vm)})
        require(type(reply["connected"]) is bool and type(reply["vacant"]) is bool
                and reply["vm"] == vm and reply["phase"] in
                ("vacant", "stopping", "stopped", "stop-failed", "stop-spawn-failed", "start-spawn-failed", "starting", "connected", "disconnected")
                and reply["connected"] == (reply["phase"] == "connected")
                and reply["vacant"] == (reply["phase"] == "vacant"))
        return dict(vm=reply["vm"], connected=reply["connected"], remote=True,
                    hosted=True, conclusive=True, vacant=reply["vacant"], phase=reply["phase"])
    except (RuntimeError, ValueError, KeyError, TypeError):
        return dict(vm=vm, connected=False, remote=True, hosted=True,
                    conclusive=False, vacant=False, phase="unknown")


def migration(canonical, vm, tart):
    """Narrow admission for the actual abandoned vmf2 supervisor; never signal it.

    Authenticated *disconnected* is required twice, with pinned metadata. EOF,
    timeouts and PID absence alone grant nothing. The old canonical orphan stays
    subject to its existing strict OS checks. M3 vacancy is checked separately.
    """
    old = session.route_state(canonical, vm)
    if not any(os.path.lexists(old / n) for n in session.SESSION_EVIDENCE):
        return
    require(old == canonical.with_name("vmf2"))
    session.legacy_disconnected(canonical, vm)
    require(not any(os.path.lexists(old / n) for n in
        ("shutdown-pending", "recovery-pending", "drain-token", "drain-precommit", "sim-posture-pending")))
    with contextlib.ExitStack() as stack:
        records = {n: stack.enter_context(session.pinned_record(old / n))
                   for n in ("owner", "owner.pid", "vm", "tart.pid", "tunnel.pid", "holder.pid")}
        require(records["vm"][0] == vm)
        owner_pid = int(records["owner.pid"][0])
        require(owner_pid > 1)
        socket_path = old / "control.sock"
        socket_info = socket_path.lstat()
        require(stat.S_ISSOCK(socket_info.st_mode) and socket_info.st_uid == os.getuid()
                and not socket_info.st_mode & 0o077)
        previous = None
        for observation in range(2):
            parent = session.process_identity(owner_pid)  # Live, same UID, start time.
            require(session.request(old, "status")["connected"] is False)
            # The authenticated endpoint must belong to this live supervisor.
            sockets = subprocess.run(["/usr/sbin/lsof", "-nP", "-a", "-p", str(owner_pid), "-U", "-Fn"],
                                     capture_output=True, timeout=3)
            require(sockets.returncode == 0 and not sockets.stderr
                    and ("n" + str(socket_path)) in sockets.stdout.decode().splitlines())
            children = []
            for name in ("tart.pid", "tunnel.pid", "holder.pid"):
                pid = int(records[name][0])
                require(pid > 1 and pid != owner_pid)
                probe = subprocess.run(["/bin/ps", "-p", str(pid), "-o", "ppid=,state=,comm="],
                                       capture_output=True, timeout=3)
                require(not probe.stderr)
                if probe.returncode == 1 and not probe.stdout:
                    children.append(None)  # Positive absence.
                else:
                    require(probe.returncode == 0)
                    fields = probe.stdout.decode().strip().split(None, 2)
                    require(len(fields) == 3 and fields[0] == str(owner_pid)
                            and re.fullmatch(r"Z[+<NLRsVXWE]*", fields[1]) is not None
                            and fields[2] and "\n" not in fields[2])
                    children.append(tuple(fields))  # Kernel zombie, never a live child.
            require(len({int(records[n][0]) for n in ("tart.pid", "tunnel.pid", "holder.pid")}) == 3)
            current = (parent, children)
            if previous is not None:
                require(current == previous)
            previous = current
            require(session.record_version(socket_path.lstat()) == session.record_version(socket_info))
            if observation == 0:
                time.sleep(.1)
        for name, (_, info) in records.items():
            session.unchanged_record(old / name, info)
    require(rpc(tart, {"op": "status", "vm": vm})["vacant"] is True)


def initialize(state, tart, vm):
    session.init_state(state)
    path = state / "hosted.json"
    value = dict(tart=str(tart), vm=vm, endpoint=list(wrapper.configuration(tart)))
    if os.path.lexists(path):
        require(json.loads(session.metadata_text(path)) == value)
    else:
        with path.open("x") as output:
            json.dump(value, output)
    session.primary(state, vm)
    if not (state / "vm").exists():
        (state / "vm").write_text(vm)


def prepare(state, tart, vm, canonical, phase):
    initialize(state, tart, vm)
    if phase == "admit":
        deployed(tart, vm)  # Must prove existing M3 code/venv before accepting drain.
        remote = rpc(tart, {"op": "status", "vm": vm})
        require(remote["vacant"] is True)
        migration(Path(canonical), vm, tart)
    elif phase == "start":
        # The remote side independently requires positively stopped Tart and
        # reserves ownership before detaching. An uncertain reply is queried.
        token = identifier(session.metadata_text(state / "recovery-pending"))
        try:
            rpc(tart, {"op": "start", "vm": vm, "drain": token})
        except RuntimeError:
            pass  # Query authoritative reservation/readiness, never replay start.
        until = time.monotonic() + 100
        while time.monotonic() < until:
            try:
                current = rpc(tart, {"op": "status", "vm": vm})
                if current.get("connected") is True and current.get("drain") == token:
                    return
                if current.get("phase") == "start-spawn-failed":
                    break
            except RuntimeError:
                pass
            time.sleep(.5)
        raise RuntimeError("resident readiness unconfirmed; query status; no rollback")
    else:
        raise ValueError()


def cleanup(state, tart, vm, ssh, lease):
    """Only the authenticated resident session for this drain can retire it."""
    if not (state / "recovery-pending").exists():
        require(not any(os.path.lexists(state / n) for n in
                        ("drain-token", "shutdown-pending", "drain-precommit")))
        return
    token = identifier(session.metadata_text(state / "recovery-pending"))
    fence = session.ShutdownFence(lease, state, token)
    with session.pinned_record(state / "recovery-pending") as (_, identity), \
            session.pinned_record(Path(lease) / "holder.json") as (holder, holder_identity):
        metadata = json.loads(holder)
        require(metadata.get("holder") == "duo-fold" and metadata.get("job") == "vm-fold-prepare"
                and metadata.get("pid") == os.getppid())
        with fence.scheduler():
            require(set(Path(lease).iterdir()) == {Path(lease) / "holder.json"})
            session.unchanged_record(Path(lease) / "holder.json", holder_identity)
            require(not any(os.path.lexists(state / n) for n in ("shutdown-pending", "drain-precommit")))
        current = rpc(tart, {"op": "status", "vm": vm})
        require(current["connected"] is True and current["drain"] == token)
        if (state / "drain-token").exists():
            require(session.metadata_text(state / "drain-token") == token)
        session.clean_guest_drain(state, ssh, session.CLEAN_DRAIN, token)
        with fence.scheduler():
            require(set(Path(lease).iterdir()) == {Path(lease) / "holder.json"})
            session.unchanged_record(Path(lease) / "holder.json", holder_identity)
            session.unchanged_record(state / "recovery-pending", identity)
            if (state / "drain-token").exists():
                require(session.metadata_text(state / "drain-token") == token)
                (state / "drain-token").unlink()
            (state / "recovery-pending").unlink()


def drained_stop(tart, vm, token):
    # Query uncertain admission; retry only a durable, proven pre-spawn failure.
    try:
        rpc(tart, dict(op="drained-stop", vm=vm, drain=identifier(token)))
    except RuntimeError:
        pass
    deadline = time.monotonic() + 95
    while time.monotonic() < deadline:
        try:
            reply = rpc(tart, dict(op="stop-receipt", vm=vm, drain=token))
            if reply.get("spawn_failed") is True:
                # Proven pre-spawn only; resident independently checks this token
                # and removes the failure latch durably before another attempt.
                rpc(tart, dict(op="drained-stop", vm=vm, drain=token))
            if reply["receipt"] is not None:
                result = reply["receipt"]
                require(result == dict(joined=True, stopped=True, code=0))
                return 0
        except RuntimeError:
            pass
        time.sleep(.5)
    raise RuntimeError("resident stop receipt unconfirmed; scheduler/guest fences retained")


def query_receipt(state, tart, vm, lease):
    """Read the exact outstanding scheduler intent; never clear fences or click."""
    endpoint(state, tart, vm)
    record = json.loads(session.metadata_text(Path(lease) / "sim-posture-pending/intent.json"))
    require(set(record) == {"session", "request", "guard", "endpoint"}
            and record["endpoint"] == list(wrapper.configuration(tart)))
    result = rpc(tart, dict(op="operation", vm=vm, operation=dict(op="receipt",
        session=identifier(record["session"]), request=identifier(record["request"]), guard=record["guard"])))
    require(set(result) == {"session", "request", "guard", "receipt"}
            and all(result[k] == record[k] for k in ("session", "request", "guard")))
    receipt = result["receipt"]
    if receipt is not None:
        require(type(receipt) is dict and set(receipt) in
                ({"joined", "executed"}, {"joined", "executed", "error"})
                and receipt["joined"] is True and type(receipt["executed"]) is bool
                and ("error" not in receipt or receipt["error"] in ("expired", "input")))
    return dict(receipt=receipt)


def atomic_json(path, value):
    fd, temporary = tempfile.mkstemp(prefix=".record-", dir=path.parent)
    try:
        with os.fdopen(fd, "w") as output:
            json.dump(value, output)
            output.flush()
            os.fsync(output.fileno())
        os.replace(temporary, path)
        sync_directory(path.parent)
    finally:
        if os.path.lexists(temporary):
            os.unlink(temporary)


def sync_directory(path):
    fd = os.open(path, os.O_RDONLY)
    try:
        os.fsync(fd)
    finally:
        os.close(fd)


def spawn_failure(path, vm, token):
    if not os.path.lexists(path):
        return False
    require(json.loads(session.metadata_text(path)) == dict(vm=vm, drain=token))
    return True


def spawn_reserved(folder, kind, vm, token, argv):
    failure = folder / (kind + "-spawn-failed.json")
    if spawn_failure(failure, vm, token):
        failure.unlink()
        sync_directory(folder)  # Crash after here means uncertain, never retry.
    log = None
    try:
        # Earlier failed attempts may have opened the log, but created no peer.
        fd = os.open(folder / (kind + ".log"), os.O_WRONLY | os.O_APPEND | os.O_CREAT | os.O_NOFOLLOW | os.O_NONBLOCK, 0o600)
        info = os.fstat(fd)
        if not (stat.S_ISREG(info.st_mode) and info.st_uid == os.getuid()
                and not info.st_mode & 0o077 and info.st_nlink == 1):
            os.close(fd)
            raise OSError("unsafe spawn log")
        log = os.fdopen(fd, "ab")
        subprocess.Popen(argv, stdin=subprocess.DEVNULL, stdout=log, stderr=log,
                         cwd="/", close_fds=True, start_new_session=True)
    except OSError:
        # Popen's OSError proves exec did not succeed. All other interruptions,
        # including death before this durable write, keep the reservation fenced.
        atomic_json(failure, dict(vm=vm, drain=token))
        raise
    finally:
        if log is not None:
            log.close()


def resident_root(runtime):
    root = Path(runtime) / VERSION
    session.guard_state(root)
    require(root.is_dir())
    for name in FILES:
        info = (root / name).lstat()
        require(stat.S_ISREG(info.st_mode) and info.st_uid == os.getuid()
                and not info.st_mode & 0o077 and info.st_nlink == 1)
    return root


def preflight(root, vm):
    from importlib.metadata import version
    from vncdotool import api
    from PIL import Image
    require(version("vncdotool") == "1.4.2" and version("Pillow") == "11.3.0")
    require((root.parent / "bin/tart").is_file() and os.access(root.parent / "bin/tart", os.X_OK))
    require(not session.remote_host(str(root.parent / "bin/tart")))
    sibling("sim-posture")  # Load the actual first-operation dependency chain.
    require(len(os.fsencode(root / "state/control.sock")) < 104)
    for name in ("state", "operations"):
        folder = root / name
        session.init_state(folder)
        # Unique probes never adopt or unlink existing runtime evidence.
        with tempfile.TemporaryDirectory(prefix=".probe-", dir=folder) as directory:
            probe = Path(directory)
            atomic_json(probe / "record", {"probe": True})
            require(json.loads(session.metadata_text(probe / "record")) == {"probe": True})
        if name == "state":
            # No longer than control.sock; bind is exclusive, even on collision.
            path = folder / ("p" + secrets.token_hex(5))
            identity = None
            with socket.socket(socket.AF_UNIX) as listener:
                try:
                    listener.bind(str(path))
                    identity = session.file_identity(path.lstat())
                    listener.listen(1)
                finally:
                    if identity is not None:
                        session.unlink_owned(path, identity)
    tart_state(root, vm)
    return dict(ready=True, digest=digest({n: (root / n).read_text() for n in FILES}))


def tart_state(root, vm):
    result = subprocess.run([str(root.parent / "bin/tart"), "list", "--format", "json"],
        stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, timeout=10)
    require(result.returncode == 0 and len(result.stdout) <= 1024 * 1024)
    inventory = json.loads(result.stdout)
    require(type(inventory) is list and all(type(e) is dict for e in inventory))
    entries = [e for e in inventory if e.get("Name") == vm]
    require(len(entries) == 1 and entries[0].get("State") in ("running", "stopped"))
    return entries[0]["State"]


def require_empty_runtime(root):
    for name in ("state", "operations"):
        folder = root / name
        session.guard_state(folder)
        require(not folder.exists() or not list(folder.iterdir()))


def owner_status(root, vm):
    state = root / "state"
    session.guard_state(state)
    session.primary(state, vm)
    if os.path.lexists(root / "launch.json"):
        launch = json.loads(session.metadata_text(root / "launch.json"))
        require(set(launch) == {"vm", "drain"} and launch["vm"] == vm)
        identifier(launch["drain"])
        if not os.path.lexists(state / "owner"):
            failed = spawn_failure(root / "owner-spawn-failed.json", vm, launch["drain"])
            if failed:
                require_empty_runtime(root)
            return dict(vm=vm, connected=False, vacant=False,
                        phase="start-spawn-failed" if failed else "starting", drain=launch["drain"])
    elif any(os.path.lexists(state / n) for n in session.SESSION_EVIDENCE):
        raise RuntimeError("resident ownership unknown")
    else:
        drains = root / "drains"
        session.guard_state(drains)
        entries = list(drains.iterdir()) if drains.exists() else []
        if entries:
            require(len(entries) == 1)
            token = identifier(entries[0].name)
            receipt = stop_receipt(root, vm, token)
            if spawn_failure(entries[0] / "stop-spawn-failed.json", vm, token):
                phase = "stop-spawn-failed"
            else:
                phase = "stopping" if receipt is None else "stopped" if receipt == dict(
                    joined=True, stopped=True, code=0) else "stop-failed"
            return dict(vm=vm, connected=False, vacant=False, phase=phase, drain=token)
        require_empty_runtime(root)
        return dict(vm=vm, connected=False, vacant=True, phase="vacant")
    reply = session.request(state, "status")  # Missing/uncertain is never vacancy.
    return dict(vm=vm, connected=reply["connected"], vacant=False,
                phase="connected" if reply["connected"] else "disconnected", session=reply["session"],
                drain=launch["drain"])


def detached_start(root, vm, token):
    preflight(root, vm)
    current = owner_status(root, vm)
    if current["phase"] in ("connected", "starting", "disconnected"):
        return current  # Never spawn or reconnect in response to a retry.
    require(current["phase"] in ("stopped", "start-spawn-failed") and current["drain"] == token)
    require(tart_state(root, vm) == "stopped")
    require(stop_receipt(root, vm, token) == dict(joined=True, stopped=True, code=0))
    state = root / "state"
    session.init_state(state)
    require_empty_runtime(root)
    if current["phase"] == "stopped":
        # Exclusive reservation precedes spawn; uncertain launches never replay.
        with (root / "launch.json").open("x") as record:
            json.dump(dict(vm=vm, drain=token), record)
            record.flush()
            os.fsync(record.fileno())
        sync_directory(root)
    spawn_reserved(root, "owner", vm, token,
        [str(root / "venv/bin/python"), "-I", str(root / "vnc-hosted.py"),
         "serve", str(root.parent), vm])
    return dict(vm=vm, connected=False, vacant=False, phase="starting")


def stop_directory(root, vm, token):
    path = root / "drains" / identifier(token)
    session.guard_state(path)
    if path.exists():
        require(json.loads(session.metadata_text(path / "intent.json")) == dict(vm=vm, drain=token))
    return path


def stop_receipt(root, vm, token):
    path = stop_directory(root, vm, token) / "receipt.json"
    return json.loads(session.metadata_text(path)) if os.path.lexists(path) else None


def reserve_stop(root, vm, token):
    preflight(root, vm)
    path = stop_directory(root, vm, token)
    if path.exists():
        if not spawn_failure(path / "stop-spawn-failed.json", vm, token):
            return dict(reserved=True)  # Uncertain launch is never retried.
        require(not any(os.path.lexists(path / n) for n in ("executing", "receipt.json")))
        require(not os.path.lexists(root / "launch.json"))
        require_empty_runtime(root)
    else:
        require(owner_status(root, vm)["vacant"] is True)
        drains = root / "drains"
        session.init_state(drains)
        require(not list(drains.iterdir()))
        path.mkdir(mode=0o700)
        atomic_json(path / "intent.json", dict(vm=vm, drain=token))
        sync_directory(drains)
    spawn_reserved(path, "stop", vm, token,
        [str(root / "venv/bin/python"), "-I", str(root / "vnc-hosted.py"),
         "stop-peer", str(root.parent), vm, token])
    return dict(reserved=True)



def stop_peer(root, vm, token):
    path = stop_directory(root, vm, token)
    # Exclusive peer admission also survives an accidentally duplicated launcher.
    with (path / "executing").open("x") as record:
        record.write("stop reserved\n")
    result = subprocess.run([str(root.parent / "bin/tart"), "stop", vm, "--timeout", "60"],
                            stdin=subprocess.DEVNULL, timeout=75)
    receipt = dict(joined=True, code=result.returncode, stopped=tart_state(root, vm) == "stopped")
    temporary = path / "receipt.tmp"
    temporary.write_text(json.dumps(receipt))
    temporary.replace(path / "receipt.json")


class Operations:
    """Serialized on the existing supervisor, surviving the requesting SSH peer.

    Only locally generated paths reach Holder. A consumed ticket is never sent
    twice, even after a crash before its receipt. Queries cannot publish input.
    """
    def __init__(self, root):
        self.root = root
        self.holder = None
        self.active = None

    def __call__(self, message, connected, token):
        pose = sibling("sim-posture")
        sid = hashlib.sha256(token.encode()).hexdigest()
        require(type(message) is dict and message.get("session") == sid)
        op = message.get("op")
        expected = {"op", "session"} | ({"point"} if op == "move-capture" else
            {"request", "guard"} if op in ("reserve", "click", "receipt") else set())
        require(set(message) == expected)
        folder = self.root / "operations"
        session.init_state(folder)
        if self.holder is None:
            self.holder = pose.Holder(self.root / "state", connected, None, folder)
        if self.active is not None:
            active = folder / self.active
            # Abandoned unpublished reservations expire without emitting input.
            ticket_data = json.loads(session.metadata_text(active / "ticket.json"))
            receipt_path = active / "receipt.json"
            if (not os.path.lexists(active / "attempted") and time.monotonic() >= ticket_data["deadline"]):
                receipt_path.write_text('{"joined":true,"executed":false,"error":"expired"}')
            if self.holder.click_receipt(receipt_path) is not None:
                self.active = None
        def ticket():
            rid, guard = identifier(message["request"]), message["guard"]
            require(type(guard) is str and re.fullmatch(r"[0-9a-f]{32}", guard) is not None)
            target = folder / rid
            return rid, guard, target
        if op == "reserve":
            require(connected() and self.active is None)
            rid, guard, target = ticket()
            target.mkdir(mode=0o700)  # Unique request/session, exclusive admission.
            (target / "ticket.json").write_text(json.dumps(dict(
                session=sid, guard=guard, deadline=time.monotonic() + 5)))
            self.active = rid
            return dict(reserved=True)
        if op in ("click", "receipt"):
            rid, guard, target = ticket()
            data = json.loads(session.metadata_text(target / "ticket.json"))
            require(data["session"] == sid and data["guard"] == guard)
            receipt = target / "receipt.json"
            if op == "click":
                require(self.active == rid)
                # Latch BEFORE publication. Uncertain writes never get retried.
                with (target / "attempted").open("x") as latch:
                    latch.write("publication reserved\n")
                deadline = data["deadline"]
                require(type(deadline) in (int, float) and math.isfinite(deadline))
                if time.monotonic() >= deadline:
                    receipt.write_text('{"joined":true,"executed":false,"error":"expired"}')
                else:
                    fd = self.holder.open_publication()
                    try:
                        payload = ("posture-click " + repr(deadline) + " " + str(receipt) + "\n").encode()
                        require(len(payload) <= os.fpathconf(fd, "PC_PIPE_BUF") and payload.count(b"\n") == 1)
                        require(os.write(fd, payload) == len(payload))
                    finally:
                        os.close(fd)
            result = self.holder.click_receipt(receipt)
            if result is not None:
                require(type(result) is dict and result.get("joined") is True)
                if self.active == rid:
                    self.active = None
            return dict(request=rid, session=sid, guard=guard, receipt=result)
        require(op in ("capture", "move-capture") and self.active is None)
        command = None
        if op == "move-capture":
            point = message["point"]
            require(type(point) is list and len(point) == 2
                    and all(type(v) is int and 0 <= v <= 65535 for v in point))
            command = "move %d %d" % tuple(point)
        path = self.holder.batch(command)
        try:
            require(path.stat().st_size <= LIMIT)
            return dict(png=base64.b64encode(path.read_bytes()).decode())
        finally:
            path.unlink(missing_ok=True)


def resident_rpc(root, message):
    require(type(message) is dict)
    op = message.get("op")
    if op == "preflight":
        require(set(message) == {"op", "vm"})
        return preflight(root, vm_name(message["vm"]))
    vm = vm_name(message.get("vm"))
    require(set(message) == ({"op", "vm", "operation"} if op == "operation" else
                            {"op", "vm", "drain"} if op in ("start", "drained-stop", "stop-receipt") else {"op", "vm"}))
    if op == "status":
        return owner_status(root, vm)
    if op == "start":
        return detached_start(root, vm, identifier(message["drain"]))
    if op == "drained-stop":
        return reserve_stop(root, vm, identifier(message["drain"]))
    if op == "stop-receipt":
        token = identifier(message["drain"])
        return dict(receipt=stop_receipt(root, vm, token),
                    spawn_failed=spawn_failure(stop_directory(root, vm, token) / "stop-spawn-failed.json", vm, token))
    require(op == "operation")
    operation = message["operation"]
    require(type(operation) is dict and operation.get("op") in
            ("capture", "move-capture", "reserve", "click", "receipt"))
    require(owner_status(root, vm)["vacant"] is False)
    return {k: v for k, v in session.request(root / "state", operation,
            timeout=135 if operation["op"] in ("capture", "move-capture") else 10,
            max_bytes=2 * LIMIT).items() if k != "ok"}


class RemoteHolder:
    # Bind the common fencing/click implementation lazily to avoid a module cycle.
    def __new__(cls, state, connected, guest, folder, lease=None):
        pose = sibling("sim-posture")
        class Transport(pose.Holder):
            def __init__(self):
                self.state, self.connected, self.guest, self.folder = state, connected, guest, folder
                self.lease = lease
                self.lease_identity = None if lease is None else (lease.stat().st_dev, lease.stat().st_ino)
                self.armed = self.pending = False
                self.capable = True
                self.log = io.BytesIO()
                self.sequence = 0
                self.tart, self.vm = endpoint(state)
                status = rpc(self.tart, dict(op="status", vm=self.vm))
                require(status["connected"] is True)
                self.sid = identifier(status["session"])

            def operation(self, op, **values):
                return rpc(self.tart, dict(op="operation", vm=self.vm,
                    operation=dict(op=op, session=self.sid, **values)))

            def batch(self, command=None):
                require(command is None or re.fullmatch(r"move [0-9]+ [0-9]+", command))
                self.sequence += 1
                path = capture_into(self.tart, self.vm, self.sid, self.folder, self.sequence,
                                    None if command is None else [int(n) for n in command.split()[1:]])
                require(pose.inspection.complete_png(path))
                pose.image_size(path)
                return path

            def open_publication(self):
                return None

            def publication(self, fd, latest, receipt):
                # Deadline is sampled on M3 BEFORE publication, never translated
                # from an M5 monotonic timestamp. Network time only subtracts.
                start = time.monotonic()
                self.rid = secrets.token_hex(32)
                self.guard = self.guest.request["guard"]
                result = self.operation("reserve", request=self.rid, guard=self.guard)
                require(result == {"reserved": True})
                received = time.monotonic()
                # M3 sampled its five-second ticket before replying. Reserve an
                # entire ticket from receipt, plus the measured round trip.
                require(received + 5 + (received - start) < latest)
                return start + 5, b"remote-click"

            def click_intent(self, receipt, deadline):
                return dict(session=self.sid, request=self.rid, guard=self.guard,
                            endpoint=list(wrapper.configuration(self.tart)))

            def publish(self, fd, payload):
                try:
                    self.operation("click", request=self.rid, guard=self.guard)
                except RuntimeError:
                    # The RPC may have published. Only receipt queries follow.
                    pass
                return len(payload)

            def click_receipt(self, receipt):
                try:
                    result = self.operation("receipt", request=self.rid, guard=self.guard)
                except RuntimeError:
                    return None
                require(result.keys() == {"request", "session", "guard", "receipt"}
                        and result["request"] == self.rid and result["session"] == self.sid
                        and result["guard"] == self.guard)
                return result["receipt"]
        return Transport()


def main():
    os.umask(0o077)
    try:
        action, *args = sys.argv[1:]
        if action in ("rpc", "serve", "stop-peer"):
            root = resident_root(args[0])
            if action == "stop-peer":
                stop_peer(root, vm_name(args[1]), identifier(args[2]))
                return 0
            if action == "serve":
                vm = vm_name(args[1])
                session.serve(root / "state", str(root.parent / "bin/tart"), vm,
                              str(root / "venv/bin/python"), Operations(root))
                return 0
            data = sys.stdin.buffer.read(8193)
            require(len(data) <= 8192)
            # Serialize admission/start/install, but never retain a controller
            # channel or its FD in the detached owner.
            fd = os.open(root / "install.lock", os.O_CREAT | os.O_RDWR | os.O_NOFOLLOW | os.O_NONBLOCK, 0o600)
            with os.fdopen(fd, "r+") as lock:
                info = os.fstat(fd)
                require(stat.S_ISREG(info.st_mode) and info.st_uid == os.getuid()
                        and not info.st_mode & 0o077 and info.st_nlink == 1)
                fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
                reply = resident_rpc(root, json.loads(data))
            print(json.dumps(dict(ok=True, **reply)))
        elif action in ("deploy", "preflight"):
            deployed(args[0], vm_name(args[1]), install=action == "deploy")
            print(json.dumps(dict(ready=True, hosted=True)))
        elif action == "prepare":
            state, tart, vm, canonical, phase = args
            prepare(Path(state), tart, vm, canonical, phase)
        elif action == "bind":
            state, tart, vm = args
            initialize(Path(state), tart, vm_name(vm))
        elif action == "cleanup":
            state, tart, vm, ssh, lease = args
            cleanup(Path(state), tart, vm, ssh, lease)
        elif action == "receipt":
            state, tart, vm, lease = args
            print(json.dumps(query_receipt(Path(state), tart, vm, lease)))
        else:
            raise ValueError()
        return 0
    except (OSError, ValueError, KeyError, TypeError, RuntimeError, subprocess.SubprocessError):
        if sys.argv[1:2] == ["rpc"]:
            print('{"ok":false}')
        else:
            print("sim-posture: hosted operation unconfirmed; private evidence retained; do not repeat input or restart", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
