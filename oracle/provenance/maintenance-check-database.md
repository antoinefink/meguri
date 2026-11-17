# Check Database: cards, history, notes and note types (U128, U129)

Spec-writer notes for `specs/maintenance/check-database.md` and
`specs/maintenance/check-database-notes.md`. Read at tag 26.09.3
(`refs/anki-26.09.3`); no code was copied. Written 2026-10-27 by mapping each
section of the committed text to the files that back it. The problem kinds and
the `check_db` op are `specs/conformance/collection-check.md` K5 and K10, whose
sources are in `fixtures-synthetic.md` and `driver-check.md`; the integrity
verdict and the optimize statements are in `maintenance-optimize.md`.

Meguri's own design, with no Anki source (the specs' `Anki:` lines say so):
check-database C2, C3's order of check and backup, C5, C7's op shape, C12's
report members, C13's error set, C14's records, change set and derived store,
C15, C16; check-database-notes N2's write rule and N6.6's write rule.

| Spec section | Anki files read | Read for |
|---|---|---|
| check-database C1 | `qt/aqt/dbcheck.py`, `qt/aqt/main.py` (forced backup), `pylib/anki/collection.py`, `rslib/src/collection/service.rs`, `rslib/src/dbcheck.rs` | what Anki's command runs: a forced backup (made in `qt/aqt/main.py`, requested by `qt/aqt/dbcheck.py`), the quick check and the optimize, then the repairs in one transaction, then the list of fixes |
| check-database C3 | `rslib/src/dbcheck.rs` | the order of Anki's stages (quick check, optimize, cards, notes, history, next position, future ids) |
| check-database C4 | `rslib/src/dbcheck.rs`, `rslib/src/storage/sqlite.rs` (quick check) | the quick check at the start and its corrupt result returned before any repair; no index is created |
| check-database C6 | `rslib/src/storage/sqlite.rs` (optimize) | the statements of the optimize stage, run outside the repair transaction |
| check-database C7 | `rslib/src/dbcheck.rs`, `rslib/src/collection/transact.rs` | one transaction without an undo step; the repair instant taken once |
| check-database C8, C8.1 to C8.6 | `rslib/src/dbcheck.rs` (card properties), `rslib/src/storage/card/mod.rs`, `rslib/src/storage/card/fix_due_new.sql`, `fix_due_other.sql`, `fix_odue.sql`, `fix_ivl.sql`, `fix_mod.sql`, `fix_ordinal.sql` | each card repair's rule, bounds and stamps; the v1 scheduler branch; the counts that make up the card-properties kind |
| check-database C8.7 | `rslib/src/storage/card/mod.rs` (last review time), `rslib/src/storage/card/data.rs`, `rslib/src/scheduler/fsrs/memory_state.rs` (last review info from the review log) | the last review time from the latest graded row, new cards skipped, the count and the schema mark |
| check-database C9.1 | `rslib/src/dbcheck.rs`, `rslib/src/storage/card/mod.rs` (orphaned cards) | cards whose note is missing are deleted, with the schema mark |
| check-database C9.2 | `rslib/src/dbcheck.rs`, `rslib/src/storage/deck/mod.rs` (missing decks), `rslib/src/decks/addupdate.rs` (recovered deck) | a deck created for each missing `did` with that id, its name and stamps; `storage/deck/missing-decks.sql` reads `did` alone, so an `odid` naming no deck creates none (C9.3's last sentence, `filtered-deck-kept`) |
| check-database C9.3 | `rslib/src/dbcheck.rs` (filtered cards), `rslib/src/scheduler/filtered/card.rs`, `rslib/src/storage/card/filtered.rs` | cards with filtered fields outside a filtered deck restored to their home values, with the schema mark |
| check-database C10.1 | `rslib/src/storage/revlog/mod.rs`, `rslib/src/storage/revlog/fix_props.sql` | the review-log columns repaired and their bounds |
| check-database C10.2 | `rslib/src/dbcheck.rs` (missing deck names), `rslib/src/decks/addupdate.rs`, `rslib/src/decks/name.rs` | parent decks created for names whose parents are missing |
| check-database C10.3 | `rslib/src/dbcheck.rs` (next new position), `rslib/src/config/mod.rs`, `rslib/src/storage/card/mod.rs` | the next new-card position set from the highest stored one |
| check-database C10.4 | `rslib/src/dbcheck.rs` (invalid ids), `rslib/src/storage/dbcheck/mod.rs`, `rslib/src/storage/dbcheck/invalid_ids_count.sql`, `invalid_ids_create.sql`, `invalid_ids_update.sql`, `invalid_ids_drop.sql` | the one-day cutoff, the tables covered, the new ids' start value, the constraint error on a taken id (deviation `check-db-future-ids-skip-used`) |
| check-database C11 | `rslib/src/dbcheck.rs`, `rslib/src/collection/timestamps.rs` | which repairs mark the schema modified; `col.mod` set on every run (deviation `check-db-no-change-keeps-mod`) |
| check-database C12 | `rslib/src/dbcheck.rs` (output counters and their messages), `ftl/core/database-check.ftl`, `qt/aqt/dbcheck.py` | the twelve counters and which carry a number; Anki's messages, which Meguri never shows |
| check-database C13 | `rslib/src/dbcheck.rs`, `rslib/src/error/mod.rs`, `rslib/src/error/db.rs` | the corrupt result and the error a failed repair gives; rollback of the transaction |
| check-database C14 (undo) | `rslib/src/collection/transact.rs`, `rslib/src/undo/mod.rs`, `rslib/src/notetype/mod.rs` (undoable add of a recovered note type) | both histories cleared by the run; the one "Add Note Type" step Anki keeps after a recovery |
| check-database C17 | the files of C8.7, C9.3, C10.1, C10.4 and C11 | the Anki column of each deviation: an out-of-range column stopping the run, the taken id, `col.mod` |
| check-database-notes N1, N3 | `rslib/src/dbcheck.rs` (note types and notes), `rslib/src/storage/notetype/mod.rs` (note ids by note type) | the stage's place between cards and history; only note types with notes visited |
| check-database-notes N2 | `rslib/src/dbcheck.rs`, `rslib/src/storage/tag/mod.rs` (expanded tags, clearing, restoring), `rslib/src/tags/register.rs` | Anki emptying the registry and re-registering each note's tags, then restoring expanded rows still used (deviation `check-db-tags-keep-rows`) |
| check-database-notes N4 | `rslib/src/dbcheck.rs` (recover note type), `rslib/src/notetype/stock.rs` | the recovered note type's kind, field count from the first note, name and id; invalid UTF-8 in that note stopping Anki (deviation `check-db-recover-invalid-utf8`) |
| check-database-notes N5 | `rslib/src/dbcheck.rs` (missing field tags), `rslib/src/image_occlusion/notetype.rs` | Image Occlusion field tags restored on every visit (deviation `check-db-io-unchanged-skip`) |
| check-database-notes N6.1 | `rslib/src/dbcheck.rs` (note fixing invalid UTF-8), `rslib/src/storage/note/mod.rs` | the lossy fix of `flds`, `tags` and `sfld` and its count |
| check-database-notes N6.2, N6.3 | `rslib/src/dbcheck.rs` (duplicate ordinals, cards without template), `rslib/src/storage/notetype/mod.rs` (existing cards of a note), `rslib/src/storage/notetype/existing_cards.sql` | which card of a duplicated ordinal stays, cards past the templates deleted |
| check-database-notes N6.4 | `rslib/src/dbcheck.rs`, `rslib/src/notes/mod.rs` (field count fix) | pad and merge, the `db-check` tag |
| check-database-notes N6.5 | `rslib/src/dbcheck.rs` | a future `mod` lowered to the repair instant, `usn` kept |
| check-database-notes N6.6 | `rslib/src/notes/mod.rs` (note update generating cards, no stamping), `rslib/src/notetype/cardgen.rs`, `rslib/src/dbcheck.rs` | every note written without stamps, missing cards generated with the note type's last deck; when no candidate deck is usable the generation falls back on deck 1 and stops with an error when deck 1 is missing or filtered (N6.6, N9's `check-db-default-deck-restored` and `check-db-default-deck-filtered`) |
| check-database-notes N7 | `rslib/src/dbcheck.rs`, `rslib/src/notetype/mod.rs` (add), `rslib/src/notetype/stock.rs` | the stock Basic note type added when none is left, its stamps (deviation `check-db-basic-stamped`) |
| check-database-notes N8 | `rslib/src/dbcheck.rs` | which notes-stage counts mark the schema modified |
| check-database-notes N9 | the files of N2, N4, N5 and N7 | the Anki column of each deviation |

## Session s-20261117T152613Z-96466c16 (spec writer): deferred harvest rows, batch 2

Read at tag 26.09.3 (`refs/anki-26.09.3`) to classify deferred harvest rows, write their scenarios and draft spec text; no code, test text or fixture value was copied unless a row says otherwise, and every expected value comes from the recorded oracle goldens.

| Spec section and scenarios | Anki file | Read for |
|---|---|---|
| maintenance/check-db/card-fixes-counted, maintenance/check-db/revlog-row-counted (check-database C8, C10.1 counting per fix and per row); harvest links h8bba9c90dc5d, h5b5bc3629af2, hec5bf5d4f824 | `rslib/src/dbcheck.rs` | Which behaviors the card, revlog and deck-name tests require. Expectations come from the oracle goldens. |
| maintenance/check-db-notes/tag-states-kept (check-database-notes N2); harvest links h984772533897, h45394f1a8123, h19a3e91fc826 | `rslib/src/dbcheck.rs` | Which behaviors the tag, note-card link and field-count tests require (tag states kept for used tags, duplicate and template-less cards, field joining and padding). Expectations come from the oracle goldens. |
| maintenance/check-database C10.4 (ids in the future) and scenarios maintenance/check-db/future-ids-each-table and future-ids-card-order | `rslib/src/storage/dbcheck/mod.rs`, `rslib/src/storage/dbcheck/invalid_ids_count.sql`, `rslib/src/storage/dbcheck/invalid_ids_create.sql`, `rslib/src/storage/dbcheck/invalid_ids_update.sql`, `rslib/src/storage/dbcheck/invalid_ids_drop.sql`, `rslib/src/dbcheck.rs` (the caller's cutoff and first new id only) | Which behavior the four harvested tests require: the count across the three tables, re-keying in ascending order of old id from the given first id, and card nid and review-log cid following the new ids. I confirmed that C10.4 already states each rule. The goldens come from the oracle. |

## Audit fixes (2026-11-17, session s-20261117T172637Z-685ca4ee)

Read at tag 26.09.3 (`refs/anki-26.09.3`) to fix spec readiness audit findings; no code was copied, and every rule was restated in the spec writer's words.

| Subject | Anki file | Read for |
|---|---|---|
| check-database C9.3 (every deck record decoded before the filtered-card check), C10.3 (nextPos write through the config setter), C18.2 undecodable-deck and repaired-dump (config table left out because the card-properties check computes the day and writes a missing rollover key); collection-check K5 new_positions_too_high predicate | `rslib/src/dbcheck.rs`, `rslib/src/storage/deck/mod.rs`, `rslib/src/storage/card/mod.rs`, `rslib/src/storage/card/fix_due_new.sql`, `rslib/src/config/mod.rs`, `rslib/src/config/undo.rs` | Stage order and the get_decks_map decode before any card check; the new-position fold predicate; nextPos as max(due)+1 and the byte comparison in the config update; the timing_today call in the card-properties check that writes rollover when missing |
| check-database-notes N2 (no ancestor rows, respelling to the first note's spelling, unstamped note write), N5 (unchecked save recomputes names, sort index and reqs and renumbers ordinals), N6.4/N8 field-count schema stamp | `rslib/src/dbcheck.rs`, `rslib/src/tags/register.rs`, `rslib/src/storage/notetype/mod.rs`, `rslib/src/notetype/mod.rs` | clear_all_tags and the restore of expanded tags; tag registration spelling; the note write without mark-modified; the Image Occlusion save path (prepare_for_update, add_or_update_notetype_with_existing_id_inner, field and template row rewrite with enumerate ordinals); set_schema_modified on field_count_mismatch |
