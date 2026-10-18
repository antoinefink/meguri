# Oracle driver: deck options save and ignore-date count (U46)

Read at tag 26.09.3 to choose the backend calls behind `save_deck_options` and
`ignored_before_count` (`driver/ops/deck_options.py`; meanings in
`specs/conformance/deck-options-ops.md`); no code was copied. The specs' own
sources are in `sched-memory.md`.

| Anki file | Read for |
|---|---|
| `proto/anki/deck_config.proto` | the screen state (`DeckConfigsForUpdate`: configs, current deck and its limits, the collection-wide values), the save request's fields, the count request |
| `rslib/src/deckconfig/update.rs` | the screen state pre-fills empty FSRS-6 parameters from older lists; the save writes the given configs (date clamp, parameter check), points the target deck at the last config and writes its limits from the request, then recomputes and writes the collection-wide keys; an unchanged config is not stamped (`rslib/src/deckconfig/mod.rs`) |
| `ts/routes/deck-options/lib.ts` | the screen's save sends the current preset last (even unmodified), the deck limits as loaded, the collection-wide values as loaded; the preset name escaped for searches (`\` and `"`) |
| `ts/routes/deck-options/AdvancedOptions.svelte` | the count's search: the preset's `paramSearch` when not empty, else the escaped preset search without suspended cards |
| `rslib/src/deckconfig/service.rs`, `rslib/src/scheduler/fsrs/params.rs` | the count parses the date first (an invalid-input error), then searches with `-is:new` |
| `pylib/anki/_backend.py`, the wheel's generated backend | keyword arguments of the generated methods; `InvalidInput` is the class pylib raises for the backend's invalid-input kind |
| `rslib/src/decks/schema11.rs`, `rslib/src/deckconfig/schema11.rs` | the legacy JSON keys the driver self-tests read back (`desiredRetention` in percent, `sm2Retention`, `ignoreRevlogsBeforeDate`, `weightSearch`) |

Probed in the oracle image (`driver/tests/test_deck_options.py`):

- The first computation of the day on a new collection writes `rollover` 4
  (`rslib/src/scheduler/mod.rs`); a save on such a collection does it too.
  The self-test sets `rollover` in its setup so that config rows compare.
- Anki's `preset:` search compares the name as written, without unescaping
  (`rslib/src/search/parser.rs`, `sqlwriter.rs`), so the screen's count for a
  preset whose name holds `"` or `\` finds no card (0 of 0). The driver keeps
  the screen's escaping; the spec writer has the finding.
