# SPDX-License-Identifier: AGPL-3.0-or-later
"""Deck and preset ops and the decks and deck_tree observations
(specs/conformance/deck-ops.md K1-K12) through real workers. Expected values
come from the specs (decks/decks.md, decks/deck-presets.md), the scenario
clock and rows read back with SQL, never from the driver."""

from __future__ import annotations

import unittest

from driver.harness import WorkerProcess
from driver.tests.test_worker import epoch, run_ok

START = "2030-01-15T10:00:00"
T = epoch(START, "UTC")
# deck-ops.md K13: a filtered deck row as Anki's build leaves it.
FILTERED = (
    "INSERT INTO decks (id, name, mtime_secs, usn, common, kind) VALUES (100, {name}, 0, 0, "
    "X'08011001', X'12130801120a0a066465636b3a78106428d804383c')"
)
# deck-presets.md P4: the FSRS-6 default parameters a new preset holds.
FSRS6 = [0.212, 1.2931, 2.3065, 8.2956, 6.4133, 0.8334, 3.0194, 0.001, 1.8722, 0.1666, 0.796,
         1.4835, 0.0614, 0.2629, 1.6483, 0.6014, 1.8729, 0.5425, 0.0912, 0.0658, 0.1542]
PAIR = {"name": "Pair", "fields": ["F", "B"], "templates": [{"name": "C", "front": "{{F}}", "back": "{{B}}"}]}


def scenario(steps, setup=None):
    return {"id": "t/decks", "clock": {"tz": "UTC", "start": START}, "setup": setup or {}, "steps": steps}


def run(worker, steps, setup=None):
    return worker.request({"cmd": "run", "scenario": scenario(steps, setup)})


def rows(step):
    return step["db_exec"]["rows"]


def error_at(test, resp, kind, step):
    test.assertFalse(resp["ok"], resp)
    test.assertEqual((resp["error"]["kind"], resp["error"].get("step")), (kind, step), resp)


class Naming(unittest.TestCase):
    def test_decks_and_presets_are_named_by_their_exact_stored_names(self):
        steps = [
            {"db_exec": {"sql": FILTERED.format(name="'X' || char(31) || 'Y'")}},
            {"set_current_deck": {"deck": "X::Y"}},
            {"db_exec": {"sql": "SELECT CAST(val AS TEXT) FROM config WHERE KEY = 'curDeck'"}},
            {"add_deck": {"name": "  Typed :: Sub "}},
            {"add_note": {"notetype": "Pair", "deck": "Typed::Sub", "fields": ["q", "a"]}},
            {"observe": ["card"]},
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario(steps, {"decks": ["Lang::Es"], "notetypes": [PAIR]}))
            missing_deck = run(w, [{"rename_deck": {"deck": "lang::es", "name": "Z"}}], {"decks": ["Lang::Es"]})
            missing_note_deck = run(w, [{"add_note": {"notetype": "Pair", "deck": "Nope", "fields": ["q"]}}],
                                    {"notetypes": [PAIR]})
            missing_preset = run(w, [{"clone_preset": {"preset": "default", "name": "C"}}])
            twice = run(w, [
                {"db_exec": {"sql": "INSERT INTO deck_config SELECT 5, name, mtime_secs, usn, config "
                                    "FROM deck_config WHERE id = 1"}},
                {"rename_preset": {"preset": "Default", "name": "D"}},
            ])
        # A db_exec row is found by its stored name in human form (K1, record-ops W1).
        self.assertEqual(rows(out[2]), [["100"]])
        # add_deck reports the stored name (decks D1), and add_note takes it.
        self.assertEqual(out[3], {"add_deck": {"name": "Typed::Sub"}})
        self.assertEqual(out[5]["card"]["#c1"]["did"], "Typed::Sub")
        self.assertEqual(missing_deck["error"], {"kind": "unknown_ref", "ref": "lang::es", "step": 0})
        self.assertEqual(missing_note_deck["error"], {"kind": "unknown_ref", "ref": "Nope", "step": 0})
        self.assertEqual(missing_preset["error"], {"kind": "unknown_ref", "ref": "default", "step": 0})
        error_at(self, twice, "invalid_scenario", 1)

    def test_arguments_are_checked(self):
        bad = [
            {"add_deck": {}},
            {"add_deck": {"name": 1}},
            {"add_deck": {"name": "x", "deck": "A"}},
            {"rename_deck": {"deck": "A"}},
            {"rename_deck": {"deck": "", "name": "B"}},
            {"reparent_decks": {"decks": [], "parent": ""}},
            {"reparent_decks": {"decks": ["A"]}},
            {"reparent_decks": {"decks": ["A"], "parent": None}},
            {"delete_decks": {"decks": "A"}},
            {"set_deck_collapsed": {"decks": ["A"], "scope": "home", "collapsed": True}},
            {"set_deck_collapsed": {"decks": ["A"], "scope": "study", "collapsed": 1}},
            {"set_current_deck": "A"},
            {"add_preset": {"name": None}},
            {"clone_preset": {"preset": "Default"}},
            {"rename_preset": {"preset": "Default", "name": 2}},
            {"remove_preset": {"preset": "Default"}},
            {"set_deck_preset": {"deck": "A", "preset": "Default", "subtree": "yes"}},
            {"observe": ["decks(A)"]},
            {"observe": [{"deck_tree": {"x": 1}}]},
        ]
        with WorkerProcess("UTC") as w:
            for step in bad:
                resp = run(w, [{"observe": ["ids"]}, step], {"decks": ["A"]})
                with self.subTest(step=step):
                    error_at(self, resp, "invalid_scenario", 1)
            same = run(w, [{"observe": ["ids"]}, {"remove_preset": {"preset": "P", "fallback": "P"}}])
        # Checked when the scenario loads: no collection, no step run.
        self.assertEqual(same["error"]["kind"], "invalid_scenario", same)
        self.assertEqual(same["error"].get("step"), 1, same)


class DeckOps(unittest.TestCase):
    def test_create_rename_and_their_refusals(self):
        steps = [
            {"db_exec": {"sql": FILTERED.format(name="'F'")}},
            {"add_deck": {"name": "A::B::C"}},
            {"add_deck": {"name": "a::b"}},
            {"add_deck": {"name": "F::sub"}},
            {"add_deck": {"name": "::".join(str(i) for i in range(1, 13))}},
            {"rename_deck": {"deck": "A::B::C", "name": "F::C"}},
            {"rename_deck": {"deck": "A::B::C", "name": "::".join(str(i) for i in range(1, 13))}},
            {"rename_deck": {"deck": "A::B::C", "name": "a::Cee"}},
            {"db_exec": {"sql": "SELECT id, replace(name, char(31), '::') FROM decks ORDER BY id"}},
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario(steps))
        # decks D3: missing parents first, nearest first, ids from the frozen clock.
        self.assertEqual(out[1], {"add_deck": {"name": "A::B::C"}})
        # D5: an existing name, ignoring case, returns that deck with its spelling.
        self.assertEqual(out[2], {"add_deck": {"name": "A::B"}})
        self.assertEqual([out[i] for i in (3, 4)], [{"add_deck": {"error": "filtered_parent"}},
                                                     {"add_deck": {"error": "too_deep"}}])
        self.assertEqual([out[i] for i in (5, 6)], [{"rename_deck": {"error": "filtered_parent"}},
                                                     {"rename_deck": {"error": "too_deep"}}])
        # D3 case adoption: the rename takes A's spelling.
        self.assertEqual(out[7], {"rename_deck": {"name": "A::Cee"}})
        self.assertEqual(rows(out[8]), [[1, "Default"], [100, "F"], [T * 1000, "A::B"], [T * 1000 + 1, "A"],
                                        [T * 1000 + 2, "A::Cee"]])

    def test_move_delete_collapse_and_select(self):
        steps = [
            {"db_exec": {"sql": FILTERED.format(name="'F'")}},
            {"advance": "1m"},
            {"reparent_decks": {"decks": ["x::y", "a"], "parent": ""}},
            {"reparent_decks": {"decks": ["y"], "parent": "F"}},
            {"set_deck_collapsed": {"decks": ["a", "x"], "scope": "study", "collapsed": False}},
            {"set_deck_collapsed": {"decks": ["y"], "scope": "browser", "collapsed": False}},
            {"set_current_deck": {"deck": "y"}},
            {"observe": ["decks"]},
            {"db_exec": {"sql": "SELECT CAST(val AS TEXT) = CAST((SELECT id FROM decks WHERE name = 'y') AS TEXT) "
                                "FROM config WHERE KEY = 'curDeck'"}},
            {"delete_decks": {"decks": ["x", "y"]}},
            {"db_exec": {"sql": "SELECT replace(name, char(31), '::') FROM decks ORDER BY id"}},
        ]
        setup = {"decks": ["x::y", "a"], "notetypes": [PAIR],
                 "notes": [{"notetype": "Pair", "deck": "x", "fields": ["1"]},
                           {"notetype": "Pair", "deck": "x::y", "fields": ["2"]},
                           {"notetype": "Pair", "deck": "x::y", "fields": ["3"]}]}
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario(steps, setup))
        # decks D7: x::y moves to the top level as y; a already is there.
        self.assertEqual(out[2], {"reparent_decks": {"count": 1}})
        self.assertEqual(out[3], {"reparent_decks": {"error": "filtered_parent"}})
        decks = out[7]["decks"]
        self.assertEqual(sorted(decks), ["Default", "F", "a", "x", "y"])
        # K11 values: D4's new decks (both flags set, preset 1), the scope's flag only
        # (D10), stamps of written decks at now (the clock moved 1m).
        self.assertEqual(decks["Default"], {"id": 1, "filtered": False, "config_id": 1, "study_collapsed": True,
                                            "browser_collapsed": True, "mtime_secs": 0, "usn": 0})
        self.assertEqual(decks["F"], {"id": 100, "filtered": True, "config_id": None, "study_collapsed": True,
                                      "browser_collapsed": True, "mtime_secs": 0, "usn": 0})
        self.assertEqual((decks["a"]["study_collapsed"], decks["a"]["browser_collapsed"]), (False, True))
        self.assertEqual((decks["x"]["study_collapsed"], decks["x"]["browser_collapsed"]), (False, True))
        self.assertEqual((decks["y"]["study_collapsed"], decks["y"]["browser_collapsed"]), (True, False))
        self.assertEqual({k: (v["mtime_secs"], v["usn"]) for k, v in decks.items() if k in ("a", "x", "y")},
                         {"a": (T + 60, -1), "x": (T + 60, -1), "y": (T + 60, -1)})
        self.assertEqual(rows(out[8]), [[1]])
        # D8: the count is the cards removed, the subtree's included.
        self.assertEqual(out[9], {"delete_decks": {"count": 3}})
        self.assertEqual(rows(out[10]), [["Default"], ["F"], ["a"]])


class Observations(unittest.TestCase):
    def test_deck_tree_in_unicase_order_with_the_default_deck_hidden(self):
        steps = [
            {"observe": ["deck_tree"]},
            {"delete_decks": {"decks": ["b", "a"]}},
            {"observe": ["deck_tree"]},
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario(steps, {"decks": ["b::Z", "b::y", "a"]}))
        leaf = lambda name: {"name": name, "children": []}  # noqa: E731
        # decks D12: unicase name order, the empty default deck hidden while
        # another top-level deck exists, shown when it is the only one.
        self.assertEqual(out[0]["deck_tree"], [leaf("a"), {"name": "b", "children": [leaf("y"), leaf("Z")]}])
        self.assertEqual(out[2]["deck_tree"], [leaf("Default")])


class PresetOps(unittest.TestCase):
    def test_add_clone_rename(self):
        steps = [
            {"add_preset": {"name": "New"}},
            {"clone_preset": {"preset": "New", "name": "Copy"}},
            {"observe": [{"legacy_json": {"presets": ["Default", "New"]}}]},
            {"db_exec": {"sql": "SELECT name, mtime_secs, usn, hex(config) FROM deck_config ORDER BY id"}},
            {"advance": "1m"},
            {"rename_preset": {"preset": "Copy", "name": "Copy"}},
            {"db_exec": {"sql": "SELECT (SELECT mod FROM col), mtime_secs FROM deck_config WHERE name = 'Copy'"}},
            {"rename_preset": {"preset": "Copy", "name": "copy"}},
            {"db_exec": {"sql": "SELECT (SELECT mod FROM col), mtime_secs, usn FROM deck_config WHERE name = 'copy'"}},
            {"observe": ["decks"]},
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario(steps))
        legacy = out[2]["legacy_json"]["presets"]
        # deck-presets P4: the built-in defaults with the FSRS-6 parameters.
        self.assertEqual(legacy["New"]["fsrsParams6"], FSRS6)
        same = {"id", "name", "mod", "usn", "fsrsParams6"}
        self.assertEqual({k: v for k, v in legacy["New"].items() if k not in same},
                         {k: v for k, v in legacy["Default"].items() if k not in same})
        [default, new, copy] = rows(out[3])
        # P4, P5: stamped at now; the clone's bytes equal its source's.
        self.assertEqual((new[:3], copy[:3]), (["New", T, -1], ["Copy", T, -1]))
        self.assertEqual(copy[3], new[3])
        # P6: the same name writes nothing (col.mod stays at the clone's instant);
        # another name stamps.
        self.assertEqual(rows(out[6]), [[T * 1000, T]])
        self.assertEqual(rows(out[8]), [[(T + 60) * 1000, T + 60, -1]])
        # K8: no deck changes.
        self.assertEqual(out[9]["decks"]["Default"]["config_id"], 1)

    def test_assign_and_remove(self):
        steps = [
            {"db_exec": {"sql": FILTERED.format(name="'L' || char(31) || 'F'")}},
            {"add_preset": {"name": "P"}},
            {"add_preset": {"name": "Q"}},
            {"set_deck_preset": {"deck": "L", "preset": "P", "subtree": True}},
            {"set_deck_preset": {"deck": "L::F", "preset": "P"}},
            {"set_deck_preset": {"deck": "S", "preset": "Q"}},
            {"observe": ["decks"]},
            {"db_exec": {"sql": "UPDATE col SET ls = scm"}},
            {"advance": "1m"},
            {"remove_preset": {"preset": "Default", "fallback": "Q"}},
            {"db_exec": {"sql": "SELECT scm = ls FROM col"}},
            {"remove_preset": {"preset": "P", "fallback": "Q"}},
            {"observe": ["decks"]},
            {"db_exec": {"sql": "SELECT scm, (SELECT count() FROM graves), (SELECT group_concat(name) FROM deck_config) "
                                "FROM col"}},
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario(steps, {"decks": ["L::A", "S"]}))
        before, after = out[6]["decks"], out[12]["decks"]
        p, q = before["L"]["config_id"], before["S"]["config_id"]
        # deck-presets P9: the subtree's normal decks; the filtered one is left alone and refused.
        self.assertEqual(before["L::A"]["config_id"], p)
        self.assertEqual(before["L::F"]["config_id"], None)
        self.assertEqual(out[4], {"set_deck_preset": {"error": "filtered_deck"}})
        self.assertNotEqual(p, q)
        # P8: the default preset is refused and nothing changes (scm still equals ls).
        self.assertEqual(out[9], {"remove_preset": {"error": "default_preset"}})
        self.assertEqual(rows(out[10]), [[1]])
        # Decks of the removed preset get the fallback, stamped; scm is now; no grave.
        self.assertEqual({k: after[k]["config_id"] for k in ("L", "L::A", "S", "Default")},
                         {"L": q, "L::A": q, "S": q, "Default": 1})
        self.assertEqual((after["L"]["mtime_secs"], after["S"]["mtime_secs"]), (T + 60, T))
        self.assertEqual(rows(out[13]), [[(T + 60) * 1000, 0, "Default,Q"]])


if __name__ == "__main__":
    unittest.main()
