# Oracle driver: scheduling ops on chosen cards (U48)

Read at tag 26.09.3 (`refs/anki-26.09.3`) to choose the calls behind the ops
of `specs/conformance/sched-ops.md` J1-J9 (`driver/ops/sched_manual.py`; the
`{cards}` form of `suspend` moved there from `driver/ops/sched.py`). No code
was copied. The behavior the ops run is the spec writer's reading in
`specs/sched/manual-ops.md` and `specs/sched/set-due-date.md`
(`provenance/sched-manual-ops.md`).

| Anki file | Read for |
|---|---|
| `pylib/anki/scheduler/base.py` | the scheduler calls: `bury_cards` (manual, user bury) and `bury_notes`, `suspend_cards` and `suspend_notes`, `unsuspend_cards` (the restore of buried and suspended cards), `unbury_deck` with its mode, `set_due_date` with an optional string config key, `schedule_cards_as_new` (always with the log row) with its context, `empty_filtered_deck` |
| `qt/aqt/operations/scheduling.py` | Grade Now is the backend's `grade_now` with the card ids and a rating, outside any scheduler wrapper; Reset passes the dialog's two booleans and the context |
| `proto/anki/scheduler.proto`, `proto/anki/config.proto`, `proto/anki/decks.proto` | the unbury modes (all, sched-only, user-only), the reset contexts, the set-due string keys, the deck kind container stored in `decks.kind` |
| `rslib/src/scheduler/service/mod.rs` | note ids are expanded to their cards only when no card id is given |
| `rslib/src/scheduler/reviews.rs` | Set Due Date parses the text before its transaction and refuses it with invalid input; inside the transaction a card whose home deck is filtered is refused with invalid input too |
| `rslib/src/collection/transact.rs`, `rslib/src/undo/mod.rs` | a transaction without changes records no step and leaves `col.mod`; a failed one empties the undo history |
| `pylib/anki/_backend.py` | `InvalidInput` is the pylib class of exactly the invalid-input kind |

Choices:

- `set_due` tells a refused text from a refused card by asking again with
  the same text, no card and no key after an invalid-input failure: only a
  refused text fails again (before any transaction), so only then is the
  step `invalid_days`. The extra call runs only on that failure path.
- `empty_filtered` reads the deck's `kind` blob with SQL and decodes it as
  Anki's deck kind container, so a deck row a `db_exec` wrote or changed is
  judged as stored; a normal deck is `not_filtered` before any call.
- Deck names are matched against the stored names in human form, as
  deck-ops.md K1 says; U28's resolver was not on the integration branch yet,
  so `sched_manual.py` has its own lookup with the same rules.

Checked through real workers (`driver/tests/test_sched_manual.py`): the
examples of manual-ops M5, M6.2 (both columns), M7 and M8 and set-due-date S8
hold on the oracle; a damaged card in a filtered deck makes `set_due` a run
error (`InvalidInput`), not `invalid_days`. The twelve `sched/ops` goldens
compared equal three times on Linux and once on the Apple lane.
