#!/usr/bin/env python3
"""One recorded pre-prepare failure: stop, independently confirm, archive.

No original source is executed, no inherited lease is released, and no failure
cleanup removes evidence. Root owns invocation and review of uncertain claims.
"""
import ast
import base64
import contextlib
import ctypes
import errno
import fcntl
import hashlib
import json
import os
from pathlib import Path
import re
import shlex
import signal
import stat
import struct
import subprocess
import sys
import tempfile
import time

RUN = "20261102T200056Z-94582"
COMMIT = "5b0a661d100019136038a4562500b83af08eccd5"
SOURCE_SHA = "73b2fef1be62821f20fe1dded9507c0600d442dd20af21f7d07fa615f02e2171"
SOURCE_SIZE = 48275
SCOPE = "VM-XCODE-MAINTENANCE-ONLY"
FRAMES = list(zip(("run", "main", "host", "__init__", "call_command", "command", "need"),
                  (1072, 1067, 1016, 841, 865, 281, 57)))
DIAGNOSTIC = {"exception": "Refused", "stack": [{"function": f, "line": n} for f, n in FRAMES]}
LIMIT = 1024 * 1024
DISCOVERY_SECONDS = 5
DIR_FLAGS = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC
FILE_FLAGS = os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK | os.O_CLOEXEC
SIGNALS = (signal.SIGINT, signal.SIGTERM, signal.SIGHUP)
# READ predicates from mvl_stop_tests; never invoke its draining implementation.
MANAGED = (r"xcodebuild[ ]test-without-building|UITests-Runner[.]app/Contents/MacOS/|"
           r"UITests-Runner[.]app/[^/]*UITests-Runner|/[x]ctest( |$)|"
           r"[m]ac-hidden-launch[.]sh|/Build/Products/Debug/Meguri[.]app/Contents/MacOS/[M]eguri")
EXTRA = r"xcodebuild|XCTest|vm-xcode-stage[.]py|sim-record[.]sh|recordVideo|xctrace|vnc-holder[.]py|duo-fold[.]sh"
VM_JOBS = ("mac-ui", "a11y", "drive", "sim", "sim-setup", "sim-reset", "run", "screenshot",
           "ui-tree", "record", "metrics", "snapshots", "xcode-check", "perf-ui", "mac-hosted-test",
           "cloudkit-agent-install", "vm-xcode-stage", "vm-crash-info")


class Refused(Exception):
    pass


class Cancelled(BaseException):
    pass


def need(condition):
    if not condition:
        raise Refused()


# Exact compiler filename of this helper, captured without a filesystem probe.
PRIVATE_SOURCE = need.__code__.co_filename
PRIVATE_FRAME_CAP = 8
PRIVATE_RECORD_CAP = 2048
PRIVATE_CLASSES = ((Refused, "Refused"), (Cancelled, "Cancelled"),
                   (OSError, "OSError"), (PermissionError, "PermissionError"),
                   (FileNotFoundError, "FileNotFoundError"),
                   (ProcessLookupError, "ProcessLookupError"),
                   (ChildProcessError, "ChildProcessError"),
                   (TimeoutError, "TimeoutError"),
                   (subprocess.TimeoutExpired, "TimeoutExpired"),
                   (ValueError, "ValueError"), (TypeError, "TypeError"),
                   (KeyError, "KeyError"), (RuntimeError, "RuntimeError"),
                   (json.JSONDecodeError, "JSONDecodeError"))


def private_failure(error):
    """Only bounded class/errno/own-frame locations go to the private sink."""
    try:
        label = next((name for cls, name in PRIVATE_CLASSES if type(error) is cls), "Other")
        record = {"exception": label, "stack": []}
        if isinstance(error, OSError):
            number = error.errno
            if type(number) is int and 0 <= number <= 4095:
                record["errno"] = number
        frame = error.__traceback__
        while frame is not None:
            code = frame.tb_frame.f_code
            if code.co_filename == PRIVATE_SOURCE:
                name, line = code.co_name, frame.tb_lineno
                if (type(name) is str and re.fullmatch(
                        r"[A-Za-z_][A-Za-z0-9_]{0,63}|<(module|lambda|listcomp|dictcomp|setcomp|genexpr)>", name)
                        and type(line) is int and 1 <= line <= 1000000):
                    record["stack"].append({"function": name, "line": line})
                    # Retain the innermost failing location when truncating.
                    if len(record["stack"]) > PRIVATE_FRAME_CAP:
                        del record["stack"][0]
            frame = frame.tb_next
        rendered = json.dumps(record, separators=(",", ":")) + "\n"
        if len(rendered) <= PRIVATE_RECORD_CAP:
            sys.stderr.write(rendered)
            sys.stderr.flush()
    except BaseException:
        # Even cancellation or a broken renderer/sink cannot replace refusal.
        pass


def loads(raw):
    def pairs(items):
        result = {}
        for key, value in items:
            need(key not in result)
            result[key] = value
        return result
    return json.loads(raw, object_pairs_hook=pairs,
                      parse_constant=lambda _: (_ for _ in ()).throw(Refused()))


def encoded(value):
    return (json.dumps(value, sort_keys=True, separators=(",", ":")) + "\n").encode()


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def identity(s):
    # APFS directory nlink/mtime changes as entries are added; never pin those.
    return (s.st_dev, s.st_ino, s.st_mode, s.st_uid, s.st_gid)


def file_identity(s):
    return identity(s) + (s.st_nlink, s.st_size, s.st_mtime_ns, s.st_ctime_ns)


class Pins:
    """Retain no-follow ancestor and entry descriptors until the final rename."""
    def __init__(self):
        self.fds, self.entries, self.files, self.inventories = [], [], [], {}
        self.checks, self.absences = {}, []
        self.rootfd = os.open("/", DIR_FLAGS)
        self.fds.append(self.rootfd)
        self.directories = {"/": self.rootfd}

    def directory(self, path):
        need(isinstance(path, str) and path.startswith("/") and os.path.normpath(path) == path)
        if path in self.directories:
            return self.directories[path]
        parent = self.directory(os.path.dirname(path))
        name = os.path.basename(path)
        fd = os.open(name, DIR_FLAGS, dir_fd=parent)
        self.fds.append(fd)
        s = os.fstat(fd)
        need(stat.S_ISDIR(s.st_mode) and s.st_uid in (0, os.getuid()))
        need(not s.st_mode & 0o022 or s.st_uid == 0 and s.st_mode & stat.S_ISVTX)
        self.entries.append((parent, name, fd, identity(s), False))
        self.directories[path] = fd
        return fd

    def file(self, path, limit=LIMIT, executable=False, optional=False):
        parent = self.directory(os.path.dirname(path))
        name = os.path.basename(path)
        if optional:
            try:
                os.stat(name, dir_fd=parent, follow_symlinks=False)
            except FileNotFoundError:
                self.absences.append((parent, name))
                self.check()
                return None
        fd = os.open(name, FILE_FLAGS, dir_fd=parent)
        self.fds.append(fd)
        s = os.fstat(fd)
        need(stat.S_ISREG(s.st_mode) and s.st_nlink == 1 and s.st_uid in (0, os.getuid())
             and not s.st_mode & 0o022 and s.st_size <= limit)
        need(not executable or s.st_mode & 0o111)
        original = file_identity(s)
        raw = os.pread(fd, limit + 1, 0)
        need(len(raw) == s.st_size)
        self.entries.append((parent, name, fd, original, True))
        self.files.append((path, fd, original, raw))
        self.check()
        return raw

    def inventory(self, path, maximum=128):
        fd = self.directory(path)
        names = set()
        # scandir(fd) is bounded while enumerating, including oversized directories.
        with os.scandir(fd) as entries:
            for entry in entries:
                names.add(entry.name)
                need(len(names) <= maximum)
        return names

    def freeze(self, path, expected=None, maximum=128):
        names = self.inventory(path, maximum)
        need(expected is None or names == set(expected))
        self.inventories[path] = (names, maximum)
        return names

    def stage_names(self, path):
        deadline = time.monotonic() + DISCOVERY_SECONDS
        fd = self.directory(path)
        names = set()
        # Only exact stage names matter. Unrelated OS entries may churn and
        # never consume the relevant-context bound or acquire individual pins.
        with os.scandir(fd) as entries:
            for entry in entries:
                need(time.monotonic() < deadline)
                if re.fullmatch(r"meguri-vm-xcode\.[A-Za-z0-9]{6}", entry.name):
                    need(entry.name not in names)
                    names.add(entry.name)
                    need(len(names) <= 64)
        need(time.monotonic() < deadline)
        return names

    def freeze_stages(self, path):
        names = self.stage_names(path)
        need(path not in self.checks or self.checks[path] == names)
        self.checks[path] = names
        return names

    def check(self):
        for parent, name, fd, original, regular in self.entries:
            pin = file_identity if regular else identity
            need(pin(os.fstat(fd)) == original == pin(os.stat(name, dir_fd=parent, follow_symlinks=False)))
        for _, fd, original, raw in self.files:
            need(os.pread(fd, len(raw) + 1, 0) == raw)
        for path, (names, maximum) in self.inventories.items():
            need(self.inventory(path, maximum) == names)
        for parent, name in self.absences:
            try:
                os.stat(name, dir_fd=parent, follow_symlinks=False)
            except FileNotFoundError:
                continue
            raise Refused()
        for path, names in self.checks.items():
            need(self.stage_names(path) == names)

    def manifest(self):
        return [{"path": path, "pin": list(pin), "sha256": sha(raw)}
                for path, _, pin, raw in self.files]

    def close(self):
        for fd in reversed(self.fds):
            os.close(fd)
        self.fds.clear()


def source_admission(raw):
    need(len(raw) == SOURCE_SIZE and sha(raw) == SOURCE_SHA)
    tree = ast.parse(raw)
    lines = raw.decode().splitlines()
    for function, line in FRAMES:
        need(any(isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)) and node.name == function
                 and node.lineno <= line <= node.end_lineno for node in ast.walk(tree)))
    need(lines[840].strip() == '_, raw = self.call_command("/usr/sbin/ioreg -a -r -d 1 -c IOPlatformExpertDevice")')
    need(lines[1015].strip() == 'vm = VM(context["selection"], lease_check, safety)')
    need(lines[864].strip() == 'result = command([self.ssh, cmd], timeout=timeout)')
    need(lines[280].strip() == 'need(child.returncode == 0)')
    command = next(n for n in tree.body if isinstance(n, ast.FunctionDef) and n.name == "command")
    owned_try = next(n for n in ast.walk(command) if isinstance(n, ast.Try) and n.finalbody)
    need(any(isinstance(n, ast.Call) and isinstance(n.func, ast.Name) and n.func.id == "join_original"
             for block in owned_try.finalbody for n in ast.walk(block)))
    # The digest fixes the complete call graph; these assertions document its
    # interpretation, rather than accepting a current file's shifted line numbers.
    need(owned_try.end_lineno < 281 and 'state = vm.call("prepare", private)' in lines[1018])
    return {"commit": COMMIT, "sha256": SOURCE_SHA, "bytes": SOURCE_SIZE,
            "first_ioreg_before_prepare": True, "original_local_child_joined": True,
            "guest_query_completion_originally_unknown": True}


def diagnostic_admission(raw):
    need(len(raw) <= LIMIT and b"\x00" not in raw)
    found = []
    for line in raw.decode("utf-8", "strict").splitlines():
        need(len(line) <= 16384)
        stripped = line.strip()
        # Normal private shell messages are permitted. Every diagnostic-looking
        # line must be the one exact structured diagnostic; no substring proof.
        if stripped.startswith(("{", "[")) or any(s in stripped for s in
                ('"exception"', '"stack"', "Traceback", "Refused", "Error:")):
            found.append(loads(stripped))
    need(found == [DIAGNOSTIC])
    return DIAGNOSTIC


@contextlib.contextmanager
def defer_signals(ignore=False):
    pending = []
    old = {s: signal.signal(s, signal.SIG_IGN if ignore else lambda n, f: pending.append(n)) for s in SIGNALS}
    try:
        yield
    finally:
        for s, handler in old.items():
            signal.signal(s, handler)
    if pending:
        raise Cancelled()


def command(argv, timeout):
    need(callable(getattr(os, "waitid", None)) and all(hasattr(os, name) for name in
             ("P_PID", "WEXITED", "WNOHANG", "WNOWAIT", "CLD_EXITED", "CLD_KILLED", "CLD_DUMPED")))
    need(signal.getsignal(signal.SIGCHLD) == signal.SIG_DFL)
    with tempfile.TemporaryFile() as out, tempfile.TemporaryFile() as err:
        child = None
        joined = completed = ownership_uncertain = join_started = False

        def observed(seconds):
            nonlocal completed, ownership_uncertain
            if completed:
                return True
            deadline = time.monotonic() + seconds
            while True:
                # Never use Popen.poll: it reaps and releases the PID/PGID pin.
                try:
                    need(child.returncode is None)
                    status = os.waitid(os.P_PID, child.pid, os.WEXITED | os.WNOHANG | os.WNOWAIT)
                    if status is not None and status.si_pid:
                        need(status.si_pid == child.pid and status.si_code in
                             (os.CLD_EXITED, os.CLD_KILLED, os.CLD_DUMPED))
                        completed = True
                        return True
                except (OSError, Refused):
                    ownership_uncertain = True
                    raise Refused() from None
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    return False
                time.sleep(min(0.05, remaining))

        def join(timeout=10):
            nonlocal joined, join_started
            # Called only after a positive non-reaping exit observation. No
            # numeric group signals are allowed after entering this function.
            join_started = True
            child.wait(timeout=timeout)
            joined = True

        try:
            with defer_signals():
                child = subprocess.Popen(argv, stdin=subprocess.DEVNULL, stdout=out, stderr=err,
                                         close_fds=True, start_new_session=True)
            if not observed(timeout):
                raise subprocess.TimeoutExpired(argv, timeout)
            # Cancellation between observation and join still enters cleanup
            # with an unreaped child; cleanup observes completion again.
            with defer_signals():
                join()
        finally:
            if child is not None and not joined and not join_started:
                with defer_signals(ignore=True):
                    need(not ownership_uncertain)
                    cleanup_deadline = time.monotonic() + 20
                    if not observed(0):
                        for sig in (signal.SIGTERM, signal.SIGKILL):
                            try:
                                os.killpg(child.pid, sig)
                            except ProcessLookupError:
                                pass  # Still require positive child exit/join.
                            if observed(max(0, min(10, cleanup_deadline - time.monotonic()))):
                                break
                        else:
                            raise Refused()
                    join(max(0, min(10, cleanup_deadline - time.monotonic())))
        need(child.returncode == 0 and os.fstat(out.fileno()).st_size <= LIMIT
             and os.fstat(err.fileno()).st_size <= LIMIT)
        return os.pread(out.fileno(), LIMIT + 1, 0)


def dead(pid):
    need(type(pid) is int and 1 < pid < 2**31)
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return True
    # EPERM and every other error are unknown; reused/live PIDs refuse too.
    return False


class ProcBSDInfo(ctypes.Structure):
    """Darwin's fixed-width PROC_PIDTBSDINFO ABI (not a kinfo_proc guess)."""
    _fields_ = [(name, ctypes.c_uint32) for name in
                ("flags", "status", "xstatus", "pid", "ppid", "uid", "gid", "ruid", "rgid",
                 "svuid", "svgid", "rfu")]
    _fields_ += [("comm", ctypes.c_char * 16), ("name", ctypes.c_char * 32)]
    _fields_ += [(name, ctypes.c_uint32) for name in
                 ("nfiles", "pgid", "pjobc", "tdev", "tpgid", "nice")]
    _fields_ += [("start_sec", ctypes.c_uint64), ("start_usec", ctypes.c_uint64)]


class ProcBSDShortInfo(ctypes.Structure):
    _fields_ = [(name, ctypes.c_uint32) for name in ("pid", "ppid", "pgid", "status")]
    _fields_ += [("comm", ctypes.c_char * 16)]
    _fields_ += [(name, ctypes.c_uint32) for name in
                ("flags", "uid", "gid", "ruid", "rgid", "svuid", "svgid", "rfu")]


def host_processes(lib=None, libc=None):
    """Bounded in-process activity snapshot of the configured namespace owner.

    The existing trusted configured-namespace model runs MG/VM controls as the
    invoking owner. Kernel real/effective/saved UIDs include temporarily assumed
    credentials. A process with all three UIDs foreign is outside that owner's
    source-owned MG activity (including unrelated protected root daemons); its
    argv need not be readable. Root's ability to replace this configuration or
    permanently transfer all credentials is already outside this trust model.
    Unknown kernel ownership, or unreadable relevant argv, always refuses.
    lib/libc are injectable C-call seams for synthetic tests, never CLI inputs.
    """
    need(sys.platform == "darwin" and os.geteuid() == os.getuid())
    need(ctypes.sizeof(ProcBSDInfo) == 136 and ProcBSDInfo.start_sec.offset == 120)
    need(ctypes.sizeof(ProcBSDShortInfo) == 64 and all(
        getattr(ProcBSDShortInfo, name).offset == offset for name, offset in
        (("pid", 0), ("ppid", 4), ("pgid", 8), ("status", 12), ("comm", 16),
         ("flags", 32), ("uid", 36), ("gid", 40), ("ruid", 44), ("rgid", 48),
         ("svuid", 52), ("svgid", 56), ("rfu", 60))))
    lib = lib if lib is not None else ctypes.CDLL("/usr/lib/libproc.dylib", use_errno=True)
    libc = libc if libc is not None else ctypes.CDLL(None, use_errno=True)
    lib.proc_listpids.argtypes = [ctypes.c_uint, ctypes.c_uint, ctypes.c_void_p, ctypes.c_int]
    lib.proc_listpids.restype = ctypes.c_int
    lib.proc_pidinfo.argtypes = [ctypes.c_int, ctypes.c_int, ctypes.c_uint64, ctypes.c_void_p, ctypes.c_int]
    lib.proc_pidinfo.restype = ctypes.c_int
    libc.sysctl.argtypes = [ctypes.POINTER(ctypes.c_int), ctypes.c_uint, ctypes.c_void_p,
                           ctypes.POINTER(ctypes.c_size_t), ctypes.c_void_p, ctypes.c_size_t]
    libc.sysctl.restype = ctypes.c_int

    def info(pid):
        value = ProcBSDInfo()
        ctypes.set_errno(0)
        size = lib.proc_pidinfo(pid, 3, 0, ctypes.byref(value), ctypes.sizeof(value))  # PROC_PIDTBSDINFO
        if size == 0 and ctypes.get_errno() == errno.ESRCH:
            return None  # Positive kernel report of exit, never EPERM/unknown.
        need(size == ctypes.sizeof(value) and value.pid == pid
             and 0 < value.status <= 5 and value.start_sec > 0 and value.start_usec < 1000000)
        return (value.pid, value.start_sec, value.start_usec,
                value.uid, value.ruid, value.svuid, value.gid, value.rgid, value.svgid,
                value.ppid, value.pgid, value.comm, value.name)

    own_pid = os.getpid()
    own = info(own_pid)
    need(own is not None and own[3:6] == (os.getuid(),) * 3)
    owner = own[3]  # Positive kernel identity, cross-checked against this caller.

    def relevant(pin):
        return owner in pin[3:6]

    def short_credentials(pid):
        value = ProcBSDShortInfo()
        ctypes.set_errno(0)
        size = lib.proc_pidinfo(pid, 13, 0, ctypes.byref(value), ctypes.sizeof(value))
        if size == 0 and ctypes.get_errno() == errno.ESRCH:
            return None
        need(size == ctypes.sizeof(value) and value.pid == pid and 0 < value.status <= 5
             and all(uid < 2**32 - 1 for uid in (value.uid, value.ruid, value.svuid)))
        return (value.uid, value.ruid, value.svuid, value.gid, value.rgid, value.svgid)

    def pids():
        values = (ctypes.c_int * 32768)()
        size = lib.proc_listpids(1, 0, values, ctypes.sizeof(values))  # PROC_ALL_PIDS; identity first, argv only if relevant.
        need(0 < size < ctypes.sizeof(values) and size % ctypes.sizeof(ctypes.c_int) == 0)
        rows = list(values[:size // ctypes.sizeof(ctypes.c_int)])
        need(all(0 <= pid < 2**31 for pid in rows))
        result = [pid for pid in rows if pid > 0]
        need(len(result) == len(set(result)) and own_pid in result)
        return set(result)

    def argv(pid):
        mib = (ctypes.c_int * 3)(1, 49, pid)  # CTL_KERN, KERN_PROCARGS2
        data = ctypes.create_string_buffer(262144)
        size = ctypes.c_size_t(len(data))
        ctypes.set_errno(0)
        rc = libc.sysctl(mib, 3, data, ctypes.byref(size), None, 0)
        if rc == -1 and ctypes.get_errno() == errno.ESRCH:
            need(info(pid) is None)
            return None
        need(rc == 0 and 4 < size.value < len(data))
        raw = data.raw[:size.value]
        nargs = struct.unpack("=i", raw[:4])[0]
        need(0 < nargs < 16384)
        executable, sep, args = raw[4:].partition(b"\x00")
        need(sep and executable.startswith(b"/"))
        args = args.lstrip(b"\x00")
        words, position = [], 0
        for _ in range(nargs):
            end = args.find(b"\x00", position)
            need(end >= position)
            words.append(args[position:end])
            position = end + 1
        need(words[0])
        return b" ".join(words).decode("utf-8", "strict")

    pins, result, total = {}, {}, 0

    def capture(pid, before):
        nonlocal total
        args = argv(pid)
        after = info(pid)
        if after is None:
            return None
        need(after == before and args is not None)
        total += len(args.encode("utf-8"))
        need(total <= LIMIT)
        return args

    for pid in sorted(pids()):
        credentials = short_credentials(pid)
        if credentials is None or owner not in credentials[:3]:
            continue  # Positive exit or positively foreign; neither creates a pin.
        before = info(pid)
        if before is None:
            continue
        need(before[3:9] == credentials)
        pins[pid] = before
        need(pid != own_pid or before == own)
        args = capture(pid, before)
        if args is not None:
            result[pid] = args
    # Revalidate pinned lifetimes and capture new relevant arrivals for activity
    # classification, sharing the first pass's total argv-byte budget.
    # Foreign creation/exit does not invalidate the snapshot. Missing relevant
    # PIDs still require ESRCH; an omitted list entry alone is not exit proof.
    for pid in sorted(pids() | set(pins)):
        if pid not in pins:
            # Never cache a foreign classification across enumerations. Pinned
            # lifetimes bypass this exclusion and always require FULL revalidation.
            credentials = short_credentials(pid)
            if credentials is None or owner not in credentials[:3]:
                continue
        current = info(pid)
        if current is None:
            need(pid != own_pid)
            result.pop(pid, None)
            continue
        if pid not in pins:
            need(current[3:9] == credentials)
        if pid in pins:
            need(current == pins[pid] and pid in result)
            args = argv(pid)
            after = info(pid)
            if after is None:
                result.pop(pid, None)
            else:
                need(after == current and args == result[pid])
        elif relevant(current):
            args = capture(pid, current)
            if args is not None:
                result[pid] = args
    need(info(own_pid) == own and own_pid in result)
    return result


def host_activity(processes, selection):
    counts = {"host_jobs": 0, "host_workers": 0, "host_controls": 0}
    jobs = r"(?:^|[/ ])(?:mg|tools/mg)\s+(?:" + "|".join(VM_JOBS) + r")(?:\s|$)"
    for pid, args in processes.items():
        if pid == os.getpid():
            continue
        # Host-only builds, physical-device jobs and the ordinary host app
        # have no dedicated-VM ownership. They are neither stopped nor claimed.
        if re.search(jobs, args) and "--physical-device" not in args:
            counts["host_jobs"] += 1
        if re.search(r"vm-xcode-stage[.]py|sim-record[.]sh|vnc-holder[.]py|duo-fold[.]sh|sim-pool[.]sh", args):
            counts["host_workers"] += 1
        if selection["ssh"] in args or selection["tart"] in args and not re.search(r"\brun\b", args):
            counts["host_controls"] += 1
        if selection["tart"] in args and "--vnc-experimental" in args:
            counts["host_controls"] += 1
        if selection["name"] in args and re.search(r"(?:^|[/ ])tart(?:\s|$)", args) and \
                (not re.search(r"\brun\b", args) or "--vnc-experimental" in args):
            counts["host_controls"] += 1
    need(not any(counts.values()))
    return counts


# Fixed read-only guest program. Metadata with no positive source-owned
# lifetime proof refuses. Expired leases are classified by expires_epoch,
# never removed; idle booted simulators alone are not activity.
GUEST_READ = r'''
import json, os, re, stat, subprocess, tempfile, time
LIMIT = 1048576
F = os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK
D = os.O_RDONLY | os.O_NOFOLLOW | os.O_DIRECTORY
fds = []
bindings, contents, listings = [], [], {}
def need(x):
    if not x: raise ValueError()
def pin(s, regular=False):
    value = (s.st_dev, s.st_ino, s.st_mode, s.st_uid, s.st_gid)
    return value + (s.st_nlink, s.st_size, s.st_mtime_ns, s.st_ctime_ns) if regular else value
def child(parent, name):
    fd = os.open(name, D, dir_fd=parent); fds.append(fd)
    s = os.fstat(fd)
    need(s.st_uid in (0, os.getuid()) and (not s.st_mode & 0o022 or s.st_uid == 0 and s.st_mode & stat.S_ISVTX))
    bindings.append((parent, name, fd, pin(s), False))
    return fd
def directory(path):
    fd = os.open('/', D); fds.append(fd)
    for name in path.split('/')[1:]:
        fd = child(fd, name)
    return fd
def names(fd):
    result = []
    with os.scandir(fd) as entries:
        for e in entries:
            result.append(e.name); need(len(result) <= 256)
    if fd in listings: need(set(result) == listings[fd])
    listings[fd] = set(result)
    return result
def read(fd, name):
    f = os.open(name, F, dir_fd=fd); fds.append(f)
    s = os.fstat(f)
    need(stat.S_ISREG(s.st_mode) and s.st_nlink == 1 and s.st_uid == os.getuid() and not s.st_mode & 0o022 and s.st_size <= 16384)
    raw = os.pread(f, 16385, 0)
    need(len(raw) == s.st_size)
    bindings.append((fd, name, f, pin(s, True), True)); contents.append((f, raw))
    def pairs(items):
        value = {}
        for key, item in items:
            need(key not in value); value[key] = item
        return value
    return json.loads(raw, object_pairs_hook=pairs)
try:
    with tempfile.TemporaryFile() as out:
        subprocess.run(['/bin/ps', '-ww', '-A', '-o', 'pid=', '-o', 'ppid=', '-o', 'command='], stdout=out, check=True, timeout=5)
        need(os.fstat(out.fileno()).st_size <= LIMIT)
        rows = {}
        for line in os.pread(out.fileno(), LIMIT + 1, 0).decode().splitlines():
            fields = line.strip().split(None, 2)
            need(len(fields) == 3 and fields[0].isdigit() and fields[1].isdigit())
            pid, parent = map(int, fields[:2]); need(pid not in rows)
            rows[pid] = (parent, fields[2])
    own = set(); pid = os.getpid()
    need(pid in rows)
    while pid and pid not in own:
        own.add(pid); pid = rows.get(pid, (0, ''))[0]
    pattern = re.compile(PATTERN)
    managed = sum(bool(pattern.search(args)) for pid, (_, args) in rows.items() if pid not in own)
    pool = os.path.expanduser('~/Library/Caches/meguri/sim-pool')
    active = 0
    try: poolfd = directory(pool)
    except FileNotFoundError:
        # A missing pool is unknown: this exact dedicated VM already had one.
        raise ValueError()
    need('pool.lock' not in names(poolfd))
    leases = child(poolfd, 'leases')
    for name in names(leases):
        need(re.fullmatch('[0-9A-Fa-f-]{36}', name))
        fd = child(leases, name)
        need(set(names(fd)) == {'lease.json'})
        lease = read(fd, 'lease.json')
        exp = lease.get('expires_epoch')
        required = {'udid','kind','holder','worktree','cwd','pid','ttl_min','acquired_at','expires_at','acquired_epoch','expires_epoch'}
        need(required <= set(lease) <= required | {'vm','renewed_at'} and lease['udid'] == name)
        need(all(isinstance(lease[k], str) and lease[k] for k in ('kind','holder','worktree','cwd','acquired_at','expires_at')))
        need(type(exp) is int and exp > 0 and type(lease['acquired_epoch']) is int and 0 < lease['acquired_epoch'] < exp)
        need(type(lease['pid']) is int and lease['pid'] > 1 and type(lease['ttl_min']) is int and lease['ttl_min'] > 0)
        active += exp > time.time()
    need(managed == 0 and active == 0)
    for parent, name, fd, original, regular in bindings:
        need(pin(os.fstat(fd), regular) == original == pin(os.stat(name, dir_fd=parent, follow_symlinks=False), regular))
    for fd, raw in contents: need(os.pread(fd, len(raw) + 1, 0) == raw)
    for fd in list(listings): names(fd)
    print(json.dumps({'guest_managed': managed, 'simulator_leases': active, 'inspection_complete': True}))
except BaseException:
    print(json.dumps({'inspection_complete': False}))
    raise SystemExit(1)
finally:
    for fd in reversed(fds): os.close(fd)
'''.replace('PATTERN', repr(MANAGED + '|' + EXTRA))


class System:
    def processes(self):
        return host_processes()

    def guest(self, selection):
        launcher = "import base64;exec(base64.b64decode(" + repr(base64.b64encode(GUEST_READ.encode()).decode()) + "))"
        cmd = "/usr/bin/python3 -B -I -c " + shlex.quote(launcher)
        value = loads(command([selection["ssh"], cmd], 30))
        need(value == {"guest_managed": 0, "simulator_leases": 0, "inspection_complete": True})
        return value

    def state(self, selection):
        records = loads(command([selection["tart"], "list", "--format", "json"], 10))
        need(isinstance(records, list) and len(records) <= 64 and all(isinstance(r, dict) for r in records))
        matches = [r for r in records if r.get("Name") == selection["name"]]
        need(len(matches) == 1 and matches[0].get("State") in ("running", "stopped"))
        # Configured namespace binding only. Preserve any returned metadata
        # privately and require that it does not change except for State.
        value = dict(matches[0]); state = value.pop("State")
        return state, value

    def stop(self, selection):
        command([selection["tart"], "stop", selection["name"], "--timeout", "60"], 75)

    def archive(self, source_fd, source_name, target_fd):
        libc = ctypes.CDLL(None, use_errno=True)
        fn = libc.renameatx_np if sys.platform == "darwin" else libc.renameat2
        fn.argtypes = [ctypes.c_int, ctypes.c_char_p, ctypes.c_int, ctypes.c_char_p, ctypes.c_uint]
        fn.restype = ctypes.c_int
        need(fn(source_fd, os.fsencode(source_name), target_fd, b"archived-lease",
                4 if sys.platform == "darwin" else 1) == 0)


def write_new(fd, name, value):
    f = os.open(name, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC, 0o600, dir_fd=fd)
    try:
        raw = encoded(value)
        with os.fdopen(f, "wb", closefd=False) as out:
            out.write(raw)
            out.flush()
            os.fsync(f)
    finally:
        os.close(f)
    os.fsync(fd)


class Recovery:
    def __init__(self, route, system=None):
        need(set(route) == {"root", "config", "selection", "lease_base", "temp_root", "recovery_run"})
        self.route, self.system = route, system or System()
        self.pins = Pins()
        self.claimed = self.stopped = self.proved = self.archived = False
        self.activity = {}
        self.claim_path = None
        self.evidence = None

    def report(self, outcome):
        return {"scope": SCOPE, "stage": "EXACT-PRE-PREPARE-STOP-CONFIRM-ARCHIVE",
                "outcome": outcome, "original_run": RUN,
                "checks": {"claim_installed": self.claimed, "original_stop_joined": self.stopped,
                           "separate_proof_durable": self.proved, "whole_lease_archived": self.archived},
                "counts": {k: v for k, v in self.activity.items() if type(v) is int}}

    @contextlib.contextmanager
    def mutex(self):
        self.pins.check()
        fd = os.open(self.lease + ".mutex", os.O_RDWR | os.O_NOFOLLOW | os.O_NONBLOCK | os.O_CLOEXEC)
        try:
            s = os.fstat(fd)
            need(stat.S_ISREG(s.st_mode) and s.st_nlink == 1 and s.st_uid == os.getuid() and not s.st_mode & 0o022)
            limit = time.monotonic() + 30
            while True:
                try:
                    fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
                    break
                except BlockingIOError:
                    need(time.monotonic() < limit)
                    time.sleep(0.05)
            need(file_identity(os.fstat(fd)) == file_identity(os.stat(self.lease + ".mutex", follow_symlinks=False)))
            self.pins.check()
            yield
        finally:
            os.close(fd)

    def admit(self):
        r, p = self.route, self.pins
        self.root = r["root"]
        config = loads(p.file(r["config"], 65536))
        need(isinstance(config, dict) and isinstance(config.get("vms"), list) and 0 < len(config["vms"]) <= 32)
        selection = []
        for entry in config["vms"]:
            entry = {"name": entry} if isinstance(entry, str) else entry
            need(isinstance(entry, dict))
            vm = {"name": entry.get("name"), "ssh": entry.get("ssh", config.get("ssh")),
                  "tart": entry.get("tart", config.get("tart"))}
            need(all(isinstance(v, str) and v for v in vm.values()) and re.fullmatch(r"[A-Za-z0-9._-]+", vm["name"]))
            selection.append(vm)
        need(r["selection"] == selection[0] and len({vm["name"] for vm in selection}) == len(selection))
        self.vm = selection[0]
        for key in ("ssh", "tart"):
            p.file(self.vm[key], 262144, executable=True)
        source = str(Path(__file__).parents[1] / "tests/fixtures/vm_xcode_94582_original_helper.py")
        self.source_proof = source_admission(p.file(source, SOURCE_SIZE))
        receipt_dir = self.root + "/build/mg/vm-xcode-stage/" + RUN
        result = loads(p.file(receipt_dir + "/result.json", 262144))
        need(result["job"] == "vm-xcode-stage" and result["run_id"] == RUN and result["root"] == self.root
             and result["status"] == "error" and result["dry_run"] is False and result["args"] == []
             and result["commit"]["sha"] == COMMIT and result["finished_at"] == "2026-11-02T20:02:40Z"
             and result["summary"] == {"total": 0, "passed": 0, "failed": 0, "skipped": 0})
        report_path = receipt_dir + "/stage-report.json"
        need(sum(a.get("path") == report_path and a.get("kind") == "json" for a in result["artifacts"]) == 1)
        need(loads(p.file(report_path, 16384)) == {"scope": SCOPE, "outcome": "refused", "checks": {}})
        self.lease = r["lease_base"] + "/mac-vm." + self.vm["name"] + ".lock"
        self.guard = self.lease + "/vm-xcode-pending"
        # Pin the mutex already used by host_lease_lock. If it is absent, evidence
        # is incomplete even for inspection; creating a replacement is forbidden.
        p.file(self.lease + ".mutex", 32)
        p.freeze(self.lease, {"holder.json", "vm-xcode-pending"})
        p.freeze(self.guard, set())
        holder = loads(p.file(self.lease + "/holder.json", 16384))
        need(set(holder) == {"job", "run_id", "pid", "worktree", "started_at"}
             and holder["job"] == "vm-xcode-stage" and holder["run_id"] == RUN
             and holder["worktree"] == self.root and isinstance(holder["started_at"], str))
        self.holder_pid = holder["pid"]
        # Retain only the relevant discovery set in the trusted temp root.
        candidates = []
        for name in sorted(p.freeze_stages(r["temp_root"])):
            directory = r["temp_root"] + "/" + name
            p.directory(directory)
            raw = p.file(directory + "/context.json", 65536, optional=True)
            if raw is None:
                continue  # Absence is pinned; it says nothing about activity.
            context = loads(raw)
            if (isinstance(context, dict) and set(context) == {"private"}
                    and isinstance(context["private"], str) and context["private"] == directory):
                continue  # SourceCheckOnly is pinned, but is not a run candidate.
            need(isinstance(context, dict) and isinstance(context.get("run"), str) and context["run"])
            if context["run"] == RUN:
                candidates.append((directory, context))
        need(len(candidates) == 1)
        private, context = candidates[0]
        need(context == {"selection": dict(self.vm, state="running"), "lease_slot": self.lease,
                         "root": self.root, "run": RUN, "private": private})
        p.freeze(private, {"context.json", "helpers.log", "worker.pid", "state"})
        p.freeze(private + "/state")  # names only; never scan unrelated private payload
        self.diagnostic = diagnostic_admission(p.file(private + "/helpers.log"))
        worker = p.file(private + "/worker.pid", 32)
        need(re.fullmatch(rb"[1-9][0-9]{0,9}\n", worker))
        self.worker_pid = int(worker)
        self.original_private = private
        p.check()
        with self.mutex():
            self.exclusion()
        # Evidence/archive created beside the original lease: same filesystem,
        # private and no-replace. Never create or alter an original directory.
        need(re.fullmatch(r"[0-9]{8}T[0-9]{6}Z-[0-9]+", r["recovery_run"]))
        self.evidence = r["lease_base"] + "/vm-xcode-recovery." + r["recovery_run"]
        base = p.directory(r["lease_base"])
        os.mkdir(os.path.basename(self.evidence), 0o700, dir_fd=base)
        self.evidence_fd = p.directory(self.evidence)
        os.fsync(base)
        write_new(self.evidence_fd, "admission.json", {"original_run": RUN, "original_private": private,
                  "pins": p.manifest(), "source": self.source_proof, "diagnostic": self.diagnostic,
                  "binding": "existing trusted root-controlled configured dedicated VM namespace; not an incarnation pin",
                  "historical_identity": None, "original_query_completion": "unknown"})
        p.file(self.evidence + "/admission.json", LIMIT)

    def exclusion(self):
        self.pins.check()
        need(dead(self.holder_pid) and dead(self.worker_pid))
        need(self.pins.inventory(self.lease) == {"holder.json", "vm-xcode-pending"})
        expected = {"recovery"} if self.claimed else set()
        need(self.pins.inventory(self.guard) == expected)
        if self.claimed:
            need(self.pins.inventory(self.claim_path) == {"claim.json"})

    def metadata_activity(self):
        p = self.pins
        # The fold helper retains a holder pid after leaving the exclusive
        # start lease. Missing/unknown partial metadata refuses, without cleanup.
        fold = str(Path(self.route["lease_base"]).parent / "vm-fold")
        try:
            names = p.inventory(fold, 32)
        except FileNotFoundError:
            names = set()
        if "holder.pid" in names:
            raw = p.file(fold + "/holder.pid", 32)
            need(re.fullmatch(rb"[1-9][0-9]{0,9}\n?", raw) and dead(int(raw)))
        else:
            need("commands" not in names)
        active = self.root + "/build/mg/record/active"
        try:
            names = p.inventory(active, 128)
        except FileNotFoundError:
            names = set()
        for name in names:
            need(re.fullmatch(r"[0-9A-Fa-f-]{36}\.json", name))
            state = loads(p.file(active + "/" + name, 65536))
            required = {"udid", "pid", "pid_started", "video", "log", "status", "started_epoch",
                        "run_id", "max_seconds", "lease_dir", "sim_runner"}
            need(required <= set(state) <= required | {"vm_ssh"})
            need(state["udid"] + ".json" == name and state["sim_runner"] == "vm"
                 and state.get("vm_ssh") == self.vm["ssh"] and state["lease_dir"] == self.lease)
            need(all(isinstance(state[k], str) and state[k] for k in
                     ("pid_started", "video", "log", "status", "run_id")))
            need(type(state["started_epoch"]) is int and state["started_epoch"] > 0
                 and type(state["max_seconds"]) is int and state["max_seconds"] > 0 and dead(state["pid"]))
        return {"fold_sessions": 0, "recordings": 0}

    def host(self):
        result = host_activity(self.system.processes(), self.vm)
        result.update(self.metadata_activity())
        need(dead(self.holder_pid) and dead(self.worker_pid))
        return result

    def claim(self):
        with self.mutex():
            self.exclusion()
            self.activity.update(self.host())
            # mkdir is the atomic exclusive claim; a crash before claim.json
            # leaves an unknown claim that every later invocation refuses.
            os.mkdir("recovery", 0o700, dir_fd=self.pins.directory(self.guard))
            self.claimed = True
            self.claim_path = self.guard + "/recovery"
            self.pins.inventories[self.guard] = ({"recovery"}, 128)
            fd = self.pins.directory(self.claim_path)
            write_new(fd, "claim.json", {"original_run": RUN, "recovery_run": self.route["recovery_run"],
                                       "evidence": self.evidence, "pid": os.getpid()})
            self.pins.freeze(self.claim_path, {"claim.json"})
            self.pins.file(self.claim_path + "/claim.json", 16384)
            os.fsync(self.pins.directory(self.guard))
            os.fsync(self.pins.directory(self.lease))

    def execute(self, mode):
        need(mode in ("--inspect", "--stop-and-archive"))
        self.admit()
        self.activity.update(self.host())
        state, namespace = self.system.state(self.vm)
        need(state == "running")
        self.pins.check()
        if mode == "--stop-and-archive":
            self.claim()
        self.activity.update(self.system.guest(self.vm))
        self.pins.check()
        if mode == "--inspect":
            with self.mutex():
                self.exclusion()
                self.activity.update(self.host())
            return self.report("inspected")
        # Fresh read checks immediately before control. Mutex is closed before
        # every worker/SSH/Tart launch; the fixed guard plus claim persists.
        need(self.system.state(self.vm) == ("running", namespace))
        self.activity.update(self.system.guest(self.vm))
        with self.mutex():
            self.exclusion()
            self.activity.update(self.host())
        self.pins.check()
        write_new(self.evidence_fd, "stop-intent.json", {"original_run": RUN, "claim": self.claim_path,
                                                       "activity": self.activity})
        self.pins.file(self.evidence + "/stop-intent.json", LIMIT)
        self.pins.check()
        with self.mutex():
            self.exclusion()
            self.activity.update(self.host())
        self.system.stop(self.vm)  # exactly one original bounded stop and join
        self.stopped = True
        self.pins.check()
        state_after, namespace_after = self.system.state(self.vm)
        need(state_after == "stopped" and namespace_after == namespace)
        self.pins.check()
        write_new(self.evidence_fd, "recovery-proof.json", {
            "original_run": RUN, "original_receipt_unchanged": True, "original_private": self.original_private,
            "source": self.source_proof, "diagnostic": self.diagnostic, "pins": self.pins.manifest(),
            "exclusive_claim": self.claim_path, "no_active_work": self.activity,
            "original_stop_completed_and_joined": True, "independent_state": "stopped",
            "namespace_metadata": namespace_after,
            "binding": "existing trusted root-controlled configured dedicated VM namespace; not an incarnation pin",
            "historical_identity": None, "original_query_completion": "unknown",
            "abandoned_guest_query_closed_by_confirmed_hypervisor_stop": True})
        self.proved = True
        self.pins.file(self.evidence + "/recovery-proof.json", LIMIT)
        self.pins.freeze(self.evidence, {"admission.json", "stop-intent.json", "recovery-proof.json"})
        need(self.system.state(self.vm) == ("stopped", namespace))
        with self.mutex():
            self.exclusion()
            self.activity.update(self.host())
            self.pins.check()
            self.system.archive(self.pins.directory(self.route["lease_base"]),
                                os.path.basename(self.lease), self.evidence_fd)
            self.archived = True
            os.fsync(self.evidence_fd)
            os.fsync(self.pins.directory(self.route["lease_base"]))
        return self.report("archived")


def main(diagnose=private_failure):
    need(sys.platform == "darwin")
    need(len(sys.argv) == 4 and sys.argv[1:3] == ["--stage-run", RUN]
         and sys.argv[3] in ("--inspect", "--stop-and-archive"))
    for s in SIGNALS:
        signal.signal(s, lambda *_: (_ for _ in ()).throw(Cancelled()))
    with os.fdopen(os.dup(3), "rb") as context:
        raw = context.read(65537)
    need(len(raw) <= 65536)
    recovery = Recovery(loads(raw))
    try:
        report = recovery.execute(sys.argv[3])
        # Failure to publish the report remains an error after the lease is retired.
        print(json.dumps(report), flush=True)
        return 0
    except (Exception, Cancelled) as error:
        diagnose(error)
        print(json.dumps(recovery.report("refused")), flush=True)
        return 1
    finally:
        recovery.pins.close()


def run():
    diagnosed = False

    def diagnose(error):
        nonlocal diagnosed
        if not diagnosed:
            diagnosed = True
            private_failure(error)

    try:
        return main(diagnose)
    except (Exception, Cancelled) as error:
        diagnose(error)
        # Public output never contains exception text, paths or process arguments.
        return 1


if __name__ == "__main__":
    sys.exit(run())
