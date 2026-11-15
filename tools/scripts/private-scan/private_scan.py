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
# The DOM EventInit dictionary's public flag is also required by synthetic
# event fixtures. Its exact name is specified by the public DOM interface.
# Observer methods: https://www.w3.org/TR/resize-observer/#resize-observer-interface
# and https://dom.spec.whatwg.org/#interface-mutationobserver. Only these
# complete qualified expressions qualify, never a standalone method name.
PUBLIC_IDENTIFIERS = ("ResizeObserver.prototype.disconnect", "MutationObserver.prototype.disconnect", "--canvas-elevated", "canvas-elevated", "dart-sass/sass", "dart-sass", "sass:color", "sass", "bubbles")
PUBLIC_COMPILER_URL = "https://github.com/sass/dart-sass/releases/download/1.102.0/dart-sass-1.102.0-linux-arm64.tar.gz"
# Fixed Xcode 27 public interface call. The receiver remains outside its span.
PUBLIC_GESTURE_CALL = ".pinch(withScale: 2, velocity: 1)"
# E-20261102T070407Z-6a04: only this case-sensitive public Swift import.
# SHA256 identifies its exact UTF-8 bytes, without quotes or a line ending.
PUBLIC_SWIFT_IMPORT = "import MachO"
PUBLIC_SWIFT_IMPORT_SHA256 = "0cd4593e5b7c20a193357a8d46e91ae8d04f5d92d0ecf4aa3167ac4dc0379c5a"
# Owner approval: docs/process/review-log/recovery-rule-publication-approval-20261114.md.
# Only this complete case-sensitive public span; SHA256 excludes a line ending.
PUBLIC_RECOVERY_RULE = "1. This exception takes effect only within the final transaction of the matching successful recovery operation: adoption at V14.2 step 9, or publication at V15.2 step 4. It permits removal without settlement solely for the covered blocked ordinary work."
PUBLIC_RECOVERY_RULE_SHA256 = "1223e97157bb7d43dc1ce067b92443dd37014a8424b1c7af45c6e3851d5cca06"
# Docker's security option, approved in E-20261024T181932Z-5d69. Quotes are
# required; bare words and matches crossing the literal remain private.
# https://docs.docker.com/reference/cli/docker/container/run/#optional-security-options---security-opt
PUBLIC_DOCKER_OPTION = "no-new-privileges"
# Fixed public Rust API spans, longest first; only complete paths qualify.
# Media I/O additions: approved in E-20261024T045314Z-f655.
PUBLIC_RUST_PATHS = (
    "tokio::sync::Semaphore::const_new",
    "tempfile::NamedTempFile::new_in",
    "tempfile::NamedTempFile::new",
    "tokio::task::spawn_blocking",
    "io::ErrorKind::Interrupted",
    "io::ErrorKind::StorageFull",
    "SimpleFileOptions::default",
    "std::future::Future::poll",
    "PoisonError::into_inner",
    "tempfile::NamedTempFile",
    "tokio::sync::Semaphore",
    "tempfile::Builder::new",
    "std::future::poll_fn",
    "io::ErrorKind::Other",
    "futures_core::Stream",
    "Semaphore::const_new",
    "tokio::sync::Notify",
    "Vec::with_capacity",
    "std::fs::read_dir",
    "Ordering::Acquire",
    "Ordering::Release",
    "tempfile::tempdir",
    "std::cell::Cell",
    "std::sync::mpsc",
    "AtomicBool::new",
    "io::Error::from",
    "io::Error::new",
    "Sha256::digest",
    "tokio::select!",
    "ZipWriter::new",
    "std::fs::File",
    "std::pin::Pin",
    "Poll::Pending",
    "mpsc::channel",
    "bytes::Bytes",
    "Poll::Ready",
    "Bytes::from",
    "Sha256::new",
    "tokio::pin!",
    "Arc::clone",
    "io::Result",
    "Cell::new",
    "Arc::new",
    "Box::pin",
    "io::sink",
)
PUBLIC_LITERAL = re.compile(
    r"(?<![^\s\"'`<>()\[\]{},;:=|])(?:" + "|".join(map(re.escape, PUBLIC_IDENTIFIERS)) +
    r")(?=$|[\s\"'`<>()\[\]{},;:=|])"
    r"|(?<![^\s\"'`<>()\[\]{},;=|])" + re.escape(PUBLIC_COMPILER_URL) +
    r"(?=$|[\s\"'`<>()\[\]{},;|])"
    r"|(?<=\.)" + re.escape(PUBLIC_GESTURE_CALL[1:]) + r"(?=$|[\s;\"'])"
    r'''|(?P<docker_quote>["'])''' + re.escape(PUBLIC_DOCKER_OPTION) + r"(?P=docker_quote)"
    r"|(?<![^\s\"'`<>()\[\]{},;=|&])(?:" + "|".join(map(re.escape, PUBLIC_RUST_PATHS)) +
    r")(?=$|[\s\"'`<>()\[\]{},;=|&])"
    r'''|(?<![^\s<>()\[\]{},;=|])(?P<swift_quote>["'`]?)'''
    r"(?P<swift_import>" + re.escape(PUBLIC_SWIFT_IMPORT) + r")(?P=swift_quote)"
    r"(?=$|[\s<>()\[\]{},;=|])"
    r"|" + re.escape(PUBLIC_RECOVERY_RULE)
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


def rust_path_context(text, start, end, context):
    """Refuse extra qualification or ambiguous adjacent Rust comments.

    Do not parse comments: even a nested block ends in */, and a preceding
    line containing // may continue qualification across a line break.
    Context supplies the surrounding file when scanning one line or run.
    """
    befores, afters = [text[:start]], [text[end:]]
    if context is not None:
        source, first, stop = context
        prefix, suffix = source[:first], source[stop:]
        if isinstance(source, bytes):
            prefix = prefix.decode("utf-8", errors="replace")
            suffix = suffix.decode("utf-8", errors="replace")
        befores = [v + text[:start] for v in variants(prefix)]
        afters = [text[end:] + v for v in variants(suffix)]
    return (any(before.rstrip().endswith(("::", "*/")) or
                "//" in before.rstrip().rsplit("\n", 1)[-1] for before in befores) or
            any(after.lstrip().startswith(("::", "/*", "//")) for after in afters))


def swift_import_context(text, start, end, context):
    """Check nearby file text for import continuations and comments."""
    befores, afters = [text[:start]], [text[end:]]
    if context is not None:
        source, first, stop = context
        prefix, suffix = source[:first], source[stop:]
        if isinstance(source, bytes):
            prefix = prefix.decode("utf-8", errors="replace")
            suffix = suffix.decode("utf-8", errors="replace")
        befores = [v + text[:start] for v in variants(prefix)]
        afters = [text[end:] + v for v in variants(suffix)]
    return (any(before.rstrip().endswith((".", ":", "_", "-", "*/")) or
                "//" in before.rstrip().rsplit("\n", 1)[-1] for before in befores) or
            any(after.lstrip().startswith((".", ":", "_", "-", "/*", "//")) for after in afters))


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

    def scan_tokens(self, text, rust_context=None):
        """Retain every match except those fully inside fixed public facts."""
        text = unicodedata.normalize("NFC", text)
        spans = []
        for m in PUBLIC_LITERAL.finditer(text):
            start, end = m.span()
            if m.group("swift_import") is not None:
                if swift_import_context(text, start, end, rust_context):
                    continue
                start, end = m.span("swift_import")
            elif m.group() in PUBLIC_RUST_PATHS and rust_path_context(text, start, end, rust_context):
                continue
            spans.append((len(text[:start].casefold()), len(text[:end].casefold())))
        if not spans:
            yield from self.find(words(text))
            return
        matches = list(WORD.finditer(text.casefold()))
        ws = [m.group() for m in matches]
        for token, first, end in self.occurrences(ws):
            start_char, end_char = matches[first].start(), matches[end - 1].end()
            if not any(start <= start_char and end_char <= stop for start, stop in spans):
                yield token

    def hit(self, text, rust_context=None):
        for _ in self.scan_tokens(text, rust_context):
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
    offset = 0
    for n, line in enumerate(text.splitlines(keepends=True), first):
        stop = offset + len(line)
        if any(matcher.hit(v, (text, offset, stop)) for v in variants(line)):
            hits.append(n)
        offset = stop
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
            for token in matcher.scan_tokens(v, (data, m.start(), m.end())):
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
        # Keep committed_texts' raw-only learning: fixed facts are not evidence
        # that a candidate's ordinary spelling outside their spans is public.
        in_repo.update(matcher.scan_tokens(text))
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
PUBLIC_ARTIFACTS = {
    PUBLIC_NOTICE_PATH: PUBLIC_NOTICE_SHA256,
    "specs/licenses/jquery-3.7.1-LICENSE.txt": "d4db9ebe6f29f5168eac45ad713f055623ac5d0dcd5ba92da23d650ae012020d",
    "app/Packages/MeguriKit/Sources/CardRendering/Resources/CardRuntime/vendor/jquery-3.7.1.min.js":
        "fc9a93dd241f6b045cbff0481cf4e1901becd0e12fb45166a8f17f95823f0b1a",
    # Owner approval 2026-11-15: only these exact documents and complete bytes.
    "docs/plans/2026-11-15-1028-feat-meguri-sync-server-plan.md":
        "ee057f23871d457641fcd78d5905dcb33ca23d2aaca543f806ed7f4aaffb5ab8",
    "docs/process/review-log/meguri-sync-plan-20261115.md":
        "3fefeb90235071d6e3d6ad364443276396cc01d9a9f4ccfa63bd6f80e99316b8",
}


# Published Askama derive 0.16.1 files verified against archive SHA256
# 071ee5ebf2138e3ad180e0aacf6940c2cab5e6d8333741d9925c7bee2b153f39.
# Keys pin exact repository-path UTF-8 bytes; values pin complete file bytes.
PUBLIC_VENDOR_ARTIFACTS = {
    '987ec1929a5a69b01078144679a1a93b0ae01cb133c1a3eb19b18c2094e4e400': '3cf5ea37ea7bad31fe17f2650ea6ad0a7a570724a2435458d1ed1efb2dcf4715',
    '35a74f0278dd035158c7c188707978180be6dac754422f7bc506b0a6b16f626e': 'a3754a3582a6ab4247c2ace9f9686f8757f94691b01e86f340234c4c18073a1b',
    '03e15d0ae33aa3bd1b02ca67981f67a15ee7aa7999f0628ebad8623a78f4c4c3': 'd0158de75a7ce3ddd72c04e974befc12dafc396a66eb4d77278bc522319416a7',
    '24b14302683cc62b6927130bdd2453e5c2bafb292543a87aabf67a92e593147d': '87cb0d734c723c083e51c825930ff42bce28596b52dee15567f6b28f19c195e3',
    '1aae91a6c2e145efa3fe99f5af4936606a2834a746bbdb91d95cf4cbe8e55adc': 'df20e0180764bf5bd76f74d47bc9e8c0069a666401629c390003a1d5eba99c92',
    '91dfe793456bcef3b558dc77e4d4499346b90db193c8e68d3591eabbdf0eee78': 'd14d50477d6ebf3a404203e2afcbe130f980b3c777e5c5d607e1a061ae3cdac7',
    'c9f1ed162ffba8af33d7b34d2c98acee092d160604691211e8a16631267891a3': '3c73a10e0f3a854d78b80e5a38bde1f052b80640ed2f47cc8d2a353d268165c2',
    'ee55cda27f30fede9d27d9d8d66ac36e1ac9a926526549ef63b834d21cdd5086': '57b54eb53aad4d1cf2ed4bf11f2ad5aaad12ab582545308e0a3cdff950187770',
    'a40864ae7ed6809299d0fdf094145dc7831deaf3d9cf07b8dc3acbe85f9145e4': '6c3c312bad0fbb5ec5fa8f46535ae4c73bb4fcbe559ab050382a72cb690402fe',
    'ff12e40018abed3255321d43f7cf800f9facc982cc413e6503b23972dab471cb': 'd11998a49687dcc509cdd3c37063f74dc29d006ac05adc0b1e9a20d0a2f4904b',
    '6769be529e7e1fae5a69c1f51e88ac1cacd5a26277e7317aa93345d2832641fa': 'b7637b42164b1bbba883c422187292cf08ad624691145b5124eaf10edf38c164',
    '56709f2ec75043028a82d27b547510e26c0269642730285cd4cf521e0c5092e1': '272ed728e3e024cefbf16b9a004ad806781cff95f613d34c35a4e8479224d16f',
    'dfe6eafd5206fb727ec554b773914eda98aabf129f9db5b9907e7d7ca41136b2': 'deaa7f2f768c036690989ae925528cc28482258e5cec7e8f44312dcb0b74fc63',
    '39e2065d9831cb3e4a0abf6aaecf3b33078e25f8c2cb0d3b0c72d3fd0190e8f2': '2813180f836edfe2230d164f0dd848244e76a9f887a46a488490467abe6c41a5',
    '3c502b291dae8821f94a64bd15fb4132a584054cdfa06541812bf06147bdbff2': '189400d3bd12a9156fcd5fa052e29bd4619ae5beb9153b0f1c92164eb41d2ca9',
    'd28d94ef8b318cef65cfb27e385e297ccdd702f0e7197b4e8d37f6daee19a908': 'd958e621560fc395323e7a97f0eaf36352d07be3fbcb06fc604bc89f996a288e',
    'c3eb5a7b15c8f5112f4b0a8515d4919c5ed7dc0f8ec36ed8c209f98d08941a0e': 'df47707cab5edab7d6aa1320c4f0ec9420ccf848c02eb0f846fcb27c7e761d2d',
    '1e7c834aacd5a908f35c1c333cf9a713423f9c8e93bd64fb01ec8ce2f1634f7f': 'b4de6e933964f65b3cbfdef2468ed2635df210301282c66cc95f4077ccd9b160',
    '800adf03a0416509de4238578e3249aa1d449575e51d6b2a88aac25a47597c0a': 'ce3ae51cf9797a5893d66b0b24a4a53d640a94d515f0567956907b8cd4078b5f',
    '0418bf3d08afa3ab61b08a4724b1b79706ed79cc57d97489218874d472882d9b': '0177b4911ff43653e087d4d686be5dc979d25eaa9244af2b9acb9ec3b4e86852',
    '96a7bb34eaa83758bf6bd6d631ff7c92723c94cba4926581cf1af31e4cfe1357': 'c3ec5db91375838e0bcce543e04c6f5b2f6c72d5c25ac4d0b90857e99c03cb26',
    '92cc0fbb136f107c49ccf455bd8d2cc1929e584e62755cc68dda43fd9a67412b': '91ee580663d8ad08460373d9e5c8ed196ab0ce61eb113dc8d43ad19ca6671422',
    '9dbea996321c954e41c1dc9d17d1a0763e102b6e4779fc6436deadc0adc8925e': '40126658a90b4041946f2caecb293797f7743c1148d481111859dfa1e3559b56',
    'e4536042b5b0dc321953b11f8e8370c541c878dc1f425f7e4624e7f99b55c9b7': 'e53715e2ebce35103eb68b341e1f7471a88366168229ea2ec72ec4a0822323ce',
    'ec99dc069fddb5f4b6152fae52f646514e01d8aa5231e963302e67b5ef54f649': 'a56a7829804ab90f08bc16fb1dd956ae9f43d69712466fd7aad41a195634137e',
    '2b8ecaaebb49a6a3e2b08ced66d4fbc95f023e37582beb1dc2436fcbb0e447cd': 'fa5fe2e62b061134ea261e151dd672281007a1ca45dbca04a68581d903f3a569',
    'f3f959f049d6942def4cdda95f2cbf35947dcbd3a0d43d2449dbd010d2773cda': 'bfc9025ab9ae61820f34ee0e595e1a46744fce436e417a3260386cd5292dd913',
    '011b33a5342c1d1972a5b12b7bc076791195d5d32297a4b830e70c79179044d2': '5c00bb481fd48e50a8209c66b0a40bbafbdf7b2f31f65b586edb259d21542b08',
    '7f56faf79b60d1ef6934a632a03b52c316c6287335933842ce3c8ec8a21083a1': 'b5bb9d8014a0f9b1d61e21e796d78dccdf1352f23cd32812f4850b878ae4944c',
    '7de5f4c8a58e1ec1e10acaecc0be37bd0955eecc37d6fa8fb509b5e058165c57': '7d865e959b2466918c9863afca942d0fb89d7c9ac0c99bafc3749504ded97730',
    '07681cf880a440e24c49d3cd546d1c69d4a69583550558de4cd2edcbfcd7f4ce': '5a4304e5548d5cf618779383dcc27eba93d826feff1c41a348101c734c034ea2',
    '437ef25afce57dcd075a9ebf739fbc76e744fbb5bc0414f2894fa201bd3e61d1': 'cd319fee61d9d0e8a00a40d55e91c338fd9e156e69773f03ba5c31db02892eb2',
    '34f8634d8ce4be4e06eb2910d3ceb43d781b871af62a80f76e5783d46d9c9b14': '1121cfccd5913f0a63fec40a6ffd44ea64f9dc135c66634ba001d10bcf4302a2',
    'c6cfb27a0c7c699402e185743e34307c9622ef0eb0b207f195bef2ef111911e9': '7d865e959b2466918c9863afca942d0fb89d7c9ac0c99bafc3749504ded97730',
    '995b4e527a8a749be95ad168a1cab7282db79e262d0f61ea454db03957e0e473': 'bf07a7fbb825fc0aae7bf4a1177b2b31fcf8a3feeaf7092761e18c859ee52a9c',
    'ac70301a7f49a5bdff0850ff180d5f63ca9c67e4b5f994d6a5396278d242311d': '86b0c5a1e2b73b08fd54c727f4458649ed9fe3ad1b6e8ac9460c070113509a1e',
    '4571ef99325e2dc800efe86a0c0aed7bf761cdea55e9a0fe083b5556d30edba0': 'a33c21547346656aead5f46f2f07e1965f4cf0c6f2a16deb8ec06019619267aa',
}

# Two unchanged upstream lines in the locally patched manifest/config. Hash
# complete raw UTF-8 bytes INCLUDING LF; CR, whitespace and additions differ.
# Only exact paths qualify. Meguri changes and surrounding lines stay scanned.
PUBLIC_SOURCE_LINES = {
    'core/vendor/askama_derive/Cargo.toml': {'a97a68a2ebcede836802fcf90cf07a563d1d43027f264cdd12ad820dd826d681'},
    'core/vendor/askama_derive/src/config.rs': {'e8aacfc627844473e382f3b486bd3b18803abc3ca21dbbdbf1c1aeb45e7ad8f9'},
    # Public PyPI requirement lines (two web-server dependencies of aqt), resolved by
    # uv pip compile as oracle/ankiconnect/README.md gives it; approved by Antoine 2026-11-10.
    'oracle/ankiconnect/requirements.lock': {
        'bded52ab3d5848e756965150abffebe4d577025193a5c714a339048862b12a27',
        'e8395f4881a04b8053c459130571cd8f05c1abe1477f65c2e5f2299cb77eca12',
    },
}
PROJECT_ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "../../.."))


def public_artifact_digest(path):
    if path in PUBLIC_ARTIFACTS:
        return PUBLIC_ARTIFACTS[path]
    try:
        encoded = path.encode("utf-8")
    except UnicodeEncodeError:
        return None
    return PUBLIC_VENDOR_ARTIFACTS.get(hashlib.sha256(encoded).hexdigest())


def public_artifact(path, data):
    expected = public_artifact_digest(path)
    return expected is not None and hashlib.sha256(data).hexdigest() == expected


def source_lines(data):
    """Raw LF-delimited lines, retaining terminators and all other bytes."""
    lines = data.split(b"\n")
    for line in lines[:-1]:
        yield line + b"\n"
    if lines[-1]:
        yield lines[-1]


def public_source_line(path, raw):
    return hashlib.sha256(raw).hexdigest() in PUBLIC_SOURCE_LINES.get(path, ())


def source_line_hits(matcher, path, data):
    if path not in PUBLIC_SOURCE_LINES:
        return line_hits(matcher, data.decode("utf-8", errors="replace"))
    hits, number = [], 1
    for raw in source_lines(data):
        text = raw.decode("utf-8", errors="replace")
        if not public_source_line(path, raw):
            hits.extend(line_hits(matcher, text, number))
        number += len(text.splitlines())
    return hits


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
        if public_artifact(relative, data):
            continue
        if is_binary(data):
            if binary_hit(matcher, data):
                found.append({"path": path, "binary": True})
        else:
            hits = source_line_hits(matcher, relative, data)
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
    for path in (path for path in staged if public_artifact_digest(path) is not None):
        data = git(repo, "cat-file", "blob", ":" + path)
        if public_artifact(path, data):
            public.add(path)
    public_lines = {}
    for path in set(PUBLIC_SOURCE_LINES).intersection(staged):
        data = git(repo, "cat-file", "blob", ":" + path)
        public_lines[path] = {n: raw for n, raw in enumerate(source_lines(data), 1)
                              if public_source_line(path, raw)}
    found, index_context = {}, {}
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
                exact_public_line = public_lines.get(path, {}).get(line_no) == raw[1:] + b"\n"
                decoded = variants(line[1:])
                context = None
                if any(fact in v for fact in PUBLIC_RUST_PATHS + (PUBLIC_SWIFT_IMPORT,) for v in decoded):
                    if path not in index_context:
                        source = git(repo, "cat-file", "blob", ":" + path).decode("utf-8", errors="replace")
                        offsets = [0] + [m.end() for m in re.finditer("\n", source)]
                        if offsets[-1] != len(source):
                            offsets.append(len(source))
                        index_context[path] = source, offsets
                    source, offsets = index_context[path]
                    context = (source, offsets[line_no - 1], offsets[line_no])
                if path not in public and not exact_public_line and any(matcher.hit(v, context) for v in decoded):
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
