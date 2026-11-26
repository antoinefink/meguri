# Filtered study

The spec writer read the isolated 26.09.3 tag on 2026-11-26 for the following observable behaviors. The native surface in F6 is Meguri's design. No source or reference test text was transferred.

| Section | Read at the pinned tag | Observation |
|---|---|---|
| F1 | `rslib/src/decks/filtered.rs`, `qt/aqt/filtered_deck.py` | initial draft and secondary term |
| F2 | `rslib/src/scheduler/filtered/mod.rs`, `rslib/src/scheduler/filtered/card.rs`, `rslib/src/storage/card/filtered.rs` | membership, ordering, original fields and atomic rebuild |
| F3 | `rslib/src/scheduler/answering/current.rs`, `rslib/src/scheduler/answering/mod.rs`, `rslib/src/scheduler/states/review.rs`, `rslib/src/scheduler/states/rescheduling_filter.rs`, `rslib/src/scheduler/queue/builder/mod.rs` | effective due, log kind, home and retained transitions, queue ordering |
| F4 | `rslib/src/scheduler/states/preview_filter.rs`, `rslib/src/scheduler/answering/preview.rs`, `rslib/src/scheduler/answering/mod.rs` | four ratings, schedule preservation and timed repeats |
| F5 | `rslib/src/scheduler/filtered/custom_study.rs`, `rslib/src/decks/stats.rs`, `rslib/src/config/deck.rs`, `qt/aqt/customstudy.py` | saved searches, day adjustment, scope, tag defaults and collisions |
