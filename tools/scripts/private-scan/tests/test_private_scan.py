"""Tests for private_scan (U15, R1940): the token set built from candidate
strings and pruned by the repository, and the scan of files and of a
commit's staged lines. Every collection here is made up."""

import json
import os
import subprocess
import sys
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
SCRIPT = os.path.join(os.path.dirname(HERE), "private_scan.py")
sys.path.insert(0, os.path.dirname(HERE))

import private_scan  # noqa: E402


def run(*args, cwd=None):
    return subprocess.run([sys.executable, SCRIPT] + list(args), stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                          universal_newlines=True, cwd=cwd)


def git(repo, *args):
    subprocess.run(["git", "-C", repo, "-c", "user.name=t", "-c", "user.email=t@localhost"] + list(args),
                   check=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)


def write(path, text, mode="w"):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, mode, **({} if "b" in mode else {"encoding": "utf-8"})) as f:
        f.write(text)


CANDIDATES = [
    {"kind": "deck", "text": "Japanisch::Hörverstehen"},
    {"kind": "deck", "text": "Hörverstehen"},
    {"kind": "deck", "text": "Japanisch"},
    {"kind": "deck", "text": "N5"},
    {"kind": "tag", "text": "jp::n5"},
    {"kind": "media", "text": "falter flug.jpg"},
    {"kind": "word", "text": "Schmetterling"},
    {"kind": "word", "text": "l'Écureuil"},
    {"kind": "word", "text": "2024"},
    {"kind": "word", "text": "card"},
    {"kind": "word", "text": "Wiese"},
]


class Words(unittest.TestCase):
    def test_words_are_case_folded_nfc_runs_of_letters_and_digits(self):
        self.assertEqual(private_scan.words("Straße, HÖR-verstehen_x 12ab"), ["strasse", "hör", "verstehen", "x", "12ab"])
        # A decomposed ö is the composed one.
        self.assertEqual(private_scan.words("Hör"), ["hör"])
        self.assertEqual(private_scan.words("蝶々が飛ぶ。"), ["蝶々が飛ぶ"])

    def test_a_token_needs_four_characters_and_a_letter(self):
        self.assertEqual(private_scan.tokens_of("word", "l'Écureuil 2024 abc"), [("écureuil",)])
        self.assertEqual(private_scan.tokens_of("deck", "N5"), [])
        self.assertEqual(private_scan.tokens_of("tag", "jp::n5"), [("jp", "n5")])
        self.assertEqual(private_scan.tokens_of("media", "falter flug.jpg"), [("falter", "flug", "jpg")])
        self.assertEqual(private_scan.tokens_of("media", "1234.mp3"), [("1234", "mp3")])
        self.assertEqual(private_scan.tokens_of("media", "12.34"), [])


class Build(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.dir = self.tmp.name
        self.repo = os.path.join(self.dir, "repo")
        os.makedirs(self.repo)
        git(self.repo, "init", "-q")
        # "card" and "Wiese" are already public: a tracked file holds them.
        write(os.path.join(self.repo, "notes.md"), "A card is shown.\nThe WIESE is green.\n")
        # An untracked file is not the repository.
        write(os.path.join(self.repo, "scratch.txt"), "Schmetterling\n")
        git(self.repo, "add", "notes.md")
        git(self.repo, "commit", "-qm", "x")
        self.candidates = os.path.join(self.dir, "candidates.jsonl")
        write(self.candidates, "".join(json.dumps(c, ensure_ascii=False) + "\n" for c in CANDIDATES))
        self.out = os.path.join(self.dir, "private", "tokens", "tokens.txt")

    def tearDown(self):
        self.tmp.cleanup()

    def test_build_keeps_private_tokens_and_drops_what_the_repository_holds(self):
        r = run("build", "--candidates", self.candidates, "--repo", self.repo, "--out", self.out)
        self.assertEqual(r.returncode, 0, r.stderr)
        with open(self.out, encoding="utf-8") as f:
            tokens = f.read().splitlines()
        self.assertEqual(tokens, sorted([
            "falter flug jpg",
            "hörverstehen",
            "japanisch",
            "japanisch hörverstehen",
            "jp n5",
            "schmetterling",
            "écureuil",
        ]))
        meta = json.loads(r.stdout)
        self.assertEqual(meta["tokens"], 7)
        self.assertEqual(meta["in_repository"], 2)
        with open(os.path.join(os.path.dirname(self.out), "meta.json"), encoding="utf-8") as f:
            self.assertEqual(json.load(f), meta)
        # stdout and meta.json hold counts, never a token.
        self.assertNotIn("japanisch", r.stdout)


class Scan(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.dir = self.tmp.name
        self.tokens = os.path.join(self.dir, "tokens.txt")
        write(self.tokens, "falter flug jpg\nhörverstehen\njapanisch hörverstehen\njp n5\nschmetterling\n")

    def tearDown(self):
        self.tmp.cleanup()

    def scan(self, *paths):
        r = run("scan", "--tokens", self.tokens, *paths)
        return r, (json.loads(r.stdout) if r.stdout else None)

    def test_a_result_json_naming_a_deck_fails_naming_the_file_not_the_token(self):
        report = os.path.join(self.dir, "report.json")
        write(report, '{\n  "checks": {"next-states": {"passed": 3}},\n  "deck": "Japanisch::Hörverstehen"\n}\n')
        clean = os.path.join(self.dir, "clean.json")
        write(clean, '{"checks": {"next-states": {"passed": 3, "failed": 0}}, "japanisch": 1}\n')
        r, out = self.scan(report, clean)
        self.assertEqual(r.returncode, 1, r.stderr)
        self.assertEqual(out["files"], [{"path": report, "lines": [3]}])
        self.assertEqual(out["files_scanned"], 2)
        self.assertNotIn("rverstehen", r.stdout.lower())
        self.assertNotIn("rverstehen", r.stderr.lower())

    def test_tokens_match_whole_words_through_case_and_escapes(self):
        cases = {
            "upper.txt": ("SCHMETTERLING flies", True),
            "longer.txt": ("schmetterlingsflügel", False),
            "json.json": ('{"d": "H\\u00f6rverstehen"}', True),
            "entity.html": ("<p>H&ouml;rverstehen</p>", True),
            "url.txt": ("https://x/media/falter%20flug.jpg", True),
            "tag.txt": ("tags: jp::n5, other", True),
            "split.txt": ("jp n6 n5", False),
            "python.txt": ("'H\\xf6rverstehen'", True),
        }
        for name, (text, expected) in cases.items():
            path = os.path.join(self.dir, name)
            write(path, text + "\n")
            r, out = self.scan(path)
            self.assertEqual(r.returncode, 1 if expected else 0, name)
            self.assertEqual(bool(out["files"]), expected, name)

    def test_folders_are_walked_and_binary_files_scanned_for_text_runs(self):
        write(os.path.join(self.dir, "out", "a.json"), "{}\n")
        write(os.path.join(self.dir, "out", "db", "copy.anki2"),
              b"SQLite format 3\0\x01\x02" + "deck Japanisch::Hörverstehen notes".encode() + b"\0\x07", "wb")
        write(os.path.join(self.dir, "out", "noise.bin"), b"\0\x01schm\x02etterling\x03", "wb")
        r, out = self.scan(os.path.join(self.dir, "out"))
        self.assertEqual(r.returncode, 1, r.stderr)
        self.assertEqual(out["files"], [{"path": os.path.join(self.dir, "out", "db", "copy.anki2"), "binary": True}])
        self.assertEqual(out["files_scanned"], 3)

    def test_errors_exit_2_without_the_tokens(self):
        r = run("scan", "--tokens", os.path.join(self.dir, "missing.txt"), self.tokens)
        self.assertEqual(r.returncode, 2)
        r = run("scan", "--tokens", self.tokens)
        self.assertEqual(r.returncode, 2)


class Staged(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.repo = os.path.join(self.tmp.name, "repo")
        os.makedirs(self.repo)
        git(self.repo, "init", "-q")
        self.tokens = os.path.join(self.tmp.name, "tokens.txt")
        write(self.tokens, "hörverstehen\nschmetterling\n")
        # Committed before the token set: only lines a commit adds count.
        write(os.path.join(self.repo, "old.md"), "line one\nSchmetterling was here\n")
        git(self.repo, "add", "old.md")
        git(self.repo, "commit", "-qm", "base")

    def tearDown(self):
        self.tmp.cleanup()

    def scan(self):
        r = run("scan", "--tokens", self.tokens, "--staged", "--repo", self.repo)
        return r, json.loads(r.stdout) if r.stdout else None

    def test_only_added_lines_are_scanned_and_reported_by_line(self):
        write(os.path.join(self.repo, "old.md"), "line one\nSchmetterling was here\nline three\n")
        write(os.path.join(self.repo, "dir with space", "new é.txt"), "fine\nfine\nnow HÖRVERSTEHEN\n")
        write(os.path.join(self.repo, "+++ odd.txt"), "+++ b/elsewhere\n++ hörverstehen\n")
        git(self.repo, "add", "-A")
        r, out = self.scan()
        self.assertEqual(r.returncode, 1, r.stderr)
        self.assertEqual(out["files"], [
            {"path": "+++ odd.txt", "lines": [2]},
            {"path": "dir with space/new é.txt", "lines": [3]},
        ])
        self.assertEqual(out["files_scanned"], 3)

    def test_a_staged_binary_file_is_scanned_whole(self):
        write(os.path.join(self.repo, "blob.bin"), b"\0\0header\0" + "Hörverstehen notes text".encode() + b"\0", "wb")
        write(os.path.join(self.repo, "quoted\"name.txt"), "nothing private\n")
        git(self.repo, "add", "-A")
        r, out = self.scan()
        self.assertEqual(r.returncode, 1, r.stderr)
        self.assertEqual(out["files"], [{"path": "blob.bin", "binary": True}])

    def test_nothing_staged_or_nothing_found_passes(self):
        r, out = self.scan()
        self.assertEqual((r.returncode, out["files"], out["files_scanned"]), (0, [], 0))
        write(os.path.join(self.repo, "quoted\"name.txt"), "nothing private\n")
        os.remove(os.path.join(self.repo, "old.md"))
        git(self.repo, "add", "-A")
        r, out = self.scan()
        self.assertEqual((r.returncode, out["files"], out["files_scanned"]), (0, [], 1))


if __name__ == "__main__":
    unittest.main()
