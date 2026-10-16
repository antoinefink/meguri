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
| decks D9 | `rslib/src/scheduler/filtered/mod.rs`, `rslib/src/scheduler/filtered/card.rs`, `rslib/src/scheduler/timing.rs` | returning cards home, queue from type, timestamp threshold |
| decks D10 | `rslib/src/decks/tree.rs`, `qt/aqt/deckbrowser.py` | collapse by scope through the deck update path, skip-undo |
| decks D11 | `rslib/src/decks/current.rs`, `rslib/src/config/mod.rs` | `curDeck` write and fallback; `activeDecks` written nowhere after creation |
| decks D12 | `rslib/src/decks/tree.rs`, `rslib/src/storage/deck/mod.rs` | name order, default deck hiding, missing parents |
| decks D14 | `rslib/src/ops.rs`, `rslib/src/undo/mod.rs` | undo step names, skip-undo keeping the history |
| deck-presets P1 to P3 | `rslib/src/deckconfig/mod.rs`, `rslib/src/storage/deckconfig/mod.rs` and `add.sql` | defaults, id allocation, fallback to preset 1 and to the defaults, read-time range checks |
| deck-presets P4 to P7 | `ts/routes/deck-options/lib.ts`, `ConfigSelector.svelte`, `DateInput.svelte`, `rslib/src/deckconfig/update.rs`; fsrs-rs 6.6.2 `src/inference.rs` | defaults with FSRS-6 parameters, clone source, naming rule with the Unix-seconds suffix, trimming of Add and Clone only |
| deck-presets P8, P9, P12 | `rslib/src/deckconfig/update.rs`, `rslib/src/deckconfig/mod.rs`, `pylib/anki/decks.py` | removal as a schema change, reassignment of decks with removed or missing presets, apply-to-children, the save's extra writes |
| new-collection B2 to B8 | `rslib/src/storage/sqlite.rs`, `rslib/src/storage/schema11.sql`, `rslib/src/config/schema11.rs`, `rslib/src/storage/deck/mod.rs`, `rslib/src/storage/deckconfig/mod.rs`, `rslib/src/notetype/stock.rs`, `rslib/src/storage/upgrades/mod.rs` | creation path, initial config object, default deck and preset, stock note types and `curModel`; values recorded in the goldens of `storage/new-collection/` |
