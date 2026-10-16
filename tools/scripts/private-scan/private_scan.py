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
(binary files whole, as runs of printable text: see PRINTABLE_RUN). A line
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
import collections
import html
import json
import math
import os
import re
import subprocess
import sys
import unicodedata
import urllib.parse

WORD = re.compile(r"[^\W_]+")
MIN_CHARS = 4
# Binary files are scanned as runs of printable characters: valid UTF-8 other
# than C0 controls, DEL and U+FFFD (SQLite pages keep text as is, between
# record headers, integers and the 0x1f field separator). A run of LONG_RUN
# characters or more is scanned wherever it is. A shorter run (MIN_CHARS or
# more) is scanned only where the bytes around it do not look compressed:
# PNG, JPEG or zip data holds about ten such runs per megabyte that equal a
# four-letter word by chance. Runs under MIN_CHARS are never scanned.
PRINTABLE_RUN = re.compile(
    rb"(?:[\x20-\x7e]"
    rb"|[\xc2-\xdf][\x80-\xbf]"
    rb"|\xe0[\xa0-\xbf][\x80-\xbf]"
    rb"|[\xe1-\xec\xee][\x80-\xbf]{2}"
    rb"|\xef(?:[\x80-\xbe][\x80-\xbf]|\xbf[\x80-\xbc\xbe\xbf])"
    rb"|\xed[\x80-\x9f][\x80-\xbf]"
    rb"|\xf0[\x90-\xbf][\x80-\xbf]{2}"
    rb"|[\xf1-\xf3][\x80-\xbf]{3}"
    rb"|\xf4[\x80-\x8f][\x80-\xbf]{2})"
    rb"{%d,}" % MIN_CHARS
)
LONG_RUN = 8
# Compressed-looking: the byte entropy of the NOISE_WINDOW bytes around a
# short run is NOISE_BITS per byte or more. Compressed data measures about
# 7.9 over 4 KiB; database pages of text and integers stay under 7.
NOISE_WINDOW = 4096
NOISE_BITS = 7.0
SURROGATE_PAIR = re.compile(r"\\u(d[89ab][0-9a-f]{2})\\u(d[c-f][0-9a-f]{2})", re.IGNORECASE)
ESCAPE = re.compile(r"\\(?:u([0-9a-fA-F]{4})|U([0-9a-fA-F]{8})|x([0-9a-fA-F]{2}))")
SNIFF = 8192


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

    def find(self, ws):
        """Yields each token found in `ws` (with repeats)."""
        for i, w in enumerate(ws):
            if w in self.single:
                yield (w,)
            for n in self.lengths.get(w, ()):
                token = tuple(ws[i : i + n])
                if len(token) == n and token in self.multi:
                    yield token

    def hit(self, text):
        for _ in self.find(words(text)):
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


def byte_entropy(data):
    """Shannon entropy of `data`, in bits per byte."""
    total = len(data)
    return -sum(n / total * math.log2(n / total) for n in collections.Counter(data).values())


def looks_compressed(data, at):
    """Whether the NOISE_WINDOW bytes around `at` (the whole of a smaller
    file) look like compressed data."""
    start = max(0, min(at - NOISE_WINDOW // 2, len(data) - NOISE_WINDOW))
    return byte_entropy(data[start:start + NOISE_WINDOW]) >= NOISE_BITS


def binary_hit(matcher, data):
    """Whether a binary file's printable runs hold a token (see PRINTABLE_RUN)."""
    for m in PRINTABLE_RUN.finditer(data):
        run = m.group(0).decode("utf-8")
        if not any(matcher.hit(v) for v in variants(run)):
            continue
        if len(run) >= LONG_RUN or not looks_compressed(data, m.start()):
            return True
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


def scan_paths(matcher, paths):
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
                if any(matcher.hit(v) for v in variants(line[1:])):
                    found.setdefault(path, []).append(line_no)
                line_no += 1
    out = [{"path": p, "lines": found[p]} for p in sorted(found)]
    for path in binary:
        data = git(repo, "cat-file", "blob", ":" + path)
        if binary_hit(matcher, data):
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
            found, scanned = scan_paths(matcher, args.paths)
        print(json.dumps({"files": found, "files_scanned": scanned, "tokens": matcher.count}, sort_keys=True))
        return 1 if found else 0
    except (OSError, ValueError, RuntimeError, KeyError) as e:
        # The message names a file or a git command, never a token.
        print("private_scan.py: %s" % e, file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
