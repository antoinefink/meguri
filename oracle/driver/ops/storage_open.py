# SPDX-License-Identifier: AGPL-3.0-or-later
"""Opening fixture collections (specs/conformance/storage-ops.md P1).

open: {fixture}   replace the session's collection with a copy of
    <ORACLE_FIXTURES>/<fixture> and open it with Anki's collection open, which
    upgrades schemas 11 to 17. -> {} or the rejection file_too_old (col.ver
    below 11) or file_too_new (12, 13, above 18). After a rejection no
    collection is open; the session lets only another open follow.

The fixture is checked before anything changes: a worker without
ORACLE_FIXTURES, or a fixture that is not a regular file below it, is the run
error fixture_error and leaves the session as it was. Then the open collection
closes without the downgrade, collection.anki2 and its -wal, -shm and -journal
files go, and the fixture's bytes are copied in. Other files of the session
directory stay (a handoff carries them).

Anki reports both refusals as a DB error whose message is the unlocalized
debug text of its error kind (FileTooOld, FileTooNew); any other failure stays
a run error. Collection() also runs the Python layer's scheduler loading, which
writes only when the v3 scheduler is on without creationOffset; the fixtures
never have that (schema-upgrade.md U16).
"""

from __future__ import annotations

import os
import re
import shutil
import stat

import anki.errors
from anki.collection import Collection

from ..errors import DriverError, Rejected, invalid
from ..registry import op

SIDECARS = ("-wal", "-shm", "-journal")
# fullmatch: `$` also matches before a final newline.
_SEGMENT = re.compile(r"[a-z0-9][a-z0-9._-]*")
_REFUSALS = (("kind: FileTooOld", "file_too_old"), ("kind: FileTooNew", "file_too_new"))


def fixture_rel(rel, suffixes: tuple[str, ...] = (".anki2",), what: str = "open fixture") -> str:
    """rel when it is a fixture path of lower-case segments ending in one of
    `suffixes` (storage-ops.md P1.1, package-ops.md E2.1), else invalid_scenario."""
    if not isinstance(rel, str) or not rel.endswith(suffixes) or not all(_SEGMENT.fullmatch(s) for s in rel.split("/")):
        raise invalid(f"{what} is a relative path of lower-case segments ending in {' or '.join(suffixes)}")
    return rel


def fixture_path(rel, suffixes: tuple[str, ...] = (".anki2",), what: str = "open fixture") -> str:
    """The fixture file below ORACLE_FIXTURES, or fixture_error."""
    fixture_rel(rel, suffixes, what)
    root = os.environ.get("ORACLE_FIXTURES")
    if not root:
        raise DriverError("fixture_error", detail="start the worker with ORACLE_FIXTURES=<conformance/fixtures>")
    path = os.path.realpath(os.path.join(root, rel))
    try:
        regular = path.startswith(os.path.realpath(root) + os.sep) and stat.S_ISREG(os.stat(path).st_mode)
    except OSError:
        regular = False
    if not regular:
        raise DriverError("fixture_error", detail=f"{rel} is not a regular file under ORACLE_FIXTURES")
    return path


@op("open", rejects=("file_too_old", "file_too_new"))
def open_fixture(session, args):
    if not isinstance(args, dict) or set(args) != {"fixture"}:
        raise invalid("open takes {fixture}")
    source = fixture_path(args["fixture"])
    session.close_collection()
    for suffix in ("",) + SIDECARS:
        try:
            os.remove(session.path + suffix)
        except FileNotFoundError:
            pass
    shutil.copyfile(source, session.path)
    try:
        session.col = Collection(session.path)
    except anki.errors.DBError as exc:
        message = str(exc)
        for marker, kind in _REFUSALS:
            if marker in message:
                raise Rejected(kind) from None
        raise
