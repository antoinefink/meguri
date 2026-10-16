# SPDX-License-Identifier: AGPL-3.0-or-later
"""Identity of this oracle: what `hello` reports and goldens record (R1885).

The same driver runs in the oracle container (linux-arm64, linux-x86_64) and
natively on an arm64 Mac (macos-arm64, oracle/macos/run-native.sh,
float-determinism.md F6). Each lane reports the lock file it was installed
from: oracle/requirements.lock in the image, oracle/macos/requirements-macos.lock
for the native venv (run-native.sh refuses to start a worker from a venv
installed from another lock).
"""

from __future__ import annotations

import hashlib
import os
import platform as _platform
import sys
from importlib.metadata import version
from pathlib import Path

from . import PROTOCOL_VERSION

DRIVER_DIR = Path(__file__).resolve().parent
ORACLE_HOME = Path(os.environ.get("ORACLE_HOME", DRIVER_DIR.parent))

PLATFORMS = ("linux-arm64", "linux-x86_64", "macos-arm64")
TIME_CONTROLS = ("faketime", "none")
_MACHINES = {"aarch64": "arm64", "arm64": "arm64", "x86_64": "x86_64", "amd64": "x86_64"}


def platform_name() -> str:
    """linux-arm64, linux-x86_64 or macos-arm64; anything else is returned as
    <os>-<machine> and refused by the worker (protocol.json hello_response)."""
    machine = _MACHINES.get(_platform.machine().lower(), _platform.machine().lower())
    system = "macos" if sys.platform == "darwin" else "linux" if sys.platform.startswith("linux") else sys.platform
    return f"{system}-{machine}"


def is_native() -> bool:
    return platform_name().startswith("macos-")


def lock_file() -> Path:
    if is_native():
        return ORACLE_HOME / "macos" / "requirements-macos.lock"
    return ORACLE_HOME / "requirements.lock"


def anki_version() -> str:
    """The pinned distribution version (26.9.3), not the tag spelling (26.09.3)."""
    return version("anki")


def lock_sha256() -> str:
    return hashlib.sha256(lock_file().read_bytes()).hexdigest()


def driver_files() -> list[Path]:
    """Every driver file that can change an observation: tests and caches excluded."""
    out = []
    for p in DRIVER_DIR.rglob("*"):
        rel = p.relative_to(DRIVER_DIR)
        if not p.is_file() or rel.parts[0] == "tests" or "__pycache__" in rel.parts or p.suffix == ".pyc":
            continue
        out.append(p)
    return sorted(out, key=lambda p: p.relative_to(DRIVER_DIR).as_posix())


def driver_sha256() -> str:
    h = hashlib.sha256()
    for p in driver_files():
        h.update(p.relative_to(DRIVER_DIR).as_posix().encode("utf-8") + b"\0")
        h.update(hashlib.sha256(p.read_bytes()).hexdigest().encode("ascii") + b"\n")
    return h.hexdigest()


def hello(tz: str, time_control: str) -> dict:
    return {
        "protocol": PROTOCOL_VERSION,
        "anki_version": anki_version(),
        "lock_sha256": lock_sha256(),
        "driver_sha256": driver_sha256(),
        "tz": tz,
        "platform": platform_name(),
        "time_control": time_control,
    }
