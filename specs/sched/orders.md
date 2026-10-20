# Queue orders and mixing

Spec: sched/orders
Requirements: R611, R613, R614, R615, R616, R617, R618, R619, R1896
Anki: 26.09.3
Scenarios: conformance/scenarios/sched/queue/order/, conformance/scenarios/sched/queue/mix/

The orders a study queue uses: the six new-card gather orders, the five new-card sort orders, the thirteen review sort orders, their hash and their tie-breaks, and the rule that mixes two lists. The option values and their stored keys (`newGatherPriority`, `newSortOrder`, `reviewOrder`, `newMix`, `interdayLearningMix`) are `sched/presets#V2`, `#V4`; an unknown stored value acts as 0. Which preset's options apply is `sched/queue-building#B3`. Orders must match Anki's exactly, not only as sets (R1896).

## O1. The hash

- **FNV-1a, 64 bits**: start from 14695981039346656037 (`0xcbf29ce484222325`); for each input byte, XOR the byte into the low 8 bits, then multiply by 1099511628211 (`0x100000001b3`) modulo 2^64.
- **Input**: each argument, in order, as 8 bytes: its value as a signed 64-bit integer, little-endian.
- Two readings of the result are used, and they sort differently:
  - **signed hash** `fnvhash(a, b)`: the result read as a signed 64-bit integer, sorted ascending as signed. Meguri registers it on every connection as the deterministic SQL scalar function `fnvhash`, taking any number of arguments, zero included (U22's connection hook; Anki's connections have the same function). An argument that is not an integer (a real, text, a blob or NULL) makes the call an SQL error;
  - **unsigned hash** `h(a, b)`: the same result read as an unsigned 64-bit integer, sorted ascending as unsigned.
- **Day salts**: the gather orders use the **gather salt** (today × 2,654,435,761) mod 2^32; the sort orders use today itself.

| Arguments | Signed hash | Unsigned hash |
|---|---|---|
| (none) | −3750763034362895579 | 14695981039346656037 |
| (0) | −6284781860667377211 | 12161962213042174405 |
| (1, 0) | 4116863941369023524 | 4116863941369023524 |
| (0, 0) | −8637869204239850395 | 9808874869469701221 |
| (−1, 5) | −2510909584869376200 | 15935834488840175416 |
| (1894701600000, 1894701600) | 5793145922124942247 | 5793145922124942247 |
| (1894701600000, 2654435761) | −3475538189761224168 | 14971205883948327448 |
| (42, 7, 9) | 3286408356281142785 | 3286408356281142785 |

| Today | Gather salt |
|---|---|
| 0 | 0 |
| 1 | 2654435761 |
| 2 | 1013904226 |
| 3 | 3668339987 |
| 100 | 3450571044 |
| 2000 | 291944144 |

## O2. New-card gather orders (R613)

`sched/queue-building#B4` phase 4 offers the new cards (queue 0) of the studied decks in the gather order; `sched/limits#L4` and `sched/queue-building#B5` decide which are taken.

| Value | Option | Offering order |
|---|---|---|
| 0 | Deck (default) | deck by deck in deck order (`sched/queue-building#B2`); within a deck, its own cards by ascending position (`due`), then ascending template ordinal (`ord`), then ascending card id. A deck with 0 new left is passed over; a deck stops at the first card offered when it has 0 left. |
| 5 | Deck, then random notes | deck by deck as for 0; within a deck by ascending `fnvhash(note id, gather salt)`, then ascending ordinal |
| 1 | Ascending position | all studied decks together, by ascending position, then ascending ordinal |
| 2 | Descending position | all together, by descending position, then ascending ordinal |
| 3 | Random notes | all together, by ascending `fnvhash(note id, gather salt)`, then ascending ordinal |
| 4 | Random cards | all together, by ascending `fnvhash(card id, gather salt)` |

- Siblings share a position, so under 0, 1, 2, 3 and 5 they stay together in ordinal order unless another note has the same position (1 and 2) or the limits or B5 drop one.
- In 1, 2, 3 and 4 a card whose deck (or an ancestor) has 0 left is refused and the next card is offered; under 0 and 5 the deck's remaining cards are not offered (the deck has nothing left to give either way).
- Ties left by these keys: O6.

## O3. New-card sort orders (R614)

Applied to the new cards taken, after gathering:

| Value | Option | Result |
|---|---|---|
| 0 | Card type, then order gathered (default) | ascending template ordinal; equal ordinals keep gather order |
| 1 | Order gathered | gather order |
| 2 | Card type, then random | ascending ordinal, then ascending `h(card id, today)` |
| 3 | Random note, then card type | ascending `h(note id, today)`, then ascending ordinal |
| 4 | Random | ascending `h(card id, today)` |

`h` is the unsigned hash and its salt is the day number itself, not the gather salt (O1). When the studied deck's preset is missing, order 1 applies (`sched/queue-building#B3`).

Goldens `sched/queue/order/new-gather-<gather order>` hold each gather order with each sort order over one fixture of 25 new cards in five decks, with a This-deck limit on a subtree.

## O4. Review sort orders (R611, R616, R617, R618)

Interday learning cards and reviews are each offered in the studied deck's review sort order (`sched/queue-building#B4` phases 2 and 3), by these keys, then always by ascending `fnvhash(card id, card mtime)` (the card's `mod` column):

| Value | Option (Meguri's FSRS label) | Keys before the hash |
|---|---|---|
| 0 | Due date, then random (default) | ascending `due` |
| 1 | Due date, then deck | ascending `due`, then the card deck's place in deck order |
| 2 | Deck, then due date | the deck's place in deck order, then ascending `due` |
| 3 | Ascending intervals | ascending `ivl` |
| 4 | Descending intervals | descending `ivl` |
| 5 | Ascending ease (descending difficulty) | descending difficulty; cards without one last |
| 6 | Descending ease (ascending difficulty) | ascending difficulty; cards without one first |
| 7 | Ascending retrievability | ascending retrievability; cards without a memory state first |
| 11 | Descending retrievability | descending retrievability; cards without a memory state last |
| 12 | Relative overdueness | ascending relative-overdueness value |
| 8 | Random | none |
| 9 | Order added | ascending note id, then ascending ordinal |
| 10 | Reverse order added | descending note id, then ascending ordinal |

- **Deck place**: the position of the card's `did` in the deck order of `sched/queue-building#B2`.
- **Per-card values** use U82a's single definitions in `search/card-extractors#X2`, `#X5`, `#X6`:
  - difficulty: `meguri_card_f32(data, 'd')`;
  - orders 7 and 11: `meguri_retrievability(data, due, ivl, today, now)`;
  - order 12: `meguri_relative_retrievability(data, due, ivl, today, now)`.
  Both retrievability calls have five arguments. `next_day_at` is not an argument. Pass the build's today and now; do not duplicate arithmetic in the scheduler. The extractor contract owns exact conversions, missing-data rules, NULL results, wrapping and f32 rounding. Orders 7/11 have no relative-overdueness fallback. Only order 12 has that fallback, under X6's conditions.
- "Cards without one" sort as SQL NULL does: first in an ascending key, last in a descending one, then by the hash.
- For a card in a filtered deck Anki passes `odue` in place of `due` to the retrievability functions; Meguri's studied decks hold no such card (`sched/queue-building#B2.2`).
- Golden `sched/queue/order/review-orders` holds the thirteen orders over 28 reviews and 4 interday learning cards in three decks, on day 200, with stored memory states, missing ones, missing `decay` and `lrt`, and ties.

### O4.1 With FSRS off

Meguri schedules with FSRS on (R702), so its builds always use the table above. For reference, Anki with FSRS off sorts 5 by ascending `factor`, 6 by descending `factor`, and 12 by ascending −(1 + (today − `due` + 0.001) ÷ `ivl`); the others are unchanged. Meguri does not implement these.

## O5. Mixing two lists (R619)

The new/review order and the interday learning/review order each take one of three values:

| Value | Option | Result for a first list A and a second list B |
|---|---|---|
| 0 | Mix with reviews (default) | B spread evenly through A, by the rule below |
| 1 | Show after reviews | A, then B |
| 2 | Show before reviews | B, then A |

- Interday learning/review order: A is the review list, B the interday learning list.
- New/review order: A is the result of the first mix, B the new list.
- **Spreading rule**, computed in 32-bit floating point: let `a` and `b` be the sizes of A and B and `r` = (a + 1) ÷ (b + 1). With `i` items of A and `j` items of B already placed, while both have items left: take the next item of B when (j + 1) × `r` < i + 1, otherwise the next item of A. When one list is used up, the rest of the other follows. `r` and the product are rounded to 32-bit floats; the comparison is strict.

Examples (A as reviews `r`, B as new cards `n`; goldens `sched/queue/mix/spread-sizes`, `new-review`):

| a, b | Result |
|---|---|
| 3, 3 | r n r n r n |
| 3, 2 | r n r n r |
| 3, 6 | n r n n r n n r n |
| 3, 8 | n n r n n r n n r n n |
| 7, 3 | r r n r r n r r n r |
| 10, 1 | r r r r r n r r r r r |
| 1, 10 | n n n n n r n n n n n |
| 0, 4 | n n n n |
| 4, 0 | r r r r |

## O6. Ties

- The final review key is the ascending signed hash of (card id, mtime). Distinct pairs can collide; a finite 64-bit hash is not injective. The recorded cases establish no further key for such a collision. Exact order between distinct colliding pairs remains an evidence gap; do not append a card-id key as a claimed parity rule.
- New-card gather order 0 and 5 break remaining ties within a deck by ascending card id.
- Orders 1 and 2, and the learning list (`sched/queue-building#B6`), break remaining ties by ascending deck id (`did`), then ascending card id. This is Anki's row order; golden `sched/queue/order/ties` (decks created in the reverse of their name order).
- New-card sort orders 0 and 1 keep gather order for ties; 2, 3 and 4 tie only on equal hashes.

## O7. What makes an order change

| Order | Changes when |
|---|---|
| gather 3, 4, 5 | the day number changes (gather salt) |
| sort 2, 3, 4 | the day number changes |
| review 8, and every review order's last key | a card's `mod` changes; not with the day |
| review 7, 11, 12 | the build instant: retrievability depends on now |

Within one day and with no card change, a random order is stable (goldens `sched/queue/order/new-gather-*`, last three reads). Review order 8 keeps the relative order of the cards already due on the next day (golden `sched/queue/order/review-orders`, last read).

## O8. Scenarios

| Scenario | Covers |
|---|---|
| `sched/queue/order/new-gather-deck`, `-deck-random-notes`, `-ascending`, `-descending`, `-random-notes`, `-random-cards` | O1, O2, O3, O7 |
| `sched/queue/order/review-orders` | O1, O4, O7 |
| `sched/queue/order/ties` | O6 |
| `sched/queue/mix/new-review`, `interday-review`, `spread-sizes` | O5 |

Meguri's required tests must check O1's two tables and O5's table directly.

### O8.1 Required 60-card supplement

`sched/queue/order/mixed-sixty` is an original 60-card mixed-deck fixture: 24 new, 28 due review, 4 due interday learning, 2 intraday learning and 2 future review cards. Thirty two-card notes provide siblings and both template ordinals. Positions, intervals, stored memory states and missing-memory cases vary across five decks. Limits allow every eligible card; sibling burying is off.

The scenario observes each of the six gather orders crossed with all five new sort orders, then all thirteen review orders. It repeats random reads at the same instant, two hours later and on the next day, with an explicit timing read and rebuild before each comparison. Gather/sort randomness uses day salts; review randomness uses card mtime. It records complete queue ordered ids, kinds and counts, plus card rows identifying all 60 inputs. Orders 7/11 include missing memory; order 12 includes memory without `dr` and no-memory fallback inputs.

This supplement has no expected values until the oracle owner records it. Preserve the smaller recorded fixtures unchanged. Linux recording, comparison and native comparison are required; a missing golden is an outstanding requirement, not permission to reduce coverage.
