# Timed operations (U18)

Read at tag 26.09.3 (`refs/anki-26.09.3`) for `specs/conformance/timed-operations.md`
(the `tools/mg oracle time` command, its protocol, catalogue and report). No code
was copied. The research note behind the operation list is
`local-data/research/13-conformance-oracle.md` §5 item 9 and §6.

| Anki file | Read for |
|---|---|
| `pylib/anki/collection.py` (`__init__`, `reopen`, `close`, `close_for_full_sync`, `_load_scheduler`) | T3.1, T6.1: what opening a collection does in the Python layer on top of the backend; a backend serves one open collection at a time and the desktop client keeps one for the profile |
| `qt/aqt/main.py` (`disable_automatic_garbage_collection`, `garbage_collect_now`) | T3.3: the desktop client turns automatic garbage collection off and collects explicitly |
| `pylib/rsbridge/lib.rs` | T5.5: backend calls release the GIL, so an in-process stub thread can serve while a backend call waits on it |
| `pylib/anki/scheduler/base.py` (`deck_due_tree`), `rslib/src/decks/tree.rs` | T5.1, T6.2: the deck list is computed at the current time with the root and the hidden empty default deck |
| `pylib/anki/scheduler/v3.py` (`get_queued_cards`, `build_answer`, `answer_card`), `qt/aqt/reviewer.py` | T6.3, T6.4: the reviewer fetches one card (with its next states and counts), answers with the states it fetched, then fetches again; the queue is updated in place after an answer |
| `rslib/src/search/sqlwriter.rs` (`write_deck`, `write_tag`, `write_rated`), `rslib/src/search/parser.rs` | T6.5: `deck:current`, `deck:*`, the tag pattern path for a non-trivial `tag:` value, `re:`, `prop:`, `rated:` all exist at the tag and need no escaping in the chosen queries |
| `pylib/anki/collection.py` (`find_cards`, `_build_sort_mode`), `rslib/src/browser_table.rs` (column keys) | T6.5, T6.6: no ordering versus a built-in column sort; the card-mode keys `noteFld` and `cardDue` |
| `proto/anki/stats.proto` (`GraphsRequest`, `GraphsResponse`) | T6.7: one request with a search and a day range returns every graph's data; the card-count and daily review-count parts used by the check |
| `rslib/src/dbcheck.rs` (`check_database`), `rslib/src/storage/sqlite.rs` (`optimize`, `close`), `pylib/anki/collection.py` (`fix_integrity`) | T6.8: quick check, then vacuum, reindex and analyze, then the repairs in one transaction; the corrupt result |
| `rslib/src/import_export/package/apkg/export.rs`, `rslib/src/import_export/package/colpkg/export.rs`, `proto/anki/import_export.proto` (`ExportAnkiPackageOptions`) | T6.9, T6.10: current-format packages hold `meta`, `collection.anki21b`, a placeholder `collection.anki2` and `media`; apkg export returns the note count; colpkg export closes the collection first and the caller reopens |
| `rslib/src/import_export/package/apkg/import/mod.rs`, `rslib/src/import_export/package/meta.rs`, `proto/anki/import_export.proto` (`ImportAnkiPackageOptions`, `ImportResponse.Log`) | T5.4, T6.11: a package without `meta` and without `collection.anki21` is version 1 with `collection.anki2`; the inner file is opened as a collection (any schema Anki opens); the import options; the log's new-notes list |
| `rslib/src/sync/collection/upload.rs`, `download.rs`, `rslib/src/collection/mod.rs` (`before_upload`), `rslib/src/backend/sync.rs` (`full_sync_inner`), `pylib/anki/collection.py` (`full_upload_or_download`) | T6.12, T6.13: the local steps of a one-way upload and download, and that the backend reopens the collection after either |
| `rslib/src/sync/http_client/mod.rs`, `io_monitor.rs`, `rslib/src/sync/collection/protocol.rs`, `rslib/src/sync/request/header_and_stream.rs`, `rslib/src/sync/response.rs` | T5.5: endpoints `sync/upload` and `sync/download` under the endpoint URL, the `anki-sync` header, zstd request bodies, the required `anki-original-size` response header with a zstd body, and `OK` as the accepted-upload text |
| `rslib/src/deckconfig/update.rs`, `rslib/src/scheduler/fsrs/memory_state.rs` | T6.14: switching FSRS on recomputes memory states for every normal deck; switching it off clears them; no rescheduling unless asked |

The raw zstd frame of T5.5.1 was checked with the reference `zstd` command
(1 to 400,000 bytes, an exact multiple of 128 KiB, and empty content), not
taken from Anki.

The driver (`driver/timed_operations.py`, oracle maintainer) also read, at the
same tag, for how the desktop client makes each call:

| Anki file | Read for |
|---|---|
| `qt/aqt/browser/table/state.py` (`CardState.find_items`) | T6.6: the browser's card search is `find_cards(search, column, reverse)` |
| `pylib/anki/decks.py` (`update_deck_configs`, `get_deck_configs_for_update`), `ts/routes/deck-options/lib.ts` (`dataForSaving`), `proto/anki/deck_config.proto` | T6.14.1: the save's fields, the current preset last, limits and collection-wide options from the screen's state |
| `rslib/src/backend/mod.rs` (`web_client`), `rslib/src/sync/http_client/io_monitor.rs`, `full_sync.rs`, `rslib/src/sync/collection/upload.rs` | T5.5: one client per backend with the system proxy settings (hence `NO_PROXY` for the loopback stub), a streamed (chunked) zstd request body, the required `anki-original-size`, the stall timer on the monotonic clock |
| `rslib/src/stats/graphs/mod.rs` | T6.7: the revlog range a days value selects |

libfaketime 0.9.13 (`src/libfaketime.c`, not Anki) for T2.3: with
`FAKETIME_NO_CACHE` unset, the timestamp file is read again only after
`FAKETIME_CACHE_DURATION` real seconds.
