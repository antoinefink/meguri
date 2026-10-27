# Oracle driver: browser config ops (U83)

Read at tag 26.09.3 to choose the `anki` Python APIs behind
`browser_config_set` and the `browser_config` observation
(`driver/ops/browser.py`; meanings in
`specs/conformance/browser-config-ops.md` B1, B2), following the spec writer's
hints in `browser-config.md`; no code was copied.

| Anki file | Read for |
|---|---|
| `pylib/anki/collection.py` | `get_config` (default on a missing key), `set_config`, `get_config_bool`, `set_config_bool`, `get_config_string`, `set_config_string` (no undo entry by default); the column loaders also set the backend's active columns, so the driver does not call them |
| `pylib/anki/config.py` | `Config` is the protobuf `ConfigKey`; the generic read turns the backend's not-found into the caller's default |
| `pylib/anki/browser.py` | `BrowserDefaults` card and note column lists, the defaults the browser passes to the generic read |
| `pylib/anki/utils.py` | values cross the bridge as JSON through orjson, so number kinds are kept both ways |
| `proto/anki/config.proto` | `ConfigKey.Bool` and `ConfigKey.String` member names |
| `rslib/src/config/bool.rs`, `rslib/src/config/string.rs` | enum member to key name (camelCase, `normalize_note_text`), typed defaults, a write without an undo entry runs as a skip-undo operation |
| `rslib/src/config/mod.rs`, `rslib/src/storage/config/mod.rs`, `rslib/src/backend/config.rs` | a missing, non-BLOB or non-JSON value reads as absent; the generic read returns the stored JSON re-encoded |

Probed in the oracle image (`driver/tests/test_browser.py`): each of the 22
keys written once is stored as its compact JSON with members sorted, usn -1 and
the frozen time; writing the same value again stamps nothing and leaves
`col.mod`; the observation on absent keys gives B2's defaults and writes
nothing; stored values of other shapes, TEXT values and bytes that are not JSON
read as the spec says; `1.0`, `-0.0` and `1e16` come back as floats and
integers as integers; an earlier note edit stays the undo step after a write
through each of the three setters.

## Sorted searches and Browse rows (U84 held scenarios)

Ported from the U84 oracle maintainer's driver work on `spec/U84` (session
s-20261016T100754Z-fdaa69c8) to `observe/search.py` (`order`) and
`observe/browser_rows.py` (`browser_rows`); meanings in
`specs/conformance/browser-ops.md` B1 to B4. Read at tag 26.09.3: besides the
files listed in `browser-columns-sorting.md` for B1 to B4, that work read
`rslib/src/backend/error.rs` (row errors by backend error kind),
`rslib/src/config/bool.rs` and `rslib/src/config/mod.rs` (Anki's own reading of
`browserTableShowNotesMode`), `rslib/src/scheduler/mod.rs` (the first day
computation writes `rollover`) and `pylib/anki/_backend.py` (the kinds pylib
maps to classes). No code was copied. The row probe is `card_excerpts`'; its
`missing` is B3's `deleted`.

`edit_note` keeps giving the cards it adds no logical id (`note-ops.md` N3), so
the three scenarios that name such a card run `adopt` after the edit instead of
the `edit_note` numbering U84 had added. Re-recorded at the current pin, all 22
goldens equal U84's apart from the `adopt` outputs and the field values that
the privacy scan made `sort-field` and `question-answer` rename.
