# Queue building, orders, limits and counts (U41 spec writer)

Spec-writer notes for `specs/sched/limits.md`, `queue-building.md`,
`orders.md`, `counts.md` and `specs/conformance/queue-ops.md`. Read at tag
26.09.3 (`refs/anki-26.09.3`); no code was copied. Every scenario under
`conformance/scenarios/sched/queue/` and `sched/counts/` was run on the oracle
image (`meguri-oracle:26.9.3-83345ca61b6ced12`, this worktree's driver) with a
scratch add-on module implementing `set_current_deck`, `queue`, `deck_counts`
and `congrats` exactly as queue-ops Q1-Q4 describe (backend calls only: select
the deck, the queued cards with a fetch limit of the card count, the deck tree
with counts below the root, the congratulations info). That module stays out of
the repository: the oracle maintainer writes the real driver module. The
outputs agreed with the rules as specified, including the tables of limits L2,
L4, orders O1, O5, counts C3 and the deviation value of
`queue-nested-filtered-deck`.

| Spec section | Anki files read | Read for |
|---|---|---|
| limits L2, L4, L6 | `rslib/src/decks/limits.rs`, `rslib/src/decks/counts.rs` | remaining limits (32-bit arithmetic, the review cap on new cards, 9999 for filtered decks and missing presets), the limits tree, decrementing and capping descendants |
| limits L3 | `rslib/src/scheduler/queue/builder/mod.rs`, `rslib/src/storage/deck/mod.rs` | which decks the tree holds, `applyAllParentLimits`, the error for a deck missing from the tree |
| queue-building B1, B2 | `rslib/src/scheduler/queue/builder/mod.rs`, `rslib/src/decks/current.rs`, `rslib/src/storage/deck/update_active.sql`, `active_deck_ids_sorted.sql`, `rslib/src/config/mod.rs` | inputs, the current deck fallback, the studied decks by name range under `unicase`, `collapseTime` |
| queue-building B3 | `rslib/src/scheduler/queue/builder/mod.rs`, `burying.rs` | display options from the studied deck's preset, the defaults when it is missing, bury modes from the home preset |
| queue-building B4, B5 | `rslib/src/scheduler/queue/builder/gathering.rs`, `burying.rs`, `rslib/src/storage/card/intraday_due.sql`, `due_cards.sql`, `new_cards.sql`, `active_new_cards.sql` | the four phases, root-limit stops, bury registration only for cards no limit refused |
| queue-building B6, B8, B9 | `rslib/src/scheduler/queue/mod.rs`, `learning.rs`, `main.rs`, `entry.rs`, `builder/mod.rs` | learning order, the three learning parts, presentation order, counts |
| queue-building B10 | `rslib/src/ops.rs`, `rslib/src/collection/transact.rs`, `rslib/src/backend/dbproxy.rs`, `rslib/src/undo/mod.rs` | which operations clear the study queues |
| orders O1-O4, O6 | `rslib/src/scheduler/queue/builder/sorting.rs`, `gathering.rs`, `rslib/src/storage/card/mod.rs` (review order clauses, new-card sorting), `rslib/src/storage/sqlite.rs` (the hash and the FSRS SQL functions), `proto/anki/deck_config.proto` | keys, salts, signed versus unsigned comparison, NULL placement, FSRS-off variants |
| orders O5 | `rslib/src/scheduler/queue/builder/intersperser.rs`, `sized_chain.rs`, `builder/mod.rs` | the spreading rule in 32-bit floats, the two mixes |
| counts C1-C4 | `rslib/src/decks/tree.rs`, `rslib/src/storage/deck/due_counts.sql`, `rslib/src/decks/mod.rs` | the tree, Default hiding, own counts (strict test for queue 1, not for queue 4), the bottom-up capping, `applyAllParentLimits` top-down |
| counts C5 | `qt/aqt/overview.py` | the deck page's signed differences |
| counts C7 | `rslib/src/scheduler/congrats.rs`, `rslib/src/storage/card/congrats.sql`, `proto/anki/scheduler.proto` | the congratulations fields and `secs_until_next_learn` |

Checked on the oracle beyond the rules' tests:

- Ties left by every key come in ascending deck id, then card id, for
  ascending and descending position and for learning cards (the SQLite plan
  walks the scheduling index per deck id); scenario `sched/queue/order/ties`.
- The studied decks' insertion order (studied deck, then the name range in
  index order) equals the `unicase` name order, so "deck" keys sort by name.
- A nested filtered deck's cards (review, new, interday learning, preview
  repeat) are gathered by Anki; the deck list counts them under the filtered
  child.
- `fnvhash` refuses reals (even 2.0), text, blobs and NULL.
- Retrievability orders put cards without a memory state first (ascending) or
  last (descending), not through the SM-2 fallback, which only relative
  overdueness uses. The plan's U41 test that expects the fallback for
  retrievability ascending does not hold at 26.09.3.
- Review order 8 (random) hashes card id and mtime, not the day.

Second spec-writer pass (session s-20261017T192017Z-9fd7375a), after the
oracle maintainer's goldens: `tools/mg oracle compare sched/queue/
sched/counts/` passed 33/33. Re-read at the tag: `intraday_due.sql` gathers
queues 1 and 4 for the active decks with no deck-type test, so a queue 4 card
left in a normal deck is an intraday learning card (queue-building B4 phase 1);
`congrats.sql` counts queue 1 only; `due_counts.sql` tests queue 4 with `<=`.
New scenario `sched/queue/gather/preview-in-normal-deck` recorded (Linux and
Apple lanes agree). `limits.rs` computes the remaining limits in `i32` and the
release build does not check overflow; a scratch scenario (not committed) gave
0 reviews for a This-deck review limit of 2,147,483,647 with `revToday` -1 and
2 (all due) for 2,147,483,646, so limits L2 states the wrap.

Harvest links: the catalog rows classified here (congrats, interspersing,
sized chain, the queue builder's unit tests, and `test_newLimits`,
`test_deckDue`, `test_deckTree`, `test_deckFlow`) point at the scenarios that
cover them; `oracle/harvest/links.csv` is unchanged.
