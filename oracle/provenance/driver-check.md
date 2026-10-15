# Oracle driver: collection check and check_db (U11)

Read at tag 26.09.3 (`refs/anki-26.09.3`) to choose the `anki` Python APIs
behind `tools/mg oracle check` (`driver/collection_check.py`, the harness's
`check`) and the `check_db` op (`driver/ops/maintenance.py`); meanings in
`specs/conformance/collection-check.md`. No code was copied. The spec's own
sources are in `fixtures-synthetic.md`.

| Anki file | Read for |
|---|---|
| `qt/aqt/dbcheck.py`, `pylib/anki/collection.py` (`fix_integrity`) | what the desktop command runs: a backup, then the backend's check database; problems come back as a list of messages, a corrupt file as a DB error |
| `rslib/src/collection/service.rs`, `rslib/src/dbcheck.rs` | the backend returns only the localized messages, one per kind with a nonzero counter; which kinds carry a number; the corrupt result is returned before optimize and before any repair |
| `ftl/core/database-check.ftl` | each message's plural forms; the recovered note types message has no number |
| `rslib/i18n/src/lib.rs` | numbers are formatted without grouping; placeables are wrapped in isolation marks when languages are given (why the driver renders each message with the backend's own translations instead of matching text) |
| `rslib/src/error/db.rs`, `pylib/anki/errors.py` | the corrupt DB error's text is the corrupt message itself; `DBError`, `TemplateError` and `NotFoundError` classes |
| `pylib/anki/_backend.py`, `pylib/anki/lang.py` | backend languages and the default `en` |
| `qt/aqt/main.py` (`_loadCollection`) | the desktop opens a collection as `Collection(path, backend=...)` and nothing else |
| `pylib/anki/collection.py` (`__init__`, `reopen`, `_load_scheduler`, `close`), `rslib/src/storage/sqlite.rs` | opening writes only `creationOffset` (v3 without it) and nothing for a file already at schema 18; the connection holds an exclusive lock, so table digests read the file while it is closed; `quick_check` and `optimize` |
| `rslib/src/storage/dbcheck/mod.rs`, `invalid_ids_count.sql`, `dbcheck.rs` (`maybe_fix_invalid_ids`) | the future-id cutoff, for the frozen-clock driver test |
| `rslib/src/storage/card/mod.rs`, `fix_due_new.sql`, `fix_due_other.sql`, `fix_ivl.sql`, `rslib/src/storage/revlog/fix_props.sql` | the stored states the driver tests plant for each kind |
| `pylib/anki/scheduler/base.py` (`deck_due_tree`), `pylib/anki/decks.py` (`id_for_name`, `find_deck_in_tree`), `proto/anki/decks.proto`, `qt/aqt/deckbrowser.py` | the deck list's three numbers per row (with children, limits applied), computed at the frozen clock |
| `pylib/anki/cards.py` (`render_output`), `pylib/anki/template.py` (`TemplateRenderContext`), `rslib/src/notetype/render.rs`, `rslib/src/template.rs` | rendering an existing card for review; a template error is raised by the partial render and turned into text by `render()`; the empty-front flag; question and answer text without CSS |
| `pylib/anki/dbproxy.py` | for the driver tests: blob parameters are not accepted, blobs come back as lists of byte values |

Probed in the oracle image (`driver/tests/test_check.py`): a header free page
count larger than the free list makes `quick_check` fail while Anki still opens
the file and counts rows, so Check Database returns the corrupt error; a
`sqlite3` reader of an Anki collection needs a `unicase` collation registered
to open cursors on its indexes.
