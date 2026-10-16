# Browser and search configuration keys (U83)

Spec-writer notes for `specs/browser/config-keys.md` and
`specs/conformance/browser-config-ops.md`. Read at tag 26.09.3
(`refs/anki-26.09.3`); no code was copied. Black-box probes ran in the oracle
image (`meguri-oracle:26.9.3-*`) through the `anki` Python API with no network:
the keys of a new collection, every typed read on absent keys and on values of
other shapes (K2), the compact bytes of each write, an unchanged write, the
insertion of a default value, and the undo status around a write without an
undo entry (an earlier "Add Note" stays undoable; a pending redo is dropped).

| Spec section | Anki files read | Read for |
|---|---|---|
| config-keys K1, K2.1, K2.2 | `rslib/src/config/bool.rs`, `rslib/src/config/string.rs`, `rslib/src/config/mod.rs`, `rslib/src/backend/config.rs`, `rslib/src/storage/config/mod.rs`, `rslib/src/config/schema11.rs`, `proto/anki/config.proto`, `pylib/anki/collection.py` | key names, defaults (two boolean keys default to true), typed reads falling back on any decode failure, the generic read used by the browser, keys of a new collection |
| config-keys K3 | `rslib/src/browser_table.rs` | the 19 column keys, sortability per mode, first sort direction |
| config-keys K4 | `pylib/anki/browser.py`, `pylib/anki/collection.py`, `qt/aqt/browser/table/state.py`, `qt/aqt/browser/table/model.py`, `qt/aqt/browser/table/table.py` | default column lists, toggling (append at the end, remove the first match), unknown keys shown as add-on columns, column order kept in local header state |
| config-keys K5 | `qt/aqt/browser/table/state.py`, `qt/aqt/browser/table/model.py`, `qt/aqt/browser/table/table.py`, `pylib/anki/collection.py` | sort keys per mode, fallback for unknown or missing columns, no order for unsortable columns, header-click direction |
| config-keys K6.1 | `qt/aqt/browser/table/table.py` | the notes-mode toggle write |
| config-keys K6.2 | `qt/aqt/browser/sidebar/tree.py` | section-to-key mapping, collapsed state written on expand and collapse |
| config-keys K6.3 | `qt/aqt/browser/previewer.py` | meaning of the both-sides key |
| config-keys K6.4 | `qt/aqt/browser/browser.py` | default search when the trimmed text is non-empty, else `deck:current` with an empty field |
| config-keys K6.5 | `rslib/src/search/sqlwriter.rs`, `rslib/src/preferences.rs` | where the two search keys are read and written |
| config-keys K6.6 | `qt/aqt/browser/browser.py`, `qt/aqt/operations/scheduling.py`, `rslib/src/scheduler/reviews.rs`, `rslib/src/scheduler/new.rs` | remembered Set Due Date text and Reset options written inside their operations, browser and reviewer keys kept apart |
| config-keys K7 | `qt/aqt/browser/sidebar/tree.py`, `qt/aqt/utils.py` | saved-search save, update, rename (quotes removed), delete, drop onto the section, listing order |
| config-keys K8 | `qt/aqt/flags.py`, `qt/aqt/browser/sidebar/tree.py` | flag labels (specified in `specs/tags/flags-and-marked.md` F4) |
| config-keys K9 | `rslib/src/scheduler/new.rs` | Reposition writes its two option keys |
| config-keys K10.2 | `rslib/src/undo/mod.rs`, `rslib/src/collection/transact.rs`, `rslib/src/preferences.rs`, `pylib/anki/collection.py` | writes without an undo step keep the undo queue and drop redo; preferences are one undoable step |
| browser-config-ops B1, B2 | `pylib/anki/collection.py`, `rslib/src/backend/config.rs` | typed setters and getters the driver should call; the generic read returns the default for a missing, non-BLOB or non-JSON value |

Driver hints for the oracle maintainer (U83 role work):

- `browser_config_set {key, value}`: validate the key against B1's closed list and the value's JSON type (booleans are not integers; an array must hold strings only; an object's members must be strings), else `invalid_scenario`. Call `col.set_config_bool(Config.Bool.<KEY>, value)` for the twelve boolean keys that have one, `col.set_config_string(Config.String.<KEY>, value)` for `setDueBrowser` and `defaultSearchText`, and `col.set_config(key, value)` for `sortType`, `noteSortType`, `sortBackwards`, `browserNoteSortBackwards`, `activeCols`, `activeNoteCols`, `savedFilters`, `flagLabels`. All three default to no undo entry. No output.
- `browser_config`: `col.get_config_bool(...)` for the thirteen boolean keys (`normalize_note_text` included), `col.get_config_string(...)` for the two string keys, and `col.get_config(key, default)` for the other eight with defaults `["noteFld","template","cardDue","deck"]`, `["noteFld","note","template","noteTags"]`, `None`, `None`, `False`, `False`, `{}`, `{}`. Do not call the column loaders that also set the backend's active columns. Emit floats as floats (`1.0`, `-0.0`) and integers as integers, as `db_exec` does.
- Expected values were checked by the probes: `browser/config/wrong-shapes` gives `false` for every boolean key except `collapseDecks` (` true `) and `previewBothSides` (`true`), `true` for `restorePositionBrowser` and `normalize_note_text`, `"0"` and `""` for the string keys, then `""` and `"  deck:x "` after the second insert; the default card column list for `activeCols` (TEXT), `["note", 5, null]`, `"question"`, `null`, `1`, `false`, `{"a": "y", "b": 3}` and `[]` for the other untyped keys.
