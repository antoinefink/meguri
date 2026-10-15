# SPDX-License-Identifier: AGPL-3.0-or-later
"""Model ops (specs/conformance/model-ops.md L1-L7) through real workers:
the legacy_json, blob_decode and package_col observations, the legacy_update
op, the legacy form of export, suspend, and the db_exec uses the model
scenarios rely on. Expected values come from the scenario (its clock, patches
and SQL), SQLite's own functions, packages the tests write themselves and the
spec texts, never from the driver."""

from __future__ import annotations

import os
import shutil
import sqlite3
import tempfile
import unittest
import zipfile

from driver.harness import WorkerProcess
from driver.tests.test_worker import NOTETYPE, epoch, run_ok, scenario

START = "2030-01-15T10:00:00"


def run(worker, steps, notes=0, setup=None):
    return worker.request({"cmd": "run", "scenario": scenario("t/model", "UTC", START, steps, notes=notes, setup=setup)})


def rows(step):
    return step["db_exec"]["rows"]


class LegacyJson(unittest.TestCase):
    def test_objects_by_name_with_integer_and_float_kinds_kept(self):
        steps = [
            {"observe": [{"legacy_json": {"notetypes": ["QA"], "decks": ["A", "X::Y"], "presets": ["P"]}}]},
            {"db_exec": {"sql": "SELECT (SELECT id FROM notetypes WHERE name = 'QA'), "
                                "(SELECT id FROM decks WHERE name = 'A'), "
                                "(SELECT id FROM decks WHERE name = 'X' || char(31) || 'Y'), "
                                "(SELECT id FROM deck_config WHERE name = 'P')"}},
        ]
        setup = {"decks": [{"name": "A", "preset": "P"}, "X::Y"]}
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario("t/model", "UTC", START, steps, setup=setup))
        value = out[0]["legacy_json"]
        [[ntid, did, nested_did, conf_id]] = rows(out[1])
        self.assertEqual(sorted(value), ["decks", "notetypes", "presets"])
        self.assertEqual(sorted(value["decks"]), ["A", "X::Y"])
        nt, deck, nested, preset = value["notetypes"]["QA"], value["decks"]["A"], value["decks"]["X::Y"], value["presets"]["P"]
        self.assertEqual((nt["id"], nt["name"]), (ntid, "QA"))
        self.assertEqual([f["name"] for f in nt["flds"]], ["Q", "A"])
        self.assertEqual([t["name"] for t in nt["tmpls"]], ["C1"])
        # Deck names in human form (schema11-json S11); conf is the preset's id.
        self.assertEqual((deck["id"], deck["name"], deck["conf"]), (did, "A", conf_id))
        self.assertEqual((nested["id"], nested["name"]), (nested_did, "X::Y"))
        self.assertEqual((preset["id"], preset["name"]), (conf_id, "P"))
        # The scenario helper's preset: learn steps 1m and 10m (minutes, f32),
        # desired retention 0.9 (f32). f32 values keep their float kind, counts stay ints.
        self.assertEqual(preset["new"]["delays"], [1.0, 10.0])
        self.assertEqual([type(x) for x in preset["new"]["delays"]], [float, float])
        self.assertEqual((preset["desiredRetention"], type(preset["desiredRetention"])), (0.9, float))
        self.assertIs(type(preset["new"]["perDay"]), int)
        self.assertIs(type(deck["id"]), int)

    def test_reads_what_raw_sql_wrote_not_a_cached_copy(self):
        steps = [
            {"observe": [{"legacy_json": {"notetypes": ["QA"], "decks": ["A"]}}]},
            {"db_exec": {"sql": "UPDATE notetypes SET name = 'QB' WHERE name = 'QA'"}},
            {"db_exec": {"sql": "UPDATE decks SET name = 'B' || char(31) || 'C' WHERE name = 'A'"}},
            {"observe": [{"legacy_json": {"notetypes": ["QB"], "decks": ["B::C"]}}]},
            {"observe": [{"legacy_json": {"decks": ["A"]}}]},
        ]
        with WorkerProcess("UTC") as w:
            resp = run(w, steps)
        self.assertEqual(resp["error"], {"kind": "unknown_ref", "ref": "A", "step": 4}, resp)
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario("t/model", "UTC", START, steps[:4]))
        before, after = out[0]["legacy_json"], out[3]["legacy_json"]
        self.assertEqual(after["notetypes"]["QB"]["name"], "QB")
        self.assertEqual(after["notetypes"]["QB"]["id"], before["notetypes"]["QA"]["id"])
        self.assertEqual(after["decks"]["B::C"]["name"], "B::C")
        self.assertEqual(after["decks"]["B::C"]["id"], before["decks"]["A"]["id"])

    def test_observing_changes_no_stored_row(self):
        tables = ["col", "notetypes", "fields", "templates", "decks", "deck_config", "config", "notes", "cards", "tags"]
        dump = [{"db_exec": {"sql": f"SELECT * FROM {t} ORDER BY 1"}} for t in tables]
        steps = dump + [{"observe": [{"legacy_json": {"notetypes": ["QA"], "decks": ["A"], "presets": ["P"]}}]}] + dump
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario("t/model", "UTC", START, steps, notes=2))
        n = len(tables)
        self.assertEqual(out[:n], out[n + 1:])
        self.assertEqual(len(rows(out[tables.index("cards")])), 2)

    def test_unknown_names_and_malformed_requests(self):
        # Names match exactly as the collection holds them (Anki's own lookup
        # ignores case).
        unknown = (("notetypes", "Nope"), ("decks", "a"), ("presets", "p"), ("decks", "x::y"), ("decks", "X::Y::"))
        bad = [None, "decks", ["A"], {}, {"decks": []}, {"decks": "A"}, {"decks": [""]}, {"decks": [1]},
               {"cards": ["A"]}, {"decks": ["A"], "notes": ["A"]}]
        with WorkerProcess("UTC") as w:
            for kind, name in unknown:
                resp = run(w, [{"observe": ["ids"]}, {"observe": [{"legacy_json": {"decks": ["X::Y"], kind: [name]}}]}],
                           setup={"decks": [{"name": "A", "preset": "P"}, "X::Y"]})
                with self.subTest(kind=kind, name=name):
                    self.assertEqual(resp["error"], {"kind": "unknown_ref", "ref": name, "step": 1}, resp)
            for args in bad:
                resp = run(w, [{"observe": ["ids"]}, {"observe": [{"legacy_json": args}]}])
                with self.subTest(args=args):
                    self.assertEqual((resp["error"]["kind"], resp["error"].get("step")), ("invalid_scenario", 1), resp)


class LegacyUpdate(unittest.TestCase):
    def test_patch_is_a_json_merge_patch(self):
        # Unknown deck keys travel through common.other (schema11-json S3), so
        # they show the patch rules without Anki's own value conversions.
        steps = [
            {"legacy_update": {"deck": "A", "patch": {
                "desc": "hi", "zz_list": [1, None, {"k": None}], "zz_obj": {"a": 1, "b": 2}, "zz_num": 5}}},
            {"observe": [{"legacy_json": {"decks": ["A"]}}]},
            {"legacy_update": {"deck": "A", "patch": {
                "zz_obj": {"b": None, "c": 3}, "zz_num": {"a": None, "b": 1}, "zz_list": None, "zz_new": {"x": {"y": 1}}}}},
            {"observe": [{"legacy_json": {"decks": ["A"]}}]},
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario("t/model", "UTC", START, steps))
        self.assertEqual((out[0], out[2]), ({}, {}))
        first, second = out[1]["legacy_json"]["decks"]["A"], out[3]["legacy_json"]["decks"]["A"]
        self.assertEqual(first["desc"], "hi")
        # Arrays replace the member whole, nulls inside them kept.
        self.assertEqual(first["zz_list"], [1, None, {"k": None}])
        self.assertEqual((first["zz_obj"], first["zz_num"]), ({"a": 1, "b": 2}, 5))
        # An object merges member by member; null removes; a non-object member
        # or a missing one is merged into {}.
        self.assertEqual(second["zz_obj"], {"a": 1, "c": 3})
        self.assertEqual(second["zz_num"], {"b": 1})
        self.assertEqual(second["zz_new"], {"x": {"y": 1}})
        self.assertNotIn("zz_list", second)
        self.assertEqual(second["desc"], "hi")

    def test_usn_and_mtime(self):
        t = epoch(START, "UTC")
        steps = [
            {"legacy_update": {"deck": "A", "patch": {"usn": 5, "mod": 1700000000}}},
            {"legacy_update": {"notetype": "QA", "patch": {"usn": 6, "mod": 1700000001}}},
            {"advance": "1h"},
            {"legacy_update": {"deck": "A", "patch": {"desc": "later"}}},
            {"legacy_update": {"notetype": "QA", "patch": {"latexPre": "% later"}}},
            {"legacy_update": {"preset": "P", "patch": {"maxTaken": 30}}},
            {"db_exec": {"sql": "SELECT usn, mtime_secs FROM decks WHERE name = 'A'"}},
            {"db_exec": {"sql": "SELECT usn, mtime_secs FROM notetypes WHERE name = 'QA'"}},
            {"db_exec": {"sql": "SELECT usn, mtime_secs FROM deck_config WHERE name = 'P'"}},
            {"observe": [{"legacy_json": {"decks": ["A"], "notetypes": ["QA"], "presets": ["P"]}}]},
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario("t/model", "UTC", START, steps))
        # Note types and decks keep the usn and mod the object holds; a preset
        # is saved as a local change at the clock's second (model-ops.md L2).
        self.assertEqual(rows(out[6]), [[5, 1700000000]])
        self.assertEqual(rows(out[7]), [[6, 1700000001]])
        self.assertEqual(rows(out[8]), [[-1, t + 3600]])
        value = out[9]["legacy_json"]
        self.assertEqual((value["decks"]["A"]["desc"], value["notetypes"]["QA"]["latexPre"]), ("later", "% later"))
        self.assertEqual(value["presets"]["P"]["maxTaken"], 30)

    def test_objects_that_do_not_convert_are_rejected_and_change_nothing(self):
        state = [
            {"db_exec": {"sql": "SELECT name, mtime_secs, usn, hex(common), hex(kind) FROM decks ORDER BY id"}},
            {"db_exec": {"sql": "SELECT name, mtime_secs, usn, hex(config) FROM notetypes ORDER BY id"}},
            {"db_exec": {"sql": "SELECT name, mtime_secs, usn, hex(config) FROM deck_config ORDER BY id"}},
        ]
        # Keys schema11-json marks "refuse" (deck collapsed, preset maxTaken, note type type).
        refused = [
            {"legacy_update": {"deck": "A", "patch": {"collapsed": 1}}},
            {"legacy_update": {"preset": "P", "patch": {"maxTaken": "60"}}},
            {"legacy_update": {"notetype": "QA", "patch": {"type": 2}}},
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario("t/model", "UTC", START, state + refused + [{"advance": "1h"}] + state))
        self.assertEqual(out[3:6], [{"legacy_update": {"error": "invalid_json"}}] * 3)
        self.assertEqual(out[:3], out[7:])

    def test_other_save_failures_are_run_errors(self):
        with WorkerProcess("UTC") as w:
            resp = run(w, [{"observe": ["ids"]}, {"legacy_update": {"notetype": "QA", "patch": {"flds": []}}}])
        self.assertEqual(resp["error"], {"kind": "anki_error", "op": "legacy_update", "exception": "InvalidInput", "step": 1})

    def test_unknown_names_and_malformed_arguments(self):
        bad = [
            None, "A", {}, {"patch": {}}, {"deck": "A"}, {"deck": "A", "patch": None}, {"deck": "A", "patch": [1]},
            {"deck": "A", "notetype": "QA", "patch": {}}, {"deck": "", "patch": {}}, {"deck": 1, "patch": {}},
            {"deck": "A", "patch": {}, "extra": 1},
        ]
        with WorkerProcess("UTC") as w:
            for args in ({"deck": "Nope", "patch": {}}, {"notetype": "Nope", "patch": {}}, {"preset": "Nope", "patch": {}}):
                resp = run(w, [{"observe": ["ids"]}, {"legacy_update": args}])
                with self.subTest(args=args):
                    self.assertEqual(resp["error"], {"kind": "unknown_ref", "ref": "Nope", "step": 1}, resp)
            for args in bad:
                resp = run(w, [{"observe": ["ids"]}, {"legacy_update": args}])
                with self.subTest(args=args):
                    self.assertEqual((resp["error"]["kind"], resp["error"].get("step")), ("invalid_scenario", 1), resp)


class BlobDecode(unittest.TestCase):
    # Blobs and their outcomes from specs/model/protobuf-records.md P9's table.
    ALL = {"notetypes": ["QA"], "decks": ["A", "X::Y"], "presets": ["P"]}
    SETUP = {"decks": [{"name": "A", "preset": "P"}, "X::Y"]}

    def test_each_blob_of_a_record_counts_and_reads_come_from_the_file(self):
        field_a = "ntid = (SELECT id FROM notetypes WHERE name = 'QA') AND ord = 1"
        steps = [
            {"observe": [{"blob_decode": self.ALL}]},
            {"db_exec": {"sql": "UPDATE deck_config SET config = X'0f' WHERE name = 'P'"}},
            {"db_exec": {"sql": "UPDATE decks SET kind = X'0a05' WHERE name = 'X' || char(31) || 'Y'"}},
            {"db_exec": {"sql": f"UPDATE fields SET config = X'ff' WHERE {field_a}"}},
            {"observe": [{"blob_decode": self.ALL}]},
        ]
        ok = {"notetypes": {"QA": "ok"}, "decks": {"A": "ok", "X::Y": "ok"}, "presets": {"P": "ok"}}
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario("t/model", "UTC", START, steps, setup=self.SETUP))
        self.assertEqual(out[0], {"blob_decode": ok})
        self.assertEqual(out[4], {"blob_decode": {
            "notetypes": {"QA": "invalid_blob"}, "decks": {"A": "ok", "X::Y": "invalid_blob"},
            "presets": {"P": "invalid_blob"}}})

    def test_note_type_config_and_template_blobs_and_decodable_odd_blobs(self):
        cases = [
            # A note type's own config and its template's config.
            ("notetypes", "UPDATE notetypes SET config = X'0000' WHERE name = 'QA'", "invalid_blob"),
            ("notetypes", "UPDATE templates SET config = X'3b' WHERE ntid = (SELECT id FROM notetypes WHERE name = 'QA')",
             "invalid_blob"),
            # A deck's common blob.
            ("decks", "UPDATE decks SET common = X'0c' WHERE name = 'A'", "invalid_blob"),
            # P9's two blobs that decode: a 10-byte varint for unknown field 7,
            # and other holding bytes that are not UTF-8.
            ("presets", "UPDATE deck_config SET config = X'38ffffffffffffffffff01' WHERE name = 'P'", "ok"),
            ("presets", "UPDATE deck_config SET config = X'fa0f02ffff' WHERE name = 'P'", "ok"),
            ("presets", "UPDATE deck_config SET config = X'38ffffffffffffffffff02' WHERE name = 'P'", "invalid_blob"),
        ]
        names = {"notetypes": "QA", "decks": "A", "presets": "P"}
        with WorkerProcess("UTC") as w:
            for section, sql, status in cases:
                steps = [{"db_exec": {"sql": sql}}, {"observe": [{"blob_decode": {section: [names[section]]}}]}]
                out = run_ok(self, w, scenario("t/model", "UTC", START, steps))
                with self.subTest(sql=sql):
                    self.assertEqual(out[1], {"blob_decode": {section: {names[section]: status}}})

    def test_other_read_failures_are_run_errors(self):
        # model-ops.md L4: a deck kind with no member, a blob column holding
        # TEXT (the schema keeps NULL out of these columns).
        cases = [
            ("decks", "UPDATE decks SET kind = X'' WHERE name = 'A'"),
            ("presets", "UPDATE deck_config SET config = 'text' WHERE name = 'P'"),
            ("notetypes", "UPDATE fields SET config = 'text' WHERE ord = 0"),
        ]
        names = {"notetypes": "QA", "decks": "A", "presets": "P"}
        with WorkerProcess("UTC") as w:
            for section, sql in cases:
                resp = run(w, [{"db_exec": {"sql": sql}}, {"observe": [{"blob_decode": {section: [names[section]]}}]}])
                with self.subTest(sql=sql):
                    self.assertEqual((resp["error"]["kind"], resp["error"].get("step")), ("anki_error", 1), resp)

    def test_observing_changes_no_stored_row(self):
        tables = ["col", "notetypes", "fields", "templates", "decks", "deck_config", "config"]
        dump = [{"db_exec": {"sql": f"SELECT * FROM {t} ORDER BY 1"}} for t in tables]
        steps = ([{"db_exec": {"sql": "UPDATE deck_config SET config = X'0f' WHERE name = 'P'"}}] + dump
                 + [{"observe": [{"blob_decode": self.ALL}]}] + dump)
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario("t/model", "UTC", START, steps, setup=self.SETUP))
        n = len(tables)
        self.assertEqual(out[n + 1]["blob_decode"]["presets"], {"P": "invalid_blob"})
        self.assertEqual(out[1:n + 1], out[n + 2:])

    def test_unknown_names_and_malformed_requests(self):
        unknown = (("notetypes", "Nope"), ("decks", "a"), ("presets", "p"), ("decks", "x::y"))
        bad = [None, "decks", ["A"], {}, {"decks": []}, {"decks": "A"}, {"decks": [""]}, {"decks": [1]},
               {"cards": ["A"]}, {"decks": ["A"], "notes": ["A"]}]
        with WorkerProcess("UTC") as w:
            for kind, name in unknown:
                resp = run(w, [{"observe": ["ids"]}, {"observe": [{"blob_decode": {"decks": ["X::Y"], kind: [name]}}]}],
                           setup=self.SETUP)
                with self.subTest(kind=kind, name=name):
                    self.assertEqual(resp["error"], {"kind": "unknown_ref", "ref": name, "step": 1}, resp)
            for args in bad:
                resp = run(w, [{"observe": ["ids"]}, {"observe": [{"blob_decode": args}]}])
                with self.subTest(args=args):
                    self.assertEqual((resp["error"]["kind"], resp["error"].get("step")), ("invalid_scenario", 1), resp)


class Suspend(unittest.TestCase):
    def test_listed_cards_are_suspended_and_suspended_ones_left_alone(self):
        t = epoch(START, "UTC")
        query = {"db_exec": {"sql": "SELECT queue, mod FROM cards ORDER BY id"}}
        steps = [
            {"advance": "1h"},
            {"suspend": {"cards": ["#c1", "#c3"]}},
            query,
            {"advance": "1h"},
            {"suspend": {"cards": ["#c1"]}},
            query,
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario("t/model", "UTC", START, steps, notes=3))
        self.assertEqual((out[1], out[4]), ({}, {}))
        # Queue -1 is suspended, 0 new (specs/model/row-records.md); setup ran at t.
        self.assertEqual(rows(out[2]), [[-1, t + 3600], [0, t], [-1, t + 3600]])
        self.assertEqual(rows(out[5]), rows(out[2]))

    def test_malformed_arguments_and_unknown_cards(self):
        with WorkerProcess("UTC") as w:
            missing = run(w, [{"suspend": {"cards": ["#c1", "#c9"]}}], notes=1)
            self.assertEqual(missing["error"], {"kind": "unknown_ref", "ref": "#c9", "step": 0})
            for args in ("#c1", ["#c1"], {}, {"cards": "#c1"}, {"cards": ["#n1"]}, {"cards": ["#c1"], "x": 1}):
                resp = run(w, [{"suspend": args}], notes=1)
                with self.subTest(args=args):
                    self.assertEqual((resp["error"]["kind"], resp["error"].get("step")), ("invalid_scenario", 0), resp)


class DbExecForModelScenarios(unittest.TestCase):
    def test_hex_text_blob_appends_and_bound_card_data(self):
        steps = [
            {"db_exec": {"sql": "SELECT hex(X'0a04ff'), typeof(hex(X'0a04ff'))"}},
            {"db_exec": {"sql": "SELECT typeof(common), hex(common) FROM decks WHERE name = 'A'"}},
            {"db_exec": {"sql": "UPDATE decks SET common = CAST(common || X'fa0f027b7d' AS BLOB) WHERE name = 'A'"}},
            {"db_exec": {"sql": "SELECT typeof(common), hex(common) FROM decks WHERE name = 'A'"}},
            {"db_exec": {"sql": "UPDATE cards SET data = ? WHERE id = (SELECT id FROM cards ORDER BY id LIMIT 1)",
                         "params": ['{"s": 1.5}']}},
            {"db_exec": {"sql": "SELECT typeof(data), data FROM cards ORDER BY id"}},
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario("t/model", "UTC", START, steps, notes=2))
        self.assertEqual(rows(out[0]), [["0A04FF", "text"]])
        [[kind, common]] = rows(out[1])
        self.assertEqual(kind, "blob")
        self.assertEqual((rows(out[2]), rows(out[4])), ([], []))
        self.assertEqual(rows(out[3]), [["blob", common + "FA0F027B7D"]])
        # Anki writes a card with no data as {} (card-data-json C4).
        self.assertEqual(rows(out[5]), [["text", '{"s": 1.5}'], ["text", "{}"]])


LEGACY = {"format": "apkg", "name": "x.apkg", "legacy": True}
QB = {"name": "QB", "fields": ["F", "B"], "templates": [{"name": "T", "front": "{{F}}", "back": "{{B}}"}], "css": ".b {}"}
TABLES = ["col", "notetypes", "fields", "templates", "decks", "deck_config", "config", "notes", "cards", "tags",
          "revlog", "graves"]


class ExportLegacy(unittest.TestCase):
    """model-ops.md L6."""

    def test_whole_collection_note_types_of_exported_notes_and_replacement(self):
        ids = {"db_exec": {"sql": "SELECT (SELECT id FROM notetypes WHERE name = 'QA'), "
                                  "(SELECT id FROM notetypes WHERE name = 'QB'), "
                                  "(SELECT id FROM decks WHERE name = 'A'), "
                                  "(SELECT id FROM deck_config WHERE name = 'P')"}}
        cols = ["ver", "conf", "models", "decks", "dconf", "tags"]
        steps = [
            ids,
            {"export": LEGACY},
            {"observe": [{"package_col": {"name": "x.apkg", "columns": cols}},
                         {"legacy_json": {"notetypes": ["QA"], "decks": ["A"], "presets": ["P"]}}]},
            {"add_note": {"notetype": "QB", "deck": "A", "fields": ["f", "b"]}},
            {"export": LEGACY},
            {"observe": [{"package_col": {"name": "x.apkg", "columns": ["models"]}},
                         {"legacy_json": {"notetypes": ["QA", "QB"]}}]},
        ]
        setup = {"notetypes": [NOTETYPE, QB]}
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario("t/model", "UTC", START, steps, notes=2, setup=setup))
        [[qa, qb, did, conf_id]] = rows(out[0])
        # Every note of the collection is exported (limit none).
        self.assertEqual((out[1], out[4]), ({"export": {"notes": 2}}, {"export": {"notes": 3}}))
        first, objects = out[2]["package_col"], out[2]["legacy_json"]
        self.assertEqual(sorted(first), sorted(cols))
        # The exported collection is at schema 11 (schema11-json S12).
        self.assertEqual(first["ver"], 11)
        self.assertIs(type(first["ver"]), int)
        self.assertEqual((type(first["conf"]), type(first["tags"])), (dict, dict))
        # Only the note types of exported notes, keyed by decimal id, each
        # equal to its legacy_json object (schema11-json S14.1).
        self.assertEqual(first["models"], {str(qa): objects["notetypes"]["QA"]})
        self.assertEqual(first["decks"][str(did)]["name"], "A")
        # Presets are included; f32 values keep their float kind (L7 numbers as L1).
        preset = first["dconf"][str(conf_id)]
        self.assertEqual(preset["name"], "P")
        self.assertEqual(preset["new"]["delays"], [1.0, 10.0])
        self.assertEqual([type(x) for x in preset["new"]["delays"]], [float, float])
        self.assertIs(type(preset["new"]["perDay"]), int)
        # A second export of the same name replaces the file.
        second = out[5]
        self.assertEqual(second["package_col"]["models"],
                         {str(qa): second["legacy_json"]["notetypes"]["QA"], str(qb): second["legacy_json"]["notetypes"]["QB"]})

    def test_exporting_changes_no_stored_row_once_a_rollover_hour_is_stored(self):
        dump = [{"db_exec": {"sql": f"SELECT * FROM {t} ORDER BY 1"}} for t in TABLES]
        steps = dump + [{"advance": "1h"}, {"export": LEGACY}] + dump
        setup = {"config": {"fsrs": True, "rollover": 4}}
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario("t/model", "UTC", START, steps, notes=2, setup=setup))
        n = len(TABLES)
        self.assertEqual(out[n + 1], {"export": {"notes": 2}})
        self.assertEqual(out[:n], out[n + 2:])
        self.assertEqual(len(rows(out[TABLES.index("cards")])), 2)

    def test_without_a_rollover_hour_export_stores_the_default(self):
        # Not in L6 yet (reported to the spec writer): exporting reads today,
        # and Anki then stores the default rollover hour 4 as a local change
        # when the collection has none. Nothing else changes.
        t = epoch(START, "UTC")
        config = {"db_exec": {"sql": "SELECT key, usn, mtime_secs, CAST(val AS TEXT) FROM config ORDER BY key"}}
        dump = [{"db_exec": {"sql": f"SELECT * FROM {table} ORDER BY 1"}} for table in TABLES if table != "config"]
        steps = [config] + dump + [{"advance": "1h"}, {"export": LEGACY}, config] + dump
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario("t/model", "UTC", START, steps, notes=1))
        n = len(dump)
        before, after = rows(out[0]), rows(out[n + 3])
        self.assertNotIn("rollover", [r[0] for r in before])
        self.assertEqual(sorted(after), sorted(before + [["rollover", -1, t + 3600, "4"]]))
        self.assertEqual(out[1:n + 1], out[n + 4:])

    def test_malformed_arguments_fail_before_any_collection(self):
        bad = [
            None, "x.apkg", ["x.apkg"], {"format": "apkg", "legacy": True}, {"name": "x.apkg", "legacy": True},
            dict(LEGACY, format="colpkg"), dict(LEGACY, legacy=False), dict(LEGACY, legacy=1), dict(LEGACY, legacy="true"),
            dict(LEGACY, extra=1), dict(LEGACY, with_media=False),
        ] + [dict(LEGACY, name=name) for name in (
            "", "x", "x.zip", "x.APKG", ".x.apkg", "_x.apkg", "-x.apkg", "a/x.apkg", "../x.apkg", "x y.apkg",
            "x.apkg\n", "x.apkg/", "é.apkg", 1, None)]
        with WorkerProcess("UTC") as w:
            for args in bad:
                resp = run(w, [{"observe": ["ids"]}, {"export": args}])
                with self.subTest(args=args):
                    self.assertEqual((resp["error"]["kind"], resp["error"].get("step")), ("invalid_scenario", 1), resp)
            # Names the schema's package_name allows.
            for name in ("x.apkg", "X-1_a.b.apkg", "9.apkg", "a..apkg"):
                out = run_ok(self, w, scenario("t/model", "UTC", START, [{"export": dict(LEGACY, name=name)}], notes=1))
                with self.subTest(name=name):
                    self.assertEqual(out, [{"export": {"notes": 1}}])

    def test_other_export_forms_stay_unsupported(self):
        # The package area's draft forms (dsl.md D7.2): unsupported before any
        # collection, as when the driver had no export at all.
        drafts = [{"format": "apkg", "name": "x.apkg"}, {"format": "colpkg", "name": "x.colpkg"},
                  {"format": "apkg", "name": "x.apkg", "with_media": True}]
        with WorkerProcess("UTC") as w:
            for args in drafts:
                resp = run(w, [{"observe": ["ids"]}, {"export": args}])
                with self.subTest(args=args):
                    self.assertEqual(resp["error"], {"kind": "unsupported_op", "op": "export", "step": 1}, resp)


def _col_db(path: str, ver: int, models: str = "{}") -> None:
    con = sqlite3.connect(path)
    con.execute("CREATE TABLE col (id INTEGER PRIMARY KEY, crt INTEGER, mod INTEGER, scm INTEGER, ver INTEGER, "
                "dty INTEGER, usn INTEGER, ls INTEGER, conf TEXT, models TEXT, decks TEXT, dconf TEXT, tags TEXT)")
    con.execute("INSERT INTO col VALUES (1, 0, 0, 0, ?, 0, 0, 0, '{\"k\": 1}', ?, '{}', '{}', '{\"t\": 0}')", (ver, models))
    con.commit()
    con.close()


class PackageCol(unittest.TestCase):
    """model-ops.md L7, on the package an export step wrote and on packages
    the test writes into a handed-off session directory (dsl.md D9.2)."""

    # export, hand the session over, read the package on the other side.
    STEPS = [
        {"export": LEGACY},
        {"handoff": "meguri"},
        {"observe": [{"package_col": {"name": "x.apkg", "columns": ["ver", "models", "conf", "tags"]}}]},
    ]

    def setUp(self):
        self.root = tempfile.mkdtemp(prefix="handoff-", dir=os.environ.get("ORACLE_SCRATCH_ROOT"))
        self.work = tempfile.mkdtemp(prefix="pkg-", dir=os.environ.get("ORACLE_SCRATCH_ROOT"))
        self.sc = scenario("t/pkg", "UTC", START, self.STEPS, notes=1)

    def tearDown(self):
        shutil.rmtree(self.root, ignore_errors=True)
        shutil.rmtree(self.work, ignore_errors=True)

    def handed_package(self, w):
        resp = w.request({"cmd": "run", "scenario": self.sc, "steps": [0, 1], "handoff": "t/pkg"})
        self.assertTrue(resp["ok"], resp)
        self.assertEqual(resp["observations"]["steps"], [{"export": {"notes": 1}}])
        return resp["state"], os.path.join(self.root, "t/pkg/x.apkg")

    def read(self, w, state):
        return w.request({"cmd": "run", "scenario": self.sc, "steps": [2, 3], "resume": state})

    def write_package(self, path, entries):
        """entries: zip entry name -> bytes, or (ver, models) for a collection."""
        with zipfile.ZipFile(path, "w") as z:
            for name, content in entries.items():
                if isinstance(content, tuple):
                    db = os.path.join(self.work, name)
                    _col_db(db, *content)
                    z.write(db, name)
                    os.remove(db)
                else:
                    z.writestr(name, content)

    def test_the_exported_package_crosses_a_handoff(self):
        with WorkerProcess("UTC", extra_env={"ORACLE_HANDOFF": self.root}) as w:
            state, path = self.handed_package(w)
            # L6's layout: meta names version 2 (field 1 varint), the schema-11
            # collection is collection.anki21, with a placeholder and media.
            with zipfile.ZipFile(path) as z:
                self.assertEqual(z.read("meta"), b"\x08\x02")
                self.assertLessEqual({"collection.anki21", "collection.anki2", "media"}, set(z.namelist()))
            resp = self.read(w, state)
        self.assertTrue(resp["ok"], resp)
        [observed] = resp["observations"]["steps"]
        value = observed["package_col"]
        self.assertEqual(sorted(value), ["conf", "models", "tags", "ver"])
        self.assertEqual(value["ver"], 11)
        [nt] = value["models"].values()
        self.assertEqual(nt["name"], "QA")
        self.assertEqual(str(nt["id"]), next(iter(value["models"])))

    def test_entry_chosen_by_meta_version_or_entry_presence(self):
        both = {"collection.anki2": (1,), "collection.anki21": (2,)}
        cases = [
            ({"meta": b"\x08\x01", **both}, 1),
            ({"meta": b"\x08\x02", **both}, 2),
            # A later occurrence of a field replaces the earlier (protobuf).
            ({"meta": b"\x08\x01\x08\x02", **both}, 2),
            (dict(both), 2),
            ({"collection.anki2": (1,)}, 1),
        ]
        models = '{"5": {"a": 1.0, "b": 2, "c": 1e3, "d": [0.5, -3], "e": {"z": null}}}'
        with WorkerProcess("UTC", extra_env={"ORACLE_HANDOFF": self.root}) as w:
            state, path = self.handed_package(w)
            for entries, ver in cases:
                entries = {k: (v[0], models) if isinstance(v, tuple) else v for k, v in entries.items()}
                self.write_package(path, entries)
                resp = self.read(w, state)
                with self.subTest(entries=sorted(entries)):
                    self.assertTrue(resp["ok"], resp)
                    value = resp["observations"]["steps"][0]["package_col"]
                    self.assertEqual(value, {"ver": ver, "models": {"5": {"a": 1.0, "b": 2, "c": 1000.0,
                                                                         "d": [0.5, -3], "e": {"z": None}}},
                                             "conf": {"k": 1}, "tags": {"t": 0}})
                    nt = value["models"]["5"]
                    self.assertEqual([type(nt[k]) for k in "abc"], [float, int, float])
                    self.assertEqual([type(x) for x in nt["d"]], [float, int])

    def test_versions_without_a_schema_11_collection_are_invalid(self):
        both = {"collection.anki2": (1,), "collection.anki21": (2,), "collection.anki21b": b"x"}
        with WorkerProcess("UTC", extra_env={"ORACLE_HANDOFF": self.root}) as w:
            state, path = self.handed_package(w)
            for meta in (b"\x08\x03", b"", b"\x08\x00", b"\x08\x07"):
                self.write_package(path, {"meta": meta, **both})
                resp = self.read(w, state)
                with self.subTest(meta=meta):
                    self.assertEqual((resp["error"]["kind"], resp["error"].get("step")), ("invalid_scenario", 2), resp)

    def test_packages_that_do_not_read_are_run_errors(self):
        cases = [
            ("not a zip", None),
            ("meta not a message", {"meta": b"\xff", "collection.anki21": (2, "{}")}),
            ("no such entry", {"meta": b"\x08\x02", "collection.anki2": (1, "{}")}),
            ("no entry at all", {"media": b"{}"}),
            ("entry not SQLite", {"collection.anki21": b"not sqlite"}),
            ("models not JSON", {"collection.anki21": (2, "{")}),
            ("repeated member", {"collection.anki21": (2, '{"5": {"a": 1, "a": 2}}')}),
        ]
        with WorkerProcess("UTC", extra_env={"ORACLE_HANDOFF": self.root}) as w:
            state, path = self.handed_package(w)
            for label, entries in cases:
                if entries is None:
                    with open(path, "wb") as f:
                        f.write(b"PK not really")
                else:
                    self.write_package(path, entries)
                resp = self.read(w, state)
                with self.subTest(label):
                    self.assertFalse(resp["ok"], resp)
                    self.assertEqual((resp["error"]["kind"], resp["error"].get("step")), ("internal_error", 2), resp)

    def test_observing_twice_reads_the_same_and_changes_no_row(self):
        dump = [{"db_exec": {"sql": f"SELECT * FROM {t} ORDER BY 1"}} for t in TABLES]
        observe = {"observe": [{"package_col": {"name": "x.apkg", "columns": ["ver", "conf", "models", "decks", "dconf", "tags"]}}]}
        steps = [{"export": LEGACY}] + dump + [observe, observe] + dump
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario("t/model", "UTC", START, steps, notes=1))
        n = len(TABLES)
        self.assertEqual(out[n + 1], out[n + 2])
        self.assertEqual(out[1:n + 1], out[n + 3:])

    def test_unknown_files_and_malformed_requests(self):
        ok = {"name": "x.apkg", "columns": ["ver"]}
        bad = [None, "x.apkg", ["x.apkg"], {}, {"name": "x.apkg"}, {"columns": ["ver"]}, dict(ok, columns=[]),
               dict(ok, columns="ver"), dict(ok, columns=["ver", "ver"]), dict(ok, columns=["mod"]), dict(ok, columns=[1]),
               dict(ok, columns=["VER"]), dict(ok, name="x.zip"), dict(ok, name="../x.apkg"), dict(ok, name="x.apkg\n"),
               dict(ok, name=1), dict(ok, extra=1)]
        with WorkerProcess("UTC") as w:
            # A file no export step wrote (and one an earlier run wrote) is unknown.
            run_ok(self, w, scenario("t/model", "UTC", START, [{"export": dict(LEGACY, name="y.apkg")}]))
            for name in ("nope.apkg", "y.apkg"):
                resp = run(w, [{"export": LEGACY}, {"observe": [{"package_col": dict(ok, name=name)}]}])
                with self.subTest(name=name):
                    self.assertEqual(resp["error"], {"kind": "unknown_ref", "ref": name, "step": 1}, resp)
            for args in bad:
                resp = run(w, [{"export": LEGACY}, {"observe": [{"package_col": args}]}])
                with self.subTest(args=args):
                    self.assertEqual((resp["error"]["kind"], resp["error"].get("step")), ("invalid_scenario", 1), resp)


if __name__ == "__main__":
    unittest.main()
