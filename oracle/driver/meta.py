# SPDX-License-Identifier: AGPL-3.0-or-later
"""Identity of this oracle: what `hello` reports and goldens record (R1885)."""

from __future__ import annotations

import hashlib
import os
from importlib.metadata import version
from pathlib import Path

from . import PROTOCOL_VERSION

DRIVER_DIR = Path(__file__).resolve().parent
ORACLE_HOME = Path(os.environ.get("ORACLE_HOME", DRIVER_DIR.parent))


def anki_version() -> str:
    """The pinned distribution version (26.9.3), not the tag spelling (26.09.3)."""
    return version("anki")


def lock_sha256() -> str:
    return hashlib.sha256((ORACLE_HOME / "requirements.lock").read_bytes()).hexdigest()


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


def hello(tz: str) -> dict:
    return {
        "protocol": PROTOCOL_VERSION,
        "anki_version": anki_version(),
        "lock_sha256": lock_sha256(),
        "driver_sha256": driver_sha256(),
        "tz": tz,
    }
