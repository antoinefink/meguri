# Oracle driver: live-chain segments and handoffs (U4)

Read at tag 26.09.3 to choose the `anki` Python APIs that resume a handed-over
collection by the scenario's names; no code was copied.

| Anki file | Read for |
|---|---|
| `pylib/anki/models.py` | `id_for_name` (note type id by name) |
| `pylib/anki/decks.py` | `id_for_name` (deck id by name), `all_config` (presets with their names and ids) |
| `pylib/anki/collection.py` | `close(downgrade=False)` before the session directory is copied |
