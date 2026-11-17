# specs/sync/normal-sync.md and the sync/normal, sync/conflicts scenarios (U107)

Read at tag 26.09.3 (`refs/anki-26.09.3`) to state the normal sync in
Meguri's own words; no code was copied. `specs/sync/hold-back.md` is Meguri's
own design (KTD6, KTD7) and has no Anki source.

| Anki file | Read for | Spec sections |
|---|---|---|
| `rslib/src/sync/collection/normal.rs` | the round's order; the unbury and undo discard before it; one transaction around the round; rollback, `abort` and `scm` on a failed sanity check; the AnkiWeb size check before the status check | N2, N3, N8, N10 |
| `rslib/src/sync/collection/start.rs`, `rslib/src/sync/collection/graves.rs`, `rslib/src/storage/graves/mod.rs`, `rslib/src/storage/graves/add.sql` | `start` arguments; local graves read and stamped before the server's are applied; the 250-id split; the server's graves applied note, card, deck, each removing only its own row and adding an insert-or-ignore grave with the server USN | N4 |
| `rslib/src/sync/collection/changes.rs`, `rslib/src/storage/sync.rs`, `rslib/src/storage/tag/mod.rs`, `rslib/src/storage/config/mod.rs` | pending objects and tags sent and stamped; config and `crt` only when local is newer, config usns set to 0; the order of applying the answer; the mtime rules for note types (with the field and template count check), decks and presets; tags registered with the server USN; config replaced whole | N5 |
| `rslib/src/notetype/mod.rs` (name uniqueness, the save used by sync), `rslib/src/decks/name.rs`, `rslib/src/storage/deck/mod.rs`, `rslib/src/storage/deckconfig/mod.rs`, `rslib/src/storage/notetype/mod.rs` | the `+` suffix loop and its stamps; rows written under their ids with no other row touched | N5.2 to N5.4, N5.9 |
| `rslib/src/tags/register.rs` | normalizing and registering an incoming tag; an existing tag left as it is | N5.5 |
| `rslib/src/sync/collection/chunks.rs`, `rslib/src/storage/revlog/add.sql`, `rslib/src/storage/card/mod.rs`, `rslib/src/storage/note/mod.rs`, `rslib/src/storage/note/add_or_update.sql` | chunk loops; the pending-and-newer rule; review rows insert-or-ignore; incoming card data parsed and rebuilt; incoming notes with control characters removed and no NFC, sort field and checksum recomputed, flags and data reset; missing note type; pending ids collected after the server's chunks | N6, N7 |
| `rslib/src/notes/mod.rs` | the field count check and the field normalization of an incoming note | N6.3 |
| `rslib/src/sync/collection/sanity.rs`, `rslib/src/storage/sync_check.rs` | the pending check over eight tables; the counts tuple; the server zeroing due and graves counts | N8 |
| `rslib/src/sync/collection/finish.rs` | `ls`, `mod` and `usn` after `finish`; the server committing before it answers | N9 |
| `rslib/src/collection/timestamps.rs`, `rslib/src/collection/undo.rs`, `rslib/src/storage/collection_timestamps.rs` | setting `scm` touches no other `col` column | N8 |
| `rslib/src/sync/http_server/user.rs` | the server dropping its round on a failed request (400), on `abort`, and on a new `start` | N10 |
| `rslib/src/sync/collection/progress.rs`, `rslib/src/sync/collection/normal.rs` (progress) | the three stages and the four counters behind sent and received | N11 |
| `rslib/src/error/network.rs`, `rslib/src/backend/error.rs` | HTTP 400 as the database-check kind; the sanity failure as its own kind; both reaching Python as the same "other" sync error with the same message | N13; sync-ops S4.4 |
| `rslib/src/decks/stats.rs`, `rslib/src/scheduler/answering/mod.rs` | an answer updates the deck's day counts and stamps the deck | scenarios `sync/holdback/run-with-flag`, `deck-change-not-applied` |
| `rslib/src/scheduler/timing.rs` | moving `crt` by 60 s within its day changes no day count | scenario `sync/normal/config-whole` |
| `rslib/src/sync/collection/tests.rs` | which tests exist (harvest classification only); nothing of them is reproduced | harvest rows |

## Notes for the oracle maintainer

- `sanity_check_failed` and `db_check_required` (sync-ops.md S4.4) reach
  Python as the same sync error of the "other" kind, and their messages are
  the same translated string, so the message cannot tell them apart. What
  differs is what the failure leaves: a failed sanity check sets the
  collection's schema time to now after the rollback; an HTTP 400 leaves it.
  Compare the actor's `scm` before and after the failed `normal`. Every
  other failure keeps its current mapping (`clock_incorrect` by its own
  translation, the rest `anki_error`).
- `review_session` (S13) does nothing on the oracle and records `{}`.
- `sync/conflicts/tie-local-wins` records a converge difference by design:
  on a tie both the client and the server keep their own card.

## Intake on 2026-10-25

Prepared neutral contracts and scenarios were restored from `8da0e61d`.
N15 and the H15 entry correction come from the current Product Contract's
KTD26 and `sync/anki-transfers` M1, M2 and M6. The added N14 production upload
check is U29's recorded U107 obligation. The session terminal-authority link
uses `sched/review-session` C17.1. No new reference source was read for this intake.

## Session s-20261117T152613Z-96466c16 (spec writer): deferred harvest rows, batch 2

Read at tag 26.09.3 (`refs/anki-26.09.3`) to classify deferred harvest rows, write their scenarios and draft spec text; no code, test text or fixture value was copied unless a row says otherwise, and every expected value comes from the recorded oracle goldens.

| Spec section and scenarios | Anki file | Read for |
|---|---|---|
| normal-sync N5.4 incoming presets added under their ids; scenario sync/normal/preset-added | `rslib/src/deckconfig/mod.rs`, `rslib/src/deckconfig/service.rs`, `pylib/anki/decks.py`, `rslib/src/sync/collection/changes.rs` (caller located only), `rslib/src/import_export/insert.rs` | which behavior the harvested tests prove: a legacy preset save with id 0 adds the preset with a fresh id, and a non-zero id is stored as given. Also which Meguri-facing paths (sync apply, package import) save presets under their existing ids. |
