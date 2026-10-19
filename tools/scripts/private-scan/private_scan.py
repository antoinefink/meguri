#!/usr/bin/env python3
"""The output guard's token set and scanner (U15, R1940).

    private_scan.py build --candidates <jsonl> --repo <dir> --out <tokens.txt>
    private_scan.py scan --tokens <tokens.txt> [--tokens ...] (--staged --repo <dir> | <path>...)

`build` turns the candidate strings `meguri-cli private tokens` read from a
copy of the collection into tokens: each deck name, tag and media name is one
token (its word sequence), whatever its length; each field word of four or
more characters that holds a letter is one token. Tokens that already occur
in a file committed at the repository's HEAD are dropped: they are not
private, and keeping them would block ordinary commits. Only committed text
counts, never the working tree or the index (a private word written into a
tracked file must not exempt itself before that file is scanned and
committed), and never `conformance/reports/private/`, which holds results
from the collection. It writes one token per line (words joined by one
space) and `meta.json` (counts only) beside it.

`scan` reports the files that contain a token, as whole words: whole files
for paths (folders are walked), or the lines a commit adds for `--staged`
(binary files whole, as runs of printable text: see PRINTABLE). A line
also counts when its HTML entities, JSON or Python escapes, or URL
percent-escapes hide a token. It prints the files and line numbers, never a
token.

Words are maximal runs of letters and digits (`str.isalnum`) after NFC
normalization and case folding, so `Hörverstehen` in a deck name matches
`HÖRVERSTEHEN` and `h\\u00f6rverstehen` in a JSON file.

stdout: one JSON object. Exit 0: nothing found (scan) or built; 1: a token
found; 2: usage or input error. Runs on macOS's Python 3.9 (the hook) and in
the containers.
"""

import argparse
import hashlib
import html
import json
import os
import re
import subprocess
import sys
import unicodedata
import urllib.parse
import zlib

WORD = re.compile(r"[^\W_]+")
MIN_CHARS = 4
# Binary files are scanned as runs of printable characters: valid UTF-8 other
# than C0 controls, DEL and U+FFFD. SQLite keeps text as is, between record
# headers and integers. 0x1f belongs to a run as a separator: Anki puts it
# between a note's fields and between a deck name's components, so a nested
# deck name reads as its words.
PRINTABLE = (
    rb"(?:[\x1f-\x7e]"
    rb"|[\xc2-\xdf][\x80-\xbf]"
    rb"|\xe0[\xa0-\xbf][\x80-\xbf]"
    rb"|[\xe1-\xec\xee][\x80-\xbf]{2}"
    rb"|\xef(?:[\x80-\xbe][\x80-\xbf]|\xbf[\x80-\xbc\xbe\xbf])"
    rb"|\xed[\x80-\x9f][\x80-\xbf]"
    rb"|\xf0[\x90-\xbf][\x80-\xbf]{2}"
    rb"|[\xf1-\xf3][\x80-\xbf]{3}"
    rb"|\xf4[\x80-\x8f][\x80-\xbf]{2})"
)
TEXT_RUN = re.compile(PRINTABLE + rb"+")
# Every run is scanned, whatever its length, except in the ranges a file's
# format marks as compressed data, fixed numbers or tables (opaque_ranges:
# PNG, JPEG, zip, zstd). There printable runs equal short words by chance (a
# megabyte of deflate data holds thousands of two-to-seven-letter words), so
# only a token of LONG_TOKEN UTF-8 bytes or more counts. Such a token takes
# two characters at least. A file in any other format is text throughout.
LONG_TOKEN = 8
OPAQUE_RUN = re.compile(PRINTABLE + rb"{2,}")
SURROGATE_PAIR = re.compile(r"\\u(d[89ab][0-9a-f]{2})\\u(d[c-f][0-9a-f]{2})", re.IGNORECASE)
ESCAPE = re.compile(r"\\(?:u([0-9a-fA-F]{4})|U([0-9a-fA-F]{8})|x([0-9a-fA-F]{2}))")
SNIFF = 8192

# Fixed interoperability spellings (render/card-runtime C4.1) and compiler
# names, module and archive from the official version 1.102.0 distribution,
# independently verified by css-reference's pinned archive and module check.
# Only complete literal spans qualify;
# no caller-provided file or list can exempt surrounding text. Match before
# case folding, including in each decoded variant the scanner already checks.
PUBLIC_IDENTIFIERS = ("--canvas-elevated", "canvas-elevated", "dart-sass/sass", "dart-sass", "sass:color", "sass")
PUBLIC_COMPILER_URL = "https://github.com/sass/dart-sass/releases/download/1.102.0/dart-sass-1.102.0-linux-arm64.tar.gz"
PUBLIC_LITERAL = re.compile(
    r"(?<![^\s\"'`<>()\[\]{},;:=|])(?:" + "|".join(map(re.escape, PUBLIC_IDENTIFIERS)) +
    r")(?=$|[\s\"'`<>()\[\]{},;:=|])"
    r"|(?<![^\s\"'`<>()\[\]{},;=|])" + re.escape(PUBLIC_COMPILER_URL) +
    r"(?=$|[\s\"'`<>()\[\]{},;|])"
)


def words(text):
    """The words of `text`: NFC, case-folded, maximal runs of letters and digits."""
    return WORD.findall(unicodedata.normalize("NFC", text).casefold())


def keep_word(word):
    """A field word is a token when it has four or more characters and a letter."""
    return len(word) >= MIN_CHARS and any(c.isalpha() for c in word)


def tokens_of(kind, text):
    """The tokens one candidate gives. The length rule is for field words
    only: a deck, tag or media name is private whatever its length."""
    if kind == "word":
        return [(w,) for w in words(text) if keep_word(w)]
    token = tuple(words(text))
    return [token] if token else []


class Matcher:
    """Finds tokens (tuples of words) in a word list, at word boundaries."""

    def __init__(self, tokens):
        self.single = set()
        self.multi = set()
        self.lengths = {}
        for token in tokens:
            if len(token) == 1:
                self.single.add(token[0])
            else:
                self.multi.add(token)
                self.lengths.setdefault(token[0], set()).add(len(token))
        self.count = len(self.single) + len(self.multi)

    def occurrences(self, ws):
        """Yields each token and its word interval (with repeats)."""
        for i, w in enumerate(ws):
            if w in self.single:
                yield (w,), i, i + 1
            for n in self.lengths.get(w, ()):
                token = tuple(ws[i : i + n])
                if len(token) == n and token in self.multi:
                    yield token, i, i + n

    def find(self, ws):
        """Yields each token found in `ws` (with repeats)."""
        for token, _, _ in self.occurrences(ws):
            yield token

    def scan_tokens(self, text):
        """Retain every match except those fully inside fixed public facts."""
        text = unicodedata.normalize("NFC", text)
        spans = [(len(text[:m.start()].casefold()), len(text[:m.end()].casefold()))
                 for m in PUBLIC_LITERAL.finditer(text)]
        if not spans:
            yield from self.find(words(text))
            return
        matches = list(WORD.finditer(text.casefold()))
        ws = [m.group() for m in matches]
        for token, first, end in self.occurrences(ws):
            start_char, end_char = matches[first].start(), matches[end - 1].end()
            if not any(start <= start_char and end_char <= stop for start, stop in spans):
                yield token

    def hit(self, text):
        for _ in self.scan_tokens(text):
            return True
        return False


def _unescape_codes(text):
    def pair(m):
        high, low = int(m.group(1), 16), int(m.group(2), 16)
        return chr(0x10000 + ((high - 0xD800) << 10) + (low - 0xDC00))

    def one(m):
        code = int(next(g for g in m.groups() if g is not None), 16)
        try:
            return chr(code)
        except (ValueError, OverflowError):
            return m.group(0)

    return ESCAPE.sub(one, SURROGATE_PAIR.sub(pair, text))


def variants(line):
    """The line and the decodings that could hide a token in it."""
    out = [line]
    if "&" in line:
        out.append(html.unescape(line))
    if "\\" in line:
        out.append(_unescape_codes(line))
    if "%" in line:
        out.append(urllib.parse.unquote(line, errors="replace"))
    return out


def line_hits(matcher, text, first=1):
    """Line numbers (from `first`) of the lines of `text` holding a token."""
    hits = []
    for n, line in enumerate(text.splitlines(), first):
        if any(matcher.hit(v) for v in variants(line)):
            hits.append(n)
    return hits


# ------------------------------------------------------------ binary files
#
# opaque_ranges marks the byte ranges a known format defines as compressed
# data, fixed numbers or tables. Everything else (names, comments, metadata,
# stored data, unknown chunks, bytes past the end, any file of another
# format, and a format whose structure does not check out from the point
# where it stops) is text. Each parser returns at the first byte that breaks
# its format, keeping the ranges it has checked.


def _u16(data, at, order="little"):
    return int.from_bytes(data[at:at + 2], order)


def _u32(data, at, order="little"):
    return int.from_bytes(data[at:at + 4], order)


PNG_SIGNATURE = b"\x89PNG\r\n\x1a\n"
# Chunks whose data is image data, numbers or palettes, never text. iDOT is
# Apple's (integers locating the image data): every screenshot and snapshot
# test image made on a Mac or an iPhone has one.
PNG_OPAQUE = {b"IHDR", b"PLTE", b"IDAT", b"IEND", b"tRNS", b"gAMA", b"cHRM", b"sRGB", b"sBIT", b"bKGD", b"hIST",
              b"pHYs", b"tIME", b"acTL", b"fcTL", b"fdAT", b"iDOT"}


def _png(data, start, end, out):
    """The signature, each chunk's length, type and CRC (a chunk counts only
    when its CRC matches), the data of PNG_OPAQUE chunks, and the compressed
    text of zTXt, iCCP and a compressed iTXt (their keywords are text)."""
    out.append((start, start + 8))
    p = start + 8
    while p + 12 <= end:
        length = _u32(data, p, "big")
        kind, body, tail = data[p + 4:p + 8], p + 8, p + 8 + length
        if tail + 4 > end or zlib.crc32(data[p + 4:tail]) != _u32(data, tail, "big"):
            return
        out += [(p, body), (tail, tail + 4)]
        nul = data.find(b"\0", body, tail)
        if kind in PNG_OPAQUE:
            out.append((body, tail))
        elif kind in (b"zTXt", b"iCCP") and nul >= 0:
            # keyword NUL method compressed-text
            out.append((nul + 1, tail))
        elif kind == b"iTXt" and nul >= 0 and nul + 1 < tail and data[nul + 1] == 1:
            # keyword NUL flag method language NUL translated-keyword NUL text
            language = data.find(b"\0", nul + 3, tail)
            translated = data.find(b"\0", language + 1, tail) if language >= 0 else -1
            if translated >= 0:
                out.append((translated + 1, tail))
        p = tail + 4
        if kind == b"IEND":
            return


def _jpeg(data, start, end, out):
    """Markers, segment lengths, the data of every segment but APPn and COM
    (tables, frame and scan headers), and the entropy-coded data after each
    scan header. Bytes after the end marker are text."""
    out.append((start, start + 2))
    p = start + 2
    while p + 2 <= end:
        if data[p] != 0xFF:
            return
        marker = data[p + 1]
        if marker == 0xFF:  # fill byte
            out.append((p, p + 1))
            p += 1
            continue
        if marker == 0xD9 or 0xD0 <= marker <= 0xD7 or marker == 0x01:
            out.append((p, p + 2))
            p += 2
            if marker == 0xD9:
                return
            continue
        if p + 4 > end:
            return
        length = _u16(data, p + 2, "big")
        body, tail = p + 4, p + 2 + length
        if length < 2 or tail > end:
            return
        out.append((p, body))
        if not (0xE0 <= marker <= 0xEF or marker == 0xFE):
            out.append((body, tail))
        p = tail
        if marker == 0xDA:
            # Entropy-coded data runs to the next marker: 0xFF 0x00 is a
            # stuffed 0xFF and 0xFF 0xD0-0xD7 a restart, both inside it.
            q = p
            while True:
                q = data.find(b"\xff", q, end - 1)
                if q < 0:
                    q = end
                    break
                if data[q + 1] == 0 or 0xD0 <= data[q + 1] <= 0xD7:
                    q += 2
                    continue
                break
            out.append((p, q))
            p = q


def _zip(data, start, end, out):
    """The fixed fields of the end record, central directory entries, local
    headers and data descriptors, and the data of each entry that is not
    stored (compressed or encrypted). A stored entry's data is classified in
    turn. Zip64 archives are not read: they are text."""
    eocd = data.rfind(b"PK\x05\x06", max(start, end - 22 - 0xFFFF), end)
    if eocd < 0 or eocd + 22 > end or eocd + 22 + _u16(data, eocd + 20) > end:
        return
    count, cd_size, cd_offset = _u16(data, eocd + 10), _u32(data, eocd + 12), _u32(data, eocd + 16)
    cd_start = start + cd_offset
    if count == 0xFFFF or cd_offset == 0xFFFFFFFF or cd_start + cd_size > eocd:
        return
    found = [(eocd, eocd + 22)]
    entries = []
    p = cd_start
    for _ in range(count):
        if p + 46 > eocd or data[p:p + 4] != b"PK\x01\x02":
            return
        flags, method, size, local = _u16(data, p + 8), _u16(data, p + 10), _u32(data, p + 20), _u32(data, p + 42)
        if size == 0xFFFFFFFF or local == 0xFFFFFFFF:
            return
        found.append((p, p + 46))
        entries.append((start + local, flags, method, size))
        p += 46 + _u16(data, p + 28) + _u16(data, p + 30) + _u16(data, p + 32)
    out += found
    previous = start
    for local, flags, method, size in sorted(entries):
        if local < previous or local + 30 > cd_start or data[local:local + 4] != b"PK\x03\x04":
            return
        body = local + 30 + _u16(data, local + 26) + _u16(data, local + 28)
        tail = body + size
        if tail > cd_start:
            return
        out.append((local, local + 30))
        if method == 0 and not flags & 1:
            _classify(data, body, tail, out)
        else:
            out.append((body, tail))
        previous = tail
        if flags & 8:
            # Data descriptor: an optional signature, CRC and two sizes.
            descriptor = 16 if data[tail:tail + 4] == b"PK\x07\x08" else 12
            out.append((tail, min(tail + descriptor, cd_start)))
            previous = tail + descriptor


ZSTD_MAGIC = 0xFD2FB528


def _zstd_literals(data, body, tail):
    """A compressed block's ranges: all of it, except raw literals, which are
    stored as they are."""
    head = data[body]
    form = head >> 2 & 3
    literals = body + (1, 2, 1, 3)[form]
    if head & 3 != 0 or literals > tail:
        return [(body, tail)]
    if form in (0, 2):
        size = head >> 3
    elif form == 1:
        size = (head >> 4) + (data[body + 1] << 4)
    else:
        size = (head >> 4) + (data[body + 1] << 4) + (data[body + 2] << 12)
    if literals + size > tail:
        return [(body, tail)]
    return [(body, literals), (literals + size, tail)]


def _zstd(data, start, end, out):
    """zstd frames: the frame header, block headers, compressed blocks
    (their raw literals are text), RLE bytes and checksums. Raw blocks and
    the user data of skippable frames are text."""
    p = start
    while p + 8 <= end:
        magic = _u32(data, p)
        if magic & 0xFFFFFFF0 == 0x184D2A50:
            size = _u32(data, p + 4)
            if p + 8 + size > end:
                return
            out.append((p, p + 8))
            p += 8 + size
            continue
        descriptor = data[p + 4]
        if magic != ZSTD_MAGIC or descriptor & 0x08:
            return
        single = descriptor >> 5 & 1
        q = p + 5 + (1 - single) + (0, 1, 2, 4)[descriptor & 3] + (single, 2, 4, 8)[descriptor >> 6]
        frame = [(p, q)]
        while True:
            if q + 3 > end:
                return
            header = int.from_bytes(data[q:q + 3], "little")
            last, kind, size = header & 1, header >> 1 & 3, header >> 3
            body = q + 3
            tail = body + (1 if kind == 1 else size)
            if kind == 3 or tail > end or (kind == 2 and size == 0):
                return
            frame.append((q, body))
            if kind == 1:
                frame.append((body, tail))
            elif kind == 2:
                frame += _zstd_literals(data, body, tail)
            q = tail
            if last:
                break
        if descriptor & 0x04:
            if q + 4 > end:
                return
            frame.append((q, q + 4))
            q += 4
        out += frame
        p = q


def _classify(data, start, end, out):
    """Adds the opaque ranges of data[start:end], by the format its first
    bytes name, to `out`."""
    head = data[start:start + 8]
    if head == PNG_SIGNATURE:
        _png(data, start, end, out)
    elif head[:3] == b"\xff\xd8\xff":
        _jpeg(data, start, end, out)
    elif head[:4] in (b"PK\x03\x04", b"PK\x05\x06"):
        _zip(data, start, end, out)
    elif len(head) >= 4 and (_u32(head, 0) == ZSTD_MAGIC or _u32(head, 0) & 0xFFFFFFF0 == 0x184D2A50):
        _zstd(data, start, end, out)


def opaque_ranges(data):
    """The (start, end) byte ranges of `data` that only long tokens can hit,
    sorted."""
    out = []
    _classify(data, 0, len(data), out)
    return sorted(r for r in out if r[0] < r[1])


def _runs_hit(matcher, data, start, end, run, min_bytes):
    """Whether a printable run of data[start:end] holds a token of
    `min_bytes` UTF-8 bytes or more."""
    for m in run.finditer(data, start, end):
        if m.end() - m.start() < min_bytes:
            continue
        text = m.group(0).decode("utf-8")
        for v in variants(text):
            for token in matcher.scan_tokens(v):
                if sum(len(w.encode("utf-8")) for w in token) >= min_bytes:
                    return True
    return False


def binary_hit(matcher, data):
    """Whether a binary file holds a token: in any printable run of its
    text, or as a long token in its opaque ranges (see PRINTABLE)."""
    pos = 0
    for start, end in opaque_ranges(data) + [(len(data), len(data))]:
        if start > pos and _runs_hit(matcher, data, pos, start, TEXT_RUN, 1):
            return True
        start = max(start, pos)
        if end > start and _runs_hit(matcher, data, start, end, OPAQUE_RUN, LONG_TOKEN):
            return True
        pos = max(pos, end)
    return False


def is_binary(data):
    return b"\0" in data[:SNIFF]


def git(repo, *args):
    run = subprocess.run(["git", "-C", repo] + list(args), stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    if run.returncode != 0:
        raise RuntimeError("git %s failed: %s" % (args[0], run.stderr.decode("utf-8", "replace").strip()))
    return run.stdout


# Committed files that never make a token public: results from the
# collection itself.
PRIVATE_RESULTS = "conformance/reports/private/"


def committed_texts(repo):
    """Yields the text of each regular file committed at HEAD (binary files and
    PRIVATE_RESULTS left out). Nothing when HEAD has no commit."""
    head = subprocess.run(["git", "-C", repo, "rev-parse", "-q", "--verify", "HEAD^{commit}"],
                          stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    if head.returncode != 0:
        return
    commit = head.stdout.decode("ascii").strip()
    blobs = []
    for record in git(repo, "ls-tree", "-r", "-z", "--full-tree", commit).split(b"\0"):
        if not record:
            continue
        meta, path = record.split(b"\t", 1)
        mode, kind, sha = meta.split(b" ")
        # Regular files only: not symlinks (120000) or submodules (commit).
        if kind != b"blob" or mode not in (b"100644", b"100755"):
            continue
        if os.fsdecode(path).startswith(PRIVATE_RESULTS):
            continue
        blobs.append(sha)
    if not blobs:
        return
    # One reader for every blob: "<sha> blob <size>", the bytes, a newline.
    proc = subprocess.Popen(["git", "-C", repo, "cat-file", "--batch"], stdin=subprocess.PIPE,
                            stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
    try:
        for sha in blobs:
            proc.stdin.write(sha + b"\n")
            proc.stdin.flush()
            header = proc.stdout.readline().split()
            if len(header) != 3 or header[1] != b"blob":
                raise RuntimeError("reading a committed file from git failed")
            data = proc.stdout.read(int(header[2]))
            proc.stdout.read(1)
            if not is_binary(data):
                yield data.decode("utf-8", errors="replace")
    finally:
        proc.stdin.close()
        proc.stdout.read()
        proc.wait()


# ------------------------------------------------------------------ build


def build(candidates, repo, out):
    tokens = set()
    by_kind = {}
    with open(candidates, encoding="utf-8") as f:
        for line in f:
            if not line.strip():
                continue
            record = json.loads(line)
            kind, text = record["kind"], record["text"]
            if kind not in ("deck", "tag", "media", "word"):
                raise ValueError("unknown candidate kind %r" % kind)
            found = tokens_of(kind, text)
            by_kind[kind] = by_kind.get(kind, 0) + len(found)
            tokens.update(found)
    matcher = Matcher(tokens)
    in_repo = set()
    for text in committed_texts(repo):
        in_repo.update(matcher.find(words(text)))
    final = sorted(" ".join(t) for t in tokens - in_repo)
    os.makedirs(os.path.dirname(os.path.abspath(out)), exist_ok=True)
    tmp = out + ".tmp"
    with open(tmp, "w", encoding="utf-8") as f:
        for token in final:
            f.write(token + "\n")
    os.replace(tmp, out)
    meta = {
        "tokens": len(final),
        "candidates": len(tokens),
        "in_repository": len(in_repo),
        "by_kind": by_kind,
    }
    meta_path = os.path.join(os.path.dirname(os.path.abspath(out)), "meta.json")
    with open(meta_path + ".tmp", "w", encoding="utf-8") as f:
        json.dump(meta, f, indent=2, sort_keys=True)
        f.write("\n")
    os.replace(meta_path + ".tmp", meta_path)
    return meta


def load_tokens(paths):
    tokens = set()
    for path in paths:
        with open(path, encoding="utf-8") as f:
            for line in f:
                line = line.rstrip("\n")
                if line:
                    tokens.add(tuple(line.split(" ")))
    return Matcher(tokens)


# ------------------------------------------------------------------- scan


# Byte-exact public notice, fetched from https://www.unicode.org/license.txt.
# No caller-controlled manifest or partial content can exempt added text.
PUBLIC_NOTICE_PATH = "specs/render/unicode-data-LICENSE.txt"
PUBLIC_NOTICE_SHA256 = "e7a93b009565cfce55919a381437ac4db883e9da2126fa28b91d12732bc53d96"
PROJECT_ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "../../.."))


def public_notice(path, data):
    return path == PUBLIC_NOTICE_PATH and hashlib.sha256(data).hexdigest() == PUBLIC_NOTICE_SHA256


def scan_paths(matcher, paths, repo=None):
    files = []
    for given in paths:
        if os.path.isdir(given):
            for top, dirs, names in os.walk(given):
                dirs.sort()
                files.extend(os.path.join(top, n) for n in sorted(names))
        else:
            files.append(given)
    found = []
    for path in files:
        if os.path.islink(path):
            continue
        with open(path, "rb") as f:
            data = f.read()
        relative = os.path.relpath(os.path.abspath(path), repo or PROJECT_ROOT)
        if public_notice(relative, data):
            continue
        if is_binary(data):
            if binary_hit(matcher, data):
                found.append({"path": path, "binary": True})
        else:
            hits = line_hits(matcher, data.decode("utf-8", errors="replace"))
            if hits:
                found.append({"path": path, "lines": hits})
    return found, len(files)


C_ESCAPES = {"a": 7, "b": 8, "t": 9, "n": 10, "v": 11, "f": 12, "r": 13, '"': 34, "\\": 92}


def _c_unquote(body):
    """Git's C-style quoting undone: backslash escapes and octal bytes."""
    out = bytearray()
    i = 0
    while i < len(body):
        c = body[i]
        if c == "\\" and i + 1 < len(body):
            nxt = body[i + 1]
            if nxt in C_ESCAPES:
                out.append(C_ESCAPES[nxt])
                i += 2
                continue
            if nxt in "0123" and re.match(r"[0-7]{3}", body[i + 1 : i + 4]):
                out.append(int(body[i + 1 : i + 4], 8))
                i += 4
                continue
        out.extend(c.encode("utf-8", "surrogateescape"))
        i += 1
    return out.decode("utf-8", "surrogateescape")


def _diff_path(raw):
    """The path of a `+++ b/<path>` line."""
    if raw.endswith("\t"):
        raw = raw[:-1]
    if raw.startswith('"') and raw.endswith('"'):
        raw = _c_unquote(raw[1:-1])
    return raw[2:] if raw.startswith("b/") else raw


HUNK = re.compile(r"^@@ -\d+(?:,\d+)? \+(\d+)(?:,\d+)? @@")


def scan_staged(matcher, repo):
    filters = ["--no-renames", "--diff-filter=ACMRT"]
    numstat = git(repo, "diff", "--cached", "--numstat", "-z", *filters)
    staged, binary = [], []
    for record in numstat.split(b"\0"):
        if not record:
            continue
        added, _deleted, path = record.split(b"\t", 2)
        staged.append(os.fsdecode(path))
        if added == b"-":
            binary.append(os.fsdecode(path))
    public = set()
    if PUBLIC_NOTICE_PATH in staged:
        data = git(repo, "cat-file", "blob", ":" + PUBLIC_NOTICE_PATH)
        if public_notice(PUBLIC_NOTICE_PATH, data):
            public.add(PUBLIC_NOTICE_PATH)
    found = {}
    if len(binary) < len(staged):
        diff = git(
            repo,
            "-c",
            "core.quotePath=false",
            "diff",
            "--cached",
            "-U0",
            "--no-color",
            "--no-ext-diff",
            "--src-prefix=a/",
            "--dst-prefix=b/",
            *filters,
        )
        # A file's header runs from `diff --git` to its first hunk; inside
        # hunks every line starts with `+`, `-` or `\`, so content that looks
        # like a header is never read as one.
        path, line_no, header = None, 0, False
        for raw in diff.split(b"\n"):
            line = raw.decode("utf-8", errors="replace")
            if line.startswith("diff --git "):
                path, header = None, True
            elif header and line.startswith("+++ "):
                path = _diff_path(raw[4:].decode("utf-8", "surrogateescape"))
            elif line.startswith("@@"):
                header = False
                m = HUNK.match(line)
                line_no = int(m.group(1)) if m else 0
            elif not header and line.startswith("+") and path is not None:
                if path not in public and any(matcher.hit(v) for v in variants(line[1:])):
                    found.setdefault(path, []).append(line_no)
                line_no += 1
    out = [{"path": p, "lines": found[p]} for p in sorted(found)]
    for path in binary:
        data = git(repo, "cat-file", "blob", ":" + path)
        if path not in public and binary_hit(matcher, data):
            out.append({"path": path, "binary": True})
    return out, len(staged)


def main(argv):
    parser = argparse.ArgumentParser(prog="private_scan.py")
    sub = parser.add_subparsers(dest="command", required=True)
    b = sub.add_parser("build")
    b.add_argument("--candidates", required=True)
    b.add_argument("--repo", required=True)
    b.add_argument("--out", required=True)
    s = sub.add_parser("scan")
    s.add_argument("--tokens", action="append", required=True)
    s.add_argument("--staged", action="store_true")
    s.add_argument("--repo")
    s.add_argument("paths", nargs="*")
    args = parser.parse_args(argv)
    try:
        if args.command == "build":
            print(json.dumps(build(args.candidates, args.repo, args.out), sort_keys=True))
            return 0
        if args.staged == bool(args.paths) or (args.staged and not args.repo):
            parser.error("give --staged with --repo, or paths")
        matcher = load_tokens(args.tokens)
        if args.staged:
            found, scanned = scan_staged(matcher, args.repo)
        else:
            found, scanned = scan_paths(matcher, args.paths, repo=args.repo)
        print(json.dumps({"files": found, "files_scanned": scanned, "tokens": matcher.count}, sort_keys=True))
        return 1 if found else 0
    except (OSError, ValueError, RuntimeError, KeyError) as e:
        # The message names a file or a git command, never a token.
        print("private_scan.py: %s" % e, file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
