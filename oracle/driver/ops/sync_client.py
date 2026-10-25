# SPDX-License-Identifier: AGPL-3.0-or-later
"""Actors, sync, network faults, reopen, convergence and review sessions
(specs/conformance/sync-ops.md S2, S4-S7, S13).

actor: <name>               the declared actor that runs the next steps (global)
sync: {kind: login | normal | status | full_upload | full_download,
       credentials?: account | wrong_password (login only)}
                            on the current actor -> {result}; rejections
                            auth_failed, clock_incorrect, network,
                            sanity_check_failed and db_check_required
net_fault: {kind: reset, direction: to_client | to_server} | {kind: clear}
                            on the run's fault proxy (global)
reopen: {}                  close the current collection and open it again
converge: {}                a fresh client's full download against every
                            actor's raw dump -> {differences, dump} (global)
review_session: begin | end Meguri's review session on the current actor
                            (S13); nothing here, output none

Static checks: actor names are declared, sync values have S4.1's and S4.5's
shape, and each actor's first sync, in step order, is a login; per actor (the
one collection without actors), review_session begin and end alternate from
begin, and no reopen, full_upload or full_download runs while one is open.

A login with credentials wrong_password sends the account's user name and its
password followed by -wrong (S4.5).

status is the backend's sync status (specs/sync/meta-status.md M6) with the
same host key and endpoint as normal. It runs on the actor's open collection:
the answer remembered for 300 s lives in that collection's backend, which a
sync job also updates, so the op never opens the collection again.

Rejections come from the exception's class and kind, never its text (R1887):
Anki's NetworkError is `network`; a sync error of the auth kind (Anki's
mapping of HTTP 403, whatever the request) is `auth_failed`, and takes the
actor's host key (S4.2); the clock error reaches Python as a sync error of the
"other" kind, told apart only by its message, which is compared with the same
collection's own translation of the clock-error string and never written
anywhere. A failed sanity check and an HTTP 400 during a normal's round reach
Python as the same "other" sync error with the same message, recognized the
same way through the translation of the sanity-check string. The message
cannot tell them apart; the actor's scm does, which Anki sets to the current
instant after a failed sanity check and leaves after a 400 (S4.4): changed
is sanity_check_failed, unchanged db_check_required. With the clock frozen, a
scm that already holds the current instant cannot show the change, so that
case is invalid_scenario. Unlike the other rejections, sanity_check_failed
leaves a changed collection (its scm): S4.4 records what Anki's client
leaves. Every other failure is anki_error, a 400 of a status, full_upload or
full_download included. A full sync closes the collection in the backend and
reopens it whatever the result; no server usn is passed, so no media sync
follows.
"""

from __future__ import annotations

import os
import shutil
import tempfile
from typing import Any

from anki.collection import Collection
from anki.errors import NetworkError, SyncError, SyncErrorKind
from anki.sync import SyncAuth, SyncOutput, SyncStatus

from ..errors import DriverError, Rejected, invalid
from ..observe.db_dump import dump
from ..registry import op, scenario_check
from . import sync_server

KINDS = ("login", "normal", "status", "full_upload", "full_download")
CREDENTIALS = ("account", "wrong_password")
REVIEW_SESSION = ("begin", "end")
# What closes the collection, so an actor with an open review session never
# runs it (S13).
CLOSING_SYNCS = ("full_upload", "full_download")
WRONG_PASSWORD_SUFFIX = "-wrong"
OUTCOMES = {
    SyncOutput.NO_CHANGES: "no_changes",
    SyncOutput.FULL_SYNC: "full_sync",
    SyncOutput.FULL_UPLOAD: "full_upload",
    SyncOutput.FULL_DOWNLOAD: "full_download",
}
STATUSES = {
    SyncStatus.NO_CHANGES: "no_changes",
    SyncStatus.NORMAL_SYNC: "normal_sync",
    SyncStatus.FULL_SYNC: "full_sync",
}
# Columns that record a party's sync history, not its content (S7.3): those of
# the col row, and those of every config entry (config travels whole, and each
# round restamps all of it on both sides).
EXCLUDED_COLUMNS = {"col": ("mod", "usn", "ls"), "config": ("usn", "mtime_secs")}


def sync_args(args) -> tuple[str, str]:
    """A sync step's (kind, credentials), or invalid_scenario (S4.1, S4.5)."""
    if not isinstance(args, dict) or not set(args) <= {"kind", "credentials"} or args.get("kind") not in KINDS:
        raise invalid("sync takes {kind: login | normal | status | full_upload | full_download, credentials?}")
    if "credentials" in args and args["kind"] != "login":
        raise invalid("credentials go only with kind login")
    credentials = args.get("credentials", "account")
    if credentials not in CREDENTIALS:
        raise invalid("credentials is account or wrong_password")
    return args["kind"], credentials


def login_credentials(credentials: str) -> tuple[str, str]:
    """The user name and password a login sends (S4.5)."""
    password = sync_server.PASSWORD if credentials == "account" else sync_server.PASSWORD + WRONG_PASSWORD_SUFFIX
    return sync_server.USERNAME, password


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
            try:
                kind, _ = sync_args(args)
            except DriverError as exc:
                exc.fields["step"] = index
                raise
            if current not in logged_in and kind != "login":
                raise invalid(f"{current}'s first sync must be a login", step=index)
            logged_in.add(current)


@scenario_check
def check_review_sessions(scenario: dict) -> None:
    """S13, per actor in step order (the one collection when there are no
    actors; check_actor_steps has checked the names)."""
    actors = scenario.get("actors") or [None]
    current, in_session = actors[0], set()
    for index, step in enumerate(scenario.get("steps", [])):
        ((name, args),) = step.items()
        if name == "actor":
            current = args
        elif name == "review_session":
            if args not in REVIEW_SESSION:
                raise invalid("review_session takes begin or end", step=index)
            if (args == "begin") == (current in in_session):
                state = "already open" if args == "begin" else "not open"
                raise invalid(f"review_session {args}: the session is {state}", step=index)
            in_session ^= {current}
        elif current in in_session and (
            name == "reopen" or name == "sync" and isinstance(args, dict) and args.get("kind") in CLOSING_SYNCS
        ):
            raise invalid(f"{name} closes the collection while a review session is open", step=index)


@op("actor", scope="global", needs_actors=True)
def actor(session, args):
    session.switch_actor(args)


def _run(session) -> sync_server.SyncRun:
    return session.resources["sync"]


def _rejected_kind(col: Collection, exc: Exception) -> str | None:
    if isinstance(exc, NetworkError):
        return "network"
    if isinstance(exc, SyncError) and exc.kind == SyncErrorKind.AUTH:
        return "auth_failed"
    if isinstance(exc, SyncError) and exc.kind == SyncErrorKind.OTHER and str(exc) == col.tr.sync_clock_off():
        return "clock_incorrect"
    return None


def _scm(col: Collection) -> int:
    return col.db.scalar("select scm from col")


def _round_failed(col: Collection, exc: Exception) -> bool:
    """A normal's round ended by a failed sanity check or an HTTP 400: the
    same "other" sync error with the same translated message (S4.4)."""
    return (
        isinstance(exc, SyncError)
        and exc.kind == SyncErrorKind.OTHER
        and str(exc) == col.tr.sync_sanity_check_failed()
    )


def round_failure_kind(scm_before: int, scm_after: int, now_ms: int) -> str:
    """sanity_check_failed when the failed round moved scm, else
    db_check_required (S4.4); invalid_scenario when scm already held the
    frozen instant, so a sanity failure could not have moved it."""
    if scm_after != scm_before:
        return "sanity_check_failed"
    if scm_before == now_ms:
        raise invalid(
            "scm already holds the current instant, so sanity_check_failed and db_check_required"
            " cannot be told apart: advance the clock before this sync"
        )
    return "db_check_required"


def _full_sync(col: Collection, auth: SyncAuth, upload: bool) -> None:
    col.close_for_full_sync()
    try:
        col.full_upload_or_download(auth=auth, server_usn=None, upload=upload)
    finally:
        col.reopen(after_full_sync=True)


@op(
    "sync",
    rejects=("auth_failed", "clock_incorrect", "network", "sanity_check_failed", "db_check_required"),
    needs_actors=True,
)
def sync(session, args):
    kind, credentials = sync_args(args)
    col, current = session.col, session.current
    endpoint = _run(session).actor_endpoint(current.name, session.step_index)
    scm_before = _scm(col) if kind == "normal" else None
    try:
        if kind == "login":
            username, password = login_credentials(credentials)
            auth = col.sync_login(username=username, password=password, endpoint=endpoint)
            current.data["hkey"] = auth.hkey
            return {"result": "ok"}
        if "hkey" not in current.data:
            raise invalid(f"{current.name} holds no host key: no login of it succeeded, or auth_failed took the key")
        auth = SyncAuth(hkey=current.data["hkey"], endpoint=endpoint)
        if kind == "normal":
            out = col.sync_collection(auth, sync_media=False)
            if out.required not in OUTCOMES:
                raise RuntimeError("a collection sync asked for another normal sync")
            return {"result": OUTCOMES[out.required]}
        if kind == "status":
            status = col.sync_status(auth)
            if status.required not in STATUSES:
                raise RuntimeError("the sync status is none of the three the protocol names")
            return {"result": STATUSES[status.required]}
        _full_sync(col, auth, upload=kind == "full_upload")
        return {"result": "ok"}
    except (NetworkError, SyncError) as exc:
        rejected = _rejected_kind(col, exc)
        if rejected is None and kind == "normal" and _round_failed(col, exc):
            rejected = round_failure_kind(scm_before, _scm(col), session.clock.now_ms())
        if rejected is None:
            raise
        if rejected == "auth_failed":
            current.data.pop("hkey", None)  # whatever an earlier login gave (S4.2, S4.4)
        raise Rejected(rejected) from exc


@op("net_fault", scope="global", needs_actors=True)
def net_fault(session, args):
    _run(session).fault(args)


@op("reopen")
def reopen(session, args):
    if args != {}:
        raise invalid("reopen takes {}")
    session.reopen()


@op("review_session")
def review_session(session, args):
    # S13: the oracle plays a Meguri actor as Anki, which has no review session.
    if args not in REVIEW_SESSION:
        raise invalid("review_session takes begin or end")


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
