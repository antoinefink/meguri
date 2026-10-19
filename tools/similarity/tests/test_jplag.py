"""Required JPlag references, using synthetic tag listings and a stub Java run."""

import contextlib
import io
import json
import os
import shutil
import subprocess
import unittest
from types import SimpleNamespace
from unittest.mock import patch

from support import CONFIG, read_json, tmpdir, write_tree

from meguri_similarity import jplag
from meguri_similarity.config import load_config


class RequiredReferences(unittest.TestCase):
    def setUp(self):
        self.base = tmpdir()
        self.cfg = load_config(CONFIG)
        self.cfg["corpus"]["tags"] = {"min": "1.0", "max": "2.0", "count": 2}
        self.cfg["jplag"]["tags"] = ["1.0", "2.0", "latest"]
        write_tree(self.base, {"core/own.rs": "pub fn own() {}\n", "files.lst": "core/own.rs\n"})
        self.args = SimpleNamespace(
            config="synthetic", refs=os.path.join(self.base, "synthetic-refs"), root=self.base,
            files_from=os.path.join(self.base, "files.lst"), null=False,
            out=os.path.join(self.base, "out"), jar="synthetic.jar",
        )

    def tearDown(self):
        shutil.rmtree(self.base, ignore_errors=True)

    def run_jplag(self, releases):
        def run(cmd, **_kwargs):
            if cmd[0] == "git":
                return subprocess.CompletedProcess(cmd, 0, stdout="\n".join(releases))
            self.assertEqual(cmd[0], "java")
            result = cmd[cmd.index("-r") + 1]
            write_tree(result, {"results.csv": (
                "submissionName1,submissionName2,averageSimilarity,maxSimilarity\n"
                "new/meguri,old/1.0,0.2,0.3\n"
                "old/2.0,new/meguri,0.4,0.5\n"
                "old/1.0,old/2.0,1.0,1.0\n"
            )})
            with open(result + ".jplag", "wb"):
                pass
            return subprocess.CompletedProcess(cmd, 0, stdout="", stderr="")

        with patch.object(jplag, "load_config", return_value=self.cfg), \
                patch.object(jplag.subprocess, "run", side_effect=run) as process, \
                patch.object(jplag, "_export_tag", return_value=1) as export, \
                contextlib.redirect_stdout(io.StringIO()) as stdout:
            code = jplag.main(self.args, lambda _message: None)
        summary = read_json(os.path.join(self.args.out, "jplag-summary.json"))
        self.assertEqual(json.loads(stdout.getvalue())["status"], summary["status"])
        java_calls = [c for c in process.call_args_list if c.args[0][0] == "java"]
        return code, summary, export, java_calls

    def assert_incomplete(self, releases, missing_count, selected_count):
        code, summary, export, java_calls = self.run_jplag(releases)
        self.assertEqual((code, summary["status"]), (2, "error"))
        self.assertEqual(summary["languages"], {})
        self.assertEqual(len(summary["missing_tags"]), missing_count)
        self.assertEqual(len(summary["tags"]), selected_count)
        self.assertEqual(summary["error"], (
            f"required JPlag references are incomplete: {missing_count} configured tag(s) missing; "
            f"{selected_count} tag(s) selected"
        ))
        export.assert_not_called()
        self.assertEqual(len(java_calls), 0)

    def test_missing_all_fails_even_without_own_inputs(self):
        write_tree(self.base, {"files.lst": ""})
        self.assert_incomplete([], 3, 0)

    def test_missing_subset_never_starts_comparison(self):
        self.assert_incomplete(["1.0"], 2, 1)

    def test_latest_only_requires_pinned_maximum(self):
        self.cfg["jplag"]["tags"] = ["latest"]
        self.assert_incomplete(["1.0"], 1, 0)

    def test_latest_with_older_references_requires_pinned_maximum(self):
        self.cfg["jplag"]["tags"] = ["latest", "1.0", "1.5"]
        self.assert_incomplete(["1.0", "1.5"], 1, 2)

    def test_no_selected_tags_fails(self):
        self.cfg["jplag"]["tags"] = []
        self.assert_incomplete(["1.0", "2.0"], 0, 0)

    def test_complete_selected_tags_compare_and_skip_languages_without_inputs(self):
        code, summary, export, java_calls = self.run_jplag(["1.0", "2.0"])
        self.assertEqual((code, summary["status"]), (0, "warn"))
        self.assertEqual(summary["tags"], ["1.0", "2.0"])
        self.assertEqual(summary["missing_tags"], [])
        self.assertNotIn("error", summary)
        self.assertEqual([c.args[1] for c in export.call_args_list], ["1.0", "2.0"])
        self.assertEqual(len(java_calls), 1)
        rust = summary["languages"]["rust"]
        self.assertEqual([c["tag"] for c in rust["comparisons"]], ["2.0", "1.0"])
        self.assertEqual(rust["max_similarity"], 0.5)
        self.assertTrue(os.path.isfile(rust["report"]))
        self.assertEqual(summary["languages"]["typescript"]["skipped"], "no Meguri files in this language")


if __name__ == "__main__":
    unittest.main()
