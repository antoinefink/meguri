"""Scanner behavior against a small synthetic corpus (no Anki source involved).

The "corpus" files below are written for these tests; they stand in for Anki
sources so every channel, the allowlist, the scope rules and the report views
can be checked without the real corpus.
"""

import json
import os
import re
import shutil
import unittest

from support import CONFIG, file_entry, make_corpus, run_cli, scan, tmpdir, write_tree

CORPUS_RS = '''\
// Spreads the given number of items over buckets so that no bucket exceeds the
// limit, preferring the buckets that are currently the least loaded of all.
pub fn spread_items(buckets: &mut Vec<Bucket>, mut remaining: usize, limit: usize) -> Result<usize, SpreadError> {
    if buckets.is_empty() {
        return Err(SpreadError::NoBuckets);
    }
    let mut placed = 0;
    while remaining > 0 {
        let mut best: Option<usize> = None;
        for (index, bucket) in buckets.iter().enumerate() {
            if bucket.load >= limit {
                continue;
            }
            match best {
                Some(current) if buckets[current].load <= bucket.load => {}
                _ => best = Some(index),
            }
        }
        let Some(chosen) = best else {
            return Err(SpreadError::Full { placed, remaining });
        };
        let room = limit - buckets[chosen].load;
        let step = room.min(remaining).max(1);
        buckets[chosen].load += step;
        buckets[chosen].history.push(step as u32);
        remaining -= step;
        placed += step;
        if buckets[chosen].history.len() > 16 {
            buckets[chosen].history.remove(0);
        }
    }
    log::debug!("spread {} items over {} buckets", placed, buckets.len());
    Ok(placed)
}

pub fn bucket_report(buckets: &[Bucket]) -> String {
    let mut out = String::new();
    for (i, b) in buckets.iter().enumerate() {
        out.push_str(&format!("bucket {} holds {} of {}\\n", i, b.load, b.capacity));
    }
    out
}
'''

RENAMES = {
    "spread_items": "distribute", "buckets": "bins", "Bucket": "Bin", "remaining": "left_over",
    "limit": "cap", "SpreadError": "PlaceError", "NoBuckets": "Empty", "placed": "done", "best": "pick",
    "index": "idx", "bucket": "bin", "load": "fill", "current": "cur", "chosen": "target",
    "Full": "Saturated", "room": "space", "step": "chunk", "history": "trail", "bucket_report": "describe",
    "out": "text", "capacity": "size",
}


def renamed_and_stripped(src):
    src = re.sub(r"//[^\n]*\n", "", src)
    return re.sub(r"\b[A-Za-z_]\w*\b", lambda m: RENAMES.get(m.group(0), m.group(0)), src)


CORPUS_FTL = """\
## Deck screen
deck-empty-message = This deck has no cards that are ready for study at the moment.
deck-title = Decks
again-button = Again
"""

CORPUS_CSS = """\
.panel {
  font-family: system-ui, sans-serif;
  font-size: 18px;
  text-align: left;
  color: navy;
  background-color: ivory;
}
"""

MEGURI_OWN_RS = '''\
/// Returns the median of the values, or `None` for an empty slice.
pub fn median(values: &[f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mid = sorted.len() / 2;
    Some(if sorted.len() % 2 == 0 { (sorted[mid - 1] + sorted[mid]) / 2.0 } else { sorted[mid] })
}

/// Counts how often each word occurs, case-insensitively.
pub fn word_counts(text: &str) -> std::collections::BTreeMap<String, usize> {
    let mut counts = std::collections::BTreeMap::new();
    for word in text.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()) {
        *counts.entry(word.to_lowercase()).or_insert(0) += 1;
    }
    counts
}
'''


def field_struct(name, fields):
    lines = [f"    pub {f}: {t}," for f, t in fields]
    return "#[derive(Debug, Clone, PartialEq)]\npub struct " + name + " {\n" + "\n".join(lines) + "\n}\n"


CORPUS_STRUCT = field_struct(
    "ReviewSettings",
    [(f"setting_{i}", t) for i, t in enumerate(["u32", "f32", "bool", "u32", "i64", "f32", "u32", "bool"] * 3)],
)


def catalog(values):
    strings = {
        f"key{i}": {"localizations": {"en": {"stringUnit": {"state": "translated", "value": v}}}}
        for i, v in enumerate(values)
    }
    return json.dumps({"sourceLanguage": "en", "strings": strings, "version": "1.0"}, indent=2)


class SyntheticCorpus(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.base = tmpdir()
        cls.refs = make_corpus(
            os.path.join(cls.base, "refs"),
            [
                ("2.1.49", {"old/only_old.rs": MEGURI_OWN_RS}),
                ("2.1.51", {"old/only_old.rs": None, "rslib/src/sched/spread.rs": CORPUS_RS, "ftl/core/deck.ftl": CORPUS_FTL,
                            "rslib/src/config.rs": CORPUS_STRUCT}),
                ("2.1.52rc1", {"rc/only_rc.py": "def rc_only():\n    return [x * 3 for x in range(40)]\n" * 8}),
                ("23.10", {"rc/only_rc.py": None, "ts/panel.css": CORPUS_CSS}),
                ("27.01", {"future/only_future.rs": MEGURI_OWN_RS.replace("median", "middle")}),
            ],
        )
        cls.cache = os.path.join(cls.base, "cache")

    def setUp(self):
        self.root = tmpdir()
        self.out = os.path.join(self.root, ".out")

    def tearDown(self):
        shutil.rmtree(self.root, ignore_errors=True)

    def scan(self, files, paths=None, **kw):
        write_tree(self.root, files)
        return scan(self.refs, self.cache, self.root, paths or list(files), self.out, **kw)

    def allowlist(self, entries):
        path = os.path.join(self.root, "allow.yaml")
        with open(path, "w") as f:
            json.dump({"entries": entries}, f)  # JSON is valid YAML
        return path

    # -- detection -----------------------------------------------------------

    def test_copied_function_with_renamed_identifiers_fails_on_struct(self):
        code, summary, views, err = self.scan({"core/crates/meguri-sched/src/spread.rs": renamed_and_stripped(CORPUS_RS)})
        self.assertEqual(code, 1, err)
        self.assertEqual(summary["status"], "fail")
        f = file_entry(views["implementer"], "core/crates/meguri-sched/src/spread.rs")
        self.assertEqual(f["status"], "flagged")
        self.assertGreaterEqual(f["channels"]["struct"]["score"], 0.9)
        flag = next(x for x in f["flags"] if x["channel"] == "struct")
        self.assertTrue(flag["regions"])
        self.assertEqual(flag["action"], "rewrite this region from the spec")
        r = file_entry(views["reviewer"], "core/crates/meguri-sched/src/spread.rs")
        anki = next(x for x in r["flags"] if x["channel"] == "struct")["regions"][0]["anki"][0]
        self.assertEqual((anki["path"], anki["tag"], anki["source"]), ("rslib/src/sched/spread.rs", "2.1.51", "anki"))

    def test_implementer_view_never_names_the_corpus(self):
        self.scan({"core/a.rs": CORPUS_RS})
        with open(os.path.join(self.out, "implementer.json")) as f:
            text = f.read()
        with open(os.path.join(self.out, "implementer.md")) as f:
            text += f.read()
        with open(os.path.join(self.out, "summary.json")) as f:
            flags = json.dumps(json.load(f)["flags"])
        for needle in ("rslib/", "spread.rs", "2.1.51", '"anki"'):
            self.assertNotIn(needle, text)
            self.assertNotIn(needle, flags)
        with open(os.path.join(self.out, "reviewer.md")) as f:
            self.assertIn("rslib/src/sched/spread.rs", f.read())

    def test_unrelated_field_lists_do_not_match(self):
        # Long field lists look alike once names are abstracted; the periodicity
        # filter keeps such declarations out of the struct channel.
        own = field_struct("ImportOptions", [(f"opt_{i}", t) for i, t in enumerate(["bool", "u32", "f32", "u32", "i64", "bool"] * 4)])
        code, _s, views, err = self.scan({"core/import.rs": own})
        self.assertEqual(code, 0, err)
        f = file_entry(views["implementer"], "core/import.rs")
        self.assertEqual(f["channels"]["struct"]["matched"], 0)

    def test_own_code_passes(self):
        code, summary, views, err = self.scan({"core/stats.rs": MEGURI_OWN_RS})
        self.assertEqual(code, 0, err)
        f = file_entry(views["implementer"], "core/stats.rs")
        self.assertEqual(f["status"], "clean")
        self.assertLessEqual(f["channels"]["struct"]["score"], 0.05)
        self.assertLessEqual(f["channels"]["lex"]["score"], 0.05)

    def test_corpus_is_release_tags_in_range_only(self):
        code, summary, views, err = self.scan(
            {
                "core/old.rs": MEGURI_OWN_RS,
                "core/future.rs": MEGURI_OWN_RS.replace("median", "middle"),
                "tools/rc.py": "def rc_only():\n    return [x * 3 for x in range(40)]\n" * 8,
            }
        )
        self.assertEqual(code, 0, err)
        self.assertEqual(summary["corpus"]["tags"], 2)
        self.assertEqual((summary["corpus"]["first_tag"], summary["corpus"]["last_tag"]), ("2.1.51", "23.10"))
        self.assertEqual(summary["corpus"]["missing_boundary_tags"], ["2.1.50", "26.09.3"])

    def test_verbatim_comment_fails_on_com_even_when_code_differs(self):
        comment = (
            "// Spreads the given number of items over buckets so that no bucket exceeds the\n"
            "// limit, preferring the buckets that are currently the least loaded of all.\n"
        )
        code, _s, views, err = self.scan({"core/x.rs": comment + MEGURI_OWN_RS})
        self.assertEqual(code, 1, err)
        f = file_entry(views["implementer"], "core/x.rs")
        self.assertEqual([x["channel"] for x in f["flags"]], ["com"])
        self.assertEqual(f["flags"][0]["regions"][0]["lines"], [1, 2])

    def test_nine_shared_comment_words_pass(self):
        comment = "// spreads the given number of items over buckets so\n"
        code, _s, views, err = self.scan({"core/x.rs": comment + MEGURI_OWN_RS})
        self.assertEqual(code, 0, err)

    def test_string_catalog_value_with_a_copied_sentence_fails_on_text(self):
        sentence = "This deck has no cards that are ready for study at the moment."
        code, _s, views, err = self.scan(
            {"app/Meguri/Localizable.xcstrings": catalog([sentence, "Again", "Your deck has no cards that are ready now."])}
        )
        self.assertEqual(code, 1, err)
        f = file_entry(views["implementer"], "app/Meguri/Localizable.xcstrings")
        self.assertEqual([x["channel"] for x in f["flags"]], ["text"])
        self.assertEqual(f["flags"][0]["regions"][0]["keys"], ["key0"])

    def test_single_words_and_seven_word_overlaps_pass(self):
        code, _s, views, err = self.scan(
            {"app/Meguri/Localizable.xcstrings": catalog(["Again", "Decks", "No cards that are ready for study now"])}
        )
        self.assertEqual(code, 0, err)
        self.assertEqual(file_entry(views["implementer"], "app/Meguri/Localizable.xcstrings")["status"], "clean")

    def test_css_with_a_copied_run_fails_on_text(self):
        copied = ".card {\n  font-family: system-ui, sans-serif;\n  font-size: 18px;\n  text-align: left;\n  color: navy;\n}\n"
        own = ".card {\n  margin: 0 auto;\n  max-width: 40em;\n  color: var(--text);\n}\n"
        code, _s, views, err = self.scan({"app/a.css": copied, "app/b.css": own})
        self.assertEqual(code, 1, err)
        self.assertEqual(file_entry(views["implementer"], "app/a.css")["status"], "flagged")
        self.assertEqual(file_entry(views["implementer"], "app/b.css")["status"], "clean")

    # -- allowlist -----------------------------------------------------------

    def test_allowlisted_path_passes_and_is_reported(self):
        allow = self.allowlist(
            [{"path": "core/**/schema.rs", "reason": "interop fact (test)", "approved_by": "antoine"}]
        )
        code, summary, views, err = self.scan({"core/storage/schema.rs": CORPUS_RS}, allowlist=allow)
        self.assertEqual(code, 0, err)
        f = file_entry(views["implementer"], "core/storage/schema.rs")
        self.assertEqual(f["status"], "allowlisted")
        self.assertEqual({a["channel"] for a in f["allowlisted"]} >= {"struct"}, True)
        self.assertEqual(f["allowlisted"][0]["entries"], [1])
        self.assertEqual(summary["counts"]["allowlisted"], 1)

    def test_allowlist_lines_and_channels_limit_the_exemption(self):
        allow = self.allowlist(
            [{"path": "core/storage/schema.rs", "lines": "1-5", "channels": ["struct"], "reason": "r", "approved_by": "antoine"}]
        )
        code, _s, views, err = self.scan({"core/storage/schema.rs": CORPUS_RS}, allowlist=allow)
        self.assertEqual(code, 1, err)
        self.assertEqual(file_entry(views["implementer"], "core/storage/schema.rs")["status"], "flagged")

    def test_allowlist_entries_need_a_reason_and_antoines_approval(self):
        for entry in (
            {"path": "core/a.rs", "reason": "r"},
            {"path": "core/a.rs", "reason": "r", "approved_by": "agent"},
            {"path": "core/a.rs", "approved_by": "antoine"},
            {"path": "core/a.rs", "reason": "r", "approved_by": "antoine", "lines": "9-1"},
        ):
            code, out, err = run_cli(
                "scan", "--refs", self.refs, "--cache", self.cache, "--root", self.root, "--out", self.out,
                "--allowlist", self.allowlist([entry]), "core/a.rs",
            )
            self.assertEqual(code, 2, entry)
            self.assertEqual(out["status"], "error")

    # -- interop vocabulary --------------------------------------------------

    def test_interop_vocabulary_masks_shared_names(self):
        # The corpus file's names and literals in the same order but another
        # structure: only the lex channel can see it.
        from meguri_similarity.lexer import Lexer

        import yaml

        with open(CONFIG) as f:
            cfg = yaml.safe_load(f)
        cfg["channels"]["lex"]["string_stopwords"] = []  # isolate masking from stopwords
        config = os.path.join(self.root, "config.yaml")
        with open(config, "w") as f:
            yaml.safe_dump(cfg, f)
        words = Lexer().lex(CORPUS_RS, "rust").lex
        files = {"tools/reuse.py": "NAMES = " + json.dumps(" ".join(words)) + "\n"}
        code, _s, views, err = self.scan(files, config=config)
        lex = file_entry(views["implementer"], "tools/reuse.py")["channels"]["lex"]
        self.assertGreater(lex["score"], 0.9, err)
        vocab = os.path.join(self.root, "specs", "sched", "interop-vocab.txt")
        os.makedirs(os.path.dirname(vocab))
        with open(vocab, "w") as f:
            f.write("\n".join(sorted(set(w for w in words if not w.isdigit()))) + "\n")
        code2, summary2, views2, err2 = scan(
            self.refs, self.cache, self.root, list(files), self.out, vocab_root=self.root, config=config
        )
        lex2 = file_entry(views2["implementer"], "tools/reuse.py")["channels"]["lex"]
        self.assertEqual(code2, 0, err2)
        self.assertEqual(lex2["fingerprints"], 0)
        self.assertTrue(summary2["corpus"]["built"], "a vocabulary change must rebuild the index")

    # -- scope and errors ----------------------------------------------------

    def test_scope_skips_quarantine_prose_and_unknown_types(self):
        files = {
            "oracle/driver/run.py": CORPUS_RS,
            "docs/notes.md": "x",
            "specs/sched/spread.md": "x",
            "conformance/scenarios/sched/a.yaml": "x",
            "tools/run.sh": "echo",
        }
        code, summary, _v, err = self.scan(files)
        self.assertEqual(code, 0, err)
        self.assertEqual(summary["counts"]["scanned"], 0)
        self.assertEqual(summary["counts"]["skipped"], 5)

    def test_swift_uses_only_the_cross_language_channels(self):
        code, _s, views, err = self.scan({"app/A.swift": "func f() -> Int { return 1 }\n"})
        self.assertEqual(code, 0, err)
        self.assertEqual(sorted(file_entry(views["implementer"], "app/A.swift")["channels"]), ["com", "lex"])

    def test_invalid_string_catalog_is_an_error(self):
        code, summary, _v, err = self.scan({"app/L.xcstrings": "{ not json"})
        self.assertEqual(code, 2, err)
        self.assertEqual(summary["counts"]["errors"], 1)

    def test_index_is_reused_until_the_corpus_changes(self):
        cache = os.path.join(self.root, "cache")
        refs = os.path.join(self.root, "refs")
        shutil.copytree(self.refs, refs, symlinks=True)
        files = {"core/stats.rs": MEGURI_OWN_RS}
        write_tree(self.root, files)
        _c, s1, _v, _e = scan(refs, cache, self.root, list(files), self.out)
        _c, s2, _v, _e = scan(refs, cache, self.root, list(files), self.out)
        from support import git

        git(os.path.join(refs, "anki"), "tag", "24.06", "23.10")
        _c, s3, _v, _e = scan(refs, cache, self.root, list(files), self.out)
        self.assertEqual((s1["corpus"]["built"], s2["corpus"]["built"], s3["corpus"]["built"]), (True, False, True))
        self.assertNotEqual(s1["corpus"]["key"], s3["corpus"]["key"])
        self.assertEqual(sorted(os.listdir(cache)), [s3["corpus"]["key"]])

    def test_missing_corpus_is_an_error(self):
        code, out, err = run_cli("scan", "--refs", self.root, "--cache", self.cache, "--root", self.root, "--out", self.out)
        self.assertEqual(code, 2)
        self.assertIn("no Anki repository", out["error"])


if __name__ == "__main__":
    unittest.main()
