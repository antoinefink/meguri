"""Calibration against the real Anki corpus (mounted read-only at /refs).

Positives are generated here, in the container, from the corpus and written
only to temporary directories; they are never written into the repository.
Assertion messages carry scores and counts only, never corpus text.
"""

import glob
import json
import os
import re
import shutil
import subprocess
import time
import unittest

from support import FIXTURES, REFS, file_entry, record_calibration, run_cli, scan, tmpdir, write_tree

from meguri_similarity.corpus import _RELEASE_TAG, version_tuple
from meguri_similarity.lexer import KEYWORDS, TEXT_WORD, _SEGMENT_RE

ANKI = os.path.join(REFS, "anki")
HAVE_CORPUS = os.path.isdir(ANKI)
FSRS = os.environ.get("FSRS_CALIBRATION_DIR", "/opt/calibration/fsrs")
# fsrs-rs files (relative to the crate root) that contain text taken from Anki.
FSRS_ANKI_DERIVED = {"benches/benchmark.rs", "src/convertor_tests.rs", "src/simulation.rs", "src/optimal_retention.rs"}

_STATE = {}


def corpus_git(*args):
    return subprocess.run(["git", "-C", ANKI, *args], check=True, capture_output=True).stdout


def release_tags():
    tags = corpus_git("tag", "--list").decode().split()
    lo, hi = version_tuple("2.1.50"), version_tuple("26.09.3")
    return sorted((t for t in tags if _RELEASE_TAG.match(t) and lo <= version_tuple(t) <= hi), key=version_tuple)


def files_at(tag, prefix, suffix):
    names = corpus_git("ls-tree", "-r", "--name-only", tag, "--", prefix).decode().splitlines()
    return sorted(n for n in names if n.endswith(suffix))


def blob(tag, path):
    return corpus_git("show", f"{tag}:{path}").decode("utf-8", "replace")


def blank(text, language):
    """Comments and strings replaced by spaces (same length, newlines kept)."""
    return _SEGMENT_RE[language].sub(lambda m: re.sub(r"[^\n]", " ", m.group(0)), text)


def rust_functions(text):
    """(start, end) spans of every `fn` item with a body."""
    b = blank(text, "rust")
    spans = []
    for m in re.finditer(r"\bfn\s+[A-Za-z_]\w*", b):
        open_at = b.find("{", m.end())
        semi = b.find(";", m.end())
        if open_at < 0 or (0 <= semi < open_at):
            continue
        depth, i = 0, open_at
        while i < len(b):
            if b[i] == "{":
                depth += 1
            elif b[i] == "}":
                depth -= 1
                if depth == 0:
                    spans.append((m.start(), i + 1))
                    break
            i += 1
    return spans


def renamed_without_comments(src):
    """Comments dropped, every non-keyword identifier outside strings renamed."""
    names = {}
    out, last = [], 0
    for m in _SEGMENT_RE["rust"].finditer(src):
        out.append(_rename(src[last : m.start()], names))
        if m.group("com") is None:
            out.append(m.group(0))
        last = m.end()
    out.append(_rename(src[last:], names))
    return "".join(out)


def _rename(code, names):
    def sub(m):
        w = m.group(0)
        if w in KEYWORDS["rust"]:
            return w
        return names.setdefault(w, f"v{len(names)}")

    return re.sub(r"\b[A-Za-z_]\w*\b", sub, code)


def shared_index():
    """The calibration index: built once (timed) with the storage interop
    vocabulary, then reused by every calibration test."""
    if "root" not in _STATE:
        base = tmpdir()
        vocab_root = os.path.join(base, "vocab")
        os.makedirs(os.path.join(vocab_root, "specs", "storage"))
        shutil.copy(os.path.join(FIXTURES, "interop-vocab-storage.txt"), os.path.join(vocab_root, "specs", "storage", "interop-vocab.txt"))
        cache = os.path.join(base, "cache")
        t0 = time.monotonic()
        code, out, err = run_cli("index", "--refs", REFS, "--cache", cache, "--vocab-root", vocab_root, "--rebuild")
        _STATE.update(base=base, vocab_root=vocab_root, cache=cache, index_code=code, index=out, index_wall=time.monotonic() - t0, root=True)
    return _STATE


@unittest.skipUnless(HAVE_CORPUS, "no Anki corpus mounted at /refs (run tools/mg similarity-selftest from the main checkout)")
class Calibration(unittest.TestCase):
    def setUp(self):
        self.s = shared_index()
        self.assertEqual(self.s["index_code"], 0, "index build failed")
        self.root = tmpdir()
        self.out = os.path.join(self.root, ".out")

    def tearDown(self):
        shutil.rmtree(self.root, ignore_errors=True)

    def scan(self, files, allowlist=None):
        write_tree(self.root, files)
        return scan(REFS, self.s["cache"], self.root, list(files), self.out, allowlist=allowlist, vocab_root=self.s["vocab_root"])

    def test_00_index_over_every_tag_builds_within_30s(self):
        corpus = self.s["index"]["corpus"]
        record_calibration(
            "index",
            {"tags": corpus["tags"], "blobs": corpus["blobs"], "build_seconds": corpus["build_seconds"],
             "wall_seconds": round(self.s["index_wall"], 2), "first_tag": corpus["first_tag"], "last_tag": corpus["last_tag"]},
        )
        self.assertGreaterEqual(corpus["tags"], 54)
        self.assertEqual(corpus["last_tag"], "26.09.3")
        self.assertLess(corpus["build_seconds"], 30)

    def test_ae85_scheduler_function_renamed_and_stripped_fails(self):
        tag = next(t for t in release_tags() if version_tuple(t) >= version_tuple("2.1.51"))
        best = None
        for path in files_at(tag, "rslib/src/scheduler", ".rs"):
            text = blob(tag, path)
            for start, end in rust_functions(text):
                size = len(blank(text[start:end], "rust").split())
                if best is None or size > best[0]:
                    best = (size, path, text[start:end])
        self.assertIsNotNone(best, "no scheduler function found")
        meguri_path = "core/crates/meguri-sched/src/calibration_positive.rs"
        code, summary, views, _err = self.scan({meguri_path: renamed_without_comments(best[2]) + "\n"})
        imp = file_entry(views["implementer"], meguri_path)
        rev = file_entry(views["reviewer"], meguri_path)
        struct = imp["channels"]["struct"]
        record_calibration("ae85_positive", {"tag": tag, "struct": struct, "lex": imp["channels"]["lex"]})
        self.assertEqual(code, 1)
        self.assertEqual(summary["status"], "fail")
        self.assertGreaterEqual(struct["score"], 0.9, f"struct score {struct['score']}")
        self.assertIn("struct", [f["channel"] for f in imp["flags"]])
        imp_regions = next(f for f in imp["flags"] if f["channel"] == "struct")["regions"]
        rev_regions = next(f for f in rev["flags"] if f["channel"] == "struct")["regions"]
        self.assertTrue(imp_regions)
        self.assertEqual([r["lines"] for r in imp_regions], [r["lines"] for r in rev_regions])
        self.assertTrue(all("anki" not in r for r in imp_regions))
        self.assertTrue(rev_regions[0]["anki"][0]["path"].startswith("rslib/src/scheduler"))

    def _fsrs_files(self, wanted):
        files = {}
        for src in sorted(glob.glob(os.path.join(FSRS, "**", "*.rs"), recursive=True)):
            rel = os.path.relpath(src, FSRS)
            if wanted(rel.split("/", 1)[1]):
                with open(src, encoding="utf-8") as f:
                    files["core/fsrs-calibration/" + rel] = f.read()
        return files

    def test_fsrs_rs_sources_stay_below_5_percent(self):
        # Unrelated Rust: fsrs-rs minus the files that carry text fsrs-rs took
        # from Anki (see the next test). 19 files of 6.6.2 and 11 of 1.4.9.
        files = self._fsrs_files(lambda rel: rel not in FSRS_ANKI_DERIVED)
        keep = sorted(p for p in files if "/fsrs-6.6.2/" in p) + sorted(p for p in files if "/fsrs-1.4.9/" in p)[:11]
        files = {p: files[p] for p in keep}
        self.assertEqual(len(files), 30)
        t0 = time.monotonic()
        code, summary, views, _err = self.scan(files)
        wall = time.monotonic() - t0
        scores = {
            p: {c: file_entry(views["implementer"], p)["channels"][c].get("score") for c in ("struct", "lex")}
            for p in files
        }
        worst = {c: max(s[c] for s in scores.values()) for c in ("struct", "lex")}
        com = sum(file_entry(views["implementer"], p)["channels"]["com"]["matched"] for p in files)
        record_calibration(
            "fsrs_negative",
            {"files": len(files), "max_struct": worst["struct"], "max_lex": worst["lex"], "com_matches": com,
             "max_struct_run": max(file_entry(views["implementer"], p)["channels"]["struct"]["longest_run"] for p in files),
             "scan_seconds": summary["timing"]["scan_seconds"], "total_seconds": summary["timing"]["total_seconds"],
             "cli_wall_seconds": round(wall, 2)},
        )
        self.assertEqual(code, 0)
        self.assertEqual(summary["status"], "pass")
        self.assertEqual(summary["counts"]["flagged"], 0)
        self.assertLessEqual(worst["struct"], 0.05)
        self.assertLessEqual(worst["lex"], 0.05)
        self.assertLess(summary["timing"]["total_seconds"], 2.0, "scan of 30 files must take under 2 s")

    def test_fsrs_rs_files_with_anki_derived_comments_fail_on_com_only(self):
        # fsrs-rs carries some text from Anki: an Anki AGPL header in its
        # benchmark, and Anki's revlog documentation in its converter tests and
        # simulator. The com channel finds it; struct and lex stay low.
        files = self._fsrs_files(lambda rel: rel in FSRS_ANKI_DERIVED)
        self.assertEqual(len(files), 6)
        code, _s, views, _err = self.scan(files)
        result = {p: file_entry(views["implementer"], p) for p in files}
        record_calibration(
            "fsrs_anki_derived",
            {p: {"flags": [x["channel"] for x in f.get("flags", [])], "struct": f["channels"]["struct"]["score"],
                 "lex": f["channels"]["lex"]["score"]} for p, f in result.items()},
        )
        self.assertEqual(code, 1)
        for p, f in result.items():
            self.assertEqual([x["channel"] for x in f["flags"]], ["com"], p)
            self.assertLessEqual(f["channels"]["struct"]["score"], 0.05, p)
            self.assertLessEqual(f["channels"]["lex"]["score"], 0.05, p)

    def _create_table_text(self):
        tag = release_tags()[-1]
        candidates = [p for p in corpus_git("ls-tree", "-r", "--name-only", tag).decode().splitlines() if p.endswith(".sql")]
        candidates.sort(key=lambda p: ("schema" not in p, p))
        for path in candidates:
            statements = re.findall(r"(?is)create table.*?\);", blob(tag, path))
            if len(statements) >= 2:
                return "\n\n".join(statements) + "\n"
        self.fail("no CREATE TABLE text in the corpus")

    def test_allowlisted_schema_ddl_passes_and_unlisted_fails(self):
        ddl = self._create_table_text()
        path = "core/crates/meguri-storage/src/schema18.sql"
        code, summary, views, _err = self.scan({path: ddl})
        flagged = file_entry(views["implementer"], path)
        allow = os.path.join(self.root, "allow.yaml")
        with open(allow, "w") as f:
            json.dump(
                {"entries": [{"path": path, "reason": "Schema-18 DDL is a file-format fact (calibration)", "approved_by": "antoine"}]}, f
            )
        code2, summary2, views2, _err2 = self.scan({path: ddl}, allowlist=allow)
        listed = file_entry(views2["implementer"], path)
        record_calibration("ddl", {"unlisted": flagged["channels"], "status_unlisted": flagged["status"], "status_listed": listed["status"]})
        self.assertEqual(code, 1)
        self.assertEqual(flagged["status"], "flagged")
        self.assertEqual(code2, 0)
        self.assertEqual(summary2["status"], "pass")
        self.assertEqual(listed["status"], "allowlisted")
        self.assertEqual(listed["allowlisted"][0]["entries"], [1])

    def test_independent_storage_sql_with_masked_vocabulary_passes(self):
        files = {}
        for name, dest in (("storage_rows.rs", "core/crates/meguri-storage/src/rows.rs"), ("card_queue.sql", "core/crates/meguri-storage/src/card_queue.sql")):
            with open(os.path.join(FIXTURES, "independent", name)) as f:
                files[dest] = f.read()
        code, summary, views, _err = self.scan(files)
        result = {p: file_entry(views["implementer"], p)["channels"] for p in files}
        record_calibration("storage_sql_negative", result)
        self.assertEqual(code, 0, json.dumps(result))
        for p, channels in result.items():
            self.assertLess(channels["struct"]["score"], 0.10, p)
            self.assertLess(channels["lex"]["score"], 0.15, p)
            self.assertLess(channels["struct"]["longest_run"], 6, p)

    def test_verbatim_comment_fails_on_com_even_when_code_differs(self):
        tag = release_tags()[-1]
        comment = None
        for path in files_at(tag, "rslib/src", ".rs"):
            text = blob(tag, path)
            for m in _SEGMENT_RE["rust"].finditer(text):
                body = m.group("com")
                if body and body.startswith("//") and not body.startswith("///") and len(TEXT_WORD.findall(body.lower())) >= 12:
                    comment = body
                    break
            if comment:
                break
        self.assertIsNotNone(comment, "no comment found")
        own = (
            "pub fn clamp_ratio(numerator: u64, denominator: u64) -> f64 {\n"
            f"    {comment}\n"
            "    if denominator == 0 { 0.0 } else { (numerator as f64 / denominator as f64).min(1.0) }\n"
            "}\n"
        )
        path = "core/crates/meguri-stats/src/ratio.rs"
        code, _s, views, _err = self.scan({path: own})
        f = file_entry(views["implementer"], path)
        record_calibration("comment_positive", f["channels"])
        self.assertEqual(code, 1)
        self.assertEqual([x["channel"] for x in f["flags"]], ["com"])
        self.assertEqual(f["flags"][0]["regions"][0]["lines"], [2, 2])

    def test_string_catalog_with_a_12_word_ftl_sentence_fails_and_again_passes(self):
        tag = release_tags()[-1]
        sentence = None
        for path in files_at(tag, "ftl", ".ftl"):
            for line in blob(tag, path).splitlines():
                m = re.match(r"^[A-Za-z][\w-]*\s*=\s*([^{}]+)$", line)
                if m and len(TEXT_WORD.findall(m.group(1).lower())) == 12:
                    sentence = m.group(1).strip()
                    break
            if sentence:
                break
        self.assertIsNotNone(sentence, "no 12-word message found")
        catalog = {
            "sourceLanguage": "en",
            "strings": {
                "review.empty": {"localizations": {"en": {"stringUnit": {"state": "translated", "value": sentence}}}},
                "Again": {},
            },
            "version": "1.0",
        }
        path = "app/Packages/MeguriKit/Sources/Features/Review/Localizable.xcstrings"
        code, _s, views, _err = self.scan({path: json.dumps(catalog, indent=2)})
        f = file_entry(views["implementer"], path)
        record_calibration("string_catalog", {"status": f["status"], "keys": f["flags"][0]["regions"][0].get("keys") if f.get("flags") else None})
        self.assertEqual(code, 1)
        self.assertEqual(f["flags"][0]["channel"], "text")
        self.assertEqual(f["flags"][0]["regions"][0]["keys"], ["review.empty"])
        code2, _s2, views2, _e2 = self.scan(
            {path: json.dumps({"sourceLanguage": "en", "strings": {"Again": {}}, "version": "1.0"})}
        )
        self.assertEqual(code2, 0)
        self.assertEqual(file_entry(views2["implementer"], path)["status"], "clean")


if __name__ == "__main__":
    unittest.main()
