# Deck, preset and new-collection specs (U28)

Read at tag 26.09.3 (`refs/anki-26.09.3`) to write `specs/decks/decks.md`,
`specs/decks/deck-presets.md`, `specs/storage/new-collection.md` and
`specs/conformance/deck-ops.md`. No code was copied; the rules were
re-derived in the spec writer's words and checked by black-box probes of the
pinned `anki` wheel in the oracle image (2026-10-16): id order of created
parents, an existing name returned, case adoption, rename under a descendant,
the byte-prefix drop rule, a listed subdeck moving on its own, the `+` rule,
the default deck kept and stamped, the nesting limit (12 components refused,
11 accepted), cards in a filtered deck removed with their home deck, the
`_deck_<id>_` key that survives, a filtered deck's cards returned home,
stored-name normalization on a collapse write, `e` U+0001 U+0301 stored as
`é`, the tree's handling of a missing parent, the filtered-deck kind blob of
deck-ops K13, preset removal through the deck options save (decks reassigned,
collection-wide keys added), and the undo names.

| Spec section | Anki file | Read for |
|---|---|---|
| decks D1 | `rslib/src/decks/name.rs`, `rslib/src/decks/addupdate.rs` | component normalization, normalization of stored names on every deck write (applied twice in practice) |
| decks D2 | `rslib/src/decks/name.rs` (unique-name loop), `rslib/src/storage/deck/mod.rs` | `+` suffix, lookups through the `unicase` column |
| decks D3 | `rslib/src/decks/addupdate.rs`, `rslib/src/decks/name.rs`, `rslib/src/storage/deck/mod.rs` | nearest existing ancestor, case adoption, parents created nearest first, recursion limit 11, filtered ancestor refusal, descendant range query |
| decks D4 | `rslib/src/decks/mod.rs`, `rslib/src/decks/filtered.rs` | collapsed flags and preset 1 of a new deck |
| decks D5 | `pylib/anki/decks.py`, `qt/aqt/operations/deck.py`, `rslib/src/decks/service.rs` | create returns an existing deck by name |
| decks D6 | `rslib/src/decks/addupdate.rs`, `rslib/src/decks/name.rs`, `qt/aqt/deckbrowser.py` | rename steps and the final parent creation; no refusal of a rename under a descendant |
| decks D7 | `rslib/src/decks/reparent.rs`, `rslib/src/decks/name.rs` | drop rule (byte prefix and same root), skip when unchanged, no normalization or parent creation, count |
| decks D8 | `rslib/src/decks/remove.rs`, `rslib/src/storage/deck/cards_for_deck.sql`, `rslib/src/card/mod.rs`, `rslib/src/config/deck.rs`, `rslib/src/config/mod.rs`, `rslib/src/storage/config/mod.rs` | subtree order, cards by `did` or `odid`, default deck rename, aux config prefix removal (strictly greater than the prefix) |
| decks D8 (deleted-notes log) | `pylib/anki/collection.py`, `qt/aqt/main.py` | the desktop appends to its deleted-notes file only from the note-removal hook, which deck removal does not fire (2026-10-16, second session) |
| decks D9 | `rslib/src/scheduler/filtered/mod.rs`, `rslib/src/scheduler/filtered/card.rs`, `rslib/src/scheduler/timing.rs` | returning cards home, queue from type, timestamp threshold; every returned card is stamped through the card update path, also one whose `odid` is 0 (`rslib/src/card/mod.rs`) |
| decks D10 | `rslib/src/decks/tree.rs`, `qt/aqt/deckbrowser.py` | collapse by scope through the deck update path, skip-undo |
| decks D11 | `rslib/src/decks/current.rs`, `rslib/src/config/mod.rs` | `curDeck` write and fallback; `activeDecks` written nowhere after creation |
| decks D12 | `rslib/src/decks/tree.rs`, `rslib/src/storage/deck/mod.rs` | name order, default deck hiding, missing parents |
| decks D14 | `rslib/src/ops.rs`, `rslib/src/undo/mod.rs` | undo step names, skip-undo keeping the history |
| deck-presets P1 to P3 | `rslib/src/deckconfig/mod.rs`, `rslib/src/storage/deckconfig/mod.rs` and `add.sql` | defaults, id allocation, fallback to preset 1 and to the defaults, read-time range checks |
| deck-presets P4 to P7 | `ts/routes/deck-options/lib.ts`, `ConfigSelector.svelte`, `DateInput.svelte`, `rslib/src/deckconfig/update.rs`; fsrs-rs 6.6.2 `src/inference.rs` | defaults with FSRS-6 parameters, clone source, naming rule with the Unix-seconds suffix, trimming of Add and Clone only |
| deck-presets P8, P9, P12 | `rslib/src/deckconfig/update.rs`, `rslib/src/deckconfig/mod.rs`, `pylib/anki/decks.py` | removal as a schema change, reassignment of decks with removed or missing presets, apply-to-children, the save's extra writes |
| new-collection B2 to B8 | `rslib/src/storage/sqlite.rs`, `rslib/src/storage/schema11.sql`, `rslib/src/config/schema11.rs`, `rslib/src/storage/deck/mod.rs`, `rslib/src/storage/deckconfig/mod.rs`, `rslib/src/notetype/stock.rs`, `rslib/src/storage/upgrades/mod.rs` | creation path, initial config object, default deck and preset, stock note types and `curModel`; values recorded in the goldens of `storage/new-collection/` |
| new-collection B4 (`lastUnburied`) | `rslib/src/scheduler/bury_and_suspend.rs`, `rslib/src/scheduler/service/mod.rs` | the unbury check of the first day query does nothing on day 0 with the key missing, so a new collection gains no `lastUnburied` on its creation day |
| deck-ops K15 (2026-10-17) | `pylib/anki/decks.py` (`id`, `new_deck_legacy`, `add_deck_legacy`), `rslib/src/decks/service.rs`, `rslib/src/decks/addupdate.rs`; the oracle's `driver/ops/decks.py` setup section | setup decks: lookup by name first, else the legacy default deck added through the undoable add path (parents nearest first, D4's blobs); the preset through the legacy save, which skips an unchanged deck; the golden of `decks/collapse/scopes` for the ids and stamps |
| new-collection B11 (2026-10-17) | none; the plan's U28 Verification (amended 2026-10-16) and `specs/conformance/notetype-ops.md` T10.2 | the executor's new collection replaces the stand-in |

## Session s-20261117T152613Z-96466c16 (spec writer): deferred harvest rows, batch 2

Read at tag 26.09.3 (`refs/anki-26.09.3`) to classify deferred harvest rows, write their scenarios and draft spec text; no code, test text or fixture value was copied unless a row says otherwise, and every expected value comes from the recorded oracle goldens.

| Spec section and scenarios | Anki file | Read for |
|---|---|---|
| decks/presets/missing-reads-default (default-preset fallback for a deck whose preset is missing) | `rslib/src/deckconfig/service.rs`, `rslib/src/deckconfig/mod.rs`, `rslib/src/scheduler/reviews.rs`, `rslib/src/notetype/cardgen.rs`, `rslib/src/card/mod.rs`, `rslib/src/stats/card.rs` | Which behavior the getter tests target (an unknown id falls back to preset 1, then to the built-in defaults) and which collection paths read a deck's preset through that fallback. Those paths are Set Due Date's starting ease and the insertion order of new cards, which the scenario observes. |
| classification of the legacy-add and add-if-unique rows (covered by sync/normal/preset-added and package/apkg-import/presets) | `rslib/src/deckconfig/service.rs`, `rslib/src/deckconfig/mod.rs`, `rslib/src/storage/deckconfig/mod.rs`, `rslib/src/import_export/package/apkg/import/decks.rs`, `rslib/src/sync/collection/changes.rs` (call site only), `rslib/src/import_export/insert.rs` (call site only) | Which callers reach the legacy add and the add-if-unique paths: deck-package import uses add-if-unique, while sync and legacy saves write under the existing id. This confirmed that existing scenarios prove the obligations. |

## Audit fixes (2026-11-17, session s-20261117T172637Z-685ca4ee)

Read at tag 26.09.3 (`refs/anki-26.09.3`) to fix spec readiness audit findings; no code was copied, and every rule was restated in the spec writer's words.

| Subject | Anki file | Read for |
|---|---|---|
| decks/decks#D10 (one deck written per call in Anki; the browser scope is used by the filter sidebar's deck items) | `qt/aqt/browser/sidebar/tree.py` | Checked that the sidebar's deck items use the browser scope and toggle one deck per write |
