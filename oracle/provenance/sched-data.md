# Scheduling data model (U42)

Spec-writer notes for `specs/sched/card-data.md`, `specs/sched/presets.md` and
`specs/sched/config-keys.md`. Read at tag 26.09.3 (`refs/anki-26.09.3`); no
code was copied. Every finding the specs rely on is repeated by a committed
scenario under `conformance/scenarios/sched/data/`, recorded with
`tools/mg oracle record`.

| Spec section | Anki files read | Read for |
|---|---|---|
| card-data K1, K2 | `rslib/src/card/mod.rs`, `rslib/src/scheduler/timing.rs` | type and queue numbers, which count a queue feeds, the timestamp threshold |
| card-data K3 | `rslib/src/scheduler/filtered/card.rs`, `rslib/src/scheduler/bury_and_suspend.rs` | queue from type and due; leaving a filtered deck (odue restore, negative queues kept) |
| card-data K4 | `rslib/src/card/mod.rs`, `rslib/src/scheduler/answering/review.rs`, `relearning.rs`, `learning.rs` | `left` modulo 1000, ease rounding on write, flags bits |
| card-data K5, K5.1 | `rslib/src/scheduler/answering/mod.rs`, `learning.rs`, `review.rs`, `relearning.rs`, `preview.rs`, `rslib/src/storage/card/data.rs`, `rslib/src/scheduler/new.rs`, `rslib/src/card/mod.rs`, `rslib/src/scheduler/fsrs/memory_state.rs` | which answer path sets dr, decay, lrt and the memory state; preview answers; custom data only from the answer request; Forget and Change Deck clearing |
| card-data K6 | `rslib/src/scheduler/answering/current.rs`, `rslib/src/scheduler/states/mod.rs`, `rslib/src/scheduler/reviews.rs`, `rslib/src/scheduler/new.rs` | the position an answer saves (due or original due, negative to 0), the position set due date saves (unsigned cast), restore on Forget |
| card-data K7, config-keys G2 (`nextPos`) | `rslib/src/config/mod.rs` | the counter's read and wrapping write |
| presets V1 | `rslib/src/scheduler/answering/mod.rs`, `rslib/src/deckconfig/mod.rs`, `rslib/src/decks/mod.rs` | the answer path's built-in defaults for a missing preset versus the default-preset fallback elsewhere; a filtered home deck refused |
| presets V2, V3 | `rslib/src/deckconfig/mod.rs`, `rslib/src/storage/deckconfig/mod.rs`, `rslib/src/sync/collection/changes.rs` | built-in defaults, the read-time bounds and where they apply (every storage read but the schema-16 upgrade, sync included) |
| presets V4 | `rslib/src/scheduler/answering/mod.rs`, `rslib/src/deckconfig/mod.rs` (enum reads go through the generated getters, which give the 0 variant for an unknown number) | unknown enum numbers acting as 0; values from `model/protobuf-records` P6.6 |
| presets V6 | `rslib/src/deckconfig/mod.rs`, `rslib/src/scheduler/fsrs/memory_state.rs`, `rslib/src/scheduler/answering/mod.rs` | list precedence, decay by length, short-term parameters check |
| presets V7 | `rslib/src/decks/mod.rs`, `rslib/src/scheduler/answering/mod.rs` | deck value over preset value, no bounds on the deck value |
| presets V8 | `rslib/src/deckconfig/mod.rs`, `rslib/src/deckconfig/update.rs` | writes start from the bounded read; equal configs are not written |
| presets D2, D3 | `rslib/src/decks/limits.rs`, `rslib/src/deckconfig/update.rs` | today-only, deck and preset limits; clearing a today-only limit |
| presets D4 | `rslib/src/decks/stats.rs`, `rslib/src/decks/counts.rs`, `rslib/src/storage/deck/mod.rs`, `rslib/src/scheduler/answering/mod.rs` | lazy reset, which queue feeds which counter, the capped time, the ancestor walk |
| config-keys G1, G2 | `rslib/src/config/mod.rs`, `bool.rs`, `number.rs`, `string.rs`, `rslib/src/preferences.rs`, `rslib/src/scheduler/new.rs`, `rslib/src/scheduler/queue/builder/mod.rs`, `rslib/src/deckconfig/update.rs` | key names, readers, defaults, writers |
