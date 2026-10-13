# Oracle driver: actor scenarios, sync ops and db_dump (U13)

Read at tag 26.09.3 to choose the `anki` Python APIs and the server start-up
behind `ops/sync_server.py`, `ops/sync_client.py` and `observe/db_dump.py`;
no code was copied. What the ops and observations mean is
`specs/conformance/sync-ops.md` and `observations.md` O8 (see `sync-ops.md`
here for the files read for those specs).

| Anki file | Read for |
|---|---|
| `pylib/anki/syncserver.py`, `rslib/src/sync/http_server/mod.rs` | `python -m anki.syncserver` with `SYNC_BASE`, `SYNC_HOST` (an IP address), `SYNC_PORT` and `SYNC_USER1` as the worker's child |
| `pylib/anki/collection.py` | `sync_login`, `sync_collection(auth, sync_media=False)`, `close_for_full_sync`, `full_upload_or_download(auth, server_usn=None, upload)` and `reopen(after_full_sync=True)` |
| `pylib/anki/sync.py`, `proto/anki/sync.proto` | `SyncAuth` (host key and endpoint) and the collection sync's required outcomes |
| `pylib/anki/errors.py`, `rslib/src/error/network.rs` | `NetworkError`, `SyncError` and `SyncErrorKind`; the clock error reaches Python as the "other" kind, told apart by the collection's own `tr.sync_clock_off()` |
| `rslib/src/backend/dbproxy.rs` | a statement that is not a `select` marks the collection modified, so `db_dump` reads columns through `pragma_table_info` inside a `select` |
| `rslib/src/backend/mod.rs` | the sync HTTP client honors proxy variables, so the worker removes them and sets its own only in a recording run |
