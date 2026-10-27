# SPDX-License-Identifier: AGPL-3.0-or-later
"""Deck packages (specs/conformance/package-ops.md E6, E7, E10, E11) through
real workers: the deck package forms of export and import with their
rejections, and the observations package_media and package_db_dump. Packages
are the scenario's own exports and archives this file writes itself (zip
entries and raw-block zstd frames as RFC 8878 defines them). Expected values
come from the specs (package/apkg-export.md, apkg-import.md I13,
package/container.md P3, P5, P17), the scenario and SQLite reads of files this
file made, never from the driver."""

from __future__ import annotations

import hashlib
import os
import shutil
import sqlite3
import tempfile
import unittest
import zipfile

from driver.harness import WorkerProcess
from driver.tests.test_package import raw_frame, write_zip

START = "2030-01-15T10:00:00"
NOTETYPES = [{"name": "Pair", "fields": ["Front", "Back"], "templates": [{"name": "Card 1", "front": "{{Front}}", "back": "{{Back}}"}]}]
OPTIONS = {
    "merge_notetypes": False, "update_notes": "if_newer", "update_notetypes": "if_newer",
    "with_scheduling": True, "with_deck_configs": False,
}
EMPTY_LOG = {
    "found_notes": 0, "new": [], "updated": [], "duplicate": [], "conflicting": [], "missing_notetype": [],
    "missing_deck": [], "first_field_match": [], "empty_first_field": [],
}


def export(name="x.apkg", limit="collection", **options):
    args = {"format": "apkg", "name": name, "limit": limit, "scheduling": True, "presets": False, "media": False}
    args.update(options)
    return {"export": args}


def import_name(name="x.apkg", **options):
    return {"import": {"format": "apkg", "name": name, **OPTIONS, **options}}


def sc(steps, notes=(), decks=("A", "A::B", "C"), media=()):
    # rollover set: the first day computation (an export's) writes it otherwise.
    setup = {"config": {"rollover": 4}, "decks": list(decks), "notetypes": NOTETYPES,
             "notes": [{"notetype": "Pair", "deck": d, "fields": f} for d, f in notes]}
    if media:
        setup["media_files"] = [{"name": n, "text": t} for n, t in media]
    return {"id": "t/apkg", "clock": {"tz": "UTC", "start": START}, "setup": setup, "steps": steps}


class Apkg(unittest.TestCase):
    def setUp(self):
        scratch = os.environ.get("ORACLE_SCRATCH_ROOT")
        self.root = tempfile.mkdtemp(prefix="fixtures-", dir=scratch)
        self.handoff = tempfile.mkdtemp(prefix="handoff-", dir=scratch)
        self.worker = WorkerProcess("UTC", extra_env={"ORACLE_FIXTURES": self.root, "ORACLE_HANDOFF": self.handoff})

    def tearDown(self):
        self.worker.close()
        shutil.rmtree(self.root, ignore_errors=True)
        shutil.rmtree(self.handoff, ignore_errors=True)

    def run_sc(self, scenario):
        return self.worker.request({"cmd": "run", "scenario": scenario})

    def ok(self, *args, **kw):
        resp = self.run_sc(sc(*args, **kw))
        self.assertTrue(resp["ok"], resp)
        return resp["observations"]["steps"]

    def error(self, *args, **kw):
        resp = self.run_sc(sc(*args, **kw))
        self.assertFalse(resp["ok"], resp)
        return resp["error"]

    def handed(self, steps, **kw):
        """Run steps, hand the session directory over; its path."""
        scenario = sc(steps + [{"handoff": "meguri"}], **kw)
        resp = self.worker.request({"cmd": "run", "scenario": scenario, "steps": [0, len(steps)], "handoff": "out"})
        self.assertTrue(resp["ok"], resp)
        return os.path.join(self.handoff, "out"), resp["observations"]["steps"]


class Export(Apkg):
    NOTES = [("A", ["a1", "x"]), ("A::B", ["b1", "y"]), ("C", ["c1", "z"])]

    def test_limits_count_notes(self):
        steps = self.ok([
            export("all.apkg"), export("a.apkg", {"deck": "A"}), export("b.apkg", {"deck": "A::B"}),
            export("n.apkg", {"notes": ["#n1", "#n3"]}), export("c.apkg", {"cards": ["#c2"]}),
        ], notes=self.NOTES)
        self.assertEqual(steps, [{"export": {"notes": n}} for n in (3, 2, 1, 2, 1)])

    def test_latest_layout_and_nothing_changes(self):
        dump = {"observe": ["db_dump"]}
        out, steps = self.handed([dump, export(), dump], notes=self.NOTES)
        with zipfile.ZipFile(os.path.join(out, "x.apkg")) as z:
            # container.md P11: meta 3, the zstd collection, the placeholder, the map.
            self.assertEqual(z.namelist(), ["meta", "collection.anki21b", "collection.anki2", "media"])
            self.assertEqual(z.read("meta"), b"\x08\x03")
        self.assertEqual(steps[1], {"export": {"notes": 3}})
        self.assertEqual(steps[0], steps[2])

    def test_unknown_deck_and_malformed_forms(self):
        err = self.error([export(limit={"deck": "Nope"})], notes=self.NOTES)
        self.assertEqual((err["kind"], err.get("step")), ("unknown_ref", 0), err)
        bad = [
            {"format": "apkg", "name": "x.apkg"},
            dict(export()["export"], extra=1),
            dict(export()["export"], limit="all"),
            dict(export()["export"], limit={"notes": []}),
            dict(export()["export"], limit={"deck": "A", "notes": ["#n1"]}),
            dict(export()["export"], media=1),
            dict(export()["export"], name="x.colpkg"),
        ]
        for args in bad:
            with self.subTest(args=args):
                err = self.error([{"advance": "1m"}, {"export": args}], notes=self.NOTES)
                self.assertEqual((err["kind"], err.get("step")), ("invalid_scenario", 1), err)
        # E8, the legacy deck form, is a draft.
        err = self.error([{"advance": "1m"}, {"export": dict(export()["export"], legacy=True)}])
        self.assertEqual(err, {"kind": "unsupported_op", "op": "export", "step": 1})

    def test_future_ids_refused_and_no_file(self):
        # apkg-export.md A8 rule 1: a note id three days ahead.
        steps = [
            {"db_exec": {"sql": "UPDATE cards SET nid = nid + 259200000 WHERE nid = (SELECT min(id) FROM notes)"}},
            {"db_exec": {"sql": "UPDATE notes SET id = id + 259200000 WHERE id = (SELECT min(id) FROM notes)"}},
            export("f.apkg"),
        ]
        out, observed = self.handed(steps, notes=self.NOTES)
        self.assertEqual(observed[2], {"export": {"error": "invalid_ids"}})
        self.assertNotIn("f.apkg", os.listdir(out))
        self.assertFalse([n for n in os.listdir(out) if n.endswith((".apkg", ".tmp"))], os.listdir(out))

    def test_unnormalized_media_name_refused(self):
        # A9.2 rule 5: con.jpg is a name Check Media would change.
        steps = self.ok([export(media=True)], notes=[("A", ['<img src="con.jpg">', ""])], media=[("con.jpg", "x")])
        self.assertEqual(steps, [{"export": {"error": "media_check_required"}}])


class Import(Apkg):
    def test_round_trip_updates_and_adds(self):
        notes = [("A", ["one", "uno"]), ("A::B", ["two", "dos"])]
        steps = self.ok([
            export(),
            {"advance": "1m"},
            {"db_exec": {"sql": "UPDATE notes SET flds = 'one' || char(31) || 'changed', mod = mod - 100 WHERE sfld = 'one'"}},
            {"delete_notes": {"notes": ["#n2"]}},
            import_name(update_notes="always"),
            {"db_exec": {"sql": "SELECT flds FROM notes ORDER BY id"}},
        ], notes=notes)
        log = steps[4]["import"]
        self.assertEqual(log["found_notes"], 2)
        self.assertEqual(log["updated"], [{"note": "#n1", "fields": ["one", "uno"]}])
        # The deleted note comes back under its own id, which #n2 still names.
        self.assertEqual(log["new"], [{"note": "#n2", "fields": ["two", "dos"]}])
        for key in ("duplicate", "conflicting", "missing_notetype", "missing_deck", "first_field_match", "empty_first_field"):
            self.assertEqual(log[key], [], key)

    def test_never_logs_duplicates_and_writes_options(self):
        steps = self.ok([
            export(),
            import_name(update_notes="never", merge_notetypes=True, with_deck_configs=True, with_scheduling=False),
            {"db_exec": {"sql": "SELECT key, CAST(val AS TEXT) FROM config WHERE key IN ('mergeNotetypes', "
                                "'withScheduling', 'withDeckConfigs', 'updateNotes', 'updateNotetypes') ORDER BY key"}},
            {"observe": ["undo_status"]},
        ], notes=[("A", ["one", "uno"])])
        self.assertEqual(steps[1]["import"]["duplicate"], [{"note": "#n1", "fields": ["one", "uno"]}])
        self.assertEqual(len(steps[2]["db_exec"]["rows"]), 5)
        self.assertEqual(steps[3]["undo_status"]["undo"], "import")

    def test_kind_conflict_refused_unchanged(self):
        # I8: merging a standard and a cloze note type. The target's note type
        # becomes cloze after the export (protobuf field 1, last occurrence wins).
        dump = {"observe": [{"db_dump": {"tables": ["notes", "cards", "notetypes", "config"]}}]}
        steps = self.ok([
            export(),
            {"db_exec": {"sql": "UPDATE notetypes SET config = CAST(config || X'0801' AS BLOB) WHERE name = 'Pair'"}},
            {"reopen": {}},
            dump,
            import_name(merge_notetypes=True),
            dump,
        ], notes=[("A", ["one", "uno"])])
        self.assertEqual(steps[4], {"import": {"error": "notetype_kind_conflict"}})
        self.assertEqual(steps[3], steps[5])

    def test_refusals_from_fixtures(self):
        os.makedirs(os.path.join(self.root, "package", "apkg"))
        apkg = os.path.join(self.root, "package", "apkg")
        with open(os.path.join(apkg, "not-a-zip.apkg"), "wb") as f:
            f.write(b"this is not a zip archive\n")
        write_zip(os.path.join(apkg, "meta-4.apkg"), [("meta", b"\x08\x04"), ("collection.anki2", b"")])
        write_zip(os.path.join(apkg, "traversal.apkg"), [
            ("collection.anki21", self.legacy_collection()), ("0", b"abc"), ("media", b'{"0": "../x.jpg"}'),
        ])
        write_zip(os.path.join(apkg, "bad-map.apkg"), [
            ("collection.anki21", self.legacy_collection()), ("media", b"not json"),
        ])
        cases = {"not-a-zip": "corrupt", "meta-4": "too_new", "traversal": "corrupt", "bad-map": "corrupt"}
        for name, kind in cases.items():
            with self.subTest(name=name):
                steps = self.ok([{"import": {"format": "apkg", "fixture": f"package/apkg/{name}.apkg", **OPTIONS}}],
                                notes=[("A", ["one", "uno"])])
                self.assertEqual(steps, [{"import": {"error": kind}}])

    def legacy_collection(self) -> bytes:
        """A schema-11 collection with no note (this file's own SQLite)."""
        work = tempfile.mkdtemp(dir=self.root)
        try:
            from anki.collection import Collection

            col = Collection(os.path.join(work, "c.anki2"))
            col.close(downgrade=True)
            with open(os.path.join(work, "c.anki2"), "rb") as f:
                return f.read()
        finally:
            shutil.rmtree(work)

    def test_forms(self):
        err = self.error([{"advance": "1m"}, import_name("missing.apkg")])
        self.assertEqual((err["kind"], err.get("step")), ("unknown_ref", 1), err)
        good = import_name()["import"]
        bad = [
            {"format": "apkg", "name": "x.apkg"},
            {k: v for k, v in good.items() if k != "with_scheduling"},
            dict(good, fixture="package/x.apkg"),
            dict(good, update_notes="sometimes"),
            dict(good, merge_notetypes="yes"),
            dict(good, name="x.colpkg"),
            dict(good, extra=1),
        ]
        for args in bad:
            with self.subTest(args=args):
                err = self.error([{"advance": "1m"}, {"import": args}])
                self.assertEqual((err["kind"], err.get("step")), ("invalid_scenario", 1), err)


class Contents(Apkg):
    def test_package_media_latest(self):
        notes = [("A", ['<img src="b.jpg">[sound:a.mp3]', ""])]
        steps = self.ok([
            export(media=True),
            {"observe": [{"package_media": {"name": "x.apkg"}}]},
            export("none.apkg"),
            {"observe": [{"package_media": {"name": "none.apkg"}}]},
        ], notes=notes, media=[("a.mp3", "sound"), ("b.jpg", "image!"), ("c.png", "unused")])
        media = sorted(steps[1]["package_media"], key=lambda m: m["name"])
        self.assertEqual(media, [
            {"name": "a.mp3", "size": 5, "sha1": hashlib.sha1(b"sound").hexdigest(), "entry_size": 5},
            {"name": "b.jpg", "size": 6, "sha1": hashlib.sha1(b"image!").hexdigest(), "entry_size": 6},
        ])
        self.assertEqual(steps[3]["package_media"], [])

    def test_package_media_legacy_fixture(self):
        os.makedirs(os.path.join(self.root, "p"))
        write_zip(os.path.join(self.root, "p", "legacy.apkg"), [
            ("collection.anki21", b""), ("1", b"two"), ("0", b"one!"), ("media", b'{"1": "b.txt", "0": "a.txt", "2": "c.txt"}'),
        ], deflated=("1",))
        steps = self.ok([{"observe": [{"package_media": {"fixture": "p/legacy.apkg"}}]}])
        self.assertEqual(steps[0]["package_media"], [
            {"name": "a.txt", "size": 4, "sha1": hashlib.sha1(b"one!").hexdigest(), "entry_size": 4},
            {"name": "b.txt", "size": 3, "sha1": hashlib.sha1(b"two").hexdigest(), "entry_size": 3},
            {"name": "c.txt", "size": None, "sha1": None, "entry_size": None},
        ])

    def test_package_db_dump_matches_rows(self):
        notes = [("A", ["one", "uno"]), ("C", ["two", "dos"])]
        steps = self.ok([
            export(limit={"deck": "A"}),
            {"observe": [{"package_db_dump": {"name": "x.apkg", "tables": ["notes", "cards"]}}]},
            {"observe": [{"db_dump": {"tables": ["notes", "cards"]}}]},
        ], notes=notes)
        package = steps[1]["package_db_dump"]
        live = steps[2]["db_dump"]
        # A5: with scheduling the selected rows are unchanged.
        self.assertEqual(len(package["notes"]), 1)
        (key,) = package["notes"]
        self.assertEqual(package["notes"][key], live["notes"][key])
        self.assertEqual(package["cards"], {k: v for k, v in live["cards"].items() if v["nid"] == int(key)})

    def test_package_db_dump_legacy_fixture_and_errors(self):
        os.makedirs(os.path.join(self.root, "p"))
        db = os.path.join(self.root, "legacy.anki2")
        con = sqlite3.connect(db)
        con.execute("CREATE TABLE col (id integer primary key, ver integer)")
        con.execute("INSERT INTO col VALUES (1, 11)")
        con.execute("CREATE TABLE notes (id integer primary key, flds text)")
        con.execute("INSERT INTO notes VALUES (5, 'x')")
        con.commit()
        con.close()
        with open(db, "rb") as f:
            raw = f.read()
        write_zip(os.path.join(self.root, "p", "l.apkg"), [("collection.anki21", raw), ("media", b"{}")])
        steps = self.ok([{"observe": [{"package_db_dump": {"fixture": "p/l.apkg"}}]}])
        self.assertEqual(steps[0]["package_db_dump"], {
            "col": {"1": {"id": 1, "ver": 11}}, "notes": {"5": {"id": 5, "flds": "x"}},
        })
        resp = self.run_sc(sc([{"observe": [{"package_db_dump": {"fixture": "p/l.apkg", "tables": ["decks"]}}]}]))
        self.assertEqual(resp["error"]["kind"], "internal_error", resp)
        err = self.error([{"observe": [{"package_db_dump": {"name": "x.apkg", "tables": []}}]}])
        self.assertEqual(err["kind"], "invalid_scenario", err)
        err = self.error([{"observe": [{"package_media": {"name": "x.apkg"}}, {"package_media": {"name": "y.apkg"}}]}])
        self.assertEqual((err["kind"], err.get("step")), ("invalid_scenario", 0), err)
        err = self.error([{"observe": [{"package_media": {"name": "x.apkg"}}]}])
        self.assertEqual(err["kind"], "unknown_ref", err)


if __name__ == "__main__":
    unittest.main()
