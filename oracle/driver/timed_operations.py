# SPDX-License-Identifier: AGPL-3.0-or-later
"""The worker side of `tools/mg oracle time` (specs/conformance/timed-operations.md).

time: {dir, now, ops, repeat} -> the report's {now, tz, protocol, collection,
operations} (T7; the harness adds report_version, side, platform and oracle
from hello), or the run errors no_collection and open_failed (T8).

Only a timing worker serves it: one started with ORACLE_CLOCK_CACHE=1, whose
libfaketime read the clock file once, at `now` (T2.3, timecontrol.py). One
backend serves the whole run, as the desktop client keeps one per profile
(T3.1); every run opens its own copy of the image in it. Automatic garbage
collection is off for the run and each run's teardown collects (T3.3).

The interval is time.perf_counter_ns() (real: DONT_FAKE_MONOTONIC) around an
operation's `timed` method, whose body is only the calls T6 times. Nothing in
the report names a deck, tag, field or path: inputs are chosen from ids and
counts (T5), checks are counts, errors carry an exception's class name only.
The import log carries field text; only its length is read.

The sync stub (T5.5) is a stdlib HTTP server in this process: backend calls
release the GIL, so its thread serves while a full sync waits on it. It waits
only on sockets (no timed lock, event or queue wait: under libfaketime on
macOS those deadlines come from the frozen wall clock, oracle/AGENTS.md).
"""

from __future__ import annotations

import gc
import json
import math
import os
import shutil
import socket
import socketserver
import sqlite3
import tempfile
import threading
import time
import zipfile
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from typing import Any

from anki._backend import RustBackend
from anki.collection import (
    Collection,
    DeckIdLimit,
    ExportAnkiPackageOptions,
    ImportAnkiPackageOptions,
    ImportAnkiPackageRequest,
)
from anki.decks import UpdateDeckConfigs, UpdateDeckConfigsMode
from anki.import_export_pb2 import ImportAnkiPackageUpdateCondition
from anki.scheduler_pb2 import CardAnswer
from anki.sync import SyncAuth

from .clock import Clock
from .errors import DriverError, log_exception
from .ops.maintenance import database_problems, problem_counts
from .timed_catalogue import OPERATIONS, REPEAT_MAX, REPEAT_MIN, WARMUP, in_catalogue_order

REQUEST_KEYS = {"cmd", "id", "dir", "now", "ops", "repeat"}
COUNTED_TABLES = (
    ("notes", "notes"), ("cards", "cards"), ("revlog", "revlog"), ("notetypes", "notetypes"),
    ("decks", "decks"), ("presets", "deck_config"), ("tags", "tags"), ("graves", "graves"),
)
DEFAULT_DECK_ID = 1
ANSWER_TIME_MS = 10_000
GRAPH_DAYS = 365
IF_NEWER = ImportAnkiPackageUpdateCondition.IMPORT_ANKI_PACKAGE_UPDATE_CONDITION_IF_NEWER
PENDING_TABLES = ("notes", "cards", "revlog", "decks", "deck_config", "notetypes", "tags")
HOST_KEY = "timing"


# --- request and run --------------------------------------------------------------


def _is_int(v: Any) -> bool:
    return isinstance(v, int) and not isinstance(v, bool)


def check_request(req: dict) -> tuple[str, int, list[str], int]:
    ops, repeat = req.get("ops"), req.get("repeat")
    if (
        set(req) - REQUEST_KEYS
        or not isinstance(req.get("dir"), str)
        or not req["dir"]
        or not _is_int(req.get("now"))
        or req["now"] < 0
        or not isinstance(ops, list)
        or not ops
        or not all(isinstance(n, str) and n in OPERATIONS for n in ops)
        or not _is_int(repeat)
        or not REPEAT_MIN <= repeat <= REPEAT_MAX
    ):
        raise DriverError(
            "protocol_error",
            detail=f"time is {{dir, now, ops: [T6 names], repeat: {REPEAT_MIN}..{REPEAT_MAX}}}",
        )
    return req["dir"], req["now"], in_catalogue_order(ops), repeat


def run(req: dict, tz: str, scratch: str, clock: Clock) -> dict:
    directory, now, names, repeat = check_request(req)
    if not clock.cached:
        raise DriverError("protocol_error", detail="time needs a timing worker (ORACLE_CLOCK_CACHE=1, T2.3)")
    clock.set(now)  # a cached clock refuses any other instant
    source = os.path.join(directory, "collection.anki2")
    if not os.path.isfile(source):
        raise DriverError("no_collection", detail="the directory holds no collection.anki2")
    ctx = Context(tempfile.mkdtemp(prefix="time-", dir=scratch), now)
    gc_was_enabled = gc.isenabled()
    gc.disable()
    try:
        make_image(source, ctx.image)
        collection = summary(ctx)
        operations = [measure(ctx, CATALOGUE[name], repeat) for name in names]
    finally:
        ctx.close()
        if gc_was_enabled:
            gc.enable()
    return {
        "now": now,
        "tz": tz,
        "protocol": {"warmup": WARMUP, "repeat": repeat},
        "collection": collection,
        "operations": operations,
    }


class Context:
    """One run of the command: its scratch, the image, the backend, and the
    inputs built once (the T5.4 package, the T5.5 stub)."""

    def __init__(self, work: str, now: int) -> None:
        self.work = work
        self.now = now
        self.image = os.path.join(work, "image", "collection.anki2")
        self.backend = RustBackend(langs=["en"])
        self._package: str | None = None
        self._stub: SyncStub | None = None
        # The sync client must reach the loopback stub directly (T5.5).
        self._proxy_env = {k: os.environ.get(k) for k in ("NO_PROXY", "no_proxy")}
        os.environ["NO_PROXY"] = os.environ["no_proxy"] = "127.0.0.1"

    def package(self) -> str:
        if self._package is None:
            self._package = build_package(self.image, os.path.join(self.work, "import.apkg"))
        return self._package

    def stub(self) -> "SyncStub":
        if self._stub is None:
            with open(self.image, "rb") as f:
                self._stub = SyncStub(f.read())
        return self._stub

    def close(self) -> None:
        try:
            if self._stub is not None:
                self._stub.close()
        finally:
            for key, value in self._proxy_env.items():
                if value is None:
                    os.environ.pop(key, None)
                else:
                    os.environ[key] = value
            shutil.rmtree(self.work, ignore_errors=True)


class RunDir:
    """One run (T3.2): its directory, the copy of the image, the open collection."""

    def __init__(self, ctx: Context) -> None:
        self.dir = tempfile.mkdtemp(prefix="run-", dir=ctx.work)
        self.path = os.path.join(self.dir, "collection.anki2")
        shutil.copyfile(ctx.image, self.path)
        self.col: Collection | None = None


def make_image(source: str, image: str) -> None:
    """T2.1: the source (and its -wal) copied, the WAL folded in with plain
    SQLite. A file SQLite cannot read is not a collection (T8 open_failed)."""
    os.makedirs(os.path.dirname(image))
    shutil.copyfile(source, image)
    if os.path.isfile(source + "-wal"):
        shutil.copyfile(source + "-wal", image + "-wal")
    try:
        con = sqlite3.connect(image, isolation_level=None)
        try:
            con.execute("pragma wal_checkpoint(TRUNCATE)").fetchall()
        finally:
            con.close()
    except sqlite3.DatabaseError as exc:
        raise DriverError("open_failed", exception=type(exc).__name__) from None
    wal = image + "-wal"
    if os.path.isfile(wal) and os.path.getsize(wal) == 0:
        os.remove(wal)


def _rows(col: Collection, table: str) -> int:
    return col.db.scalar(f"select count() from {table}")


def summary(ctx: Context) -> dict[str, int]:
    """T2.2: row counts of a copy of the image, opened as T6.1 opens it."""
    r = RunDir(ctx)
    try:
        try:
            r.col = Collection(r.path, backend=ctx.backend)
        except Exception as exc:  # noqa: BLE001 - whatever Anki raises, it could not open the file
            log_exception(exc, "open_failed")
            raise DriverError("open_failed", exception=type(exc).__name__) from None
        return {name: _rows(r.col, table) for name, table in COUNTED_TABLES}
    finally:
        teardown(ctx, r)


# --- measurement --------------------------------------------------------------------


class NoInput(Exception):
    """T7.2 no_input: the collection lacks what the operation needs."""


class Failed(Exception):
    def __init__(self, kind: str, stage: str, exception: str | None = None) -> None:
        super().__init__(kind)
        self.error = {"kind": kind, "stage": stage, **({"exception": exception} if exception else {})}


def sample_ms(ns: int) -> float:
    """T4.2: milliseconds to the nearest microsecond, halves up."""
    return ((ns + 500) // 1000) / 1000


def statistic(samples: list[float]) -> dict[str, float]:
    """T4.3: the lower median (the sample at ceil(n/2) in ascending order) and the minimum."""
    ordered = sorted(samples)
    return {"median_ms": ordered[(len(ordered) + 1) // 2 - 1], "min_ms": ordered[0]}


def measure(ctx: Context, op: "Operation", repeat: int) -> dict:
    """T3.2, T3.6: one warm-up and `repeat` timed runs; the first failing run
    ends the operation."""
    samples: list[float] = []
    first: dict | None = None
    try:
        for index in range(WARMUP + repeat):
            ns, check = run_once(ctx, op)
            if first is None:
                first = check
            elif check != first:
                raise Failed("unstable", "check")
            if index >= WARMUP:
                samples.append(sample_ms(ns))
    except Failed as failure:
        return {"name": op.name, "status": "error", "error": failure.error}
    return {"name": op.name, "status": "ok", "samples_ms": samples, "statistic": statistic(samples), "check": first}


def _failed(stage: str, exc: BaseException) -> Failed:
    log_exception(exc, f"time: failed at {stage}")
    return Failed("failed", stage, type(exc).__name__)


def run_once(ctx: Context, op: "Operation") -> tuple[int, dict]:
    """Copy, setup, the timed interval, check, teardown (T3.2)."""
    r = None
    try:
        try:
            r = RunDir(ctx)
            state = op.setup(ctx, r)
        except NoInput:
            raise Failed("no_input", "setup") from None
        except Exception as exc:  # noqa: BLE001 - reported as the operation's error
            raise _failed("setup", exc) from None
        try:
            started = time.perf_counter_ns()
            result = op.timed(ctx, r, state)
            ns = time.perf_counter_ns() - started
        except Exception as exc:  # noqa: BLE001
            raise _failed("timed", exc) from None
        try:
            check = op.check(ctx, r, state, result)
        except Exception as exc:  # noqa: BLE001
            raise _failed("check", exc) from None
    except Failed:
        if r is not None:
            teardown(ctx, r, quiet=True)
        raise
    try:
        teardown(ctx, r)
    except Exception as exc:  # noqa: BLE001 - closing is after the check
        raise _failed("check", exc) from None
    return ns, check


def teardown(ctx: Context, r: RunDir, quiet: bool = False) -> None:
    """Close the run's collection, delete its directory, collect garbage. A
    run that failed half-way may leave the backend holding a collection
    without a Python object; it is closed too, so the next run can open."""
    try:
        if r.col is not None and r.col.db is not None:
            col, r.col = r.col, None
            try:
                col.close(downgrade=False)
            except Exception:
                if not quiet:
                    raise
        else:
            try:
                ctx.backend.close_collection(downgrade_to_schema11=False)
            except Exception:  # noqa: BLE001 - nothing was open: the normal case
                pass
    finally:
        shutil.rmtree(r.dir, ignore_errors=True)
        gc.collect()


# --- shared inputs (T5) -------------------------------------------------------------


def open_copy(ctx: Context, r: RunDir) -> Collection:
    r.col = Collection(r.path, backend=ctx.backend)
    return r.col


def study_deck(tree) -> int:
    """T5.1: the top-level row with the largest new + learning + review,
    ties to the smallest deck id."""
    return min((-(n.new_count + n.learn_count + n.review_count), n.deck_id) for n in tree.children)[1]


def standard_setup(ctx: Context, r: RunDir) -> int:
    """T5.1: open, the deck list at now, the study deck selected as clicking it does."""
    col = open_copy(ctx, r)
    did = study_deck(col.sched.deck_due_tree())
    col.decks.set_current(did)
    return did


def build_package(image: str, path: str) -> str:
    """T5.4: a version-1 package: collection.anki2 (the image's bytes) and
    media ({}), both stored, no meta."""
    with zipfile.ZipFile(path, "w", compression=zipfile.ZIP_STORED) as archive:
        archive.write(image, "collection.anki2")
        archive.writestr("media", b"{}")
    return path


ZSTD_MAGIC = bytes.fromhex("28b52ffd")
ZSTD_FRAME_HEADER = bytes.fromhex("0038")  # no content size, no checksum; a 128 KiB window
ZSTD_BLOCK = 131072


def raw_zstd_frame(content: bytes) -> bytes:
    """T5.5.1: the content in raw blocks of 128 KiB, the last one flagged."""
    pieces = [content[i:i + ZSTD_BLOCK] for i in range(0, len(content), ZSTD_BLOCK)] or [b""]
    out = [ZSTD_MAGIC, ZSTD_FRAME_HEADER]
    for index, piece in enumerate(pieces):
        last = 1 if index == len(pieces) - 1 else 0
        out.append((last + 8 * len(piece)).to_bytes(3, "little"))
        out.append(piece)
    return b"".join(out)


class _LoopbackServer(ThreadingHTTPServer):
    block_on_close = False  # idle keep-alive connections are shut in SyncStub.close

    def server_bind(self) -> None:
        # HTTPServer.server_bind looks the host's name up; a loopback stub needs none.
        socketserver.TCPServer.server_bind(self)
        self.server_name, self.server_port = self.server_address[:2]


class SyncStub:
    """T5.5: a loopback HTTP/1.1 server standing in for a sync server.
    POST /sync/upload answers OK, POST /sync/download the image; each after
    reading the request body to its end. Anything else is 404."""

    def __init__(self, download: bytes) -> None:
        answers = {
            "/sync/upload": (raw_zstd_frame(b"OK"), 2),
            "/sync/download": (raw_zstd_frame(download), len(download)),
        }
        connections: set[socket.socket] = set()
        guard = threading.Lock()
        self._connections, self._guard = connections, guard

        class Handler(BaseHTTPRequestHandler):
            protocol_version = "HTTP/1.1"
            # Headers and body go out as two writes: without TCP_NODELAY the
            # body would wait for the client's delayed ACK.
            disable_nagle_algorithm = True

            def setup(self) -> None:
                super().setup()
                with guard:
                    connections.add(self.connection)

            def finish(self) -> None:
                try:
                    super().finish()
                finally:
                    with guard:
                        connections.discard(self.connection)

            def log_message(self, format: str, *args: Any) -> None:  # noqa: A002 - the base class's name
                pass

            def _answer(self) -> None:
                discard_body(self)
                found = answers.get(self.path) if self.command == "POST" else None
                if found is None:
                    self.send_response(404)
                    self.send_header("Content-Length", "0")
                    self.end_headers()
                    return
                body, size = found
                self.send_response(200)
                self.send_header("anki-original-size", str(size))
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)

            do_POST = do_GET = do_HEAD = do_PUT = do_DELETE = do_PATCH = do_OPTIONS = _answer

        self.server = _LoopbackServer(("127.0.0.1", 0), Handler)
        self.port = self.server.server_address[1]
        self.endpoint = f"http://127.0.0.1:{self.port}/"
        self._thread = threading.Thread(target=self.server.serve_forever, name="sync-stub", daemon=True)
        self._thread.start()

    def close(self) -> None:
        self.server.shutdown()
        self.server.server_close()
        with self._guard:
            open_connections = list(self._connections)
        for connection in open_connections:
            try:
                connection.shutdown(socket.SHUT_RDWR)
            except OSError:
                pass


def discard_body(handler: BaseHTTPRequestHandler) -> None:
    """Read a request body to its end, chunked or with a length."""
    rfile = handler.rfile
    if "chunked" in handler.headers.get("Transfer-Encoding", "").lower():
        while True:
            line = rfile.readline(65537)
            if not line:
                raise ConnectionError("the request ended inside its chunks")
            size = int(line.split(b";", 1)[0].strip(), 16)
            if size == 0:
                while rfile.readline(65537) not in (b"\r\n", b"\n", b""):
                    pass  # trailers
                return
            _skip(rfile, size)
            rfile.readline(65537)  # the CRLF after the chunk
    else:
        _skip(rfile, int(handler.headers.get("Content-Length") or 0))


def _skip(rfile, count: int) -> None:
    while count > 0:
        data = rfile.read(min(count, 1 << 16))
        if not data:
            raise ConnectionError("the request body ended early")
        count -= len(data)


# --- card data (model/card-data-json C3) --------------------------------------------

_CARD_DATA_KEYS = ("pos", "s", "d", "dr", "decay", "lrt", "cd")


class _Pairs(list):
    """A JSON object as its member pairs, repeats kept."""


class _Overflow:
    """A number beyond the 64-bit float range (model/json-numbers N10)."""


_OVERFLOW = _Overflow()


def _parse_float(text: str) -> Any:
    value = float(text)
    return _OVERFLOW if math.isinf(value) else value


def _parse_int(text: str) -> Any:
    value = int(text)
    try:
        float(value)
    except OverflowError:
        return _OVERFLOW
    return value


def _not_json(name: str) -> Any:
    raise ValueError(f"{name} is not JSON")


def _overflows(value: Any) -> bool:
    if value is _OVERFLOW:
        return True
    if isinstance(value, _Pairs):
        return any(_overflows(v) for _, v in value)
    if isinstance(value, list):
        return any(_overflows(v) for v in value)
    return False


def _number(value: Any) -> bool:
    return isinstance(value, (int, float)) and not isinstance(value, bool)


def has_memory_state(data: Any) -> bool:
    """C3 rules 1-4 and 6: whether a card's data reads with both s and d."""
    if not isinstance(data, str):
        return False  # C1: another storage class reads as empty text
    try:
        top = json.loads(
            data, object_pairs_hook=_Pairs, parse_float=_parse_float, parse_int=_parse_int, parse_constant=_not_json
        )
    except (ValueError, RecursionError):
        return False
    if isinstance(top, _Pairs):
        known = [(k, v) for k, v in top if k in _CARD_DATA_KEYS]
        if len({k for k, _ in known}) != len(known) or any(_overflows(v) for _, v in known):
            return False
        values = dict(known)
        s, d = values.get("s"), values.get("d")
    elif isinstance(top, list):
        if len(top) > 7 or _overflows(top):
            return False
        s, d = (top[1] if len(top) > 1 else None), (top[2] if len(top) > 2 else None)
    else:
        return False
    return _number(s) and _number(d)


# --- the catalogue (T6) -------------------------------------------------------------


class Operation:
    name = ""

    def setup(self, ctx: Context, r: RunDir) -> Any:
        return standard_setup(ctx, r)

    def timed(self, ctx: Context, r: RunDir, state: Any) -> Any:
        raise NotImplementedError

    def check(self, ctx: Context, r: RunDir, state: Any, result: Any) -> dict:
        raise NotImplementedError


def _queue_counts(queued) -> dict[str, int]:
    return {"new": queued.new_count, "learning": queued.learning_count, "review": queued.review_count}


class CollectionOpen(Operation):
    """T6.1."""

    name = "collection.open"

    def setup(self, ctx, r):
        return None

    def timed(self, ctx, r, state):
        r.col = Collection(r.path, backend=ctx.backend)

    def check(self, ctx, r, state, result):
        return {name: _rows(r.col, name) for name in ("cards", "notes", "revlog")}


class DeckCounts(Operation):
    """T6.2."""

    name = "decks.counts"

    def setup(self, ctx, r):
        open_copy(ctx, r)

    def timed(self, ctx, r, state):
        return r.col.sched.deck_due_tree()

    def check(self, ctx, r, state, tree):
        decks, pending = 0, list(tree.children)
        while pending:
            node = pending.pop()
            decks += 1
            pending.extend(node.children)
        return {"decks": decks, "new": tree.new_count, "learning": tree.learn_count, "review": tree.review_count}


class QueueBuild(Operation):
    """T6.3."""

    name = "queue.build"

    def timed(self, ctx, r, state):
        return r.col.sched.get_queued_cards()

    def check(self, ctx, r, state, queued):
        return _queue_counts(queued)


class ReviewAnswer(Operation):
    """T6.4, T5.2."""

    name = "review.answer"

    def setup(self, ctx, r):
        standard_setup(ctx, r)
        queued = r.col.sched.get_queued_cards()
        if not queued.cards:
            raise NoInput()
        top = queued.cards[0]
        return CardAnswer(
            card_id=top.card.id,
            current_state=top.states.current,
            new_state=top.states.good,
            rating=CardAnswer.GOOD,
            answered_at_millis=ctx.now * 1000,
            milliseconds_taken=ANSWER_TIME_MS,
        )

    def timed(self, ctx, r, answer):
        r.col.sched.answer_card(answer)
        return r.col.sched.get_queued_cards()

    def check(self, ctx, r, state, queued):
        return _queue_counts(queued)


class Search(Operation):
    """T6.5."""

    def __init__(self, name: str, query: str) -> None:
        self.name, self.query = name, query

    def timed(self, ctx, r, state):
        return r.col.find_cards(self.query, order=False)

    def check(self, ctx, r, state, ids):
        return {"cards": len(ids)}


class BrowserSort(Operation):
    """T6.6: the browser's card search, every card, one column ascending."""

    def __init__(self, name: str, column: str) -> None:
        self.name, self.column = name, column

    def setup(self, ctx, r):
        standard_setup(ctx, r)
        column = r.col.get_browser_column(self.column)
        if column is None:
            raise LookupError(f"no browser column {self.column}")
        return column

    def timed(self, ctx, r, column):
        return r.col.find_cards("deck:*", order=column, reverse=False)

    def check(self, ctx, r, state, ids):
        return {"cards": len(ids)}


class StatsGraphs(Operation):
    """T6.7: every graph, deck:*, one year."""

    name = "stats.graphs"

    def timed(self, ctx, r, state):
        return r.col._backend.graphs(search="deck:*", days=GRAPH_DAYS)

    def check(self, ctx, r, state, graphs):
        cards = graphs.card_counts.including_inactive
        reviews = graphs.reviews.count.values()
        return {
            "cards": cards.newCards + cards.learn + cards.relearn + cards.young + cards.mature + cards.suspended
            + cards.buried,
            "reviews": sum(day.learn + day.relearn + day.young + day.mature + day.filtered for day in reviews),
        }


class CheckDatabase(Operation):
    """T6.8: the K5 object, or the corrupt result."""

    name = "maintenance.check_db"

    def timed(self, ctx, r, state):
        return database_problems(r.col)

    def check(self, ctx, r, state, problems):
        return {"error": "corrupt"} if problems is None else problem_counts(r.col.tr, problems)


def _entries(path: str) -> list[str]:
    with zipfile.ZipFile(path) as archive:
        return sorted(archive.namelist())


class ApkgExport(Operation):
    """T6.9: the study deck with its subdecks, current format, scheduling and
    presets, no media."""

    name = "package.apkg_export"
    options = ExportAnkiPackageOptions(with_scheduling=True, with_deck_configs=True, with_media=False, legacy=False)

    def setup(self, ctx, r):
        return DeckIdLimit(standard_setup(ctx, r)), os.path.join(r.dir, "export.apkg")

    def timed(self, ctx, r, state):
        limit, out = state
        return r.col.export_anki_package(out_path=out, options=self.options, limit=limit)

    def check(self, ctx, r, state, notes):
        return {"entries": _entries(state[1]), "notes": notes}


class ColpkgExport(Operation):
    """T6.10: the collection closed and exported; reopened after the interval."""

    name = "package.colpkg_export"

    def setup(self, ctx, r):
        standard_setup(ctx, r)
        return os.path.join(r.dir, "export.colpkg")

    def timed(self, ctx, r, out):
        r.col.export_collection_package(out, include_media=False, legacy=False)

    def check(self, ctx, r, out, result):
        r.col.reopen()
        return {"entries": _entries(out)}


class ApkgImport(Operation):
    """T6.11: the T5.4 package into a new collection (T5.3)."""

    name = "package.apkg_import"

    def setup(self, ctx, r):
        package = ctx.package()
        r.col = Collection(os.path.join(r.dir, "new.anki2"), backend=ctx.backend)
        return ImportAnkiPackageRequest(
            package_path=package,
            options=ImportAnkiPackageOptions(
                merge_notetypes=False,
                update_notes=IF_NEWER,
                update_notetypes=IF_NEWER,
                with_scheduling=True,
                with_deck_configs=True,
            ),
        )

    def timed(self, ctx, r, request):
        return r.col.import_anki_package(request)

    def check(self, ctx, r, state, response):
        # The log's notes carry field text: only their number is read.
        return {"notes_added": len(response.log.new), "cards": _rows(r.col, "cards"), "revlog": _rows(r.col, "revlog")}


class FullSync(Operation):
    """T6.12, T6.13: a one-way sync against the stub, reopened after it."""

    def __init__(self, name: str, upload: bool) -> None:
        self.name, self.upload = name, upload

    def setup(self, ctx, r):
        standard_setup(ctx, r)
        return SyncAuth(hkey=HOST_KEY, endpoint=ctx.stub().endpoint)

    def timed(self, ctx, r, auth):
        r.col.close_for_full_sync()
        try:
            r.col.full_upload_or_download(auth=auth, server_usn=None, upload=self.upload)
        finally:
            r.col.reopen(after_full_sync=True)

    def check(self, ctx, r, state, result):
        if not self.upload:
            return {"cards": _rows(r.col, "cards"), "revlog": _rows(r.col, "revlog")}
        pending = sum(r.col.db.scalar(f"select count() from {t} where usn = -1") for t in PENDING_TABLES)
        return {"cards": _rows(r.col, "cards"), "graves": _rows(r.col, "graves"), "pending": pending}


def deck_options_save(col: Collection, fsrs: bool):
    """T6.14.1: the default deck's options saved with only the FSRS switch set:
    every preset as stored, the default deck's last, its limits as stored."""
    current = col.decks.get_deck_configs_for_update(DEFAULT_DECK_ID)
    preset = current.current_deck.config_id
    configs = [c.config for c in current.all_config if c.config.id != preset]
    configs += [c.config for c in current.all_config if c.config.id == preset]
    if configs[-1].id != preset:
        raise LookupError("the default deck's preset is not stored")
    return UpdateDeckConfigs(
        target_deck_id=DEFAULT_DECK_ID,
        configs=configs,
        removed_config_ids=[],
        mode=UpdateDeckConfigsMode.UPDATE_DECK_CONFIGS_MODE_NORMAL,
        card_state_customizer=current.card_state_customizer,
        limits=current.current_deck.limits,
        new_cards_ignore_review_limit=current.new_cards_ignore_review_limit,
        fsrs=fsrs,
        apply_all_parent_limits=current.apply_all_parent_limits,
        fsrs_reschedule=False,
        fsrs_health_check=current.fsrs_health_check,
    )


class FsrsRecompute(Operation):
    """T6.14: FSRS switched off in setup, then on in the interval."""

    name = "fsrs.recompute_memory_states"

    def setup(self, ctx, r):
        standard_setup(ctx, r)
        r.col.decks.update_deck_configs(deck_options_save(r.col, fsrs=False))
        return deck_options_save(r.col, fsrs=True)

    def timed(self, ctx, r, save):
        r.col.decks.update_deck_configs(save)

    def check(self, ctx, r, state, result):
        return {"memory_states": sum(1 for data in r.col.db.list("select data from cards") if has_memory_state(data))}


SEARCHES = (
    ("search.deck", "deck:current"),
    ("search.state", "is:due"),
    ("search.tag", "tag:*::*"),
    ("search.text", "kamo"),
    ("search.regex", "re:k[aeiou]m"),
    ("search.prop", "prop:ivl>=21"),
    ("search.rated", "rated:7"),
    ("search.combined", "deck:current (is:new OR is:due) -is:suspended"),
)
SORTS = (("browser.sort_field", "noteFld"), ("browser.sort_due", "cardDue"))

CATALOGUE: dict[str, Operation] = {
    op.name: op
    for op in (
        CollectionOpen(),
        DeckCounts(),
        QueueBuild(),
        ReviewAnswer(),
        *(Search(name, query) for name, query in SEARCHES),
        *(BrowserSort(name, column) for name, column in SORTS),
        StatsGraphs(),
        CheckDatabase(),
        ApkgExport(),
        ColpkgExport(),
        ApkgImport(),
        FullSync("sync.full_upload", upload=True),
        FullSync("sync.full_download", upload=False),
        FsrsRecompute(),
    )
}
assert tuple(CATALOGUE) == OPERATIONS, "the catalogue's classes follow T6's order"
