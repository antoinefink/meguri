# Leeches

Spec: sched/leeches
Requirements: R671, R672, R673, R721
Anki: 26.09.3
Scenarios: conformance/scenarios/sched/leech/

A leech is a card that keeps being forgotten. When an answer makes a card a leech, its note gets the tag `leech`, and the card is suspended if its preset says so. Whether a state is leeched is the next-state engine's rule (`sched/fsrs-transitions#T12`, U45); this file says what an answer does with it.

## L1. When an answer is a leech

- An answer is a leech answer when its chosen state is leeched: the `leeched` flag of a review state, or of the review part of a relearning state (`observations.md` O5).
- Only Again on a review card can be leeched (`sched/fsrs-transitions#T12`): the card's new lapse count `l` and its home preset's threshold `t` (`lapse.leechFails`, read with `sched/presets#V3`: a stored 0 reads as 8) make it a leech when `l ≥ t` and `l − t` is a multiple of `t / 2` rounded up. With the default 8: lapses 8, 12, 16, …
- Every leech answer acts again, not only the first (AE25: the 12th lapse is a leech answer like the 8th).
- **A stored 0 does not disable leeches.** R671 says a stored threshold of 0 turns leech detection off. Anki 26.09.3 never sees a 0: the read bounds (1 to 9999) turn a stored 0 into 8 before the threshold is used, so the 8th lapse is a leech answer. Meguri does the same (R1890; golden `sched/leech/zero-threshold`: a blob holding 0, a 7-lapse card answered Again gets `leech`, a 2-lapse card does not, and the stored bytes stay 0).

## L2. The tag (R672)

- In the answer's transaction, the tag text `leech` is added to the card's note as `tags/tags#T10` Add does for one note: the name is spelled and registered by `tags/tags#T3` (a registry row `Leech` or `LEECH` gives that spelling), the note is written only when it has no tag equal to the name (compared case-insensitively, `tags/tags#T5`), and a written note gets `mod` = `now_s` and `usn` −1 (`tags/tags#T14`).
- So a note that already carries the tag is not written again (AE25's 12th lapse), and a registry row is added only when no row matches.
- The answer's undo step covers the note and the registry row: undo puts the note's `tags`, `mod` and `usn` back and removes a row the answer registered (`tags/tags#T15`, R721). The tag is not a separate undo step.

## L3. The leech action (R672)

The home preset's leech action (`leech_action`, schema-11 `lapse.leechAction`; `sched/presets#V4`):

| Stored value | Action | The answered card |
|---|---|---|
| 1 (default) | Tag Only | written as `sched/answering#A3` says; it stays in its learning or review queue |
| 0 | Suspend Card | written as A3 says, then `queue` −1 |
| any other number in the preset's protobuf blob | as 0 (`sched/presets#V4`) | `queue` −1 |

A preset arriving as schema-11 JSON (sync, legacy packages) with a `leechAction` other than 0 or 1 is stored as 1 (`model/schema11-json`), so only a blob written by another means holds another number (golden `sched/leech/thresholds`: 5 written into the blob suspends).

- With Suspend Card, only `queue` changes from A3's values: a card going into relearning keeps `type` 3, its `left` and its `due` (a timestamp with learning fuzz for an intraday step), and one going straight to review keeps `type` 2, `ivl` and `due`. Unsuspending later restores the queue from `type` (U48).
- A suspended result is not added to the load balancer's cache (`sched/load-balancer#W8`).
- The tag is added with both actions.

## L4. What does not change

- Sibling burying (`sched/sibling-burying`), the deck counters (`sched/answering#A6`, by the queue before the answer) and the review-log row (A5) are the same as for any answer.
- An answer that is not a leech answer adds no tag and suspends nothing, even when the note already has `leech` or the card has more lapses than the threshold.

## L5. The answer's result and the notice (R673)

The answer reports two flags (`sched/answering#A13`):

- `leech`: the chosen state was leeched (L1);
- `suspended`: the answer set `queue` −1 through the leech action (L3).

The review screen (U175) then shows a brief notice that does not block the session. Its text is the first string below, followed by one space and the second string when `suspended` is set. Meguri takes these strings only from this table:

| Id | Text |
|---|---|
| `leech-notice` | Card was a leech. |
| `leech-notice-suspended` | It has been suspended. |

## L6. Examples (AE25)

Threshold 8, Tag Only, relearning steps 10m: a review card with 7 lapses answered Again goes to relearning (`type` 3, `queue` 1, `lapses` 8) and its note gets `leech`; at its 10th lapse nothing is added; at its 12th the answer is a leech answer again (the result says so) and the note, which has the tag, is not written. With Suspend Card the 8th lapse also writes `queue` −1, and the result has `suspended` set (goldens `sched/leech/ae25-tag-only`, `sched/leech/suspend`).

## L7. Scenarios

| Scenario | Covers |
|---|---|
| `sched/leech/ae25-tag-only` | AE25: lapses 8, 10 and 12 at threshold 8 with Tag Only; the tag added once; the note not written at the 12th |
| `sched/leech/suspend` | L3 with Suspend Card, with relearning steps (`type` 3, `queue` −1) and without (`type` 2, `queue` −1); undo removes the tag and its registry row and puts the queue back |
| `sched/leech/thresholds` | L1 with threshold 1 (two leech answers in a row, the second leaving the note unwritten), an unknown leech action in the blob (acts as Suspend Card), a registry row spelled `LEECH` |
| `sched/leech/zero-threshold` | L1: a stored threshold of 0 reads as 8 |
