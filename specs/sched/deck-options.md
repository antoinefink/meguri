# Deck options and scheduling preferences

Spec: sched/deck-options
Requirements: R636, R665, R686, R690, R730, R731, R732, R733, R734, R735, R736, R737, R738, R739, R740, R741, R742, R743, R744, R745, R746, R747, R748, R749, R750, R751, R752, R753, R760, R761
Anki: 26.09.3
Scenarios: conformance/scenarios/sched/options/

The core behind the deck options screen (U232) and the scheduling settings (U160): what the screen reads, the options it shows with their defaults, ranges and warnings, how a step text is read and written, how the screen's preset menu stages its changes, and the one undoable save that applies everything. Also the re-sorting of new cards when the insertion order changes, the deck-level limits a save writes, and the scheduling preferences op.

This file builds on:

- `sched/presets`: the option fields and their schema-11 keys (V2), the read bounds (V3), enums (V4), steps (V5), desired retention in force (V7), how a preset is written (V8, V9), the deck fields and limits in force (D1 to D3);
- `decks/deck-presets`: the default preset and built-in defaults (P2), the preset of a deck (P3), add, clone, rename, remove, assign and update (P4 to P10), and the preset menu's naming rule (P7);
- `sched/step-remap`: the step translation the save applies;
- `sched/memory-state`: the recompute (M4), which decks a save recomputes (M7.1), and the "N of M cards included" count (M5);
- `notetypes/schema-changes`: the schema stamp and the confirmation gate (G1, G4, G5);
- `sched/config-keys`: the collection keys, their readers and defaults (G1, G2);
- `sched/day-clock`: today, the rollover hour and when the day is computed afresh (D2, D6, D8, D13).

The snapshot's shape (O2) and the validation codes (O4) are Meguri's own, from the Product Contract; everything a save writes follows Anki 26.09.3, except where O12 says otherwise.

## O1. Terms

- **Target deck**: the normal deck whose options screen is open. A filtered deck has no options screen: reading or saving one is refused with `filtered_deck`; a deck that does not exist with `not_found`.
- **Snapshot**: what the screen reads when it opens (O2). **Draft**: the screen's editable copy of it (O6). **Save**: the op that applies a draft (O7).
- **Target preset**: the preset selected in the draft when it is saved; the target deck gets it.
- For a normal deck and a save: its **previous preset** is the preset its `config_id` named before the save (none when it named no preset); its **current preset** is the one it uses after the save.
- **Today** is the day clock's day number at the read or the save (`sched/day-clock`), and **today's date** the local calendar date of that day's start: the local date of `next_day_at` − 86,400 s.
- **Built-in defaults**: `decks/deck-presets#P2`. An option's **default** is its built-in default (O3); it is what "restore default" puts back (R734).
- Floating-point options are stored as `f32`. Two values are **equal** when their `f32` values are equal; step lists are equal when they have the same length and equal elements.

## O2. The snapshot (Meguri-defined shape)

A read that writes nothing (apart from the day clock's own query, `sched/day-clock#D5`). It holds:

1. **Presets**: every `deck_config` row, each with its id, name, use count and options. The use count is the number of normal decks whose `config_id` is the preset's id (`decks/deck-presets#P3`). Options are read with the read bounds (`sched/presets#V3`), so a value an old client stored as 0 or NaN appears as its built-in default (R753). Order: by name with the `unicase` comparison (`text/unicode-primitives#U2`), then by the name's bytes, then by id.
2. **The target deck**: its id, its name in human form, its stored `config_id`, and the **opening preset**: the preset `config_id` names, or the default preset (id 1) when it names none (then `preset_missing` is true).
3. **Deck values** (`sched/presets#D1`), each optional: This-deck new and review limits (`new_limit`, `review_limit`); Today-only new and review limits (`new_limit_today`, `review_limit_today`), each given as its stored `limit` with `active` = its stored `today` equals today; the deck's own desired retention (`sched/presets#V7`).
4. **Global options** (R735), each marked global: `newCardsIgnoreReviewLimit` and `applyAllParentLimits` (Daily Limits), `loadBalancerEnabled` (Advanced), read with `sched/config-keys#G1` (missing reads as false, false and true).
5. **Defaults, ranges and choices** of every option of O3, for the restore-default controls (R734) and the editor's bounds.
6. **`schema_modified`**: true when `scm` is greater than `ls` (`notetypes/schema-changes#G1`). Removing a preset then needs no confirmation (O6.5).
7. **The ignore-date count** (R690): for the opening preset and its stored `ignore_revlogs_before_date`, the result of `sched/memory-state#M5`: `included`, `total`, and the warning flag (M5: `total` > 0 and 10 × `included` < `total`). It is absent when the stored date is empty or `1970-01-01`, as Anki shows no count for those. The screen asks M5 again for each date the user enters, for the preset being edited. For a preset the draft adds (O6.3), M5's card set is its `param_search` when that is not empty (a clone copies it), otherwise empty: 0 of 0, since no deck uses it yet.

The screen holds no scheduling rule of its own (U232): ranges, defaults, warnings and the count all come from here and from O4.

## O3. The options the screen shows (R736, R739 to R751)

"Essentials" holds New cards/day, Maximum reviews/day, Desired retention and Don't play audio automatically; every other option sits under "Advanced" in Anki's sections (R736). Fields are `sched/presets#V2`; deck values `sched/presets#D1`; keys `sched/config-keys#G2`. Editor values are the values a user can enter; O4 checks them.

| Section | Option | Field or key | Default | Editor values | Warnings (O4) |
|---|---|---|---|---|---|
| Daily Limits | New cards/day | `new_per_day` | 20 | integer 0 to 9999 | |
| Daily Limits | Maximum reviews/day | `reviews_per_day` | 200 | integer 0 to 9999 | `reviews_below_ten_times_new` |
| Daily Limits | This deck: new, reviews | deck `new_limit`, `review_limit` | none | integer 0 to 9999, or none | (as above) |
| Daily Limits | Today only: new, reviews | deck `new_limit_today`, `review_limit_today` | none | integer 0 to 9999, or none | (as above) |
| Daily Limits | New cards ignore review limit (global) | `newCardsIgnoreReviewLimit` | false | boolean | |
| Daily Limits | Limits start from top (global) | `applyAllParentLimits` | false | boolean | |
| FSRS | Desired retention | `desired_retention` | 0.90 | 0.70 to 0.99 | `retention_low`, `retention_high` |
| FSRS | Desired retention, This deck | deck `desired_retention` | none | 0.70 to 0.99, or none | (as above) |
| New Cards | Learning steps | `learn_steps` | `1m 10m` | step text (O5) | `steps_one_day_or_more` |
| New Cards | Insertion order | `new_card_insert_order` | 0 Sequential | 0 Sequential, 1 Random | |
| Lapses | Relearning steps | `relearn_steps` | `10m` | step text (O5) | `steps_one_day_or_more` |
| Lapses | Minimum interval | `minimum_lapse_interval` | 1 | integer 1 to 9999 | |
| Lapses | Leech threshold | `leech_threshold` | 8 | integer 1 to 9999 | |
| Lapses | Leech action | `leech_action` | 1 Tag Only | 0 Suspend Card, 1 Tag Only | |
| Display Order | New card gather order | `new_card_gather_priority` | 0 | 0, 5, 1, 2, 3, 4 | |
| Display Order | New card sort order | `new_card_sort_order` | 0 | 0, 1, 2, 3, 4 | |
| Display Order | New/review order | `new_mix` | 0 | 0, 1, 2 | |
| Display Order | Interday learning/review order | `interday_learning_mix` | 0 | 0, 1, 2 | |
| Display Order | Review sort order | `review_order` | 0 | 0, 1, 2, 3, 4, 6, 5, 7, 11, 12, 8, 9, 10 | |
| Burying | Bury new siblings | `bury_new` | false | boolean | |
| Burying | Bury review siblings | `bury_reviews` | false | boolean | |
| Burying | Bury interday learning siblings | `bury_interday_learning` | false | boolean | |
| Audio | Don't play audio automatically | `disable_autoplay` | false | boolean | |
| Audio | Skip question when replaying answer | `skip_question_when_replaying_answer` | false | boolean | |
| Timers | Maximum answer seconds | `cap_answer_time_to_secs` | 60 | integer 1 to 7200 | `answer_seconds_high` |
| Timers | Show on-screen timer | `show_timer` | false | boolean | |
| Timers | Stop on-screen timer on answer | `stop_timer_on_answer` | false | boolean | |
| Auto Advance | Seconds to show question for | `seconds_to_show_question` | 0.0 (off) | 0 to 9999, step 0.1 | |
| Auto Advance | Seconds to show answer for | `seconds_to_show_answer` | 0.0 (off) | 0 to 9999, step 0.1 | |
| Auto Advance | Wait for audio | `wait_for_audio` | true | boolean | |
| Auto Advance | Question action | `question_action` | 0 Show Answer | 0 Show Answer, 1 Show Reminder | |
| Auto Advance | Answer action | `answer_action` | 0 Bury Card | 0 Bury Card, 1 Answer Again, 2 Answer Good, 3 Answer Hard, 4 Show Reminder | |
| Advanced | Maximum interval | `maximum_review_interval` | 36500 | integer 1 to 36500 | `max_interval_short` |
| Advanced | Historical retention | `historical_retention` | 0.90 | 0.70 to 0.97 | |
| Advanced | Ignore cards reviewed before | `ignore_revlogs_before_date` | empty | empty, or a date `YYYY-MM-DD` not after today's date | the count (O2 item 7) |
| Advanced | Load balancer (global) | `loadBalancerEnabled` | true | boolean | |

- Enum lists are in the order the menus show them; the values and their meanings are `sched/presets#V4`. Under FSRS, Review sort order labels 6 "ascending difficulty" and 5 "descending difficulty" (they are the stored ease orders), and 7 and 11 the retrievability orders.
- Desired retention is edited as a percentage (R742); the core takes any value in range. The Auto Advance times take tenths of a second.
- **Hidden options** (R752): every other field of `sched/presets#V2` (`initial_ease`, `easy_multiplier`, `hard_multiplier`, `lapse_multiplier`, `interval_multiplier`, `graduating_interval_good`, `graduating_interval_easy`, `easy_days_percentages`, the three FSRS parameter lists, `param_search`, `new_per_day_minimum`, `other` and unknown fields), and the keys `fsrs`, `fsrsHealthCheck`, `cardStateCustomizer` and `fsrsShortTermWithStepsEnabled`. The screen has no control for them and a save never writes them: they keep their stored values and bytes (`sched/presets#V8`, `#V9`; `sched/config-keys#G2`).

## O4. Validation (Meguri-defined codes)

A pure function of the draft, the snapshot it came from and today's date. It returns a list of issues `{field, code, severity, params}`; `severity` is `error`, `warning` or `strong_warning`. The editor shows them next to their fields; a save with any `error` is refused (O7.1). `field` names the option by its field or key name in O3 (`learn_steps`, `new_limit_today`, `applyAllParentLimits`), and for preset options also the preset (its id, or the draft's handle for a preset the draft adds).

### O4.1 Which values are checked

- Range and choice checks apply only to values that differ from the snapshot's value (O1's equality). A stored value outside the editor's range but inside the read bounds (for example Maximum answer seconds 8000, or a deck desired retention of 0.5 stored by another client) is shown as it is and saved unchanged; it becomes an error only when the user changes it to another out-of-range value. A preset the draft adds (O6.3) is checked whole.
- Warnings apply to the values the draft holds, whatever their origin.

### O4.2 Errors

| Code | Field | When | `params` |
|---|---|---|---|
| `out_of_range` | any numeric option or deck value | the value is outside O3's editor range, or is NaN or infinite | `{min, max}` |
| `not_whole` | integer options and deck limits | the value has a fraction | |
| `invalid_choice` | enum options | the value is not in O3's list | |
| `step_malformed` | `learn_steps`, `relearn_steps` | a token of the step text is not a step (O5.1) | `{index, token}` (0-based token index) |
| `step_zero` | the same | a token is a step of length zero (`0m`, `0`) | `{index, token}` |
| `step_negative` | the same | a token is `-` followed by a step (`-5`, `-1h`) | `{index, token}` |
| `step_too_long` | the same | a token's length is above 2,147,483,648 seconds (O5.1) | `{index, token}` |
| `invalid_date` | `ignore_revlogs_before_date` | the text is not empty and is not a date `sched/fsrs-items#I1.3` reads | |
| `date_in_future` | `ignore_revlogs_before_date` | the date is after today's date (R690) | `{today}` (the date text) |

Each step token gives at most one error, the first that applies in the order `step_malformed`, `step_negative`, `step_zero`, `step_too_long`; one issue is reported per bad token.

### O4.3 Warnings

| Code | Field | Severity | When |
|---|---|---|---|
| `steps_one_day_or_more` | `learn_steps`, `relearn_steps` | warning | any step is 1,440 minutes or longer (R743, R745) |
| `reviews_below_ten_times_new` | `reviews_per_day` | warning | the smaller of 9999 and 10 × `new` is greater than `reviews`, where `new` and `reviews` are the limits the editor shows: the Today-only value when the draft holds one, else the This-deck value when it holds one, else the preset's (R739); `params` `{expected}` = that smaller value |
| `retention_low` | `desired_retention` (preset, or deck) | warning; strong_warning below 0.70 | the desired retention in force for the target deck (the deck's own value when the draft holds one, else the selected preset's), rounded to 2 decimal places, is below 0.80 (R742) |
| `retention_high` | the same | warning; strong_warning above 0.97 | that rounded value is above 0.95 (R742) |
| `max_interval_short` | `maximum_review_interval` | warning; strong_warning below 50 | the value is below 180 (R751) |
| `answer_seconds_high` | `cap_answer_time_to_secs` | warning | the value is above 600 (R749) |

The ignore-date count's warning (R690) is part of the count (O2 item 7), not of this list.

## O5. Step text (R636)

Steps are stored as minutes (`f32`, `sched/presets#V5`); the editor shows and takes them as text.

### O5.1 Reading a step text

- The text is split at runs of white space (Unicode `White_Space`); leading and trailing white space is ignored. An empty or blank text is the empty list, which is valid (no steps).
- Each token must be one or more ASCII digits `0`–`9`, optionally followed by exactly one unit letter: `s` (1 second), `m` (60 s), `h` (3,600 s), `d` (86,400 s). No unit means minutes. Upper-case units, decimals (`1.5m`), signs, other letters (`5x`) and separators inside a token are malformed (O4.2).
- The token's length in seconds is its number times its unit (computed without overflow: a very long digit string is simply too long). Zero is `step_zero`; above 2,147,483,648 seconds (2^31, the longest step Anki stores) is `step_too_long`.
- The step is that length divided by 60, rounded to the nearest `f32`. Steps keep the text's order; repeated and decreasing steps are allowed.

| Text | Steps (minutes) |
|---|---|
| `1m 10m 1h 2h 1d` | 1, 10, 60, 120, 1440 |
| `1s 5s 90s 400m` | 1/60, 5/60, 1.5, 400 (each as `f32`) |
| `10` | 10 |
| `  1m   10m ` | 1, 10 |
| empty | none |
| `0m`, `5x`, `-5`, `1.5m`, `1M` | errors: `step_zero`, `step_malformed`, `step_negative`, `step_malformed`, `step_malformed` |

### O5.2 Writing a step text

Each step becomes one token and the tokens are joined by one space, in order:

1. `t` = the step's minutes (the stored `f32`, exactly) × 60, in `f64`.
2. The unit is the largest of `d`, `h`, `m` whose length in seconds divides `t` exactly, that is `|t − round(t / u) × u|` is below 2^−52; otherwise `s`. Only units not longer than `t` are tried.
3. The number is `round(t / u)` for that unit, halves rounded up.

| Steps (minutes, `f32`) | Text |
|---|---|
| 1, 10, 60, 120, 1440 | `1m 10m 1h 2h 1d` |
| 1/60, 5/60, 1.5, 400 | `1s 5s 90s 400m` |
| 0.16666667 (10 s) | `10s` |
| 2160 (1.5 days) | `36h` |
| 525600 (365 days) | `365d` |

- A stored step that is zero, negative, infinite or NaN (only a damaged or foreign preset holds one) is written as the token `invalid`.
- The draft keeps a preset's stored list until the user edits that field's text. An unedited field is never read back, so showing a list as text and saving it changes nothing even where the text would read back to other `f32` values.

## O6. The draft and the preset menu (R731 to R735)

The draft starts as a copy of the snapshot, with the opening preset selected. Nothing is written until the save (O7). The screen may hold one draft per target deck.

### O6.1 Selecting a preset (R731)

Selecting a preset makes it the target preset; the save assigns it to the target deck (or its subtree, O7). The picker shows each preset as its name and a use count: the stored count, minus 1 for the preset the deck's stored `config_id` names, plus 1 for the selected preset. The counts do not change for "Save to all subdecks".

### O6.2 Editing options

Edits change the draft's values for the selected preset. Values of other presets edited earlier in the same draft are kept and saved too. The deck values and the global options are edited directly.

The limit tabs (R740, Anki's rule, for U232): the editor opens on the most specific tab that is set, Today only when the snapshot's Today-only limit is active, else This deck when set, else Preset. Using a tab sets that tab's value and clears the more specific ones in the draft (choosing This deck clears the Today-only limit; choosing Preset clears both); less specific tabs keep their snapshot values. Opening the Today-only tab while its stored limit is not active offers the stored `limit` again. The same holds for Desired retention's Preset and This deck tabs. What the save writes for each state is O9.

### O6.3 Add and Clone (R732)

- Both prompt for a name; the menu applies `decks/deck-presets#P7` (trimming, empty does nothing, a duplicate gets the time appended).
- Add stages a new preset with the values `decks/deck-presets#P4` gives (the built-in defaults with the FSRS-6 default parameters). Clone stages a new preset with the selected preset's draft values, unsaved edits included, and everything `decks/deck-presets#P5` copies (`other`, unknown fields, the three parameter lists).
- The staged preset becomes selected, with a use count of 0.

### O6.4 Rename (R732)

Renames the selected preset in the draft, following `decks/deck-presets#P7` (no trimming; a text equal to the current name does nothing; a duplicate gets the time appended).

### O6.5 Remove (R733)

- The default preset (id 1) cannot be removed; the menu disables the item, and a save that tries is refused (O7.1).
- A staged preset is simply dropped. An existing preset is added to the draft's removals.
- Removing an existing preset forces the next sync to be one-way. The screen asks the user to confirm first when the snapshot's `schema_modified` is false and the draft has no earlier removal of an existing preset (`notetypes/schema-changes#G4`); the confirmation travels with the save (O7.1).
- After a removal the preset before it in O2's order becomes selected (the first preset when it was first).

### O6.6 Unsaved changes and defaults (R734)

- The draft has unsaved changes when, compared with the snapshot under O1's equality, a preset's value or name differs, a preset was staged or removed, another preset is selected, or a deck value or global option differs. Dismissing the screen then asks Discard or Keep editing.
- An option shows its restore-default control when its draft value differs from its default (O2 item 5). Restoring puts the default into the draft.

## O7. The save (R737)

One op, named "Deck Options" (undo id `deck_options`, `conformance/undo-ops#V4`), source user. Input: the target deck; the target preset (an existing preset or one the draft staged); the draft's presets with their values and names; the staged presets; the removals; whether to save to all subdecks; the five deck values; the three global options; and whether the user confirmed a schema change.

### O7.1 Refusals

The save writes nothing and records no undo step when:

- the target deck is filtered (`filtered_deck`) or missing (`not_found`), or a preset the draft edits, removes or selects no longer exists (`not_found`);
- the removals contain the default preset (`default_preset`), or the target preset (invalid argument);
- validation (O4) gives an error: the result carries the issues;
- the removals contain an existing preset, `scm` ≤ `ls`, and the save is not confirmed: the result is `NeedsSchemaConfirmation` with the reason `preset_removed` (`notetypes/schema-changes#G4`).

Validation runs before anything is written, then the gate; the effects below run in one transaction.

### O7.2 Effects, in order

1. **Removals.** Each removed preset's row is deleted, without a grave (`decks/deck-presets#P8`); `scm` and `col.mod` become now (`notetypes/schema-changes#G5`). Decks that used it are reassigned in step 3.
2. **Presets.** Written in this order: the draft's other staged or changed presets in O2's order, then the target preset last. A staged preset is added (a new id per `storage/ids-usn-mtime#I2`, allocated in that order, and stamped). An existing preset is updated field by field from the preset as read (`sched/presets#V8`, `decks/deck-presets#P10`): only the fields of O3 and the name can differ, every hidden option keeps its stored value, and a preset equal to its stored record is not written.
3. **Assignment and deck values.** The deck set is: the target deck; with "Save to all subdecks", every normal descendant (`decks/decks#D3`), filtered decks left alone; and every normal deck whose `config_id` names no preset after steps 1 and 2 (those of removed presets, and any deck whose preset was already missing). Each deck of the set gets the target preset's id as `config_id`, and the save's five deck values written as O9 says. A deck whose record ends equal to the stored one is not written; the others are stamped (`decks/decks#D13`). Every deck of the set gets the same values, the target deck's: a descendant's own limits and desired retention, and those of a deck whose preset was removed or missing, are replaced (Anki's save; R732 and R733 name only the preset).
4. **Insertion order.** Every normal deck whose previous and current presets have different insertion orders gets its new cards re-sorted (O8). A deck with no previous preset counts as Sequential before.
5. **Steps.** The remaining steps of learning and relearning cards are translated (`sched/step-remap#R3`).
6. **Memory states.** Recompute the decks selected by `sched/memory-state#M7.1` using M4 inside this operation. Do not change the FSRS setting. Changes to parameter lists or effective desired retention trigger this work; a missing previous preset also qualifies (`sched/options/missing-preset`). Skip presets that cannot construct a next-state engine (M4.3).
7. **Global options.** Each of `newCardsIgnoreReviewLimit`, `applyAllParentLimits` and `loadBalancerEnabled` is written when the draft's value differs from the stored value (a missing key reads as its default and is not added when the value stays the default; `sched/config-keys#G1`).

- No other key is written: not `fsrs`, `fsrsHealthCheck`, `cardStateCustomizer`, nor `nextPos` (O8).
- `col.mod` moves once when anything changed (`storage/ids-usn-mtime#S5`). A save that changes nothing writes nothing and records no step.
- The change set carries every row written, the recomputed cards marked as memory-state changes (U46), so the study queue is rebuilt (U43) and counts refresh.
- Undo restores every row the save changed, removed presets and `scm` included (`notetypes/schema-changes#G5`); redo applies them again.

## O8. Re-sorting new cards (R744)

For one normal deck `D` and the order in force after the save:

1. The cards: every card of `type` 0 (new) whose `did` is `D` or whose `odid` is `D`, whatever its `queue` (suspended and buried new cards included).
2. Their notes, each once, in this order: Sequential, by note id ascending (the order the notes were added); Random, a random permutation drawn from the core's random source (the randomness seam, R1875).
3. The first note gets position 1, the next 2, and so on; every card of `D` of that note gets its note's position, also when the note has cards elsewhere. A card in a filtered deck gets it in `odue`, any other card in `due`.
4. Cards outside `D` are not touched. Positions start at 1 for each deck, whatever positions other decks hold, and `nextPos` (`sched/config-keys#G2`) does not change.
5. A card whose position is already the new one is not written; the others are stamped.

New cards added later get their positions from card generation, which reads the insertion order of their deck's preset (U27, `sched/card-data#K7`).

Because a random order cannot be reproduced, scenarios check its result as a set: each deck's positions are 1 to the number of its notes with new cards, siblings share one, and other decks are untouched (golden `sched/options/insertion-order-switch`).

## O9. Deck values written by a save (R740, R742)

For each deck of O7.2 step 3, with the save's value for each field:

| Field | Value given | Value none |
|---|---|---|
| `new_limit`, `review_limit` (This deck) | stored | removed |
| `desired_retention` (This deck) | stored | removed |
| `new_limit_today`, `review_limit_today` (Today only) | `{limit: value, today: today}` | `sched/presets#D3`: removed when its stored `today` is 0 or after today; otherwise kept with `today` = the smaller of its `today` and today − 1 |

- On the collection's first day (today is 0) a cleared Today-only limit is therefore removed, not kept.
- What the screen passes (O6.2): a Today-only value only when that tab is the one in use; so a save from a screen opened while the stored Today-only limit is not active passes none, which removes a future one and keeps a past one as it is.
- The limits in force follow from the result (`sched/presets#D2`): after clearing a Today-only limit of 50, the stored `limit` stays 50 with a past day, and the This-deck limit, else the preset's, applies.

## O10. Scheduling preferences (R760, R761, R665)

A separate op, named "Preferences" (undo id `preferences`), source user, for the settings screen (U160) and the Load balancer toggle outside deck options. Its input is any of:

| Setting | Key (`sched/config-keys#G2`, `sched/day-clock#D2`) | Default | Values | Stored as |
|---|---|---|---|---|
| Next day starts at | `rollover` | 4 | integer hour 0 to 23 | that integer |
| Learn ahead limit | `collapseTime` | 20 minutes | integer minutes 0 to 999 | minutes × 60, an integer |
| Load balancer | `loadBalancerEnabled` | on | boolean | `true` or `false` |

- Values outside these ranges are refused (`out_of_range`), nothing written. The screen shows the effective values: the hour of `sched/day-clock#D2`, `collapseTime` / 60 rounded down, and the load balancer read with `sched/config-keys#G1`.
- A save first reads all three effective settings, then replaces the supplied fields. An omitted field retains its effective value; it does not skip that setting's save behavior. Preserve an omitted learn-ahead value in seconds exactly: for example, 333 seconds stays 333 seconds. Do not round it through the editor's minutes or apply editor bounds to it. Explicit minute input uses the conversion in the table. Validate the supplied fields before writing anything.
- Submit the resulting `collapseTime` and `loadBalancerEnabled` values through `storage/config-table#C4`, even when they equal their read defaults. An absent row is inserted; a stored equal value keeps its bytes and stamps. This also applies to an explicitly supplied default value. A read default and an equal stored row are distinct states.
- Save `rollover` when the resulting hour differs from its effective hour before the save. When the hour is unchanged, this save does not materialize a missing rollover row. This conditional rule follows the pinned preferences save; the day query's own writes remain D5's responsibility.
- An empty request is invalid before execution: require at least one supplied field (E4). Completing omitted fields applies only after a request passes that validation.
- No other preference key is written. All actual writes belong to one Preferences undo step; no changed row means no new step. Undo restores the preceding rows exactly, including absence for a key this save inserted; redo reapplies the saved rows. These rules are separate from O7.2 step 7: the deck options save preserves a missing global key when its default stays unchanged.
- A change of `rollover` makes the day clock compute afresh at once (`sched/day-clock#D6`, `#D8`, `#D13`): today and `next_day_at` follow the new hour, and when the day number changes the rollover op runs (`sched/day-clock#D9`: unbury when the day moved forward past `lastUnburied`, undo history cleared).
- The change set rebuilds the study queue and refreshes deck counts at once (R761); a learn-ahead change takes effect at the next queue build. Scenario `sched/options/prefs-rollover` (golden pending, O13): at 05:00 UTC on day 2, moving the hour from 4 to 6 makes today 1 again, so a review due on day 2 is no longer due; moving it back to 4 makes today 2, unburies a card buried on day 1 and writes `lastUnburied` 2.

## O11. Strings

The screen's labels, help texts and warning wording are Meguri's (R734, U232); this file defines codes, not text. Option and section names are Anki's English names as O3 lists them (R736).

## O12. Differences from Anki

- **Validation** (O4): Meguri refuses malformed, zero and negative step tokens, values outside the editor ranges and dates after today. Anki's screen drops tokens it cannot read, reads `5x` as 5 minutes and `-5` as 5 minutes, clamps spin boxes, and its save moves a future ignore date back to today.
- **Opening preset** (O2 item 2): when the deck's `config_id` names no preset, Meguri opens on the default preset; Anki's screen opens on the first preset by name.
- **Global keys** (O7.2 step 7): Anki's save writes `newCardsIgnoreReviewLimit`, `applyAllParentLimits`, `cardStateCustomizer` and `fsrsHealthCheck` every time, adding them when missing, and `fsrs` when it was switched. Meguri writes the first two and `loadBalancerEnabled` only when they change, and never the others (R763, R702).
- **Parameter lists**: Anki's screen copies an older FSRS list into `fsrs_params_6` for display, so its save stores it there. Meguri never writes a parameter list (R752, R1486).
- **Ignore date**: Anki's screen fills an empty ignore date with `1970-01-01` when it shows the FSRS section, and its save stores it. Meguri keeps the empty text (both mean no cutoff, `sched/fsrs-items#I1.3`).
- **Re-sorting** (O8 step 5): Anki stamps every new card of a re-sorted deck; Meguri only the cards whose position changes (R1472).
- **Historical retention** range 0.70 to 0.97, the read bounds (R753). Anki's screen accepts 0.5 to 1.0 and then reads values outside the bounds as 0.9.
- **Step warning** on any step of 1 day or more (R743). Anki looks at the last step only.
- **Step text** (O5.2): for a step of an exact multiple of 43,800 minutes that is not a multiple of 525,600, Anki shows a rounded number of days; Meguri shows exact hours.
- **Load balancer** toggle on the deck options screen and in the settings (R665); Anki's desktop shows none.

None of these shows in a scenario's golden; scenarios avoid them (O13).

## O13. Scenarios and tests

| Scenario | Covers | Golden |
|---|---|---|
| `sched/options/remap-steps` | O7.2 step 5 (`sched/step-remap#R3`) | recorded |
| `sched/options/undo-save` | O7 as one step: desired retention and steps changed, undone, redone | recorded |
| `sched/options/insertion-order-switch` | O8 through preset switches: Random as a set, Sequential exactly, other decks untouched, undo | recorded |
| `sched/options/missing-preset` | O7.2 step 3 for a deck whose preset is missing: assigned the saved preset and given the target deck's values, no remap | recorded |
| `sched/options/live-save` | a Meguri save read back by Anki: every preset and the deck decode, legacy JSON equal (T1, R762, R765) | recorded (live) |
| `sched/options/insertion-order-option` | O8 when the option changes on a preset used by two decks | awaits the driver (`conformance/deck-options-ops#E3`) |
| `sched/options/deck-limits` | O9: set, cleared on day 0, cleared later, a future Today-only limit, This deck | awaits the driver |
| `sched/options/subtree` | O7.2 step 3 with "Save to all subdecks" | awaits the driver |
| `sched/options/remove-preset` | O7.1, O7.2 steps 1 and 3: removal, reassignment with the target deck's values, `scm`, `default_preset`, undo | awaits the driver |
| `sched/options/globals` | O7.2 step 7 for the two limit keys | awaits the driver |
| `sched/options/prefs-rollover` | O10 | awaits the driver (`conformance/deck-options-ops#E4`) |

Meguri's own tests (no oracle): O2's snapshot on a collection with an out-of-range preset (V3 replacements shown), a missing preset (`preset_missing`), use counts and order; O4's every code and the "changed values only" rule; O5's two tables and round trips; O6's staging (add, clone with unsaved edits, rename, remove with and without confirmation, selection after removal, use counts); O7.1's refusals each writing nothing; O7's undo restoring presets, decks, cards and `scm`; O7.2 step 7 not adding missing keys; O10's ranges, its undo step and the day clock recomputing. Swift tests in CoreBridge cover the snapshot and validation API (U49 Verification).
