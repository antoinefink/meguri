# Schema-11 JSON form of note types, decks and presets

Spec: model/schema11-json
Requirements: R1464, R1465, R1484, R1485, R1486, R903, R904, R322; supports R1478, R1481
Anki: 26.09.3
Scenarios: conformance/scenarios/model/schema11/, conformance/scenarios/model/proto/

Before schema 15, Anki stored note types, decks and deck presets as JSON objects in the `col` row. That JSON form is still the one these records take on the sync wire, in the `col` row of a schema-11 collection, and in legacy packages. This file defines the conversion between that form and the schema-18 records (`model/protobuf-records`), in both directions. Number and string rules are `model/json-numbers`.

Users of the conversion: the schema upgrade (U22), the schema-11 writer (U33), normal sync (U107) and legacy package import and export (U120, U123).

## S1. Terms

- **Record**: the schema-18 form: row columns plus protobuf message(s).
- **Object**: the schema-11 JSON form.
- **Output** is record → object; **input** is object → record.
- **Refuse**: input fails for the whole object; nothing is converted. Each user decides what that means (a sync error, a failed upgrade, a failed import). Scenarios record it as the rejection kind `invalid_json` (`specs/conformance/model-ops.md` L2).
- Readers named in the tables (`model/json-numbers`): **strict** (the declared type, N2), **int-or-string** (N6), **bool-any** (bool from anything, N6), **lenient** (default on invalid, N6). Under "Missing", **refuse** means the key is required.

## S2. Rules shared by all objects

- Output writes the keys of the tables in table order, then the `other` members (S3). Keys marked "only when …" are left out otherwise; every other key is always written, `null` included.
- Output numbers: integer types per N3, `f32` per N4. Strings per N8. No whitespace.
- Input accepts the keys in any order. A key with a value its reader does not accept refuses the object (strict, int-or-string, bool-any) or takes its default (lenient). A missing key takes the "Missing" column.
- Input keeps every member it does not read as an `other` member (S3).
- Conversions marked "low 16 bits", "low 8 bits" or "as i32" keep the low bits of the value (wrapping), as a cast between integer types does. "f32 arithmetic" means every intermediate result is rounded to `f32`. "Truncate" means toward zero, with negative values and NaN giving 0 and values above the target range giving its maximum.

## S3. `other`

Five messages carry an `other` field (`model/protobuf-records` P7): notetype config, field config, template config, deck common and deck config. In the object, its members sit at the top level of the object (deck config: S10.5).

Output (record → object):

1. If `other` is empty, is not valid JSON, or is JSON but not an object, nothing is added. A number beyond the 64-bit float range anywhere in `other`, inside any member, makes it invalid JSON (`model/json-numbers` N10): `{"keep":1,"zz":1e400}` adds nothing, not even `keep` (oracle, `schema11/other-out-of-range`).
2. Members whose key is in the object's reserved list (below) are dropped.
3. Members whose key the object has already written are dropped as well. (Anki writes such a key twice; Meguri never writes duplicate keys. Keys left out by an "only when …" rule are not written, so an `other` member of that name does appear, as in Anki.)
4. The remaining members follow the known keys, in the order they are stored, each with its value as stored text (`model/json-numbers` N7).

Input (object → record):

1. Every member the object type does not read (S4 to S10) is kept, in input order, with its value as exact text.
2. If there is at least one, `other` becomes a compact JSON object of them (`{"a":1,"b":[2]}`); otherwise `other` is empty (zero bytes, so the protobuf field is left out).

Round trip: when `other` holds a compact JSON object with no reserved key and no key the object writes, record → object → record gives back the same `other` bytes. Anki writes the top-level members of `other` in no particular order and sorts the keys of nested objects; Meguri keeps the stored order (N7). Conformance compares parsed values, so this never shows.

Reserved lists (dropped on output even when present in `other`):

| Object | Reserved keys |
|---|---|
| notetype | `css`, `did`, `flds`, `id`, `latexPost`, `latexPre`, `latexsvg`, `mod`, `name`, `originalId`, `originalStockKind`, `req`, `sortf`, `tmpls`, `type`, `usn` |
| field | `collapsed`, `description`, `excludeFromSearch`, `font`, `id`, `name`, `ord`, `plainText`, `preventDeletion`, `rtl`, `size`, `sticky`, `tag` |
| template | `afmt`, `bafmt`, `bfont`, `bqfmt`, `bsize`, `did`, `id`, `name`, `ord`, `qfmt` |
| deck (normal and filtered) | `browserCollapsed`, `collapsed`, `conf`, `desc`, `desiredRetention`, `dyn`, `extendNew`, `extendRev`, `id`, `lrnToday`, `mod`, `name`, `newLimit`, `newLimitToday`, `newToday`, `revToday`, `reviewLimit`, `reviewLimitToday`, `timeToday`, `usn` |
| deck config, top level | `answerAction`, `autoplay`, `buryInterdayLearning`, `desiredRetention`, `dyn`, `easyDaysPercentages`, `fsrsParams5`, `fsrsParams6`, `fsrsWeights`, `id`, `ignoreRevlogsBeforeDate`, `interdayLearningMix`, `maxTaken`, `mod`, `name`, `newGatherPriority`, `newMix`, `newPerDayMinimum`, `newSortOrder`, `questionAction`, `replayq`, `reviewOrder`, `secondsToShowAnswer`, `secondsToShowQuestion`, `sm2Retention`, `stopTimerOnAnswer`, `timer`, `usn`, `waitForAudio`, `weightSearch` (and `new`, `rev`, `lapse`, S10.5) |
| deck config `new` | `bury`, `delays`, `initialFactor`, `ints`, `order`, `perDay` |
| deck config `rev` | `bury`, `ease4`, `hardFactor`, `ivlFct`, `maxIvl`, `perDay` |
| deck config `lapse` | `delays`, `leechAction`, `leechFails`, `minInt`, `mult` |

Older Anki versions kept some of these keys in `other` before they became protobuf fields; the protobuf field always wins.

Example (deck, oracle): `common.other` = `{"browser_collapsed":true,"extendNew":10,"x_addon":[1]}` and `extend_new` = 3. The object has `"browser_collapsed":true` and `"x_addon":[1]` at the top level after `dyn`, and `"extendNew":3` from the record; `extendNew` in `other` is reserved and dropped. Converting that object back gives `other` = `{"browser_collapsed":true,"x_addon":[1]}` and `extend_new` 3.

## S4. Notetype

Record: the `notetypes` row (`id`, `name`, `mtime_secs`, `usn`), its notetype config, and its `fields` and `templates` rows in `ord` order.

| Key | Output | Input reader | Missing | Input result |
|---|---|---|---|---|
| `id` | row `id` | int-or-string `i64` | refuse | row `id` |
| `name` | row `name` | strict string | refuse | row `name` |
| `type` | 1 when `kind` is 1, else 0 | strict: exactly 0 or 1 | refuse | `kind` |
| `mod` | `mtime_secs` | strict `i64` | refuse | `mtime_secs` |
| `usn` | `usn` | strict `i32` | refuse | `usn` |
| `sortf` | `sort_field_idx`, low 16 bits | strict `u16` | refuse | `sort_field_idx` |
| `did` | `target_deck_id_unused`, or `null` when 0 | lenient `i64` or `null` (invalid gives `null`) | refuse | the number, or 0 for `null` |
| `tmpls` | array of template objects (S6) | strict array of S6 objects | refuse | templates, `ord` = array index |
| `flds` | array of field objects (S5) | strict array of S5 objects | refuse | fields, `ord` = array index |
| `css` | `css` | lenient string | refuse | `css` (`""` when invalid) |
| `latexPre` | `latex_pre` | strict string | `""` | `latex_pre` |
| `latexPost` | `latex_post` | strict string | `""` | `latex_post` |
| `latexsvg` | `latex_svg` | lenient bool (not bool-any: `"1"` gives false) | false | `latex_svg` |
| `req` | `reqs` (S4.1) | lenient (S4.1) | `[]` | `reqs` |
| `originalStockKind` | `original_stock_kind`, only when not 0 | strict `i32` | 0 | `original_stock_kind` |
| `originalId` | `original_id`, only when set | strict `i64` or `null` | unset | `original_id` (unset for `null`) |

The `fields.ord` and `templates.ord` columns are the position in `flds` and `tmpls`. The `ord` value inside each field and template object is carried as that field's or template's previous ordinal, which the note type structure-change logic uses (notetypes area); it is not written to the columns.

Older objects carry keys such as `tags` and `vers`; they are ordinary `other` members.

### S4.1 `req`

Output: an array with one entry per card requirement, each a three-element array `[card_ord, kind, field_ords]`: `card_ord` low 16 bits; `kind` the string `"any"` for 1, `"all"` for 2 and `"none"` for any other value; `field_ords` an array of the field ordinals, each low 16 bits. Example: `[[0,"any",[0]]]`.

Input: an array of such three-element arrays, `card_ord` and each field ordinal a strict `u16`, `kind` exactly `"any"`, `"all"` or `"none"`. Anything else makes the whole `req` key invalid, so `reqs` is empty.

Anki recomputes `reqs` whenever it saves a note type through its editing paths; the conversion itself only carries them.

## S5. Field

Record: a `fields` row (`ord`, `name`) and its field config.

| Key | Output | Input reader | Missing | Input result |
|---|---|---|---|---|
| `name` | row `name` | strict string | refuse | row `name` |
| `ord` | row `ord`, low 16 bits | strict `u16` or `null` | `null` | previous ordinal (S4) |
| `sticky` | `sticky` | bool-any | refuse | `sticky` |
| `rtl` | `rtl` | bool-any | refuse | `rtl` |
| `font` | `font_name` | strict string | refuse | `font_name` |
| `size` | `font_size`, low 16 bits | strict `u16` | refuse | `font_size` |
| `description` | `description` | lenient string | `""` | `description` |
| `plainText` | `plain_text` | lenient bool | false | `plain_text` |
| `collapsed` | `collapsed` | lenient bool | false | `collapsed` |
| `excludeFromSearch` | `exclude_from_search` | lenient bool | false | `exclude_from_search` |
| `id` | `id`, or `null` when unset | lenient `i64` or `null` | `null` | `id` (unset for `null` or invalid, so `"12"` gives unset) |
| `tag` | `tag`, or `null` when unset | lenient `u32` or `null` | `null` | `tag` |
| `preventDeletion` | `prevent_deletion` | lenient bool | false | `prevent_deletion` |

## S6. Template

Record: a `templates` row (`ord`, `name`, `mtime_secs`, `usn`) and its template config. Anki never sets a template's `mtime_secs` or `usn` (both stay 0); a template converted from an object gets 0 for both.

| Key | Output | Input reader | Missing | Input result |
|---|---|---|---|---|
| `name` | row `name` | strict string | refuse | row `name` |
| `ord` | row `ord`, low 16 bits | strict `u16` or `null` | `null` | previous ordinal (S4) |
| `qfmt` | `q_format` | strict string | refuse | `q_format` |
| `afmt` | `a_format` | strict string | `""` | `a_format` |
| `bqfmt` | `q_format_browser` | strict string | `""` | `q_format_browser` |
| `bafmt` | `a_format_browser` | strict string | `""` | `a_format_browser` |
| `did` | `target_deck_id` when above 0, else `null` | lenient `i64` or `null` | `null` | the number, or 0 for `null` |
| `bfont` | `browser_font_name` | lenient string | `""` | `browser_font_name` |
| `bsize` | `browser_font_size`, low 8 bits | lenient `u8` | 0 | `browser_font_size` (300 is invalid, so 0) |
| `id` | `id`, or `null` when unset | lenient `i64` or `null` | `null` | `id` |

## S7. Deck

Record: the `decks` row (`id`, `name`, `mtime_secs`, `usn`), its deck common and its deck kind. The object starts with the keys of S7 in this order, then the `other` members (S3), then the keys of S8 (normal) or S9 (filtered).

| Key | Output | Input reader | Missing | Input result |
|---|---|---|---|---|
| `id` | row `id` | int-or-string `i64` | refuse | row `id` |
| `mod` | `mtime_secs` | int-or-string `i64` | 0 | `mtime_secs` |
| `name` | human form of the row `name` (S11) | strict string | refuse | native form (S11) |
| `usn` | `usn` | strict `i32` | refuse | `usn` |
| `lrnToday` | `[last_day_studied as i32, learning_studied]` | S7.1 | refuse | S7.1 |
| `revToday` | `[last_day_studied as i32, review_studied]` | S7.1 | refuse | S7.1 |
| `newToday` | `[last_day_studied as i32, new_studied]` | S7.1 | refuse | S7.1 |
| `timeToday` | `[last_day_studied as i32, milliseconds_studied]` | S7.1 | refuse | S7.1 |
| `collapsed` | `study_collapsed` | strict bool | refuse | `study_collapsed` |
| `browserCollapsed` | `browser_collapsed` | strict bool | false | `browser_collapsed` |
| `desc` | normal deck: `description`; filtered deck: `""` | strict string | `""` | normal deck: `description` |
| `md` | normal deck: `markdown_description`, only when true; filtered: never | strict bool | false | normal deck: `markdown_description` |
| `dyn` | 0 for normal, 1 for filtered | S7.2 | refuse | the deck kind |

Input also drops a top-level key `return` (an obsolete key) without keeping it in `other`.

### S7.1 Day counters

- Each of the four keys must hold a JSON array (anything else refuses the object). Its last element is the amount and the element before it the day; each counts only when it is an integer within the `i64` range, and is then taken as i32 (low 32 bits); otherwise, or when the array is too short, it is 0. So `[5]` is day 0 amount 5, `[]` is 0 and 0, `[1,2,3]` is day 2 amount 3.
- The deck's `last_day_studied` becomes the largest of the days of `timeToday`, `newToday` and `revToday` (the day of `lrnToday` does not count), stored as `u32` (low 32 bits).
- `learning_studied`, `review_studied` and `new_studied` take the amounts of `lrnToday`, `revToday` and `newToday`, each set to 0 when its own day differs from `last_day_studied`. `milliseconds_studied` takes the amount of `timeToday` as is.
- Example (oracle): `lrnToday` `[5,2]`, `revToday` `[7,3]`, `newToday` `[7,4]`, `timeToday` `[6,1000]` give day 7, learning 0, review 3, new 4, time 1000, which output writes back as `[7,0]`, `[7,3]`, `[7,4]`, `[7,1000]`.

### S7.2 `dyn`

The key is required. `true` and `false` mean filtered and normal. An integer from 0 to 255 means filtered when it is 1 and normal otherwise (`0` and `2` are normal). Any other value refuses the object: a string, a negative or larger number, or a number with a fraction (`1.0`). Input then reads the rest of the object as S8 (normal) or S9 (filtered).

## S8. Normal deck

| Key | Output | Input reader | Missing | Input result |
|---|---|---|---|---|
| `conf` | `config_id` | int-or-string `i64` | refuse | `config_id` |
| `extendNew` | `extend_new` as i32 | lenient `i32` | 0 | `extend_new`, negative values give 0 |
| `extendRev` | `extend_review` as i32 | lenient `i32` | 0 | `extend_review`, negative values give 0 |
| `reviewLimit` | `review_limit`, or `null` when unset | lenient `u32` or `null` | `null` | `review_limit` |
| `newLimit` | `new_limit`, or `null` when unset | lenient `u32` or `null` | `null` | `new_limit` (`-1` is invalid, so unset) |
| `reviewLimitToday` | `{"limit":L,"today":T}`, or `null` when unset | lenient: an object with both `limit` and `today` as strict `u32` (other members ignored), or `null` | `null` | `review_limit_today` (`{"limit":5}` is invalid, so unset) |
| `newLimitToday` | as `reviewLimitToday` | as `reviewLimitToday` | `null` | `new_limit_today` |
| `desiredRetention` | `desired_retention` × 100 in f32 arithmetic, truncated to `u32`; `null` when unset | lenient `u32` or `null` | `null` | the integer converted to `f32` and divided by 100 in f32 arithmetic (85 gives 0.85) |

The normal deck has no `other`; its unknown fields are protobuf-level (`model/protobuf-records` P4) and have no JSON form.

## S9. Filtered deck

| Key | Output | Input reader | Missing | Input result |
|---|---|---|---|---|
| `resched` | `reschedule` | bool-any | refuse | `reschedule` |
| `terms` | array of `[search, limit as i32, order]` per search term | strict array of three-element arrays `[strict string, int-or-string i32, strict i32]` | refuse | search terms; a negative limit gives 0 |
| `separate` | always `true` | strict bool | false | not stored |
| `delays` | `delays` as an array of `f32`, or `null` when empty | lenient array of `f32` or `null` | `null` | `delays` (empty for `null` or invalid) |
| `previewDelay` | `preview_delay` | strict `u32` | 0 | `preview_delay` |
| `previewAgainSecs` | `preview_again_secs` | strict `u32` | 0 | `preview_again_secs` |
| `previewHardSecs` | `preview_hard_secs` | strict `u32` | 0 | `preview_hard_secs` |
| `previewGoodSecs` | `preview_good_secs` | strict `u32` | 0 | `preview_good_secs` |

Example (oracle) of a filtered deck with one search `""`, limit 100, order 0, reschedule on, Again 60 s, Hard 600 s: `…,"dyn":1,"resched":true,"terms":[["",100,0]],"separate":true,"delays":null,"previewDelay":0,"previewAgainSecs":60,"previewHardSecs":600,"previewGoodSecs":0}`.

## S10. Deck config

Record: the `deck_config` row (`id`, `name`, `mtime_secs`, `usn`) and its deck config message.

| Key | Output | Input reader | Missing | Input result |
|---|---|---|---|---|
| `id` | row `id` | int-or-string `i64` | refuse | row `id` |
| `mod` | `mtime_secs` | int-or-string `i64` | refuse | `mtime_secs` |
| `name` | row `name` | strict string | refuse | row `name` |
| `usn` | `usn` | strict `i32` | refuse | `usn` |
| `maxTaken` | `cap_answer_time_to_secs` as i32 | strict `i32` (`"60"` refuses) | refuse | `cap_answer_time_to_secs`, negative values give 0 |
| `autoplay` | not `disable_autoplay` | strict bool | refuse | `disable_autoplay` = not the value |
| `timer` | 1 when `show_timer`, else 0 | lenient `u8` | refuse | `show_timer` = value is not 0 |
| `replayq` | not `skip_question_when_replaying_answer` | strict bool | false | `skip_question_when_replaying_answer` = not the value |
| `new` | S10.1 | lenient object (S10.1) | refuse | S10.1 |
| `rev` | S10.2 | lenient object (S10.2) | refuse | S10.2 |
| `lapse` | S10.3 | lenient object (S10.3) | refuse | S10.3 |
| `dyn` | always `false` | lenient bool | false | not stored |
| `newMix` | `new_mix` | strict `i32` | 0 | `new_mix` |
| `newPerDayMinimum` | `new_per_day_minimum` | strict `u32` | 0 | `new_per_day_minimum` |
| `interdayLearningMix` | `interday_learning_mix` | strict `i32` | 0 | `interday_learning_mix` |
| `reviewOrder` | `review_order` | strict `i32` | 0 | `review_order` |
| `newSortOrder` | `new_card_sort_order` | strict `i32` | 0 | `new_card_sort_order` |
| `newGatherPriority` | `new_card_gather_priority` | strict `i32` | 0 | `new_card_gather_priority` |
| `buryInterdayLearning` | `bury_interday_learning` | strict bool | false | `bury_interday_learning` |
| `fsrsWeights` | `fsrs_params_4` | strict array of `f32` | `[]` | `fsrs_params_4` |
| `fsrsParams5` | `fsrs_params_5` | strict array of `f32` | `[]` | `fsrs_params_5` |
| `fsrsParams6` | `fsrs_params_6` | strict array of `f32` | `[]` | `fsrs_params_6` |
| `desiredRetention` | `desired_retention` | strict `f32` | 0.0 | `desired_retention` |
| `ignoreRevlogsBeforeDate` | `ignore_revlogs_before_date` | strict string | `""` | `ignore_revlogs_before_date` |
| `easyDaysPercentages` | `easy_days_percentages` | strict array of `f32` | `[]` | `easy_days_percentages` |
| `stopTimerOnAnswer` | `stop_timer_on_answer` | strict bool | false | `stop_timer_on_answer` |
| `secondsToShowQuestion` | `seconds_to_show_question` | strict `f32` | 0.0 | `seconds_to_show_question` |
| `secondsToShowAnswer` | `seconds_to_show_answer` | strict `f32` | 0.0 | `seconds_to_show_answer` |
| `questionAction` | `question_action` when 1, else 0 | strict: exactly 0 or 1 | 0 | `question_action` |
| `answerAction` | `answer_action` when 1 to 4, else 0 | strict: exactly 0 to 4 | 0 | `answer_action` |
| `waitForAudio` | `wait_for_audio` | strict bool | true | `wait_for_audio` |
| `sm2Retention` | `historical_retention` | strict `f32` | 0.0 | `historical_retention` |
| `weightSearch` | `param_search` | strict string | `""` | `param_search` |

There is no key for a later FSRS parameter list. One that arrives as a key (`fsrsParams7`) is an ordinary `other` member and round-trips through sync; one that arrives as an unknown protobuf field has no JSON form and does not travel over sync, in Meguri as in Anki.

### S10.1 `new`

The value must be an object whose keys all fit their readers; when it is not (not an object, a required key missing, a strict key with a wrong value), the whole `new` part takes the defaults in the last column, and its members that the part does not read are dropped: none of them reaches `other` (S10.5). Output order is the table order, then the `new` extras of S10.5.

| Key | Output | Input reader | Missing | Input result | Default |
|---|---|---|---|---|---|
| `bury` | `bury_new` | strict bool | false | `bury_new` | false |
| `delays` | `learn_steps` | lenient array of `f32` | invalid `new` | `learn_steps` | `[1.0,10.0]` |
| `initialFactor` | `initial_ease` × 1000 in f32 arithmetic, truncated to `u16` | strict `u16` | invalid `new` | `initial_ease` = value as `f32` / 1000 in f32 arithmetic | 2500 |
| `ints` | `[graduating_interval_good` low 16 bits, `graduating_interval_easy` low 16 bits, `0]` | S10.4 | invalid `new` | `graduating_interval_good`, `graduating_interval_easy` | `[1,4,0]` |
| `order` | 0 when `new_card_insert_order` is 1 (random), else 1 | lenient: exactly 0 or 1, else 1 | invalid `new` | `new_card_insert_order`: 0 gives 1 (random), 1 gives 0 (due) | 1 |
| `perDay` | `new_per_day` | lenient `u32` | invalid `new` | `new_per_day` | 20 |

### S10.2 `rev`

Same rule, unknown members dropped with an invalid part included; defaults in the last column.

| Key | Output | Input reader | Missing | Input result | Default |
|---|---|---|---|---|---|
| `bury` | `bury_reviews` | strict bool | false | `bury_reviews` | false |
| `ease4` | `easy_multiplier` | strict `f32` | invalid `rev` | `easy_multiplier` | 1.3 |
| `ivlFct` | `interval_multiplier` | strict `f32` | invalid `rev` | `interval_multiplier` | 1.0 |
| `maxIvl` | `maximum_review_interval` | strict `u32` | invalid `rev` | `maximum_review_interval` | 36500 |
| `perDay` | `reviews_per_day` | lenient `u32` | invalid `rev` | `reviews_per_day` | 200 |
| `hardFactor` | `hard_multiplier` | strict `f32` | 1.2 | `hard_multiplier` | 1.2 |

### S10.3 `lapse`

Same rule as `new`, unknown members dropped with an invalid part included.

| Key | Output | Input reader | Missing | Input result | Default |
|---|---|---|---|---|---|
| `delays` | `relearn_steps` | lenient array of `f32` | invalid `lapse` | `relearn_steps` | `[10.0]` |
| `leechAction` | 1 when `leech_action` is 1, else 0 | lenient: exactly 0 or 1, else 1 | invalid `lapse` | `leech_action` | 1 |
| `leechFails` | `leech_threshold` | strict `u32` | invalid `lapse` | `leech_threshold` | 8 |
| `minInt` | `minimum_lapse_interval` | strict `u32` | invalid `lapse` | `minimum_lapse_interval` | 1 |
| `mult` | `lapse_multiplier` | strict `f32` | invalid `lapse` | `lapse_multiplier` | 0.0 |

### S10.4 `ints`

The value counts only when it is an array of at least two elements that are all strict `u16`; then the first is the graduating interval and the second the easy interval, and further elements are ignored. Otherwise the value is `[1,4]`. Examples (oracle): `[3,9]` gives 3 and 9 (Anki once wrote two elements); `[2,5,7,9]` gives 2 and 5; `[-5,4,3]` and `[4]` give 1 and 4. Output always writes three elements, the last 0.

### S10.5 `other` of a deck config

Output:

1. Parse `other` as in S3 rule 1.
2. Take out the members `new`, `rev` and `lapse`. Each one that is an object adds its members, minus that part's reserved keys (S3 table), after the known keys of the matching part (S10.1 to S10.3). One that is not an object is dropped.
3. The other members, minus the top-level reserved keys and keys already written, follow the known top-level keys (S3 rules 2 to 4).

Input:

1. Members of `new`, `rev` and `lapse` that the part does not read are kept per part, in input order, when the part is valid. A part that takes its defaults (S10.1) keeps nothing; the unknown top-level members are kept all the same.
2. `other` becomes an object of the unknown top-level members in input order, followed by `"new"`, `"rev"` and `"lapse"`, each present only when its part had unknown members and holding an object of them. Empty means zero bytes.

Example (oracle, `schema11/preset-part-defaults`): saving `new` = `{…,"perDay":7,"keepNew":1}` gives `other` = `{"new":{"keepNew":1}}`. Saving `new` = `{…,"initialFactor":70000,"keepNew":1,"keepNew2":2}` next gives the default `new` part (`perDay` 20) and an empty `other`. Saving `{"keepTop":8,"new":[1,2]}` gives the default `new` part and `other` = `{"keepTop":8}`.

Example (oracle): `other` = `{"maxTaken":5,"new":{"perDay":3,"keep":1},"keep2":2}` gives an object whose `maxTaken` and `new.perDay` come from the record, with `"keep":1` added at the end of `new` and `"keep2":2` at the end of the object. Unknown members `zz_new` in `new` and `zz_top` at the top give `other` = `{"zz_top":…,"new":{"zz_new":…}}` (Anki writes the same members in no particular order).

An add-on's legacy keys, like those a scheduling add-on leaves in a user's Default preset, are such members. They survive both directions and every later save, including one that changes only `fsrsParams6` (R1464, R1486). The scenario `schema11/preset-addon-roundtrip` carries a synthetic block of this kind through output, input and a second save.

## S11. Deck names

Deck names are stored in native form: the components of the hierarchy joined by U+001F. Their human form joins the components with `::` (R1465). Note type and preset names have no hierarchy and are not converted.

- Native to human: replace every U+001F by `::`.
- Human to native, used for every name that arrives in human form (S7 input; the decks area uses the same steps for names the user types):
  1. Split at each `::`, scanning left to right without overlap (`a:::b` gives `a` and `:b`).
  2. In each component: normalize to Unicode NFC; remove every ASCII control character (U+0000 to U+001F and U+007F); remove leading and trailing characters that are Unicode White_Space or `:`; if nothing is left, use `blank`.
  3. Join the components with U+001F.

  A stored deck name is normalized once more whenever a deck is written (`decks/decks#D1`), so a component is NFC also when removing a control character lets a combining mark compose: `e` U+0001 U+0301 is stored as `é`. `decks/decks#D1` gives the steps in the order that yields the stored result in one pass.
- Nothing else changes: emoji, mixed scripts and other characters round-trip byte for byte.

| Human input | Native result (U+001F shown as `␟`) | Human output |
|---|---|---|
| `A::B` | `A␟B` | `A::B` |
| `  A ::  :B: ` + U+0007 + `x ::  ` | `A␟B: x␟blank` | `A::B: x::blank` |
| `a:::b` | `a␟b` | `a::b` |
| `::a` | `blank␟a` | `blank::a` |
| `Cafe` + U+0301 | `Café` (NFC) | `Café` |
| `日本語::😀 Emoji` | `日本語␟😀 Emoji` | `日本語::😀 Emoji` |

## S12. Legacy JSON columns of `col`

At schema 11 the `col` row holds five JSON texts. At schema 18 all five are the empty string (R1456).

| Column | JSON value | Member key | Member value |
|---|---|---|---|
| `conf` | object | config key | the config value, any JSON (N7) |
| `models` | object | note type id, decimal | notetype object (S4) |
| `decks` | object | deck id, decimal | deck object (S7) |
| `dconf` | object | preset id, decimal | deck config object (S10) |
| `tags` | object | tag name | the tag's `usn` (strict `i32`) |

- Input: an id key must be a decimal integer (optional `-`, digits) within `i64`; any other key, or any member that its object rules refuse, refuses the whole column. For `dconf` only, when the text fails to convert, Anki parses it as plain JSON first (a repeated key keeps its last value) and converts that; and the member key, not the object's own `id`, is the preset's id.
- Output: one compact JSON object per column. Anki writes the members in no particular order; Meguri writes ids in ascending numeric order, tags in the order of the `tags` table, and config keys in ascending byte order.
- An empty collection at schema 11 holds `{}` in each column.
- A legacy package carries its collection at schema 11, so the package's `col` row holds these five columns too. Anki's exporter copies a note type as stored: the package's `models` member for a note type is the object that S4 output gives for its record in the exported collection (S14, scenario `schema11/apkg-notetypes`).
- What an upgrade or a downgrade does with the converted records (renaming duplicate names, replacing a note type id of 0, the starting-ease fixes of schemas 15 and 16, rebuilding tags, the `ver` and journal mode) belongs to the storage specs (U22, U33). This section is only the conversion.

## S13. Examples

New-collection objects, as Anki writes them (oracle; note type ids, field ids and template ids vary per collection).

Default preset:

```json
{"id":1,"mod":0,"name":"Default","usn":0,"maxTaken":60,"autoplay":true,"timer":0,"replayq":true,"new":{"bury":false,"delays":[1.0,10.0],"initialFactor":2500,"ints":[1,4,0],"order":1,"perDay":20},"rev":{"bury":false,"ease4":1.3,"ivlFct":1.0,"maxIvl":36500,"perDay":200,"hardFactor":1.2},"lapse":{"delays":[10.0],"leechAction":1,"leechFails":8,"minInt":1,"mult":0.0},"dyn":false,"newMix":0,"newPerDayMinimum":0,"interdayLearningMix":0,"reviewOrder":0,"newSortOrder":0,"newGatherPriority":0,"buryInterdayLearning":false,"fsrsWeights":[],"fsrsParams5":[],"fsrsParams6":[],"desiredRetention":0.9,"ignoreRevlogsBeforeDate":"","easyDaysPercentages":[1.0,1.0,1.0,1.0,1.0,1.0,1.0],"stopTimerOnAnswer":false,"secondsToShowQuestion":0.0,"secondsToShowAnswer":0.0,"questionAction":0,"answerAction":0,"waitForAudio":true,"sm2Retention":0.9,"weightSearch":""}
```

Default deck:

```json
{"id":1,"mod":0,"name":"Default","usn":0,"lrnToday":[0,0],"revToday":[0,0],"newToday":[0,0],"timeToday":[0,0],"collapsed":true,"browserCollapsed":true,"desc":"","dyn":0,"conf":1,"extendNew":0,"extendRev":0,"reviewLimit":null,"newLimit":null,"reviewLimitToday":null,"newLimitToday":null,"desiredRetention":null}
```

A field and a template of a note type (shape only):

```json
{"name":"Front","ord":0,"sticky":false,"rtl":false,"font":"Arial","size":20,"description":"","plainText":false,"collapsed":false,"excludeFromSearch":false,"id":-7170859537887897377,"tag":null,"preventDeletion":false}
{"name":"Card 1","ord":0,"qfmt":"{{Front}}","afmt":"{{Back}}","bqfmt":"","bafmt":"","did":null,"bfont":"","bsize":0,"id":3100154441998115515}
```

Anki applies range checks to preset values when it reads a preset (`sched/presets#V3`), so an object Anki writes from a preset with an out-of-range value already holds the replacement. The conversion in this file has no range checks; its callers convert the preset as read with those checks applied.

## S14. Goldens as conversion pairs

The model goldens hold Anki's own conversion in both directions, so tests can use them as input-output pairs without an executor. A step that reads blobs with `db_exec` (`hex()` of the blob columns, with the row columns beside them) followed by a `legacy_json` observation of the same records gives a record and the object Anki converts it to. A `legacy_update` step followed by such a read gives an object (what `legacy_json` reports just before the step, with the patch applied, `specs/conformance/model-ops.md` L2) and the record Anki converts it to.

| Record | Scenarios with blob reads and objects |
|---|---|
| note type, field, template | `proto/notetype-bytes`, `schema11/notetype-values`, `schema11/notetype-other-reserved`, `schema11/apkg-notetypes` (with a legacy package's objects, S14.1) |
| deck (normal and filtered) | `proto/deck-normal-bytes`, `proto/deck-filtered-bytes`, `schema11/deck-values`, `schema11/deck-other-flatten`, `schema11/deck-other-roundtrip`, `schema11/deck-names` |
| deck config | `proto/preset-bytes`, `schema11/preset-values`, `schema11/preset-other`, `schema11/preset-addon-roundtrip` (objects only) |
| refused objects | `schema11/notetype-rejections`, `schema11/deck-rejections`, `schema11/preset-rejections` |

- Compare objects as parsed JSON values, as the runner does (`specs/conformance/observations.md` O7).
- Compare an `other` field whose JSON object has more than one top-level member as a parsed value too: Anki writes those members in no set order (S3), so its bytes are not a fixed expected value. Every other byte of a golden blob is.
- A record read after `legacy_update` also shows what the save added to the conversion (`specs/conformance/model-ops.md` L2): a note type's `reqs` are the ones Anki recomputed, not the object's `req` (the conversion carries `req` as given, S4.1), and a preset has `usn` -1 and `mtime_secs` of the clock's second instead of the object's `usn` and `mod`.
- Legacy packages carry these same objects in their `col` columns (S12). S14.1 covers the note types of one; the package units (U120, U123) test import and export end to end with their own scenarios.

### S14.1 Note types in a legacy package

`schema11/apkg-notetypes` builds a standard and a cloze note type, each with one add-on member in the `other` of the note type, of a field and of the template, adds a note of each, and exports the collection as a legacy package (`specs/conformance/model-ops.md` L6). Before the export it reads the note types' rows and blobs with `db_exec` and their `legacy_json` objects; after it, `package_col` reports the package's `ver` (11) and its `models` column (L7), whose members are keyed by the note type ids. For each note type this gives three checks:

1. Record → object: the row columns and blobs, converted per S4 to S6, give the package's member under that note type's id, and the `legacy_json` object.
2. Object → record → object: the `models` value, written as JSON text and read per S12, gives one record per member; S4 output of each record gives that member back.
3. The package's member and the `legacy_json` object are the same value.

Expected differences between a package's member and the `legacy_json` object:

- In value, none. The exporter copies a note type as stored (same id, name, `mod`, `usn`, configs, and field and template ids), and both objects are the same conversion of the same record. Unknown protobuf fields of the blobs have no JSON form (`model/protobuf-records` P4), so they show in neither, and a schema-11 package cannot carry them.
- In text, only order and spelling. The note types in `models` and the top-level `other` members of each object come in no set order (S3, S12); nested objects inside `other` members come with sorted keys and their numbers as Anki spells a parsed value (`model/json-numbers` N7). Meguri's output keeps the stored order and spelling instead. Comparing parsed values (O7) hides all of these.
- What a legacy package holds besides its note types (decks, presets, `conf`), and what the exporter changes there, is the package area's (U120, U123).
