#!/usr/bin/env python3
"""Internal J2.2 approved-source reader/projection; no CLI or delivery.

The future parent must supply ORIGINAL, already validated header bytes and an
independently verified development product container. This helper does not
establish that product verification, account setup, delivery or signed execution.
Keep the context open through delivery/readback and call revalidate at each
admission boundary. Its bytes are private, never an MG artifact or log value.
"""

import contextlib
import importlib.util
import json
import os
from pathlib import Path
import re
import select
import stat
import subprocess
import sys

_spec = importlib.util.spec_from_file_location(
    "cloud_agent_account_handoff", Path(__file__).with_name("cloud-agent-handoff.py"))
h = importlib.util.module_from_spec(_spec)
sys.modules[_spec.name] = h
_spec.loader.exec_module(h)
v, need, strict = h.v, h.need, h.strict
LIMIT = 1_048_576  # Same bound as the existing device J2.2 consumer.
_NAME = "approved-accounts.json"
_BINDINGS = ("job", "run_id", "source_commit", "device", "executable_sha256",
             "container", "environment", "account_scope")
_HEADER_FIELDS = {"protocol_version", "job", "run_id", "lane", "source_commit", "tree_clean",
                  "device", "device_kind", "os_build", "app_build", "executable_sha256",
                  "build_receipt", "build_receipt_sha256", "deployment_receipt", "container",
                  "environment", "account_scope", "account_labels", "selection", "fixture_delivery"}


class InputFailure(v.Refused):
    """Only fixed outcomes may cross the private helper boundary."""
    def __init__(self, kind="refused"):
        self.outcome, self.category = {
            "refused": ("failed_evidence", "account-input-refused"),
            "setup": ("awaiting_user", "cloudkit-test-accounts"),
            "missing": ("implementation_missing", "account-input-platform-query"),
        }[kind]
        super().__init__(self.category)

    def public_outcome(self):
        return {"outcome": self.outcome, "category": self.category}


_ERRORS = (v.Refused, v.Invalid, OSError, ValueError, TypeError, KeyError,
           IndexError, AttributeError, UnicodeError, RecursionError, subprocess.SubprocessError)


def _platform_application_support_root():
    """Fixed non-UI Foundation query of this host process user's OS root.

    No path argument, HOME concatenation, environment override, SDK or current
    account lookup. JXA loads Foundation only; both command streams stay private.
    """
    if sys.platform != "darwin":
        raise InputFailure("missing")
    query = ("ObjC.import('Foundation'); "
             "var urls = $.NSFileManager.defaultManager.URLsForDirectoryInDomains("
             "$.NSApplicationSupportDirectory, $.NSUserDomainMask); "
             "if (Number(urls.count) !== 1) { throw Error('unavailable'); } "
             "JSON.stringify(ObjC.unwrap(urls.objectAtIndex(0).path));")
    try:
        result = subprocess.run(["/usr/bin/osascript", "-l", "JavaScript", "-e", query],
                                capture_output=True, timeout=30, check=False, env={})
        need(result.returncode == 0 and len(result.stdout) + len(result.stderr) <= LIMIT,
             "platform-query")
        path = strict(result.stdout.decode("utf-8"))
        need(isinstance(path, str) and path.startswith("/") and "\x00" not in path and
             len(path.encode()) <= 4096, "platform-query")
        return path
    except _ERRORS:
        raise InputFailure("missing") from None


def _header(raw, container):
    # Full header/product admission belongs to the parent. These checks protect
    # the projection's own exact wire schema and ensure no weakened byte shapes.
    need(type(raw) is bytes and 0 < len(raw) <= LIMIT, "header")
    value = strict(raw.decode("utf-8"))
    need(isinstance(value, dict) and set(value) == _HEADER_FIELDS and
         type(value["protocol_version"]) is int and value["protocol_version"] == 1,
         "header")
    need(isinstance(container, str) and re.fullmatch(r"iCloud\.[A-Za-z0-9.-]+", container) and
         value["container"] == container and value["environment"] == "development" and
         value["job"] == "cloudkit-signed" and value["lane"] == "cloud-signed" and
         value["tree_clean"] is True, "header")
    need(all(isinstance(value[k], str) and value[k] for k in _BINDINGS), "header")
    need(v.MG_RUN_ID.fullmatch(value["run_id"]) and
         re.fullmatch(r"[A-Za-z][A-Za-z0-9_-]{0,63}", value["device"]) and
         re.fullmatch(r"[0-9a-f]{40}", value["source_commit"]) and
         re.fullmatch(r"[0-9a-f]{64}", value["executable_sha256"]), "header")
    labels = value["account_labels"]
    need(isinstance(labels, list) and labels in (["acct1"], ["acct1", "acct2"], ["acct2", "acct1"]),
         "header")
    return value


class _Projection:
    def __init__(self):
        self._root, self._fd, self._events = None, None, None
        self._directories = []
        self._invalid = False

    def _watch(self, fd):
        if self._events is not None:
            event = select.kevent(fd, filter=select.KQ_FILTER_VNODE,
                                 flags=select.KQ_EV_ADD | select.KQ_EV_CLEAR,
                                 fflags=select.KQ_NOTE_RENAME | select.KQ_NOTE_DELETE |
                                 select.KQ_NOTE_REVOKE | select.KQ_NOTE_ATTRIB)
            self._events.control([event], 0, 0)

    def _check_directories(self):
        self._root.check()
        for parent, name, fd, pin in self._directories:
            need(h.directory_pin(os.fstat(fd)) == pin and
                 h.directory_pin(os.stat(name, dir_fd=parent, follow_symlinks=False)) == pin,
                 "private-directory-changed")
        if self._events is not None:
            need(not self._events.control(None, 1, 0), "private-directory-changed")

    def _missing(self, parent, name):
        # Absence is setup only while the original existing ancestors still bind.
        self._check_directories()
        try:
            os.stat(name, dir_fd=parent, follow_symlinks=False)
        except FileNotFoundError:
            raise InputFailure("setup") from None
        raise InputFailure()

    def _regular(self, info):
        need(stat.S_ISREG(info.st_mode) and stat.S_IMODE(info.st_mode) == 0o600 and
             info.st_uid == os.getuid() and info.st_nlink == 1 and
             0 < info.st_size <= LIMIT, "private-file-authority")

    def _check_file(self):
        for info in (os.fstat(self._fd), os.stat(_NAME, dir_fd=self._parent, follow_symlinks=False)):
            self._regular(info)
            need(h.file_pin(info) == self._pin, "private-file-changed")

    def _read(self):
        chunks, count = [], 0
        while True:
            raw = os.pread(self._fd, min(65536, LIMIT + 1 - count), count)
            if not raw:
                break
            chunks.append(raw)
            count += len(raw)
            need(count <= LIMIT, "private-file-size")
        need(count == self._pin[4], "private-file-size")
        return b"".join(chunks)

    def _open(self, root, header_raw, container):
        header = _header(header_raw, container)
        # Resolve ONLY the platform-observed trusted root. Fixed private suffixes
        # are opened relative to retained descriptors, never canonicalized.
        try:
            root = str(Path(root).resolve(strict=True))
        except FileNotFoundError:
            raise InputFailure("setup") from None
        self._root = h.Directory(root)
        if sys.platform == "darwin":
            self._events = select.kqueue()
            for fd in self._root.held:
                self._watch(fd)
        fd = self._root.fd
        for name in ("Meguri", "CloudKitSignedPrivate"):
            try:
                child = os.open(name, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW |
                                os.O_NONBLOCK, dir_fd=fd)
            except FileNotFoundError:
                self._missing(fd, name)
            try:
                info = os.fstat(child)
                pin = h.directory_pin(info)
            except BaseException:
                os.close(child)
                raise
            self._directories.append((fd, name, child, pin))
            self._watch(child)
            need(info.st_uid == os.getuid() and stat.S_IMODE(info.st_mode) == 0o700,
                 "private-directory-authority")
            fd = child
            self._check_directories()
        self._parent = fd
        try:
            self._fd = os.open(_NAME, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK, dir_fd=fd)
        except FileNotFoundError:
            self._missing(fd, _NAME)
        info = os.fstat(self._fd)
        self._regular(info)
        self._pin = h.file_pin(info)
        self._check_file()
        self._raw = self._read()
        self.revalidate()
        approved = strict(self._raw.decode("utf-8"))
        need(isinstance(approved, dict) and set(approved) ==
             {"protocol_version", "container", "environment", "accounts"}, "approved-schema")
        need(type(approved["protocol_version"]) is int and approved["protocol_version"] == 1 and
             approved["container"] == container and approved["environment"] == "development",
             "approved-binding")
        accounts = approved["accounts"]
        need(isinstance(accounts, dict) and set(accounts) in ({"acct1"}, {"acct1", "acct2"}),
             "approved-labels")
        need(all(isinstance(identity, list) and identity and
                 all(type(byte) is int and 0 <= byte <= 255 for byte in identity)
                 for identity in accounts.values()), "approved-identity")
        need("acct2" not in accounts or accounts["acct1"] != accounts["acct2"], "approved-distinct")
        if not set(header["account_labels"]) <= set(accounts):
            raise InputFailure("setup")
        projected = {"protocol_version": 1, "header_sha256": v.digest(header_raw),
                     **{key: header[key] for key in _BINDINGS},
                     "accounts": {label: accounts[label] for label in header["account_labels"]}}
        self._projected = (json.dumps(projected, sort_keys=True, allow_nan=False) + "\n").encode()
        need(len(self._projected) <= LIMIT, "private-projection-size")
        self.revalidate()

    def revalidate(self):
        """Retain original bytes, file and ancestor authority; failure is sticky."""
        try:
            need(not self._invalid, "private-context-closed")
            self._check_directories()
            self._check_file()
            need(self._read() == self._raw, "private-source-changed")
            self._check_file()
            self._check_directories()
        except _ERRORS:
            self._invalid = True
            raise InputFailure() from None

    def private_bytes(self):
        """Exact existing J2.2 JSON bytes, for the private delivery caller only."""
        self.revalidate()
        return self._projected

    def _close(self):
        self._invalid = True
        if self._events is not None:
            self._events.close()
        if self._fd is not None:
            os.close(self._fd)
        for _, _, fd, _ in reversed(self._directories):
            os.close(fd)
        if self._root is not None:
            self._root.close()


@contextlib.contextmanager
def approved_account_input(original_header_bytes, verified_container):
    """Yield a private projection; retain this context until delivery finishes.

    No source path parameter or public result on success. Missing approved data
    raises InputFailure(awaiting_user/cloudkit-test-accounts); missing platform
    query raises implementation_missing. Other refusals are content-free.
    The parent still owns full header/product checks and actual private delivery.
    """
    context = _Projection()
    try:
        try:
            root = _platform_application_support_root()
            context._open(root, original_header_bytes, verified_container)
        except InputFailure:
            raise
        except _ERRORS:
            raise InputFailure() from None
        yield context
        context.revalidate()
    finally:
        context._close()
