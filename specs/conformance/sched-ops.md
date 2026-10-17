# Scheduling ops on chosen cards: bury, suspend, unsuspend, unbury, set due, reset, Grade Now, Empty

Spec: conformance/sched-ops
Requirements: R1883, R634, R635, R682, R683, R706, R708, R709, R713, R715, R716, R717, R718, R719
Anki: 26.09.3
Scenarios: conformance/scenarios/sched/ops/

The scenario ops (`dsl.md` D7.2) of U48's operations. Both executors implement them with the meanings below; `conformance/schema/scenario.schema.json` and `observation.schema.json` hold their shapes. The behavior they run is `specs/sched/manual-ops.md` and `specs/sched/set-due-date.md`. Nothing here adds an observation kind: scenarios read the results with `card`, `revlog`, `undo_status` and `db_exec`.

## J1. Shared rules

- **Cards and notes.** `cards` is a list of at least one `#cN`, `notes` of at least one `#nN`, each listed at most once (`invalid_scenario` otherwise: Anki fails a list with a repeated card with a database error, Meguri refuses it, `sched/manual-ops#M9`). A reference to a card or note that was deleted is passed on, and the operation skips it (`sched/manual-ops#M1`), except in `grade_now`.
- **Decks.** A deck is named by its stored name in human form, read from the collection when the step runs, as `deck-ops.md` K1 says; so a filtered deck inserted with `db_exec` is found by its name.
- **Order.** The executor passes the cards in the listed order. `forget` and `grade_now` keep that order (positions and review-log ids depend on it); the other ops sort them by card id (`sched/manual-ops#M1`, `sched/set-due-date#S7`).
- **Output.** None (`{}`), except the rejections listed per op (`dsl.md` D7.3).
- **Each op is one operation** of its spec, with its undo step (`undo-ops.md` V4): Meguri's executor submits the engine op with no review session; the oracle calls the matching operation of Anki's scheduler. Each drops a held study queue as `answer-ops.md` H3's card-changing ops do; its undo and redo too.

## J2. `bury`

- **Argument** `{cards}` or `{notes}`: exactly one of the two.
- **Effect** `sched/manual-ops#M2`: Bury Card on the cards, or Bury Note on the notes (all their cards). Always the user's bury (queue -3).

## J3. `suspend`

- **Argument** `{cards}` or `{notes}`: exactly one of the two.
- **Effect** `sched/manual-ops#M3`: Suspend Card or Suspend Note. `model-ops.md` L3's `{cards}` form is this op.

## J4. `unsuspend`

- **Argument** `{cards}`.
- **Effect** `sched/manual-ops#M4`: every listed card with queue -1, -2 or -3 gets its queue back from its type, so this op also unburies. Step "Unbury/Unsuspend".

## J5. `unbury_deck`

- **Argument** `{deck, mode}`: `mode` is `all`, `manual` (queue -3 only) or `siblings` (queue -2 only).
- **Effect** `sched/manual-ops#M5` on the deck and its descendants. Step "Unbury/Unsuspend".

## J6. `set_due`

- **Argument** `{cards, days, context?}`: `days` the text as typed (any string, passed unchanged); `context` `reviewer` or `browser`, none when missing.
- **Effect** `sched/set-due-date`; with a context the text is remembered in `setDueReviewer` or `setDueBrowser` (S6).
- **Rejection** `invalid_days` when the text is refused (`sched/set-due-date#S2`); nothing changes. The oracle maps Anki's invalid-input refusal of the text to it; Meguri maps each parser kind (`Empty`, `Malformed`, `TooLarge`) to it. Any other failure is a run error.

## J7. `forget`

- **Argument** `{cards, restore_position?, reset_counts?, context?}`: two booleans, `false` when missing (the operation's own defaults; the dialog's remembered defaults are M6.4's and are not part of the op); `context` `reviewer` or `browser`, none when missing.
- **Effect** `sched/manual-ops#M6`, Reset with the two choices, the cards in the listed order, a marker row per card, and the choice keys of the context.

## J8. `grade_now`

- **Argument** `{cards, rating}`: `rating` `again`, `hard`, `good` or `easy`.
- **Effect** `sched/manual-ops#M7` in the listed order, with time 0 and the answer instant `now_ms`. Scenarios list cards of normal decks only; a card that does not exist is a run error (both sides fail the operation).

## J9. `empty_filtered`

- **Argument** `{deck}`.
- **Effect** `sched/manual-ops#M8`.
- **Rejection** `not_filtered` when the deck is a normal deck; nothing changes.

## J10. Writing scheduling-op scenarios

- **FSRS and the load balancer.** Set `fsrs: true` (Meguri always schedules with FSRS, `sched/set-due-date` S4.3). Scenarios with `grade_now` also set `loadBalancerEnabled: false` and read no `queue`: Anki uses a load balancer for Grade Now only while it holds a study queue, Meguri builds its own (`sched/load-balancer` W10).
- **The day.** Start with `observe: [timing]`, observe `timing` again after any `reopen`, and keep every op and its undo in one scheduler day (`sched/day-clock` D14.2; `undo-ops.md` V5): buried cards come back at the first day query of a later day.
- **Stamps.** `advance` the clock before the first op whose stamps a scenario reads, so they differ from the creation instant, and between ops whose stamps must differ.
- **Filtered decks.** Use the fixture insertion contract from `deck-ops.md` K13. Prepare card membership with raw updates: retain its source deck and due value in the original-deck columns, set the filtered deck id, and assign the filtered position. A new card retains its source position; a preview-repeat card uses queue 4 with its timestamp retained in the original-due column.
- **Filtered decks.** Insert the deck row with `db_exec` as `deck-ops.md` K13 says (a small fixed id, `mtime_secs` and `usn` 0, its common and kind bytes) and move cards in with `db_exec` (`did`, `odid`, `odue`, `due`, `queue`), as a build leaves them: a new card's position goes to `odue` and its `due` becomes a large negative filtered position; a preview repeat card has queue 4 and a timestamp `odue`.
- **Ranges.** A set due with an `N-M` text draws a day per card that the oracle cannot reproduce. Never observe the drawn values (`card`, `revlog`, `db_dump` of those cards); read them with `db_exec` expressions that hold for any draw: `due` − today between N and M, `ivl` − (`due` − today) for a card with `lrt`, the review-log `ivl` equal to the card's `ivl`.
- **Undo.** Run a `db_exec` that is not a `select` before the first `undo` or `undo_status` (`undo-ops.md` V5); the oracle's setup leaves "Add Note" steps.

## J11. Driver and executor status

- The oracle driver implements every op, form and rejection of this file. It refuses a repeated reference before calling Anki, records `invalid_days` only when Anki refuses the text itself (Anki refusing a card, such as one whose home deck is filtered, is a run error) and decides `not_filtered` from the deck's row before Anki runs. Every scenario under `conformance/scenarios/sched/ops/` has its golden, recorded with `tools/mg oracle record sched/ops/`; they compare equal on the Linux and Apple oracles, with no overlay. The goldens hold the values of the scenarios' comments, and the examples of `sched/manual-ops` (M5, M6.2, M7, M8) and `sched/set-due-date` (S8) were checked on the oracle through the driver.
- The testkit model (`core/crates/meguri-testkit/src/scenario/`) reads these ops with the shapes of `scenario.schema.json` and J1's load-time rules: at least one card or note, each listed once, and non-empty deck names.
- Meguri's executor implements them in its scheduling adapter (`core/crates/meguri-testkit/src/exec/sched.rs`) with U48's engine ops; until then the scenarios are `pending` (`dsl.md` D10). They also need the new collection (U28), `setup.decks` and deck names (`deck-ops.md` K1, K15), `db_exec`, `undo` and `undo_status` (U25), and for `grade_now` the answer path (U44).
