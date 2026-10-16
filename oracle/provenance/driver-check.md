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

## Schedule (K11, U15)

Read at tag 26.09.3 for `tools/mg oracle check --schedule`
(`collection_check.schedule_report`). No code was copied.

| Anki file | Read for |
|---|---|
| `qt/aqt/deckbrowser.py`, `qt/aqt/operations/deck.py`, `pylib/anki/decks.py` (`set_current`, `select`) | clicking a deck in the deck list sets the current deck through the backend, nothing else |
| `rslib/src/decks/current.rs` | setting another current deck drops the cached study queues; the next fetch builds them for that deck; the current deck is always a deck id (a missing one falls back to the default deck), so every queue is a deck's and the root has none (K11.3 `root`) |
| `pylib/anki/scheduler/v3.py`, `proto/anki/scheduler.proto` (`QueuedCards`) | `get_queued_cards(fetch_limit, intraday_learning_only)`, each entry's queue (new, learning, review) and the three counts |
| `rslib/src/scheduler/queue/mod.rs`, `learning.rs`, `entry.rs` | the fetch reads the queue in presentation order (learning due now, the main queue, learning within the learn-ahead limit) up to the fetch limit; interday learning entries report as learning; counts refresh the learning cutoff only when all are zero |
| `rslib/src/scheduler/queue/builder/mod.rs`, `gathering.rs`, `rslib/src/storage/deck/mod.rs` (`update_active_decks`) | the queue is built for the current deck and its children, burying siblings as it gathers; the load balancer, when on, is built over every deck's preset; active decks go to a temporary table, not the file |
| `rslib/src/scheduler/answering/mod.rs` (`get_scheduling_states`) | next states use the load balancer of the queues cached at that moment, so they are read after the last queue is built |
| `rslib/src/decks/tree.rs` (`deck_tree`, `hide_default_deck`) | the root node's numbers; an empty default deck without children is left out of the tree when other decks exist |

Probed in the oracle image: on the 10k generated collection a deck's queue
counts can be lower than its deck-list numbers, because building the queue
buries siblings and the deck list does not; the driver tests compare the two
only on notes with one card each. Two runs at the same frozen instant give the
same `schedule.sha256`. So does a copy of that collection whose fields have
every ASCII letter and digit outside cloze openers replaced, whose decks carry
A11.1 rank names and whose tag components are hashed (a probe, not the
anonymizer: it left `sfld` and `csum` stale, so its Check Database rewrote
`notes`).
