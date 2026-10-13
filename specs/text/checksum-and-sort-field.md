# Checksum and sort field

Spec: text/checksum-and-sort-field
Requirements: R773, R774, R1457; supports R775, R807, R808
Anki: 26.09.3
Scenarios: conformance/scenarios/text/fields/, conformance/scenarios/text/corpus/

The values a note save derives from its fields: the normalized fields themselves, the sort-field text stored in `notes.sfld`, and the checksum stored in `notes.csum`. Both derived columns are rewritten on every save that stores the note, so they must equal Anki's bit for bit or synced and imported notes disagree.

## C1. Field save normalization

On every note save, each field is normalized before anything is derived from it:

1. Remove every ASCII control character except TAB (U+0009) and LF (U+000A): U+0000–U+0008, U+000B–U+001F and U+007F. CR goes too (`line` CR LF `break` saves as `line` LF `break`).
2. Then, when the collection config key `normalize_note_text` is true or absent (the default), normalize the field to NFC (`unicode-normalization` 0.1.24, Unicode 16.0.0). When it is false, skip NFC; step 1 still applies.

The normalized fields are what is stored in `notes.flds` (R769) and what C2, C4 and card generation (`field-emptiness.md` E2) read.

## C2. Sort-field text

The sort-field text is `strip-html.md` H5 (stripping that keeps media names) applied to the normalized field at the note type's sort field index (notetype config `sort_field_idx`). An index past the last field gives the empty string. The text is not trimmed and inner whitespace is kept:

| Field | Sort-field text |
|---|---|
| `<b>私</b>` | `私` |
| `<img src="a b.jpg">text` | ` a b.jpg text` |
| `[sound:私.mp3]` | `[sound:私.mp3]` |
| `<style>x</style>私` | `私` |
| `Tom &amp; Jerry` | `Tom & Jerry` |
| `&#49;&#50;` | `12` |
| TAB `5` LF | TAB `5` LF |

## C3. Storing the sort-field text

`notes.sfld` is declared `integer`. Meguri binds the sort-field text as TEXT, never a number it parsed itself, and SQLite's INTEGER column affinity decides the stored value: text that reads as a numeric literal is stored as a number, everything else as text. Numbers then sort before text and among themselves numerically (R773). Observed on the oracle (SQLite 3.49.2):

| Sort-field text | Stored as |
|---|---|
| `123`, `007`, ` 12 `, TAB `5` LF, `+4` | integer 123, 7, 12, 5, 4 |
| `-4` | integer −4 |
| `1.0`, `1e3`, `3.0e+5` | integer 1, 1000, 300000 |
| `1.5` | real 1.5 |
| `9223372036854775807` | integer 9223372036854775807 |
| `9223372036854775808` | real 9.223372036854776e18 |
| `1e400` | real +infinity |
| `0x10`, `Inf`, `NaN`, `12abc`, `1,5`, empty | text |

These conversions are SQLite's, so Meguri reproduces them by binding text into the same column, not by code of its own. The storage units (U22, U23) confirm the table with their bundled SQLite.

## C4. Checksum

The checksum is computed from the first field (index 0), whatever the sort field is:

1. Take `strip-html.md` H5 of the normalized first field (the same text as C2 when the sort field is the first field).
2. Compute SHA-1 of its UTF-8 bytes.
3. Read the first 4 digest bytes as a big-endian unsigned 32-bit integer.

`notes.csum` stores that integer (0 to 4294967295), bound as an integer.

| First field | Checksum |
|---|---|
| empty | 3661210606 |
| `私`, `<b>私</b>`, `<div>私</div>`, `&#31169;` | 1270766077 |
| ` 私` | 872227349 |
| `私 ` | 2729607979 |
| `<img src="a b.jpg">text` | 4222853378 |
| `e` U+0301 and U+00E9 | 3205873265 (NFC makes them one value) |
| `a` U+0001 `b` and `ab` | 3659751758 |
| `Tom & Jerry` and `Tom &amp; Jerry` | 495331847 (the first does not decode, the second decodes to the same text) |

## C5. First-field comparisons

The editor's first-field checks (R807, R808; built in U191 and U27) use the comparison text: the candidate's first field, NFC-normalized when `normalize_note_text` is on (no control-character removal, since the candidate is not saved yet), then `strip-html.md` H5.

- The first field counts as empty when the comparison text, with Unicode whitespace removed from both ends, is empty.
- Otherwise a note is a duplicate when another note of the same note type (a different note id) has the checksum C4 computes from the comparison text, and the H5 text of that note's stored first field equals the comparison text exactly. So `<b>私</b>` duplicates a stored `私`, while `私 ` does not.

## C6. Scenarios

- `fields/checksum` (covers AE34, checksum half): C1, C2 and C4 on the rows above.
- `fields/sort-field`: the sort-field text for the C3 rows (the observation records the text; the stored type is C3's table).
- `corpus/fields-*`: 48,000 distinct generated fields.
- Observation functions (`specs/conformance/observations.md` O9): `sort_field` (C1 then C2) and `checksum` (C1 then C4), both with `normalize_note_text` on.
