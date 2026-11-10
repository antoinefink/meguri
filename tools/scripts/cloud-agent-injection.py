#!/usr/bin/env python3
"""Prepare a verified Xcode input for native CopyFiles/CodeSignOnCopy only.

No product signing or receipt repair happens here. The native build completes
all nested and outer signing before app.sh invokes its existing final freeze.
"""
import contextlib
import importlib.util
import os
from pathlib import Path
import signal
import stat
import subprocess
import sys
import tempfile

spec = importlib.util.spec_from_file_location("injection_xcode_authority", Path(__file__).with_name("vm-xcode-stage.py"))
stage = importlib.util.module_from_spec(spec)
spec.loader.exec_module(stage)

NAME = "libXCTestBundleInject.dylib"
SOURCE = "Contents/Developer/Platforms/MacOSX.platform/Developer/usr/lib/" + NAME
SOURCES = {"mac": SOURCE, "ios-sim": "Contents/Developer/Platforms/iPhoneSimulator.platform/Developer/usr/lib/" + NAME}
DESTINATIONS = {"platform=macOS,arch=arm64": "mac", "generic/platform=iOS Simulator": "ios-sim"}
LIMIT = 64 * 1024 * 1024


def enabled(env):
    flag = env.get("MEGURI_CLOUD_AGENT_INJECTION", "NO")
    stage.need(flag in ("NO", "YES"))
    return flag == "YES" and env.get("PLATFORM_NAME") in ("macosx", "iphonesimulator") and env.get("CONFIGURATION") == "Debug"


def read_exact(fd, expected):
    stage.need(stat.S_ISREG(expected.st_mode) and expected.st_nlink == 1 and
               0 < expected.st_size <= LIMIT)
    os.lseek(fd, 0, 0)
    chunks, count = [], 0
    while count <= expected.st_size:
        raw = os.read(fd, min(1024 * 1024, expected.st_size + 1 - count))
        if not raw:
            break
        chunks.append(raw)
        count += len(raw)
    stage.need(count == expected.st_size and stage.file_pin(os.fstat(fd)) == stage.file_pin(expected))
    return b"".join(chunks)


def bound_file(directory, name, fd, original, raw):
    directory.check()
    stage.need(stage.file_pin(os.stat(name, dir_fd=directory.fd, follow_symlinks=False)) ==
               stage.file_pin(original) == stage.file_pin(os.fstat(fd)))
    stage.need(read_exact(fd, original) == raw)


@contextlib.contextmanager
def prepare(run, *, platform="mac"):
    stage.need(platform in SOURCES)
    source = SOURCES[platform]
    # Each invocation gets an exclusive private stage; never adopt old inputs.
    with stage.Directory(run) as run_dir:
        private_path = tempfile.mkdtemp(prefix="cloud-agent-input-", dir=run)
        run_dir.check()
        with stage.Directory(private_path) as private, stage.selected_source(private) as selected:
            developer, bundle, authority = selected
            with stage.Directory(bundle.path + "/" + source.rsplit("/", 1)[0]) as source_dir:
                # Check every original ancestor using the same installed-source
                # authority, including OS-owned group-writable Xcode entries.
                stage.need(source_dir.check()[:len(bundle.bindings)] == bundle.check())
                for _, _, fd, _ in source_dir.bindings[len(bundle.bindings):]:
                    metadata = os.fstat(fd)
                    stage.entry_mode(stat.S_IMODE(metadata.st_mode), False,
                                     installed_source=authority, metadata=metadata)
                fd = os.open(NAME, stage.FILE_FLAGS, dir_fd=source_dir.fd)
                try:
                    original = os.fstat(fd)
                    stage.need(original.st_uid in (0, os.getuid()))
                    stage.entry_mode(stat.S_IMODE(original.st_mode), False,
                                     installed_source=authority, metadata=original)
                    raw = read_exact(fd, original)
                    bound_file(source_dir, NAME, fd, original, raw)
                    # Revalidate the signed source after reading the exact bytes.
                    stage.facts(bundle, source_diagnostics=private)
                    bound_file(source_dir, NAME, fd, original, raw)
                    os.mkdir("input", 0o700, dir_fd=private.fd)
                    with stage.Directory(private_path + "/input") as destination:
                        out = os.open(NAME, os.O_RDWR | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW,
                                      0o600, dir_fd=destination.fd)
                        try:
                            remaining = memoryview(raw)
                            while remaining:
                                count = os.write(out, remaining)
                                stage.need(count > 0)
                                remaining = remaining[count:]
                            # Native CodeSignOnCopy must be able to sign its
                            # normal writable copy; retained pins bind our input.
                            os.fchmod(out, 0o644 | (stat.S_IMODE(original.st_mode) & 0o111))
                            os.fsync(out)
                            copied = os.fstat(out)
                            bound_file(destination, NAME, out, copied, raw)
                            yield destination.path
                            # Keep both descriptors through the joined native
                            # build; substitutions fail before final freezing.
                            bound_file(destination, NAME, out, copied, raw)
                            bound_file(source_dir, NAME, fd, original, raw)
                            authority.check()
                            developer.check()
                            private.check()
                        finally:
                            os.close(out)
                finally:
                    os.close(fd)


def remove(frameworks):
    """Remove only the old generated input before native product signing."""
    try:
        directory = stage.Directory(frameworks)
    except FileNotFoundError:
        return
    with directory:
        try:
            s = os.stat(NAME, dir_fd=directory.fd, follow_symlinks=False)
        except FileNotFoundError:
            return
        stage.need(stat.S_ISREG(s.st_mode) and s.st_nlink == 1 and s.st_uid == os.getuid())
        directory.check()
        os.unlink(NAME, dir_fd=directory.fd)
        directory.check()


def cleanup(env):
    active = enabled(env)
    stage.need(env.get("MEGURI_CLOUD_AGENT_EMBED", "NO") == ("YES" if active else "NO"))
    if not active:
        remove(env["TARGET_BUILD_DIR"] + "/" + env["FRAMEWORKS_FOLDER_PATH"])


def build(run, frameworks, args):
    stage.need(args and args[0] == "build-for-testing")
    for flag, value in (("-project", "app/Meguri.xcodeproj"), ("-scheme", "Meguri"),
                        ("-testPlan", "CloudAgent"), ("-configuration", "Debug")):
        stage.need(args.count(flag) == 1 and args.index(flag) + 1 < len(args) and
                   args[args.index(flag) + 1] == value)
    stage.need(args.count("-destination") == 1 and args.index("-destination") + 1 < len(args))
    destination = args[args.index("-destination") + 1]
    stage.need(destination in DESTINATIONS)
    platform = DESTINATIONS[destination]
    stage.need(not any(arg.startswith(("MEGURI_CLOUD_AGENT_", "EXCLUDED_SOURCE_FILE_NAMES=",
                                       "COPY_PHASE_STRIP=")) for arg in args))
    with prepare(run, platform=platform) as prepared:
        remove(frameworks)
        env = dict(os.environ)
        for key in ("DEVELOPER_DIR", "SDKROOT", "TOOLCHAINS"):
            env.pop(key, None)
        child = None
        try:
            # Retain the original group before deferred cancellation can unwind
            # preparation and release its verified source and staged file FDs.
            with stage.launching():
                child = subprocess.Popen(["/usr/bin/xcodebuild", *args,
                                          "MEGURI_CLOUD_AGENT_INJECTION=YES",
                                          "MEGURI_CLOUD_AGENT_INJECTION_DIR=" + prepared],
                                         env=env, start_new_session=True)
            return child.wait()
        finally:
            with stage.finishing():
                if child is not None:
                    stage.join_original(child)


def main(args):
    for sig in (signal.SIGINT, signal.SIGTERM, signal.SIGHUP):
        signal.signal(sig, lambda *_: (_ for _ in ()).throw(stage.Cancelled()))
    if args == ["--cleanup"]:
        cleanup(os.environ)
        return 0
    if len(args) >= 5 and args[0] == "--build" and args[3] == "--":
        return build(args[1], args[2], args[4:])
    raise stage.Refused()


if __name__ == "__main__":
    try:
        sys.exit(main(sys.argv[1:]))
    except stage.Cancelled:
        sys.exit(130)
    except (stage.Refused, OSError, KeyError, ValueError):
        print("CloudAgent injection preparation refused; no signed build admission", file=sys.stderr)
        sys.exit(1)
