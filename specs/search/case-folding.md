# Letter case in search

Spec: search/case-folding
Requirements: R1036; supports R1038, R1042, R1085, R1916, R1917
Anki: 26.09.3
Scenarios: conformance/scenarios/search/folding/

How each text comparison of a search treats letter case, in Meguri's two fold modes, and where Meguri finds more than Anki. The comparisons themselves are `text-matching.md` and `fields.md`; the name terms (deck, note type, template, tag) are `names-and-ids.md`.

## C1. Fold modes

- **Unicode**: the product mode, always used outside tests. Plain, field and `sc:` searches fold case for every alphabet (R1036), and so does `nc:` (C2).
- **AsciiCompat**: reproduces Anki exactly. It exists only for tests (the Rust feature `test-hooks`): U91's differential runs against the live oracle and the matcher tests (U81 `fold_props`). Conformance runs of the scenarios against their goldens (T0, T1) always use Unicode mode, with the R1036 deviations applied (`specs/conformance/search-ops.md` S1). Decided 2026-10-27: no golden comparison runs in AsciiCompat mode, so a deviation entry needs no fold-mode condition and `conformance/dsl.md` D13 applies every matching entry as it stands.

The mode is a value of the search context (`query-model.md` Q8). It changes only the comparisons marked "fold mode" in C2.

## C2. Rule per comparison

| Comparison | Unicode mode | AsciiCompat mode (= Anki) |
|---|---|---|
| plain text (`text-matching.md` T5), `sc:` (T9), field normal mode (`fields.md` F4) | simple folding (C3) | ASCII (C6) |
| `nc:` (T8), field `nc:` mode (F6), and plain text and `sc:` under ignore accents (T6): after the combining-free transform | simple folding (C3) | ASCII (C6) |
| `re:`, `w:`, field `re:` mode (F5), all-fields names (F7) | simple folding (regular expression `(?i)`) | the same |
| field name with wildcards (F2 step 3) | simple folding (regular expression `(?i)`) | the same |
| field name without wildcards (F2 step 4) | unicase full folding (`text/unicode-primitives#U2`) | the same |
| deck, note type, template and tag names | simple folding or unicase equality, per term (`names-and-ids.md` N1) | the same |

- The first two rows depend on the mode. Meguri's Unicode results for them differ from Anki's only by the extra matches C3 allows; these are deliberate deviations (C7).
- `nc:` in Unicode mode follows the plan's default for the open question on explicit `nc:` leaves: simple folding, like plain text.
- Regular expressions fold with simple folding in Anki too, so `w:über` and `re:über` find `Über` in both modes (AE49), and so does `*:über`.

## C3. Simple case folding

Two characters are equal ignoring case when their simple case foldings are equal. The simple case folding of a character is its mapping with status C or S in CaseFolding.txt of Unicode 16.0.0; a character without such a mapping folds to itself. This is the table of `regex-syntax` 0.8.6, which the pinned `regex` crate uses for `(?i)`, so plain text and regular expressions agree. Each character folds to exactly one character, so `_` still matches one character and patterns keep their positions.

| Characters | Equal under simple folding? |
|---|---|
| `Ü`, `ü` | yes |
| `ß`, `ẞ` (U+1E9E) | yes |
| `ß`, `ss`, `SS` | no (that needs full folding) |
| `Σ`, `σ`, `ς` | yes |
| `Ǆ`, `ǅ`, `ǆ` (U+01C4–U+01C6) | yes |
| `П`, `п` | yes |
| `Ａ`, `ａ` (fullwidth) | yes |
| `Ａ`, `A` | no |
| `İ` (U+0130), `i`; `ı` (U+0131), `I` | no (Turkic mappings are not simple foldings) |
| `は`, `ハ` | no (no case) |

## C4. Non-ASCII characters equal to ASCII letters

Exactly two characters outside ASCII fold together with ASCII letters under simple folding:

- U+212A KELVIN SIGN with `k` and `K`;
- U+017F LATIN SMALL LETTER LONG S with `s` and `S`.

So even an all-ASCII search term needs folding on the stored side in Unicode mode: `k` finds a stored U+212A and `s` finds a stored `ſ`, which Anki does not. NFC turns U+212A into `K` (and U+212B ANGSTROM SIGN into `Å`) when a field is saved with normalization on, so a stored U+212A exists only in fields saved with normalization off; `ſ` has no canonical decomposition and stays.

## C5. Differential table

`search/folding/differential` (normalization off, so U+212A stays stored as typed). Notes mode; the AsciiCompat column is the oracle golden, the Unicode column is Meguri's result.

| Search | Stored text | AsciiCompat (Anki) | Unicode (Meguri) |
|---|---|---|---|
| `über` | `Über`, cloze `{{c1::Über}}` | none | both |
| `ß` | `ẞ`, `ß`, `STRASSE` | `ß` | `ẞ`, `ß` |
| `ς` | `Σ`, `ς`, `σ` | `ς` | all three |
| `привет` | `ПРИВЕТ` | none | `ПРИВЕТ` |
| `ａｂｃ` | `ＡＢＣ`, `abc` | none | `ＡＢＣ` |
| `ǆ` | `ǅ` | none | `ǅ` |
| `s` | `STRASSE`, `ſ` | `STRASSE` | both |
| `k` | U+212A | none | U+212A |
| `front:über` | `Über` | none | `Über` |
| `front:σ` | `Σ`, `ς`, `σ` | `σ` | all three |
| `sc:über` | `{{c1::Über}}` | none | it |
| `nc:привет` | `ПРИВЕТ` | none | `ПРИВЕТ` |
| `nc:σ` | `Σ`, `ς`, `σ` | `σ` | all three |
| `ss`, `nc:ss` | `STRASSE`, `ß`, `ẞ`, `ſ` | `STRASSE` | `STRASSE` |
| `abc` | `abc`, `ＡＢＣ` | `abc` | `abc` |
| `i`, `I` | `İ`, `ı` | none | none |
| `nc:i` | `İ`, `ı` | both | both |
| `ハ` / `は` | `ハ` | `ハ` / none | `ハ` / none |
| `re:σ`, `*:σ` | `Σ`, `ς`, `σ` | all three | all three |
| `re:s` | `STRASSE`, `ſ` | both | both |
| `re:k` | U+212A | it | it |
| `re:ǆ`, `re:ａｂｃ` | `ǅ`, `ＡＢＣ` | it | it |
| `w:über`, `nc:uber` | `Über`, `{{c1::Über}}` | both | both |

The rows whose two result columns differ are the second observe step of the scenario; deviation `search-unicode-differential` gives Meguri's results. `search/folding/umlaut` (AE49) and `search/folding/ignore-accents` (R1038: plain `привет` finds `Привет` with ignore accents on, `uber` finds `ÜBER` in both modes) carry their own deviations.

## C6. ASCII folding

The 26 letters `A`–`Z` equal `a`–`z`; every other character equals only itself. This is SQLite's `LIKE` without ICU, which Anki uses for plain text, `sc:`, `nc:` and normal and `nc:` field searches.

## C7. Deviations and their property

- `conformance/deviations.yaml` entries `search-unicode-umlaut`, `search-unicode-differential` and `search-unicode-ignore-accents` (requirement R1036) replace the golden's ids with Meguri's Unicode results. Conformance runs are always in Unicode mode (C1), so they always apply. AsciiCompat results are compared with Anki only by U91's differential runs, which compare with the live oracle and apply no deviation.
- Property for the matcher tests (U81 `fold_props`) and U91: for a query whose text terms are not negated, the Unicode result contains the AsciiCompat result, and every note found only in Unicode mode holds, at the matched position, a character that equals the pattern's character under C3 but not under C6. Under negation the inclusion goes the other way for that term.
