# SPDX-License-Identifier: AGPL-3.0-or-later
"""Inventory of Anki's tests at the pinned tag (R1877-R1882; spec side only).

  python3 -B -I oracle/harvest/inventory.py inventory --anki DIR --tag 26.09.3 \
      --links oracle/harvest/links.csv --catalog conformance/catalog/harvest.csv
  python3 -B -I oracle/harvest/inventory.py check --links ... --catalog ...

`inventory` lists every Rust `#[test]`, pytest-style Python test and
vitest/playwright TypeScript test in an Anki checkout at the pinned tag. Each
gets an opaque harvest id (h + 12 hex digits of a sha256 over language, path,
name and occurrence). `links.csv` (here, AGPL) maps ids to Anki files and test
names; `conformance/catalog/harvest.csv` (MPL, implementers read it) holds only
the id, language, Meguri area, classification, reason, scenarios and status.
Rows already in the catalog keep their classification, reason, scenarios and
status; new rows get the rules below or `unclassified`; rows whose test no
longer exists are dropped and listed.

`check` fails when the catalog and links disagree on ids, or when any catalog
value contains an Anki file path, file name or distinctive test name (R1881).

Only stdlib: it runs in any Python 3.10+ (`tools/mg harvest-inventory` runs it
in the dev container).
"""

from __future__ import annotations

import argparse
import ast
import csv
import hashlib
import io
import re
import sys
import warnings
from collections import Counter
from dataclasses import dataclass
from pathlib import Path

CATALOG_HEADER = ["harvest_id", "language", "area", "classification", "reason", "scenarios", "status"]
LINKS_HEADER = ["harvest_id", "language", "tag", "file", "test"]
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

    @property
    def harvest_id(self) -> str:
        key = f"{self.language}:{self.file}::{self.name}#{self.occurrence}"
        return "h" + hashlib.sha256(key.encode("utf-8")).hexdigest()[:12]


# --- scanning -----------------------------------------------------------------

_RUST_TEST_ATTR = re.compile(r"^\s*#\[(?:tokio::)?test\]")
_RUST_FN = re.compile(r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)")
_TS_TEST = re.compile(
    r"""(?m)^\s*(?:test|it)(?:\.(?:only|skip|concurrent|fixme|fails))*(?:\.each\([^)]*\))?\(\s*(['"`])((?:\\.|(?!\1).)*)\1"""
)


def _files(root: Path, pattern: str):
    for path in sorted(root.rglob(pattern)):
        rel = path.relative_to(root)
        if any(part in SKIP_DIRS for part in rel.parts) or not path.is_file():
            continue
        yield path, rel.as_posix()


def rust_tests(root: Path) -> list[tuple[str, str]]:
    out = []
    for path, rel in _files(root, "*.rs"):
        lines = path.read_text(encoding="utf-8", errors="replace").splitlines()
        for i, line in enumerate(lines):
            if not _RUST_TEST_ATTR.match(line):
                continue
            for following in lines[i + 1 : i + 12]:
                m = _RUST_FN.match(following)
                if m:
                    out.append((rel, m.group(1)))
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


def python_tests(root: Path) -> list[tuple[str, str]]:
    out = []
    for pattern in ("test_*.py", "*_test.py"):
        for path, rel in _files(root, pattern):
            try:
                with warnings.catch_warnings():
                    # Anki's sources may hold invalid escapes; only the syntax tree matters.
                    warnings.simplefilter("ignore", SyntaxWarning)
                    tree = ast.parse(path.read_text(encoding="utf-8", errors="replace"))
            except SyntaxError:
                continue
            for node in tree.body:
                if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)) and node.name.startswith("test"):
                    out.append((rel, node.name))
                elif isinstance(node, ast.ClassDef) and _is_test_class(node):
                    for item in node.body:
                        if isinstance(item, (ast.FunctionDef, ast.AsyncFunctionDef)) and item.name.startswith("test"):
                            out.append((rel, f"{node.name}.{item.name}"))
    return sorted(set(out))


def typescript_tests(root: Path) -> list[tuple[str, str]]:
    out = []
    for pattern in ("*.test.ts", "*.spec.ts", "*.test.js", "*.spec.js"):
        for path, rel in _files(root, pattern):
            text = path.read_text(encoding="utf-8", errors="replace")
            for m in _TS_TEST.finditer(text):
                out.append((rel, m.group(2)))
    return out


def scan(root: Path) -> list[Test]:
    tests = []
    for language, found in (
        ("rust", rust_tests(root)),
        ("python", python_tests(root)),
        ("typescript", typescript_tests(root)),
    ):
        seen: Counter = Counter()
        for rel, name in found:
            seen[(rel, name)] += 1
            tests.append(Test(language, rel, name, seen[(rel, name)]))
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


def status_for(classification: str, scenarios: str) -> str:
    if classification in ("not-applicable", "out-of-scope"):
        return "done"
    if classification == "unclassified" or not scenarios.split():
        return "open"
    return "done"


def cmd_inventory(args) -> int:
    root = Path(args.anki)
    tests = scan(root)
    ids = Counter(t.harvest_id for t in tests)
    clashes = [i for i, n in ids.items() if n > 1]
    if clashes:
        print(f"inventory: harvest id clash: {clashes}", file=sys.stderr)
        return 1
    existing = {row["harvest_id"]: row for row in read_csv(Path(args.catalog), CATALOG_HEADER)}
    links, catalog = [], []
    for t in tests:
        links.append({"harvest_id": t.harvest_id, "language": t.language, "tag": args.tag, "file": t.file, "test": t.name})
        row = existing.get(t.harvest_id)
        if row is None:
            area, classification, reason = default_classification(t)
            row = {
                "harvest_id": t.harvest_id,
                "language": t.language,
                "area": area,
                "classification": classification,
                "reason": reason,
                "scenarios": "",
                "status": status_for(classification, ""),
            }
        catalog.append(row)
    dropped = sorted(set(existing) - {t.harvest_id for t in tests})
    links.sort(key=lambda r: (r["language"], r["file"], r["test"], r["harvest_id"]))
    catalog.sort(key=lambda r: (r["area"], r["language"], r["harvest_id"]))
    write_csv(Path(args.links), LINKS_HEADER, links)
    write_csv(Path(args.catalog), CATALOG_HEADER, catalog)
    counts = Counter(t.language for t in tests)
    classes = Counter(r["classification"] for r in catalog)
    print(
        f"inventory: {len(tests)} tests at {args.tag}: "
        + ", ".join(f"{lang} {counts[lang]}" for lang in ("rust", "python", "typescript"))
        + "; "
        + ", ".join(f"{c} {n}" for c, n in sorted(classes.items())),
        file=sys.stderr,
    )
    if dropped:
        print(f"inventory: {len(dropped)} catalog rows dropped (test gone): {' '.join(dropped)}", file=sys.stderr)
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
    inv.add_argument("--links", required=True)
    inv.add_argument("--catalog", required=True)
    chk = sub.add_parser("check")
    chk.add_argument("--links", required=True)
    chk.add_argument("--catalog", required=True)
    args = p.parse_args(argv)
    return {"inventory": cmd_inventory, "check": cmd_check}[args.command](args)


if __name__ == "__main__":
    sys.exit(main())
