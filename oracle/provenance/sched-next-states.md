# Next states, steps, fuzz and labels (U45)

Read at tag 26.09.3 (`refs/anki-26.09.3`) to write `specs/sched/steps.md`,
`specs/sched/fsrs-transitions.md` T7 to T14, `specs/sched/fuzz.md` Z7 to Z9,
`specs/sched/answer-labels.md` and `specs/conformance/observations.md` O10;
no code was copied. fsrs-rs was read at its tag `v6.6.2` (`refs/fsrs-rs`) for
the parameter forms it accepts.

| Anki file | Read for | Spec sections |
|---|---|---|
| `rslib/src/scheduler/states/steps.rs` | step minutes to seconds, the current index from the remaining count, Again, Hard (first-step average, single-step rule, day rounding), Good, remaining counts | steps L1 to L5 |
| `rslib/src/scheduler/states/normal.rs` | a new card answered as a learning card at its first step, with the current state kept new | steps L6, transitions T7 |
| `rslib/src/scheduler/states/learning.rs` | graduation from learning, Easy's minimum from the fuzzed Good interval, the short-term condition for learning cards, which fields keep their values | transitions T8, T10; steps L10 |
| `rslib/src/scheduler/states/relearning.rs` | relearning answers with and without steps, the review part kept, elapsed values kept, short-term relearning | transitions T9, T10; steps L9, L10 |
| `rslib/src/scheduler/states/review.rs` | ease deltas and floor, the leech predicate, Again's unfuzzed review part, the no-relearning-steps branch | transitions T4, T11, T12 |
| `rslib/src/scheduler/states/mod.rs` | the minimum/maximum clamp of review intervals and the inputs of a state computation | transitions T5, T14; fuzz Z8 |
| `rslib/src/scheduler/states/fuzz.rs` | every place the review fuzz runs and the load-balancer hook in front of it; the widening step | fuzz Z4 note, Z8 |
| `rslib/src/scheduler/states/interval_kind.rs` | seconds that reach the next day become whole days; the review-log interval sign | steps L7; transitions T14; labels A1 |
| `rslib/src/scheduler/answering/current.rs` | the current state per card type and queue, the elapsed-seconds reconstruction with the previous review count wrapping in 32 bits | transitions T7; steps L8 |
| `rslib/src/scheduler/answering/learning.rs` | learning-step fuzz (quarter of the step, at most 300 s, an integer draw) | fuzz Z7 |
| `rslib/src/scheduler/answering/mod.rs` | fuzz seed and factor, the FSRS call and its elapsed days, short-term permission from the parameter list, the label inputs (learn-ahead limit, seconds until the next day) | fuzz Z1, Z7; transitions T2.3, T10; labels A1, A5 |
| `rslib/src/scheduler/answering/revlog.rs`, `review.rs`, `relearning.rs` | the review-log preview (interval, last interval, kind, difficulty-based factor) and which card fields each state writes | transitions T11, T14 |
| `rslib/src/scheduler/timespan.rs` | unit thresholds, rounding of the amount, the `<` prefix and the end label | labels A2, A3, A5 |
| `rslib/i18n/src/lib.rs`, `rslib/i18n/write_strings.rs` | number formatting of Fluent arguments (two-place rounding, trailing zeros dropped) and the isolation marks | labels A3, A6 |
| `ftl/core/scheduling.ftl` | the English suffixes and the end label | labels A4, A6 |
| `rslib/src/deckconfig/mod.rs`, `proto/anki/deck_config.proto` | read bounds (leech threshold 0 reads as 8), the parameter list order, reserved field numbers 7 and 8 | transitions T12, T13 |
| `rslib/src/config/mod.rs`, `rslib/src/config/bool.rs` | `collapseTime` default and `fsrsShortTermWithStepsEnabled` | labels A5; transitions T10 |
| `rslib/src/card/mod.rs`, `rslib/src/timestamp.rs` | `left` modulo 1000, ease as factor / 1000, difficulty shifted for the review log, whole-day arithmetic | steps L2; transitions T2.3, T14 |
| `pylib/anki/scheduler/v3.py`, `pylib/anki/lang.py` | how the Python layer gets the labels (`describe_next_states`) and strips isolation marks, for the oracle's `answer_labels` | observations O10 |

Black-box checks (the oracle image, 26.9.3 wheel): every number in the spec
tables comes from the goldens of `conformance/scenarios/sched/{steps,fsrs,fuzz}/`
or, for labels, from a run of the four `sched/labels` scenarios with a scratch
`answer_labels` observation (not committed) that read `describe_next_states`
and split the English text as O10 says.

Test links for the classified harvest rows are in `oracle/harvest/links.csv`
(rows of `scheduler/states/*`, `steps.rs`, `timespan.rs answer_buttons` and
`answering/mod.rs elapsed_secs`).
