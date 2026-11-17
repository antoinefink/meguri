# Deck options and step remapping (U49)

Spec-writer notes for `specs/sched/deck-options.md`, `specs/sched/step-remap.md`
and the U49 additions to `specs/conformance/deck-options-ops.md`. Read at tag
26.09.3 (`refs/anki-26.09.3`); no code was copied. Five scenarios under
`conformance/scenarios/sched/options/` have goldens recorded with
`tools/mg oracle record` and compared twice on the Linux oracle and once on the
Apple oracle (`remap-steps`, `undo-save`, `insertion-order-switch`,
`missing-preset`, `live-save`); they confirm step-remap R2 and R3, deck-options
O7.2 steps 3 to 6 and O8 before the specs were final.

| Spec section | Anki files read | Read for |
|---|---|---|
| step-remap R1, R2 | `rslib/src/card/mod.rs` | the remaining count modulo 1000; the translation (old list empty, no passed step, last new step not longer than the last passed one, at least one); no write when unchanged; the whole column replaced |
| step-remap R3 | `rslib/src/deckconfig/update.rs`, `rslib/src/search/builder.rs`, `rslib/src/search/sqlwriter.rs` | per normal deck, previous preset from the map before the save (none when missing), lists compared before translating, learning and relearning searches by card type only, cards by `did` or `odid` |
| step-remap R4 | `rslib/src/card/mod.rs` | Change Deck's adjuster: home deck's preset with the default-preset and built-in fallbacks, target's preset with the same fallback, no equality check, translation before the move, cards already in the target skipped |
| deck-options O2 | `rslib/src/deckconfig/update.rs`, `ts/routes/deck-options/lib.ts`, `ts/routes/deck-options/AdvancedOptions.svelte`, `rslib/src/storage/deckconfig/mod.rs` | what the screen reads (presets with use counts, the deck's limits with the Today-only active flag, the global keys, defaults with FSRS-6 parameters, schema modified); bounded reads; the initial selection when the preset is missing; when the ignore-date count is shown |
| deck-options O3 | `ts/routes/deck-options/*.svelte`, `ts/routes/deck-options/choices.ts`, `ts/lib/components/SpinBox.svelte`, `rslib/src/deckconfig/mod.rs` | sections, spin box ranges (defaults 0 to 9999 and the explicit ones), menu orders and FSRS labels, which options the FSRS screen shows |
| deck-options O4 | `ts/routes/deck-options/DailyLimits.svelte`, `FsrsOptions.svelte`, `NewOptions.svelte`, `LapseOptions.svelte`, `TimerOptions.svelte`, `AdvancedOptions.svelte`, `ts/lib/components/SpinBox.svelte` | warning conditions and thresholds (reviews under ten times new capped at 9999, retention 0.80/0.95 and 0.70/0.97 on the rounded value, last step of a day or more under FSRS, answer seconds above 600, maximum interval 180 and 50); spin boxes clamp only edited values |
| deck-options O5 | `ts/routes/deck-options/steps.ts`, `ts/lib/tslib/time.ts` | Anki's lenient parse (single-space split, digits then any suffix, zeros dropped, 2^31 s cap) and its formatting walk from the natural unit with months and years mapped to days |
| deck-options O6 | `ts/routes/deck-options/lib.ts`, `ConfigSelector.svelte`, `TabbedValue.svelte`, `DailyLimits.svelte` | staging of add, clone (current draft values), rename, remove (selection moves to the previous entry, confirmation only before the first removal when the schema is unmodified), use counts shown, which presets a save sends and in which order, the tab rules, change detection with f32 comparison |
| deck-options O7 | `rslib/src/deckconfig/update.rs`, `rslib/src/deckconfig/mod.rs`, `rslib/src/decks/addupdate.rs`, `rslib/src/config/undo.rs`, `rslib/src/ops.rs` | order of the save (removals with schema change, presets added or updated with no write when equal, deck loop assigning the last preset and the request's limits to selected, child and orphaned decks, re-sort on order change, step adjustment, recompute, global keys), unchanged config values not rewritten, the undo name |
| deck-options O8 | `rslib/src/scheduler/new.rs`, `rslib/src/search/sqlwriter.rs`, `rslib/src/notetype/cardgen.rs` | the deck re-sort: new-type cards by `did` or `odid`, notes by id or shuffled, positions from 1, `odue` for filtered cards, every card stamped; random positions at card generation |
| deck-options O9 | `rslib/src/deckconfig/update.rs` | limits written to each assigned deck, the Today-only clear rule (removal on day 0 or for a future day, else day moved into the past) |
| deck-options O10 | `rslib/src/preferences.rs`, `rslib/src/config/mod.rs`, `rslib/src/scheduler/mod.rs`, `qt/aqt/preferences.py`, `qt/aqt/forms/preferences.ui`, `rslib/src/collection/service.rs`, `rslib/src/ops.rs` | the preferences save (rollover only when changed, cached day dropped, learn-ahead seconds, load balancer in the reviewing part, other keys written), UI ranges 0 to 23 and 0 to 999 minutes, the separate load balancer toggle op, queue rebuild after preference and deck options ops |
| deck-options-ops E1, E4 | `proto/anki/deck_config.proto`, `proto/anki/config.proto`, `rslib/src/deckconfig/update.rs`, `rslib/src/preferences.rs` | the save request's mode, removed ids, limits and global fields; the preferences message parts |

Oracle-maintainer hints for the awaiting driver work (spec side only):

- `save_deck_options` extensions map onto the same update request: `remove` to the removed config ids (the backend refuses id 1 with an invalid-input error: record `default_preset`), `subtree` to the apply-to-children mode, `globals` to the two request booleans, `options.insertion_order` to the config's insert order, `limits` onto the limits read from the screen after clearing each Today-only value whose active flag is false (E1's rule for a missing one).
- `set_scheduling_prefs`: read the preferences, change scheduling `rollover` and `learn_ahead_secs` (minutes × 60) and reviewing `load_balancer_enabled`, and save the scheduling and reviewing parts in one preferences save, which is the "Preferences" undo step.

## Scheduling preference row completion clarification

Tag: 26.09.3. Sections: deck-options O10; deck-options-ops E4.

| Source | Read for |
|---|---|
| `rslib/src/preferences.rs` | Effective settings; duration and boolean writes; rollover condition |
| `rslib/src/scheduler/mod.rs` | Effective rollover fallback |

The conformance request is a partial edit of a complete save. Omitted learn-ahead and load-balancer values are retained from the read and submitted to setters; omission does not suppress their writes. A missing load-balancer row is therefore present with true after the first rollover-only preference save. A later false save's undo restores that true row. Rollover remains conditional on a changed effective hour. The empty-request rejection remains the existing Meguri scenario/API rule.

## Session s-20261117T152613Z-96466c16 (spec writer): deferred harvest rows, batch 2

Read at tag 26.09.3 (`refs/anki-26.09.3`) to classify deferred harvest rows, write their scenarios and draft spec text; no code, test text or fixture value was copied unless a row says otherwise, and every expected value comes from the recorded oracle goldens.

| Spec section and scenarios | Anki file | Read for |
|---|---|---|
| sched/options/remove-preset-undo-rows (preset removal inside the deck options save, its undo and redo) | `rslib/src/deckconfig/undo.rs`, `rslib/src/deckconfig/mod.rs`, `rslib/src/undo/mod.rs` | Which test behavior must be proven: undo of a removal restores the stored row under its existing id with its stamps. Also that a removal outside the undoable save empties the undo history, which is why the scenario removes through save_deck_options. |
| step-remap R2/R3 scenarios sched/options/remap-answered/learning and relearning | `rslib/src/deckconfig/update.rs`, `rslib/src/tests.rs` | Which behaviors the remaining-step tests prove: an unreached step added raises the count; an edit of the other step kind leaves the card; a passed step put before keeps the count; a shortened list keeps one step. How the test collections reach a learning card (one Again on a new card) and a relearning card (Easy, then due, then Again). That the save adjusts per deck only when the old and new lists differ. Expected values come from the goldens. |
| deck-options O12 ignore-date clamp; scenarios sched/options/ignore-date/kept and future | `rslib/src/deckconfig/update.rs` | That the deck options save replaces an ignore date after today's date (the date of next_day_at minus one day) with that date, and keeps past, current and empty values. That the comparison is between date texts as strings. Expected values come from the goldens. |
| deck-options O7.2 steps 1-4; scenario sched/options/save-writes | `rslib/src/deckconfig/update.rs` | Which effects the save test checks together: a save with no changes; an option change writing only the preset; a new preset staged in the request and assigned to the deck; a Random re-sort, seen after positions are cleared; a removal that reassigns the default preset, re-sorts the cards and forces a one-way sync that was not required before. Expected values come from the golden. |
| Scenarios sched/options/remap-floor-shortened and sched/options/remap-passed-step-inserted (step-remap R2 rules 3 and 4, R3; deck-options O7.2 step 5) | `rslib/src/deckconfig/update.rs` (tests module); `rslib/src/tests.rs` (test collection helpers) | Which behaviors the two harvested tests require: after one passed learning step, a shorter step list keeps one remaining step, and inserting a step no longer than the passed one keeps the remaining count. Also that the helpers change the default preset's steps and then adjust remaining steps in its deck. Expected values come from the oracle goldens, not from the test assertions. |
| Scenario sched/options/nested-deck-opening-preset (deck-options O2 item 2, O6 opening preset; deck-options-ops E1 save without preset) | `ts/routes/deck-options/lib.test.ts` | Which behavior the harvested start-up test requires: the screen state built for a nested deck holds that deck and its own preset as the current selection. The scenario's decks, presets and values are my own. Expected values come from the oracle golden. |
| Harvest triage of the deck-options step-text rows, and the proposed amendments to sched/deck-options O5.1/O5.2 example rows, O12 Validation and O13, and to deck-options-ops E3/E6 | `ts/routes/deck-options/steps.test.ts`; `ts/routes/deck-options/steps.ts`; `qt/aqt/deckconf.py` (a grep only) | steps.test.ts: which step-text behaviors the four rows must prove (whole-unit and fractional formatting and parsing, f32 noise in formatting, empty or blank text and unreadable tokens). The current O5 example rows repeat that file's fixture values, so the amendment replaces them with new values. steps.ts: confirmed that the step-text functions exist only in the deck options web page. Also confirmed how the page reads text: it splits only at single spaces, reads the first digit run with the rest as the unit, drops pieces that read as zero, and caps at 2^31 seconds. These points went into the O12 Validation amendment and E6's scenario limits. deckconf.py: confirmed that the legacy dialog's list text is a different function. |
| Proposed spec gap sched/presets D4.1 (applying studied-count deltas) and the needs-op note for update_deck_stats | `rslib/src/scheduler/service/mod.rs` (update_stats, extend_limits, counts_for_deck_today service entries and their tests); `rslib/src/decks/stats.rs` (per-deck delta application, ancestors, reset on a new day, stamping); `pylib/anki/scheduler/legacy.py` (grep only: the legacy caller of update_stats) | Behavior scope of the legacy studied-delta request: deck plus ancestors, day reset, no undo step. This is to describe the missing op and spec rule. |
| sched/deck-options O5.2 and O12 step text (native-ui row) | `ts/lib/tslib/time.test.ts`, `ts/lib/tslib/time.ts`, `ts/routes/deck-options/steps.ts` | the whole-unit choice behind the step text writer, and confirming the whole-month rounding difference already listed in O12 |

## Audit fixes (2026-11-17, session s-20261117T172637Z-685ca4ee)

Read at tag 26.09.3 (`refs/anki-26.09.3`) to fix spec readiness audit findings; no code was copied, and every rule was restated in the spec writer's words.

| Subject | Anki file | Read for |
|---|---|---|
| sched/deck-options O3 (percent and Auto Advance editing, hidden options as read), O4.3 (tabbed-row field, retention rounding), O6.7 and O7.2 step 2 (draft order, write order), O14.1 (out-of-range commit) | `ts/routes/deck-options/lib.ts`; `ts/routes/deck-options/FsrsOptions.svelte`; `ts/lib/components/SpinBox.svelte`; `ts/routes/deck-options/SpinBoxFloatRow.svelte`; `ts/routes/deck-options/AutoAdvance.svelte`; `ts/routes/deck-options/AdvancedOptions.svelte` | sorting of the draft preset list after add and rename, selection after removal, order of configs sent on save; roundedRetention via toFixed(2) used for warning and class; spin box clamps and keeps the value only for NaN, display decimals from the step; Auto Advance step 0.1; historical retention as percentage with default step 0.01 (whole percent); desired retention spin box min 0.7, max 0.99, percentage |
