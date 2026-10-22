# Oracle driver and fixtures: collection packages (U124a)

Read at tag 26.09.3 to choose the `anki` calls behind the collection package
form of `export` (`driver/ops/package.py`, `driver/ops/colpkg.py`), the
collection package `import` and its refusals (`driver/ops/colpkg.py`), and the
backup the package fixtures start from (`fixtures/package/make.py`). Meanings
are `specs/conformance/package-ops.md` E1 to E4; `package_listing`
(`driver/observe/package_listing.py`) reads archives with Python's `zipfile`
and `sqlite3` and the `meta` entry with the driver's own reader of
`specs/package/container.md` P3, and calls Anki only to open a copy. No code
was copied.

| Anki file | Read for |
|---|---|
| `pylib/anki/collection.py` (`export_collection_package`, `close_for_full_sync`, `reopen`, `create_backup`, `__init__`) | the export leaves the Python collection without a DB and the backend closes the collection itself, so the driver reopens it; `Collection(path, backend=...)` reopens on the same backend; a forced backup with `wait_for_completion` |
| `qt/aqt/import_export/exporting.py` (`ColpkgExporter`), `qt/aqt/main.py` (restore), `qt/aqt/import_export/importing.py` (`import_collection_package_op`) | the desktop reopens after an export, also after a failure; it closes the collection, imports with the profile's collection path, media folder and media database, and loads the profile again |
| `rslib/src/backend/import_export.rs` | export takes the open collection; import needs the collection closed |
| `rslib/src/import_export/package/colpkg/import.rs`, `package/meta.rs`, `package/media.rs` | the errors the import raises before its rename: import errors (too new, corrupt), protobuf decode errors (`meta`, the media map), zip errors, I/O errors (a collection entry that is not zstd) |
| `rslib/src/import_export/mod.rs` (`ImportError::message`), `rslib/src/error/mod.rs`, `rslib/src/error/network.rs`, `rslib/src/backend/error.rs`, `pylib/anki/_backend.py` (`backend_exception_to_pylib`) | an import error's message is the translation of its kind (`errors_collection_too_new`, `importing_the_provided_file_is_not_a`); zip errors become sync errors of kind other; pylib raises import and protobuf errors as plain `BackendError`, so the driver reads the kind from the backend error |
| `rslib/src/collection/backup.rs` | backups: the in-memory "changed since the last backup" stamp lets the first forced backup after an open through; the file name comes from the local time (`backup-%Y-%m-%d-%H.%M.%S.colpkg`) |
| `pylib/anki/media.py` (`media_paths_from_col_path`) | the session's media folder and media database next to `collection.anki2` |

Probed in the oracle image (`driver/tests/test_package.py`, the fixture maker
and the goldens of `conformance/scenarios/package/collection-package/`): the
export has the four stored entries of container P11 and leaves every row as it
was; an import of it changes only `col.scm` and `col.mod`, both to the import
instant in milliseconds (a `timing` observed later may write
`lastUnburied`, `specs/sched/day-clock.md` D14.2); meta 4 and an empty meta are
too new; `ff`, a missing collection entry, a raw collection entry, a non-zip
file, schema 12, the damaged index page, the three media-map faults and a
missing media map are corrupt; each refusal leaves the rows and the session
directory's files as they were and the collection open. Anki's placeholder
reads version 11, rollback journal, one note, one card, no review, and Anki
opens it. The backup at 2030-01-14T12:00:00Z has E4's layout (meta `08 03`,
four stored entries, a 9-byte empty media frame); the maker's packages are
byte-identical across runs.
