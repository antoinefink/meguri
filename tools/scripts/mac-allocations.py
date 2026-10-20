#!/usr/bin/env python3
"""One VM-only Allocations observation for ui-test's thousand-swap diagnosis.

No app launch or test coordination. Capture state lives beside this helper;
a VM cache registration keeps interrupted launches discoverable.
KERN_PROCARGS2 supplies real argv boundaries (ps flattens spaces/quotes).
"""
import ctypes
import datetime
import hashlib
import json
import os
from pathlib import Path
import re
import signal
import shutil
import subprocess
import sys
import time
import threading


def stamp():
    return datetime.datetime.now(datetime.timezone.utc).isoformat()


def write(path, data):
    tmp = path.with_suffix("." + str(os.getpid()) + ".tmp")
    tmp.write_text(json.dumps(data, indent=2) + "\n")
    tmp.replace(path)


def read(path):
    return json.loads(path.read_text())


def registration(root):
    # Survives even an intent-only interruption with no discoverable process.
    key = hashlib.sha256(os.fsencode(str(root))).hexdigest()
    return Path.home() / "Library/Caches/meguri/allocations" / (key + ".json")


def argv_for(pid):
    # Same-user procargs only; never enumerate or publish other processes' args.
    libc = ctypes.CDLL(None, use_errno=True)
    mib = (ctypes.c_int * 3)(1, 49, pid)  # CTL_KERN, KERN_PROCARGS2
    size = ctypes.c_size_t(1024 * 1024)
    buf = ctypes.create_string_buffer(size.value)
    if libc.sysctl(mib, 3, buf, ctypes.byref(size), None, 0):
        raise RuntimeError("cannot read candidate argv (KERN_PROCARGS2 errno " + str(ctypes.get_errno()) + ")")
    raw = buf.raw[:size.value]
    argc = ctypes.c_int.from_buffer_copy(raw[:4]).value
    executable, rest = raw[4:].split(b"\0", 1)
    args = rest.lstrip(b"\0").split(b"\0")[:argc]
    if argc < 1 or len(args) != argc:
        raise RuntimeError("incomplete candidate argv")
    return os.fsdecode(executable), [os.fsdecode(a) for a in args]


def identity(pid):
    if not isinstance(pid, int) or pid <= 1:
        raise RuntimeError("non-numeric or unsafe pid")
    ps = subprocess.run(["ps", "-ww", "-p", str(pid), "-o", "uid=", "-o", "stat=", "-o", "lstart="],
                        capture_output=True, text=True)
    if ps.returncode == 1 and not ps.stdout.strip() and not ps.stderr.strip():
        return None
    if ps.returncode or not ps.stdout.strip():
        raise RuntimeError("process identity check failed; exit unknown")
    uid, status, start = ps.stdout.strip().split(None, 2)
    if status[0] in "ZX":  # Defunct process: no executable remains to signal.
        return None
    if int(uid) != os.getuid():
        raise RuntimeError("candidate has another uid")
    try:
        executable, args = argv_for(pid)
    except RuntimeError:
        # A process can exit between ps and procargs. Explicit absence or
        # an explicit exited state certifies that; access failures stay unknown.
        gone = subprocess.run(["ps", "-p", str(pid), "-o", "stat="], capture_output=True, text=True)
        if gone.returncode == 1 and not gone.stdout.strip() and not gone.stderr.strip():
            return None
        if gone.returncode == 0 and gone.stdout.strip() and gone.stdout.strip()[0] in "ZX" and not gone.stderr.strip():
            return None
        raise
    return {"pid": pid, "uid": int(uid), "start": start, "executable": executable}, args


def matches(record, args=None):
    current = identity(record["pid"])
    if current is None:
        return False
    if current[0] != record or (args is not None and current[1][1:] != args):
        raise RuntimeError("saved process identity or arguments no longer match; no signal sent")
    return True


def target(binary):
    p = subprocess.run(["pgrep", "-x", "Meguri"], capture_output=True, text=True)
    if p.returncode not in (0, 1) or p.stderr.strip():
        raise RuntimeError("candidate lookup failed")
    found = []
    for value in p.stdout.split():
        if not value.isdecimal():
            raise RuntimeError("non-numeric candidate")
        current = identity(int(value))
        if current is None:
            continue
        record, args = current
        if record["executable"] != binary:
            continue
        routes = [args[i + 1] for i in range(len(args) - 1) if args[i] == "-MeguriRoute"]
        if routes != ["spike/card-swap"] or args[-1] == "-MeguriRoute":
            raise RuntimeError("copied app has the wrong route")
        found.append((record, args[1:]))
    if len(found) > 1:
        raise RuntimeError("multiple matching apps")
    return found[0] if found else None


def child_args(role, cfg, root):
    if role == "observer":
        return [str(root / "mac-allocations.py"), "--observe", str(root)]
    if role == "waiter":
        return ["-1", cfg["notification"]]
    return ["record", "--template", "Allocations", "--attach", str(cfg["target_pid"]),
            "--time-limit", "90s", "--output", str(root / "allocations.trace"),
            "--no-prompt", "--notify-tracing-started", cfg["notification"]]


def remember(proc, role, executable, cfg, root):
    # xcrun execs xctrace in place. Do not save the transient xcrun identity.
    end = time.monotonic() + 2
    last_error = None
    while time.monotonic() < end:
        if proc.poll() is not None:
            raise RuntimeError(role + " exited before its identity was recorded")
        try:
            current = identity(proc.pid)
        except RuntimeError as exc:
            # UNKNOWN can be transient during exec. Only this still-owned
            # child gets another check; no identity is saved or signal sent.
            last_error = str(exc)
            current = None
        if current and current[0]["executable"] == executable and current[1][1:] == child_args(role, cfg, root):
            write(root / (role + ".json"), current[0])
            return current[0]
        time.sleep(0.02)
    raise RuntimeError(role + " executable and arguments could not be verified" +
                       (": " + last_error if last_error is not None else ""))


def launch(role, command, executable, cfg, root, **kwargs):
    path = root / (role + ".launch.json")
    intent = {"run": root.parent.name, "uid": os.getuid(), "executable": executable,
              "args": child_args(role, cfg, root), "cleanup_confirmed": False}
    write(path, intent)  # Must precede Popen, including the observer's spawn.
    proc = None
    try:
        proc = subprocess.Popen(command, **kwargs)
        intent["pid"] = proc.pid
        write(path, intent)
        remember(proc, role, executable, cfg, root)
        return proc
    except Exception:
        # No wait thread has been started for this child. Popen's signal/wait
        # methods retain ownership and never signal a PID after reaping it.
        # This also handles xcrun before it has exec'd the verified xctrace.
        try:
            if proc is not None:
                proc.send_signal(signal.SIGINT if role == "trace" else signal.SIGTERM)
                try:
                    intent["exit"] = proc.wait(timeout=2)
                except subprocess.TimeoutExpired:
                    intent["forced"] = True
                    proc.kill()
                    intent["exit"] = proc.wait(timeout=2)
            else:
                intent["not_spawned"] = True
            intent["cleanup_confirmed"] = True
        except Exception as exc:
            intent["cleanup_error"] = str(exc)
        write(path, intent)
        raise


def role_gone(role, cfg, root):
    path = root / (role + ".json")
    intent = root / (role + ".launch.json")
    if intent.exists():
        saved = read(intent)
        if (saved.get("run") != root.parent.name or saved.get("uid") != os.getuid()
                or saved.get("args") != child_args(role, cfg, root)):
            raise RuntimeError(role + " launch ownership unknown")
        if saved.get("cleanup_confirmed") is True and (type(saved.get("exit")) is int or saved.get("not_spawned") is True):
            return True  # Persisted direct-child wait, never a recovered PID.
        if not path.exists():
            raise RuntimeError(role + " launch identity unknown; evidence retained")
    if path.exists():
        return not matches(read(path), child_args(role, cfg, root))
    return role != "observer"


def stop_owned(role, cfg, root, sig, seconds, proc=None):
    path = root / (role + ".json")
    if role_gone(role, cfg, root):
        return
    if not path.exists():
        raise RuntimeError(role + " identity unknown; evidence retained")
    record = read(path)
    args = child_args(role, cfg, root)
    if not matches(record, args):
        return
    os.kill(record["pid"], sig)
    end = time.monotonic() + seconds
    while time.monotonic() < end:
        if proc is not None:
            proc.poll()  # Reap our child while retaining its real exit code.
        if not matches(record, args):
            return
        time.sleep(0.1)
    # Verify again, immediately before the fallback signal.
    if matches(record, args):
        os.kill(record["pid"], signal.SIGKILL)
    end = time.monotonic() + 2
    while time.monotonic() < end:
        if proc is not None:
            proc.poll()
        if not matches(record, args):
            raise RuntimeError(role + " required SIGKILL; not finalized")
        time.sleep(0.1)
    raise RuntimeError(role + " exit unconfirmed")


def observe(root):
    cfg = read(root / "config.json")
    meta = {"diagnostic_mode": "allocations", "ordinary_performance_receipt": False,
            "coverage": "uncertified/partial", "observer_started_at": stamp(),
            "start_seen": False, "finalized": False, "errors": [], "status": "error"}
    stopping = False
    trace = waiter = None
    trace_log = waiter_log = notification_thread = None

    def interrupted(_sig, _frame):
        nonlocal stopping
        stopping = True
        if not (root / "stop").exists():
            meta["errors"].append("observer interrupted without normal stop (orphan/cancellation)")

    signal.signal(signal.SIGTERM, interrupted)
    signal.signal(signal.SIGHUP, interrupted)
    # Background shell children inherit ignored SIGINT. Restore it before
    # spawning xcrun so graceful SIGINT works, without threaded preexec code.
    signal.signal(signal.SIGINT, signal.SIG_DFL)
    try:
        own = identity(os.getpid())
        if own is None or own[1][1:] != child_args("observer", cfg, root):
            raise RuntimeError("observer identity unknown")
        write(root / "observer.json", own[0])
        write(root / "metadata.json", meta)
        (root / "ready").touch()
        deadline = time.monotonic() + 30
        selected = None
        while not stopping and not (root / "stop").exists() and time.monotonic() < deadline:
            selected = target(cfg["binary"])
            if selected:
                break
            time.sleep(0.1)
        if not selected or stopping or (root / "stop").exists():
            raise RuntimeError("target not observed within 30s or observation stopped before attach")
        record, args = selected
        cfg["target_pid"] = record["pid"]
        write(root / "config.json", cfg)
        meta["target"] = dict(record, route="spike/card-swap", verified_at=stamp())
        # Register the real Darwin notification waiter BEFORE xctrace. -1 exits
        # on the first notification; only its success certifies a start event.
        notify = shutil.which("notifyutil")
        if not notify:
            raise RuntimeError("notifyutil unavailable")
        waiter_log = open(root / "notification.log", "wb")
        waiter = launch("waiter", [notify] + child_args("waiter", cfg, root), notify, cfg, root,
                                  stdin=subprocess.DEVNULL, stdout=waiter_log, stderr=subprocess.STDOUT)
        meta["waiter_started_at"] = stamp()

        def notification_time():
            if waiter.wait() == 0:
                write(root / "notification.json", {"observed_at": stamp()})

        notification_thread = threading.Thread(target=notification_time, daemon=True)
        notification_thread.start()
        xctrace = subprocess.check_output(["xcrun", "--find", "xctrace"], text=True).strip()
        # Final lookup repeats uniqueness, uid, executable, route and start.
        again = target(cfg["binary"])
        if again != selected or not matches(record, args) or stopping or (root / "stop").exists():
            raise RuntimeError("target changed immediately before attach")
        meta["attach_requested_at"] = stamp()
        trace_log = open(root / "xctrace.log", "wb")
        trace = launch("trace", ["xcrun", "xctrace"] + child_args("trace", cfg, root), xctrace, cfg, root,
                                 stdin=subprocess.DEVNULL, stdout=trace_log, stderr=subprocess.STDOUT)
        meta["trace_process_started_at"] = stamp()
        write(root / "metadata.json", meta)
        deadline = time.monotonic() + 95
        while not stopping and not (root / "stop").exists() and time.monotonic() < deadline:
            if not meta["start_seen"] and (root / "notification.json").exists():
                meta.update(start_seen=True, tracing_started_notification_at=read(root / "notification.json")["observed_at"])
                write(root / "metadata.json", meta)
            if trace.poll() is not None:
                meta["trace_ended_at"] = stamp()
                break
            time.sleep(0.05)
        if time.monotonic() >= deadline:
            meta["errors"].append("trace exceeded bounded observation")
    except Exception as exc:
        meta["errors"].append(str(exc))
    finally:
        # This process can no longer attach. Finalize only our verified trace,
        # then end our notification waiter; never signal the target app.
        for role, sig, seconds in (("trace", signal.SIGINT, 15), ("waiter", signal.SIGTERM, 2)):
            try:
                proc = trace if role == "trace" else waiter
                if proc is not None and proc.poll() is None:
                    stop_owned(role, cfg, root, sig, seconds, proc)
            except Exception as exc:
                meta["errors"].append(str(exc))
            if role == "trace" and trace is not None and trace.poll() is not None:
                meta.setdefault("trace_ended_at", stamp())
        if trace is not None:
            meta["trace_exit"] = trace.poll()
            meta["finalized"] = meta["trace_exit"] == 0
        if waiter is not None:
            # The waiter thread writes the time at notification receipt, even
            # while SIGINT finalizes the trace. It never infers start from pid.
            if waiter.poll() is not None and notification_thread is not None:
                notification_thread.join(timeout=1)
            if (root / "notification.json").exists():
                meta.update(start_seen=True, tracing_started_notification_at=read(root / "notification.json")["observed_at"])
        if not meta["start_seen"]:
            meta["errors"].append("tracing-started notification missing")
        if not meta["finalized"]:
            meta["errors"].append("trace did not finalize successfully")
        if not any(p.is_file() and p.stat().st_size > 0 for p in (root / "allocations.trace").rglob("*")):
            meta["errors"].append("trace absent or empty")
        meta["launches"] = {role: read(root / (role + ".launch.json"))
                            for role in ("waiter", "trace") if (root / (role + ".launch.json")).exists()}
        if trace is None and "trace" in meta["launches"]:
            meta["trace_exit"] = meta["launches"]["trace"].get("exit")
        meta["observer_ended_at"] = stamp()
        meta["status"] = "error" if meta["errors"] else "pass"
        write(root / "metadata.json", meta)
        for log in (trace_log, waiter_log):
            if log is not None:
                log.close()


def stop(root, orphan=False):
    cfg = read(root / "config.json")
    (root / "stop").touch()  # First prohibit a late attach, even before ready.
    errors = []
    try:
        stop_owned("observer", cfg, root, signal.SIGTERM, 22)
    except Exception as exc:
        errors.append(str(exc))
    # Survives a lost SSH client or a dead observer. Identity mismatch never
    # permits a signal. Observer is confirmed gone before touching children.
    try:
        if not role_gone("observer", cfg, root):
            raise RuntimeError("observer still running; children left for next lease")
        cfg = read(root / "config.json")
        for role, sig, seconds in (("trace", signal.SIGINT, 15), ("waiter", signal.SIGTERM, 2)):
            stop_owned(role, cfg, root, sig, seconds)
    except Exception as exc:
        errors.append(str(exc))
    meta = read(root / "metadata.json") if (root / "metadata.json").exists() else {"errors": ["observer metadata absent"]}
    meta["errors"].extend(errors)
    confirmed = True
    for role in ("observer", "trace", "waiter"):
        path = root / (role + ".json")
        try:
            if not role_gone(role, cfg, root):
                confirmed = False
            elif not path.exists() and not (root / (role + ".launch.json")).exists() and meta.get("trace_process_started_at" if role == "trace" else "waiter_started_at"):
                confirmed = False
        except Exception:
            confirmed = False
    meta["cleanup_confirmed"] = confirmed
    if not confirmed:
        meta["errors"].append("owned process exit unknown; remote evidence retained")
    if orphan:
        meta["errors"].append("orphan recovered at next VM lease; diagnostic failed")
    if not meta.get("observer_ended_at"):
        meta["errors"].append("observer exit/finalization unknown")
    meta["status"] = "error" if meta["errors"] else "pass"
    write(root / "metadata.json", meta)
    if not confirmed:
        return 2
    registration(root).unlink(missing_ok=True)
    return 0 if orphan or meta["status"] == "pass" else 1


def main():
    action, directory, *extra = sys.argv[1:]
    root = Path(directory)
    if not re.search(r"/build/mg/ui-test/[A-Za-z0-9-]+/allocations$", str(root)) or root != Path(__file__).absolute().parent:
        raise RuntimeError("helper must live in its ui-test run-owned allocations directory")
    if action == "--observe":
        observe(root)
        return 0
    if action in ("--stop", "--orphan-stop"):
        return stop(root, orphan=action == "--orphan-stop")
    if action != "--start" or len(extra) != 2:
        raise RuntimeError("invalid invocation")
    binary, runid = extra
    if runid != root.parent.name or not re.fullmatch(r"[A-Za-z0-9-]+", runid):
        raise RuntimeError("invalid run identity")
    if not binary.startswith("/") or not binary.endswith("/Build/Products/Profile/Meguri.app/Contents/MacOS/Meguri"):
        raise RuntimeError("expected copied Profile binary full path")
    if (root / "config.json").exists():
        raise RuntimeError("one observer per run; refusing restart")
    if target(binary) is not None:
        raise RuntimeError("target already running before observation; refusing stale attach")
    own = identity(os.getpid())
    if own is None:
        raise RuntimeError("current interpreter identity unknown")
    # sys.executable may be a launcher that execs this observed interpreter.
    interpreter = own[0]["executable"]
    cfg = {"binary": binary, "notification": "com.antoinefink.meguri.mg.allocations." + runid}
    write(root / "config.json", cfg)
    registered = registration(root)
    registered.parent.mkdir(parents=True, exist_ok=True)
    write(registered, {"root": str(root)})
    with open(root / "observer.log", "wb") as log:
        launch("observer", [sys.executable] + child_args("observer", cfg, root), interpreter, cfg, root,
                         stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
    deadline = time.monotonic() + 3
    while time.monotonic() < deadline:
        if (root / "ready").exists():
            return 0
        time.sleep(0.02)
    (root / "stop").touch()
    raise RuntimeError("observer readiness unknown; stop required")


if __name__ == "__main__":
    try:
        sys.exit(main())
    except Exception as error:
        print("mac-allocations: " + str(error), file=sys.stderr)
        sys.exit(1)
