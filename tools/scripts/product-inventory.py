#!/usr/bin/env python3
"""Guarded product reads and inventory shared by physical-device verification."""
import contextlib
import hashlib
import os
from pathlib import Path
import plistlib
import re
import stat
import subprocess
import sys
sys.path.insert(0, str(Path(__file__).parent / "gates"))
from milestone_gate import Invalid, _first_sync_bytes
PRODUCT_BYTES = 16 * 1024 * 1024 * 1024
PRODUCT_ENTRIES = 32768
CHUNK_BYTES = 1024 * 1024

class Refused(Exception):
    """Its arguments are fixed public categories, never input values."""


def need(condition, category):
    if not condition:
        raise Refused(category)


def digest(raw):
    return hashlib.sha256(raw).hexdigest()


def contained(boundary, path):
    need(isinstance(path, str) and os.path.isabs(path) and
         ".." not in path.split(os.sep) and "\x00" not in path, "artifact-path")
    need(os.path.normpath(path) == path and
         os.path.commonpath((os.path.abspath(boundary), path)) == os.path.abspath(boundary)
         and path != os.path.abspath(boundary), "artifact-path")
    return path


def command(argv, category):
    """Fixed executable paths; no shell, public overrides or diagnostic echo."""
    try:
        result = subprocess.run(argv, capture_output=True, timeout=60, check=False)
        need(result.returncode == 0 and
             len(result.stdout) + len(result.stderr) <= 16 * 1024 * 1024, category)
        return result
    except (OSError, subprocess.SubprocessError):
        raise Refused(category) from None


def plist(raw, category):
    try:
        value = plistlib.loads(raw)
        need(isinstance(value, dict), category)
        return value
    except (ValueError, TypeError, plistlib.InvalidFileException):
        raise Refused(category) from None


def identity(info):
    return (info.st_dev, info.st_ino, info.st_mode, info.st_nlink,
            info.st_size, info.st_mtime_ns, info.st_ctime_ns)


def directory_identity(info):
    return (info.st_dev, info.st_ino, info.st_mode)


@contextlib.contextmanager
def authority(path, witness=None):
    """Hold every physical parent and check its binding before releasing it."""
    need(os.path.isabs(path) and os.path.normpath(path) == path and
         "\x00" not in path, "artifact-path")
    held, bindings = [], []
    try:
        fd = os.open("/", os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
        held.append(fd)
        for part in path.split("/")[1:]:
            child = os.open(part, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=fd)
            held.append(child)
            pin = directory_identity(os.fstat(child))
            bindings.append((fd, part, child, pin))
            fd = child
        def check():
            for parent, name, child, pin in bindings:
                need(directory_identity(os.fstat(child)) == pin and
                     directory_identity(os.stat(name, dir_fd=parent, follow_symlinks=False)) == pin,
                     "product-changed")
        if witness is not None:
            witness["check"] = check
        yield fd
        check()
    except OSError:
        raise Refused("product-path") from None
    finally:
        for fd in reversed(held):
            os.close(fd)


def stream_file(parent, name, budget):
    """Hash the opened inode; refuse mutation, replacement and hard links."""
    fd = os.open(name, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK, dir_fd=parent)
    try:
        info = os.fstat(fd)
        need(stat.S_ISREG(info.st_mode) and info.st_nlink == 1, "product-path")
        pin = identity(info)
        need(info.st_size <= budget[0], "product-size")
        hashed, count = hashlib.sha256(), 0
        while True:
            raw = os.read(fd, min(CHUNK_BYTES, budget[0] + 1))
            if not raw:
                break
            count += len(raw)
            budget[0] -= len(raw)
            need(budget[0] >= 0, "product-size")
            hashed.update(raw)
        need(count == info.st_size and identity(os.fstat(fd)) == pin and
             identity(os.stat(name, dir_fd=parent, follow_symlinks=False)) == pin,
             "product-changed")
        return {"type": "file", "sha256": hashed.hexdigest(), "bytes": count,
                "mode": stat.S_IMODE(info.st_mode)}, pin
    finally:
        os.close(fd)


def relative(path):
    need(isinstance(path, str) and path and not path.startswith("/") and
         "\x00" not in path and all(p not in ("", ".", "..") for p in path.split("/")),
         "product-path")
    return path


def link_terminal(entries, name):
    parts, resolved, links = name.split("/"), [], 0
    while parts:
        part = parts.pop(0)
        if part in ("", "."):
            continue
        if part == "..":
            need(resolved, "product-link")
            resolved.pop()
            continue
        current = "/".join(resolved + [part])
        need(current in entries, "product-link")
        entry = entries[current]
        if entry["type"] == "symlink":
            links += 1
            need(links <= 40, "product-link")
            target = entry["target"]
            need(isinstance(target, str) and target and not target.startswith("/") and
                 "\x00" not in target, "product-link")
            # Expand the target before processing any following parent step.
            parts = target.split("/") + parts
        else:
            if parts:
                need(entry["type"] == "directory", "product-link")
            resolved.append(part)
    return relative("/".join(resolved))


def snapshot(boundary, product):
    """Inventory all product inputs through held no-follow directory handles."""
    contained(boundary, product)
    entries, pins, budget = {}, {}, [PRODUCT_BYTES]

    def walk(fd, prefix):
        before = os.fstat(fd)
        names = sorted(os.listdir(fd))
        for name in names:
            path = prefix + name
            relative(path)
            need(len(entries) < PRODUCT_ENTRIES, "product-size")
            info = os.stat(name, dir_fd=fd, follow_symlinks=False)
            if stat.S_ISDIR(info.st_mode):
                child = os.open(name, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=fd)
                try:
                    pin = directory_identity(info)
                    need(directory_identity(os.fstat(child)) == pin, "product-changed")
                    entries[path] = {"type": "directory", "mode": stat.S_IMODE(info.st_mode)}
                    pins[path] = pin
                    walk(child, path + "/")
                    need(directory_identity(os.stat(name, dir_fd=fd, follow_symlinks=False)) == pin,
                         "product-changed")
                finally:
                    os.close(child)
            elif stat.S_ISLNK(info.st_mode):
                target = os.readlink(name, dir_fd=fd)
                need(identity(os.stat(name, dir_fd=fd, follow_symlinks=False)) == identity(info),
                     "product-changed")
                entries[path] = {"type": "symlink", "target": target}
                pins[path] = identity(info)
            else:
                entry, pin = stream_file(fd, name, budget)
                need(pin == identity(info), "product-changed")
                entries[path], pins[path] = entry, pin
        after = os.fstat(fd)
        need(directory_identity(after) == directory_identity(before) and
             after.st_mtime_ns == before.st_mtime_ns and after.st_ctime_ns == before.st_ctime_ns and
             sorted(os.listdir(fd)) == names, "product-changed")

    with authority(product) as fd:
        pins[""] = directory_identity(os.fstat(fd))
        walk(fd, "")
        for name, entry in entries.items():
            if entry["type"] == "symlink":
                link_terminal(entries, name)
    need(entries, "product-path")
    return entries, pins
