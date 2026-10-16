"""Tests for private_scan (U15, R1940): the token set built from candidate
strings and pruned by the repository, and the scan of files and of a
commit's staged lines. Every collection here is made up."""

import hashlib
import io
import json
import os
import re
import struct
import subprocess
import sys
import tempfile
import unittest
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


if __name__ == "__main__":
    unittest.main()
