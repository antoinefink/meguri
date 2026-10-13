# Oracle driver v0 (U3)

Read at tag 26.09.3 to choose the `anki` Python APIs the driver calls; no
code was copied.

| Anki file | Read for |
|---|---|
| `pylib/anki/scheduler/v3.py` | building and submitting a card answer (`answer_card`, `CardAnswer` fields) |
| `pylib/anki/scheduler/base.py` | `_timing_today` (days elapsed, next rollover) |
| `pylib/anki/models.py` | creating a note type with no stock fields or templates |
| `pylib/anki/decks.py` | preset creation and update through the schema-11 deck-config JSON |
| `pylib/anki/collection.py` | `fuzz_delta`, config setters |
| `proto/anki/scheduler.proto` | `SchedulingState` shape (oneof `kind`, optional memory state), `FuzzDelta` |
| `proto/anki/deck_config.proto` | typed deck-config field names used to verify the preset mapping |
| `proto/anki/config.proto`, `rslib/src/config/bool.rs` | collection config key names (`fsrs`, `loadBalancerEnabled`) |
| `rslib/src/deckconfig/schema11.rs` | schema-11 deck-config JSON key names |
| `rslib/src/scheduler/answering/mod.rs`, `rslib/src/scheduler/states/fuzz.rs` | which review count the fuzz seed uses, and that `ANKI_TEST_MODE` disables fuzz |
| `rslib/src/timestamp.rs`, `rslib/src/lib.rs` | `ANKI_TEST_MODE` also shifts the clock near rollover |
