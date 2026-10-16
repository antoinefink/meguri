#!/usr/bin/env python3
# SPDX-License-Identifier: MPL-2.0
"""Writes the render area's template corpus as generated scenarios.

    python3 -B -I conformance/corpora/templates/generate.py [--out conformance/scenarios/render/corpus]

Output (dsl.md D1, generated `.json` scenarios with oracle goldens):
  templates-01.json .. templates-20.json  4 observe steps each, one `template`
                                          request per step (one per note type
                                          below), 50 new-note cases per request
                                          (specs/conformance/render-ops.md Q1)

That is 4,000 cases, all distinct. Templates mix text, tags in both delimiter
modes, comments, sections on fields, special names and card-number keys,
filter chains of built-in, empty and unknown names, stray braces and the odd
structural mistake; field values mix markup, entities, ruby text, sound tags,
spacer tags and white space. Cloze filters are left out (U61 owns them).

The cases come from a seeded generator: the same Python version writes the
same files (3.13.1 wrote the committed ones). Never edit the output by hand;
regenerate, then re-record the goldens with `tools/mg oracle record
render/corpus`.
"""

from __future__ import annotations

import argparse
import json
import random
from pathlib import Path

SEED = 20261015
FILES, CASES = 20, 50

NOTETYPES = [
    {"name": "Corpus Basic", "fields": ["Front", "Back", "Extra"]},
    {"name": "Corpus Words", "fields": ["Word", "Reading", "My Field", "日本語"]},
    {"name": "Corpus Shadow", "fields": ["Front", "Tags", "c2", "FrontSide"]},
    {"name": "Corpus Single", "fields": ["Text"]},
]
SPECIALS = ["Tags", "Type", "Deck", "Subdeck", "Card", "CardFlag", "CardID", "FrontSide"]
CARD_KEYS = ["c1", "c2", "c3", "c0", "c01", "c+1", "c500"]
BAD_KEYS = ["Nope", "front", "C1", "", " Front", "#", "text:Front"]
FILTERS = [
    "text", "furigana", "kana", "kanji", "hint", "tts en_US", "tts ja_JP voices=Kyoko speed=0.9",
    "type", "type-nc", "type-cloze", "nc", "ci", "editable", "edit", "", "Text", "custom",
]
TEXTS = [
    "", " ", "Q: ", "<div class=front>", "</div>", "<br>", "<hr id=answer>", "\n", "日本", "x[y]",
    "}}", "{", "}", "%>", "-->", ":", "#", "^", "&nbsp;", "\U0001f600", "<b>", "</b>",
    "<script>var a = {b: 1};</script>", "[sound:q.mp3]",
]
ODDITIES = ["{{Front", "<% Front", "{{", "<%", "<!--", "text }} more", "{{=<% %>=}}"]
VALUE_PIECES = [
    "", "word", "Back text", "<b>bold</b>", "<br>", " ", "&nbsp;", "&amp;", "<div></div>",
    " 日本[にほん]語[ご]", "&nbsp;私[わたし]", "a>b[c]",
    "[sound:a.mp3]", "語[sound:b.mp3]", "x[]]", "\U0001f600", "\n", "<span></span>", "{{Front}}",
    "}}", "<!-- c -->", "&bogus;", "&#x41;", "　", "é", "<img src=\"a.png\">", "\t",
    "<BR/>", " ", "<i>it</i> &lt;tag&gt;",
]
TAGS = ["alpha", "beta", "日本語", "x::y"]
TEMPLATE_NAMES = ["Reverse", "カード", "Card A"]


class Template:
    """One template text under construction, in one delimiter mode."""

    def __init__(self, r: random.Random, fields: list[str], alt: bool) -> None:
        self.r = r
        self.fields = fields
        self.open, self.close = ("<%", "%>") if alt else ("{{", "}}")

    def key(self, section: bool = False) -> str:
        p = self.r.random()
        if p < (0.60 if section else 0.78):
            return self.r.choice(self.fields)
        if p < (0.80 if section else 0.96):
            return self.r.choice(SPECIALS)
        if p < 0.98:
            return self.r.choice(CARD_KEYS) if section else "c1"
        return self.r.choice(BAD_KEYS)

    def pad(self) -> str:
        return self.r.choice(["", "", "", " ", "\n", "　"])

    def tag(self, content: str) -> str:
        braces = self.r.choice(["", "", "", "", "", "", "{", "{{"])
        return self.open + braces + self.pad() + content + self.pad() + self.close

    def replacement(self, key: str | None = None) -> str:
        filters = [self.r.choice(FILTERS) for _ in range(self.r.choice([0, 0, 0, 1, 1, 2, 3]))]
        text = self.tag(":".join(filters + [key if key is not None else self.key()]))
        if self.r.random() < 0.05:
            text += "}"
        return text

    def section(self, depth: int) -> str:
        key = self.key(section=True)
        while key == "":
            key = self.key(section=True)
        sign = self.r.choice(["#", "#", "^"])
        out = self.tag(sign + key) + self.nodes(depth + 1)
        p = self.r.random()
        if p < 0.98:
            out += self.tag("/" + key)
        elif p < 0.99:
            out += self.tag("/" + self.key(section=True))
        return out

    def comment(self, depth: int) -> str:
        inner = self.nodes(depth + 1) if self.r.random() < 0.6 else self.r.choice(TEXTS)
        return "<!--" + inner + ("-->" if self.r.random() < 0.9 else "")

    def nodes(self, depth: int) -> str:
        out = []
        for _ in range(self.r.randint(1, 4 if depth < 2 else 2)):
            p = self.r.random()
            if p < 0.40:
                out.append(self.replacement())
            elif p < 0.70:
                out.append(self.r.choice(TEXTS))
            elif p < 0.88 and depth < 3:
                out.append(self.section(depth))
            elif p < 0.96:
                out.append(self.comment(depth))
            elif p < 0.98:
                out.append("<!-- " + self.replacement(self.r.choice(self.fields)))
            else:
                out.append(self.r.choice(ODDITIES))
        return "".join(out)


def template(r: random.Random, fields: list[str], answer: bool) -> str:
    alt = r.random() < 0.08
    t = Template(r, fields, alt)
    body = t.nodes(0)
    if not answer and r.random() < 0.7:
        body = t.replacement(r.choice(fields)) + body
    if answer and r.random() < 0.6:
        side = "{{FrontSide}}" if not alt else "<%FrontSide%>"
        body = side + r.choice(["<hr id=answer>", "", "\n"]) + body
    if alt:
        body = r.choice(["", " ", "\n  "]) + "{{=<% %>=}}" + body
    return body


def value(r: random.Random) -> str:
    return "".join(r.choice(VALUE_PIECES) for _ in range(r.choice([0, 1, 1, 2, 2, 2, 3, 3])))


def case(r: random.Random, fields: list[str]) -> dict:
    c: dict = {"front": template(r, fields, False), "back": template(r, fields, True)}
    ord = r.choice([0, 0, 0, 0, 0, 0, 1, 1, 2, 499])
    if ord:
        c["ord"] = ord
    if r.random() < 0.15:
        c["name"] = r.choice(TEMPLATE_NAMES)
    c["fields"] = [value(r) for _ in fields]
    tags = [t for t in TAGS if r.random() < 0.25]
    if tags:
        c["tags"] = tags
    return c


def scenario(n: int, steps: list) -> dict:
    return {
        "id": f"render/corpus/templates-{n:02d}",
        "description": f"Generated templates and field values, part {n} of {FILES} (corpora/templates/generate.py).",
        "tags": ["generated"],
        "clock": {"tz": "UTC", "start": "2030-01-15T10:00:00"},
        "steps": steps,
    }


def write(path: Path, value: dict) -> None:
    text = json.dumps(value, ensure_ascii=False, indent=1)
    path.write_text(text + "\n", encoding="utf-8")


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", default=str(Path(__file__).resolve().parents[2] / "scenarios" / "render" / "corpus"))
    out = Path(ap.parse_args().out)
    out.mkdir(parents=True, exist_ok=True)
    r = random.Random(SEED)
    seen: set[str] = set()
    for n in range(1, FILES + 1):
        steps = []
        for nt in NOTETYPES:
            cases = []
            while len(cases) < CASES:
                c = case(r, nt["fields"])
                key = json.dumps([nt["name"], c], ensure_ascii=False, sort_keys=True)
                if key not in seen:
                    seen.add(key)
                    cases.append(c)
            steps.append({"observe": [{"template": {"notetype": nt, "cases": cases}}]})
        write(out / f"templates-{n:02d}.json", scenario(n, steps))


if __name__ == "__main__":
    main()
