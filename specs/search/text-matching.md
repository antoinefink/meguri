# Search text matching

Spec: search/text-matching
Requirements: R1033, R1034, R1035, R1037, R1038, R1039, R1040, R1041, R1042, R1043; supports R1036, R1075, R1082, R1085
Anki: 26.09.3
Scenarios: conformance/scenarios/search/text/, conformance/scenarios/search/regex/

What the text terms match: plain text, `nc:`, `sc:`, `w:` and `re:`, and the pattern rules that field searches (`fields.md`) share. Letter case is `case-folding.md`; how terms combine, the modes and the plan are `query-model.md`; the values a term holds are `grammar.md` G7 and G8. Every rule here was checked against the oracle; the scenarios each section names pin its examples (T17), except those of `search/text/nc-escapes`, `search/text/pattern-limit-unreached` and `search/regex/glob-size`, which await their first recording. Pattern conventions (shortest match, "any character", word characters) are `text/strip-html#H1`.

## T1. Terms

All terms here have note scope (`query-model.md` Q3, Q5).

| Term | Value (`grammar.md` G7, G8) | Text tested | Context read (`query-model.md` Q8) |
|---|---|---|---|
| plain text | U | sort text and field string (T5) | normalization, ignore accents, fold mode |
| `nc:` | U | sort text and field string, combining-free (T8) | normalization, fold mode |
| `sc:` | U | sort text and field string, cloze markup removed (T9) | normalization, ignore accents, fold mode |
| `w:` | U | field string (T10) | normalization, ignore accents |
| `re:` | Q | field string (T11) | normalization |

"Normalization" is the config key `normalize_note_text` (true when missing), "ignore accents" the config key `ignoreAccentsInSearch` (false when missing), "fold mode" Unicode or AsciiCompat (`case-folding.md` C1). Field terms read normalization and, in normal and `nc:` mode, the fold mode (`fields.md`).

## T2. The note's text

- **Field string**: the column `notes.flds` as stored: the note's fields in field order joined by U+001F, raw HTML, entities not decoded. Field saves already removed control characters and applied NFC when normalization was on at that time (`text/checksum-and-sort-field#C1`); a search never changes stored text.
- **Sort text**: the column `notes.sfld`, read as text. It holds `text/checksum-and-sort-field#C2` (HTML removed keeping media names, entities decoded). The column has INTEGER affinity, so SQLite stores a sort text that reads as a decimal number as that number and gives back the number's text: `1.50` reads back as `1.5`, `1e3` as `1000`, `007` as `7`, ` 12` and `12 ` as `12`, `1.0` as `1`; `0x10` and `12a` stay text. Matching uses the text SQLite gives back (`CAST(sfld AS TEXT)`). So for a sort field `<b>1</b>.50`, the plain search `1.50` finds nothing and `1.5` finds the note (`search/text/html-and-sort-field`). Reals with more than 15 significant digits can read back differently under other SQLite versions; no scenario relies on them.

## T3. Wildcard patterns

Plain text, `nc:`, `sc:` and normal and `nc:` field values are matched as wildcard patterns (R1033).

### T3.1 From value to pattern

The value (after T4) becomes a pattern in one pass from left to right; each row takes characters that no earlier replacement took:

| In the value | In the pattern |
|---|---|
| `\\` | `\\` (matches one backslash) |
| `\*` | `*` (matches one star) |
| `*` | `%` (matches any run) |
| `%` | `\%` (matches one percent sign) |
| any other character | the same character |

So `_` stays `_` (one character) and `\_` stays `\_` (one underscore). `\\*` gives `\\%`: a backslash, then any run.

### T3.2 Matching

A pattern matches a text when it matches the whole text; T5 and T7 add the `%` that make plain text a substring search.

- `%` matches any run of zero or more characters.
- `_` matches exactly one character (one Unicode scalar value).
- `\` followed by a character matches that character, compared like any other pattern character in the fold mode (`case-folding.md` C2, C3): `\D` matches `d` too, as SQLite's `LIKE` does. A `\` at the very end of a pattern makes the pattern match nothing.
- Any other pattern character matches one text character equal to it in the fold mode (`case-folding.md` C2, C3, C6).
- "Character" includes U+001F, line breaks and every other code point, so `%` and `_` cross field boundaries in the field string (T5.2).

These are SQLite's `LIKE` rules with `ESCAPE '\'`. In AsciiCompat mode SQLite's built-in `LIKE` (without ICU) gives exactly this.

### T3.3 Size limit

A pattern longer than 50,000 bytes in UTF-8 fails the search with `search_failed` (`errors.md` E3). The length is that of the whole pattern as tested: the `%` around plain text, and the U+001F and `%` that a run adds (T7.2, `fields.md` F4). So 49,998 `a` pass as plain text (50,000 bytes) and 49,999 fail, and `front:` with 49,999 `a` fails for a `Front` that is the first of two fields (50,001 bytes) (`search/text/pattern-limit`). Anki fails only when a row reaches the test; Meguri checks every pattern of the plan when it builds the plan, as for regular expressions (T12.2, T12.3), the run patterns of note types without notes included.

## T4. Normalizing the search text (R1037)

- When normalization is on, the value of every term here and of every field term is converted to NFC (`text/unicode-primitives#U3`) before anything else. When it is off, the value is used as typed.
- Stored text keeps the form it was saved in: a field saved while normalization was off is not normalized at search time. With a precomposed `café` and a decomposed `cafe` + U+0301 both saved with normalization off, each search form finds only its own note; after normalization is turned on, both search forms find only the precomposed note (`search/text/normalization`).
- Field names are always converted to NFC (`fields.md` F2).

## T5. Plain text (R1034, R1035)

### T5.1 Match

The pattern is `%`, then T3.1 of the value, then `%`. When no note type of the collection has an excluded field (T7.1), a note matches when the pattern matches its sort text or its field string. Otherwise T7.2 applies. With ignore accents on, T6 applies.

### T5.2 Consequences

| Search | Stored | Matches? |
|---|---|---|
| `dog` | `<b>hot</b>dog`, `dog house` | yes |
| `hotdog` | `hot<b>dog</b>` as the sort field | yes (sort text `hotdog`) |
| `hotdog` | `hot<b>dog</b>` in another field | no |
| `&amp;` | `Tom &amp; Jerry` | yes |
| `&` | `a &lt; b` | yes (raw HTML holds `&`) |
| `<` | `a &lt; b` in a field that is not the sort field | no |
| `<` | `c &lt; d` as the sort field | yes (sort text `c < d`) |
| `<` | `<b>x</b>` | yes (the tag) |
| `do*g` | `dosomethingg` | yes |
| `d_g` | `dig` / `dg` | yes / no |
| `a\*b`, `a\_b`, `a\\b`, `a%b` | `a*b`, `a_b`, `a\b`, `a%b` | each only its own |
| `dog*cat`, `dog_cat` | fields `dog` and `cat` | yes (across the separator) |
| `"dog cat"` | fields `dog` and `cat` | no |
| `dog_` | fields `dog` and empty | yes (`_` is the separator) |

Scenarios: `search/text/plain-substring`, `html-and-sort-field`, `field-separator`.

## T6. Ignoring accents (R1038)

When ignore accents is on:

- **plain text** and **`sc:`**: the pattern of T5.1 (for `sc:`, T9.3) is put through the combining-free transform (T8.1) and tested against the combining-free sort text and field string (for `sc:`, after cloze markup removal). The result is the same as `nc:` (or, for `sc:`, `nc:` on the cloze-free text).
- **`w:`**: T10.1.
- Not affected: `nc:` (always combining-free), `re:`, field searches in every mode, all-fields searches (`fields.md` F7).
- The fold mode still applies after the transform (`case-folding.md` C2).

Examples with the key on, pinned by `search/text/ignore-accents` (the results that depend on the fold mode, such as `привет` finding `Привет`, are `search/folding/ignore-accents`): `uber`, `über`, `w:uber` and `w:ÜBER` find `über`, `ÜBER`, `uber` and the cloze note `{{c1::über}} all`; `sc:uber` finds the cloze `{{c1::über}}`; `front:uber`, `re:uber` and `*:uber` find only `uber`; `strasse` finds nothing next to `Straße` and `strase` finds it (T8.2).

## T7. Excluded fields (R1039)

### T7.1 The option

A field is excluded when its option "exclude from unqualified searches" is set: `excludeFromSearch` in the schema-11 field object, `exclude_from_search` in the field config record. The sort field is the note type's `sortf` (`sort_field_idx`). "A note type has an excluded field" looks at every note type of the collection when the plan is built, including types without notes and the new collection's own types.

### T7.2 Plain text, `nc:` and `sc:`

When at least one note type of the collection has an excluded field, these terms test each note type separately:

- The note type's included (not excluded) field positions, ascending, form runs: maximal sequences of consecutive positions.
- A note of a type with N fields matches when, for some run from position s to position e (0-based, inclusive), its field string (transformed as the term requires) matches the pattern made of s times `%` U+001F, then the term's pattern, then N − 1 − e times U+001F `%`; or when the sort field is not excluded and the term's pattern matches the sort text.
- A note type with no included field matches none of its notes.
- When no note type of the collection has an included field, these terms fail with `search_failed`, negated or not (`search/text/no-searchable-field`).

Since the term's pattern starts and ends with `%`, a note matches when the text lies inside the fields of one run; its wildcards may cross separators inside the run but never reach an excluded field. A note type without excluded fields has one run of all its fields, which is T5.1 again. Examples (`search/text/excluded-fields`, fields `Front`, `Back`, `Notes` with `Notes` excluded): `secret` in `Notes` is found by `notes:secret` and `*:secret` only; `x*y` finds the fields `x`, `y`, `dog`; `y*dog` does not find `x`, `y`, `dog`; a note type whose excluded field is the sort field is not found through its sort text.

### T7.3 `re:` and `w:`

When at least one note type of the collection has an excluded field:

- notes of a type without excluded fields are tested on the whole field string, as T11.2;
- notes of a type with excluded fields are tested on each included field alone; the note matches when one of them matches;
- when some note type has every field excluded, every `re:` and `w:` search fails with `search_failed`, negated or not, also when that type has no notes (`search/text/excluded-all`).

So `re:^x` matches a note of such a type whose second field starts with `x`, but not a note of a type without excluded fields whose second field does.

### T7.4 Not affected

Field searches in every mode, all-fields searches included, ignore the option (`fields.md` F9).

## T8. `nc:` (R1042)

### T8.1 Combining-free transform

The combining-free form of a text:

1. NFKD (`unicode-normalization` 0.1.24, Unicode 16.0.0);
2. remove every character of General_Category Mark (Mn, Mc, Me), as that crate classifies them;
3. replace each character of the T8.2 table by its replacement.

Every other character is kept. NFKD folds compatibility forms: fullwidth `ｄｏｇ` becomes `dog`, half-width `ﾊﾞ` becomes `ハ` plus a voiced mark (then removed), `ﬁ` becomes `fi`, `İ` becomes `I` plus a combining dot (then removed).

### T8.2 Extra replacements

| Character | Replacement | Character | Replacement |
|---|---|---|---|
| U+20AC `€` | `E` | U+0131 `ı` | `i` |
| U+00C6 `Æ` | `AE` | U+0138 `ĸ` | `k` |
| U+00D0 `Ð` | `D` | U+0141 `Ł` | `L` |
| U+00D8 `Ø` | `O` | U+0142 `ł` | `l` |
| U+00DE `Þ` | `TH` | U+014A `Ŋ` | `N` |
| U+00DF `ß` | `s` | U+014B `ŋ` | `n` |
| U+00E6 `æ` | `ae` | U+0152 `Œ` | `OE` |
| U+00F0 `ð` | `d` | U+0153 `œ` | `oe` |
| U+00F8 `ø` | `o` | U+0166 `Ŧ` | `T` |
| U+00FE `þ` | `th` | U+0167 `ŧ` | `t` |
| U+0110 `Đ` | `D` | U+018F `Ə` | `E` |
| U+0111 `đ` | `d` | U+01DD `ǝ` | `e` |
| U+0126 `Ħ` | `H` | U+0251 `ɑ` | `a` |
| U+0127 `ħ` | `h` | | |

`ß` becomes a single `s`: `nc:strasse` does not find `Straße`, `nc:strase` and `nc:straße` do. U+1E9E `ẞ` has no entry and stays.

### T8.3 Match

The pattern is `%`, T3.1 of the value, `%`, then put through T8.1 as a whole. The sort text and field string are put through T8.1 too, and the test is T5.1 (or T7.2). Because the pattern is transformed after T3.1, a character whose NFKD is a pattern character becomes one: U+FF05 and U+FE6A become `%` (any run), U+FF3F, U+FE33, U+FE34 and U+FE4D–U+FE4F become `_` (one character), U+FF3C and U+FE68 become `\` (escape). `nc:a％b` therefore finds `a％b`, `azb` and `þaw ayakkabı €xit` (`search/text/nc`; `nc:uber` is pinned by `search/folding/umlaut`, the transform through other terms by `search/tags/regex-and-nc` and `search/fields/modes`). An escape made this way follows T3.2: `nc:a＼b` finds `ab` and not `a＼b` (the `\` escapes the `b`), `nc:a＼＼b` finds `a＼b` and `a\b`, and `nc:a＼` escapes the closing `%`, so it finds only a sort text or field string ending in `a%`; in a field pattern an escape before U+001F matches the separator itself, so `front:nc:a＼` (Front the first of two fields) finds a Front of exactly `a`, while `back:nc:a＼` (Back the last field) ends in `\` and finds nothing (`search/text/nc-escapes`).

| Search | Stored | Matches? |
|---|---|---|
| `nc:uber`, `nc:über`, `nc:ÜBER` | `über`, `Über`, `uber` | yes |
| `nc:は` | `ば`, `ぱ` | yes |
| `nc:ﾊ`, `nc:ハ` | `ﾊﾞ` | yes |
| `nc:aeon` | `ÆON` | yes |
| `nc:dog` | `ｄｏｇ` | yes |
| `nc:izmir` | `İzmir` | yes |
| `nc:fine` | `ﬁne` | yes |

## T9. `sc:` (R1043)

### T9.1 Cloze note types only

`sc:` matches only notes whose note type is of the cloze kind (`type` 1). When the collection has no cloze note type, `sc:` fails with `search_failed`, negated or not (`search/text/sc-no-cloze-type`).

### T9.2 Cloze markup removal

Each cloze in a text is replaced by its answer:

- A cloze starts at `{{c` (lower-case `c`), followed by one or more characters each a decimal digit (General_Category Nd, so `١` counts) or a comma, followed by `::`.
- It ends at the first `}}` after that `::`, whatever lies between (line breaks, U+001F, other openers).
- Its answer is the text between that `::` and the `}}`, cut before its first `::` when it holds one (the hint goes).
- The scan goes left to right; after a replacement it continues after the removed `}}`. A start with no `}}` anywhere after it is kept as written, and the scan goes on from the next character.

| Text | After removal |
|---|---|
| `{{c1::capital}} city` | `capital city` |
| `{{c1::one::hint}} two` | `one two` |
| `{{c1,2::both}} {{c3::}}end` | `both end` |
| `a {{c2::b {{c1::c}} d}} e` | `a b {{c1 d}} e` |
| `{{c١::arabic}}` | `arabic` |
| `{{c1::x` U+001F `y}} z` | `x` U+001F `y z` |
| `{{C1::upper}}`, `{{c::none}}`, `{{c1:single}}`, `{{c1::x` | unchanged |

### T9.3 Match

The pattern is that of plain text (T5.1). The field string and the sort text go through T9.2 (then T8.1 when ignore accents is on, with the pattern transformed too, T6), and the test is T5.1 or T7.2, limited to cloze note types (T9.1). Removal runs on the field string and on the sort text separately, so a cloze whose `}}` lies in a later field is removed from the field string but stays in the sort text: with the fields `{{c1::x` and `y}} z` (the first being the sort field), `sc:"x_y z"` matches through the field string and `sc:"c1::"` through the sort text.

### T9.4 A `-` directly before `sc:`

Written directly before an `sc:` term (`-sc:capital`, `grammar.md` G4), `-` negates only the cloze condition: the term then matches the notes of note types that are not of the cloze kind and whose text, after T9.2 (and T8.1 under ignore accents), matches the pattern. A negated group `-(sc:capital)` is the ordinary negation: every note that `sc:capital` does not match. So with a cloze note `{{c1::capital}} city` and a standard note holding the same text, `-sc:capital` and `-sc:"capital city"` find only the standard note, and `-sc:capital -sc:city` finds it too (`search/text/sc`). The tree keeps `-sc:x` and `-(sc:x)` apart (`query-model.md` Q1 keeps groups).

## T10. `w:` (R1040)

### T10.1 Expression

The regular expression is `(?i)\b`, then the glob conversion (T13.1) of the value, then `\b`. `*` becomes `.*` and `_` becomes `.`; every other character is literal. With ignore accents on, the expression after `(?i)` is put through T8.1 as a whole and tested against T8.1 of the field string.

### T10.2 Match

As `re:` (T11.2, T7.3): the whole field string, or each included field. The sort text is never tested. `\b` is the `regex` crate's Unicode word boundary (`text/strip-html#H1` word characters), so tags and punctuation are boundaries and a run of ideographs is one word.

| Search | Stored | Matches? |
|---|---|---|
| `w:dog`, `w:DOG` | `the dog.`, `x<b>dog</b>` | yes |
| `w:dog` | `hotdog`, `dogs`, `Dog_house` | no (`_` is a word character) |
| `w:dog*` | `dogs` | yes |
| `w:*og` | `hotdog` | yes |
| `w:dog*cat`, `w:dog_cat` | fields `dog` and `cat` | yes (`.` matches U+001F) |
| `w:a.b`, `w:c+d` | `a.b`, `c+d` | yes (literal) |
| `w:日本` | `日本語` | no |
| `w:über` | `Über` | yes in both fold modes |

Scenario: `search/text/word`. An empty `w:` matches every note whose field string holds a word character.

## T11. `re:` (R1041)

### T11.1 Expression

The regular expression is `(?i)` followed by the value (kind Q: only `\"` became `"`; then T4). Meguri compiles it with the pinned `regex` crate 1.11.2 through `Regex::new` on `&str` with the default options: Unicode on, compiled size limit 10 × 2^20 bytes, nesting limit 250. Anki builds it the same way, so acceptance (T12) and matches are identical. `(?-i)` inside the value turns case sensitivity back on.

A `(` ends an unquoted term (`grammar.md` G3), so a pattern holding `(` needs quotes: `"re:(?-i)Dog"`. Unquoted, `re:(?-i)Dog` is `re:` AND a group AND `Dog`.

### T11.2 Match

When no note type of the collection has an excluded field, the expression is tested on the whole field string of each note, unanchored: `^` and `$` are the start and end of the whole string unless `(?m)` is used, `.` matches U+001F but not a line feed unless `(?s)` is used. Otherwise T7.3. The sort text is never tested. ignore accents never applies.

| Search | Stored fields | Matches? |
|---|---|---|
| `re:dog` | `Dog`, `hotdog`, `<b>dog</b>` | yes |
| `"re:(?-i)Dog"` | `Dog` / `hotdog` | yes / no |
| `re:^dog` | `Dog` \| `x` | yes |
| `re:^dog` | `x` \| `dog` | no (not the start of the string) |
| `re:dog$` | `x` \| `dog` | yes |
| `re:^x$` | `Dog` \| `x` | no |
| `re:g.x` | `Dog` \| `x` | yes (`.` is the separator) |
| `"re:\x1f"` | any note with two fields | yes |
| `re:a\"b` | `a"b` | yes |
| `re:one.line` / `"re:(?s)one.line"` | `line one` LF `line two` | no / yes |

Scenario: `search/regex/basics`. An empty `re:` matches every note.

## T12. Invalid regular expressions

### T12.1 Kinds

Every regular expression a query builds is compiled as T11.1 says, at plan time (T12.2): `re:`, field `re:` (`fields.md` F5), the all-fields expressions (F7), `w:` (T10.1), `tag:re:` (`names-and-ids.md` N2.3), and also the expressions built by glob conversion or escaping: field-name patterns (`fields.md` F2 step 3), normal and `nc:` tag values (N2.2 step 4, N2.4), deck names and `deck:current` (N3.3, N3.4), and note type and template names with wildcards (N4.1, N4.2). The second group can never have a syntax error, since every metacharacter is escaped, but a long run of wildcards can pass the size limit (each `_` of a deck name becomes a Unicode `.`, each `_` of a tag value a Unicode `\S`, each `*` a Unicode `\S*`): `deck:` followed by 30,000 `_` is `search_failed (expression too large)`, never a panic or an empty result. Its range is the whole term. The same holds for `note:`, `card:` (by name) and `tag:` values with 30,000 `_` (`search/regex/glob-size`). Anki also reports `search_failed` for these when a card row reaches the term, since it tests the names in SQL (the scenario holds a note); on a collection where no row does, whether SQLite compiles the expression depends on its plan (`-deck:` with 30,000 `_` and no cards gives no ids), a further case of T12.3's timing difference that no scenario observes; for a field-name pattern over the limit (`fields.md` F2 step 3) Anki panics instead, so no scenario holds one and Meguri's `search_failed` there is tested only by U81's matcher tests.

- A syntax error, nesting beyond the limit included, is `invalid_regex` (`errors.md` E1). `"re:("`, `re:[`, `re:a{2,1}`, `re:\p{Nope}` and `re:*` are syntax errors. With the `(?i)` prefix, 249 nested groups are accepted and 250 are an error.
- An expression that parses but whose compiled form exceeds the size limit (`re:a{1000}{1000}`) is `search_failed`.
- `w:` values become literal text, so `w:[` is a valid search (with ignore accents on, T8.1 can still make an invalid expression, for example from U+FF3C).
- An unquoted `re:(` is `unclosed_group`, a grammar error (`errors.md` E3).

Scenarios: `search/regex/invalid`, `search/regex/glob-size`.

### T12.2 When Meguri reports them

Meguri compiles every expression of the query while it builds the plan, before any result and whatever the collection holds: in a negated term, in a field term whose field no note type has, on an empty collection. Which error is reported when a search has several is `errors.md` E4 step 3: the statement checks of every term first, then the pattern checks (T3.3, then compilation) in reading order. The range is the whole term holding the expression (`errors.md` E5). The plan keeps the compiled expressions, so per-row work never compiles again and never fails.

### T12.3 Deviation from Anki

Anki compiles an expression the first time a row reaches its test, and returns results when no row does. In `search/regex/invalid-unreached` it returns no ids for `re:[` and `front:re:[` on a collection without notes and for `nosuchfield:re:[` with a note; Meguri reports `invalid_regex` for each (deviation `search-invalid-regex-unreached`, `conformance/deviations.yaml`). The pattern size limit (T3.3) has the same timing difference, a second deviation, `search-pattern-limit-unreached`: on a collection without notes, `front:` followed by 49,999 `a` (a `Front` that is the first of two fields) returns no ids in Anki and `search_failed (pattern too long)` in Meguri; so does a T7.2 run pattern over the limit that only a note type without notes has. Decided 2026-10-27: Meguri keeps the build-time check (one rule for every pattern, no error that depends on which rows exist). The scenario is `search/text/pattern-limit-unreached`; its `conformance/deviations.yaml` entry `search-pattern-limit-unreached` (Meguri's `search_failed` at `/steps/0/search/0` and `/steps/3/search/0` only, not the `dog` search) is still to be added before the scenario becomes active. `search-invalid-regex-unreached` covers regular expressions only. The glob expressions of T12.1 have the same timing difference on a collection without cards; Meguri reports `search_failed` there too, and no scenario observes it, so no entry covers it.

## T13. Glob conversion to a regular expression

Used by `w:` (T10), field-name patterns and all-fields values (`fields.md` F2, F7), and the name terms (`names-and-ids.md` N1).

### T13.1 Conversion

Read the text from left to right in units: a backslash together with the character after it is one unit; any other character is a unit of its own.

| Unit | Expression |
|---|---|
| `\\` | `\\` (one backslash) |
| `\*` | `\*` (one star) |
| `\_` | `_` (one underscore) |
| `*` | `.*` |
| `_` | `.` |
| any other unit | the unit with every regular-expression metacharacter escaped (the `regex` crate's `escape`) |

A lone backslash at the end is a unit of its own and becomes `\\`. Search texts hold no line feed (`grammar.md` G1).

### T13.2 Has wildcards

A text has wildcards when one of its units is a lone `*` or `_` (a `*` or `_` after an even number of backslashes, zero included).

## T14. SQL functions (Meguri design)

Not Anki behavior: the functions through which Meguri's plan runs the per-row work of this spec. `core/crates/meguri-search/src/sqlfns.rs` defines them and registers them with the card-data functions of `search/card-extractors` X2.1, so the connection initializers of `storage/open-and-connections` C5 add them to the writer and to every reader. No index, view, trigger, generated column or CHECK constraint of the collection file may use them (R2016). Their names start with `meguri_` so they can never collide with a name a collection expects.

| Function | Result |
|---|---|
| `meguri_like(text, pattern, mode)` | 1 when `pattern` matches the whole `text` (T3.2), else 0; `mode` 0 is AsciiCompat, 1 is Unicode (`case-folding.md` C3) |
| `meguri_transform(text, flags)` | `text` with cloze markup removed (T9.2) when bit 1 of `flags` is set, then combining-free (T8.1) when bit 0 is set |
| `meguri_regex(pattern, text)` | 1 when the expression matches anywhere in `text` (T11.2), else 0 |
| `meguri_regex_fields(pattern, flds, ord, ...)` | 1 when the expression matches one of the fields at the listed positions of the field string `flds` (split at U+001F), else 0; with no position listed, any field |

- A NULL `text` or `flds` gives NULL (unknown, `query-model.md` Q7). An INTEGER or REAL argument is read as SQLite's text for it (T2).
- The regex functions take the pattern text the plan compiled (T12.2) and keep the compiled expression in SQLite's per-statement auxiliary data; they never report a compile error, since the plan already compiled every pattern.
- In AsciiCompat mode the plan may use SQLite's `LIKE ... ESCAPE '\'` instead of `meguri_like`.
- The same module holds the card-data functions (`search/card-extractors` X2), the custom-data function (`properties.md` P15) and the tags function (`names-and-ids.md` N7).

## T15. The plan

- Field-name resolution (`fields.md` F2), exclusions (T7), cloze note types (T9.1) and the run patterns are computed from the note types when the plan is built. As `names-and-ids.md` N1 says for names, the result must equal evaluating these rules on the collection state the statement reads, so the plan builds and runs on one read transaction, or is built again when the note types changed between the two (a cache keyed only on context values, `query-model.md` Q8, does not suffice).
- The plan records the context values each term read (T1) and marks as slow every term that calls a T14 function per row (`query-model.md` Q9). In Unicode mode every term of this spec does. In AsciiCompat mode plain text and normal field terms may run on SQLite's `LIKE` and are then not slow.
- Errors raised while building the plan: `invalid_regex` and `search_failed` from T3.3, T7.2, T7.3, T9.1 and T12.1. Their messages are in `errors.md` E7; T16 defines the texts they insert.

## T16. Messages

The messages are `errors.md` E7's, which holds every search message; this section defines the texts they insert. The backtick rule of E7 applies to each.

- `{pattern}` (`invalid_regex`, `search_failed (expression too large)`): the term's value as the user wrote the expression, after the grammar's reading, without the `(?i)`, anchors and boundaries Meguri adds: `re:` and field `re:` (also `*:re:`): kind Q; `w:`: the `w:` value; `tag:re:`: kind Q of the text after `re:`, after NFC (`names-and-ids.md` N2.3); the normal and nc modes of the all-fields names (`fields.md` F7): the value as read, before the glob conversion of T13.1; the glob and escaped expressions of T12.1's second group (`search_failed (expression too large)` only): the text that was converted, as read and after NFC, before the conversion: the field name for a field-name pattern, the tag value for `tag:` and `tag:nc:`, the deck value for `deck:` (the stored name of the current deck for `deck:current`), the note type or template value for `note:` and `card:`.
- `{reason}` (`invalid_regex`): from the `regex` crate's syntax error text, the part after `error: ` on its last line that starts with `error: `, for example `unclosed group`, `unclosed character class`, `repetition operator missing expression`.
- `{notetype}` (`search_failed (all fields excluded)`): the name of the first note type that excludes all its fields, in ascending order of `notetypes.name` under the `unicase` comparison (`text/unicode-primitives#U2`), then of id. Decided 2026-10-27 (Meguri's wording; Anki names none): with `b` and `B2`, or `Z` and `a`, excluding every field, the message names `b`, or `a`.

The `search_failed` causes of this spec and their E7 lines (Anki shows only a database error for them):

| Cause | Section | E7 line |
|---|---|---|
| a pattern over 50,000 bytes | T3.3 | `search_failed (pattern too long)` |
| a regular expression over the compiled size limit | T12.1 | `search_failed (expression too large)` |
| plain text, `nc:` or `sc:` when no note type has an included field | T7.2 | `search_failed (no searchable field)` |
| `sc:` when the collection has no cloze note type | T9.1 | `search_failed (no cloze note type)` |
| `re:` or `w:` when a note type excludes all its fields | T7.3 | `search_failed (all fields excluded)` |

The range of each is the whole term that failed (`errors.md` E5).

## T17. Scenarios

| Scenario | Sections |
|---|---|
| `search/text/plain-substring` | T3, T5 |
| `search/text/html-and-sort-field` | T2, T5 |
| `search/text/field-separator` | T3.2, T5 |
| `search/text/normalization` | T4 |
| `search/text/ignore-accents` | T6 |
| `search/text/excluded-fields` | T7.2, T7.3 |
| `search/text/excluded-all` | T7.2, T7.3 |
| `search/text/no-searchable-field` | T7.2 |
| `search/text/nc` | T8 |
| `search/text/nc-escapes` (no golden yet) | T3.2, T5.1, T6, T8.3, T10.1, T12.1; `fields.md` F4, F6 |
| `search/text/sc`, `sc-no-cloze-type` | T9 |
| `search/text/word` | T10 |
| `search/text/pattern-limit` | T3.3 |
| `search/text/pattern-limit-unreached` (no golden yet) | T3.3, T7.2, T12.3; `fields.md` F4 |
| `search/regex/basics` | T11 |
| `search/regex/invalid`, `invalid-unreached` | T12 |
| `search/regex/glob-size` (no golden yet) | T12.1; `names-and-ids.md` N2.2 step 4, N3.3, N4.1, N4.2 |

All result scenarios carry the tag `needs-text` (`specs/conformance/search-ops.md` S4). Several set field options and the sort field with `legacy_update` (`specs/conformance/model-ops.md` L2) before adding notes, so Meguri's executor needs that op for note types.
