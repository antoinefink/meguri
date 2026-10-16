# Memory states

Spec: sched/memory-state
Requirements: R645, R649, R650, R686, R687, R688, R690, R697, R704, R1849, R1852
Anki: 26.09.3
Scenarios: conformance/scenarios/sched/memory/

Where a card's FSRS memory state comes from when the card has none, how memory states, `dr` and `decay` are recomputed in bulk without rescheduling, the count behind "N of M cards included", and the repair of the last review time. The items these build on are `sched/fsrs-items`; the card data keys are `sched/card-data#K5` and `model/card-data-json`; a card's home deck, home preset, parameter list, decay and desired retention in force are `sched/presets#V1`, `#V6` and `#V7`.

## M1. Terms

- A card's **memory state** is the pair `s` (stability) and `d` (difficulty) of its card data. A card **has no memory state** when its data has neither; a lone `s` or `d` reads as neither (`model/card-data-json#C3` rule 4).
- A card is **new** when its `type` is 0.
- **The model** of a preset is the crate's `FSRS` built from the preset's parameter list (`sched/fsrs-transitions#T2.1`). A list U45 cannot build reports `UnsupportedParams` (R704).

## M2. A memory state on the fly (R650)

When U45 computes a card's next states (for the buttons, and again when U44 applies the answer), FSRS is on, the card is not new and it has no memory state, the memory state used for the computation is:

1. the memory state of the card's memory item (`sched/fsrs-items#I3` to `#I5`), built from all its review-log rows with its home preset's model, cutoff date and historical retention;
2. when the card gives no item (`sched/fsrs-items#I6`) and its `ivl` is not 0: the SM-2 conversion of the card itself, `FSRS::memory_state_from_sm2(factor / 1000, ivl, historical retention)` with the card's `factor` and `ivl` as `f32`. The card's `factor` is taken as it is: 0 stays an ease of 0, and a `factor` of 1100 or less is not read as a difficulty (unlike a review-log row's, `sched/fsrs-items#I5`);
3. otherwise (no item and `ivl` 0) none: FSRS computes the states of a new card.

- Nothing is written. The state is only an input of the next states; the answer stores the state it leads to, with `dr`, `decay` and `lrt` (`sched/card-data#K5`). Computing the states again gives the same state.
- When the home preset's cutoff date is refused (`sched/fsrs-items#I1.3`) or the crate fails, the states cannot be computed and the card cannot be answered (Anki shows an error). A home preset reporting `UnsupportedParams` is U45's case (R704).
- A card in a filtered deck uses its home deck's preset (`sched/presets#V1`).

| Golden `sched/memory/…` | Card | Memory state on the fly |
|---|---|---|
| `on-the-fly` c1 | H (learning rows, reviews, a lapse), no memory state | 39.451553, 7.6402645 |
| `on-the-fly` c3 | the same with a stored memory state 30.0, 6.0 | 30.0, 6.0 (stored, not computed) |
| `no-history` c1 | review card, `ivl` 20, `factor` 2500, no review log | 20.0, 6.413895 |
| `no-history` c2 | the same with `factor` 0 | 20.0, 10.0 |
| `no-history` c8 | review card, `ivl` 100, `factor` 820, no review log | 100.0, 10.0 |
| `no-history` c4, c7 | learning card, or review card with `ivl` 0, no review log | none |
| `no-history` c5, c6 | as c1 with only a set-due-date or a rescheduled row | as c1 |
| `ignore-before` c4 | H, all of it before the cutoff | the card's own: 25.0, 6.8748355 |

`on-the-fly` also shows that computing the states writes nothing (`data`, `mod` and `usn` unchanged) and what the first answer stores.

## M3. Elapsed days when `lrt` is missing (R649)

`sched/fsrs-transitions#T2.3` counts FSRS's elapsed days from the card data's `lrt`. When the card data has no `lrt`, the instant of the card's last review stands in for it: the `id` of the card's latest review-log row whose `ease` is 1 to 4 and that is not a cram row (`type` 3 with `factor` 0), divided by 1000 and rounded down. Reset markers do not matter here: a row before the last marker counts. When the card has no such row, the elapsed days are 0.

| Golden `sched/memory/…` | Card | Elapsed days (`next_day_at` 2026-03-03 04:00 UTC) |
|---|---|---|
| `on-the-fly` c1 | no `lrt`, last graded row 2025-11-11 10:00 | 111 |
| `on-the-fly` c2 | `lrt` 2026-02-20 10:00 | 10 |
| `reset` c5 | no `lrt`, H then a Reset marker as the last row | 111 (from H's last row) |
| `no-history` c1 | no `lrt`, no review log | 0 (Good keeps the stability) |

U46 provides this lookup; U45 calls it. It is one indexed query per card (by `cid`, latest `id` first).

## M4. Recomputing in bulk (R686, R687, R697)

### M4.1 Which cards and what they get

The recompute takes a set of cards (M7 says which sets the callers pass). A card of the set is **processed** when it is not new and has at least one review-log row. Every other card of the set keeps its row exactly as it is, stale `s`, `d`, `dr` and `decay` included.

For each processed card, with its home deck and home preset:

- `s`, `d`: the memory state of its memory item (`sched/fsrs-items#I3` to `#I5`) with the preset's model, cutoff date and historical retention. When the card gives no item, `s` and `d` are removed; the card's own values are not used (unlike M2).
- `dr`: the desired retention in force for its home deck (`sched/presets#V7`).
- `decay`: the decay of the preset's parameter list (`sched/presets#V6`), as stored before the crate's clamp.

Nothing else changes: not `type`, `queue`, `due`, `ivl`, `factor`, `reps`, `lapses`, `left`, `odue`, `odid`, `flags`, nor the data keys `pos`, `lrt` and `cd`, nor any review-log row. The recompute never reschedules and never writes a review-log row (R686, R697); Anki's optional "reschedule cards on change" is not part of it. Suspended and buried cards are processed like the others.

### M4.2 Writing only what changed (R687)

- A processed card is written only when its new values differ from its stored ones as stored: compare presence and value of `s`, `d`, `dr` and `decay`, each rounded as `model/card-data-json#C4` writes it (4, 3, 2 and 3 places), between the stored data (read with C3) and the new values. When all four are equal, the card's row is not written at all: its `data` text, `mod` and `usn` stay as they are.
- A changed card is written with its `data` rebuilt (C4), `mod` = now in seconds and `usn` = the collection's local usn (`storage/ids-usn-mtime`).
- Anki writes and stamps every processed card, changed or not. Meguri's difference is the deviations `memory-recompute-unchanged-mod` and `memory-recompute-unchanged-usn` in `conformance/deviations.yaml`.

### M4.3 The operation

- An engine op over a card set given as presets, decks or the whole collection. Its source and undo name come from the caller: a user's save is one undoable step (U49 names it "Deck Options"); the optimizer and maintenance run it with the sources `Optimizer` and `Maintenance`. Undo restores the previous `s`, `d`, `dr`, `decay`, `mod` and `usn` of every card it wrote.
- It reads the review log in one pass ordered by `cid` then `id` (`sched/fsrs-items#I1.1`), one preset's cards at a time, and never queries card by card.
- It runs in batches with a cancel token and progress (KTD5). Cancelling, or any failure, leaves the collection as it was: nothing is written (one transaction for the op, or rolled back).
- Cards whose home preset reports `UnsupportedParams` are skipped and keep their bytes (R704). Anki has no such case: it refuses to save parameters its crate cannot build.
- A refused cutoff date of a preset whose cards are processed, or a crate error on any card, fails the whole op with nothing written, as Anki fails the save.
- The change stream marks the written cards as memory-state-only changes (`engine/change-stream`).

### M4.4 Example

Scenario `sched/memory/recompute` (its golden comes with the oracle op `save_deck_options`, `conformance/deck-options-ops#E1`): after Lang's desired retention goes from 0.9 to 0.85, c1 (H, no memory state before) should hold `{"s":39.4516,"d":7.64,"dr":0.85,"decay":0.154}` (the state of `sched/fsrs-items#I5`'s first example, rounded), c4 (a set-due-date row only, a stale state) only `dr` 0.85 and `decay` 0.154, while c3 (no review log) and c5 (new) keep their stale values and the review-log digest stays the same. The golden is the authority once recorded.

## M5. "N of M cards included" (R690)

A read query for the deck options screen (U49, U232), given a preset and the date text being edited (not necessarily the stored one):

- **Card set:** the cards the preset's search finds, its `param_search` (schema-11 `weightSearch`) when not empty, otherwise `preset:"<name>" -is:suspended`; then without new cards.
- **M** (`total`): the number of cards in the set.
- **N** (`included`): the number of those cards with at least one review-log row whose stored `type` is 0 and whose `id` is after the cutoff of the given text (`sched/fsrs-items#I1.3`; the empty text gives 0, so every such row counts). The button does not matter, and the stored `type` is compared: a row of an unknown type does not count here.
- **Warning** (R690): when M is above 0 and 10 × N is below M.
- A refused date text gives an error (`invalid_date` in the conformance observation) instead of the counts.
- Anki's screen asks for the count only when the date is neither empty nor `1970-01-01`, and colours the line in three levels: under 10 % included (the warning), some excluded, all included. The wording is U232's.

Golden `sched/memory/ignore-before-count` (recorded with the observation `ignored_before_count`, `conformance/deck-options-ops#E2`) covers a row at exactly the cutoff, a row a second before, suspended, new and review-only cards, a row without a button, a row of type 7, another preset's card, and the date texts of `sched/fsrs-items#I1.3`.

## M6. Repairing the last review time (R688)

A pure rule over a card and its review-log rows (in `id` order), which Check Database applies to every card (U128):

- It applies to cards that are not new and have at least one review-log row. Other cards are left alone, whatever their `lrt`.
- The expected `lrt` is the second (`id` / 1000, rounded down) of the card's latest graded row (`sched/fsrs-items#I2`; a Reset marker that has a button counts as graded here), unless a Reset marker without a button comes after that row; then there is none.
- When the stored `lrt` differs from the expected one (in value, or present on one side only), the card data gets the expected value, or loses `lrt` when there is none. Only `lrt` changes; the data is rebuilt (`model/card-data-json#C4`) but the row keeps its `mod` and `usn`.
- The number of cards changed is Check Database's `card_last_review_time` count (`conformance/collection-check#K5`). U128 marks the schema as modified (a full sync) when the count is above 0.
- This is not M3's lookup: M3 ignores Reset markers and requires an `ease` from 1 to 4.

Golden `sched/memory/lrt-repair`:

| Card | Stored `lrt` | After |
|---|---|---|
| review card with H | 1000 | 1762855200 (H's last row) |
| review card with H | none | 1762855200 |
| review card, H then a Reset marker as the last row | 1762855200 | removed |
| new card with H | 1000 | 1000 (left alone) |
| review card without review log | 5000 | 5000 (left alone) |
| review card with H | 1762855200 | unchanged, not counted |
| review card, H then a set-due-date row and a cram row | 1000 | 1762855200 |
| review card, H then a graded row with `id` 1762907200999 | 1000 | 1762907200 |
| review card, H then a Reset marker with `ease` 3 | 1000 | 1764576000 (the marker's second) |
| learning card with two learning rows | 1000 | the second row's second |

Eight cards are counted; every row keeps `mod` and `usn`.

## M7. Callers

Which cards each caller passes to M4 and when, as Anki does it. The callers' units implement the triggers; U46 provides the op.

### M7.1 Saving deck options (U49, R686)

When Anki saves the deck options, it compares every normal deck before and after the save. A deck is recomputed when FSRS was turned on or off by the save, when the parameter list in use for it (`sched/presets#V6`) changed, or when its desired retention in force (`sched/presets#V7`: its own value, or its preset's) changed. Reassigning a deck to another preset counts through those values. A recomputed deck's set is the cards whose `did` or `odid` is that deck, processed with the deck's preset after the save. Changing only the historical retention, the cutoff date, steps or limits recomputes nothing. The save, the preset writes and the recompute are one undoable step, "Deck Options" (`conformance/undo-ops#V4` `deck_options`).

Golden `sched/memory/recompute`: Lang's desired retention changed from Words recomputes Words, not Phrases (its own retention holds) nor Misc (another preset); Lang's parameter list changed from empty to the same values written out recomputes Words and Phrases; Phrases' own retention recomputes Phrases only; historical retention and cutoff date alone recompute nothing; moving Misc to Lang recomputes Misc.

### M7.2 Change Deck (U27, U48; R645)

Anki's Change Deck removes `s`, `d`, `dr` and `decay` from the cards it moves (`sched/card-data#K5`). Then, when FSRS is on and at least one card moved, it recomputes, inside the same op, the cards of the target deck (`did` or `odid`) that have no memory state, the moved ones and any that were already there without one. A moved card without review-log rows is not processed (M4.1), so it ends with none of the four keys until its next answer. So R645 holds at once, not lazily.

### M7.3 Optimizer (U47, R697)

After adopting new parameters for a preset, the optimizer recomputes the cards of every deck using that preset, in the same Optimizer op that writes the parameters (KTD8), and never reschedules.

### M7.4 Turning FSRS on (U50, R702, R703)

When Meguri turns the global `fsrs` key on, it recomputes every card of the collection, as Anki's save does for every deck when FSRS is toggled.

### M7.5 Check Database (U128, R688)

Check Database runs M6 on every card; it runs no M4 recompute.

## M8. Meguri budgets

- R1849: a recompute of 100,000 cards takes at most 3 s on the M3 Max in release, in the background and cancellable. The bench is `core/tools/meguri-cli/src/bench/memory_recompute.rs`, stored by the U12 harness.
- R1852: peak memory during FSRS optimization stays at or below 300 MB on iPhone at 500,000 cards; the item builder holds one preset's items at a time (`sched/fsrs-items#I7`), and the recompute holds one batch of cards with their rows.
- The cost is SQLite (KTD8): the ordered review-log pass and batched card updates, not the crate.

## M9. Scenarios

| Scenario | Covers | Golden |
|---|---|---|
| `sched/memory/on-the-fly` | M2, M3, the first answer after it | recorded |
| `sched/memory/sm2-history` | `sched/fsrs-items#I3`, `#I5` (incomplete histories), historical retention 0.9 and 0.8 | recorded |
| `sched/memory/no-history` | M2 steps 2 and 3, M3 without rows | recorded |
| `sched/memory/reset` | `sched/fsrs-items#I3` (Reset markers, runs without a marker), M3 | recorded |
| `sched/memory/skipped-rows` | `sched/fsrs-items#I2`, `#I3` (manual, rescheduled, cram and early filtered rows) | recorded |
| `sched/memory/crafted-rows` | `sched/fsrs-items#I2` (rows no client writes) | recorded |
| `sched/memory/ignore-before` | `sched/fsrs-items#I1.3`, `#I3` (cutoff) | recorded |
| `sched/memory/ignore-before-boundary` | rows at exactly the cutoff | recorded |
| `sched/memory/lrt-repair` | M6 | recorded |
| `sched/memory/recompute` | M4, M7.1 | awaiting the oracle op `save_deck_options` |
| `sched/memory/ignore-before-count` | M5, `sched/fsrs-items#I1.3` | awaiting the oracle observation `ignored_before_count` |

- The scenarios observe through `next_states` (memory states compare with the 1e-5 tolerance of `conformance/observations#O7`), `card`, `db_exec` and `check_db`. Meguri runs them once its executor has U45's next states, U44's answer, U128's `check_db` and U49's `save_deck_options`; until then they are `pending`, and U46's tests read their goldens directly: the stored rows and review logs as input, the memory states, card rows and counts as expected output.
- Many cards come in pairs whose memory states must be equal (the scenario comments say which); the pairs pin down the start rules without depending on the crate's numbers.
- Meguri-only tests (no oracle): a recompute cancelled half way writes nothing; a user-initiated recompute is one undo step that restores `s`, `d`, `dr`, `decay`, `mod` and `usn`; presets with `UnsupportedParams` are skipped; the op reads the review log once per preset; the R1849 bench.
- Training items (`sched/fsrs-items#I7`) are checked by U47's optimizer scenarios.

## M10. Differences from Anki

- M4.2: unchanged cards keep `mod` and `usn` (R687); deviations `memory-recompute-unchanged-mod` and `memory-recompute-unchanged-usn`.
- M4.3: cards of presets with parameters the crate cannot build are skipped (R704); Anki cannot have such presets after its own save.
- Meguri never turns FSRS off (R702), so Anki's clearing of `s`, `d`, `dr` and `decay` when a save turns FSRS off is not reproduced.
