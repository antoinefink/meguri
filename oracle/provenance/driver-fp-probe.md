# Oracle driver: fsrs_next, fsrs_context, the native lane (U14)

Read at tag 26.09.3 (`refs/anki-26.09.3`) and in the `anki` 26.9.3 wheel to
realize `specs/conformance/float-determinism.md` F5.4 and F6 and the
`fsrs_context` member of `conformance/schema/protocol.json`
(`driver/observe/fsrs_next.py`, `driver/observe/sched.py`,
`driver/fsrs_model.py`, `driver/meta.py`, `driver/timecontrol.py`,
`macos/run-native.sh`); no code was copied. The rules themselves are the spec
writer's (`float-determinism.md` in this directory).

| Anki file | Read for |
|---|---|
| `rslib/src/scheduler/answering/mod.rs` | `get_scheduling_states` reads the stored card; the FSRS states use the home deck's preset, its effective desired retention, and whole days from the card data's `lrt` to `next_day_at` (so the driver places `lrt` from `next_day_at`) |
| `rslib/src/storage/card/data.rs` | card data keys `s`, `d`, `lrt` (seconds) and how they are parsed |
| `rslib/src/config/bool.rs` | the config keys `fsrs`, `loadBalancerEnabled`, `fsrsShortTermWithStepsEnabled` and their defaults |
| `rslib/src/deckconfig/mod.rs` | the parameter list fallback order (`fsrs_params_6`, `_5`, `_4`) behind the schema-11 keys |
| `rslib/src/decks/mod.rs`, `rslib/src/decks/schema11.rs` | a deck's own desired retention wins over its preset's; the schema-11 deck value is a whole percent |
| `rslib/src/scheduler/fsrs/memory_state.rs` | the decay constants for empty and pre-FSRS-6 lists (`fsrs_context`; the 21-value clamp is the spec's, from fsrs 6.6.2) |
| wheel `anki/_backend_generated.py`, `anki/decks_pb2.pyi`, `anki/deck_config_pb2.pyi` | `get_scheduling_states`, `sched_timing_today`, `get_deck`, `get_deck_config` and the optional `desired_retention` of a normal deck |

Probed natively and in the image: libfaketime 0.9.13 built from source for
arm64/arm64e and injected with `DYLD_INSERT_LIBRARIES` into python-build-standalone
CPython 3.12.15 moves Python's clock, SQLite's `unixepoch()`, a new
collection's `crt` and the scheduler day; `time.sleep` stays real with
`DONT_FAKE_MONOTONIC`, but a Python timed lock wait computes its deadline from
the faked wall clock and hangs (libfaketime does not wrap
`pthread_cond_timedwait` on macOS). Not Anki code: libfaketime's
`src/Makefile.OSX` and `src/libfaketime.c` (sleep and wait wrappers) were read
for that.

Cross-check (F3.5): input 4 of `fp-probe/next-states-edges` gives the next
states of `conformance/goldens/smoke/fsrs-new-card.json` step 6 on both lanes.
