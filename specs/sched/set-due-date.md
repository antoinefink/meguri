# Set due date

Spec: sched/set-due-date
Requirements: R682, R708, R709, R710, R711, R712, R719, R768, AE26
Anki: 26.09.3
Scenarios: conformance/scenarios/sched/ops/set-due/

Set Due Date makes the chosen cards review cards due on a day the user types, relative to today. This file gives the text the user may type, the row each card gets, the review-log row it writes and the remembered text. The scenario op is `set_due` (`specs/conformance/sched-ops.md` J6).

Terms, as in `sched/answering`: `now_ms` is the operation's instant in epoch milliseconds and `now_s` = `now_ms` / 1000 rounded down; `today` and `next_day_at` are the day clock's day number and next day boundary at `now_s` (`sched/day-clock`; the operation reads them as answering does, without the unbury check). Card types, queues, `due`, the home deck and the queue-from-type rule are `sched/card-data` K1 to K3; the home preset and its read bounds are `sched/presets` V1 and V3.

## S1. Inputs

- The target cards: a list of distinct card ids (`sched/manual-ops#M1`). Ids that name no card are skipped.
- The days text the user typed (S2).
- The context: none, the review screen or the browser (S6).
- Meguri always runs Set Due Date with FSRS on (R702); S4.3 says what Anki does with FSRS off, for reading goldens only.

## S2. The days text (R708)

The text is accepted when it is, exactly and with nothing before or after it, one of:

| Form | Meaning |
|---|---|
| `N` | due in N days (0 is today) |
| `N-M` | each card due on its own day between N and M days from today, both included; the bounds may be given in either order |
| `N!`, `N-M!` | the same; the `!` asks SM-2 to reset the interval (S4.3) and changes nothing with FSRS on |

- `N` and `M` are one or more ASCII digits `0`-`9`, leading zeros allowed (`007` is 7), with a value from 0 to 4,294,967,295.
- Everything else is refused, and the operation writes nothing: the empty text, any space or other white space (also around a valid text), signs (`-1`, `+3`), a decimal point, more than one hyphen or `!`, a missing bound (`3-`, `-4`), digits other than ASCII (`٣`), and a number above 4,294,967,295.

| Text | Result |
|---|---|
| `0` | 0 days |
| `007` | 7 days |
| `5-2!` | 2 to 5 days, `!` |
| `70-50` | 50 to 70 days |
| `4294967295` | 4,294,967,295 days (S4.1 says how the due day wraps) |
| `abc`, `-1`, `3--4`, `1-2-3`, `3.5`, `3!!`, `""`, `" 3"` | refused |
| `4294967296` | refused |

- Meguri's parser reports the refusal as a typed error (S9) so that the dialog shows an inline error while the text is edited (R708). Its kinds: `Empty` (the empty text), `TooLarge` (a bound is all ASCII digits but above 4,294,967,295), `Malformed` (anything else).
- Anki's dialog treats an empty or all-white-space text as Cancel and runs nothing; any other text goes to the operation unchanged, white space included.

## S3. The day of each card

- `N`: every card gets `d` = N.
- `N-M`: each card gets its own `d`, drawn uniformly among the whole numbers from min(N, M) to max(N, M), independently of the other cards. Anki draws from an unseeded source, so its days cannot be reproduced; Meguri draws from the engine's random source (the randomness seam of R1875), in the order the cards are written (S7), so a seeded test is reproducible. Scenarios check a drawn day only as membership in the range, and the values that follow from it only through relations that hold for any draw (`specs/conformance/sched-ops.md` J10).

## S4. The card row (R710, R711, R712)

### S4.1 Columns

Every target card is written, also when its row would not otherwise change:

| Column | Value |
|---|---|
| `type`, `queue` | 2 and 2, whatever they were: new, learning, relearning, suspended, buried and preview cards all become active review cards |
| `due` | `today + d`, computed modulo 2^32 and stored as the signed 32-bit value of the result (so `4294967295` on day 0 stores -1; any realistic `d` simply adds) |
| `ivl` | S4.2 |
| `factor` | unchanged when it is not 0; when it is 0, the home preset's starting ease (`sched/presets` V2 `initial_ease`, read with V3) × 1000 in 32-bit float arithmetic, rounded to the nearest integer (2.5 gives 2500, 1.31 gives 1310) |
| `did`, `odid`, `odue` | when `odid` is not 0: `did` = `odid`, then `odid` and `odue` 0 (the card leaves its filtered deck); otherwise all three unchanged |
| `reps`, `lapses`, `left`, `flags`, `nid`, `ord` | unchanged; `left` keeps a learning card's step count although the card is now a review card |
| `data` | `pos` per `sched/card-data` K6 rule 2 (a card of type 0 saves its position, `odue` when `odid` is not 0, else `due`, read as unsigned 32-bit); every other key, the memory state `s` and `d`, `dr`, `decay`, `lrt` and `cd` included, unchanged; the text re-encoded (`model/card-data-json` C4) |
| `mod`, `usn` | `now_s`, -1 |

- The home preset is the preset of the home deck (`sched/presets` V1). Set Due Date reads it for every target card, whatever its `factor`: when the home deck does not exist the operation fails with `NotFound`, and when the home deck is a filtered deck (a damaged row with `odid` 0 inside a filtered deck) it fails with `HomeDeckFiltered`; either way nothing is written. When no preset has the home deck's `config_id`, the default preset (id 1) is used, and the built-in defaults (starting ease 2.5) when that is missing too. This differs from the answer path's fallback (`sched/presets` V1).
- Taking the card out of a filtered deck does not restore its `odue`: the new `due` replaces both.

### S4.2 The new interval with FSRS on (R711)

The first rule that applies:

1. The card's `queue` is 0, or its `ivl` is 0: `ivl` = 0. (So a new card, a learning card that never graduated and a preview card keep 0, also when the operation is repeated.)
2. The card data holds `lrt` (`sched/card-data` K5): `ivl` = `e + d`, where `e` = (`next_day_at` − `lrt`) / 86,400 rounded down, and 0 when `lrt` is at or after `next_day_at`. The sum is taken modulo 2^32.
3. Otherwise `ivl` = the old `ivl` + (`today + d` − `D`), floored at 0 and capped at 4,294,967,295, where `D` is the day the card was due on: its stored due value is `odue` when `odid` is not 0, else `due`; a value above 1,000,000,000 is an epoch second (`sched/card-data` K2) and gives `D` = `today` + (value − `next_day_at`) / 86,400 with the division truncated toward zero; any other value is `D` itself.

- The memory state is not touched (R712): `s` and `d` stay as they were.
- The `!` suffix changes nothing with FSRS on.

| Card before (today = day T, `next_day_at` = B) | Text | `ivl` after |
|---|---|---|
| review, `ivl` 10, `lrt` = B − 5 × 86,400 − 64,800 (5 days and 18 hours before B) | `3` | 5 + 3 = 8 (AE26) |
| review, `ivl` 10, `lrt` as above | `3!` | 8 |
| new (`queue` 0) | `3` | 0 |
| learning, `ivl` 0, intraday `due` | `3` | 0 |
| review, `ivl` 10, `due` T + 2, no `lrt` | `5` | 10 + (T + 5 − (T + 2)) = 13 |
| review, `ivl` 10, `due` T + 2, no `lrt` | `0` | 8 |
| suspended review, `ivl` 10, `due` T + 2, no `lrt` | `3` | 11 |
| relearning, `ivl` 4, `due` = now + 600 s (an epoch second before B), no `lrt` | `3` | `D` = T, so 4 + 3 = 7 |
| review in a filtered deck, `ivl` 6, `odue` T − 1, filtered `due` −100,000, no `lrt` | `2` | 6 + (T + 2 − (T − 1)) = 9 |
| review, `ivl` 2, `due` T + 30, no `lrt` | `0` | 2 − 30 floored: 0 |

### S4.3 Anki with FSRS off

Meguri never runs this branch; it is here for reading goldens recorded with `fsrs` off. With FSRS off Anki sets `ivl` to max(`d`, 1) when the text has `!` or the card's type is not review (2) or relearning (3), and otherwise to max(old `ivl`, 1). Everything else is as S4.1 and S5 say. Scenarios of this unit set `fsrs: true`.

## S5. The review-log row (R682)

Each target card gets one row, inserted right after its card row is computed:

| Column | Value |
|---|---|
| `id` | `now_ms`, or the table's largest id plus one when that is taken (`sched/revlog` E2): cards written in one operation get `now_ms`, `now_ms` + 1, … in the order of S7 |
| `cid` | the card |
| `usn` | -1 |
| `ease` | 0 (no button) |
| `ivl` | the new `ivl` (S4.2), capped at 2,147,483,647 |
| `lastIvl` | the `ivl` before the operation, capped at 2,147,483,647 |
| `factor` | with a memory state (the card data holds both `s` and `d`): `((d − 1) / 9 + 0.1) × 1000` in 32-bit float arithmetic, then truncated toward zero (not rounded, unlike an answer's row, `sched/answering` A5), saturating at 0 and 65,535; without a memory state: the card's `factor` after S4.1 (so a card that had none records the starting ease) |
| `time` | 0 |
| `type` | 4 (manual) |

- Example: `d` 5.048 gives (4.048 / 9 + 0.1) × 1000 = 549.78, written 549; an answer's row would write 550.
- A set-due row has `type` 4 and a `factor` above 0, so the memory-state history never takes it for a Reset marker (`sched/fsrs-items`, U46: a marker is `type` 4 with `factor` 0). Its `factor` is 0 only for a stored difficulty below about 0.101, far outside FSRS's 1 to 10; V3's bounds keep the starting ease above 0.

## S6. The remembered text (R709)

- When the operation runs from the review screen, the text exactly as typed (before parsing, so `5-2!` stays `5-2!`) is stored in the config key `setDueReviewer`; from the browser, in `setDueBrowser`. Without a context no key is written.
- The value is a JSON string, written after the cards in the same operation as any config write is (`storage/config-table` C4): only when it differs from the stored value, then with `mtime_secs` = `now_s` and `usn` -1. Undoing the operation restores the key (removing it when it was missing).
- The dialog is prefilled with the key's value, or its default when the key is missing or not a string: `"1"` for `setDueReviewer`, `"0"` for `setDueBrowser` (`sched/config-keys` G2).
- A refused text (S2) writes nothing, the key included.

## S7. The operation

- One undoable operation named "Set Due Date" (`specs/conformance/undo-ops.md` V4 `set_due_date`; R712, R719), in one transaction: S2's check first, then for each target card in ascending card id order (whatever order the caller listed them in) its row (S4) and its review-log row (S5), then the remembered text (S6). `col.mod` becomes `now_ms`.
- A list whose ids name no card changes nothing and records no step (`engine/undo` A2).
- Undo restores every card row and the config row exactly as they were (`mod` and `usn` included), deletes the review-log rows and restores `col.mod` (`sched/revlog` E4, `engine/undo` A7). Redo inserts the rows again under their ids.
- A card in a filtered deck is taken out of it (R710); the filtered deck itself is not changed.
- No note, tag or deck row is written.

## S8. Examples (oracle, 26.09.3; `specs/conformance/sched-ops.md` J11)

FSRS on, `today` 0, `next_day_at` B, text `3`, one operation on five cards:

| Card before | Card after | Review-log row (`ease`, `ivl`, `lastIvl`, `factor`, `time`, `type`) |
|---|---|---|
| review, `ivl` 10, `factor` 2500, `due` 5, data `{"s":4.5,"d":5.048,"lrt":B − 496,800}` | type 2, queue 2, `due` 3, `ivl` 8, `factor` 2500, data unchanged | 0, 8, 10, 549, 0, 4 |
| new at position 2, data `{}` | type 2, queue 2, `due` 3, `ivl` 0, `factor` 2500, data `{"pos":2}` | 0, 0, 0, 2500, 0, 4 |
| suspended review, `ivl` 10, `due` 2, `factor` 2300, no data | type 2, queue 2, `due` 3, `ivl` 11, `factor` 2300 | 0, 11, 10, 2300, 0, 4 |
| relearning, queue 1, `ivl` 4, `factor` 0, `left` 1, `due` now + 600 | type 2, queue 2, `due` 3, `ivl` 7, `factor` 2500, `left` 1 | 0, 7, 4, 2500, 0, 4 |
| learning, queue 1, `ivl` 0, `left` 2 | type 2, queue 2, `due` 3, `ivl` 0, `factor` 2500, `left` 2 | 0, 0, 0, 2500, 0, 4 |

## S9. Meguri's operation (engine, R768)

- **Parser.** Parse the supplied days text without accessing collection state. Return the bounds and whether interval reset was requested, or one of the S2 error categories. The app uses this result to explain invalid input.
- **Op.** Source `UserUndoable` (`engine/op-model` O1), step name "Set Due Date", kind `Other`. Inputs: distinct card ids, the days text (parsed again inside the op), the context (none, reviewer, browser) and an optional review-session id. A list holding an id twice is refused with `InvalidArgument` before anything is written (Anki fails such a list with a database error; no Meguri caller sends one).
- **Errors.** `InvalidDays(kind)` (S2), `NotFound` and `HomeDeckFiltered` (S4.1), `InvalidArgument`. Each leaves the collection unchanged.
- **Result.** The change set (`engine/change-stream`) and, with a session id, the session's head diff (`sched/review-session` C13; R1812). The step records the session (`engine/undo` M1).

## S10. Scenarios

| Scenario | Covers |
|---|---|
| `sched/ops/set-due/ae26` | AE26: S4.2 rule 2, the memory state kept, the review-log row (S5) with a truncated factor, undo and redo, the step name |
| `sched/ops/set-due/parse` | S2: accepted and refused texts, nothing written on a refusal, `5-2!` as a range, leading zeros, the largest number and S4.1's wrap at day 0 |
| `sched/ops/set-due/card-kinds` | S4: new, buried new, learning, relearning, suspended, filtered and review cards, S4.2 rules 1 and 3, the starting ease of the home preset, `pos`, `left` kept, ids in card id order whatever the listed order (S7) |
| `sched/ops/set-due/context` | S6: both keys, the text stored as typed, no key without a context, the key restored by undo |
| `sched/ops/set-due/range` | S3: drawn days in the range, `ivl` and the review-log row consistent with the drawn day |
