# Decks

Spec: decks/decks
Requirements: R1465, R322, R1474, R26, R27, R36, R37, R38, R39, R40, R41, R42, R53, R54, R55, R56, R57, R59, R60, R61, R62, R63, R64, R65, R66, R67, R68, R69, R1473, R1483, R2008
Anki: 26.09.3
Scenarios: conformance/scenarios/decks/

What a deck is, how its name is stored, matched and made unique, and what the deck operations do to the collection: create, rename, move (drag and drop), delete, collapse, select the current deck, and the deck tree. Presets are `decks/deck-presets`; the decks and preset of a new collection are `storage/new-collection`. The columns are `storage/schema-18` S2, the blobs `model/protobuf-records` P6.4 and P6.5, the legacy JSON form `model/schema11-json` S7 to S9 and S11. Stamping terms (`usn`, `now_ms`, `now_s`, `col.mod`, "stamped") are those of `storage/ids-usn-mtime`: a stamped deck gets `usn` -1 and `mtime_secs` `now_s`. Graves are `storage/graves`, config keys `storage/config-table`. Conformance ops and observations are `conformance/deck-ops`.

## D1. Records and names

- A deck is one row of `decks`: `id`, `name`, `mtime_secs`, `usn`, `common` (deck common) and `kind` (normal or filtered deck). Normal and filtered are the two members of the deck kind (P6.5).
- The stored name is the **native** form: the components of the hierarchy joined by U+001F. The **human** form joins them with `::` (R1465). Every name a user or caller gives is human; the core converts it to native with the component rule below before it compares or stores it. The human form of a stored name replaces each U+001F by `::`.
- **Component rule** (the effective result Anki stores; `model/schema11-json#S11` gives the same steps for legacy JSON):
  1. Split the human name at each `::`, left to right without overlap (`a:::b` gives `a` and `:b`).
  2. In each component, remove every ASCII control character (U+0000 to U+001F and U+007F), then normalize to Unicode NFC, then remove leading and trailing characters that are Unicode White_Space or `:`. A component left empty becomes `blank`.
  3. Join the components with U+001F.

  Anki normalizes to NFC before it removes control characters, and then normalizes every stored deck name again before it writes it (below), so the stored component is NFC even when removing a control character lets a combining mark compose: `e` U+0001 U+0301 is stored as `é` (U+00E9). The order above gives that result in one pass.
- **Stored-name normalization.** Whenever an operation writes a deck record (any op of this spec, the preset assignment of `decks/deck-presets`, and every other deck update), it first applies step 2 to each component of the stored native name (split at U+001F). A name that changes is written normalized and the deck is stamped. So a deck that arrived from another client as `Zed ` U+001F `kid ` is stored as `Zed` U+001F `kid` after it is collapsed (oracle).
- Emoji, mixed scripts and every other character the rule keeps round-trip byte for byte (R1465): `Café::日本語::😀 Emoji` is stored as the UTF-8 bytes of `Café`, U+001F, `日本語`, U+001F, `😀 Emoji`.
- Only these name changes happen: Meguri never trims the whole name, changes case, folds width or replaces characters.

| Typed (human) | Stored (U+001F shown as `␟`) |
|---|---|
| `A::B::C` | `A␟B␟C` |
| `  A ::  :B: ` + U+0007 + `x ::  ` | `A␟B: x␟blank` |
| `a:::b` | `a␟b` |
| `::a` | `blank␟a` |
| the empty string | `blank` |
| `e` U+0001 U+0301 | `é` |

## D2. Matching names and the unique-name rule

- Two deck names are the **same name** when they are equal under the `unicase` collation (`text/unicode-primitives#U2`), the collation of `decks.name`, whose unique index (`idx_decks_name`) forbids two decks with the same name. So `Straße` and `STRASSE` are the same name, and so are `ä` and `Ä`.
- Finding a deck by a typed name: convert it (D1), then look for a stored name that is the same name.
- **Unique-name rule** (R61). When a rename, a move or the reset of the default deck gives a deck a native name that another deck already has (same name, the deck itself not counting), it appends `+` to the end of the native name, so to the last component, and checks again, until no other deck has the name. The user sees the final name (R61).

| Other decks | Name given | Stored |
|---|---|---|
| `Spanish` | `spanish` | `spanish+` |
| `Spanish`, `spanish+` | `SPANISH` | `SPANISH++` |
| `a::bc` | `a::BC` | `a::BC+` |

## D3. Hierarchy

- The **parent** of a deck is the deck whose stored name is the same name (D2) as the deck's name without its last component. Ancestors follow the same way. A deck with one component is top-level.
- The **descendants** of deck `D` are the decks whose stored name starts with `D`'s stored name followed by U+001F, compared under `unicase`. Listed in `unicase` order of their names, they come in preorder (a deck right before its own descendants), since U+001F sorts before every character a stored name contains.
- The **nearest existing ancestor** of a native name is found by checking its parent name, then the grandparent name, and so on, and taking the first that names a deck (D2).
- **Case adoption** (R57). When a deck is created or renamed under an existing deck, its leading components take the stored spelling of its nearest existing ancestor: the new name is that ancestor's stored name followed by the remaining components as typed. Typing `a::b` when `A` exists gives `A::b`.
- **Missing ancestors** (R55). Every ancestor name that names no deck is created as a new deck (D4), the nearest first, up to the top level. Each one gets its id when it is created (`storage/ids-usn-mtime` I2). So creating `A::B::C` in a collection with none of them at instant T gives `A::B` id T, `A` id T + 1 and `A::B::C` id T + 2 (oracle).
- **Nesting limit.** When a name has 12 or more components and its 11 nearest ancestors (the names with 1 to 11 fewer components) all name no deck, an op that would create or rename a deck to it is refused with `too_deep` and writes nothing. Creating `1::2::…::12` in a collection with none of its ancestors is refused; `1::2::…::11` succeeds; `1::2::…::12` succeeds when `1` exists. (Anki refuses these with an invalid-input error.)
- **Filtered ancestors** (R62). When the nearest existing ancestor of the name is a filtered deck, creating or renaming a deck to that name is refused with `filtered_parent`, writing nothing. Only the nearest existing ancestor counts.

## D4. A new deck

Every deck an operation of this spec creates, typed or a missing ancestor, is a normal deck with:

| Part | Value | Bytes |
|---|---|---|
| deck common | `study_collapsed` true, `browser_collapsed` true, every other field default | `08 01 10 01` |
| deck kind | normal deck with `config_id` 1, every other field default and the optional ones absent | `0a 02 08 01` |
| `id` | `storage/ids-usn-mtime` I2 (and I3's cap) | |
| `usn`, `mtime_secs` | stamped | |

New decks start collapsed in both scopes, so a deck shows its subdecks only once the user expands it. The default deck of a new collection has the same blobs (`storage/new-collection` B6).

## D5. Create a deck (R55, R57)

Input: a typed name. The UI disables its confirm button for an empty or blank name (R58); the core does not refuse it and stores `blank` (D1).

1. Convert the name (D1).
2. When a deck already has the same name (D2), the result is that deck: nothing is written, no undo step is recorded, and the deck keeps its stored spelling (R57).
3. Otherwise apply D3's refusals (`filtered_parent`, `too_deep`), then case adoption, then create the missing ancestors and finally the deck itself (D4). The deck is the last row created.
4. The operation is one undoable step named "Add Deck". It returns the deck's id and stored name.

| Decks before | Typed | Result (oracle) |
|---|---|---|
| none but `Default` | `A::B::C` | adds `A::B`, `A`, `A::B::C`, in that order |
| `A`, `A::B`, `A::B::C` | `a::b` | `A::B`, nothing written |
| `A`, `A::B` | `a::x::y` | adds `A::x`, then `A::x::y` |
| `A` | `  a ::  c ` | adds `A::c` |
| `F` (filtered) | `F::sub` | refused, `filtered_parent` |

## D6. Rename a deck (R59, R60, R61, R62)

Input: a deck and a typed full name (the UI pre-fills the current full name, R60; "New Subdeck" pre-fills `<full name>::`, R59, and is a create, D5).

1. Convert the new name (D1).
2. Apply the unique-name rule (D2) against the other decks.
3. When the result is the deck's stored name byte for byte, the rename changes nothing: nothing is written.
4. Refusals, each writing nothing:
   - the new name's parent path (all its components but the last) begins with all the components of one of the deck's current descendants, compared component by component under `unicase`, so that the deck would land inside its own subtree: `under_descendant` (Meguri only, D15.1). Renaming `A` to `A::B::A` or `a::b::z::A` while `A::B` exists is refused; renaming `A` to `A::X` or `A::Z::A` is not (the new parent path starts with the deck itself, which is not its own descendant, and `A::Z` does not exist yet);
   - `filtered_parent` and `too_deep` (D3), for the nearest existing ancestor of the new name found while the deck still has its old name (that ancestor can be the deck itself).
5. Case adoption (D3) from that nearest existing ancestor, then creation of the missing ancestors, nearest first.
6. Every descendant of the deck by its old name, found after step 5 (so an ancestor that step 5 created under the old name is one of them), gets the deck's new name in place of the old one at the start of its name, the rest unchanged; each is stamped. The unique-name rule is not applied to them.
7. The deck gets its new name and is stamped.
8. Every ancestor of the new name that names no deck after steps 6 and 7 is created, nearest first. This happens when a deck moves under its own old name: renaming `A` to `A::X` creates a new, empty `A`.
9. The operation is one undoable step named "Rename Deck" and returns the stored name.

| Decks before | Rename | Decks after (oracle) |
|---|---|---|
| `A`, `A::B` | `A` to `X::A` | `X` (new), `X::A`, `X::A::B` |
| `A`, `A::B` | `A` to `A::X` | `A` (new, empty), `A::X`, `A::X::B` |
| `A`, `A::B` | `A` to `A::Z::A` | step 5 creates `A::Z` under the old `A`, which then moves with the subtree: `A::Z::A`, `A::Z::A::B`, `A::Z::A::Z`, and step 8 creates new `A::Z` and `A` |
| `a`, `a::bc`, `a::x` | `a::x` to `A::bc` | `a::bc+` (D2, then case adoption) |
| `a`, `a::b` | `a` to `Ä` | `Ä`, `Ä::b` |
| `a`, `a::b` | `a` to `A` | `A`, `A::b`: a change of case only renames the subtree too |
| `A`, `A::B` | `A` to `A::B::A` | Meguri: refused, `under_descendant`. Anki: `A::B::A`, `A::B::A::B`, and new `A::B` and `A` |
| `A`, `F` (filtered) | `A` to `F::A` | refused, `filtered_parent` |

## D7. Move decks (drag and drop, R63, R64, R65)

Input: a list of decks, in order, and a target: a deck, or the top level (a drop on the "Decks" header or the top drop zone, R63).

1. When the target is a filtered deck, the whole operation is refused with `filtered_parent` and writes nothing. When the target deck does not exist (any more), Meguri refuses with `NotFound`; Anki moves the decks to the top level then. No UI path gives a missing target.
2. Each listed deck, in order, with the names as the earlier moves of the same operation left them (a listed deck that no longer exists is skipped):
   1. Its new name is the target's stored name, U+001F and the deck's last component; for the top level, its last component alone.
   2. It is skipped, unchanged and not counted, when the target is the deck itself or one of its descendants (D3), and when the new name equals its stored name byte for byte (it is already there).
   3. Otherwise the unique-name rule (D2) applies, its descendants are renamed as D6 step 6, and the deck gets its new name; all are stamped.

   No conversion (D1), case adoption or ancestor creation happens: the target exists and the moved names are stored names.
3. The operation is one undoable step named "Rename Deck" (Anki's name for moving) and returns the number of listed decks that moved; their descendants are not counted (R64's toast "N decks moved").

| Decks before | Move | Decks after (oracle) | Count |
|---|---|---|---|
| `a`, `x`, `x::y` | `x` and then `x::y` onto `a` | `a`, `a::x`, `a::y` (the listed subdeck moves on its own) | 2 |
| `a`, `a::b` | `a::b` onto `a` | unchanged | 0 |
| `a`, `ab` | `a` onto `ab` | `ab`, `ab::a` | 1 |
| `a`, `a::b` | `a::b` to the top level | `a`, `b` | 1 |
| `b`, `x`, `x::b` | `x::b` to the top level | `b`, `x`, `b+` | 1 |
| `a`, `a::b`, `a::bc` | `a::b` onto `a::bc` | Meguri: `a::bc::b`, count 1. Anki: unchanged, count 0 (D15.2) | |

## D8. Delete decks (R54, R66, R67, R68, R2008)

Input: a list of decks.

1. Each listed deck that still exists, in order, is processed together with its descendants (D3, found when it is reached), the deck first and then its descendants in `unicase` order of their names. For each processed deck:
   - **Normal deck.** Every card whose `did` is the deck, and every card whose `odid` is the deck (a card of this deck sitting in a filtered deck), is removed with the card removal of the notes area that also removes the notes it leaves empty (`notes/note-records` D2, U27): a type 0 grave per card, and every note left without cards removed with its type 1 grave (`storage/graves` G3). Deleting a deck writes no line to the deleted-notes log (`notes/deleted-notes-log`; only a note deletion does).
   - **Filtered deck.** Its cards return to their home decks (D9); none is removed.
   - Every config key whose name starts with `_deck_<id>_` (the deck's id in decimal) and has at least one more character is removed, with no grave (`storage/config-table` C10). A key named exactly `_deck_<id>_` stays (oracle).
   - **The default deck** (id 1) is never removed (R68): it gets the name `Default` (with the unique-name rule, so `Default+` when another deck has the name `Default`), becomes top-level if it was nested, and is stamped, even when its name was already `Default`. Its cards and its key family go as for any normal deck.
   - **Any other deck**: its row is removed and a type 2 grave is added (`usn` -1).
2. The operation is one undoable step named "Delete Deck". It returns the number of cards removed (cards returned home are not counted) and, for Meguri's UI, the number of decks processed. R66's toast is "Deleted <deck> (N cards)".
3. Nothing else changes. `curDeck`, `_nt_<id>_lastDeck` keys and template deck overrides that name a deleted deck keep their values; their readers fall back (D11).
4. **Backup** (R2008). When the operation will remove at least one card, Meguri makes U127's pre-operation backup before it commits.
5. **Deletion summary** (R67). A read-only query on a list of decks returns `decks`, the number of distinct decks in the listed decks' subtrees (each listed deck with its descendants; the default deck counts when it is among them), and `cards`, the number of cards the deletion would remove (cards whose `did` or `odid` is a normal deck of those subtrees). The UI asks for confirmation when `cards` is at least 1, stating both numbers.

Example (oracle): `Home` with 2 cards, one of them pulled into the filtered deck `Filt`; `Home::Sub` with 2 cards; `Other` with 2 cards, one of them also in `Filt`. Deleting `Home` removes 4 cards and their 4 notes (one card each), with 4 type 0, 4 type 1 and 2 type 2 graves, and removes `_deck_<Home id>_lastNotetype`. The `Other` card stays in `Filt`. Deleting `Filt` then returns that card to `Other` and removes no card (count 0).

Example: `Default` with the subdeck `Default::Kid` and no cards: deleting `Default` keeps it, stamped, and removes `Default::Kid`; the count is 0.

## D9. Filtered decks (R53, R54, R1483)

- Meguri never creates or rebuilds a filtered deck; it keeps every filtered deck, its search terms and its cards' `odid` and `odue` as they are.
- A filtered deck can be renamed (D6), moved (D7), deleted (D8) and collapsed (D10). Nothing can be created, renamed or moved under it (D3, D7).
- **Returning a card home** (deleting a filtered deck): for each card whose `did` is the deck (or, for a damaged collection, whose `odid` is the deck):
  - when `odid` is 0 its `did`, `due`, `odue` and `queue` stay as they are (it then names a missing deck, which Check Database repairs, U128);
  - otherwise `did` takes `odid`, `odid` becomes 0, `due` takes `odue` when `odue` is not 0, `odue` becomes 0, and when `queue` is 0 or more it is set from `type`: 0 (new) gives 0, 2 (review) gives 2, 1 or 3 (learning, relearning) give 1 when the new `due` is above 1,000,000,000 (a timestamp) and 3 otherwise. A negative `queue` (suspended, buried) stays;
  - in both cases the card is stamped (`usn` -1, `mod` `now_s`), even when nothing else in its row changed.

## D10. Collapse and expand (R36, R37, R38, R69)

Input: a list of decks, a scope and a state. The study scope sets `study_collapsed` (Home's tree and the sidebar's deck tree, R36), the browser scope `browser_collapsed` (the browser sidebar, R37); Home never changes `browser_collapsed`.

- Each listed deck that exists gets the flag; nothing else in its record changes, apart from D1's stored-name normalization and the unique-name rule that every deck write applies.
- A deck already in that state is not written.
- A written deck is stamped, so the change syncs (R69).
- ⌥-click (R38) passes the deck and all its descendants.
- Meguri records the whole list as one undoable step named "Expand/Collapse". Anki records no step and keeps its undo history (D15.3).

## D11. The current deck (R40, R41, R42, R1474)

- The config key `curDeck` holds the current deck's id as a JSON integer.
- Selecting a deck writes `curDeck` as `storage/config-table` C4 says: nothing when it already holds that id; otherwise `usn` -1, `mtime_secs` `now_s`, and `col.mod` `now_ms`, so the collection counts as changed for sync (R1474).
- The write is skip-undo (`engine/undo`): it is never recorded and never empties the undo history (R42). Anki records it as a "Select Deck" step (D15.4).
- Reading the current deck gives the deck `curDeck` names when that deck exists, and the default deck (id 1) when the key is missing or unreadable (C3) or names no deck (a deleted deck, D8).
- Selecting a deck id that names no deck is refused with `NotFound` (Anki writes any id; no UI path gives one).
- Anki 26.09.3 writes only `curDeck` when a deck is selected. `activeDecks` keeps the value a new collection gave it (`[1]`, `storage/new-collection` B4); R41's `activeDecks` write is an open point for U152 and U175.

## D12. The deck tree (R36, R39, R53)

The tree that Home and the sidebars show (counts are U41's):

- One node per deck. A node's children are its child decks, in `unicase` order of their stored names (no two are equal).
- A node holds the deck's id, the last component of its name, whether it is filtered, and its two collapse flags.
- A deck whose parent deck does not exist is left out, with its descendants, until Check Database creates the missing parents (U128). (Anki places such a deck under the node that precedes it in name order when that node is one level up; D15.5.)
- The default deck is hidden when it is top-level, has no child decks, no card has `did` 1, and at least one other top-level deck exists (R39). Cards of the default deck sitting in a filtered deck do not count.

## D13. Other deck writes and what they keep (R322, R69)

- Every deck write, here and in other areas (description, deck limits, preset assignment), edits only the decoded fields it means to change. Everything else comes from what was read: `browser_collapsed`, `extend_new`, `extend_review`, `learning_studied` and the day counters, the description, `other` (legacy JSON keys, such as a shared deck's `sharedFrom`), and unknown protobuf fields (`model/protobuf-records` P4, P5, P8).
- The write path of every deck write: D1's stored-name normalization, D2's unique-name rule, then the write is skipped when the record equals the stored one (`storage/ids-usn-mtime` S4); otherwise the deck is stamped. When the write changes the name, D6 steps 5 to 8 apply.
- Example: renaming a deck that carries `browser_collapsed` true, `extend_new` 5, `extend_review` 7, a `sharedFrom` key and an unknown key in `other` keeps all of them; only `name`, `mtime_secs` and `usn` change.

## D14. Operations

| Operation | Undo step | Returns |
|---|---|---|
| create (D5) | "Add Deck" (none when the deck existed) | id, stored name |
| rename (D6) | "Rename Deck" | stored name |
| move (D7) | "Rename Deck" | moved count |
| delete (D8) | "Delete Deck" | removed cards, processed decks |
| collapse (D10) | "Expand/Collapse" (Meguri only) | |
| select (D11) | none (skip-undo) | |

- None of them is a schema change (`notetypes/schema-changes` G2).
- Each is one operation of the engine (U24): one transaction, one change set (decks added, changed and removed, cards changed and removed, notes removed, config keys, graves), and an undo step as listed (U25). An operation that changes nothing records no step but, like every operation, empties the redo history (`engine/undo` A2).
- Refusals write nothing and record no step: `filtered_parent`, `too_deep`, `under_descendant`, `NotFound`.

## D15. Differences from Anki

### D15.1 Renaming under a descendant (R62)

Anki renames a deck under one of its own descendants: renaming `A` to `A::B::A` while `A::B` exists gives `A::B::A`, `A::B::A::B` and new empty `A::B` and `A`. Meguri refuses (`under_descendant`, D6 step 4). Deviations `decks-rename-under-descendant-*`, scenario `decks/rename/under-descendant`.

### D15.2 The move rule (R62, R63)

Anki decides whether a drop is onto the dragged deck or its subtree by comparing stored names as byte strings: it skips a deck when the target's stored name starts with the deck's stored name and both have the same first component. So dropping `a::b` onto its sibling `a::bc` does nothing in Anki; Meguri moves it (D7 step 2.2 uses descendants, D3). Deviations `decks-move-prefix-sibling-*`, scenario `decks/move/prefix-sibling`. The same byte comparison lets Anki drop a deck onto its own descendant when the two stored names differ in case (possible only after a sync or import); Meguri skips that deck.

### D15.3 Collapse is an undo step

Anki's collapse records no undo step and keeps the history; Meguri's is an undoable "Expand/Collapse" step (plan, Open Questions). No scenario observes undo after a collapse; `core/crates/meguri-engine/tests/decks.rs` asserts Meguri's step.

### D15.4 Selecting a deck

Anki records "Select Deck"; Meguri records nothing and keeps the history (R42, `engine/undo`). Asserted by Meguri's tests, not by a scenario.

### D15.5 Not observable through the oracle

Refusing a missing move target (D7) or an unknown current deck id (D11), and leaving decks with a missing parent out of the tree (D12), differ from Anki only for ids or collections no UI path or consistent collection produces. No scenario shows them.

## D16. Scenarios

| Scenario | Covers |
|---|---|
| `decks/create/parents` | D3, D4, D5: missing ancestors with their ids, case adoption, an existing name returns the deck |
| `decks/create/names` | D1: normalization, emoji and mixed scripts round-trip byte for byte, the unique index under `unicase` |
| `decks/create/refused` | D3: `filtered_parent`, `too_deep` |
| `decks/rename/subtree` | D6: subtree moved, new parents, `+` rule, case adoption, case-only rename, stamps |
| `decks/rename/under-descendant` | D6 step 4, D15.1 (deviation) |
| `decks/rename/own-subtree` | D6 steps 5 to 8: a deck renamed under its own old name, an ancestor created under the old name moving with the subtree, the `+` rule against its own subdeck |
| `decks/rename/preserve` | D13, R322: legacy keys and `other` survive a rename |
| `decks/move/rules` | D7: target, top level, skips, count, `+` rule, filtered target refused |
| `decks/move/prefix-sibling` | D15.2 (deviation) |
| `decks/delete/subtree` | D8: cards and notes removed, graves, config keys, count, cards in a filtered deck |
| `decks/delete/default` | D8: the default deck stays, renamed `Default` (or `Default+`) and moved to the top level |
| `decks/delete/filtered` | D8, D9: cards return home with `odue` and their queue; no card removed |
| `decks/collapse/scopes` | D10: one flag per scope, stamps, no write when unchanged |
| `decks/current/select` | D11: `curDeck` written, no write when unchanged, `col.mod` |
| `decks/tree/structure` | D12: order, nesting, the default deck hidden and shown |
| `decks/tree/missing-parent` | D12: a deck whose parent is missing is left out |

Meguri's own behavior that no scenario shows is tested in `core/crates/meguri-engine/tests/decks.rs`: one undo restoring a rename, a move and a delete of a deck with 2 subdecks and 120 cards (rows, graves, config keys and `col.mod`); the change set of each operation; the deletion summary and the backup call (R67, R2008); the collapse step and the skip-undo selection (D15.3, D15.4); the `NotFound` refusals.

## D17. All deck names (Meguri query, R26, R27)

The Go to Deck query returns `DeckNameSnapshot { generation: UInt64, decks: [DeckNameEntry] }`, where each entry is `{ id: Int64, fullName: String, filtered: Bool }`.

- Return exactly one entry per persisted deck record: its actual id, full human name and whether its kind is filtered (D1). Include the actual, possibly renamed default deck even when D12 hides it; include filtered decks, descendants of collapsed decks, and records with missing parents independently. Do not create entries for absent records, infer missing ancestors or substitute a `Default` name. D12's display tree is unchanged.
- Obtain `fullName` with the existing `human_name` conversion: replace stored U+001F separators with `::` (D1). Preserve the stored components' spelling and bytes; do not apply the input component rule or stored-name normalization on this read. Order by the full stored name under `unicase` (D3), with ascending id as a stable tie-breaker. Human-name formatting does not change the sort key.
- Read all entries in one short snapshot on one Query reader (`storage/open-and-connections` C6–C7). Return the generation of the collection actually read, checked while holding that reader (`engine/lifecycle` L3), with the entries from that snapshot. End the snapshot and release the reader within the query call. The caller rejects publication if that generation is no longer current.
- Existing closed/unavailable collection, reader/storage error and `StaleGeneration{planned, current}` rules apply; propagate the error rather than returning an empty or partial successful snapshot. An empty array succeeds only when the snapshot has no deck records.
- This query performs no writer operation, day maintenance, count or card query, normalization or other write, undo change or event publication. It does not change deck creation, matching, selection or filtered-deck study eligibility.

Focused tests for this Meguri projection:

- A hidden, empty id 1 named `Personal`, a filtered record, collapsed descendants and a missing-parent record all appear once with their actual full names, ids and kinds. No missing parent is invented; when id 1 is absent, no default entry appears.
- Nested Unicode names retain their actual spelling and full paths; stored-name `unicase` order (including parent before descendants) and the id tie-breaker are deterministic. A read of an unnormalized stored name leaves it unchanged.
- A concurrent rename yields one coherent before-or-after snapshot. Replacement cannot relabel old entries with a new generation: a stale reader request fails, and an already completed old-generation result is rejected at publication.
- Closed/unavailable and storage-error cases fail without a partial value. A successful read changes no persisted state, undo history or events, and uses no writer, day-maintenance, count or card path.
