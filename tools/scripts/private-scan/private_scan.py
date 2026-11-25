#!/usr/bin/env python3
"""The output guard's token set and scanner (U15, R1940).

    private_scan.py build --candidates <jsonl> --repo <dir> --out <tokens.txt>
    private_scan.py scan --tokens <tokens.txt> [--tokens ...] (--staged --repo <dir> | <path>...)
    private_scan.py pins --repo <dir> [--commit <full-oid>]

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
token. A file listed with `stale_public_artifact` has a whole-file digest in
PUBLIC_ARTIFACTS that its bytes no longer match: it was admitted by a public
review and edited since, so its reviewed lines are scanned again.

`pins` reads no token set: it lists the PUBLIC_ARTIFACTS paths whose
committed bytes (HEAD, or `--commit`) differ from the pinned digest, and
those the commit does not hold.

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
    'app/Packages/MeguriKit/Sources/CardRendering/Audio/AudioSessionCoordinator.swift':
        '2609f9fde767c011219bba509519fd667f167a6f3937ce865095b64795acd2a3',
    'app/Packages/MeguriKit/Tests/CardRenderingTests/AudioContinuityTests.swift':
        '836c8128dd2bd184b1a41329cf2920e59ef5cf000736d2ed561fbd3fd84bc1b7',
    'app/Packages/MeguriKit/Tests/CardRenderingTests/CardAudioControllerTests.swift':
        '5c88bf3b94b11c8a7dcea45903bb551d5db4826f9f6f8e59c5fbe6f00a8edcd9',
    'specs/render/audio-playback.md':
        '11eda11c77a802d0951393f3a2c5b3f96a5fd1187284a2157c8b54539f579311',
    'docs/process/review-log/meguri-sync-collection-backups-20261125.md':
        '7c0b0a10b44ae639c53a312462f26a50cd71368853473614c4745431e949ba56',
    'server/src/accounts/backups.rs':
        '8d9801640afbe5dbb862060d8aea8accb4ddf47063dc775278e176559b32622a',
    'server/tests/support/mod.rs':
        'fbaccbd87526b237ac1db6d1a2f17ff810838bb15366409b5f0ef254d4cecce8',
    'server/tests/media_recovery.rs':
        'd786705aa844ae1cc1e41c9164a6f65975bd6d94355c2bc926cf48559d7e0d8f',
    'server/tests/deletion.rs':
        'ab0089c0ae552f21ca4cced65c799cdaf3f49e2605c5a66aa8c712dc31827cbe',
    'server/tests/backups.rs':
        'bc445dc6d148029a6164588148d50a05f2c25ce21ce8ceff4730657784c38d5d',
    'server/src/storage/reset.rs':
        'eb4b91a93c9976b7b07d3212f58e1e8e62b4deea0a9346e5e7c824c7dd052ee9',
    'server/src/storage/mod.rs':
        '7285b36d0b29aa93c82cb32b632239ea4b58cfd658eab661769dc0a756e05d46',
    'server/src/storage/collection.rs':
        'e27e2657c752118e14a57eee533d7070b2e40f29348c732b88058382a57c7537',
    'server/src/storage/backups.rs':
        'baf12de62a3bd512fb3d90ce639fbe94c003d5cc0f547bdb40ccb4cb2d1dd7ab',
    'server/web/tests/api.test.ts':
        '07eedde417f5b6a0a338c665cf42673b01d56a852fda07c5fc011cccd9ea19be',
    'server/web/tests/browser/mcp.spec.ts':
        '261b286236452b954829529242b040eb4d3784019eeb0e321b2f10ae7ece1f10',
    'server/web/tests/browser/backups.spec.ts':
        '415de8426361101e7835eb600665c7af271f7553c7f573090d576a8cf7cc25ba',
    # Complete-content public review, collection recovery fixture, 2026-11-25.
    'server/tests/backup_round_trip.rs':
        '6549998c8c16276260dc3acd8f05ec395dca05f29c29072a8d25c9cee6a2a1fd',
    'app/MeguriSnapshotTests/SentenceMiningPaneTests.swift':
        '3a36cc9a416d05baeca475a15c39b7c8a68a452a1967e31de47235eed1be50e7',
    'app/Packages/MeguriKit/Tests/AppModelTests/SentenceMiningPortTests.swift':
        '62037f92f41a3f706093d5c233055a9e26afa54151fb441388fb844cca074067',
    # Complete-content PUBLIC review: cleanroom 20261124T133218Z-2230.
    'app/Packages/MeguriKit/Sources/AppModel/Integrations/MiningHTTP.swift':
        '18e72c0d652f3b438b3c5811cbfe83fbbfb7aff0c9ef5db68c880a534b1d8317',
    'app/Packages/MeguriKit/Sources/AppModel/Integrations/MiningListener.swift':
        '00b15c38d211c08a3949cb883c52bcbfa9fb2dcc059d6ab3f1c93b046bd0171f',
    'app/Packages/MeguriKit/Sources/AppModel/Integrations/MiningWebSocket.swift':
        'bb35c52f2d96a87117c374f9cab80da43ce9c22c430883fc508f305a77cdaa46',
    'app/Packages/MeguriKit/Tests/AppModelTests/SentenceMiningEntryTests.swift':
        '207a37c06ba700b606294f567677085ff72f583b713a5caf1e9ae3461b819687',
    'app/Packages/MeguriKit/Tests/AppModelTests/SentenceMiningTests.swift':
        '7a31f9686977ee8a856e1691f8bc78fced6f815a461005358bfd9d3ad716d521',
    'core/crates/meguri-ffi/src/notes.rs':
        'e9fdf9804387494879a07f43a8cf21fa59343f0a7f41d1759512aee5315d61ae',
    # Complete-content PUBLIC review: s-20261124T142356Z-950aa36a, 2026-11-24.
    'app/Packages/MeguriKit/Sources/Features/Stats/Charts/StatsChartFormat.swift':
        '1cf8eb49077cdd24dcc2303c5063cdc326376f8dff8a98ccc33cc7c56acb52fa',
    # Complete-content PUBLIC review: Mac heatmap history, 2026-11-24.
    'app/Packages/MeguriKit/Sources/HeatmapKit/ActivityGrid.swift':
        '06bdb1cf1bb568541feedda2187a1a67d37bfe33353c25b1672419edf06da417',
    'core/crates/meguri-engine/tests/activity.rs':
        '37ad2a34372338cdac430f3b1d45299d433256d46cbbbc95032915ecb1fe3948',
    'core/crates/meguri-ffi/src/activity.rs':
        '1bf97e602e93e0809c259aae993be3b7328aab5ce181f192cfdf95cadc3bfa8d',
    'core/crates/meguri-storage/tests/activity.rs':
        'd5dc18eda6c1284885c9a23b32805add3ab078b91fbd63cead5bd9adc27f0b24',
    # Complete-content PUBLIC review: compact card statistics, privacy_review, 2026-11-25.
    'app/Packages/MeguriKit/Sources/Features/Stats/Page/Sections/StatsCardsSection.swift':
        'f73bae7a59bae22db476fea13ab47348a12f6796476fe829ca0e7a3f24e9254c',
    'app/Packages/MeguriKit/Tests/StatsTests/StatsSectionViewsTests.swift':
        'f7f5e48845f1d6ad2c4a240eff0ee7de5e6904344ba50330e7314b6c4850c5d0',
    'docs/design/native-interface-contract.md':
        'd9ed2a06da3dfacd3009e68c5ae3c56e046be708202777c41b9dcf873ada9a3f',
    'specs/stats/statistics-page.md':
        '41b9df08990e51d6eb84a23b1cd9dfc24c536b539d6892638b3f61bcbc0e53b3',
    # Complete-content PUBLIC review: add_note_privacy_review_2, 2026-11-19.
    'app/Packages/MeguriKit/Tests/AppModelTests/AddNoteFormTests.swift':
        '6b0c861d5e52b05545e3bfbd8466384c65c72ad2e3bc48f540f2f99a403f1414',
    # Whole-file PUBLIC review in cleanroom 20261124T154119Z-15087.
    'app/Packages/MeguriKit/Tests/AppModelTests/BrowseCardPreviewTests.swift':
        'f5b4994c338dd6d93d99cadbf15c90761a44269fe7d6dc7f0fe1030d6ee11c95',
    'docs/process/review-log/mcp-note-attachments-20261122.md':
        '9ab4f1074eba351094ed1660f1a69fea51d46097a917c040eac6e1fbd7bf4ea8',
    'docs/process/review-log/meguri-collection-mcp-20261119.md':
        '05cb4950ef3ced22e56e850a70ff9d1859ac49a216953314eb4aa0ada3ff6ab2',
    'server/src/accounts/oauth/mod.rs':
        '34f18c2a351778b43a6d71e8a774bca913c140feec5015689233502aa49df933',
    'server/src/httpapi/oauth.rs':
        'efe2fe1940f7a3e08f0982fd79073a64ec0202a1af8932254207f95e2b2e43c8',
    # Complete-content PUBLIC review: mcp-automatic-media, 2026-11-23.
    'server/src/mcp_transport/descriptors.rs':
        'c23fa9bc748dca54d7fc9464f896894b55ad21a238acd0128da736dd9e0accf8',
    'server/tests/mcp_oauth.rs':
        'fb74eaa7b7aa836199eb566f51034baf429477c3303d7c8dd049dd56932cc578',
    'server/src/mcp_service/transport.rs':
        '6c0fed2e3865fbdf02cd4faa84340fc70e8b888f3834cd64529ade113705bab9',
    # Complete-content PUBLIC review: mcp-search-candidates, 2026-11-22.
    'server/src/collection/mcp/search_cache.rs':
        'f0f5d92a2f3d79056c19d1d4953d2d9376c4633f3ab198380db38c5ba48d82be',
    # Complete-content PUBLIC review: mcp-search-budget, 2026-11-23.
    'server/src/collection/mcp/read.rs':
        '9a46822ad1682fee5b4226c231efd779b9171e883e75610c1bb0276933c9536d',
    'server/src/collection/mcp/search_index.rs':
        '22ba70723d6e216a853814908300f82a1b8e7eecee8b90e82398b8dd92da27b8',
    'server/src/mcp_service/authority_tests.rs':
        'ce8b31f955e6aac28cf117bcb041cee67566273a6c6583bd560285c46b23a4bc',
    # Complete-content PUBLIC review: mcp-sqlite-download, 2026-11-23.
    'server/src/mcp_service/export.rs':
        '463edb515478d490e6d07a1010edada22a852bbd3c451000228d245ad35fca1d',
    'server/src/mcp_service/attachments/format/audio.rs':
        '1eca9c96b3c638ad66723119d54ed0537246a3fedcd3fe6fd9acb5e8fa8043a9',
    'server/src/mcp_service/attachments/format/images.rs':
        'cea187846d99d4dd66f1b5ed068168f88270d1e1eec6e922c95d096aeff7249b',
    'server/src/mcp_service/attachments/format/isobmff.rs':
        '194a76940c9b0a123bb9203b591adee3e50a9c7954cff4f809e32af8c4ca78eb',
    'server/src/mcp_service/attachments/format/tests.rs':
        '62b28b28286f06f5973cc23ab806c4089dc1ab2a3fcbcf7dde9dd3608cc7ebdf',
    'server/tests/support/attachment_formats.rs':
        'ff6d752781665d9c0a80403079e653681629b52c1e384c834df4b3952fa76522',
    'server/src/mcp_service/attachments/format/pdf.rs':
        '2420851e5a4460084995d61923ff7f5faf2c5a4dde414988ef070ee8e42a7183',
    'server/src/mcp_transport/backend.rs':
        '4a0708c96150f2a28f5c68a493898163eee4c51bd0d52088b3c74f751e07ae62',
    'server/src/mcp_transport/diagnostics.rs':
        '3e03a73d427f6d33145db2ded9a456c7e28f500dd725f780ce5bcfd998d57bda',
    'server/src/mcp_transport/inputs.rs':
        '35fcbf999f5235196af4ae965cd2bf75b9482e5bfc83f16a566260f35d577522',
    'server/src/mcp_transport/mod.rs':
        '759e5513925799c9abc1f377d8b0bd6fba30c8ae20d9eb475f9ce24d30c56b42',
    'server/src/mcp_transport/tests.rs':
        '8d70b80220be300bab08739d2bc2bdfdf79e274f9dc6b1d833d3ada4ae5d546a',
    'server/src/collection/mcp/tests.rs':
        '78ee3ea5c3baaeac36c969d1d284165afba8651aeba1301ad8119cdebd0e7ccd',
    'server/src/collection/mcp/tests/size_scaling.rs':
        '6a7e9ae78d1ef2688703f1ce292b25ebebcb34ceb3e8f3b20dbdf66bc2dde7a0',
    'server/src/accounts/oauth/policy.rs':
        '802fd74612b1ae0d70b3ed32106b873b5e6dbd1824795eddbd40fa846ab16e46',
    'server/src/accounts/oauth/tests.rs':
        '0439b970e98b4937bbc1777b46a4e3bf88d9e50cf6da937258a63ef8bdaa0db7',
    'server/src/accounts/oauth/tokens.rs':
        '91e82ec8f188a98952328e82c7bbf29def1c2c85a2ba7bd3daaea2e9b55ae312',
    'server/src/accounts/deletion.rs':
        'a33371a855de6157e3ba332305384d8622943e270640735d90e6ce7da444bb4e',
    'server/src/storage/deletion.rs':
        'a40673bd3bbe0871786e7fbbf6bb0b80ebd3f026acf28a636b19638528d15ec9',
    "docs/process/review-log/meguri-sync-local-sign-in-recovery-20261118.md":
        "3f81b8c9ba8d4c6d8bc4912d58bfa7fb1a346f2e825d2f601e0ffa097c8603a7",
    "docs/process/review-log/app-icon-20261118.md":
        "3c02ff0da56dff510620c4c77ebb5b3919b0f47672b20699f05b7f669ff5b63e",
    'docker/sync-web.Dockerfile.dockerignore':
        'd0f1710720bc5b9d16c22f715f1094909a5978401fd22c65ecefcb07e66c60c6',
    'server/web/scripts/audit-applicability.py':
        '974bea89755ce61df03b7a8139ec7938cdbabd141339f30ae4904c64b1cfd5ad',
    'server/web/scripts/audit-assessment.json':
        'e4726bf8b73dac5b88b54cb33be38c06d30630d414f7dcff4fe62e8eddfc3dc2',
    'tools/tests/meguri_sync.bats':
        '28f47c0344b00e5586431b8e2efca2e3f870b650d23ba5f6f268d191c4a9d4b5',
    'tools/tests/sync_web_applicability_test.py':
        'cd3ebde8aee79cf4b16199847f60340dcfe5dd691eff5442a1bac86c2f31ff80',
    # Complete public tooling review: incoming_contract_review, 2026-11-17.
    "tools/mg.d/core.sh":
        "75592aa65666a46bb1260657ac45e85629d30ebdc1bb545b3eecb3b03a2842b4",
    'core/crates/meguri-host/tests/provider_bootstrap.rs':
        '50e27460dafefb7c36c5927956fa16b4b4a3c9bf0ab8415c15d714102b40cf64',
    # Complete-content public reviews: routine Core fix1 and Host fix2, 2026-11-17.
    "core/crates/meguri-host/tests/provider_normal.rs":
        "cad34eae95cbdbdf49073eb144fe56d3438907168377875253df204cbd305122",
    "core/crates/meguri-storage/tests/media_bound_reset.rs":
        "afc4b2b78e4a6482f363326fc71e687e51d0c6ee7d72f8dbdf554a1484839004",
    "core/crates/meguri-sync/src/media.rs":
        "729188c88c59067f305e662fb643e5fa3ccc6e1e04f1828619d17a820008a095",
    "core/crates/meguri-sync/src/provider/bootstrap.rs":
        "23fcbee1dfd8a262378e71c91841dab18f27ea210ba4c085126d5257741abd8e",
    'docs/process/review-log/meguri-sync-U11.md':
        '0c346236c3ff8af871444237053e8d6dd93c0eae7b04de31f58856f6ddfda5fd',
    'tools/tests/sync_recovery.bats':
        '58d02510a34db73837bd1e6fcde1441541892adfa2e46ba2128124a7b07ecb40',
    'tools/scripts/private-scan/tests/test_private_scan.py':
        'f6a486a548efb3a11731237f9d1802f9329b55532c2933ea74704158964288f1',
    'oracle/meguri-sync-server/retained_observer.py':
        '8900b3935f72fcd0e97dcd3274c1db258ee9c8b7e118dd940ba7d9267d71d410',
    'tools/tests/sync_retained.bats':
        '7c8d3c026019c49eb060d3bb07968198c8a35a551bed51c2a42e6cbf3333bbc6',
    'tools/mg.d/sync-retained.sh':
        '35e54ba829dde06fb8ad13587d01e7bbdec935ae4a2db793211af8560c25b1ff',
    'conformance/peers/retained-sync/support.py':
        'ce30fc3b176ece13e3dda3679fe8321da7b25a75ada59ff85da0802e4b9fe7cc',
    'conformance/peers/retained-sync/src/main.rs':
        '96e6a15b80ceccb78f8eb4753c6d094f09a4f1b98635e0b8b61b0f1c572dff30',
    'conformance/peers/retained-sync/src/fixture.rs':
        'c081384f0291814d7befe8aa178a5af98162331ced069c231a79fb125c7a026b',
    'conformance/peers/retained-sync/README.md':
        'b54bf7d41d4973a3df93224f0d8ce40e71748819acc69050e2316ad1e9de37d8',
    'conformance/peers/retained-sync/AGENTS.md':
        'a1aba360d6591c26719cf41483087415844925d55fdc8646419848aa68f30a06',
    'server/web/scripts/fixture.mjs':
        'd8055a16e61ad6afd14a42690d46c27f3ebfadf4569948204099e699cc680d63',
    'oracle/meguri-sync-server/tests/test_final_package.py':
        'dc7a82769bd6aa93590cc4b03a6c96540864226b7df7c28f3359ac3566b00816',
    'oracle/meguri-sync-server/tests/test_final_coverage.py':
        '2b359154ae1fcd65df0da0fbe231147cd7b136573a7bf457cc99c5a896b7fb6b',
    'oracle/meguri-sync-server/final_handshake.py':
        '9e7f889c17ebb1d9d0508d08591aa6aaf121941e76761f3655f6fa859110f823',
    'oracle/meguri-sync-server/final_desktop.py':
        '30cc39228face890621540aec5fe31e6ed040feea96fdb13c329034b1d1c3fdc',
    'oracle/meguri-sync-server/final_checks.py':
        '6876d4d4e92f33f0bb081facf6173cc01392c88addd986e356e5f8e690e17c35',
    'oracle/meguri-sync-server/driver.py':
        'd0d8b8321290b67063b8d6830dbabb6db1b19e878cd496295d6a175323447015',
    'conformance/recordings/scenarios/meguri-sync-server/candidate-final-additions.json':
        'ec691e94ea6e85fcdf8e6a71ce95d453216babae78871699c29c04e6206391f6',
    'conformance/recordings/scenarios/meguri-sync-server/candidate-mobile-final-selection.json':
        '7ca1cff89f051fc845c0e2bc31d140306f67432be5de8c5c31fe4bc70aa2b87d',
    'conformance/recordings/scenarios/meguri-sync-server/candidate-inputs.md':
        '62a4feb0a99aa774b7fcffed618c8a1d1144f759e86aba919d923cff7971076c',
    'conformance/recordings/scenarios/meguri-sync-server/candidate-retained-final-selection.json':
        '3df220aac299fe6f7c253a3e32c3c340171fb6c856b078589354f869c08592d7',
    'conformance/recordings/scenarios/meguri-sync-server/final-fixture-recipes.json':
        'f8f8488500dc11c8d3ce0da7185d0de280160b240da38a0cba11b94457902348',
    'specs/conformance/meguri-sync-local-delivery.md':
        'c39b06b7815f4ab7d074748e79ae027d2be48292f71293ee9dc6057a18fad126',

    # Complete bounded-content PUBLIC review: Codex r10_native_failure_diagnosis, 2026-11-17.
    'app/Packages/MeguriCore/Sources/CoreBridge/Sync/ProviderBootstrapService.swift':
        'edba139cde56f1dc7c10598a9de920c034b31d800320ed3e0068480e8fe6fb99',
    # Complete final-content PUBLIC review: Codex incoming_contract_review, 2026-11-17.
    'specs/sync/status-strings.md':
        '24dd422510bcb309b0cd30d0ff4f44e286f441da5a5284b1ef9acde3432b5b4e',
    # Complete final-content PUBLIC review: Codex incoming_contract_review, 2026-11-17.
    'docs/design/screens.md':
        '8075ff26d0014382e34c434f36d8c3dc9974b606fd1d4008df0a953bf39b3ac7',
    # Complete final-content PUBLIC review: Codex incoming_contract_review, 2026-11-17.
    'docs/design/screen-inventory.yaml':
        'cad9cab5b364992250a8060399849a90efa81d20109e75e145ff914e24cf0361',
    # Complete final-content PUBLIC review: Codex adoption_empty_decks, 2026-11-17.
    'core/crates/meguri-storage/src/media/inventory.rs':
        'a15fbd849371207df8a52ba993b64e26d339fcb9af56bbe3cb01652f38b7cae9',
    # Complete final-content PUBLIC review: Codex adoption_empty_decks, 2026-11-17.
    'core/crates/meguri-ffi/src/provider_bootstrap.rs':
        'f654aeec3189dbcbd57dc4d94ed14e65457c49119b37f87a1cf1de2ddfd2c7f9',
    # Complete PUBLIC review: Codex provider_test_delta_review, 2026-11-17.
    "docs/process/review-log/native-own-handoff-attribution-20261117.md":
        "05a9ce3d8eeecf5e249a920c444ca767eb915060cd7360d4a85e744d5931951e",

    # Complete final-content PUBLIC review: U37/t1m3, native-repair-fix1, 2026-11-17.
    "core/crates/meguri-ffi/src/cloud_policy_execution.rs":
        "d12ca5c4b84bbebf1916f6166715f86c255c57380091affd8f8780475bf9f3f9",
    "core/crates/meguri-ffi/src/cloud_policy_jobs.rs":
        "8302ab993a06ca1808705c51af3ceb2b73a84dd15ae382fbbe2ced8605482d17",
    "core/crates/meguri-host/src/cloud_adoption.rs":
        "cd46dffc0816cf3fd5ffe231abab5cb1f856a7074c925cb6d80cd87dd567376b",
    "core/crates/meguri-host/tests/own_handoff_attribution.rs":
        "61735c7012d7cc0d883d0caa5d211ebb426ff5181e1e6be4db060932079aa709",
    "core/crates/meguri-sync/src/cloud/runtime.rs":
        "a4217d453e672d9e072cf80e19296449d78fad30d13562fee13f4759ff6e4de5",
    "core/crates/meguri-sync/src/cloud/workflow_runtime.rs":
        "6bb6bb78447586c080d7b19adc9aae19447489c2d9d5f36bca3bd471863cb76f",

    'docs/process/review-log/meguri-sync-U10.md':
        '5c252409842d08d9d53f0b196ca737e8ec2051fa28aa38ed6fecd219e70a9126',
    'server/Dockerfile':
        '324a94c05aafbbf38065174f70578eb044337ea6d0ded209977196b685a1ccd8',
    'server/README.md':
        '4d6f4a0475d17afad978fd1ef20efc727372c7e0c5b0b8a7150ff7834fedaa72',
    'server/compose.yaml':
        '03737d36cd35810513c04e39a193b707e14228ca997e116ca93557c09e7a0d9b',
    'server/tools/package-context.py':
        'c9c5e140f77ac5b8fdfc18921c9bbda066848df146b3665149b3d7a228c7fa78',
    'tools/mg.d/sync-package.sh':
        '8ae4752a6c3951f74a376ab363517fbd18a788c6fd2d098423d19fca6a177a22',
    'tools/tests/sync_package.bats':
        '74610b07a76e2f63324db8652d9feee0ac2989dfda017156ab64589d6bc3ff78',
    'tools/tests/sync_package_context_test.py':
        'f8aea685104a88da5e0bb52e15eb251798705d55c1cc7ea24928e678f05641a6',
    "core/crates/meguri-ffi/src/sync_tests.rs":
        "461fb731a22379b6bbe009cd968526c3e706f6e08ae769ff014bc82c63a85135",
    # Complete public-file review: incoming_contract_review, 2026-11-17.
    "docs/process/review-log/cloud-protocol-consent-fixtures-20261117.md":
        "8f37e85fa552b7eece8721914aaf2c6c48c7608308b7c3c242088599b816398a",
    # Complete public-file review: U244/t1mf, 2026-11-17.
    "app/Packages/MeguriKit/Tests/AppModelTests/IssuedStartupAdmissionRegressionTests.swift":
        "1ed9e31a27b7f3f194d055a182f7097377437a5bacb1fa40377e3aad64818482",
    # Complete public-file review: owned_cleanup_diagnosis, 2026-11-19.
    "app/Packages/MeguriCore/Tests/CoreBridgeTests/CloudPolicyExecutionTests.swift":
        "b1bece502d8b3aabcef7c9ee6b5b9ebc6d43bba66812f5fc2f6e5edaf4626895",
    # Complete public-file review: owned_diagnostic_review, 2026-11-18.
    "docs/process/review-log/native-owned-provider-ui-20261118.md":
        "931d40dc291fa292c80c1f61405743827c81de3e6b952c5c07923c7a2d0ee160",
    # Complete public-file review: owned_diagnostic_review, 2026-11-19.
    "docs/process/verification-status.md":
        "5accd9f059712aa180d62515ab215d93e886faf911a4fbc3e2dbb93a662e8764",
    # Complete public-file review: hosted_core_selection_review, 2026-11-19.
    ".github/workflows/meguri-verification.yml":
        "17c70e069e904e69a3f53844e493bc3cf2155dae627ba36c7160a9e7555908c0",
    # Complete public-file review: verification_plan_review, 2026-11-19.
    'docs/process/tiers.md':
        'ce594823685b3434e0cd4dc98fb3c81d550880cb5284801f5d6de0c9506e85ec',
    # Complete public-file review: owned_diagnostic_review, 2026-11-18.
    "core/crates/meguri-engine/tests/editor_services.rs":
        "b6fb526f02c4f15c0946c9a06be200b0e47a5b836f05266b45af8d06d6bc2471",
    'app/Packages/MeguriKit/Sources/AppModel/Collection/SyncCoordinator.swift':
        '456aa2d9303b4e4a689f40f4ecfdf3692e5ee75612c8319f6652c3f8a5936b68',
    'app/Packages/MeguriKit/Tests/AppModelTests/ProviderAdmissionTests.swift':
        '4cf802ffe04f6888219aa637119e12847be828e2590840fd42d35cea9bfa86d7',
    'core/crates/meguri-ffi/src/provider_tests.rs':
        '0a1a65dce24702eda82b38a3c229604bf613a28c69c134735b02d1097775757f',
    "docs/process/review-log/meguri-provider-admission-20261117.md":
        "31af001e272ebcc61ca299e3d932f6f29bc5da69551389305956ef92cd9f0e40",
    # Complete public-file review: owned_cleanup_diagnosis, 2026-11-19.
    "core/crates/meguri-sync/src/cloud/adoption_runtime.rs":
        "512d13afe657d46086656e5854e3566f1b6ad7b2ef4e92a255b42f193d1f5c1a",
    # Complete public-source review; future bytes remain checked.
    "core/crates/meguri-sync/src/provider.rs":
        "793880794f47d8c5a5c2da40314556eb63b2a21d3a5a00e4d41948cf0948354f",
    "core/crates/meguri-host/tests/cloud_publication_phase.rs":
        "fc861d0bc2dc02b4020455251f3dbe2e0a0cb476f047f54ed0727329cae63e46",
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
        '2c0c7f8bbed6ee04f0ff217ee544d0216274626c6f7766db43feb557e2cda558',
    'docs/process/review-log/meguri-sync-normal-candidate.md':
        'a071d74e332b444e750964a80c7651915d23700b7221f55b183a998001c16996',
    'docs/process/review-log/meguri-sync-U8.md':
        'a945434c2e135435fe00a1d11de32aaaf68013dde5fadd315ba6668e2c6c3e76',
    'docs/process/review-log/meguri-sync-U6.md':
        '14fef85aa75d15af8154c274278c56744ce5040669d638378442f5e8cf4511e0',
    'oracle/meguri-sync-server/provenance.md':
        '30b781dbbeba8ffd346ec86d3224f08f33d2ac45908761b86a691e02d692d5d2',
    'server/tests/session.rs':
        '9bcc7edb13789a4abfd059bb19e03ec04544abbe441b0e2129f5f45e111d5e5d',
    'server/src/collection/graves.rs':
        '891dda6db91028e90badfc7ee5591ff1f04c060e06de3585981b16e4d6dd57ba',
    'server/src/collection/changes.rs':
        '83f506e218e17d7c878c5381110c76277d3c64e767eb8a3da299d50c81a92b33',
    'tools/gate/tiers.yaml':
        '9d5a1f79a08e96ac1699a4d7d1ba3ebb2c44a47d53ff1f5c1cda53c58c6c7d35',
    'tools/mg.d/sync-web.sh':
        '5eac87b061115351ba2287844f0c26425e20d1619c254ca573cf501ce1621914',
    'server/web/THIRD_PARTY_NOTICES.txt':
        '793118f2c4b41d07e802a9d465a3eb7424d68add574e1bf7695580e539e15ab9',
    'server/web/src/IBMPlexSans-Variable.woff2':
        'e978248b6b56da9e372975322a98dd9a51135d1b375e32ed6d5ca69f3aab792d',
    'server/web/upstream-records.json':
        'a3a66b858a72331499de6289e4745f834d34105b52655d1c22133fe0c599b099',
    'server/web/src/style.css':
        '39e6e70a5d5118f450375b13c90faa1e80f6d1de4d50f2047e348e16b5bfbd2b',
    'server/web/src/app.tsx':
        '2d32bd613d50aa1df4e388595c11bbeeed3ba1ffb4dbd780464085205753e20d',
    'server/web/tests/browser/accounts.spec.ts':
        'e212183bbaeedd51ec25b8302607b0ec7dcbb75333dba4b0ea861f179fb8a0d3',
    'server/web/scripts/run.sh':
        '45ae2cae47f70726bae42af8aa3d26621353f4ade6dd457d39f19b7b36e64418',
    'server/web/package-lock.json':
        'f2f38f36f9f092dec3723f3425047d2f10f4fb32b7c411242c2b1b32c6a1a2f4',
    'server/web/notices/rolldown-THIRD-PARTY-LICENSE.txt':
        'a877291d800ed43692f3f9ae09d8e01cc6f7293ad39d43896059c188ffbb8b7c',
    'server/web/DEPENDENCIES.md':
        '062796f5dd9be0fb9339cec1c3174aabd2db8b92d21d443fd95637239de556a9',
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
        '94ddec9d8850bbd907c6c6b4c5fb449d15c9ef703c37226ff535e27900cde962',
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
        'f6c0154c2d8d0edf2ab3559870694c9c9674f67e25b0fe78a8239e23574d7d0e',
    'oracle/meguri-sync-server/tests/test_media.py':
        'c819a65c159e28a1fe70ae3436e94d909e9a008320210f476114bf8a9c83697d',
    'server/src/media/archive.rs':
        '33e053ff1dc4f8965f61212ae5d56d3e8c3337db173473be46ff8bad0da3bded',
    'server/src/media/recovery.rs':
        'c5388c9f59ef653beb7d7472f3195dc106bb732ecb797475cfc4671c0735eba5',
    'server/tests/media.rs':
        '9aa88101ecdefc8c46120f1adac02655aa3bfe628f458dca7b6f80a6b3297a93',
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
    "docs/plans/2026-11-19-1436-feat-meguri-sync-collection-mcp-plan.md":
        "be34378df02e9c7b06534d226c0ae982109f41618c9354fd569ac5795de7020c",
    "specs/sync/server-collection.md":
        "85c3d211817ce238682043964f4e61835a5fbc83009134b69998242563c42ccb",
    "specs/sync/server-lifecycle.md":
        "ae5b10969d39c9bae94095fdb81d025d63b502cdb1d87720e5983fdd9559c5bf",
    "specs/sync/server-media.md":
        "32f9eff487e305d1d17bad276c3797de4e7664dae22662e4b72a61bd2c3166cb",
    "specs/servers/meguri-sync-web.md":
        '2a4303891cc65b73e0667511fb3d2ef941999a255f9caa816bcb5516ff599d33',
    "specs/servers/meguri-collection-mcp.md":
        "ea88b7499265974b3955511e3611d075a855c4cd5e3c446202759f56942b34a8",
    "docs/plans/2026-11-22-1323-feat-mcp-note-attachments-plan.md":
        "81191321d7af0a18c682b5bff490ea26beea3ccfa4ce1ad28b53f38dcc5f1adb",
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
        '48955b713598c6f1fb1a6de0a26dd91bd4925920ed34bb8a4799d24402f9c94a',
    'oracle/meguri-sync-server/audit_deps.py':
        '09ba79e0545fb91e5a2c653e93c75e6fa4a55734d52e54a70479ae8e8d55d151',
    'oracle/meguri-sync-server/catalog.py':
        'da44186c8ee85f1ef77b3ff2584e0adce209cc4cfb9a5ee1eb557ad5d9a0068f',
    'oracle/meguri-sync-server/evidence.py':
        '968874f037dcdcc77ef9e9e931718e6c43313d5eca3fee767818db72ca6d70cf',
    'oracle/meguri-sync-server/tests/test_validation.py':
        'e7bdbef81fb4b34a1b569b6fa4c186801e1c32904b3813d9e8c011e608a90dfd',
    'oracle/meguri-sync-server/validation.py':
        '0b18623520af011b829fee4f31728ffda31c2c99cc10046b7143b9fdba2a7034',
    'tools/mg.d/meguri-sync-reference.sh':
        '6328d822a00e648b5c3def04590fb55a0a29f0dd793780789d8783ac8b606d4b',

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
        'f83fbc6374f84b3189947e87de740cf4de749279c3d4783cca4bf97c1f1ec330',
    'server/src/accounts/provision.rs':
        'f75596225c443a3f33a58c5e1322ceba52326c4a7b507723c30b4f2aa6fc0416',
    'server/src/httpapi/AGENTS.md':
        'dbe717cde0b794b08a2fd368b90d75a4a7c9bc626c87de3dd4b7c1d04d22c89d',
    'server/src/httpapi/ingress.rs':
        'aaee8e568d0a2c915288942b847f45ec262874d13887ce2911bbe0dac4f39ef6',
    'server/src/httpapi/mod.rs':
        'd3c7d5e496d2d59a7f5afd2f1ebd4b5b8e51063f6c5a71947a6aac1bb6f8cb67',
    'server/src/httpapi/tests.rs':
        '502eb8ce076fa4fb2e8f81ce6a5efc00a3c4da7d4dad0c56eedaefece095704c',
    'server/src/storage/schema.sql':
        '4842a61b5bf0e3b8e909364d5df7667aea59f054c7175313dd28f840f6c7a31b',
    'server/src/storage/semantic.rs':
        '50df314a245ee5e41294b8c732ec0a5de9a4b8eab5fe1a9b0e4a1fd5e60de710',
    'server/src/storage/tests.rs':
        '4f31caea298b452a0e4071926355d60cf6856bb40075c78dcffbe19f04d29297',
    'server/tests/accounts.rs':
        '7dcfbdcd68c04fb9b2976ca6c5adbab58cc8b3829f90714c67dc5343f812b404',
    'server/tests/http_contract.rs':
        'b3af9669195af18792f1c9b44e8de582f7fb12ac658430f7bdf3ba469d8fcb7a',
    'server/tests/provision.rs':
        'cf31868be65159387c985c86cc6a07ea070febeff016406a0994c49ae0ef272e',
    'server/tests/recovery.rs':
        '1d31f70b941ead6f7a986a464fd3de874166308c5c3119e9793dad442f515521',

    'docs/process/review-log/meguri-sync-U4.md':
        '3dba5263b27e28389786dc37ee488630527cd3ba62a98ba350b357aac309aee8',
    # Independently reviewed public foundation receipt; delegated maintenance.
    'docs/process/review-log/meguri-sync-foundation.md':
        '1fbf5605fd269f6ee618599e607cec157dbb499b38751964eb22ba0d3fa29fc4',

    # Independently reviewed public transfer code and process evidence.
    'server/src/collection/schema.rs':
        'b6e1a6d0ad4657ad6d56b9d62cbb02e86ca153564335cceb6cdefd8c858accd2',
    'server/src/protocol/framing.rs':
        'a1349d00d0b25dd659e0e8fff5d3adc6b7ce63d20f861e17604e60071d79b777',
    'server/src/protocol/full.rs':
        '1e00dd45e0975e6fcd94c9a05b217b236a202c64573f2585a8a8096f751bd640',
    'server/src/protocol/mod.rs':
        '32c7e634ac02543ee958fa7df92aad82c2d7600c29271ed0d97a91423ac35d84',
    'server/src/storage/root.rs':
        'a060ba8e3453cae2c6ef4f4eb79287e634b9106138649dcfc12739b753a4df5a',
    'server/tools/candidate-output.py':
        '873c3ce682b93f23caa118b0d0b7f07792d7111e80a4ee1a51a0fe7631dd46f8',
    'docs/process/review-log/meguri-sync-U5.md':
        '6cc7dc20556c191e0644282d73259eb8ff8e23f7f98dbd76f107ae80b7eb9494',

    'tools/mg.d/meguri-sync.sh':
        'bde5cecf8b56ddf89a0a38fbdb20ab69486c82499d3c972b85dde46adf50eba7',

    'tools/tests/sync_server.bats':
        '6c0ad95ca4dafd841dc7dd5bc239dd810b11758a413f0b5219de28e89abda470',

    'oracle/meguri-sync-server/normal.py':
        'e4d1947437ac2a7d30c720eee3b228ffdaedc2fee55dc871213c221ef12bfb16',

    'oracle/meguri-sync-server/tests/test_normal.py':
        '2ad3925d47bd218c8914fcd6bcee2ffdd06a49baf0756f709975f4a8b3d987fe',

    'conformance/fixtures/meguri-sync-server/observations/desktop-26.09.4-normal.json':
        '33e4f576a85f19bf877baefc382e91121e4e0307fe73b4b06fc8e24807e4d4a7',

    # Independently reviewed public recovery fact snapshots, exact bytes.
    'docs/process/review-log/meguri-sync-recovery-dirty-reference.md':
        'd3e7180cff9d63d1f16b5eef4fe94cb5e115d05e4b58f5baf288299a47a48d31',
    'oracle/meguri-sync-server/README.md':
        'd7b34a4dab4a4a4f5ccf614b4cb3f2abf93201e5580790160a921882e5d222c5',
    'oracle/meguri-sync-server/tests/test_recovery_dirty.py':
        'd972f70a91c8c85032dfabcbc4d9ffb38564a5654a158610bc0c6dbe4d38d6e7',
    'core/crates/meguri-sync/tests/owned_round_admission.rs': '41cb97aab264f4ccc175dd5831089725d9afaf4f37f4620e2344b931451205c3',
    'app/MeguriSnapshotTests/MacSettingsSearchTests.swift': '8845d36966d566ed0a329c88a186a50d74b45c03f5474a4abb517f4f3eb3e43a',
    'app/Packages/MeguriKit/Sources/AppModel/Collection/ReviewCollection.swift': '079593aab3e69f01f91cb900ca3e328ffe3dea3706933bc527ba9e47d64586b4',
    'app/Packages/MeguriKit/Sources/AppModel/Sync/Resources/Sync.xcstrings': 'b53985ac1a2fc0896b0fc126f348614f962491d763da1bdedab3e309ab0e46f0',
    'app/Packages/MeguriKit/Sources/AppModel/Sync/SyncPresentation.swift': '0276bfab84c7453a688cc34bb313911fb8febb46544d23066540c410d3422c2f',
    'app/Packages/MeguriKit/Sources/Features/Settings/Settings.swift': '2fd10e5c3b0cce3f6dbe8c525b697131ab41445ec5695c2bfdf4ae2f7f359460',
    'app/Packages/MeguriKit/Sources/Features/Settings/Sync/AccountSyncView.swift': '397f631b73d908088772dbcc94a05539c16e02b49cee0489b39d8c36079755c9',
    'app/Packages/MeguriKit/Sources/Features/Settings/Sync/CloudSyncStatusButton.swift': '845d547fd3f64035420a1d723cc1bf3f38611a09dd143e9bc1be51bde8c1d2af',
    'app/Packages/MeguriKit/Sources/Features/Settings/Sync/MeguriSyncSetupViews.swift': '10614372d7279294c408d3638b01046d06c999d06d51f0daf413b17e2dee17ff',
    'app/Packages/MeguriKit/Sources/Features/Settings/Sync/SyncStatusContent.swift': '9238c2e349fd1c3cb4fcc4d6572209cd4809ccca1440565bf9d26d54f7bcf701',
    'app/Packages/MeguriKit/Tests/AppModelTests/CloudSyncPresentationTests.swift': '9656d70a62e751fe2e4a121a762ccdd6b23313ceb2822e6f696ca3a55f4f2561',
    'app/Packages/MeguriKit/Tests/AppModelTests/ReviewCollectionStartupTests.swift': 'dfbfb60e5a9a5fa55dc154d7ada9af09f2abc3a44494b29e96772b837fe8e56c',
    'core/crates/meguri-host/src/provider_bootstrap.rs': '038e6199bd9e9d63e307023e182caa3632ace08fee94c21563e61854357553bd',
    'core/crates/meguri-host/Cargo.toml': 'ef2e5f907851eb4d19984c899516e6affe5444516b59d245f4501dde2b744a73',
    'core/crates/meguri-host/examples/real_server_provider.rs': '30c41dac4cce43053fbabf3654565584fe6ebb3a8054c8660b1a8c4695d51ea5',
    'server/tools/package-smoke.mjs': '8a4ac4d7a54677a9fee3f64c4132de34e088d2f514c74c16083dbe9236ac456b',
    'tools/mg.d/meguri-provider-test.sh': 'bf637fbb25533a5cb354c079f4ee89d1518693e8e3b47c4c6d12b5c5467921f0',
    'tools/mg.d/meguri-native-provider.sh': '054b3eddd84f0d3f58a46075d4b0181a610a13ae728b87cfeda2ed09c779e94d',
    'tools/tests/remote_wrapper.bats': '4c167356b41f7f6b30467e109e1855169fbfa2bbf6f4a06c83a98fd60912af5c',
    'tools/tests/gate.bats': 'f47075ea99a15e3757e9731394b67b46a89307318eda7e18b9bff2d9a5d0a68b',
    'tools/tests/meguri_provider_real_server.bats': '08ede8953f4bd60ee36a9ae0824347d1b5340cb48a412b95523241a1dd661cbe',
    'tools/tests/meguri_native_provider.bats': 'efa0d705a98ddcec7570a1ceb9b84e828808555ca7d3252d2e3c48e5542df74f',
    'app/Packages/MeguriKit/Tests/AppModelTests/CloudPreviewActivityTests.swift': '9ad2c7dfb1e320bf428f622df4137fc660375692936788bc8202c11e43dbeded',
    'app/Packages/MeguriKit/Tests/AppModelTests/InitialCloudAdoptionTests.swift': 'adb1c143645d356dc8cc961329dfbf65d3cc800a2df0fe87e2299f9510aa193e',
    'app/Packages/MeguriKit/Tests/AppModelTests/CloudPublicationConfirmationTests.swift': 'b138a276e46e231897282352ec24692aa34d717bdf19b7f6b7a59b702567ecab',
    'app/Packages/MeguriKit/Tests/AppModelTests/Support/CloudServiceAppHarness.swift': 'ca43eb8423fe40f9b19356bce3c4005a1c173e1b8e60dcd3d074bb0eb07b1808',
    "specs/conformance/meguri-sync-mobile-package.md": "4f31ac538c06a2928e97c9c6edfd4378e2bbccaa647e4ff6101ac99b4d90cf9f",
    # Complete public-file review: owned_diagnostic_review, 2026-11-18.
    "core/crates/meguri-engine/src/editor/tag_complete.rs":
        "2c9222832834a1a4251d5388c2e158f15d69be007739c0953c7980e1ba13e768",
    # Complete public-file review: owned_cleanup_diagnosis, 2026-11-18.
    "app/Packages/MeguriKit/Sources/Features/Editor/FieldEditorHost.swift":
        "f2dd76f61515c6986bab071d1dacfab42282e8bf41c832a56311252d64c2d1a7",
    # Complete public-file review: owned_diagnostic_review, 2026-11-18.
    "app/Packages/MeguriKit/Tests/AppModelTests/FieldEditorTests.swift":
        "f4cfb32a6987d170f6b435e40cb8ca839ac1887c14b753ca97bfb0f1ca959a30",
    # Complete public-file review: owned_cleanup_diagnosis, 2026-11-18.
    "docs/process/review-log/U190-U195-field-runtime-cleanroom-20261118.md":
        "f1432c094aa8dc1498d33b196e56b0b15fb1b8ab73c23ee7bac34c15f302d455",
    # Complete public-file review: owned_diagnostic_review, 2026-11-18.
    "app/Packages/MeguriKit/Sources/AppModel/Editor/AddNoteFlow.swift":
        "577130bbe8fe6ea990045e36ca550fa4979f860306677172f8c6302af8e5dc53",
    # Complete public-file review: owned_diagnostic_review, 2026-11-18.
    "app/Packages/MeguriKit/Tests/AppModelTests/ManualTransferSettingsTests.swift":
        "797f2c54583e226dabcf48f3b7992503b60542ad79337612e37ab7b9f1628668",
    # Complete public-file review: owned_diagnostic_review, 2026-11-18.
    "docs/process/review-log/U195-add-coordinator-cleanroom-20261118.md":
        "14622ebedb2c73a88ac7e4f6285ff2d1d1f9a548825d660f324d7b6dee345601",
    # Complete-content PUBLIC review: independent non-author reviewer, 2026-11-21
    # (docs/process/review-log/card-text-keywords-privacy-20261121.md).
    'core/crates/meguri-engine/src/notetypes/card_text/keywords.rs':
        '88590bbdd44094ec0fdc7ae5942ca26bfa4fcb22810067ee3cb68e72e9a86893',
}


# Published Askama derive 0.16.1 files verified against archive SHA256
# 071ee5ebf2138e3ad180e0aacf6940c2cab5e6d8333741d9925c7bee2b153f39.
# Keys pin exact repository-path UTF-8 bytes; values pin complete file bytes.
PUBLIC_VENDOR_ARTIFACTS = {
    # Complete-content PUBLIC review: independent non-author reviewer, 2026-11-21
    # (docs/process/review-log/selftest-six-20261121.md).
    "757fd5fb3da8ccfca681ebdc23701b14eb3a62c18056c970122d156377c7099e":
        "0ff0200e298c6b2c06eca789fe37c8d967c2f3ce6a1e6fef42e89e19b4341a49",
    '7f3eddd9b055b1b5220d7e096440db86572a8f0074cc61166de2a6609b308e78':
        '3e2b6410255d6c7d07c209a11d02c793e6b0a1e1156ac8fc0509f2f375051e0d',
    '6a9d04ca41587836af459446bdc65bf4d7255154cbe98aabcce66a19b4c66043':
        '963aa51734ae21fcbee28b7037dc5c0931bbba92177b6060ef2e561d1ee9c83c',
    'be7929cd9f7a1698bea4f000aa3d1f236c8b440d58361b5dd177b9697badc26d':
        '445aa8aa694a60231e0aad0bc3c99b4c950f19eed4566abbcaf9a7ac9736491a',
    'ebe7db15d768840d3f5bcd115f2345a19c31c0d20f5435c23ebbdca9c07e4c21':
        'd1d7f78ad34c3fad64889ce65d5e245c6e2dc9b3a9c07104414b8748099f0ebb',
    'f81e67fcb2e3dbe8379aead1ebe38904691e35f510169970a987710496d3ed77':
        'c4fbcc54ab0cfef15fa5338638584eb5553536c7fe91a4220bef4652c2176aab',
    '1abb17517ad6c3616bc662765627c8e3419b3d81f0b8eaf3358cf4af9d316a33':
        'e9717da96a6a936947f83da63f3f9eecc9ae73429f5f0d698278fa329e0e4281',
    '25ea9fbf9d10948d08a6ead80c9f83ec2154ae34f40f881e4659eff9349d258f':
        'd08c9fb8ec612b54a95978b5411f5c91218d8ef4a4ac1ba64b1d5d0ac116e076',

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


def found_file(path, relative, **what):
    """A scan finding. A pinned path found here has bytes its pin no longer
    covers (a matching pin is never scanned): say so, since its reviewed
    lines then count like new ones."""
    entry = dict(path=path, **what)
    if public_artifact_digest(relative) is not None:
        entry["stale_public_artifact"] = True
    return entry


def pins(repo, commit=None):
    """The pinned paths of a commit whose bytes differ from their pin, and
    the pinned paths it lacks. No token set is read."""
    if commit is None:
        commit = git(repo, "rev-parse", "--verify", "HEAD^{commit}").decode("ascii").strip()
    elif re.fullmatch(r"(?:[0-9a-f]{40}|[0-9a-f]{64})", commit) is None:
        raise ValueError("pins needs a full commit oid")
    pinned, stale, seen = 0, [], set()
    for record in git(repo, "ls-tree", "-r", "-z", "--full-tree", commit).split(b"\0"):
        if not record:
            continue
        meta, raw = record.split(b"\t", 1)
        mode, kind, sha = meta.split(b" ")
        path = os.fsdecode(raw)
        expected = public_artifact_digest(path)
        if expected is None:
            continue
        pinned += 1
        seen.add(path)
        if kind != b"blob" or mode not in (b"100644", b"100755") or \
                hashlib.sha256(git(repo, "cat-file", "blob", sha.decode("ascii"))).hexdigest() != expected:
            stale.append(path)
    absent = sorted(set(PUBLIC_ARTIFACTS) - seen)
    # Vendor pins are keyed by a hash of their path, so only a count of the
    # ones this commit lacks can be given.
    held = {hashlib.sha256(path.encode("utf-8")).hexdigest() for path in seen}
    return {"commit": commit, "pinned": pinned, "stale": sorted(stale), "absent": absent,
            "absent_vendor": len(set(PUBLIC_VENDOR_ARTIFACTS) - held)}


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
                found.append(found_file(path, relative, binary=True))
        else:
            hits = source_line_hits(matcher, relative, data)
            if hits:
                found.append(found_file(path, relative, lines=hits))
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
    out = [found_file(p, p, lines=found[p]) for p in sorted(found)]
    for path in binary:
        data = git(repo, "cat-file", "blob", ":" + path)
        if path not in public and binary_hit(matcher, data):
            out.append(found_file(path, path, binary=True))
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
    n = sub.add_parser("pins")
    n.add_argument("--repo", required=True)
    n.add_argument("--commit")
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
        if args.command == "pins":
            print(json.dumps(pins(args.repo, args.commit), sort_keys=True))
            return 0
        if args.command == "build":
            print(json.dumps(build(args.candidates, args.repo, args.out), sort_keys=True))
            return 0
        if args.staged == bool(args.paths) or (args.staged and not args.repo):
            parser.error("give --staged with --repo, or paths")
        # An empty set would pass everything: a truncated or unreadable
        # file must not look like a clean scan, even beside a full one.
        # Each file is read once: what was checked is what is scanned with.
        tokens = set()
        for path in args.tokens:
            with open(path, encoding="utf-8") as f:
                one = set(tokens_from_lines(f))
            if not one:
                raise ValueError("a token set is empty")
            tokens.update(one)
        matcher = Matcher(tokens)
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
