# Queue ops: the studied deck, its queue, the deck list's counts and a finished deck

Spec: conformance/queue-ops
Requirements: R1883, R1894, R1895, R1896, R1897
Anki: 26.09.3
Scenarios: conformance/scenarios/sched/queue/, conformance/scenarios/sched/counts/

The op and the three observation kinds that the queue scenarios use. Both executors implement them with the meanings below; `conformance/schema/scenario.schema.json` and `observation.schema.json` hold their shapes. The behavior they observe is `specs/sched/queue-building.md`, `limits.md`, `orders.md` and `counts.md`.

## Q1. `set_current_deck` (op)

The decks area's op (`deck-ops.md` K7, U28): `{deck}`, a deck by its stored human name; it writes `curDeck` as selecting the deck does (`decks/decks`, the current deck). Output none. Queue scenarios use it to choose the studied deck, and never observe the undo status after it (Anki records a "Select Deck" step, Meguri none). This file reuses its current stable definition; it adds no second operation.

## Q2. `queue` (observation kind)

- **Request** `queue`, no arguments.
- **Value** `{cards, kinds, counts}` for the current deck (`curDeck`; the Default deck when it names none) at the clock's instant:
  - `cards`: the cards the reviewer would present, in order, if none were answered (`sched/queue-building#B8`): the learning cards due now, the main list, then the learning cards within the learn-ahead limit; each as `#cN`. Cards the queue holds for later (due beyond the learn-ahead limit) are not listed;
  - `kinds`: one per card, in the same order: `new`, `learning` (intraday and interday learning) or `review`;
  - `counts`: `{new, learning, review}`, the queue's counts (`sched/queue-building#B9`).
- **Errors.** A card the scenario did not create is listed by its real id as a string (`observations.md` O1); scenarios create every card they study.

### Q2.1 Executors

- **Oracle.** It asks Anki's backend for the queued cards of the current deck with a fetch limit at least the number of cards in the collection, and reports each card, its queue kind and the three counts the backend returns with them.
- **Meguri.** It builds the queue of the current deck at the scenario's instant (`sched/queue-building#B1`) and reports B8's presentation order and B9's counts.

### Q2.2 Side effects

Reading the queue writes nothing to the collection. Anki keeps the queue it built until something requires a rebuild; Meguri's executor keeps none (a session is U43's).

### Q2.3 Reading a fresh build

A queue read shows a fresh build only when Anki has none held, so queue scenarios read the queue only:

- as the first read of the queue after setup;
- right after `set_current_deck` names another deck than the current one;
- right after an op that changes cards (other than flags), decks or presets: `add_note`, `legacy_update`, `db_exec` with a statement other than `select` (which empties every cached view on both executors, `record-ops.md` W1), and the like;
- after the clock moved to another day.

After `advance` within a day, or after `set_config`, a scenario first selects another deck and then the studied deck again (`set_current_deck` twice), or runs a `db_exec` that changes nothing (`UPDATE cards SET usn = usn WHERE 0`). Reads after `answer` show the queue the session keeps (U43's scenarios, not this file's).

## Q3. `deck_counts` (observation kind)

- **Request** `deck_counts`, no arguments.
- **Value** a list of the deck list's top-level nodes in order, each `{name, new, learning, review, children}`: `name` the last component of the deck's name, the three numbers the deck list shows for it (`sched/counts#C3`; `learning` is Learn, `review` is Due), and `children` a list of nodes. The nodes and their order are those of the deck tree (`sched/counts#C1`, `#C2`; `deck_tree`, `deck-ops.md` K12).
- **Compatibility prelude.** This conformance observation explicitly requests D5/D7 compatibility maintenance before its read-only count calculation, on both executors (`sched/day-clock#D14.2`). Like `timing`, it can repair `rollover`, update `localOffset`, restore buried cards and write an empty `lastUnburied` marker. The calculation itself writes nothing. Production count reads instead require D8 automatic maintenance before their snapshot (`sched/counts#C6`); they must not reuse this prelude implicitly.
- **Oracle.** It asks Anki's backend for the deck tree with counts at the current instant (the deck list's call) and reports its nodes below the root.
- **Meguri.** It reports the deck list record of `sched/counts` for the current instant.
- The `deck_tree` kind (U28) keeps reporting the structure without counts.

## Q4. `congrats` (observation kind)

- **Request** `congrats`, no arguments.
- **Value** `{learn_remaining, secs_until_next_learn, review_remaining, new_remaining, have_sched_buried, have_user_buried}` for the current deck: an integer, an integer, and four booleans, as `sched/counts#C7` defines `learning_remaining`, `secs_until_next_learn`, `review_remaining`, `new_remaining` and whether `sibling_buried` and `user_buried` are above 0.
- **Oracle.** Anki's congratulations information for the current deck, those six fields.
- **Meguri.** The six compatibility facts from `sched/counts#C7` at the current instant. Native today-only counts, ready instants and limit flags are distinct members and do not replace these values.
- Reading it writes nothing and runs no unbury check.

## Q5. Writing queue scenarios

- **States.** Setup adds notes; one `db_exec` per scenario then sets card states, matching rows by logical card number: `UPDATE cards SET ... FROM (SELECT id AS cid, row_number() OVER (ORDER BY id) AS n FROM cards) AS c JOIN (VALUES (<n>, ...), ...) AS v ON v.column1 = c.n WHERE cards.id = c.cid`. Card ids follow creation order, so row number `n` is `#cN`. Positions are set note by note the same way, joining `notes` by row number. Card data goes in as JSON text (`sched/card-data#K5`); `mod` stays the creation second unless the statement sets it.
- **Options.** Display options (`newGatherPriority`, `newSortOrder`, `reviewOrder`, `newMix`, `interdayLearningMix`), and limits and studied counters of a deck (`newLimit`, `reviewLimit`, `newLimitToday`, `reviewLimitToday`, `newToday`, `revToday`), are written with `legacy_update` (`model-ops.md` L2), which saves the preset or deck as a change and so also forces a rebuild (Q2.3). Daily limits and bury options can also come from `setup.presets`. The global keys come from `setup.config` or `set_config`.
- **Filtered decks.** As `deck-ops.md` K13: a `decks` row inserted with `db_exec` (a small fixed id), cards moved into it with `did`, `odid`, `odue` and `due` as building it would leave them.
- **Days.** Scenarios whose orders depend on elapsed time set the clock forward (for example to day 200) after setting states, so that review days are after day 0.
- **Rollover.** After moving the clock across a day boundary, a scenario observes `timing` before reading counts or queues (`sched/day-clock#D14.2`).
- **Ties.** A scenario that tests an order gives its cards distinct keys, except where it tests the ties of `sched/orders#O6`.

## Q6. Current evidence

The saved packet provides 34 scenario/golden pairs. Copying their exact bytes does not prove a working current adapter or a successful replay. The stable deck-selection operation is available. Queue and count providers still require completion and validation before their cases become active.

The new `sched/queue/order/mixed-sixty` case adds 60 original cards and every required order. Its output must come from the reference runner. Linux recording and comparison, then native comparison, remain required. The earlier 25-card and 34-card cases remain required too.

Q3 keeps its explicit compatibility check. If the source owner finds a write mismatch, add a focused count observation with a raw dump. Existing explicit timing cases keep their outputs.

The filtered-child exclusion in B2.2 follows existing product policy. Its deviation needs an actual passing implementation test before activation. This packet does not add a waiver or assert test completion.
