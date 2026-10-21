# Presets and deck scheduling fields

Spec: sched/presets
Requirements: R629, R637, R647, R648, R653, R739, R740, R742, R743, R745, R746, R747, R748, R749, R750, R751, R753, R754, R755, R756, R757, R758, R762, R765
Anki: 26.09.3
Scenarios: conformance/scenarios/sched/data/

The scheduler's typed view of a deck preset (deck options group) and of the scheduling fields of a deck: which preset a card uses, every option with its default and the bounds Anki applies when it reads a preset, the FSRS parameter list and decay, the desired retention in force, how a preset is written back, the daily limits in force and the studied-today counters. The records are `model/protobuf-records` P6.5 and P6.6 (field names, numbers and encodings) and `model/schema11-json` S7, S8 and S10 (the legacy JSON form); preset records, the default preset and preset operations are `decks/deck-presets` (U28). The deck options screen (U49, U232) builds on this view; which options it shows is R752's.

## V1. The preset of a card

- A card's **home deck** is its `odid` when that is not 0 (the card sits in a filtered deck), otherwise its `did` (`sched/card-data#K2`). Its **home preset** is the preset whose id is the home deck's `config_id`.
- When the card is scheduled (its next states are computed, or an answer is applied) and no preset has that id, the card is scheduled with the **built-in defaults** (`decks/deck-presets#P2`), not with the default preset (id 1). Golden `sched/data/missing-preset`: with the default preset at desired retention 0.8, 21 parameters ending in 0.3 and a 10 s answer cap, a card of a deck whose `config_id` is 999 is answered with retention 0.9, decay 0.154 and the built-in 60 s cap (a 30 s answer is recorded as 30 000 ms), while a card of a deck using the default preset gets 0.8, 0.3 and 10 000 ms.
- That is Anki's answer path. Other paths treat a missing preset their own way, and their units say how: the deck options screen and the preset of a deck in `decks/deck-presets#P3` fall back to the default preset first; U41 states what the queue builder does.
- A home deck that is itself a filtered deck has no preset: scheduling such a card fails with an invalid-state error and changes nothing (only a damaged collection has one; Check Database repairs it, U128).

## V2. Options, defaults and read bounds

Every field of the deck config message (`model/protobuf-records#P6.6`) with its schema-11 key, its option, its built-in default (the value of a preset Meguri creates, `decks/deck-presets#P2`) and the bounds of V3. "Missing" is the value a field has when the stored blob does not contain it: the protobuf default, 0, 0.0, false, an empty list or an empty string. V3 applies after that, so a blob without `maximum_review_interval` reads as 36500, and one without `new_per_day` reads as 0.

| Field | Schema-11 key | Option (requirement) | Built-in default | Read bounds (V3) |
|---|---|---|---|---|
| `learn_steps` | `new.delays` | Learning steps, minutes (R743, R637) | `[1.0, 10.0]` | none (V5) |
| `relearn_steps` | `lapse.delays` | Relearning steps, minutes (R745, R637) | `[10.0]` | none (V5) |
| `fsrs_params_4` | `fsrsWeights` | FSRS-4.5 parameters (V6) | `[]` | none |
| `easy_days_percentages` | `easyDaysPercentages` | Easy Days (hidden, R752) | seven times 1.0 | none |
| `fsrs_params_5` | `fsrsParams5` | FSRS-5 parameters (V6) | `[]` | none |
| `fsrs_params_6` | `fsrsParams6` | FSRS-6 parameters (V6) | `[]` | none |
| `new_per_day` | `new.perDay` | New cards/day (R739) | 20 | 0 to 9999 |
| `reviews_per_day` | `rev.perDay` | Maximum reviews/day (R739) | 200 | 0 to 9999 |
| `initial_ease` | `new.initialFactor` (× 1000) | Starting ease (hidden; R587's graduation ease) | 2.5 | 1.31 to 5.0 |
| `easy_multiplier` | `rev.ease4` | Easy bonus (hidden, R653) | 1.3 | 1.0 to 5.0 |
| `hard_multiplier` | `rev.hardFactor` | Hard interval (hidden, R653) | 1.2 | 0.5 to 1.3 |
| `lapse_multiplier` | `lapse.mult` | New interval (hidden, R653) | 0.0 | 0.0 to 1.0 |
| `interval_multiplier` | `rev.ivlFct` | Interval modifier (hidden, R653) | 1.0 | 0.5 to 2.0 |
| `maximum_review_interval` | `rev.maxIvl` | Maximum interval, days (R751) | 36500 | 1 to 36500 |
| `minimum_lapse_interval` | `lapse.minInt` | Minimum interval, days (R745) | 1 | 1 to 36500 |
| `graduating_interval_good` | `new.ints[0]` | Graduating interval (hidden, R653) | 1 | 1 to 36500 |
| `graduating_interval_easy` | `new.ints[1]` | Easy interval (hidden, R653) | 4 | 1 to 36500 |
| `new_card_insert_order` | `new.order` (inverted) | Insertion order (R743, R744) | 0 sequential | enum (V4) |
| `leech_action` | `lapse.leechAction` | Leech action (R745) | 1 tag only | enum (V4) |
| `leech_threshold` | `lapse.leechFails` | Leech threshold (R745) | 8 | 1 to 9999 |
| `disable_autoplay` | `autoplay` (inverted) | Don't play audio automatically (R748, R757) | false | none |
| `cap_answer_time_to_secs` | `maxTaken` | Maximum answer seconds (R749) | 60 | 1 to 9999 |
| `show_timer` | `timer` | Show on-screen timer (R749, R754) | false | none |
| `skip_question_when_replaying_answer` | `replayq` (inverted) | Skip question when replaying answer (R748, R758) | false | none |
| `bury_new` | `new.bury` | Bury new siblings (R629, R747) | false | none |
| `bury_reviews` | `rev.bury` | Bury review siblings (R629, R747) | false | none |
| `bury_interday_learning` | `buryInterdayLearning` | Bury interday learning siblings (R629, R747) | false | none |
| `new_mix` | `newMix` | New/review order (R746) | 0 mix with reviews | enum (V4) |
| `interday_learning_mix` | `interdayLearningMix` | Interday learning/review order (R746) | 0 mix with reviews | enum (V4) |
| `new_card_sort_order` | `newSortOrder` | New card sort order (R746) | 0 card type, then order gathered | enum (V4) |
| `review_order` | `reviewOrder` | Review sort order (R746) | 0 due date, then random | enum (V4) |
| `new_card_gather_priority` | `newGatherPriority` | New card gather order (R746) | 0 deck | enum (V4) |
| `new_per_day_minimum` | `newPerDayMinimum` | none (unused, kept) | 0 | 0 to 9999 |
| `question_action` | `questionAction` | Question action (R750, R756) | 0 show answer | enum (V4) |
| `desired_retention` | `desiredRetention` | Desired retention (R742, V7) | 0.9 | 0.7 to 0.99 |
| `stop_timer_on_answer` | `stopTimerOnAnswer` | Stop on-screen timer on answer (R749, R754) | false | none |
| `historical_retention` | `sm2Retention` | Historical retention (R751; U46) | 0.9 | 0.7 to 0.97 |
| `seconds_to_show_question` | `secondsToShowQuestion` | Seconds to show question for, 0 = off (R750, R755) | 0.0 | none |
| `seconds_to_show_answer` | `secondsToShowAnswer` | Seconds to show answer for, 0 = off (R750, R755) | 0.0 | none |
| `answer_action` | `answerAction` | Answer action (R750, R756) | 0 bury card | enum (V4) |
| `wait_for_audio` | `waitForAudio` | Wait for audio (R750, R756) | true | none |
| `param_search` | `weightSearch` | optimizer search (U47) | `""` | none |
| `ignore_revlogs_before_date` | `ignoreRevlogsBeforeDate` | Ignore cards reviewed before, `YYYY-MM-DD` or empty (R751; U46, U47) | `""` | none |
| `other` | members beside the known keys | add-on and legacy keys (V9) | empty | none |

- The schema-11 conversions ("× 1000", "inverted", `ints`) are `model/schema11-json#S10`.
- The deck options editor accepts narrower ranges for some options (for example Maximum answer seconds 1 to 7200, desired retention 0.70 to 0.99, R742, R749). Those are U49's editor rules. V3's bounds are what a read applies to stored values, whoever stored them.
- The timebox limit, the learn-ahead limit, the next-day hour and the global daily-limit and load-balancer toggles are collection keys, not preset fields (`sched/config-keys`).

## V3. Read bounds (R753)

Some Anki versions let the deck options screen store 0 or NaN. So every read of a preset for use checks the bounded fields of V2:

- A bounded field whose value is below its lower bound or above its upper bound, or is NaN, reads as its built-in default. Bounds are inclusive. Float fields compare as `f32` with the `f32` nearest to the written bound (1.31 is 1.30999994…, 0.99 is 0.99000001…, 0.7 is 0.69999999…): a stored value equal to that `f32` is in range, the next `f32` beyond it is not. +∞ and -∞ are out of range.
- Fields without bounds are used as stored: an empty step list, a negative or NaN Auto Advance time, any parameter list.
- "Every read for use" means scheduling, queue building, counts, the deck options screen, the optimizer, and the schema-11 output that sync and legacy export send to other clients (`model/schema11-json#S13`). Only the schema upgrade reads presets without bounds (`storage/schema-upgrade`).
- A read never writes. The stored blob keeps its out-of-range values until the preset is written (V8).
- Only `f32` fields can hold NaN. The integer fields (`new_per_day`, `maximum_review_interval` and the others typed `uint32` in `model/protobuf-records#P6.6`) cannot, and JSON has no NaN. A legacy JSON preset whose `maxIvl` is not a `u32` (`1.5`, `-1`, `"x"`) makes its whole `rev` part invalid, so the part reads as its defaults, `maxIvl` 36500 included (`model/schema11-json#S10.2`), before V3 applies. A stored `new_per_day` of 0 is in range and reads as 0.

Examples (golden `sched/data/preset-bounds`, each preset written as a raw blob first):

| Stored | Reads as |
|---|---|
| `reviews_per_day` 10000; `new_per_day` 10000 | 200; 20 |
| `new_per_day` and `reviews_per_day` missing (0) | 0 and 0 (in range) |
| `initial_ease` 1.3; the `f32` just below 1.31 | 2.5; 2.5 |
| `initial_ease` 1.31 (its nearest `f32`) | 1.31 (schema-11 `initialFactor` 1310) |
| `easy_multiplier` 5.5; +∞ | 1.3; 1.3 |
| `hard_multiplier` NaN; the `f32` just above 1.3 | 1.2; 1.2 |
| `lapse_multiplier` 1.0; -1.0 | 1.0; 0.0 |
| `interval_multiplier` 0.4; -1.0 | 1.0; 1.0 |
| `maximum_review_interval` missing (0); 36501 | 36500; 36500 |
| `minimum_lapse_interval` 36501; missing | 1; 1 |
| `graduating_interval_good` 36501; `graduating_interval_easy` missing | 1; 4 |
| `leech_threshold` 10000; missing | 8; 8 |
| `cap_answer_time_to_secs` missing; 10000; 9999; 1 | 60; 60; 9999; 1 |
| `new_per_day_minimum` 10000 | 0 |
| `desired_retention` NaN; the `f32` just above 0.99; 0.99; 0.7 | 0.9; 0.9; 0.99; 0.7 |
| `historical_retention` 0.98; the `f32` just below 0.7; 0.97; 0.7 | 0.9; 0.9; 0.97; 0.7 |

An answer then uses the read values: the card data's `dr` is 0.9, 0.7, 0.99 and 0.9 for the presets Bounds, Edges, Edges2 and Outside, and a 100 s answer time is recorded as 60 000, 100 000, 1 000 and 60 000 ms (caps 60, 9999, 1 and 60 s).

## V4. Enum options

| Field | Values (`model/protobuf-records#P6.6`) |
|---|---|
| `new_card_insert_order` | 0 sequential (order added), 1 random |
| `leech_action` | 0 suspend card, 1 tag only |
| `new_mix`, `interday_learning_mix` | 0 mix with reviews, 1 show after reviews, 2 show before reviews |
| `new_card_sort_order` | 0 card type, then order gathered; 1 order gathered; 2 card type, then random; 3 random note, then card type; 4 random |
| `review_order` | 0 due date, then random; 1 due date, then deck; 2 deck, then due date; 3 ascending intervals; 4 descending intervals; 5 ascending ease; 6 descending ease; 7 ascending retrievability; 8 random; 9 order added; 10 reverse order added; 11 descending retrievability; 12 relative overdueness |
| `new_card_gather_priority` | 0 deck; 1 ascending position; 2 descending position; 3 random notes; 4 random cards; 5 deck, then random notes |
| `question_action` | 0 show answer, 1 show reminder |
| `answer_action` | 0 bury card, 1 answer Again, 2 answer Good, 3 answer Hard, 4 show reminder |

- A stored number outside its list is kept as stored (a later write keeps it) and acts as the value 0 of its enum: an unknown `leech_action` suspends, an unknown `review_order` sorts by due date then random. The built-in default of `leech_action` is 1, but a number Meguri does not know behaves as 0.
- Which `review_order` values the FSRS-only editor offers is U49's (R746, R752).

## V5. Steps (R637, R743, R745)

- `learn_steps` and `relearn_steps` are lists of minutes as `f32`, in order. An empty list means no steps (FSRS's short-term rule, U45).
- A preset Meguri creates has learning steps 1 and 10 minutes and one relearning step of 10 minutes (V2). A stored list is used as it is: no bounds, no sorting, no deduplication.
- How a step becomes seconds and how a card moves through steps is U45's (`sched/fsrs-transitions`).

## V6. FSRS parameters and decay (R647)

- **The list.** The preset's parameter list is the first non-empty one of `fsrs_params_6`, `fsrs_params_5` and `fsrs_params_4`, in that order, whatever its length; it is empty when all three are. The list goes to the FSRS crate unchanged (`sched/fsrs-transitions#T1`); a length other than 0, 17, 19 or 21 is refused there (U45, R704).
- **The decay.** 0.1542 when the list is empty (the FSRS-6 default); 0.5 when it has fewer than 21 values; otherwise its 21st value as stored, without the clamp the crate applies inside its model (`sched/fsrs-transitions#T2.1`). Answers write this value into the card data's `decay` (`sched/card-data#K5`), where the format rounds it to 3 places.
- **Short-term parameters.** U45 needs whether the list allows short-term scheduling: true for an empty list; for a list of 19 or more values, true exactly when its 18th and 19th values are both above 0; false otherwise (a 17-value list).
- Writes: only `fsrs_params_6` is ever written, by the optimizer (`decks/deck-presets#P10`, U47). The other lists are read-only and survive every write (R1486).
- **Later lists.** Anki 26.09.3 reads no list after `fsrs_params_6`. A later release may store one as an unknown field 7 or 8 of the blob (the numbers Anki reserves for it, `model/protobuf-records#P6.6`) or as an `fsrsParams7`-style member of `other`. The view keeps both as read (V8, V9) and gives read access to them, so that the next-state engine can refuse such a preset (`sched/fsrs-transitions#T13`, U45, R704). The list used and the decay above ignore them.

| Lists stored | List used | Decay written |
|---|---|---|
| `fsrs_params_6`: 21 values ending in 0.3 | that list | 0.3 |
| `fsrs_params_6`: 21 values ending in 0.05 | that list | 0.05 (the model computes with 0.1) |
| `fsrs_params_5`: 19 values | the 19 values | 0.5 |
| `fsrs_params_4`: 17 values | the 17 values | 0.5 |
| `fsrs_params_5` (19) and `fsrs_params_4` (17) | the 19 values | 0.5 |
| `fsrs_params_6` (21, ending in 0.3) and `fsrs_params_5` (19) | the 21 values | 0.3 |
| none | empty: the crate's defaults | 0.154 (0.1542 rounded) |

(Golden `sched/data/answer-data`; the card data of each case is in `sched/card-data#K5.2`. A first answer's stability shows which list was used: 2.3065 for the FSRS-6 values, 3.173 for the FSRS-5 values, 3.7145 for the FSRS-4.5 values.)

## V7. Desired retention in force (R648)

- The desired retention a card is scheduled with is its home deck's own value (`desired_retention` of the normal deck, "This deck") when the deck has one, otherwise its home preset's `desired_retention` read with V3.
- The deck's value has no read bounds: it is used as stored. The editor's range (0.70 to 0.99, R742) is U49's.
- A filtered deck has no value; a card in one uses its home deck's.
- Each answer writes the value in force into the card data's `dr` (`sched/card-data#K5`); the FSRS computation uses it too (`sched/fsrs-transitions#T2.2`).
- Stored forms: an optional `f32` in the deck kind blob (field 10, absent when unset); a whole percent in the schema-11 deck object (`desiredRetention`, `null` when unset, `model/schema11-json#S8`).

| Preset | Deck value | Retention in force | Deck kind bytes after the edit |
|---|---|---|---|
| 0.9 | 85 (schema-11) | 0.85 | `0a0c 08… 55 9a99593f` |
| 0.9 | 50 | 0.5 | `0a0c 08… 55 0000003f` |
| 0.9 | cleared (`null`) | 0.9 | `0a07 08…` (field 10 gone) |

(Golden `sched/data/desired-retention`; `08…` is the deck's `config_id`.)

## V8. Writing a preset

- An edit starts from the preset as read, V3 applied, changes the fields it edits and keeps everything else as read: the other options, the FSRS lists, `other`, and the unknown protobuf fields (`model/protobuf-records` P4, P5, P8; `decks/deck-presets#P10`).
- So a write stores V3's replacement values for every field that was out of bounds, as Anki does: after the preset Bounds of V3's examples has its desired retention set to 0.85, its stored fields are reviews/day 200, starting ease 2.5, easy bonus 1.3, hard interval 1.2, interval modifier 1.0, maximum interval 36500, minimum lapse interval 1, easy interval 4, leech threshold 8, answer cap 60, `new_per_day_minimum` 0, historical retention 0.9 and desired retention 0.85, and the fields that were in range keep their values (new cards/day 0, lapse multiplier 1.0, graduating interval 36500).
- When the edited preset equals the preset as read (V3 applied), nothing is written: the stored bytes, `mtime_secs` and `usn` stay, out-of-range values included. A preset that no edit touches is never written, so saving one preset leaves another's out-of-range bytes alone.
- Encoding follows `model/protobuf-records` P3 and P5: a changed preset is re-encoded in canonical form.

Golden `sched/data/preset-bounds`, last two reads: after a save of the preset Other, Bounds still holds its raw bytes; after Bounds' own save its blob is

```
0a080000803f00002041 120400002041 221c0000803f×7
50c801 5d00002040 656666a63f 6d9a99993f 750000803f 7d0000803f 8001949d02 880101
9001949d02 980104 a80101 b00108 c0013c ad029a99593f c5026666663f e00201
```

## V9. Options kept but not used (R653, R762)

Meguri schedules with FSRS only and has no editor for these, but every write keeps them as read (V8): `initial_ease` (used only for the ease a new card graduates with, R587), `easy_multiplier`, `hard_multiplier`, `lapse_multiplier`, `interval_multiplier`, `graduating_interval_good`, `graduating_interval_easy`, `easy_days_percentages`, `new_per_day_minimum`, `fsrs_params_4`, `fsrs_params_5`, `param_search`, `ignore_revlogs_before_date` (until U46 and U47 use them), `other` and unknown protobuf fields.

Golden `sched/data/preset-keep`: a preset with a starting ease of 2.3, intervals 2 and 5, easy bonus 1.4, interval modifier 0.9, hard interval 1.1, new interval 0.2, Easy Days `[1.0, 0.5, 1.0, 1.0, 1.0, 0.0, 1.0]`, an FSRS-5 list, historical retention 0.88, optimizer search `deck:K`, `newPerDayMinimum` 3 and an add-on key `x_addon_sched` in `other` gets its desired retention set to 0.85: the stored blob differs only in field 37 (`6666663f` to `9a99593f`), and the next answer writes `dr` 0.85 and `decay` 0.5.

## D1. Deck scheduling fields (R765)

| Field | Where | Schema-11 key | Meaning |
|---|---|---|---|
| `config_id` | normal deck | `conf` | the deck's preset (V1) |
| `review_limit`, `new_limit` | normal deck, optional | `reviewLimit`, `newLimit` | "This deck" limits |
| `review_limit_today`, `new_limit_today` | normal deck, optional day limit `{limit, today}` | `reviewLimitToday`, `newLimitToday` | "Today only" limits and the day they were set for |
| `desired_retention` | normal deck, optional | `desiredRetention` (whole percent) | V7 |
| `last_day_studied` | deck common | first element of the day arrays | the day the counters below belong to |
| `new_studied`, `review_studied` | deck common | `newToday`, `revToday` | D4 |
| `learning_studied` | deck common | `lrnToday` | stored legacy count; Meguri preserves it across answers and day changes (D4), never adds to it |
| `milliseconds_studied` | deck common | `timeToday` | D4 |

Every deck write keeps what it does not change (`decks/decks#D13`). Meguri writes these fields with Anki's encodings so that other clients read the same limits and counts (R765).

## D2. Limits in force (R740)

For each of the new and the review limit of a normal deck on day `today`:

1. its Today-only limit, when one is stored and its `today` equals `today`;
2. otherwise its This-deck limit, when one is stored;
3. otherwise the home preset's `new_per_day` or `reviews_per_day`, read with V3.

| Stored | Today | New limit in force |
|---|---|---|
| preset 20 | any | 20 |
| preset 20, This deck 5 | any | 5 |
| preset 20, This deck 5, Today only `{limit 50, today 12}` | 12 | 50 |
| the same | 13 | 5 |
| preset 20, Today only `{limit 50, today 12}` | 13 | 20 |

How limits combine with the studied counts, the parent decks and the global options is U41's (R605 to R609). A filtered deck has no limits.

## D3. Setting a deck's limits (R740; for U49)

- Setting a This-deck limit stores it; clearing it removes it.
- Setting a Today-only limit stores `{limit: <value>, today: <today>}`.
- Clearing a Today-only limit: when no day limit is stored, nothing changes. When the stored `today` is 0 or later than today, the day limit is removed. Otherwise it stays with its `limit` and its `today` set to the smaller of its `today` and today − 1, so it no longer applies and the editor can offer the last value again.
- Setting or clearing the deck's desired retention stores or removes the optional value (V7).

| Stored Today-only | Today | After clearing |
|---|---|---|
| `{50, 12}` | 12 | `{50, 11}` |
| `{50, 9}` | 12 | `{50, 9}` |
| `{50, 0}` | 12 | removed |
| `{50, 14}` (from an import) | 12 | removed |

The Today-only tab is the one the editor opens on when its `today` equals today (R740, U49).

## D4. Studied-today counters (R765)

- **Reading.** A deck's new and review counts studied today are `new_studied` and `review_studied` when `last_day_studied` equals today, and 0 and 0 otherwise. Reading never writes: a deck last studied yesterday reports 0 without any change to its row.
- **An answer** (U44) updates the deck the card is in when answered (`did`: the filtered deck while the card sits in one) and its ancestors, found by name from the parent upwards and stopping at the first one that does not exist (a damaged tree, `decks/decks#D3`), in this way for each:
  1. when `last_day_studied` is not today, `new_studied`, `review_studied` and `milliseconds_studied` become 0 and `last_day_studied` becomes today;
  2. `new_studied` + 1 when the card's queue before the answer was 0; `review_studied` + 1 when it was 2 or 3; nothing for 1 and 4;
  3. `milliseconds_studied` + the answer time after the home preset's cap (`cap_answer_time_to_secs` × 1000 ms, V3), which is also the time the review log records;
  4. the deck is stamped (`mtime_secs`, `usn` -1), whether or not a counter changed.
- `learning_studied` is preserved under `sched/fsrs-policy#K1` and `model/protobuf-records#P8` (R322). Anki resets it to 0 on the first answer of another day; Meguri does not. The shared `last_day_studied` still advances, so the day element of legacy `lrnToday` advances while its count stays.
- The counters are signed 32-bit values. Custom study and other clients can lower them below zero (U48); Meguri reads and keeps negative values.
- The global option "Limits start from top" (`applyAllParentLimits`) does not change which decks an answer updates.

Reference golden `sched/data/deck-counters` (decks Parent and Parent::Child, default answer cap 60 s; Child's `lrnToday` set to `[0, 5]` beforehand):

| Step | Parent | Parent::Child |
|---|---|---|
| day 0: in Child, new card (5 s), review card (7 s), intraday learning card (100 s), interday learning card (1 s) | `newToday` `[0,1]`, `revToday` `[0,2]`, `lrnToday` `[0,0]`, `timeToday` `[0,73000]` | the same, `lrnToday` `[0,5]` |
| day 1, before any answer | unchanged, `mtime_secs` unchanged | unchanged |
| day 1: new card in Child (2 s) | `[1,1]`, `[1,0]`, `[1,0]`, `[1,2000]` | the same |
| day 1: new card in Parent (3 s) | `newToday` `[1,2]`, `timeToday` `[1,5000]` | unchanged |

Meguri requires Child `lrnToday` `[1,5]` at both day-1 observations (steps 17 and 19), versus the reference `[1,0]`. Parent remains `[1,0]`; all other values in the table remain the reference values. These two scalar differences are declared in `conformance/deviations.yaml` under R322.

## D5. Filtered decks

A filtered deck has no preset, no limits and no desired retention. A card in one is scheduled with its home deck's preset (V1) and retention (V7), and its answers update the filtered deck's counters and its ancestors' (D4), not the home deck's.

## S1. Scenarios

| Scenario | Covers | Golden |
|---|---|---|
| `sched/data/answer-data` | V6, V7 (preset value) | recorded |
| `sched/data/desired-retention` | V7 (deck value set, out of the editor's range, cleared) | recorded |
| `sched/data/preset-bounds` | V3 (read, answer, legacy JSON), V8 (no write of an untouched preset, the write of an edited one) | recorded |
| `sched/data/preset-keep` | V8, V9 | recorded |
| `sched/data/missing-preset` | V1 | recorded |
| `sched/data/deck-counters` | D4 | recorded |

Meguri's own tests (`core/crates/meguri-sched/tests/preset_view.rs`) add V4's unknown enum values, V6's short-term rule and decay table, D2, D3 and D4's reading rule; they take expected values from these tables and the goldens.

## S2. Goldens as test data

The scenarios answer cards, so Meguri's executor runs them once answering exists (U44); until then they are `pending`. Several goldens already hold input and output pairs that the model's own tests can use without an executor:

| Golden | Input | Output |
|---|---|---|
| `sched/data/preset-bounds` | step 4: the raw blobs (`hex(config)`) of Bounds, Edges, Edges2 and Outside | step 5: their legacy JSON as read with V3 (compare as parsed JSON, `model/schema11-json#S14`); step 12: the answer times after the cap |
| `sched/data/preset-bounds` | step 4's Bounds blob, desired retention set to 0.85 | step 17: the blob written (V8) |
| `sched/data/preset-keep` | step 1: the blob | step 4: the blob after the desired retention is set to 0.85 (V8, V9), byte for byte |
| `sched/data/desired-retention` | the deck values 85, 50 and none | steps 1, 4 and 7: the deck kind blobs; step 9: `dr` in the card data |
| `sched/data/deck-counters` | the counters before (step 4) and the answers in the steps between | steps 10, 14, 17 and 19: the counters after (D4) |
