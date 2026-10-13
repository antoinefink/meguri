# HTML stripping

Spec: text/strip-html
Requirements: R773, R774, R1457
Anki: 26.09.3
Scenarios: conformance/scenarios/text/strip/, conformance/scenarios/text/corpus/

How field HTML becomes plain text. The stored sort field and checksum (`checksum-and-sort-field.md`), the duplicate check, browser cells and Card Info all depend on this text, so it must be identical to Anki's for every input, malformed ones included.

## H1. Ground rules

- Stripping works on the raw text with the patterns below. Never use an HTML parser: a parser repairs malformed markup differently and would change stored `sfld` and `csum` values and duplicate results.
- Implement the patterns with the pinned `regex` crate (1.11.2, KTD2). Where this file says "shortest", "first" or "tried in order", the crate's leftmost-first semantics (the same choices a backtracking engine makes) are the reference.
- Case-insensitive means the `regex` crate's Unicode case-insensitive matching (simple case folding). Besides ASCII case, U+017F LATIN SMALL LETTER LONG S matches `s`/`S` and U+212A KELVIN SIGN matches `k`/`K`. So `<ſtyle>` (with U+017F) opens a style block. No other non-ASCII character matches the ASCII letters these patterns use.
- "Any character" includes line breaks.
- A word character is a Unicode word character as the `regex` crate defines `\w`: alphabetic characters, marks, decimal digits, connector punctuation and the join controls U+200C and U+200D.
- Unicode whitespace is the White_Space property (as the `regex` crate's `\s` and Rust's `str::trim` use it): it includes U+00A0, U+3000 and U+2028; ASCII whitespace is U+0009, U+000A, U+000B, U+000C, U+000D and U+0020.

## H2. Markup removal

Scan left to right. At each `<`, try these forms in order; the first that matches at that position is removed (replaced by nothing) and scanning resumes after it:

1. **Comment**: `<!--`, then the shortest run of any characters, then `-->`. (`<!---->` and `<!-->b-->` are complete comments.)
2. **Style block**: `<style` (case-insensitive, no boundary required after it, so `<styles>` counts), then the shortest run up to the first `>`, then the shortest run up to the first case-insensitive `</style>` that starts after that `>`. It fails when no such `</style>` exists.
3. **Script block**: the same with `script` and `</script>`.
4. **Tag**: `<`, then the shortest run of any characters (possibly empty), then `>`. So the tag ends at the first `>` after the `<`, whatever lies between (quotes, newlines, other `<`).

A `<` where none of the forms matches (no `>` anywhere after it) is kept as text, and so is everything after it that no later match removes. A `>` outside a match is kept.

Consequences worth testing: `a < b > c` → `a  c`; `a<b<c>d` → `ad`; `<<em>>` → `>`; `x<style>no end` → `xno end` (only `<style>` goes); `<!-- open > tail` → ` tail`; `<img alt="q>r" src="s.png">` → `r" src="s.png">`.

This step alone (without H3) is the "markup removal" that cloze processing reuses (U61).

## H3. Entity decoding

- A text without `&` is returned unchanged.
- Otherwise the whole text is decoded at once, all or nothing: when every `&` in it starts a valid entity, each entity is replaced by its character and then every U+00A0 in the result becomes U+0020 (including U+00A0 characters that were in the text literally). When any `&` does not start a valid entity, the text is returned exactly as given: no entity is decoded and no U+00A0 is replaced.
- Valid entities, `&` included:
  - named: `&` + a name from the HTML 4 entity set (252 names, case-sensitive: `amp`, `lt`, `gt`, `quot`, `nbsp`, `eacute`, `Eacute`, `euro`, `hellip`, ...) + `;`. `&apos;` is not in the set. Names are not terminated by anything but `;`: in `&amp b;` the name read is `amp b`.
  - decimal: `&#` + one or more ASCII digits + `;`, whose value is a Unicode scalar value. Leading zeros are fine; 0 gives U+0000.
  - hexadecimal: `&#x` (lower-case `x` only) + one or more hex digits of either case + `;`, with the same value rule.
  - Invalid: a lone `&`, `& ` or `&;`, an unknown or wrongly cased name (`&AMP;`, `&EAcute;`), `&#;`, `&#x;`, `&#X41;`, surrogates (`&#55296;`, `&#xD800;`), values above U+10FFFF or too large to parse, and an entity with no `;` before the end of the text.
- The pinned `htmlescape` 0.3.1 decoder (`decode_html`) implements exactly this grammar and reports any invalid entity as an error; use it and treat its error as "return the text unchanged".

Examples: `salt &amp; pepper` → `salt & pepper`; `rock & roll` → `rock & roll` (nothing to decode); `ok &amp; then &unknown; later` → unchanged (`&amp;` stays); `&#0;z` → U+0000 then `z`; `&amp;nbsp;` → `&nbsp;` (one pass only); `raw<U+00A0>nbsp &amp; x` → the U+00A0 becomes a space; `raw<U+00A0>nbsp &bogus; x` → unchanged.

## H4. Plain stripping

H2 on the input, then H3 on the result. No trimming, no whitespace collapsing.

## H5. Stripping that keeps media names

1. Replace every media tag match (`media-references.md` M2), scanning the input left to right, by U+0020 + its raw name + U+0020. The raw name is the attribute value as written, entities not yet decoded.
2. Apply H4 to the result.

Sound tags (`[sound:...]`) are plain text here and stay. Because the name goes through H4 with the rest of the text, markup inside a name is removed (`<img src="a<b>.jpg">` → ` a.jpg `) and its entities are decoded only when the whole text decodes (`<img src="a&amp;b.png"> &bogus;` → ` a&amp;b.png  &bogus;`). Example: `<img src="a b.jpg">text` → ` a b.jpg text`.

## H6. Text line

The one-line text that tables and lists show (browser cells, Card Info, CSV export). Two variants: media names dropped and media names kept. Steps, in order:

1. Replace with one U+0020 each match of, scanning left to right: `<br`, then any run of Unicode whitespace, then an optional `/`, then `>` (case-insensitive: `<br>`, `<BR/>`, `<br />`, `<br  >`, `<br` + TAB + `/>`); the exact text `<div>` (case-insensitive, no attributes or spaces); and each U+000A (U+000D is not replaced).
2. Remove each type-answer marker: `[[type:`, then one or more characters other than `]`, then `]]` (case-sensitive).
3. Sound tags: `[sound:` (case-sensitive), then one or more characters other than `]`, then `]`. Media names kept: the tag becomes its name, with no added spaces. Media names dropped: the tag is removed. This sound form is narrower than the reference scan's (`media-references.md` M3): `[sound:]x]` is not a sound tag here.
4. Apply H5 (media names kept) or H4 (media names dropped).
5. Remove Unicode whitespace at both ends. Inner runs are kept as they are.

Examples: `[sound:a b.mp3]c` → `c` (dropped) or `a b.mp3c` (kept); `<img src="pic.png">[sound:say.mp3]` → empty (dropped) or `pic.png say.mp3` (kept); `<div>block</div><div>block</div>` → `block block`; `first` + LF + LF + `second` → `first  second`; `&nbsp;` → empty.

## H7. Scenarios

- `strip/markup`, `strip/entities`, `strip/keep-media`, `strip/text-line`: the cases above and their neighbours.
- `corpus/fields-01` … `corpus/fields-40`: 48,000 distinct generated field texts through H4, H5 and both H6 variants (with the other text functions).
- Observation functions (`specs/conformance/observations.md` O8): `strip_markup` (H4), `strip_markup_keep_media` (H5), `text_line` and `text_line_keep_media` (H6).
