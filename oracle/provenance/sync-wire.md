# specs/sync/wire-format.md, specs/sync/meta-status.md, specs/sync/errors.md (U106)

Read at tag 26.09.3 (`refs/anki-26.09.3`) to state the sync message shapes,
the status check and the error classification in Meguri's own words; no code
was copied. Wire facts marked "oracle" were checked against the recordings
`conformance/recordings/scenarios/sync/wire/` (oracle clients and the official
server of 26.9.3), recorded on 2026-10-15.

| Anki file | Read for | Spec sections |
|---|---|---|
| `rslib/src/sync/collection/meta.rs` | the `meta` record members, `empty` default, `hostNum`; the server's 501 for unsupported versions and its forced schema change for oversized collections; the comparison order (equal `mod`, then `scm`, then normal) and the upload/download availability formula; the redirect handling on `meta` | wire-format W3; meta-status M3 |
| `rslib/src/sync/collection/status.rs`, `rslib/src/collection/timestamps.rs`, `rslib/src/storage/collection_timestamps.rs` | offline status from `mod`, `scm`, `ls` (never synced first); the online check order: `cont`, then the 300 s clock rule, then the comparison | meta-status M2, M5 |
| `rslib/src/backend/sync.rs` | the remembered status answer: 300 s on the collection clock, stored with the check's start time, replaced by each completed collection sync's outcome and by a successful one-way sync (no changes), untouched by failures; offline answers skip it | meta-status M6, M6.1 |
| `rslib/src/sync/collection/normal.rs`, `rslib/src/sync/collection/protocol.rs` | method order, empty inputs, the sanity-failure rollback; `lnewer`; status mapping of one-way outcomes | wire-format W4, W7; meta-status M3, M4 |
| `rslib/src/sync/collection/start.rs`, `rslib/src/sync/collection/graves.rs` | `start` members (`minUsn`, `lnewer`, `graves` null), graves object, 250-id graves chunks filled cards, notes, decks | wire-format W4, W5 |
| `rslib/src/sync/collection/changes.rs` | the changes object (`models`, the two-array `decks`, `tags`, optional `conf` and `crt`), usn substitution on send, who sends config | wire-format W6 |
| `rslib/src/sync/collection/chunks.rs`, `rslib/src/serde.rs`, `rslib/src/card/mod.rs`, `rslib/src/revlog/mod.rs`, `rslib/src/types.rs`, `rslib/src/tags/mod.rs` | chunk object defaults and omission of empty arrays; 250-object chunks with the revlog/note/card rotation and the `done` rule; card, note and review tuples and their element types; the integer-or-float reader and its casts; review `type` default on invalid or missing; tag split and join | wire-format W7 to W11 |
| `rslib/src/sync/collection/sanity.rs`, `rslib/src/storage/sync_check.rs` | counts tuple, zero due counts, server zeroing graves, `c`/`s` lenient | wire-format W12 |
| `rslib/src/sync/collection/finish.rs`, `rslib/src/sync/collection/upload.rs`, `rslib/src/sync/collection/download.rs`, `rslib/src/sync/response.rs` | `finish` timestamp; `upload` text answer; downloaded `mod`/`ls` | wire-format W13, W14; meta-status M3 |
| `rslib/src/sync/media/protocol.rs`, `begin.rs`, `changes.rs`, `download.rs`, `sanity.rs`, `upload.rs`, `zip.rs`, `mod.rs`, `syncer.rs`; `rslib/src/sync/http_server/media_manager/download.rs` | media result envelope; media request and result members; change tuples; download and upload zip layouts, `_meta` forms, server limits | wire-format W15 to W17 |
| `rslib/src/storage/config/mod.rs` | config sent whole and parsed from stored JSON; sync fails on a value that is not JSON | wire-format W6.4 |
| `rslib/src/error/network.rs`, `rslib/src/sync/error.rs`, `rslib/src/sync/http_server/user.rs`, `rslib/src/sync/http_server/handlers.rs`, `rslib/src/sync/request/mod.rs` | Anki's status-to-kind mapping and network kinds; the server's 400 for failed sync operations, 409 for session-key mismatch and missing media files; JSON decode as 400 | errors E4, E9 |
| `pylib/anki/errors.py`, `proto/anki/backend.proto` | which sync error kinds reach Python as distinct classes (only authentication; the rest share one class) | sync-ops S4.4 (driver reasoning) |
| serde_json 1.0 `src/de.rs` (crate source, not Anki) | the 127-level nesting limit of Anki's JSON reader | wire-format W1.2 |
| `rslib/src/sync/collection/tests.rs` (`meta`, `new_empty_collection_should_not_require_full_sync`, `sync_errors_should_prompt_db_check`) | harvest classification | catalog |
