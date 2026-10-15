# SPDX-License-Identifier: AGPL-3.0-or-later
"""The collection check (specs/conformance/collection-check.md K1-K9, K11) and
the check_db op (K10), through real workers and the harness entry point.

Each stored problem is planted here with SQL on a collection Anki made, so the
expected count is the number of rows planted. Expected zones come from
zoneinfo, expected render sizes from the templates' own text. K11's expected
queue orders follow the default preset's gather and sort orders (deck by deck
in name order, then position); its digests are recomputed here from K11.2's
line form."""

from __future__ import annotations

import hashlib
import json
import os
import shutil
import sqlite3
import struct
import tempfile
import unittest
from datetime import datetime, timezone
from pathlib import Path
from zoneinfo import ZoneInfo

import anki.errors
from anki import notetypes_pb2
from anki._backend import RustBackend
from anki.collection import Collection

from driver import harness
from driver.canon import golden_text
from driver.errors import DriverError
from driver.harness import ORACLE_HOME, WorkerProcess
from driver.ops import maintenance
from driver.tests.test_worker import run_ok, scenario

NOW = int(datetime(2030, 1, 15, 10, 0, tzinfo=timezone.utc).timestamp())
NOW_MS = NOW * 1000
DAY_MS = 86_400_000
ZEROS = dict.fromkeys(
    ["card_properties", "new_positions_too_high", "card_last_review_time", "cards_without_note", "decks_missing",
     "field_counts", "duplicate_card_ords", "cards_without_template", "revlog_properties", "notes_invalid_utf8",
     "future_ids", "notetypes_recovered"],
    0,
)
EMPTY_SHA256 = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
HEX64 = r"^[0-9a-f]{64}$"
CARD_COPY = (
    "insert into cards select ?, ?, did, ?, mod, usn, type, queue, due, ivl, factor, reps, lapses, left, odue, "
    "odid, flags, data from cards where id = ?"
)


def build(root: str, name: str, mutate=None, notes: int = 3) -> str:
    """<root>/<name>/collection.anki2 with `notes` Basic notes in deck D,
    then mutate(col, note ids, card ids) through Anki's own connection."""
    directory = os.path.join(root, name)
    os.makedirs(directory)
    col = Collection(os.path.join(directory, "collection.anki2"))
    basic = col.models.by_name("Basic")
    did = col.decks.id("D")
    nids = []
    for i in range(notes):
        note = col.new_note(basic)
        note["Front"], note["Back"] = f"q{i}", f"a{i}"
        col.add_note(note, did)
        nids.append(note.id)
    cids = [col.db.scalar("select id from cards where nid = ?", nid) for nid in nids]
    if mutate:
        mutate(col, nids, cids)
    col.close(downgrade=False)
    return directory


def set_template(col, ntid: int, ord_: int, **formats: str) -> None:
    """Rewrite a stored template's formats, as an older release or an import
    could have left them (Anki refuses to save a template naming no field)."""
    blob = col.db.scalar("select config from templates where ntid = ? and ord = ?", ntid, ord_)
    config = notetypes_pb2.Notetype.Template.Config()
    config.ParseFromString(bytes(blob))
    for key, value in formats.items():
        setattr(config, key, value)
    col.db.execute(f"update templates set config = X'{config.SerializeToString().hex()}' where ntid = ? and ord = ?",
                   ntid, ord_)


def corrupt_freelist(path: str) -> None:
    """Make the header's free page count larger than the free list: SQLite's
    quick check reports it, reads never touch it."""
    with open(path, "r+b") as f:
        f.seek(36)
        count = struct.unpack(">I", f.read(4))[0]
        f.seek(36)
        f.write(struct.pack(">I", count + 3))


def plant_mixed(col, nids, cids) -> None:
    """Deck D under a four-new-per-day preset with one review card due, one
    learning card due now and four more new cards; D::S with two new cards."""
    did = col.decks.id_for_name("D")
    preset = col.decks.add_config_returning_id("Four")
    conf = col.decks.get_config(preset)
    conf["new"]["perDay"] = 4
    col.decks.update_config(conf)
    deck = col.decks.get(did)
    deck["conf"] = preset
    col.decks.save(deck)
    sub = col.decks.id("D::S")
    col.db.execute("update cards set did = ? where id in (?, ?)", sub, cids[6], cids[7])
    col.db.execute("update cards set type = 2, queue = 2, ivl = 1, due = 0, factor = 2500 where id = ?", cids[0])
    col.db.execute("update cards set type = 1, queue = 1, left = 1, due = ? where id = ?", NOW - 60, cids[1])


def lines_sha256(lines: list[str]) -> str:
    """K11.2: each line followed by LF."""
    return hashlib.sha256("".join(line + "\n" for line in lines).encode("utf-8")).hexdigest()


def sql(directory: str, *statements: str) -> None:
    con = sqlite3.connect(os.path.join(directory, "collection.anki2"))
    for statement in statements:
        con.execute(statement)
    con.commit()
    con.close()


def deck_ids(directory: str) -> list[int]:
    con = sqlite3.connect(f"file:{os.path.join(directory, 'collection.anki2')}?immutable=1", uri=True)
    ids = [row[0] for row in con.execute("select id from decks order by id")]
    con.close()
    return ids


def files_digest(directory: str) -> dict[str, str]:
    return {
        name: hashlib.sha256(Path(directory, name).read_bytes()).hexdigest()
        for name in sorted(os.listdir(directory)) if os.path.isfile(os.path.join(directory, name))
    }


class CheckBase(unittest.TestCase):
    """One worker per test: the TAP runner runs tests one by one, without
    class fixtures."""

    tz = "UTC"

    def setUp(self):
        self.tmp = tempfile.mkdtemp(prefix="check-test-")
        self.addCleanup(shutil.rmtree, self.tmp, True)
        self.worker = WorkerProcess(self.tz)
        self.addCleanup(self.worker.close)

    def check(self, directory: str, now: int = NOW, **fields) -> dict:
        resp = self.worker.request({"cmd": "check", "dir": directory, "now": now, **fields})
        self.assertTrue(resp["ok"], resp)
        return resp["check"]

    def check_error(self, request: dict) -> dict:
        resp = self.worker.request({"cmd": "check", **request})
        self.assertFalse(resp["ok"], resp)
        return resp["error"]

    def counts_of(self, name: str, mutate, **kw) -> dict:
        report = self.check(build(self.tmp, name, mutate, **kw))
        return report["check_db"]


class CheckDbKinds(CheckBase):
    """K5: each kind from its planted problems, every other kind 0."""

    def expect(self, name, mutate, kind, count, **kw):
        self.assertEqual(self.counts_of(name, mutate, **kw), {**ZEROS, kind: count})

    def test_a_clean_collection_reports_nothing_and_changes_only_col_and_config(self):
        report = self.check(build(self.tmp, "clean", None))
        self.assertEqual(report["check_db"], ZEROS)
        self.assertLessEqual(set(report["changed_tables"]), {"col", "config"})

    def test_card_properties(self):
        def plant(col, nids, cids):
            col.db.execute("update cards set due = due + 0.5 where id in (?, ?)", cids[0], cids[1])
        self.expect("card-properties", plant, "card_properties", 2)

    def test_new_positions_too_high_from_two_million(self):
        def plant(col, nids, cids):
            col.db.execute("update cards set due = 2000000 where id = ?", cids[0])
            col.db.execute("update cards set due = 1500000 where id = ?", cids[1])
        self.expect("positions", plant, "new_positions_too_high", 1)

    def test_card_last_review_time(self):
        def plant(col, nids, cids):
            for i, cid in enumerate(cids[:2]):
                col.db.execute(
                    "update cards set type = 2, queue = 2, ivl = 3, due = 5, factor = 2500, reps = 1, data = '{}' "
                    "where id = ?", cid)
                col.db.execute("insert into revlog values (?, ?, -1, 3, 3, 0, 2500, 4000, 0)", NOW_MS - 3 * DAY_MS + i, cid)
        self.expect("last-review", plant, "card_last_review_time", 2)

    def test_cards_without_note(self):
        def plant(col, nids, cids):
            for n in (1, 2):
                col.db.execute(CARD_COPY, cids[-1] + 1000 * n, 4242, 0, cids[0])
        self.expect("orphans", plant, "cards_without_note", 2)

    def test_decks_missing_counts_missing_ids_and_missing_parents(self):
        def plant(col, nids, cids):
            col.db.execute("update cards set did = 4242 where id = ?", cids[0])
            col.db.execute("update decks set name = ? where name = 'D'", "P\x1fQ")
        self.expect("decks", plant, "decks_missing", 2)

    def test_field_counts(self):
        def plant(col, nids, cids):
            col.db.execute("update notes set flds = 'only' where id = ?", nids[0])
        report = self.check(build(self.tmp, "fields", plant))
        self.assertEqual(report["check_db"], {**ZEROS, "field_counts": 1})
        self.assertIn("notes", report["changed_tables"])

    def test_duplicate_card_ords(self):
        def plant(col, nids, cids):
            for n in (1, 2):
                col.db.execute(CARD_COPY, cids[-1] + 1000 * n, nids[0], 0, cids[0])
        self.expect("duplicates", plant, "duplicate_card_ords", 2)

    def test_cards_without_template(self):
        def plant(col, nids, cids):
            col.db.execute(CARD_COPY, cids[-1] + 1000, nids[0], 3, cids[0])
        self.expect("no-template", plant, "cards_without_template", 1)

    def test_revlog_properties(self):
        def plant(col, nids, cids):
            for i in (1, 2):
                col.db.execute("insert into revlog values (?, ?, -1, 0, 0, 0, 0, 0, 0)", NOW_MS - DAY_MS + i, cids[0])
        self.expect("revlog", plant, "revlog_properties", 2)

    def test_notes_invalid_utf8(self):
        def plant(col, nids, cids):
            col.db.execute("update notes set flds = CAST(X'71ff1f61' AS TEXT) where id = ?", nids[0])
        self.expect("utf8", plant, "notes_invalid_utf8", 1)

    def test_notetypes_recovered_is_one_however_many_were_missing(self):
        def plant(col, nids, cids):
            col.db.execute("update notes set mid = 777 where id = ?", nids[0])
            col.db.execute("update notes set mid = 778 where id = ?", nids[1])
        self.expect("notetypes", plant, "notetypes_recovered", 1)

    def test_future_ids_count_from_the_frozen_clock(self):
        def plant(col, nids, cids):
            for nid, new in ((nids[0], NOW_MS + DAY_MS + 1), (nids[1], NOW_MS + DAY_MS)):
                col.db.execute("update notes set id = ? where id = ?", new, nid)
                col.db.execute("update cards set nid = ? where nid = ?", new, nid)
            col.db.execute("insert into revlog values (?, ?, -1, 3, 1, 0, 0, 1000, 0)", NOW_MS + 2 * DAY_MS, cids[2])
        directory = build(self.tmp, "future", plant)
        # Cutoff: the check's now plus one day; one second later the first
        # note is no longer after it.
        self.assertEqual(self.check(directory)["check_db"], {**ZEROS, "future_ids": 2})
        self.assertEqual(self.check(directory, now=NOW + 1)["check_db"], {**ZEROS, "future_ids": 1})

    def test_a_corrupt_file_is_the_rejection_and_changes_nothing(self):
        def plant(col, nids, cids):
            col.db.execute("update notes set flds = ? || char(31) || 'a'", "x" * 4000)
            col.remove_notes(nids[:250])
        directory = build(self.tmp, "corrupt", plant, notes=300)
        path = os.path.join(directory, "collection.anki2")
        con = sqlite3.connect(path)
        self.assertGreater(con.execute("pragma freelist_count").fetchone()[0], 0)
        con.close()
        corrupt_freelist(path)
        report = self.check(directory)
        self.assertEqual(report["check_db"], {"error": "corrupt"})
        self.assertEqual(report["changed_tables"], [])
        self.assertEqual(report["counts"]["notes"], 50)


class Messages(unittest.TestCase):
    """K5's mapping from Anki's messages, both plural forms."""

    def setUp(self):
        self.tmp = tempfile.mkdtemp(prefix="check-messages-")
        self.addCleanup(shutil.rmtree, self.tmp, True)
        self.col = Collection(os.path.join(self.tmp, "collection.anki2"), backend=RustBackend(langs=["en"]))
        self.addCleanup(self.col.close, False)

    def test_every_counted_kind_reads_back_its_count(self):
        tr = self.col.tr
        # The K5 table's kinds by their messages (ftl database-check).
        expected = {
            "card_properties": "database_check_card_properties",
            "new_positions_too_high": "database_check_new_card_high_due",
            "card_last_review_time": "database_check_card_last_review_time_empty",
            "cards_without_note": "database_check_card_missing_note",
            "decks_missing": "database_check_missing_decks",
            "field_counts": "database_check_field_count",
            "duplicate_card_ords": "database_check_duplicate_card_ords",
            "cards_without_template": "database_check_missing_templates",
            "revlog_properties": "database_check_revlog_properties",
            "notes_invalid_utf8": "database_check_notes_with_invalid_utf8",
            "future_ids": "database_check_fixed_invalid_ids",
        }
        self.assertEqual(set(expected) | {"notetypes_recovered"}, set(ZEROS))
        for kind, method in expected.items():
            for n in (1, 2, 10, 1234567):
                with self.subTest(kind=kind, n=n):
                    counts = maintenance.problem_counts(tr, [getattr(tr, method)(count=n)])
                    self.assertEqual(counts, {**ZEROS, kind: n})

    def test_several_kinds_at_once_and_the_recovered_note_types(self):
        tr = self.col.tr
        problems = [tr.database_check_notetypes_recovered(), tr.database_check_missing_decks(count=3),
                    tr.database_check_card_properties(count=1)]
        self.assertEqual(maintenance.problem_counts(tr, problems),
                         {**ZEROS, "notetypes_recovered": 1, "decks_missing": 3, "card_properties": 1})

    def test_an_unknown_or_repeated_message_is_a_driver_error(self):
        tr = self.col.tr
        for problems in (["Something else."], [tr.database_check_missing_decks(count=2)] * 2):
            with self.subTest(problems=problems), self.assertRaises(DriverError) as ctx:
                maintenance.problem_counts(tr, problems)
            self.assertEqual(ctx.exception.kind, "internal_error")
            self.assertNotIn("Something", json.dumps(ctx.exception.to_json()))

    def test_the_corrupt_refusal_is_recognized_by_its_text(self):
        col = self.col

        class Backend:
            @staticmethod
            def check_database():
                raise anki.errors.DBError(col.tr.database_check_corrupt(), None, None, None)

        class Fake:
            tr = col.tr
            _backend = Backend()

        self.assertIsNone(maintenance.check_database(Fake()))

        def other():
            raise anki.errors.DBError("Some other database error", None, None, None)

        Backend.check_database = staticmethod(other)
        with self.assertRaises(anki.errors.DBError):
            maintenance.check_database(Fake())


class CheckReport(CheckBase):
    tz = "Etc/GMT-1"

    def test_counts_timing_and_the_source_left_untouched(self):
        directory = build(self.tmp, "plain", None, notes=4)
        before = files_digest(directory)
        report = self.check(directory)
        self.assertEqual(files_digest(directory), before)
        con = sqlite3.connect(f"file:{os.path.join(directory, 'collection.anki2')}?immutable=1", uri=True)
        con.create_collation("unicase", lambda a, b: (a.casefold() > b.casefold()) - (a.casefold() < b.casefold()))
        expected = {name: con.execute(f"select count() from {table}").fetchone()[0]
                    for name, table in (("notes", "notes"), ("cards", "cards"), ("revlog", "revlog"),
                                        ("notetypes", "notetypes"), ("decks", "decks"), ("presets", "deck_config"),
                                        ("tags", "tags"), ("graves", "graves"))}
        con.close()
        self.assertEqual(expected["notes"], 4)
        self.assertEqual(report["counts"], expected)
        self.assertEqual(report["now"], NOW)
        self.assertEqual(report["tz"], "Etc/GMT-1")
        self.assertEqual(report["timing"]["now"], NOW)
        self.assertEqual(report["timing"]["utc_offset_secs"], 3600)
        self.assertTrue(NOW < report["timing"]["next_day_at"] <= NOW + 86400)
        self.assertEqual((report["decks"], report["render"]), ({}, {}))

    def test_a_wal_file_beside_the_collection_is_read(self):
        directory = build(self.tmp, "wal-source", None)
        copy = os.path.join(self.tmp, "wal")
        os.makedirs(copy)
        path = os.path.join(directory, "collection.anki2")
        con = sqlite3.connect(path)
        con.execute("pragma wal_autocheckpoint = 0")
        con.execute("insert into graves values (1, 0, -1)")
        con.commit()
        for suffix in ("", "-wal"):
            shutil.copyfile(path + suffix, os.path.join(copy, "collection.anki2" + suffix))
        con.close()
        alone = sqlite3.connect(f"file:{os.path.join(copy, 'collection.anki2')}?immutable=1", uri=True)
        self.assertEqual(alone.execute("select count() from graves").fetchone()[0], 0)
        alone.close()
        self.assertEqual(self.check(copy)["counts"]["graves"], 1)

    def test_deck_rows_count_subdecks_under_their_limits(self):
        def plant(col, nids, cids):
            did = col.decks.id_for_name("D")
            preset = col.decks.add_config_returning_id("Four")
            conf = col.decks.get_config(preset)
            conf["new"]["perDay"] = 4
            col.decks.update_config(conf)
            deck = col.decks.get(did)
            deck["conf"] = preset
            col.decks.save(deck)
            sub = col.decks.id("D::S")
            col.db.execute("update cards set did = ? where id in (?, ?)", sub, cids[6], cids[7])
            col.db.execute("update cards set type = 2, queue = 2, ivl = 1, due = 0, factor = 2500 where id = ?", cids[0])
            col.db.execute("update cards set type = 1, queue = 1, left = 1, due = ? where id = ?", NOW - 60, cids[1])
        report = self.check(build(self.tmp, "decks", plant, notes=8), decks=["D", "D::S", "Nope"])
        self.assertEqual(report["decks"], {
            "D": {"new": 4, "learning": 1, "review": 1},
            "D::S": {"new": 2, "learning": 0, "review": 0},
            "Nope": None,
        })

    def test_render_statuses_sizes_and_order(self):
        ids = {}

        def plant(col, nids, cids):
            mm = col.models
            model = mm.new("R")
            for name in ("Front", "Back"):
                mm.add_field(model, mm.new_field(name))
            for name, q, a in (("C1", "Q:{{Front}}", "A:{{Back}}"), ("C2", "{{Back}}", "B")):
                t = mm.new_template(name)
                t["qfmt"], t["afmt"] = q, a
                mm.add_template(model, t)
            mm.add(model)
            broken = mm.copy(model, add=False)
            broken["name"] = "Broken"
            mm.add(broken)
            r, b = mm.by_name("R"), mm.by_name("Broken")
            set_template(col, b["id"], 0, a_format="{{Nope}}")
            did = col.decks.id("D")
            for key, model_ in (("ok", r), ("empty", r), ("broken", b)):
                note = col.new_note(model_)
                note["Front"], note["Back"] = "é", "xyz"
                col.add_note(note, did)
                ids[key] = note.id
            col.db.execute("update notes set flds = ? where id = ?", "\x1fxyz", ids["empty"])
        directory = build(self.tmp, "render", plant, notes=0)
        report = self.check(directory, render_notes=[ids["ok"], ids["empty"], ids["broken"], 4242])
        cards = {key: report["render"][str(nid)] for key, nid in ids.items()}
        self.assertIsNone(report["render"]["4242"])
        strip = [{k: v for k, v in c.items() if k != "card"} for c in cards["ok"]]
        self.assertEqual(strip, [
            {"ord": 0, "status": "ok", "question_bytes": len("Q:é".encode()), "answer_bytes": len("A:xyz".encode())},
            {"ord": 1, "status": "ok", "question_bytes": len(b"xyz"), "answer_bytes": len(b"B")},
        ])
        self.assertEqual([c["status"] for c in cards["empty"]], ["empty", "ok"])
        self.assertEqual(cards["broken"][0]["status"], "template_error")
        self.assertEqual((cards["broken"][0]["question_bytes"], cards["broken"][0]["answer_bytes"]), (0, 0))
        self.assertEqual(cards["broken"][1]["status"], "ok")
        con = sqlite3.connect(f"file:{os.path.join(directory, 'collection.anki2')}?immutable=1", uri=True)
        for key, nid in ids.items():
            stored = [row[0] for row in con.execute("select id from cards where nid = ? order by ord", (nid,))]
            self.assertEqual([c["card"] for c in cards[key]], stored)
        con.close()

    def test_errors_no_collection_open_failed_and_bad_requests(self):
        empty = os.path.join(self.tmp, "empty")
        os.makedirs(empty)
        self.assertEqual(self.check_error({"dir": empty, "now": NOW})["kind"], "no_collection")
        garbage = os.path.join(self.tmp, "garbage")
        os.makedirs(garbage)
        Path(garbage, "collection.anki2").write_bytes(b"not a collection" * 512)
        self.assertEqual(self.check_error({"dir": garbage, "now": NOW})["kind"], "open_failed")
        newer = build(self.tmp, "newer", None)
        con = sqlite3.connect(os.path.join(newer, "collection.anki2"))
        con.execute("update col set ver = 19")
        con.commit()
        con.close()
        self.assertEqual(self.check_error({"dir": newer, "now": NOW})["kind"], "open_failed")
        for bad in ({"dir": newer}, {"dir": newer, "now": -1}, {"dir": newer, "now": True},
                    {"dir": newer, "now": NOW, "decks": "D"}, {"dir": newer, "now": NOW, "render_notes": ["1"]},
                    {"dir": newer, "now": NOW, "extra": 1}, {"dir": "", "now": NOW}):
            with self.subTest(bad=bad):
                self.assertEqual(self.check_error(bad)["kind"], "protocol_error")


class Schedule(CheckBase):
    """K11: the schedule member, its digests and what changes them."""

    tz = "Etc/GMT-1"

    def schedule(self, directory: str) -> dict:
        return self.check(directory, schedule=True)["schedule"]

    def copy_of(self, directory: str, name: str) -> str:
        copy = os.path.join(self.tmp, name)
        shutil.copytree(directory, copy)
        return copy

    def test_schedule_is_present_only_with_the_flag(self):
        directory = build(self.tmp, "plain", None)
        self.assertNotIn("schedule", self.check(directory))
        self.assertNotIn("schedule", self.check(directory, schedule=False))
        self.assertIn("schedule", self.check(directory, schedule=True))
        for bad in (1, "true", None):
            with self.subTest(schedule=bad):
                error = self.check_error({"dir": directory, "now": NOW, "schedule": bad})
                self.assertEqual(error["kind"], "protocol_error")

    def test_the_member_has_the_report_schema_shape(self):
        """collection-check.schema.json's schedule, deck_counts and
        schedule_deck (the host checks validate a report with jsonschema)."""
        directory = build(self.tmp, "shape", plant_mixed, notes=8)
        report = self.schedule(directory)
        counts_keys = {"new", "learning", "review"}

        def counts(value):
            self.assertEqual(set(value), counts_keys)
            for v in value.values():
                self.assertIsInstance(v, int)
                self.assertGreaterEqual(v, 0)

        self.assertEqual(set(report), {"cards", "decks", "root", "sha256"})
        self.assertEqual(report["cards"], 8)
        counts(report["root"])
        self.assertRegex(report["sha256"], HEX64)
        self.assertEqual(sorted(int(k) for k in report["decks"]), deck_ids(directory))
        for key, deck in report["decks"].items():
            with self.subTest(deck=key):
                self.assertRegex(key, r"^-?[0-9]+$")
                self.assertEqual(set(deck), {"cards", "next_states_sha256", "queue", "tree"})
                self.assertRegex(deck["next_states_sha256"], HEX64)
                self.assertEqual(set(deck["queue"]), counts_keys | {"entries", "order_sha256"})
                self.assertRegex(deck["queue"]["order_sha256"], HEX64)
                counts({k: deck["queue"][k] for k in counts_keys})
                if deck["tree"] is not None:
                    counts(deck["tree"])
        self.assertEqual(sum(d["cards"] for d in report["decks"].values()), 8)
        body = {k: report[k] for k in ("cards", "decks", "root")}
        compact = json.dumps(body, sort_keys=True, separators=(",", ":"), ensure_ascii=False)
        self.assertEqual(report["sha256"], hashlib.sha256(compact.encode("utf-8")).hexdigest())

    def test_an_empty_default_deck_has_no_tree_and_empty_decks_have_empty_digests(self):
        ids = {}

        def plant(col, nids, cids):
            ids["e"] = col.decks.id("E")

        report = self.schedule(build(self.tmp, "empty", plant))
        empty_queue = {"entries": 0, "new": 0, "learning": 0, "review": 0, "order_sha256": EMPTY_SHA256}
        self.assertEqual(report["decks"]["1"], {
            "cards": 0, "next_states_sha256": EMPTY_SHA256, "queue": empty_queue, "tree": None,
        })
        self.assertEqual(report["decks"][str(ids["e"])], {
            "cards": 0, "next_states_sha256": EMPTY_SHA256, "queue": empty_queue,
            "tree": {"new": 0, "learning": 0, "review": 0},
        })

    def test_queue_counts_equal_the_deck_list_without_siblings(self):
        ids = {}

        def plant(col, nids, cids):
            plant_mixed(col, nids, cids)
            ids.update(cids=cids, d=col.decks.id_for_name("D"), s=col.decks.id_for_name("D::S"))

        directory = build(self.tmp, "counts", plant, notes=8)
        report = self.schedule(directory)
        for key, deck in report["decks"].items():
            if deck["tree"] is not None:
                with self.subTest(deck=key):
                    self.assertEqual({k: deck["queue"][k] for k in ("new", "learning", "review")}, deck["tree"])
        d, s = report["decks"][str(ids["d"])], report["decks"][str(ids["s"])]
        self.assertEqual((d["cards"], d["tree"], d["queue"]["entries"]), (6, {"new": 4, "learning": 1, "review": 1}, 6))
        self.assertEqual((s["cards"], s["tree"], s["queue"]["entries"]), (2, {"new": 2, "learning": 0, "review": 0}, 2))
        self.assertEqual(report["root"], {"new": 4, "learning": 1, "review": 1})
        cids = ids["cids"]
        self.assertEqual(s["queue"]["order_sha256"], lines_sha256([f"{cids[6]}\tnew", f"{cids[7]}\tnew"]))
        self.assertEqual(self.schedule(directory), report, "the frozen clock gives the same schedule twice")

    def test_field_text_alone_leaves_the_schedule_unchanged(self):
        directory = build(self.tmp, "text", plant_mixed, notes=8)
        copy = self.copy_of(directory, "text-copy")
        sql(copy, "update notes set flds = 'other ' || id || char(31) || 'text, longer than before'")
        con = sqlite3.connect(f"file:{os.path.join(copy, 'collection.anki2')}?immutable=1", uri=True)
        self.assertEqual(con.execute("select count() from notes where flds like 'other %'").fetchone()[0], 8)
        con.close()
        self.assertEqual(self.schedule(copy), self.schedule(directory))

    def test_swapping_the_name_order_of_two_siblings_reorders_their_parents_queue(self):
        ids = {}

        def plant(col, nids, cids):
            a, b = col.decks.id("P::A"), col.decks.id("P::B")
            col.db.execute("update cards set did = ? where id in (?, ?)", a, cids[0], cids[1])
            col.db.execute("update cards set did = ? where id in (?, ?)", b, cids[2], cids[3])
            ids.update(cids=cids, p=col.decks.id_for_name("P"), a=a, b=b)

        directory = build(self.tmp, "siblings", plant, notes=4)
        copy = self.copy_of(directory, "siblings-renamed")
        col = Collection(os.path.join(copy, "collection.anki2"))
        col.decks.rename(ids["a"], "P::Y")
        col.decks.rename(ids["b"], "P::X")
        col.close(downgrade=False)
        before, after = self.schedule(directory), self.schedule(copy)
        c = [f"{cid}\tnew" for cid in ids["cids"]]
        parent = str(ids["p"])
        # Default preset: gathered deck by deck in name order, positions within.
        self.assertEqual(before["decks"][parent]["queue"]["order_sha256"], lines_sha256(c))
        self.assertEqual(after["decks"][parent]["queue"]["order_sha256"], lines_sha256(c[2:] + c[:2]))
        for did in (ids["a"], ids["b"]):
            self.assertEqual(after["decks"][str(did)], before["decks"][str(did)])
        self.assertNotEqual(after["sha256"], before["sha256"])

    def test_one_cards_due_changes_its_decks_next_states_and_the_whole_digest(self):
        ids = {}

        def plant(col, nids, cids):
            plant_mixed(col, nids, cids)
            ids.update(cids=cids, d=col.decks.id_for_name("D"))

        directory = build(self.tmp, "due", plant, notes=8)
        copy = self.copy_of(directory, "due-copy")
        # The last new card of D (positions are the notes' order) stays last.
        sql(copy, f"update cards set due = due + 100 where id = {ids['cids'][5]}")
        before, after = self.schedule(directory), self.schedule(copy)
        changed = sorted(k for k in before["decks"]
                         if before["decks"][k]["next_states_sha256"] != after["decks"][k]["next_states_sha256"])
        self.assertEqual(changed, [str(ids["d"])])
        self.assertEqual(after["decks"][str(ids["d"])]["queue"], before["decks"][str(ids["d"])]["queue"])
        self.assertNotEqual(after["sha256"], before["sha256"])


class CheckDbOp(unittest.TestCase):
    """K10 in scenarios."""

    def test_check_db_reports_then_finds_nothing(self):
        steps = [
            {"db_exec": {"sql": "INSERT INTO revlog VALUES (1, 1, -1, 0, 0, 0, 0, 0, 0)"}},
            {"check_db": {}},
            {"check_db": {}},
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario("t/check-db", "UTC", "2030-01-15T10:00:00", steps, notes=1))
        self.assertEqual(out[1], {"check_db": {**ZEROS, "revlog_properties": 1}})
        self.assertEqual(out[2], {"check_db": ZEROS})

    def test_check_db_takes_an_empty_mapping(self):
        with WorkerProcess("UTC") as w:
            for args in ({"x": 1}, None, []):
                with self.subTest(args=args):
                    resp = w.request({"cmd": "run", "scenario": scenario(
                        "t/check-db", "UTC", "2030-01-15T10:00:00", [{"check_db": args}])})
                    self.assertEqual(resp["error"]["kind"], "invalid_scenario", resp)


class Defaults(unittest.TestCase):
    """K1's zone and clock defaults (harness side)."""

    def test_whole_hour_offsets_name_etc_zones_with_that_offset(self):
        at = datetime.fromtimestamp(NOW, timezone.utc)
        for hours in range(-12, 15):
            with self.subTest(hours=hours):
                zone = harness.zone_for_offset(hours * 3600)
                self.assertEqual(zone, "UTC" if hours == 0 else f"Etc/GMT{'-' if hours > 0 else '+'}{abs(hours)}")
                self.assertEqual(at.astimezone(ZoneInfo(zone)).utcoffset().total_seconds(), hours * 3600)

    def test_other_offsets_need_tz(self):
        for offset in (1800, 19800, -16200, 15 * 3600, -13 * 3600, True, None, "3600", 3600.0):
            with self.subTest(offset=offset), self.assertRaises(harness.CheckUsage):
                harness.zone_for_offset(offset)

    def test_now_and_zone_come_from_the_options_or_the_manifest(self):
        with tempfile.TemporaryDirectory() as tmp:
            d = Path(tmp)
            with self.assertRaises(harness.CheckUsage):
                harness.check_defaults(d, None, "UTC")
            with self.assertRaises(harness.CheckUsage):
                harness.check_defaults(d, "2030-01-15T10:00:00Z", None)
            self.assertEqual(harness.check_defaults(d, "2030-01-15T11:00:00+01:00", "Asia/Kathmandu"),
                             (NOW, "Asia/Kathmandu"))
            (d / "manifest.json").write_text(json.dumps({"now_ms": NOW_MS + 999, "utc_offset_secs": -18000}))
            self.assertEqual(harness.check_defaults(d, None, None), (NOW, "Etc/GMT+5"))
            self.assertEqual(harness.check_defaults(d, "2030-01-15T10:00:01Z", "UTC"), (NOW + 1, "UTC"))
            (d / "manifest.json").write_text(json.dumps({"now_ms": NOW_MS, "utc_offset_secs": 19800}))
            with self.assertRaises(harness.CheckUsage):
                harness.check_defaults(d, None, None)
            for bad_tz in ("Mars/Olympus", "", "../etc/passwd"):
                with self.subTest(tz=bad_tz), self.assertRaises(harness.CheckUsage):
                    harness.check_defaults(d, None, bad_tz)

    def test_now_is_a_whole_second_with_an_offset(self):
        for text in ("2030-01-15T10:00:00", "2030-01-15T10:00:00.5Z", "2030-01-15 10:00:00Z", "2030-13-15T10:00:00Z",
                     "20300115T100000Z", "1969-12-31T23:59:59Z", "2030-01-15T10:00:00+0100"):
            with self.subTest(text=text), self.assertRaises(harness.CheckUsage):
                harness.parse_check_now(text)
        self.assertEqual(harness.parse_check_now("2030-01-15T05:00:00-05:00"), NOW)

    def test_note_ids_are_decimal_i64(self):
        self.assertEqual([harness.parse_note_id(t) for t in ("1", "-5", str(2**63 - 1))], [1, -5, 2**63 - 1])
        for text in ("", "1.0", "0x10", str(2**63), " 1"):
            with self.subTest(text=text), self.assertRaises(harness.CheckUsage):
                harness.parse_note_id(text)


class HarnessCheck(unittest.TestCase):
    """K4, K7: the report file and the error file."""

    def setUp(self):
        self.tmp = tempfile.mkdtemp(prefix="check-harness-")
        self.addCleanup(shutil.rmtree, self.tmp, True)
        self.directory = build(self.tmp, "col", None)

    def run_check(self, directory: str, *options: str) -> tuple[int, Path]:
        out = Path(tempfile.mkdtemp(dir=self.tmp))
        rc = harness.main(["check", "--dir", directory, "--label", "build/fixtures/x", "--out", str(out), *options])
        return rc, out

    def test_the_report_is_written_in_the_golden_layout(self):
        Path(self.directory, "manifest.json").write_text(json.dumps({"now_ms": NOW_MS + 500, "utc_offset_secs": 3600}))
        rc, out = self.run_check(self.directory, "--deck=D", "--render-note=-1")
        self.assertEqual(rc, 0)
        self.assertFalse((out / "check-error.json").exists())
        text = (out / "check.json").read_text(encoding="utf-8")
        report = json.loads(text)
        self.assertEqual(text, golden_text(report))
        lock = hashlib.sha256((ORACLE_HOME / "requirements.lock").read_bytes()).hexdigest()
        self.assertEqual(report["report_version"], 1)
        self.assertEqual(report["oracle"]["lock_sha256"], lock)
        self.assertEqual(report["oracle"]["anki_version"], "26.9.3")
        self.assertRegex(report["oracle"]["driver_sha256"], "^[0-9a-f]{64}$")
        self.assertEqual((report["dir"], report["now"], report["tz"]), ("build/fixtures/x", NOW, "Etc/GMT-1"))
        self.assertEqual(report["timing"]["utc_offset_secs"], 3600)
        self.assertEqual(report["render"], {"-1": None})
        self.assertEqual(set(report["decks"]), {"D"})
        self.assertEqual(sorted(report), sorted(["report_version", "oracle", "dir", "now", "tz", "timing", "counts",
                                                 "decks", "render", "check_db", "changed_tables"]))

    def test_schedule_flag_adds_the_schedule_member(self):
        rc, out = self.run_check(self.directory, "--tz", "UTC", "--now", "2030-01-15T10:00:00Z", "--schedule")
        self.assertEqual(rc, 0)
        report = json.loads((out / "check.json").read_text(encoding="utf-8"))
        self.assertEqual(report["schedule"]["cards"], 3)
        self.assertEqual(report["report_version"], 1)

    def test_errors_are_written_with_their_k7_kind(self):
        empty = tempfile.mkdtemp(dir=self.tmp)
        garbage = tempfile.mkdtemp(dir=self.tmp)
        Path(garbage, "collection.anki2").write_bytes(b"x" * 4096)
        for directory, options, kind in (
            (empty, ("--tz", "UTC", "--now", "2030-01-15T10:00:00Z"), "no_collection"),
            (self.directory, ("--tz", "UTC"), "usage"),
            (self.directory, ("--tz", "UTC", "--now", "2030-01-15T10:00:00"), "usage"),
            (self.directory, ("--tz", "UTC", "--now", "2030-01-15T10:00:00Z", "--render-note", "x"), "usage"),
            (garbage, ("--tz", "UTC", "--now", "2030-01-15T10:00:00Z"), "open_failed"),
        ):
            with self.subTest(kind=kind, options=options):
                rc, out = self.run_check(directory, *options)
                self.assertEqual(rc, 1)
                self.assertFalse((out / "check.json").exists())
                error = json.loads((out / "check-error.json").read_text(encoding="utf-8"))
                self.assertEqual(error["kind"], kind)
                self.assertIsInstance(error["detail"], str)
