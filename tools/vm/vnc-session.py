"""Private transport for duo-fold.sh; never print Tart output or credentials."""
import json
import errno
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


def request(state, action, timeout=15):
    token = (state / "owner").read_text()
    with socket.socket(socket.AF_UNIX) as channel:
        channel.settimeout(timeout)
        channel.connect(str(state / "control.sock"))
        channel.sendall((json.dumps([token, action]) + "\n").encode())
        with channel.makefile("r") as response:
            result = json.loads(response.readline(65536))
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
    with os.fdopen(fd) as source:
        info = os.fstat(source.fileno())
        if (not stat.S_ISREG(info.st_mode) or info.st_uid != os.getuid()
                or info.st_mode & 0o022 or info.st_nlink != 1):
            raise ValueError("session metadata unavailable")
        return source.read(65536)


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
    recorded = None
    remote = (state / "tunnel.pid").exists()
    try:
        recorded = metadata_text(state / "vm").strip()
        connected = recorded == vm and (request(state, "status")["connected"]
                    if os.path.lexists(state / "owner") else legacy_connected(state, vm, tart))
    except (OSError, ValueError, KeyError, IndexError, RuntimeError, subprocess.SubprocessError):
        connected = False
    return {"vm": recorded, "connected": connected, "remote": remote}


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


def connect(state, children, tart, vm):
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
        def listening():
            if tunnel.poll() is not None:
                raise RuntimeError("SSH forward exited; inspect private tunnel.log")
            with socket.socket() as probe:
                try:
                    probe.bind(("127.0.0.1", local_port))
                except OSError as error:
                    if error.errno == errno.EADDRINUSE:
                        return True
                    raise
            return False

        wait_until(listening, 15, "SSH did not bind its loopback forward within 15s")
        port = local_port
    fifo = state / "commands"
    fifo.unlink(missing_ok=True)
    os.mkfifo(fifo, 0o600)
    env = dict(os.environ, DUO_FOLD_VNC_URL=f"vnc://:{secret}@127.0.0.1:{port}")
    holder = launch(state, children, "holder", [str(state / "venv/bin/python"), "-I",
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


def serve(state, tart, vm):
    children = {}
    remote = bool(remote_host(tart))
    token = secrets.token_hex(32)
    # Exclusive creation: do not adopt or replace an earlier owner's evidence.
    with (state / "owner").open("x") as owner:
        owner.write(token)
    (state / "owner.pid").write_text(str(os.getpid()))  # Diagnostic only.
    with socket.socket(socket.AF_UNIX) as listener:
        listener.bind(str(state / "control.sock"))
        listener.listen(2)
        ready = False
        try:
            connect(state, children, tart, vm)
            ready = True
        except (OSError, ValueError, RuntimeError, subprocess.TimeoutExpired):
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
                    elif action in ("ready", "status"):
                        result = {"ok": action == "status" or ready,
                                  "connected": ready and all(c.poll() is None for c in children.values())}
                    else:
                        raise ValueError("unknown action")
                except (OSError, ValueError, RuntimeError, subprocess.TimeoutExpired):
                    result = {"ok": False}
                try:
                    channel.sendall((json.dumps(result) + "\n").encode())
                except OSError:
                    pass
            if done:
                return


def start(state, tart, vm):
    with (state / "owner.log").open("wb") as log:
        child = subprocess.Popen([sys.executable, str(Path(__file__).resolve()),
                                  "serve", str(state), tart, vm],
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
    elif any((state / name).exists() for name in
             ("vm", "commands", "holder.pid", "tunnel.pid", "tart.pid", "control.sock")):
        raise RuntimeError("unowned legacy session; evidence retained; no PID signalled")


def owned(state):
    if (state / "owner").exists():
        request(state, "status", timeout=120)
    elif any((state / name).exists() for name in
             ("vm", "commands", "holder.pid", "tunnel.pid", "tart.pid", "control.sock")):
        raise RuntimeError("unowned legacy session; evidence retained; no PID signalled")


# Read every acquisition record, including leases without a current device.
# Never reclaim, renew, cancel or delete one. Missing/malformed expiry is unknown.
DRAIN = r'''
import json, os, pathlib, signal, sys, time
pool = pathlib.Path(os.environ.get("MEGURI_SIM_POOL_DIR", str(pathlib.Path.home() / "Library/Caches/meguri/sim-pool")))
lock = pool / "pool.lock"
owned = None
def expired(*unused):
    raise TimeoutError("drain guard expired")
signal.signal(signal.SIGALRM, expired)
signal.signal(signal.SIGTERM, expired)
signal.signal(signal.SIGHUP, expired)
signal.alarm(110)  # Well below legacy clients' ten-minute stale-lock threshold.
try:
    pool.mkdir(parents=True, exist_ok=True)
    deadline = time.monotonic() + 10
    while True:
        try:
            lock.mkdir()
            owned = lock.stat().st_ino
            (lock / "pid").write_text(str(os.getpid()))
            (lock / "vnc-drain-owner").write_text(sys.argv[1])
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
    print("drained", flush=True)
    sys.stdin.buffer.read(1)  # EOF releases; shutdown may instead end this guest.
finally:
    if owned is not None and lock.exists() and lock.stat().st_ino == owned:
        (lock / "vnc-drain-owner").unlink(missing_ok=True)
        (lock / "pid").unlink(missing_ok=True)
        lock.rmdir()
'''


# A hard VM stop may prevent the guest guard's finally block. Only remove our
# exact guard after that stop has completed, under the still-held scheduler.
CLEAN_DRAIN = r'''
import os, pathlib, sys
pool = pathlib.Path(os.environ.get("MEGURI_SIM_POOL_DIR", str(pathlib.Path.home() / "Library/Caches/meguri/sim-pool")))
lock = pool / "pool.lock"
marker = lock / "vnc-drain-owner"
if marker.exists() and marker.read_text() == sys.argv[1]:
    marker.unlink()
    (lock / "pid").unlink(missing_ok=True)
    lock.rmdir()
'''


def guest_command(script, token):
    command = ["python3", "-c", script, token]
    if "MEGURI_SIM_POOL_DIR" in os.environ:
        command = ["env", "MEGURI_SIM_POOL_DIR=" + os.environ["MEGURI_SIM_POOL_DIR"], *command]
    return shlex.join(command)


def clean_drain(state, ssh):
    marker = state / "drain-token"
    if marker.exists():
        with (state / "drain-cleanup.log").open("wb") as log:
            result = subprocess.run([ssh, guest_command(CLEAN_DRAIN, marker.read_text())],
                                    stdin=subprocess.DEVNULL, stdout=log, stderr=log, timeout=20)
        if result.returncode:
            raise RuntimeError("guest drain cleanup unconfirmed; evidence retained")
        marker.unlink()


def shutdown(state, ssh, tart, vm):
    clean_drain(state, ssh)
    token = secrets.token_hex(32)
    (state / "drain-token").write_text(token)
    with (state / "drain.log").open("wb") as log:
        guard = subprocess.Popen([ssh, guest_command(DRAIN, token)], stdin=subprocess.PIPE,
                                 stdout=subprocess.PIPE, stderr=log)
        try:
            readable, _, _ = select.select([guard.stdout], [], [], 20)
            if not readable or guard.stdout.readline() != b"drained\n" or guard.poll() is not None:
                raise RuntimeError("guest acquisitions active, unknown or pool busy; shutdown refused")
            # The guest pool mutex stays held across the *remote* stop command,
            # including already-running legacy renew clients outside the scheduler.
            (state / "recovery-pending").touch()
            pending = state / "shutdown-pending"
            pending.touch()
            with (state / "stop.log").open("wb") as output:
                result = subprocess.run([tart, "stop", vm, "--timeout", "60"],
                                        stdin=subprocess.DEVNULL, stdout=output, stderr=output, timeout=75)
            if result.returncode >= 0 and result.returncode != 255:
                pending.unlink()
            if result.returncode:
                raise RuntimeError("Tart shutdown status uncertain; evidence retained")
        finally:
            guard.stdin.close()
            try:
                guard.wait(timeout=5)
            except subprocess.TimeoutExpired:
                # Only our child transport, never a PID read from disk. The guest
                # guard also cleans up on EOF/HUP or its bounded alarm.
                guard.terminate()
                guard.wait(timeout=5)
            guard.stdout.close()


def main():
    action, path, *args = sys.argv[1:]
    state = Path(path)
    try:
        if action == "host":
            print(remote_host(path))
        elif action == "status":
            print(json.dumps(status(state, *args)))
        elif action == "serve":
            serve(state, *args)
        elif action == "start":
            start(state, *args)
        elif action == "stop":
            stop(state)
        elif action == "owned":
            owned(state)
        elif action == "shutdown":
            shutdown(state, *args)
        elif action == "clean-drain":
            clean_drain(state, *args)
    except RuntimeError as error:
        print("duo-fold: " + str(error), file=sys.stderr)
        return 1
    except (OSError, ValueError, KeyError, TypeError, subprocess.TimeoutExpired):
        # Errors and subprocess output may contain the session secret.
        print("duo-fold: " + ("guest leases active or unreadable; use tools/mg sim status/release"
              if action == "shutdown" else "VNC transport unavailable; inspect private build/vm-fold logs and wrapper remote.json"), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
