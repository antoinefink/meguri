# FSRS-only policy and collection compatibility (U50)

Spec-writer notes for `specs/sched/fsrs-policy.md` and
`specs/conformance/fsrs-policy-ops.md`. Read at tag 26.09.3
(`refs/anki-26.09.3`) and, for the parameter check, the `fsrs` crate at its
`v6.6.2` tag (`refs/fsrs-rs`, MIT/BSD); no code was copied. Every finding the
spec relies on was probed in the oracle image as a black box through the
`anki` package, and is repeated by a scenario under
`conformance/scenarios/sched/compat/`.

| Spec section | Anki files read | Read for |
|---|---|---|
| S1, V1 | `rslib/src/config/mod.rs`, `rslib/src/config/bool.rs`, `pylib/anki/collection.py` (`sched_ver`, `v3_scheduler`, `_load_scheduler`) | the readers of `fsrs`, `schedVer` and `sched2021`; what counts as the current scheduler |
| E1 to E5, F2 | `rslib/src/deckconfig/update.rs` (the deck options save), `rslib/src/scheduler/fsrs/memory_state.rs` (the bulk computation and its card search), `rslib/src/search/sqlwriter.rs` (deck ids matching home decks), `rslib/src/card/mod.rs` (card writes stamp every time, clearing FSRS data), `proto/anki/deck_config.proto` (the save request) | which cards the switch computes; what it writes to cards, presets, decks and global keys; the extra writes of the save (four global keys, missing-preset decks reassigned, older lists cleared, ignore date moved back, every computed card stamped); the switch-off clearing s, d, dr, decay |
| U1 | `rslib/src/deckconfig/update.rs` (the save builds every sent preset's model); fsrs `src/model.rs` at `v6.6.2` (`check_and_fill_parameters`) | the constructor's length and finiteness rules; Anki's failure for such a preset |
| U2, U3 | `proto/anki/deck_config.proto` (field numbers; 7 to 8 reserved next to the parameter lists with a comment keeping them for later parameter changes; 39 reserved) | no later list field at the tag; fields 7 and 8 are where a later list would go, as `sched/fsrs-transitions` T13 (U45) and `sched/presets` V6 (U42) also read it (later schema-11 keys land in `other`: `specs/model/schema11-json.md` S10). The 2026-10-17 completion pass aligned U1 to U3 with T13, the check U45, U46, U47 and U50 share, replacing the earlier rule that ignored fields 7 and 8 |
| V2, F3 | `qt/aqt/deckbrowser.py` (the update prompt and its action), `qt/aqt/reviewer.py` (no study before the update), `pylib/anki/collection.py` (`set_v3_scheduler`), `rslib/src/preferences.rs` (what the scheduler load writes when `creationOffset` is missing) | what accepting the update writes |
| K2 | `qt/aqt/reviewer.py` (the reviewer hands the script to its page) | that only the reviewer runs the script; the probes' answers go through the core and ignore it |

Probes (oracle image, `anki` 26.9.3; throwaway scripts outside the repository):

- The deck options save with FSRS on, rescheduling off and only the Default
  deck's stored preset sent: on a new collection it adds the four global keys
  (`applyAllParentLimits`, `cardStateCustomizer`, `fsrsHealthCheck`,
  `newCardsIgnoreReviewLimit`) when missing and moves a deck whose preset id
  names no preset to preset 1; with those keys present and no such deck it
  writes only `fsrs`, the computed cards and `col.mod`. A review card with no
  review-log row is not touched; a card whose only row is Set Due Date gets
  `dr` and `decay` and no memory state; filtered cards are computed through
  their home deck; a deck's own desired retention is written as `dr`.
- The same save with FSRS already on writes nothing, `col.mod` included.
- The switch-off removes `s`, `d`, `dr` and `decay`, keeps `pos` and `lrt`,
  stamps the cards and writes `fsrs` false.
- A preset with 34 values in `fsrsParams6`: its deck counts; computing answer
  states for its card, building the queue of its deck and the FSRS switch all
  fail with Anki's invalid-input error.
- `sched2021` missing, `false`, `1` or `"true"` all leave the previous
  scheduler; accepting the update writes `sched2021` true (local `usn`, now as
  `mtime_secs`), moves `col.mod`, and nothing else when `creationOffset` is
  present. `fsrs` stored as `1` or `"true"` reads off.
- All scenarios of `conformance/scenarios/sched/compat/` were run against the
  oracle with a throwaway driver extension implementing
  `fsrs-policy-ops.md` F2 and F3 (not committed: the oracle maintainer writes
  the driver ops and records the goldens).
