# Template filters

Spec: render/filters
Requirements: R529, R956, R962, R964, R965, R966, R967, R968, R1907, R1908, R1909
Anki: 26.09.3
Scenarios: conformance/scenarios/render/filters/, conformance/scenarios/render/corpus/

What each filter of a replacement does to the text, and how a chain of filters is applied. Splitting `{{f2:f1:Field}}` into a key and filter names is `template-syntax.md` T5; `FrontSide` and the empty key ignore filters entirely (T7 rules 1 and 2). Output must equal Anki's byte for byte (R1907).

## F1. Applying a chain

The **chain** is the list of filter names in application order: nearest the key first, so `{{f2:f1:Front}}` is `f1`, `f2`.

1. **Type rewrite.** If the last filter of the chain (the leftmost name in the tag) is exactly `type`, the chain is replaced before anything runs:
   - by the single filter `type-cloze` when the chain is exactly `cloze`, `type` (`{{type:cloze:F}}`);
   - by the single filter `type-nc` when it is exactly `nc`, `type` (`{{type:nc:F}}`);
   - otherwise by the single filter `type`, dropping every other name (`{{type:text:F}}`, `{{type:nc:ci:F}}`, `{{type:edit:F}}` and `{{type::F}}` all become `type`).
2. **Run.** Starting from the field's text, take the filters in order:
   - the empty name changes nothing (`{{::F}}`);
   - a built-in name (F2) replaces the text with its output;
   - any other name **ends the chain**: the output is the text as it is at that point, and no later filter applies, built in or not.

So `{{edit:furigana:F}}` applies furigana and gives the ruby markup, while `{{furigana:edit:F}}` gives the field's raw text. Anki hands the rest of such a chain to add-ons; with none installed (the oracle, and Meguri, which has no add-ons) nothing happens to it.

## F2. Built-in names

Exact, case-sensitive: `text` (F3), `furigana`, `kana`, `kanji` (F4), `hint` (F5), any name that begins with the four characters `tts` + U+0020 (F6), `type`, `type-nc`, `type-cloze` (F7), `cloze` and `cloze-only` (F8). Everything else is unknown, including `Text`, `tts` without the space, `tts-voices`, `nc` and `ci` on their own, `editable`, `edit` and `clickable` (R968).

## F3. `text`

The text through `text/strip-html#H4` (markup removed, then entities decoded all or nothing, then U+00A0 to space). `<b>a&amp;b</b>` gives `a&b`; `<i>x</i> &bogus;` gives `x &bogus;`.

## F4. `furigana`, `kana`, `kanji`

1. Every occurrence of the exact text `&nbsp;` (lower case; `&NBSP;` and `&#160;` stay) becomes one U+0020. This happens to the whole text, also where no group follows.
2. **Ruby groups** are found left to right, without overlap. A group is:
   - an optional single U+0020;
   - the **base**: one or more characters, none of them U+0020 or `>` (line breaks, `<`, `[` and `]` are allowed);
   - `[`;
   - the **reading**: one or more characters, none of them U+000A;
   - `]`.

   Among the groups that could start at the current position or later, the one that starts earliest is taken. At that start the optional space is taken when it is there; the base is the shortest one that a `[`, a valid reading and a `]` can follow; the reading is the shortest valid one after that base. So in `a[` LF `]b[c]` the base is `a[` LF `]b` and the reading `c`, and in `a[b[c]` the base is `a` and the reading `b[c`. The search goes on after the group's `]`.
3. A group whose reading starts with `sound:` is left exactly as it is, space included. Every other group is replaced, its leading space dropped, by:
   - `furigana`: `<ruby><rb>` base `</rb><rt>` reading `</rt></ruby>`;
   - `kana`: the reading;
   - `kanji`: the base.

Base and reading are inserted as they are, with no escaping. Examples (furigana / kana / kanji):

| Input | `furigana` | `kana` | `kanji` |
|---|---|---|---|
| ` 日本[にほん]語[ご]` | `<ruby><rb>日本</rb><rt>にほん</rt></ruby><ruby><rb>語</rb><rt>ご</rt></ruby>` | `にほんご` | `日本語` |
| `&nbsp;私[わたし]` | `<ruby><rb>私</rb><rt>わたし</rt></ruby>` | `わたし` | `私` |
| `a>b[c]` | `a><ruby><rb>b</rb><rt>c</rt></ruby>` | `a>c` | `a>b` |
| `abc日本[にほん]` | base `abc日本` | `にほん` | `abc日本` |
| `a  b[c]` (two spaces) | `a ` + ruby of `b`, `c` | `a c` | `a b` |
| `<b>漢字[かんじ]</b>` | `<b><ruby><rb>漢字</rb><rt>かんじ</rt></ruby></b>` | `<b>かんじ</b>` | `<b>漢字</b>` |
| `[sound:x.mp3]`, `語[sound:x.mp3]`, ` 語[sound:x.mp3]` | unchanged | unchanged | unchanged |
| `a&nbsp;b` | `a b` | `a b` | `a b` |
| `x[]`, `a` LF `b[c` LF `d]` | unchanged | unchanged | unchanged |
| `x[]]` | ruby of `x`, `]` | `]` | `x` |
| `a` LF `b[c]` | ruby of `a` LF `b`, `c` | `c` | `a` LF `b` |
| `&NBSP;x[y]` | ruby of `&NBSP;x`, `y` | `y` | `&NBSP;x` |

## F5. `hint`

- If the text with Unicode White_Space removed at both ends is empty, the output is the text unchanged (empty stays empty, a lone space stays a space).
- Otherwise the output is the markup below, where `KEY` is the replacement's key as written in the tag (T5), `TEXT` the current text, both inserted as they are, and `ID` 16 lower-case hexadecimal digits: the first 8 bytes of the BLAKE3 hash (default mode, the pinned `blake3` 1.8.2 crate) of the UTF-8 bytes of `TEXT` immediately followed by the UTF-8 bytes of `KEY`.

The markup is one U+000A followed by these six lines, each ended by U+000A (so it starts with an empty line and ends with a line break):

```

<a class=hint href="#"
onclick="this.style.display='none';
document.getElementById('hintID').style.display='block';
return false;" draggable=false>
KEY</a>
<div id="hintID" class=hint style="display: none">TEXT</div>
```

`hintID` means the letters `hint` followed directly by `ID`. Examples: text `hello` with key `Front` gives `ID` `eb589297cb31b693`; text `b` with key `Back` gives `90eb8a52408c5123`. The hash covers the current text, so `{{hint:text:F}}` hashes the stripped text.

## F6. `tts`

A filter whose name is `tts` + U+0020 + `OPTIONS` (anything up to the next `:`, kept exactly, possibly empty or starting with more spaces) outputs `[anki:tts lang=` + `OPTIONS` + `]` + text + `[/anki:tts]`.

- `{{tts ja_JP voices=Apple_Kyoko speed=0.8:F}}` with `こんにちは` gives `[anki:tts lang=ja_JP voices=Apple_Kyoko speed=0.8]こんにちは[/anki:tts]`.
- `{{tts  ja_JP:F}}` (two spaces) gives `[anki:tts lang= ja_JP]…`; `{{tts :F}}` gives `[anki:tts lang=]…`.
- `{{tts:F}}` and `{{TTS en:F}}` are unknown filters (F1).
- Options cannot hold `:` (it splits the tag): `{{tts en_US voices=a:b:F}}` is the unknown filter `b` followed by `tts en_US voices=a`, and `b` ends the chain.

What review does with this markup (R967: show the text, do not speak it) is the render pipeline's (U62).

## F7. Type-in markers

- `type` outputs `[[type:` + `KEY` + `]]`; `type-nc` outputs `[[type:nc:` + `KEY` + `]]`; `type-cloze` outputs `[[type:cloze:` + `KEY` + `]]`. `KEY` is the replacement's key as written; the current text is discarded. U64 later turns the marker into the answer box and the comparison.
- After F1's rewrite, a chain whose outermost filter is `type` is a single marker filter. `type`, `type-nc` and `type-cloze` met elsewhere in a chain are ordinary built-in filters: later filters see the marker as text. `{{text:type:F}}` gives `[[type:F]]`; `{{furigana:type:F}}` gives `<ruby><rb>[</rb><rt>type:F</rt></ruby>]`; `{{type-cloze:text:F}}` gives `[[type:cloze:F]]`; `{{nc:type:F}}` gives `[[type:F]]` (then `nc` ends the chain).
- `{{type:}}` has an empty key: no marker, no output (T7 rule 2). `{{type:FrontSide}}` is the FrontSide text (T7 rule 1).

| Tag | Output |
|---|---|
| `{{type:F}}`, `{{type:text:F}}`, `{{type:unknown:F}}`, `{{type:type:F}}` | `[[type:F]]` |
| `{{type:nc:F}}`, `{{type-nc:F}}` | `[[type:nc:F]]` |
| `{{type:cloze:F}}`, `{{type-cloze:F}}` | `[[type:cloze:F]]` |
| `{{type:ci:F}}`, `{{type:nc:ci:F}}`, `{{type:ci:nc:F}}`, `{{type:cloze:nc:F}}` | `[[type:F]]` |
| `{{Type:F}}` | the field's text (`Type` is an unknown filter) |

## F8. `cloze` and `cloze-only`

Built in (they never end a chain). What they output depends on the card ordinal and on the side, and is specified with cloze syntax in `specs/render/cloze.md` (U61). This unit's scenarios do not use them.

## F9. Differences from the Product Contract's wording

- R968 says later filters still apply after an unknown one. In Anki 26.09.3 no filter applied after an unknown one has any effect (F1). Meguri follows the oracle (R1907). The common add-on form, with the unknown filter outermost (`{{editable:furigana:F}}`, `{{edit:F}}`, `{{clickable:F}}`), is unaffected.
- R964 asks for a `ci` modifier and modifiers combined in any order. Anki 26.09.3 has no `ci`: every chain other than exactly `type:nc` and `type:cloze` gives the plain `[[type:KEY]]` marker (F7). Meguri's engine emits Anki's marker. The parse result keeps every replacement's filter names (`template-syntax.md` T12), so U64 can read the modifiers R964 lists from the template; any typing behavior Meguri adds on top is U64's, with its deviation entry.
- R965 says empty text gives nothing: text that is only white space comes back unchanged (F5).

## F10. Scenarios

- `filters/chains`: F1 order, empty names, unknown filters at every position, `editable`, `edit`, `clickable`.
- `filters/text`, `filters/furigana`, `filters/hint`, `filters/tts`, `filters/type`: F3–F7.
- `corpus/templates-*`: random chains over random field texts.
- Observation kind: `template` (`specs/conformance/render-ops.md` Q1).
