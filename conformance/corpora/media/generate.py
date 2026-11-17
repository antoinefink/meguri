#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Writes the media area's naming differential as a generated scenario.

    python3 -B -I conformance/corpora/media/generate.py [--out conformance/scenarios/media/corpus]

Output (specs/conformance/dsl.md D1, a generated `.json` scenario with an
oracle golden):
  names-01.json   2,000 `add_media` steps (specs/conformance/media-ops.md F5)
                  into one media folder, then `media_dir` and `media_pending`

The adds reuse earlier desired names, often in another letter case and with
the same or other bytes, so the dedupe rows, the lowercase candidates and the
hash names of specs/media/media-files.md M3 and M4 all occur. 2,000 counts
steps; desired names repeat on purpose.

Constraints, so that Meguri must match the golden on every lane:
- Only adds write to the folder, so every stored name equals its own case
  key (media-files.md M3: where Meguri and Anki choose the same names).
- No character whose lowercase mapping and case folding disagree (sigma
  forms, sharp s, long s, micro sign, dotless i, Greek symbol variants,
  Cherokee, ligatures): two names that differ only there would be one name
  on a case-insensitive volume but two to the case key.
- Every name keeps an ASCII letter or digit through the transform, and none
  is ignored (media-files.md M8), since Anki and Meguri handle those apart.

The steps come from a seeded generator: the same Python version writes the
same file (3.13.1 wrote the committed one). Never edit the output by hand;
regenerate, then re-record the golden with `tools/mg oracle record
media/corpus`. Case variants come from Python's own Unicode tables, so
another Python version may write other names.
"""

from __future__ import annotations

import argparse
import json
import random
import re
import unicodedata
from pathlib import Path

SEED = 20261016
STEPS = 2000

WORDS = ["photo", "Photo", "PHOTO", "img", "IMG", "Scan", "audio", "Clip", "clip", "diagram", "README", "Notes",
         "x", "Q", "a", "B", "word", "Lesson", "card"]
DIGITS = ["1", "2", "07", "2023", "99"]
SEPARATORS = [" ", "-", "_", ".", "  ", "..", "~", "'", "(", ")", "&", "#", "%", "+", ","]
REMOVED = ["[", "]", "<", ">", ":", "\"", "/", "?", "*", "^", "\\", "|", "\x01", "\t", "\x7f",
           "\U0001f970", "͸"]
LETTERS = ["é", "É", "ñ", "Ñ", "ü", "Ü", "ø", "Ø", "å", "Å",
           "Α", "α", "Ω", "ω", "Δ", "δ", "Λ", "λ", "ά", "Ά",
           "Ж", "ж", "Я", "я", "Ё", "ё", "Ա", "ա",
           "İ", "K", "Å", "Ǆ", "ǅ", "ǆ", "\U00010400", "\U00010428"]
SCRIPTS = ["日本", "漢字", "猫", "ひらがな", "カタカナ",
           "한국", "\U0001f600", "❤"]
SPECIAL = [" ", "é", "Å", "　"]
DEVICES = ["CON", "con", "Prn", "AUX", "nul", "COM1", "lpt9", "com10"]
EXTS = [".jpg", ".JPG", ".Jpg", ".png", ".PNG", ".mp3", ".MP3", ".ogg", ".webp", ".tar.gz", ".TAR.GZ", "",
        ".abcdefghijklm", ".ÉÉÉÉÉÉÉ", ".日本語", ".a b", ".", " "]
CONTENTS = ["c0", "c1", "c2", "c3", "c4", "c5"]

# Characters whose lowercase mapping and case folding disagree, or that a
# case change of another character can produce: never in a name.
AVOID = set("Σσςßẞſµıϐϑϕϖϰϱϵϴ"
            "ẛιͅﬀﬁﬂﬃﬄﬅﬆ")
FORBIDDEN = set("[]<>:\"/?*^\\|")
IGNORED = re.compile(r"^:?thumbs.db$|^.ds_store$", re.IGNORECASE)


def rough_transform(name: str) -> str:
    """Enough of text/media-filenames F1 to test the generator's constraints."""
    name = unicodedata.normalize("NFC", name)
    name = "".join(ch for ch in name if ch not in FORBIDDEN and not (ord(ch) < 0x20 or ord(ch) == 0x7f))
    return name.replace(" ", " ")


def acceptable(name: str) -> bool:
    if any(ch in AVOID for ch in name) or any(ch in AVOID for ch in name.lower() + name.upper()):
        return False
    rough = rough_transform(name)
    if not re.search(r"[A-Za-z0-9]", rough):
        return False
    return not IGNORED.match(rough) and not IGNORED.match(rough.lower())


def piece(r: random.Random) -> str:
    k = r.random()
    if k < 0.35:
        return r.choice(WORDS)
    if k < 0.45:
        return r.choice(DIGITS)
    if k < 0.60:
        return r.choice(SEPARATORS)
    if k < 0.75:
        return r.choice(LETTERS)
    if k < 0.85:
        return r.choice(SCRIPTS)
    if k < 0.93:
        return r.choice(REMOVED)
    return r.choice(SPECIAL)


def fresh_name(r: random.Random) -> str:
    while True:
        parts = [r.choice(DEVICES)] if r.random() < 0.1 else []
        parts.append(r.choice(WORDS + DIGITS))
        for _ in range(r.randint(0, 4)):
            parts.append(piece(r))
        stem = "".join(parts)
        if r.random() < 0.12:
            stem *= r.randint(4, 16)
        name = stem + r.choice(EXTS)
        if acceptable(name):
            return name


def case_variant(r: random.Random, name: str) -> str:
    k = r.random()
    if k < 0.3:
        return name.upper()
    if k < 0.55:
        return name.lower()
    if k < 0.7:
        return name.title()
    if k < 0.85:
        return name.swapcase()
    return name


def steps(r: random.Random) -> list[dict]:
    out: list[dict] = []
    last_content: dict[str, str] = {}
    history: list[str] = []
    while len(out) < STEPS:
        if history and r.random() < 0.45:
            base = r.choice(history)
            name = case_variant(r, base)
            if not acceptable(name):
                continue
            content = last_content[base] if r.random() < 0.5 else r.choice(CONTENTS)
        else:
            name = fresh_name(r)
            content = r.choice(CONTENTS)
        history.append(name)
        last_content[name] = content
        out.append({"add_media": {"name": name, "text": content}})
    out.append({"observe": ["media_dir", "media_pending"]})
    return out


def write(path: Path, value: dict) -> None:
    text = json.dumps(value, ensure_ascii=False, indent=1)
    path.write_text(text + "\n", encoding="utf-8")


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", default=str(Path(__file__).resolve().parents[2] / "scenarios" / "media" / "corpus"))
    out = Path(ap.parse_args().out)
    out.mkdir(parents=True, exist_ok=True)
    r = random.Random(SEED)
    write(out / "names-01.json", {
        "id": "media/corpus/names-01",
        "description": "Generated media adds with case variants and collisions (corpora/media/generate.py).",
        "tags": ["generated"],
        "clock": {"tz": "UTC", "start": "2030-01-15T10:00:00"},
        "steps": steps(r),
    })


if __name__ == "__main__":
    main()
