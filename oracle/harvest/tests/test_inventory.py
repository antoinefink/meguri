# SPDX-License-Identifier: AGPL-3.0-or-later
"""Tests of the harvest inventory: python3 -B -I -m unittest discover -s oracle/harvest/tests"""

from __future__ import annotations

import csv
import io
import json
import shutil
import sys
import tempfile
import unittest
from contextlib import redirect_stderr
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

import inventory  # noqa: E402

FAKE_TREE = {
    "rslib/src/scheduler/answering.rs": (
        "fn helper() {}\n#[test]\nfn answering_moves_the_card() {}\n"
        "mod more {\n    #[test]\n    #[should_panic]\n    fn panics_on_bad_input() {}\n}\n"
    ),
    "rslib/src/search/mod.rs": "#[tokio::test]\nasync fn plain_search_matches() {}\n",
    "build/ninja_gen/src/lib.rs": "#[test]\nfn builds_rules() {}\n",
    "pylib/tests/test_schedv3.py": (
        "def test_new_cards_are_shown():\n    pass\n\ndef helper():\n    pass\n"
        "class TestFiltered:\n    def test_rebuild(self):\n        pass\n    def not_a_test(self):\n        pass\n"
    ),
    "qt/tests/test_toolbar.py": "def test_toolbar_draws():\n    pass\n",
    "ts/routes/deck-options/steps.test.ts": (
        'import { test } from "vitest";\ntest("parses learning steps from text", () => {});\n'
        "describe('x', () => {\n  it(`keeps the order of steps`, () => {});\n});\n"
    ),
    "ts/node_modules/dep/x.test.ts": 'test("never inventoried here", () => {});\n',
}


def read(path: Path) -> list[dict]:
    with path.open(encoding="utf-8", newline="") as f:
        return list(csv.DictReader(f))


def edit(root: Path, rel: str, old: str, new: str) -> None:
    path = root / rel
    text = path.read_text(encoding="utf-8")
    assert text.count(old) == 1, (rel, old)
    path.write_text(text.replace(old, new), encoding="utf-8")


# Test sources whose spans hold braces, parens and quotes that are not code.
SPAN_TREE = {
    "rslib/src/template.rs": (
        "#[test]\n#[should_panic]\nfn renders_braces() {\n"
        "    let a = \"}}{{Front}\";\n    let b = '}';\n    let r = r#\"\"}\"#;\n"
        "    // }\n    /* } /* } */ } */\n    let l: &'static str = \"\\\"}\";\n}\n"
        "fn helper() {}\n#[test]\nfn after_braces() {}\n"
    ),
    "pylib/tests/test_span.py": (
        "import pytest\n\n@pytest.mark.skip\ndef test_first():\n    x = '}'\n\n"
        "def helper():\n    pass\n\ndef test_second():\n    pass\n"
    ),
    "ts/lib/span.test.ts": (
        'test("first", () => {\n  const s = `)${f(")")}`;\n  expect(/\\)/.test(s)).toBe(true); // )\n});\n'
        "const helper = 1;\n"
        'test("second", () => {});\n'
    ),
}


class InventoryTest(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        self.anki = self.root / "anki"
        for rel, text in FAKE_TREE.items():
            p = self.anki / rel
            p.parent.mkdir(parents=True, exist_ok=True)
            p.write_text(text, encoding="utf-8")
        self.links = self.root / "links.csv"
        self.catalog = self.root / "harvest.csv"

    def tearDown(self) -> None:
        self.tmp.cleanup()

    def run_inventory(self, *extra: str, anki: Path | None = None, tag: str = "26.09.3") -> str:
        err = io.StringIO()
        with redirect_stderr(err):
            rc = inventory.main(
                ["inventory", "--anki", str(anki or self.anki), "--tag", tag, *extra,
                 "--links", str(self.links), "--catalog", str(self.catalog)]
            )
        self.assertEqual(rc, 0, err.getvalue())
        return err.getvalue()

    def run_check(self, *extra: str) -> tuple[int, str]:
        err = io.StringIO()
        with redirect_stderr(err):
            rc = inventory.main(["check", "--links", str(self.links), "--catalog", str(self.catalog), *extra])
        return rc, err.getvalue()

    def main_checkout(self, added: dict[str, str]) -> Path:
        """Anki's main branch: the tag's tree plus `added` (path -> text appended)."""
        main = self.root / "anki-main"
        shutil.copytree(self.anki, main)
        for rel, text in added.items():
            p = main / rel
            p.parent.mkdir(parents=True, exist_ok=True)
            with p.open("a", encoding="utf-8") as f:
                f.write(text)
        return main

    def pin(self, version: str) -> Path:
        path = self.root / "oracle.json"
        path.write_text(json.dumps({"anki_version": version, "lock_sha256": "0" * 64}), encoding="utf-8")
        return path

    def test_every_test_is_inventoried_once_with_an_opaque_id(self) -> None:
        summary = self.run_inventory()
        links = read(self.links)
        found = sorted((r["language"], r["file"], r["test"]) for r in links)
        self.assertEqual(
            found,
            [
                ("python", "pylib/tests/test_schedv3.py", "TestFiltered.test_rebuild"),
                ("python", "pylib/tests/test_schedv3.py", "test_new_cards_are_shown"),
                ("python", "qt/tests/test_toolbar.py", "test_toolbar_draws"),
                ("rust", "build/ninja_gen/src/lib.rs", "builds_rules"),
                ("rust", "rslib/src/scheduler/answering.rs", "answering_moves_the_card"),
                ("rust", "rslib/src/scheduler/answering.rs", "panics_on_bad_input"),
                ("rust", "rslib/src/search/mod.rs", "plain_search_matches"),
                ("typescript", "ts/routes/deck-options/steps.test.ts", "keeps the order of steps"),
                ("typescript", "ts/routes/deck-options/steps.test.ts", "parses learning steps from text"),
            ],
        )
        self.assertIn("rust 4, python 3, typescript 2", summary)
        catalog = read(self.catalog)
        self.assertEqual(len(catalog), 9)
        for row in catalog:
            self.assertRegex(row["harvest_id"], r"^h[0-9a-f]{12}$")
        by_lang_file = {(r["language"], r["file"]): r["harvest_id"] for r in links}
        rows = {r["harvest_id"]: r for r in catalog}
        toolbar = rows[by_lang_file[("python", "qt/tests/test_toolbar.py")]]
        self.assertEqual((toolbar["area"], toolbar["classification"], toolbar["status"]), ("desktop", "not-applicable", "done"))
        self.assertEqual({r["source"] for r in catalog}, {"tag"})
        build = rows[by_lang_file[("rust", "build/ninja_gen/src/lib.rs")]]
        self.assertEqual((build["area"], build["classification"]), ("tooling", "not-applicable"))
        search = rows[by_lang_file[("rust", "rslib/src/search/mod.rs")]]
        self.assertEqual((search["area"], search["classification"], search["status"]), ("search", "unclassified", "open"))
        self.assertEqual(self.run_check()[0], 0)

    def test_classifications_survive_a_re_inventory_and_gone_tests_are_dropped(self) -> None:
        self.run_inventory()
        rows = read(self.catalog)
        links = {r["test"]: r["harvest_id"] for r in read(self.links)}
        rebuild = links["TestFiltered.test_rebuild"]
        for row in rows:
            if row["harvest_id"] == rebuild:
                row.update(classification="preservation-only", reason="Filtered decks are kept, not studied",
                           scenarios="preservation/filtered-deck-sync", status="done")
        inventory.write_csv(self.catalog, inventory.CATALOG_HEADER, rows)
        (self.anki / "qt/tests/test_toolbar.py").unlink()
        summary = self.run_inventory()
        after = {r["harvest_id"]: r for r in read(self.catalog)}
        self.assertEqual(after[rebuild]["classification"], "preservation-only")
        self.assertEqual(after[rebuild]["scenarios"], "preservation/filtered-deck-sync")
        self.assertEqual(len(after), 8)
        self.assertIn("1 catalog rows dropped", summary)

    def test_check_flags_anki_names_in_the_catalog(self) -> None:
        self.run_inventory()
        rows = read(self.catalog)
        rows[0]["reason"] = "Ported from answering_moves_the_card"
        rows[1]["reason"] = "See test_schedv3.py"
        rows[2]["scenarios"] = "Same as parses learning steps from text"
        inventory.write_csv(self.catalog, inventory.CATALOG_HEADER, rows)
        rc, out = self.run_check()
        self.assertEqual(rc, 1)
        self.assertIn(f"{rows[0]['harvest_id']} reason: contains an Anki test name", out)
        self.assertIn(f"{rows[1]['harvest_id']} reason: contains an Anki file name", out)
        self.assertIn(f"{rows[2]['harvest_id']} scenarios: contains an Anki test name", out)
        # The finding never repeats the name it found.
        for name in ("answering_moves_the_card", "test_schedv3.py", "parses learning steps"):
            self.assertNotIn(name, out)

    def test_check_flags_ids_missing_on_either_side(self) -> None:
        self.run_inventory()
        rows = read(self.catalog)
        inventory.write_csv(self.catalog, inventory.CATALOG_HEADER, rows[1:])
        rc, out = self.run_check()
        self.assertEqual(rc, 1)
        self.assertIn("in links.csv but not in the catalog", out)

    def test_tests_added_on_main_are_supplementary_and_survive_a_re_inventory(self) -> None:
        self.run_inventory()
        main = self.main_checkout({
            "rslib/src/scheduler/answering.rs": "#[test]\nfn relearning_keeps_the_lapse_count() {}\n",
            "pylib/tests/test_schedv3.py": "\ndef test_buried_siblings_return_next_day():\n    pass\n",
        })
        summary = self.run_inventory("--supplementary", str(main), "--supplementary-tag", "main@0123456789ab")
        # The counts at the tag are unchanged; the new tests are listed apart.
        self.assertIn("9 tests at 26.09.3: rust 4, python 3, typescript 2", summary)
        self.assertIn("2 supplementary: rust 1, python 1, typescript 0", summary)
        links = {r["test"]: r for r in read(self.links)}
        added = links["relearning_keeps_the_lapse_count"]
        self.assertEqual(added["tag"], "main@0123456789ab")
        self.assertEqual(links["answering_moves_the_card"]["tag"], "26.09.3")
        rows = {r["harvest_id"]: r for r in read(self.catalog)}
        self.assertEqual(rows[added["harvest_id"]]["source"], "supplementary")
        self.assertEqual(rows[added["harvest_id"]]["classification"], "unclassified")
        self.assertEqual(rows[links["answering_moves_the_card"]["harvest_id"]]["source"], "tag")
        self.assertEqual(len(rows), 11)
        self.assertEqual(self.run_check()[0], 0)

        # A spec writer classifies one; a re-inventory at the tag alone keeps
        # both supplementary rows and the classification.
        catalog = read(self.catalog)
        for row in catalog:
            if row["harvest_id"] == added["harvest_id"]:
                row.update(classification="in-scope", reason="The lapse count after relearning exists at the release",
                           status="deferred")
        inventory.write_csv(self.catalog, inventory.CATALOG_HEADER, catalog)
        summary = self.run_inventory()
        self.assertIn("2 supplementary", summary)
        rows = {r["harvest_id"]: r for r in read(self.catalog)}
        self.assertEqual(rows[added["harvest_id"]]["classification"], "in-scope")
        self.assertEqual(rows[added["harvest_id"]]["source"], "supplementary")

        # Once the pinned release has the test (a bump), it is a tag row with
        # the same id and keeps its classification.
        summary = self.run_inventory(anki=main, tag="26.10.0")
        self.assertIn("0 supplementary", summary)
        links = {r["test"]: r for r in read(self.links)}
        self.assertEqual(links["relearning_keeps_the_lapse_count"]["tag"], "26.10.0")
        rows = {r["harvest_id"]: r for r in read(self.catalog)}
        self.assertEqual(rows[added["harvest_id"]]["source"], "tag")
        self.assertEqual(rows[added["harvest_id"]]["classification"], "in-scope")

    def test_check_fails_when_the_inventory_is_not_at_the_oracle_pin(self) -> None:
        main = self.main_checkout({"rslib/src/search/mod.rs": "#[test]\nfn search_after_the_release() {}\n"})
        self.run_inventory("--supplementary", str(main), "--supplementary-tag", "main@0123456789ab")
        # 26.9.3 is the pin's distribution version of tag 26.09.3; supplementary
        # rows carry a main revision and do not count.
        self.assertEqual(self.run_check("--pin", str(self.pin("26.9.3")))[0], 0)
        rc, out = self.run_check("--pin", str(self.pin("26.10.0")))
        self.assertEqual(rc, 1)
        self.assertIn("inventoried at 26.09.3, the oracle pin is 26.10.0", out)
        self.assertIn("tools/mg harvest-inventory", out)

    def test_a_test_whose_source_changes_is_listed_and_its_row_reopened(self) -> None:
        self.run_inventory()
        links = {r["test"]: r for r in read(self.links)}
        for row in links.values():
            self.assertRegex(row["body_sha256"], r"^[0-9a-f]{64}$")
        moved = links["answering_moves_the_card"]["harvest_id"]
        rebuild = links["TestFiltered.test_rebuild"]["harvest_id"]
        steps = links["parses learning steps from text"]["harvest_id"]
        kept = links["plain_search_matches"]["harvest_id"]
        toolbar = links["test_toolbar_draws"]["harvest_id"]
        catalog = read(self.catalog)
        for row in catalog:
            if row["harvest_id"] in (moved, rebuild, steps, kept):
                row.update(classification="in-scope", reason="Covered by a scenario",
                           scenarios="sched/answer-good", status="done")
        inventory.write_csv(self.catalog, inventory.CATALOG_HEADER, catalog)

        # The next release changes three test bodies, a helper next to one of
        # them and a desktop test, and keeps every name.
        release = self.root / "anki-next"
        shutil.copytree(self.anki, release)
        edit(release, "rslib/src/scheduler/answering.rs", "fn answering_moves_the_card() {}",
             "fn answering_moves_the_card() {\n    assert_eq!(1, 1);\n}")
        edit(release, "rslib/src/scheduler/answering.rs", "fn helper() {}", "fn helper() { let _ = 1; }")
        edit(release, "pylib/tests/test_schedv3.py", "    def test_rebuild(self):\n        pass\n",
             "    def test_rebuild(self):\n        assert True\n")
        edit(release, "ts/routes/deck-options/steps.test.ts", 'test("parses learning steps from text", () => {});',
             'test("parses learning steps from text", () => { expect(1).toBe(1); });')
        edit(release, "qt/tests/test_toolbar.py", "pass", "assert True")
        summary = self.run_inventory(anki=release, tag="26.10.0")

        after = {r["test"]: r for r in read(self.links)}
        self.assertEqual(after["answering_moves_the_card"]["harvest_id"], moved)
        self.assertEqual(after["answering_moves_the_card"]["tag"], "26.10.0")
        self.assertNotEqual(after["answering_moves_the_card"]["body_sha256"],
                            links["answering_moves_the_card"]["body_sha256"])
        self.assertEqual(after["plain_search_matches"]["body_sha256"], links["plain_search_matches"]["body_sha256"])
        line = next(x for x in summary.splitlines() if "changed since 26.09.3" in x)
        self.assertIn("4 tests changed since 26.09.3", line)
        for hid in (moved, rebuild, steps, toolbar):
            self.assertIn(hid, line)
        self.assertNotIn(kept, line)
        rows = {r["harvest_id"]: r for r in read(self.catalog)}
        for hid in (moved, rebuild, steps):
            self.assertEqual(
                (rows[hid]["classification"], rows[hid]["scenarios"], rows[hid]["status"]),
                ("in-scope", "sched/answer-good", "open"),
            )
        self.assertEqual(rows[kept]["status"], "done")
        # Its path alone makes the desktop test not applicable, which the
        # inventory decides again at every release: listed, not reopened.
        self.assertEqual((rows[toolbar]["classification"], rows[toolbar]["status"]), ("not-applicable", "done"))
        self.assertIn("3 catalog rows reopened", summary)
        self.assertEqual(self.run_check()[0], 0)

        # Unchanged at the next inventory, the rows stay as reviewers leave them.
        summary = self.run_inventory(anki=release, tag="26.10.0")
        self.assertNotIn("changed since", summary)

    def test_a_test_span_ends_with_its_own_source(self) -> None:
        for rel, text in SPAN_TREE.items():
            p = self.anki / rel
            p.parent.mkdir(parents=True, exist_ok=True)
            p.write_text(text, encoding="utf-8")

        def hashes() -> dict[str, str]:
            return {t.name: t.body_sha256 for t in inventory.scan(self.anki)}

        before = hashes()
        for first, second, rel, old, new in [
            ("renders_braces", "after_braces", "rslib/src/template.rs", "fn after_braces() {}", "fn after_braces() { }"),
            ("test_first", "test_second", "pylib/tests/test_span.py", "def test_second():\n    pass",
             "def test_second():\n    return"),
            ("first", "second", "ts/lib/span.test.ts", 'test("second", () => {});', 'test("second", () => { });'),
        ]:
            edit(self.anki, rel, old, new)
            now = hashes()
            self.assertEqual(now[first], before[first], f"{first} changed with {second}")
            self.assertNotEqual(now[second], before[second], second)
            before = now
        # Code between tests is in no test's span.
        for rel, old, new in [
            ("rslib/src/template.rs", "fn helper() {}", "fn helper() { }"),
            ("pylib/tests/test_span.py", "def helper():\n    pass", "def helper():\n    return"),
            ("ts/lib/span.test.ts", "const helper = 1;", "const helper = 2;"),
        ]:
            edit(self.anki, rel, old, new)
            self.assertEqual(hashes(), before, rel)
        # The attribute or decorator and the last line belong to the test.
        for name, rel, old, new in [
            ("renders_braces", "rslib/src/template.rs", "#[should_panic]", "#[ignore]"),
            ("renders_braces", "rslib/src/template.rs", '"\\"}";\n}', '"\\"}"; \n}'),
            ("test_first", "pylib/tests/test_span.py", "@pytest.mark.skip", "@pytest.mark.slow"),
            ("test_first", "pylib/tests/test_span.py", "x = '}'", "x = '{'"),
            ("first", "ts/lib/span.test.ts", "toBe(true); // )\n});", "toBe(true); // )\n });"),
        ]:
            edit(self.anki, rel, old, new)
            now = hashes()
            self.assertNotEqual(now[name], before[name], (name, old))
            before = now

    def test_check_flags_a_source_that_disagrees_with_links(self) -> None:
        self.run_inventory()
        rows = read(self.catalog)
        rows[0]["source"] = "supplementary"
        inventory.write_csv(self.catalog, inventory.CATALOG_HEADER, rows)
        rc, out = self.run_check()
        self.assertEqual(rc, 1)
        self.assertIn(f"{rows[0]['harvest_id']}: source supplementary in the catalog, tag in links.csv", out)


if __name__ == "__main__":
    unittest.main()
