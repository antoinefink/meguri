# Import and export sheets

Spec: package/transfer-sheets
Requirements: R1600, R1601, R1615, R1617, R1618, R1625, R1626 (the sheet side of each); supports R52, R273, R280, R283, R1227, R1605, R1624, R1628, R1636, R1660
Anki: 26.09.3 for the Anki paragraphs of X2 to X4 (what Anki desktop's deck package import page, its import log page, its collection import question and its export dialog offer, in which order and with which defaults); the Meguri paragraphs are the Product Contract's and the plan's (U162)
Scenarios: none (these rules live in Anki's desktop pages and dialogs, which the oracle cannot run: `conformance/AGENTS.md`; X6 lists the tests)

The choices U162's import and export sheets offer, in which order, starting from which values, and what their results report. Data rules are elsewhere: the deck package import is `package/apkg-import`, the collection package import `package/container#P16` with `engine/collection-replace`, the deck package export `package/apkg-export`, the collection file export `package/file-export`. The look is Meguri's (`docs/design/native-interface-contract.md`, the screen contract `settings.importExport`). The Anki wording these sheets show is listed in `package/transfer-strings`; everything else they show is Meguri's wording (R1959).

## X1. Shared rules

- A sheet's text, other than `package/transfer-strings`, is Meguri's own: titles, help, group titles, result lines, errors, confirmations, format names (Anki's format names contain the product name, R156).
- Every count is shown with plural rules (R191).
- A refusal that P18 or `package/file-intake#K6` gives is a message, never a sheet.

## X2. Deck package import sheet

### X2.1 Before the options (R1600)

Meguri shows the preview facts of `package/apkg-import#I4` (notes, cards, decks, whether the package carries scheduling) above the options, once the inspect has run. Anki shows only the options. Deviation noted in R1600 itself; no golden sees it.

### X2.2 Options (R1601)

Anki 26.9.3's page shows the five options in two groups, in this order:

1. "Import any learning progress" (`withScheduling`), a switch;
2. "Import any deck presets" (`withDeckConfigs`), a switch;
3. a group of the three update options, collapsed when the page opens:
   1. "Merge note types" (`mergeNotetypes`), a switch;
   2. "Update notes" (`updateNotes`), a choice of three;
   3. "Update note types" (`updateNotetypes`), a choice of three.

The two choices list "If newer", "Always" and "Never", in that order. Every option starts from the value `package/apkg-import#I5` reads from the collection's config, and the import writes the chosen values back (I5). Anki's page has a help text per option; Meguri writes its own.

Meguri keeps this order: the two progress options first, then the three update options together. Whether those three form a group and whether it starts collapsed is the screen contract's (`settings.importExport`), not Anki's: Anki's collapsed block is presentation.

### X2.3 Merge note types (R1605)

Anki's help for Merge note types warns that a merge needs a one-way sync. Meguri shows no such warning: with Merge note types on, a merge with destructive mappings shows U121's preview before anything is written (`package/apkg-import#I8`, `package/notetype-merge#J10`), and Anki full-transfer warnings belong to explicit manual transfers only (R1605).

### X2.4 Progress and cancel

The stages and Cancel are `package/apkg-import#I14.5`. The sheet stays over the current screen while it runs (R52).

### X2.5 Result (R1615)

Anki 26.9.3's log page shows the number of notes found in the file, then up to four summary lines, in this order, each only when its count is above 0:

| Line | Notes | Show in Browse |
|---|---|---|
| new | the log's `new` | yes |
| already present | `duplicate` | yes |
| used to update existing notes | `updated` | yes |
| could not be imported | `conflicting`, `missing_notetype`, `missing_deck` and `empty_first_field` together | no |

A line's Show opens Browse with a search for exactly those notes (`nid:` with their ids). Under the lines, a table lists every note of the four lines, one row each: a status (added, skipped or updated, with the specific reason as help text), the note's fields joined with commas, and a search button that opens Browse on that one note. The button is enabled in the rows of the first three lines and disabled in the rows of "could not be imported".

Meguri (`package/apkg-import#I13`, I17):

- The found count and the first three lines as Anki: same order, same hiding of empty lines, same Show, which searches the target ids the entries carry.
- **Could not be imported**: `conflicting` and `missing_notetype` (and `empty_first_field`, always empty for a deck package). It offers no Show, as Anki; each `conflicting` row may name the target note it matched (`existing_id`), which Anki cannot.
- **Placed in the default deck**: `missing_deck` entries are not failures in Meguri: those notes were imported and their cards went to the default deck (I11.1, I17). They get a fifth line after the four, shown only when not empty, with Show. Each of these notes is also counted in its own line (new, updated or duplicate), so this line is not added to the others.
- The table has one row per note, read in pages through U133: its status (added, updated or skipped) from its one list among new, updated, duplicate, conflicting and `missing_notetype` (I13: a note is in exactly one of them), with that list's reason, and the entry's first field (I13's reduced text, as I13 says the UI shows). A note that is also in `missing_deck` keeps that one row and gains the default-deck reason beside its own; it never gets a second row.
- Each row of a note that was imported (new, updated, duplicate, and so every `missing_deck` note) offers Show in Browse for that one note, as Anki's row button does; a row of "could not be imported" offers none.
- After a successful import, an Undo action for the one "Import" step (I14.3) stays available as the app's undo toast does.

### X2.6 Errors

The kinds are `package/apkg-import#I16`; the sheet words each (Meguri). `TooNew` says that the package was made by a newer version (R1597). `SchemaUnsupported{gate}` says which: a collection too old to read, too new, or one that uses the old scheduler. `NotetypeKindConflict` says that a cloze note type and a standard one cannot be merged and that the import can run with Merge note types off, which is what Anki's message says too.

- Every error before the commit, `Cancelled` included, says that the collection is unchanged.
- `MediaCommitPending` comes after the commit (I14.5): the sheet shows the result of X2.5 with one line saying that the notes were imported and that some media files are still being put in place, which finishes on its own (the journal's recovery). It never says that the collection is unchanged and never offers to import again.

## X3. Collection package import (R1626, R1628)

Anki 26.9.3, before reading the file: one question warning that the existing collection will be deleted and replaced by the file's data, with No as the default answer. Then the forced backup (`package/backup-triggers#T4.1`), the import, and a short message that importing is complete; on a failure, the error, and the previous collection opened again unchanged.

Meguri (U124, `onboarding.collectionImport` and U162):

- Changed 2026-11-19 (X7): the import starts at the system file picker; the confirmation shows the file's name, one summary line and at most one consequence line, in place of the preview facts and statements below. The table's three states still decide what happens to the current collection; X7.2 gives what the sheet says for each.
- The inspect runs first; the confirmation then shows what P16.4's preview gives (the counts, that media is merged and old media kept), and what it says about the current collection depends on the lifecycle state (`package/file-intake#K6`):

  | State | What the confirmation says about the current collection |
  |---|---|
  | `Ready` | it is replaced as a whole, and it is backed up first (`package/backup-triggers#T6.1`, T6.2) |
  | `NoCollection` (`onboarding.collectionImport`) | nothing: there is none to replace and no backup is taken (T6.2, "Nothing to back up"); the file becomes this device's collection |
  | `Failed` (the open failed) | the collection that could not be opened is replaced, and its files are copied first as they are (T7) |

- The confirm button is destructive and never the default button; Return confirms nothing and Esc cancels. This is N8's separate destructive confirmation (`docs/design/native-interface-contract.md`), with the button rules N11.8 states for iCloud replacements, applied here; it also keeps Anki's default No.
- A corrupt or unreadable file is refused at the inspect, before the confirmation, and nothing changes (R1628, `package/container#P13`).
- Success lands on Home with R159's arrival line (U166's wording); with iCloud enabled on this device, the publication choice follows (`sync.publicationChoice`, `sync/cloudkit-recovery#V12.2`).
- Refused while a review session is open (KTD17; `package/file-intake#K6`).

## X4. Export sheet

### X4.1 Format and starting scope per entry point (R1617, R1625)

Anki 26.9.3's export dialog offers its formats in this order: deck package, collection package (then two plain-text formats, out of Meguri's scope). Which one is selected when it opens depends on where it was opened from:

| Opened from | Format selected | Scope selected | Include scheduling information |
|---|---|---|---|
| File › Export… (no deck, no selection) | collection package | none (a collection package has no scope) | the dialog default (on) |
| a deck's menu | deck package | that deck | off |
| a Browse selection | deck package | the selected notes | the dialog default (on) |

The format list always holds every format, from every entry point. Switching to the deck package from File › Export… shows the scope list with its first entry, the whole collection, selected. Switching to the collection package hides the scope list; that format ignores the deck and the selection and exports the whole collection.

Meguri:

| Entry point | Row | Formats offered |
|---|---|---|
| File › Export… (⌘E), Settings › Collection's Export… | File › Export… | both |
| File › Export Deck… for the selected deck (R280), Deck › Export Deck… (R283), a deck's context menu and the iPhone deck ⋯ menu (R1660) | a deck's menu | both |
| Browse's Export Notes (⇧⌘E; R273, R1227) | a Browse selection: U210's handoff, `Notes(ids)` in notes mode, `Cards(ids)` in cards mode (`package/apkg-export#A2`) | the deck package only |
| Settings › Backups' Export Collection with Media… (R1636, U163a) | its own row: the collection package, with Include media on whatever value is remembered; the other option is A2's | both |

- Switching formats within one sheet follows Anki's rules above: the deck package from a File › Export… opening shows the whole-collection entry selected; the scope selected stays when the format switches away and back.
- From a Browse selection the collection package is not offered: it would ignore the selection (X5).
- When Meguri remembers the options (`package/apkg-export#A2`, decided 2026-10-27), the remembered values replace the "dialog default" cells, and also the deck row's "off" once a deck package export has been made: A2's remembering applies from every entry point. Export Collection with Media… is the one exception, since its label promises media; it changes no remembered value until the user exports.

### X4.2 Scope list of a deck package (R1617)

Anki: the first entry stands for the whole collection (`package/apkg-export#A2` `Collection`), followed by every deck by its full name, in `decks/decks#D17`'s order: the default deck and filtered decks included, empty decks included. When the dialog was opened from a Browse selection, the list has one entry, the selection, and nothing else can be chosen.

Meguri: the same list, from D17's query. Choosing a deck gives `Deck(id)` (its subdecks included, A2). A deck deleted while the sheet is open makes the export refuse with `DeckNotFound` (A12).

### X4.3 Options per format (R1618, R1625)

| Format | Options shown, in this order |
|---|---|
| deck package | Include scheduling information, Include deck presets, Include media, Support older Anki versions |
| collection package | Include media, Support older Anki versions |

- Starting values: `package/apkg-export#A2` (first-use defaults and remembering) and X4.1.
- Anki keeps one set of option values while its dialog is open: switching the format keeps the values of the options both formats share (Include media, Support older Anki versions), and an option hidden by the switch keeps its value for when it shows again. Meguri does the same within one sheet. What Meguri remembers after the export is A2's, per package kind.
- The collection package has no scope and no scheduling or preset option: it is the whole collection as it is (A14).

### X4.4 Result

Anki: after a deck package export, a short message giving the number of notes exported; after a collection package export, a short message that the collection was exported.

Meguri: the counts of `package/apkg-export#A11` (notes, cards, media files) once the package is Ready, and "Saved" only after the destination completes (`package/file-export#E4`). Changed 2026-11-19 (X7.3): the collection export shows neither; a finished save closes the sheet. A selection of zero notes is not an error; the sheet says so before the user saves (A3).

### X4.5 File name and save location

- The suggested name is `package/apkg-export#A11`'s for a deck and E3's for the collection. For comparison, Anki suggests the selected scope entry's text with each of `\ / ? < > : * | " ^` replaced by `_` for a deck package (so its whole-collection and selection entries give their own label as the name), and `collection-` followed by the local date and time as `YYYY-MM-DD@HH-MM-SS` for a collection package, each with its extension.
- Anki refuses a save location inside its base data folder (the folder that holds every profile, not only the current one) and asks for another. Meguri's rule is `package/file-export#E4`: a destination that aliases the collection, its SQLite sidecars or the private artifact is refused.

### X4.6 While the collection is replaced

Export is disabled while the collection is in an exclusive phase (U153's gate); an export running when a replacement starts ends `Stale` (A11, `package/file-export#E2`).

## X5. Differences from Anki

| Situation | Anki 26.9.3 | Meguri | Why |
|---|---|---|---|
| Deck package import sheet | options only | preview facts first (X2.1) | R1600 |
| Merge note types help | warns of a one-way sync | merge preview, no full-transfer warning (X2.3) | R1605 |
| `missing_deck` notes in the result | its page would group them under "could not be imported"; its deck package import never produces them (I17) | a separate line; the notes were imported (X2.5) | `package/apkg-import#I17` |
| `conflicting` rows | the package note only | may name the matched target note (X2.5) | `package/apkg-import#I13` |
| Collection package confirmation | a warning, default No | the inspect's preview, then a destructive confirmation that is never the default (X3) | R1626, N8 |
| Formats from a Browse selection | both; the collection package exports the whole collection | the deck package only (X4.1) | the selection is the point of Export Notes |
| Export option values | the dialog's defaults at every opening | remembered per package kind on the device (X4.1) | `package/apkg-export#A2` |
| Collection package export | closes and reopens the collection | runs from a snapshot while study goes on | R1625, `package/file-export#E2` |

None of them is observable through the oracle; no deviation entry is needed.

## X6. Tests

U162's tests (`ImportFlowTests.swift`, `ExportSheetTests.swift`, `app/MeguriUITests/ImportExportTests.swift`):

- the import sheet shows X2.2's order, starting from a FakeCore config that holds each I5 value; changing "Import any learning progress" and importing sends that value (core write recorded);
- a FakeCore log with entries in each I13 list → the lines of X2.5 in order, empty ones hidden, Show on the first three and on the default-deck line with the `nid:` search of their target ids, none on "could not be imported"; a log with only duplicates → one line; a note in `new` and `missing_deck` → one row with both reasons; a row's Show opens Browse on that note, none on a "could not be imported" row;
- `MediaCommitPending` → the result with the media line and no "unchanged" text; `SchemaUnsupported` for each gate → its own message;
- the collection package confirmation in `Ready`, `NoCollection` and `Failed` → X3's text for each; Return confirms nothing, the destructive button starts the import (X3); changed 2026-11-19: the text is X7.2's, and X7.4 adds the import and export flow tests;
- each X4.1 entry point → the format, scope, scheduling value and formats offered it gives, with no remembered values, then with remembered ones; Export Collection with Media… with Include media remembered off → on; switching File › Export… to the deck package → the whole-collection entry;
- the deck scope list equals D17's list after a "whole collection" entry, a filtered deck and the default deck included; from a Browse selection the list is the selection alone;
- switching the format to collection package and back keeps Include media and Support older Anki versions as set (X4.3).

## X7. Collection file import and export in two steps (2026-11-19)

Decided by Antoine on 2026-11-19 (`docs/design/decisions.md`, `collection.import` and `settings.importExport`; `docs/design/native-interface-contract.md` N17.2 and N17.3). Where X3, X4.4, `package/container#P16.4`'s first two sentences and `package/file-export` E1 and E4 differ, this section wins. It changes only what the interface shows and in which order. The inspection, the safety backup, the replacement, its recovery, the export's snapshot and contents, and every refusal are unchanged. Media is never a separate choice: a collection import and a collection export always carry the media with the cards.

### X7.1 Import: the file picker first

- Every entry that imports a collection file (`package/file-intake#K1`'s pickers, the welcome screen's Import a Collection File) opens the system file picker at once, on the window that asked. No sheet, explanation or Choose File step comes before it. A file that arrives by a drop or by Open in Meguri skips the picker and continues at the next item.
- Cancelling the picker ends the flow with nothing read and nothing changed.
- A chosen collection package is inspected (`package/container#P16.1`) while one sheet shows that the file is being read, with Cancel. A file the inspection refuses gives one plain message that says nothing was changed, with a way to choose another file (X1, X3's fourth item).
- A deck package chosen in the same picker goes to X2's sheet; X7 does not change it.

### X7.2 Import: one confirmation

After the inspection the same sheet is the one confirmation. It shows, and only this:

| Element | Content | Shown |
|---|---|---|
| file name | the chosen file's name, as the question "Import “<file name>”?" | always |
| summary | one line with three counts from the inspection: notes · cards · media files, each with plural forms (X1) | always |
| consequence | one line that the collection on this device is replaced and that Meguri keeps a backup of the current one | only when a collection is replaced |
| actions | Import and Cancel | always |

| State (`package/file-intake#K6`) | Consequence line |
|---|---|
| `Ready` | shown: the collection is replaced as a whole after its backup (`package/backup-triggers#T6.1`, T6.2) |
| `NoCollection` | none: there is nothing to replace and no backup is taken |
| `Failed` (the open failed) | shown, with the same words: the copy of the unopened collection's files (T7) is the backup it names (spec-writer choice, 2026-11-19: one wording for both replacing states) |

- **X3's button rule stands.** When a collection is replaced (`Ready`, `Failed`), Import has the destructive role and is never the default: Return confirms nothing and Esc cancels. With `NoCollection`, Import is the default action.
- Not required in the interface any more: the total decoded media size, the statements that media is merged and that old media is kept, the destination, the validated-file and decoded-byte rows, and the per-phase media counts during an ordinary import or after an ordinary cancel. The review count need not be in the line; it is in the summary's accessibility value. Where the interface still shows a line about merged media, it must stay true to `package/container#P16.2`.
- Still required: the confirmation is bound to the inspected file (a changed or replaced file needs a new inspection); after Import, progress with Cancel while cancelling is safe (`package/container#P16.3`'s phases), no Cancel from `Committing` on; a result that is not success keeps its message and the receipts the user must keep (the backup, a retained media reference, a state that needs recovery), which appear only then; success lands on Home as X3 says, and the publication choice follows as X3 says (`sync/cloudkit-recovery#V12.2`; through V27.4's sheet once V27 is approved).
- Refused while a review session is open, as before (X3).

### X7.3 Export: prepare, then the save dialog

For Export Collection (`package/file-export` E1: Settings › Collection, File › Export… where it exports the collection, Settings › Backups' export):

- Choosing Export Collection starts the export job at once (`package/file-export` E2). No explanation, Prepare step or option comes first: the export is the whole collection with its media in the latest format (E1, E7). The sheet shows that the export is being prepared, with a progress bar from E2's stages (indeterminate while a stage has no total) and Cancel.
- When the artifact is Ready the system save dialog opens at once: the save panel on Mac, the Files export on touch (`package/file-export` E4). No Ready to Save step, Choose Save Location button or count summary comes between.
- **Cancel at either step discards the prepared file.** Cancel while preparing cancels the job and removes its output (E5). Cancel in the save dialog removes the Ready artifact and closes the sheet; it is not kept for a later retry. The sheet's own Cancel is disabled while the save dialog shows and while the destination is being written.
- A finished save closes the sheet with no "Saved" step. The artifact is removed when E4 and E5 allow it (the provider has its own copy or every provider read has ended); closing the sheet never removes a file a provider still reads.
- A save that fails keeps the sheet with one message, a way to choose another location, and Cancel. The artifact is kept for that retry only (E4) and removed on Cancel.
- No route leaves a prepared file, or a provider's hold on it, behind a closed sheet. An artifact left by a killed app is removed when the next export starts, under E5's proof rule.
- That media is included is said where the export is offered (the Collection section's footer), which meets E1's "show that media is included before creation".
- X4.1 to X4.3's format list, scope list and options belong to the deck package export (a deck's menu, a Browse selection). This decision does not cover them, and X7.3 gives the collection export none of them: Include media is always on for it, and Support older Anki versions is not offered (E1: no legacy format). Spec-writer reading, 2026-11-19, open to the owner's correction.

### X7.4 Tests

Added to X6: each import entry point opens the picker with no sheet before it; the confirmation's three elements for `Ready`, `NoCollection` and `Failed`, with no consequence line for `NoCollection`; no size, merge or kept-media text is required; Return confirms nothing while a collection is replaced and confirms with `NoCollection`; a refused file's message offers another file. Export: the job starts on the row's activation; the save dialog opens when the artifact is Ready; Cancel while preparing and Cancel in the save dialog each leave no artifact and no lease; a failed save keeps the artifact until Cancel; a finished save closes the sheet.
