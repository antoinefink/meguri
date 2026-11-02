#!/usr/bin/env python3
"""Publish a native Cargo executable for an admitted Swift or Xcode Unit run."""
import importlib.util
import os
from pathlib import Path
import stat
import sys

# Reuse the existing retained-directory authority and bounded code hashing.
# Never use the review lane's Docker-only COMPILED path or invoke its runner.
sys.dont_write_bytecode = True
_spec = importlib.util.spec_from_file_location(
    "native_review", Path(__file__).with_name("native-media-review-capture.py"))
review = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(review)
require = review.require


def publish(run, source):
    require(run.parent in (review.ROOT / "build/mg/swift-test",
                           review.ROOT / "build/mg/xcode-check"), "Swift/Xcode Unit run scope")
    require(run.name not in ("", ".", "..", "latest") and str(run) == os.environ.get("MG_RUN_DIR")
            and run == run.resolve(strict=True), "current canonical MG run required")
    if run.parent.name == "xcode-check":
        require(os.environ.get("MG_JOB") == "xcode-check"
                and os.environ.get("MG_RUN_ID") == run.name, "current Xcode MG run required")
    require(source.is_absolute(), "absolute Cargo executable required")
    with review.owned_directory(run) as (_, verify_run):
        with review.export.retained_directory(source.parent) as (parent, verify_parent):
            fd = os.open(source.name, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK, dir_fd=parent)
            try:
                before = os.fstat(fd)
                # Cargo legitimately hardlinks its deps artifact. Only the copy
                # we publish must have one link; never constrain source nlink.
                require(stat.S_ISREG(before.st_mode) and before.st_uid == os.getuid()
                        and 0 < before.st_size <= review.CODE_LIMIT
                        and before.st_mode & 0o022 == 0 and before.st_mode & 0o111,
                        "native executable owner/mode/size")
                pin = review.code_signature(before)

                def verify_source():
                    require(pin == review.code_signature(os.fstat(fd))
                            == review.code_signature(os.stat(source.name, dir_fd=parent, follow_symlinks=False)),
                            "native executable changed")
                    verify_parent()

                actual = review.code_hash(fd)
                verify_source()
                require(actual["bytes"] == before.st_size, "native executable size changed")
                directory = run / "cloud-service-code"
                review.fresh_directory(directory)
                binary = directory / "meguri-cli"
                with review.owned_directory(directory) as (destination, verify_destination):
                    out = os.open(binary.name, os.O_RDWR | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW,
                                  0o600, dir_fd=destination)
                    try:
                        os.lseek(fd, 0, os.SEEK_SET)
                        remaining = actual["bytes"]
                        while remaining:
                            block = os.read(fd, min(1024 * 1024, remaining))
                            require(block, "native executable truncated")
                            remaining -= len(block)
                            pending = memoryview(block)
                            while pending:
                                count = os.write(out, pending)
                                require(count > 0, "native copy progress")
                                pending = pending[count:]
                        require(not os.read(fd, 1), "native executable grew")
                        verify_source()
                        require(review.code_hash(fd) == actual, "native executable hash changed")
                        verify_source()
                        os.fchmod(out, 0o500)
                        os.fsync(out)
                        metadata = review.export.file_metadata(os.fstat(out))
                        require(os.fstat(out).st_uid == os.getuid() and review.code_hash(out) == actual
                                and metadata == review.export.file_metadata(os.fstat(out))
                                == review.export.file_metadata(os.stat(binary.name, dir_fd=destination,
                                                                       follow_symlinks=False)),
                                "published executable changed")
                        verify_destination()
                        verify_source()
                        verify_run()
                        review.write(run / "cloud-service-code.json", {
                            "version": 1, "run": str(run), "compiler": str(source),
                            "compiler_pin": pin, "compiled": actual, "published": str(binary),
                            "executable": actual, "published_pin": review.code_signature(os.fstat(out)),
                            "mode": 0o500, "build_profile": "dev",
                            "build": review.identity(run / "cloud-service-build.jsonl"),
                        })
                        verify_destination()
                        verify_source()
                        require(metadata == review.export.file_metadata(os.stat(
                            binary.name, dir_fd=destination, follow_symlinks=False))
                                and os.fstat(out).st_uid == os.getuid(), "published binding changed")
                    finally:
                        os.close(out)
            finally:
                os.close(fd)
    return binary


if __name__ == "__main__":
    try:
        require(len(sys.argv) == 3, "publication arguments")
        print(publish(Path(sys.argv[1]), Path(sys.argv[2])))
    except (OSError, ValueError):
        # No native error/path/body interpolation in failure diagnostics.
        print("cloud service helper publication-error", file=sys.stderr)
        sys.exit(1)
