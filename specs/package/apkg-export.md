# Deck package export and the export options

Spec: package/apkg-export
Requirements: R1617 (core limits), R1618 (core options; remembering them is U160), R1619 (latest format; legacy part U123), R1620, R1621, R1622, R1623, R1624 (atomic output; native handoff U133/U160), R1625 (the collection package options), R2004, R2015
Anki: 26.09.3
Scenarios: conformance/scenarios/package/apkg-export/ (A17 lists them)

How Meguri writes a deck package (`.apkg`, U122): which rows go in for a deck, a note or card selection, or the whole collection; how scheduling is removed when it is not included; which media files go in; the latest-format container; and Meguri's export operation (input, output, progress, cancellation, errors, temporary files, atomic output). A14 states the two collection package options of U124 (R1625). The container, its limits and the writer reused here are `package/container` (P1–P11, P15). The job, owned artifact, native destination and cancel rules are `package/file-export` E2–E5. The legacy format ("Support older Anki versions") is U123's: the schema-11 copy is `storage/schema11-downgrade`, the legacy container `package/legacy-export` (U123); A13 only says where it plugs in. Importing a deck package is U120's (`package/apkg-import`).

## A1. Scope and entry points

- Entry points (UI units): a deck's menu (U152), a Browse selection of notes or cards (U210, U219), and Settings for the whole collection (U160). All of them call the one operation of A11.
- The output is always a file for the user to save or share: the owned artifact of `package/file-export#E3`, delivered through `#E4`. Export never changes the collection, its media folder, its media journal, backups or sync state, and starts no network request (`package/file-export#E1`).
- It needs an open, `Ready` collection, as `package/file-export#E1` says.
- Every entry point and what it starts with (File › Export… and Settings › Collection's Export…, File › Export Deck… and Deck › Export Deck… (R280, R283), a deck's context menu and the iPhone deck ⋯ menu, Browse's Export Notes (R273, R1227), Settings › Backups' Export Collection with Media… (R1636)), which formats each offers, the scope list (a whole-collection entry, then `decks/decks#D17`'s decks) and which options each format shows: `package/transfer-sheets#X4`. The option labels are `package/transfer-strings#S3`.

## A2. Inputs and options

**Limit**, exactly one of:

| Limit | Selected notes |
|---|---|
| `Collection` | every note |
| `Deck(id)` | the notes that have at least one card whose `did` is that deck or one of its descendants (`decks/decks#D3`). Only the card's current `did` counts: a card that a filtered deck outside the tree holds does not select its note; a card in a filtered deck inside the tree does. |
| `Notes(ids)` | the notes with those ids; ids that name no note are ignored |
| `Cards(ids)` | the notes of the cards with those ids; ids that name no card are ignored |

A `Deck` limit whose deck no longer exists is refused with `DeckNotFound` (Anki exports an empty package; Meguri's UI can only ask for this after the deck was deleted under it).

**Options** (R1618):

| Option | Effect | First-use default |
|---|---|---|
| Include scheduling information | A5 when on, A6 when off | on; off when the export starts from a deck's menu |
| Include deck presets | A7 | off |
| Include media | A9 when on; no media file when off | on |
| Support older Anki versions | A13 | off |

- These are Anki desktop's export dialog defaults, including scheduling off from a deck's menu.
- Decided 2026-10-27: after the first export, U160 remembers the last choice of each option per package kind (`.apkg`, `.colpkg`, A14), on the device, and uses it from every entry point instead of the first-use default. Anki has no collection key for export choices, so Meguri never stores them in the collection or in synced config: remembering never changes `col.mod`, a `usn` or the cloud state.

## A3. Selection (R1620)

All rows come from one read snapshot (A11 step 2).

- **Notes**: A2's selected notes, including notes that have no card (possible with `Collection` and `Notes`).
- **Cards**: every card of every selected note, also cards outside the limit. A `Cards` selection of one card of a two-card note exports both cards; a `Deck` export of a note that has another card in another deck exports that card too.
- **Note types**: the note types named by the selected notes' `mid`, each with its `fields` and `templates` rows.
- **Decks**, in this order of building the set:
  1. every deck named by a selected card's `did`;
  2. with scheduling, also every deck named by a selected card's non-zero `odid`;
  3. the ancestors of every deck in the set (`decks/decks#D3`), each once: from each deck, go up one parent at a time by name (compared case-insensitively, as the `decks` name column compares), adding each parent found; this stops at the first ancestor name that names no deck or that the set already has. So in a damaged tree with `A::B::C` and `A` but no `A::B`, going up from `A::B::C` does not add `A` (another deck of the set under `A` still adds it);
  4. without scheduling, the deck with id 1 is removed from the set, whatever put it there.

  A deck without a selected card that is not such an ancestor is not exported: an empty subdeck of an exported deck stays out.
- **Presets**: only with "Include deck presets" (A7).
- **Review log**: with scheduling, every `revlog` row whose `cid` is a selected card; without scheduling, none.
- **Nothing else**: no `graves`, no `tags` rows (the importer registers tags from the notes), no source config key except `creationOffset` (and the source's `crt`, which goes in the `col` row; A4).
- A selection of zero notes is not an error: the package holds no note (Anki writes the same). The result's note count (A11) lets the UI say so before the user saves.

## A4. The package collection

The collection inside the package is a new file built in staging, never a copy of the live file:

- Start from a new collection made at the export instant `t` as `storage/new-collection` B2 to B6 and B8 describe, with these differences:
  - no note type at all: no `notetypes`, `fields` or `templates` row, and no `curModel` key (B7 does not apply);
  - `col.crt` is the source's `col.crt`;
  - `creationOffset` is the source's value read as a 32-bit signed integer (a JSON integer in that range; `-60.0`, `"-60"` and `null` are unreadable), written as that integer in a local config change (`usn` -1, `mtime_secs` `now_s`); when the source's key is missing or unreadable, the package has none (Anki reads it the same way);
  - `col.mod` is the instant in milliseconds at which the rows below are committed. The builder writes it explicitly in that commit, since its `Raw` writes (A11 step 5) do not move `col.mod`.
  No other source config key is copied: the package has no `rollover`, `localOffset` or `nextPos` of the source (its `nextPos` is B4's `1`).
- Then the rows of A3, after A5, A6 and A7: decks, notes, cards, note types (with their fields and templates), review log and presets. A source row keeps its `id`, `mod`/`mtime_secs` and `usn`; the export stamps nothing.
- Every record is written by its owner's codec: protobuf blobs per `model/protobuf-records` P5 (an unchanged record keeps its exact bytes; a changed one is encoded per P3 and keeps unknown fields per P4), card `data` per `model/card-data-json` C4 (unknown keys kept, deviation `card-data-unknown-keys`), note `tags` as in A5.
- A deck row of the source with id 1 (with scheduling, when A3 put it in the set) replaces the new collection's default deck. Otherwise B6's default deck stays, with B6's bytes (A5 and A6 change source decks only, so its `common` stays `08 01 10 01`), so cards whose `did` is 1 still name a deck. The default preset of B5 stays unless A7 adds the source's preset 1.
- Decided 2026-10-27: when B6's default deck would stay and a package deck with another id has its name `Default` under `unicase` (`decks/decks#D2`: the source's deck 1 was renamed, for example to `Default+` by `decks/decks#D8`, and another deck took the name), B6's row is left out, since the unique `decks` name index holds only one of them. This is Anki's result: its deck writer replaces B6's row with the source deck. A package card whose `did` is 1 (only without scheduling: with scheduling, such a card puts the source's deck 1 in the set) then names no package deck, the one unresolved reference a package may hold; both importers keep its `did` 1, the target's default deck, which every collection has (`package/apkg-import#I11.1`). Writing B6's row under another name was rejected: an import with scheduling would take that deck by its new name and add a stray deck to the target.
- Schema 18. Closed as `package/container#P11` requires of a snapshot: header bytes 18 and 19 are 2, no `-wal` or `-shm` file.

Why `crt` and `creationOffset`: an importer computes the package's "today" from them (with its default rollover) and shifts day-based due dates by the difference with its own today (R1609). Copying anything else would change what Anki's importer does with Meguri's package.

## A5. Rows with scheduling

With "Include scheduling information" on, rows are copied as A4 says, unchanged in content, with one exception for filtered decks.

- Note `tags`: the stored text split at tag separators (`tags/tags#T1`) with empty pieces dropped, then written as `tags/tags#T4` step 5 writes a list, in the same order and without dropping repeats or normalizing: `"  b a  a "` is written ` b a a `. Note `flags` is written 0 and note `data` the empty string, as Anki 26.9.3 writes them whatever the source holds (no importer reads either; `package/apkg-import#I9` writes the same). Every other note column is copied as stored.
- **Filtered decks** stay filtered decks, with their cards in them and the cards' home decks in the package (A3 step 2), when every selected card meets one of:
  - its `odid` is 0 and its `did` names a deck of the package that is not filtered;
  - its `odid` is 1, or names a deck of the package.

  Otherwise (a card whose non-zero `odid` names no deck of the package, or one with `odid` 0 sitting in a filtered deck) every selected card whose `odid` is above 0 is returned home in place (a card with a negative `odid`, only possible in a damaged row, is copied as stored, as Anki does): it stays in the deck its `did` names, `odid` becomes 0, `due` becomes `odue` when `odue` is not 0, `queue` is set from the type and the restored `due` as `sched/card-data#K3` says when it is 0 or above (a negative queue stays), and `odue` becomes 0. Every filtered deck in the package then becomes a normal deck: its `kind` is a new normal deck with preset 1 and every other field at its default (`0a 02 08 01`, as `storage/new-collection#B6`); its `common` is kept.

## A6. Scheduling removed (R1621)

With "Include scheduling information" off, the copies change as below. The live rows never change.

**Cards.** Take the selected cards in ascending `id` order. Read the source's `nextPos` once as `sched/card-data#K7` reads it (unsigned 32-bit, 0 when missing or unreadable) as the next free position `P`. For each card:

| Column | Value |
|---|---|
| `due` | the card's last position when it has one: for a card of type 0, `odue` when `odid` is above 0, else `due` (also for a damaged negative `odid`), read as unsigned 32-bit; for another type, its `pos` key when present. A card without a last position gets `P`, and `P` moves on by one (from 4,294,967,295 to 0). Stored as a signed 32-bit value (K7). |
| `type`, `queue` | 0, 0 (suspended and buried cards become plain new cards) |
| `ivl`, `factor`, `reps`, `lapses` | 0 |
| `did` | unchanged: a card in a filtered deck stays in it, and that deck becomes a normal deck (decks, below) |
| `odid`, `odue` | `odid` 0; `odue` 0 when `odid` was not 0, else unchanged (a damaged card with `odid` 0 and `odue` 4 keeps 4, as in Anki) |
| `flags` | 0 (the whole column, not only the user flag bits) |
| `data` | `s`, `d` and `pos` removed; `dr`, `decay`, `lrt`, `cd` and unknown keys kept (C4) |
| `left`, `nid`, `ord`, `mod`, `usn` | unchanged |

The source's `nextPos` is not written. No review-log row is added (unlike Reset, `sched/manual-ops#M6.3`).

Example, derived from these rules (cards by ascending id, source `nextPos` 7):

| Card before | `due` after |
|---|---|
| review, no `pos` | 7 |
| new, `due` 3 | 3 |
| suspended review, no `pos` | 8 |
| review, `pos` 2 | 2 |
| new held by a filtered deck, `odue` 5 | 5, in the filtered deck, now normal |
| learning, `pos` 4, `left` 1002 | 4, `left` still 1002 |

**Notes.** The tags `marked` and `leech` are removed, compared with the whole tag and ignoring ASCII case only (`Marked` and `LEECH` go; `leech::x` and `marked2` stay). The tags, `flags` and `data` are written as in A5. Nothing else changes, `mod` and `usn` included.

**Decks.** Every source deck in the package (B6's default deck, when A4 keeps it, keeps B6's bytes):

- `common` becomes an empty message: collapse states, the studied-today counters and their day, and any unknown field or `other` content are gone (`model/protobuf-records#P6.4`).
- A normal deck: `extend_new` and `extend_review` become 0; `review_limit`, `new_limit`, `review_limit_today` and `new_limit_today` become absent; `config_id` becomes 1 unless deck presets are included. `description`, `markdown_description`, `desired_retention` and unknown fields stay (P4).
- A filtered deck becomes a new normal deck with preset 1, every other field at its default (`0a 02 08 01`).
- The deck with id 1 is not exported (A3 step 4); B6's default deck takes its place, unless A4 leaves it out for a name clash.

**Review log.** None (A3).

## A7. Deck presets

- Off: no preset is exported. Every normal deck's `config_id` becomes 1, with and without scheduling, so the package holds only B5's default preset.
- On: every preset named by the `config_id` of a normal deck of the package (as read from the source, before A5 and A6 convert filtered decks), each once, with its row as stored. Without scheduling, its `fsrs_params_4`, `fsrs_params_5` and `fsrs_params_6` lists become empty and every other field stays. A source preset with id 1 replaces B5's row; otherwise B5's row stays.
- A normal deck whose `config_id` names no preset makes the export fail with `NeedsCheckDatabase{reason: MissingPreset}` (Anki fails with its not-found error). Off, this check does not run, since the ids become 1.

## A8. Checks before anything is written

On the snapshot, before staging exists (A11 step 3):

1. **Future ids** (R1623). When any selected card id or note id, or with scheduling any selected review-log id, is greater than the export instant in milliseconds plus 86,400,000: `InvalidIds`. An id equal to that bound passes. The message suggests Check Database (wording U160). Deck, note type and preset ids are not checked.
2. **Broken references** (Meguri's rule, data safety): a selected note whose `mid` names no note type, or a selected card whose `did` names no deck: `NeedsCheckDatabase{reason: MissingNotetype | MissingDeck}`. Anki writes such a package and its importer then fails or guesses; Meguri never writes a package its own importer would refuse. Check Database repairs both (U128, U129).
3. A7's missing-preset check.
4. **Unreadable records** (data safety): every row the export reads as a record must read under `model/row-records#R10` (its blobs under `model/protobuf-records#P9`, including a deck `kind` with one member): the `decks` rows of A3's set, ancestors included, whose `kind` tells A3, A5 and A7 whether a deck is filtered and which preset it uses; the `notetypes` rows of the selected notes with their `fields` and `templates` rows (A9.3 reads `latex_svg`, A9.4 and A9.5 the CSS and templates); and with "Include deck presets" the `deck_config` rows A7 exports. Otherwise: `NeedsCheckDatabase{reason: UnreadableRecord{table, id}}`, naming the first such row in that order of tables, then by ascending id. Anki fails on these too. Notes, cards and review-log rows are copied as SQLite values and are not checked here.

Nothing is created before these checks pass: no staging directory, no temporary file, no artifact.

## A9. Media selection (R1622)

With "Include media" on, the package holds the files that the exported notes and note types reference and that exist in the media folder. Unreferenced files never go in.

### A9.1 Candidate names

1. **Fields.** For every field of every exported note, the decoded names of `text/media-references#M4` (media tags and sound tags), remote ones included (rule 2 drops them).
2. **LaTeX.** For every field of every exported note, the names of the generated LaTeX images (A9.3). Fields are scanned as stored; cloze markers are not expanded first.
3. **Note types.** For every exported note type, the `_` names of its `css` (A9.4) and of the `q_format` and `a_format` of each of its templates (A9.5). Only names starting with `_` come from note types; the browser templates are not scanned.

Each candidate is taken in NFC as it is found, and the NFC names form a set compared byte for byte: a name found several times, or in spellings that differ only in Unicode normalization (an NFC `é.png` in one field and an NFD `e` + U+0301 `.png` in another), counts once. Case variants stay distinct (A9.2 step 6).

### A9.2 From candidates to files

For each candidate, in any order:

1. The name is already in NFC (A9.1), so no two candidates give the same entry name.
2. Drop it when it is empty, contains `/`, or is `.` or `..` (it cannot be one plain file name; remote URLs fall here).
3. When the media folder's entry with exactly that name is a directory, a symbolic link or another non-regular entry: `MediaUnreadable{name}` (Anki follows links and fails on directories; Meguri never reads through a link, as `package/file-export#E7.1`).
4. Resolve it with `media/media-files#M5`. Not found: drop it silently (Anki ignores missing files too).
5. Found, but the name is not safe (`package/container#P6`: the media file name transform would change it, for example `con.jpg`, `a:b.png`, `a\b.png`): `MediaNeedsCheck{name}`. Anki fails the export the same way (its "check media" error). Check Media renames such files (U130).
6. Found and safe: the package gets one media entry named with this NFC name, holding the bytes of the file M5 resolved. When the reference's case differs from the file's (M5 step 3), the entry keeps the reference's spelling, so the importing collection finds it under the name its notes use. Two references that differ only in case give two entries with the same bytes.

Every kind of regular file qualifies: `_`-prefixed, zero-byte and larger than 100 MiB files, and files the journal does not track (M5 step 2). A file larger than 4,294,967,295 bytes: `MediaTooLarge{name}` (the map's size is a `uint32`; Anki writes size 0 for it).

Order of the entries: by the name's UTF-8 bytes (Anki's order is arbitrary; no importer depends on it).

### A9.3 LaTeX image names

Scan the field left to right for, in this order of preference at each position, case-insensitive, each with a non-empty body that is the shortest possible and may span lines:

| Form | LaTeX text |
|---|---|
| `[latex]` body `[/latex]` | the body |
| `[$]` body `[/$]` | `$` + body + `$` |
| `[$$]` body `[/$$]` | `\begin{displaymath}` + body + `\end{displaymath}` |

A match consumes its text and the scan goes on after it. In the LaTeX text, replace each `<br>`, `<br/>` and `<div>` (ASCII case-insensitive) with LF, then apply `text/strip-html#H4`. `<br />` (with a space) is not replaced: H4 removes it like any tag, with no LF. The name is `latex-`, the 40 lower-case hex digits of the SHA-1 of the result's UTF-8 bytes, and `.svg` when the note type's `latex_svg` is on, else `.png`.

| Field | `latex_svg` | Name |
|---|---|---|
| `[latex]x^2[/latex]` | off | `latex-7046d961a8144b7b2c2da6066849a9f889ff2ac9.png` |
| `[$]\frac{1}{2}[/$]` | on | `latex-2f960094315d60883495f9b74148e17487ee9584.svg` |
| `[LaTeX]a<br>b[/latex]` | off | `latex-fcd127ffa1016069006ad91f3f361248f9bdf272.png` (text `a` LF `b`) |
| `[latex]a<br/>b[/latex]` | off | `latex-fcd127ffa1016069006ad91f3f361248f9bdf272.png` (text `a` LF `b`) |
| `[latex]a<br />b[/latex]` | off | `latex-da23614e02469a0d7c7bd1bdab5c9c474b1904dc.png` (text `ab`) |

Meguri never generates these images (`render/card-render-pipeline`); it exports the ones that exist.

### A9.4 `_` names in a note type's CSS

Scan the CSS left to right (rules of `text/strip-html#H1`: leftmost match wins, alternatives tried in the order listed at the same position; "white space" is Unicode white space; "a character" is any character except LF). The whole scan is case-insensitive under Unicode simple case folding: every literal of forms 1 to 5 (`@import`, `url(`, `css`) matches in any case, so `@IMPORT "_a.CSS"` is form 1 and `'_x.Css'` form 2. A match consumes its text. The name is the part shown in bold:

1. `@import`, one or more white-space characters, `"`, **`_`, any run without `"`, one character, `css`**, `"`.
2. `'`, **`_`, any run without `'`, one character, `css`**, `'`.
3. `url(`, optional white space, `"`, **`_` and one or more characters other than `"`**, `"`.
4. `'`, **`_` and one or more characters other than `'`**, `'` (anywhere, not only inside `url(`).
5. **`_` and the shortest run of one or more characters** that is followed by optional white space and `)` (anywhere).

| CSS | Names |
|---|---|
| `@import "_base.css";` | `_base.css` |
| `@import "_a.CSS";` | `_a.CSS` |
| `@font-face { src: url(_f.woff); }` | `_f.woff` |
| `src: url("_g.ttf")`, `src: url('_h.ttf')` | `_g.ttf`, `_h.ttf` |
| `url( _x.png )` | `_x.png` |
| `.a_b { background: url(c.png) }` | `_b { background: url(c.png` (form 5; no such file, so A9.2 drops it) |

### A9.5 `_` names in a template

Scan each template side left to right with the same matching rules, case-sensitive:

1. `[sound:`, **`_` and one or more characters other than `]`**, `]`.
2. `"`, **`_` and one or more characters other than `"`**, `"`.
3. `'`, **`_` and one or more characters other than `'`**, `'`.
4. `src=` or `data=`, starting at a word boundary (H1's word characters), then **`_` and one or more characters other than `>` and ASCII white space (space, tab, LF, FF, CR)**.

Decided 2026-10-27 (R1622: a referenced file goes in): form 4 ends the unquoted value where HTML ends it, at white space. Anki's form 4 stops only at `>` (spaces and line breaks included), so for `<img src=_y.png alt=a>` its candidate is `_y.png alt=a`, normally no file, and it leaves `_y.png` out. A16 lists this difference.

| Template | Names |
|---|---|
| `<script src="_x.js"></script>` | `_x.js` (form 2) |
| `<img src=_y.png>` | `_y.png` (form 4) |
| `<img src=_y.png alt=a>` | `_y.png` (form 4; Anki: `_y.png alt=a`) |
| `<img src=_y.png class="_hint">` | `_y.png`, `_hint` (forms 4 and 2; Anki: `_y.png class="_hint"` only) |
| `[sound:_tick.mp3]` | `_tick.mp3` |
| `<div class="_hint">` | `_hint` (form 2; dropped by A9.2 unless such a file exists) |
| `<img src="y.png">` | none |

## A10. The container (R1619, latest format)

The package is the latest format of `package/container#P15`, used as it is:

- entries `meta` (`08 03`), `collection.anki21b` (the A4 file), `collection.anki2` (the placeholder of P7), the numbered media entries `0`, `1`, ... in A9 order, then `media`, all stored, each payload one zstd frame at level 3 with its content size and checksum;
- the media map of P5 with each entry's NFC name, exact size and SHA-1;
- with "Include media" off, or no file selected: no numbered entry, and `media` is the zstd encoding of zero bytes, as in P11;
- the P8 limits: at most 400,000 zip entries (so at most 399,996 media files), 64 MiB of decoded map, 8 GiB decoded collection, 16 GiB of decoded data in all. Past one: `LimitExceeded{what}`, found from the file sizes before any media is copied. P8's ratio rule (the decoded `collection.anki21b`, and the decoded `media`, each at most the larger of 16 MiB and 1,000 times its compressed entry) can only be checked after compressing: the writer checks it as each of the two entries is written, before A11 step 9's reread, with the same `LimitExceeded{what}`, as `package/legacy-export#L6` does. Only a collection made almost entirely of repeated bytes can fail it.
- Large entries may be compressed with several threads; the frame format does not change.

Anki 26.9.3 writes the same entries in the same order, stored, its frames at the library's default level without size or checksum.

## A11. The operation

Input: the limit and options of A2. Output: the owned artifact of `package/file-export#E3`, with the metadata listed there, the suggested name below, and the counts of notes, cards and media files in the package.

**Steps**, each observing cancellation:

1. **Admission.** As `package/file-export#E2`: an open `Ready` collection, the source generation and admission sequence, the review flush on the export worker before the snapshot, at most one active export per flow.
2. **Snapshot and selection.** One read transaction on the maintenance reader (`storage/open-and-connections` C6) holds every read of A3, A6's `nextPos`, A4's `crt` and `creationOffset`, and A8. It may read in pages by id range; it never holds the writer, so answers keep committing (R1635). Commits after its first read are not in the package.
3. **Checks.** A8 on the snapshot. Nothing is written before they pass.
4. **Preflight 1** (R2015): on the collection's file system, the size of `collection.anki2` plus its `-wal` plus 4 MiB, the bound for the built collection (`package/container#P10`).
5. **Build.** In a fresh P9 staging directory, build `snapshot.anki2` as A4 says with U23's storage-only builder (`Raw` stamping), streaming rows from the snapshot with A5 to A7 applied to the copies only. Collect A9's candidates while the notes are copied. End the read transaction. Close the file as A4 says.
6. **Media.** A9.2 to an inventory of names, files and sizes; its refusals and A10's limits apply here.
7. **Preflight 2** (R2015), before capture creates a file: one check per file system, with checked arithmetic (an overflow refuses with `InsufficientSpace`); when the two file systems are one, the sums add up. Decided 2026-10-27: this table replaces P15's capacity checks for a deck package, since the built collection already exists and preflight 1 covered it. Let `C` be the built collection's size, `M` the sum of the inventory's sizes, `n` their number, `J` the encoded size of the P5 map (known from the names and sizes), `H` the placeholder's size, and `z(x)` = `x` + `x`/128 + 1 KiB the bound for one zstd frame of `x` bytes (above zstd's worst case, with its frame header and checksum):

   | File system | `needed` in bytes |
   |---|---|
   | the collection's (staging) | `M` (the media copies; the built file is not counted again) |
   | the output's (temporary package file) | `z(C)` + the sum of `z(s)` over the inventory's sizes `s` + `z(J)` + `z(H)` + 512 × (`n` + 4) + 64 KiB (`meta`, headers, central directory, zip64 records) |

   Once it has passed, a write that still fails for space is `DiskFull` (P10).
8. **Capture and write.** Capture the inventory's files as immutable staging copies with the per-file rules of `package/file-export#E7.2` (open the exact entry without following links, require a regular file, record identity, length and modification metadata, check them around the copy and again before Ready; a changed, vanished or replaced file: `MediaChanged{name}`). The protected-tree walk of E7.2 does not apply: only the named files are read. Then P15's writer writes the package into the temporary package file (P9, purpose `export`).
9. **Verify and publish.** P15's reread of the closed file (collection length and SHA-256, every map record and numbered entry, frames, SHA-1, limits), then U32's atomic publication with `ExclusiveCreate` into the operation's output directory (`package/file-export#E3`), and the generation recheck of E2 before Ready.

**Progress stages**: `queued`, `gathering`, `building` (notes copied of total), `media` (files captured of total), `writing`, `verifying`, `ready`. A stage whose total is unknown is indeterminate.

**Cancellation** at any step ends with `Cancelled` once the job has stopped: the staging directory and the temporary file are removed, no artifact exists, and the collection, the media folder and the destination are as before. After Ready, cancellation and destination rules are `package/file-export#E4` and `#E5`.

**Temporary files**: only P9's staging directory (built collection, media copies) and temporary package file, in their U32 namespaces, removed at the end whatever the outcome and swept at the next launch after a kill (R1493). The Ready artifact lives outside those namespaces (E3).

**Atomic output** (R2004, R1624): the artifact appears only complete and verified; saving it to a user location follows E4's safe publication. Anki writes its temporary package next to the chosen path and renames it there; Meguri's two-step artifact then destination keeps a failed or cancelled export from touching a user file.

**Suggested file name**: for a `Deck` limit, the deck's full name with each of `\ / ? < > : * | " ^` replaced by `_`, plus `.apkg` (`Alpha::Beta` gives `Alpha__Beta.apkg`), as Anki desktop does; for `Collection`, `Notes` and `Cards`, a localized name of U160's plus `.apkg`. The user may change it in the save panel.

**Concurrency**: the export is non-destructive and not undoable (op catalog). It holds no exclusive phase. A collection replacement during the job makes it stale (`package/file-export#E2`): `Stale`, and its output is discarded.

## A12. Errors

| Error | When |
|---|---|
| `NoCollection`, `CollectionUnavailable` | A1: no open `Ready` collection, or an exclusive phase |
| `DeckNotFound` | A2 |
| `InvalidIds` | A8 rule 1 (R1623) |
| `NeedsCheckDatabase{reason}` | A8 rules 2 and 4, A7 (`MissingNotetype`, `MissingDeck`, `MissingPreset`, `UnreadableRecord{table, id}`) |
| `MediaNeedsCheck{name}` | A9.2 rule 5 |
| `MediaUnreadable{name}` | A9.2 rule 3; a file that cannot be opened or read |
| `MediaTooLarge{name}` | A9.2 |
| `MediaChanged{name}` | A11 step 8 |
| `LimitExceeded{what}` | A10, the ratio rule included |
| `VerificationFailed{check}` | A11 step 9: P15's reread refuses the closed package; `check` names what failed (entries, collection length or SHA-256, a map record, a frame, a SHA-1, a limit). Meguri never publishes a package its own reader refuses. The legacy writer uses the same kind (`package/legacy-export#L10`). |
| `InsufficientSpace{needed, available}` | A11 steps 4 and 7 |
| `DiskFull` | a write failed for lack of space after a preflight passed (P10) |
| `Stale` | A11 |
| `Cancelled` | the caller cancelled |
| `Io` | any other file-system failure |

Every error leaves no artifact, no staging directory and no temporary file, and changes nothing else.

## A13. Support older Anki versions (U123)

The option does not change A2 to A9: the same selection, scrub and media list. It changes the container and the collection schema only:

- `.apkg`: the A4 file is converted by `storage/schema11-downgrade` (its closed-file source, D1) and packed in the legacy layout Anki 26.9.3 writes for this option (`meta` version 2, `collection.anki21` at schema 11, the placeholder, raw media and a JSON map; `specs/conformance/model-ops.md` L6), per `package/legacy-export` (U123; steps `package/legacy-export#L7`).
- `.colpkg`: the whole collection, downgraded by `storage/schema11-downgrade` from the open collection (D1), in the same legacy layout, with or without media (A14; steps `package/legacy-export#L8`).

This spec says nothing more about that format: `package/legacy-export` L2 to L6 (container, media map, limits), L9 to L11 (preflight, verification, errors) and L13 (differences) own it.

## A14. Collection package options (U124, R1625)

The whole-collection `.colpkg` export of `package/file-export` has two options, with A2's first-use defaults and remembering:

| Include media | Support older Anki versions | Writer |
|---|---|---|
| on (default) | off (default) | `package/container#P15` with the media inventory of `package/file-export#E7` (every regular media file, referenced or not) |
| off | off | `package/container#P11`, the collection-only writer (as backups use) |
| on or off | on | U123's legacy writer (A13, `package/legacy-export#L8`) |

- The job, owned artifact, destination, cancel and restart rules of `package/file-export` E2 to E6 apply to every combination. Only the writer and its preflight differ.
- A `.colpkg` export has no limit and no scheduling or preset option: it is the whole collection as it is, scheduling, presets and the default deck included. A6 never applies to it.
- The suggested name stays E3's.

## A15. What an importer expects

The facts Meguri's package must hold for Anki 26.9.3's importer and for Meguri's (U120) to import it as the source's subset:

- `meta` `08 03`; `collection.anki21b` decodes to a collection the importer opens (schema 18, WAL header, `col.ver` 18, `schedVer` 2 from B4 so no scheduler upgrade runs).
- The importer reads the whole package collection again with its own options: notes, their cards, the cards' decks and the decks' ancestors, the notes' note types, and presets and review log only when it is asked to import them. So a deck of the package without a card is not imported (B6's default deck included), and without its scheduling option the importer resets the cards again (R1610).
- The day shift of R1609 uses the package's `crt` and `creationOffset` (A4).
- Each media map name must be safe (P6), else the importer refuses the whole package as corrupt; each SHA-1 must be the true SHA-1 of the payload, because the importer compares it with its own files to decide between reusing, adding and renaming (R1612) and does not hash the payload again. The `n`-th map record's payload is entry `n`.
- Ids more than a day ahead are repaired by the importer (R1613), but A8 refuses them first.

## A16. Differences from Anki

| Difference | Observable through |
|---|---|
| The package collection has no note type, field or template rows besides the exported ones, and no `curModel` (Anki's has its new collection's stock note types' field and template rows, with Anki's own template text, and a `curModel` naming a deleted note type). Meguri never writes stock text (R1959). | a direct dump of the package collection only; no import reads them |
| Card `data` keeps unknown keys (deviation `card-data-unknown-keys`); protobuf blobs keep unknown fields (`model/protobuf-records#P4`) | a direct dump; Anki's importer drops them anyway |
| A8 rule 2 refuses broken references that Anki writes | a damaged source collection only |
| `DeckNotFound` where Anki writes an empty package | a stale UI request only |
| A media file above 4,294,967,295 bytes is refused; Anki writes size 0 | synthetic tests only |
| Media entries in name order; Anki's order is arbitrary | no importer reads the order |
| Frames carry size and checksum (P15) | the bytes only |
| A referenced media entry that is a symbolic link, a directory or another non-regular entry gives `MediaUnreadable` (A9.2 rule 3); Anki follows links and fails with an I/O error on a directory | synthetic tests only |
| A media file that changes, vanishes or is replaced during capture gives `MediaChanged` (A11 step 8); Anki has no such check | synthetic tests only |
| Media names equal after NFC count once (A9.1); Anki on Apple platforms writes one map record and entry for each spelling, all with the same NFC name | `package_media` of a source whose fields reference one file in NFC and NFD spellings |
| A9.5 form 4 ends an unquoted `src=`/`data=` value at white space; Anki's runs to `>`, so it can miss a referenced `_` file and Meguri exports it | a template with an unquoted `_` value followed by another attribute; no ported scenario has one |
| Anki's export computes the source collection's scheduler day, so it writes the two keys any first day query writes (`sched/day-clock#D5`): `rollover` = `4` with `usn` -1 when the collection has no `rollover`, and `localOffset` = the zone's current offset when that differs from the stored value (a missing value counts as UTC, so at UTC a missing key stays missing). Decided 2026-10-27: Meguri's export makes no day query and writes neither key, nor anything else, to the source (A1); nothing in A3 to A8 needs today. The day clock computes the day at open without storing either key (`sched/day-clock#D8`, D14.2), and only an explicit compatibility query writes them | the source's `config` after an export from a collection with no earlier day query that lacks `rollover`, or whose `localOffset` is not the zone's current offset; scenarios observe `timing` first, which writes both keys on both sides (`sched/day-clock#D14.2`), or set `rollover` and a `localOffset` equal to the zone's offset in their setup (`conformance/package-ops#E7`) |

No ported scenario observes any of them. A scenario that dumps the package collection directly needs a `conformance/deviations.yaml` entry for the first row, and one whose template has an unquoted `_` value followed by another attribute needs one for the A9.5 row.

## A17. Scenarios and tests

Status (2026-10-27): the oracle driver implements E6 to E11 of `specs/conformance/package-ops.md`, and every scenario of the table below is recorded; Meguri's executor implements none of them yet. The forms and observations they use:

- `export`, deck-package form: `conformance/package-ops#E7` (capability `export.apkg`; output `{notes: <count>}`; rejections `invalid_ids` and `media_check_required`; a `{deck: <name>}` limit that names no deck is `unknown_ref`).
- `export`, collection-package options: `conformance/package-ops#E9` (U124).
- `import`, deck-package form, for the live chains: `conformance/package-ops#E6`.
- Observation `package_media`: `conformance/package-ops#E10`, `[{name, size, sha1, entry_size}]` in map order, `entry_size` the decoded size of the numbered entry; scenarios compare it under `compare.set`.
- Observation `package_db_dump`: `conformance/package-ops#E11`, the normalized dump of the decoded `collection.anki21b` read with plain SQLite.

Scenarios to record (`conformance/scenarios/package/apkg-export/`):

| Scenario | Shows |
|---|---|
| `deck-with-subdecks` (live, T1) | Meguri exports deck `A` (`A`, `A::B`, empty `A::C`, `A::B::D`) with scheduling and presets, then deletes `A` and its subdecks; after the handoff Anki imports the package into that collection with scheduling and presets; the dump (graves of the deletion included) equals Anki's own export, deletion and import of the same deck. The package's `crt` is the importing collection's own (A4). |
| `listing` | `package_listing` of Meguri's package equals Anki's: entry names, order, methods, `meta_version` 3, the placeholder (A10) |
| `no-scheduling` | `package_db_dump` of Meguri's and Anki's packages without scheduling: A6's card table (every type and queue, suspended, buried, learning with `left`, review with and without `pos`, new in a filtered deck, flags), `nextPos` 7, the tags `marked`, `Leech`, `leech::x`, deck limits, today counters, collapse states, filtered deck converted, source deck 1 left out and B6's default deck row in its place with B6's bytes (`common` `08 01 10 01`), note `flags` and `data` written 0 and empty |
| `with-scheduling` | the same source with scheduling: rows unchanged, review log present, filtered deck kept with its home decks |
| `filtered-home-missing` | with scheduling, a card whose `odid` names a deleted deck: every filtered card returned home in place, filtered deck converted (A5) |
| `presets` | presets off (every `config_id` 1, only the default preset), on with scheduling (FSRS lists kept), on without (lists emptied), a deck on preset 1 (A7) |
| `card-selection` | one card of a two-card note whose cards sit in two decks: both cards, both decks and their ancestors (A3) |
| `note-selection` | three notes across two subdecks: exactly those notes, their decks and ancestors |
| `deck-limit-reach` | a note with cards in `A` and `B`, export `A`: `B` comes too; a card of `A` held by a filtered deck outside `A` does not select its note (A2) |
| `default-name-clash` | deck 1 renamed `Inbox`, another deck named `Default` with notes, one of them with a second card in deck 1; export of `Default` without scheduling: `package_db_dump` equals Anki's (no row with id 1, the second card's `did` 1) (A4) |
| `media-selection` | fields `[sound:a.mp3]`, `<img src="b&amp;c.jpg">`, a remote image, a missing file, `[latex]x^2[/latex]` with its image present; CSS `url(_f.woff)`, `@import "_base.css"`, `@import "_t.CSS"`; template `"_x.js"`; an unused `c.png`: `package_media` equals Anki's set (sizes and SHA-1; order not compared) |
| `media-unnormalized` | a field references an existing `con.jpg`: both refuse, `media_check_required` |
| `future-ids` | a note id three days ahead: both refuse, `invalid_ids`; an export of another deck succeeds. That a refused export writes no file is not observable with the current kinds: Meguri's own A8 test checks it |
| `empty-selection` | a deck whose notes all moved away: `{notes: 0}`, package listing equal |
| `selection/invalid-ids` | A8 rule 1 across the selection: a note id at microsecond scale refuses a whole-collection export with `invalid_ids`, and an export of another deck passes. A note id equal to the export instant plus 86,400,000 passes (the package holds it) and one millisecond more refuses. A card id past the bound refuses without scheduling. A review-log id past it refuses only with scheduling; without scheduling the export passes with an empty package `revlog` |
| `selection/whole-collection` | a `Collection` limit takes every note, a card-less one included, with its stored row (`mod` and `usn` as stored), every card, and the decks and ancestors the cards name (A2, A3) |
| `selection/note-ids` | a `Notes` limit takes exactly the listed notes that exist, a card-less one included, and ignores a deleted note's id (A3) |
| `selection/card-ids` | a `Cards` limit takes each listed card's note once with all its cards and only their decks, and ignores a deleted card's id (A3) |
| `selection/deck-children` | a `Deck` limit takes the deck's descendants, not the sibling decks whose names share its prefix, and adds the parent as an ancestor row without its notes; an empty subdeck stays out (A2, A3) |
| `selection/empty-collection` | a collection with no note exports `{notes: 0}` with every option off and with every option on: the latest layout, no note, card or review-log row, only the default deck; the deck-package import reads the package back with no note (A3, A10) |
| `roundtrip-deck-limit` | a deck export, latest and legacy, of `Plot` with a subdeck; the target emptied and each package imported with progress and presets: only the deck's notes return, the deck and subdeck are recreated, an unrelated deck's note and the default deck's note stay out (A2, A3) |
| `roundtrip-collection` | the whole collection without scheduling, both formats: every note returns to its deck, the default deck's note to the target's default deck (A3, A4, A6) |
| `roundtrip-media` | a referenced file exported with media, both formats, imported into an emptied media folder: the same bytes (size and SHA-1); an unreferenced file in the folder never travels (A9) |
| `roundtrip-with-scheduling` | a reviewed card imported with progress on keeps its type, queue, interval and repetitions, and its review log returns; its `due` is unchanged here only because source and target are the same collection (no day shift, `package/apkg-import#I11.2`); a new card stays new (A5) |
| `roundtrip-without-scheduling` | the same reviewed cards from a package without scheduling, imported with progress on: new, with no repetitions, lapses, interval or factor, and no review log (A6) |
| `roundtrip-fsrs-params` | a preset holding all three FSRS lists (`fsrsWeights`, `fsrsParams5`, `fsrsParams6` in its legacy JSON), exported with presets, both formats: without scheduling it imports with all three lists empty and its other options kept; with scheduling they are kept (A7) |
| `fsrs-params-gathered` | the same preset in the packages themselves: `package_col` `dconf` of the legacy packages shows the three lists empty without scheduling (other options kept) and as stored with scheduling; in the latest format the preset's `deck_config` row differs between the two packages, while a preset without FSRS lists is identical (A7) |
| `colpkg/export-with-media` (live, T1, U124) | Meguri's `.colpkg` with media from the standard collection and two files; Anki removes the files and imports it; collection dump and media folder equal the source's (`col.scm` and `col.mod` the import instant), and `package_listing` shows the latest layout with two numbered entries |

Goldens are recorded with `tools/mg oracle record package/apkg-export` at the pin of `conformance/oracle.json`.

Meguri's own tests (`core/crates/meguri-package/tests/apkg_export.rs`, U122):

- A whole-collection export with scheduling imported into a new Meguri collection: the normalized dump equals the source's, except the documented fields (R1936).
- A8: a card id three days ahead gives `InvalidIds` and creates no staging directory, temporary file or artifact; each broken-reference case gives its reason; a truncated deck `kind` blob, a deck `kind` with neither member, a truncated note type `config` and, with presets, a truncated preset `config` give `UnreadableRecord` with their table and id, before staging.
- A4's name clash, with and without scheduling: no row with id 1, the export succeeds, and Meguri's importer puts a `did` 1 card in the target's default deck.
- A9.1: one file referenced as NFC `é.png` and as NFD `e` + U+0301 `.png` gives one entry, in the latest and the legacy format (where P17 rereads the map).
- A10 and A11 step 9: a collection of repeated bytes larger than 16 MiB that compresses past 1,000 times gives `LimitExceeded`; a corrupted entry injected before the reread gives `VerificationFailed`; both leave no artifact.
- Cancel at 50% of media capture: `Cancelled`, no artifact, staging and temporary file removed. A disk-full failpoint during writing: `DiskFull`, the same cleanup. Injected low space before each preflight: `InsufficientSpace` and nothing written.
- A media file rewritten in place (same length) during capture, and one replaced by another file: `MediaChanged`. A referenced symbolic link and a directory: `MediaUnreadable`.
- Export of a 150k-card synthetic collection while the interactive lane commits answers: no answer waits on the export; the package holds the snapshot taken at step 2. Bench `benches/export.rs` against U12's baselines.
- The live collection file, its `-wal` and the media folder keep their bytes during an export with no other op running.
- A6's position table, A9.3 to A9.5's tables and A9.2's rules as unit tests.
