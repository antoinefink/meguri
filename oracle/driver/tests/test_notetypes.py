# SPDX-License-Identifier: AGPL-3.0-or-later
"""Note type ops (specs/conformance/notetype-ops.md T1-T8) through real
workers: add_notetype, add_stock_notetype, clone_notetype, update_notetype,
ensure_io_notetype, the notetype_structure observation and add_note by stored
name. Expected values come from the scenario (its names, patches, edits and
clock), SQLite and the spec texts (records N4, stock-kinds K1 and K4,
validation V2 and V3), never from the driver."""

from __future__ import annotations

import os
import shutil
import tempfile
import unittest

from driver.harness import WorkerProcess
from driver.tests.test_worker import epoch, run_ok, scenario

START = "2030-01-15T10:00:00"
NOW = epoch(START, "UTC")
ONE = {"name": "One", "front": "{{F}}"}
# notetypes/stock-kinds K1 and K4: name, kind, original_stock_kind,
# fields as (name, tag, protected), templates as (name, requirement).
STOCK = {
    "basic": ("Basic", "standard", 1, [("Front", None, False), ("Back", None, False)], [("Card 1", ("any", [0]))]),
    "basic_and_reversed": (
        "Basic (and reversed card)", "standard", 2,
        [("Front", None, False), ("Back", None, False)],
        [("Card 1", ("any", [0])), ("Card 2", ("any", [1]))],
    ),
    "basic_optional_reversed": (
        "Basic (optional reversed card)", "standard", 3,
        [("Front", None, False), ("Back", None, False), ("Add Reverse", None, False)],
        [("Card 1", ("any", [0])), ("Card 2", ("all", [1, 2]))],
    ),
    "basic_typing": (
        "Basic (type in the answer)", "standard", 4,
        [("Front", None, False), ("Back", None, False)], [("Card 1", ("any", [0, 1]))],
    ),
    "cloze": ("Cloze", "cloze", 5, [("Text", 0, True), ("Back Extra", 1, False)], [("Cloze", ("any", [0]))]),
    "image_occlusion": (
        "Image Occlusion", "cloze", 6,
        [("Occlusion", 0, True), ("Image", 1, True), ("Header", 2, True), ("Back Extra", 3, True), ("Comments", 4, False)],
        [("Image Occlusion", ("any", [0, 1, 2]))],
    ),
}
# A notes row count and col.mod: what a refused save must leave alone.
UNCHANGED = "SELECT (SELECT count() FROM notetypes), (SELECT mod FROM col), (SELECT group_concat(name, '|') FROM notetypes)"
CUR_MODEL = "SELECT CAST(c.val AS TEXT) = CAST(n.id AS TEXT) FROM config c, notetypes n WHERE c.KEY = 'curModel' AND n.name = ?"


def run(worker, steps, setup=None, notes=0):
    return worker.request({"cmd": "run", "scenario": scenario("t/nt", "UTC", START, steps, notes=notes, setup=setup)})


def rows(step):
    return step["db_exec"]["rows"]


def nt(name, fields=("F",), templates=(ONE,), **extra):
    return {"name": name, "fields": list(fields), "templates": list(templates), **extra}


class AddNotetype(unittest.TestCase):
    def test_stored_names_are_normalized_unique_under_unicase_and_the_last_add_is_current(self):
        steps = [
            {"add_notetype": nt("qa")},  # setup's QA under unicase
            {"add_notetype": nt('Pa"ir', fields=["F", "G"])},
            {"add_notetype": nt("Café", kind="cloze", templates=[{"name": "C", "front": "{{cloze:F}}", "back": "{{cloze:F}}"}])},
            {"db_exec": {"sql": "SELECT name FROM notetypes WHERE id > (SELECT id FROM notetypes WHERE name = 'QA') ORDER BY id"}},
            {"db_exec": {"sql": CUR_MODEL, "params": ["Café"]}},
            {"observe": [{"legacy_json": {"notetypes": ["Pair", "Café"]}}]},
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario("t/nt", "UTC", START, steps))
        self.assertEqual(out[:3], [{"add_notetype": {"name": n}} for n in ("qa+", "Pair", "Café")])
        self.assertEqual(rows(out[3]), [["qa+"], ["Pair"], ["Café"]])
        self.assertEqual(rows(out[4]), [[1]])
        pair, cafe = out[5]["legacy_json"]["notetypes"]["Pair"], out[5]["legacy_json"]["notetypes"]["Café"]
        # N5.1: kind as given, original_stock_kind 1, the given fields and templates, css "" when missing.
        self.assertEqual((pair["type"], pair["originalStockKind"], pair["css"]), (0, 1, ""))
        self.assertEqual([f["name"] for f in pair["flds"]], ["F", "G"])
        self.assertEqual([(t["name"], t["qfmt"], t["afmt"]) for t in pair["tmpls"]], [("One", "{{F}}", "")])
        self.assertEqual(cafe["type"], 1)

    def test_each_refusal_kind_and_a_refused_add_writes_nothing(self):
        cases = [
            ("duplicate_front", nt("D", templates=[{"name": "A", "front": "{{F}}"}, {"name": "B", "front": "{{F}}"}])),
            ("no_front_field", nt("N", templates=[{"name": "A", "front": "plain", "back": "{{F}}"}])),
            ("unknown_field", nt("U", templates=[{"name": "A", "front": "{{F}}", "back": "{{Nope}}"}])),
            ("template_parse_error", nt("P", templates=[{"name": "A", "front": "{{#F}}x", "back": "{{F}}"}])),
            ("missing_cloze", nt("M", kind="cloze", templates=[{"name": "A", "front": "{{F}}", "back": "{{F}}"}])),
            ("invalid_notetype", nt('""')),
            ("invalid_notetype", nt("Colon", fields=["::"])),
            # V3: the first failure in order wins: the duplicate before the unknown field.
            ("duplicate_front", nt("O", templates=[{"name": "A", "front": "{{Nope}}"}, {"name": "B", "front": "{{Nope}}"}])),
            # The detail, not the header, tells a parse error from an unknown
            # field: names that hold the other detail's words change nothing.
            ("unknown_field", nt("Q", fields=["F"], templates=[
                {"name": "A", "front": "{{F}}", "back": "{{See the preview for more information.}}"}])),
            ("unknown_field", nt("N<br>See the preview for more information.", templates=[
                {"name": "A", "front": "{{F}}", "back": "{{Nope}}"}])),
            ("template_parse_error", nt("Field 'x' not found.", templates=[{"name": "A", "front": "{{F}}", "back": "{{/F}}"}])),
        ]
        steps = [{"advance": "1m"}, {"db_exec": {"sql": UNCHANGED}}]
        steps += [{"add_notetype": spec} for _, spec in cases]
        steps += [{"db_exec": {"sql": UNCHANGED}}]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario("t/nt", "UTC", START, steps))
        self.assertEqual(out[2:-1], [{"add_notetype": {"error": kind}} for kind, _ in cases])
        self.assertEqual(rows(out[-1]), rows(out[1]))

    def test_wrong_json_types_are_invalid_scenario(self):
        bad = [
            {**nt("X"), "fields": "F"},
            nt("X", fields=[]),
            nt("X", kind="normal"),
            nt("X", templates=[{"name": "A", "front": 1}]),
            nt("X", css=None),
            {"name": "X", "fields": ["F"]},
            {**nt("X"), "extra": 1},
        ]
        with WorkerProcess("UTC") as w:
            resps = [run(w, [{"add_notetype": spec}]) for spec in bad]
        for spec, resp in zip(bad, resps):
            with self.subTest(spec=spec):
                self.assertEqual((resp["error"]["kind"], resp["error"]["step"]), ("invalid_scenario", 0), resp)


class AddStock(unittest.TestCase):
    def test_each_kind_holds_anki_stock_structure_under_its_name_or_the_given_one(self):
        steps = [{"db_exec": {"sql": "UPDATE col SET ls = scm"}}, {"advance": "1m"}]
        steps += [{"add_stock_notetype": {"kind": kind}} for kind in STOCK]
        steps += [
            {"add_stock_notetype": {"kind": "basic_typing", "name": 'Typing "Drill"'}},
            {"add_stock_notetype": {"kind": "cloze", "name": '""'}},
            {"observe": [{"notetype_structure": {"notetypes": [name + "+" for name, *_ in STOCK.values()]}}]},
            {"db_exec": {"sql": "SELECT name, mtime_secs, usn FROM notetypes WHERE name = 'Typing Drill'"}},
            {"db_exec": {"sql": CUR_MODEL, "params": ["Typing Drill"]}},
            {"db_exec": {"sql": "SELECT scm > ls FROM col"}},
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario("t/nt", "UTC", START, steps))
            bad = [run(w, [{"add_stock_notetype": args}]) for args in ({"kind": "Basic"}, {"kind": "basic", "name": ""}, {})]
        # The new collection's six names are taken: each add appends '+' (records N4).
        self.assertEqual(out[2:8], [{"add_stock_notetype": {"name": name + "+"}} for name, *_ in STOCK.values()])
        self.assertEqual(out[8:10], [{"add_stock_notetype": {"name": "Typing Drill"}}, {"add_stock_notetype": {"error": "invalid_notetype"}}])
        structure = out[10]["notetype_structure"]
        for kind, (name, nt_kind, stock_kind, fields, templates) in STOCK.items():
            with self.subTest(kind=kind):
                s = structure[name + "+"]
                self.assertEqual((s["kind"], s["original_stock_kind"], s["original_id"], s["sort_field"]),
                                 (nt_kind, stock_kind, None, 0))
                self.assertEqual([(f["name"], f["tag"], f["prevent_deletion"]) for f in s["fields"]], fields)
                self.assertEqual({(f["font_name"], f["font_size"]) for f in s["fields"]}, {("Arial", 20)})
                self.assertEqual(
                    [(t["name"], t["deck"], (t["requirement"]["kind"], t["requirement"]["fields"])) for t in s["templates"]],
                    [(n, None, req) for n, req in templates],
                )
        # A local add (N5): stamped now, usn -1, current; not a schema change (G2).
        self.assertEqual(rows(out[11]), [["Typing Drill", NOW + 60, -1]])
        self.assertEqual(rows(out[12]) + rows(out[13]), [[1], [0]])
        for resp in bad:
            self.assertEqual((resp["error"]["kind"], resp["error"]["step"]), ("invalid_scenario", 0), resp)


class Clone(unittest.TestCase):
    def test_a_clone_keeps_ids_and_options_drops_the_original_id_and_becomes_current(self):
        patch = {
            "sortf": 1,
            "originalId": 77,
            "css": ".c {}",
            "flds": [
                {"name": "Q", "ord": 0, "sticky": True, "rtl": False, "font": "Georgia", "size": 31, "id": 501},
                {"name": "A", "ord": 1, "sticky": False, "rtl": True, "font": "Arial", "size": 20, "id": 502},
            ],
            "tmpls": [{"name": "C1", "ord": 0, "qfmt": "{{Q}}", "afmt": "{{A}}", "bqfmt": "b", "id": 601}],
        }
        steps = [
            {"legacy_update": {"notetype": "QA", "patch": patch}},
            {"advance": "1m"},
            {"clone_notetype": {"notetype": "QA"}},
            {"clone_notetype": {"notetype": "QA", "name": "qa copy"}},
            {"observe": [{"legacy_json": {"notetypes": ["QA", "QA copy"]}}]},
            {"db_exec": {"sql": "SELECT name, mtime_secs, usn FROM notetypes WHERE name LIKE '%copy%' ORDER BY id"}},
            {"db_exec": {"sql": CUR_MODEL, "params": ["qa copy+"]}},
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario("t/nt", "UTC", START, steps))
            unknown = run(w, [{"clone_notetype": {"notetype": "qa"}}])
        self.assertEqual(out[2:4], [{"clone_notetype": {"name": "QA copy"}}, {"clone_notetype": {"name": "qa copy+"}}])
        src, copy = out[4]["legacy_json"]["notetypes"]["QA"], out[4]["legacy_json"]["notetypes"]["QA copy"]
        self.assertEqual(src["originalId"], 77)
        self.assertNotIn("originalId", copy)
        self.assertNotEqual(copy["id"], src["id"])
        for key in ("sortf", "css", "type", "originalStockKind", "latexPre", "latexPost", "flds", "tmpls"):
            self.assertEqual(copy[key], src[key], key)
        self.assertEqual([f["id"] for f in copy["flds"]] + [t["id"] for t in copy["tmpls"]], [501, 502, 601])
        self.assertEqual(rows(out[5]), [["QA copy", NOW + 60, -1], ["qa copy+", NOW + 60, -1]])
        self.assertEqual(rows(out[6]), [[1]])
        self.assertEqual(unknown["error"], {"kind": "unknown_ref", "ref": "qa", "step": 0})


TRIO = nt("Trio", fields=["A", "B", "C"], templates=[{"name": "X", "front": "{{A}}", "back": "{{B}}"},
                                                     {"name": "Y", "front": "{{B}}", "back": "{{A}}"}])
TRIO_IDS = {
    "flds": [{"name": n, "ord": i, "sticky": False, "rtl": False, "font": "Arial", "size": 20, "id": 1 + i}
             for i, n in enumerate("ABC")],
    "tmpls": [{"name": "X", "ord": 0, "qfmt": "{{A}}", "afmt": "{{B}}", "id": 11},
              {"name": "Y", "ord": 1, "qfmt": "{{B}}", "afmt": "{{A}}", "id": 12}],
}


class Update(unittest.TestCase):
    def setup(self):
        return {"decks": [{"name": "A", "preset": "P"}, "P::Q"], "notetypes": [TRIO]}

    def test_edits_apply_in_order_to_a_draft_that_keeps_field_and_template_identity(self):
        edits = [
            {"rename_field": {"from": "C", "to": "Cee"}},
            {"move_field": {"field": "Cee", "to": 0}},
            {"add_field": "New"},
            {"field_options": {"field": "A", "sticky": True, "rtl": True, "font_name": "Georgia", "font_size": 30,
                               "description": "d", "plain_text": True, "collapsed": True, "exclude_from_search": True}},
            {"sort_field": "B"},
            {"rename_template": {"from": "Y", "to": "Why"}},
            {"move_template": {"template": "Why", "to": 0}},
            {"add_template": {"name": "Z", "front": "{{New}}", "back": "{{A}}"}},
            {"template": {"name": "X", "back": "{{Cee}}", "browser_front": "bq {{A}}", "browser_back": "ba",
                          "browser_font_name": "Courier", "browser_font_size": 11, "deck": "P::Q"}},
            {"css": ".x {}"},
            {"rename": "Tri"},
        ]
        steps = [
            {"legacy_update": {"notetype": "Trio", "patch": TRIO_IDS}},
            {"update_notetype": {"notetype": "Trio", "edits": edits}},
            {"observe": [{"legacy_json": {"notetypes": ["Tri"]}}]},
            {"db_exec": {"sql": "SELECT id FROM decks WHERE name = 'P' || char(31) || 'Q'"}},
            {"update_notetype": {"notetype": "Tri", "edits": [{"template": {"name": "X", "deck": None}},
                                                               {"template": {"name": "Why", "deck": "Default"}}]}},
            {"observe": [{"legacy_json": {"notetypes": ["Tri"]}}]},
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario("t/nt", "UTC", START, steps, setup=self.setup()))
        self.assertEqual(out[1], {"update_notetype": {"name": "Tri"}})
        saved = out[2]["legacy_json"]["notetypes"]["Tri"]
        self.assertEqual([f["name"] for f in saved["flds"]], ["Cee", "A", "B", "New"])
        self.assertEqual([f["id"] for f in saved["flds"]][:3], [3, 1, 2])
        self.assertNotIn(saved["flds"][3]["id"], (None, 1, 2, 3))
        a = saved["flds"][1]
        self.assertEqual(
            (a["sticky"], a["rtl"], a["font"], a["size"], a["description"], a["plainText"], a["collapsed"], a["excludeFromSearch"]),
            (True, True, "Georgia", 30, "d", True, True, True),
        )
        self.assertEqual(saved["sortf"], 2)  # B, now third
        self.assertEqual([t["name"] for t in saved["tmpls"]], ["Why", "X", "Z"])
        self.assertEqual([t["id"] for t in saved["tmpls"]][:2], [12, 11])
        x = saved["tmpls"][1]
        [[deck_id]] = rows(out[3])
        self.assertEqual((x["qfmt"], x["afmt"], x["bqfmt"], x["bafmt"], x["bfont"], x["bsize"], x["did"]),
                         ("{{A}}", "{{Cee}}", "bq {{A}}", "ba", "Courier", 11, deck_id))
        self.assertEqual((saved["tmpls"][2]["qfmt"], saved["tmpls"][2]["afmt"]), ("{{New}}", "{{A}}"))
        self.assertEqual(saved["css"], ".x {}")
        again = out[5]["legacy_json"]["notetypes"]["Tri"]
        self.assertEqual([t["did"] for t in again["tmpls"]], [1, None, None])

    def test_skip_checks_saves_what_a_checked_save_refuses_and_a_refusal_writes_nothing(self):
        broken = [{"template": {"name": "C1", "back": "{{Nope}}"}}]
        steps = [
            {"advance": "1m"},
            {"db_exec": {"sql": UNCHANGED}},
            {"update_notetype": {"notetype": "QA", "edits": broken}},
            {"update_notetype": {"notetype": "QA", "edits": [{"remove_template": "C1"}], "skip_checks": True}},
            {"update_notetype": {"notetype": "QA", "edits": [{"rename": '"'}]}},
            {"db_exec": {"sql": UNCHANGED}},
            {"db_exec": {"sql": "SELECT mtime_secs FROM notetypes WHERE name = 'QA'"}},
            {"update_notetype": {"notetype": "QA", "edits": broken, "skip_checks": True}},
            {"observe": [{"legacy_json": {"notetypes": ["QA"]}}]},
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario("t/nt", "UTC", START, steps))
        self.assertEqual(out[2:5], [{"update_notetype": {"error": k}} for k in ("unknown_field", "invalid_notetype", "invalid_notetype")])
        self.assertEqual(rows(out[5]), rows(out[1]))
        self.assertLess(rows(out[6])[0][0], NOW + 60)
        self.assertEqual(out[7], {"update_notetype": {"name": "QA"}})
        self.assertEqual(out[8]["legacy_json"]["notetypes"]["QA"]["tmpls"][0]["afmt"], "{{Nope}}")

    def test_unknown_names_are_unknown_ref_and_malformed_edits_invalid_scenario(self):
        unknown = [
            ("Nope", []),
            ("Trio", [{"remove_field": "a"}]),  # exact names: no case folding
            ("Trio", [{"rename_field": {"from": "A", "to": "A2"}}, {"field_options": {"field": "A", "sticky": True}}]),
            ("Trio", [{"move_template": {"template": "Nope", "to": 0}}]),
        ]
        malformed = [
            [{"add_field": "N"}, {"sort_field": "N"}],
            [{"move_field": {"field": "A", "to": 3}}],
            [{"rename": "x", "css": "y"}],
            [{"recolor": "x"}],
            [{"template": {"name": "X", "deck": "Nope"}}],
            [{"field_options": {"field": "A", "font_size": -1}}],
            [{"field_options": {"field": "A", "tag": 2**32}}],
            [{"field_options": {"field": "A", "tag": True}}],
            [{"field_options": {"field": "A", "tag": 1.0}}],
            [{"field_options": {"field": "A", "prevent_deletion": None}}],
            [{"template": {"name": "X", "front": None}}],
        ]
        with WorkerProcess("UTC") as w:
            refs = [run(w, [{"update_notetype": {"notetype": name, "edits": edits}}], setup=self.setup())
                    for name, edits in unknown]
            bad = [run(w, [{"update_notetype": {"notetype": "Trio", "edits": edits}}], setup=self.setup())
                   for edits in malformed]
            bad.append(run(w, [{"update_notetype": {"notetype": "Trio", "edits": [], "skip_checks": "yes"}}], setup=self.setup()))
            bad.append(run(w, [{"update_notetype": {"notetype": "Trio", "edits": {}}}], setup=self.setup()))
        for ref, resp in zip(["Nope", "a", "A", "Nope"], refs):
            self.assertEqual(resp["error"], {"kind": "unknown_ref", "ref": ref, "step": 0}, resp)
        for resp in bad:
            self.assertEqual((resp["error"]["kind"], resp["error"]["step"]), ("invalid_scenario", 0), resp)

    def test_io_field_options_set_and_remove_the_tag_and_protection(self):
        # notetype-ops.md T5.1 (draft): tag (u32 or null) and prevent_deletion.
        def opts(field, **kw):
            return {"field_options": {"field": field, **kw}}

        steps = [
            {"ensure_io_notetype": {}},
            {"update_notetype": {"notetype": "Image Occlusion", "edits": [opts("Header", tag=None, prevent_deletion=False)]}},
            {"observe": [{"notetype_structure": {"notetypes": ["Image Occlusion"]}}]},
            {"update_notetype": {"notetype": "Trio", "edits": [opts("A", tag=2**32 - 1, prevent_deletion=True), opts("B", tag=0)]}},
            {"observe": [{"notetype_structure": {"notetypes": ["Trio"]}}]},
            {"update_notetype": {"notetype": "Trio", "edits": [opts("A", tag=None)]}},
            {"observe": [{"notetype_structure": {"notetypes": ["Trio"]}}]},
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario("t/nt", "UTC", START, steps, setup=self.setup()))
        io = out[2]["notetype_structure"]["Image Occlusion"]["fields"]
        self.assertEqual([(f["tag"], f["prevent_deletion"]) for f in io[:3]], [(0, True), (1, True), (None, False)])
        trio = out[4]["notetype_structure"]["Trio"]["fields"]
        self.assertEqual([(f["tag"], f["prevent_deletion"]) for f in trio], [(2**32 - 1, True), (0, False), (None, False)])
        trio = out[6]["notetype_structure"]["Trio"]["fields"]
        self.assertEqual([(f["tag"], f["prevent_deletion"]) for f in trio], [(None, True), (0, False), (None, False)])

    def test_each_op_reads_the_rows_db_exec_wrote(self):
        steps = [
            {"db_exec": {"sql": "UPDATE notetypes SET name = 'Renamed' WHERE name = 'QA'"}},
            {"update_notetype": {"notetype": "Renamed", "edits": [{"css": "x"}]}},
            {"clone_notetype": {"notetype": "Renamed"}},
            {"db_exec": {"sql": "SELECT name FROM notetypes WHERE name LIKE 'Renamed%' OR name = 'QA' ORDER BY id"}},
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario("t/nt", "UTC", START, steps, notes=1))
        self.assertEqual(out[1:3], [{"update_notetype": {"name": "Renamed"}}, {"clone_notetype": {"name": "Renamed copy"}}])
        self.assertEqual(rows(out[3]), [["Renamed"], ["Renamed copy"]])


class EnsureIo(unittest.TestCase):
    def test_present_writes_nothing_missing_is_added_and_the_current_note_type_stays(self):
        io = "(SELECT id FROM notetypes WHERE name = 'Image Occlusion')"
        steps = [
            {"advance": "1m"},
            {"ensure_io_notetype": {}},
            {"db_exec": {"sql": "SELECT name, mtime_secs, usn FROM notetypes WHERE name LIKE 'Image%'"}},
            {"db_exec": {"sql": f"DELETE FROM templates WHERE ntid = {io}"}},
            {"db_exec": {"sql": f"DELETE FROM fields WHERE ntid = {io}"}},
            {"db_exec": {"sql": "DELETE FROM notetypes WHERE name = 'Image Occlusion'"}},
            {"ensure_io_notetype": {}},
            {"db_exec": {"sql": "SELECT name, mtime_secs, usn FROM notetypes WHERE name LIKE 'Image%'"}},
            {"db_exec": {"sql": CUR_MODEL, "params": ["QA"]}},
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario("t/nt", "UTC", START, steps))
            bad = run(w, [{"ensure_io_notetype": {"x": 1}}])
        self.assertEqual(out[1], {"ensure_io_notetype": {"name": "Image Occlusion"}})
        self.assertEqual(rows(out[2]), [["Image Occlusion", 0, 0]])
        self.assertEqual(out[6], {"ensure_io_notetype": {"name": "Image Occlusion"}})
        self.assertEqual(rows(out[7]), [["Image Occlusion", NOW + 60, -1]])
        self.assertEqual(rows(out[8]), [[1]])
        self.assertEqual((bad["error"]["kind"], bad["error"]["step"]), ("invalid_scenario", 0))


class Structure(unittest.TestCase):
    def test_values_are_the_stored_records_without_text_or_ids(self):
        duo = nt("Duo", fields=["A", "B"], templates=[{"name": "F", "front": "{{A}}"}, {"name": "R", "front": "{{B}}"}])
        patch = {
            "originalId": 4242,
            "sortf": 1,
            "flds": [
                {"name": "A", "ord": 0, "sticky": True, "rtl": True, "font": "Georgia", "size": 31, "description": "desc",
                 "plainText": True, "collapsed": True, "excludeFromSearch": True, "id": 1, "tag": 7, "preventDeletion": True},
                {"name": "B", "ord": 1, "sticky": False, "rtl": False, "font": "Arial", "size": 20, "id": 2},
            ],
            "tmpls": [{"name": "F", "ord": 0, "qfmt": "{{A}}", "afmt": "", "did": 1, "id": 3},
                      {"name": "R", "ord": 1, "qfmt": "{{B}}", "afmt": "", "did": 999999, "id": 4}],
        }
        setup = {"notetypes": [duo, nt("Bare"), nt("Cz", kind="cloze", fields=["T"],
                                                  templates=[{"name": "C", "front": "{{cloze:T}}", "back": "{{cloze:T}}"}])]}
        steps = [
            {"legacy_update": {"notetype": "Duo", "patch": patch}},
            {"db_exec": {"sql": "UPDATE notetypes SET config = X'' WHERE name = 'Bare'"}},
            {"observe": [{"notetype_structure": {"notetypes": ["Duo", "Bare", "Cz"]}}]},
            {"observe": ["notetype_structure"]},
            {"db_exec": {"sql": "SELECT name FROM notetypes ORDER BY id"}},
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario("t/nt", "UTC", START, steps, setup=setup))
            unknown = run(w, [{"observe": [{"notetype_structure": {"notetypes": ["duo"]}}]}], setup=setup)
        value = out[2]["notetype_structure"]
        self.assertEqual(value["Duo"], {
            "kind": "standard",
            "original_stock_kind": 1,
            "original_id": 4242,
            "sort_field": 1,
            "fields": [
                {"name": "A", "sticky": True, "rtl": True, "font_name": "Georgia", "font_size": 31, "description": "desc",
                 "plain_text": True, "collapsed": True, "exclude_from_search": True, "tag": 7, "prevent_deletion": True},
                {"name": "B", "sticky": False, "rtl": False, "font_name": "Arial", "font_size": 20, "description": "",
                 "plain_text": False, "collapsed": False, "exclude_from_search": False, "tag": None, "prevent_deletion": False},
            ],
            # The deck override: the default deck by name, a missing deck by id.
            "templates": [
                {"name": "F", "deck": "Default", "requirement": {"kind": "any", "fields": [0]}},
                {"name": "R", "deck": 999999, "requirement": {"kind": "any", "fields": [1]}},
            ],
        })
        # An empty config blob: every value its default, no requirement.
        bare = value["Bare"]
        self.assertEqual((bare["kind"], bare["original_stock_kind"], bare["original_id"], bare["sort_field"]),
                         ("standard", 0, None, 0))
        self.assertEqual(bare["templates"], [{"name": "One", "deck": None, "requirement": None}])
        self.assertEqual(value["Cz"]["kind"], "cloze")
        self.assertEqual(sorted(out[3]["notetype_structure"]), sorted(name for [name] in rows(out[4])))
        self.assertEqual(out[3]["notetype_structure"]["Duo"], value["Duo"])
        self.assertEqual(unknown["error"], {"kind": "unknown_ref", "ref": "duo", "step": 0})


class AddNote(unittest.TestCase):
    def test_any_note_type_resolves_by_its_exact_stored_name(self):
        steps = [
            {"add_note": {"notetype": "Basic", "fields": ["a", "b"]}},
            {"add_notetype": nt('Pa"ir', fields=["F", "G"], templates=[ONE, {"name": "Two", "front": "{{G}}"}])},
            {"add_note": {"notetype": "Pair", "fields": {"G": "y", "F": "x"}}},
            {"handoff": "meguri"},
            {"add_note": {"notetype": "Basic (and reversed card)", "fields": ["c", "d"]}},
            {"db_exec": {"sql": "SELECT n.flds, nt.name FROM notes n JOIN notetypes nt ON nt.id = n.mid ORDER BY n.id"}},
        ]
        with WorkerProcess("UTC") as w:
            out = run_ok(self, w, scenario("t/nt", "UTC", START, steps))
            misses = [run(w, [{"add_note": {"notetype": name, "fields": []}}]) for name in ('Pa"ir', "basic", "qa")]
        self.assertEqual(out[0], {"add_note": {"note": "#n1", "cards": ["#c1"]}})
        self.assertEqual(out[2], {"add_note": {"note": "#n2", "cards": ["#c2", "#c3"]}})
        self.assertEqual(out[4], {"add_note": {"note": "#n3", "cards": ["#c4", "#c5"]}})
        self.assertEqual(rows(out[5]), [["a\x1fb", "Basic"], ["x\x1fy", "Pair"], ["c\x1fd", "Basic (and reversed card)"]])
        for name, resp in zip(('Pa"ir', "basic", "qa"), misses):
            self.assertEqual(resp["error"], {"kind": "unknown_ref", "ref": name, "step": 0}, resp)

    def test_a_resumed_segment_resolves_names_its_setup_never_had(self):
        root = tempfile.mkdtemp(prefix="nt-handoff-", dir=os.environ.get("ORACLE_SCRATCH_ROOT"))
        self.addCleanup(shutil.rmtree, root, True)
        steps = [
            {"add_notetype": nt("Mine")},
            {"handoff": "oracle"},
            {"add_note": {"notetype": "Mine", "fields": ["m"]}},
            {"add_note": {"notetype": "Cloze", "fields": ["{{c1::x}}"]}},
        ]
        sc = scenario("t/nt", "UTC", START, steps)
        with WorkerProcess("UTC", extra_env={"ORACLE_HANDOFF": root}) as w:
            first = w.request({"cmd": "run", "scenario": sc, "steps": [0, 1], "handoff": "t/nt/1"})
            second = w.request({"cmd": "run", "scenario": sc, "steps": [2, 4], "resume": first["state"]})
        self.assertTrue(first["ok"], first)
        self.assertTrue(second["ok"], second)
        self.assertEqual(second["observations"]["steps"],
                         [{"add_note": {"note": "#n1", "cards": ["#c1"]}}, {"add_note": {"note": "#n2", "cards": ["#c2"]}}])


if __name__ == "__main__":
    unittest.main()
