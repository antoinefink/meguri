#!/usr/bin/env python3
"""OWNER ONLY J2.2.1 producer. No normal header, account projection or lane proof.

FD 3 is the retained MG VM context. Identity data travels only in the original
private duplex transport and process memory. No candidate file or attachment.
"""
import argparse
import contextlib
import ctypes
import fcntl
import importlib.util
import inspect
import json
import os
from pathlib import Path
import plistlib
import re
import select
import signal
import socket
import stat
import subprocess
import sys
import tempfile
import time

_spec = importlib.util.spec_from_file_location("cloud_account_setup_smoke", Path(__file__).with_name("cloud-agent-smoke.py"))
s = importlib.util.module_from_spec(_spec)
sys.modules[_spec.name] = s
_spec.loader.exec_module(s)
a, h, v, i = s.a, s.h, s.v, s.i
need, strict, CAP, WAIT = s.need, s.strict, s.CAP, s.WAIT
NAME = "approved-accounts.json"
LOCK_NAME = ".approved-accounts.lock"  # Stable authority: never unlink/replace.


def identity(value):
    need(isinstance(value, list) and value and len(value) <= a.LIMIT and
         all(type(b) is int and 0 <= b <= 255 for b in value), "setup-identity")
    return value


def approved(raw, container):
    value = strict(raw)
    need(isinstance(value, dict) and set(value) == {"protocol_version", "container", "environment", "accounts"}
         and type(value["protocol_version"]) is int and value["protocol_version"] == 1 and
         value["container"] == container and value["environment"] == "development", "setup-source")
    accounts = value["accounts"]
    need(isinstance(accounts, dict) and set(accounts) in ({"acct1"}, {"acct1", "acct2"}), "setup-source")
    for item in accounts.values():
        identity(item)
    need("acct2" not in accounts or accounts["acct1"] != accounts["acct2"], "setup-equal-accounts")
    return value


class SetupDiagnostic:
    """Closed private diagnostics only; never account or exception text."""
    PHASES = frozenset(('initial', 'terminal', 'vm-context', 'native-build', 'product-input',
        'product-check', 'approved-source', 'copy-products', 'path-preflight', 'support-authority',
        'owner-launch', 'loaded-owner-proof', 'capture-precheck', 'capture', 'capture-proof',
        'terminal-approval', 'fresh-precheck', 'fresh', 'fresh-proof', 'publish', 'finish'))
    CODES = frozenset(('setup-interactive-terminal', 'setup-terminal-changed', 'setup-context',
        'setup-lease', 'setup-info', 'setup-container', 'setup-source', 'setup-directory', 'setup-writer',
        'setup-source-changed', 'setup-source-invalid', 'setup-directory-changed', 'setup-writer-changed',
        'setup-root-authority', 'setup-loaded-changed', 'setup-account-read', 'setup-capture', 'setup-peer',
        'setup-order', 'setup-selection', 'setup-original-ended', 'setup-wire', 'setup-plan-changed',
        'setup-machine-changed', 'setup-product-changed', 'setup-product-authority-changed',
        'setup-delivery', 'setup-delivery-changed', 'setup-root', 'setup-test-failed', 'setup-test-result',
        'setup-original-join', 'setup-declined', 'setup-cancelled', 'setup-identity', 'setup-fresh-changed',
        'setup-original-changed', 'setup-plan', 'setup-environment', 'setup-write-readback',
        'product-signature', 'product-profile', 'product-schema', 'profile-device', 'source-changed',
        'source-dirty', 'setup-observation', 'setup-bundle', 'setup-image-path', 'setup-loaded-binding',
        'setup-images', 'setup-image', 'setup-original-bytes', 'setup-missing-image', 'setup-support-root'))

    def __init__(self):
        self.phase = 'initial'

    def enter(self, phase):
        self.phase = phase if type(phase) is str and phase in self.PHASES else 'unknown'

    def emit(self, error):
        try:
            code = error.args[0] if (isinstance(error, v.Refused) and len(error.args) == 1 and
                type(error.args[0]) is str and error.args[0] in self.CODES) else 'unknown'
            kind = next((name for cls, name in ((v.Refused, 'refused'), (subprocess.TimeoutExpired, 'timeout'),
                (OSError, 'os-error'), (ValueError, 'value-error'), (TypeError, 'type-error'),
                (KeyError, 'key-error'), (KeyboardInterrupt, 'interrupted')) if isinstance(error, cls)), 'other')
            phase = self.phase if type(self.phase) is str and self.phase in self.PHASES else 'unknown'
            print('MG_OWNER_SETUP_DIAGNOSTIC ' + json.dumps({'phase': phase, 'refusal': code,
                'kind': kind}, sort_keys=True), file=sys.stderr)
        except (OSError, ValueError, TypeError, AttributeError):
            pass


class PublicationUncertain(v.Refused):
    pass


def exchange(source, name, destination, leaf):
    """Exchange both existing entries atomically; no replace fallback."""
    libc = ctypes.CDLL(None, use_errno=True)
    if sys.platform == "darwin":
        fn, flag = libc.renameatx_np, 0x00000002  # RENAME_SWAP
    elif sys.platform == "linux":
        fn, flag = libc.renameat2, 2  # RENAME_EXCHANGE (generated tests)
    else:
        raise v.Refused("setup-exchange-unavailable")
    fn.argtypes = (ctypes.c_int, ctypes.c_char_p, ctypes.c_int, ctypes.c_char_p, ctypes.c_uint)
    fn.restype = ctypes.c_int
    need(fn(source, os.fsencode(name), destination, os.fsencode(leaf), flag) == 0, "setup-exchange-failed")


def exchanged_pin(info):
    # An exchange may change ctime on both inodes. Nothing else in the original
    # pin (including mtime), nor either file's bytes, may change.
    return (info.st_dev, info.st_ino, info.st_mode, info.st_nlink,
            info.st_size, info.st_mtime_ns, info.st_uid, info.st_gid)


class Source:
    """Pin absent/present source and private suffixes, then publish once.

    Production root is OS-derived only. The root argument is internal and also
    permits generated filesystem checks; it is never a CLI/candidate path.
    """
    def __init__(self, root, container):
        self.held = contextlib.ExitStack()
        self.container, self.old, self.fd, self.started = container, None, None, False
        self.events, self.file_events, self.invalid = None, None, False
        try:
            root = str(Path(root).resolve(strict=True))  # trusted OS root only
            self.base = h.Directory(root)
            self.held.callback(self.base.close)
            if sys.platform == "darwin":
                self.events = select.kqueue()
                self.held.callback(self.events.close)
                self.file_events = select.kqueue()
                self.held.callback(self.file_events.close)
            self.dirs = []
            fd = self.base.fd
            for name in ("Meguri", "CloudKitSignedPrivate"):
                try:
                    os.mkdir(name, 0o700, dir_fd=fd)
                except FileExistsError:
                    pass
                child = os.open(name, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_NONBLOCK, dir_fd=fd)
                self.held.callback(os.close, child)
                info = os.fstat(child)
                need(info.st_uid == os.getuid() and stat.S_IMODE(info.st_mode) == 0o700, "setup-directory")
                self.dirs.append((fd, name, child, h.directory_pin(info)))
                fd = child
            self.parent = fd
            self.writer_fd = os.open(LOCK_NAME, os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW | os.O_NONBLOCK,
                                     0o600, dir_fd=fd)
            self.held.callback(os.close, self.writer_fd)
            info = os.fstat(self.writer_fd)
            need(stat.S_ISREG(info.st_mode) and stat.S_IMODE(info.st_mode) == 0o600 and
                 info.st_uid == os.getuid() and info.st_nlink == 1 and info.st_size == 0, "setup-writer")
            self.writer_pin = h.file_pin(info)
            if self.events:
                for directory in [*self.base.held, *(item[2] for item in self.dirs)]:
                    self.watch(directory, False)
            try:
                self.fd = os.open(NAME, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK, dir_fd=fd)
            except FileNotFoundError:
                self.raw, self.pin = None, None
                self.accounts = {}
            else:
                self.held.callback(os.close, self.fd)
                self.pin = h.file_pin(os.fstat(self.fd))
                self.prior_pin = exchanged_pin(os.fstat(self.fd))
                self.regular(os.fstat(self.fd))
                self.watch(self.fd, True)
                self.raw = self.read(self.fd)
                self.accounts = approved(self.raw, container)["accounts"]
            self.check()
        except BaseException:
            self.held.close()
            raise

    def watch(self, fd, file):
        if self.events:
            flags = select.KQ_NOTE_RENAME | select.KQ_NOTE_DELETE | select.KQ_NOTE_REVOKE | select.KQ_NOTE_ATTRIB
            if file:
                flags |= select.KQ_NOTE_WRITE | select.KQ_NOTE_EXTEND | select.KQ_NOTE_LINK
            (self.file_events if file else self.events).control([select.kevent(fd, filter=select.KQ_FILTER_VNODE,
                flags=select.KQ_EV_ADD | select.KQ_EV_CLEAR, fflags=flags)], 0, 0)

    def regular(self, info):
        need(stat.S_ISREG(info.st_mode) and stat.S_IMODE(info.st_mode) == 0o600 and
             info.st_uid == os.getuid() and info.st_nlink == 1 and 0 < info.st_size <= a.LIMIT, "setup-file")

    def read(self, fd):
        raw = os.pread(fd, a.LIMIT + 1, 0)
        need(0 < len(raw) <= a.LIMIT and len(raw) == os.fstat(fd).st_size, "setup-file")
        return raw

    def directories(self):
        self.base.check()
        for parent, name, fd, pin in self.dirs:
            need(h.directory_pin(os.fstat(fd)) == pin and h.directory_pin(
                os.stat(name, dir_fd=parent, follow_symlinks=False)) == pin, "setup-directory-changed")
        for info in (os.fstat(self.writer_fd), os.stat(LOCK_NAME, dir_fd=self.parent, follow_symlinks=False)):
            need(h.file_pin(info) == self.writer_pin, "setup-writer-changed")
        if self.events:
            need(not self.events.control(None, 1, 0), "setup-source-changed")

    def check(self):
        try:
            need(not self.invalid and not self.started, "setup-source-invalid")
            self.directories()
            if self.fd is None:
                try:
                    os.stat(NAME, dir_fd=self.parent, follow_symlinks=False)
                except FileNotFoundError:
                    pass
                else:
                    raise v.Refused("setup-source-changed")
            else:
                for info in (os.fstat(self.fd), os.stat(NAME, dir_fd=self.parent, follow_symlinks=False)):
                    self.regular(info)
                    need(h.file_pin(info) == self.pin, "setup-source-changed")
                need(self.read(self.fd) == self.raw, "setup-source-changed")
            self.directories()
            if self.file_events:
                need(not self.file_events.control(None, 1, 0), "setup-source-changed")
        except BaseException:
            self.invalid = True
            raise

    def proposed(self, label, candidate):
        need(label in ("acct1", "acct2"), "setup-label")
        identity(candidate)
        if label == "acct2" and "acct1" not in self.accounts:
            raise s.UserStep("cloudkit-test-accounts")
        result = dict(self.accounts)
        result[label] = candidate
        value = {"protocol_version": 1, "container": self.container, "environment": "development", "accounts": result}
        raw = i.raw_json(value)
        approved(raw, self.container)
        return raw

    def publish(self, raw, recheck):
        """Serialize cooperative writers; never retry or roll back an exchange."""
        try:
            fcntl.flock(self.writer_fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
            try:
                self._publish(raw, recheck)
            finally:
                fcntl.flock(self.writer_fd, fcntl.LOCK_UN)
        except BaseException:
            if self.started:
                raise PublicationUncertain("publication-uncertain") from None
            raise

    def _publish(self, raw, recheck):
        self.check()
        name = ".approved-" + os.urandom(16).hex()
        fd = os.open(name, os.O_RDWR | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600, dir_fd=self.parent)
        self.held.callback(os.close, fd)
        with os.fdopen(os.dup(fd), "wb") as out:
            out.write(raw)
            out.flush()
            os.fsync(out.fileno())
        self.regular(os.fstat(fd))
        pin = h.file_pin(os.fstat(fd))
        new_pin = exchanged_pin(os.fstat(fd))
        need(self.read(fd) == raw, "setup-write-readback")
        recheck()
        self.check()
        need(h.file_pin(os.stat(name, dir_fd=self.parent, follow_symlinks=False)) == pin, "setup-temp-changed")
        self.started = True
        try:
            if self.fd is None:
                h.publish_exclusive(self.parent, name, self.parent, NAME)
            else:
                try:
                    exchange(self.parent, name, self.parent, NAME)
                finally:
                    # Even lost completion/cancellation retains the entry
                    # privately through its own nofollow descriptor. Never
                    # unlink it: it may carry a competing label's approval.
                    prior = os.open(name, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK, dir_fd=self.parent)
                    self.held.callback(os.close, prior)
            os.fsync(self.parent)
            if self.fd is not None:
                for info in (os.fstat(prior), os.fstat(self.fd),
                             os.stat(name, dir_fd=self.parent, follow_symlinks=False)):
                    self.regular(info)
                    need(exchanged_pin(info) == self.prior_pin, "setup-prior-changed")
                need(self.read(prior) == self.raw and self.read(self.fd) == self.raw,
                     "setup-prior-changed")
            # Old source descriptor now names the replaced inode. Validate
            # directories separately, excluding that intentional event.
            self.directories()
            published = os.open(NAME, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK, dir_fd=self.parent)
            try:
                self.regular(os.fstat(published))
                need(exchanged_pin(os.fstat(fd)) == new_pin and
                     h.file_pin(os.fstat(published)) == h.file_pin(os.fstat(fd)) and self.read(published) == raw,
                     "setup-publication-readback")
                need(h.file_pin(os.stat(NAME, dir_fd=self.parent, follow_symlinks=False)) ==
                     h.file_pin(os.fstat(published)), "setup-publication-changed")
            finally:
                os.close(published)
            recheck()
        except BaseException:
            raise PublicationUncertain("publication-uncertain") from None

    def close(self):
        self.invalid = True
        self.held.close()


class Terminal:
    """A live foreground terminal, never stdin pipes/defaults/env approval."""
    def __init__(self):
        need(os.isatty(0), "setup-interactive-terminal")
        self.fd = os.open("/dev/tty", os.O_RDWR | os.O_NOCTTY | os.O_CLOEXEC | os.O_NOFOLLOW)
        try:
            need(os.isatty(self.fd) and os.tcgetpgrp(self.fd) == os.getpgrp() and
                 os.tcgetpgrp(0) == os.getpgrp(), "setup-interactive-terminal")
            self.pin = h.directory_pin(os.fstat(self.fd))
        except BaseException:
            os.close(self.fd)
            raise

    def approve(self, label, container, changed):
        need(label in ("acct1", "acct2") and re.fullmatch(r"iCloud\.[A-Za-z0-9.-]+", container), "setup-prompt")
        need(h.directory_pin(os.fstat(self.fd)) == self.pin and os.tcgetpgrp(self.fd) == os.getpgrp(), "setup-terminal-changed")
        expected_input = ("replace " if changed else "approve ") + label
        # Do not display an account-derived value or even its digest.
        os.write(self.fd, (label + " " + container + " stored entry changes: " +
                 ("yes" if changed else "no") + "\nEnter " + expected_input + ": ").encode())
        response, complete = bytearray(), False
        while len(response) <= 64:
            chunk = os.read(self.fd, 1)
            if not chunk or chunk == b"\n":
                complete = chunk == b"\n"
                break
            response.extend(chunk)
        return complete and bytes(response) == expected_input.encode()

    def close(self):
        os.close(self.fd)


@contextlib.contextmanager
def owned_vm(root, run, context):
    need(isinstance(context, dict) and set(context) == {"selection", "lease_slot"}, "setup-context")
    holder = h.Original(os.path.join(context["lease_slot"], "holder.json"))
    try:
        need(holder.value.get("job") == "cloudkit-account-setup" and holder.value.get("run_id") == run and
             holder.value.get("worktree") == root and type(holder.value.get("pid")) is int and
             holder.value["pid"] > 1, "setup-lease")
        def check():
            holder.check()
            os.kill(holder.value["pid"], 0)
            raw = v.command(["/bin/ps", "-A", "-o", "pid=", "-o", "ppid="], "setup-lease").stdout
            parents = dict(tuple(map(int, line.split())) for line in raw.splitlines())
            pid = os.getpid()
            for _ in range(200):
                if pid == holder.value["pid"]:
                    return
                pid = parents.get(pid, 0)
            raise v.Refused("setup-lease")
        check()
        yield h.VM(root, context["selection"], check), check
        check()
    finally:
        holder.close()


def setup_plan(manifest, path):
    products = manifest["products"]["path"]
    xpath = products + "/" + manifest["xctestrun"]["path"]
    raw = h.checked_raw(xpath)
    need(v.digest(raw) == manifest["xctestrun"]["sha256"] and len(raw) == manifest["xctestrun"]["bytes"], "setup-plan")
    plan = plistlib.loads(raw)
    target = plan["TestConfigurations"][0]["TestTargets"][0]
    need(len(plan["TestConfigurations"]) == 1 and len(plan["TestConfigurations"][0]["TestTargets"]) == 1 and
         target["BlueprintName"] == "MeguriSnapshotTests" and
         isinstance(target["OnlyTestIdentifiers"], list) and len(target["OnlyTestIdentifiers"]) == 2 and
         all(isinstance(name, str) for name in target["OnlyTestIdentifiers"]) and
         set(target["OnlyTestIdentifiers"]) == {"CloudKitSoakTests", "CloudKitPathPreflightTests"} and
         not target.get("SkipTestIdentifiers"), "setup-selection")
    target["OnlyTestIdentifiers"] = ["CloudKitAccountSetupTests/testOwnerSetup"]
    for key in ("EnvironmentVariables", "TestingEnvironmentVariables"):
        need(not any(k.startswith(("MEGURI_CLOUD_AGENT_", "MEGURI_CLOUD_ACCOUNT_", "TEST_RUNNER_"))
                     for k in target.get(key, {})), "setup-environment")
    env = target.setdefault("EnvironmentVariables", {})
    env["MEGURI_CLOUD_ACCOUNT_SETUP"] = "1"
    env["MEGURI_CLOUD_ACCOUNT_DIRECTORY"] = path
    def expand(value):
        if isinstance(value, str):
            return value.replace("__TESTROOT__", os.path.dirname(xpath))
        if isinstance(value, list):
            return [expand(item) for item in value]
        if isinstance(value, dict):
            return {key: expand(item) for key, item in value.items()}
        return value
    return raw, plistlib.dumps(expand(plan))


def guest_setup():
    """Original VM supervisor: private socket, original app, original join."""
    request, raw = s.frame_read(sys.stdin.buffer)
    need(request.pop("action") == "launch" and not raw, "setup-order")
    manifest, support = request["manifest"], request["support"]
    base = h.Directory(support["path"])
    child, peer, server = None, None, None
    with contextlib.ExitStack() as held:
        held.callback(base.close)
        need(h.root_state(base, support["path"]) == support, "setup-root")
        # Trusted OS Application Support is canonical before private creation.
        name = ".meguri-account-" + os.urandom(16).hex()
        os.mkdir(name, 0o700, dir_fd=base.fd)
        path = support["path"] + "/" + name
        directory = h.Directory(path)
        held.callback(directory.close)
        need(stat.S_IMODE(os.fstat(directory.fd).st_mode) == 0o700, "setup-directory")
        original, plan = setup_plan(manifest, path)
        h.write_new(directory.fd, "owner.xctestrun", plan)
        server = held.enter_context(socket.socket(socket.AF_UNIX, socket.SOCK_STREAM))
        server.settimeout(WAIT)
        previous = os.open(".", os.O_RDONLY | os.O_DIRECTORY)
        try:
            os.fchdir(directory.fd)
            server.bind("channel")
            os.chmod("channel", 0o600)
        finally:
            os.fchdir(previous)
            os.close(previous)
        server.listen(1)
        out, err = held.enter_context(tempfile.TemporaryFile()), held.enter_context(tempfile.TemporaryFile())
        try:
            environment = {k: val for k, val in os.environ.items()
                           if not k.startswith(("TEST_RUNNER_", "MEGURI_CLOUD_AGENT_", "MEGURI_CLOUD_ACCOUNT_"))}
            with h.setup_launch():
                child = subprocess.Popen(["/usr/bin/xcodebuild", "test-without-building", "-xctestrun", path + "/owner.xctestrun",
                    "-destination", "platform=macOS", "-only-testing:MeguriSnapshotTests/CloudKitAccountSetupTests/testOwnerSetup",
                    "-parallel-testing-enabled", "NO", "-collect-test-diagnostics", "never", "-resultBundlePath", path + "/owner.xcresult"],
                    env=environment, stdout=out, stderr=err, start_new_session=True)
            peer, _ = server.accept()
            held.callback(peer.close)
            peer.settimeout(WAIT)
            # Darwin LOCAL_PEERPID: bind this retained socket to its original
            # app PID, not a later app found by name or bundle path.
            pid = peer.getsockopt(0, 2)
            need(pid > 1, "setup-peer")
            stream = peer.makefile("rwb")
            held.callback(stream.close)
            hello, payload = s.frame_read(stream)
            need(not payload and set(hello) == {"pid", "observation", "container"} and hello["pid"] == pid, "setup-peer")
            s.frame_write(sys.stdout.buffer, hello)
            phase = 0
            while True:
                request, raw = s.frame_read(sys.stdin.buffer)
                need(not raw and child.poll() is None, "setup-original-ended")
                os.kill(pid, 0)
                base.check()
                directory.check()
                need(h.checked_raw(manifest["products"]["path"] + "/" + manifest["xctestrun"]["path"]) == original,
                     "setup-plan-changed")
                action = request["action"]
                need((phase == 0 and action == "capture") or (phase == 1 and action == "fresh") or
                     (phase == 2 and action in ("hold", "finish")), "setup-order")
                s.frame_write(stream, {"action": action})
                reply, value = s.frame_read(stream)
                need(not value, "setup-wire")
                if action == "finish":
                    need(reply == {"finished": True}, "setup-finish")
                    child.wait(timeout=WAIT)
                    need(child.returncode == 0, "setup-test-failed")
                    summary = strict(h.setup_command(["/usr/bin/xcrun", "xcresulttool", "get", "test-results", "summary",
                                                       "--path", path + "/owner.xcresult"]))
                    need(summary.get("passedTests") == 1 and summary.get("failedTests") == 0 and
                         summary.get("skippedTests") == 0, "setup-test-result")
                    s.frame_write(sys.stdout.buffer, {"joined": True})
                    return
                s.frame_write(sys.stdout.buffer, reply)
                if action != "hold":
                    phase += 1
        finally:
            if child is not None:
                h.setup_join(child)


def guest_source():
    definitions = (v.Refused, v.need, v.digest, v.identity, h.directory_pin, h.file_pin, i.strict, i.raw_json,
        h.Directory, h.write_new, h.root_state, h.checked_raw, h.setup_cancel, h.setup_launch,
        h.setup_join, h.setup_command, s.frame_read, s.frame_write, setup_plan, guest_setup)
    names = ("Directory", "write_new", "root_state", "checked_raw", "setup_cancel", "setup_launch", "setup_join", "setup_command")
    preamble = ("import os,sys,stat,json,re,hashlib,contextlib,signal,subprocess,tempfile,plistlib,socket\n"
                "from types import SimpleNamespace\nCAP=16*1024*1024\nWAIT=600\nTREE_CAP=128*1024*1024\n")
    return (preamble + "\n".join(inspect.getsource(f) for f in definitions) +
        "\nv=SimpleNamespace(digest=digest,identity=identity,Refused=Refused)\ni=SimpleNamespace(raw_json=raw_json)\n" +
        "h=SimpleNamespace(**{k:globals()[k] for k in " + repr(names) + "})\n" +
        "s=SimpleNamespace(frame_read=frame_read,frame_write=frame_write)\n" +
        "for n in (signal.SIGINT,signal.SIGTERM,signal.SIGHUP): signal.signal(n,setup_cancel)\n"
        "try: guest_setup()\nexcept BaseException: sys.exit(1)\n").encode()


class Transport(s.Transport):
    def source(self):
        return guest_source()


def perform(label, source, terminal, transport, check, diagnostic=None):
    """The generated-test boundary. Production callbacks below are actual only."""
    diagnostic = diagnostic or SetupDiagnostic()
    diagnostic.enter("capture-precheck")
    source.check()
    check()
    diagnostic.enter("capture")
    reply, raw = transport.request("capture")
    need(not raw and set(reply) == {"status", "identity", "observation"}, "setup-capture")
    if reply["status"] == "no-account":
        raise s.UserStep("cloudkit-test-accounts")
    need(reply["status"] == "available", "setup-account-read")
    candidate = identity(reply["identity"])
    observation = reply["observation"]
    diagnostic.enter("capture-proof")
    check(observation)
    source.check()
    proposed = source.proposed(label, candidate)
    changed = label in source.accounts and source.accounts[label] != candidate
    diagnostic.enter("terminal-approval")
    if not terminal.approve(label, source.container, changed):
        raise v.Refused("setup-declined")
    diagnostic.enter("fresh-precheck")
    source.check()
    check()
    diagnostic.enter("fresh")
    fresh, raw = transport.request("fresh")
    need(not raw and set(fresh) == {"status", "identity", "observation"} and
         fresh["status"] == "available" and identity(fresh["identity"]) == candidate and
         fresh["observation"] == observation, "setup-fresh-changed")
    diagnostic.enter("fresh-proof")
    check(fresh["observation"])
    source.check()
    def retained():
        check()
        reply, raw = transport.request("hold")
        need(not raw and reply == {"observation": observation}, "setup-original-changed")
        check(reply["observation"])
    diagnostic.enter("publish")
    source.publish(proposed, retained)
    try:
        diagnostic.enter("finish")
        reply, raw = transport.request("finish")
        need(not raw and reply == {"joined": True}, "setup-original-join")
    except BaseException:
        raise PublicationUncertain("publication-uncertain") from None


def run(root, run_id, label, context, diagnostic=None):
    need(label in ("acct1", "acct2") and v.MG_RUN_ID.fullmatch(run_id), "setup-arguments")
    # Refuse noninteractive callers before building or obtaining any candidate.
    diagnostic = diagnostic or SetupDiagnostic()
    diagnostic.enter("terminal")
    terminal = Terminal()
    source = None
    try:
        with contextlib.ExitStack() as held:
            held.callback(terminal.close)
            diagnostic.enter("vm-context")
            vm, lease = held.enter_context(owned_vm(root, run_id, context))
            machine = i.machine(vm)
            sha = v.source(root)
            diagnostic.enter("native-build")
            reference = s.child(root, "build-app", ["--platform", "mac", "--config", "Debug", "--test-plan", "CloudAgent"], sha)
            diagnostic.enter("product-input")
            results = os.path.join(root, "build/mg")
            receipt, receipt_hash = v.receipt(root, results, reference, sha)
            manifest = v.json_input(os.path.join(root, "build"), v.declared_reference(root, receipt, receipt["details"]["agent_build"]))
            products = manifest["products"]["path"]
            product = products + "/" + manifest["app"]["path"]
            info = v.plist(v._first_sync_bytes(products, product + "/Contents/Info.plist"), "setup-info")
            container = info.get("MeguriCloudContainer")
            need(isinstance(container, str) and re.fullmatch(r"iCloud\.[A-Za-z0-9.-]+", container) and
                 info.get("MeguriCloudEnvironment") == "development", "setup-container")
            expected = {"source_commit": sha, "build_receipt": reference, "device_kind": "mac", "app_build": manifest["app_build"],
                        "executable_sha256": manifest["app"]["executable_sha256"], "container": container, "environment": "development",
                        "os_build": machine["os_build"]}
            originals = []
            for path in (os.path.join(results, "build-app", reference["run_id"], "result.json"),
                         receipt["details"]["agent_build"]["path"],
                         os.path.join(results, "build-app", reference["run_id"], "agent-cloud-settings.json")):
                original = h.Original(path)
                held.callback(original.close)
                originals.append(original)
            product_pin = None
            def product_check():
                nonlocal product_pin
                for original in originals:
                    original.check()
                lease()
                need(i.machine(vm) == machine and v.source(root) == sha, "setup-machine-changed")
                actual = v.admit_product(root, results, expected, machine["provisioning_udid"])
                need(actual[0] == receipt and actual[1] == receipt_hash and actual[2] == manifest, "setup-product-changed")
                if product_pin is None:
                    product_pin = actual[3]
                need(actual[3] == product_pin, "setup-product-authority-changed")
                return actual[3]
            try:
                diagnostic.enter("product-check")
                frozen = product_check()
            except v.Refused as error:
                if error.args == ("product-schema",):
                    raise s.UserStep("cloudkit-dev-container") from None
                raise
            diagnostic.enter("approved-source")
            source = Source(a._platform_application_support_root(), container)
            held.callback(source.close)
            if label == "acct2" and "acct1" not in source.accounts:
                raise s.UserStep("cloudkit-test-accounts")
            diagnostic.enter("copy-products")
            vm.copy(root, products, frozen[0])
            delivered = vm.guest("inventory-pins", os.path.join(root, "build"), products)
            need(isinstance(delivered, dict) and set(delivered) == {"path", "entries", "pins"} and
                 delivered["path"] == products and delivered["entries"] == frozen[0] and
                 isinstance(delivered["pins"], dict) and set(delivered["pins"]) == {"", *frozen[0]}, "setup-delivery")
            product_check()
            diagnostic.enter("path-preflight")
            preparation = vm.setup("preflight", manifest)
            support_path = h.admit_preflight(root, manifest, expected, preparation)
            diagnostic.enter("support-authority")
            support = vm.setup("support", {"root": support_path})
            need(support == {"path": support_path, "chain": preparation["application_support_chain"]}, "setup-root-authority")
            transport = Transport(vm)
            held.callback(transport.close)
            diagnostic.enter("owner-launch")
            hello, raw = transport.request("launch", manifest=manifest, support=support)
            need(not raw and set(hello) == {"pid", "observation", "container"} and hello["container"] == container,
                 "setup-hello")
            observation = hello["observation"]
            def check(current=None):
                product_check()
                need(vm.guest("inventory-pins", os.path.join(root, "build"), products) == delivered, "setup-delivery-changed")
                need(observation["application_support_chain"] == support["chain"], "setup-root-authority")
                if current is not None:
                    need(current == observation, "setup-loaded-changed")
                # Independent live Mach-O UUID/hash binding before either SDK read.
                need(h.admit_preflight(root, manifest, expected, observation) == support_path, "setup-root-changed")
            diagnostic.enter("loaded-owner-proof")
            check(observation)
            try:
                perform(label, source, terminal, transport, check, diagnostic)
                need(transport.child.wait(timeout=WAIT) == 0, "setup-transport-join")
                lease()
            except BaseException:
                if source.started:
                    raise PublicationUncertain("publication-uncertain") from None
                raise
    except BaseException:
        if source is not None and source.started:
            raise PublicationUncertain("publication-uncertain") from None
        raise
    return {"scope": "SETUP-ONLY", "outcome": "setup_completed", "label": label, "signed_execution": False}


def main():
    outcome, category, code, label = "failed_evidence", "setup-refused", 1, None
    completed = False
    diagnostic = SetupDiagnostic()
    try:
        parser = argparse.ArgumentParser(add_help=False)
        parser.add_argument("--root", required=True)
        parser.add_argument("--run-id", required=True)
        parser.add_argument("--label", choices=("acct1", "acct2"), required=True)
        args = parser.parse_args()
        label = args.label
        for number in (signal.SIGINT, signal.SIGTERM, signal.SIGHUP):
            signal.signal(number, h.setup_cancel)
        with os.fdopen(os.dup(3), "rb") as stream:
            context = strict(stream.read(CAP + 1))
        result = run(args.root, args.run_id, label, context, diagnostic)
        completed = True
        print(json.dumps(result, sort_keys=True))
        return 0
    except PublicationUncertain:
        outcome, category = "failed_evidence", "publication-uncertain"
    except s.UserStep as error:
        outcome, category, code = "awaiting_user", str(error), 76
    except a.InputFailure as error:
        outcome, category = error.outcome, error.category
        code = 78 if outcome == "implementation_missing" else 1
    except BaseException as error:
        diagnostic.emit(error)
        if completed:
            category = "publication-uncertain"
        # Never echo exception/SDK/private path/candidate data.
    print(json.dumps({"scope": "SETUP-ONLY", "outcome": outcome, "category": category,
                      "label": label, "signed_execution": False}, sort_keys=True))
    return code


if __name__ == "__main__":
    sys.exit(main())
