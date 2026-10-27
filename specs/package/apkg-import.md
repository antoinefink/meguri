# Deck package import

Spec: package/apkg-import
Requirements: R1595 (core side), R1596 (with U124), R1597, R1598, R1599 (the legacy and `.apkg` parts), R1600 (data; the sheet is U162), R1601, R1602, R1603, R1604, R1607, R1608, R1609, R1610, R1611, R1612, R1613, R1614, R1615, R1616 (core side), R2004, R2015; AE67; test-enforces R1846 and R1852 for import; the interface for R1605 and R1606 (U121)
Anki: 26.09.3
Scenarios: conformance/scenarios/package/apkg-import/ (none recorded yet; I18 lists them)

How Meguri imports a deck package (`.apkg`, U120) into the open collection: the session that reads and checks the package (I2 to I4), the options (I5), what is taken from the package (I6), note types without a merge (I7), the port that note type merging plugs into (I8, U121), notes (I9), decks and presets (I10), cards and review log (I11), media (I12), the import log (I13), and Meguri's operation with its preview, progress, cancellation, errors, temporary files and undo class (I14 to I16). The container, its limits and staging are `package/container` (P1 to P10, P12; the legacy formats P17, classification P18). The UI that drives it is U162 through U133. Importing a whole collection (`.colpkg`) is U124's (`package/container#P16`, `engine/collection-replace`).

Stamping terms (`usn` -1, `now_s`, `now_ms`, `col.mod`) are those of `storage/ids-usn-mtime`. "The package collection" is the staged copy of the collection inside the package (I3); "the target" is the open collection.

## I1. Scope and entry points

- Entry points (U162): the Import command, drag and drop, Dock and Finder opens, Open in / share sheet, Files. Each gives a file; P18 classifies it. `Apkg` goes to this import; `Colpkg` to U124; `Unsupported` is refused before anything is copied, with a message naming the accepted types (U162).
- The import adds to the target and never replaces it. It runs while a review session stays open (R52): it holds no exclusive phase (`engine/lifecycle#L9`), and its commit reaches the session as a change made elsewhere (`sched/review-session#C13`).
- Its apply needs an open, `Ready` collection (I14). Its inspect (I3) opens no connection to the target; it uses only the collection directory, for staging (I15).

## I2. The session (R1600)

An import is a session with three calls: `inspect(file)` (I3, I4), `apply(session, options)` (I14) and `close(session)`.

- **Source copy.** `inspect` first takes the owned source copy of `package/container#P16.1` (preflight P10's last row): one copy of the selected file into a fresh P9 staging directory of the session, under a fixed name of the implementer's choice (never one derived from the file or an entry). From then on every read goes to that copy. The selected file is never read again, never changed and never removed; U162's own temporary copy of a security-scoped file is U162's to remove.
- The session owns its staging directory: the source copy, the package collection (I3) and nothing else. `close`, dropping the session, and the engine closing remove it (P9). After a kill, U32's launch sweep removes it.
- A session commits at most once. An `apply` that failed or was cancelled before its commit leaves the session open for another `apply`; an `apply` after a commit or after `close` is refused with `SessionClosed`.
- The session belongs to the engine, not to a generation: a collection replaced after `inspect` does not invalidate it, since `apply` plans against the target as it is then (I14). A session whose engine closed is closed.

## I3. Inspect

Steps, in this order; the first failure ends the inspect with its error (I16) and removes the session's staging directory:

1. **Archive and format.** Read the archive (P2) and `meta` (P3). Version 3: the latest format (P4, P5). No `meta`, version 1 or version 2: the legacy formats (P17). Any other version: `TooNew`.
2. **Preflight** (I15) and **extract** the collection entry into the staging directory as `collection.anki2`, within P8's limits (P4 for the latest format, P17 for the legacy ones).
3. **Open gate** on that file, as `package/container#P12` step 3, with one change: the v1-scheduler marker (`storage/open-and-connections#C2.2` step 2) does not refuse; the file goes on and step 6 converts it. This is the only place Meguri accepts a v1-scheduler collection, and only as a package collection that is read, never activated. Every other refusal stays (`TooOld`, `TooNew`, `NotACollection`, `UpgradeFailed`, `Corrupt`); schema 11 to 17 is upgraded to 18 on this copy (`storage/schema-upgrade`).
4. **Integrity check**: `PRAGMA integrity_check` with `unicase` registered; anything but the single row `ok` is `Corrupt` (P12 step 4).
5. **Id repair** (I3.1).
6. **Scheduler conversion** (I3.2).
7. **Field count repair** (I3.3).
8. **Media map**: P5 (latest) or P17 (legacy), with P6's checks for the latest format. Every refusal is `Corrupt`.
9. **Preview facts** (I4) are read from the package collection as steps 5 to 7 left it.

Steps 5 to 7 write the package collection only, in one transaction. Nothing outside the staging directory changes during an inspect: the target, its `-wal`, the media folder, the media journal and the backups keep their bytes (R1599). Inspect takes no slot (`engine/lifecycle#L5`) and runs while the target is open and used.

### I3.1 Id repair (R1613)

The cutoff is `now_ms` + 86,400,000. In each of `notes`, `cards` and `revlog`, in that order, every row whose `id` is above the cutoff gets a new id; an id equal to the cutoff stays.

- The new ids of one table are consecutive integers, given in ascending order of the old ids, starting at the larger of `now_ms` and (the largest id of that table that is at most the cutoff) + 1, or at `now_ms` when no such id exists.
- A note's new id is written into its cards' `nid`; a card's into its review-log rows' `cid`.
- Example (`now_ms` T): notes with ids 5, T + 86,400,000 and T + 9 × 10^12: the last becomes T + 86,400,001, since the largest note id at or below the cutoff is T + 86,400,000. Cards with ids 7, T + 3 days and T + 4 days: the two become T and T + 1, in that order.
- Decided 2026-10-27: the start above `now_ms` keeps new ids unique in their table. Anki 26.9.3 starts the new ids at `now_ms` whatever valid ids lie between `now_ms` and the cutoff, so a package holding such an id and one above the cutoff fails to import there with a constraint error; Meguri imports it (I17).

### I3.2 Scheduler conversion (R1614)

Applies when the package collection's scheduler marker (C2.2 step 2) means v1 and at least one card has a `queue` other than 0. Otherwise nothing is converted (a v1 package whose cards are all in the new queue carries no scheduling to convert). In the package collection, in this order:

1. **Review log.** Every row with `type` 0 or 2 and `ease` 2 or 3 gets `ease` + 1.
2. **Cards.** Each card whose `did` names a filtered deck, or whose `type` is 2 or 3, is changed as follows, in this order (which cards, and n below, are decided from the rows as they were before this step):
   1. `type` 2 with `queue` 1 or 3 becomes `type` 3 (relearning).
   2. When the card's `odid` is not 0 and its `did` names a filtered deck, with that deck's `reschedule` and `delays`:
      - when `delays` is not empty: `left` becomes min(`left`, n), where n is the number of `relearn_steps` (a card of `type` 2 before step 2.1) or `learn_steps` (any other card) of the preset of the deck named by `odid` (preset 1 when that deck is missing or filtered; n = 0 when that preset is missing);
      - with `reschedule` on: a card in `queue` 0 whose `type` is not 0 takes the queue of `sched/card-data#K3` for its type and `due`;
      - with `reschedule` off: a card in `queue` 0 goes to `queue` 2; then a card of `type` 1 gets `queue` 4 and `type` 0.
3. **Config.** `schedVer` becomes the JSON integer `2`. When the package collection has no creation offset (`sched/day-clock#D2`), `creationOffset` is set to the device's offset at the instant `col.crt`, in minutes west.

Nothing else changes. The converted package collection is read as any other (I6); its day count follows `sched/day-clock#D3` once it has a creation offset. A v1 package left unconverted (all cards new) is read with `sched/day-clock#D4`'s legacy count: only deck today-limits could see the difference, and v1-era packages have none.

### I3.3 Field count repair (R780)

A note whose number of field values differs from its note type's field count is repaired with `notes/note-records#F1` against the package's note type. Anki 26.9.3 refuses the whole import instead (I17). A note whose `mid` names no note type of the package is left alone (I9 skips it).

## I4. Preview facts (R1600)

Returned by `inspect`, before any option is chosen:

| Fact | Meaning |
|---|---|
| `format` | `latest`, `legacy2` or `legacy1` (P1) |
| `notes`, `cards` | row counts of the package collection |
| `decks` | the human names (`decks/decks#D1`) of the decks named by the cards' `did` and non-zero `odid`, with their ancestors, sorted by `unicase`; the deck with id 1 is listed only when a card names it |
| `notetypes` | the names of the note types named by the notes' `mid` |
| `has_scheduling` | true when a card has a `queue` other than 0 or a review-log row exists |
| `has_presets` | true when a normal deck of `decks` uses a preset other than id 1 |
| `media_files`, `media_bytes` | the media map's entry count and the sum of its sizes (latest) or of its entries' sizes in the zip directory (legacy) |
| `repaired` | counts of I3.1's new ids, of I3.3's notes, and whether I3.2 converted |
| `defaults` | the five option values I5 reads from the target's config |

These are facts about the package. What the options will do (new, updated, duplicate notes) is known only after the apply's plan (I14); a merge's destructive mappings come from U121's preview (I8).

## I5. Options (R1601)

| Option | Config key | Values | When the key is missing or unreadable |
|---|---|---|---|
| Merge note types | `mergeNotetypes` | JSON `true` or `false` | `false` |
| Update notes | `updateNotes` | JSON `"IfNewer"`, `"Always"` or `"Never"` | `"IfNewer"` |
| Update note types | `updateNotetypes` | the same three strings | `"IfNewer"` |
| Import any learning progress | `withScheduling` | JSON `true` or `false` | `false` |
| Import any deck presets | `withDeckConfigs` | JSON `true` or `false` | `false` |

- Any value of another JSON type, and any other string, reads as the default. The sheet (U162) shows these values first.
- The apply writes the five chosen values to these keys at the start of its write phase, as local config changes (`storage/config-table#C4`: only a key whose value differs is written). They are part of the import's undo step: undoing the import restores them. An import that fails or is cancelled writes none.
- The string forms are those Anki 26.9.3 stores (to be confirmed by scenario `apkg-import/options-keys`, I18).

## I6. What is taken from the package (R1610)

The import reads these rows of the package collection, after I3, and changes them in memory only (the package collection is not written again):

- **Notes**: every note, in ascending `id` order.
- **Cards**: every card of those notes. **Review log**: with scheduling, every row whose `cid` is one of those cards; without, none.
- **Note types**: those named by the notes' `mid`, each with its fields and templates.
- **Decks** and **presets**, and the changes made to cards, notes and decks with and without scheduling: exactly what `package/apkg-export` A3 (the deck set), A5 (filtered decks with scheduling), A6 (scheduling removed) and A7 (presets) say for an export of every note of the package collection with the import's scheduling and preset options, the package collection standing in for the source. In particular:
  - without scheduling, the deck with id 1 is not taken, cards become new at positions counted from the package collection's own `nextPos` (A6), flags become 0, `marked` and `leech` are removed, filtered decks become normal decks, and every deck's `common` is emptied;
  - with scheduling, filtered decks stay filtered unless A5's condition sends their cards home;
  - with presets on, the presets named by the taken normal decks, their FSRS parameters emptied without scheduling; with presets off, none, and every normal deck's `config_id` becomes 1.
- Two differences from the export: a deck whose `config_id` names no preset of the package gets `config_id` 1 (the export refuses such a deck, A7; Anki's import fails); and A8 does not apply to an import at all: future ids were already repaired by I3.1, a note whose `mid` names no note type is skipped by I9 step 1, and a card whose deck is missing goes to the default deck (I11.1). None of them refuses the import.
- Note types not used by a note, decks without a card that are not an ancestor or a home deck, and every other table (`graves`, `tags`, config) are not read.
- The target's `nextPos` is not used for imported cards (I17 explains the Product Contract wording of R1610). Its only use is I7's card regeneration for target notes, which reads and advances it.

## I7. Note types without a merge (R1604)

Each taken note type, in ascending `id` order, before any note. First its `original_id` is set to its own id (the package's). Then, with Merge note types off:

| Target has a note type with this id? | Same schema (below)? | Result |
|---|---|---|
| no | — | **added** under the package's id |
| yes | yes | **existing**: the target's note type; updated when the Update note types rule holds against it |
| yes | no, and a target note type other than that one has `original_id` equal to this id and the same schema | **earlier copy**: that note type; this id maps to it for every note; updated when the rule holds against it |
| yes | no, and no such copy | **copy added** under a new id (`storage/ids-usn-mtime#I2`); this id maps to it for every note |

- **Same schema**: the same number of fields and of templates, and at each position the two fields (templates) match: when both have an `id` (`notetypes/records#N2`), the ids are equal; otherwise the names are equal byte for byte. Names, options, templates' text and CSS do not count.
- **Update note types** (no merge): `IfNewer` updates when the target's `mtime_secs` is below the incoming one; `Always` when the two differ; `Never` never.
- When several target note types qualify as an earlier copy, the one with the smallest id.
- **Writing.** An added note type, a copy and an update all write the incoming record (its fields and templates included, every stored value of `notetypes/records#N1` kept), normalized by `notetypes/records#N4` steps 1 to 8 as an unchecked save (`notetypes/validation#V1`; no template check). Its `usn` is -1; its `mtime_secs` stays the package's, except that a name made unique by N4 step 7 also sets it to `now_s`. Template rows keep the package's `mtime_secs` and `usn` (N3.3). An update keeps the target's id. Adding a copy sets `curModel` to the copy's id (`storage/config-table#C4`); adding under the package's id and updating do not touch `curModel`.
- **An update whose sort field differs** from the target's (`sort_field_idx`) writes every target note of that note type, changed or not: fields normalized by `notes/note-records#N2`, `sfld` and `csum` recomputed (`text/checksum-and-sort-field`), `mod` `now_s`, `usn` -1, tags and every other column unchanged. It marks the schema: reason `sort_field_changed` (`notetypes/schema-changes#G2`, classified by its effect, G3). This is the only way an import without a merge changes the schema, and it is a structural edit (I14). It runs before I9, so I9 compares against those rewritten `mod` values.
- **An update whose template fronts differ** from the target's (the list of `q_format` texts by position differs, compared exactly, as `notetypes/structure-changes`' "fronts changed" says) runs `notes/card-generation#G9` on the target's notes of that note type, with the updated note type: missing cards are added to every such note that has a card and is not complete, placed by `notes/deck-placement-and-position#D4` and positioned by that spec's P2 from the target's `nextPos`, which advances (that spec's P1). This is the import's case of `notes/card-generation#G1`'s import row. It applies to "existing" and "earlier copy" updates; an added note type or copy has no target notes. G9's plan runs in the apply's plan (I14.1 step 3) against the snapshot, after the sort-field rewrite, and its commit runs inside the write phase before I9, so I11.1's pair test sees these cards. A front change marks no schema by itself (`notetypes/schema-changes#G2` has no reason for it, and the schemas are equal). Example: the target's 1001 has templates with fronts `{{Front}}` and `{{#Rev}}{{Back}}{{/Rev}}`; the package's newer 1001 has the same template ids and the second front `{{Back}}`: every target note of 1001 with only its ordinal 0 card and a non-empty `Back` gains ordinal 1, in D4's deck, at the next counter value.
- Otherwise an update never changes the target's notes or cards: the schemas are equal, so field and template positions do not move and no card is removed or moved.

Examples: a target note type 1001 with fields `Front`, `Back`; the package's 1001 with the same field ids and `Back` renamed `Answer`, newer: updated, a rename only. The package's 1001 with an extra field: copy added as `Name+` (N4 step 7, when the name is taken), notes of 1001 go to the copy; importing the same package again finds that copy (same `original_id` 1001, same schema) and adds no third note type.

## I8. The merge port (U121; R1605, R1606)

With Merge note types on, I7's table is replaced by U121's merge for every taken note type whose id the target has; a note type whose id the target lacks is added as I7 says. U121 (`package/notetype-merge`, J1 to J14) specifies the merge itself. This spec fixes what it receives and returns.

- **Called** during the plan (I14 step 3), after I6, with: the taken note types, the target's note types read from the plan's snapshot, the package notes' (GUID, `mid`) pairs, and the target notes' (GUID, id, `mid`) for the GUIDs both hold; and, from the same snapshot, in the terms of `package/notetype-merge` (T, copy, E): the ids of every note of T, of each copy and of each E (J5 steps 3 and 5, J6), the target notes' `flds` and `mod` (J5 step 3, J10, J11), the package notes' `mod` (J10) and the Update notes option (I5, for J10). The merge reads nothing else and nothing live. It is called before any note decision.
- **Returns**, or an error that ends the whole import with nothing written:
  - the note type writes (records to add, update or remove) and the moves of target notes between note types, as planned writes of the same write phase, with the cards `notes/card-generation#G9` adds to target notes when a merged note type gains a template or a changed front (placed and positioned as I7 says);
  - for each incoming note type id, the target note type its notes go to, a **field map** (for each field position of the target note type, the incoming position whose value fills it, or none) and a **template map** (incoming ordinal → target ordinal);
  - the structural reason `notetypes_merged` when the merge has an effect (`package/notetype-merge#J9`; `notetypes/schema-changes#G2`, G3) and the structure report of `engine/change-stream#S2.5` (layouts, removed note types, type changes);
  - a **preview** of destructive mappings (fields or templates whose content would be dropped), empty when there are none.
- **Errors**: `NotetypeKindConflict{incoming, target}` when a standard and a cloze note type would merge (R1606). Anki 26.9.3 ends the whole import there with nothing changed; Meguri does the same.
- **This spec then**: reorders each incoming note's fields by its field map before I9 (an unfilled position gets the empty string); with merge on, a GUID match never logs `conflicting` (I9), since U121 moved the target note to a common note type (`package/notetype-merge#J6`), and I9 step 5 compares against the `mod` that `package/notetype-merge#J11` gives; maps each added card's `ord` by the template map, and applies I11's "pair exists" test to the mapped ordinal (Meguri: Anki tests the package's ordinal, which can add a second card for one pair after a reorder; `package/notetype-merge#J14` records the case).
- **Confirmation**: when the preview is not empty, `apply` returns `ConfirmationRequired{preview, token}` and writes nothing; the caller shows the preview (U162) and calls `apply` again with the token. Decided 2026-10-27: a token holds the session, the five options and a digest of the preview (SHA-256 of its canonical encoding, the implementer's); the plan's snapshot seq is recorded in it for diagnostics only and never checked. A later `apply` accepts the token when it names this session and its new plan has equal options and an equal preview digest, so answers committed in between never invalidate a confirmation; otherwise it returns `ConfirmationRequired` again with a new token and writes nothing. The same test runs after every replan inside one apply (I14.2): a replan whose preview digest differs ends the apply with `ConfirmationRequired` and a new token, its staged media deleted, nothing written (a safety backup already published stays).
- **Backup**: a merge with a structural effect (`package/notetype-merge#J9`) is a structural edit; I14 takes its safety backup after the confirmation and before the first write. A merge without one whose preview was not empty and was confirmed takes the safety backup the same way (J9, I14.4).

## I9. Notes (R1602, R1603)

After I7 or I8. The target's notes are read as the plan's snapshot holds them after I7's sort-field rewrite, keyed by `guid` (byte equality). Then each package note, in ascending id order:

1. A note whose `mid` names no note type of the package is skipped and logged `missing_notetype` (Meguri; Anki's import fails, I17). Its cards and review log are not imported.
2. Its `mid` is mapped by I7 (or I8), and its fields reordered by I8's field map with a merge.
3. **No target note has its GUID**, and no earlier package note had it: **new**. It is added with its package id, or, when a target note or an earlier imported note has that id, the id + 999, again until free. Its row: the fields after I12.2's reference rewrite, normalized by `notes/note-records#N2`, tags as `tags/tags#T4` stores and registers them, `sfld` and `csum` from the target note type, `guid` as in the package, `mod` as in the package, `usn` -1, `flags` 0, `data` empty. No card is generated: the note's cards come from I11 only.
4. **An earlier package note had the same GUID** (only a damaged package has two): logged `duplicate` with that earlier note's target id; nothing is written (Meguri; Anki adds both, I17).
5. **A target note has its GUID**:
   - merge off and the target note's `mid` differs from the mapped `mid`: **conflicting**; logged, nothing written, and the target note keeps its exact bytes (R1603). This covers a target note moved to another note type and an incoming note type that was copied (I7).
   - otherwise, by Update notes: `IfNewer` updates when the target note's `mod` is below the incoming `mod`; `Always` when the two differ; `Never` never. **Updated**: the target row gets the incoming fields after I12.2, normalized (N2), the incoming tags (T4), `sfld` and `csum` recomputed, `mod` the incoming `mod` (not `now_s`, so a later import of a later export still compares, `storage/ids-usn-mtime#S3`), `usn` -1; `id`, `guid` and `mid` stay. No card is generated. Otherwise **duplicate**: logged with the target note's id, nothing written.
6. Every new, updated and duplicate note maps its package id to its target id, so I11 can add its missing cards.

Example (AE67): target note `g1`, `mod` 100, field `<img src="cat.jpg">`; the package's `g1` has `mod` 200 and the same field; Update notes `IfNewer`; the package's `cat.jpg` differs from the target's: the note is updated, `mod` 200, and its field becomes `<img src="cat-<sha1>.jpg">` (I12.3).

## I10. Decks and presets (R1607, R1611)

### I10.1 Presets

With presets on, each taken preset in ascending id order: when the target has no preset with its id, it is added with that id, its stored values and `mtime_secs`, `usn` -1; otherwise it is skipped and the target's preset with that id stays as it is. Nothing else is written for presets.

### I10.2 Decks

The taken decks, sorted by the number of components of their names, then by ascending package id. Decided 2026-10-27: ties at one level go by ascending package id; Anki 26.9.3 sorts by level only, in an order it leaves unspecified, so scenarios avoid depending on it. A suffix `S` is fixed once per import: a space and `now_s` in decimal. For each deck, in that order:

1. **Earlier renames.** When an earlier deck of this import was renamed (step 3 or 5) and this deck's native name starts with that deck's old native name followed by U+001F, that prefix (old name + U+001F) is replaced by the new name + U+001F (the first such rename that applies, in the order the renames were recorded).
2. **Today limits** (normal decks): for `new_limit_today` and `review_limit_today` when present, with T the target's today and P the package's today (I11.2): a limit whose `today` is P gets `today` T; otherwise, when T > 0, `today` becomes min(`today`, T − 1); when T = 0, the limit is removed.
3. **Match by name** (`decks/decks#D2`, `unicase`):
   - a target deck of the same kind (normal or filtered): **updated** (step 4);
   - a target deck of the other kind: this deck's name gets `S` appended to its last component, recorded as a rename, and it is added (step 5);
   - none: added (step 5).
4. **Update.** The target deck keeps its id, name and `common`; a normal deck changes only in: `description` and `markdown_description` taken from the incoming deck when its `description` is not empty; `config_id` taken when the incoming one is not 1; each of `review_limit`, `new_limit`, `review_limit_today`, `new_limit_today` taken when the incoming deck has it, else kept. A filtered deck takes the incoming deck's whole filtered part. When the result equals the stored deck, nothing is written; otherwise it is written with `mtime_secs` `now_s`, `usn` -1. The package deck maps to the target deck.
5. **Add.** When the nearest existing target ancestor of the name (`decks/decks#D3`) is a filtered deck, a new normal deck (D4) named that ancestor's name plus `S` is added first, recorded as a rename of the ancestor's name, and step 1 is applied again. Then the deck is added with a new id (`storage/ids-usn-mtime#I2`) and its package content (kind and `common` as I6 left them), `mtime_secs` `now_s`, `usn` -1, its stored name normalized (`decks/decks#D1`), made unique (D2's `+` rule), its leading components taking the nearest existing ancestor's spelling and its missing ancestors created (D3, D4). The package deck maps to it. A name past D3's nesting limit ends the import with `DeckTooDeep{name}` and nothing written (Anki fails the same way).

Examples: the package's `japanese::verbs` with a target `Japanese` gives `Japanese::verbs`; the package's normal `Kanji` with a target filtered `Kanji` gives `Kanji 1894676400` and its child `Kanji::N5` becomes `Kanji 1894676400::N5`; the package's `A::B` without `A` creates `A` (D4) and `A::B`.

## I11. Cards and review log (R1608, R1609)

### I11.1 Cards

Each taken card, in ascending id order, whose note maps to a target note (I9 step 6; so not for conflicting and skipped notes):

- Its `nid` becomes the target note's id and its `ord` the mapped ordinal (I8; unchanged without a merge).
- **Pair exists**: when the target already has a card with that (`nid`, `ord`), the cards I7's (or U121's) regeneration adds included, or an earlier card of this import was added with it, the card is skipped, with its review log. Existing cards are never changed.
- Otherwise it is **added**: its package `id`, or + 999 until no target card and no earlier imported card has it; `did` and `odid` mapped to the target decks of I10.2; `due` and `odue` shifted (I11.2, with scheduling only); `usn` -1; every other column as the package holds it, `mod` included, `data` kept with unknown keys (`model/card-data-json#C4`, deviation `card-data-unknown-keys`), so FSRS memory state stays (R1609).
- **Decks a card names that the import did not take**: id 1 stays 1 (the default deck, which every collection has). Any other unmapped `did`, and any other unmapped non-zero `odid`, becomes 1, and the note is logged `missing_deck` once. Only a damaged package reaches this (Meguri; Anki keeps the id, I17).

### I11.2 Day shift (with scheduling)

- P is the package collection's today and T the target's, both at the plan's instant: P by `sched/day-clock` D3 (or D4 without a creation offset) from the package collection's `crt`, `creationOffset` and effective `rollover` (D2), with the device's current offset, writing nothing; T is the day of the target's day clock (`sched/day-clock#D8`). The shift is P − T, a signed number of days.
- `due` becomes `due` − (P − T) when the card's `queue` is 2 or 3, or its `type` is 2. `odue` becomes `odue` − (P − T) when the card's `type` is 2 and `odue` is not 0. Nothing else shifts: positions, epoch-second `due` values of learning cards, and a relearning card outside queues 2 and 3 keep their values.
- Example: P 107, T 100: a review card due 110 becomes due 103; a suspended review card due 95 becomes 88; a new card at position 12 stays 12; a learning card due at an epoch second stays.
- Without scheduling, cards are new (I6) and nothing shifts.

### I11.3 Review log

With scheduling, each taken review-log row in ascending id order whose card was added in I11.1: `cid` becomes the card's target id, `usn` -1, every other column kept. When the target already has a row with its id, the row is not inserted and the target's row stays (`storage/ids-usn-mtime#I4`). Rows of skipped cards are not imported.

## I12. Media (R1612, R1598)

### I12.1 Entries

The media map (P5 latest, P17 legacy) lists entries in map order, each with a name. An entry is **static** when its name starts with `_` or with `latex-` (case-sensitive); every other entry is **checked**.

### I12.2 References and their rewrite

The references of a field are those of `text/media-references` M2 (media tags), then M3 (sound tags). Only new and updated notes' fields (I9) are scanned and rewritten; duplicates, conflicts and skipped notes are not.

For each media tag match, left to right, then each sound tag match in the result, with raw name r and decoded name d (M1):

1. d is **eligible** when it is not empty, contains no `/`, and is neither `.` nor `..`. An ineligible name (a remote URL included, M5) stays as written.
2. Let N be `text/media-filenames#F1`'s transform of d. When a checked entry's transformed name is N, that entry is **used**, and the replacement is its stored name (I12.3) when that differs from d. Otherwise, when N differs from d, the replacement is N. Otherwise there is none.
3. With a replacement R: when d differs from r (the name held entities), R is written with `&`, `<`, `>`, `"` and `'` as `&amp;`, `&lt;`, `&gt;`, `&quot;` and `&#x27;`. Every occurrence of r inside the match is replaced by R; the rest of the field stays.

Static entries are never matched by references and never renamed, so CSS and templates that name them keep working.

### I12.3 Stored names

In map order, for each used checked entry and each static entry, with bytes B (the decoded payload, I12.4) and content hash H:

- **Checked**: the stored name is what `media/media-files#M3` gives for desired name N and bytes B: an existing case variant with the same content is reused and nothing is written (M3 row a); a free name is stored lowercased (row b); a name taken by other content gets the hash name of M4 (row c), M4 step 4 included: when a file with exactly that hash name, or a present case variant of it, holds other content (something outside Meguri's add path put it there), that file goes to the trash (`media/media-files#M10.1`) and B is written under the hash name. M3's refusals skip the entry: a name M8 ignores is never written, and since the entry then has no stored name, a reference to it gets N in step 2 (or stays when N equals d).
- **Static**: with S the entry's name in the media map (P6 and P17 leave it unchanged by F1): when the media folder has no present case variant of S (M2), B is written under S exactly (an exact-name write, `media/media-files#M6` step 4); otherwise the entry is skipped and the target's file stays, whatever its content (R1612). M6's refusals skip the entry. Anki 26.9.3 skips only when the exact name S is present, so on the Linux oracle a target `_Font.woff` and an incoming `_font.woff` give a second file there and none in Meguri (I17).
- Unused checked entries are not written (R1612). Entries are decided in map order, and each decision sees the names earlier entries of this import reserved (M13), so two incoming case variants with different bytes get `a.png` and a hash name.
- No target file is ever replaced by an import. The only target file an import moves is M4 step 4's: a file under a hash name whose content is not that hash, which goes to the trash, recoverable, and is not restored by the import's undo (I14.3). Decided 2026-10-27: the import keeps M4 as written rather than skipping the entry, because a skipped entry would leave the reference naming a file with other content, so the card would show the wrong media.
- Decided 2026-10-27: checked names go through M3 rather than Anki's exact-name rule, because on Apple's case-insensitive volumes an exact-name write of `Cat.jpg` would replace a target `cat.jpg` (R1570); the case rules then match every other add (R981).

AE67's `cat.jpg`: the target holds `cat.jpg` with other bytes, so M3 row c gives `cat-<H>.jpg`, where H is the incoming content hash, and I12.2 rewrites the updated note's reference to it.

### I12.4 Payloads

- Latest format: entry n's zip member `n` (P5), one zstd stream (P4's rules); legacy: the member P17 names, raw. A member a used or static entry needs that is missing, or that fails P2 or P4, is `Corrupt`.
- Latest format: the decoded bytes must have exactly the map's size and SHA-1; otherwise `Corrupt` (Meguri checks; Anki trusts the map, I17). Legacy: the size and SHA-1 are computed while decoding.
- Each payload is decoded once, streaming, straight into a staged file of the media store (`media/media-files#M13`, `media/media-journal#J4`), whose hash decides I12.3; a payload I12.3 then does not write is deleted from staging at once. No payload is held in memory whole (R1852), and every decoded byte counts toward P8's 16 GiB.
- Staged files move into the media folder only after the write phase commits (M13); on cancel, failure or rollback they are deleted and no media file is left behind. After a kill, M13's launch recovery decides from the journal's manifest.

## I13. The import log (R1615)

The apply's result carries one log:

| Member | Content |
|---|---|
| `found_notes` | the number of package notes |
| `new` | I9 step 3, with the new note's target id |
| `updated` | I9 step 5 updated, with the target id |
| `duplicate` | I9 steps 4 and 5, with the target id |
| `conflicting` | I9 step 5 conflicting, with the package note's id (as Anki), plus `existing_id`, the target note with that GUID (Meguri) |
| `missing_notetype` | I9 step 1, with the package note's id |
| `missing_deck` | I11.1, with the target note's id |
| `first_field_match`, `empty_first_field` | always empty for a deck package; they exist for the text importers |

- Each entry holds its id and every field of the note: for new and updated notes as written, for the others as in the package after I8's reorder. Each field is reduced: `text/strip-html` H5, then each U+000A replaced by U+0020, then cut to at most 80 bytes of UTF-8 at a character boundary.
- Entries are in I9's order. A note is in exactly one list, except that a new, updated or duplicate note can also be in `missing_deck`.
- The log is kept with the session's result and read in pages through U133 (category, offset, count); the UI shows each entry's first field (U162).

## I14. The operation

### I14.1 Steps

1. **Admission.** `apply(session, options, token?)` needs an open `Ready` target and takes the long-operation slot (`engine/lifecycle#L5`; held: `Busy{running_op}`). It keeps the slot until the operation ends. It never enters an exclusive phase.
2. **Snapshot.** One read transaction on the maintenance reader (`engine/op-model#O6`) for every target read of the plan.
3. **Plan** (cancellable, phase `Planning`): I6, then I7 or I8 (with the plan half of G9 for I7's front changes), I9, I10, I11, and I12.2's used-entry set, all in memory against the snapshot and the package collection. Also its structural reasons (I14.4) and, with a merge, U121's preview (I8).
4. **Confirmation** (merge only, I8).
5. **Preflight** (I15).
6. **Media staging** (cancellable, `media` stage): I12.3 and I12.4 for every used and static entry; then I12.2's rewrites, which need the stored names.
7. **Safety backup**, only when I14.4 takes one (a structural edit, or a confirmed merge preview).
8. **Write phase**: one op `apkg_apply` (`engine/op-model#O9`), source `UserUndoable`, step name "Import" (`conformance/undo-ops` V4 `import`), in one transaction (O3): the five config keys (I5), note types (I7 or I8), the sort-field rewrite (I7), the cards I7's regeneration adds with the `nextPos` they draw, notes (I9), presets and decks (I10), cards and review log (I11), the tags registered by the notes, `col.mod` = `now_ms` once. Then the staged media are published (M13).
9. **Result** (I14.5).

### I14.2 Write-phase rules

- **Preconditions** (`engine/op-model#O7`): `TableUntouched` since the snapshot on `notes`, `notetypes`, `fields`, `templates`, `deck_config` and `tags`. A failure replans from step 2 (at most 3 more times, then `Stale`). Media already staged for the earlier plan are kept when the new plan uses the same stored names, else deleted. A note edit, a Mark or a leech tag during the plan therefore costs one replan.
- **Re-derived** inside the transaction (O7 rule 1), so answers during the plan never make it stale: I10.2 for every deck (answers change decks' `common` counters), each "pair exists" test of I11.1 and each card id and note id choice (+ 999 against the live rows), each review-log insert (I11.3), the cards of I7's regeneration (G9's commit plans a note again when its cards changed since the plan, and draws `nextPos` from its live value), the config writes (C4 against live values), and the target's today T (when the day clock's day differs from the plan's T, the write phase ends `Stale` and replans).
- **Long write**: when the plan writes more than 5,000 rows, `apkg_apply` declares its expected length (O8), so answers wait in U43's overlay and journal (KTD6). U241's benchmark may lower the threshold; an undeclared write phase must stay within about 200 ms.
- The write phase cannot be cancelled once the writer takes it (O5; `apkg_apply` is not cancellable between batches).

### I14.3 Undo (R1615)

- One undo step "Import" of class undoable (`engine/undo#M11`): undo removes every row the import added (notes, cards, review log, note types, decks, presets, registered tags), restores every row it changed from its before-image (updated notes and their `mod`, notes the sort-field rewrite touched, note types, decks, the config keys and `nextPos` when I7's regeneration drew from it, `col.scm` when it was set, `notetypes/schema-changes#G5`), and puts `col.mod` back. Redo applies the after-images.
- Media are not part of the step: files the import wrote stay after an undo, a file M4 step 4 moved to the trash (I12.3) stays there, and Check Media reports those no note uses (U130). A redo finds them in place.
- A step of a declared long write is undone and redone through `undo_long_write` and `redo_long_write` (O9).

### I14.4 Schema, backup and CloudKit

- An import is not a schema change unless its plan marks one: `notetypes_merged` (U121, a merge with an effect, `package/notetype-merge#J9`) or `sort_field_changed` (I7). Adding note types, copies, notes, decks or presets never sets `scm` (`notetypes/schema-changes#G2`).
- **Safety backup** (`package/backup-triggers#T6.1`): an import whose plan marks neither and that needed no merge confirmation takes none (it is undoable, not "destructive, not undoable", and T6.1 has no other row for it). One that marks either is a structural edit and falls under the row "a structural edit … other than deleting a note type", with its coalescing rule. One that marks neither but whose merge preview (`package/notetype-merge#J10`) was not empty and was confirmed falls under T6.1's row for a confirmed merge preview, with the same coalescing (decided 2026-10-27, data safety: `package/notetype-merge#J9`). In both cases the backup runs as T6.2 says for a plan-and-commit op (after the plan and the merge confirmation, before the write phase; none again after a replan); its failure refuses the import with `SafetyBackupFailed{cause}` and nothing is written.
- **Structure report** (`engine/change-stream#S2.5`, `sync/cloudkit-records#K19.1`): every import reports 8 `apkg_apply`. With a merge that has an effect (J9), 9 `notetypes_merged` with U121's layouts, removed note types and type changes; a merge without an effect reports no 9. An import whose I7 rewrote a sort field also reports 3 `sort_field_changed` with that note type's identity layout. An import that updates a note type whose field or template names changed (I7, same schema) reports an identity layout for it, which adds no reason code. These are the cases K19.1 and S2.5 list. The import is one op, so it is one CloudKit generation. Ids an import keeps from the package meet other devices' rows through `sync/cloudkit-records#K14`.

### I14.5 Result, progress and cancellation

- **Result**: the op's seq, generation and undo step (O4), the log (I13), counts (notes added, updated; cards added; review-log rows added; note types added, updated, copied; with a merge, notes moved between note types and note types removed (`package/notetype-merge` J5, J6, J8); decks added, updated; presets added; media files written, reused, skipped), the safety backup's name or none. After a commit whose media publication needs recovery, `MediaCommitPending` (O4): the rows are committed and the caller never repeats the import.
- **Progress** (`engine/jobs#J5`): inspect reports the stages `extracting` (source copy and collection entry) and `analysing` (gate, repairs, preview facts); apply reports the phase `Planning` (I14.1 step 3), then the stages `media` (payloads staged of total), `backup` (when I14.4 takes one), then `Committing` for the write phase. Updates at least once per second during long stages (R1616); work runs off the main thread and off the writer except step 8.
- **Cancellation**: during inspect, plan, media staging and backup (T6.2's cancel). It ends with `Cancelled` once the work has stopped: staged media deleted, the target, its media folder and journal unchanged; a cancelled `apply` leaves the session open (I2), a cancelled `inspect` removes it. From step 8 the import completes.
- **Kill** (R2003): before the commit, the target holds none of the import and M13's launch recovery moves staged media to the trash; after it, all of it, and the journal's manifest completes the media publication.

## I15. Preflight and temporary files (R2015, R1599)

| When | Checked on | `needed` in bytes |
|---|---|---|
| before the source copy (I2) | the collection's file system | `package/container#P10`'s source-copy row |
| before extraction (I3 step 2) | the same | P10's inspect row (latest); for a legacy package, the collection member's uncompressed size from the zip directory + 4 MiB; then P10's pre-upgrade row when the file is below schema 18 |
| before media staging (I14 step 5) | the media folder's file system (the collection's) | the sum of the sizes of the used and static entries + (2 × the package collection's size) + 4 MiB, plus `package/backups#B4`'s need when I14.4 takes a backup |

- Too little space: `InsufficientSpace{needed, available}`, before the step writes anything. A write that still fails for space: `DiskFull`, with the same cleanup as any failure (P10).
- Temporary files are only P9's staging directory of the session (I2) and the media store's staging (M13). Both use U32's names and are swept after a kill (R1493). No entry name or media name ever becomes a path outside them; nothing outside the package and those directories is read (R1599).
- Limits are P8's for every entry read, legacy members included (P17). A crafted entry that decodes past a limit is `Corrupt`, with memory bounded and staging removed.

## I16. Errors

| Error | When |
|---|---|
| `UnsupportedFile` | P18 classified the file `Unsupported` |
| `TooNew` | I3 step 1 (R1597, "made by a newer version") |
| `Corrupt{detail}` | P2 to P8, P17, the gate's `NotACollection`, `UpgradeFailed` and `Corrupt`, a failed integrity check, I12.4 |
| `SchemaUnsupported{gate}` | the gate refused the package collection as `TooOld` or `TooNew` |
| `InsufficientSpace{needed, available}`, `DiskFull` | I15 |
| `Busy{running_op}`, `CollectionUnavailable`, `StaleGeneration` | I14 step 1 (`engine/lifecycle`) |
| `Stale` | I14.2, after 3 replans |
| `ConfirmationRequired{preview, token}` | I8 |
| `NotetypeKindConflict{incoming, target}` | I8 (R1606) |
| `DeckTooDeep{name}` | I10.2 step 5 |
| `SafetyBackupFailed{cause}` | I14.4 |
| `SessionClosed` | I2 |
| `Cancelled` | I14.5 |
| `MediaCommitPending` | I14.5, after a commit |
| `Io` | any other file-system failure |

Every error before the commit leaves the target, its media folder, media journal and backups as they were (a published safety backup stays). U133 maps these to its FFI error set; U162 words them.

For scenarios, Anki 26.9.3's deck package import errors are recorded as `too_new` (an unknown `meta` version) and `corrupt` (everything Anki reports as not a valid file, a protobuf or JSON decode error, a zip error), as `specs/conformance/package-ops.md` E2.3 does for collection packages, plus `notetype_kind_conflict`.

## I17. Differences from Anki and from the Product Contract's wording

Decided 2026-10-27: Meguri differs where Anki fails, guesses, can overwrite a file or writes a damaged state, choosing data safety first, then the plan's requirements (R780, R1570, R1615), then Anki's behavior. Each difference that a golden can see needs a `conformance/deviations.yaml` entry when its scenario is recorded (proposed ids in brackets):

| Situation | Anki 26.9.3 | Meguri |
|---|---|---|
| Incoming checked media name with upper-case letters, or a case variant of a target file | written under the name as given (on a case-sensitive volume a second file; on a case-insensitive one it can overwrite a target file) | `media/media-files#M3`: lowercased, case variants reused or hash-named, references rewritten (`apkg-import-media-case`) |
| A static (`_`, `latex-`) entry whose name is a case variant of a target file, not the exact name | written (on a case-sensitive volume a second file) | skipped, the target's file kept (I12.3) (`apkg-import-media-case`) |
| A note whose field count differs from its note type | the whole import fails | repaired by F1 (I3.3) (`apkg-import-field-count-repaired`) |
| A note whose `mid` names no package note type | the whole import fails | skipped, `missing_notetype` (`apkg-import-missing-notetype`) |
| A card whose deck the package lacks | keeps the id, which may name an unrelated target deck or none | default deck, `missing_deck` (`apkg-import-missing-deck`) |
| Two package notes with one GUID | both added | the second logged `duplicate` (`apkg-import-duplicate-guid`) |
| A latest-format payload whose size or SHA-1 differs from the map | written as is | `Corrupt` (crafted packages only) |
| A package collection that fails `integrity_check` | not checked: read as it is | `Corrupt` |
| A valid id between `now_ms` and the cutoff next to an id past it | constraint error | repaired (I3.1) |
| A deck whose preset is missing from the package, presets on | the import fails | `config_id` 1 (I6) |
| "Pair exists" after a merge that reorders templates | tested on the package ordinal | tested on the mapped ordinal (I8) |
| Two package cards with one (`nid`, `ord`) | both added | the second skipped (I11.1) |
| `conflicting` log entries | package id only | also `existing_id` |
| Unknown card `data` keys and unknown protobuf fields | dropped | kept (`card-data-unknown-keys`, `notetype-unknown-fields-kept`) |

- **R1610's "next new positions"**: Anki 26.9.3, and so Meguri, gives imported new cards the positions of the package collection's own `nextPos` (I6) and does not use the target's counter for them; the imported new cards therefore do not go after the target's existing new cards. The one exception is I7's card regeneration for target notes after a note type update with changed fronts: those cards take positions from the target's `nextPos` and advance it. The goldens are Anki's; the plan's U120 test "cards new at the next positions" reads as this.
- **R1608's "only for newly imported notes"**: cards are added for every new, updated and duplicate note whose (`nid`, `ord`) pair is missing in the target (I11.1), as Anki does; existing cards are never changed.
- **R1615's categories**: for a deck package Anki fills only new, updated, duplicate and conflicting; `missing_notetype` and `missing_deck` are filled only for damaged packages (above), `first_field_match` and `empty_first_field` never.

## I18. Scenarios and tests

### I18.1 What the oracle maintainer adds

No `apkg-import` scenario can be recorded yet: the oracle driver's `import` has only the collection package forms (`specs/conformance/package-ops.md` E2), and `export` has no latest deck-package form (`package/apkg-export#A17`). Needed, with the package-ops owner fixing their shapes in `specs/conformance/package-ops.md`:

- `import`, deck-package form: `{format: apkg, fixture | name, merge_notetypes, update_notes: if_newer | always | never, update_notetypes: ..., with_scheduling, with_deck_configs}`, every option required. Output: the I13 log with logical ids (`#nN` for target notes, package ids where I13 says so), each list's fields, and `found_notes`. Rejections `too_new`, `corrupt`, `notetype_kind_conflict`. Oracle: Anki's deck-package import with those options into the session collection, its media folder and media database. Capability `import.apkg`.
- The latest deck-package form of `export` (A17), so a scenario can export its own collection, change it, and import the package back.
- Fixture packages under `conformance/fixtures/package/apkg/`, made by the maker of `package-ops.md` E4 from scenario-style collections (own note types, R1959) with Anki 26.9.3: a latest-format export with media, the same exported with "Support older Anki versions" (legacy 2), and a legacy 1 package (`collection.anki2`, a JSON map, no `meta`) from the storage maker's old wheels; a v1-scheduler package with learning cards, a relearning card and cards in a filtered deck with and without rescheduling; packages with ids past the cutoff; and crafted variants (an unsafe legacy name `../x.jpg`, `meta` version 4, a legacy map that is not JSON, a member that decodes past P8's ratio).

### I18.2 Scenarios to record

| Scenario | Shows |
|---|---|
| `ae67` | AE67: update if newer, `cat.jpg` hash-named, reference rewritten (needs `media_dir`, `db_dump`) |
| `update-matrix` | Update notes `IfNewer`, `Always`, `Never` × target `mod` older, equal, newer: updated or duplicate, incoming `mod` kept |
| `conflicting` | same GUID, target note moved to another note type, merge off: logged, target note byte-identical |
| `notetypes` | I7: new id added; same schema newer updated with `IfNewer`; different schema copy added and notes remapped; re-import reuses the copy; `curModel` after a copy; a name collision gets `+`; a newer same-schema update with a changed front adds the missing cards to target notes (deck, positions, target `nextPos` advanced, `scm` unchanged) and an incoming card for such a pair is then skipped |
| `notetype-sort-field` | I7: an update with another sort field rewrites target notes' `sfld`, `mod`, and sets `scm`; then `IfNewer` notes compare against the new `mod` |
| `decks` | I10.2: case match `japanese`/`Japanese`; normal vs filtered renamed with the suffix (the clock frozen) and its child; missing parent created; a filtered ancestor; today limits remapped |
| `cards-existing-pairs` | I11.1: existing (`nid`, `ord`) pairs skipped, existing card rows unchanged; a duplicate note gains its missing card |
| `scheduling-on` | I11.2 and I11.3: package and target days 7 apart, review, suspended review, learning and filtered cards, revlog added, card `data` kept |
| `scheduling-off` | I6: new cards at the package's positions, flags 0, `leech`/`marked` removed, filtered decks normal, default deck not taken, target `nextPos` unchanged |
| `presets` | I10.1: off none; on only absent ids; FSRS parameters emptied without scheduling |
| `media-rules` | I12: identical content reused, a `_font.woff` present with other content kept, unreferenced entries not copied, a static entry copied unreferenced, entities in a name |
| `ids` | I3.1 and I9: a note id equal to a target id shifted by 999; card ids 3 days ahead repaired |
| `legacy-1`, `legacy-2` | P17: both formats import and match |
| `v1-scheduler` | I3.2: conversion of revlog, relearning and filtered cards |
| `refusals` | `meta` 4 → `too_new`; `../x.jpg` in a legacy map, a malformed map, a non-zip `.apkg` → `corrupt`; target unchanged |
| `undo` | import, `undo_status` "Import", undo → dump equals the pre-import dump, redo → equals the import's |
| `options-keys` | I5: the five keys after imports with each value |

### I18.3 Meguri's own tests

`core/crates/meguri-package/tests/apkg_import.rs` (U120), against the goldens once recorded, and before that against these rules:

- every I17 Meguri behavior, each with a package built in the test (crafted packages from the test's own writer, never from `local-data/`);
- refusals leave no staging directory, the target and its directory listing unchanged (SHA-256), and no file in the media folder or media staging;
- a 1 KiB member that decodes past 16 MiB: `Corrupt`, peak memory bounded;
- cancel during `media`: target unchanged, staged files gone; cancel during `backup`: `Cancelled`, the published backup kept;
- injected low space before each I15 check: `InsufficientSpace`, nothing written;
- the failpoint kill during the write phase (child `meguri-cli`, `test-hooks`): the reopened target passes `integrity_check` and holds all or none of the import (R2003);
- answers committed on the interactive lane during the plan and the media stage do not make the import stale, and an edit of a target note does (one replan);
- the safety-backup ordering test of `package/backup-triggers` T10 for an import that changes a sort field (and, with U121, one that merges with an effect and one whose confirmed merge preview has no structural effect; a merge with neither takes none);
- the structure report of I14.4 (`op_catalog.rs`): reason 8 alone, 8 and 9, and 8 and 3; a merge without an effect reports 8 alone; an update that only renames a field reports 8 and an identity layout;
- `benches/import.rs`: 10,000 notes with 10,000 media files from U11 imported in at most 3 s excluding media copy (R1846), a write phase under 1 s, peak memory at most 300 MB (R1852).

The `package_reader` fuzz target (`core/fuzz/fuzz_targets/package_reader.rs`) is extended to the legacy formats and to `inspect`.
