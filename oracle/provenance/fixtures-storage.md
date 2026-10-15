# Storage fixtures: the maker in `oracle/fixtures/storage/` (U22)

Read to make the collection files of `specs/storage/schema-upgrade.md` U16 with
each release's own Python API (`make.py`); no code was copied into Meguri. The
old releases were read in their wheels as installed in the legacy images
(`anki` 2.1.26, 2.1.33, 2.1.40, 2.1.44 and `ankirspy` 2.1.26, 2.1.33), the
current one at tag 26.09.3.

| Anki file | Read for |
|---|---|
| `anki/collection.py` (2.1.26 to 2.1.44) | `close(save, downgrade=False)`, `changeSchedulerVer` / `upgrade_to_v2_scheduler`, `addNote` / `add_note`, `remNotes` / `remove_notes`, `getCard` |
| `anki/models.py` (2.1.26, 2.1.40) | `new`, `newField`/`new_field`, `addTemplate`/`add_template`, `add`, `rem`/`remove`; `_setID` waits for the clock to move (the reason for a running fake clock); `defaultModel` and `defaultField` legacy keys (`vers`, `tags`, `media`) |
| `anki/decks.py` (2.1.26, 2.1.40) | `id`, `add_config_returning_id`, `get_config`, `update_config`, `newDyn`/`new_filtered`, `rem`, `setDeck` |
| `anki/schedv2.py` (2.1.26, 2.1.40) | `answerCard`, `suspendCards`/`suspend_cards`, `buryCards`/`bury_cards`, `rebuildDyn`/`rebuild_filtered_deck`, `reset` |
| `anki/consts.py` | `DYN_ADDED`, `MODEL_CLOZE` |
| `pylib/anki/models.py`, `decks.py`, `scheduler/v3.py`, `collection.py` (26.09.3) | the same operations under their current names; v3 `answerCard` |
| `rslib/src/notetype/schema11.rs`, `rslib/src/decks/schema11.rs`, `rslib/src/deckconfig/schema11.rs` (26.09.3) | which keys a conversion keeps as `other` (hash-map JSON), and how a preset's `new`, `rev` and `lapse` unknown keys nest in it |
| `proto/anki/notetypes.proto`, `decks.proto`, `deck_config.proto` (26.09.3) | `other` is field 255 and the starting ease field 11 of the preset config, for the checks in `make.py` |

Black-box findings the recipes rely on (each checked on the made files):
each release writes the schema version its range says (14, 15, 16, 17); new
collections of 2.1.26 to 2.1.44 start on the v1 scheduler; 2.1.33 stores a
legacy `initialFactor` F as F / 10 and reads it back × 10, so its own Default
keeps 2.5 until the legacy API saves 2500; only the notetypes of 2.1.26 carry
two unknown keys (`tags`, `vers`) for 26.09.3's upgrade; 26.09.3's new
collections have `sched2021` and `creationOffset`, so opening them writes
nothing.
