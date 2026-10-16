# Browser config ops: writing and reading the browser keys

Spec: conformance/browser-config-ops
Requirements: R1883, R1265, R1266, R1138
Anki: 26.09.3
Scenarios: conformance/scenarios/browser/config/

One scenario op (`dsl.md` D7.2) and one observation kind (`observations.md` O4) for the browser and search config keys of `specs/browser/config-keys.md`. Both executors implement them with the meanings below; `conformance/schema/scenario.schema.json` and `observation.schema.json` hold their shapes. The oracle runs Anki's collection library, not its desktop browser: the op writes a whole value and the kind reports what Anki's own reads give, never the browser's interpretation (column display, sort fallback, saved-search actions), which Meguri's tests cover.

## B1. `browser_config_set`

- **Argument** `{key, value}`: `key` one of the 22 keys below, `value` of the key's shape. Any other key, or a value of another JSON type, is `invalid_scenario`.

  | Keys | Value |
  |---|---|
  | `browserTableShowNotesMode`, `sortBackwards`, `browserNoteSortBackwards`, `collapseSavedSearches`, `collapseToday`, `collapseFlags`, `collapseCardState`, `collapseDecks`, `collapseNotetypes`, `collapseTags`, `previewBothSides`, `ignoreAccentsInSearch`, `restorePositionBrowser`, `resetCountsBrowser` | boolean |
  | `sortType`, `noteSortType`, `setDueBrowser`, `defaultSearchText` | string |
  | `activeCols`, `activeNoteCols` | array of strings (any strings: unknown column keys allowed) |
  | `savedFilters`, `flagLabels` | object whose member values are strings |

  `normalize_note_text` is not in the list: Meguri never writes it. Scenarios set it with `set_config` (`record-ops.md` W4).
- **Effect** sets the key to the value as one operation with no undo entry, by `storage/config-table` C4.
  - Oracle: the collection's typed setter where Anki has one (the boolean setter for the `collapse*` keys, `browserTableShowNotesMode`, `previewBothSides`, `ignoreAccentsInSearch`, `restorePositionBrowser`, `resetCountsBrowser`; the string setter for `setDueBrowser` and `defaultSearchText`), the generic config setter for the other eight; each without an undo entry. Anki writes when the value's compact bytes differ from the stored bytes.
  - Meguri: the key's whole-value write (`browser/config-keys` K10.1). Meguri writes when the value differs from the stored value (deviation `config-same-value-other-bytes`, `storage/config-table` C4): scenarios that run this op on a value stored with other spacing or member order apply that deviation, or avoid the case.
- **Output** none (`{}`).

## B2. `browser_config` (observation kind)

- **Request** `browser_config`, no arguments.
- **Value** an object with exactly these 23 members, each the value Anki's own read of the key gives:

  | Members | Value |
  |---|---|
  | `browserTableShowNotesMode`, `collapseSavedSearches`, `collapseToday`, `collapseFlags`, `collapseCardState`, `collapseDecks`, `collapseNotetypes`, `collapseTags`, `previewBothSides`, `ignoreAccentsInSearch`, `restorePositionBrowser`, `resetCountsBrowser`, `normalize_note_text` | boolean: `browser/config-keys` K2.1 with the K1 default (true for `restorePositionBrowser` and `normalize_note_text`) |
  | `setDueBrowser`, `defaultSearchText` | string: K2.2 (`"0"`, `""` by default) |
  | `activeCols`, `activeNoteCols`, `sortType`, `noteSortType`, `sortBackwards`, `browserNoteSortBackwards`, `savedFilters`, `flagLabels` | the stored JSON value as `storage/config-table` C3 reads it, whatever its shape (`1`, `"question"` and `[]` are reported as such; repeated member names keep the last); when the key is unreadable (C3: missing, not BLOB, not JSON), the default: the K1 column lists for the two column keys, `null` for `sortType` and `noteSortType`, `false` for the two backwards keys, `{}` for `savedFilters` and `flagLabels` |

  Anki has no typed read for the last eight keys outside its desktop browser; the kind reports what its generic config read returns with the default its browser passes. Meguri's adapter reports the same from its raw read of the key, not from K4, K5 or K7's interpretation.
- Numbers keep their kind (`observations.md` O6): `1` an integer, `1.0` and `-0` floats (C3).
- Nothing in the collection changes: no row is written and `col.mod` does not move (R1138).

## B3. Status

- The oracle driver implements B1 and B2 (stable in `dsl.md` D7.2 and `observations.md` O4). Every scenario under `conformance/scenarios/browser/config/` has a golden; the oracle ran every segment of `browser/config/live-roundtrip` to record it.
- Meguri's executor implements them with U83's accessor (`core/crates/meguri-search/src/config.rs`) and write ops; until then the scenarios are `pending` (`dsl.md` D10). Until the testkit model (`core/crates/meguri-testkit/src/scenario/`) reads the op and the kind, T0 reports the scenarios that use them as `invalid`; U83's implementer adds the typed arguments with the adapter.
- What else the scenarios need (`dsl.md` D8): `absent-defaults`, `wrong-shapes`, `write-shapes` and `other-keys-untouched` start with `open` of `storage/schema-18.anki2` (`storage-ops.md` P1) and use `db_exec`, and `other-keys-untouched` also `set_config` (`record-ops.md` W1, W4); none of them needs the new collection. `new-collection` needs Meguri's new collection (U28). `live-roundtrip` needs `handoff` and the ops of Meguri's segment, which opens the oracle's collection (D9.2).
