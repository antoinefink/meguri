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

## Session s-20261117T152613Z-96466c16 (spec writer): deferred harvest rows, batch 2

Read at tag 26.09.3 (`refs/anki-26.09.3`) to classify deferred harvest rows, write their scenarios and draft spec text; no code, test text or fixture value was copied unless a row says otherwise, and every expected value comes from the recorded oracle goldens.

| Spec section and scenarios | Anki file | Read for |
|---|---|---|
| sched/counts/new-studied-today and sched/counts/nested-new-limits (deck tree counts with studied counters, nested per-deck new limits, applyAllParentLimits capping a child's limit); harvest rows h25a2a4a1f100, h8673919e1db4, hdddcc971bba4 | `rslib/src/decks/tree.rs` (tests counts and nested_counts_v3; the v3 sum-and-limit function and the deck-tree-with-counts read) and `rslib/src/decks/service.rs` (test deck_tree_with_now_set_includes_counts) | Which behavior each harvested test requires (counts in the deck tree at the current instant, studied-today counters against the new limit on a parent and its child, per-level new limits applied to the children's limited totals), and how applyAllParentLimits caps a child's limits, so that a scenario step could tell the two settings apart. The expected values come from the oracle goldens. |
| Parent and child new limits: sched/queue/limits/new-parent-child (specs sched/limits L2-L4; sched/queue-building B4) | `pylib/tests/test_schedv3.py` (test behind h56c0cea0f0c6) | Which behavior to prove: a parent deck's limit and a child preset's limit both cap the gathered new total, and the parent's cards come first. Deck sizes and limits are Meguri's own; expected values come from the oracle. |
| sched/queue/empty-collection; sched/counts/missing-deck-card; sched/session/head/sibling-order-unburied (queue-building B5/B8/B9, counts C1/C3/C7, orders O2/O3) | `pylib/tests/test_schedv3.py` | Which behaviors to prove: an empty collection yields no study card; deck-tree counts sum children while keeping child-local counts and tolerate a card in a missing deck; with sibling burying off, one note's card ordinals come up in order across answers. Expected values come from the oracle goldens only. |
| sched/counts/shared-ancestors; proposed sched/counts C1 sentence | `pylib/tests/test_schedv3.py` (test_deckTree) | Which behavior row head34b2562fb obliges: an ancestor shared by nested decks appears once in the count tree beside a prefix-sharing top-level deck. |
| sched/counts C7 empty-deck facts and sched/orders O5/O8 interspersing (coverage links only, no new scenario) | `rslib/src/scheduler/congrats.rs` (empty-collection test); `rslib/src/scheduler/queue/builder/intersperser.rs` (interspersing test) | Confirming that the existing goldens sched/counts/finished and sched/queue/mix/spread-sizes show the whole tested behavior. |
| limits L2-L4 harvest coverage: new-limit-not-spent-by-reviews, new-ignore-zero-review-limit, ancestor-new-limit; existing intraday-uncapped | `rslib/src/scheduler/queue/builder/mod.rs`, `rslib/src/scheduler/queue/builder/gathering.rs` | which limit behaviors the builder tests require (zero review cap, new cards ignoring the review limit, reviews not using up the new limit, applyAllParentLimits with an ancestor new limit of 0), and confirming root-limit stops during gathering |
| orders O4 order 12 harvest coverage: relative-overdueness; O2/O3 existing new-gather-* and O5 existing mix scenarios | `rslib/src/scheduler/queue/builder/mod.rs`, `rslib/src/scheduler/queue/builder/sized_chain.rs`, `proto/anki/deck_config.proto` | the behaviors the tests require (relative overdueness over due day and interval with ties, nested-limit gather orders with sibling order, concatenation of two queue lists) and the enum values of the review order, gather priority and sort order |
| queue-building B5 / sibling-burying B2 at queue level: new-first-review-sibling | `rslib/src/scheduler/queue/builder/mod.rs`, `rslib/src/scheduler/queue/builder/gathering.rs`, `rslib/src/scheduler/answering/mod.rs`, `rslib/src/scheduler/bury_and_suspend.rs` | the required behavior (new-before-review mix with review-sibling burial: the queue must shrink by exactly one after the new card's answer) and confirming that an answer from the new queue excludes kinds gathered earlier from burial |
| Scenarios sched/counts/congrats-work-kinds and sched/counts/congrats-user-buried (finished-deck facts by work kind; a user-buried card excluded from new work; a single-new-card queue batch) | `rslib/src/scheduler/service/mod.rs` (test module) | Which behaviors the harvested congrats, queued-cards and answer-again tests check. Expectations come from the oracle goldens, not the test assertions. |
