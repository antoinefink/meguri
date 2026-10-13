# Oracle driver: model ops (U21)

Read to choose the `anki` Python APIs behind `legacy_json`, `legacy_update`
and `suspend` (`driver/ops/model.py`, `driver/ops/sched.py`; meanings in
`specs/conformance/model-ops.md`), in the oracle image's `anki` 26.9.3 wheel;
no code was copied. The save paths and the reopen come from the spec writer's
reading in `model.md` (model-ops L1-L3 rows). `db_exec` (`driver/ops/storage.py`)
is U23's op, from `driver-records.md`.

| Anki file (wheel) | Read for |
|---|---|
| `anki/_backend_generated.py` | `get_notetype_legacy`, `get_deck_legacy`, `get_deck_config_legacy` return the JSON bytes; keyword names of `add_or_update_notetype`, `add_or_update_deck_legacy`, `add_or_update_deck_config_legacy`; each generated method only builds the request and calls `self._run_command` |
| `anki/_backend.py` | `_run_command` turns the bridge's error bytes into a `BackendError` message and maps it with `backend_exception_to_pylib`, which gives JsonError, ProtoError and unmapped kinds the plain `BackendError` class |
| `anki/scheduler/base.py` | `suspend_cards` is `bury_or_suspend_cards` with mode SUSPEND |

Probed in the oracle image (`driver/tests/test_model.py` and one-off runs):
closing and reopening a collection leaves the `col`, `decks`, `deck_config` and
`notetypes` rows unchanged; a notetype with no fields or templates, or an empty
name, is `InvalidInput` (a run error, not the rejection); a patched `id` that
matches no record is saved under that id (scenarios never patch `id`); the
legacy getters already saw a raw SQL rename without a reopen in these probes.
