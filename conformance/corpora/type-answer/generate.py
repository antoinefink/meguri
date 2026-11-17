#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Writes the type-in comparison corpus as generated scenarios.

    python3 -B -I conformance/corpora/type-answer/generate.py [--out conformance/scenarios/render/type-answer/corpus]

Output (dsl.md D1, generated `.json` scenarios with oracle goldens):
  compare-01.json .. compare-10.json  300 [expected, typed] pairs each, run
                                      through `compare` and `compare_nc` of the
                                      `type_answer` kind
                                      (specs/conformance/type-answer-ops.md K1)

That is 3,000 distinct pairs. Expected texts are field-like: words in many
scripts (precomposed and decomposed accents, Greek, Cyrillic, Arabic and
marks on several scripts, syllable blocks, voiced kana,
kanji, emoji with modifiers, joiners and variation selectors), sometimes
wrapped in markup, entities, line breaks, sound tags and white space, and one
in twenty long enough (200 characters or more) to reach the sequence
matcher's long-text behavior (specs/render/type-answer.md A6.4). Typed texts
are what a learner might type: the plain answer, or it with case changed,
marks dropped, another normalization form, letters dropped, added, swapped
or replaced, spaces changed, cut short, doubled, or something else entirely,
and now and then nothing.

The pairs come from a seeded generator: the same Python version writes the
same files (the current corpus uses the digest-pinned Python 3.12 oracle runtime; the generation receipt records its exact Python and Unicode versions). Never edit the output by hand;
regenerate, then re-record the goldens with `tools/mg oracle record
render/type-answer/corpus`. Normalization forms and case variants come from
Python's own Unicode tables, so another Python version may write other
pairs.
"""

from __future__ import annotations

import argparse
import json
import random
import unicodedata
from pathlib import Path

SEED = 20261016
FILES, PAIRS = 10, 300

WORDS = [
    # Artificial syllables retain script, accent and case coverage.
    "qav", "Zovik", "vexul", "Qurav", "qav zex", "qávul", "qävox", "qçavel", "Qëvax", "Qraße", "vüber",
    "Ævø", "qörvå", "Łivź", "İqav", "ıqır", "zúvñel", "qème vûlée", "Åqvöm", "ǅavul",
    # Greek and Cyrillic samples.
    "ξλαφ", "ΞΛΑΦ", "ξλάφ", "Φρέξα", "фжэ", "Жфэю", "ёфж", "цжф", "Їфж",
    # Samples with marks in right-to-left scripts.
    "ظغث", "ظَغَثَ", "ثَغْظَة", "צקף", "צָׁקְף", "ץְקֵפִׁף",
    # Further script samples.
    "झफठ", "झिफी", "ฌฬฆ", "ฬิฌี",
    # Korean samples.
    "쀼쐬", "쯤뾰", "쐐쭈", "ㄲㅃㅉ",
    # Japanese samples include voiced marks and compatibility kana.
    "ゐゑゎ", "ゔゐ", "ぱゑ", "ヰヱヮ", "ヷヸヹ", "ｦｧｨ", "齟齬", "麒麟", "鬱蒼", "々",
    # Chinese forms.
    "龘齉", "鬱龘", "齾靐",
    # Compatibility characters.
    "ﬁqav", "①②", "x²", "Ｑａｖ", "½", "™",
    # Emoji, modifiers, joiners and variation selectors.
    "👍", "👍🏽", "❤️", "❤", "👩‍👩‍👧", "🇯🇵",
    # Digits and punctuation.
    "42", "3.14", "a-b", "q'v", "\"qav\"", "(x + y)", "a/b", "?!",
]

MARKS = ["́", "̀", "̈", "̧", "̵", "̃", "゙", "゚", "ָ", "َ", "️", "̇"]
WRAPS = [
    ("<b>", "</b>"), ("<i>", "</i>"), ("<div>", "</div>"), ("<span style=\"color: red\">", "</span>"),
    ("", "<br>"), ("<br>", ""), ("", "<br />"), ("", "&nbsp;"), ("&nbsp;", ""), (" ", " "), ("\n", ""),
    ("", "[sound:say it.mp3]"), ("[sound:a.ogg]", ""), ("<div>", "</div><div></div>"), ("<!-- c -->", ""),
    ("", "<img src=\"pic.png\">"), ("　", ""), ("\t", "\n"),
]
SEPARATORS = [" ", ", ", " / ", "<br>", " &amp; ", "&nbsp;", "\n", " - ", "<div>", ""]
ENTITIES = [("&", "&amp;"), ("<", "&lt;"), (">", "&gt;"), ("\"", "&quot;"), (" ", "&nbsp;")]
LETTERS = "abcdefghijklmnopqrstuvwxyzáéíóúàèäöüçñßαβγδабвгдあいうかがきぎ漢字가나다"


def pick_words(r: random.Random, n: int) -> list[str]:
    words = [r.choice(WORDS) for _ in range(n)]
    if r.random() < 0.15:
        # a stray combining mark after a word, or at the very start
        i = r.randrange(len(words))
        mark = r.choice(MARKS)
        words[i] = (mark + words[i]) if r.random() < 0.2 else (words[i] + mark)
    return words


def plain_answer(r: random.Random) -> str:
    if r.random() < 0.05:
        # long: 200 characters or more
        segment = " ".join(pick_words(r, r.randint(3, 6)))
        text = segment
        while len(text) < 200:
            text += r.choice([" ", ", ", " "]) + (segment if r.random() < 0.6 else " ".join(pick_words(r, 3)))
        return text
    n = r.choices([1, 2, 3, 4], weights=[5, 3, 2, 1])[0]
    words = pick_words(r, n)
    return r.choice([" ", " ", ", "]).join(words)


def expected_field(r: random.Random, plain: str) -> str:
    """The stored field: the plain answer with markup, entities and media."""
    text = plain
    if r.random() < 0.4:
        parts = text.split(" ")
        text = r.choice(SEPARATORS).join(parts) if len(parts) > 1 and r.random() < 0.5 else text
    if r.random() < 0.3:
        for raw, ent in ENTITIES:
            if raw in text and r.random() < 0.7:
                text = text.replace(raw, ent)
    for _ in range(r.choices([0, 1, 2], weights=[5, 3, 1])[0]):
        a, b = r.choice(WRAPS)
        text = a + text + b
    if r.random() < 0.1:
        form = r.choice(["NFD", "NFC", "NFKC"])
        text = unicodedata.normalize(form, text)
    return text


def mutate(r: random.Random, plain: str) -> str:
    kind = r.choices(
        ["same", "case", "strip_marks", "nfd", "nfkd", "drop", "add", "swap", "replace", "spaces",
         "cut", "double", "other", "empty", "multi"],
        weights=[10, 8, 8, 4, 3, 10, 8, 6, 10, 5, 5, 2, 6, 3, 8],
    )[0]
    chars = list(plain)
    if kind == "same":
        return plain
    if kind == "case":
        return r.choice([plain.lower(), plain.upper(), plain.swapcase(), plain.capitalize(), plain.casefold()])
    if kind == "strip_marks":
        return "".join(c for c in unicodedata.normalize("NFD", plain) if not unicodedata.category(c).startswith("M"))
    if kind == "nfd":
        return unicodedata.normalize("NFD", plain)
    if kind == "nfkd":
        return unicodedata.normalize("NFKD", plain)
    if kind == "empty":
        return ""
    if kind == "other":
        return r.choice([plain_answer(r), r.choice(WORDS), " ".join(pick_words(r, 2))])
    if kind == "spaces":
        return r.choice([" " + plain, plain + " ", plain.replace(" ", ""), plain.replace(" ", "  "), " ".join(plain.split(" "))])
    if kind == "cut":
        return plain[: r.randint(0, max(0, len(plain) - 1))]
    if kind == "double":
        return plain + r.choice(["", " "]) + plain
    if kind == "multi":
        out = plain
        for _ in range(r.randint(2, 4)):
            out = mutate(r, out) or out
        return out
    if not chars:
        return r.choice(LETTERS)
    i = r.randrange(len(chars))
    if kind == "drop":
        del chars[i]
    elif kind == "add":
        chars.insert(i, r.choice(LETTERS + "".join(MARKS)))
    elif kind == "swap" and len(chars) > 1:
        j = min(i + 1, len(chars) - 1)
        chars[i], chars[j] = chars[j], chars[i]
    else:
        chars[i] = r.choice(LETTERS)
    return "".join(chars)


def scenario(index: int, pairs: list[list[str]]) -> dict:
    return {
        "id": f"render/type-answer/corpus/compare-{index:02d}",
        "description": f"Generated type-in comparisons, part {index} of {FILES} (corpora/type-answer/generate.py).",
        "tags": ["generated"],
        "clock": {"tz": "UTC", "start": "2030-01-15T10:00:00"},
        "steps": [{"observe": [{"type_answer": {"fns": ["compare", "compare_nc"], "inputs": pairs}}]}],
    }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--out", default="conformance/scenarios/render/type-answer/corpus")
    args = parser.parse_args()
    r = random.Random(SEED)
    seen: set[tuple[str, str]] = set()
    pairs: list[list[str]] = []
    while len(pairs) < FILES * PAIRS:
        plain = plain_answer(r)
        expected = expected_field(r, plain)
        typed = mutate(r, plain)
        key = (expected, typed)
        if key in seen:
            continue
        seen.add(key)
        pairs.append([expected, typed])
    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    for i in range(FILES):
        chunk = pairs[i * PAIRS : (i + 1) * PAIRS]
        path = out / f"compare-{i + 1:02d}.json"
        path.write_text(json.dumps(scenario(i + 1, chunk), ensure_ascii=False, indent=1) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
