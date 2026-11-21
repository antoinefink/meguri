"""Synthetic coverage of the explicit test-data lane; no real corpus reads."""

import json
import os
import shutil
import unittest

from meguri_similarity.lexer import Lexer
from meguri_similarity.test_data import lex
from support import make_corpus, scan, synthetic_config, tmpdir, write_tree

SCENARIOS = "conformance/scenarios/search"
GOLDENS = "conformance/goldens/search"
STORY = "The implementer view names only Meguri files regions channels and scores it is what an implementer session may see"
COMMENT = "Lex tokens identifier words split from camel and snake case plus the words and numbers inside string literals and number literals"
OWN = "Every token carries the line"


class TestDataTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.shared = tmpdir()
        cls.refs = make_corpus(os.path.join(cls.shared, "corpus"), [
            ("2.1.51", {"synthetic/story.py": "STORY = " + json.dumps(STORY) + "\n# " + COMMENT + "\n"}),
            ("23.10", {}),
        ])

    @classmethod
    def tearDownClass(cls):
        shutil.rmtree(cls.shared)

    def setUp(self):
        self.root = tmpdir()
        self.out = os.path.join(self.root, "out")
        self.config = synthetic_config(self.root, "2.1.51", "23.10", 2)

    def tearDown(self):
        shutil.rmtree(self.root)

    def check(self, files, paths=None, enabled=True, flag="--test-data"):
        write_tree(self.root, files)
        return scan(self.refs, os.path.join(self.shared, "cache"), self.root,
                    [SCENARIOS, GOLDENS] if paths is None else paths, self.out,
                    config=self.config, extra=[flag] if enabled else [])

    def pair(self, value, golden=OWN):
        return {SCENARIOS + "/one.yaml": "description: " + json.dumps(value) + "\n",
                GOLDENS + "/one.json": json.dumps({"description": golden})}

    def test_data_is_still_excluded_by_default(self):
        code, summary, _views, error = self.check(self.pair(STORY),
            paths=[SCENARIOS + "/one.yaml", GOLDENS + "/one.json"], enabled=False)
        self.assertEqual(code, 0, error)
        self.assertEqual((summary["counts"]["scanned"], summary["counts"]["skipped"]), (0, 2))
        self.assertNotIn("test_data", summary)

    def test_recursive_pairs_have_hash_manifest_and_real_fingerprint_counts(self):
        files = self.pair(OWN)
        files.update({SCENARIOS + "/nested/two.json": json.dumps({"values": ["one", "two"]}),
                      GOLDENS + "/nested/two.json": json.dumps({"result": "three"})})
        code, summary, views, error = self.check(files)
        self.assertEqual(code, 0, error)
        data = summary["test_data"]
        self.assertEqual(data["pairs"], 2)
        self.assertEqual([item["path"] for item in data["manifest"]], sorted(files))
        self.assertTrue(all(len(item["sha256"]) == 64 for item in data["manifest"]))
        self.assertEqual(summary["counts"]["scanned"], 4)
        self.assertEqual(summary["counts"]["skipped"], 0)
        self.assertEqual(summary["status"], "warn")
        self.assertTrue(data["limited"])
        self.assertTrue(all(set(item["fingerprints"]) == {"lex", "com", "text"} for item in data["coverage"]))
        self.assertIn("test_data", views["implementer"])

    def test_whole_area_roots_exclude_area_manifests_from_pairing(self):
        files = self.pair(OWN)
        files[SCENARIOS + "/area.yaml"] = "area: search\nstatus: active\n"
        files[SCENARIOS + "/area.yml"] = "area: search\nstatus: pending\n"
        code, summary, _views, error = self.check(files)
        self.assertEqual(code, 0, error)
        self.assertEqual(summary["counts"]["scanned"], 2)
        self.assertEqual(summary["test_data"]["pairs"], 1)
        self.assertEqual([item["path"] for item in summary["test_data"]["manifest"]],
                         sorted(self.pair(OWN)))

    def test_copied_scalar_and_yaml_comment_are_detected_across_languages(self):
        files = self.pair(STORY)
        files[SCENARIOS + "/one.yaml"] += "# " + COMMENT + "\n"
        code, summary, views, error = self.check(files)
        self.assertEqual(code, 1, error)
        self.assertEqual(summary["status"], "fail")
        own = next(item for item in views["implementer"]["files"] if item["path"].endswith("one.yaml"))
        self.assertGreater(own["channels"]["text"]["matched"], 0)
        self.assertGreater(own["channels"]["com"]["matched"], 0)
        encoded = json.dumps(views["implementer"]) + json.dumps(summary["flags"])
        for hidden in (STORY, COMMENT, "synthetic/story.py", "23.10", "2.1.51"):
            self.assertNotIn(hidden, encoded)
        self.assertIn("synthetic/story.py", json.dumps(views["reviewer"]))

    def test_wrapped_comment_copy_is_one_block_with_source_lines(self):
        words = COMMENT.split()
        wrapped = ["# " + " ".join(words[index:index + 4]) for index in range(0, len(words), 4)]
        self.assertTrue(all(len(line.split()) - 1 < 10 for line in wrapped))
        files = self.pair(OWN)
        files[SCENARIOS + "/one.yaml"] += "\n".join(wrapped) + "\n"
        code, summary, views, error = self.check(files)
        self.assertEqual(code, 1, error)
        self.assertEqual(summary["status"], "fail")
        item = next(item for item in views["implementer"]["files"] if item["path"].endswith("one.yaml"))
        self.assertGreater(item["channels"]["com"]["matched"], 0)
        flag = next(flag for flag in item["flags"] if flag["channel"] == "com")
        self.assertEqual(flag["regions"][0]["lines"][0], 2)
        self.assertGreaterEqual(flag["regions"][0]["lines"][1], 4)
        separated = "# " + " ".join(words[:8]) + "\nseparator: value\n# " + " ".join(words[8:]) + "\n"
        blocks = lex(separated, SCENARIOS + "/one.yaml", Lexer()).comments
        self.assertEqual([len(block[0]) for block in blocks], [8, len(words) - 8])

    def test_copied_json_string_and_independent_data(self):
        code, summary, views, error = self.check(self.pair(OWN, STORY))
        self.assertEqual(code, 1, error)
        self.assertTrue(any(item["path"].startswith(GOLDENS) for item in summary["flags"]))
        independent = "String literals are namespaced by language family so struct tokens only match within one family"
        code, summary, _views, error = self.check(self.pair(independent, independent))
        self.assertEqual(code, 0, error)
        self.assertEqual(summary["counts"]["flagged"], 0)
        self.assertEqual(summary["status"], "pass")
        self.assertFalse(summary["test_data"]["limited"])

    def test_every_area_pairs_within_itself(self):
        files = self.pair(OWN)
        files.update({"conformance/scenarios/sched/area.yaml": "area: sched\n",
                      "conformance/scenarios/sched/one.yaml": "description: " + json.dumps(STORY) + "\n",
                      "conformance/goldens/sched/one.json": json.dumps({"description": OWN}),
                      "conformance/scenarios/fp-probe/nested/two.json": json.dumps({"values": ["a"]}),
                      "conformance/goldens/fp-probe/nested/two.json": json.dumps({"result": "b"})})
        code, summary, _views, error = self.check(files, paths=["conformance/scenarios", "conformance/goldens"])
        self.assertEqual(code, 1, error)
        self.assertEqual(summary["test_data"]["pairs"], 3)
        self.assertEqual(summary["counts"]["scanned"], 6)
        self.assertEqual({item["path"] for item in summary["flags"]}, {"conformance/scenarios/sched/one.yaml"})
        code, summary, _views, error = self.check({}, paths=["conformance/scenarios/fp-probe", "conformance/goldens/fp-probe"])
        self.assertEqual((code, summary["test_data"]["pairs"]), (0, 1), error)

    def test_json_with_characters_yaml_refuses_is_still_scanned(self):
        golden = '{"description": "' + STORY.replace(" ", " \u007f", 1) + '", "raw": "a\u0085b\u0080c"}'
        self.assertIn("\x7f", golden)
        files = self.pair(OWN)
        files[GOLDENS + "/one.json"] = golden
        code, summary, _views, error = self.check(files)
        self.assertEqual(code, 1, error)
        self.assertEqual(summary["counts"]["errors"], 0)
        self.assertEqual({item["path"] for item in summary["flags"]}, {GOLDENS + "/one.json"})
        words = [word for item in lex(golden, GOLDENS + "/one.json", Lexer()).texts for word in item[0]]
        self.assertIn("implementer", words)
        self.assertEqual(lex(golden, GOLDENS + "/one.json", Lexer()).texts[0][1][0], 1)

    def test_the_old_flag_still_selects_the_lane(self):
        code, summary, _views, error = self.check(self.pair(STORY), flag="--search-data")
        self.assertEqual(code, 1, error)
        self.assertIn("test_data", summary)

    def test_names_pair_only_within_one_area(self):
        files = {SCENARIOS + "/one.yaml": "description: value\n",
                 "conformance/goldens/sched/one.json": "{}"}
        code, summary, _views, _error = self.check(files, paths=["conformance/scenarios", "conformance/goldens"])
        self.assertEqual((code, summary["status"]), (2, "error"))
        self.assertIn("same area", summary["error"])

    def test_outside_missing_unsupported_unpaired_and_empty_inputs_refuse(self):
        cases = [
            ({}, []),
            ({}, [SCENARIOS + "/missing.yaml", GOLDENS]),
            ({"core/one.json": "{}"}, ["core/one.json"]),
            ({"conformance/goldens-apple/search/one.json": "{}"}, ["conformance/goldens-apple/search/one.json"]),
            ({"conformance/scenarios/one.yaml": "x: y", "conformance/goldens/one.json": "{}"},
             ["conformance/scenarios", "conformance/goldens"]),
            ({}, [SCENARIOS + "/../../../../outside.json"]),
            ({SCENARIOS + "/one.txt": "text"}, [SCENARIOS]),
            ({SCENARIOS + "/one.yaml": "description: value\n"}, [SCENARIOS]),
            ({SCENARIOS + "/one.yaml": "x: y", SCENARIOS + "/one.yml": "x: y",
              GOLDENS + "/one.json": "{}"}, [SCENARIOS, GOLDENS]),
        ]
        for files, paths in cases:
            with self.subTest(paths=paths):
                for directory in ("conformance",):
                    shutil.rmtree(os.path.join(self.root, directory), ignore_errors=True)
                code, summary, _views, _error = self.check(files, paths=paths)
                self.assertEqual(code, 2)
                self.assertEqual(summary["status"], "error")

    def test_links_and_nonregular_files_cannot_import_other_content(self):
        for kind in ("file", "directory", "hardlink", "fifo"):
            with self.subTest(kind=kind):
                destination = os.path.join(self.root, SCENARIOS, kind + ".yaml")
                os.makedirs(os.path.dirname(destination), exist_ok=True)
                outside = os.path.join(self.root, "outside")
                os.makedirs(outside, exist_ok=True)
                target = os.path.join(outside, "payload")
                with open(target, "w") as stream:
                    stream.write("description: " + STORY)
                if kind == "file":
                    os.symlink(target, destination)
                elif kind == "directory":
                    os.symlink(outside, destination)
                elif kind == "hardlink":
                    os.link(target, destination)
                else:
                    os.mkfifo(destination)
                code, summary, _views, _error = self.check({}, paths=[destination])
                self.assertEqual(code, 2)
                self.assertNotIn(STORY, json.dumps(summary))
                os.unlink(destination)

    def test_invalid_data_reports_errors(self):
        for value in ("x: [broken", "x: &loop [*loop]", "x: !unknown value", "x: !unknown {}"):
            with self.subTest(value=value):
                files = self.pair(OWN)
                files[SCENARIOS + "/one.yaml"] = value
                code, summary, _views, _error = self.check(files)
                self.assertEqual(code, 2)
                self.assertEqual(summary["counts"]["errors"], 1)
                self.assertNotIn(value, json.dumps(summary))
        files = self.pair(OWN)
        files[GOLDENS + "/one.json"] = '{"bad": NaN}'
        code, summary, _views, _error = self.check(files)
        self.assertEqual(code, 2)
        self.assertEqual(summary["counts"]["errors"], 1)

    def test_decoded_and_folded_scalar_lines_and_comments(self):
        source = 'description: >\n  Every token carries\n  the line it starts on\n# comment blocks lowercased words of each run\n'
        value = lex(source, SCENARIOS + "/one.yaml", Lexer())
        words, lines, _label = next(item for item in value.texts if len(item[0]) > 1)
        self.assertEqual(words[:3], ["every", "token", "carries"])
        self.assertEqual(lines[:3], [2, 2, 2])
        self.assertEqual(lines[3:], [3] * (len(lines) - 3))
        self.assertEqual(value.comments[0][1], [4] * len(value.comments[0][0]))
        decoded = lex('{"description":"\\u0045very token carries the line it starts on"}',
                      GOLDENS + "/one.json", Lexer())
        self.assertTrue(any(item[0][0] == "every" for item in decoded.texts))
        self.assertTrue(all(line == 1 for item in decoded.texts for line in item[1]))


if __name__ == "__main__":
    unittest.main()
