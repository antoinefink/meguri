"""Private transport for duo-fold.sh; never print Tart output or credentials."""
import contextlib
import fcntl
import hashlib
import json
import os
from pathlib import Path
import re
import shlex
import secrets
import select
import socket
import stat
import subprocess
import sys
import time


def remote_host(tart):
    config = Path(tart).resolve().with_name("remote.json")
    if not config.exists():
        return ""
    host = json.loads(config.read_text())["host"]
    if (not isinstance(host, str) or not host or host.startswith("-")
            or any(c.isspace() or ord(c) < 32 for c in host)):
        raise ValueError("invalid host in wrapper's adjacent remote.json")
    return host


def address(output):
    urls = re.findall(r"vnc://\S+", output)
    if not urls:
        return None
    # Tart 2.32.1 authenticated experimental mode prints this loopback URI.
    match = re.fullmatch(r"vnc://:([^\s/@]+)@127\.0\.0\.1:([0-9]+)", urls[-1])
    if not match or not 0 < int(match[2]) < 65536:
        raise ValueError("Tart did not print an authenticated 127.0.0.1 VNC URI")
    return match[1], int(match[2])


def request(state, action, timeout=15, max_bytes=65536):
    token = metadata_text(state / "owner")
    with socket.socket(socket.AF_UNIX) as channel:
        channel.settimeout(timeout)
        channel.connect(str(state / "control.sock"))
        channel.sendall((json.dumps([token, action]) + "\n").encode())
        with channel.makefile("r") as response:
            result = json.loads(response.readline(max_bytes))
    if not result["ok"]:
        raise RuntimeError("VNC ownership or remote run completion uncertain; evidence retained")
    return result


def process_identity(pid):
    """Read OS identity without signalling a PID or reading its environment."""
    result = subprocess.check_output(
        ["/bin/ps", "-ww", "-p", str(pid), "-o", "uid=,stat=,lstart=,command="],
        stderr=subprocess.DEVNULL, timeout=2).decode().strip().split(None, 7)
    if len(result) != 8 or int(result[0]) != os.getuid() or "Z" in result[1]:
        raise ValueError("legacy process identity unavailable")
    return result[0], result[2:7], result[7]


def metadata_text(path):
    # A malformed operator entry must not block status on a FIFO, follow a
    # replacement symlink or read an unbounded private log.
    fd = os.open(path, os.O_RDONLY | os.O_NONBLOCK | os.O_NOFOLLOW)
    with os.fdopen(fd, "rb") as source:
        info = os.fstat(source.fileno())
        if (not stat.S_ISREG(info.st_mode) or info.st_uid != os.getuid()
                or info.st_mode & 0o022 or info.st_nlink != 1):
            raise ValueError("session metadata unavailable")
        value = source.read(65537)
        if len(value) > 65536:
            raise ValueError("session metadata oversized")
        return value.decode()


def record_version(info):
    return (info.st_dev, info.st_ino, info.st_mode, info.st_uid, info.st_nlink,
            info.st_size, info.st_mtime_ns, info.st_ctime_ns)


@contextlib.contextmanager
def pinned_record(path):
    # Keep the inode alive through cleanup, including replacement checks.
    fd = os.open(path, os.O_RDONLY | os.O_NONBLOCK | os.O_NOFOLLOW)
    with os.fdopen(fd, "rb") as source:
        info = os.fstat(source.fileno())
        if (not stat.S_ISREG(info.st_mode) or info.st_uid != os.getuid()
                or info.st_mode & 0o022 or info.st_nlink != 1):
            raise RuntimeError("unsafe cleanup metadata; evidence retained")
        value = source.read(65537)
        if len(value) > 65536:
            raise RuntimeError("cleanup metadata oversized; evidence retained")
        yield value.decode(), info


def unchanged_record(path, info):
    if record_version(path.lstat()) != record_version(info):
        raise RuntimeError("cleanup metadata changed; evidence retained")


def primary(state, vm):
    if os.path.lexists(state / "vm") and metadata_text(state / "vm").strip() != vm:
        raise RuntimeError("global VNC state belongs to another VM; retain evidence for operator recovery")


def legacy_connected(state, vm, tart):
    """Read-only admission of the previous operator format; never adopt it.

    PID files alone prove nothing. Bind the private metadata to the live holder,
    its established loopback connection, and the matching SSH/Tart target.
    """
    if not tart or os.path.lexists(state / "control.sock"):
        return False
    directory = state.lstat()
    if (not stat.S_ISDIR(directory.st_mode) or directory.st_uid != os.getuid()
            or directory.st_mode & 0o077):
        return False
    names = ("vm", "commands", "holder.pid", "tunnel.pid", "tart.pid", "tart.log", "holder.log")
    def metadata():
        entries = {}
        for name in names:
            info = (state / name).lstat()
            kind = stat.S_ISFIFO if name == "commands" else stat.S_ISREG
            if (not kind(info.st_mode) or info.st_uid != os.getuid()
                    or info.st_mode & 0o022 or info.st_nlink != 1):
                raise ValueError("legacy metadata unavailable")
            entries[name] = (info.st_dev, info.st_ino, info.st_mode, info.st_size, info.st_mtime_ns)
        return entries
    before = metadata()
    if metadata_text(state / "vm").strip() != vm or "connected\n" not in metadata_text(state / "holder.log"):
        return False
    pids = {name: int(metadata_text(state / (name + ".pid"))) for name in ("holder", "tunnel", "tart")}
    if len(set(pids.values())) != 3 or min(pids.values()) <= 1:
        return False
    identities = {name: process_identity(pid) for name, pid in pids.items()}
    holder = identities["holder"][-1]
    # ps prints argv without quoting. Match the full known path suffix, including
    # spaces in checkout/state paths, rather than splitting it as shell text.
    if not re.fullmatch(r"/[^\n]+/(?:python[0-9.]*|Python) -I /[^\n]+/tools/vm/vnc-holder\.py "
                        + re.escape(str(state / "commands")) + " "
                        + re.escape(str(state / "holder.pid")), holder):
        return False
    host = remote_host(tart)
    config = json.loads(Path(tart).resolve().with_name("remote.json").read_text())
    runtime = config["runtime"]
    if not isinstance(runtime, str) or not runtime.startswith("/") or "\0" in runtime:
        return False
    run = shlex.join([runtime.rstrip("/") + "/bin/tart", "run", "--no-graphics", "--vnc-experimental", vm])
    transport = identities["tart"][-1]
    if (not transport.startswith("/usr/bin/ssh ") or not transport.partition(" -- ")[2].startswith(host + " ")
            or not transport.endswith("; exec " + run)):
        return False
    uri = address(metadata_text(state / "tart.log"))
    if not uri:
        return False
    argv = shlex.split(identities["tunnel"][-1])
    if argv[0] != "/usr/bin/ssh" or argv[-2:] != ["--", host] or argv.count("-L") != 1:
        return False
    for option in ("BatchMode=yes", "StrictHostKeyChecking=yes", "ExitOnForwardFailure=yes"):
        if not any(argv[i:i+2] == ["-o", option] for i in range(len(argv) - 1)):
            return False
    forward = re.fullmatch(r"127\.0\.0\.1:([0-9]+):127\.0\.0\.1:" + str(uri[1]), argv[argv.index("-L") + 1])
    if not forward or not 0 < int(forward[1]) < 65536:
        return False
    sockets = subprocess.check_output(
        ["/usr/sbin/lsof", "-nP", "-a", "-p", str(pids["holder"]), "-iTCP", "-sTCP:ESTABLISHED", "-Fn"],
        stderr=subprocess.DEVNULL, timeout=2).decode().splitlines()
    if not any(re.fullmatch(r"n127\.0\.0\.1:[0-9]+->127\.0\.0\.1:" + forward[1], line) for line in sockets):
        return False
    listener = subprocess.check_output(
        ["/usr/sbin/lsof", "-nP", "-a", "-p", str(pids["tunnel"]), "-iTCP:" + forward[1],
         "-sTCP:LISTEN", "-Fn"], stderr=subprocess.DEVNULL, timeout=2).decode().splitlines()
    if "n127.0.0.1:" + forward[1] not in listener:
        return False
    # Reject a process or metadata replacement during the observation.
    after = state.lstat()
    return (metadata() == before and (directory.st_dev, directory.st_ino, directory.st_mode)
            == (after.st_dev, after.st_ino, after.st_mode)
            and all(process_identity(pid) == identities[name] for name, pid in pids.items()))


def status(state, vm, tart=None):
    if hosted_state(state) and tart:
        return hosted_module().hosted_status(state, vm, tart)
    recorded = None
    conclusive = True
    remote = (state / "tunnel.pid").exists()
    try:
        recorded = metadata_text(state / "vm").strip()
        connected = recorded == vm and (request(state, "status")["connected"]
                    if os.path.lexists(state / "owner") else legacy_connected(state, vm, tart))
    except (OSError, ValueError, KeyError, IndexError, RuntimeError, subprocess.SubprocessError):
        connected = False
        conclusive = False
    return {"vm": recorded, "connected": connected, "remote": remote, "conclusive": conclusive}


def legacy_disconnected(state, vm):
    """Read-only proof of the disconnected pre-supervisor format, observed twice."""
    def snapshot():
        directory = state.lstat()
        if (not stat.S_ISDIR(directory.st_mode) or directory.st_uid != os.getuid()
                or directory.st_mode & 0o077 or vm != "meguri-mac"
                or any(os.path.lexists(state / n) for n in
                       ("owner", "owner.pid", "control.sock", "shutdown-pending", "recovery-pending", "drain-token", "drain-precommit"))):
            raise RuntimeError("legacy session unknown; routing refused")
        fifo = (state / "commands").lstat()
        if (not stat.S_ISFIFO(fifo.st_mode) or fifo.st_uid != os.getuid()
                or fifo.st_mode & 0o077 or fifo.st_nlink != 1
                or "connected\n" not in metadata_text(state / "holder.log")
                or not address(metadata_text(state / "tart.log"))):
            raise RuntimeError("legacy session format unknown; routing refused")
        names = ("vm", "holder.pid", "tunnel.pid", "tart.pid")
        values = {name: metadata_text(state / name).strip() for name in names}
        if values["vm"] != vm or any(not re.fullmatch(r"[1-9][0-9]*", values[n])
                                    or int(values[n]) <= 1 for n in names[1:]):
            raise RuntimeError("legacy metadata unknown; routing refused")
        pids = [int(values[n]) for n in names[1:]]
        if len(set(pids)) != 3:
            raise RuntimeError("legacy process identity unknown; routing refused")
        for pid in pids[1:]:
            # ps's empty no-match (exit 1) is the sole accepted result.
            probe = subprocess.run(["/bin/ps", "-ww", "-p", str(pid), "-o", "pid="],
                                   capture_output=True, timeout=3)
            if probe.returncode != 1 or probe.stdout or probe.stderr:
                raise RuntimeError("legacy process live or unknown; routing refused")
        holder = None
        fifo = None
        probe = subprocess.run(["/bin/ps", "-ww", "-p", str(pids[0]), "-o", "pid="],
                               capture_output=True, timeout=3)
        if not (probe.returncode == 1 and not probe.stdout and not probe.stderr):
            if (probe.returncode != 0 or probe.stdout.strip() != str(pids[0]).encode()
                    or probe.stderr or not state.is_absolute()):
                raise RuntimeError("legacy holder live or unknown; routing refused")
            holder = process_identity(pids[0])
            # Use the previous-format full argv binding, including absolute
            # script/FIFO/PID paths; never signal or send to this idle holder.
            if not re.fullmatch(r"/[^\n]+/(?:python[0-9.]*|Python) -I /[^\n]+/tools/vm/vnc-holder\.py "
                                + re.escape(str(state / "commands")) + " "
                                + re.escape(str(state / "holder.pid")), holder[-1]):
                raise RuntimeError("legacy holder identity unknown; routing refused")
            info = (state / "commands").lstat()
            if (not stat.S_ISFIFO(info.st_mode) or info.st_uid != os.getuid()
                    or info.st_mode & 0o077 or info.st_nlink != 1):
                raise RuntimeError("legacy holder FIFO unknown; routing refused")
            fifo = (info.st_dev, info.st_ino, info.st_mode, info.st_ctime_ns)
        sockets = subprocess.run(["/usr/sbin/lsof", "-nP", "-a", "-p", str(pids[0]),
                                  "-iTCP", "-Fn"],
                                 capture_output=True, timeout=3)
        if sockets.returncode != 1 or sockets.stdout or sockets.stderr:
            raise RuntimeError("legacy holder sockets live or unknown; routing refused")
        return ((directory.st_dev, directory.st_ino, directory.st_mode), values, holder, fifo,
                [(s.st_dev, s.st_ino, s.st_mode, s.st_size, s.st_mtime_ns, s.st_ctime_ns)
                 for s in ((state / n).lstat() for n in (*names, "commands", "holder.log", "tart.log"))])
    if snapshot() != snapshot():
        raise RuntimeError("legacy observation changed; routing refused")


# Fixed beside the canonical root: shared across worktrees, and shorter than
# vm-fold for Darwin's small Unix socket path budget. Never rename the orphan.
SESSION_EVIDENCE = ("vm", "commands", "holder.pid", "tunnel.pid", "tart.pid",
                    "owner", "owner.pid", "control.sock", "shutdown-pending",
                    "recovery-pending", "drain-token", "drain-precommit")


def guard_state(state):
    if not state.is_absolute() or ".." in state.parts:
        raise RuntimeError("session state path must be absolute")
    for path in (*reversed(state.parents), state):
        try:
            info = path.lstat()
        except FileNotFoundError:
            continue
        if (not stat.S_ISDIR(info.st_mode) or info.st_uid not in (0, os.getuid())
                or (info.st_mode & 0o022 and not info.st_mode & stat.S_ISVTX)
                or (path == state and (info.st_uid != os.getuid() or info.st_mode & 0o077))):
            raise RuntimeError("session state directory must be owned, private and unsymlinked")


def hosted_module():
    import importlib.util
    spec = importlib.util.spec_from_file_location("vnc_hosted", Path(__file__).with_name("vnc-hosted.py"))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def hosted_state(state):
    return state.name == "vmh3"


def route_state(canonical, vm, tart=None):
    if tart and remote_host(tart):
        hosted = canonical.with_name("vmh3")
        guard_state(hosted)
        primary(hosted, vm)
        # An aborted admission can leave only hosted endpoint metadata. It is
        # not ownership, but neither a legacy timeout nor resident uncertainty
        # permits falling back across a migration or input/stop fence.
        fences = ("shutdown-pending", "recovery-pending", "drain-token",
                  "drain-precommit", "sim-posture-pending")
        if any(os.path.lexists(hosted / name)
               for name in (*SESSION_EVIDENCE, *fences) if name != "vm"):
            return hosted
        resident = status(hosted, vm, tart)
        if resident["conclusive"] is not True or resident.get("vacant") is not True:
            return hosted
        old = route_state(canonical, vm)
        if not any(os.path.lexists(old / name) for name in fences):
            legacy = status(old, vm, tart)
            if legacy["conclusive"] is True and legacy["connected"] is True:
                return old  # Reuse only the validated existing connection.
        return hosted
    managed = canonical.with_name("vmf2")
    guard_state(canonical)
    guard_state(managed)
    primary(canonical, vm)
    primary(managed, vm)
    if os.path.lexists(managed):
        # Even an empty managed root is not authority to ignore old evidence.
        legacy_disconnected(canonical, vm)
        return managed
    if (not os.path.lexists(canonical)
            or not any(os.path.lexists(canonical / n) for n in SESSION_EVIDENCE)
            or os.path.lexists(canonical / "owner")):
        return canonical
    try:
        legacy_disconnected(canonical, vm)
    except (OSError, ValueError, RuntimeError, subprocess.SubprocessError):
        # Original connected/unknown legacy state stays read-only. Its existing
        # lifecycle guards still refuse stop/recovery or replacement.
        return canonical
    return managed


def vacant(state):
    # A rejected guest drain retains its token for clean-drain on the next try;
    # it is not by itself a VNC session or a completed/uncertain stop.
    if any(os.path.lexists(state / name) for name in SESSION_EVIDENCE if name not in ("drain-token", "drain-precommit")):
        raise RuntimeError("existing session is unavailable; owned sessions may use stop once drained; "
                           "legacy or uncertain remote exits require operator recovery with evidence retained")


def init_state(state):
    guard_state(state)
    state.mkdir(mode=0o700, parents=True, exist_ok=True)
    guard_state(state)


def launch(state, children, name, argv, env=None):
    with (state / (name + ".log")).open("wb") as log:
        child = subprocess.Popen(argv, stdin=subprocess.DEVNULL, stdout=log,
                                 stderr=log, env=env, cwd="/", close_fds=True,
                                 start_new_session=True)
    children[name] = child
    (state / (name + ".pid")).write_text(str(child.pid))
    return child


def wait_until(probe, seconds, message):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        value = probe()
        if value:
            return value
        time.sleep(0.2)
    raise RuntimeError(message)


def ssh_listening(child, port):
    """macOS OS evidence for our foreground SSH child, never a VNC probe."""
    if child.poll() is not None:
        raise RuntimeError("SSH forward exited; inspect private tunnel.log")
    try:
        lines = subprocess.check_output([
            "/usr/sbin/lsof", "-nP", "-a", "-p", str(child.pid),
            "-iTCP:" + str(port), "-sTCP:LISTEN", "-Fn",
        ], stderr=subprocess.DEVNULL, timeout=2).decode().splitlines()
    except subprocess.CalledProcessError as error:
        if error.returncode == 1:  # No listener yet.
            return False
        raise
    return ("p" + str(child.pid) in lines and "n127.0.0.1:" + str(port) in lines
            and child.poll() is None)


def connect(state, children, tart, vm, holder_python=None):
    host = remote_host(tart)
    (state / "vm").write_text(vm)
    child = launch(state, children, "tart", [tart, "run", "--no-graphics", "--vnc-experimental", vm])

    def printed_address():
        if child.poll() is not None:
            raise RuntimeError("VNC Tart run exited; inspect private tart.log")
        return address((state / "tart.log").read_text(errors="replace"))

    secret, port = wait_until(printed_address, 60, "Tart printed no VNC URI within 60s")
    if host:
        # Reserve a candidate port without contacting VNC. SSH owns the listener;
        # a bind race fails closed through ExitOnForwardFailure.
        with socket.socket() as candidate:
            candidate.bind(("127.0.0.1", 0))
            local_port = candidate.getsockname()[1]
        tunnel = launch(state, children, "tunnel", [
            "/usr/bin/ssh", "-N", "-T", "-o", "BatchMode=yes",
            "-o", "StrictHostKeyChecking=yes", "-o", "ConnectTimeout=8",
            "-o", "ServerAliveInterval=15", "-o", "ServerAliveCountMax=3",
            "-o", "ForwardAgent=no", "-o", "RequestTTY=no",
            "-o", "ExitOnForwardFailure=yes", "-o", "GatewayPorts=no", "-o", "ControlMaster=no",
            "-o", "ControlPath=none", "-o", "ForkAfterAuthentication=no",
            "-L", f"127.0.0.1:{local_port}:127.0.0.1:{port}", "--", host,
        ])
        # Keep SSH in the foreground in its detached coordinator session.
        # No separate socket probe: that would make a second Tart connection.
        wait_until(lambda: ssh_listening(tunnel, local_port), 15,
                   "SSH did not own its loopback forward within 15s")
        port = local_port
    fifo = state / "commands"
    fifo.unlink(missing_ok=True)
    os.mkfifo(fifo, 0o600)
    env = dict(os.environ, DUO_FOLD_VNC_URL=f"vnc://:{secret}@127.0.0.1:{port}")
    holder = launch(state, children, "holder", [str(holder_python or state / "venv/bin/python"), "-I",
                    str(Path(__file__).with_name("vnc-holder.py")),
                    str(fifo), str(state / "holder.pid")], env)

    def connected():
        if holder.poll() is not None:
            raise RuntimeError("holder failed; inspect private holder.log")
        if child.poll() is not None or (host and tunnel.poll() is not None):
            raise RuntimeError("VNC transport exited during connection")
        return "connected\n" in (state / "holder.log").read_text(errors="replace")

    wait_until(connected, 30, "holder did not authenticate within 30s; inspect private logs")


def join_transports(children, remote):
    # Only the parent signals its still-owned children. PID files are never
    # authority. Never signal the SSH running Tart: its exit must prove that
    # the remote command ended, rather than merely losing our transport.
    for name in ("holder", "tunnel"):
        child = children.get(name)
        if child is not None:
            child.terminate()
            child.wait(timeout=10)
    child = children.get("tart")
    if child is not None:
        code = child.wait(timeout=10)
        if remote and (code < 0 or code == 255):
            raise RuntimeError("remote VNC run completion unknown")


def file_identity(info):
    return info.st_dev, info.st_ino, info.st_mode


def unlink_owned(path, identity):
    try:
        if file_identity(path.lstat()) == identity:
            path.unlink()
    except FileNotFoundError:
        pass


def control_listener(state):
    path = state / "control.sock"
    # Darwin sockaddr_un.sun_path is 104 bytes including its terminator.
    # Enforce the coordinator's limit even in Linux/Docker consistency checks.
    if len(os.fsencode(path)) >= 104:
        raise RuntimeError("control socket path exceeds macOS limit; shorten the main checkout path before preparing")
    listener = socket.socket(socket.AF_UNIX)
    identity = None
    try:
        listener.bind(str(path))
        identity = file_identity(path.lstat())
        listener.listen(2)
        return listener, identity
    except BaseException:
        try:
            if identity is not None:
                unlink_owned(path, identity)
        finally:
            listener.close()
        raise


def preflight(state):
    listener, identity = control_listener(state)
    try:
        unlink_owned(state / "control.sock", identity)
    finally:
        listener.close()


def serve(state, tart, vm, holder_python=None, operations=None):
    children = {}
    remote = bool(remote_host(tart))
    token = secrets.token_hex(32)
    records = {}
    try:
        # Exclusive creation: do not adopt or replace earlier evidence. Before
        # any children exist, failed bind/listen can discard only our records.
        for name, value in (("owner", token), ("owner.pid", str(os.getpid()))):
            record = (state / name).open("x")
            records[name] = (file_identity(os.fstat(record.fileno())), record)
            record.write(value)
            record.flush()
        listener, _ = control_listener(state)
    except BaseException:
        for name, (identity, _) in records.items():
            unlink_owned(state / name, identity)
        raise
    finally:
        # Pin regular-file inodes through failed-setup cleanup so a recycled
        # inode cannot turn replacement evidence into an owned record.
        for _, record in records.values():
            record.close()
    with listener:
        ready = False
        try:
            connect(state, children, tart, vm, holder_python)
            ready = True
        except (OSError, ValueError, RuntimeError, subprocess.SubprocessError):
            pass  # Keep ownership alive for the caller's guarded rollback.
        while True:
            channel, _ = listener.accept()
            done = False
            with channel:
                channel.settimeout(5)
                try:
                    with channel.makefile("r") as source:
                        supplied, action = json.loads(source.readline(4096))
                    if not secrets.compare_digest(supplied, token):
                        raise ValueError("wrong owner")
                    if action == "stop":
                        join_transports(children, remote)
                        for name in ("holder.pid", "tunnel.pid", "tart.pid", "commands", "vm",
                                     "owner", "owner.pid", "control.sock"):
                            (state / name).unlink(missing_ok=True)
                        done = True
                        result = {"ok": True}
                    elif isinstance(action, str) and action in ("ready", "status"):
                        connected = ready and all(c.poll() is None for c in children.values())
                        result = {"ok": action == "status" or connected,
                                  "connected": connected,
                                  "session": hashlib.sha256(token.encode()).hexdigest(),
                                  "peers": {n: c.poll() for n, c in children.items()}}
                    elif operations is not None and isinstance(action, dict):
                        connected = lambda: ready and all(c.poll() is None for c in children.values())
                        result = dict(ok=True, **operations(action, connected, token))
                    else:
                        raise ValueError("unknown action")
                except (OSError, ValueError, KeyError, TypeError, RuntimeError, subprocess.TimeoutExpired):
                    result = {"ok": False}
                try:
                    channel.sendall((json.dumps(result) + "\n").encode())
                except OSError:
                    pass
            if done:
                return


def start(state, tart, vm, holder_python=None):
    with (state / "owner.log").open("wb") as log:
        child = subprocess.Popen([sys.executable, str(Path(__file__).resolve()),
                                  "serve", str(state), tart, vm,
                                  str(holder_python or state / "venv/bin/python")],
                                 stdin=subprocess.DEVNULL, stdout=log, stderr=log,
                                 close_fds=True, start_new_session=True, cwd="/")
    def listening():
        if child.poll() is not None:
            raise RuntimeError("VNC owner unavailable; evidence retained")
        return (state / "control.sock").exists()
    wait_until(listening, 5, "VNC owner did not start; evidence retained")
    request(state, "ready", timeout=120)


def stop(state):
    if (state / "owner").exists():
        request(state, "stop", timeout=35)
    elif any(os.path.lexists(state / name) for name in
             ("vm", "commands", "holder.pid", "tunnel.pid", "tart.pid", "owner.pid", "control.sock")):
        raise RuntimeError("unowned legacy session; evidence retained; no PID signalled")


def owned(state):
    if (state / "owner").exists():
        request(state, "status", timeout=120)
    elif any(os.path.lexists(state / name) for name in
             ("vm", "commands", "holder.pid", "tunnel.pid", "tart.pid", "owner.pid", "control.sock")):
        raise RuntimeError("unowned legacy session; evidence retained; no PID signalled")


# This inode is private and permanent: never unlink/replace it, including after
# a timed-out SSH cleanup. Admission and every remover use the same mutex.
POOL_MUTEX = r'''
import contextlib, fcntl, os, pathlib, stat, sys, time
pool = pathlib.Path(os.environ.get("MEGURI_SIM_POOL_DIR", str(pathlib.Path.home() / "Library/Caches/meguri/sim-pool")))
lock = pool / "pool.lock"
@contextlib.contextmanager
def pool_mutex():
    pool.mkdir(parents=True, exist_ok=True)
    fd = os.open(pool / "vnc-drain.mutex", os.O_CREAT | os.O_RDWR | os.O_NOFOLLOW | os.O_NONBLOCK, 0o600)
    try:
        info = os.fstat(fd)
        if (not stat.S_ISREG(info.st_mode) or info.st_uid != os.getuid()
                or info.st_mode & 0o077 or info.st_nlink != 1):
            raise RuntimeError("unsafe drain mutex")
        deadline = time.monotonic() + 10
        while True:
            try:
                fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
                break
            except BlockingIOError:
                if time.monotonic() >= deadline:
                    raise TimeoutError("drain mutex busy")
                time.sleep(.05)
        bound = (pool / "vnc-drain.mutex").lstat()
        if (info.st_dev, info.st_ino) != (bound.st_dev, bound.st_ino):
            raise RuntimeError("drain mutex replaced")
        yield
    finally:
        os.close(fd)

def remove_drain(token, owned=None):
    # Caller holds pool_mutex. All child operations bind to the opened inode;
    # admission cannot recycle its pathname until the last operation finishes.
    root = os.open(pool, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
    try:
        try:
            fd = os.open("pool.lock", os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=root)
        except FileNotFoundError:
            return
        try:
            info = os.fstat(fd)
            if owned is not None and info.st_ino != owned:
                return
            try:
                owner = os.open("vnc-drain-owner", os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK, dir_fd=fd)
            except FileNotFoundError:
                return
            with os.fdopen(owner) as source:
                if not stat.S_ISREG(os.fstat(source.fileno()).st_mode) or source.read(129) != token:
                    return
            # Preserve posture-pending and any foreign/unknown entries intact.
            if set(os.listdir(fd)) - {"pid", "vnc-drain-owner"}:
                raise RuntimeError("foreign or pending pool entries; fence retained")
            bound = os.stat("pool.lock", dir_fd=root, follow_symlinks=False)
            if (bound.st_dev, bound.st_ino) != (info.st_dev, info.st_ino):
                return
            os.unlink("vnc-drain-owner", dir_fd=fd)
            try:
                os.unlink("pid", dir_fd=fd)
            except FileNotFoundError:
                pass
            os.rmdir("pool.lock", dir_fd=root)
        finally:
            os.close(fd)
    finally:
        os.close(root)
'''


# Read every acquisition record, including leases without a current device.
# Never reclaim, renew, cancel or delete one. Missing/malformed expiry is unknown.
DRAIN = POOL_MUTEX + r'''
import json, os, pathlib, signal, sys, time
pool = pathlib.Path(os.environ.get("MEGURI_SIM_POOL_DIR", str(pathlib.Path.home() / "Library/Caches/meguri/sim-pool")))
lock = pool / "pool.lock"
token = sys.argv[1]
cancelled = pool / "vnc-drain-cancelled" / token
owned = None
committed = False
def expired(*unused):
    if committed:
        return  # Transport loss cannot release a possibly in-flight stop's fence.
    raise TimeoutError("drain guard expired")
def check_cancelled():
    if cancelled.exists():
        raise RuntimeError("drain cancelled")
signal.signal(signal.SIGALRM, expired)
signal.signal(signal.SIGTERM, expired)
signal.signal(signal.SIGHUP, expired)
signal.alarm(110)  # Bound all work before the coordinator's explicit commit.
try:
    deadline = time.monotonic() + 10
    while True:
        check_cancelled()
        try:
            with pool_mutex():
                check_cancelled()
                lock.mkdir()
                owned = lock.stat().st_ino
                (lock / "pid").write_text(str(os.getpid()))
                (lock / "vnc-drain-owner").write_text(token)
            break
        except FileExistsError:
            if time.monotonic() >= deadline:
                raise TimeoutError("pool busy")
            time.sleep(.1)
    root = pool / "leases"
    now = time.time()
    for entry in (list(root.iterdir()) if root.exists() else []):
        expiry = json.loads((entry / "lease.json").read_text())["expires_epoch"]
        if type(expiry) not in (int, float) or not 0 <= expiry < float("inf") or expiry > now:
            raise ValueError("active or unknown acquisition")
    check_cancelled()
    print("drained", flush=True)
    # EOF, a partial/wrong instruction, or the admission alarm releases the
    # unpublished fence. Receipt alone never authorizes permanent exclusion.
    instruction = sys.stdin.buffer.readline(128)
    if instruction != ("commit " + token + "\n").encode():
        raise RuntimeError("drain commit refused")
    check_cancelled()
    # Latch before ack: a lost ack must be reconciled with token cancellation.
    committed = True
    signal.alarm(0)
    try:
        print("committed", flush=True)
    except OSError:
        pass
    while True:
        if cancelled.exists():
            break
        try:
            if not lock.exists() or lock.stat().st_ino != owned:
                break
            if (lock / "vnc-drain-owner").read_text() != token:
                break
            os.utime(lock, None)
        except FileNotFoundError:
            break
        time.sleep(.1)
finally:
    # A normal committed cleanup owns removal on its SSH channel. Only an
    # unpublished/cancelled guard removes its own fence, avoiding two removers.
    if (not committed or cancelled.exists()) and owned is not None:
        with pool_mutex():
            remove_drain(token, owned)
    # Once this exact guard exits it cannot install/publish anything later.
    try:
        cancelled.rmdir()
    except FileNotFoundError:
        pass
'''


# A confirmed token cancellation also covers a guard whose SSH command has not
# started installing its lock yet. Leave the latch until that exact guard exits;
# a delayed guard consumes it. Rare failed launches may leave one inert latch.
CANCEL_DRAIN = r'''
import os, pathlib, sys
pool = pathlib.Path(os.environ.get("MEGURI_SIM_POOL_DIR", str(pathlib.Path.home() / "Library/Caches/meguri/sim-pool")))
(pool / "vnc-drain-cancelled" / sys.argv[1]).mkdir(parents=True, exist_ok=True)
'''


# Only after admission was acknowledged, and no stop/known command completion
# or confirmed postrestart readiness, under the still-held scheduler.
CLEAN_DRAIN = POOL_MUTEX + r'''
with pool_mutex():
    remove_drain(sys.argv[1])
'''




def guest_command(script, token):
    command = ["python3", "-c", script, token]
    if "MEGURI_SIM_POOL_DIR" in os.environ:
        command = ["env", "MEGURI_SIM_POOL_DIR=" + os.environ["MEGURI_SIM_POOL_DIR"], *command]
    return shlex.join(command)


def clean_guest_drain(state, ssh, script, token):
    try:
        with (state / "drain-cleanup.log").open("wb") as log:
            result = subprocess.run([ssh, guest_command(script, token)],
                                    stdin=subprocess.DEVNULL, stdout=log, stderr=log, timeout=120)
    except (OSError, subprocess.SubprocessError):
        raise RuntimeError("guest drain cleanup unconfirmed; inspect private drain-cleanup.log; evidence retained") from None
    if result.returncode:
        raise RuntimeError("guest drain cleanup unconfirmed; inspect private drain-cleanup.log; evidence retained")


def clean_drain(state, ssh, token=None, *, retain_token=False):
    if os.path.lexists(state / "shutdown-pending"):
        raise RuntimeError("remote shutdown completion unknown; guest fence retained")
    marker = state / "drain-token"
    if not os.path.lexists(marker):
        return
    with contextlib.ExitStack() as stack:
        recorded, marker_info = stack.enter_context(pinned_record(marker))
        if token is not None and recorded != token:
            raise RuntimeError("guest drain ownership changed; evidence retained")
        precommit = state / "drain-precommit"
        admission = (stack.enter_context(pinned_record(precommit))
                     if os.path.lexists(precommit) else None)
        if admission is not None and admission[0] != recorded:
            raise RuntimeError("guest drain admission ownership changed; evidence retained")
        script = CANCEL_DRAIN if admission is not None else CLEAN_DRAIN
        clean_guest_drain(state, ssh, script, recorded)
        if os.path.lexists(state / "shutdown-pending"):
            raise RuntimeError("remote shutdown completion unknown; guest fence retained")
        unchanged_record(marker, marker_info)
        if admission is not None:
            unchanged_record(precommit, admission[1])
            precommit.unlink()
        elif os.path.lexists(precommit):
            raise RuntimeError("guest drain admission changed; evidence retained")
        if not retain_token:
            marker.unlink()


def poststart_cleanup(state, ssh, vm, lease, pid, phase):
    """Finish only a drain that predates this authenticated supervisor's start.

    The recovery token must predate the authenticated supervisor, including
    when a prior successful cleanup already removed drain-token. Never accept
    new stop records written beside an older connected session.
    """
    guard_state(state)
    fence = ShutdownFence(lease, state, "")
    with contextlib.ExitStack() as stack:
        records = {name: stack.enter_context(pinned_record(state / name))
                   for name in ("owner", "vm", "recovery-pending")}
        if os.path.lexists(state / "drain-token"):
            records["drain-token"] = stack.enter_context(pinned_record(state / "drain-token"))
        holder, holder_info = stack.enter_context(pinned_record(Path(lease) / "holder.json"))
        metadata = json.loads(holder)
        if (metadata.get("pid") != int(pid) or metadata.get("holder") != "duo-fold"
                or metadata.get("job") != "vm-fold-prepare"):
            raise RuntimeError("post-start cleanup requires the prepare scheduler; evidence retained")
        token = records["recovery-pending"][0]
        if (not re.fullmatch(r"[0-9a-f]{64}", token) or records["vm"][0] != vm
                or ("drain-token" in records and records["drain-token"][0] != token)):
            raise RuntimeError("post-start cleanup records do not match; evidence retained")
        owner_info = records["owner"][1]
        for name in ("recovery-pending", "drain-token"):
            if name not in records:
                continue
            info = records[name][1]
            if (info.st_mtime_ns >= owner_info.st_mtime_ns
                    or info.st_ctime_ns >= owner_info.st_ctime_ns):
                raise RuntimeError("drain does not predate the VNC supervisor; post-start cleanup refused")

        def validate_records(remaining):
            # Caller holds the scheduler mutex. Repeat at final removal so an
            # intervening fence/metadata replacement cannot consume recovery.
            if set(Path(lease).iterdir()) != {Path(lease) / "holder.json"}:
                raise RuntimeError("competing scheduler fence; post-start cleanup refused")
            unchanged_record(Path(lease) / "holder.json", holder_info)
            if any(os.path.lexists(state / name) for name in ("shutdown-pending", "drain-precommit")):
                raise RuntimeError("stop or admission uncertain; post-start cleanup refused")
            for name in remaining:
                unchanged_record(state / name, records[name][1])
            if "drain-token" not in remaining and os.path.lexists(state / "drain-token"):
                raise RuntimeError("guest drain ownership changed; evidence retained")
            primary(state, vm)

        def validate(remaining):
            with fence.scheduler():
                validate_records(remaining)
            reply = request(state, "status")
            if not reply["connected"]:
                raise RuntimeError("owned VNC session disconnected; post-start cleanup refused")

        validate(records)
        if phase == "check":
            return
        if phase != "finish":
            raise ValueError("unknown cleanup phase")
        # Keep the original pre-start recovery token through both local unlinks.
        # Its ordering against the authenticated owner remains proof on resume;
        # a later/forged token or an uncertain stop never grants cleanup.
        if "drain-token" in records:
            clean_drain(state, ssh, token, retain_token=True)
        else:
            clean_guest_drain(state, ssh, CLEAN_DRAIN, token)
        validate(records)
        with fence.scheduler():
            validate_records(records)
            if "drain-token" in records:
                (state / "drain-token").unlink()
            (state / "recovery-pending").unlink()



class ShutdownFence:
    """Use the primary VM's existing cross-worktree scheduler mutex/lease."""
    def __init__(self, lease, state, token):
        self.lease = Path(lease)
        self.identity = file_identity(self.lease.stat())
        self.marker = self.lease / "vm-shutdown-pending"
        self.state, self.token = state, token
        self.owned = None

    @contextlib.contextmanager
    def scheduler(self):
        with Path(str(self.lease) + ".mutex").open("a") as mutex:
            deadline = time.monotonic() + 1
            while True:
                try:
                    fcntl.flock(mutex, fcntl.LOCK_EX | fcntl.LOCK_NB)
                    break
                except BlockingIOError:
                    if time.monotonic() >= deadline:
                        raise RuntimeError("scheduler mutex busy; shutdown refused")
                    time.sleep(.05)
            if file_identity(self.lease.stat()) != self.identity:
                raise RuntimeError("scheduler lease replaced; shutdown refused")
            yield

    def arm(self):
        with self.scheduler():
            self.marker.mkdir(mode=0o700)
            self.owned = file_identity(self.marker.stat())
            (self.marker / "intent.json").write_text(json.dumps(dict(
                token=self.token, state=str(self.state))))

    def clear(self):
        if self.owned is None:
            return
        with self.scheduler():
            if file_identity(self.marker.stat()) != self.owned:
                raise RuntimeError("shutdown scheduler fence replaced; evidence retained")
            (self.marker / "intent.json").unlink(missing_ok=True)
            self.marker.rmdir()
            self.owned = None


def shutdown(state, ssh, tart, vm, lease):
    clean_drain(state, ssh)  # Refuses any previous stop with unknown completion.
    token = secrets.token_hex(32)
    fence = ShutdownFence(lease, state, token)
    # Mark uncertainty before launching: cleanup on another SSH channel may
    # overtake this command even before its guest process exists.
    (state / "drain-precommit").write_text(token)
    (state / "drain-token").write_text(token)
    guard = None
    launched = False
    records = {}
    try:
        with (state / "drain.log").open("wb") as log:
            guard = subprocess.Popen([ssh, guest_command(DRAIN, token)], stdin=subprocess.PIPE,
                                     stdout=subprocess.PIPE, stderr=log)
        readable, _, _ = select.select([guard.stdout], [], [], 20)
        if not readable or guard.stdout.readline() != b"drained\n" or guard.poll() is not None:
            raise RuntimeError("guest acquisitions active, unknown or pool busy; shutdown refused")
        guard.stdin.write(("commit " + token + "\n").encode())
        guard.stdin.flush()
        readable, _, _ = select.select([guard.stdout], [], [], 20)
        if not readable or guard.stdout.readline() != b"committed\n" or guard.poll() is not None:
            raise RuntimeError("guest drain commit unconfirmed; shutdown refused")
        (state / "drain-precommit").unlink()
        # Arm evidence before exec. Exclusive, inode-pinned records let a proven
        # launch failure roll back this attempt without removing earlier evidence.
        for name in ("recovery-pending", "shutdown-pending"):
            path = state / name
            try:
                record = path.open("x")
            except FileExistsError:
                if name == "shutdown-pending":
                    raise RuntimeError("remote shutdown completion unknown; evidence retained")
            else:
                records[name] = (file_identity(os.fstat(record.fileno())), record)
                record.write(token)
                record.flush()
        fence.arm()  # Before any actual stop, while the scheduler still owns this lease.
        with (state / "stop.log").open("wb") as output:
            if hosted_state(state):
                launched = True  # An uncertain RPC may already have reserved the stop.
                code = hosted_module().drained_stop(tart, vm, token)
            else:
                try:
                    child = subprocess.Popen([tart, "stop", vm, "--timeout", "60"],
                                             stdin=subprocess.DEVNULL, stdout=output, stderr=output)
                except OSError:
                    raise RuntimeError("shutdown wrapper could not launch; no stop issued") from None
                launched = True
                try:
                    code = child.wait(timeout=75)
                except (OSError, subprocess.TimeoutExpired):
                    raise RuntimeError("remote shutdown completion unknown; evidence retained; wrapper retry cannot resolve the lost exit") from None
        if code < 0 or code == 255:
            raise RuntimeError("remote shutdown completion unknown; evidence retained; wrapper retry cannot resolve the lost exit")
        unlink_owned(state / "shutdown-pending", records["shutdown-pending"][0])
        fence.clear()  # The actual stop joined, including a known nonzero exit.
        if code:
            # A known exit proves the stop command has joined, even on failure.
            clean_drain(state, ssh, token)
            raise RuntimeError("shutdown command exited unsuccessfully; VM state needs confirmation; evidence retained")
        # Successful hard stop may have killed the guard; cleanup follows the
        # caller's confirmed restart readiness, under the scheduler lease.
    finally:
        # EOF releases only precommit guards. Uncertain commit delivery uses a
        # confirmed cancellation latch before discarding the recovery token.
        if guard is not None:
            guard.stdin.close()
        try:
            if not launched:
                fence.clear()  # Proven no launch must leave the scheduler available.
                for name, (identity, _) in records.items():
                    unlink_owned(state / name, identity)
                clean_drain(state, ssh, token)  # Proven no stop; release only this token.
        finally:
            for _, record in records.values():
                record.close()
            if guard is not None:
                try:
                    guard.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    # Signal only our transport. A published guest guard survives
                    # EOF/HUP/TERM and refreshes its fence until token cleanup.
                    guard.terminate()
                    guard.wait(timeout=5)
                guard.stdout.close()


def main():
    action, path, *args = sys.argv[1:]
    state = Path(path)
    try:
        if action == "host":
            print(remote_host(path))
        elif action == "primary":
            guard_state(state)
            primary(state, *args)
        elif action == "route":
            print(route_state(state, *args))
        elif action == "vacant":
            vacant(state)
        elif action == "init":
            init_state(state)
        elif action == "status":
            print(json.dumps(status(state, *args)))
        elif action == "serve":
            serve(state, *args)
        elif action == "start":
            start(state, *args)
        elif action == "preflight":
            preflight(state)
        elif action == "stop":
            stop(state)
        elif action == "owned":
            owned(state)
        elif action == "shutdown":
            shutdown(state, *args)
        elif action == "clean-drain":
            clean_drain(state, *args)
        elif action == "poststart-cleanup":
            poststart_cleanup(state, *args)
    except RuntimeError as error:
        print("duo-fold: " + str(error), file=sys.stderr)
        return 1
    except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError):
        # Errors and subprocess output may contain the session secret.
        print("duo-fold: " + ("guest leases active or unreadable; use tools/mg sim status/release"
              if action == "shutdown" else "VNC transport unavailable; inspect private build/vm-fold logs and wrapper remote.json"), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
