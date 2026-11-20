# Find and Replace

Spec: browser/find-replace
Requirements: R1200, R1201, R1202, R1203, R1204, R1205, R1206, R1207, AE47; supports R1179, R1181, R1182, R1183, R1184, R1185, R1245, R1832, R1860
Anki: 26.09.3
Scenarios: conformance/scenarios/browser/find-replace/
Status: Approved 2026-11-20 (R1232) for the lines of F2, F8, F10 that begin "Changed 2026-11-20 (round 8)". Approved by Antoine in chat (round 8, decided 2026-11-20), after an independent review and five reviewer rounds (`docs/process/review-log/browse-note-types-specs-20261120.md`). Every other section is in force and unchanged.

Browse's Find and Replace rewrites text in the fields or in the tags of notes. This file gives what the sheet offers (F2, F3), how the find text becomes a pattern (F4) and the replacement text (F5), what the operation does to the notes it rewrites (F6, F7), which notes it reads (F8), what it reports (F9), the live preview Meguri adds (F10) and the operation's contract (F11). How a bulk operation runs (job, compute phase, commit phase, cancellation, progress, change set, undo entry) is the Meguri design doc `browser/bulk-framework` (U86); this file says only what the run computes and writes. The selection's ids come from `browser/result-sets` H12 (`resolve_selection`). The scenario ops are `specs/conformance/bulk-ops.md` X1 and X3.

Terms: `now_s` and `now_ms` are the operation's instant in seconds and milliseconds. Separators, tag normalization and spelling are `tags/tags` T1 to T4. `unicase` is `text/unicode-primitives#U2`, NFC `#U3`.

## F1. Inputs

| Input | Values |
|---|---|
| Notes | a list of note ids, or **all notes** (F8) |
| Find | a string, possibly empty |
| Replace With | a string, possibly empty |
| In | **All Fields**, **Tags**, or a field name |
| Treat input as regular expression | boolean |
| Ignore case | boolean |

Anki's backend takes "match case", the inverse of Ignore case, and an empty note list for all notes. Meguri's op takes an explicit all-notes target (F11).

## F2. The sheet (R1200, R1201)

Changed 2026-11-20 (round 8): the sheet's rows, the Notes choice (Selected or All Found), Match Case, Whole Words and the preview are `browser/workspace` V9. Selected notes only, Ignore case as a checkbox and the all-notes target are replaced there.

- **In** lists, in this order: All Fields, Tags, then the field names of F3. Opened from a field in Browse's sidebar (R1179), the field names are only that field's name, and it is preselected. Otherwise the field names are F3's for the selection as it is when the sheet opens, or, with an empty selection, for every note type (F3's Meguri listing). The list does not change while the sheet is open: turning Selected notes only off or on keeps it and the chosen entry (as in Anki, whose list always comes from the selection). When the sheet is not opened from a sidebar field, the entry chosen first is the remembered In choice when the list offers it again (below), else All Fields.
- **Selected notes only**: on when the sheet opens; off and disabled when the selection is empty; off, and enabled when there is a selection, when opened from a sidebar field. Off means all notes (F8). The selected notes are the selection's notes: in cards mode the distinct notes of the selected cards, in notes mode the selected notes (`browser/result-sets` H12, target notes).
- **Ignore case** is on and **Treat input as regular expression** off the first time.
- **Remembered per device** (R1201), never in the collection (Anki keeps them in its profile, so they never sync):
  - the last state of Ignore case and of Treat input as regular expression, saved on every Apply;
  - a history of Find texts and one of Replace With texts, newest first, the last 30 of each (Anki keeps 50). Each Apply enters both texts before the operation runs, so a refused run enters them too (as in Anki). A text already in the history, compared code point by code point, moves to the front instead of being entered twice. Meguri never enters an empty text; Anki enters it like any other;
  - the In choice, as the entry's text and position, saved on every Apply and restored when the list offers the same text at the same position. Anki restores it only within one app session and keeps it in memory; Meguri remembers it across launches.
  - Within one app launch, a sheet opens with Find and with Replace With prefilled with the text of the last Apply when that text is still the newest history entry; otherwise they open empty. Anki does the same within one app session.
- **Selected notes only** is not remembered: R1201's checkbox choices are read as the two checkboxes above. It follows the selection each time the sheet opens, as in Anki, so a replacement that was run on all notes once never silently widens a later one.
- The sheet needs no text to run: an empty Find is a valid pattern (F4). Apply starts F11's operation; the sheet closes when it succeeds and shows F4's error when it is refused.

## F3. Field names (R1200)

The field names the In list offers for a set of notes (`selection_summary` in `browser/bulk-framework`):

1. Take the note types of the notes that exist. Notes that no longer exist add nothing.
2. Take every field name of those note types.
3. Keep one entry per name under `unicase`: `Meaning` in one note type and `meaning` in another give one entry. Its spelling is the one of the note type with the lowest id among them (Anki leaves the choice to SQLite).
4. Order the entries by `unicase`, then by code points for names `unicase` finds equal (none remain after step 3).

| Note types of the notes | Field names offered |
|---|---|
| `Vocab` (Word, Meaning) and `Gloss` (Text, Aside) | Aside, Meaning, Text, Word |
| `Plain` (Text, Extra) and `Gloss` (Text, Aside) | Aside, Extra, Text |
| none (empty selection) | none: In offers All Fields and Tags only |

With the all-notes target and an empty selection Anki offers no field name. Meguri's `selection_summary` for the all-notes target lists the fields of every note type of the collection (U86's design), so a collection-wide replacement in one field can be started without a selection.

## F4. The pattern (R1202, R1203)

The find text becomes one pattern of the Rust `regex` crate (Anki 26.09.3 builds with `regex` 1.11.2, `regex-syntax` 0.8.6): Unicode-aware, matching characters, not bytes. Meguri compiles it with the same crate and its default builder limits, and pins the crate to the 1.11 series with its matching `regex-syntax`. The accepted syntax, the Unicode tables, the limits and the error messages, which the user sees (step 5), can change between crate versions, so an update of either crate is a compatibility change reviewed against this file and its scenarios.

1. **Literal** (Treat input as regular expression off): every character of the find text stands for itself. Anki escapes the text with the crate's escape function, which puts `\` before each ASCII meta character; any escaping with the same meaning is correct.
2. **Regular expression** (on): the find text is the pattern as typed, in the crate's syntax: classes such as `\w` and `\d` are Unicode classes, `.` does not match LF, `^` and `$` anchor at the ends of the whole text unless the pattern turns on `(?m)`.
3. **Ignore case** (on): the pattern is preceded by `(?i)`, so it matches case-insensitively by Unicode simple case folding: `k` matches `K` and U+212A KELVIN SIGN, `ss` never matches `ß`. A pattern of its own may still turn the flag off with `(?-i)`.
4. **NFC** (In is All Fields or a field, not Tags): when `normalize_note_text` reads true (`browser/config-keys` K1: also when missing or not a boolean), the pattern text is normalized to NFC after steps 1 to 3. Escaping adds only `\` before ASCII punctuation and `(?i)` ends with `)`, neither of which composes with what follows, so for literal text this equals normalizing the find text first. So with the key on, a decomposed `cafe` U+0301 finds the stored `café` (U+00E9). With the key off the text is used as typed and finds only text stored in the same form. With In set to Tags there is no NFC step: stored tags are NFC (`tags/tags` T2), so a decomposed literal find text never matches them.
5. A pattern the crate refuses (a syntax error, or a pattern over its compiled-size or nesting limits) refuses the whole operation with `InvalidRegex`, carrying the crate's message; Anki shows that message in a warning. Nothing is written and no undo step is recorded. A literal find text can fail only by size.

The empty find text is a pattern that matches the empty string at every character boundary: before the first character, between any two characters and after the last. So it also matches once in an empty field.

| Find | Regex | Ignore case | Text | Matches |
|---|---|---|---|---|
| `a.b` | off | on | `a.b axb` | `a.b` only |
| `a.b` | on | on | `a.b axb` | `a.b`, `axb` |
| `CAT` | off | off | `cat Cat CAT` | the last word |
| `cat` | off | on | `cat Cat CAT` | all three |
| `k` | off | on | `K`, U+212A | both |
| `ss` | off | on | `ß` | none |
| `(` | off | on | `f(x)` | `(` |
| `(` | on | on | any | refused, `InvalidRegex` |
| (empty) | off | on | `ab` | three empty matches, at 0, 1 and 2 |

## F5. The replacement (R1202)

Every match of the pattern in a text is replaced, left to right, without overlap, each by the replacement text read as follows:

- **Literal mode**: the Replace With text exactly as typed; `$` is an ordinary character (`$5` stays `$5`).
- **Regular-expression mode**: the crate's replacement syntax.

  | In the replacement | Becomes |
  |---|---|
  | `$$` | one `$` |
  | `${name}` or `${7}` | the text of that capture group: a number when the braces hold ASCII digits, optionally after one `+`, whose value fits an unsigned 64-bit integer; otherwise a name. Any text without `}` may stand between the braces, so `${+1}` is group 1, and a run of digits too long for that integer is a name, which no group has (nothing) |
  | `$name` or `$7` | the same, the reference being the longest run of ASCII letters, digits and `_` after `$`: `$1a` is the group named `1a`, not group 1 followed by `a` |
  | `$0`, `${0}` | the whole match |
  | a reference to a group the pattern does not have, or one that took part in no match | nothing (the empty string) |
  | `$` followed by anything else, or `${` with no `}` after it | the characters as they stand (`$ ` stays `$ `) |

| Find (regex) | Replace With | Text | Result |
|---|---|---|---|
| `(\w+)@` | `${1} at` | `Foo@ example` | `Foo at example` |
| `(?P<n>\d+)` | `${n}kg` | `12 and 7` | `12kg and 7kg` |
| `(\d)` | `$1a` | `a1b2` | `ab` |
| `[aeiou]` | `<$0>` | `kiwi` | `k<i>w<i>` |
| `x` | `$$-$ -${` | `x` | `$-$ -${` |
| `(\d)` | `<${+1}>` | `a1` | `a<1>` |
| `^$` | `none` | (empty) | `none` |
| (empty) | `-` (literal) | `aé` | `-a-é-` |

A match whose replacement equals the matched text still counts as a match (F6, F7).

## F6. In All Fields or a field (R1204, R1207)

1. **Order.** The operation reads the notes in ascending note type id, then ascending note id. The order shows only in the cards that step 5 adds: in their ids (`notes/card-generation` G8) and in their new-queue positions (`notes/deck-placement-and-position` P2), since each note that needs a position takes the next counter value in this order.
2. **Fields read.** The stored `flds` is split at every U+001F (`notes/note-records` N1), so a damaged row is read with the count it holds. All Fields: every stored value, those beyond the note type's field count included. A field name: the note type's field whose name equals it under `unicase` (`meaning` finds `Meaning`), read as the stored value at that field's ordinal, or as nothing when the row holds fewer values (no match is possible there); a note whose note type has no such field is skipped: not read, not written, not counted. The text read is the stored value, markup included: a pattern sees tags such as `<b>` and entities such as `&nbsp;` as written.
3. **Matched notes.** A note matches when the pattern has at least one match in a field it reads. Every such field gets F5's result; the others stay as stored.
4. **The new row.** A matched note is saved as a note update (`notes/note-records` U3) with these differences: its tags are not canonified, and the comparison below replaces U2. The stored `tags` text is split as `tags/tags` T6 step 1 does (at U+0020 and U+3000, empty pieces dropped) and joined again with one U+0020 between tags and one at each end, or stored as the empty string when no tag remains: order, case and duplicates stay, nothing is normalized, spelled or registered, but the spacing becomes canonical (` zz  aa `, `zz　aa` and `zz aa` are all stored as ` zz aa `; a text of separators only as the empty string). So every field is normalized (`notes/note-records` N2: controls other than TAB and LF removed, then NFC when the key is on), `sfld` and `csum` are computed again (`text/checksum-and-sort-field` C2 to C4), `mod` is `now_s` and `usn` -1; `guid`, `mid`, `flags` and `data` stay.
5. **Cards.** The cards the note now requires are added (`notes/card-generation` G7), placed by `notes/deck-placement-and-position` D3 and positioned by its P2: each note is its own generation, so a note that needs a new position reads and advances the counter once, in step 1's order. No card is ever deleted, also when a field the template needs becomes empty.
6. **Unchanged matches** (R1204). Meguri writes a matched note only when its new `flds` differs from the stored `flds`; a note whose replaced, normalized fields equal the stored ones is not written, not counted, and adds no card. Anki writes and counts every matched note: replacing `a` with `a`, `(x)` with `$1`, or an empty find text with an empty replacement stamps the note (`mod`, `usn`, `col.mod`) and runs card generation, although `flds` stays the same. Deviation `find-replace-unchanged-match` (R1204, R1207), scenario `browser/find-replace/unchanged-match`.
7. **Field count.** A matched note whose stored field count differs from its note type's (a damaged row, which Check Database repairs, `notes/note-records` F1) refuses the whole operation before anything is committed: `FieldCountMismatch{noteId}`, the first such note in step 1's order. A match found only in a value beyond the note type's count (step 2) is a match and refuses too. Anki fails the same way (invalid input) and keeps nothing. A damaged note the pattern does not match does not refuse the operation.
   - **A missing note type.** A listed note whose `mid` names no note type (a damaged collection, `maintenance/check-database-notes` N4) refuses the whole operation with `NotFound{noteId}`, the first such note in step 1's order, whether or not the pattern would match it. Anki refuses it with invalid input before it reads that note type's notes. The Tags target (F7) never reads note types and has no such refusal.
8. **Count.** The number of notes written.

Examples (In `Meaning`, literal, Ignore case on, Replace With `$5`):

| Note type, fields | Find `cost` gives |
|---|---|
| Vocab: `rate`, `the cost of it` | Meaning `the $5 of it`, written |
| Vocab: `charge`, `Cost cost` | Meaning `$5 $5`, written |
| Vocab: `cost`, `worth` | no match in Meaning: untouched |
| Plain (no Meaning): `cost here`, `x` | skipped |

## F7. In Tags (R1205)

1. **Refusal first.** A Replace With text holding a separator (U+0020 or U+3000) refuses the operation with `InvalidInput` before the pattern is built ("replacement name cannot contain a space" is Anki's message; the wording is U219's).
2. **Pattern and replacement.** F4 steps 1 to 3 and 5 (no NFC) and F5.
3. **Per tag.** The listed notes that exist are handled in ascending note id, whatever the list's order; the order shows in T3's spelling, since a tag a note registers earlier in the operation spells the same name in a later note. For each note, its stored tags are split at separators, dropping empty pieces (`tags/tags` T6 step 1), and the pattern runs on each tag on its own, in stored order: `^` and `$` anchor at the ends of one tag, and no match spans two tags. A note matches when at least one tag has a match. A note without tags has nothing to run on and never matches, also with an empty find text, which matches every tagged note (F4).
4. **The new tags.** A matched note's tag list, each tag replaced by F5's result (unmatched tags as they were), is stored as a note save stores tags (`tags/tags` T4 steps 1 to 5): empty results are dropped, each tag is normalized (T2) and spelled and registered (T3), duplicates go, the rest is sorted and joined. Only `tags`, `mod` (`now_s`) and `usn` (-1) change; fields, `sfld` and `csum` stay, nothing is normalized in them and no card is added. Tag rows the note no longer uses stay in the registry (`tags/tags` T11 removes them).
5. **Unchanged matches.** As in F6 step 6, Meguri writes a matched note only when the new `tags` text differs from the stored one; Anki writes and counts every matched note. Same deviation.
6. **Count.** The number of notes written.

| Stored tags | Find | Regex | Replace With | Stored after |
|---|---|---|---|---|
| ` keep old::x ` | `old` | off | `new` | ` keep new::x ` |
| ` new::x old::x ` | `old` | off | `new` | ` new::x ` |
| ` OLD::y ` (Ignore case on) | `old` | off | `new` | ` new::y ` |
| ` also solo ` | `^so` | on | `x` | ` also xlo ` |
| ` ax x::x ` | `x$` | on | `z` | ` az x::z ` |
| ` drop stay ` | `drop` | off | (empty) | ` stay ` |
| ` fee ` | `fee` | off | `$9` | ` $9 ` |
| ` a::b ` | `b` | off | `::` | ` a::blank::blank ` (T2: empty components become `blank`) |
| ` ab ` | (empty) | off | `x` | ` xaxbx ` |
| (empty) | (empty) | off | `x` | not matched, untouched |
| ` stay ` | `stay` | off | `two words` | refused, step 1 |

## F8. Which notes (R1200)

Changed 2026-11-20 (round 8): Browse no longer offers all notes: "All Found" passes the note ids of the current results (`browser/workspace` V9). The operation's all-notes target stays for other callers.

- **Selected notes only**: the selected notes (F2), as ids. Ids of notes deleted since the selection was taken are skipped.
- **All notes**: every note of the collection when the operation reads it (the compute phase's snapshot, `browser/bulk-framework`).
- The considered count of F9 is the number of note ids the operation was given: the selected notes, those the field target skips (F6 step 2) included; for all notes, the number of notes read.

## F9. Result (R1207, R1184)

- The operation returns the count of F6 step 8 or F7 step 6 (Meguri's `affected`) and the number of notes considered (F8; Meguri's `considered`).
- Anki's English message: with Selected notes only, "`N` of `M` notes updated" (singular "note" when `M` is 1), `M` being the selected notes; for all notes, "`N` notes updated." (singular when `N` is 1). Meguri uses the same rule: R1207's "`N` of `M` notes updated" for a target of note ids, `N` the `affected` and `M` the `considered` count, and "`N` notes updated" for the all-notes target. The wording and its plural forms are U219's.
- **Undo.** One undoable step: "Find and Replace" (`specs/conformance/undo-ops.md` V4 `find_and_replace`) for All Fields and a field; "Update Tag" (V4 `update_tag`) for Tags, which is Anki's name for that operation. Undo restores every note row the step wrote, removes the cards it added (`engine/undo` A9), removes the tag rows it registered, and restores `col.mod`.
- **Nothing written** (count 0): no step is recorded and `col.mod` stays; like every operation it still empties the redo history (`engine/undo` A2).
- **Refused or cancelled** (F11): nothing is written, no step is recorded, and Meguri's undo and redo histories stay as they were (`engine/undo` M2: an op that fails and rolls back leaves both). Anki differs for `InvalidRegex` (both targets), `FieldCountMismatch` and the missing note type (F6 step 7): it refuses them inside its transaction, which empties its undo and redo histories; it keeps them for F7 step 1's refusal, made before the transaction (`specs/conformance/bulk-ops.md` X1).
- `col.mod` becomes `now_ms` when anything was written.

## F10. Live preview (Meguri, R1206)

Changed 2026-11-20 (round 8): the preview counts notes that would change and lists the first 100 changes (`browser/workspace` V9); F10's error rules stay.

Anki shows neither a count nor an error before Apply. Meguri's sheet shows, while the user types (U86's `find_replace_preview`, run as `browser/bulk-framework` says):

- **Matching notes**: the number of F8's notes that match (F6 step 3 for fields, F7 step 3 for tags) with the current inputs. It counts matches, so a note whose replacement would leave it as stored (F6 step 6) is counted here although Apply leaves it alone.
- **Invalid pattern**: F4 step 5's refusal as an inline error: the message of the pattern Apply would build (F4 steps 1 to 4), and a range when one can be found. Apply is disabled while it shows. With Tags, a Replace With text holding a separator shows F7 step 1's refusal the same way, with no range.
  - **The range** is a half-open interval of character offsets (Unicode scalar values) into the find text as typed, as `search/errors` E5's ranges are, and crosses the FFI in UTF-16 code units (`browser/result-sets` H15). The `regex` crate's compile error carries only a message, so the range comes from a second parse with `regex-syntax`'s parser (with the crate's default nesting limit), which reports a span with each of its errors: the find text as typed, without the `(?i)` prefix (case-insensitivity is set as a parser option instead) and without the NFC step, so its offsets are already offsets into the typed text, whatever NFC would change. Its byte offsets are converted to character offsets.
  - No range: in literal mode (only a size limit can refuse, step 5 of F4); when that parse finds no error, as for a pattern over the compiled-size limit, which only compiling finds; or when its error differs in kind from the refusal of the built pattern (possible only when NFC changes the text).
  - Example, regex mode, Ignore case on, the key on: the find text `e`, U+0301, `(`, `x` (four characters) is refused for an unclosed group; NFC would make it three characters, but the range is 2..3 in characters and 2..3 in UTF-16, the `(` as typed. With `😀(` the range is 1..2 in characters and 2..3 in UTF-16.

## F11. The operation (engine)

| Input | |
|---|---|
| target | note ids, or all notes |
| find, replace | strings |
| regex, ignore case | booleans |
| in | all fields, tags, or a field name |

- Refusals, each before anything is written, in this order: `InvalidArgument` for a target list that is empty or names a note twice (Anki takes an empty list for all notes and rewrites a repeated note once; no Meguri caller sends either), and for a field name that is empty (Anki takes an empty field name for All Fields; Meguri's sheet never offers one); `InvalidInput` (F7 step 1); `InvalidRegex{message}` (F4 step 5); then, while notes are read, `NotFound{noteId}` and `FieldCountMismatch{noteId}` (F6 step 7). F9 says what a refusal leaves of the undo and redo histories.
- Output: `{affected, considered}` (F9), with the change set: the notes written, the cards added, the tag rows registered.
- One engine op named `find_replace` (`engine/op-model` O1), source `UserUndoable`, one step named as F9 says. Above U24's row threshold it declares its write under the `bulk_op` catalog entry (O9), whose write phase is not cancellable. It reads the notes and computes the new rows in its compute phase and writes them in one transaction (`browser/bulk-framework`); a run cancelled before its commit leaves the collection and both histories exactly as they were and records no step (R1860).
- A field name that no note type of the target has is not an error: every note is skipped and the count is 0.

## F12. Scenarios

Every scenario below uses the `find_replace` op (`specs/conformance/bulk-ops.md` X1) and observes the rows with `db_dump`, as U86's execution note asks, with these exceptions: `field-names` uses the `find_replace_fields` observation (X3), `mixed-notetypes` uses it too and reads the note rows with `db_exec`, and `mode-counts` reads the note rows with `db_exec`. None is active. A row marked *held* is recorded with an oracle golden, committed on the branch `held/harvest-batch-2` and not on `master`: it lands with the change that makes Meguri's typed scenario model parse the ops, and is recorded again then (X5, which lists every held scenario). Meguri's executor does not have the op and the kind yet. A row marked *planned* has no scenario file. Each was written and recorded once (2026-11-10) but never committed, and the files are lost. What blocks them is the same as for the held rows: a scenario that uses `find_replace` does not parse yet, so `tools/mg goldens-check` refuses it on `master`. Each is written again and recorded onto a held branch, with the steps its row gives. Every undo observation follows X4 and `undo-ops.md` V5: a `db_exec` that is not a `select`, such as the `usn` reset, before the first `undo` or `undo_status`.

| Scenario | State | Covers |
|---|---|---|
| `browser/find-replace/ae47` | planned | AE47: F5 literal `$`, F6 named field across two note types, untouched notes keep `mod` and `usn`, the count, one undo step restoring every row. Ten notes of two note types, four of them with a `Meaning` field and three of those holding `cost`; Find `cost`, Replace With `$5`, In `Meaning`: count 3. `db_dump` of `notes`, `cards` and `col` before the op, after it and after one `undo`, which gives back every row and `col.mod`; `undo_status` gives `find_and_replace` to undo after the op, and nothing to undo with `find_and_replace` to redo after the undo |
| `browser/find-replace/regex` | planned | F4 regex mode, case flags (U+212A KELVIN SIGN stored raw with `db_exec`, since a save would compose it to `K`), literal meta characters, F5's replacement table, the empty find text |
| `browser/find-replace/all-fields` | planned | F6 All Fields over two note types, field name matched under `unicase`, markup in field text, `sfld` and `csum` recomputed, controls removed by the save, stored tags re-joined with canonical spacing but neither sorted nor registered (step 4) |
| `browser/find-replace/nfc` | planned | F4 step 4 with `normalize_note_text` on and off. A literal find text `e` U+0301 against a stored `café` (U+00E9): with the key set to true (`set_config`) it matches, count 1; with the key set to false it matches nothing, count 0, and matches a decomposed text stored raw with `db_exec`. With In set to Tags it never matches the stored, composed tag |
| `browser/find-replace/card-generation` | planned | F6 step 5: a field filled adds the card, a field emptied deletes none. Two notes of one note type each get a card added in one run: their decks (D3) and positions, one counter value per note in step 1's order, read with `db_exec` |
| `browser/find-replace/tags` | planned | F7: per-tag matching, anchors, deduplication, spelling and registration, empty results, literal `$`, the empty find text and a note without tags, the "Update Tag" step. Includes F7's table row ` new::x old::x ` with `old` replaced by `new`, stored as ` new::x `; `undo_status` gives `update_tag` to undo, and one `undo` removes the tag rows the run registered |
| `browser/find-replace/all-notes` | planned | F8: the all-notes target, a deleted note in the list skipped, a field name no note type has |
| `browser/find-replace/refusals` | planned | F4 step 5 in both targets, F7 step 1 (U+0020 and U+3000), F6 step 7 with a damaged note matched and not matched, nothing written. `invalid_regex` with `in: all`, with a field and with `in: tags`; `space_in_replacement` once with U+0020 and once with U+3000, run first, after an undoable step, and `undo_status` still names that step (both executors keep the history there); `field_count` for a note given one value too many in `flds` with `db_exec`, then a run whose pattern misses that note, count 1. `db_dump` after each refusal shows nothing written. `undo_status` after `invalid_regex` or `field_count` only where the history was already empty (X1) |
| `browser/find-replace/no-match` | planned | F9: count 0, no step, `col.mod` kept, the earlier step still undoable, the redo history emptied. After the `usn` reset: a run in Tags that writes ("Update Tag"), a run in a field that writes ("Find and Replace"), one `undo` of the second, then a run in a field that matches nothing: count 0, `col.mod` as the undo left it (`db_dump` of `col`), and `undo_status` gives `update_tag` to undo and nothing to redo |
| `browser/find-replace/unchanged-match` | planned | F6 step 6 and F7 step 5 on Anki: matched notes stamped and counted although unchanged; deviation `find-replace-unchanged-match`. Four steps, each after a `usn` reset and an `advance`, each observed with `db_dump` of `notes`, `cards` and `col`: literal `a` replaced by `a`; regex `(x)` replaced by `$1`; an empty find text and an empty replacement in a named field; In Tags with a replacement equal to the matched tag text. On Anki each counts 1 and stamps the matched note (`mod`, `usn` -1) and `col.mod` |
| `browser/find-replace/field-names` | held | F3 |
| `browser/find-replace/mode-counts` | held | F4 steps 1 to 3 (a dot that is literal with regex off and matches any character with regex on, Ignore case on by default), F6 steps 2, 3 and 8 (All Fields rewriting a different field in each of two notes, then a named field rewriting only the note that holds the text there), and a count of 0 that writes nothing |
| `browser/find-replace/literal-dollar-replacement` | held | F5: in literal mode a replacement holding `$3`, `${1}`, `$$` and `$name` is stored as typed in every field it lands in; in regex mode a braced group reference expands, `$$` becomes `$` and a reference to a group the pattern does not have becomes nothing; untouched notes keep `mod` and `usn` |
| `browser/find-replace/whole-collection-count` | held | F8, F9, F6 step 8: the all-notes target over two note types counts and writes only the matching notes, Ignore case on by default; a named field skips the note type without it; count 0 writes nothing and keeps `col.mod` |
| `browser/find-replace/mixed-notetypes` | held | F3, F4 step 2, F6 steps 2, 3 and 8 over notes of three note types. A regex with its own inline case flag (Ignore case off) rewrites the matching fields of every note type, and only notes with a match are written and counted. The In list for the whole selection is the union of the note types' field names, a shared name listed once, and a smaller selection lists only its own note type's names. A replacement limited to one field skips the notes whose note type lacks that field, even when other fields hold the find text, and changes only that field in the others |
| `notes/transform/replace-saves` | held | F6 steps 4 and 5 in field mode: a rewritten note is stamped (`mod`, `usn` -1); its stored tags are re-joined with canonical spacing, neither sorted nor given the case of a registered tag, and none is registered; a card is added when an empty field gets filled; no card is deleted when a field becomes empty; unlisted notes are untouched |
| `tags/find-replace/listed-notes` | held | F7 steps 2 to 4 and 6, F8: a literal, case-insensitive find on listed notes (mixed-case find text, lower-case stored tag). Only the listed note with a match is written, and only its tags, `mod` and `usn` change. An unlisted note with the same tag, and a listed note with no match, are not written. The old row stays registered |
| `tags/find-replace/all-notes` | held | F8 all notes with In Tags: every note is read. Only the matched text of a component is replaced, so a child tag keeps its later components. Untagged and unmatched notes are untouched |
| `tags/find-replace/pattern-modes` | held | F4 steps 1 to 3 in Tags: a literal versus regex quantifier, Ignore case off missing a case difference (count 0, nothing written), an anchored alternation, and an anchored class with an empty replacement dropping two whole tags (F7 step 4) |
| `tags/find-replace/invalid-regex` | held | F7 step 2 bringing in F4 step 5: the `invalid_regex` refusal leaves the note and the registry unchanged. The same text in literal mode is valid and matches nothing |

Meguri's tests (U86, `core/crates/meguri-engine/tests/find_replace.rs`) cover what no oracle shows: F2's sheet state and history, F3's all-notes listing, F6 step 6 and F7 step 5 on Meguri's side (the deviation's test), F10's preview count, ranges (F10's two examples) and supersession, F11's `InvalidArgument` cases, `NotFound` for a note of a missing note type, both histories kept after each refusal, cancellation before commit leaving the database digest equal and no step, and R1832's budget (150k notes in at most 5 s with progress, U242).

## F13. Differences from Anki

| Difference | Why | Where |
|---|---|---|
| A matched note whose rewrite leaves it as stored is not written or counted | R1204 | F6 step 6, F7 step 5; deviation `find-replace-unchanged-match` |
| History of 30 entries (Anki: 50) | R1201 | F2 |
| In choice remembered across app launches | R1201 | F2 |
| All-notes field list from every note type | U86 design | F3 |
| Live matching count and inline pattern error | R1206 | F10 |
| A target naming a note twice, an empty target list or an empty field name is refused | engine invariant | F11 |
| A refusal keeps the undo and redo histories (Anki empties both for an invalid pattern, a field-count mismatch and a missing note type) | `engine/undo` M2 | F9 |
| An empty Find or Replace With text is not entered in its history | native history behavior | F2 |
