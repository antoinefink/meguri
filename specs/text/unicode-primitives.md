# Unicode primitives

Spec: text/unicode-primitives
Requirements: supports R1451 (the `unicase` collation), R891 (tag text), R775 (field normalization)
Anki: 26.09.3
Scenarios: conformance/scenarios/text/fold/, conformance/scenarios/text/corpus/unicase-01.json

Small Unicode building blocks other text specs and units share. Their tables come from exact-pinned crates (KTD2), so a crate upgrade is an oracle re-pin, never a routine update.

## U1. ASCII control characters

U+0000–U+001F and U+007F. Field saves remove them except TAB and LF (`checksum-and-sort-field.md` C1), media file names remove all of them (`media-filenames.md` F1), and tag canonicalization (R891, U30) removes them too.

## U2. Case-insensitive comparison (`unicase`)

The comparison behind the SQLite collation named `unicase` that every connection registers (R1451, U22). It orders deck, note type, field, template, preset and tag names, and decides when two names collide.

- Replace every character of each string by its case folding, then compare the two folded sequences code point by code point; a proper prefix sorts first. Equal folded sequences compare equal.
- The folding is the full case folding table bundled with `unicase` 2.6.0 (statuses C and F of CaseFolding.txt as of Unicode 12.1; characters cased in later versions are not folded): `ß` and `ẞ` fold to `ss`, `İ` (U+0130) to `i` + U+0307, `ǅ` to `ǆ`, U+212A KELVIN SIGN to `k`, U+212B ANGSTROM SIGN to `å`. Turkic-only mappings are not used, so `ı` (U+0131) does not equal `I`.
- Quirk of the pinned table, kept for parity: a character whose folding has three characters contributes them in the order third, first, second. `ﬃ` (U+FB03, folding `f f i`) compares equal to `iff`, not to `ffi`; U+0390 (folding U+03B9 U+0308 U+0301) equals U+0301 U+03B9 U+0308. Two-character foldings keep their order.
- `UniCase::new(a).cmp(&UniCase::new(b))` of `unicase` =2.6.0 is exactly this comparison; use it.

| a | b | Result |
|---|---|---|
| `a` | `B` | −1 |
| `Z` | `[` | 1 (`z` after `[`) |
| `_` | `a` | −1 |
| `Straße` | `STRASSE` | 0 |
| `İ` | `i` | 1 |
| `İ` | `i` + U+0307 | 0 |
| `ﬃ` | `ffi` | 1 |
| `ﬃ` | `iff` | 0 |
| U+A7C0 `X` | U+A7C1 `x` | −1 (Unicode 14 letters are not folded) |
| empty | `a` | −1 |

## U3. NFC

NFC is the Unicode Normalization Form C of `unicode-normalization` 0.1.24 (Unicode 16.0.0). Field saves (R775), media file names (R979) and tags (R891) use it.

## U4. Scenarios

- `fold/unicase`: the table above and more cased scripts.
- `corpus/unicase-01`: 2,000 distinct generated pairs (case variants, near misses, unrelated strings).
- Observation function (`specs/conformance/observations.md` O9): `unicase_compare`.
