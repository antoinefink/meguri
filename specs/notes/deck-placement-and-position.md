# Deck placement and position of new cards

Spec: notes/deck-placement-and-position
Requirements: R791, R792, R793, R794, R795; AE31, AE32
Anki: 26.09.3
Scenarios: conformance/scenarios/cardgen/placement/, conformance/scenarios/cardgen/position/, conformance/scenarios/cardgen/normal/edits-only-add.yaml

Which deck a card gets when `notes/card-generation` creates it, and its position in the new queue. The terms "standard", "cloze", "the card of G6" and the generation events are those of `notes/card-generation`.

## D1. Usable decks

A deck id is usable when a deck with that id exists and is a normal deck (its `kind` is not filtered, `model/protobuf-records`). The Default deck, id 1, is always usable: U28 keeps it. If it is not (a damaged collection), the operation that generates fails and writes nothing.

## D2. Cards of a new note (R791, R793, R794)

Each new card goes to the first usable deck of:

1. its template's deck override, when the note type is standard and the override is set (the template's `target_deck_id` is greater than 0; schema-11 `did`), except for the card of G6, which has none;
2. the deck the caller gave (`notes/note-records` A1);
3. the Default deck.

Cloze note types have no override: a `target_deck_id` stored on a cloze note type's template is ignored (R793).

| Override | Caller's deck | Card goes to |
|---|---|---|
| Target (normal) | Culture | Target |
| none | Culture | Culture |
| a deleted deck | Culture | Culture (AE32) |
| Cram (filtered) | Culture | Culture |
| none | Cram (filtered) | Default (AE32) |
| none | a deleted deck | Default |
| Target | Cram (filtered) | Target |
| Target, on template 0, for the card of G6 | Culture | Culture |
| Target, on a cloze note type | Culture | Culture |

Anki 26.09.3 does not walk the list: when the override is set it takes it if usable and otherwise the Default deck, without trying the caller's deck. So rows 3 and 4 go to Default there. Deviation `deck-placement-chain` (R791, AE32), scenario `cardgen/placement/new-note`.

## D3. Cards added to an existing note (R792, R793, R794)

When an update, Check Database or an import adds cards to a note that has cards, each new card goes to the first usable deck of:

1. its template's deck override, as in D2 item 1;
2. the note's shared home deck: a card's home deck is its `odid` when that is not 0 (a card in a filtered deck), otherwise its `did`; when every existing card of the note has the same home deck, that deck;
3. the note type's last deck: the collection config key `_nt_<note type id>_lastDeck` read as a deck id (`storage/config-table` C3: missing or not an integer means no candidate);
4. the Default deck.

| Situation (last deck Other) | Card goes to |
|---|---|
| siblings all in Inbox | Inbox |
| a sibling in Inbox, a sibling in Elsewhere | Other |
| the only sibling in the filtered deck Cram, home deck Inbox | Inbox |
| override Target | Target |
| override names a deleted deck, siblings in Inbox | Inbox |
| the siblings' shared deck no longer exists | Other |
| siblings split, last deck deleted or filtered, or no last-deck key | Default |
| cloze, siblings in Inbox, override Target | Inbox |
| cloze, siblings split | Other |

Anki 26.09.3 takes item 1 when the override is set and otherwise item 2 when the siblings share a home deck, and goes to Default when the deck it took is not usable; only when neither is set does it try item 3. So the fifth and sixth rows go to Default there. Deviation `deck-placement-chain-existing-note` (R792), scenario `cardgen/placement/existing-note`.

AE31 (scenario `cardgen/normal/edits-only-add`): card 1 is in German::Mining and the note type's last deck is Other; the card that typing into Add Reverse creates goes to German::Mining.

## D4. Note type regeneration

`notes/card-generation` G9 places cards by D3's list (R792 covers template changes). Anki 26.09.3 leaves out item 3 here: a card with no set override and no shared home deck goes to Default, as does a card whose item 1 or 2 is not usable. U34 records the deviation with the scenarios that show it.

## D5. Filtered decks (R794)

D1 makes every filtered deck unusable, so no new card is ever placed in one. A new card of a note whose cards sit in a filtered deck goes to their home deck (D3 item 2).

## P1. The position counter

The collection config key `nextPos` holds the next new-card position.

- It is read as an unsigned 32-bit integer: a JSON integer from 0 to 4294967295. Anything else reads as 0: a missing key, a negative or larger integer, a float (`7.0`), a string.
- A new collection holds 1 (written at creation, U28).
- Writing it stores the JSON integer (p + 1) modulo 2^32, as a local change (`storage/config-table` C4): after 4294967295 comes 0.

| Stored | Read | Written after one note |
|---|---|---|
| (missing) | 0 | 1 |
| 1 | 1 | 2 |
| 4294967295 | 4294967295 | 0 |
| 2147483648 | 2147483648 | 2147483649 |
| 4294967296, -3, 7.0, `"12"` | 0 | 1 |

## P2. Positions of one note's new cards (R795)

When a generation (one added note, one update, one note of Check Database or of G9) creates cards for a note:

1. If the note has an existing card of type 0 (new, whatever its queue: suspended and buried new cards count), every new card takes that card's position: its `odue` when `odue` is not 0, otherwise its `due`, and 0 when that value is negative. When several existing cards are new, the one with the lowest card id gives the position. The counter is neither read nor written.
2. Otherwise the counter is read once, giving p, and written once (P1). Each new card takes p, or R(p) (P4) when the preset of the deck it goes to inserts new cards in random order. Cards of one note that go to decks with different presets can get different positions; cards that all go to random decks share R(p).

A generation that creates no card does not touch the counter. A bulk add draws once per note.

| Note | Positions |
|---|---|
| added with two cards when the counter is 1 | both 1; counter 2 |
| card 1 new at position 5, card 2 added | 5; counter unchanged |
| card 1 reviewed (type 1), card 2 added, counter 5 | 5; counter 6 |
| card 1 new with `due` -5, card 2 added | 0 |
| card 1 new and suspended at position 4, card 2 added | 4 |
| card 1 in a filtered deck, `odue` 3, card 2 added | 3 |

## P3. Storing a position

`cards.due` holds a signed 32-bit value (`model/row-records#R4`). A position from 0 to 2147483647 is stored as it is; one from 2147483648 to 4294967295 is stored as the position minus 4294967296, so 4294967295 is stored as -1 and 2147483648 as -2147483648. P2 item 1 reads `due` and `odue` as stored and only raises negative values to 0.

## P4. Random insertion order

The preset of a deck is the one its deck config id names, or the default preset (id 1) when that one does not exist. It inserts new cards in random order when its `new_card_insert_order` is `NEW_CARD_INSERT_ORDER_RANDOM` (protobuf value 1; schema-11 `new.order` 0).

R(p) is a value drawn from a pseudo-random generator seeded with p, so it depends on p alone:

- the generator is `StdRng` of the `rand` crate 0.9.4 (ChaCha12 from `rand_chacha` 0.9.0), created with `SeedableRng::seed_from_u64(p)` (p as an unsigned 64-bit value);
- R(p) is its first draw of `Rng::random_range(1..m)` for the type `u32`, with m = max(p, 1000), so 1 ≤ R(p) ≤ m - 1.

Meguri pins `rand` at 0.9.4 (`core/Cargo.toml`), so it computes the same values. The seed comes from collection data, not from the randomness seam (R1875).

| p | R(p) | Stored `due` |
|---|---|---|
| 0 | 801 | 801 |
| 1 | 825 | 825 |
| 2 | 82 | 82 |
| 3 | 647 | 647 |
| 998 | 779 | 779 |
| 999 | 574 | 574 |
| 1000 | 998 | 998 |
| 1001 | 661 | 661 |
| 5000 | 4542 | 4542 |
| 123456789 | 33597438 | 33597438 |
| 4294967295 | 2331729548 | -1963237748 |

The values come from `cardgen/position/random`'s golden; U27's Rust tests check this table.

## S1. Scenarios

| Scenario | Covers | Golden |
|---|---|---|
| `cardgen/placement/new-note` | D1, D2, AE32, A4 of `notes/note-records` | recorded; deviation `deck-placement-chain` |
| `cardgen/placement/existing-note` | D1, D3, D5, P2 item 1 | recorded; deviation `deck-placement-chain-existing-note` |
| `cardgen/normal/edits-only-add` | D3, P2, AE31 | recorded |
| `cardgen/position/counter` | P1, P2, P3 | recorded |
| `cardgen/position/random` | P2, P4 | recorded |

## S2. Differences from the Product Contract's wording

- R791 and R792 describe a list of fallbacks, which Meguri walks; Anki 26.09.3 falls back to Default once its first pick is not usable (D2, D3: deviations).
- R792 includes the note type's last deck for template changes; Anki 26.09.3 does not use it there (D4).
- R791's "each new card" excludes the card of R787, which ignores template 0's override (D2).
