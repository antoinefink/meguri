# Floating-point probe, FSRS review transitions and fuzz (U14)

Read at tag 26.09.3 (`refs/anki-26.09.3`) to write `specs/conformance/float-determinism.md`, `specs/sched/fsrs-transitions.md` (T1–T6) and `specs/sched/fuzz.md` (Z1–Z6). No code was copied; the rules were restated in the spec writer's words. The fuzz examples in `sched/fuzz` Z4 and Z5 were worked from the restated rules with chosen values, not taken from Anki's unit tests; the Z6 and T6 examples come from the oracle's goldens.

| Spec section | Anki file | Read for |
|---|---|---|
| fsrs-transitions T1, T3 | `rslib/src/scheduler/answering/mod.rs`, `rslib/src/scheduler/answering/current.rs` | which crate call gives the states, the current review state's fields |
| fsrs-transitions T2.1 | `rslib/src/deckconfig/mod.rs`, `rslib/src/deckconfig/schema11.rs`, `rslib/src/scheduler/fsrs/memory_state.rs` | parameter list fallback order and keys, the decay written to cards |
| fsrs-transitions T2.2 | `rslib/src/decks/mod.rs`, `rslib/src/decks/schema11.rs` | deck desired retention override, whole percent in the legacy deck object |
| fsrs-transitions T2.3 | `rslib/src/timestamp.rs`, `rslib/src/scheduler/answering/mod.rs` | whole days from the last review time to the next day boundary, revlog fallback |
| fsrs-transitions T4, T5 | `rslib/src/scheduler/states/review.rs`, `rslib/src/scheduler/states/steps.rs`, `rslib/src/scheduler/states/mod.rs` | Again's unfuzzed rounding, relearning state, leech rule, ease deltas, the Hard/Good/Easy chain, minimum and maximum clamps |
| fuzz Z1, Z2 | `rslib/src/scheduler/answering/mod.rs`, `Cargo.lock` (`rand` 0.9.4) | seed (card id plus reps, previous reps for recomputation), the factor draw, test mode disabling fuzz |
| fuzz Z3–Z5 | `rslib/src/scheduler/states/fuzz.rs` | fuzz ranges, bounds, widening, pick, no-regression minimum |
| fuzz Z6 | `rslib/src/scheduler/states/fuzz.rs`, `proto/anki/scheduler.proto` | the fuzz-delta call the `fuzz_table` observation uses |
| float-determinism F3.4 | `rslib/src/lib.rs`, `rslib/src/timestamp.rs` | `ANKI_TEST_MODE` turns fuzz off and shifts the clock between 02:00 and 04:00 local time |
| float-determinism F5.4 | `proto/anki/scheduler.proto` | no backend call computes FSRS states from a bare memory state, so the oracle evaluates a stored card |

Third-party crate sources read for exact behavior (not Anki code): `fsrs` 6.6.2 (tag `v6.6.2` of `refs/fsrs-rs`: `src/inference.rs` `next_states` and the default parameters, `src/model.rs` parameter filling, `next_interval` and the 0.9 constant as `f32`, `src/parameter_clipper.rs` clipping ranges, `src/simulation.rs` stability and difficulty bounds).

F8.3 as revised and F11 (the probe's decision record) come from the probe's own data, not from Anki: the committed goldens and Apple overlays of `fp-probe/corpus`, the Meguri sides' outputs, and a 64-bit sensitivity calculation over the corpus based on the `fsrs` 6.6.2 stability formulas (`src/model.rs`: stability after a lapse and after a recall; `src/parameter_clipper.rs` for the parameter ranges). No Anki file was read for them.
