# SPDX-License-Identifier: AGPL-3.0-or-later
"""Note type structure ops (specs/conformance/notetype-structure-ops.md M1-M3)
through real workers: remove_notetype, restore_notetype with its
needs_stock_kind rejection, and the notetype_templates observation. Expected
values come from the scenario (its names, texts and SQL), SQLite and the spec
texts (structure-changes S12, restore-to-default D2 and D3, stock-kinds K1 and
K4 through test_notetypes.STOCK), never from the driver."""

from __future__ import annotations

import unittest

from driver.harness import WorkerProcess
from driver.tests.test_notetypes import STOCK, UNCHANGED, nt, rows, run
from driver.tests.test_worker import run_ok, scenario

START = "2030-01-15T10:00:00"
PAIR = nt("Pair", fields=["Front", "Back"], templates=[{"name": "Forward", "front": "{{Front}}", "back": "{{Back}}"},
                                                       {"name": "Reverse", "front": "{{Back}}", "back": "{{Front}}"}])
SOLO = nt("Solo", fields=["Text"], templates=[{"name": "Only", "front": "{{Text}}"}])
# original_stock_kind is protobuf field 9 of the note type config; a later
# occurrence wins (model/protobuf-records P2): 0x48 is its varint key.
SET_STOCK_KIND = "UPDATE notetypes SET config = CAST(config || X'48{:02x}' AS BLOB) WHERE name = ?"
NOTETYPE_ROW = "SELECT id, mtime_secs, usn FROM notetypes WHERE name = ?"


def setup(*notetypes, notes=()):
    return {"notetypes": list(notetypes), "notes": [{"notetype": n, "fields": f} for n, f in notes]}


def shape(structure):
    """A notetype_structure value as compared with test_notetypes.STOCK."""
    return (
        structure["kind"],
        structure["original_stock_kind"],
        [(f["name"], f["tag"], f["prevent_deletion"]) for f in structure["fields"]],
        [(t["name"], (t["requirement"]["kind"], t["requirement"]["fields"])) for t in structure["templates"]],
    )


class RemoveNotetype(unittest.TestCase):
    def test_notes_cards_rows_and_keys_go_with_graves_and_the_rest_stays(self):
        pair_cards = ("SELECT n.id, c.id FROM notes n JOIN cards c ON c.nid = n.id JOIN notetypes t ON t.id = n.mid "
                      "WHERE t.name = 'Pair' ORDER BY n.id, c.id")
        nt_keys = "SELECT KEY FROM config WHERE KEY LIKE '\\_nt\\_%' ESCAPE '\\' ORDER BY KEY"
        steps = [
            {"db_exec": {"sql": "INSERT INTO config (KEY, usn, mtime_secs, val) SELECT '_nt_' || id || '_x', 0, 0, "
                                "CAST('1' AS BLOB) FROM notetypes WHERE name IN ('Pair', 'Solo')"}},
            {"db_exec": {"sql": pair_cards}},
            {"db_exec": {"sql": "SELECT n.id, c.id FROM notes n JOIN cards c ON c.nid = n.id ORDER BY n.id, c.id"}},
            {"db_exec": {"sql": nt_keys}},
            {"db_exec": {"sql": "SELECT id FROM notetypes WHERE name = 'Pair'"}},
            {"remove_notetype": {"notetype": "Pair"}},
            {"db_exec": {"sql": "SELECT oid, type FROM graves ORDER BY oid, type"}},
            {"db_exec": {"sql": "SELECT n.id, c.id FROM notes n JOIN cards c ON c.nid = n.id ORDER BY n.id, c.id"}},
            {"db_exec": {"sql": nt_keys}},
            {"db_exec": {"sql": "SELECT (SELECT count() FROM notetypes WHERE name = 'Pair'), "
                                "(SELECT count() FROM fields WHERE ntid NOT IN (SELECT id FROM notetypes)), "
                                "(SELECT count() FROM templates WHERE ntid NOT IN (SELECT id FROM notetypes))"}},
        ]
        sc = scenario("t/ntrm", "UTC", START, steps,
                      setup=setup(PAIR, SOLO, notes=[("Pair", ["a", "b"]), ("Solo", ["s"]), ("Pair", ["c", ""])]))
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, sc)
        self.assertEqual(out[5], {})
        removed = rows(out[1])
        self.assertEqual(len({n for n, _ in removed}), 2)
        # structure-changes S12: a card grave (type 0) per card and a note grave (type 1) per note.
        expected_graves = [[cid, 0] for _, cid in removed] + [[nid, 1] for nid in {n for n, _ in removed}]
        self.assertEqual(rows(out[6]), sorted(expected_graves))
        self.assertEqual(rows(out[7]), [r for r in rows(out[2]) if r not in removed])
        # Every _nt_<Pair's id>_ key goes (its last deck and the custom one); Solo's stay.
        [[pair_id]] = rows(out[4])
        before = rows(out[3])
        self.assertIn([f"_nt_{pair_id}_x"], before)
        self.assertEqual(rows(out[8]), [k for k in before if not k[0].startswith(f"_nt_{pair_id}_")])
        self.assertEqual(sum(k[0].endswith("_x") for k in rows(out[8])), 1)
        self.assertEqual(rows(out[9]), [[0, 0, 0]])

    def test_the_rows_db_exec_wrote_are_read_and_names_are_exact(self):
        steps = [
            {"db_exec": {"sql": "UPDATE notetypes SET name = 'Renamed' WHERE name = 'Solo'"}},
            {"remove_notetype": {"notetype": "Renamed"}},
            {"db_exec": {"sql": "SELECT count() FROM notetypes WHERE name IN ('Solo', 'Renamed')"}},
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario("t/ntrm", "UTC", START, steps, setup=setup(PAIR, SOLO)))
            unknown = [run(w, [{"remove_notetype": {"notetype": name}}], setup=setup(PAIR)) for name in ("pair", "Nope")]
            bad = [run(w, [{"remove_notetype": args}], setup=setup(PAIR))
                   for args in ({}, {"notetype": 1}, {"notetype": ""}, {"notetype": "Pair", "x": 1}, "Pair")]
        self.assertEqual(out[1:], [{}, {"db_exec": {"rows": [[0]]}}])
        for name, resp in zip(("pair", "Nope"), unknown):
            self.assertEqual(resp["error"], {"kind": "unknown_ref", "ref": name, "step": 0}, resp)
        for resp in bad:
            self.assertEqual((resp["error"]["kind"], resp["error"]["step"]), ("invalid_scenario", 0), resp)


class RestoreNotetype(unittest.TestCase):
    def test_an_unknown_stored_kind_is_refused_before_anki_and_writes_nothing(self):
        steps = [
            {"db_exec": {"sql": SET_STOCK_KIND.format(0), "params": ["Pair"]}},
            {"db_exec": {"sql": SET_STOCK_KIND.format(7), "params": ["Solo"]}},
            {"observe": [{"notetype_structure": {"notetypes": ["Pair", "Solo"]}}]},
            {"advance": "1m"},
            {"db_exec": {"sql": UNCHANGED}},
            {"db_exec": {"sql": "SELECT mtime_secs, usn, config FROM notetypes ORDER BY id"}},
            {"restore_notetype": {"notetype": "Pair"}},
            {"restore_notetype": {"notetype": "Solo"}},
            {"db_exec": {"sql": UNCHANGED}},
            {"db_exec": {"sql": "SELECT mtime_secs, usn, config FROM notetypes ORDER BY id"}},
            {"observe": [{"notetype_structure": {"notetypes": ["Pair", "Solo"]}}]},
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario("t/ntrs", "UTC", START, steps, setup=setup(PAIR, SOLO)))
        self.assertEqual([s["original_stock_kind"] for s in out[2]["notetype_structure"].values()], [0, 7])
        self.assertEqual(out[6:8], [{"restore_notetype": {"error": "needs_stock_kind"}}] * 2)
        self.assertEqual(rows(out[8]), rows(out[4]))
        self.assertEqual(rows(out[9]), rows(out[5]))
        self.assertEqual(out[10], out[2])

    def test_a_refusal_keeps_the_undo_and_redo_histories(self):
        # dsl.md D7.3: a refused step changes nothing, Anki's undo and redo
        # histories included (a reopen empties both, undo-ops.md V5).
        status = [{"observe": ["undo_status"]}]
        refused = [{"restore_notetype": {"notetype": "Pair"}}]
        steps = [
            {"db_exec": {"sql": SET_STOCK_KIND.format(0), "params": ["Pair"]}},
            {"update_notetype": {"notetype": "Solo", "edits": [{"template": {"name": "Only", "back": "x"}}]}},
            {"undo": {}},
            *status, *refused, *status,
            {"redo": {}},
            *status, *refused, *status,
            {"undo": {}},
            {"observe": [{"notetype_templates": {"notetypes": ["Solo"]}}]},
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario("t/ntrs", "UTC", START, steps, setup=setup(PAIR, SOLO)))
        redo_only = {"undo_status": {"undo": None, "redo": "update_note_type"}}
        undo_only = {"undo_status": {"undo": "update_note_type", "redo": None}}
        no_kind = {"restore_notetype": {"error": "needs_stock_kind"}}
        self.assertEqual(out[2:11], [{}, redo_only, no_kind, redo_only, {}, undo_only, no_kind, undo_only, {}])
        self.assertEqual(out[11]["notetype_templates"]["Solo"][0]["back"], "")

    def test_each_given_kind_restores_that_stock_kind_and_stores_it(self):
        names = [f"K{i}" for i in range(len(STOCK))]
        steps = [{"db_exec": {"sql": SET_STOCK_KIND.format(0), "params": [name]}} for name in names]
        steps += [{"restore_notetype": {"notetype": name, "kind": kind}} for name, kind in zip(names, STOCK)]
        steps += [{"observe": [{"notetype_structure": {"notetypes": names}}]}]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario("t/ntrs", "UTC", START, steps, setup=setup(*(nt(n) for n in names))))
        self.assertEqual(out[len(names):2 * len(names)], [{}] * len(names))
        structure = out[-1]["notetype_structure"]
        for name, (kind, (_, nt_kind, stock_kind, fields, templates)) in zip(names, STOCK.items()):
            with self.subTest(kind=kind):
                self.assertEqual(shape(structure[name]), (nt_kind, stock_kind, fields, [(n, req) for n, req in templates]))

    def test_without_a_kind_the_stored_one_decides_and_name_and_id_stay(self):
        # A note type made from scratch stores kind 1 (records N5.1): Basic.
        steps = [
            {"db_exec": {"sql": NOTETYPE_ROW, "params": ["Solo"]}},
            {"db_exec": {"sql": SET_STOCK_KIND.format(2), "params": ["Pair"]}},
            {"restore_notetype": {"notetype": "Solo"}},
            {"restore_notetype": {"notetype": "Pair"}},
            {"observe": [{"notetype_structure": {"notetypes": ["Solo", "Pair"]}}]},
            {"db_exec": {"sql": NOTETYPE_ROW, "params": ["Solo"]}},
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario("t/ntrs", "UTC", START, steps, setup=setup(PAIR, SOLO)))
        self.assertEqual(out[2:4], [{}, {}])
        structure = out[4]["notetype_structure"]
        _, nt_kind, stock_kind, fields, templates = STOCK["basic"]
        self.assertEqual(shape(structure["Solo"]), (nt_kind, stock_kind, fields, list(templates)))
        _, nt_kind, stock_kind, fields, templates = STOCK["basic_and_reversed"]
        self.assertEqual(shape(structure["Pair"]), (nt_kind, stock_kind, fields, list(templates)))
        self.assertEqual(rows(out[5])[0][0], rows(out[0])[0][0])

    def test_unknown_names_are_unknown_ref_and_malformed_arguments_invalid_scenario(self):
        with WorkerProcess("UTC") as w:
            unknown = [run(w, [{"restore_notetype": args}], setup=setup(PAIR))
                       for args in ({"notetype": "pair"}, {"notetype": "Nope", "kind": "basic"})]
            bad = [run(w, [{"restore_notetype": args}], setup=setup(PAIR))
                   for args in ({}, {"notetype": 1}, {"notetype": ""}, {"notetype": "Pair", "kind": "Basic"},
                                {"notetype": "Pair", "kind": None}, {"notetype": "Pair", "kind": 0},
                                {"notetype": "Pair", "kind": ["basic"]},
                                {"notetype": "Pair", "x": 1}, "Pair")]
        for name, resp in zip(("pair", "Nope"), unknown):
            self.assertEqual(resp["error"], {"kind": "unknown_ref", "ref": name, "step": 0}, resp)
        for resp in bad:
            self.assertEqual((resp["error"]["kind"], resp["error"]["step"]), ("invalid_scenario", 0), resp)


class NotetypeTemplates(unittest.TestCase):
    def test_templates_in_ordinal_order_with_four_texts_as_stored(self):
        edits = [
            {"template": {"name": "Forward", "browser_front": "[{{Front}}]"}},
            {"template": {"name": "Reverse", "browser_back": "{{ Back }}", "back": ""}},
            {"move_template": {"template": "Reverse", "to": 0}},
        ]
        steps = [
            {"observe": [{"notetype_templates": {"notetypes": ["Pair", "Solo"]}}]},
            {"update_notetype": {"notetype": "Pair", "edits": edits}},
            # rows db_exec wrote are read: a renamed template and note type
            {"db_exec": {"sql": "UPDATE templates SET name = 'Lone' WHERE name = 'Only'"}},
            {"db_exec": {"sql": "UPDATE notetypes SET name = 'Single' WHERE name = 'Solo'"}},
            {"observe": [{"notetype_templates": {"notetypes": ["Single", "Pair"]}}]},
            {"db_exec": {"sql": "SELECT mod FROM col"}},
            {"observe": [{"notetype_templates": {"notetypes": ["Pair"]}}]},
            {"db_exec": {"sql": "SELECT mod FROM col"}},
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario("t/ntt", "UTC", START, steps, setup=setup(PAIR, SOLO)))
        forward = {"name": "Forward", "front": "{{Front}}", "back": "{{Back}}", "browser_front": "", "browser_back": ""}
        reverse = {"name": "Reverse", "front": "{{Back}}", "back": "{{Front}}", "browser_front": "", "browser_back": ""}
        only = {"name": "Only", "front": "{{Text}}", "back": "", "browser_front": "", "browser_back": ""}
        self.assertEqual(out[0], {"notetype_templates": {"Pair": [forward, reverse], "Solo": [only]}})
        self.assertEqual(out[4]["notetype_templates"], {
            "Single": [{**only, "name": "Lone"}],
            "Pair": [{**reverse, "back": "", "browser_back": "{{ Back }}"}, {**forward, "browser_front": "[{{Front}}]"}],
        })
        self.assertEqual(out[6]["notetype_templates"]["Pair"], out[4]["notetype_templates"]["Pair"])
        self.assertEqual(rows(out[7]), rows(out[5]))

    def test_unknown_names_are_unknown_ref_and_malformed_requests_invalid_scenario(self):
        with WorkerProcess("UTC") as w:
            unknown = run(w, [{"observe": [{"notetype_templates": {"notetypes": ["Pair", "pair"]}}]}], setup=setup(PAIR))
            bad = [run(w, [{"observe": [spec]}], setup=setup(PAIR))
                   for spec in ("notetype_templates", {"notetype_templates": {}}, {"notetype_templates": {"notetypes": []}},
                                {"notetype_templates": {"notetypes": "Pair"}}, {"notetype_templates": {"notetypes": [1]}},
                                {"notetype_templates": {"notetypes": [""]}},
                                {"notetype_templates": {"notetypes": ["Pair"], "x": 1}}, {"notetype_templates": ["Pair"]})]
        self.assertEqual(unknown["error"], {"kind": "unknown_ref", "ref": "pair", "step": 0}, unknown)
        for resp in bad:
            self.assertEqual((resp["error"]["kind"], resp["error"]["step"]), ("invalid_scenario", 0), resp)


if __name__ == "__main__":
    unittest.main()
