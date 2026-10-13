# SPDX-License-Identifier: AGPL-3.0-or-later
"""Inventory of Anki's tests at the pinned tag (R1877-R1882; spec side only).

  python3 -B -I oracle/harvest/inventory.py inventory --anki DIR --tag 26.09.3 \
      [--supplementary MAIN_DIR --supplementary-tag main@<sha>] \
      --links oracle/harvest/links.csv --catalog conformance/catalog/harvest.csv
  python3 -B -I oracle/harvest/inventory.py check --links ... --catalog ... [--pin conformance/oracle.json]

`inventory` lists every Rust `#[test]`, pytest-style Python test and
vitest/playwright TypeScript test in an Anki checkout at the pinned tag. Each
gets an opaque harvest id (h + 12 hex digits of a sha256 over language, path,
name and occurrence). `links.csv` (here, AGPL) maps ids to Anki files and test
names, with a sha256 of each test's source (`body_sha256`: Rust from `#[test]`
to the end of the function, Python the function or method with its
decorators, TypeScript the test call); `conformance/catalog/harvest.csv` (MPL,
implementers read it) holds only the id, language, source, Meguri area,
classification, reason, scenarios and status. Rows already in the catalog keep
their classification, reason, scenarios and status; new rows get the rules
below or `unclassified`; rows whose test no longer exists are dropped and
listed.

Changed tests (R1882): a test whose source hash differs from the one links.csv
had is listed under "changed since <old tag>" and its catalog row goes back to
`open` (classification, reason and scenarios stay) for its spec writer to
review, unless the row is still what NOT_APPLICABLE gives for its path (that
rule depends on the path alone). Its id does not change.

Supplementary tests: with --supplementary, tests on Anki's main branch whose
(language, file, name) the tag does not have are added with links.csv tag
`main@<sha>` and catalog source `supplementary`; spec writers classify whether
their behavior exists at the tag. A re-inventory without --supplementary keeps
the supplementary rows it has (dropping those the tag now holds), so only a
main re-scan replaces them. They are not in the counts at the tag.

`check` fails when the catalog and links disagree on ids or sources, when a
links row has no source hash, when any catalog value contains an Anki file
path, file name or distinctive test name (R1881), or, with --pin, when
links.csv was inventoried at another release than the oracle pin (R1882:
re-inventory in the change that adopts a bump).

Only stdlib: it runs in any Python 3.10+ (`tools/mg harvest-inventory` runs it
in the dev container).
"""

from __future__ import annotations

import argparse
import ast
import csv
import hashlib
import io
import json
import re
import sys
import warnings
from collections import Counter
from dataclasses import dataclass
from pathlib import Path

CATALOG_HEADER = ["harvest_id", "language", "source", "area", "classification", "reason", "scenarios", "status"]
LINKS_HEADER = ["harvest_id", "language", "tag", "file", "test", "body_sha256"]
SKIP_DIRS = {".git", "node_modules", "target", "out", ".venv", "__pycache__", "dist"}

# (language, path prefix) -> (classification, reason): the groups this
# inventory classifies itself (U4: clearly not applicable). Area spec writers
# classify everything else in the catalog.
NOT_APPLICABLE = [
    ("python", "qt/", "desktop", "Anki's desktop Qt user interface, installer and add-on manager; Meguri has its own app"),
    ("python", "tools/", "tooling", "Anki's documentation and release tooling"),
    ("rust", "build/", "tooling", "Anki's build system"),
    ("rust", "ftl/", "tooling", "Anki's translation-file tooling; Meguri writes its own strings"),
    ("rust", "rslib/linkchecker/", "tooling", "Anki's documentation link checker"),
    ("rust", "rslib/process/", "tooling", "Anki's build-time process helper"),
]

# Path prefix -> Meguri spec area, first match wins.
AREAS = [
    ("rslib/src/scheduler/", "sched"),
    ("rslib/src/import_export/", "package"),
    ("rslib/src/notetype/", "notetypes"),
    ("rslib/src/search/", "search"),
    ("rslib/src/decks/", "decks"),
    ("rslib/src/deckconfig/", "deckconfig"),
    ("rslib/src/tags/", "tags"),
    ("rslib/src/sync/", "sync"),
    ("rslib/src/storage/", "storage"),
    ("rslib/src/media/", "media"),
    ("rslib/src/stats/", "stats"),
    ("rslib/src/notes/", "notes"),
    ("rslib/src/card/", "notes"),
    ("rslib/src/adding.rs", "notes"),
    ("rslib/src/card_rendering/", "render"),
    ("rslib/src/template", "render"),
    ("rslib/src/cloze.rs", "render"),
    ("rslib/src/typeanswer.rs", "render"),
    ("rslib/src/latex.rs", "render"),
    ("rslib/src/image_occlusion/", "render"),
    ("rslib/src/text.rs", "text"),
    ("rslib/src/findreplace.rs", "browser"),
    ("rslib/src/browser_table", "browser"),
    ("rslib/src/dbcheck.rs", "maintenance"),
    ("rslib/src/config/", "model"),
    ("rslib/src/serde.rs", "model"),
    ("rslib/", "engine"),
    ("pylib/tests/test_schedv3.py", "sched"),
    ("pylib/tests/test_find.py", "search"),
    ("pylib/tests/test_decks.py", "decks"),
    ("pylib/tests/test_models.py", "notetypes"),
    ("pylib/tests/test_stdmodels.py", "notetypes"),
    ("pylib/tests/test_notes.py", "notes"),
    ("pylib/tests/test_cards.py", "notes"),
    ("pylib/tests/test_tags.py", "tags"),
    ("pylib/tests/test_flags.py", "tags"),
    ("pylib/tests/test_media.py", "media"),
    ("pylib/tests/test_config.py", "model"),
    ("pylib/tests/test_importing_exporting.py", "package"),
    ("pylib/tests/test_stats.py", "stats"),
    ("pylib/tests/test_latex.py", "render"),
    ("pylib/tests/test_template.py", "render"),
    ("pylib/tests/test_sound.py", "render"),
    ("pylib/tests/test_utils.py", "text"),
    ("pylib/tests/test_httpclient.py", "sync"),
    ("pylib/", "engine"),
    ("ts/reviewer/", "review"),
    ("ts/routes/deck-options/", "deckconfig"),
    ("ts/routes/change-notetype/", "notetypes"),
    ("ts/routes/card-info/", "stats"),
    ("ts/routes/editor/", "editor"),
    ("ts/lib/editable/", "editor"),
    ("ts/lib/domlib/", "editor"),
    ("ts/lib/html-filter/", "editor"),
    ("ts/tests/e2e/", "editor"),
    ("ts/lib/tslib/", "text"),
    ("ts/", "editor"),
    ("qt/", "desktop"),
    ("tools/", "tooling"),
    ("build/", "tooling"),
    ("ftl/", "tooling"),
]


@dataclass(frozen=True)
class Test:
    language: str
    file: str  # path relative to the Anki checkout, '/' separators
    name: str
    occurrence: int  # 1 for the first test with this file and name
    body_sha256: str = ""  # sha256 of the test's source span; not part of the id

    @property
    def harvest_id(self) -> str:
        key = f"{self.language}:{self.file}::{self.name}#{self.occurrence}"
        return "h" + hashlib.sha256(key.encode("utf-8")).hexdigest()[:12]


# --- scanning -----------------------------------------------------------------

_RUST_TEST_ATTR = re.compile(r"^\s*#\[(?:tokio::)?test\]")
_RUST_FN = re.compile(r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)")
_RUST_CHAR = re.compile(r"'(?:[^'\\\n]|\\(?:x[0-9a-fA-F]{2}|u\{[0-9a-fA-F_]{1,8}\}|.))'")
_RUST_RAW = re.compile(r'(#*)"')
_IDENT = re.compile(r"[A-Za-z_][A-Za-z0-9_]*")
_TS_TEST = re.compile(
    r"""(?m)^\s*(?P<call>(?:test|it)(?:\.(?:only|skip|concurrent|fixme|fails))*(?:\.each\([^)]*\))?)"""
    r"""(?P<paren>\()\s*(?P<q>['"`])(?P<name>(?:\\.|(?!(?P=q)).)*)(?P=q)"""
)
_JS_IDENT = re.compile(r"[A-Za-z_$][A-Za-z0-9_$]*")
# After these a `/` starts a regular expression literal, not a division.
_JS_REGEX_AFTER = set("(,=:[!&|?{};+-*%<>~^")
_JS_KEYWORDS_BEFORE_EXPR = {"return", "typeof", "case", "do", "else", "in", "of", "new", "delete", "void", "throw",
                            "instanceof", "yield", "await"}


def body_sha256(text: str) -> str:
    return hashlib.sha256(text.replace("\r\n", "\n").encode("utf-8")).hexdigest()


def _files(root: Path, pattern: str):
    for path in sorted(root.rglob(pattern)):
        rel = path.relative_to(root)
        if any(part in SKIP_DIRS for part in rel.parts) or not path.is_file():
            continue
        yield path, rel.as_posix()


def _rust_item_end(text: str, i: int) -> int:
    """The index after the body of the item whose signature starts at i:
    braces count outside comments, strings, raw strings and char literals."""
    n, depth = len(text), 0
    while i < n:
        c = text[i]
        if text.startswith("//", i):
            j = text.find("\n", i)
            i = n if j < 0 else j + 1
        elif text.startswith("/*", i):
            nest, i = 1, i + 2
            while i < n and nest:
                if text.startswith("/*", i):
                    nest, i = nest + 1, i + 2
                elif text.startswith("*/", i):
                    nest, i = nest - 1, i + 2
                else:
                    i += 1
        elif c.isalpha() or c == "_":
            ident = _IDENT.match(text, i)
            i = ident.end()
            raw = _RUST_RAW.match(text, i) if ident.group() in ("r", "br", "cr") else None
            if raw:
                close = text.find('"' + raw.group(1), raw.end())
                i = n if close < 0 else close + 1 + len(raw.group(1))
        elif c == '"':
            i += 1
            while i < n and text[i] != '"':
                i += 2 if text[i] == "\\" else 1
            i += 1
        elif c == "'":
            char = _RUST_CHAR.match(text, i)
            i = char.end() if char else i + 1  # otherwise a lifetime or a label
        elif c == "{":
            depth, i = depth + 1, i + 1
        elif c == "}":
            depth, i = depth - 1, i + 1
            if depth == 0:
                return i
        elif c == ";" and depth == 0:
            return i + 1
        else:
            i += 1
    return n


def rust_tests(root: Path) -> list[tuple[str, str, str]]:
    out = []
    for path, rel in _files(root, "*.rs"):
        text = path.read_text(encoding="utf-8", errors="replace").replace("\r\n", "\n")
        lines = text.splitlines(keepends=True)
        offsets = [0]
        for line in lines:
            offsets.append(offsets[-1] + len(line))
        for i, line in enumerate(lines):
            if not _RUST_TEST_ATTR.match(line):
                continue
            for j in range(i + 1, min(i + 12, len(lines))):
                m = _RUST_FN.match(lines[j])
                if m:
                    out.append((rel, m.group(1), text[offsets[i] : _rust_item_end(text, offsets[j] + m.end())]))
                    break
    return out


def _is_test_class(node: ast.ClassDef) -> bool:
    if node.name.startswith("Test"):
        return True
    for base in node.bases:
        name = base.attr if isinstance(base, ast.Attribute) else getattr(base, "id", "")
        if name.endswith("TestCase"):
            return True
    return False


def python_tests(root: Path) -> list[tuple[str, str, str]]:
    found: dict[tuple[str, str], str] = {}  # a later def with the same name replaces the earlier one, as when run
    for pattern in ("test_*.py", "*_test.py"):
        for path, rel in _files(root, pattern):
            text = path.read_text(encoding="utf-8", errors="replace").replace("\r\n", "\n").replace("\r", "\n")
            try:
                with warnings.catch_warnings():
                    # Anki's sources may hold invalid escapes; only the syntax tree matters.
                    warnings.simplefilter("ignore", SyntaxWarning)
                    tree = ast.parse(text)
            except SyntaxError:
                continue
            lines = text.split("\n")

            def span(node: ast.AST) -> str:
                first = min([node.lineno, *(d.lineno for d in node.decorator_list)])
                return "\n".join(lines[first - 1 : node.end_lineno])

            for node in tree.body:
                if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)) and node.name.startswith("test"):
                    found[(rel, node.name)] = span(node)
                elif isinstance(node, ast.ClassDef) and _is_test_class(node):
                    for item in node.body:
                        if isinstance(item, (ast.FunctionDef, ast.AsyncFunctionDef)) and item.name.startswith("test"):
                            found[(rel, f"{node.name}.{item.name}")] = span(item)
    return [(rel, name, body) for (rel, name), body in sorted(found.items())]


def _js_string(text: str, i: int) -> int:
    quote, i = text[i], i + 1
    while i < len(text) and text[i] not in (quote, "\n"):
        i += 2 if text[i] == "\\" else 1
    return i + 1 if i < len(text) and text[i] == quote else i


def _js_template(text: str, i: int) -> int:
    i += 1
    while i < len(text):
        if text[i] == "\\":
            i += 2
        elif text[i] == "`":
            return i + 1
        elif text.startswith("${", i):
            i = _js_scan(text, i + 1, "{", "}")
        else:
            i += 1
    return len(text)


def _js_regex(text: str, i: int) -> int:
    """The index after a regular expression literal at i, or i + 1 when the
    line ends first (then the slash was a division)."""
    j, in_class = i + 1, False
    while j < len(text):
        c = text[j]
        if c == "\\":
            j += 2
            continue
        if c == "\n":
            return i + 1
        if in_class:
            in_class = c != "]"
        elif c == "[":
            in_class = True
        elif c == "/":
            flags = _JS_IDENT.match(text, j + 1)
            return flags.end() if flags else j + 1
        j += 1
    return i + 1


def _js_scan(text: str, i: int, opener: str, closer: str) -> int:
    """The index after the closer matching the opener at i, outside comments,
    strings, template literals and regular expression literals."""
    n, depth, prev = len(text), 0, "("
    while i < n:
        c = text[i]
        if c.isspace():
            i += 1
        elif text.startswith("//", i):
            j = text.find("\n", i)
            i = n if j < 0 else j
        elif text.startswith("/*", i):
            j = text.find("*/", i + 2)
            i = n if j < 0 else j + 2
        elif c in "'\"":
            i, prev = _js_string(text, i), "a"
        elif c == "`":
            i, prev = _js_template(text, i), "a"
        elif c == "/" and prev in _JS_REGEX_AFTER:
            i, prev = _js_regex(text, i), "a"
        elif c.isalnum() or c in "_$":
            word = _JS_IDENT.match(text, i)
            end = word.end() if word else i + 1
            prev = "(" if word and word.group() in _JS_KEYWORDS_BEFORE_EXPR else "a"
            i = end
        else:
            if c == opener:
                depth += 1
            elif c == closer:
                depth -= 1
                if depth == 0:
                    return i + 1
            i, prev = i + 1, c
    return n


def typescript_tests(root: Path) -> list[tuple[str, str, str]]:
    out = []
    for pattern in ("*.test.ts", "*.spec.ts", "*.test.js", "*.spec.js"):
        for path, rel in _files(root, pattern):
            text = path.read_text(encoding="utf-8", errors="replace").replace("\r\n", "\n")
            for m in _TS_TEST.finditer(text):
                out.append((rel, m.group("name"), text[m.start("call") : _js_scan(text, m.start("paren"), "(", ")")]))
    return out


def scan(root: Path) -> list[Test]:
    tests = []
    for language, found in (
        ("rust", rust_tests(root)),
        ("python", python_tests(root)),
        ("typescript", typescript_tests(root)),
    ):
        seen: Counter = Counter()
        for rel, name, body in found:
            seen[(rel, name)] += 1
            tests.append(Test(language, rel, name, seen[(rel, name)], body_sha256(body)))
    return tests


def area_of(test: Test) -> str:
    for prefix, area in AREAS:
        if test.file.startswith(prefix):
            return area
    return "engine"


def default_classification(test: Test) -> tuple[str, str, str]:
    """(area, classification, reason) for a new row."""
    for language, prefix, area, reason in NOT_APPLICABLE:
        if test.language == language and test.file.startswith(prefix):
            return area, "not-applicable", reason
    return area_of(test), "unclassified", ""


# --- files --------------------------------------------------------------------


def read_csv(path: Path, header: list[str]) -> list[dict]:
    if not path.exists():
        return []
    with path.open(encoding="utf-8", newline="") as f:
        reader = csv.DictReader(f)
        if reader.fieldnames != header:
            raise SystemExit(f"{path}: header must be {','.join(header)}")
        return list(reader)


def write_csv(path: Path, header: list[str], rows: list[dict]) -> None:
    buf = io.StringIO()
    writer = csv.DictWriter(buf, fieldnames=header, lineterminator="\n")
    writer.writeheader()
    writer.writerows(rows)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(buf.getvalue(), encoding="utf-8", newline="")


def is_supplementary(tag: str) -> bool:
    """A links.csv tag is a release (26.09.3) or a branch revision (main@<sha>)."""
    return "@" in tag


def release_tag(anki_version: str) -> str:
    """The pin's distribution version (26.9.3) as Anki's git tag (26.09.3)."""
    major, minor, patch = (int(x) for x in anki_version.split("."))
    return f"{major}.{minor:02d}.{patch}"


def status_for(classification: str, scenarios: str) -> str:
    if classification in ("not-applicable", "out-of-scope"):
        return "done"
    if classification == "unclassified" or not scenarios.split():
        return "open"
    return "done"


def cmd_inventory(args) -> int:
    if bool(args.supplementary) != bool(args.supplementary_tag) or (
        args.supplementary_tag and not is_supplementary(args.supplementary_tag)
    ):
        print("inventory: --supplementary needs --supplementary-tag main@<sha>, and the reverse", file=sys.stderr)
        return 2
    tests = scan(Path(args.anki))
    previous = {row["harvest_id"]: row for row in read_csv(Path(args.links), LINKS_HEADER)}
    at_tag = {(t.language, t.file, t.name) for t in tests}

    def link_row(t: Test, tag: str) -> dict:
        return {"harvest_id": t.harvest_id, "language": t.language, "tag": tag, "file": t.file, "test": t.name,
                "body_sha256": t.body_sha256}

    links = [link_row(t, args.tag) for t in tests]
    if args.supplementary:
        extra = [t for t in scan(Path(args.supplementary)) if (t.language, t.file, t.name) not in at_tag]
        supplementary = [link_row(t, args.supplementary_tag) for t in extra]
    else:
        supplementary = [
            row
            for row in previous.values()
            if is_supplementary(row["tag"]) and (row["language"], row["file"], row["test"]) not in at_tag
        ]
    links += supplementary
    ids = Counter(row["harvest_id"] for row in links)
    clashes = [i for i, n in ids.items() if n > 1]
    if clashes:
        print(f"inventory: harvest id clash: {clashes}", file=sys.stderr)
        return 1
    # Old tag -> ids whose source changed since (R1882).
    changed: dict[str, list[str]] = {}
    for link in links:
        old = previous.get(link["harvest_id"])
        if old and old["body_sha256"] and old["body_sha256"] != link["body_sha256"]:
            changed.setdefault(old["tag"], []).append(link["harvest_id"])
    changed_ids = {i for changed_at in changed.values() for i in changed_at}
    existing = {row["harvest_id"]: row for row in read_csv(Path(args.catalog), CATALOG_HEADER)}
    catalog = []
    reopened = []
    for link in links:
        source = "supplementary" if is_supplementary(link["tag"]) else "tag"
        row = existing.get(link["harvest_id"])
        area, classification, reason = default_classification(Test(link["language"], link["file"], link["test"], 1))
        if row is None:
            row = {
                "harvest_id": link["harvest_id"],
                "language": link["language"],
                "area": area,
                "classification": classification,
                "reason": reason,
                "scenarios": "",
                "status": status_for(classification, ""),
            }
        elif (
            link["harvest_id"] in changed_ids
            and row["status"] != "open"
            # The path rule alone classifies it, so the new source changes nothing.
            and not (classification == "not-applicable" and (row["classification"], row["reason"]) == (classification, reason))
        ):
            row = {**row, "status": "open"}
            reopened.append(link["harvest_id"])
        catalog.append({**row, "source": source})
    dropped = sorted(set(existing) - set(ids))
    links.sort(key=lambda r: (r["language"], r["file"], r["test"], r["harvest_id"]))
    catalog.sort(key=lambda r: (r["area"], r["language"], r["harvest_id"]))
    write_csv(Path(args.links), LINKS_HEADER, links)
    write_csv(Path(args.catalog), CATALOG_HEADER, catalog)
    counts = Counter(t.language for t in tests)
    classes = Counter(r["classification"] for r in catalog)
    extra_counts = Counter(r["language"] for r in supplementary)
    print(
        f"inventory: {len(tests)} tests at {args.tag}: "
        + ", ".join(f"{lang} {counts[lang]}" for lang in ("rust", "python", "typescript"))
        + f"; {len(supplementary)} supplementary: "
        + ", ".join(f"{lang} {extra_counts[lang]}" for lang in ("rust", "python", "typescript"))
        + "; "
        + ", ".join(f"{c} {n}" for c, n in sorted(classes.items())),
        file=sys.stderr,
    )
    if dropped:
        print(f"inventory: {len(dropped)} catalog rows dropped (test gone): {' '.join(dropped)}", file=sys.stderr)
    for tag, changed_at in sorted(changed.items()):
        print(f"inventory: {len(changed_at)} tests changed since {tag}: {' '.join(sorted(changed_at))}", file=sys.stderr)
    if reopened:
        print(
            f"inventory: {len(reopened)} catalog rows reopened (status open) for their spec writer to review: "
            + " ".join(sorted(reopened)),
            file=sys.stderr,
        )
    return 0


# --- leak check ---------------------------------------------------------------


def distinctive(name: str) -> bool:
    """A test name specific enough that seeing it in the catalog means it was
    copied: an identifier with an underscore or of 10+ characters, or a title
    of three or more words."""
    if " " in name.strip():
        return len(name.split()) >= 3
    return "_" in name or len(name) >= 10


def leaks(catalog: list[dict], links: list[dict]) -> list[str]:
    """Catalog cells that contain an Anki file path, file name or distinctive
    test name. The finding names the row, the column and the kind of name, not
    the name itself: tools/mg logs that implementers read carry it."""
    needles: dict[str, str] = {}
    for link in links:
        needles.setdefault(link["file"], "file path")
        needles.setdefault(link["file"].rsplit("/", 1)[-1], "file name")
        for part in [link["test"], *link["test"].split(".")]:
            if distinctive(part):
                needles.setdefault(part, "test name")
    found = []
    for row in catalog:
        for column in CATALOG_HEADER:
            value = row.get(column, "")
            kinds = sorted({kind for needle, kind in needles.items() if needle and needle in value})
            found += [f"{row['harvest_id']} {column}: contains an Anki {kind}" for kind in kinds]
    return found


def cmd_check(args) -> int:
    catalog = read_csv(Path(args.catalog), CATALOG_HEADER)
    links = read_csv(Path(args.links), LINKS_HEADER)
    problems = []
    catalog_ids = {r["harvest_id"] for r in catalog}
    link_ids = {r["harvest_id"] for r in links}
    problems += [f"{i}: in the catalog but not in links.csv" for i in sorted(catalog_ids - link_ids)]
    problems += [f"{i}: in links.csv but not in the catalog" for i in sorted(link_ids - catalog_ids)]
    problems += [
        f"{r['harvest_id']}: body_sha256 must be 64 lower-case hex digits (re-inventory)"
        for r in links
        if not re.fullmatch(r"[0-9a-f]{64}", r["body_sha256"])
    ]
    sources = {r["harvest_id"]: "supplementary" if is_supplementary(r["tag"]) else "tag" for r in links}
    problems += [
        f"{r['harvest_id']}: source {r['source']} in the catalog, {sources[r['harvest_id']]} in links.csv"
        for r in catalog
        if r["harvest_id"] in sources and r["source"] != sources[r["harvest_id"]]
    ]
    if args.pin:
        pin = json.loads(Path(args.pin).read_text(encoding="utf-8"))
        want = release_tag(pin["anki_version"])
        found = sorted({r["tag"] for r in links if not is_supplementary(r["tag"])})
        if found != [want]:
            problems.append(
                f"links.csv was inventoried at {', '.join(found) or 'no release'}, the oracle pin is {want}: "
                "re-inventory in the change that adopts the bump (tools/mg harvest-inventory, R1882)"
            )
    problems += leaks(catalog, links)
    for p in problems:
        print(f"check: {p}", file=sys.stderr)
    print(f"check: {len(catalog)} catalog rows, {len(links)} links, {len(problems)} problems", file=sys.stderr)
    return 1 if problems else 0


def main(argv: list[str] | None = None) -> int:
    p = argparse.ArgumentParser(prog="inventory.py")
    sub = p.add_subparsers(dest="command", required=True)
    inv = sub.add_parser("inventory")
    inv.add_argument("--anki", required=True, help="Anki checkout at the pinned tag")
    inv.add_argument("--tag", required=True)
    inv.add_argument("--supplementary", help="Anki checkout of the main branch")
    inv.add_argument("--supplementary-tag", help="main@<sha> of that checkout")
    inv.add_argument("--links", required=True)
    inv.add_argument("--catalog", required=True)
    chk = sub.add_parser("check")
    chk.add_argument("--links", required=True)
    chk.add_argument("--catalog", required=True)
    chk.add_argument("--pin", help="conformance/oracle.json: links.csv must be inventoried at its release")
    args = p.parse_args(argv)
    return {"inventory": cmd_inventory, "check": cmd_check}[args.command](args)


if __name__ == "__main__":
    sys.exit(main())
