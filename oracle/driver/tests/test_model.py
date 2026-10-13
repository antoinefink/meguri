# SPDX-License-Identifier: AGPL-3.0-or-later
"""Model ops (specs/conformance/model-ops.md L1-L3) through real workers:
the legacy_json observation, the legacy_update op, suspend, and the db_exec
uses the model scenarios rely on. Expected values come from the scenario (its
clock, patches and SQL), SQLite's own functions and the spec texts, never from
the driver."""

from __future__ import annotations

import unittest

from driver.harness import WorkerProcess
from driver.tests.test_worker import epoch, run_ok, scenario

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


if __name__ == "__main__":
    unittest.main()
