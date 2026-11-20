"""Tests for private_scan (U15, R1940): the token set built from candidate
strings and pruned by the repository, and the scan of files and of a
commit's staged lines. Every collection here is made up."""

import errno
import hashlib
import io
import json
import os
from pathlib import Path
import re
import struct
import subprocess
import sys
import tempfile
import unittest
from unittest import mock
import zipfile
import zlib

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


def noise(n, seed=0):
    """`n` made-up bytes that look like compressed data."""
    out = b"".join(hashlib.sha256(b"%d-%d" % (seed, i)).digest() for i in range(n // 32 + 1))
    return out[:n]


def png(*chunks, bad_crc=()):
    """A PNG file of `chunks` ((type, data) pairs); the chunks numbered in
    `bad_crc` get a wrong CRC."""
    out = [b"\x89PNG\r\n\x1a\n"]
    for n, (kind, body) in enumerate(chunks):
        crc = zlib.crc32(kind + body) ^ (1 if n in bad_crc else 0)
        out.append(struct.pack(">I", len(body)) + kind + body + struct.pack(">I", crc))
    return b"".join(out)


def gray_image(width, height):
    """A grayscale image's zlib-compressed scanlines: a gradient with noise, so
    the compressed bytes hold printable runs that are words by chance."""
    pixels = noise(width * height, seed=7)
    raw = b"".join(b"\0" + bytes((x + y + pixels[y * width + x] % 16) % 256 for x in range(width))
                   for y in range(height))
    return struct.pack(">IIBBBBB", width, height, 8, 0, 0, 0, 0), zlib.compress(raw, 9)


# The words a scan of a binary file sees by chance: maximal ASCII letter and
# digit runs of 2 to 7 characters between bytes that cannot be part of a word.
CHANCE_WORD = re.compile(rb"(?<![\x30-\x39\x41-\x5a\x61-\x7a\x80-\xff])[0-9A-Za-z]{2,7}(?![\x30-\x39\x41-\x5a\x61-\x7a\x80-\xff])")


def chance_words(data, besides=""):
    """The chance words of `data`, less the words of `besides` (the text a
    test writes beside the data: zlib's output differs between versions)."""
    return sorted({w.decode("ascii").lower() for w in CHANCE_WORD.findall(data)} - set(private_scan.words(besides)))


def zip_archive(entries, comment=b""):
    """A zip archive written by hand: `entries` are (name, method, data)
    with `data` as stored in the archive (method 8 data is not checked)."""
    local, central = [], []
    offset = 0
    for name, method, data in entries:
        name = name.encode("utf-8")
        fixed = struct.pack("<HHHHHIIIHH", 20, 0, method, 0, 0x21, zlib.crc32(data), len(data), len(data), len(name), 0)
        local.append(b"PK\x03\x04" + fixed + name + data)
        central.append(b"PK\x01\x02" + struct.pack("<H", 20) + fixed + struct.pack("<HHHII", 0, 0, 0, 0, offset) + name)
        offset += len(local[-1])
    cd = b"".join(central)
    end = b"PK\x05\x06" + struct.pack("<HHHHIIH", 0, 0, len(entries), len(entries), len(cd), offset, len(comment))
    return b"".join(local) + cd + end + comment


def zstd_frame(*blocks, trailer=b""):
    """A zstd frame written by hand: `blocks` are (type, data) with type 0
    raw, 1 RLE (data is the one byte) or 2 compressed (data as stored)."""
    out = [b"\x28\xb5\x2f\xfd", bytes([0x00, 0x00])]  # no single segment, window byte, no size
    for n, (kind, body) in enumerate(blocks):
        size = 7 if kind == 1 else len(body)
        out.append((size << 3 | kind << 1 | (n == len(blocks) - 1)).to_bytes(3, "little") + body)
    return b"".join(out) + trailer


# The AC luminance Huffman table of the JPEG standard (its values): every
# baseline JPEG carries it, and its bytes hold words such as "qa" and "3br".
JPEG_AC_VALUES = bytes.fromhex(
    "01020300041105122131410613516107227114328191a1082342b1c11552d1f02433627282090a161718191a25262728292a3435363738393a"
    "434445464748494a535455565758595a636465666768696a737475767778797a838485868788898a92939495969798999aa2a3a4a5a6a7a8a9"
    "aab2b3b4b5b6b7b8b9bac2c3c4c5c6c7c8c9cad2d3d4d5d6d7d8d9dae1e2e3e4e5e6e7e8e9eaf1f2f3f4f5f6f7f8f9fa")


def jpeg(com=None, app1=None, scan=b"", after=b""):
    """A baseline JPEG written by hand: the segments a camera writes, `scan`
    as the entropy-coded data (0xFF stuffed), optional COM and APP1 data,
    and `after` past the end marker."""
    def seg(marker, body):
        return bytes([0xFF, marker]) + struct.pack(">H", len(body) + 2) + body
    out = [b"\xff\xd8", seg(0xE0, b"JFIF\0\x01\x01\0\0\x01\0\x01\0\0")]
    if app1 is not None:
        out.append(seg(0xE1, app1))
    if com is not None:
        out.append(seg(0xFE, com))
    out += [seg(0xDB, b"\0" + bytes(range(1, 65))),
            seg(0xC0, b"\x08\0\x10\0\x10\x01\x01\x11\0"),
            seg(0xC4, b"\x10" + bytes([0, 2, 1, 3, 3, 2, 4, 3, 5, 5, 4, 4, 0, 0, 1, 0x7d]) + JPEG_AC_VALUES),
            seg(0xDA, b"\x01\x01\0\0\x3f\0"),
            scan.replace(b"\xff", b"\xff\x00"),
            b"\xff\xd9", after]
    return b"".join(out)


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

    def test_a_field_word_token_needs_four_characters_and_a_letter(self):
        self.assertEqual(private_scan.tokens_of("word", "l'Écureuil 2024 abc"), [("écureuil",)])

    def test_deck_tag_and_media_names_are_tokens_whatever_their_length(self):
        # The four-character rule is for field words only (U15): a short or
        # numeric name is still a name from the collection.
        self.assertEqual(private_scan.tokens_of("deck", "N5"), [("n5",)])
        self.assertEqual(private_scan.tokens_of("deck", "2024"), [("2024",)])
        self.assertEqual(private_scan.tokens_of("tag", "nn"), [("nn",)])
        self.assertEqual(private_scan.tokens_of("tag", "jp::n5"), [("jp", "n5")])
        self.assertEqual(private_scan.tokens_of("media", "falter flug.jpg"), [("falter", "flug", "jpg")])
        self.assertEqual(private_scan.tokens_of("media", "1234.mp3"), [("1234", "mp3")])
        self.assertEqual(private_scan.tokens_of("media", "12.34"), [("12", "34")])
        # A name with no letter or digit at all gives no token.
        self.assertEqual(private_scan.tokens_of("deck", "::"), [])
        self.assertEqual(private_scan.tokens_of("media", "_.-"), [])


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

    def build(self):
        r = run("build", "--candidates", self.candidates, "--repo", self.repo, "--out", self.out)
        self.assertEqual(r.returncode, 0, r.stderr)
        with open(self.out, encoding="utf-8") as f:
            return r, f.read().splitlines()

    def test_build_keeps_private_tokens_and_drops_what_the_repository_holds(self):
        r, tokens = self.build()
        self.assertEqual(tokens, sorted([
            "falter flug jpg",
            "hörverstehen",
            "japanisch",
            "japanisch hörverstehen",
            "jp n5",
            "n5",
            "schmetterling",
            "écureuil",
        ]))
        meta = json.loads(r.stdout)
        self.assertEqual(meta["tokens"], 8)
        self.assertEqual(meta["in_repository"], 2)
        with open(os.path.join(os.path.dirname(self.out), "meta.json"), encoding="utf-8") as f:
            self.assertEqual(json.load(f), meta)
        # stdout and meta.json hold counts, never a token.
        self.assertNotIn("japanisch", r.stdout)

    def test_only_committed_text_makes_a_token_public(self):
        # A private word written into a tracked file, a staged file or a new
        # file is not in the repository until it is committed: exempting it
        # would let the commit that adds it through the scan.
        write(os.path.join(self.repo, "notes.md"), "A card is shown.\nThe WIESE is green.\nSchmetterling\n")
        write(os.path.join(self.repo, "staged.md"), "Japanisch\n")
        git(self.repo, "add", "staged.md")
        write(os.path.join(self.repo, "report.json"), '{"deck": "Hörverstehen"}\n')
        git(self.repo, "add", "report.json")
        _, tokens = self.build()
        for token in ("schmetterling", "japanisch", "hörverstehen"):
            self.assertIn(token, tokens)
        # A committed tracked file still counts, as committed.
        self.assertNotIn("wiese", tokens)

    def test_committed_fixed_import_does_not_learn_its_contained_candidate(self):
        fact = private_scan.PUBLIC_SWIFT_IMPORT
        module = fact.split(" ", 1)[1]
        write(self.candidates, json.dumps({"kind": "deck", "text": module}) + "\n")
        public = os.path.join(self.repo, "public.swift")
        bare = os.path.join(self.dir, "bare.txt")
        write(bare, module + "\n")
        for text in (fact, '"' + fact + '"', "'" + fact + "'"):
            with self.subTest(text=text):
                write(public, text + "\n")
                git(self.repo, "add", "public.swift")
                git(self.repo, "commit", "-qm", "fixed fact")
                r, tokens = self.build()
                self.assertEqual(tokens, [module.casefold()])
                self.assertEqual(json.loads(r.stdout)["in_repository"], 0)
                self.assertEqual(run("scan", "--tokens", self.out, public).returncode, 0)
                result = run("scan", "--tokens", self.out, bare)
                self.assertEqual(result.returncode, 1, result.stderr)
                self.assertEqual(json.loads(result.stdout)["files"], [{"path": bare, "lines": [1]}])
        # The same file's ordinary occurrence still establishes public usage.
        write(public, fact + "\n" + module + "\n")
        git(self.repo, "add", "public.swift")
        git(self.repo, "commit", "-qm", "ordinary occurrence")
        r, tokens = self.build()
        self.assertEqual(tokens, [])
        self.assertEqual(json.loads(r.stdout)["in_repository"], 1)

    def test_committed_learning_keeps_raw_text_semantics_for_encoded_imports(self):
        fact = private_scan.PUBLIC_SWIFT_IMPORT
        prefix, module = fact.split(" ", 1)
        write(self.candidates, json.dumps({"kind": "deck", "text": module}) + "\n")
        public = os.path.join(self.repo, "public.txt")
        encoders = (lambda value: "".join("&#%d;" % ord(c) for c in value),
                    lambda value: "".join("\\u%04x" % ord(c) for c in value),
                    lambda value: "".join("\\U%08x" % ord(c) for c in value),
                    lambda value: "".join("%%%02X" % byte for byte in value.encode("utf-8")))
        for encode in encoders:
            for text in (encode(fact), encode(module)):
                with self.subTest(text=text):
                    write(public, text + "\n")
                    git(self.repo, "add", "public.txt")
                    git(self.repo, "commit", "-qm", "encoded text")
                    self.assertIn(text + "\n", list(private_scan.committed_texts(self.repo)))
                    _, tokens = self.build()
                    self.assertEqual(tokens, [module.casefold()])
        # No decoding is added to learning: a raw ordinary word after an
        # encoded prefix remains an outside-fact committed occurrence.
        write(public, encoders[1](prefix) + " " + module + "\n")
        git(self.repo, "add", "public.txt")
        git(self.repo, "commit", "-qm", "raw word beside encoded text")
        _, tokens = self.build()
        self.assertEqual(tokens, [])

    def test_committed_cross_span_match_is_still_learned(self):
        fact = private_scan.PUBLIC_SWIFT_IMPORT
        module = fact.split(" ", 1)[1]
        candidate = module + " sentinelprivate"
        write(self.candidates, json.dumps({"kind": "deck", "text": candidate}) + "\n")
        public = os.path.join(self.repo, "public.swift")
        write(public, fact + " sentinelprivate\n")
        git(self.repo, "add", "public.swift")
        git(self.repo, "commit", "-qm", "crossing occurrence")
        _, tokens = self.build()
        self.assertEqual(tokens, [])

    def test_committed_private_results_never_make_a_token_public(self):
        # conformance/reports/private/ holds results from the collection: a
        # word there is no evidence that the word is public.
        write(os.path.join(self.repo, "conformance", "reports", "private", "M1.json"), '{"deck": "Japanisch"}\n')
        write(os.path.join(self.repo, "conformance", "private", "README.md"), "Hörverstehen\n")
        git(self.repo, "add", "-A", "conformance")
        git(self.repo, "commit", "-qm", "results")
        _, tokens = self.build()
        self.assertIn("japanisch", tokens)
        self.assertNotIn("hörverstehen", tokens)

    def test_a_repository_without_commits_exempts_nothing(self):
        empty = os.path.join(self.dir, "empty")
        os.makedirs(empty)
        git(empty, "init", "-q")
        write(os.path.join(empty, "notes.md"), "card Wiese\n")
        git(empty, "add", "notes.md")
        r = run("build", "--candidates", self.candidates, "--repo", empty, "--out", self.out)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(json.loads(r.stdout)["in_repository"], 0)

    def test_short_and_numeric_names_are_tokens_and_fail_a_result_json(self):
        write(self.candidates, "".join(json.dumps(c) + "\n" for c in [
            {"kind": "deck", "text": "N5"},
            {"kind": "deck", "text": "2024"},
            {"kind": "tag", "text": "nn"},
            {"kind": "media", "text": "7.mp3"},
            {"kind": "word", "text": "2025"},
            {"kind": "word", "text": "abc"},
        ]))
        _, tokens = self.build()
        self.assertEqual(tokens, ["2024", "7 mp3", "n5", "nn"])
        results = os.path.join(self.dir, "results")
        cases = {
            "deck.json": ('{"deck": "N5", "passed": 3}', True),
            "number.json": ('{"deck": 2024}', True),
            "tag.json": ('{"tags": ["NN"]}', True),
            "media.json": ('{"media": "7.mp3"}', True),
            "word.json": ('{"passed": 2025, "x": "abc"}', False),
            "inside.json": ('{"deck": "N50", "y": "nnx"}', False),
        }
        for name, (text, _) in cases.items():
            write(os.path.join(results, name), text + "\n")
        r = run("scan", "--tokens", self.out, results)
        self.assertEqual(r.returncode, 1, r.stderr)
        found = sorted(os.path.basename(f["path"]) for f in json.loads(r.stdout)["files"])
        self.assertEqual(found, sorted(n for n, (_, hit) in cases.items() if hit))


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

    def test_short_tokens_in_binary_files_are_found(self):
        # A database keeps a short value as a short run between control
        # bytes: a four-letter field word, a tag, a field between the 0x1f
        # separators of a note's fields, a deck or tag name of two letters,
        # a nested deck name whose components 0x1f separates.
        tokens = os.path.join(self.dir, "short-tokens.txt")
        write(tokens, "hund\njp n5\nkatze\nnn\nx7\njapanisch hörverstehen\n")
        cases = {
            "word.bin": (b"\0\x01Hund\0\x07", True),
            "tag.bin": (b"\0\x15jp::n5\x1f\0", True),
            "field.bin": (b"\0" * 64 + b"\x02\x1fKATZE\x1fcat\0" + b"\0" * 64, True),
            "two.bin": (b"\0\x05NN\0\x02", True),
            "digit.bin": (b"\0\x7fx7\x01", True),
            "deck.bin": (b"\0\x15" + "Japanisch\x1fHörverstehen".encode() + b"\0", True),
            "inside.bin": (b"\0Hunde\0katzen\0nnx\0x70\0", False),
        }
        for name, (data, expected) in cases.items():
            path = os.path.join(self.dir, "bin", name)
            write(path, data, "wb")
            r = run("scan", "--tokens", tokens, path)
            self.assertEqual(r.returncode, 1 if expected else 0, (name, r.stderr))
            out = json.loads(r.stdout)
            self.assertEqual(out["files"], [{"path": path, "binary": True}] if expected else [], name)

    def test_no_exemption_for_data_that_only_looks_compressed(self):
        # Bytes after a PNG signature that are no PNG chunks: nothing marks
        # them as compressed data, so a short run there counts, however
        # random the bytes around it.
        tokens = os.path.join(self.dir, "short-tokens.txt")
        write(tokens, "hund\nnn\n")
        data = noise(64000)
        self.assertEqual(chance_words(data).count("hund") + chance_words(data).count("nn"), 0)
        for name, inserted in (("word.png", b"\0Hund\0"), ("two.png", b"\x03nn\x04")):
            path = os.path.join(self.dir, "bin", name)
            write(path, b"\x89PNG\r\n\x1a\n" + data[:30000] + inserted + data[30000:], "wb")
            r = run("scan", "--tokens", tokens, path)
            self.assertEqual(r.returncode, 1, (name, r.stderr))

    def test_errors_exit_2_without_the_tokens(self):
        r = run("scan", "--tokens", os.path.join(self.dir, "missing.txt"), self.tokens)
        self.assertEqual(r.returncode, 2)
        r = run("scan", "--tokens", self.tokens)
        self.assertEqual(r.returncode, 2)


class Formats(unittest.TestCase):
    """Compressed data holds printable runs that equal short words by chance.
    Only where a file's format says the bytes are compressed data, fixed
    numbers or tables does a match need a long token (eight bytes or more);
    names, metadata and stored data are read as text."""

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.dir = self.tmp.name

    def tearDown(self):
        self.tmp.cleanup()

    def scan(self, tokens, data):
        """Whether a scan of `data`, as a file, with `tokens`, finds one."""
        token_file = os.path.join(self.dir, "tokens.txt")
        write(token_file, "".join(t + "\n" for t in tokens))
        path = os.path.join(self.dir, "file.bin")
        write(path, data, "wb")
        r = run("scan", "--tokens", token_file, path)
        self.assertIn(r.returncode, (0, 1), r.stderr)
        self.assertEqual(json.loads(r.stdout)["files"], [{"path": path, "binary": True}] if r.returncode else [])
        return r.returncode == 1

    def test_png_image_data_is_compressed_and_its_metadata_text(self):
        ihdr, idat = gray_image(256, 256)
        words = chance_words(idat)
        self.assertGreater(len(words), 100)
        meta = [(b"sRGB", b"\0"), (b"pHYs", struct.pack(">IIB", 5669, 5669, 1))]
        image = [(b"IHDR", ihdr)] + meta + [(b"IDAT", idat[:30000]), (b"IDAT", idat[30000:]), (b"IEND", b"")]
        self.assertFalse(self.scan(words + ["phys", "srgb", "ihdr", "idat", "iend"], png(*image)))
        # The same bytes where the chunks do not check out: no exemption.
        self.assertTrue(self.scan(words, png(*image, bad_crc=(3,))))
        # Text chunks, EXIF and unknown chunks next to the image data are text.
        for chunk in ((b"tEXt", b"Comment\0" + words[0].encode()),
                      (b"iTXt", b"Title\0\0\0en\0\0" + words[1].upper().encode()),
                      (b"zTXt", words[2].encode() + b"\0\0" + zlib.compress(b"x")),
                      (b"eXIf", b"MM\0*\0\0\0\x08\0\x01\x01\x0e\0\x02\0\0\0\x03" + words[3].encode() + b"\0"),
                      (b"prVt", b"\x01" + words[4].encode() + b"\x02")):
            with_meta = png(*(image[:3] + [chunk] + image[3:]))
            self.assertTrue(self.scan(words, with_meta), chunk[0])
        # A long token is found inside the compressed data too.
        idat_text = idat[:20000] + b"\0Schmetterling\0" + idat[20000:]
        self.assertTrue(self.scan(["schmetterling"], png((b"IHDR", ihdr), (b"IDAT", idat_text), (b"IEND", b""))))
        # Compressed text chunks: the text is compressed, the keyword is not.
        packed = zlib.compress(idat[:4000])
        zword = chance_words(packed, "Comment de")
        self.assertGreater(len(zword), 0)
        self.assertFalse(self.scan(zword, png((b"IHDR", ihdr), (b"zTXt", b"Comment\0\0" + packed),
                                             (b"iTXt", b"Comment\0\x01\0de\0\0" + packed), (b"IEND", b""))))

    def test_zip_entries_compressed_or_stored(self):
        content = " ".join(hashlib.sha256(b"%d" % i).hexdigest()[: 8 + i % 23] for i in range(20000)).encode()
        packer = zlib.compressobj(zlib.Z_DEFAULT_COMPRESSION, zlib.DEFLATED, -15)
        deflated = packer.compress(content) + packer.flush()
        words = chance_words(deflated, "collection.data meta commentary")
        self.assertGreater(len(words), 50)
        # Written by zipfile: the deflated entry and a stored one.
        buf = io.BytesIO()
        with zipfile.ZipFile(buf, "w") as zf:
            zf.writestr(zipfile.ZipInfo("collection.data"), content, zipfile.ZIP_DEFLATED)
            zf.writestr(zipfile.ZipInfo("meta"), b"\x08\x02", zipfile.ZIP_STORED)
        archive = buf.getvalue()
        self.assertIn(deflated, archive)
        self.assertFalse(self.scan(words, archive))
        # Entry names, the archive comment and stored entries are text.
        self.assertFalse(self.scan(words, zip_archive([("a", 8, deflated)], b"commentary")))
        self.assertTrue(self.scan(words, zip_archive([(words[0] + ".txt", 8, deflated)])))
        self.assertTrue(self.scan(words, zip_archive([("a", 8, deflated)], words[1].encode())))
        self.assertTrue(self.scan(words, zip_archive([("a", 8, deflated), ("b", 0, deflated)])))
        self.assertTrue(self.scan(["nn"], zip_archive([("a", 0, b"\x1f\0nn\x01")])))
        # A long token in the compressed data is found.
        self.assertTrue(self.scan(["schmetterling"], zip_archive([("a", 8, deflated[:900] + b"\0Schmetterling\0")])))
        # A broken archive is no archive.
        self.assertTrue(self.scan(words, zip_archive([("a", 8, deflated)])[:-10]))

    def test_zstd_frames_compressed_blocks_and_raw_text(self):
        data = noise(9000, seed=3)
        words = chance_words(data)
        self.assertGreater(len(words), 10)
        compressed = (2, b"\x02" + data)  # compressed literals, then sequences
        self.assertFalse(self.scan(words, zstd_frame(compressed, (1, b"\0"), compressed)))
        self.assertTrue(self.scan(["hund"], zstd_frame(compressed, (0, b"\0Hund\0"))))
        # Raw literals inside a compressed block are stored as they are.
        raw_literals = bytes([6 << 3]) + b"\0Hund\0" + data[:300]
        self.assertTrue(self.scan(["hund"], zstd_frame((2, raw_literals))))
        self.assertFalse(self.scan(["hund"], zstd_frame((2, bytes([6 << 3]) + b"\0Haus\0" + b"\0Hund\0"))))
        # Skippable frames carry user data; bytes after the last frame are not zstd.
        skippable = b"\x50\x2a\x4d\x18" + struct.pack("<I", 6) + b"\0Hund\0"
        self.assertTrue(self.scan(["hund"], zstd_frame(compressed) + skippable))
        self.assertTrue(self.scan(["hund"], zstd_frame(compressed, trailer=b"\0Hund\0")))
        # A frame in a stored zip entry, as an Anki package holds its collection.
        package = zip_archive([("collection.anki21b", 0, zstd_frame(compressed)), ("media", 0, b"\x28\xb5\x2f\xfd\0\0\x01\0\0")])
        self.assertFalse(self.scan(words, package))
        self.assertTrue(self.scan(["hund"], zip_archive([("collection.anki21b", 0, zstd_frame((0, b"\0Hund\0")))])))

    def test_jpeg_tables_and_scan_data_are_opaque_and_metadata_text(self):
        scan = noise(20000, seed=5)
        quant = bytes([16, 11, 10, 16, 24, 40, 51, 61, 12, 12, 14, 19, 26, 58, 60, 55, 14, 13, 16, 24, 40, 57, 69, 56,
                       14, 17, 22, 29, 51, 87, 80, 62, 18, 22, 37, 56, 68, 109, 103, 77, 24, 35, 55, 64, 81, 104, 113,
                       92, 49, 64, 78, 87, 103, 121, 120, 101, 72, 92, 95, 98, 112, 100, 103, 99])
        image = jpeg(scan=scan)
        image = image.replace(bytes(range(1, 65)), quant)
        words = chance_words(scan.replace(b"\xff", b"\xff\x00") + b"\0" + JPEG_AC_VALUES + b"\0" + quant)
        self.assertGreater(len(words), 10)
        self.assertIn("qa", words)
        self.assertFalse(self.scan(words, image))
        # Without the start marker, the same bytes are no JPEG.
        self.assertTrue(self.scan(words, image[2:]))
        self.assertTrue(self.scan(words, jpeg(com=words[0].encode(), scan=scan)))
        self.assertTrue(self.scan(words, jpeg(app1=b"Exif\0\0MM\0*" + words[1].encode() + b"\0", scan=scan)))
        self.assertTrue(self.scan(words, jpeg(scan=scan, after=b"\0" + words[2].encode() + b"\0")))
        self.assertTrue(self.scan(["schmetterling"], jpeg(scan=scan[:5000] + b"\0Schmetterling\0" + scan[5000:])))


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

    def test_a_short_token_in_a_staged_binary_file_is_found(self):
        tokens = os.path.join(self.tmp.name, "short-tokens.txt")
        write(tokens, "hund\n")
        write(os.path.join(self.repo, "fixture.anki2"), b"SQLite format 3\0" + b"\0" * 32 + b"\x1fHund\x1fdog\0", "wb")
        git(self.repo, "add", "-A")
        r = run("scan", "--tokens", tokens, "--staged", "--repo", self.repo)
        self.assertEqual(r.returncode, 1, r.stderr)
        self.assertEqual(json.loads(r.stdout)["files"], [{"path": "fixture.anki2", "binary": True}])

    def test_short_names_and_metadata_in_staged_binary_files_are_found(self):
        ihdr, idat = gray_image(128, 128)
        words = chance_words(idat, "SQLite format 3 a.json Title")
        self.assertGreater(len(words), 20)
        tokens = os.path.join(self.tmp.name, "chance-tokens.txt")
        write(tokens, "".join(w + "\n" for w in words + ["nn", "x7"]))
        image = [(b"IHDR", ihdr), (b"IDAT", idat), (b"IEND", b"")]
        write(os.path.join(self.repo, "snapshot.png"), png(*image), "wb")
        git(self.repo, "add", "-A")
        r = run("scan", "--tokens", tokens, "--staged", "--repo", self.repo)
        self.assertEqual((r.returncode, json.loads(r.stdout)["files"]), (0, []), r.stderr)
        git(self.repo, "commit", "-qm", "snapshot")
        # Changed: a text chunk beside the image data, a zip entry named by a
        # short tag, a database holding a two-letter deck name.
        write(os.path.join(self.repo, "snapshot.png"),
              png(image[0], (b"tEXt", b"Title\0" + words[0].encode()), *image[1:]), "wb")
        write(os.path.join(self.repo, "package.apkg"), zip_archive([("nn.json", 8, idat[:5000])]), "wb")
        write(os.path.join(self.repo, "fixture.anki2"), b"SQLite format 3\0" + b"\0" * 32 + b"\x0fX7\x1f\0", "wb")
        write(os.path.join(self.repo, "clean.apkg"), zip_archive([("a.json", 8, idat[:5000])]), "wb")
        git(self.repo, "add", "-A")
        r = run("scan", "--tokens", tokens, "--staged", "--repo", self.repo)
        self.assertEqual(r.returncode, 1, r.stderr)
        self.assertEqual(json.loads(r.stdout)["files"], [
            {"path": "fixture.anki2", "binary": True},
            {"path": "package.apkg", "binary": True},
            {"path": "snapshot.png", "binary": True},
        ])

    def test_nothing_staged_or_nothing_found_passes(self):
        r, out = self.scan()
        self.assertEqual((r.returncode, out["files"], out["files_scanned"]), (0, [], 0))
        write(os.path.join(self.repo, "quoted\"name.txt"), "nothing private\n")
        os.remove(os.path.join(self.repo, "old.md"))
        git(self.repo, "add", "-A")
        r, out = self.scan()
        self.assertEqual((r.returncode, out["files"], out["files_scanned"]), (0, [], 1))


class PublicInterfaceTests(unittest.TestCase):
    """Fixed public facts do not exempt surrounding or modified text."""

    key = "--public-surface"
    url = "https://github.com/sass/dart-sass/releases/download/1.102.0/dart-sass-1.102.0-linux-arm64.tar.gz"

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.repo = self.temp.name
        git(self.repo, "init", "-q")
        # Exercise modified fixtures through the same
        # production boundary expression. Public facts are checked separately.
        pattern = private_scan.PUBLIC_LITERAL.pattern.replace(re.escape("canvas-elevated"), re.escape("public-surface"))
        patch = mock.patch.object(private_scan, "PUBLIC_LITERAL", re.compile(pattern))
        patch.start()
        self.addCleanup(patch.stop)
        self.matcher = private_scan.Matcher({("public",), ("surface",),
                                             tuple(private_scan.words("dart-sass")), ("sentinelprivate",)})

    def check_all(self, text, expected, matcher=None):
        matcher = matcher or self.matcher
        for binary in (False, True):
            data = text.encode("utf-8") + (b"\0" if binary else b"\n")
            full = os.path.join(self.repo, "report.bin" if binary else "report.txt")
            write(full, data, "wb")
            git(self.repo, "add", "--", os.path.basename(full))
            staged, _ = private_scan.scan_staged(matcher, self.repo)
            self.assertEqual(bool(staged), expected, (text, "staged", binary))
            files, _ = private_scan.scan_paths(matcher, [full], repo=self.repo)
            self.assertEqual(bool(files), expected, (text, "file", binary))
            git(self.repo, "reset", "-q")

    def test_exact_public_interface_and_compiler_url(self):
        for text in (self.key, "public-surface", "dart-sass", self.url,
                     '`%s`: white; value="%s"' % (self.key, self.url)):
            self.check_all(text, False)

    def test_modified_literals_and_longer_identifiers_are_scanned(self):
        for text in ("--public_surface", self.key + "-extra", "x" + self.key,
                     self.key.upper(), self.key + "\u0301", "\u0301" + self.key,
                     self.url.replace("1.102.0", "1.101.0"), self.url + "/extra",
                     self.url + ".evil", self.url + "?extra", "x" + self.url, "?" + self.url, "%" + self.url):
            self.check_all(text, True)

    def test_surrounding_private_text_is_scanned(self):
        for literal in (self.key, self.url):
            for text in ("sentinelprivate " + literal, literal + " sentinelprivate",
                         literal + "?sentinelprivate", literal + "#sentinelprivate"):
                self.check_all(text, True)

    def test_multiword_tokens_crossing_public_spans_still_match(self):
        cases = [("before " + self.key, ("before", "public")),
                 (self.key + " after", ("surface", "after")),
                 (self.key + " " + self.key, ("surface", "public"))]
        for text, token in cases:
            self.check_all(text, True, private_scan.Matcher({token}))

    def test_normalization_before_the_literal_preserves_match_positions(self):
        for text in ("Straße " + self.key, "Ho\u0308r " + self.url):
            self.check_all(text, False)

    def test_clean_working_file_never_hides_private_index_text(self):
        full = os.path.join(self.repo, "report.txt")
        write(full, self.key + " sentinelprivate\n")
        git(self.repo, "add", "report.txt")
        write(full, self.key + "\n")
        staged, _ = private_scan.scan_staged(self.matcher, self.repo)
        self.assertEqual(staged, [{"path": "report.txt", "lines": [1]}])


APPROVED_MEDIA_IO_RUST_PATHS = (
    "std::fs::File",
    "std::cell::Cell",
    "std::fs::read_dir",
    "std::future::Future::poll",
    "std::future::poll_fn",
    "std::pin::Pin",
    "std::sync::mpsc",
    "Arc::clone",
    "Arc::new",
    "AtomicBool::new",
    "Box::pin",
    "Cell::new",
    "Ordering::Acquire",
    "Ordering::Release",
    "PoisonError::into_inner",
    "Poll::Pending",
    "Poll::Ready",
    "Vec::with_capacity",
    "mpsc::channel",
    "io::Error::from",
    "io::Error::new",
    "io::ErrorKind::Interrupted",
    "io::ErrorKind::Other",
    "io::ErrorKind::StorageFull",
    "io::Result",
    "io::sink",
    "bytes::Bytes",
    "Bytes::from",
    "futures_core::Stream",
    "Sha256::digest",
    "Sha256::new",
    "tempfile::NamedTempFile",
    "tempfile::NamedTempFile::new",
    "tempfile::NamedTempFile::new_in",
    "tempfile::Builder::new",
    "tempfile::tempdir",
    "Semaphore::const_new",
    "tokio::task::spawn_blocking",
    "tokio::pin!",
    "tokio::select!",
    "ZipWriter::new",
    "SimpleFileOptions::default",
)


class PublicRustPathsTests(unittest.TestCase):
    """Synthetic matches exercise only the three complete public Rust paths."""

    facts = ("tokio::sync::Semaphore::const_new", "tokio::sync::Semaphore", "tokio::sync::Notify")
    check_all = PublicInterfaceTests.check_all

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.repo = self.temp.name
        git(self.repo, "init", "-q")
        self.matcher = private_scan.Matcher(
            {(word,) for fact in self.facts for word in private_scan.words(fact)} | {("sentinelprivate",)})

    def test_exactly_45_complete_paths_and_constructor_longest(self):
        actual = private_scan.PUBLIC_RUST_PATHS
        approved = APPROVED_MEDIA_IO_RUST_PATHS
        self.assertEqual(len(approved), 42)
        self.assertEqual(len(set(approved)), 42)
        self.assertTrue(set(self.facts).isdisjoint(approved))
        self.assertEqual(set(actual), set(self.facts) | set(approved))
        self.assertEqual(len(actual), 45)
        self.assertEqual(len(set(actual)), 45)
        self.assertEqual(list(map(len, actual)), sorted(map(len, actual), reverse=True))
        for fact in self.facts:
            self.assertEqual([m.group() for m in private_scan.PUBLIC_LITERAL.finditer(fact)], [fact])
            for text in (fact, "&" + fact, "Option<" + fact + ">", '`"' + fact + '"`'):
                self.check_all(text, False)
        constructor = self.facts[0]
        self.check_all(constructor + "(1)", False,
                       private_scan.Matcher({tuple(private_scan.words(constructor))}))

    def test_qualification_identifier_continuations_and_combining_marks_are_scanned(self):
        for fact in self.facts:
            for prefix in ("x", "_", "custom::", "::", "custom:: ", "::\t", "r#", ".", "-", "\u0301", "\u034f"):
                self.check_all(prefix + fact, True)
            for suffix in ("x", "_extra", "::extra", "::", " ::extra", "\t::", ".", "-extra", "\u0301", "\u034f"):
                self.check_all(fact + suffix, True)

    def test_case_punctuation_near_spellings_and_bare_names_are_scanned(self):
        replacements = (("tokio", "Tokio"), ("sync", "Sync"),
                        ("::", "."), ("sync", "syn_c"), ("sync", "sync\u0301"))
        for fact in self.facts:
            for text in (fact.upper(), fact.lower(), fact.replace("::", ":", 1)):
                self.check_all(text, True)
            for old, new in replacements:
                self.check_all(fact.replace(old, new), True)
        for fact in (self.facts[1], self.facts[2], self.facts[0]):
            self.check_all(fact.rsplit("::", 1)[1], True)
        for fact, removed in ((self.facts[1], 1), (self.facts[2], 1)):
            self.check_all(fact.split("::", removed)[-1], True)
        for fact in self.facts[1:]:
            self.check_all(fact[:-1], True)
        prefix, separator, name = self.facts[0].rpartition("::")
        for suffix in (name.replace("_", "-"), name[0].upper() + name[1:]):
            self.check_all(prefix + separator + suffix, True)

    def test_comments_cannot_hide_extra_qualification_in_either_direction(self):
        encoders = (lambda text: text,
                    lambda text: "".join("&#%d;" % ord(c) for c in text),
                    lambda text: "".join("\\u%04x" % ord(c) for c in text),
                    lambda text: "".join("%%%02X" % ord(c) for c in text))
        trivia = (" /* note */ ", " // note\n ", " /* first\n second */\n ",
                  " /* outer /* inner */ tail */ ",
                  " /* outer\n /* inner */\n tail */\n ")
        for fact in self.facts:
            for gap in trivia:
                for text in ("custom::" + gap + fact, fact + gap + "::extra"):
                    for encode in encoders:
                        encoded = encode(text)
                        self.assertTrue(any(self.matcher.hit(v) for v in private_scan.variants(encoded)), encoded)
                        self.check_all(encoded, True)

    def test_decoded_trivia_across_lines_is_scanned(self):
        # Encoding preserves line breaks, requiring adjacent text for each scan.
        contexts = (("custom&#58;&#58; &#47;&#47; note\n", ""),
                    ("custom\\x3a\\x3a \\x2f\\x2f note\n", ""),
                    ("custom%3A%3A %2F%2F note\n", ""),
                    ("", "\n&#47;&#42; note &#42;&#47; ::extra"),
                    ("", "\n\\x2f\\x2a note \\x2a\\x2f ::extra"),
                    ("", "\n%2F%2A note %2A%2F ::extra"))
        for fact in self.facts:
            for prefix, suffix in contexts:
                self.check_all(prefix + fact + suffix, True)

    def test_clean_working_file_cannot_hide_comment_qualified_private_index_suffix(self):
        full = os.path.join(self.repo, "report")
        for fact in self.facts:
            for gap in (" /* note */ ", " // note\n ", " /* outer\n /* inner */\n tail */\n "):
                for binary in (False, True):
                    tail = b"\0" if binary else b"\n"
                    write(full, (fact + gap + "::sentinelprivate").encode() + tail, "wb")
                    git(self.repo, "add", "report")
                    write(full, fact.encode() + tail, "wb")
                    self.assertFalse(private_scan.scan_paths(self.matcher, [full], self.repo)[0])
                    self.assertTrue(private_scan.scan_staged(self.matcher, self.repo)[0])
                    git(self.repo, "reset", "-q")

    def test_surrounding_generics_arguments_and_cross_span_tokens_remain_scanned(self):
        for fact in self.facts:
            for text in ("sentinelprivate " + fact, fact + " sentinelprivate",
                         fact + "<sentinelprivate>", fact + "(sentinelprivate)"):
                self.check_all(text, True)
            ws = private_scan.words(fact)
            for text, token in (("sentinelprivate " + fact, ("sentinelprivate", ws[0])),
                                (fact + "<sentinelprivate>", (ws[-1], "sentinelprivate")),
                                (fact + "(sentinelprivate)", (ws[-1], "sentinelprivate")),
                                (fact + " " + fact, (ws[-1], ws[0]))):
                self.check_all(text, True, private_scan.Matcher({token}))

    def test_decoded_paths_in_text_binary_and_index_keep_span_boundaries(self):
        encoders = (lambda text: "".join("&#%d;" % ord(c) for c in text),
                    lambda text: "".join("\\u%04x" % ord(c) for c in text),
                    lambda text: "".join("%%%02X" % ord(c) for c in text))
        for fact in self.facts:
            for encode in encoders:
                self.check_all(encode(fact), False)
                for text in ("custom::" + fact, fact + "::extra", fact + "\u034f",
                             fact + "(sentinelprivate)", fact + "<sentinelprivate>"):
                    self.check_all(encode(text), True)
                ws = private_scan.words(fact)
                self.check_all(encode(fact + " sentinelprivate"), True,
                               private_scan.Matcher({(ws[-1], "sentinelprivate")}))

    def test_dirty_working_file_never_hides_staged_text_or_binary(self):
        for fact in self.facts:
            for binary in (False, True):
                full = os.path.join(self.repo, "report")
                tail = b"\0" if binary else b"\n"
                write(full, (fact + "(sentinelprivate)").encode() + tail, "wb")
                git(self.repo, "add", "report")
                write(full, fact.encode() + tail, "wb")
                self.assertFalse(private_scan.scan_paths(self.matcher, [full], self.repo)[0])
                self.assertTrue(private_scan.scan_staged(self.matcher, self.repo)[0])
                git(self.repo, "add", "report")
                write(full, (fact + " sentinelprivate").encode() + tail, "wb")
                self.assertFalse(private_scan.scan_staged(self.matcher, self.repo)[0])
                self.assertTrue(private_scan.scan_paths(self.matcher, [full], self.repo)[0])
                git(self.repo, "reset", "-q")


class PublicMediaIoRustPathsTests(unittest.TestCase):
    """Only the finite approved public spans may contain synthetic matches."""

    facts = APPROVED_MEDIA_IO_RUST_PATHS
    setUp = PublicRustPathsTests.setUp
    check_all = PublicInterfaceTests.check_all

    def check_plain_and_files(self, text, expected, matcher=None):
        matcher = matcher or self.matcher
        self.assertEqual(any(matcher.hit(v) for v in private_scan.variants(text)), expected, text)
        self.assertEqual(bool(private_scan.line_hits(matcher, text)), expected, text)
        self.check_all(text, expected, matcher)

    def test_all_42_exact_spans_contain_single_and_whole_multiword_matches(self):
        for fact in self.facts:
            ws = private_scan.words(fact)
            singles = private_scan.Matcher({(word,) for word in ws})
            whole = private_scan.Matcher({tuple(ws)})
            with self.subTest(fact=fact):
                self.assertEqual([m.group() for m in private_scan.PUBLIC_LITERAL.finditer(fact)], [fact])
                for matcher in (singles, whole):
                    for text in (fact, "&" + fact, "Option<" + fact + ">", fact + "()"):
                        self.check_plain_and_files(text, False, matcher)

    def test_overlapping_tempfile_paths_use_the_complete_longest_span(self):
        for fact in ("tempfile::NamedTempFile", "tempfile::NamedTempFile::new",
                     "tempfile::NamedTempFile::new_in"):
            with self.subTest(fact=fact):
                self.assertEqual([m.group() for m in private_scan.PUBLIC_LITERAL.finditer(fact)], [fact])
                # A shorter type-only exception would leave the constructor words scanned.
                self.check_plain_and_files(fact, False)
                self.check_plain_and_files(fact + "::extra", True)

    def test_case_punctuation_and_bare_terminal_names_are_not_facts(self):
        for fact in self.facts:
            near = (fact[0].swapcase() + fact[1:], fact.replace("::", ":", 1),
                    fact.replace("::", ".", 1), fact.rsplit("::", 1)[1])
            if fact.endswith("!"):
                near += (fact[:-1], fact[:-1] + "?", fact[:-1] + "!!")
            for text in near:
                with self.subTest(fact=fact, text=text):
                    self.check_plain_and_files(text, True)

    def test_extra_qualification_longer_identifiers_and_unicode_remain_scanned(self):
        for fact in self.facts:
            for prefix in ("custom::", "::", "custom:: \t", "x", "_", "r#", ".",
                           "\u0301", "\u034f", "\u03bb"):
                with self.subTest(fact=fact, prefix=prefix):
                    self.check_plain_and_files(prefix + fact, True)
            for suffix in ("::extra", " ::", "\t::extra", "x", "_extra", ".",
                           "\u0301", "\u034f", "\u03bb"):
                with self.subTest(fact=fact, suffix=suffix):
                    self.check_plain_and_files(fact + suffix, True)

    def test_outside_inventory_imports_types_and_receiver_methods_stay_scanned(self):
        excluded = ("Read", "PathBuf", "Arc", "AtomicBool", "File", "Bytes", "Digest", "Sha256",
                    "NamedTempFile", "Builder", "Notify", "Semaphore", "ZipWriter",
                    "write::SimpleFileOptions", "Stream::Item", "Stream::poll_next", "Self::Item",
                    "as_file", "prefix", "tempfile_in", "update", "finalize", "notify_waiters",
                    "start_file", "finish", "clone", "read", "write_all", "assert!", "vec!")
        self.assertTrue(set(self.facts).isdisjoint(excluded))
        for text in excluded:
            with self.subTest(text=text):
                matcher = private_scan.Matcher({tuple(private_scan.words(text))})
                self.check_plain_and_files(text, True, matcher)

    def test_receivers_arguments_generics_and_surrounding_text_are_scanned(self):
        sentinel = private_scan.Matcher({("sentinelprivate",)})
        for fact in self.facts:
            for text in ("sentinelprivate " + fact, fact + " sentinelprivate",
                         "sentinelprivate." + fact + "()", fact + "(sentinelprivate)",
                         fact + "<sentinelprivate>", fact + "::<sentinelprivate>"):
                with self.subTest(fact=fact, text=text):
                    self.check_plain_and_files(text, True, sentinel)

    def test_matches_crossing_one_or_two_public_spans_are_scanned(self):
        for fact in self.facts:
            ws = private_scan.words(fact)
            cases = (("sentinelprivate " + fact, ("sentinelprivate", ws[0])),
                     (fact + " sentinelprivate", (ws[-1], "sentinelprivate")),
                     (fact + "(sentinelprivate)", (ws[-1], "sentinelprivate")),
                     (fact + "<sentinelprivate>", (ws[-1], "sentinelprivate")),
                     (fact + " " + fact, (ws[-1], ws[0])))
            for text, token in cases:
                with self.subTest(fact=fact, text=text):
                    self.check_plain_and_files(text, True, private_scan.Matcher({token}))

    def test_decoded_paths_in_text_binary_and_index_keep_span_boundaries(self):
        encoders = (lambda text: "".join("&#%d;" % ord(c) for c in text),
                    lambda text: "".join("\\u%04x" % ord(c) for c in text),
                    lambda text: "".join("%%%02X" % ord(c) for c in text))
        for fact in self.facts:
            ws = private_scan.words(fact)
            for encode in encoders:
                with self.subTest(fact=fact, encoded=encode(fact)):
                    for matcher in (private_scan.Matcher({(word,) for word in ws}),
                                    private_scan.Matcher({tuple(ws)})):
                        self.check_plain_and_files(encode(fact), False, matcher)
                    for text in ("custom::" + fact, fact + "::extra", fact + "\u034f",
                                 fact + "(sentinelprivate)", fact + "<sentinelprivate>"):
                        self.check_plain_and_files(encode(text), True)
                    cross = private_scan.Matcher({(ws[-1], "sentinelprivate")})
                    self.check_plain_and_files(encode(fact + " sentinelprivate"), True, cross)

    def test_comment_and_whitespace_qualification_remains_conservative(self):
        # Comments adjacent to facts are deliberately ambiguous to the scanner.
        facts = ("tempfile::NamedTempFile::new_in", "tokio::select!", "io::ErrorKind::Other")
        encoders = (lambda text: text,
                    lambda text: "".join("&#%d;" % ord(c) for c in text),
                    lambda text: "".join("\\u%04x" % ord(c) for c in text),
                    lambda text: "".join("%%%02X" % ord(c) for c in text))
        gaps = (" \t", "\n ", " /* note */ ", " // note\n ",
                " /* first\n second */\n ", " /* outer /* inner */ tail */ ")
        for fact in facts:
            for gap in gaps:
                for text in ("custom::" + gap + fact, fact + gap + "::extra"):
                    for encode in encoders:
                        self.check_plain_and_files(encode(text), True)
            contexts = (("custom&#58;&#58; &#47;&#47; note\n", ""),
                        ("custom\\x3a\\x3a \\x2f\\x2f note\n", ""),
                        ("custom%3A%3A %2F%2F note\n", ""),
                        ("", "\n&#47;&#42; note &#42;&#47; ::extra"),
                        ("", "\n\\x2f\\x2a note \\x2a\\x2f ::extra"),
                        ("", "\n%2F%2A note %2A%2F ::extra"))
            for prefix, suffix in contexts:
                self.check_plain_and_files(prefix + fact + suffix, True)


class FixedPublicFactsTests(unittest.TestCase):
    def test_xctest_gesture_is_a_complete_fixed_call_only(self):
        call = private_scan.PUBLIC_GESTURE_CALL
        matcher = private_scan.Matcher({(word,) for word in private_scan.words(call)})
        self.assertFalse(matcher.hit("cardSurface" + call))
        for text in (call[1:], "cardSurface" + call.upper(),
                     "cardSurface" + call.replace(": 2", ": 3"),
                     "cardSurface" + call + "Suffix"):
            self.assertTrue(matcher.hit(text))
        call_words = private_scan.words(call)
        for token in (("sentinelprivate",), ("sentinelprivate", call_words[0]),
                      (*call_words[-2:], "sentinelprivate")):
            guarded = private_scan.Matcher({token})
            self.assertTrue(guarded.hit("sentinelprivate" + call + " sentinelprivate"))

    def test_known_interface_spellings_and_archive_are_exact(self):
        facts = ("--canvas-elevated", "canvas-elevated", "dart-sass", "dart-sass/sass", "sass:color", "sass",
                 "bubbles", "ResizeObserver.prototype.disconnect", "MutationObserver.prototype.disconnect",
                 "https://github.com/sass/dart-sass/releases/download/1.102.0/dart-sass-1.102.0-linux-arm64.tar.gz")
        matcher = private_scan.Matcher({(word,) for fact in facts for word in private_scan.words(fact)})
        for fact in facts:
            self.assertFalse(matcher.hit(fact))
            self.assertTrue(matcher.hit(fact.upper()))

    def test_real_facts_do_not_exempt_surrounding_file_or_index_text(self):
        with tempfile.TemporaryDirectory() as repo:
            git(repo, "init", "-q")
            matcher = private_scan.Matcher({("sentinelprivate",)})
            for fact in private_scan.PUBLIC_IDENTIFIERS + (private_scan.PUBLIC_COMPILER_URL,):
                for binary in (False, True):
                    full = os.path.join(repo, "report")
                    write(full, (fact + " sentinelprivate").encode() + (b"\0" if binary else b"\n"), "wb")
                    git(repo, "add", "report")
                    self.assertTrue(private_scan.scan_paths(matcher, [full], repo)[0])
                    write(full, fact.encode(), "wb")
                    self.assertTrue(private_scan.scan_staged(matcher, repo)[0])


class PublicDockerOptionTests(unittest.TestCase):
    """Only the approved, complete quoted Docker spelling is public."""

    check_all = PublicInterfaceTests.check_all

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.repo = self.temp.name
        git(self.repo, "init", "-q")
        self.option = private_scan.PUBLIC_DOCKER_OPTION
        self.option_words = private_scan.words(self.option)
        self.matcher = private_scan.Matcher({(word,) for word in self.option_words})

    def check_decoded_and_files(self, text, expected, matcher=None):
        matcher = matcher or self.matcher
        encoders = (lambda value: value,
                    lambda value: "".join("&#%d;" % ord(c) for c in value),
                    lambda value: "".join("\\u%04x" % ord(c) for c in value),
                    lambda value: "".join("%%%02X" % ord(c) for c in value))
        for encode in encoders:
            encoded = encode(text)
            self.assertEqual(bool(private_scan.line_hits(matcher, encoded)), expected, text)
            self.check_all(encoded, expected, matcher)

    def test_exact_single_and_double_quoted_option_is_public(self):
        self.assertEqual(self.option, "no-new-privileges")
        for quote in ('"', "'"):
            literal = quote + self.option + quote
            self.check_decoded_and_files(literal, False)
            self.check_decoded_and_files('docker("--security-opt", ' + literal + ')', False)
            self.check_decoded_and_files(literal, False,
                                         private_scan.Matcher({tuple(self.option_words)}))

    def test_bare_modified_and_incomplete_literals_are_scanned(self):
        for text in (self.option, '`' + self.option + '`',
                     '"' + self.option + "'", '"' + self.option,
                     '"' + self.option.upper() + '"',
                     '"prefix-' + self.option + '"', '"' + self.option + '-suffix"',
                     '"' + self.option + '=true"', '" ' + self.option + '"',
                     '"' + self.option + '\u034f"'):
            self.check_decoded_and_files(text, True)

    def test_surrounding_and_crossing_tokens_remain_scanned(self):
        literal = '"' + self.option + '"'
        cases = (("sentinelprivate " + literal, ("sentinelprivate",)),
                 (literal + " sentinelprivate", ("sentinelprivate",)),
                 ("sentinelprivate " + literal, ("sentinelprivate", self.option_words[0])),
                 (literal + " sentinelprivate", (self.option_words[-1], "sentinelprivate")),
                 (literal + ", " + literal, (self.option_words[-1], self.option_words[0])))
        for text, token in cases:
            self.check_decoded_and_files(text, True, private_scan.Matcher({token}))


class PublicSwiftImportTests(unittest.TestCase):
    """E-20261102T070407Z-6a04 approves one complete compiler fact."""

    check_all = PublicInterfaceTests.check_all

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.repo = self.temp.name
        git(self.repo, "init", "-q")
        self.fact = private_scan.PUBLIC_SWIFT_IMPORT
        self.prefix, self.module = self.fact.split(" ", 1)
        self.ws = private_scan.words(self.fact)
        self.matcher = private_scan.Matcher({(word,) for word in self.ws})

    def check_decoded_and_files(self, text, expected, matcher=None):
        matcher = matcher or self.matcher
        encoders = (lambda value: value,
                    lambda value: "".join("&#%d;" % ord(c) for c in value),
                    lambda value: "".join("\\u%04x" % ord(c) for c in value),
                    lambda value: "".join("\\U%08x" % ord(c) for c in value),
                    lambda value: "".join("%%%02X" % c for c in value.encode("utf-8")))
        for encode in encoders:
            encoded = encode(text)
            with self.subTest(text=text, encoded=encoded):
                self.assertEqual(any(matcher.hit(v) for v in private_scan.variants(encoded)), expected)
                self.assertEqual(bool(private_scan.line_hits(matcher, encoded)), expected)
                self.check_all(encoded, expected, matcher)

    def test_exact_approved_literal_digest_and_restored_source(self):
        self.assertEqual(self.fact, "import MachO")
        self.assertEqual(private_scan.PUBLIC_SWIFT_IMPORT_SHA256,
                         "0cd4593e5b7c20a193357a8d46e91ae8d04f5d92d0ecf4aa3167ac4dc0379c5a")
        self.assertEqual(hashlib.sha256(self.fact.encode("utf-8")).hexdigest(),
                         private_scan.PUBLIC_SWIFT_IMPORT_SHA256)
        root = Path(SCRIPT).resolve().parents[3]
        source = root / "app/Packages/MeguriKit/Tests/AppModelTests/CloudKitAgent/Support/AgentRunningProduct.swift"
        lines = source.read_text(encoding="utf-8").splitlines()
        self.assertEqual(lines.count(self.fact), 1)
        self.assertEqual(lines[:4], ["import Darwin", "import Foundation", self.fact, "import ObjectiveC"])

    def test_only_complete_raw_and_quoted_spans_hide_contained_matches(self):
        for text in (self.fact, '"' + self.fact + '"', "'" + self.fact + "'", '`' + self.fact + '`'):
            for matcher in (self.matcher, private_scan.Matcher({tuple(self.ws)})):
                self.check_decoded_and_files(text, False, matcher)
        self.assertEqual([m.group() for m in private_scan.PUBLIC_LITERAL.finditer(self.fact)], [self.fact])

    def test_bare_module_case_spacing_and_incomplete_quotes_stay_checked(self):
        for text in (self.module, self.fact.lower(), self.fact.upper(), self.fact.swapcase(),
                     self.prefix + "  " + self.module, self.prefix + "\t" + self.module,
                     self.prefix + " /* note */ " + self.module,
                     self.prefix + "\n" + self.module,
                     '"' + self.fact, self.fact + '"', '"' + self.fact + "'",
                     '"prefix ' + self.fact + '"', '"' + self.fact + ' suffix"'):
            self.check_decoded_and_files(text, True)
        other = self.prefix + " CustomSDK"
        self.check_decoded_and_files(other, True, private_scan.Matcher({tuple(private_scan.words(other))}))

    def test_custom_compounds_qualification_and_unicode_remain_checked(self):
        for prefix in ("custom.", ".", "custom::", "x", "_", "-", "\u0301", "\u034f", "\u03bb"):
            self.check_decoded_and_files(prefix + self.fact, True)
        for suffix in (".Custom", "::Custom", "x", "_extra", "-extra", "\u0301", "\u034f", "\u03bb"):
            self.check_decoded_and_files(self.fact + suffix, True)
        for quote in ('"', "'", '`'):
            self.check_decoded_and_files("custom." + quote + self.fact + quote, True)
            self.check_decoded_and_files(quote + self.fact + quote + ".Custom", True)

    def test_surrounding_and_crossing_tokens_are_never_hidden(self):
        sentinel = "sentinelprivate"
        for quote in ("", '"', "'", '`'):
            literal = quote + self.fact + quote
            for text, token in ((sentinel + " " + literal, (sentinel,)),
                                (literal + " " + sentinel, (sentinel,)),
                                (sentinel + " " + literal, (sentinel, self.ws[0])),
                                (literal + " " + sentinel, (self.ws[-1], sentinel)),
                                (literal + " " + literal, (self.ws[-1], self.ws[0]))):
                self.check_decoded_and_files(text, True, private_scan.Matcher({token}))
        # One approved line does not exempt any other line of the file.
        self.check_all(self.fact + "\n" + self.module, True)
        self.check_all(self.module + "\n" + self.fact, True)

    def test_whitespace_and_comments_cannot_separate_qualification(self):
        gaps = (" \t", "\n ", " /* note */ ", " // note\n ",
                " /* first\n second */\n ", " /* outer /* inner */ tail */ ")
        for gap in gaps:
            for text in ("custom." + gap + self.fact, self.fact + gap + ".Custom",
                         "custom::" + gap + self.fact, self.fact + gap + "::Custom",
                         self.fact + gap + "_extra"):
                self.check_decoded_and_files(text, True)
        # Preserve real line/run boundaries while encoding the adjacent trivia.
        contexts = (("custom&#46; &#47;&#47; note\n", ""),
                    ("custom\\x2e \\x2f\\x2f note\n", ""),
                    ("custom%2E %2F%2F note\n", ""),
                    ("", "\n&#46;Custom"), ("", "\n\\x2eCustom"), ("", "\n%2ECustom"),
                    ("", "\n&#47;&#42; note &#42;&#47; .Custom"),
                    ("", "\n\\x2f\\x2a note \\x2a\\x2f .Custom"),
                    ("", "\n%2F%2A note %2A%2F .Custom"))
        for prefix, suffix in contexts:
            text = prefix + self.fact + suffix
            self.assertTrue(private_scan.line_hits(self.matcher, text))
            self.check_all(text, True)

    def test_index_context_uses_staged_bytes_and_unchanged_adjacent_lines(self):
        full = os.path.join(self.repo, "report.swift")
        # The qualification is committed; only the newly added import is scanned.
        write(full, "custom.\n")
        git(self.repo, "add", "report.swift")
        git(self.repo, "commit", "-qm", "qualification context")
        for binary in (False, True):
            tail = b"\0" if binary else b"\n"
            for text in ("custom.\n" + self.fact, self.fact + "\n.Custom",
                         self.fact + " /* note */\n.Custom"):
                write(full, text.encode("utf-8") + tail, "wb")
                git(self.repo, "add", "report.swift")
                write(full, self.fact.encode("utf-8") + tail, "wb")
                self.assertFalse(private_scan.scan_paths(self.matcher, [full], self.repo)[0])
                self.assertTrue(private_scan.scan_staged(self.matcher, self.repo)[0])
                git(self.repo, "reset", "-q")


class PublicRecoveryRuleTests(unittest.TestCase):
    """The 2026-11-14 approval covers one complete public prose span."""

    check_all = PublicInterfaceTests.check_all
    check_decoded_and_files = PublicSwiftImportTests.check_decoded_and_files

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.repo = self.temp.name
        git(self.repo, "init", "-q")
        self.fact = private_scan.PUBLIC_RECOVERY_RULE
        self.ws = private_scan.words(self.fact)
        self.sentinel = "sentinelprivate"
        self.matcher = private_scan.Matcher({(word,) for word in self.ws})

    def test_exact_approved_bytes_and_digest(self):
        approved = b"1. This exception takes effect only within the final transaction of the matching successful recovery operation: adoption at V14.2 step 9, or publication at V15.2 step 4. It permits removal without settlement solely for the covered blocked ordinary work."
        digest = "1223e97157bb7d43dc1ce067b92443dd37014a8424b1c7af45c6e3851d5cca06"
        self.assertEqual(self.fact.encode("utf-8"), approved)
        self.assertEqual(private_scan.PUBLIC_RECOVERY_RULE_SHA256, digest)
        self.assertEqual(hashlib.sha256(approved).hexdigest(), digest)
        self.assertEqual([m.group() for m in private_scan.PUBLIC_LITERAL.finditer(self.fact)], [self.fact])

    def test_complete_literal_exempts_contained_single_and_multiword_matches(self):
        for text in (self.fact, '"' + self.fact + '"', "'" + self.fact + "'",
                     '`' + self.fact + '`', 'rule = r"' + self.fact + '"'):
            for matcher in (self.matcher, private_scan.Matcher({tuple(self.ws)})):
                self.check_decoded_and_files(text, False, matcher)

    def test_case_modified_partial_and_bare_text_remains_checked(self):
        for text in (self.fact.lower(), self.fact.upper(), self.fact.swapcase(),
                     re.sub(r"(?<=\.)\d+", lambda m: str(int(m.group()) + 1), self.fact),
                     re.sub(r"[^\W\d_]+", self.sentinel, self.fact, count=1),
                     self.fact.replace(" ", "  ", 1),
                     self.fact.replace(" ", "\t", 1),
                     self.fact.replace(" ", "\n", 1),
                     self.fact[:-1], self.fact.partition(" ")[2], self.fact.rsplit(". ", 1)[0],
                     " ".join(self.ws[-3:])):
            self.check_decoded_and_files(text, True)
        for word in self.ws:
            with self.subTest(word=word):
                self.check_decoded_and_files(word, True)

    def test_python_prefix_outside_and_crossing_matches_remain_checked(self):
        sentinel = self.sentinel
        cases = ((sentinel + " " + self.fact, (sentinel,)),
                 (self.fact + " " + sentinel, (sentinel,)),
                 (sentinel + " " + self.fact, (sentinel, self.ws[0])),
                 (self.fact + " " + sentinel, (self.ws[-1], sentinel)),
                 (self.fact + " " + self.fact, (self.ws[-1], self.ws[0])),
                 ('rule = r"' + self.fact + '"', ("rule",)),
                 ('rule = r"' + self.fact + '"', ("r", self.ws[0])))
        for text, token in cases:
            self.check_decoded_and_files(text, True, private_scan.Matcher({token}))
        for prefix in ("", "r", "u", "b", "f"):
            for text in ('rule = ' + prefix + '"' + sentinel + ' ' + self.fact + '"',
                         'rule = ' + prefix + '"' + self.fact + ' ' + sentinel + '"'):
                self.check_decoded_and_files(text, True, private_scan.Matcher({(sentinel,)}))
        for word in self.ws:
            self.check_all(self.fact + "\n" + word, True)
            self.check_all(word + "\n" + self.fact, True)

    def test_hex_escapes_keep_exact_and_crossing_behavior(self):
        sentinel = self.sentinel
        cases = [(self.fact, (word,), False) for word in self.ws]
        cases += [(self.fact.lower(), (word,), True) for word in self.ws]
        cases += [(sentinel + " " + self.fact, (sentinel,), True),
                  (sentinel + " " + self.fact, (sentinel, self.ws[0]), True),
                  (self.fact + " " + sentinel, (self.ws[-1], sentinel), True)]
        for text, token, expected in cases:
            encoded = "".join("\\x%02x" % ord(c) for c in text)
            matcher = private_scan.Matcher({token})
            self.assertEqual(bool(private_scan.line_hits(matcher, encoded)), expected)
            self.check_all(encoded, expected, matcher)

    def test_committed_literal_keeps_candidates_and_bare_runtime_hits(self):
        candidates = os.path.join(self.repo, "candidates.jsonl")
        tokens = os.path.join(self.repo, "synthetic-tokens.txt")
        case_data = self.ws + [self.fact, self.sentinel]
        write(candidates, "".join(json.dumps({"kind": "deck", "text": text}) + "\n"
                                  for text in case_data))
        expected = sorted({" ".join(private_scan.words(text)) for text in case_data})
        public = os.path.join(self.repo, "public.py")
        for text in (self.fact, '"' + self.fact + '"', "'" + self.fact + "'",
                     '`' + self.fact + '`', 'rule = r"' + self.fact + '"'):
            with self.subTest(text=text):
                write(public, text + "\n")
                git(self.repo, "add", "public.py")
                git(self.repo, "commit", "-qm", "fixed fact")
                result = run("build", "--candidates", candidates, "--repo", self.repo, "--out", tokens)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(Path(tokens).read_text(encoding="utf-8").splitlines(), expected)
                self.assertEqual(json.loads(result.stdout)["in_repository"], 0)
                matcher = private_scan.load_tokens([tokens])
                self.check_decoded_and_files(text, False, matcher)
                for word in self.ws + [self.sentinel]:
                    self.check_all(word, True, matcher)

    def test_index_snapshot_is_checked(self):
        full = os.path.join(self.repo, "rule.py")
        tokens = os.path.join(self.repo, "synthetic-tokens.txt")
        sentinel = self.sentinel
        write(tokens, "".join(word + "\n" for word in self.ws + [sentinel]))
        # An unchanged adjacent line stays outside the staged addition.
        write(full, "# " + sentinel + "\n")
        git(self.repo, "add", "rule.py")
        git(self.repo, "commit", "-qm", "synthetic context")
        for binary in (False, True):
            tail = b"\0" if binary else b"\n"
            for staged, working, expected in (
                    (self.fact, sentinel, False),
                    ('rule = r"' + sentinel + ' ' + self.fact + '"', self.fact, True),
                    ('rule = r"' + self.fact + ' ' + sentinel + '"', self.fact, True)):
                # Text scans only added lines; binary scans the entire file.
                context = "" if binary else "# " + sentinel + "\n"
                write(full, (context + staged).encode("utf-8") + tail, "wb")
                git(self.repo, "add", "rule.py")
                write(full, working.encode("utf-8") + tail, "wb")
                result = run("scan", "--tokens", tokens, "--staged", "--repo", self.repo)
                self.assertEqual(result.returncode, int(expected), result.stderr)
                self.assertEqual(bool(json.loads(result.stdout)["files"]), expected)
                result = run("scan", "--tokens", tokens, full)
                self.assertEqual(result.returncode, int(not expected), result.stderr)
                self.assertEqual(bool(json.loads(result.stdout)["files"]), not expected)
                git(self.repo, "reset", "-q")


class PublicSyncPlanTests(unittest.TestCase):
    def test_exact_approved_documents_and_index_bytes(self):
        data = b"meguri sync approved technical fixture\n"
        digest = hashlib.sha256(data).hexdigest()
        paths = ("docs/plans/2026-11-15-1028-feat-meguri-sync-server-plan.md",
                 "docs/process/review-log/meguri-sync-plan-20261115.md")
        # Public words stand in for private tokens; no real token set is read.
        matcher = private_scan.Matcher({("meguri",), ("sync",), ("sentinelprivate",)})
        for path in paths:
            # Real plans may change. Only fixed synthetic bytes are approved in
            # this test; revised content must pass through the normal scanner.
            with self.subTest(path=path), tempfile.TemporaryDirectory() as repo, \
                    mock.patch.object(private_scan, "PUBLIC_ARTIFACTS", {path: digest}):
                self.assertTrue(private_scan.line_hits(matcher, data.decode("utf-8")))
                git(repo, "init", "-q")
                full = os.path.join(repo, path)
                for changed, expected in ((data, False), (data + b" ", True),
                                          (b"meguri sync revised technical fixture\n", True),
                                          (data + b"\nsentinelprivate\n", True)):
                    write(full, changed, "wb")
                    git(repo, "add", "--", path)
                    self.assertEqual(bool(private_scan.scan_paths(matcher, [full], repo)[0]), expected)
                    self.assertEqual(bool(private_scan.scan_staged(matcher, repo)[0]), expected)
                # The index controls staged approval even when disk bytes differ.
                write(full, data, "wb")
                self.assertFalse(private_scan.scan_paths(matcher, [full], repo)[0])
                self.assertTrue(private_scan.scan_staged(matcher, repo)[0])
                git(repo, "add", "--", path)
                write(full, data + b"\nsentinelprivate\n", "wb")
                self.assertTrue(private_scan.scan_paths(matcher, [full], repo)[0])
                self.assertFalse(private_scan.scan_staged(matcher, repo)[0])
                # Identical approved bytes at a different path stay scanned.
                copied = "copies/" + path
                other = os.path.join(repo, copied)
                write(other, data, "wb")
                git(repo, "add", "--", copied)
                self.assertTrue(private_scan.scan_paths(matcher, [other], repo)[0])
                found, _ = private_scan.scan_staged(matcher, repo)
                self.assertEqual([item["path"] for item in found], [copied])

    def test_original_review_note_grant_is_unchanged(self):
        self.assertEqual(
            private_scan.PUBLIC_ARTIFACTS["docs/process/review-log/meguri-sync-plan-20261115.md"],
            "3fefeb90235071d6e3d6ad364443276396cc01d9a9f4ccfa63bd6f80e99316b8")


class PublicLibraryTests(unittest.TestCase):
    def test_public_asset_mutations(self):
        root = Path(SCRIPT).resolve().parents[3]
        facts = {path: private_scan.PUBLIC_ARTIFACTS[path] for path in (
            private_scan.PUBLIC_NOTICE_PATH,
            "specs/licenses/jquery-3.7.1-LICENSE.txt",
            "app/Packages/MeguriKit/Sources/CardRendering/Resources/CardRuntime/vendor/jquery-3.7.1.min.js",
        )}
        matched = 0
        for raw_path in private_scan.git(str(root), "ls-files", "-z").split(b"\0"):
            if not raw_path:
                continue
            digest = private_scan.PUBLIC_VENDOR_ARTIFACTS.get(hashlib.sha256(raw_path).hexdigest())
            if digest is not None:
                facts[os.fsdecode(raw_path)] = digest
                matched += 1
        self.assertEqual(matched, len(private_scan.PUBLIC_VENDOR_ARTIFACTS))
        for path, digest in facts.items():
            if path == private_scan.PUBLIC_NOTICE_PATH:
                continue
            data = (root / path).read_bytes()
            self.assertEqual(hashlib.sha256(data).hexdigest(), digest)
            matcher = private_scan.Matcher({(word,) for word in private_scan.words(data.decode("utf-8"))}
                                           | {("sentinelprivate",)})
            with tempfile.TemporaryDirectory() as repo:
                git(repo, "init", "-q")
                full = os.path.join(repo, path)
                for changed, expected in ((data, False), (data + b" ", True),
                                          (data + b"\nsentinelprivate\n", True)):
                    write(full, changed, "wb")
                    git(repo, "add", "--", path)
                    self.assertEqual(bool(private_scan.scan_staged(matcher, repo)[0]), expected)
                    self.assertEqual(bool(private_scan.scan_paths(matcher, [full], repo)[0]), expected)
                write(full, data, "wb")
                self.assertTrue(private_scan.scan_staged(matcher, repo)[0])
                other = os.path.join(repo, "other.txt")
                write(other, data, "wb")
                self.assertTrue(private_scan.scan_paths(matcher, [other], repo)[0])


class PublicSourceLineTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.repo = self.temp.name
        git(self.repo, "init", "-q")
        self.path = "vendor/sample/config.rs"
        self.public = b"copyright upstreamtoken\n"
        self.matcher = private_scan.Matcher({("upstreamtoken",), ("sentinelprivate",)})
        facts = {self.path: {hashlib.sha256(self.public).hexdigest()}}
        patch = mock.patch.object(private_scan, "PUBLIC_SOURCE_LINES", facts, create=True)
        patch.start()
        self.addCleanup(patch.stop)

    def check_both(self, data, expected, path=None):
        path = self.path if path is None else path
        full = os.path.join(self.repo, path)
        write(full, data, "wb")
        git(self.repo, "add", "--", path)
        self.assertEqual(bool(private_scan.scan_staged(self.matcher, self.repo)[0]), expected)
        self.assertEqual(bool(private_scan.scan_paths(self.matcher, [full], self.repo)[0]), expected)

    def test_only_exact_complete_public_line_qualifies(self):
        self.check_both(self.public, False)
        for changed in (b" " + self.public, self.public[:-1] + b" \n",
                        self.public.replace(b"\n", b"\r\n"), self.public[:-1],
                        self.public[:-1] + b" sentinelprivate\n"):
            with self.subTest(changed=changed):
                self.check_both(changed, True)

    def test_surrounding_rows_and_encoded_content_get_checked(self):
        self.check_both(self.public + b"sentinelprivate\n", True)
        self.matcher = private_scan.Matcher({("sentinelprivate",)})
        self.check_both(self.public[:-1] + br" sentinel\u0070rivate" + b"\n", True)
        self.check_both(b"fine\xe2\x80\xa8sentinelprivate\n" + self.public, True)

    def test_same_line_at_other_path_is_not_public(self):
        self.check_both(self.public, True, "other.rs")

    def test_staged_scan_uses_index_bytes(self):
        full = os.path.join(self.repo, self.path)
        self.check_both(self.public + b"sentinelprivate\n", True)
        write(full, self.public, "wb")
        self.assertTrue(private_scan.scan_staged(self.matcher, self.repo)[0])
        self.assertFalse(private_scan.scan_paths(self.matcher, [full], self.repo)[0])
        git(self.repo, "add", "--", self.path)
        write(full, self.public + b"sentinelprivate\n", "wb")
        self.assertFalse(private_scan.scan_staged(self.matcher, self.repo)[0])
        self.assertTrue(private_scan.scan_paths(self.matcher, [full], self.repo)[0])

    def test_binary_files_keep_full_scan(self):
        self.check_both(b"\0" + self.public, True)


    @unittest.skipUnless(os.name == "posix", "raw filenames require POSIX")
    def test_raw_filename_bytes_do_not_skip_the_scan(self):
        raw = os.path.join(os.fsencode(self.repo), b"bad-\xff.rs")
        try:
            write(raw, b"sentinelprivate\n", "wb")
        except OSError as error:
            # APFS (macOS) refuses names that are not valid UTF-8; the dev
            # container's filesystem accepts them, so tools/mg selftest runs it.
            if error.errno != errno.EILSEQ:
                raise
            self.skipTest("this filesystem refuses non-UTF-8 file names (EILSEQ)")
        git(self.repo, "add", "-A")
        self.assertTrue(private_scan.scan_staged(self.matcher, self.repo)[0])
        self.assertTrue(private_scan.scan_paths(self.matcher, [os.fsdecode(raw)], self.repo)[0])


class PublicNoticeTests(unittest.TestCase):
    """A fixed public notice may pass only with its complete verified bytes."""

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.repo = self.temp.name
        git(self.repo, "init", "-q")
        self.path = "specs/render/unicode-data-LICENSE.txt"
        source = os.path.abspath(os.path.join(os.path.dirname(SCRIPT), "../../..", self.path))
        with open(source, "rb") as f:
            self.notice = f.read()
        self.matcher = private_scan.Matcher({("unicode",), ("sentinelprivate",)})

    def check_both(self, path, data, expected):
        full = os.path.join(self.repo, path)
        write(full, data, "wb")
        git(self.repo, "add", "--", path)
        staged, count = private_scan.scan_staged(self.matcher, self.repo)
        self.assertEqual(bool(staged), expected)
        self.assertEqual(count, 1)
        files, count = private_scan.scan_paths(self.matcher, [full], repo=self.repo)
        self.assertEqual(bool(files), expected)
        self.assertEqual(count, 1)

    def test_exact_official_notice(self):
        self.assertEqual(hashlib.sha256(self.notice).hexdigest(),
                         "e7a93b009565cfce55919a381437ac4db883e9da2126fa28b91d12732bc53d96")
        self.check_both(self.path, self.notice, False)

    def test_one_byte_change_is_scanned(self):
        self.check_both(self.path, self.notice + b" ", True)

    def test_appended_private_text_is_scanned(self):
        self.check_both(self.path, self.notice + b"\nsentinelprivate\n", True)

    def test_notice_at_another_path_is_scanned(self):
        self.check_both("other-LICENSE.txt", self.notice, True)

    def test_index_bytes_control_staged_scan(self):
        full = os.path.join(self.repo, self.path)
        write(full, self.notice + b"\nsentinelprivate\n", "wb")
        git(self.repo, "add", "--", self.path)
        write(full, self.notice, "wb")
        found, _ = private_scan.scan_staged(self.matcher, self.repo)
        self.assertTrue(found)

    def test_known_notice_never_exempts_other_files(self):
        write(os.path.join(self.repo, self.path), self.notice, "wb")
        write(os.path.join(self.repo, "report.txt"), "sentinelprivate\n")
        git(self.repo, "add", "-A")
        found, _ = private_scan.scan_staged(self.matcher, self.repo)
        self.assertEqual(found, [{"path": "report.txt", "lines": [1]}])


if __name__ == "__main__":
    unittest.main()
