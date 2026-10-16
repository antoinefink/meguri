# Undo: observable results, undo ops and the undo status (U25)

Spec-writer notes for `specs/engine/undo.md` A1-A11, `specs/conformance/undo-ops.md`,
and the undo parts of `specs/conformance/record-ops.md` W1-W4,
`specs/storage/ids-usn-mtime.md` I4, S3, S5 and `specs/storage/graves.md` G6.
Read at tag 26.09.3 (`refs/anki-26.09.3`); no code was copied. Two black-box
probes ran in the oracle image (`meguri-oracle:26.9.3-0189c1d0ce915343`) through
the `anki` Python API under libfaketime, with no network; they confirmed every
rule below (merge window and its start, redo emptied by no-change ops and by
config writes without an undo entry, G6 removal and the pending graves after
redo, `col.mod` after redo following a config write, config rows restored with
their stamps, the review-log restore under a taken id and the later undo that
deletes the occupying row, the 30-step limit, setup's "Add Note" steps).

| Spec section | Anki files read | Read for |
|---|---|---|
| engine/undo A1, undo-ops V4 | `rslib/src/ops.rs`, `ftl/core/actions.ftl`, `ftl/core/studying.ftl`, `ftl/core/decks.ftl`, `ftl/core/browsing.ftl`, `ftl/core/deck-config.ftl`, `ftl/core/preferences.ftl`, `ftl/core/notetypes.ftl`, `ftl/core/undo.ftl`, `rslib/src/scheduler/bury_and_suspend.rs` | the name each operation gives its step, shared names |
| engine/undo A2 | `rslib/src/undo/mod.rs`, `rslib/src/collection/transact.rs`, `rslib/src/backend/dbproxy.rs`, `rslib/src/config/mod.rs`, `rslib/src/backend/config.rs`, `pylib/anki/collection.py`, `pylib/anki/config.py`, `rslib/src/scheduler/queue/mod.rs` | 30-step limit, redo emptied when any op begins, skip-undo ops keep undo steps, ops without undo record and raw non-select statements empty both, rollover clears only with stale queues |
| engine/undo A3 | `rslib/src/notes/undo.rs` (merge of note updates), `rslib/src/undo/mod.rs` (step time), `rslib/src/timestamp.rs` | the merge condition, the 60 s window measured from the step's first op in whole seconds, clock moved back |
| engine/undo A4 | `rslib/src/notes/mod.rs` (update path, tag canonicalization, card generation), `rslib/src/notes/undo.rs`, `rslib/src/tags/undo.rs`, `rslib/src/card/undo.rs`, `rslib/src/notetype/cardgen.rs`, `rslib/src/storage/note/*.sql` | rows a note update changes and what undo and redo write back |
| engine/undo A5, graves G6 | `rslib/src/notes/mod.rs` (removal), `rslib/src/notes/undo.rs`, `rslib/src/card/undo.rs`, `rslib/src/decks/undo.rs`, `rslib/src/storage/graves/mod.rs`, `add.sql`, `remove.sql`, `rslib/src/storage/note/add_or_update.sql`, `rslib/src/storage/card/add_or_update.sql`, `rslib/src/collection/mod.rs` (`usn()`) | grave removal on undo whatever its origin, redo's insert-or-ignore with the deletion's usn (-1), restored note `flags` and `data` |
| engine/undo A6 | `rslib/src/config/undo.rs`, `rslib/src/config/mod.rs`, `rslib/src/storage/config/mod.rs`, `rslib/src/storage/config/add.sql` | config rows restored with their stamps, value comparison on undo, removal is always undoable |
| engine/undo A7, ids-usn-mtime S5 | `rslib/src/collection/transact.rs`, `rslib/src/collection/timestamps.rs`, `rslib/src/collection/undo.rs` | no `col.mod` bump while undoing; the restored value and the value saved for the inverse step |
| engine/undo A8, ids-usn-mtime I4 | `rslib/src/revlog/undo.rs`, `rslib/src/storage/revlog/mod.rs`, `rslib/src/storage/revlog/add.sql` | restore without uniquify, the change recorded even when ignored, removal by id |
| undo-ops V1, V2 | `rslib/src/collection/service.rs`, `rslib/src/backend/error.rs`, `pylib/anki/collection.py`, `pylib/anki/_backend.py`, `pylib/anki/errors.py` | the undo, redo and undo-status calls, the "undo empty" error for both |

Harvest classification (this pass): `rslib/src/undo/mod.rs` tests `undo`
(`h876dbf5f6be7`), `coalesce_note_undo_entries` (`h8c7ca9736257`) and `custom`
(`hc80955057f3f`, not applicable: add-on interface), `rslib/src/config/undo.rs`
`undo` (`hde5c73b93e34`) and the supplementary
`add_revlog_entry_returns_none_on_collision_without_uniquify`
(`hf7664c5a108b`, deferred for its sync and import parts). Left for their
areas: `rslib/src/notes/mod.rs` `undo` (`h4907663a10e2`, U27), the queue undo
tests in `rslib/src/scheduler/queue/undo.rs` (U43, U44) and the deck preset
undo tests in `rslib/src/deckconfig/undo.rs` (supplementary, U42).

Driver hints for the oracle maintainer (U25 role work):

- `undo` and `redo`: the collection's undo and redo; Anki's "undo empty" error
  (`UndoEmpty` in pylib) is the rejection `nothing_to_undo` or
  `nothing_to_redo`. Any other error stays a run error. pylib's undo and redo
  already drop its note type cache when note types changed.
- `undo_status`: the collection's undo status; map each name back to its V4
  id by comparing with the backend's own translation of each name (the
  `tr` methods for the operations listed in `rslib/src/ops.rs`), never with
  English text in the driver. An empty name is `null`; an unknown name is a
  run error.
- `set_config` with `undoable: true`: the config setter with `undoable=True`.
  `remove_config`: the collection's config removal (always undoable).
- `delete_notes` already calls nothing when no listed note exists; keep it so
  (`record-ops.md` W3: the histories stay).
