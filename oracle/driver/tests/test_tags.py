# SPDX-License-Identifier: AGPL-3.0-or-later
"""Tag ops and the tag tree (specs/conformance/tag-ops.md G1-G10) through real
workers. Expected values come from the arguments and the behavior specs
(specs/tags/tags.md, specs/tags/flags-and-marked.md), never from the driver."""

from __future__ import annotations

import unittest

from driver.harness import WorkerProcess
from driver.tests.test_worker import epoch, run_ok

START = "2030-01-15T10:00:00"
PLAIN = {"name": "Plain", "fields": ["Text"], "templates": [{"name": "Card", "front": "{{Text}}", "back": "{{Text}}"}]}
NOTE_TAGS = {"db_exec": {"sql": "SELECT tags FROM notes ORDER BY id"}}
TAG_ROWS = {"db_exec": {"sql": "SELECT tag, collapsed FROM tags ORDER BY tag"}}
COL_MOD = {"db_exec": {"sql": "SELECT mod FROM col"}}


def note(*tags):
    return {"add_note": {"notetype": "Plain", "fields": ["x"], "tags": list(tags)}}


def scenario(steps):
    return {"id": "t/tags", "clock": {"tz": "UTC", "start": START}, "setup": {"notetypes": [PLAIN]}, "steps": steps}


def rows(step):
    return step["db_exec"]["rows"]


def refused(test, worker, steps, op_index):
    """The run fails at op_index with invalid_scenario."""
    resp = worker.request({"cmd": "run", "scenario": scenario(steps)})
    test.assertEqual((resp.get("error", {}).get("kind"), resp.get("error", {}).get("step")),
                     ("invalid_scenario", op_index), resp)


class NoteTags(unittest.TestCase):
    def test_bulk_add_and_remove_count_the_notes_written(self):
        steps = [
            note("a"), note("b"), note("c"), note("d"),
            {"delete_notes": {"notes": ["#n4"]}},
            {"add_note_tags": {"notes": ["#n1", "#n2", "#n4"], "tags": "x"}},
            {"add_note_tags": {"notes": ["#n1"], "tags": "x"}},
            NOTE_TAGS,
            {"remove_note_tags": {"notes": ["#n1", "#n3"], "tags": "X"}},
            NOTE_TAGS,
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario(steps))
        # T10: a deleted note is skipped, a note that already has the tag is
        # untouched; removal matches in any case.
        self.assertEqual(out[5:7], [{"add_note_tags": {"count": 2}}, {"add_note_tags": {"count": 0}}])
        self.assertEqual(rows(out[7]), [[" a x "], [" b x "], [" c "]])
        self.assertEqual(out[8], {"remove_note_tags": {"count": 1}})
        self.assertEqual(rows(out[9]), [[" a "], [" b x "], [" c "]])

    def test_delete_takes_descendants_from_notes_and_registry(self):
        steps = [
            note("a", "keep"), note("a::b"), note("ab"),
            {"set_tag_collapsed": {"tags": ["lone"], "collapsed": False}},
            {"delete_tags": {"tags": "A"}},
            NOTE_TAGS,
            {"delete_tags": {"tags": "lone"}},
            TAG_ROWS,
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario(steps))
        # T9: a registry-only name counts 0 notes and still loses its row.
        self.assertEqual((out[4], out[6]), ({"delete_tags": {"count": 2}}, {"delete_tags": {"count": 0}}))
        self.assertEqual(rows(out[5]), [[" keep "], [""], [" ab "]])
        self.assertEqual(rows(out[7]), [["ab", 1], ["keep", 1]])


class Rename(unittest.TestCase):
    def test_rename_counts_matching_notes_and_refuses_bad_names_without_a_change(self):
        steps = [
            note("foo", "other"), note("foo::kid"), note("food"),
            {"advance": "1h"},
            {"rename_tag": {"from": "FOO", "to": "bar"}},
            NOTE_TAGS,
            COL_MOD,
            {"advance": "1h"},
            {"rename_tag": {"from": "bar", "to": "has space"}},
            {"rename_tag": {"from": "bar", "to": ""}},
            {"rename_tag": {"from": "bar", "to": "\t "}},
            {"rename_tag": {"from": "bar", "to": "a　b"}},
            {"rename_tag": {"from": "nothing", "to": "else"}},
            NOTE_TAGS,
            COL_MOD,
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario(steps))
        self.assertEqual(out[4], {"rename_tag": {"count": 2}})
        self.assertEqual(rows(out[5]), [[" bar other "], [" bar::kid "], [" food "]])
        self.assertEqual(rows(out[6]), [[(epoch(START, "UTC") + 3600) * 1000]])
        # T7 step 1 refuses; step 2 counts 0 when no note matches.
        self.assertEqual(out[8:12], [{"rename_tag": {"error": "invalid_name"}}] * 4)
        self.assertEqual(out[12], {"rename_tag": {"count": 0}})
        self.assertEqual((out[13], out[14]), (out[5], out[6]))


class Reparent(unittest.TestCase):
    def test_reparent_moves_onto_a_parent_or_the_top_level_and_skips_its_own_descendants(self):
        steps = [
            note("ra"), note("ra::kid"), note("rb"),
            {"reparent_tags": {"tags": ["ra"], "parent": "rb"}},
            NOTE_TAGS,
            {"reparent_tags": {"tags": ["rb"], "parent": "rb::ra"}},
            {"reparent_tags": {"tags": ["rb::ra"], "parent": ""}},
            NOTE_TAGS,
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario(steps))
        self.assertEqual(out[3], {"reparent_tags": {"count": 2}})
        self.assertEqual(rows(out[4]), [[" rb::ra "], [" rb::ra::kid "], [" rb "]])
        # T8: onto a descendant of itself is skipped; "" is the top level.
        self.assertEqual(out[5:7], [{"reparent_tags": {"count": 0}}, {"reparent_tags": {"count": 2}}])
        self.assertEqual(rows(out[7]), [[" ra "], [" ra::kid "], [" rb "]])


class Registry(unittest.TestCase):
    def test_clear_unused_counts_removed_rows_and_keeps_parents_of_used_tags(self):
        steps = [
            note("a::c", "z"),
            {"set_tag_collapsed": {"tags": ["a", "q"], "collapsed": False}},
            {"clear_unused_tags": {}},
            TAG_ROWS,
            {"clear_unused_tags": {}},
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario(steps))
        self.assertEqual(out[2], {"clear_unused_tags": {"count": 1}})
        self.assertEqual(rows(out[3]), [["a", 0], ["a::c", 1], ["z", 1]])
        self.assertEqual(out[4], {"clear_unused_tags": {"count": 0}})

    def test_set_collapsed_runs_once_per_name_in_order_with_no_output(self):
        steps = [
            {"set_tag_collapsed": {"tags": ["FOO", "foo::bar"], "collapsed": False}},
            TAG_ROWS,
            {"set_tag_collapsed": {"tags": ["foo::bar"], "collapsed": True}},
            TAG_ROWS,
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario(steps))
        # T3: FOO, registered by the first call, spells the second name.
        self.assertEqual((out[0], out[2]), ({}, {}))
        self.assertEqual(rows(out[1]), [["FOO", 0], ["FOO::bar", 0]])
        self.assertEqual(rows(out[3]), [["FOO", 0], ["FOO::bar", 1]])


class Flags(unittest.TestCase):
    def test_set_flag_counts_written_cards_and_refuses_values_above_seven(self):
        flags = {"db_exec": {"sql": "SELECT flags FROM cards ORDER BY id"}}
        steps = [
            note(), note(),
            {"db_exec": {"sql": "UPDATE cards SET flags = 8 WHERE id = (SELECT min(id) FROM cards)"}},
            {"set_flag": {"cards": ["#c1", "#c2"], "flag": 3}},
            {"set_flag": {"cards": ["#c1"], "flag": 3}},
            {"set_flag": {"cards": ["#c2"], "flag": 8}},
            flags,
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario(steps))
        # F2: the low three bits change, the others stay.
        self.assertEqual(out[3:6], [{"set_flag": {"count": 2}}, {"set_flag": {"count": 0}},
                                    {"set_flag": {"error": "invalid_flag"}}])
        self.assertEqual(rows(out[6]), [[11], [3]])


class TagTree(unittest.TestCase):
    def test_tree_nodes_with_implicit_parents_and_no_change(self):
        dump = {"observe": ["db_dump"]}
        steps = [
            {"observe": ["tag_tree"]},
            note("a::b", "c"),
            {"set_tag_collapsed": {"tags": ["c"], "collapsed": False}},
            dump,
            {"advance": "1h"},
            {"observe": ["tag_tree"]},
            {"observe": ["tag_tree()"]},
            {"observe": [{"tag_tree": None}]},
            dump,
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario(steps))
        self.assertEqual(out[0], {"tag_tree": []})
        # T13: `a` has no row, so it is an implicit parent, collapsed.
        leaf = {"name": "b", "collapsed": True, "children": []}
        tree = [{"name": "a", "collapsed": True, "children": [leaf]}, {"name": "c", "collapsed": False, "children": []}]
        self.assertEqual(out[5:8], [{"tag_tree": tree}] * 3)
        # Observing changes nothing, col.mod included.
        self.assertEqual(out[3], out[8])


class Arguments(unittest.TestCase):
    def test_wrong_shapes_and_types_are_invalid_scenarios(self):
        bad = {
            "add_note_tags": [None, {"notes": ["#n1"]}, {"notes": [], "tags": "x"}, {"notes": "#n1", "tags": "x"},
                              {"notes": ["#c1"], "tags": "x"}, {"notes": ["#n1"], "tags": ["x"]},
                              {"notes": ["#n1"], "tags": 1}, {"notes": ["#n1"], "tags": "x", "extra": 1}],
            "remove_note_tags": [{"notes": ["#n1"], "tags": None}, {"tags": "x"}],
            "delete_tags": [{}, {"tags": ["x"]}, {"tags": True}, "x"],
            "rename_tag": [{"from": "a"}, {"from": "", "to": "b"}, {"from": "a b", "to": "c"},
                           {"from": "a　b", "to": "c"}, {"from": 1, "to": "b"}, {"from": "a", "to": None}],
            "reparent_tags": [{"tags": "a", "parent": "b"}, {"tags": [], "parent": "b"}, {"tags": [""], "parent": "b"},
                              {"tags": ["a"], "parent": None}, {"tags": [1], "parent": "b"}, {"tags": ["a"]}],
            "clear_unused_tags": [None, [], {"x": 1}],
            "set_tag_collapsed": [{"tags": ["a"], "collapsed": 1}, {"tags": ["a"], "collapsed": "true"},
                                  {"tags": [], "collapsed": True}, {"tags": "a", "collapsed": True},
                                  {"tags": ["a"]}],
            "set_flag": [{"cards": ["#c1"], "flag": -1}, {"cards": ["#c1"], "flag": True},
                         {"cards": ["#c1"], "flag": 1.0}, {"cards": ["#c1"], "flag": "1"},
                         {"cards": ["#c1"], "flag": 2**32}, {"cards": [], "flag": 1},
                         {"cards": ["#n1"], "flag": 1}, {"cards": ["#c1"]}],
        }
        with WorkerProcess("UTC") as w:
            for op, cases in bad.items():
                for args in cases:
                    with self.subTest(op=op, args=args):
                        refused(self, w, [note("a"), {op: args}], 1)
            for spec in ("tag_tree(#n1)", {"tag_tree": {}}, {"tag_tree": "x"}):
                with self.subTest(observation=spec):
                    refused(self, w, [note("a"), {"observe": [spec]}], 1)
            unknown = w.request({"cmd": "run", "scenario": scenario([note("a"), {"add_note_tags": {"notes": ["#n2"], "tags": "x"}}])})
        self.assertEqual(unknown["error"], {"kind": "unknown_ref", "ref": "#n2", "step": 1})


if __name__ == "__main__":
    unittest.main()
