# Manual scheduling operations

Spec: sched/manual-ops
Requirements: R634, R635, R682, R683, R704, R706, R707, R713, R714, R715, R716, R717, R718, R719, R729, R768, R1812, AE45
Anki: 26.09.3
Scenarios: conformance/scenarios/sched/ops/

The scheduling actions a user applies to chosen cards, other than answering (`sched/answering`) and Set Due Date (`sched/set-due-date`): bury, suspend, unsuspend and unbury, unbury a deck, Reset, Grade Now and Empty (of a filtered deck). Each is one operation with one undo step. The scenario ops are `specs/conformance/sched-ops.md`.

Terms as in `sched/answering`: `now_ms`, `now_s`, `today`, `next_day_at`. Card types, queues, `due`, the filtered-deck columns, the home deck and the queue-from-type rule are `sched/card-data` K1 to K3; card data keys are `sched/card-data` K5 and `model/card-data-json`.

## M1. Shared rules

- **Targets.** Each operation takes a list of distinct card ids. Bury and suspend also take a list of note ids instead (Bury Note, Suspend Note): the targets are then all cards of those notes. Ids that name no card, and notes without cards, are skipped without error, except in Grade Now (M7). An operation whose targets change nothing writes nothing and records no step (`engine/undo` A2).
- **Order.** Reset and Grade Now handle the cards in the order the caller lists them, which decides positions (M6) and review-log ids (M7). The other operations handle them in ascending card id order; only review-log ids could show the order, and they write none.
- **A card written** gets `mod` = `now_s` and `usn` -1, and its `data` text is encoded again (`model/card-data-json` C4), so a card whose data held unknown or misplaced keys is normalized by any of these operations. A card an operation leaves alone keeps its row byte for byte, `mod` and `usn` included.
- **`col.mod`** becomes `now_ms` when the operation wrote anything (`storage/ids-usn-mtime` S5).
- **Review log.** Bury, suspend, unsuspend, unbury and Empty write no review-log row (R683). Reset writes a marker row per card (M6.3), Set Due Date a manual row per card (`sched/set-due-date` S5), Grade Now an answer row per card (M7).
- **Filtered decks.** Meguri does not study filtered decks (R706). Bury, suspend and unsuspend change only the queue of a card that sits in one, which stays there (`did`, `odid`, `odue` unchanged). Reset and Set Due Date take it out. Grade Now refuses it (M7). Empty returns every card a filtered deck holds (M8). Nothing Meguri does on its own changes a preview card (queue 4) or an interday learning card in a filtered deck (R707): only the user's actions of this file and of `sched/set-due-date` do.
- **No other rows.** Apart from what each section lists, no note, tag, deck, preset or config row is written.

## M2. Bury (R634)

Bury Card and Bury Note bury the targets by hand.

| Card's `queue` before | After |
|---|---|
| -1 (suspended) | unchanged, not written: burying never touches a suspended card |
| -3 (buried by the user) | unchanged, not written |
| -2 (buried by the scheduler) | -3, written |
| 0, 1, 2, 3, 4 | -3, written |

- Only `queue` changes; `due`, `type` and the rest stay, so M4 or the next day's unbury (`sched/day-clock` D7) can restore the queue from the type.
- Step name "Bury" (`specs/conformance/undo-ops.md` V4 `bury`). One undo restores every card the operation buried, all cards of a note included.
- Burying by the scheduler (sibling burying, queue -2) is the answer's (`sched/sibling-burying`), not this operation.

## M3. Suspend (R717)

Suspend Card and Suspend Note.

| Card's `queue` before | After |
|---|---|
| -1 | unchanged, not written |
| -3, -2 (buried) | -1, written: a buried card can be suspended |
| 0, 1, 2, 3, 4 | -1, written |

- Only `queue` changes. Step name "Suspend" (V4 `suspend`). Suspending several cards is one step, and one undo restores all of them (AE45).

## M4. Unsuspend and unbury of chosen cards (R717)

One operation serves the browser's Toggle Suspend and Toggle Bury when they restore the selected cards, and any other unsuspend or unbury of chosen cards:

| Card's `queue` before | After |
|---|---|
| -1, -2, -3 | the queue `sched/card-data` K3 gives for its `type` and its current `due`, written |
| 0, 1, 2, 3, 4 | unchanged, not written |

- So unsuspending a card that is buried also unburies it, and unburying a suspended card unsuspends it.
- K3 reads the card's current `due`. A learning card buried or suspended inside a filtered deck has its filtered-deck `due` there (a negative position), which K3 reads as a day number: it comes back to queue 3 in the filtered deck; Empty then restores its home `due` and queue (M8).
- Step name "Unbury/Unsuspend" (V4 `unbury_unsuspend`; R719's "Unbury" is Anki's "Unbury/Unsuspend").

## M5. Unbury a deck (R635)

Unbury on the deck page returns the deck's buried cards to their queues.

- **Inputs**: a deck and a mode:

  | Mode | Cards it restores | Deck page label |
  |---|---|---|
  | all | `queue` -2 and -3 | All buried cards |
  | manual | `queue` -3 | Manually buried cards |
  | siblings | `queue` -2 | Buried siblings |

- **Cards**: those whose `did` is the deck or one of its descendants (every deck whose name starts with the deck's name followed by `::`), filtered decks among them included. A card whose home deck is in that tree but which sits in a filtered deck outside it is not restored. A deck id that names no deck restores nothing and is not an error.
- Each card of the mode is restored as M4 says (K3) and written; no other card is touched (suspended cards stay suspended).
- Step name "Unbury/Unsuspend" (V4 `unbury_unsuspend`).
- **The choice (R635).** The deck page offers the three modes, in the order manual, siblings, all, with manual preselected, only when the deck's tree (the same cards as above) holds at least one card with `queue` -2 and at least one with `queue` -3; otherwise Unbury runs mode all at once. Meguri's API reports the two facts (M9).

Example (oracle library): decks `Home`, `Home::Sub` and `Other`. A card of `Home::Sub` with queue -2 and one with queue -3; two learning cards of `Other` with queue -2, one with `due` an epoch second and one with `due` day 3. Siblings mode on `Home` restores only the -2 card of `Home::Sub` (queue 0); all on `Other` gives the learning cards queue 1 and queue 3; manual on `Home` then restores the -3 card.

## M6. Reset (R713, R714, R715, R716)

Reset (Anki's "Forget") makes the targets new cards again.

### M6.1 Inputs

The cards in the caller's order; two choices, "Restore original position where possible" and "Reset repetition and lapse counts"; and the context (none, the review screen or the browser, M6.4).

### M6.2 The card row (R713, R714)

| Column | Value |
|---|---|
| `type`, `queue` | 0, 0 |
| `due` | the card's position (below), stored as a signed 32-bit value (`sched/card-data` K7: positions from 2,147,483,648 up are stored negative) |
| `ivl`, `factor` | 0, 0 |
| `reps`, `lapses` | 0 and 0 with "Reset repetition and lapse counts"; unchanged without |
| `did`, `odid`, `odue` | when `odid` is not 0: `did` = `odid`, then `odid` and `odue` 0 (the card leaves its filtered deck); otherwise all three unchanged |
| `left`, `flags`, `nid`, `ord` | unchanged (a learning card keeps its `left`) |
| `data` | `s`, `d` and `pos` removed; `dr`, `decay`, `lrt`, `cd` and every other key kept (`sched/card-data` K5, K6 rule 3) |
| `mod`, `usn` | `now_s`, -1 |

**Positions.** The operation reads `nextPos` once (`sched/card-data` K7: an unsigned 32-bit number, 0 when missing or unreadable) as the next free position `P`. Then for each card, in the caller's order:

1. With "Restore original position" on, a card that has a last position goes back to it: a card of type 0 has its current position (`odue` when `odid` is not 0, else `due`, read as unsigned 32-bit), any other card its `pos` key when present. `P` does not move.
2. Otherwise the card gets `P`, and `P` moves on by one (from 4,294,967,295 to 0).

After the cards, `nextPos` is written with `P` (as `storage/config-table` C4 writes: only when the value differs, so an operation in which every card restored its position leaves the key alone unless it was missing or unreadable).

| Card (in the order listed; `nextPos` 7) | Restore on | Restore off |
|---|---|---|
| review, no `pos` | 7 | 7 |
| new, position 3 | 3 | 8 |
| suspended review, no `pos` | 8 | 9 |
| review, `pos` 2 | 2 | 10 |
| `nextPos` after | 9 | 11 |

(The "restore on" column is the oracle library's result.)

### M6.3 The Reset marker (R682, R716)

Each card gets one review-log row, inserted right after its row is computed:

| Column | Value |
|---|---|
| `id` | `now_ms`, bumped as `sched/revlog` E2 says, in the caller's order |
| `cid`, `usn` | the card, -1 |
| `ease` | 0 |
| `ivl` | 0 |
| `lastIvl` | the `ivl` before, capped at 2,147,483,647 |
| `factor` | 0 |
| `time` | 0 |
| `type` | 4 |

A new card that is reset gets a marker too. The memory-state history drops everything before a card's last marker (`sched/fsrs-items`, U46).

### M6.4 Remembered choices (R715)

- From the review screen the operation stores the two choices in `restorePositionReviewer` and `resetCountsReviewer`; from the browser in `restorePositionBrowser` and `resetCountsBrowser`; without a context, nowhere. The values are JSON `true` or `false`, written after `nextPos` as config writes are (C4: only when different).
- The dialog prefills the choices from the context's keys, with the defaults `true` for restore position and `false` for reset counts when a key is missing (`sched/config-keys` G2).

### M6.5 The operation

- One undoable operation named "Reset Card" (V4 `reset_card`; R716's "Reset" is Anki's "Reset Card"). `col.mod` = `now_ms`.
- The note is not touched: the `leech` tag stays (R716).
- Undo restores the cards, `nextPos` and the choice keys exactly, deletes the marker rows and restores `col.mod`.

## M7. Grade Now (R718)

The browser's Grade Now answers the chosen cards with one rating, outside any study queue.

- **Inputs**: the cards in the caller's order and a rating (Again, Hard, Good, Easy).
- **Refusals, before anything is written** (Meguri):
  - an id that names no card: `NotFound` (Anki fails the same way);
  - a card in a filtered deck (`odid` not 0): `InFilteredDeck` (`sched/answering` A12; Anki answers it with its filtered-deck states, which Meguri does not have);
  - a card whose home preset U45 refuses (`sched/fsrs-transitions` T13, for example 34 parameters): `PresetUnsupported{presetIds}`, the refused presets' ids in ascending order (R704; `sched/fsrs-policy`, U50).
- **Each card, in the caller's order**: its next states are computed at the operation's instant from its row as the operation has left it so far, with the load balancer as `sched/load-balancer` W10 says for Grade Now; the state for the rating is applied as one answer (`sched/answering` A2 to A6 with A13's application, the siblings it buries and a leech), with time taken 0 and answer instant `now_ms`.
  - No head-of-queue check (R729's exception) and no queue: the card's `queue` before does not matter, so a suspended or buried card becomes active with the queue its new state gives. The deck counters (`sched/answering` A6) go by that `queue` before: a suspended or buried card counts in neither.
  - Each answer writes its own review-log row: `id` `now_ms`, `now_ms` + 1, … in the caller's order, `time` 0, the other columns as `sched/answering` A5.
  - A card listed after its sibling sees the sibling's answer (a sibling buried by it is then answered and leaves the buried queue).
- **The operation.** One undoable operation named "Grade Now" (V4 `grade_now`), not "Answer Card": one undo reverts every card, review-log row, deck counter, buried sibling and leech tag of the operation, and touches no load-balancer cache (`sched/load-balancer` W9). It does not go through a review session (`sched/review-session` C9); an open session sees the change set (C13).

Example (oracle library, FSRS on, load balancer off): Grade Now Good on a buried new card, a suspended new card and a review card (`ivl` 3, `due` 5, `factor` 2500), in that order: the two new cards get learning states (queue 1, `left` 1) and rows `ease` 3, `ivl` -600, `lastIvl` 0, `factor` 224, `time` 0, `type` 0; the review card gets a review state and a row `type` 1 with `lastIvl` 3. The three rows' ids are `now_ms`, `now_ms` + 1 and `now_ms` + 2 in that order.

## M8. Empty a filtered deck (R706, R707)

- **Input**: a deck. The operation fails and writes nothing when no deck has that id (`NotFound`) or when it is a normal deck (`NotFiltered`).
- **Cards**: every card whose `did` is the filtered deck.
- Each card leaves the deck as `sched/card-data` K3 says for leaving a filtered deck, and is written:

  | Column | Value |
  |---|---|
  | `did` | `odid` when `odid` is not 0; unchanged otherwise |
  | `due` | `odue` when `odid` and `odue` are both not 0; unchanged otherwise |
  | `queue` | when the card had `odid` not 0 and `queue` 0 or above (0 to 4, preview repeat 4 included): K3's queue for its `type` and the restored `due`; a negative queue (suspended or buried in the filtered deck) stays |
  | `odid`, `odue` | 0 |
  | everything else | unchanged; `mod` = `now_s`, `usn` -1 |

- A card the filtered deck holds with `odid` 0 (a damaged row) keeps all its values but is written (stamped).
- The filtered deck's own row is not changed: it stays, empty.
- No review-log row. Step name "Empty" (V4 `empty`); one undo puts every card back.

| Card in the filtered deck (home deck `Home`) | After Empty |
|---|---|
| new, `odue` 1 (its position), `due` -100,000 | `Home`, queue 0, `due` 1 |
| review, `odue` 5 | `Home`, queue 2, `due` 5 |
| learning, preview repeat (queue 4), `odue` an epoch second | `Home`, queue 1, `due` that second |
| relearning, preview repeat (queue 4), `odue` day 2 | `Home`, queue 3, `due` 2 |
| relearning, user-buried (queue -3), `odue` 1 | `Home`, queue -3, `due` 1 |
| suspended review, `odue` 0, `due` 2 | `Home`, queue -1, `due` 2 |
| `odid` 0 | unchanged, stamped |

(Oracle library results, from cards built by Anki's filtered-deck build and changed with SQL.)

## M9. Meguri's operations (engine, R768, R1812)

Every operation of this file is one engine op of source `UserUndoable` (`engine/op-model` O1), kind `Other`, named with its step name:

| Op | Inputs | Step name | Errors |
|---|---|---|---|
| bury | card ids, or note ids | Bury | |
| suspend | card ids, or note ids | Suspend | |
| unsuspend (also unbury of chosen cards) | card ids | Unbury/Unsuspend | |
| unbury deck | deck id, mode (all, manual, siblings) | Unbury/Unsuspend | |
| reset | card ids in order, restore position, reset counts, context (none, reviewer, browser) | Reset Card | |
| grade now | card ids in order, rating | Grade Now | `NotFound`, `InFilteredDeck`, `PresetUnsupported{presetIds}` |
| empty filtered deck | deck id | Empty | `NotFound`, `NotFiltered` |
| set due date | `sched/set-due-date` S9 | Set Due Date | S9 |

- Each op also takes an optional review-session id. With one, the step records the session (`engine/undo` M1) and the result carries the session's head diff (`sched/review-session` C13), so the review screen shows the next card without a rebuild (R1812). Without one, an open session reacts to the change set like to any other change.
- A list holding the same id twice is refused with `InvalidArgument` before anything is written. (Anki fails such a list with a database error for bury, suspend, unsuspend and Set Due Date, and handles the card twice for Reset and Grade Now, writing two review-log rows; no Meguri caller sends one.)
- Results: the change set (`engine/change-stream`); bury and suspend also the number of cards they wrote; the head diff with a session id.
- **Buried kinds.** For a deck, Meguri reports whether the cards of M5 include a card with `queue` -2 and whether they include one with `queue` -3 (a read, not an op), for the deck page's choice (R635).
- The FFI and Swift bridge expose the same ops (U37's error mapping); the keyboard shortcuts and menus of R768 are the app's.

## M10. Undo summary

| Operation | Rows its undo restores | Step |
|---|---|---|
| bury, suspend, unsuspend, unbury deck, Empty | the cards it wrote | Bury, Suspend, Unbury/Unsuspend, Empty |
| Reset | the cards, `nextPos` and the choice keys; its marker rows deleted | Reset Card |
| Grade Now | the cards, siblings, decks, note tags and tag rows its answers wrote; its review-log rows deleted | Grade Now |
| Set Due Date | the cards and the remembered-text key; its rows deleted (`sched/set-due-date` S7) | Set Due Date |

Every undo also restores `col.mod` (`engine/undo` A7); redo applies the step again, inserting review-log rows under their ids (`sched/revlog` E4).

## M11. Scenarios

| Scenario | Covers |
|---|---|
| `sched/ops/bury/cards-and-notes` | M2: queues before and after, the suspended card skipped (also in Bury Note), stamps only on cards written, no review-log row, one undo for a note's cards |
| `sched/ops/bury/unbury-deck` | M5: the three modes, subdecks, a filtered subdeck, learning cards to queues 1 and 3, suspended cards left, another deck's cards left; M4 through `unsuspend` |
| `sched/ops/suspend/cards-and-notes` | M3, M4, AE45: three cards suspended in one step and restored by one undo, a buried card suspended, Suspend Note, unsuspend by type, unsuspend of a buried card, an active card left unstamped, a card in a filtered deck staying there, no review-log row |
| `sched/ops/reset/restore-position` | M6: restore on, positions in the caller's order, `nextPos`, counts kept, `left` kept, data keys, the leech tag, the markers, a card in a filtered deck, the browser's choice keys, undo |
| `sched/ops/reset/end-of-queue` | M6: restore off with reset counts, the reviewer's choice keys, a new card moved to the end |
| `sched/ops/grade-now/four-cards` | M7: four cards of four kinds, one step, review-log ids in the caller's order, `time` 0, counters, a suspended card made active, one undo |
| `sched/ops/empty-filtered/restore` | M8: every row of the table, a card buried in the filtered deck by `bury`, the deck row unchanged, the refusal on a normal deck, undo |

Grade Now's refusals are Meguri's own: the oracle at the pin cannot hold a preset U45 refuses (`sched/fsrs-transitions` T13), and Anki answers a card in a filtered deck. Meguri's engine tests assert `PresetUnsupported` (a 34-value preset in the selection) and `InFilteredDeck`, each with nothing written (`core/crates/meguri-engine/tests/sched_ops.rs`).
