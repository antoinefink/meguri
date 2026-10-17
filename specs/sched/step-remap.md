# Remapping remaining steps

Spec: sched/step-remap
Requirements: R644, R645
Anki: 26.09.3
Scenarios: conformance/scenarios/sched/options/

When the steps a learning or relearning card is going through change, because its preset's steps were edited or because the card moved to a deck with another preset, the card's count of remaining steps is translated to the new list. This file gives the translation (R2), when the deck options save applies it (R3, U49), and the preset-change hook that Change Deck calls (R4, R645; U27, U87). The preset fields are `sched/presets#V2` and `#V5`; the card columns `sched/card-data#K1` and `#K4`; how a card moves through its steps is U45's (`sched/fsrs-transitions`).

## R1. Terms

- A **learning card** has `type` 1 and a **relearning card** `type` 3, whatever their `queue`: intraday (1) or interday (3) learning, suspended (-1) and buried (-2, -3) cards all count.
- A card's **remaining count** `r` is its `left` modulo 1000 (`sched/card-data#K4`: older clients stored a second number in the thousands).
- The **step list** of a card under a preset is the preset's `learn_steps` for a learning card and its `relearn_steps` for a relearning card: minutes as `f32`, in stored order, used as stored (`sched/presets#V5`: no sorting, no deduplication, possibly empty).
- A card whose remaining count is `r` under a list of `n` steps is on step `n − r + 1` of `n` (1-based) while `1 ≤ r ≤ n`: with 3 steps, `r` = 3 is the first step and `r` = 1 the last. The step before the current one is the **last passed step**.

## R2. The translation

Input: the remaining count `r`, the old list `O` (length `n`) and the new list `N` (length `m`). Output: a new count, or "unchanged".

1. When `O` is empty, the count is unchanged.
2. When `r ≥ n`, the card has passed no step of `O` (or holds a count no list explains): the candidate is `m`, every new step.
3. Otherwise the last passed step is `O[n − r − 1]` (0-based), with delay `d`. Find the last element of `N` (the one with the highest index) whose value is at most `d`, comparing `f32` values. The candidate is the number of elements of `N` after it. When no element of `N` is at most `d`, the candidate is `m`.
4. The new count is the larger of the candidate and 1: a learning or relearning card keeps at least one step, also when `N` is empty.
5. When the new count equals `r`, the result is "unchanged". Otherwise it is the new count.

In words: the card keeps the progress it made. It counts as having passed every new step up to the last one that is no longer than the step it last passed, and has the steps after that one still to do. R644's "closest equivalent delay" is this rule.

A card the translation changes gets the new count as its whole `left` (any thousands part is dropped) and is stamped (`mod` = now, `usn` -1). A card whose result is "unchanged" is not written: its `left`, thousands part included, its `mod` and its `usn` stay.

Examples (golden `sched/options/remap-steps`; steps in minutes):

| `O` | `N` | `left` before | Result | `left` after |
|---|---|---|---|---|
| 1 10 60 | 5 120 | 3 (step 1 of 3, nothing passed) | every new step: 2 | 2 |
| 1 10 60 | 5 120 | 2 (step 2, passed 1) | no new step ≤ 1, so every new step: 2, equal | 2, not written |
| 1 10 60 | 5 120 | 1 (step 3, passed 10) | last new step ≤ 10 is 5, one step after it: 1, equal | 1, not written |
| 1 10 60 | 5 120 | 0 (passed 60) | last new step ≤ 60 is 5, one step after it: 1 | 1 |
| 1 10 60 | 5 120 | 7 (more than the steps) | every new step: 2 | 2 |
| 1 10 60 | 5 120 | 1002 (count 2) | 2, equal | 1002, not written |
| 1 10 60 | 5 120 | 3003 (count 3) | 2 | 2 |
| 5 120 | none | 2 (nothing passed) | every new step: 0; at least 1 | 1 |
| 5 120 | none | 1 (passed 5) | no new step: 0, at least 1, equal | 1, not written |
| none | 1 10 | 1 | unchanged: the old list is empty | 1, not written |
| 10 | 5 30 120 | 1 (its only step) | every new step: 3 | 3 |
| 10 | 5 30 120 | 0 (passed 10) | last new step ≤ 10 is 5, two after it: 2 | 2 |
| 1 10 | 1 5 20 | 1 (passed 1) | last new step ≤ 1 is 1, two after it: 2 | 2 |
| 1 10 | 1 5 20 | 2 (step 1) | every new step: 3 | 3 |

The rule only compares values, so a list with repeated or decreasing values can change the count even when `O` equals `N`: with `O` = `N` = 1 1 10 and `r` 2 (passed the first 1), the last element ≤ 1 is the second one, one step follows, and the count becomes 1. R3 never applies the translation to equal lists; R4 does.

## R3. The deck options save (R644)

The deck options save (`sched/deck-options#O7`, step 5) translates, for every normal deck `D` of the collection:

- `P`: the preset `D` used before the save, as stored before any of the save's changes (a preset the save removes still counts as `P` for its decks); none when `D`'s `config_id` named no preset.
- `Q`: the preset `D` uses after the save, with the save's edits.
- When `P` is none, nothing is translated for `D`.
- When `P`'s `learn_steps` and `Q`'s differ (in length, or in any element compared as `f32`), every learning card of `D` gets R2 with `O` = `P`'s list and `N` = `Q`'s list. Likewise relearning cards with `relearn_steps`. Equal lists translate nothing.
- The cards of `D` are those whose `did` is `D`, and those whose `odid` is `D` (cards of `D` sitting in a filtered deck). New and review cards are never touched.

Steps change for `D` both when its preset's steps are edited and when `D` gets another preset (assigned by the save, a "Save to all subdecks", or because its preset was removed). Golden `sched/options/remap-steps`:

| Save | Effect |
|---|---|
| A: Lang's learning steps 1m 10m 1h → 5m 2h | the learning cards of Words, in its filtered deck, suspended and interday ones included, follow R2's first rows; Phrases (another preset), Words' relearning and review cards are untouched |
| B: Lang's relearning steps 10m → 5m 30m 2h | only Words' relearning cards: 1 → 3, 0 → 2 |
| C: Lang's learning steps → none | every learning card with a count other than 1 gets 1 |
| D: Lang's learning steps none → 1m 10m | nothing: the old list is empty |
| E: Phrases moves from Other (1m 10m) to Lang, whose steps become 1m 5m 20m | Words' cards translate from Lang's old list, Phrases' from Other's |

## R4. The preset-change hook (R645)

Change Deck (U27's set-deck operation, which U87, U140 and U142 call) is the only operation that moves cards between decks. For every card it moves to the normal deck `T`, before the move:

1. `O`: the card's step list under the preset of its home deck (its `odid` when that is not 0, otherwise its `did`). `N`: its step list under `T`'s preset. Both presets are resolved with the fallback of `decks/deck-presets#P3`: a `config_id` that names no preset gives the default preset (id 1), and a missing default preset the built-in defaults. A home deck that does not exist, or that is filtered, gives the built-in defaults.
2. The card gets R2 when it is a learning or relearning card. Unlike R3, the translation runs even when `O` equals `N`; for lists in strictly increasing order it then changes nothing.
3. The card then moves as Change Deck says, which leaves a filtered deck and removes `s`, `d`, `dr` and `decay` from the card data (`sched/memory-state#M7.2`), and the operation recomputes the memory states of the target deck's cards that have none (M7.2), in the same undoable step ("Change Deck").

- A card whose `did` already is `T` is not moved and not translated.
- The hook's contract for U27: given the card's `type` and `left` and the two presets, it returns the new `left` or "unchanged", applying R1 and R2; the caller writes the card once, with the move.
- Change Deck's scenarios (U87) cover the hook end to end, with the recompute; they need a scenario op for Change Deck, which does not exist yet. Until then Meguri's tests check the hook with R2's table and R5's cases.

## R5. Tests

| Scenario | Covers | Golden |
|---|---|---|
| `sched/options/remap-steps` | R2, R3 (learning, relearning, filtered, suspended, interday, review untouched, empty lists, a preset switch with an edit) | recorded |
| `sched/options/undo-save` | R3 inside a save that is undone and redone | recorded |

Meguri's own tests (`core/crates/meguri-sched/src/options/remap.rs` unit tests and `core/crates/meguri-engine/tests/deck_options.rs`) take their expected values from R2's table, the golden, and these hook cases (R4), each a card moved from deck A to deck B:

| A's steps | B's steps | Card | `left` after |
|---|---|---|---|
| learning 1 10 | learning 1 10 100 | learning, on its first step (`left` 2) | 3 |
| learning 1 10 | learning 1 10 100 | learning, on its last step (`left` 1, passed 1) | 2: the last element ≤ 1 is the first, two follow |
| relearning 10 | relearning 1 10 100 | relearning, `left` 1 | 3 |
| learning none | learning 1 10 | learning, `left` 1 | 1, unchanged |
| learning 1 1 10 | learning 1 1 10 | learning, `left` 2 | 1 (equal lists still translate) |
| A's `config_id` names no preset, default preset learning 1 10 | learning 1 10 100 | learning, `left` 2 | 3 |
| learning 1 10 | learning 1 10 100 | review card, `left` 0 | unchanged |
