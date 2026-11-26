# Building a study queue

Spec: sched/queue-building
Requirements: R602, R603, R604, R609, R610, R612, R620, R623, R626, R630, R631, R706, R707, R1894, R1895
Anki: 26.09.3
Scenarios: conformance/scenarios/sched/queue/

Which cards a deck's study queue holds, in which order it presents them, and the counts it reports. Limits are `sched/limits`, the orders and the mixing rule `sched/orders`, the deck list's counts and a finished deck's facts `sched/counts`. The session that answers cards from the queue, updates it after each answer and reacts to waiting learning cards is U43's. Card columns and queues are `sched/card-data#K1`, `#K2`.

## B1. Inputs

A build takes:

- the **studied deck**: the current deck, `curDeck` (`decks/decks`); when it names no deck, the Default deck (id 1);
- the instant **now** (epoch seconds) and, from the maintained day clock at that instant (`sched/day-clock#D8`), **today** (the day number) and **next_day_at**;
- **L**, the learn-ahead limit in seconds: `collapseTime` (`sched/config-keys#G2`, default 1,200);
- the collection keys `fsrs`, `applyAllParentLimits` and `newCardsIgnoreReviewLimit` (`sched/config-keys#G2`);
- the decks and presets.

A build reads only. Its caller completes automatic day maintenance (`sched/day-clock#D8`) before acquiring the read snapshot. It never submits maintenance from inside a busy writer operation. Explicit conformance timing and count reads have the separate compatibility prelude of `conformance/queue-ops#Q3` and `#Q5`. Anki uses temporary connection state for the studied decks; this is not collection data.

## B2. The studied decks

- The queue draws from the studied deck and every deck whose name starts with the studied deck's name followed by `::` (its descendants), compared with the `unicase` comparison (`text/unicode-primitives#U2`) (R603). No other deck, and never the whole collection.
- **Deck order**: these decks sorted by their full names with the `unicase` comparison, the name written with the separator U+001F between components as stored (`model/schema11-json#S11`). The studied deck comes first and every deck is followed by its own descendants. `sched/orders` uses this order for "deck" keys.

### B2.1 Which cards

A card belongs to the queue's decks when its `did` is one of them. A card whose home deck (`odid`) is a studied deck but which sits in a filtered deck outside them is not gathered (R612).

### B2.2 Filtered decks among the studied decks

Anki gathers the cards of a filtered deck that is a descendant of the studied deck, preview-repeat cards (queue 4) and interday learning included, with that deck's limits of 9999. When a normal deck is selected, Meguri builds the queue from normal decks alone (R612). When a filtered deck is selected directly, `decks/filtered-study#F3` and F4 apply. Deviation `queue-nested-filtered-deck` (golden `sched/queue/gather/nested-filtered`). The deck list still counts such a deck under its parent (`sched/counts#C3`), and the deck page shows the difference (`sched/counts#C5`).

## B3. Which preset decides what (R604)

- **Display options** come from the studied deck's preset: new-card gather order, new-card sort order, review sort order, new/review order and interday learning/review order (`sched/presets#V2`, `#V4`).
- When the studied deck's preset id names no preset, the display options are: new-card sort order 1 (order gathered) and 0 for the four others (deck; due date, then random; mix with reviews; mix with reviews). Golden `sched/queue/gather/missing-preset`.
- **Per-card options** come from each card's home preset (`sched/presets#V1`): the three sibling-burying options used in B5, and everything an answer uses (U44, U45).
- **Limits** come from each deck's own remaining limits (`sched/limits#L2`).

## B4. Gathering (R610)

Cards are gathered in four phases, in this order. Within a phase cards are offered in the order the phase states; each offered card is either taken, skipped by burying (B5) or refused by a limit (`sched/limits#L4`).

1. **Intraday learning**: every card in queue 1 or queue 4 with `due` at or before `next_day_at`. All are taken; no limit applies (`sched/limits#L5`). They go to the learning list (B6). They are offered in ascending deck id, then ascending card id, which B6's ordering keeps for equal keys.
   - Queue 4 (preview repeat) belongs to filtered decks and is also supported when another client left such a card in a normal deck. It is then an intraday learning card in every respect: B5, B6, B8 and B9 treat queues 1 and 4 alike (golden `sched/queue/gather/preview-in-normal-deck`). How the session answers it is U43's and U44's.
2. **Interday learning**: cards in queue 3 with `due` at or before today, in the studied deck's review sort order (`sched/orders#O4`), using the review limit.
3. **Reviews**: cards in queue 2 with `due` at or before today, in the review sort order, using the review limit.
4. **New cards**: cards in queue 0, in the new-card gather order (`sched/orders#O2`), using the new limit.

- Cards in queues −1, −2 and −3 (suspended, buried by a sibling, buried by the user) are never gathered (R612). A flag changes nothing.
- A phase stops offering cards once the root of the limits tree has 0 left for its kind; skipping a phase changes nothing else.
- Taken interday learning cards and reviews form two lists in the order taken; taken new cards form one list in gather order, which the new-card sort order then rearranges (`sched/orders#O3`).

## B5. Burying siblings while gathering (R630, R631)

- Each note gets a **bury mode**, three flags (new, review, interday learning), all off at the start of the build.
- When a card is offered and no limit refuses it (rule 1 of `sched/limits#L4`; intraday learning cards are never refused):
  1. it is **skipped** when its note's bury mode, as it stood before this card, has the flag for the card's kind: new for a new card, review for a review, interday learning for an interday learning card. Intraday learning cards are never skipped;
  2. then, skipped or not, the note's bury mode gains the flags its home preset has on (`bury_new`, `bury_reviews`, `bury_interday_learning`): a flag once set stays.
- A card whose limit refused it neither is skipped nor changes the note's bury mode.
- So a card is skipped because of the presets of siblings offered before it that no limit refused (taken or skipped), never because of its own preset; gathering order decides which sibling stays (intraday learning, then interday learning, then reviews, then new cards). A new card never skips a review or an interday learning card of its note, since those come first.
- A skipped card lowers no limit and is not written: it stays in queue 0, 2 or 3 and the deck list still counts it (`sched/counts#C3`). It is buried (queue −2) only when a sibling is answered (R632, U44).

| Bury options on (one preset) | Note's cards | Taken |
|---|---|---|
| new | review due today, new | the review (golden `sched/queue/bury/new-siblings`) |
| new | new, new | the first in gather order |
| new | intraday learning, new | the learning card |
| review | two reviews due today | the first in review order (golden `sched/queue/bury/review-siblings`) |
| review | interday learning, review | the interday learning card |
| review | intraday learning, review | the learning card |
| review | review, new | both |
| interday learning | intraday learning, interday learning | the intraday learning card (golden `sched/queue/bury/interday-siblings`) |
| interday learning | interday learning, review | both |

With two presets (P::A burying all kinds, P::B burying none, P::A gathered first), golden `sched/queue/bury/mixed-presets`: a note with new cards in P::B, P::A, P::B keeps only its P::A card; a note whose review in P::B is taken first keeps that review and its first new card in P::A, which then skips the second.

## B6. The learning list (R620)

- Intraday learning cards are ordered by: attempted first (`reps` above 0 before `reps` 0), then ascending `due`; cards equal on both keep the order of B4 phase 1 (ascending deck id, then card id).
- At the build instant the list has three parts:
  - **due now**: `due` at or before now;
  - **learn ahead**: `due` after now and at or before now + L;
  - **later**: `due` after now + L. These are held for the session (U43) and not presented yet.
- Golden `sched/queue/learn-ahead/window` (L 1,200 s): cards due 5 minutes ago (3 reviews), 2 minutes ago (never answered), in 15 minutes, in exactly 20 minutes and in 25 minutes come as the first two, then the main list, then the 15- and 20-minute cards; the 25-minute card is not presented.

## B7. The main list

The interday learning list and the review list are combined with the interday learning/review order, then the result and the new list with the new/review order (`sched/orders#O5`).

## B8. The queue value and its order

A build returns a value; the session (U43) owns the mutable state made from it. The value holds:

- the main list: entries `{card id, kind (new, review or interday learning), card mtime}` in the order of B7;
- the learning list: entries `{card id, due, reps, card mtime}` in the order of B6;
- the learning cutoff (the build instant) and L;
- the counts (B9);
- the remaining-limits snapshot (`sched/limits#L6`);
- today, next_day_at, and whether FSRS is on.

**Presentation order**, when nothing is answered: the learning list's due-now part, then the main list, then the learning list's learn-ahead part (R602). The `queue` observation (`specs/conformance/queue-ops.md` Q2) reports exactly this sequence with each card's kind (`learning` for both learning lists' entries and for interday learning; `review`; `new`).

## B9. Counts (R626)

- **new**: the new cards taken;
- **review**: the reviews taken;
- **learning**: the intraday learning cards (queues 1 and 4, B4 phase 1) with `due` at or before now + L, plus the interday learning cards taken.

Counts count cards, not remaining steps. The deck list counts differently (`sched/counts#C3`); the deck page shows the queue's counts and the difference (`sched/counts#C5`).

## B10. When a queue must be rebuilt (R623)

A built queue stays valid until a change affects it. Classification returns **rebuild**, **patch** (keep the queue and render the current card again) or **none**. The first matching row decides.

The inputs are the `ChangeSet`, the `ObservedCommit` seq and generation, its changed-card before/after rows (`engine/change-stream#S1`, `#S2`), and the queue session's ownership context. `ChangeSet` alone cannot prove flag-only changes or ownership of an answer, undo or redo. The caller supplies that context; the source `UserUndoable` alone is insufficient. Without proof of ownership, apply the ordinary rows. Without sufficient card payloads to prove flag-only, rebuild conservatively.

A **flag-only** entry is an updated card with both images present and no changed columns outside `flags`, `mod` and `usn`. Added and removed cards never qualify. Stored memory changes do not qualify: `memory_state_only` can change difficulty or retrievability ordering and therefore still rebuilds. In-core observers must not submit another operation and wait for it.

| Change set | Result |
|---|---|
| `whole_collection` is true, its source is `SyncIncoming`, `CloudIncoming` or `CloudRekey` (`engine/change-stream#S1.2`), or the collection generation changed (including a collection-replacement event) | rebuild |
| the session identifies this commit as its own answer, or its own undo or redo of that answer, in the same generation | none: the session updates its queue itself (U43) |
| any `cards` entry that is not proven flag-only | rebuild |
| a `decks` or `deck_configs` entry | rebuild |
| a `config_keys` entry for a key the build reads: `curDeck`, `collapseTime`, `applyAllParentLimits`, `newCardsIgnoreReviewLimit`, `fsrs`, `fsrsShortTermWithStepsEnabled`, `loadBalancerEnabled`, `rollover`, `creationOffset`, `localOffset` | rebuild |
| a `notes` or `notetypes` entry | patch |
| anything else (tags, review log, media, other config keys, the `col` row) | none |

- A new day also rebuilds: the day clock's rollover (`sched/day-clock#D8`) discards the queue.
- The first row's sources are the remote-apply ops (`engine/op-model#O1.1`): `SyncIncoming`, a manual Anki transfer's merge (U107, frozen); `CloudIncoming`, a CloudKit apply of another device's changes (`sync/cloudkit-records#K13.2`); and `CloudRekey`, a CloudKit re-key that gives a colliding local card, note or deck a new id (`sync/cloudkit-records#K14.2`), so a queue that holds the old id must rebuild.
- Anki rebuilds after every operation that changes cards (except setting a flag), decks or presets, after selecting a deck, saving preferences, saving deck options or toggling the load balancer, after any raw database write, and when the day changes; an answer updates its queue in place. The table gives the same results for the operations Meguri has.
- `whole_collection` also covers external raw changes reported with source `Maintenance`; it wins even when no row lists are available. A full replacement is recognized by its generation/event, not an invented full-sync source.
- Examples: setting a flag → none; saving a preset → rebuild; editing note fields → patch; adding a note and its cards → rebuild. A registry-only tag change → none. A tag rename that changes notes → patch, unless an earlier rebuild rule applies.

## B11. Differences from Anki

| Situation | Anki | Meguri |
|---|---|---|
| a filtered deck below the studied deck (B2.2) | gathers its cards | leaves them out (deviation `queue-nested-filtered-deck`) |
| a deck below the studied deck whose parent deck is missing (`sched/limits#L3`) | attaches it under a deck one level up on the path from its nearest existing ancestor (`sched/limits#L3`) and gathers its cards under that deck's limits; when that path has no such deck, leaves it out of the limits tree, and the build fails once that deck is checked against a limit (its intraday learning cards are still gathered) | the deck, its descendants and all their cards are left out |

## B12. Scenarios

| Scenario | Covers |
|---|---|
| `sched/queue/gather/card-states` | B2, B2.1, B4 (suspended, buried, due tomorrow, flagged, subdeck, sibling deck, a top-level filtered deck) |
| `sched/queue/gather/nested-filtered` | B2.2 (deviation) |
| `sched/queue/gather/preview-in-normal-deck` | B4 phase 1, B6, B9 (queue 4 cards in a normal deck) |
| `sched/queue/gather/missing-preset` | B3 |
| `sched/queue/bury/*` | B5 |
| `sched/queue/learn-ahead/window`, `ready-in` | B6, B8, B9 |
| `sched/queue/limits/*` | B4 with `sched/limits` |
| `sched/queue/order/*`, `sched/queue/mix/*` | B4, B7 with `sched/orders` |
| `sched/queue/empty-collection` (held) | B8, B9: an empty collection presents no card, every count 0 |
| `sched/counts/deck-page-difference` | B9 against the deck list |

All of them observe the queue right after a rebuild (`specs/conformance/queue-ops.md` Q2.3). B10 and B11's damaged tree have no scenario; Meguri's required tests (`core/crates/meguri-sched/tests/queue.rs`) must check B10's table row by row and every example.

## B13. Plan alignment

The U41 plan now states these pinned rules explicitly:

- Review orders 7 and 11 use retrievability, with NULL memory values first and last respectively. The relative-overdueness fallback belongs only to order 12. U82a's current extractor contracts define those values exactly.
- New gather orders 3, 4 and 5 use a day-derived gather salt. New sort orders 2, 3 and 4 use today. Review random order and final review ties use card id and mtime, so a day change alone does not reorder the existing due cards.
- Filtered membership excludes a card from its home queue. Queue 4 stored in a normal deck still participates as intraday learning.
- Deck-page differences have one signed value for each count. The Learn difference may be negative.
- The 60-card mixed case is still required. The saved 25-card and 34-card fixtures establish their recorded order behavior; they do not replace the larger case. `sched/queue/order/mixed-sixty` provides original inputs for all 43 required combinations/orders and still needs reference output. Both lanes remain required.

The other test scenarios and their goldens (under `sched/queue/` unless named): AE21 `limits/ae21`; `applyAllParentLimits` `limits/parent-limits`; interday learning first `limits/interday-first`; intraday learning uncapped `limits/intraday-uncapped`; the 7-and-3 mix and Show after or before `mix/new-review`, `mix/interday-review`; burying new siblings `bury/new-siblings`; the learn-ahead window `learn-ahead/window`; the three-level tree at 10:00, 03:59:59 and 04:00:01 `sched/counts/limits-tree`; sibling order and the Default deck `sched/counts/tree-order`, `default-only`, `default-shown`; `DeckStudyState` `learn-ahead/ready-in`, `sched/counts/finished`; filtered-deck cards `gather/card-states`, `gather/nested-filtered`. `rebuild_rules` (B10) and the benches have no golden.
