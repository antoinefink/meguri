# Deck presets

Spec: decks/deck-presets
Requirements: R732, R731, R733, R1475, R1486, R696, R1484, R1463, R1464
Anki: 26.09.3
Scenarios: conformance/scenarios/decks/presets/

Deck presets ("deck options groups", the `deck_config` table): their records, the default preset, which preset a deck uses, and the preset operations the deck options screen (U232, U49) and the optimizer (U47) build on: add from defaults, clone, rename, remove, assign to a deck or a subtree, and update. The options themselves, their ranges and the save of the deck options screen are U42's and U49's. The record is `model/protobuf-records` P6.6, its legacy JSON form `model/schema11-json` S10, the stamping terms `storage/ids-usn-mtime` ("stamped": `usn` -1, `mtime_secs` `now_s`).

## P1. Records

| Column | Content |
|---|---|
| `id` | the preset's id: 1 for the default preset, otherwise allocated as `storage/ids-usn-mtime` I2 says for `deck_config` |
| `name` | the name, stored as given; collation `unicase`, but no unique index: two presets may have the same name |
| `mtime_secs`, `usn` | stamps |
| `config` | the deck config message (P6.6) |

- Presets have no hierarchy and no name conversion: the name is stored exactly as the operation receives it (the UI's rule is P7).
- Removing a preset adds no grave (`storage/graves` G1); it is a schema change instead (P8).

## P2. The default preset

- Id 1, name `Default`. Every collection has it; it cannot be removed (R733).
- In a new collection (`storage/new-collection` B5) it holds the **built-in defaults**, `mtime_secs` 0 and `usn` 0. The built-in defaults are the values of the canonical blob that `model/protobuf-records` P6.6 shows: learning steps 1 and 10 minutes, relearning step 10 minutes, Easy Days 1.0 for each of the seven weekdays, 20 new cards and 200 reviews a day, starting ease 2.5, easy bonus 1.3, hard interval 1.2, lapse multiplier 0.0, interval modifier 1.0, maximum interval 36,500 days, minimum lapse interval 1 day, graduating intervals 1 and 4 days, leech action tag only, leech threshold 8, answer time cap 60 s, desired retention 0.9, historical retention 0.9, wait for audio on, every other field at its default (no FSRS parameter list, empty `param_search` and `ignore_revlogs_before_date`, no `other`).
- Meguri always names it `Default`. (Anki takes the name from the user interface language when it creates a collection; in English it is `Default`.)

## P3. The preset of a deck

- A normal deck uses the preset whose id is its `config_id` (P6.5.1). A filtered deck uses no preset.
- When `config_id` names no preset, the deck uses the default preset; when that is missing too, the built-in defaults (P2). Nothing is written by this fallback.
- When a preset is read for scheduling, values outside Anki's ranges are replaced (`model/schema11-json` S13; U42).
- **Use count** (R731): the number of normal decks whose `config_id` is the preset's id. The picker shows "name (used by N decks)".

## P4. Add a preset from the defaults (R732)

- Input: a name (P7 gives the UI's naming).
- The new preset holds the built-in defaults (P2) with one difference: `fsrs_params_6` holds the 21 FSRS-6 default parameters of fsrs-rs 6.6.2, in this order: 0.212, 1.2931, 2.3065, 8.2956, 6.4133, 0.8334, 3.0194, 0.001, 1.8722, 0.1666, 0.796, 1.4835, 0.0614, 0.2629, 1.6483, 0.6014, 1.8729, 0.5425, 0.0912, 0.0658, 0.1542 (each stored as its `f32`). This is what Anki's deck options screen adds. `fsrs_params_4` and `fsrs_params_5` are empty and `other` is empty.
- It gets a new id (P1) and is stamped. It is not assigned to any deck by this operation.
- Note for U232: when Anki's screen shows its FSRS date field for a preset whose `ignore_revlogs_before_date` is empty, it fills in `1970-01-01`, which a save then stores. Whether Meguri's save does the same is U232's decision with U49.

## P5. Clone a preset (R732)

- Input: the source preset and a name.
- The new preset's config is the source's: every field, `other` byte for byte, and the unknown fields Meguri captured (`model/protobuf-records` P4). The three FSRS parameter lists are copied as stored (R1486).
- It gets a new id and is stamped. It is not assigned to any deck by this operation.
- Anki's screen first fills an empty `fsrs_params_6` from `fsrs_params_5`, or else `fsrs_params_4`, to display it, so its clone stores that list in `fsrs_params_6` too; and Anki's storage drops unknown protobuf fields. Meguri does neither (R1486, R1463); the two differ only for a source preset whose FSRS-6 list is empty while an older list is not, or that carries fields of a later Anki. No scenario uses such a source.

## P6. Rename a preset (R732)

- Input: a preset and a name. The name is stored as given.
- A name equal to the stored one byte for byte writes nothing; otherwise the preset is stamped. Nothing else in the record changes.

## P7. Naming in the preset menu (UI rule, R732)

Anki's deck options screen, which Meguri's preset menu (U232) follows:

- **Add** and **Clone** prompt for a name (Clone pre-filled with the current preset's name). The text is trimmed of leading and trailing white space; an empty result does nothing.
- **Rename** prompts with the current name; Anki uses the text as typed, not trimmed. A text equal to the current name does nothing.
- When the name (after trimming, for Add and Clone) equals another preset's name exactly, byte for byte and with case, the current Unix time in seconds, rounded to the nearest whole second (a time exactly half-way rounds up), in decimal, is appended: `Spanish` becomes `Spanish1894701600` at 2030-01-15 10:00 UTC. At 10:00:00.499 the suffix is `1894701600`; at 10:00:00.500 it is `1894701601`. The check is made once; the result is not checked again.
- The core offers this as a helper that takes the candidate name, the existing names and the clock; the preset operations store what they receive.

## P8. Remove a preset (R733, R1475)

- Input: the preset to remove and the **fallback** preset: the preset that the screen is saving for its deck (R733: decks that used the removed preset get the preset being saved). The fallback must exist and differ from the removed preset (`InvalidArgument` otherwise).
- The default preset (id 1) cannot be removed: refused with `default_preset`, nothing written.
- It is a schema change with reason `preset_removed` (`notetypes/schema-changes` G2). It commits without asking, whatever `scm` and `ls` hold (G4), and sets `scm` to `now_ms` (G5). Changed 2026-10-27: the confirmation gate and its `NeedsSchemaConfirmation` refusal are gone.
- It is a structural edit, so the operation that runs it takes a safety backup before its first collection write, unless T6.1's coalescing applies (`package/backup-triggers#T6.1`, T6.2). Decided 2026-10-27: the backup belongs to the operation that runs the removal. Run inside the deck options save, that is the save's one backup (P11; `sched/deck-options` O7); run on its own, the removal's. A refused removal (`default_preset`, `InvalidArgument`) is refused while its plan is made and takes none.
- Effects, in one transaction:
  1. every normal deck whose `config_id` is the removed preset's id, or names no preset that exists after the removal, gets the fallback's id and is stamped (the deck write path of `decks/decks` D13 applies);
  2. the preset's row is deleted, without a grave;
  3. `scm` and `col.mod` are set to `now_ms`.
- Decks that used other presets, and filtered decks, do not change.

## P9. Assign a preset (R731, R732)

- Input: a normal deck, a preset, and whether to apply to the deck's subtree ("Save to all subdecks", R732).
- The deck, and with the subtree option every normal descendant (`decks/decks` D3), gets the preset's id as `config_id`. A deck that already has it is not written; any other is stamped. Filtered descendants are left alone.
- A filtered deck as the input deck is refused with `filtered_deck`, nothing written. A preset that does not exist is refused with `NotFound`.
- The deck write path of `decks/decks` D13 applies; nothing else in the deck changes (the deck's own limits are U232's and U49's).

## P10. Update a preset (R1486, R696, R1484)

- An update edits the decoded fields it means to change and keeps everything else as read: other options, `other` and unknown fields (`model/protobuf-records` P4, P5, P8). The bytes follow P5: unchanged records keep their stored bytes.
- A record equal to the stored one is not written (`storage/ids-usn-mtime` S4); otherwise it is stamped.
- `fsrs_params_6` has a setter only for the optimizer (U47, R696). `fsrs_params_4`, `fsrs_params_5` and any later list are never written by an update (R1486).
- The deck options save (U232) and the optimizer (U47) are the callers; the undo step is theirs ("Deck Options" for the save, R737).

## P11. Operations

- P4 to P10 are core primitives that run inside the caller's operation (U24): the deck options save runs them in one undoable step named "Deck Options" (R737). On their own (conformance ops, tests) each is one operation.
- Only P8 is a schema change. The others change rows and stamps only.

## P12. What Anki's deck options save adds (for U49 and U232)

Anki's screen saves everything in one "Deck Options" operation. Observed with the oracle and recorded here so that U49 and U232 decide them explicitly; none is part of P4 to P10:

- removals run first, then the edited presets are saved, the current one last; the target deck (or its subtree) is assigned the last saved preset;
- every normal deck whose preset was removed, or names no existing preset, is assigned that preset too, and gets the target deck's daily limits written into its own record;
- the screen writes the collection-wide keys `cardStateCustomizer`, `newCardsIgnoreReviewLimit`, `applyAllParentLimits` and `fsrsHealthCheck`, adding them when they were missing, and `fsrs` when FSRS was switched on or off;
- an empty `fsrs_params_6` is filled from the older lists for display (P5), so the save writes it.

## P13. Scenarios

| Scenario | Covers |
|---|---|
| `decks/presets/add-clone-rename` | P4, P5, P6: ids, stamps, the stored config (defaults with FSRS-6 parameters; a clone's bytes equal its source's) |
| `decks/presets/assign` | P9: one deck, a subtree, filtered descendants untouched, no write when unchanged, `filtered_deck` |
| `decks/presets/remove` | P8 after a sync (`scm <= ls`), run as accepted (Meguri asks nothing, `notetypes/schema-changes#G4`): decks of the removed preset and a deck with a missing preset move to the fallback, `scm` set past `ls`, no grave; `default_preset` |
| `decks/presets/missing-reads-default` | P3: a deck whose `config_id` names no preset reads the default preset, not the built-in defaults: Set Due Date takes the default preset's starting ease and a new card takes the default preset's random insertion order (`notes/deck-placement-and-position#P4`); the deck's row is not written |
| `sched/ops/set-due/missing-preset` (held) | P3 seen through Set Due Date: a deck naming a missing preset uses the default preset |
| `sched/ops/set-due/missing-default-preset` (held) | P3 seen through Set Due Date: with the default preset missing too, the built-in defaults (P2) |

Meguri's own behavior is tested in `core/crates/meguri-engine/tests/deck_config.rs`: with `scm <= ls` a removal commits at once, sets `scm` and takes no confirmation input (R1476); its safety backup is published before its first write, and a refused removal takes none (T6.1); P7's helper, with the clock on each side of a half second; P10's field-level update keeping unknown fields and the older FSRS lists (R1486); the fallback of P3.
