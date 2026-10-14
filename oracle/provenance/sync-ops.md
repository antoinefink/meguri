# specs/conformance/sync-ops.md and observations.md O8 (U13)

Read at tag 26.09.3 (`refs/anki-26.09.3`) to state the sync harness's
behavior in Meguri's own words; no code was copied.

| Anki file | Read for | Spec sections |
|---|---|---|
| `pylib/anki/syncserver.py`, `rslib/src/sync/http_server/mod.rs` | how the server starts; `SYNC_USER<n>`, `SYNC_BASE`, `SYNC_HOST`, `SYNC_PORT` | S3.1 |
| `rslib/src/sync/http_server/user.rs` | the account's collection is opened (created) at the first request that reads it | S3.1, S4.3 |
| `rslib/src/storage/sqlite.rs`, `rslib/src/storage/schema11.sql` | a new collection: `mod` 0, `scm` now, default deck, default preset and stock note types | S2.2, S4.3 |
| `pylib/anki/collection.py` | sync entry points: login, collection sync with media off, full upload or download with no server usn (no media sync after it) | S4.1 |
| `proto/anki/sync.proto`, `rslib/src/backend/sync.rs` | the outcomes a collection sync returns, and that a full sync closes and reopens the collection whatever the result | S4.1 |
| `rslib/src/sync/collection/normal.rs` | a completed incremental sync reports no changes | S4.1 |
| `rslib/src/sync/collection/meta.rs`, `rslib/src/sync/collection/status.rs` | outcome rules (equal modification times, different schema times, empty sides) and the 300 s clock check, made only by the status check | S4.1, S4.3, S4.4 |
| `rslib/src/sync/collection/upload.rs`, `rslib/src/sync/collection/download.rs` | full upload and download make no status check; the server raises its usn when it serves a download; a downloader sets `ls` to `mod` | S4.1, S7.3 |
| `rslib/src/collection/mod.rs` (preparing an upload), `rslib/src/collection/transact.rs` | what an uploader changes locally before it uploads; transactions without undo set `mod` | S7.3 |
| `rslib/src/sync/collection/finish.rs`, `rslib/src/sync/collection/graves.rs` | `mod`, `usn` and `ls` at the end of an incremental sync on both sides; received deletions are stored with the server's usn | S7.3 |
| `rslib/src/sync/http_server/mod.rs` (server config) | `SYNC_HOST` is parsed as an IP address, default all addresses; one bind address only | S3.1 |
| `rslib/src/sync/collection/changes.rs`, `rslib/src/storage/config/mod.rs`, `rslib/src/sync/collection/start.rs` | the whole config travels from the side with the later collection `mod` (the server on a tie); the receiver replaces all entries with the round's server usn and its own current time; the sender zeroes every entry's usn; the round's usn is the server collection's usn at start | S7.3 |
| `rslib/src/collection/mod.rs` (preparing an upload) | the uploader leaves config entries unchanged | S7.3 |
| `rslib/src/error/network.rs`, `rslib/src/backend/error.rs`, `pylib/anki/errors.py`, `pylib/anki/_backend.py` | error classes seen from Python: network errors, auth errors, other sync errors; a reset after the response headers surfaces as an other sync error | S4.4 |
| `rslib/src/sync/http_client/io_monitor.rs`, `rslib/src/sync/http_client/full_sync.rs` | the 30 s stall timeout; how body read failures are reported | S4.4 |
| `rslib/src/backend/mod.rs` | the sync HTTP client is built with the library's defaults, so it honors proxy environment variables | S8.1, S9 |
| `rslib/src/storage/upgrades/schema14_upgrade.sql`, `schema15_upgrade.sql`, `schema17_upgrade.sql`, `schema18_upgrade.sql` | the schema-18 tables, columns and primary keys | O8 |
| `rslib/src/notes/mod.rs`, `rslib/src/notetype/fields.rs`, `rslib/src/notetype/templates.rs`, `proto/anki/notetypes.proto` | values drawn at random: note guids, field and template ids (field config field 9, template config field 8) | O8 |
| `rslib/src/notetype/stock.rs` | stock note types written into every new collection | O8, S12 |

## Notes for the oracle maintainer

- From Python, a clock error is a sync error of the "other" kind; only its
  message tells it apart. Classify it by comparing the message with the same
  collection's own translation of the clock-error string (the collection's
  translation helper has one per sync error), never by a substring, and never
  put the text in an output (R1887). Network errors have their own class.
- Pass no server usn to the full sync call, so no media sync starts after it.
  Keep using the same collection object after a full sync; opening the path
  again fails while it is open.
- A reset on the first response bytes fails before the headers arrive, so the
  client reports a network error. A reset later in the body would not
  (S4.4). `reset_peer` with timeout 0 should reset at the first chunk in its
  direction; the golden of `smoke/sync/reset-during-download` confirms it (its
  first full download must record `network`). If it does not, report it to
  the spec writer rather than changing the scenario.
- S7.3 also excludes `/config/<key>/mtime_secs` and `/config/<key>/usn`
  for every config row, from the actors' raw dumps and from the converge
  output's `dump`. The three-party golden recorded before this change showed
  exactly the pattern S7.3's example predicts (client_a and client_b differ
  in both columns of all 19 entries, client_c in `mtime_secs` only), and no
  other difference. After the change every `differences` list of the smoke
  scenarios must be `[]`; if one is not, report it to the spec writer.
- S3.1: the server listens on `0.0.0.0` whenever a fault proxy or recorder
  must reach it, because the converge client still uses loopback.
