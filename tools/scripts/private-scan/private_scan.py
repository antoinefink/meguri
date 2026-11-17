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
import contextlib
import io
import hashlib
import html
import json
import os
import re
import secrets
import stat
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


def committed_texts(repo, commit=None):
    """Yields the text of each regular file committed at HEAD (binary files and
    PRIVATE_RESULTS left out). Nothing when HEAD has no commit."""
    if commit is None:
        head = subprocess.run(["git", "-C", repo, "rev-parse", "-q", "--verify", "HEAD^{commit}"],
                              stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        if head.returncode != 0:
            return
        commit = head.stdout.decode("ascii").strip()
    elif re.fullmatch(r"(?:[0-9a-f]{40}|[0-9a-f]{64})", commit) is None:
        raise ValueError("committed text needs a full commit oid")
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


def public_tokens(matcher, repo, commit=None):
    found = set()
    for text in committed_texts(repo, commit):
        # Raw-only learning; masked fixed facts do not establish ordinary use.
        found.update(matcher.scan_tokens(text))
    return found


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
    in_repo = public_tokens(Matcher(tokens), repo)
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


def tokens_from_lines(lines):
    for line in lines:
        line = line.rstrip("\n")
        if line:
            yield tuple(line.split(" "))


def load_tokens(paths):
    tokens = set()
    for path in paths:
        with open(path, encoding="utf-8") as f:
            tokens.update(tokens_from_lines(f))
    return Matcher(tokens)


def refresh_public(repo, expected, shared_main=False):
    """Explicit idle-writer maintenance; only tokens.txt is authoritative."""
    def need(ok):
        if not ok:
            raise ValueError("unsafe or changed refresh input")

    def pin(info, directory=False):
        value = (info.st_dev, info.st_ino, info.st_mode, info.st_uid, info.st_gid)
        return value if directory else value + (info.st_nlink, info.st_size, info.st_mtime_ns, info.st_ctime_ns)

    need(os.path.isabs(repo) and repo == os.path.normpath(repo) == os.path.realpath(repo))
    need(re.fullmatch(r"(?:[0-9a-f]{40}|[0-9a-f]{64})", expected) is not None)
    with contextlib.ExitStack() as stack:
        bindings = []

        def directory(path):
            fd = os.open("/", os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
            stack.callback(os.close, fd)
            for name in path.split("/")[1:]:
                child = os.open(name, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=fd)
                stack.callback(os.close, child)
                info = os.fstat(child)
                need(info.st_uid in (0, os.getuid()) and
                     (not info.st_mode & 0o022 or (info.st_uid == 0 and info.st_mode & stat.S_ISVTX)))
                bindings.append((fd, name, child, pin(info, True)))
                fd = child
            return fd

        def check_directories():
            for parent, name, fd, before in bindings:
                need(pin(os.fstat(fd), True) == before ==
                     pin(os.stat(name, dir_fd=parent, follow_symlinks=False), True))

        root = directory(repo)
        marker = os.stat(".git", dir_fd=root, follow_symlinks=False)
        need(marker.st_uid == os.getuid() and not marker.st_mode & 0o022 and
             (stat.S_ISDIR(marker.st_mode) or (stat.S_ISREG(marker.st_mode) and marker.st_nlink == 1)))
        marker_pin = pin(marker, stat.S_ISDIR(marker.st_mode))
        common = os.fsdecode(git(repo, "rev-parse", "--path-format=absolute", "--git-common-dir")).rstrip("\n")
        need(common == os.path.normpath(common) == os.path.realpath(common) and
             os.path.isabs(common) and os.path.basename(common) == ".git")
        need(common == repo + "/.git" or shared_main)
        target = os.path.dirname(common) if shared_main else repo
        directory(common)  # The shared target must be this repository's real common root.
        private = directory(target + "/build/private")
        need(os.fstat(private).st_uid == os.getuid() and stat.S_IMODE(os.fstat(private).st_mode) == 0o700)
        parent = directory(target + "/build/private/tokens")
        need(os.fstat(parent).st_uid == os.getuid())

        def snapshot():
            check_directories()
            need(pin(os.stat(".git", dir_fd=root, follow_symlinks=False), stat.S_ISDIR(marker.st_mode)) == marker_pin)
            need(git(repo, "rev-parse", "--verify", "HEAD^{commit}").decode("ascii").strip() == expected)
            need(os.fsdecode(git(repo, "rev-parse", "--show-toplevel")).rstrip("\n") == repo)
            need(os.fsdecode(git(repo, "rev-parse", "--path-format=absolute", "--git-common-dir")).rstrip("\n") == common)

        snapshot()
        fd = os.open("tokens.txt", os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK, dir_fd=parent)
        stack.callback(os.close, fd)
        original = os.fstat(fd)
        need(stat.S_ISREG(original.st_mode) and original.st_uid == os.getuid() and
             original.st_nlink == 1 and not original.st_mode & 0o022 and original.st_size <= 64 * 1024 * 1024)
        with os.fdopen(os.dup(fd), "rb") as stream:
            before = stream.read(64 * 1024 * 1024 + 1)
        need(len(before) <= 64 * 1024 * 1024)

        def unchanged(name, opened, info):
            need(pin(os.fstat(opened)) == pin(info) ==
                 pin(os.stat(name, dir_fd=parent, follow_symlinks=False)))

        unchanged("tokens.txt", fd, original)
        tokens = set(tokens_from_lines(io.StringIO(before.decode("utf-8"), newline=None)))
        public = public_tokens(Matcher(tokens), repo, expected)
        after = "".join(" ".join(token) + "\n" for token in sorted(tokens - public)).encode("utf-8")
        result = {"head": expected, "before": len(tokens), "removed": len(public), "after": len(tokens - public),
                  "before_sha256": hashlib.sha256(before).hexdigest(),
                  "after_sha256": hashlib.sha256(after if public else before).hexdigest()}
        snapshot()
        unchanged("tokens.txt", fd, original)
        if not public:
            return result

        def saved(name, data):
            out = os.open(name, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600, dir_fd=parent)
            stack.callback(os.close, out)
            os.fchmod(out, 0o600)
            with os.fdopen(os.dup(out), "wb") as stream:
                stream.write(data)
                stream.flush()
                os.fsync(out)
            return out, os.fstat(out)

        nonce = secrets.token_hex(16)
        backup, temporary = "tokens.before-" + nonce, "tokens.refresh-" + nonce
        backup_fd, backup_info = saved(backup, before)
        temporary_fd, temporary_info = saved(temporary, after)
        os.fsync(parent)
        snapshot()
        unchanged("tokens.txt", fd, original)
        unchanged(backup, backup_fd, backup_info)
        unchanged(temporary, temporary_fd, temporary_info)
        os.replace(temporary, "tokens.txt", src_dir_fd=parent, dst_dir_fd=parent)
        os.fsync(parent)
        return result


# ------------------------------------------------------------------- scan


# Byte-exact public notice, fetched from https://www.unicode.org/license.txt.
# No caller-controlled manifest or partial content can exempt added text.
PUBLIC_NOTICE_PATH = "specs/render/unicode-data-LICENSE.txt"
PUBLIC_NOTICE_SHA256 = "e7a93b009565cfce55919a381437ac4db883e9da2126fa28b91d12732bc53d96"
PUBLIC_ARTIFACTS = {
    'docs/process/review-log/meguri-sync-U10.md':
        '565221177f72b2182784c8c442b1c07eda55c6d33186dfe1add3f36e0474cdf0',
    'server/Dockerfile':
        '324a94c05aafbbf38065174f70578eb044337ea6d0ded209977196b685a1ccd8',
    'server/README.md':
        '66e29ca5c1fb282f4cb5af33791e1129c8ca9c09a266eef24bbc9dd26220238c',
    'server/compose.yaml':
        '03737d36cd35810513c04e39a193b707e14228ca997e116ca93557c09e7a0d9b',
    'server/tools/package-context.py':
        '84db291e6ba4b27dbc5bfef38dc01434342cd4d06016e901ae544c212fd55724',
    'tools/mg.d/sync-package.sh':
        '0385c96b9fff90961214012cf2077653704a61752377e6aa746ddadc8011ca63',
    'tools/tests/sync_package.bats':
        'a8a9f046c1f5f7aa9fc9b55c04a43d60eff2d631136ab3f4133b1c8dbee22514',
    'tools/tests/sync_package_context_test.py':
        '8d8d511920aaecc53c7ac0801ebb013955fea587fef03c64dc4d0fad1ebda3f3',
    "core/crates/meguri-ffi/src/sync_tests.rs":
        "461fb731a22379b6bbe009cd968526c3e706f6e08ae769ff014bc82c63a85135",
    # Complete public-file review: incoming_contract_review, 2026-11-17.
    "docs/process/review-log/cloud-protocol-consent-fixtures-20261117.md":
        "8f37e85fa552b7eece8721914aaf2c6c48c7608308b7c3c242088599b816398a",
    "docs/process/verification-status.md":
        "9b135ccc396583635359a8afb9019871d22bde51e4bf8937452df9a4b2510b28",
    'app/Packages/MeguriKit/Sources/AppModel/Collection/SyncCoordinator.swift':
        'd17a17aafe357db445010b6d580ad11bac91f5d7e23496088de5f60f865b9130',
    'app/Packages/MeguriKit/Tests/AppModelTests/ProviderAdmissionTests.swift':
        '4cf802ffe04f6888219aa637119e12847be828e2590840fd42d35cea9bfa86d7',
    'core/crates/meguri-ffi/src/provider_tests.rs':
        '0a1a65dce24702eda82b38a3c229604bf613a28c69c134735b02d1097775757f',
    "docs/process/review-log/meguri-provider-admission-20261117.md":
        "31af001e272ebcc61ca299e3d932f6f29bc5da69551389305956ef92cd9f0e40",
    # Complete public-source review; future bytes remain checked.
    "core/crates/meguri-sync/src/provider.rs":
        "cf9a25cf5e229bbb62147c7304143d4e347b8a4fb6da7b5826de22a9b5be25bc",
    "core/crates/meguri-host/tests/cloud_publication_phase.rs":
        "9c5b08115ee7c9969554a0f6d68628cca262a7def0aee665866f0ebab78de622",
    "docs/process/review-log/cancellation-provider-restoration-20261117.md":
        "f73b989f4c5183f5ecf435af4aaa63105666afad147b7ac9cf4b044ef030a55c",
    'specs/sync/one-way.md':
        '8de7695fada9f38fa5d6c54c26538d025cd7e3b45b1867a1b8650417533b0812',
    'specs/sync/meguri-provider.md':
        'a3ba46013feac12fb76e211b7ff4660bf50fa13e8055ec71426e2d3d2fa0c899',
    'docs/process/review-log/meguri-provider-contract-20261116.md':
        '70b7eb39ea3f7ea6886d3d1f82717e81be96938d078147a01947f912794ef7f9',
    "app/MeguriSnapshotTests/NativeReplayAccessibilityTests.swift":
        "48452d45977876bb07b2a55deb8197ef2c0693ff35661bec2e301ca0bee607ac",
    # Independently reviewed public Docker lifecycle tooling, exact bytes.
    'tools/mg.d/conformance-admission.py':
        '7fe260aa7857773c6f472037929f9f6f3a5fe7f2d1eb2eda722238942f1bf7ae',
    'tools/mg.d/conformance-edge.py':
        'db717ee3b016fb7f351552cdf9bbff12eff88936d16aefb9b6aabbdd04fa6f8e',
    'specs/conformance/meguri-sync-candidate.md':
        '17992fe697babc4d609586223cb2ecf3d6b371a8864123dcdf453aa76e406d5f',
    'docs/process/review-log/meguri-sync-normal-candidate.md':
        'a071d74e332b444e750964a80c7651915d23700b7221f55b183a998001c16996',
    'docs/process/review-log/meguri-sync-U8.md':
        'a945434c2e135435fe00a1d11de32aaaf68013dde5fadd315ba6668e2c6c3e76',
    'docs/process/review-log/meguri-sync-U6.md':
        '14fef85aa75d15af8154c274278c56744ce5040669d638378442f5e8cf4511e0',
    'oracle/meguri-sync-server/provenance.md':
        'aa288eadbcb43c55193cccdf27a25fdd7833e6798d5ee09b54c4198f5b1b8980',
    'server/tests/session.rs':
        '9bcc7edb13789a4abfd059bb19e03ec04544abbe441b0e2129f5f45e111d5e5d',
    'server/src/collection/graves.rs':
        '891dda6db91028e90badfc7ee5591ff1f04c060e06de3585981b16e4d6dd57ba',
    'server/src/collection/changes.rs':
        '83f506e218e17d7c878c5381110c76277d3c64e767eb8a3da299d50c81a92b33',
    'tools/gate/tiers.yaml':
        'd63c27f6b8673ba9581c433695f3f35154d25a4f19e482a1aa846b7b22be9028',
    'tools/mg.d/sync-web.sh':
        '278ad9f6f50691f5e8447189d248404a016bf626fe4c51974ebd3b1e27f8a22b',
    'server/web/upstream-records.json':
        'e04eceba6bc281397bf6379e28e6232ac93143f1de536de20fe1c46c500e3037',
    'server/web/src/style.css':
        '73f23c7c83c63be975c6a5156e8adb5020a89cfcc4b9f8aaaaa0c40bfe0ec5e5',
    'server/web/src/app.tsx':
        '4e3224e691b9887c4e5aa8dee485118fb5308e20b15d15ccb0d885cb0915e8fd',
    'server/web/scripts/run.sh':
        'ffb7660f0da45f44125a20d1ce2888094e80810cade350714c537eb9f52d67d3',
    'server/web/package-lock.json':
        'f2f38f36f9f092dec3723f3425047d2f10f4fb32b7c411242c2b1b32c6a1a2f4',
    'server/web/notices/rolldown-THIRD-PARTY-LICENSE.txt':
        'a877291d800ed43692f3f9ae09d8e01cc6f7293ad39d43896059c188ffbb8b7c',
    'server/web/DEPENDENCIES.md':
        'eab8aa0fb4938f5e3881df295dc93f6a457b5bb4735efbbbdcd2a6983f32fbdf',
    'server/build.rs':
        'a4dba10f1d809a0c05836778c7c075b2e2243d7c4aeeff12ddc6f9eec3cfc6e4',
    'docker/sync-web.Dockerfile':
        'b77e146b870049a542f1abb6bef8aaf0ea2cb7a6dc3590f370d2171fb31b5b5f',
    # Independently reviewed public recovery facts and synthetic reference tools.
    'conformance/recordings/scenarios/meguri-sync-server/desktop-recovery-selection.json':
        '4de3f24f1ca4ff76a8d064d19c05629663ae7216bbf72a9e4fadb0be4ad1d12d',
    'docs/process/review-log/meguri-sync-recovery-reference.md':
        'c288e103d154610056247ad256c4c4756d6852eace398adfa233287a9cf8e949',
    'oracle/meguri-sync-server/recovery.py':
        '10c99555818fab5301df58a72013413a807bff8f8dcbe87e8f43a06e19ac6512',
    'oracle/meguri-sync-server/tests/test_recovery.py':
        '46e472a8234de8590af2b52c61ddb388c37f3abdf3f444bc9368bbeb3fd15e68',
    # Independently reviewed original media verification history, exact bytes.
    'docs/process/review-log/meguri-sync-U7.md':
        '8646e5dcff0d9dd1c23cd8566f08888099783ce5f13d02c1b995ab42ccbd43d7',
    # Independently reviewed evidence for the approved instruction change.
    'docs/process/review-log/meguri-sync-privacy-maintenance-20261115.md':
        'c3f782e9d61556d6e032f3b2093ef8573dc3e5ce4d0b12e632c209f607c430d7',
    # Owner-approved instruction proposal; independent full-content review.
    'AGENTS.md':
        '36089c33aa814ef45e2666bf3f9c41e96e8dc6fc1ea3c587f06fd16f81a6b18b',
    'docs/process/provenance.md':
        '2cfc1f7c8eb2399bf6bc2e8d77274682dd00563b2c467d15d17eb29956feee03',
    # Independently classified original public session metadata, exact bytes.
    'docs/process/provenance/sessions.jsonl':
        'db3d0429ab2026c1799c33bff95d98a7f950d432957fbea6db3a52125b6085ae',
    # Independently reviewed first-party license scope, exact complete text.
    'LICENSING.md':
        '98c119e6dedf0192268fc328b9a8e0be58771760a4bc2156387db57ae97c7f2a',
    'docs/process/review-log/mit-license-20261117.md':
        'e73a0d15e92e20ee01b4f3fcf8c5cbc7faf7a767143e466ea12b77ecbc3b2c29',
    'conformance/fixtures/meguri-sync-server/observations/desktop-26.09.4-normal-matrix.json':
        '8a4e17346939516eda8b5b650203444693fb32e2173f4d6741242b4089da04eb',
    'specs/sync/server-desktop-current.md':
        '423f2c24074f9c4edd2ab422ce03bf665afd2d5cc249f5a0f4abb3dc6a6e0be2',
    'oracle/meguri-sync-server/tests/test_normal_matrix.py':
        'ec667ccfd6cd9e7782a3c555e78cf8e31b4db721e0e7b4d5a9f230e3a3da3763',
    'oracle/meguri-sync-server/normal_matrix.py':
        '5f98f7d6db36d70d9a9f78fd224540fe58230e13eeea2f38b22f52bfba0e8644',
    'conformance/fixtures/meguri-sync-server/media.json':
        '1f87dc5efcf2521dec86a843bbe041f791f4de283234ac6933c4a4911ce8e97d',
    'conformance/recordings/scenarios/meguri-sync-server/desktop-media-selection.json':
        '42620ebc2d79e113d89b1e85d98ce598803618191ae8585ed1f0f6b4b8b6c0d9',
    'oracle/meguri-sync-server/media.py':
        'a75f35b5d4adda3f222ba61a56fa33ae6ef6000572a7c0b2e85413e85a3f4b62',
    'oracle/meguri-sync-server/tests/test_media.py':
        'c819a65c159e28a1fe70ae3436e94d909e9a008320210f476114bf8a9c83697d',
    'server/src/media/archive.rs':
        '33e053ff1dc4f8965f61212ae5d56d3e8c3337db173473be46ff8bad0da3bded',
    'server/src/media/recovery.rs':
        'c5388c9f59ef653beb7d7472f3195dc106bb732ecb797475cfc4671c0735eba5',
    'server/tests/media.rs':
        '1f397837922affc5e36fee7e450248c648ab3f2426cc81622829a52b884b4fe5',
    'server/tests/media_disk_full.rs':
        '98c3a131c1bb05110a2338ae9aee42fd37d8608e8f2ea854475a33ad1518ac57',
    PUBLIC_NOTICE_PATH: PUBLIC_NOTICE_SHA256,
    "specs/licenses/jquery-3.7.1-LICENSE.txt": "d4db9ebe6f29f5168eac45ad713f055623ac5d0dcd5ba92da23d650ae012020d",
    "app/Packages/MeguriKit/Sources/CardRendering/Resources/CardRuntime/vendor/jquery-3.7.1.min.js":
        "fc9a93dd241f6b045cbff0481cf4e1901becd0e12fb45166a8f17f95823f0b1a",
    # Owner approval 2026-11-15: these exact original review-note bytes.
    "docs/process/review-log/meguri-sync-plan-20261115.md":
        "3fefeb90235071d6e3d6ad364443276396cc01d9a9f4ccfa63bd6f80e99316b8",
    # Delegated maintenance authority 2026-11-15: agent-applied grants for
    # independently reviewed technical contracts, exact paths and full bytes.
    "docs/plans/2026-11-15-1028-feat-meguri-sync-server-plan.md":
        "80af6e19d1dad2fc8d100d502f53bece53be1ab52d5e49fe4a9f16131f51d067",
    "specs/sync/server-collection.md":
        "36f7c4adee4eb035c0342e715f0d052a77bd57137d33222e5ee7e84a8f0b9bfa",
    "specs/sync/server-lifecycle.md":
        "abacc93c438e5230f8cd8a60a671e8da7efd521465d0d98d189e95444ecd20f1",
    "specs/sync/server-media.md":
        "32f9eff487e305d1d17bad276c3797de4e7664dae22662e4b72a61bd2c3166cb",
    "specs/servers/meguri-sync-web.md":
        '416530b250c2dd1868f67fe30df9798dbaac586a1d5747baf603f87e00463d99',
    "specs/conformance/meguri-sync-server.md":
        '4c0e7651ab72236ad31320925e8c1c163c6dc7f1904e9c0c3e52df50e8f9e479',
    "conformance/schema/meguri-sync-server.schema.json":
        "cd905c134ed497f166566e18733465fa7e992156944957ee16c2e17ca586814a",
    # Same delegated maintenance: independently authored/reviewed tooling,
    # exact complete bytes only. These entries cover tooling, not production.
    "docs/process/review-log/meguri-sync-boundary-authoring-20261115.md":
        "c4aa47634166c890025e2ec1fd3a552069677f0d30fc554a98ae7bec82ce2c2a",
    "tools/cleanroom/source_view.py":
        "be74880b8aaa6768678ec1ad577424a05c8dcc07041a02a8e2f18cb5907530ad",
    "tools/cleanroom/test_source_view.py":
        "baed9e4626b60421f37783144317b3823f8d559b71694818c6bdd2f40f4a9508",
    "tools/gate/static-checks/browser-retries.py":
        "a69bee98c29564e9bdb30eb0f623f20db91795623c6eea1a982cc740d4b1565e",
    "tools/similarity/tests/test_scan_synthetic.py":
        "7a2488452f49b17772f611678883627bbe0c3b96e732de027bfb4f466fd35ca9",
    # Delegated maintenance: independently reviewed original reference tooling,
    # synthetic observations and evidence. Exact paths and complete bytes only.
    'conformance/catalog/meguri-sync-server.json':
        '4a1b6b865f8dce3c501c9ba14161e4559b0e6dea99c699a00835dbfc091e7898',
    'conformance/fixtures/meguri-sync-server/observations/desktop-26.09.4-foundation.json':
        'b302e7c77890d1c55dfdddf5e8630b79e85aa289b40a2fac496f299ae02850b9',
    'docker/meguri-sync-reference.compose.yml':
        'bb518192af60c60a54d4f62849d72b51bcea2bdb9b32dacd21726c3ac7966d9c',
    'docs/process/review-log/meguri-sync-U3.md':
        '50d4016dcbc654f7692f61a94849b6925de6dc73dac019e70c8923d27ff90923',
    'oracle/meguri-sync-server/AGENTS.md':
        'bdc60a465c52a548391469c7febb82de6c6a88c69f0a24329ae5bd31c260071f',
    'oracle/meguri-sync-server/adapter.py':
        '3f8810a24666cc30a994a88417f302d878105f6f301ba8e3caaeaa51263b2063',
    'oracle/meguri-sync-server/audit_deps.py':
        '09ba79e0545fb91e5a2c653e93c75e6fa4a55734d52e54a70479ae8e8d55d151',
    'oracle/meguri-sync-server/catalog.py':
        'da44186c8ee85f1ef77b3ff2584e0adce209cc4cfb9a5ee1eb557ad5d9a0068f',
    'oracle/meguri-sync-server/evidence.py':
        'c9f4c0c3f0da17fde02e38f15b9ac3433d73327d850f9431684d5a15eea72f38',
    'oracle/meguri-sync-server/tests/test_validation.py':
        'e7bdbef81fb4b34a1b569b6fa4c186801e1c32904b3813d9e8c011e608a90dfd',
    'oracle/meguri-sync-server/validation.py':
        '462a83ed4bb835e8f9b7446b214682531b5f29fcec1cc4476c26d18adda4af76',
    'tools/mg.d/meguri-sync-reference.sh':
        'd0c67f154ed4b9d61c5a28f6c24208a538535d186fbc4ef23d9c5f7289be9ac6',

    'docs/process/review-log/meguri-sync-reference-privacy.md':
        '4ef0a4f96b31e513f73415ac2160a9ce14e335fbd9cc841ddbe4ed2159e643a8',

    # Delegated maintenance, 2026-11-15: independently authored/reviewed
    # account-service code, tests and documentation. Includes production paths;
    # each entry admits only its exact path and complete reviewed bytes.
    'server/DEPENDENCIES.md':
        '33c9b563af531ec0b5cd6b02b0b43d3dec2c5db2aa025d7528a23a896ac79a23',
    'server/src/error.rs':
        '33e51ae13c05fb5124242b189fc02851103684fb3e03e104de63bd1d0a151261',
    'server/src/main.rs':
        '46699d99127a80945a264e9a461c17cdfcaf1f28cacf56ebf80ed9cd8d9882bf',
    'server/src/accounts/mod.rs':
        '21134b14725958e298960a4d10423348a9db1ec5d6b52cfec270dba763c9d606',
    'server/src/accounts/provision.rs':
        'f75596225c443a3f33a58c5e1322ceba52326c4a7b507723c30b4f2aa6fc0416',
    'server/src/httpapi/AGENTS.md':
        'ee08f06fbb1c5c420b5dbf55137846012523c0ec5aa37d6f73435f07bb722785',
    'server/src/httpapi/ingress.rs':
        'aaee8e568d0a2c915288942b847f45ec262874d13887ce2911bbe0dac4f39ef6',
    'server/src/httpapi/mod.rs':
        '9bf76b2caf14890359cd8e0278ba632bb7528afd76905e6ed2043c27adc5f024',
    'server/src/httpapi/tests.rs':
        '370858800185720681b7f457ceded7bd414d1ae1d2defd513decb72ece76528d',
    'server/src/storage/schema.sql':
        '4842a61b5bf0e3b8e909364d5df7667aea59f054c7175313dd28f840f6c7a31b',
    'server/src/storage/semantic.rs':
        '3da133a09e7d4e3f225a696fad20652c465309522f97825f78a8f022e2f18945',
    'server/src/storage/tests.rs':
        '2fca66030368d01fbdf61c1bf5596834c2a0fb570c9df1b18855055c7ae54084',
    'server/tests/accounts.rs':
        '7e0fa1344776c603a782ff3327e7aa47791357d21e049b9d26cb05cdb4adb2d5',
    'server/tests/http_contract.rs':
        'b3af9669195af18792f1c9b44e8de582f7fb12ac658430f7bdf3ba469d8fcb7a',
    'server/tests/provision.rs':
        '73e9e3159679125a5aea2d5fd845326fb7d36cd7b328f84a04b4eef30c02aa9f',
    'server/tests/recovery.rs':
        '1d31f70b941ead6f7a986a464fd3de874166308c5c3119e9793dad442f515521',

    'docs/process/review-log/meguri-sync-U4.md':
        '3dba5263b27e28389786dc37ee488630527cd3ba62a98ba350b357aac309aee8',
    # Independently reviewed public foundation receipt; delegated maintenance.
    'docs/process/review-log/meguri-sync-foundation.md':
        '1fbf5605fd269f6ee618599e607cec157dbb499b38751964eb22ba0d3fa29fc4',

    # Independently reviewed public transfer code and process evidence.
    'server/src/collection/schema.rs':
        'e17519c1a370abb797bba2f0beebeb6813f80d954fc2e7a952a626c74d887fea',
    'server/src/protocol/framing.rs':
        'a1349d00d0b25dd659e0e8fff5d3adc6b7ce63d20f861e17604e60071d79b777',
    'server/src/protocol/full.rs':
        '1e00dd45e0975e6fcd94c9a05b217b236a202c64573f2585a8a8096f751bd640',
    'server/src/protocol/mod.rs':
        '32c7e634ac02543ee958fa7df92aad82c2d7600c29271ed0d97a91423ac35d84',
    'server/src/storage/root.rs':
        'bae80427f23487936d206ea04d9f9ae81db6b920939b752d7a29f2517ac0b9cc',
    'server/tools/candidate-output.py':
        '4f0e092bdb45811046defae71b0c822d97b1994311e9cda2f3489a7c1b355763',
    'docs/process/review-log/meguri-sync-U5.md':
        '6cc7dc20556c191e0644282d73259eb8ff8e23f7f98dbd76f107ae80b7eb9494',

    'tools/mg.d/meguri-sync.sh':
        '56e47f8fcebc5f8eaebb561f21a529ab04d63b83743272f22b5eb1593e1d2526',

    'tools/tests/sync_server.bats':
        '6c0ad95ca4dafd841dc7dd5bc239dd810b11758a413f0b5219de28e89abda470',

    'oracle/meguri-sync-server/normal.py':
        'cb9e7dd7bd2d4ee03f98cc38ea31b7d8881f77e80074b57dc8a87caba1018196',

    'oracle/meguri-sync-server/tests/test_normal.py':
        '2ad3925d47bd218c8914fcd6bcee2ffdd06a49baf0756f709975f4a8b3d987fe',

    'conformance/fixtures/meguri-sync-server/observations/desktop-26.09.4-normal.json':
        '33e4f576a85f19bf877baefc382e91121e4e0307fe73b4b06fc8e24807e4d4a7',

    # Independently reviewed public recovery fact snapshots, exact bytes.
    'docs/process/review-log/meguri-sync-recovery-dirty-reference.md':
        'd3e7180cff9d63d1f16b5eef4fe94cb5e115d05e4b58f5baf288299a47a48d31',
    'oracle/meguri-sync-server/README.md':
        '1bfa096ec19e3b3e4c619b46baa145a9d27e329d02f2e957ca333f1c313d83d0',
    'oracle/meguri-sync-server/tests/test_recovery_dirty.py':
        'd972f70a91c8c85032dfabcbc4d9ffb38564a5654a158610bc0c6dbe4d38d6e7',
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
    'tools/scripts/private-scan/private_scan.py': {
        '810bcc7c5a23cc66d257b5b5b5431ae419646e4662391a7216da6552dc2468d8',
    },
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
    r = sub.add_parser("refresh-public")
    r.add_argument("--repo", required=True)
    r.add_argument("--head", required=True)
    r.add_argument("--shared-main", action="store_true")
    s = sub.add_parser("scan")
    s.add_argument("--tokens", action="append", required=True)
    s.add_argument("--staged", action="store_true")
    s.add_argument("--repo")
    s.add_argument("paths", nargs="*")
    args = parser.parse_args(argv)
    try:
        if args.command == "refresh-public":
            print(json.dumps(refresh_public(args.repo, args.head, args.shared_main), sort_keys=True))
            return 0
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
        message = "refresh refused or interrupted; owner must inspect private backup before retrying" if args.command == "refresh-public" else str(e)
        print("private_scan.py: %s" % message, file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
