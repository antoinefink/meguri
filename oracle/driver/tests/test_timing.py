# SPDX-License-Identifier: AGPL-3.0-or-later
"""tools/mg oracle time (specs/conformance/timed-operations.md).

In this process: the stub's zstd frames (bytes from T5.5.1 by hand) and its
answers over a real socket, the card-data reading (model/card-data-json C3),
the measurement protocol with fake operations, and the harness's arguments.
Through the harness and timing workers (container only: they need
libfaketime): every operation on a small collection, an empty queue, the T8
errors, and what a timing worker refuses. Expected counts come from how the
test collections were built, never from the driver."""

from __future__ import annotations

import gc
import hashlib
import http.client
import json
import os
import shutil
import socket
import sqlite3
import tempfile
import time
import unittest
from datetime import datetime, timezone
from pathlib import Path

from anki.collection import Collection
from anki.scheduler_pb2 import CardAnswer

from driver import harness, timed_operations as timing
from driver.canon import golden_text
from driver.clock import Clock, write_epoch
from driver.harness import ORACLE_HOME, WorkerProcess
from driver.timed_catalogue import OPERATIONS

NOW = 1894701600  # 2030-01-15T10:00:00Z
SENTINEL = "Zebracorn"
ZEROS = dict.fromkeys(
    ["card_properties", "new_positions_too_high", "card_last_review_time", "cards_without_note", "decks_missing",
     "field_counts", "duplicate_card_ords", "cards_without_template", "revlog_properties", "notes_invalid_utf8",
     "future_ids", "notetypes_recovered"],
    0,
)
PACKAGE_ENTRIES = ["collection.anki2", "collection.anki21b", "media", "meta"]
OK_FRAME = bytes.fromhex("28B52FFD00381100004F4B")  # T5.5.1's example
COUNTED = (("notes", "notes"), ("cards", "cards"), ("revlog", "revlog"), ("notetypes", "notetypes"),
           ("decks", "decks"), ("presets", "deck_config"), ("tags", "tags"), ("graves", "graves"))


def iso(epoch: int) -> str:
    return datetime.fromtimestamp(epoch, timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def source_counts(directory: str) -> dict[str, int]:
    con = sqlite3.connect(f"file:{os.path.join(directory, 'collection.anki2')}?immutable=1", uri=True)
    con.create_collation("unicase", lambda a, b: (a.casefold() > b.casefold()) - (a.casefold() < b.casefold()))
    try:
        return {name: con.execute(f"select count() from {table}").fetchone()[0] for name, table in COUNTED}
    finally:
        con.close()


def files_digest(directory: str) -> str:
    h = hashlib.sha256()
    for path in sorted(Path(directory).rglob("*")):
        h.update(str(path.relative_to(directory)).encode())
        if path.is_file():
            h.update(path.read_bytes())
    return h.hexdigest()


# Notes of the small collection: (front, back, tags, in the subdeck). Text
# "kamo" is in notes 0 and 2; the regex k[aeiou]m matches notes 0, 1 and 2;
# notes 0 and 2 have a tag with a "::" level.
NOTES = (
    (f"kamo {SENTINEL}", "x", [f"{SENTINEL}::tag"], False),
    ("kemono", "y", ["plain"], True),
    ("hello", "kamo", [f"{SENTINEL}::tag"], True),
    ("zzz", "zzz", [], False),
    ("abc", "def", [], False),
    ("ghi", "jkl", [], True),
)
ANSWERED = 2  # the first cards of the queue, answered Good once each while building


def build_small(root: str) -> str:
    """One top-level deck and its subdeck, every name, tag and field holding
    SENTINEL somewhere; two cards answered on the real clock."""
    directory = os.path.join(root, "small")
    os.makedirs(directory)
    col = Collection(os.path.join(directory, "collection.anki2"))
    model = col.models.copy(col.models.by_name("Basic"), add=False)
    model["name"] = f"{SENTINEL} type"
    col.models.add_dict(model)
    model = col.models.by_name(f"{SENTINEL} type")
    top = col.decks.id(f"{SENTINEL} deck")
    sub = col.decks.id(f"{SENTINEL} deck::{SENTINEL} sub")
    for front, back, tags, in_sub in NOTES:
        note = col.new_note(model)
        note["Front"], note["Back"] = front, back
        note.tags = tags
        col.add_note(note, sub if in_sub else top)
    col.decks.set_current(top)
    for _ in range(ANSWERED):
        top_card = col.sched.get_queued_cards().cards[0]
        col.sched.answer_card(CardAnswer(
            card_id=top_card.card.id, current_state=top_card.states.current, new_state=top_card.states.good,
            rating=CardAnswer.GOOD, answered_at_millis=int(time.time() * 1000), milliseconds_taken=1000,
        ))
    col.close(downgrade=False)
    return directory


def build_empty(root: str, name: str = "empty") -> str:
    directory = os.path.join(root, name)
    os.makedirs(directory)
    Collection(os.path.join(directory, "collection.anki2")).close(downgrade=False)
    return directory


# --- in this process ------------------------------------------------------------------


class Frames(unittest.TestCase):
    """T5.5.1, from its text: magic, 00 38, raw blocks of 128 KiB."""

    def test_ok_is_the_spec_example(self):
        self.assertEqual(timing.raw_zstd_frame(b"OK"), OK_FRAME)

    def test_empty_content_is_one_empty_last_block(self):
        self.assertEqual(timing.raw_zstd_frame(b""), bytes.fromhex("28B52FFD0038010000"))

    def test_block_boundaries(self):
        exact = b"a" * 131072
        # last 1 + 8 * 131072 = 0x100001, little-endian.
        self.assertEqual(timing.raw_zstd_frame(exact), bytes.fromhex("28B52FFD0038010010") + exact)
        over = exact + b"b"
        # 0 + 8 * 131072 = 0x100000, then the last block of one byte: 1 + 8 = 9.
        self.assertEqual(
            timing.raw_zstd_frame(over), bytes.fromhex("28B52FFD0038000010") + exact + bytes.fromhex("090000") + b"b"
        )


class Stub(unittest.TestCase):
    """T5.5's answers, over a real loopback connection."""

    def setUp(self):
        self.content = bytes(range(256)) * 3
        self.stub = timing.SyncStub(self.content)
        self.addCleanup(self.stub.close)

    def test_upload_download_and_others_on_one_connection(self):
        conn = http.client.HTTPConnection("127.0.0.1", self.stub.port)
        self.addCleanup(conn.close)
        conn.request("POST", "/sync/upload", body=iter([b"abc", b"defgh" * 1000]), encode_chunked=True,
                     headers={"anki-sync": '{"v":11}', "Content-Type": "application/octet-stream"})
        resp = conn.getresponse()
        self.assertEqual((resp.status, resp.getheader("anki-original-size")), (200, "2"))
        self.assertEqual(resp.getheader("Content-Length"), str(len(OK_FRAME)))
        self.assertEqual(resp.read(), OK_FRAME)
        conn.request("POST", "/sync/download", body=b"z" * 700)
        resp = conn.getresponse()
        self.assertEqual((resp.status, resp.getheader("anki-original-size")), (200, str(len(self.content))))
        size = 1 + 8 * len(self.content)
        self.assertEqual(resp.read(), bytes.fromhex("28B52FFD0038") + size.to_bytes(3, "little") + self.content)
        for method, path in (("GET", "/sync/upload"), ("POST", "/sync/meta"), ("POST", "/sync/upload?x=1")):
            with self.subTest(method=method, path=path):
                conn.request(method, path, body=b"" if method == "POST" else None)
                resp = conn.getresponse()
                self.assertEqual((resp.status, resp.read()), (404, b""))

    def test_close_stops_the_server_and_its_open_connections(self):
        conn = http.client.HTTPConnection("127.0.0.1", self.stub.port)
        self.addCleanup(conn.close)
        conn.request("POST", "/sync/upload", body=b"")
        conn.getresponse().read()
        self.stub.close()
        conn.request("POST", "/sync/upload", body=b"")
        with self.assertRaises((http.client.HTTPException, ConnectionError)):
            conn.getresponse()
        with self.assertRaises(ConnectionRefusedError):
            socket.create_connection(("127.0.0.1", self.stub.port))


class CardData(unittest.TestCase):
    """model/card-data-json C3 rules 1-4 and 6, for s and d."""

    def test_memory_state_presence(self):
        huge = "1" + "0" * 400
        for data, present in (
            ('{"s":1.5,"d":5}', True), ('{ "d" : 2 , "s" : 3.0 }', True), ('{"s":1e39,"d":1}', True),
            ('{"s":1.5}', False), ('{"d":1}', False), ("", False), ("not json", False), ("5", False), ('"x"', False),
            ("[1,2,3]", True), ("[null,1,2]", True), ("[1,2]", False), ("[1,2,3,4,5,6,7,8]", False),
            ('{"s":1e400,"d":1}', False), (f'{{"s":{huge},"d":1}}', False), ('[1,2,3,1e400]', False),
            ('{"pos":5,"zz":1e400,"s":1,"d":1}', True), ('{"s":1,"d":1,"cd":[1e400]}', False),
            ('{"s":1,"s":2,"d":1}', False), ('{"s":1,"d":1,"zz":1,"zz":2}', True),
            ('{"s":"1.5","d":1}', False), ('{"s":true,"d":1}', False), ('{"s":null,"d":1}', False),
            ('{"s":NaN,"d":1}', False), ('{"s":Infinity,"d":1}', False),
            (b'{"s":1,"d":1}', False), (None, False), (7, False),
        ):
            with self.subTest(data=data):
                self.assertIs(timing.has_memory_state(data), present)


class FakeOp(timing.Operation):
    name = "fake"

    def __init__(self, check=lambda n: {"n": 1}, setup=None, timed=None):
        self.calls, self._check, self._setup, self._timed = 0, check, setup, timed
        self.seen: list = []

    def setup(self, ctx, r):
        self.calls += 1
        if self._setup:
            self._setup()

    def timed(self, ctx, r, state):
        self.seen.append((os.path.getsize(r.path), gc.isenabled()))
        with open(r.path, "ab") as f:  # every run must start from the image's bytes again
            f.write(b"x" * 10)
        if self._timed:
            self._timed()

    def check(self, ctx, r, state, result):
        return self._check(self.calls)


class Measurement(unittest.TestCase):
    """T3.2, T3.6, T4, T7.2 with fake operations."""

    def setUp(self):
        self.tmp = tempfile.mkdtemp(prefix="timing-test-")
        self.addCleanup(shutil.rmtree, self.tmp, True)
        source = build_empty(self.tmp, "source")
        self.ctx = timing.Context(os.path.join(self.tmp, "work"), NOW)
        os.makedirs(self.ctx.work)
        self.addCleanup(self.ctx.close)
        timing.make_image(os.path.join(source, "collection.anki2"), self.ctx.image)

    def test_samples_statistic_and_check_of_stable_runs(self):
        op = FakeOp()
        entry = timing.measure(self.ctx, op, 4)
        self.assertEqual(sorted(entry), ["check", "name", "samples_ms", "statistic", "status"])
        self.assertEqual((entry["status"], entry["check"], op.calls), ("ok", {"n": 1}, 5))
        self.assertEqual(len(entry["samples_ms"]), 4)
        ordered = sorted(entry["samples_ms"])
        self.assertEqual(entry["statistic"], {"median_ms": ordered[1], "min_ms": ordered[0]})
        image_size = os.path.getsize(self.ctx.image)
        self.assertEqual({size for size, _ in op.seen}, {image_size})
        self.assertEqual(os.listdir(self.ctx.work), ["image"])

    def test_a_check_that_changes_is_unstable(self):
        op = FakeOp(check=lambda n: {"n": n})
        entry = timing.measure(self.ctx, op, 5)
        self.assertEqual(entry, {"name": "fake", "status": "error", "error": {"kind": "unstable", "stage": "check"}})
        self.assertEqual(op.calls, 2)  # the warm-up and the first timed run

    def test_a_change_in_a_later_run_is_unstable_too(self):
        entry = timing.measure(self.ctx, FakeOp(check=lambda n: {"n": n >= 4}), 5)
        self.assertEqual(entry["error"], {"kind": "unstable", "stage": "check"})

    def test_failures_name_the_stage_and_the_class_only(self):
        def boom():
            raise ValueError(f"{SENTINEL} field text")

        for stage, op in (("setup", FakeOp(setup=boom)), ("timed", FakeOp(timed=boom)),
                          ("check", FakeOp(check=lambda n: boom()))):
            with self.subTest(stage=stage):
                entry = timing.measure(self.ctx, op, 3)
                self.assertEqual(entry["error"], {"kind": "failed", "stage": stage, "exception": "ValueError"})
                self.assertEqual(op.calls, 1)
                self.assertNotIn(SENTINEL, json.dumps(entry))
                self.assertEqual(os.listdir(self.ctx.work), ["image"])

    def test_no_input(self):
        def nothing():
            raise timing.NoInput()

        entry = timing.measure(self.ctx, FakeOp(setup=nothing), 3)
        self.assertEqual(entry["error"], {"kind": "no_input", "stage": "setup"})

    def test_milliseconds_and_the_lower_median(self):
        self.assertEqual(timing.sample_ms(41_207_400), 41.207)
        self.assertEqual(timing.sample_ms(1_500), 0.002)  # halves up
        self.assertEqual(timing.sample_ms(1_000_500), 1.001)  # where float rounding of 1.0005 gives 1.0
        self.assertEqual(timing.sample_ms(1_499), 0.001)
        self.assertEqual(timing.sample_ms(499), 0.0)
        self.assertEqual(timing.statistic([3.0, 1.0, 2.0, 4.0]), {"median_ms": 2.0, "min_ms": 1.0})
        self.assertEqual(timing.statistic([5.0, 9.0, 7.0]), {"median_ms": 7.0, "min_ms": 5.0})
        self.assertEqual(timing.statistic([2.5]), {"median_ms": 2.5, "min_ms": 2.5})


class RunInProcess(unittest.TestCase):
    """T3.3 and the request checks of run(), with collection.open replaced by
    a fake operation (no libfaketime here: a Clock marked cached stands in)."""

    def setUp(self):
        self.tmp = tempfile.mkdtemp(prefix="timing-run-")
        self.addCleanup(shutil.rmtree, self.tmp, True)
        self.source = build_empty(self.tmp, "source")
        self.clock_file = os.path.join(self.tmp, "clock")
        write_epoch(self.clock_file, NOW)

    def request(self, **fields):
        return {"cmd": "time", "dir": self.source, "now": NOW, "ops": ["collection.open"], "repeat": 2, **fields}

    def test_garbage_collection_is_off_during_runs_and_back_after(self):
        fake = FakeOp()
        fake.name = "collection.open"
        real = timing.CATALOGUE["collection.open"]
        timing.CATALOGUE["collection.open"] = fake
        self.addCleanup(timing.CATALOGUE.__setitem__, "collection.open", real)
        self.assertTrue(gc.isenabled())
        report = timing.run(self.request(), "UTC", self.tmp, Clock(self.clock_file, cached=True))
        self.assertEqual([enabled for _, enabled in fake.seen], [False] * 3)
        self.assertTrue(gc.isenabled())
        self.assertEqual(report["operations"][0]["status"], "ok")
        self.assertEqual(sorted(report), ["collection", "now", "operations", "protocol", "tz"])
        self.assertEqual(report["protocol"], {"warmup": 1, "repeat": 2})
        self.assertEqual([p for p in os.listdir(self.tmp) if p.startswith("time-")], [])

    def test_bad_requests_and_clocks(self):
        cached = Clock(self.clock_file, cached=True)
        for bad in ({"ops": []}, {"ops": ["nope"]}, {"ops": "collection.open"}, {"repeat": 0}, {"repeat": 101},
                    {"repeat": True}, {"now": -1}, {"dir": ""}, {"extra": 1}):
            with self.subTest(bad=bad), self.assertRaises(timing.DriverError) as raised:
                timing.run(self.request(**bad), "UTC", self.tmp, cached)
            self.assertEqual(raised.exception.kind, "protocol_error")
        with self.assertRaises(timing.DriverError) as raised:
            timing.run(self.request(), "UTC", self.tmp, Clock(self.clock_file))
        self.assertEqual(raised.exception.kind, "protocol_error")
        with self.assertRaises(timing.DriverError) as raised:
            timing.run(self.request(now=NOW + 1), "UTC", self.tmp, cached)
        self.assertEqual(raised.exception.kind, "protocol_error")
        with self.assertRaises(timing.DriverError) as raised:
            timing.run(self.request(dir=self.tmp), "UTC", self.tmp, cached)
        self.assertEqual(raised.exception.kind, "no_collection")


class HarnessArguments(unittest.TestCase):
    """T1: --op and --repeat."""

    def test_operations_in_catalogue_order(self):
        self.assertEqual(harness.time_operations(None), list(OPERATIONS))
        self.assertEqual(harness.time_operations(["search.tag", "collection.open", "search.tag"]),
                         ["collection.open", "search.tag"])
        with self.assertRaises(harness.CheckUsage):
            harness.time_operations(["collection.open", "collection.close"])

    def test_repeat(self):
        self.assertEqual(harness.time_repeat(None), 5)
        self.assertEqual((harness.time_repeat("1"), harness.time_repeat("100")), (1, 100))
        for text in ("0", "101", "-1", "1.0", "x", "", " 5"):
            with self.subTest(text=text), self.assertRaises(harness.CheckUsage):
                harness.time_repeat(text)


# --- through the harness and timing workers (container) ----------------------------


class Runs(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.mkdtemp(prefix="timing-harness-")
        self.addCleanup(shutil.rmtree, self.tmp, True)

    def run_time(self, directory: str, *options: str) -> tuple[int, Path]:
        out = Path(tempfile.mkdtemp(dir=self.tmp))
        rc = harness.main(["time", "--dir", directory, "--out", str(out), *options])
        return rc, out

    def report(self, out: Path) -> dict:
        self.assertFalse((out / "timing-error.json").exists())
        text = (out / "timing.json").read_text(encoding="utf-8")
        report = json.loads(text)
        self.assertEqual(text, golden_text(report))
        return report

    def test_every_operation_on_a_small_collection(self):
        directory = build_small(self.tmp)
        before = files_digest(directory)
        now = int(time.time()) + 1  # after every id the build made: nothing is in the future
        rc, out = self.run_time(directory, "--now", iso(now), "--tz", "UTC", "--repeat", "2")
        self.assertEqual(rc, 0)
        text = (out / "timing.json").read_text(encoding="utf-8")
        report = self.report(out)
        self.assertEqual(files_digest(directory), before)
        self.assertNotIn(SENTINEL.lower(), text.lower())
        self.assertNotIn(self.tmp, text)
        lock = hashlib.sha256((ORACLE_HOME / "requirements.lock").read_bytes()).hexdigest()
        self.assertEqual(sorted(report), ["collection", "now", "operations", "oracle", "platform", "protocol",
                                          "report_version", "side", "tz"])
        self.assertEqual((report["report_version"], report["side"], report["now"], report["tz"]), (1, "anki", now, "UTC"))
        self.assertTrue(report["platform"].startswith("linux-"))
        self.assertEqual(report["oracle"]["lock_sha256"], lock)
        self.assertEqual(report["oracle"]["anki_version"], "26.9.3")
        self.assertRegex(report["oracle"]["driver_sha256"], "^[0-9a-f]{64}$")
        self.assertEqual(report["protocol"], {"warmup": 1, "repeat": 2})
        counts = source_counts(directory)
        self.assertEqual(report["collection"], counts)
        cards, notes, revlog = counts["cards"], counts["notes"], counts["revlog"]
        self.assertEqual((cards, notes, revlog), (len(NOTES), len(NOTES), ANSWERED))
        entries = {e["name"]: e for e in report["operations"]}
        self.assertEqual([e["name"] for e in report["operations"]], list(OPERATIONS))
        for name, entry in entries.items():
            with self.subTest(op=name):
                self.assertEqual(entry["status"], "ok", entry)
                self.assertEqual(sorted(entry), ["check", "name", "samples_ms", "statistic", "status"])
                self.assertEqual(len(entry["samples_ms"]), 2)
                self.assertTrue(all(isinstance(s, float) and s >= 0 for s in entry["samples_ms"]))
                self.assertEqual(entry["statistic"], {"median_ms": min(entry["samples_ms"]),
                                                      "min_ms": min(entry["samples_ms"])})
        check = {name: e["check"] for name, e in entries.items()}
        self.assertEqual(check["collection.open"], {"cards": cards, "notes": notes, "revlog": revlog})
        # The default deck is empty and left out: the tree is the top deck and its subdeck.
        self.assertEqual(check["decks.counts"]["decks"], 2)
        self.assertEqual(check["queue.build"]["new"], check["decks.counts"]["new"])
        self.assertEqual(check["queue.build"]["new"], len(NOTES) - ANSWERED)
        self.assertEqual(check["search.deck"], {"cards": cards})
        self.assertEqual(check["search.tag"], {"cards": 2})
        self.assertEqual(check["search.text"], {"cards": 2})
        self.assertEqual(check["search.regex"], {"cards": 3})
        self.assertEqual(check["search.prop"], {"cards": 0})
        self.assertEqual(check["search.rated"], {"cards": ANSWERED})
        self.assertEqual(check["browser.sort_field"], {"cards": cards})
        self.assertEqual(check["browser.sort_due"], {"cards": cards})
        self.assertEqual(check["stats.graphs"], {"cards": cards, "reviews": revlog})
        self.assertEqual(check["maintenance.check_db"], ZEROS)
        self.assertEqual(check["package.apkg_export"], {"entries": PACKAGE_ENTRIES, "notes": notes})
        self.assertEqual(check["package.colpkg_export"], {"entries": PACKAGE_ENTRIES})
        self.assertEqual(check["package.apkg_import"], {"notes_added": notes, "cards": cards, "revlog": revlog})
        self.assertEqual(check["sync.full_upload"], {"cards": cards, "graves": 0, "pending": 0})
        self.assertEqual(check["sync.full_download"], {"cards": cards, "revlog": revlog})
        self.assertEqual(check["fsrs.recompute_memory_states"], {"memory_states": ANSWERED})

    def test_an_empty_collection_has_no_card_to_answer(self):
        directory = build_empty(self.tmp)
        rc, out = self.run_time(directory, "--now", iso(NOW), "--tz", "UTC", "--repeat", "1",
                                "--op", "review.answer", "--op", "queue.build")
        self.assertEqual(rc, 0)
        operations = self.report(out)["operations"]
        self.assertEqual([e["name"] for e in operations], ["queue.build", "review.answer"])
        self.assertEqual((operations[0]["status"], operations[0]["check"]),
                         ("ok", {"new": 0, "learning": 0, "review": 0}))
        self.assertEqual(operations[1], {"name": "review.answer", "status": "error",
                                         "error": {"kind": "no_input", "stage": "setup"}})

    def test_now_and_zone_from_the_manifest_and_a_wal_folded_in(self):
        directory = build_empty(self.tmp)
        Path(directory, "manifest.json").write_text(json.dumps({"now_ms": NOW * 1000 + 999, "utc_offset_secs": -7200}))
        path = os.path.join(directory, "collection.anki2")
        con = sqlite3.connect(path)
        con.execute("pragma journal_mode = wal")
        con.execute("pragma wal_autocheckpoint = 0")
        con.execute("insert into graves values (1, 0, -1)")
        con.commit()
        wal_dir = os.path.join(self.tmp, "wal")
        os.makedirs(wal_dir)
        for name in ("collection.anki2", "collection.anki2-wal", "manifest.json"):
            shutil.copyfile(os.path.join(directory, name), os.path.join(wal_dir, name))
        con.close()
        self.assertEqual(source_counts(wal_dir)["graves"], 0)  # the main file alone
        rc, out = self.run_time(wal_dir, "--op", "collection.open", "--repeat", "1")
        self.assertEqual(rc, 0)
        report = self.report(out)
        self.assertEqual((report["now"], report["tz"]), (NOW, "Etc/GMT+2"))
        self.assertEqual(report["collection"]["graves"], 1)
        self.assertEqual(sorted(os.listdir(wal_dir)), ["collection.anki2", "collection.anki2-wal", "manifest.json"])

    def test_errors_are_written_with_their_t8_kind(self):
        good = build_empty(self.tmp)
        nothing = tempfile.mkdtemp(dir=self.tmp)
        garbage = tempfile.mkdtemp(dir=self.tmp)
        Path(garbage, "collection.anki2").write_bytes(b"not a collection" * 512)
        newer = build_empty(self.tmp, "newer")
        con = sqlite3.connect(os.path.join(newer, "collection.anki2"))
        con.execute("update col set ver = 19")
        con.commit()
        con.close()
        clock = ("--tz", "UTC", "--now", iso(NOW))
        for directory, options, kind in (
            (nothing, clock, "no_collection"),
            (good, ("--tz", "UTC"), "usage"),
            (good, (*clock, "--op", "collection.close"), "usage"),
            (good, (*clock, "--repeat", "0"), "usage"),
            (good, (*clock, "--repeat", "101"), "usage"),
            (garbage, clock, "open_failed"),
            (newer, clock, "open_failed"),
        ):
            with self.subTest(kind=kind, options=options):
                rc, out = self.run_time(directory, *options)
                self.assertEqual(rc, 1)
                self.assertFalse((out / "timing.json").exists())
                error = json.loads((out / "timing-error.json").read_text(encoding="utf-8"))
                self.assertEqual(sorted(error), ["detail", "kind"])
                self.assertEqual(error["kind"], kind)


class TimingWorkers(unittest.TestCase):
    """T2.3: only a timing worker times, and its clock never moves."""

    def setUp(self):
        self.tmp = tempfile.mkdtemp(prefix="timing-worker-")
        self.addCleanup(shutil.rmtree, self.tmp, True)
        self.directory = build_empty(self.tmp)

    def time_request(self, worker, now=NOW):
        return worker.request({"cmd": "time", "dir": self.directory, "now": now, "ops": ["collection.open"],
                               "repeat": 1})

    def test_time_needs_a_timing_worker(self):
        with WorkerProcess("UTC") as worker:
            resp = self.time_request(worker)
        self.assertEqual(resp["error"]["kind"], "protocol_error", resp)

    def test_a_timing_worker_stays_at_its_instant(self):
        with WorkerProcess("UTC", extra_env={"ORACLE_CLOCK_CACHE": "1"}, clock=NOW) as worker:
            self.assertEqual(worker.hello["time_control"], "faketime")
            self.assertEqual(self.time_request(worker, NOW + 1)["error"]["kind"], "protocol_error")
            resp = self.time_request(worker)
            self.assertTrue(resp["ok"], resp)
            self.assertEqual(sorted(resp["timing"]), ["collection", "now", "operations", "protocol", "tz"])
            other = {"id": "t/other", "clock": {"tz": "UTC", "start": "2030-01-15T10:00:01"},
                     "steps": [{"observe": ["timing"]}]}
            self.assertEqual(worker.request({"cmd": "run", "scenario": other})["error"]["kind"], "protocol_error")
            same = {"id": "t/same", "clock": {"tz": "UTC", "start": "2030-01-15T10:00:00"},
                    "steps": [{"observe": ["timing"]}]}
            resp = worker.request({"cmd": "run", "scenario": same})
            self.assertTrue(resp["ok"], resp)
            self.assertEqual(resp["observations"]["steps"][0]["timing"]["now"], NOW)
