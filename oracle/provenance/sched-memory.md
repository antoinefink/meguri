# Memory states and FSRS items (U46)

Spec-writer notes for `specs/sched/fsrs-items.md`, `specs/sched/memory-state.md`
and `specs/conformance/deck-options-ops.md`. Read at tag 26.09.3
(`refs/anki-26.09.3`); no code was copied. The findings are repeated by the
scenarios under `conformance/scenarios/sched/memory/`; nine have goldens
recorded with `tools/mg oracle record`, and black-box probes of the oracle
image (the backend's deck options save and ignored-before count, called from
a throwaway script) confirmed the rest before the specs were written.

| Spec section | Anki files read | Read for |
|---|---|---|
| fsrs-items I1.1 | `rslib/src/scheduler/fsrs/params.rs`, `rslib/src/storage/revlog/mod.rs`, `get.sql` | rows per card in (cid, id) order; whole-collection reads include rows of deleted cards; an unknown or non-integer `type` reads as learning |
| fsrs-items I1.3 | `rslib/src/scheduler/fsrs/params.rs`, `rslib/src/deckconfig/update.rs`, `rslib/src/timestamp.rs`, chrono 0.4.41 (Anki's `Cargo.lock`) | the date parse (format `%Y-%m-%d`, UTC midnight in ms); the save-time clamp to today's local date by string comparison; the parse leniency and year range were probed on the oracle |
| fsrs-items I1.4 | `rslib/src/deckconfig/mod.rs`, `schema11.rs` | `historical_retention` / `sm2Retention`, default and bounds |
| fsrs-items I2 | `rslib/src/revlog/mod.rs` | button, cram, reset and graded predicates |
| fsrs-items I3, I4 | `rslib/src/scheduler/fsrs/params.rs` | where a history starts (learning run, reset marker, cutoff, the last-row exception of the memory form, the training exclusions); day numbers relative to `next_day_at`; delta_t |
| fsrs-items I5, I6 | `rslib/src/scheduler/fsrs/memory_state.rs`, fsrs 6.6.2 `inference.rs`, `model.rs` | SM-2 starting state from the first kept row (factor 0 as 2500, difficulty from factors up to 1100), the removed first review, batch equal to single calls |
| fsrs-items I7 | `rslib/src/scheduler/fsrs/params.rs` | training items per review with delta_t above 0, sort by revlog id, aligned card ids, item and review counts |
| memory-state M2 | `rslib/src/scheduler/answering/mod.rs`, `rslib/src/scheduler/fsrs/memory_state.rs` | the on-the-fly path in the state updater: item, else the card's own SM-2 values unless its interval is 0; no write |
| memory-state M3 | `rslib/src/scheduler/answering/mod.rs`, `rslib/src/storage/revlog/time_of_last_review.sql` | the lrt fallback query (ease 1 to 4, no cram rows, resets ignored), 0 days without a row |
| memory-state M4 | `rslib/src/scheduler/fsrs/memory_state.rs`, `rslib/src/card/mod.rs`, `rslib/src/card/undo.rs`, `rslib/src/search/sqlwriter.rs` | cards visited only through revlog groups (no rows: untouched), not-new filter, itemless cards cleared, dr from the deck map else the preset, decay from the params; every visited card stamped; batches of 1000 sorted by review count (no effect on values) |
| memory-state M5 | `rslib/src/deckconfig/service.rs`, `rslib/src/storage/card/get_ignored_before_count.sql`, `ts/routes/deck-options/AdvancedOptions.svelte` | the count (stored type 0, id after the cutoff), the search the screen passes, the three warning levels and when the count is shown |
| memory-state M6 | `rslib/src/storage/card/mod.rs`, `rslib/src/scheduler/fsrs/memory_state.rs`, `rslib/src/dbcheck.rs` | last review time from the latest graded row after the last reset; new and revlog-less cards skipped; storage write without stamping; schema marked modified |
| memory-state M7.1 | `rslib/src/deckconfig/update.rs` | which decks a save recomputes (fsrs toggled, params, effective desired retention, or easy days only when rescheduling) and the search per deck (did or odid) |
| memory-state M7.2 | `rslib/src/card/mod.rs` | Change Deck clears FSRS data, then recomputes cards of the target deck without a memory state |
| deck-options-ops E1, E2 | `proto/anki/deck_config.proto`, `rslib/src/deckconfig/update.rs`, `rslib/src/ops.rs`, `ftl/core/deck-config.ftl` | the save request's fields and its undo name; the count request |

Oracle-maintainer hints for the two new driver entries (spec side only):

- `save_deck_options`: read the screen state with `GetDeckConfigsForUpdate`
  for the deck, take the chosen preset's config from `all_config`, apply the
  options (`fsrs_params` to `fsrs_params_6`, `historical_retention`,
  `ignore_revlogs_before_date`, the setup-preset conversions for the rest),
  and call `UpdateDeckConfigs` with that one config, the target deck, mode
  normal, the current deck limits with `desired_retention` set or cleared,
  the current `fsrs`, `new_cards_ignore_review_limit`,
  `apply_all_parent_limits`, `fsrs_health_check` and
  `card_state_customizer`, and `fsrs_reschedule` false. The generated
  backend method takes keyword arguments.
- `ignored_before_count`: `GetIgnoredBeforeCount` with the preset's
  `param_search` when not empty, else `preset:"<name>" -is:suspended`, once
  per date; an invalid-input backend error is `invalid_date`.
