# Daily limits while building a queue

Spec: sched/limits
Requirements: R605, R606, R607, R608, R609, R740, AE21
Anki: 26.09.3
Scenarios: conformance/scenarios/sched/queue/limits/

How the new and review limits of a deck and of its parents decide how many cards a study queue takes. The limit in force on a day (Today-only, This-deck, preset) and the studied-today counters are `sched/presets#D2` and `#D4`; the two global options are `sched/config-keys#G2`; the queue that uses this file is `sched/queue-building`; the deck list applies the same remaining limits in its own way (`sched/counts#C4`).

## L1. Terms

- **Studied deck**: the deck the queue is built for (`sched/queue-building#B1`).
- **Kind of a card for limits**: a new card (queue 0) uses the **new** limit; a review (queue 2) and an interday learning card (queue 3) use the **review** limit; an intraday learning card (queue 1, or queue 4 left in a normal deck, `sched/queue-building#B4`) uses none (L5).
- Day numbers are `sched/day-clock`'s; "today" is the day of the build.

## L2. The remaining limits of one deck

For a normal deck on day `today`, with `newCardsIgnoreReviewLimit` read as `ignore`:

1. `review` = the review limit in force (`sched/presets#D2`) − `review_studied`; `new` = the new limit in force − `new_studied`. The studied counters are read with `sched/presets#D4` (0 and 0 when `last_day_studied` is not today).
2. When `ignore` is false: `review` = `review` − `new_studied`, then `new` = the smaller of `new` and `review`.
3. Each value below 0 becomes 0.

- The arithmetic is signed 32-bit, and a result outside that range wraps (two's complement) before step 3: a This-deck review limit of 2,147,483,647 with `review_studied` −1 gives −2,147,483,648, so 0. A negative counter (custom study by another client) raises the remaining limit: a new limit of 5 with `new_studied` −3 leaves 8 (golden `sched/queue/limits/today-only`, last step).
- A This-deck or Today-only limit is stored as an unsigned 32-bit value and enters step 1 as a signed 32-bit value: a stored value above 2,147,483,647 counts as that value − 4,294,967,296, so it leaves 0. The preset's limits are at most 9999 (`sched/presets#V3`).
- With `ignore` false, the new limit also says whether new cards are **capped by reviews** for this deck: L4 then lowers its new limit whenever its review limit goes down.
- A filtered deck, and a normal deck whose preset id names no preset, have the remaining limits 9999 and 9999 and are not capped by reviews. (A missing preset gives the built-in defaults only when a card is answered, `sched/presets#V1`; the queue and the deck list use these limits.)

Examples, preset 20 new and 200 reviews:

| This deck / Today only | `new_studied`, `review_studied` | `ignore` | remaining new, review |
|---|---|---|---|
| none | 0, 0 | false | 20, 200 |
| none | 0, 198 | false | 2, 2 |
| new 0 | 0, 198 | false | 0, 2 |
| new 5 | 3, 10 | false | 2, 187 |
| new 5 | 3, 10 | true | 2, 190 |
| none | 5, 199 | false | 0, 0 |
| new 5, Today-only `{limit 2, today}` | 0, 0 | false | 2, 200 |

## L3. The limits tree

- The tree holds the studied deck and all its descendants (`sched/queue-building#B2`). When `applyAllParentLimits` is true, it also holds each ancestor of the studied deck, up to the top level; the top-level ancestor is then the tree's root. Otherwise the studied deck is the root and decks above it do not limit it (R607).
- Siblings of ancestors are never in the tree.
- Each node starts with its deck's remaining limits (L2).
- A deck whose parent deck is missing (a damaged tree) is not in the tree, and neither are its descendants (`sched/counts#C1`). Anki fails to build the queue when such a deck holds a card the build would gather; Meguri leaves those cards out (`sched/queue-building#B11`).

## L4. Taking a card

The gathering of `sched/queue-building#B4` offers cards one at a time. A new card, review or interday learning card of deck `X`:

1. **may be taken** only while `X` and every ancestor of `X` in the tree each have more than 0 remaining for the card's kind (L1);
2. **when taken**, lowers by 1 the remaining limit of that kind at `X` and at each of those ancestors; when a lowered node is capped by reviews (L2) and the kind is review, that node's new limit then becomes the smaller of its new limit and its review limit.

- A card the sibling rule skips (`sched/queue-building#B5`) is not taken and lowers nothing. A card that rule 1 refuses is not taken either; gathering goes on with the next card, which may belong to a deck with room left.
- A deck reaches 0 for a kind as soon as it or one of its ancestors in the tree does, so the root's limit caps the whole queue (R606).
- Interday learning cards are gathered before reviews (`sched/queue-building#B4`), so they use up the review limit first (R609).
- New cards are gathered after reviews, so with `ignore` false the reviews taken lower every new limit on their path (R608).

Examples:

| Setup | Result |
|---|---|
| AE21: preset 20 new, 200 reviews; Japanese This-deck new 0, Japanese::Mining This-deck new 5, both with 198 reviews studied today, `ignore` false | studying Mining: 2 new cards; studying Japanese: 0 (golden `sched/queue/limits/ae21`) |
| Top (preset 10 reviews, 8 studied), Top::Mid (10 reviews, 7 studied), Top::Mid::Leaf (200); 2 reviews due in Mid, 6 in Leaf, 2 new in Leaf | `applyAllParentLimits` false: Leaf takes 6 reviews and 2 new cards, Mid takes 3 reviews; true: Leaf takes 2 reviews and no new card, Mid 2 reviews (golden `sched/queue/limits/parent-limits`) |
| review limit 4; 3 interday learning, 3 reviews and 3 new cards due | 3 interday learning, 1 review, no new card; with `ignore` true the 3 new cards too (golden `sched/queue/limits/interday-first`) |
| 20 new, 10 reviews; 6 reviews, 15 new | 6 reviews and 4 new cards; with 3 new cards studied today, 1; with `ignore` true, 15 (golden `sched/queue/limits/new-capped-by-reviews`) |
| Lang::Kanji This-deck new 3, its child Lang::Kanji::N5 with its own limit 30, studying Lang | Kanji and N5 together give 3 new cards, in every gather order (goldens `sched/queue/order/new-gather-*`) |

## L5. Intraday learning

Intraday learning cards (queue 1 and queue 4 left in a normal deck) are never limited and lower no limit, even when both limits are 0 (R609; golden `sched/queue/limits/intraday-uncapped`).

## L6. The remaining-limits snapshot

The queue result keeps, for each deck of the tree, its remaining new and review limits after gathering (`sched/queue-building#B8`). The session (U43) reads it; the deck list computes its own (`sched/counts#C4`).

## L7. Scenarios

| Scenario | Covers |
|---|---|
| `sched/queue/limits/ae21` | AE21, L2, L4 |
| `sched/queue/limits/parent-limits` | L3 (`applyAllParentLimits` off and on, several levels), L4 |
| `sched/queue/limits/interday-first` | L4 (interday learning before reviews; new cards capped by reviews; `newCardsIgnoreReviewLimit`) |
| `sched/queue/limits/intraday-uncapped` | L5 |
| `sched/queue/limits/today-only` | L2 (Today-only and This-deck limits across a day change; a negative counter) |
| `sched/queue/limits/new-capped-by-reviews` | L2 step 2, L4 |
| `sched/queue/gather/missing-preset` | L2 (missing preset: 9999, not capped) |
| `sched/queue/order/new-gather-*` | L4 with a This-deck limit on a subtree, in every gather order |

Meguri's required tests (`core/crates/meguri-sched/tests/queue.rs`) must add L2's 32-bit cases (a stored limit above 2,147,483,647) and the damaged tree of L3, with expected values from these tables.
