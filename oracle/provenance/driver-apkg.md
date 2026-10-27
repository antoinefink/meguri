# Oracle driver and fixtures: deck packages (E6, E7, E10, E11)

Read at tag 26.09.3 to choose the `anki` calls behind the deck package forms
of `import` and `export` (`driver/ops/apkg.py`, reached from
`driver/ops/colpkg.py` and `driver/ops/package.py`), their refusals (a JSON
error is `corrupt`, `_IMPORT_CORRUPT_KINDS`; `Rejected` in `driver/errors.py`
notes E6.4's undo exception), the observations `package_media` and
`package_db_dump` (`driver/observe/package_contents.py`) and the refusal
fixtures (`fixtures/package/apkg.py`). Meanings are
`specs/conformance/package-ops.md` E6, E7, E10 and E11. No code was copied.

| Anki file | Read for |
|---|---|
| `pylib/anki/collection.py` (`import_anki_package`, `export_anki_package`, `pb_export_limit`, `DeckIdLimit`, `NoteIdsLimit`, `CardIdsLimit`), `pylib/anki/_backend.py` (`backend_exception_to_pylib`, `RustBackend`), `proto/anki/import_export.proto` | the two backend calls, their options, the update conditions, the export limit, the import log's lists and `found_notes`, and `MediaEntries`; a call through a probe in place of the RustBackend, as `ops/colpkg.py` does |
| `rslib/src/import_export/service.rs`, `rslib/src/import_export/package/apkg/import/mod.rs` (`import_apkg`, `Context::new`) | the import runs on the open collection; the zip is opened before the transaction, the five option keys, `meta`, the media map and the rest inside one "Import" transaction |
| `rslib/src/collection/transact.rs`, `rslib/src/undo/mod.rs` (`discard_undo_and_study_queues`) | an error inside a transaction rolls it back and empties the undo history and the study queues |
| `rslib/src/import_export/package/meta.rs`, `package/media.rs` (`extract_media_entries`, `safe_normalized_file_name`), `rslib/src/import_export/mod.rs` (`ImportError::message`), `rslib/src/error/mod.rs`, `rslib/src/backend/error.rs`, `ftl/core/importing.ftl`, `ftl/core/errors.ftl` | refusals: a newer meta is too new; an unsafe legacy media name is corrupt; a legacy map that does not parse is a JSON error; the kind conflict's message; ids more than a day ahead and an unnormalized media name are invalid input with the check-database and check-media messages |
| `rslib/src/import_export/package/apkg/import/notes.rs` (`update_or_merge_notetype`, `maybe_update_existing_note`, `NoteImports`) | the kind conflict when one side is cloze; which log lists hold the package note (conflicting) and which the target note |
| `rslib/src/import_export/package/apkg/export.rs`, `rslib/src/import_export/gather.rs` (`gather_data`, `check_ids`, `gather_media_names`), `rslib/src/scheduler/mod.rs` (`timing_for_timestamp`) | export writes a temporary file beside the target and renames it; older-client support off gives the latest layout; gathering computes the scheduler day, which writes `rollover` when it is missing; the future-id cutoff |
| `rslib/src/backend/import_export.rs`, `rslib/src/import_export/package/colpkg/import.rs` (`import_colpkg`, `restore_media`), `package/media.rs` | zstd decoding: a collection package import needs no open collection, decodes each numbered media entry to the media folder unchanged, and checks the map's names against the media database, not the bytes |

Probed in the oracle image (`driver/tests/test_apkg.py` and the goldens of
`conformance/goldens/package/apkg-export/`, `apkg-import/` and
`notetype-merge/`): an export leaves the rows and the undo history as they
were and has the four entries of container P11; a successful import is one
undoable "Import" step that writes the five option keys; meta 4 is too new;
a file that is not a zip, a legacy map naming `../x.jpg` and a map that is not
JSON are corrupt; a cloze merge is `notetype_kind_conflict` with the rows
unchanged. A refused import empties Anki's undo history and study queues,
except a file that is not a zip (refused before the transaction). An export
computes the scheduler day and writes `rollover` when missing, so scenarios
set it. Adding or removing a field or template empties Anki's undo history,
so scenarios undo only plain note type edits and revert others with inverse
edits. zstd entries come back through a scratch collection package import
byte for byte; the image has no zstd library.
