# Oracle driver: queue, deck counts and congrats (U41)

Read at tag 26.09.3 to choose the `anki` Python APIs behind the `queue`,
`deck_counts` and `congrats` observations and the existing `set_current_deck`
(`driver/observe/queue.py`; meanings in `specs/conformance/queue-ops.md`
Q1-Q4); no code was copied. The specs' own sources are in `sched-queue.md`.

| Anki file | Read for |
|---|---|
| `pylib/anki/scheduler/v3.py` | `get_queued_cards` takes a fetch limit (default 1) and returns the cards with the three counts |
| `rslib/src/scheduler/queue/mod.rs` | the fetch takes that many entries from the held queue in presentation order |
| `proto/anki/scheduler.proto` | `QueuedCards` (entry queue kinds new, learning, review; `new_count`, `learning_count`, `review_count`) and `CongratsInfoResponse` (six fields reported; the filtered flag, bridge flag and description left out) |
| `pylib/anki/scheduler/base.py` | `deck_due_tree` passes the current time to the backend's deck tree, as the deck list does; `congratulations_info` |
| `rslib/src/decks/tree.rs` | with a time, the tree runs the day query and its unbury check before counting |
| `proto/anki/decks.proto` | `DeckTreeNode`: the last name component, and `new_count`, `learn_count`, `review_count` with children and limits applied |
| `rslib/src/scheduler/congrats.rs` | the congratulations info is the current deck's; 86,400 s when no learning card waits, else at least 60 |
| `pylib/anki/decks.py` | `set_current` writes the current deck through the backend |

`set_current_deck` remains U28's accepted operation. The prepared adapter's
interim fallback was removed during resume; no second handler is registered.
The source and tests otherwise resume 5839049e. The receipts below are
historical preparation evidence, not fresh verification of this integration.

Probed in the oracle image (`driver/tests/test_queue.py`): presentation order
and counts of learning cards due now, the main list with a new card spread
into two reviews, learn-ahead cards and one held for later; a card no logical
id names listed by its real id; nested deck counts with the empty Default
hidden; the congratulations fields of a deck with buried cards and of an empty
deck; `curDeck` written by stored name. All 33 scenarios under
`sched/queue/` and `sched/counts/` record the same values on the Apple lane
(no overlay entry, including the FSRS retrievability and relative overdueness
orders of `sched/queue/order/review-orders`).
