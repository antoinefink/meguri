# Bulk-action ops: find_replace, change_deck, find_replace_fields

Spec: conformance/bulk-ops
Requirements: R1883, R1200, R1202, R1203, R1204, R1205, R1207, R1187, R1188
Anki: 26.09.3
Scenarios: conformance/scenarios/browser/find-replace/, conformance/scenarios/notes/change-deck/, conformance/scenarios/notes/transform/

The scenario ops and the observation kind that Browse's Find and Replace (`browser/find-replace`) and Change Deck (`browser/bulk-actions` B7.2) need. Every other bulk action of `browser/bulk-actions` runs on ops the language already has (`sched-ops.md`, `tag-ops.md`, `record-ops.md`, `change-notetype-ops.md`, `package-ops.md`). Both executors implement them with the meanings below. The ops' shapes are draft (`dsl.md` D7.2), and the oracle implements all three. Changed 2026-11-21 (implementation review): Meguri runs `find_replace` and `find_replace_fields` (U86); its `change_deck` comes with U87 (X5).

## X1. `find_replace`

- **Argument** `{notes?, find, replace, regex?, ignore_case?, in?}`:

  | Key | Value | Default |
  |---|---|---|
  | `notes` | a non-empty list of distinct note references (`#nN`) | missing: all notes (`browser/find-replace` F8) |
  | `find` | string, possibly empty | required |
  | `replace` | string, possibly empty | required |
  | `regex` | boolean: Treat input as regular expression | `false` |
  | `ignore_case` | boolean | `true` (the sheet's first-time state, F2) |
  | `in` | `all` (All Fields), `tags`, or `{field: <name>}` (a field name, matched under `unicase`) | `all` |

  An empty `notes` list, a repeated reference, any other key or another value shape is `invalid_scenario`. A reference to a note that was deleted is allowed: the operation skips it, and it still counts among the notes considered (F8), which this op does not output.
- **Effect.** `browser/find-replace` F4 to F7 on the listed notes, or on all notes, as one operation with its undo step (F9).
- **Output** `{count}`: the notes written (F6 step 8, F7 step 6), Anki's count on the oracle (which counts every matched note, F6 step 6).
- **Rejections** (`dsl.md` D7.3), each changing nothing:

  | Kind | When |
  |---|---|
  | `invalid_regex` | the pattern does not compile (F4 step 5) |
  | `space_in_replacement` | `in: tags` and `replace` holds U+0020 or U+3000 (F7 step 1) |
  | `field_count` | `in` is `all` or a field, and a matched note's stored field count differs from its note type's (F6 step 7) |

- **Oracle.** For `all` and a field: the collection's find-and-replace call with the listed real note ids in list order (an empty list for all notes), the find and replacement texts, the regex flag, match case = not `ignore_case`, and the field name (none for `all`); for `tags`: the tag find-and-replace call with the same ids, texts and flags. Its count is the output. Anki gives all three refusals the same invalid-input error kind (its invalid-regex error has that kind), so the driver names them: with `tags`, a replacement holding U+0020 or U+3000 is `space_in_replacement` (Anki checks it before it builds the pattern) and any other such refusal `invalid_regex`; with `all` or a field, the driver asks again with the same texts and flags and one note id that no note has, and only a pattern that does not compile fails again, `invalid_regex`; otherwise the refusal is `field_count`. Any other error is a run error. A listed note whose note type is missing (`browser/find-replace` F6 step 7, Meguri's `NotetypeMissing`; changed 2026-11-21, implementation review: the code's name) also fails with that kind and passes the second call, so the driver would name it `field_count`; no scenario lists such a note, and one that does needs its own rejection kind first.
- **Undo history on the oracle.** `invalid_regex` (both targets) and `field_count` fail inside Anki's transaction, which empties its undo and redo histories and its study queues (D7.3's exceptions); `space_in_replacement` keeps them. Recorded by the driver's tests. Meguri keeps both histories after every refusal (`browser/find-replace` F9, F13). Scenarios observe `undo_status`, `undo` or `redo` after such a refusal only when the history was already empty; one that does otherwise needs a deviation entry.
- **Meguri.** The engine's find-and-replace op (`browser/find-replace` F11) with the all-notes target when `notes` is missing; `InvalidRegex`, `InvalidInput` and `FieldCountMismatch` map to the three kinds. Its `affected` is the output. Changed 2026-11-21 (implementation review): from U265 (PR #78), the executor reads the preview at the step and passes its count as the confirmed count (F11), so that refusal never occurs in a scenario.

## X2. `change_deck`

- **Argument** `{cards, deck}`: `cards` a non-empty list of distinct card references; `deck` a deck the collection holds, by its stored name in human form (a `setup.decks` entry, a deck an op created, or a filtered deck a `db_exec` inserted, `deck-ops.md` K13). An unknown name is `unknown_ref`; another shape `invalid_scenario`. A reference to a card that was deleted is passed on with its real id, and the operation skips it (`browser/bulk-actions` B7.2 step 2).
- **Effect.** `browser/bulk-actions` B7.2 on the listed cards with that deck as target, as one "Change Deck" operation.
- **Output** `{count}`: the cards moved (B7.2 step 5); cards that only the memory-state step wrote are not counted.
- **Rejection** `filtered_deck`: the deck is filtered (B7.2 step 1). Nothing changes.
- **Oracle.** The collection's set-deck call with the listed real card ids in list order and the deck's id; its count is the output, and its filtered-deck error is `filtered_deck`.
- **Meguri.** The engine's Change Deck op (U87, B7.2); `FilteredDeck` maps to `filtered_deck`.

## X3. `find_replace_fields` (observation kind)

- **Request** `{find_replace_fields: {notes: [#nN, ...]}}`, a non-empty list of distinct note references; at most one request per observe step.
- **Value** `[<field name>, ...]`: the field names Find and Replace's In list offers for those notes (`browser/find-replace` F3), in order. Deleted notes add nothing.
- Never changes the collection.
- **Oracle.** The collection's field-names-for-notes call with the real note ids.
- **Meguri.** `selection_summary`'s field names for those notes (`browser/bulk-framework`).
- Anki picks the spelling of names that `unicase` finds equal as SQLite returns them (F3 step 3). Scenarios never give two note types field names that are equal under `unicase` but spelled differently.

## X4. Writing bulk-action scenarios

- Follow `undo-ops.md` V5 (a `db_exec` that is not a `select` before the first `undo`, `redo` or `undo_status`) and `sched-ops.md` J10 (FSRS, the day, stamps, filtered decks inserted with `db_exec`).
- Set `usn` to 0 on the rows a scenario reads (`UPDATE notes SET usn = 0`, the same for cards) and `advance` before the op, so that written rows show `usn` -1 and a new `mod`, and untouched rows show neither.
- Observe results with `db_dump` limited to the tables the step can change (`notes`, `cards`, `tags`, `col`, `graves`, `revlog`) and read config keys with `db_exec`: id-named keys and their real ids stay out of goldens (`conformance/AGENTS.md`, add-defaults).
- Cards that `find_replace` adds have no logical id: read them with `db_exec` (`note-ops.md` N3), and keep one card added per note so that their order in id order is fixed.
- `find_replace` with `in: tags`, and any op that registers tags, follows the spelling rules of `tags/tags` T3: keep names in one case, or record a deviation entry for implicit-parent spellings (`conformance/AGENTS.md`, tag scenarios).
- A scenario that shows Anki's stamping of unchanged matches (F6 step 6) is `browser/find-replace/unchanged-match`; keep such matches out of every other scenario, so the deviation `find-replace-unchanged-match` names only that one.

## X5. Status

- **Oracle driver.** X1 to X3 exist (2026-11-10, `oracle/driver/ops/bulk.py`), with `dsl.md` D7.2's `find_replace` and `change_deck` rows, `observations.md` O4's row, both schemas and `protocol.json`. The oracle records scenarios that use them. Changed 2026-11-21 (implementation review): the scenarios that use X1 and X3 are on `master`; those that use X2 are not yet (next bullet).
- **Meguri's typed scenario model.** Changed 2026-11-21 (implementation review): Meguri's typed scenario model parses `find_replace` and `find_replace_fields` with the shapes of X1 and X3 (U86). It does not parse `change_deck` yet (U87), so a scenario that uses it stays off `master`. Landed with U86, unchanged and active: `browser/find-replace/` `field-names`, `literal-dollar-replacement`, `mixed-notetypes`, `mode-counts`, `whole-collection-count`; `notes/transform/replace-saves`; `tags/find-replace/` `all-notes`, `invalid-regex`, `listed-notes`, `pattern-modes`. Still held on `held/harvest-batch-2`: `notes/change-deck/` `fsrs-off`, `memory-states`, `remaining-steps`, `target-state-kept`, and `search/state/memory-state-moves`.

  Planned, with no scenario file: `browser/find-replace/` `ae47`, `regex`, `all-fields`, `nfc`, `card-generation`, `tags`, `all-notes`, `refusals`, `no-match` and `unchanged-match`. They were written and recorded once (2026-11-10) but never committed, and the files are lost. Changed 2026-11-21 (implementation review): each is still to be written again from its row in `browser/find-replace` F12, recorded, and committed on `master` with its golden; nothing holds it off `master`, since `find_replace` now parses.
- **Meguri's executor.** Changed 2026-11-21 (implementation review): the browser adapter runs X1 and X3 (U86, `core/crates/meguri-testkit/src/exec/bulk.rs`); X2 waits for U87.
- **Deviation.** `find-replace-unchanged-match` (R1204, R1207): on `browser/find-replace/unchanged-match`, the counts and the rows of the notes whose match leaves them as stored are Meguri's (not written, not counted), where Anki writes and counts them. The entry's pointers are each step's `count` output (Meguri: 0), the `mod` and `usn` of the matched note rows in each step's `db_dump` (Meguri: as before the step), and `col.mod` there (Meguri: unchanged). That scenario is planned (above), so the entry has no golden to apply to yet: the implementer adds it with its test once the scenario and its golden land (changed 2026-11-21, implementation review: U86 landed without it).
