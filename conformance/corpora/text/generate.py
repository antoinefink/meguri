#!/usr/bin/env python3
# SPDX-License-Identifier: MPL-2.0
"""Writes the text area's differential corpus as generated scenarios.

    python3 -B -I conformance/corpora/text/generate.py [--out conformance/scenarios/text/corpus]

Output (dsl.md D1, generated `.json` scenarios with oracle goldens):
  fields-01.json .. fields-40.json  1,200 field-like HTML inputs each, run through
                                    every string function of the `text` kind but
                                    media_filename (observations.md O9)
  filenames-01.json                 2,000 media file names (media_filename)
  unicase-01.json                   2,000 string pairs (unicase_compare)

That is 52,000 inputs, all distinct within their kind: U20's 50,000-case
differential corpus counts distinct inputs, not input-function pairs. A
generator draw that repeats an earlier input is drawn again.

The inputs come from a seeded generator: the same Python version writes the
same files (3.13.1 wrote the committed ones). Never edit the output by hand;
regenerate, then re-record the goldens with `tools/mg oracle record
text/corpus`. Case variants come from Python's own Unicode tables, so another
Python version may write other pairs.
"""

from __future__ import annotations

import argparse
import json
import random
from pathlib import Path

SEED = 20261013
FIELD_FILES, FIELD_INPUTS = 40, 1200
NAME_INPUTS, PAIR_INPUTS = 2000, 2000

FIELD_FNS = [
    "strip_markup", "strip_markup_keep_media", "text_line", "text_line_keep_media", "media_refs",
    "iri_encode", "iri_decode", "field_empty", "sort_field", "checksum",
]

WORDS = ["sun", "moon", "Tokyo", "deck", "card", "a", "I", "x", "TEST", "rain", "cat's", "end."]
NUMBERS = ["42", "007", "1.5", "-3", "+8", "1e3", "3.0e+2", "0x1F", " 12 ", "1,5", "12abc", "9999999999999999999", "Inf"]
SCRIPTS = [
    "\u65e5\u672c\u8a9e", "\u6f22\u5b57", "\u3072\u3089\u304c\u306a", "\u30ab\u30bf\u30ab\u30ca", "\u79c1",
    "\ud55c\uad6d\uc5b4", "\u1100\u1161\u11a8", "\u0627\u0644\u0639\u0631\u0628\u064a\u0629", "\u05e2\u05d1\u05e8\u05d9\u05ea",
    "\u0e20\u0e32\u0e29\u0e32\u0e44\u0e17\u0e22", "\u0939\u093f\u0928\u094d\u0926\u0940", "\u0395\u03bb\u03bb\u03b7\u03bd\u03b9\u03ba\u03ac",
    "\u0420\u0443\u0441\u0441\u043a\u0438\u0439", "Stra\u00dfe", "\u0130stanbul \u0131", "Tie\u0302\u0301ng Vie\u0323\u0302t",
    "e\u0301", "a\u0308", "\u00e9", "\U0001f600", "\u2764\ufe0f", "\U0001f469\u200d\U0001f469\u200d\U0001f467",
    "\U0001f970", "\U0001fae0", "\U0001fabf", "\U0001fae9", "\U00031350", "\U0002ebf0", "\U0001cc00", "\U00010d40",
    "\ufb03", "\u017f", "\u212a", "\ue000",
]
SPACES = [" ", "  ", "\t", "\n", "\r\n", "\u00a0", "\u3000", "\u2028", "\u202f", "\u200b", "\x0b", "\x0c", "\r"]
CONTROLS = ["\x01", "\x7f", "\x1f", "\x00", "\x08"]
ENTITIES = [
    "&amp;", "&lt;", "&gt;", "&quot;", "&nbsp;", "&#39;", "&#x41;", "&#160;", "&#xa0;", "&eacute;", "&hellip;",
    "&euro;", "&apos;", "&#X41;", "&bogus;", "&", "&amp", "&#;", "&#0;", "&#xD800;", "&#1114112;",
    "&#128512;", "&#x1f600;", "&AMP;", "&lt", "& ",
]
TAGS = [
    "<b>", "</b>", "<i>", "</i>", "<u>", "</u>", "<br>", "<br/>", "<BR />", "<br  >", "<br\t/>", "<div>", "</div>",
    "<DIV>", "<div class=\"c\">", "<div />", "<span style=\"color:red\">", "</span>", "<p>", "</p>", "<hr>",
    "<a href=\"https://example.org/?a=1&amp;b=2\">", "</a>", "<font color=red>", "<b", "<", ">", "<>",
    "<!-- note -->", "<!--", "-->", "<!-- a > b -->", "<style>.c{color:red}</style>", "<STYLE>", "</style>",
    "<script>var a = 1 > 0;</script>", "<script>", "</script>", "</SCRIPT>", "<\u017ftyle>", "<\u017fcript>",
    "<ruby>\u6f22<rt>\u304b\u3093</rt></ruby>", "[[type:Front]]", "[[type:cloze:Text]]", "{{c1::cloze}}",
    "<table><tr><td>1</td></tr></table>", "<p\n>", "<\n", "</ div>", "< br>", "<br\u00a0>",
]
NAMES = [
    "pic.png", "a b.jpg", "voice clip.mp3", "x.svg", "clip.mp4", "\u65e5\u672c.png", "\u00e9.mp3", "e\u0301.ogg",
    "a#b.png", "a%20b.png", "100%.png", "a+b.png", "a?b=c&d.png", "a&b.png", "a&amp;b.png", "it's.png",
    "q\"q.png", "a<b>.png", "a>b.png", "back`tick.png", "tab\tname.png", "new\nline.png", "del\x7f.png",
    "nb\u00a0sp.png", "[br].png", "s]q.png", "https://ex.org/p.png", "HTTP://EX.ORG/a b.png",
    "http&#58;//x/a.png", "//cdn.example/a b.png", "data:image/png;base64,iVBORw0KGgo=", "", "src=x.png",
    "data=y.png", "&nbsp;lead.png", "%e6%97%a5.png", "%ff.png", "a%2.png", "\U0001f600.png", "\u0130.PNG",
    "very long name " + "z" * 60 + ".png", "dot.", "trailing ", " lead.png",
]
MEDIA_TAGS = ["img", "img", "img", "img", "audio", "video", "source", "object", "IMG", "Img", "\u017fource",
              "imgx", "image", "picture", "img2"]
ATTRS = ["src", "src", "src", "src", "data", "SRC", "Src", "data-src", "srcset", "xsrc", "\u017frc"]
EQUALS = ["=", "=", "=", "=", "=", "=", " =", "= ", " = "]
PRE = ["", "", "", " alt=\"a>b\"", " alt='x'", " class=c", " title=\"t\"", " controls", "\n", " alt=\"src=k.png\""]
SEP = [" ", " ", " ", " ", "\u00a0", "\n", "\t", ""]
POST = ["", "", "", " />", "/", " alt=\"z\"", " data-x=\"1\"", " title=\"a b\""]


def media_tag(r: random.Random) -> str:
    name = r.choice(NAMES)
    quote = r.choice(['"', '"', "'", ""])
    if quote and quote in name:
        quote = "'" if quote == '"' else '"'
    close = ">" if r.random() < 0.93 else ""
    return "<%s%s%s%s%s%s%s%s%s%s" % (
        r.choice(MEDIA_TAGS), r.choice(PRE), r.choice(SEP), r.choice(ATTRS), r.choice(EQUALS),
        quote, name, quote, r.choice(POST), close)


def sound_tag(r: random.Random) -> str:
    k = r.random()
    if k < 0.8:
        return "[sound:%s]" % r.choice(NAMES)
    return r.choice(["[sound:]", "[Sound:up.mp3]", "[sound:a]b]", "[sound:\nx]", "[sound:]]", "[sound:&amp;.mp3]"])


def spacer_only(r: random.Random) -> str:
    parts = ["", " ", "\t", "\n", "\r", "<br>", "<br/>", "<BR />", "<br >", "<div>", "</div>", "<Div/>", "</br>",
             "\x0b", "\x0c"]
    return "".join(r.choice(parts) for _ in range(r.randint(0, 6)))


def field_input(r: random.Random) -> str:
    k = r.random()
    if k < 0.08:
        return spacer_only(r)
    if k < 0.12:
        return r.choice(NUMBERS)
    pools = [
        (WORDS, 6), (SCRIPTS, 4), (SPACES, 3), (CONTROLS, 1), (ENTITIES, 3), (TAGS, 5),
        (None, 3),  # media tag
        (False, 1),  # sound tag
    ]
    weights = [w for _, w in pools]
    out = []
    for _ in range(r.randint(1, 8)):
        pool = r.choices([p for p, _ in pools], weights)[0]
        if pool is None:
            out.append(media_tag(r))
        elif pool is False:
            out.append(sound_tag(r))
        else:
            out.append(r.choice(pool))
        if r.random() < 0.3:
            out.append(" ")
    return "".join(out)


DEVICES = ["con", "prn", "aux", "nul", "com1", "com9", "lpt1", "lpt9", "com0", "lpt10", "console", "auxx", "nul."]
NAME_BASES = ["pic", "voice", "a", "\u65e5\u672c\u8a9e", "\u00e9t\u00e9", "e\u0301", "\u00df", "stra\u00dfe", "x" * 40,
              "\u3042" * 30, "con", "com1", "\U0001f600"]
NAME_ODD = ["[", "]", "<", ">", ":", "\"", "/", "?", "*", "^", "\\", "|", "\x01", "\x7f", "\t", "\u00a0", "\u202f",
            "\u3000", "\u0301", "\u0378", "\ufdd0", "\U0010ffff", "\ue000", "\U0001f970", "\U0001fae9", "\u08be",
            "\u0860", " ", ".", "..", "#", "%", "&", "'", "`", "~", "-", "_", "\u200b"]
EXTS = ["", ".jpg", ".png", ".mp3", ".ogg", ".tar.gz", ".abcdefghijk", ".\u00e9\u00e9\u00e9\u00e9\u00e9\u00e9",
        ".\U0001f600\U0001f600\U0001f600", ".", ". ", ".x y", ".jpeg"]


def file_name(r: random.Random) -> str:
    while True:
        # Starts with characters the transform always keeps, so no name ends up
        # empty (the oracle's media store refuses an empty name).
        first = r.choice(DEVICES) if r.random() < 0.25 else r.choice("abcdefghijklmnopqrstuvwxyz0123456789")
        parts = [first]
        for _ in range(r.randint(0, 5)):
            parts.append(r.choice(NAME_BASES) if r.random() < 0.6 else r.choice(NAME_ODD))
        stem = "".join(parts)
        if r.random() < 0.2:
            stem *= r.randint(2, 12)
        name = stem + r.choice(EXTS) + (r.choice([" ", ".", ""]) if r.random() < 0.1 else "")
        if name.lower() == name:
            return name


CASED = ("abcxyzABCXYZ0129_[`{~ \u00df\u1e9e\u0130\u0131i\u0307\u00e9\u00c9\u01c4\u01c5\u01c6\u03a3\u03c3\u03c2"
         "\u0390\u03b0\u1f80\u1f88\u0587\ufb03\ufb05\ufb13\u017f\u212a\u212b\u00e5\u13a0\uab70\u10a0\u2d00\u1c90"
         "\u10d0\U00010400\U00010428\U0001e900\U0001e922\ua7c0\ua7c1\ua7d0\U00010570\U00010597\u0149\u01f0")


def variant(r: random.Random, s: str) -> str:
    out = []
    for ch in s:
        out.append(r.choice([ch, ch.upper(), ch.lower(), ch.casefold(), ch.title()]))
    return "".join(out)


def pair(r: random.Random) -> list[str]:
    a = "".join(r.choice(CASED) for _ in range(r.randint(0, 6)))
    k = r.random()
    if k < 0.45:
        b = variant(r, a)
    elif k < 0.75:
        b = variant(r, a)
        if b:
            i = r.randrange(len(b))
            b = b[:i] + r.choice(CASED) + b[i + 1:]
    else:
        b = "".join(r.choice(CASED) for _ in range(r.randint(0, 6)))
    return [a, b]


def distinct(r: random.Random, draw, count: int) -> list:
    """`count` different values of `draw(r)`, in the order first drawn."""
    seen: set[str] = set()
    out = []
    while len(out) < count:
        value = draw(r)
        key = json.dumps(value, ensure_ascii=False)
        if key not in seen:
            seen.add(key)
            out.append(value)
    return out


def scenario(sid: str, description: str, fns: list[str], inputs: list) -> dict:
    return {
        "id": sid,
        "description": description,
        "tags": ["generated"],
        "clock": {"tz": "UTC", "start": "2030-01-15T10:00:00"},
        "steps": [{"observe": [{"text": {"fns": fns, "inputs": inputs}}]}],
    }


def write(path: Path, value: dict) -> None:
    text = json.dumps(value, ensure_ascii=False, indent=1)
    path.write_text(text + "\n", encoding="utf-8")


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", default=str(Path(__file__).resolve().parents[2] / "scenarios" / "text" / "corpus"))
    out = Path(ap.parse_args().out)
    out.mkdir(parents=True, exist_ok=True)
    r = random.Random(SEED)
    fields = distinct(r, field_input, FIELD_FILES * FIELD_INPUTS)
    for n in range(1, FIELD_FILES + 1):
        inputs = fields[(n - 1) * FIELD_INPUTS:n * FIELD_INPUTS]
        write(out / f"fields-{n:02d}.json", scenario(
            f"text/corpus/fields-{n:02d}", f"Generated field HTML, part {n} of {FIELD_FILES} (corpora/text/generate.py).",
            FIELD_FNS, inputs))
    names = distinct(r, file_name, NAME_INPUTS)
    write(out / "filenames-01.json", scenario(
        "text/corpus/filenames-01", "Generated media file names (corpora/text/generate.py).", ["media_filename"], names))
    pairs = distinct(r, pair, PAIR_INPUTS)
    write(out / "unicase-01.json", scenario(
        "text/corpus/unicase-01", "Generated string pairs for the unicase comparison (corpora/text/generate.py).",
        ["unicase_compare"], pairs))


if __name__ == "__main__":
    main()
