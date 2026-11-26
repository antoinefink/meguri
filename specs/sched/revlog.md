# The review log

Spec: sched/revlog
Requirements: R681, R683, R684, R1462
Anki: 26.09.3
Scenarios: conformance/scenarios/sched/revlog/, conformance/scenarios/sched/answer/

The `revlog` table holds one row per review and per manual scheduling change, kept forever and synced. Its columns and read tolerances are `model/row-records#R5`. This file says which rows Meguri writes, how their ids are chosen, and that rows are never changed afterwards.

## E1. Kinds of rows and who writes them (R683, R1462)

| `type` | Kind | Written by Meguri |
|---|---|---|
| 0 | learning | an answer of a new or learning card (`sched/answering#A5`) |
| 1 | review | an answer of a review card |
| 2 | relearning | an answer of a relearning card |
| 3 | filtered (an early review in a filtered deck) | preview answers (factor zero) and early rescheduled reviews (`decks/filtered-study#F3`, F4) |
| 4 | manual | Set Due Date and Reset (U48, R682) |
| 5 | rescheduled | never (R683) |

- R1462 lists type 5 among the kinds Meguri writes; R683 and the plan's U44 say Meguri never writes it, and nothing Meguri does reschedules cards (R697). This file follows R683; `model/row-records#R5` says the same.
- Bury, suspend, unbury, unsuspend and flag changes write no row (R683; U48).
- An answer writes exactly one row (R681): its columns are `sched/answering#A5`.

## E2. Ids

- A new row's `id` is the answer instant in epoch milliseconds (`now_ms`). When a row with that id exists, it is the largest id in the table plus one (`storage/ids-usn-mtime#I2`), not the next free millisecond: with rows `T` and `T + 5000` and an answer at `T`, the new row is `T + 5001`.
- Meguri never allocates an id more than one day after `now_ms` (`storage/ids-usn-mtime#I3`); a table holding an id further ahead makes Meguri's id differ from Anki's (deviation `ids-future-cap-revlog`).
- Several answers in one millisecond therefore get `T`, `T + 1`, `T + 2`, … (golden `sched/answer/new-card-ratings`).

## E3. Rows are kept (R683, R1483)

- Meguri never updates a review-log row and never deletes one, with one exception: undoing the operation that inserted it (E4).
- Rows of every type stay as they are, type 3 and 5 rows and rows with values Meguri cannot read included (`model/row-records#R5` read tolerances); an answer of a card leaves the card's older rows untouched.
- Deleting a card or a note keeps its rows: they still count in statistics (R43) and still sync.

## E4. Undo and redo (R684)

- Undoing an answer (or a Set Due Date or Reset, U48) deletes the rows that operation inserted, and only those (`sched/answering#A7`).
- Redoing it inserts them again under their ids. When another writer took such an id in between, Meguri refuses the redo (`engine/undo#A8`, deviations `undo-revlog-taken-id-*`).

## E5. Scenarios

| Scenario | Covers |
|---|---|
| `sched/revlog/kept-rows` | E2's largest-plus-one rule after an existing row at the instant and one 5 s later; E3: rows of types 0 to 5 of the card and of a deleted card stay byte for byte; E4: undo deletes only the answer's row; a preset answer cap of 10 s |
| `sched/answer/new-card-ratings` | E2: ids bumped within one instant |
| `sched/answer/state-kinds`, `sched/answer/deck-counters` | the columns of A5 |
| `undo/revlog-taken-id` | E4's redo under a taken id |
