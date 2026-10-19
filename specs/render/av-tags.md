# Audio and TTS tags of a rendered side

Spec: render/av-tags
Requirements: R525, R528, R559, R564 (what display needs), R961, R1907, R1908
Anki: 26.09.3
Scenarios: conformance/scenarios/render/av-tags/

How a rendered card side's `[sound:…]` tags and `[anki:…]…[/anki:…]` directives become play markers and an ordered list of audio entries (AV list). The card render pipeline (`card-render-pipeline.md` K4) runs this on each side of a card in review and preview after template and cloze rendering; the display transform (`display-transform.md`) later turns the markers into replay buttons. Output must equal Anki's string for string (R1907), except the one text of A6.

## A1. Where it runs

- On the whole text of one rendered side, after the template engine and the cloze filters produced it (`template-syntax.md` T7, `cloze.md` C6): field text, template text, `<script>` bodies and attribute values alike (R525). Nothing is extracted from fields before rendering.
- Each side has a **side letter**: `q` for the question, `a` for the answer.
- Browser mode extracts nothing (`card-render-pipeline.md` K5).

## A2. Items of a side

The side is read from its first character to its last as a sequence of **items**: sound tags (A3), directives (A4) and plain text.

- A sound tag or a directive can only start at a `[`. At each `[`, a sound tag is tried first, then a directive. When neither fits there, that `[` is plain text and reading goes on with the next character.
- Everything between items is plain text. An item never starts inside another item, so `[` characters inside a sound tag's name or a directive's content are never starts.

## A3. Sound tags

`[sound:` (exact, lower case), then the **name**: one or more characters, none of them `]` (line breaks, `[`, `<` and spaces allowed), then `]`.

- `[sound:]` and `[sound:]]` are not sound tags (the name would be empty). This differs from the media-reference scan of `text/media-references#M3`, which reads `[sound:]]` as a tag naming `]`; that scan serves media checks, not playback.
- `[sound:a]b]` is the tag `a` followed by the text `b]`. `[[sound:x]]` is the text `[`, the tag `x`, the text `]`.

## A4. Directives

`[anki:` + NAME + OPTIONS + `]` + CONTENT + `[/anki:` + NAME + `]`, where:

- **NAME**: one or more characters, none of them `]`, U+0020, TAB, CR or LF. Case-sensitive; it may hold `:`, `=`, `"` and `/` (`[anki:x=y]a[/anki:x=y]` is a directive named `x=y`).
- **OPTIONS**: a run (possibly empty) of the white space characters U+0020, TAB, CR and LF; then zero or more options, each followed by such a run. An option is KEY `=` VALUE:
  - KEY: one or more characters, none of them `]`, `=` or one of the four white space characters (`"` is allowed);
  - VALUE: when it starts with `"` and another `"` follows somewhere, the text strictly between those two quotes (`]`, `=` and white space allowed, `"` not); otherwise the longest run, possibly empty, of characters other than `]`, `"` and the four white space characters.
  Options are read one after another, as many as fit. After the last one that fits, the next character must be `]`, or there is no directive at this `[`. So `[anki:foo a]`, `[anki:tts lang= ja]` (the second part ` ja` has no `=`) and `[anki:tts lang=en"]` are not directives.
- **CONTENT**: everything after the `]` that ends the options, up to the first occurrence of `[/anki:` + NAME + `]`. It may be empty and may hold anything, including other directives and sound tags, which are not read as items. When no such closing text follows, there is no directive at this `[`.
- The closing text matches NAME exactly: `[anki:tts …]x[/anki:TTS]` does not close.

Examples: `[anki:tts lang=en][anki:tts lang=fr]y[/anki:tts]z[/anki:tts]` is one directive whose content is `[anki:tts lang=fr]y`, then the text `z[/anki:tts]`; `[anki:foo]x` with no closing text is plain text.

## A5. Markers and numbering

Extraction writes each item of A2 back in order:

- a sound tag becomes a **play marker** and adds a sound entry to the side's AV list;
- a directive named exactly `tts` (a **TTS directive**) becomes a play marker and adds a TTS entry, unless its language is empty (A6);
- any other directive is written back in its normal form (A7);
- plain text is written unchanged.

The play marker is `[anki:play:` + side letter + `:` + index + `]`, the index in decimal without leading zeros. Indexes start at 0 on each side and count the entries added so far on that side, in reading order. So the first answer sound is `[anki:play:a:0]` however many sounds the question has.

## A6. Entries

- **Sound entry**: the tag's name after entity decoding (`text/strip-html#H3`, all or nothing): `[sound:a&amp;b.mp3]` gives `a&b.mp3`, `[sound:&nbsp;x.mp3]` gives ` x.mp3`, `[sound:a&b&c.mp3]` stays `a&b&c.mp3`. Other characters are kept, line breaks and `/` included.
- **TTS entry**: its **language** is the VALUE of the last option whose KEY is exactly `lang` (`[anki:tts lang=en lang=fr]` has language `fr`; quotes removed, `lang="en US"` gives `en US`); its **content** is the directive's CONTENT as written, markup included.
- **TTS directive with an empty language** (no `lang` option, or `lang=`, as `{{tts :Field}}` produces): it adds no entry and does not advance the index. Anki writes a localized error message between `[` and `]` in its place; Meguri writes the fixed text `[tts: lang not set]` (R1887: no localized text is compared; `specs/conformance/render-ops.md` Q5.4 maps Anki's message to this text).
- Other TTS options (`voices`, a comma-separated list; `speed`, a number, 1.0 when missing or unreadable; `cloze_blank`; any other key) only configure speech. Meguri does not speak (R564) and keeps none of them.

What the entries are for: the review layer autoplays a side's list and the replay buttons index into it (U67). TTS entries play nothing but hold their index, so later sound indexes match Anki's (R559). A sound entry plays the media file named by the part of the name after its last `/` (the whole name when it has none), as Anki plays only base names from the media folder (U67).

## A7. Other directives in normal form

A directive whose NAME is not exactly `tts` is written back as `[anki:` + NAME, then one ` KEY=VALUE` per distinct KEY in order of first appearance (VALUE being the last value given for that KEY, between `"` when it contains `]`, U+0020, TAB, CR or LF), then `]` + CONTENT + `[/anki:` + NAME + `]`.

| Input | Output |
|---|---|
| `[anki:foo  a=1` LF `]x[/anki:foo]` | `[anki:foo a=1]x[/anki:foo]` |
| `[anki:foo a="x y" ]x[/anki:foo]` | `[anki:foo a="x y"]x[/anki:foo]` |
| `[anki:foo a=x]]y[/anki:foo]` | unchanged (content `]y`) |
| `[anki:TTS lang=en]up[/anki:TTS]` | unchanged (not a TTS directive: no entry) |

With two or more distinct keys Anki writes the options in an order that changes from run to run, so no order is Anki's; scenarios keep at most one option on such directives.

## A8. FrontSide and the answer (R528, R961)

- The answer template's `{{FrontSide}}` receives the question text after this extraction, play markers included (`special-fields.md` P4, `card-render-pipeline.md` K4).
- When the answer side is read, a `[anki:play:q:N]` marker is plain text (it has no closing text), so it stays as it is and adds nothing to the answer's list. The answer list holds only the answer's own items, which is why the answer side never autoplays question audio (R961), while its FrontSide part still shows the question's replay buttons (R528, `display-transform.md` D5).
- The question's normal-form directives and the A6 text pass through the answer's reading unchanged.

## A9. Clozes

The active cloze on the front encodes `[`, `:` and `]` in its `data-cloze` attribute (`cloze.md` C7), so a sound inside it is no tag on the front; on the back the active cloze shows its text and its sounds are entries. Inactive clozes show their text on both sides, so their sounds are entries on both sides.

| Text field, card 1, template `{{cloze:Text}}` both sides | Question list | Answer list |
|---|---|---|
| `{{c1::犬[sound:inu.mp3]}} {{c2::猫[sound:neko.mp3]}}` | `neko.mp3` | `inu.mp3`, `neko.mp3` |

## A10. Examples

| Side text (question side) | Output | AV list |
|---|---|---|
| `a [sound:x.mp3] b [sound:y.mp3]` | `a [anki:play:q:0] b [anki:play:q:1]` | sound `x.mp3`, sound `y.mp3` |
| `<script>var s='[sound:s.mp3]';</script><img alt="[sound:t.mp3]">` | `<script>var s='[anki:play:q:0]';</script><img alt="[anki:play:q:1]">` | `s.mp3`, `t.mp3` |
| `[anki:tts lang=ja_JP]猫[/anki:tts][sound:z.mp3]` | `[anki:play:q:0][anki:play:q:1]` | TTS `ja_JP` (content `猫`), sound `z.mp3` |
| `[anki:tts]foo[/anki:tts][sound:z.mp3]` | `[tts: lang not set][anki:play:q:0]` | sound `z.mp3` |
| `[anki:tts lang=en]a[sound:in.mp3]b[/anki:tts]` | `[anki:play:q:0]` | TTS `en` (content `a[sound:in.mp3]b`) |
| `[sound:]] [sound:a]b]` | `[sound:]] [anki:play:q:0]b]` | sound `a` |
| `[anki:play:q:0] [anki:play:a:3]` (literal) | unchanged | none |

## A11. Notes for implementers

- One left-to-right pass with bounded look-ahead suffices, but a literal reading of A4's CONTENT rule (search for the closing text from every `[anki:`) is quadratic on inputs with many unclosed directives of distinct names; keep it close to linear (`template-syntax.md` T13 has the same concern), since sides can be megabytes (R1805).
- No input panics; any valid UTF-8 is accepted.

## A12. Scenarios

- `render/av-tags/numbering`: per-side numbering, sounds in a `<script>` string and an attribute value, FrontSide markers on the answer with the answer's own list (R525, R528).
- `render/av-tags/tts`: `{{tts}}` and literal TTS directives keep their index (R559), the empty-language case, a sound inside TTS content, a directive named `TTS`.
- `render/av-tags/grammar`: A3, A4 and A7 edge cases, entity decoding of names, literal markers in fields.
- `render/av-tags/cloze`: A9, and a TTS filter over a cloze.
- Observation kind: `render` (`specs/conformance/render-ops.md` Q5).
