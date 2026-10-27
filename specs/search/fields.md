# Field searches

Spec: search/fields
Requirements: R1044, R1045, R1046, R1047, R1048, R1049; supports R1036, R1085
Anki: 26.09.3
Scenarios: conformance/scenarios/search/fields/

What `field:value` matches: how the field name resolves against the note types, and the normal, `re:` and `nc:` modes. The pattern rules, the transforms and the regular expressions are `text-matching.md` (T3, T8, T11, T13); letter case is `case-folding.md`. Every rule here was checked against the oracle; the scenarios pin the examples.

## F1. The term

A field term is any `key:value` whose key is not reserved (`grammar.md` G6, R1032). `grammar.md` G8 gives its parts:

- **name**: the key, kind U, so `\\`, `\*` and `\_` are still escaped;
- **mode**: `re:` or `nc:` at the start of the value as written, lower-case only, gives regex or nc mode; anything else is normal mode (`f:RE:x` and `f:re\:x` are normal searches for `RE:x` and `re:x`);
- **value**: normal mode kind U; regex and nc mode kind Q of the rest.

The value is converted to NFC when normalization is on (`text-matching.md` T4). A field term has note scope and reads normalization and, in normal and nc mode, the fold mode; it never reads ignore accents (F9).

A field name holding a space is written inside quotes together with its value: `"my field:value"`. `"my field":value` is the quoted term `my field` followed by the term `:value`, a `missing_key` error (`errors.md` E4). The escaped space `my\ field:value` that R1046 names is `unknown_escape` at the pinned oracle (`grammar.md` G3, `search/fields/names`); Meguri follows the oracle.

## F2. Resolving the name

The name is resolved when the plan is built, against every note type of the collection: types without notes and the new collection's own types included.

1. Convert the name to NFC, whatever normalization says.
2. If the name, as written after step 1, is exactly `*`, `_*` or `*_`, it selects every field of every note type: F7. Escapes count: `\*` is not such a name.
3. Else, if the name has wildcards (`text-matching.md` T13.2), it matches the field names that the regular expression `^(?i)` + T13.1(name) + `$` matches: Unicode simple case folding (`case-folding.md` C3), `*` any run, `_` one character.
4. Else, remove each escaping backslash (`\\` gives `\`, `\*` gives `*`, `\_` gives `_`) and compare with each field name by the unicase equality of `text/unicode-primitives#U2` (full case folding).

The result is, for each note type, the positions (0-based field order) of its matching fields, ascending. Note types with no matching field take no part.

| Name | Field | Matches? | Why |
|---|---|---|---|
| `straße` | `STRASSE` | yes | step 4: `ß` and `SS` fold alike |
| `straß*` | `STRASSE` | no | step 3: simple folding keeps `ß` apart from `s` |
| `stras*` | `STRASSE` | yes | step 3 |
| `cafe` + U+0301 | `Café` | yes | step 1 |
| `CAFÉ`, `ÜNIT`, `ÜN*` | `Café`, `Ünit`, `Ünit` | yes | steps 3 and 4 |
| `a_b` | `a_b`, `axb` | yes, yes | step 3: `_` is a wildcard |
| `a\_b` | `a_b` / `axb` | yes / no | step 4 |
| `\*` | `*` | yes | step 4 |
| `**` | every field | yes | step 3, not step 2 (F4: one run) |

## F3. Which notes can match

A field term can only match notes of note types that have a matching field. When no note type has one, the term matches nothing, with no error (R1049): `nosuchfield:x` and `nosuchfield:*` match no note and `-nosuchfield:x` matches every note. `-front:dog` matches every note of a note type without a `Front` field (`search/fields/exact`).

## F4. Normal mode (R1044, R1045)

- The pattern is T3.1 of the value (`text-matching.md`), with no `%` added: the whole field must match. `front:dog` does not match `hot dog`; `front:*dog*` does.
- The matching positions of each note type form runs: maximal sequences of consecutive positions. When the value as written is exactly `*`, `_*` or `*_`, every position is a run of its own instead.
- A note of a type with N fields matches when, for some run from position s to position e (inclusive), its field string matches the pattern made of s times `%` U+001F, then the value's pattern, then N − 1 − e times U+001F `%`. Letter case follows the fold mode (`case-folding.md` C2).
- The size limit of `text-matching.md` T3.3 applies to that whole pattern.

### F4.1 Consequences of a one-position run

The whole field at that position equals the pattern: `front:` matches an empty field, `front:_*` (or `*_`) a non-empty one, and `front:*` every note whose type has `Front` (R1045). `front:d_g` matches `dog`, `DOG` and `dig`; `front:DOG` matches `dog` (ASCII case, `case-folding.md` C2).

### F4.2 Consequences of a longer run (R1047)

When a name pattern matches neighbouring fields, the run is tested as one span of the field string. Without wildcards in the value that can take a U+001F, the value then equals one field of the run, and which one is limited:

| Run | Fields the value can equal |
|---|---|
| neither first nor last field of the note type | any field of the run |
| starts at the first field, ends before the last | only the first field |
| starts after the first field, ends at the last | only the last field |
| every field of the note type | none (unless the note type has one field) |

A `%` or `_` in the pattern can take U+001F, so `a*:dog_x` matches the fields `dog`, `x`, `z` (the value spans two fields), and `**:*dog*` matches any note holding `dog`. This is the behavior of Anki 26.09.3, which Meguri reproduces (R1085 over R1047's wording). Examples with the note type `A1`, `A2`, `B` (`search/fields/glob-runs`):

| Search | `x`, `dog`, `z` | `dog`, `x`, `z` | `x`, `y`, `dog` | `dog`, `dog`, `z` |
|---|---|---|---|---|
| `a*:dog`, `a_:dog`, `a*:nc:dog` | no | yes | no | yes |
| `a1:dog` | no | yes | no | yes |
| `a2:dog` | yes | no | no | yes |
| `a*:re:dog` | yes | yes | no | yes |
| `a*:*`, `a*:_*` | yes | yes | yes | yes |
| `a*:*dog*` | yes | yes | no | yes |

With `X`, `Y1`, `Y2`, `y*:dog` matches `x`, `y`, `dog` but not `x`, `dog`, `z`; with `P`, `Q1`, `Q2`, `R`, `q*:dog` matches `dog` in either `Q` field.

## F5. Regex mode (R1048)

The regular expression is `(?i)` + the value (`text-matching.md` T11.1). A note matches when it matches at least one of its type's matching fields, each field tested alone, unanchored: `^` and `$` are that field's start and end. No runs. `front:re:^a` matches `apple` but not `x` \| `apple`; `back:re:^a` matches the latter; `front:re:` matches every note whose type has `Front`. Invalid expressions: `text-matching.md` T12.

## F6. nc mode (R1048)

As F4, with the value's pattern put through the combining-free transform (`text-matching.md` T8.1) after T3.1, and the field string put through it too. Runs as F4. `front:nc:uber`, `front:nc:Uber` and `front:nc:ÜBER` match `über`, `Über` and `uber`; `front:nc:ub*` matches all three.

## F7. All-fields names

The names `*`, `_*` and `*_` (F2 step 2) test every field of every note, whatever its note type:

- **normal and nc mode**: the regular expression `(?is)^` + T13.1(value) + `$`, tested on each field alone. It folds case with Unicode simple folding in both fold modes and `.` also matches a line feed. The nc mode is not applied: `*:nc:uber` matches only `uber`, not `über`. `*:über` and `*:ÜBER` match `über` and `Über`; `*:` matches an empty field; `*:line*two` matches `line one` LF `line two`.
- **regex mode**: `(?i)` + the value, on each field alone: `*:re:^a` matches a note with any field starting with `a`.
- A note matches when one of its fields does. Size limit: none (regular expressions, `text-matching.md` T12.1).

## F8. Missing fields

F3.

## F9. Options that do not apply

- Excluded fields (`text-matching.md` T7): field searches find text in excluded fields (`notes:secret`, `*:secret`).
- Ignore accents (`text-matching.md` T6): `front:uber` does not match `über` with the key on.

## F10. Scenarios

| Scenario | Sections |
|---|---|
| `search/fields/exact` | F3, F4.1 |
| `search/fields/names` | F1, F2, F3, F7 |
| `search/fields/glob-runs` | F4.2, F5, F6 |
| `search/fields/modes` | F1, F5, F6, F7 |
