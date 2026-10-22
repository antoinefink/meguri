# Normalized dumps and the preservation suite

Spec: conformance/normalized-dump
Requirements: R2017, R322, R323, R324, R582, R583, R1483, R1484, R1485, R1486, R1586, R1587, R1590, R2016; AE66, AE83
Anki: 26.09.3
Scenarios: conformance/scenarios/preservation/

How the suite proves that data Meguri does not use survives open, edit, review, render and sync unchanged, and that Meguri creates no sync churn: the dump comparison rules (N2 to N4), the `adopt` op (N5), the pending-change check (N6), the dump differences Meguri's own tests use (N7), the kitchen-sink fixture (N8, N9), the scenarios (N10), the legs that later units add (N11) and Meguri's own churn tests (N12). The observation `db_dump` is `observations.md` O8; scenario language and statuses are `dsl.md`; sync parties and `converge` are `sync-ops.md`.

## N1. The suite

- **Fixture.** One oracle-made collection, `conformance/fixtures/preservation/kitchen-sink.anki2`, holds every kind of data R2017 lists, in the state a completed sync leaves (N9). Its recipe is the scenario `preservation/recipe/kitchen-sink`, whose golden documents the content.
- **Scenarios** (N10) open the fixture (the sync scenario builds its own collection), run the same steps on Meguri and on the oracle, and compare dumps at each observe step with the oracle's golden (T0), or run live with sync parties (T1). Zero differences after deviations is the R2017 comparison.
- **Legs** (N11): the behavior that makes a leg pass belongs to the unit that implements it (review U43/U44, render U60/U62, sync U107/U117, device-local settings U160, optimizer U47, note type structure changes U34, the Migaku startup path U137). Each leg's scenario becomes active, or its Meguri-only test lands, with that unit. U36 owns the fixture, these rules and the suite's wiring.
- **Meguri-only tests** (N7, N11, N12) compare two raw dumps of one Meguri collection where Anki has no counterpart (device-local settings, the 1,000-op run, the integration startup path).

## N2. Comparing dumps

A dump is the value of `db_dump` (O8): the twelve tables of the schema-18 file, every column of every row, keyed by primary key. Dumps compare as JSON values (O7), which makes these rules:

- **Every column counts.** Nothing is left out beyond N3's random values: modification times, `usn`, `col.mod`, `col.ls`, `col.scm` and config `mtime_secs` are compared like any other value. A stamp that moves marks a row as changed for sync (R1472, R1586), so it is a preservation failure.
- **Storage class counts.** INTEGER 1, REAL 1.0 and TEXT `"1"` differ (O8's value forms).
- **BLOBs compare as bytes.** A BLOB is reported by the SHA-256 of its bytes, so equal digests mean equal bytes. The protobuf blobs (`notetypes.config`, `fields.config`, `templates.config`, `decks.common`, `decks.kind`, `deck_config.config`) and every `config.val` (JSON text stored as a BLOB) are therefore compared byte for byte: a re-encoded blob, a JSON object written with other member order, spacing or number spelling (`1.0` for `1`, `0.3` for `0.30000000000000004`) is a difference even when the decoded values are equal. A failure report may decode a differing blob to show what changed; the decision never uses the decoded form.
- **TEXT compares as stored**, character for character: `notes.flds` with its U+001F separators, `notes.tags` with its surrounding spaces, `cards.data` as JSON text (a rewritten `data` with the same values in another order or float spelling is a difference).
- **Rows** present on one side only are differences; so is a table present on one side only.
- O7's numeric tolerances never apply to `db_dump`.

## N3. What is masked

- The plain `db_dump` (O8) hides the values Anki draws at random while a scenario runs: `notes.guid` reads `#random`, and the digests of `fields.config` and `templates.config` leave out the field and template ids. Scenarios that create notes or note types in their own run need this to have a stable golden.
- Nothing else is ever hidden in a preservation comparison, except the unknown-field bytes N11.7 takes out of Vocab's rows, whose presence that scenario checks on its own.
- Across a sync, `converge` (sync-ops S7.3) excludes `col.mod`, `col.usn`, `col.ls` and config `mtime_secs` and `usn`, which record each party's sync history. That list is the only exclusion; the sync area pins those columns per party in its own scenarios (sync-ops S12).
- Scenarios that start from the fixture use the raw form (N4): there every guid and id comes from the fixture file, so nothing needs hiding, and hiding it would let a rewritten guid or field id pass. The one exception is N11.7: after adding a field and a template, whose ids are random, it dumps `notetypes`, `fields` and `templates` in the plain form, and reads the rows with fixture ids raw in a later step.

## N4. Raw dumps

- **Request** `{db_dump: {raw: true}}`, with or without `tables`. `raw` is a boolean, default false.
- **Value** as O8, except that `notes.guid` is the stored text and every BLOB digest covers all of its stored bytes. It equals the raw dump of `sync-ops.md` S7.2 restricted to the requested tables; both executors already compute that dump for `converge`.
- Use it only where every guid and every field and template id in the requested tables comes from a fixture or from values the scenario sets; otherwise two oracle runs disagree and the golden is unstable.

## N5. `adopt` (op)

Gives the notes and cards of an opened fixture logical ids, so that `edit_note`, `answer`, `card`, `revlog` and `template` card cases can name them.

- **Argument** `{}`. **Output** `{adopt: {notes: <count>, cards: <count>}}`: how many notes and how many cards got a logical id in this step.
- **Effect.** Every note of the collection that has no logical id gets the next `#nN`, in ascending note `id` order; then every card that has none gets the next `#cN`, in ascending card `id` order. Notes and cards that already have one keep it. Nothing in the collection changes: no row, stamp, `col.mod` or undo history.
- A later `adopt` numbers only what has no logical id yet, such as cards a note edit added (`edit_note` gives them none, `note-ops.md` N3).
- `adopt` makes no names known: ops that name a deck or note type of `setup` (`add_note`) stay unusable in a scenario without setup. Ops that find records by their stored name (`legacy_json`, `legacy_update`, `blob_decode`) need nothing.
- Not allowed in actor scenarios (the scenario schema rejects it). After a refused `open` only another `open` may follow (`storage-ops.md` P1.2).
- Since Anki allocates ids in creation order, `adopt` on the kitchen-sink fixture gives each note and card the logical id its recipe gave it, and `#c19` to the card the recipe's note edit added (N9.3).

## N6. Nothing pending

### N6.1 The pending-change query

Scenarios and Meguri's tests read what the next sync would carry with this one statement (through `db_exec`, `record-ops.md` W1), which returns one row of twelve integers:

```sql
SELECT (SELECT count() FROM notes WHERE usn = -1), (SELECT count() FROM cards WHERE usn = -1), (SELECT count() FROM revlog WHERE usn = -1), (SELECT count() FROM decks WHERE usn = -1), (SELECT count() FROM deck_config WHERE usn = -1), (SELECT count() FROM notetypes WHERE usn = -1), (SELECT count() FROM templates WHERE usn = -1), (SELECT count() FROM tags WHERE usn = -1), (SELECT count() FROM config WHERE usn = -1), (SELECT count() FROM graves), (SELECT mod > ls FROM col), (SELECT scm > ls FROM col)
```

| # | Value | Meaning |
|---|---|---|
| 1 to 8 | rows with `usn` -1 in `notes`, `cards`, `revlog`, `decks`, `deck_config`, `notetypes`, `templates`, `tags` | local changes the next normal sync sends (`storage/ids-usn-mtime` S2) |
| 9 | config entries with `usn` -1 | keys written locally since the last sync. Config travels whole when `col.mod` is newer (`storage/config-table` C9), so this is not what selects them; it shows that a write happened |
| 10 | graves | deletions not yet sent (`storage/graves` G5): a sync removes the graves it sends |
| 11 | 1 when `col.mod` is after `col.ls` | the collection changed since its last sync: a normal sync has something to send |
| 12 | 1 when `col.scm` is after `col.ls` | the schema changed since its last sync: the next sync is one-way (R1590) |

A clean collection gives `[[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]]`. The `fields` table has no `usn`; Anki leaves `templates.usn` at 0 (`model/schema11-json` S6), so column 7 counts only what another writer left. A collection that never synced (`ls` 0) is never clean by this query; the fixture has synced (N9.3).

### N6.2 Media

Nothing pending for media means `media_pending` (`media-ops.md` F11) is `{}`. The fixture is a collection file only (`open` copies no media folder), so media churn is covered by Meguri's own tests (N11.6) and by U117's AE66 sync scenario; a preservation scenario adds a `media_pending` observation once a scenario has a media folder of its own.

### N6.3 What opening a collection writes

Observed on the oracle and stated here so that Meguri's storage-level open path can be held to it. The real engine's automatic day check follows `sched/day-clock` D8; an explicit compatibility timing query has the separate writes of D5/D7.

- Opening and closing a schema-18 collection writes nothing, after any idle time, outside the cases below. `preservation/churn/reopen-ten` and `churn/relaunch` show it on the fixture: the raw dump and `sqlite_master` after ten reopens, and after 100 reopens spread over eight days with day rollovers between them, equal those after the first open, and the N6.1 query stays clean.
- Anki's open and day computation have these writes:
  - the Python layer adds `creationOffset` at open when the v3 scheduler is on and the key is missing (`storage-ops.md` P1.1);
  - the scheduler's first day computation writes `rollover` (4) when it is missing;
  - the same computation writes `localOffset` when the device's UTC offset differs from the stored one, a missing key reading as 0 (`sched/day-clock`, U40);
  - the unbury check writes `lastUnburied` when D7's day condition holds, even when no cards are buried.

  The last three happen when the scheduler computes "today" (answering, building a queue), never at storage open itself. The fixture has `creationOffset` and `rollover`, was made at UTC and is used at UTC; its raw reopen scenarios request no timing query.
- With no actual unbury or offset change, Meguri writes nothing when it opens, closes, or sits idle (R1586, R1590), including when idle time crosses days (N12). The automatic day clock never stamps an empty unbury check or repairs missing rollover config (D8). A Meguri feature that needs a write at launch (bundled media, R1587) writes only when the content differs (N11.6).

## N7. Dump differences (Meguri's own tests)

Meguri's tests with no oracle counterpart (N11, N12) compare two raw dumps (N4) of one Meguri collection, taken by the same function the executor's `db_dump` uses:

- A difference is an RFC 6901 pointer at the first level where the dumps disagree: `/<table>/<key>` when a row exists on one side only, `/<table>/<key>/<column>` when a value differs. Values compare exactly as JSON. This is `sync-ops.md` S7.4's rule without exclusions; differences are listed in its pointer order.
- Each test names the pointer patterns it allows (`observations.md` O3: `*` matches one segment). Any other difference fails the test, and the failure lists every differing pointer with its value before and after (the first 20, then the count).
- The helpers live in `core/crates/meguri-testkit/src/dump/preservation.rs`: the raw dump of an open Meguri collection, the difference list, the allowed-pattern check and the N6.1 query.

## N8. What the fixture preserves

| Data (R2017's list) | Where in the kitchen-sink fixture | Requirement |
|---|---|---|
| Unknown config keys, other clients' and add-ons' | `heatmap`; `onigiri_sidebar_collapsed` with its own spacing and `usn` 4; `addon_float` (`0.30000000000000004`), `addon_big` (`18446744073709551615`), `addon_nested` (members unsorted); `_nt_<id>_lastDeck`, `_deck_<id>_lastNotetype` | R323 |
| Reviewer and scheduling keys Meguri does not use | `setDueReviewer` `"3-7"`, `restorePositionReviewer`, `resetCountsReviewer`, `cardStateCustomizer` (a script), `fsrsLegacyEvaluate`, `renderLatex`, `dueCounts`, `estTimes` | R323, R582, R1484 |
| Preset fields | Steady: `fsrsWeights` (17 values), `fsrsParams5` (19), `fsrsParams6` (21), `fsrsParams7` (a future list, kept in `other`), protobuf field 7 (packed `[4.0]`, a future list Anki does not declare) and field 39, `easyDaysPercentages`, `sm2Retention`, `weightSearch`, `ignoreRevlogsBeforeDate`. Legacy: SM-2 ease settings, `fsrsParams5` only (empty `fsrsParams6`), an add-on block `addonLegacy` in `other` | R1484, R1486 |
| Filtered decks | Cram (reschedules; two search terms; `delays`, `previewDelay` and preview seconds) and Custom Study Session (previews); their cards keep `odid` and `odue` and sit at positions from -100000 | R1483 |
| Custom study state | Languages::Japanese: today's new count -5 (a raised limit), `extendNew` 3; Languages: `extendNew` 5, `extendRev` 10; `_deck_<id>_customStudyIncludeTags` and `…ExcludeTags` | R1484 |
| Custom scheduling data | `cardStateCustomizer`; `cd` in `cards.data` of `#c1` and `#c6` | R1484, R2017 |
| Review-log entry types | types 0 to 5: learning, review, relearning, two type 3 (an early review in Cram, a preview in Custom Study Session), a manual entry (4) and a reschedule entry (5) | R1483, R2017 |
| MathJax, LaTeX, TTS and sound content | fields with `\(…\)`, `\[…\]`, `[$]…[/$]`, `[$$]…[/$$]`, `[latex]…[/latex]`, `[anki:tts …]…[/anki:tts]`, `[sound:…]`; templates with the `tts`, `furigana`, `kana`, `type`, `hint` and `cloze` filters and two add-on filters (`sparkle`, `mathpretty`) | R583 |
| Note type data Meguri does not expose | LaTeX header, footer and `latexsvg`; `req`; browser templates (`bqfmt`, `bafmt`, `bfont`, `bsize`); `originalStockKind`, `originalId`; field `description`, `plainText`, `collapsed`, `excludeFromSearch`, `preventDeletion`; Formula's legacy key `mathAddon` in `other`; Vocab's unknown protobuf fields (note type field 20, its first field's field 20, its first template's field 30) | R1485 |
| Deck data Meguri does not edit | `browserCollapsed`, `collapsed`, descriptions (one Markdown), limit and desired-retention overrides, Languages' legacy key `provenance` in `other`, Math's unknown protobuf fields (common field 9, kind field 3) | R322 |
| Card and note states | flags 1 and 7, a suspended card, a card in learning, a card in the preview queue, `marked` and `leech` tags, hierarchical tags, a tag shown expanded | R2017 |

Anki drops undeclared protobuf fields when it re-encodes a record and Meguri keeps them (`model/protobuf-records` P4, P5; deviation `notetype-unknown-fields-kept` for note types), so the records that carry them are saved again by one scenario only, `preservation/notetype/structure-changes` (N11.7). It compares Vocab's rows without the unknown-field bytes and asks separately whether they are still there. A leg that saves another of those records (the optimizer on Steady, N11.5) asserts Meguri's bytes in its own test, or adds a deviation under R1463 or R1486 whose expected digest is computed from P4.

## N9. The kitchen-sink fixture

### N9.1 Files

- `conformance/fixtures/preservation/kitchen-sink.anki2`: the collection file. Made only by the oracle maintainer's maker, never by hand, by Meguri or from `local-data/`.
- `conformance/scenarios/preservation/recipe/kitchen-sink.yaml`: the recipe. Its golden's last step holds the fixture's normalized dump and the schema-11 objects of its note types, decks and presets.

### N9.2 Making it

The maker (`oracle/fixtures/preservation/`):

1. Runs the recipe's steps on the oracle, at the recipe's clock and time zone, without its last observe step.
2. Keeps the session's collection file, closed without the downgrade: schema 18, WAL journal mode, every change checkpointed into the main file, no `-wal`, `-shm` or `-journal` file shipped.
3. Leaves no byte of Anki's stock note types in the file, which the recipe deletes with `db_exec` (R1959): secure deletion on the connection before those statements, or a vacuum afterwards that keeps the journal mode. The file is checked like the storage fixtures (no piece of a stock note type's CSS or templates).
4. Checks the result, and writes nothing unless every check passes:
   - SQLite's integrity check reports `ok`, and the journal mode is `wal`;
   - `tools/mg oracle check` on the file reports no Check Database problem;
   - opening it on the oracle and making the observations of `preservation/fixture/open` gives exactly the value of the recipe golden's last step (the normalized dump hides the guids, the only values a remake changes).

Remaking the fixture changes the notes' guids and every page layout, so every golden recorded on it is re-recorded with it (`tools/mg oracle record preservation`); the recipe's golden does not change.

### N9.3 Contents

- Created 2030-01-01 09:00 UTC (`crt` 1893470400, 04:00 that day; `scm` 1893488400000). Day 0 is 2030-01-01. Scenarios that open it start at 2030-01-15T10:00:00 UTC, day 14.
- Synced state, as a normal sync leaves it when this client sent everything (C9 of `storage/config-table` for config): every row that had `usn` -1 holds 9, the server's usn of that sync; the rows of Anki's new collection and every template keep 0; config entries hold 0, except `onigiri_sidebar_collapsed` (4); no graves; `col.usn` 10; `col.mod` and `col.ls` both 1894644000000 (2030-01-14 18:00 UTC). So the N6.1 query is clean.
- Records (ids are real ids):

| Kind | Name | Id | Notes |
|---|---|---|---|
| deck | Default | 1 | empty, preset Default |
| deck | Languages | 1893488400000 | preset Steady |
| deck | Languages::Japanese | 1893488400001 | preset Steady |
| deck | Math | 1893488400002 | preset Legacy; unknown protobuf fields |
| filtered deck | Cram | 1894093200000 | reschedules |
| filtered deck | Custom Study Session | 1894093200001 | previews |
| preset | Default, Steady, Legacy | 1, 1893488400000, 1893488400001 | |
| note type | Vocab (standard: Word, Meaning, Reading, Audio; Recognize, Recall) | 1893488400006 | field ids 3001 to 3004, template ids 4001, 4002; unknown protobuf fields |
| note type | Gaps (cloze: Text, Extra; Cloze) | 1893488400007 | field ids 5001, 5002, template id 6001 |
| note type | Formula (standard: Front, Back, Source; Statement) | 1893488400008 | field ids 7001 to 7003, template id 8001; LaTeX settings |

- Notes `#n1` to `#n12` have ids 1893488400000 to 1893488400011 and cards `#c1` to `#c19` ids 1893488400000 to 1893488400018, so `adopt` numbers them as below.

| Card | Note (sort field) | Template | Deck (home deck) | State |
|---|---|---|---|---|
| #c1 | #n1 (食べる) | Recognize | Cram (Languages::Japanese) | review, position -100000, `cd` |
| #c2 | #n1 | Recall | Languages::Japanese | new, flag 1 |
| #c3 | #n2 (飲む) | Recognize | Languages::Japanese | review after a lapse |
| #c4 | #n2 | Recall | Languages::Japanese | new, flag 7 |
| #c5 | #n3 (本) | Recognize | Cram (Languages::Japanese) | review, position -99999 |
| #c6 | #n3 | Recall | Languages::Japanese | new, `cd` |
| #c7 | #n4 (水) | Recognize | Languages::Japanese | review (answered early in Cram, back home) |
| #c8 | #n4 | Recall | Languages::Japanese | new |
| #c9 | #n5 (学校) | Recognize | Custom Study Session (Languages::Japanese) | preview queue |
| #c10 | #n5 | Recall | Languages::Japanese | new |
| #c11 | #n6 (静か) | Recognize | Languages::Japanese | learning since day 0 |
| #c12 | #n6 | Recall | Custom Study Session (Languages::Japanese) | new, waiting, position -99999 |
| #c13 | #n7 (Paris…) | Cloze c1 | Languages | review |
| #c14 | #n8 (Water boils…) | Cloze c1 | Languages | new |
| #c15 | #n9 (Area of a circle) | Statement | Math | review |
| #c16 | #n10 (Euler's identity) | Statement | Math | review |
| #c17 | #n11 (Quadratic formula) | Statement | Math | new |
| #c18 | #n12 (Derivative of sine) | Statement | Math | suspended |
| #c19 | #n7 | Cloze c2 | Languages | new (added by the recipe's note edit) |

- Tags: `adjective`, `algebra`, `jlpt::n4`, `jlpt::n5`, `leech`, `marked`, `noun`, `skip`, `verb`; all collapsed except `verb`.

### N9.4 Templates

The recipe holds the template texts; `preservation/render/every-card` repeats them as card cases. Recognize puts the word over a `tts` filter; Recall shows the meaning in a conditional, then a type-in field, and on the back the `kana` filter and the add-on filter `sparkle`; Cloze adds a `hint` filter on the back; Statement shows the add-on filter `mathpretty` on the Source field. Unknown filters leave the field text as it is (the render golden shows it).

## N10. Scenarios

| Scenario | Covers | Meguri needs | Leg owner | Golden |
|---|---|---|---|---|
| `preservation/recipe/kitchen-sink` | the fixture's content (N9) | setup sections, `legacy_update`, `answer`, `edit_note`, `suspend`, `set_config`, `db_exec`, `legacy_json` | none: the oracle's recipe, never active | recorded |
| `preservation/fixture/open` | Meguri reads every stored value of the fixture as Anki does | `open`, `db_dump`, `legacy_json` | U36 | recorded |
| `preservation/churn/reopen-ten` | R2016, R2017: ten reopens write nothing | `open`, `reopen`, `schema_objects`, raw `db_dump` | U36 | recorded |
| `preservation/churn/relaunch` | AE66 (the part Anki shares), R1586, R1590: 100 reopens over eight idle days leave nothing pending | `open`, `reopen`, `advance`, `db_exec`, raw `db_dump` | U36 (U137 adds the integration path, N11.6) | recorded |
| `preservation/edit/unrelated-note` | AE83 (local part), R322, R323, R1483 to R1486: a note edit changes that note only | `open`, `adopt`, `edit_note`, raw `db_dump` | U36 with U27's `edit_note` | recorded |
| `preservation/review/answers` | R582: 200 reference answers write no config key; unsupported in Meguri | `open`, `adopt`, `answer`, `advance`, raw `db_dump` | reference only; supported proofs: U43, U44 | recorded |
| `preservation/render/every-card` | R583: rendering every card changes nothing | `open`, `adopt`, `template` card cases, raw `db_dump` | U60, U62 | recorded |
| `preservation/sync/filtered-deck` | AE83: filtered decks, their cards and type 3 entries survive a trip through Meguri and a sync | actor `meguri`, `sync`, `edit_note`, `db_dump` | U107, U117 (T1) | recorded |
| `preservation/notetype/structure-changes` | R1485, R2017: a field add, a field rename and a template add change only what they change (N11.7) | `open`, `adopt`, `update_notetype` with the edits `add_field`, `rename_field` and `add_template` (`notetype-ops.md` T10.1), `db_exec`, raw and plain `db_dump`, `notetype_structure` | U34 | recorded |

What the oracle does in each, confirmed by running each scenario's steps after the recipe's (so with the fixture's content, though not its file), and `notetype/structure-changes` on the fixture file itself:

- `churn/reopen-ten`, `churn/relaunch`: no difference at all; `sqlite_master` keeps its 30 objects (the twelve tables, sixteen indexes, `sqlite_stat1` and `sqlite_stat4`); the N6.1 query is clean before and after.
- `edit/unrelated-note`: exactly four values change: `col.mod` (the edit's instant), and the edited note's `flds`, `mod` and `usn` (-1). Its sort field and checksum stay, since the edit leaves the first field alone. The note type's, fields' and templates' blobs, the filtered decks and every card keep every byte.
- `review/answers`: config, presets, note types, notes and tags keep every byte; only `col.mod` and the two Languages decks' `common` (day counters), `mtime_secs` and `usn` (-1) change. No key of R582's list is written: they are written by Anki desktop's reviewer screens, not by answering.
- `render/every-card`: no difference.
- `sync/filtered-deck`: `converge` finds no difference on any party, the dumps after each sync show the filtered decks and their cards as client_a made them, and Meguri's edit travels as an incremental sync (`no_changes`, R1590).
- `notetype/structure-changes`: each update changes its note type's `notetypes` row (`config`, `mtime_secs`, `usn` -1), the renamed field's `fields` row (`name`), each template that uses the renamed field (`config`: the reference in its text follows the rename), and adds one `fields` row and one `templates` row; template rows keep `mtime_secs` and `usn` 0 (`model/schema11-json` S6). Every note of the note type gets an empty last field (`flds`, `mod`, `usn` -1; sort field and checksum unchanged), and `col.mod` and `col.scm` take the update's instant. No card is made (each new template's front uses only the new, empty field), and no card, deck, preset, tag or config entry changes. Anki writes Vocab's note type, first field and first template without their unknown fields, the first field too although no edit touches it (a save rewrites every field and template row of the note type).

`preservation/review/answers` records 200 reference answers on ten cards of the fixture's Steady preset. That preset has the later-list markers of `sched/fsrs-transitions#T13`; Meguri must refuse its answers under `sched/fsrs-policy#U4`. Preserve this complete reference input and golden; it is not a successful Meguri study case. U44 must prove R582 and `sched/fsrs-policy#K3` step 1 with 200 real answers, including rollover, on supported-home cards while preserving the fixture's opaque and unsupported data. U43 separately supplies N11.1's session API proof on supported-home fixtures. U246 reports the original reference's scope separately from those successful supported proofs.

Under `dsl.md` D9.4, `preservation/recipe/kitchen-sink` and `preservation/review/answers` are reference cases in area metadata. Keep their source documents and expected observations unchanged. Validate both for freshness and replay both on the oracle in T1. Neither establishes Meguri compatibility; N11.1 and `sched/fsrs-policy#K3` still require successful supported-card tests.

For the other scenarios, `area.yaml` lists a scenario as active in the change that lands what Meguri needs for it (`dsl.md` D9.1).

U36 also runs on Meguri's executor, with its `legacy_json`, `legacy_update` and `blob_decode` adapters and `db_exec`, the model scenarios `model/proto/` and `model/schema11/` (all but `apkg-notetypes`) and `render/template/legacy-req` (`model-ops.md` L1, L2, L4; `render-ops.md` Q3); their specs are unchanged.

## N11. Legs added by later units

### N11.1 Review session (U43)

A 200-card review session through the session API on the fixture: the raw dump difference (N7) is limited to the answered cards' `cards` rows, their new `revlog` rows, the studied decks' `common`, `mtime_secs` and `usn`, `col.mod`, and the config keys `curDeck`, `setDueReviewer`, `restorePositionReviewer` and `resetCountsReviewer`, each only when the session itself writes it. `dueCounts`, `estTimes` and every other key keep their bytes, `usn` and `mtime_secs` (R582). `preservation/review/answers` is the same check against the oracle for the answers alone.

N11.1's session proof uses supported-home cards and retains every stated preservation assertion. It must not study the Steady cards in the preserved reference case: R704 requires their refusal. U44's operation-level preservation proof does not substitute for this session API proof.

### N11.2 Render (U60, U62)

Rendering every card of the fixture through Meguri's render pipeline (stored templates, play markers, AV tags, MathJax and LaTeX left in place) leaves the raw dump unchanged (R583). U62 adds the per-card `render` kind to `preservation/render/every-card`, or a sibling scenario, when the kind leaves draft.

### N11.3 Sync (U107, U117)

`preservation/sync/filtered-deck` runs in T1 once a Meguri actor can sync. U117's R2017 sync slice adds the fixture's other data (unknown config keys, preset fields, revlog types) to its sync scenarios.

### N11.4 Device-local settings (U160)

Toggling every device-local setting through the UI test hook, with the fixture open, leaves the raw dump unchanged, config table included (R324, `storage/config-table` C8). Meguri-only: Anki has no such settings.

### N11.5 Optimizer (U47)

- An optimizer run that adopts new parameters for Steady changes, in that preset's `config` blob, only the bytes of field 6 (`fsrs_params_6`), as `model/protobuf-records` P4's example shows; `fsrsWeights`, `fsrsParams5`, Easy Days, field 7, field 39 and `other` (`fsrsParams7`) keep their bytes. Besides the preset's `mtime_secs` and `usn`, only `col.mod` and the keys U47's spec names (such as `lastFsrsOptimize`) may change (R1486, R696).
- For Legacy, whose `fsrsParams6` is empty and `fsrsParams5` set, the write fills `fsrsParams6` and leaves the `fsrsParams5` bytes as they were.
- Anki drops fields 7 and 39 when it saves the preset (N8), so this leg is a Meguri test, or a scenario with a deviation.

### N11.6 Integration startup (U137; a stub until it lands)

100 relaunches of the fixture with the Migaku integration's startup path on (until U137: U31's static-media step with a fixture font set standing in for the integration's bundled files): the N6.1 query is clean and the raw dump is unchanged; in the media folder only the first launch may add a missing bundled file, and every later launch adds or rewrites none, since each bundled file is already present with the same SHA-1, so `media_pending` lists nothing after the first sync (R1586, R1587; AE66). A sync after the loop sends no note type, config or media (U117).

### N11.7 Note type structure changes (U34)

`preservation/notetype/structure-changes`, active once U34 declares the edits it uses (`notetype-ops.md` T10.1). On the fixture, a field add, a rename of a field the templates use and a template add on Formula (LaTeX header, footer and SVG option, `mathAddon` in `other`), then the same three on Vocab (LaTeX header and footer, browser templates, `originalStockKind`, `originalId`, field options, the unknown protobuf fields of N8). Each update is one `update_notetype` step, a checked save.

- **Other tables.** After each update the raw dump of the nine tables that hold no note type blob (`col`, `notes`, `cards`, `revlog`, `graves`, `decks`, `deck_config`, `tags`, `config`) equals the oracle's: only that note type's notes and `col.mod` and `col.scm` change (N10).
- **Formula, byte for byte.** After Formula's update the plain dump (O8) of `notetypes`, `fields` and `templates` equals the oracle's. Every R1485 value keeps its bytes apart from what the edits change: the field's name, the template text that names it, the new field and template, and `req`, which the save recomputes. The plain dump masks the new rows' random ids; every other id comes from the fixture.
- **Vocab, without its unknown fields.** After Vocab's update, step 10 reads every row of those three tables: its key, `name`, `mtime_secs` and `usn`, and the digest of its `config` with Vocab's unknown-field bytes taken out: `A00107` (field 20 of the note type), `A20103616263` (field 20 of the first field) and `F00101` (field 30 of the first template), each removed where its hex text first occurs at a byte boundary, in the rows of its table. The rows the two updates added report `null` there, since their ids are random. Meguri's bytes minus those fields equal Anki's, whether Meguri wrote the row again (P4 puts each unknown field in field-number order) or kept the bytes it read (P5: no edit changes the first field, so its row may keep them).
- **The unknown fields themselves.** Step 9 asks whether Vocab's note type, first field and first template each still hold their unknown field: Anki gives `[[0, 0, 0]]` (it drops them), Meguri `[[1, 1, 1]]` (deviation `preservation-notetype-unknown-fields-kept`, R1485).
- **Structure.** `notetype_structure` of both note types equals the oracle's: field options, `original_stock_kind`, `original_id` and the card requirements, the new template's on the new field alone.

Checked on the oracle: the three byte strings occur in the fixture only as those unknown fields, each at a byte boundary, and in no row after the updates; the first field's bytes minus its unknown field are the bytes Anki writes for it.

## N12. Meguri's own churn tests

`core/crates/meguri-engine/tests/preservation_churn.rs`, on the fixture:

- Ten open and close cycles, then a run of 1,000 operations of every kind the engine has (adds, edits, deletes, answers, tag, deck and preset operations, undo and redo), then ten more cycles: `sqlite_master` equals `storage/schema-18` S6's expected objects, statistics tables allowed (R2016).
- 100 open and close cycles with idle periods between them (the engine running its scheduled jobs and crossing day rollovers with no user action): the N6.1 query is clean and the raw dump is unchanged (R1586, R1590).

The automatic engine day clock follows `sched/day-clock` D8: an empty unbury check records its day only in memory, and a missing rollover uses the effective default without repairing config. Explicit compatibility timing observations retain D5/D7's observed writes; they are not launch/idle operations. The fixture has no buried cards and runs at unchanged UTC, so the real automatic lifecycle must preserve this invariant and AE66 exactly across days. Actual buried-card restoration and real offset changes retain their specific R598/R594 tests. Later U40, U43/U44, lifecycle and integration changes must keep both sets of assertions passing; no config-write waiver, timing-key mask or golden deviation is granted.
