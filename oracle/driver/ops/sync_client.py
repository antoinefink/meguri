# SPDX-License-Identifier: AGPL-3.0-or-later
"""Actors, sync, network faults, reopen and convergence
(specs/conformance/sync-ops.md S2, S4-S7).

actor: <name>               the declared actor that runs the next steps (global)
sync: {kind: login | normal | full_upload | full_download}
                            on the current actor -> {result}; rejections
                            clock_incorrect and network
net_fault: {kind: reset, direction: to_client | to_server} | {kind: clear}
                            on the run's fault proxy (global)
reopen: {}                  close the current collection and open it again
converge: {}                a fresh client's full download against every
                            actor's raw dump -> {differences, dump} (global)

Static checks: actor names are declared and each actor's first sync, in step
order, is a login.

Rejections come from the exception's class, never its text (R1887): Anki's
NetworkError is `network`; the clock error reaches Python as a sync error of
the "other" kind, told apart only by its message, which is compared with the
same collection's own translation of the clock-error string and never
written anywhere. Every other failure is anki_error. A full sync closes the
collection in the backend and reopens it whatever the result; no server usn is
passed, so no media sync follows.
"""

from __future__ import annotations

import os
import shutil
import tempfile
from typing import Any

from anki.collection import Collection
from anki.errors import NetworkError, SyncError, SyncErrorKind
from anki.sync import SyncAuth, SyncOutput

from ..errors import Rejected, invalid
from ..observe.db_dump import dump
from ..registry import op, scenario_check
from . import sync_server

KINDS = ("login", "normal", "full_upload", "full_download")
OUTCOMES = {
    SyncOutput.NO_CHANGES: "no_changes",
    SyncOutput.FULL_SYNC: "full_sync",
    SyncOutput.FULL_UPLOAD: "full_upload",
    SyncOutput.FULL_DOWNLOAD: "full_download",
}
# Columns that record a party's sync history, not its content (S7.3): those of
# the col row, and those of every config entry (config travels whole, and each
# round restamps all of it on both sides).
EXCLUDED_COLUMNS = {"col": ("mod", "usn", "ls"), "config": ("usn", "mtime_secs")}


@scenario_check
def check_actor_steps(scenario: dict) -> None:
    actors = scenario.get("actors")
    if not actors:
        return
    current, logged_in = actors[0], set()
    for index, step in enumerate(scenario.get("steps", [])):
        ((name, args),) = step.items()
        if name == "actor":
            if args not in actors:
                raise invalid(f"actor {args!r} is not in actors", step=index)
            current = args
        elif name == "sync":
            kind = args.get("kind") if isinstance(args, dict) else None
            if current not in logged_in and kind != "login":
                raise invalid(f"{current}'s first sync must be a login", step=index)
            logged_in.add(current)


@op("actor", scope="global", needs_actors=True)
def actor(session, args):
    session.switch_actor(args)


def _run(session) -> sync_server.SyncRun:
    return session.resources["sync"]


def _rejected_kind(col: Collection, exc: Exception) -> str | None:
    if isinstance(exc, NetworkError):
        return "network"
    if isinstance(exc, SyncError) and exc.kind == SyncErrorKind.OTHER and str(exc) == col.tr.sync_clock_off():
        return "clock_incorrect"
    return None


def _full_sync(col: Collection, auth: SyncAuth, upload: bool) -> None:
    col.close_for_full_sync()
    try:
        col.full_upload_or_download(auth=auth, server_usn=None, upload=upload)
    finally:
        col.reopen(after_full_sync=True)


@op("sync", rejects=("clock_incorrect", "network"), needs_actors=True)
def sync(session, args):
    if not isinstance(args, dict) or set(args) != {"kind"} or args["kind"] not in KINDS:
        raise invalid("sync takes {kind: login | normal | full_upload | full_download}")
    kind, col, current = args["kind"], session.col, session.current
    endpoint = _run(session).actor_endpoint(current.name, session.step_index)
    try:
        if kind == "login":
            auth = col.sync_login(username=sync_server.USERNAME, password=sync_server.PASSWORD, endpoint=endpoint)
            current.data["hkey"] = auth.hkey
            return {"result": "ok"}
        if "hkey" not in current.data:
            raise invalid(f"{current.name} holds no host key: its login was rejected")
        auth = SyncAuth(hkey=current.data["hkey"], endpoint=endpoint)
        if kind == "normal":
            out = col.sync_collection(auth, sync_media=False)
            if out.required not in OUTCOMES:
                raise RuntimeError("a collection sync asked for another normal sync")
            return {"result": OUTCOMES[out.required]}
        _full_sync(col, auth, upload=kind == "full_upload")
        return {"result": "ok"}
    except (NetworkError, SyncError) as exc:
        rejected = _rejected_kind(col, exc)
        if rejected is None:
            raise
        raise Rejected(rejected) from exc


@op("net_fault", scope="global", needs_actors=True)
def net_fault(session, args):
    _run(session).fault(args)


@op("reopen")
def reopen(session, args):
    if args != {}:
        raise invalid("reopen takes {}")
    session.reopen()


# --- converge -------------------------------------------------------------------


def _exclude(d: dict) -> dict:
    for table, columns in EXCLUDED_COLUMNS.items():
        for row in (d.get(table) or {}).values():
            if isinstance(row, dict):
                for column in columns:
                    row.pop(column, None)
    return d


def _same(a: Any, b: Any) -> bool:
    """Equal as JSON values, number kind included (1 and 1.0 differ)."""
    if isinstance(a, dict) and isinstance(b, dict):
        return a.keys() == b.keys() and all(_same(a[k], b[k]) for k in a)
    if isinstance(a, list) and isinstance(b, list):
        return len(a) == len(b) and all(_same(x, y) for x, y in zip(a, b))
    return type(a) is type(b) and a == b


def _escape(segment: str) -> str:
    return segment.replace("~", "~0").replace("/", "~1")


def _utf16(segment: str) -> bytes:
    return segment.encode("utf-16-be", "surrogatepass")


def differences(actual: Any, reference: Any, path: tuple[str, ...] = ()) -> list[str]:
    """Pointers to where two dumps first disagree: a table, a row or a column
    on one side only, or a column's value (S7.4), in pointer order: segments
    in JCS member order (UTF-16 code units), a prefix before what extends it."""
    found: list[tuple[str, ...]] = []

    def walk(a: Any, b: Any, at: tuple[str, ...]) -> None:
        if len(at) < 3 and isinstance(a, dict) and isinstance(b, dict):
            for key in set(a) | set(b):
                if key not in a or key not in b:
                    found.append(at + (key,))
                else:
                    walk(a[key], b[key], at + (key,))
        elif not _same(a, b):
            found.append(at)

    walk(actual, reference, path)
    found.sort(key=lambda segs: [_utf16(s) for s in segs])
    return ["".join("/" + _escape(s) for s in segs) for segs in found]


@op("converge", scope="global", needs_actors=True, takes_dumps=True)
def converge(session, args):
    if args != {}:
        raise invalid("converge takes {}")
    endpoint = _run(session).converge_base
    workdir = tempfile.mkdtemp(prefix="converge-", dir=session.scratch)
    col = None
    try:
        col = Collection(os.path.join(workdir, "collection.anki2"))
        auth = col.sync_login(username=sync_server.USERNAME, password=sync_server.PASSWORD, endpoint=endpoint)
        _full_sync(col, SyncAuth(hkey=auth.hkey, endpoint=endpoint), upload=False)
        reference = _exclude(dump(col, raw=True))
        shown = _exclude(dump(col))
    finally:
        if col is not None:
            col.close(downgrade=False)
        shutil.rmtree(workdir, ignore_errors=True)
    dumps = session.caller_dumps or {}
    out = {}
    for name in session.actor_names:
        actor_state = session.actors[name]
        raw = dump(actor_state.col, raw=True) if actor_state.played else dumps[name]
        out[name] = differences(_exclude(raw), reference)
    return {"differences": out, "dump": shown}
