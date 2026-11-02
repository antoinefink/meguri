#!/usr/bin/env python3
"""MG-only Xcode staging. FD 3 is private lease/route context, never an option.

The same source runs in the guest over SSH. Only its fixed public report may
leave private state. Interrupted copies and private diagnostics are retained;
there is deliberately no recursive cleanup and no provisioning operation.
"""
import base64
import contextlib
import ctypes
import grp
import hashlib
import json
import os
from pathlib import Path
import plistlib
import posixpath
import pwd
import re
import shlex
import signal
import stat
import subprocess
import sys
import tarfile
import tempfile
import time
import uuid

GIB = 1024 ** 3
SCOPE = "VM-XCODE-MAINTENANCE-ONLY"
SOURCE_SCOPE = "SOURCE-CHECK-ONLY"
DIR_FLAGS = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW
FILE_FLAGS = os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK
SHUTDOWN = 10
COPY_WAIT = 3600


class Refused(Exception):
    pass


class Cancelled(BaseException):
    pass


class Uncertain(Refused):
    pass


def need(value):
    if not value:
        raise Refused()


def encoded(value):
    return base64.b64encode(json.dumps(value).encode()).decode()


def digest(value):
    return hashlib.sha256(json.dumps(value, sort_keys=True, separators=(",", ":")).encode()).hexdigest()


def pin(s):
    # APFS directory nlink/mtime change as children are created.
    return [s.st_dev, s.st_ino, s.st_mode, s.st_uid, s.st_gid]


def file_pin(s):
    return pin(s) + [s.st_size, s.st_mtime_ns, s.st_ctime_ns, s.st_nlink]


class Directory:
    def __init__(self, path):
        need(os.path.isabs(path) and os.path.normpath(path) == path)
        self.path, self.fds, self.bindings = path, [], []
        try:
            fd = os.open("/", DIR_FLAGS)
            self.fds.append(fd)
            for name in path.split("/")[1:]:
                child = os.open(name, DIR_FLAGS, dir_fd=fd)
                s = os.fstat(child)
                need(s.st_uid in (0, os.getuid()) and
                     (not s.st_mode & 0o002 or s.st_uid == 0 and s.st_mode & stat.S_ISVTX))
                self.bindings.append((fd, name, child, pin(s)))
                self.fds.append(child)
                fd = child
            self.fd = fd
            self.check()
        except BaseException:
            self.close()
            raise

    def check(self, expected=None):
        current = []
        for parent, name, fd, original in self.bindings:
            need(pin(os.fstat(fd)) == original == pin(os.stat(name, dir_fd=parent, follow_symlinks=False)))
            current.append(original)
        if expected is not None:
            need(current == expected)
        return current

    def close(self):
        for fd in reversed(self.fds):
            os.close(fd)
        self.fds = []

    def __enter__(self):
        return self

    def __exit__(self, *args):
        self.close()


class Original:
    def __init__(self, path, parse=True, own=True):
        self.path = path
        self.parent = Directory(os.path.dirname(path))
        self.name = os.path.basename(path)
        self.fd = os.open(self.name, FILE_FLAGS, dir_fd=self.parent.fd)
        s = os.fstat(self.fd)
        need(stat.S_ISREG(s.st_mode) and s.st_nlink == 1 and s.st_uid in ((os.getuid(),) if own else (0, os.getuid()))
             and not s.st_mode & 0o022 and s.st_size <= 1024 * 1024)
        self.pin = file_pin(s)
        self.raw = os.read(self.fd, 1024 * 1024 + 1)
        self.value = json.loads(self.raw) if parse else None
        self.check()

    def check(self):
        self.parent.check()
        need(file_pin(os.fstat(self.fd)) == self.pin ==
             file_pin(os.stat(self.name, dir_fd=self.parent.fd, follow_symlinks=False)))
        os.lseek(self.fd, 0, 0)
        need(os.read(self.fd, 1024 * 1024 + 1) == self.raw)

    def close(self):
        os.close(self.fd)
        self.parent.close()


@contextlib.contextmanager
def launching():
    # Same launch deferral as cloud-agent-handoff's setup_launch: retain the
    # original handle even if cancellation arrives inside Popen.
    pending = []
    saved = {s: signal.signal(s, lambda number, frame: pending.append(number))
             for s in (signal.SIGTERM, signal.SIGHUP, signal.SIGINT)}
    try:
        yield
    finally:
        for s, handler in saved.items():
            signal.signal(s, handler)
    if pending:
        raise Cancelled()


@contextlib.contextmanager
def finishing():
    saved = {s: signal.signal(s, signal.SIG_IGN) for s in (signal.SIGTERM, signal.SIGHUP, signal.SIGINT)}
    try:
        yield
    finally:
        for s, handler in saved.items():
            signal.signal(s, handler)


def join_original(child):
    with finishing():
        for sig in (signal.SIGTERM, signal.SIGKILL):
            try:
                os.killpg(child.pid, sig)
            except ProcessLookupError:
                pass
            except OSError:
                raise Uncertain() from None
            try:
                child.wait(timeout=SHUTDOWN)
            except subprocess.TimeoutExpired:
                if sig == signal.SIGKILL:
                    raise Uncertain() from None


def retain_source_failure(directory, argv, returncode, out, err):
    """Only the original failed source verification, never a public artifact."""
    need(type(directory) is Directory)
    directory.check()
    s = os.fstat(directory.fd)
    need(s.st_uid == os.getuid() and stat.S_IMODE(s.st_mode) == 0o700)
    limit, streams = 1024 * 1024, {}
    for name, stream in (("stdout", out), ("stderr", err)):
        directory.check()
        original_bytes = os.fstat(stream.fileno()).st_size
        fd = os.open("source-command." + name, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW,
                     0o600, dir_fd=directory.fd)
        retained_bytes = 0
        with os.fdopen(fd, "wb") as retained:
            stream.seek(0)
            while retained_bytes < limit:
                raw = stream.read(min(64 * 1024, limit - retained_bytes))
                if not raw:
                    break
                retained.write(raw)
                retained_bytes += len(raw)
        streams[name] = {"original_bytes": original_bytes, "retained_bytes": retained_bytes,
                         "overflow": original_bytes > limit}
    write_new(directory, "source-command.json", {"argv": argv, "returncode": returncode,
                                                 "streams": streams})
    directory.check()


def command(argv, *, data=None, pass_fds=(), check=True, env=None, timeout=180,
            source_diagnostics=None):
    """One original owned session; inherited output pipes cannot block join."""
    with tempfile.TemporaryFile() as out, tempfile.TemporaryFile() as err:
        child = None
        try:
            with launching():
                child = subprocess.Popen(argv, stdin=subprocess.PIPE if data is not None else subprocess.DEVNULL,
                                         stdout=out, stderr=err, pass_fds=pass_fds, env=env,
                                         start_new_session=True, bufsize=0)
            child.communicate(data, timeout=timeout)
        except subprocess.TimeoutExpired:
            raise Refused() from None
        finally:
            if child is not None:
                try:
                    join_original(child)
                finally:
                    if child.stdin is not None:
                        child.stdin.close()
        if child.returncode != 0 and source_diagnostics is not None:
            retain_source_failure(source_diagnostics, argv, child.returncode, out, err)
        if check:
            need(child.returncode == 0)
        need(os.fstat(out.fileno()).st_size <= 1024 * 1024 and
             os.fstat(err.fileno()).st_size <= 1024 * 1024)
        out.seek(0)
        return child.returncode, out.read(1024 * 1024 + 1)


def links_inside(entries):
    # Resolve link text against the inventory, never against the filesystem.
    for name, entry in entries.items():
        if entry[0] != "link":
            continue
        pending, resolved, traversed = name.split("/"), [], 0
        while pending:
            part = pending.pop(0)
            if part in ("", "."):
                continue
            if part == "..":
                need(resolved)
                resolved.pop()
                continue
            resolved.append(part)
            item = entries.get("/".join(resolved))
            if item is not None and item[0] == "link":
                traversed += 1
                need(traversed <= 100 and item[2] and not item[2].startswith("/"))
                resolved.pop()
                pending = item[2].split("/") + pending


class InstalledSource:
    """Local OS identity, retained only for this selected source descriptor."""
    def __init__(self, directory):
        self.directory, self.identity = directory, None

    @staticmethod
    def system_identity():
        need(sys.platform == "darwin")
        root = pwd.getpwnam("root")
        wheel = grp.getgrnam("wheel")
        need(root.pw_name == "root" and root.pw_uid == 0 and root.pw_gid == 0 and
             wheel.gr_name == "wheel" and wheel.gr_gid == 0 and
             pwd.getpwuid(0) == root and grp.getgrgid(0) == wheel)
        members = tuple(pwd.getpwnam(name) for name in wheel.gr_mem)
        need(all(member.pw_uid == 0 for member in members))
        credentials = (os.getuid(), os.geteuid(), os.getgid(), os.getegid(), tuple(sorted(os.getgroups())))
        need(credentials[0] == credentials[1] and
             (credentials[0] == 0 or 0 not in (credentials[2], credentials[3], *credentials[4])))
        return (tuple(root), tuple(wheel[:3]) + (tuple(wheel.gr_mem),),
                tuple(tuple(member) for member in members), credentials)

    def check(self):
        self.directory.check()
        if self.identity is not None:
            need(self.system_identity() == self.identity)

    def group_write(self, s):
        identity = self.system_identity()
        if self.identity is None:
            self.identity = identity
        need(identity == self.identity)
        root = os.fstat(self.directory.fd)
        need(root.st_uid == 0 and root.st_gid == 0 and not root.st_mode & 0o6002 and
             s.st_uid == 0 and s.st_gid == 0)


def copied_tree_check(root, directory):
    # root is the retained, locally opened guest_root(), never archive/context
    # ownership or a caller assertion. Check its binding through the tree too.
    need(type(root) is Directory and type(directory) is Directory and
         directory.path.startswith(root.path + "/"))
    root_pins = root.check()
    tree_pins = directory.check()
    s = os.fstat(root.fd)
    need(s.st_uid == os.getuid() and stat.S_IMODE(s.st_mode) == 0o700)
    count = len(root.bindings)
    need(tree_pins[:count] == root_pins)
    for _, _, fd, _ in directory.bindings[count:]:
        s = os.fstat(fd)
        need(s.st_uid == os.getuid() and stat.S_IMODE(s.st_mode) == 0o700)


def entry_mode(mode, symlink, *, installed_source=None, private_root=None, metadata=None):
    need(private_root is None or type(private_root) is Directory)
    need(not mode & 0o6000)
    if symlink:  # Symlink permission bits do not grant access to their target.
        return
    need(not mode & 0o002)
    if mode & 0o020:
        if private_root is None:
            need(type(installed_source) is InstalledSource and metadata is not None)
            installed_source.group_write(metadata)


def inventory(directory, *, installed_source=None, private_root=None):
    need(installed_source is None or type(installed_source) is InstalledSource and
         installed_source.directory is directory and private_root is None)

    def authority_check():
        if installed_source is not None:
            installed_source.check()
            s = os.fstat(directory.fd)
            entry_mode(stat.S_IMODE(s.st_mode), False, installed_source=installed_source, metadata=s)
        if private_root is not None:
            copied_tree_check(private_root, directory)

    entries, pins, inodes = {}, {}, {}
    total = 0

    def walk(fd, prefix):
        nonlocal total
        before = os.fstat(fd)
        names = sorted(os.listdir(fd))
        for name in names:
            relative = prefix + name
            s = os.stat(name, dir_fd=fd, follow_symlinks=False)
            mode = stat.S_IMODE(s.st_mode)
            if private_root is not None:
                need(s.st_uid == os.getuid())
            entry_mode(mode, stat.S_ISLNK(s.st_mode), installed_source=installed_source,
                       private_root=private_root, metadata=s)
            pins[relative] = file_pin(s) if not stat.S_ISDIR(s.st_mode) else pin(s)
            if stat.S_ISLNK(s.st_mode):
                # Symlink modes on macOS are 755; Linux reports 777.
                entries[relative] = ["link", os.readlink(name, dir_fd=fd)]
                entries[relative].insert(1, 0)
            elif stat.S_ISDIR(s.st_mode):
                child = os.open(name, DIR_FLAGS, dir_fd=fd)
                try:
                    need(pin(os.fstat(child)) == pin(s))
                    entries[relative] = ["dir", mode]
                    walk(child, relative + "/")
                finally:
                    os.close(child)
            else:
                need(stat.S_ISREG(s.st_mode))
                child = os.open(name, FILE_FLAGS, dir_fd=fd)
                try:
                    need(file_pin(os.fstat(child)) == file_pin(s))
                    h, count = hashlib.sha256(), 0
                    while True:
                        raw = os.read(child, 1024 * 1024)
                        if not raw:
                            break
                        count += len(raw)
                        h.update(raw)
                    need(count == s.st_size and file_pin(os.fstat(child)) == file_pin(s))
                    entries[relative] = ["file", mode, count, h.hexdigest()]
                    inodes.setdefault((s.st_dev, s.st_ino), []).append((relative, s.st_nlink))
                    total += count
                finally:
                    os.close(child)
            need((file_pin if not stat.S_ISDIR(s.st_mode) else pin)(
                os.stat(name, dir_fd=fd, follow_symlinks=False)) == pins[relative])
        after = os.fstat(fd)
        need(pin(before) == pin(after) and before.st_mtime_ns == after.st_mtime_ns and
             before.st_ctime_ns == after.st_ctime_ns and names == sorted(os.listdir(fd)))

    directory.check()
    authority_check()
    walk(directory.fd, "")
    for group in inodes.values():
        need(all(n == len(group) for _, n in group))  # no external hard links
        if len(group) > 1:
            names = sorted(name for name, _ in group)
            for name in names:
                entries[name].append(names[0])
    links_inside(entries)
    directory.check()
    authority_check()
    return {"content": digest(entries), "pins": digest(pins), "bytes": total, "entries": len(entries)}


def free_space(directory, size=0, entries=0):
    s = os.fstatvfs(directory.fd)
    # Payload plus per-entry allocation/metadata slack and run overhead. The
    # stream has no second archive on disk. Hard-linked input is overcounted.
    need(type(size) is int and size >= 0 and type(entries) is int and entries >= 0)
    overhead = GIB + entries * max(4096, s.f_frsize) * 2
    need(s.f_bavail * s.f_frsize > 40 * GIB + size + overhead)


def facts(directory, *, source_diagnostics=None):
    # Critical metadata is opened nofollow, even if a source changes after
    # inventory. Do not read an arbitrary replacement link before refusing it.
    with Directory(directory.path + "/Contents") as contents:
        values = []
        for name in ("Info.plist", "version.plist"):
            fd = os.open(name, FILE_FLAGS, dir_fd=contents.fd)
            with os.fdopen(fd, "rb") as f:
                before = os.fstat(f.fileno())
                need(stat.S_ISREG(before.st_mode) and before.st_size < 1024 * 1024)
                values.append(plistlib.load(f))
                need(file_pin(os.fstat(f.fileno())) == file_pin(before) ==
                     file_pin(os.stat(name, dir_fd=contents.fd, follow_symlinks=False)))
        contents.check()
    info, version = values
    need(info.get("CFBundleIdentifier") == "com.apple.dt.Xcode")
    v, b = version.get("CFBundleShortVersionString"), version.get("ProductBuildVersion")
    need(isinstance(v, str) and re.fullmatch(r"[0-9]+(?:\.[0-9]+){1,2}", v) and
         isinstance(b, str) and re.fullmatch(r"[A-Za-z0-9]+", b) and
         info.get("CFBundleShortVersionString") == v)
    diagnostics = {} if source_diagnostics is None else {"source_diagnostics": source_diagnostics}
    # Apple TN3127 documents the production Mac App Store leaf OID:
    # https://developer.apple.com/documentation/technotes/tn3127-inside-code-signing-requirements
    command(["/usr/bin/codesign", "--verify", "--deep", "--strict", "--all-architectures",
             "-R", '=identifier "com.apple.dt.Xcode" and (anchor apple or (anchor apple generic and '
             'certificate leaf[field.1.2.840.113635.100.6.1.9] exists))', directory.path], **diagnostics)
    directory.check()
    return {"version": v, "build": b}


def instance():
    _, raw = command(["/usr/sbin/ioreg", "-a", "-r", "-d", "1", "-c", "IOPlatformExpertDevice"], timeout=5)
    records = plistlib.loads(raw)
    ids = [r.get("IOPlatformUUID") for r in records if r.get("IOPlatformUUID")]
    need(len(ids) == 1)
    _, boot = command(["/usr/sbin/sysctl", "-n", "kern.boottime"], timeout=5)
    return [ids[0], boot.decode().strip()]


def target_name(expected):
    need(re.fullmatch(r"[0-9]+(?:\.[0-9]+){1,2}", expected["version"]) and
         re.fullmatch(r"[A-Za-z0-9]+", expected["build"]))
    return "Xcode-" + expected["version"] + "-" + expected["build"] + ".app"


def guest_root():
    # Fixed user Applications subtree: no administrator permission to stage.
    home = os.path.realpath(pwd.getpwuid(os.getuid()).pw_dir)
    with Directory(home) as d:
        need(os.fstat(d.fd).st_uid == os.getuid())
        try:
            os.mkdir("Applications", 0o700, dir_fd=d.fd)
        except FileExistsError:
            pass
    with Directory(home + "/Applications") as d:
        try:
            os.mkdir("MeguriXcode", 0o700, dir_fd=d.fd)
        except FileExistsError:
            pass
    return Directory(home + "/Applications/MeguriXcode")


def verify_bundle(bundle, expected, root):
    current = inventory(bundle, private_root=root)
    need(current["content"] == expected["content"] and facts(bundle) == expected["facts"])
    return current


def no_replace(src_dir, src, dst_dir, dst):
    libc = ctypes.CDLL(None, use_errno=True)
    if sys.platform == "darwin":
        fn, flag = libc.renameatx_np, 4  # RENAME_EXCL, atomic for directories
    else:  # generated Linux self-tests only; MG entry requires macOS
        fn, flag = libc.renameat2, 1
    fn.argtypes = [ctypes.c_int, ctypes.c_char_p, ctypes.c_int, ctypes.c_char_p, ctypes.c_uint]
    fn.restype = ctypes.c_int
    if fn(src_dir, os.fsencode(src), dst_dir, os.fsencode(dst), flag) != 0:
        raise OSError(ctypes.get_errno(), "exclusive publication refused")


def receive(stage, expected, *, private_root=None):
    """Extract tar without any unlink, replacement, or filesystem link traversal."""
    directory_modes, hardlinks = [], []
    if private_root is not None:
        copied_tree_check(private_root, stage)
    count, size = 0, 0
    need(type(expected["entries"]) is int and expected["entries"] >= 0 and
         type(expected["bytes"]) is int and expected["bytes"] >= 0)
    with tarfile.open(fileobj=sys.stdin.buffer, mode="r|*") as archive:
        for member in archive:
            name = member.name
            while name.startswith("./"):
                name = name[2:]
            if name in ("", "."):
                need(member.isdir())
                continue
            count += 1
            need(count <= expected["entries"] and len(name.encode()) <= 4096 and
                 len(member.linkname.encode()) <= 4096 and member.size >= 0)
            if member.isfile():
                size += member.size
                need(size <= expected["bytes"])
            else:
                need(member.size == 0)
            parts = name.rstrip("/").split("/")
            need(not name.startswith("/") and all(p not in ("", ".", "..") for p in parts))
            # Retain a nofollow chain from the original destination descriptor.
            with contextlib.ExitStack() as stack:
                parent = stage.fd
                for part in parts[:-1]:
                    child = os.open(part, DIR_FLAGS, dir_fd=parent)
                    stack.callback(os.close, child)
                    if private_root is not None:
                        need(os.fstat(child).st_uid == os.getuid())
                    parent = child
                leaf = parts[-1]
                mode = member.mode & 0o777
                if private_root is not None:
                    copied_tree_check(private_root, stage)
                entry_mode(member.mode, member.issym(), private_root=private_root)
                if member.isdir():
                    os.mkdir(leaf, 0o700, dir_fd=parent)
                    directory_modes.append((parts, mode))
                elif member.issym():
                    # External link text is refused before it ever reaches disk.
                    normalized = posixpath.normpath(posixpath.join(*parts[:-1], member.linkname))
                    need(member.linkname and not member.linkname.startswith("/") and
                         normalized != ".." and not normalized.startswith("../"))
                    os.symlink(member.linkname, leaf, dir_fd=parent)
                elif member.islnk():
                    hardlinks.append((parts, member.linkname))
                else:
                    need(member.isfile())
                    fd = os.open(leaf, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW,
                                 mode, dir_fd=parent)
                    with os.fdopen(fd, "wb") as out, archive.extractfile(member) as source:
                        need(os.fstat(out.fileno()).st_uid == os.getuid())
                        remaining = member.size
                        while remaining:
                            data = source.read(min(1024 * 1024, remaining))
                            need(data and len(data) <= remaining)
                            out.write(data)
                            remaining -= len(data)
                        need(not source.read(1))
                        os.fchmod(out.fileno(), mode)
    need(count == expected["entries"])
    # Hard links must name an already extracted in-bundle regular file.
    for parts, link in hardlinks:
        while link.startswith("./"):
            link = link[2:]
        need(link and not link.startswith("/") and all(p not in ("", ".", "..") for p in link.split("/")))
        with Directory(posixpath.join(stage.path, posixpath.dirname(link)).rstrip("/")) as source_parent, \
                Directory(posixpath.join(stage.path, *parts[:-1])) as dest_parent:
            name = posixpath.basename(link)
            s = os.stat(name, dir_fd=source_parent.fd, follow_symlinks=False)
            need(stat.S_ISREG(s.st_mode) and s.st_uid == os.getuid())
            if private_root is not None:
                copied_tree_check(private_root, stage)
            os.link(name, parts[-1], src_dir_fd=source_parent.fd, dst_dir_fd=dest_parent.fd,
                    follow_symlinks=False)
    for parts, mode in reversed(directory_modes):
        with Directory(stage.path + "/" + "/".join(parts)) as d:
            need(os.fstat(d.fd).st_uid == os.getuid())
            if private_root is not None:
                copied_tree_check(private_root, stage)
            os.fchmod(d.fd, mode)
    # tar's root mode isn't applied: root is deliberately run-owned/private.
    stage.check()
    if private_root is not None:
        inventory(stage, private_root=private_root)


def write_new(directory, name, value):
    directory.check()
    fd = os.open(name + ".pending", os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW,
                 0o600, dir_fd=directory.fd)
    with os.fdopen(fd, "wb") as out:
        out.write(json.dumps(value).encode())
    directory.check()
    no_replace(directory.fd, name + ".pending", directory.fd, name)


def receiver_record(work, name, context):
    with contextlib.ExitStack() as held:
        record = Original(work.path + "/" + name)
        held.callback(record.close)
        need(record.value["token"] == context["state"]["receiver"] and
             record.value["instance"] == context["instance"])
        work.check(context["state"]["work_pins"])
        return record.value


@contextlib.contextmanager
def release_proof(private_dir):
    state = {"pending": False, "joined": False, "uncertain": False}
    try:
        yield state
    except Uncertain:
        state["uncertain"] = True
        raise
    finally:
        if not state["uncertain"] and (not state["pending"] or state["joined"]):
            with finishing():
                write_new(private_dir, "lease-safe.json", {"safe": True})


def guest_argv(source, operation, context):
    launcher = "import base64;STAGING_SOURCE=" + repr(source) + ";exec(base64.b64decode(STAGING_SOURCE))"
    return [sys.executable, "-B", "-I", "-c", launcher, "--guest", operation, encoded(context)]


def extract(context):
    # The original supervisor checks instance identity. Its child does only
    # descriptor-bound extraction, with no independently launched subprocesses.
    state = context["state"]
    with guest_root() as root, Directory(root.path + "/" + state["work"]) as work, \
            Directory(work.path + "/Xcode.app") as bundle:
        need(root.path == state["root"])
        root.check(state["root_pins"])
        work.check(state["work_pins"])
        bundle.check(state["bundle_pins"])
        need(not os.listdir(bundle.fd))
        receive(bundle, context["expected"], private_root=root)


def run_receiver_owner(bundle, work, context):
    """The original SSH foreground supervisor owns and joins its one child.

    Like guest_smoke, cancellation joins that retained handle before reporting.
    A run-local stop request also works after SSH loss, without PID discovery.
    """
    child = None
    with tempfile.TemporaryFile() as out, tempfile.TemporaryFile() as err:
        try:
            if os.path.lexists(work.path + "/receiver-stop.json"):
                receiver_record(work, "receiver-stop.json", context)
                raise Refused()
            with launching():
                child = subprocess.Popen(guest_argv(STAGING_SOURCE, "extract", context),
                                         stdin=sys.stdin.buffer, stdout=out, stderr=err,
                                         start_new_session=True)
            deadline = time.monotonic() + COPY_WAIT
            while child.poll() is None:
                need(time.monotonic() < deadline)
                if os.path.lexists(work.path + "/receiver-stop.json"):
                    receiver_record(work, "receiver-stop.json", context)
                    raise Refused()
                try:
                    child.wait(timeout=0.2)
                except subprocess.TimeoutExpired:
                    pass
            need(child.returncode == 0)
        finally:
            with finishing():
                if child is not None:
                    join_original(child)
                # No receipt is written if joining the original child failed.
                work.check(context["state"]["work_pins"])
                bundle.check(context["state"]["bundle_pins"])
                write_new(work, "receiver-joined.json", {
                    "token": context["state"]["receiver"], "instance": context["instance"],
                    "joined": True, "returncode": None if child is None else child.returncode})


def join_receiver(work, context):
    write_new(work, "receiver-stop.json", {
        "token": context["state"]["receiver"], "instance": context["instance"]})
    deadline = time.monotonic() + 2 * SHUTDOWN + 5
    while not os.path.lexists(work.path + "/receiver-joined.json"):
        need(time.monotonic() < deadline)
        time.sleep(0.1)
    result = receiver_record(work, "receiver-joined.json", context)
    need(result["joined"] is True and instance() == context["instance"])
    return {"joined": True, "token": result["token"], "returncode": result["returncode"]}


def activation(bundle, expected):
    developer = bundle.path + "/Contents/Developer"
    env = dict(os.environ)
    for key in ("DEVELOPER_DIR", "SDKROOT", "TOOLCHAINS"):
        env.pop(key, None)
    # Select the actual staged tool for READ-ONLY readiness, never a consent
    # override. No first-launch installer or license acceptance is invoked.
    readiness_env = dict(env, DEVELOPER_DIR=developer)
    executable = developer + "/usr/bin/xcodebuild"
    for option in (["-checkFirstLaunchStatus"], ["-license", "check"]):
        rc, _ = command([executable] + option, env=readiness_env, check=False)
        if rc != 0:
            return "staged", False
    _, selected = command(["/usr/bin/xcode-select", "-p"], env=env)
    already = selected.decode().strip() == developer
    if not already:
        rc, _ = command(["/usr/bin/sudo", "-n", "/usr/bin/xcode-select", "--switch", developer],
                        env=env, check=False)
        if rc != 0:
            return "staged", False
    _, selected = command(["/usr/bin/xcode-select", "-p"], env=env)
    need(selected.decode().strip() == developer)
    _, raw = command(["/usr/bin/xcodebuild", "-version"], env=env)
    need(raw.decode().strip().splitlines() ==
         ["Xcode " + expected["version"], "Build version " + expected["build"]])
    bundle.check()
    return ("already-selected" if already else "activated"), True


def guest(operation, context):
    expected = context["expected"]
    need(instance() == context["instance"])
    with guest_root() as root:
        need(os.fstat(root.fd).st_uid == os.getuid() and stat.S_IMODE(os.fstat(root.fd).st_mode) == 0o700)
        name = target_name(expected["facts"])
        if operation == "prepare":
            if os.path.lexists(root.path + "/" + name):
                with Directory(root.path + "/" + name) as existing:
                    need(os.fstat(existing.fd).st_uid == os.getuid())
                    verified = verify_bundle(existing, expected, root)
                    return {"root": root.path, "root_pins": root.check(), "existing": True,
                            "bundle_pins": existing.check(), "inventory": verified}
            free_space(root, expected["bytes"], expected["entries"])
            run = ".stage-" + uuid.uuid4().hex
            os.mkdir(run, 0o700, dir_fd=root.fd)
            with Directory(root.path + "/" + run) as work:
                os.mkdir("Xcode.app", 0o700, dir_fd=work.fd)
                with Directory(work.path + "/Xcode.app") as bundle:
                    return {"root": root.path, "root_pins": root.check(), "existing": False,
                            "work": run, "work_pins": work.check(), "receiver": uuid.uuid4().hex,
                            "bundle_pins": bundle.check()}
        state = context["state"]
        need(root.path == state["root"])
        root.check(state["root_pins"])
        if operation == "join-receiver":
            need(not state["existing"])
            with Directory(root.path + "/" + state["work"]) as work:
                work.check(state["work_pins"])
                return join_receiver(work, context)
        bundle_path = root.path + "/" + (name if state["existing"] else state["work"] + "/Xcode.app")
        with Directory(bundle_path) as bundle:
            bundle.check(state["bundle_pins"])
            if operation == "receive":
                need(not state["existing"] and not os.listdir(bundle.fd))
                free_space(root, expected["bytes"], expected["entries"])
                with Directory(root.path + "/" + state["work"]) as work:
                    work.check(state["work_pins"])
                    run_receiver_owner(bundle, work, context)
                space = os.fstatvfs(root.fd)
                need(space.f_bavail * space.f_frsize > 40 * GIB)
                need(instance() == context["instance"])
                root.check(state["root_pins"])
                return {"received": True}
            need(operation == "publish")
            verified = verify_bundle(bundle, expected, root)
            if state["existing"]:
                need(verified == state["inventory"])
            need(instance() == context["instance"])
            root.check(state["root_pins"])
            bundle.check(state["bundle_pins"])
            if not state["existing"]:
                with Directory(root.path + "/" + state["work"]) as work:
                    no_replace(work.fd, "Xcode.app", root.fd, name)
        with Directory(root.path + "/" + name) as published:
            # Publication is a rename, so retain the original bundle inode.
            need(pin(os.fstat(published.fd)) == state["bundle_pins"][-1])
            verify_bundle(published, expected, root)
            outcome, match = activation(published, expected["facts"])
            verify_bundle(published, expected, root)
            root.check(state["root_pins"])
            need(instance() == context["instance"])
            return {"outcome": outcome, "match": match}


class VM:
    def __init__(self, selection, lease_check, safety):
        self.selection, self.lease_check, self.safety = selection, lease_check, safety
        self.wrappers = [Original(os.path.realpath(selection[k]), parse=False, own=False) for k in ("ssh", "tart")]
        self.ssh, self.tart = [w.path for w in self.wrappers]
        self.source = base64.b64encode(Path(__file__).read_bytes()).decode()
        self.identity = None
        self.check()
        _, raw = self.call_command("/usr/sbin/ioreg -a -r -d 1 -c IOPlatformExpertDevice")
        records = plistlib.loads(raw)
        ids = [r.get("IOPlatformUUID") for r in records if r.get("IOPlatformUUID")]
        need(len(ids) == 1)
        _, boot = self.call_command("/usr/sbin/sysctl -n kern.boottime")
        self.identity = [ids[0], boot.decode().strip()]

    def check(self):
        self.lease_check()
        for key, wrapper in zip(("ssh", "tart"), self.wrappers):
            need(os.path.realpath(self.selection[key]) == wrapper.path)
            wrapper.check()
        _, raw = command([self.tart, "list", "--format", "json"], timeout=5)
        matches = [r for r in json.loads(raw) if r.get("Name") == self.selection["name"]]
        need(len(matches) == 1 and matches[0].get("State") == "running")

    def call_command(self, cmd, timeout=180):
        self.check()
        # Every SSH control operation has its own completion obligation,
        # including identity probes, preparation and publication/activation.
        # A receiver's earlier join cannot prove this later operation stopped.
        pending, joined = self.safety["pending"], self.safety["joined"]
        self.safety.update(pending=True, joined=False)
        try:
            result = command([self.ssh, cmd], timeout=timeout)
        except BaseException:
            # Joining the local SSH process says nothing about guest completion.
            # Keep the guard even if a later control call happens to succeed.
            self.safety["uncertain"] = True
            raise
        else:
            self.safety.update(pending=pending, joined=joined)
        self.check()
        return result

    def guest_command(self, operation, context):
        # Host/guest Python paths need not match.
        argv = guest_argv(self.source, operation, context)
        argv[0] = "/usr/bin/python3"
        return shlex.join(argv)

    def call(self, operation, context):
        return json.loads(self.call_command(self.guest_command(operation, context),
                                           timeout=45 if operation == "join-receiver" else 180)[1])

    def copy(self, bundle, context, on_join=lambda: None):
        self.check()
        source = "/dev/fd/" + str(bundle.fd) + "/."
        dest = context["state"]["root"] + "/" + context["state"]["work"] + "/Xcode.app"
        # mvl_copy_to supplies the original symlink-preserving tar producer.
        # Its exact remote preamble is checked, NEVER evaluated. The only SSH
        # operation is our descriptor-bound, exclusive-create receiver.
        shell = r'''
set -u
. "$1"
original_ssh="$2" receiver="$3" source_bundle="$4" destination="$5"
expected="rm -rf $(mvl_q "$destination/.") && mkdir -p $(mvl_q "$destination") && tar -C $(mvl_q "$destination") -xf -"
vm_xcode_receiver() {
  [ "$#" -eq 1 ] && [ "$1" = "$expected" ] || return 1
  "$original_ssh" "$receiver"
}
mvl_copy_to vm_xcode_receiver dedicated "$source_bundle" "$destination"
'''
        local_uncertain = False
        try:
            command(["/bin/bash", "-c", shell, "vm-xcode-copy",
                     str(Path(__file__).with_name("mac-vm-lib.sh")), self.ssh,
                     self.guest_command("receive", context), source, dest],
                    pass_fds=(bundle.fd,), timeout=COPY_WAIT)
        except Uncertain:
            local_uncertain = True
            raise
        finally:
            with finishing():
                joined = self.call("join-receiver", context)
                need(joined["joined"] is True and joined["token"] == context["state"]["receiver"])
                if not local_uncertain:
                    on_join()
        need(joined["returncode"] == 0)
        self.check()

    def close(self):
        for wrapper in self.wrappers:
            wrapper.close()


def check_source(context):
    # A fresh derived host selection only: no supplied bundle, route or facts.
    need(set(context) == {"private"})
    with Directory(context["private"]) as private:
        s = os.fstat(private.fd)
        need(s.st_uid == os.getuid() and stat.S_IMODE(s.st_mode) == 0o700)
        env = dict(os.environ)
        for key in ("DEVELOPER_DIR", "SDKROOT", "TOOLCHAINS"):
            env.pop(key, None)

        def selection():
            _, raw = command(["/usr/bin/xcode-select", "-p"], env=env)
            path = raw.decode().strip()
            need(path.endswith(".app/Contents/Developer") and os.path.realpath(path) == path)
            return path

        selected = selection()
        with Directory(selected) as developer, \
                Directory(selected[:-len("/Contents/Developer")]) as bundle:
            authority = InstalledSource(bundle)
            authority.identity = authority.system_identity()
            authority.check()
            root = os.fstat(bundle.fd)
            entry_mode(stat.S_IMODE(root.st_mode), False, installed_source=authority, metadata=root)
            # Retain the original binding/authority even when facts() refuses.
            write_new(private, "source-check.json", {
                "selected": selected, "bundle": bundle.path, "bundle_pins": bundle.check(),
                "developer_pins": developer.check(), "authority": authority.identity})
            facts(bundle, source_diagnostics=private)
            need(selection() == selected)
            developer.check()
            authority.check()
        print(json.dumps({"scope": SOURCE_SCOPE, "outcome": "source-checked",
                          "checks": {"source_facts": True, "source_signature": True,
                                     "source_binding": True, "source_authority": True}}))
        return 0


def host(context):
    need(set(context) == {"selection", "lease_slot", "root", "run", "private"})
    with contextlib.ExitStack() as held:
        private_dir = held.enter_context(Directory(context["private"]))
        safety = held.enter_context(release_proof(private_dir))
        holder = Original(context["lease_slot"] + "/holder.json")
        held.callback(holder.close)
        value = holder.value
        need(value.get("job") == "vm-xcode-stage" and value.get("run_id") == context["run"] and
             value.get("worktree") == context["root"] and not value.get("nested") and
             type(value.get("pid")) is int and value["pid"] > 1)

        def lease_check():
            holder.check()
            os.kill(value["pid"], 0)
            _, raw = command(["/bin/ps", "-A", "-o", "pid=", "-o", "ppid="], timeout=5)
            parents = dict(tuple(map(int, line.split())) for line in raw.splitlines())
            pid = os.getpid()
            for _ in range(200):
                if pid == value["pid"]:
                    return
                pid = parents.get(pid, 0)
            raise Refused()

        env = dict(os.environ)
        for key in ("DEVELOPER_DIR", "SDKROOT", "TOOLCHAINS"):
            env.pop(key, None)

        def selection():
            _, raw = command(["/usr/bin/xcode-select", "-p"], env=env)
            path = raw.decode().strip()
            need(path.endswith(".app/Contents/Developer") and os.path.realpath(path) == path)
            return path

        selected = selection()
        bundle = held.enter_context(Directory(selected[:-len("/Contents/Developer")]))
        source_authority = InstalledSource(bundle)
        before = inventory(bundle, installed_source=source_authority)
        actual = facts(bundle)
        source_authority.check()
        expected = {"content": before["content"], "bytes": before["bytes"],
                    "entries": before["entries"], "facts": actual}

        def source_check():
            lease_check()
            need(selection() == selected and inventory(bundle, installed_source=source_authority) == before and
                 facts(bundle) == actual)
            source_authority.check()

        vm = VM(context["selection"], lease_check, safety)
        held.callback(vm.close)
        private = {"expected": expected, "instance": vm.identity}
        state = vm.call("prepare", private)
        private["state"] = state
        source_check()
        if not state["existing"]:
            with Directory(os.path.dirname(bundle.path)) as parent:
                free_space(parent, expected["bytes"], expected["entries"])
            safety["pending"] = True
            write_new(private_dir, "receiver-pending.json", {"pending": True})
            def receiver_joined():
                write_new(private_dir, "receiver-joined.json", {"joined": True})
                safety["joined"] = True
            vm.copy(bundle, private, on_join=receiver_joined)
        # Publication is authorized only after the ORIGINAL source and lease
        # survive the complete copy, not an independently re-opened success flag.
        source_check()
        result = vm.call("publish", private)
        source_check()
        need(result["outcome"] in ("staged", "activated", "already-selected") and
             result["match"] is (result["outcome"] != "staged"))
        print(json.dumps(report(result["outcome"], actual, content=True, signature=True,
                                source_unchanged=True, lease_retained=True, instance_unchanged=True,
                                reused=state["existing"], selection_matches=result["match"])))
        return 75 if result["outcome"] == "staged" else 0


def report(outcome, facts=None, **checks):
    result = {"scope": SCOPE, "outcome": outcome, "checks": checks}
    if facts:
        result.update(facts)
    return result


def main():
    for sig in (signal.SIGINT, signal.SIGTERM, signal.SIGHUP):
        signal.signal(sig, lambda *_: (_ for _ in ()).throw(Cancelled()))
    if len(sys.argv) == 4 and sys.argv[1] == "--guest":
        need(sys.platform == "darwin")
        context = json.loads(base64.b64decode(sys.argv[3]))
        if sys.argv[2] == "extract":
            extract(context)
            return 0
        print(json.dumps(guest(sys.argv[2], context)))
        return 0
    need(sys.platform == "darwin")
    if sys.argv[1:] == ["--check-source"]:
        return check_source(json.load(os.fdopen(os.dup(3))))
    need(len(sys.argv) == 1)
    # The host never accepts supplied success facts.
    return host(json.load(os.fdopen(os.dup(3))))


def run():
    try:
        return main()
    except Cancelled:
        return 130
    except (Refused, OSError, ValueError, KeyError, TypeError, tarfile.TarError) as error:
        if "--guest" not in sys.argv:
            # stderr is already retained in private helpers.log. Use fixed
            # classes and this script's frame names/lines, never error text.
            kind = next(kind.__name__ for kind in (
                Uncertain, Refused, NotADirectoryError, FileNotFoundError,
                PermissionError, FileExistsError, IsADirectoryError, OSError,
                ValueError, KeyError, TypeError, tarfile.TarError) if isinstance(error, kind))
            frames, tb = [], error.__traceback__
            while tb is not None:
                if tb.tb_frame.f_globals is globals():
                    frames.append({"function": tb.tb_frame.f_code.co_name, "line": tb.tb_lineno})
                tb = tb.tb_next
            print(json.dumps({"exception": kind, "stack": frames}), file=sys.stderr)
            print(json.dumps({"scope": SOURCE_SCOPE, "outcome": "refused", "checks": {}}
                             if sys.argv[1:] == ["--check-source"] else report("refused")))
        return 1


if __name__ == "__main__":
    sys.exit(run())
