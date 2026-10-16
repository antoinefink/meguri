# Oracle driver: FSRS policy ops (U50)

Read at tag 26.09.3 (`refs/anki-26.09.3`) to choose the `anki` Python APIs
behind `enable_fsrs` and `enable_v3` (`driver/ops/fsrs_policy.py`; meanings in
`specs/conformance/fsrs-policy-ops.md` F2, F3). No code was copied. The spec's
own sources are in `sched-fsrs-policy.md`.

| Anki file | Read for |
|---|---|
| `qt/aqt/mediasrv.py` (`update_deck_configs`), `qt/aqt/operations/deck.py` | the desktop's deck options save is `col.decks.update_deck_configs` with the request the page sends, run as a collection op |
| `ts/routes/deck-options/lib.ts` (`dataForSaving`) | the request's fields: the selected preset last, the global options, the deck's limits, `fsrs` and `fsrs_reschedule` |
| `pylib/anki/decks.py` (`get_deck_configs_for_update`, `update_deck_configs`, `UpdateDeckConfigs`, `UpdateDeckConfigsMode`) | the Python entry points and request type |
| `rslib/src/deckconfig/update.rs` | what the screen's read returns (presets pre-filled from older lists, the current deck's preset id and limits, the global options) and what the save writes |
| `rslib/src/deckconfig/service.rs`, `rslib/src/deckconfig/mod.rs` (`get_deck_config`) | the backend's preset read returns the stored preset (preset 1 or the defaults when the id names none); an unchanged preset is not written |
| `rslib/src/config/undo.rs` (`set_config_undoable`), `rslib/src/collection/transact.rs` | a config write with unchanged bytes writes nothing; an op with no changes leaves `col.mod` |
| `rslib/src/scheduler/mod.rs` (`timing_today`) | the first day query of a collection without `rollover` writes it: why the driver tests observe `timing` first |
| `qt/aqt/deckbrowser.py` (`_confirm_upgrade`), `pylib/anki/collection.py` (`set_v3_scheduler`, `_load_scheduler`), `rslib/src/config/bool.rs` (`set_config_bool`) | the scheduler update: `sched2021` true without an undo entry, then the scheduler load, which writes `creationOffset` through the preferences only when it is missing; nothing when already on |

Probed in the oracle image (`driver/tests/test_fsrs_policy.py`): with FSRS on
the save writes nothing, `col.mod` included; a missing global key comes back
as a config row and a deck whose preset is gone moves to the Default deck's
preset, both caught by the F2 check; on a new collection the save also writes
`rollover` unless a day query ran before; the scheduler update with
`creationOffset` present writes only `sched2021` and `col.mod`.
