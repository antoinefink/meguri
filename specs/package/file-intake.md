# File intake

Spec: package/file-intake
Requirements: R51, R52, R1595, R1596 (where a classified file goes); supports R157, R1591, R1626, R1629, KTD12, KTD17
Anki: 26.09.3 for K2.1 (the extensions, descriptions and media types Anki's desktop declares) and K7 (how Anki's desktop receives files, for comparison); every other section is Meguri design from the Product Contract and the plan (U7, U162, U166)
Scenarios: none (the oracle runs Anki's collection library, never its desktop: `conformance/AGENTS.md`; K8 lists the tests)

How a file reaches Meguri's import flows: the sources (K1), the file types Meguri declares to the system (K2), reading the file without copying it (K3), several files at once (K4), whole-collection files (K5), and what happens in each collection state (K6). What the file is classified as is `package/container#P18`; what the deck package import does is `package/apkg-import`; what the collection package import does is `package/container#P16` and `#P19` with `engine/collection-replace`. The sheets the flows show are `package/transfer-sheets`; the intake's messages are Meguri's wording (`package/transfer-strings#S4`).

## K1. Sources

Every way a file arrives goes through one intake (U162's file intake), never through a path of its own:

| Source | Platforms |
|---|---|
| File › Import… (⇧⌘I), through the system file picker | Mac, iPad (menu bar, N11.5) |
| Settings › Collection's Import…, through the system file picker | all |
| The empty-collection state's import entry (`home.deckList`'s empty state, N11.5) and Welcome's Import a Collection File (`onboarding.welcome`; named Import a File… before the 2026-11-19 decision, `docs/design/native-interface-contract.md` N18.2) | all |
| A drop on a main window (the deck list included) | Mac, iPad |
| A drop on the Dock icon, and Open or Open With in Finder | Mac |
| Open in Meguri from Files, Mail or the share sheet | iPhone, iPad |

- Each source gives one or more file URLs and nothing else. No source reads the file before K3.
- The system offers Meguri in the Dock, in Finder's Open With and in the share sheet only for the types it declares (K2.2). A file of another type reaches Meguri only through a drop on a window or Finder's Open With › Other… with all applications shown; the Dock refuses it before Meguri sees it, so K3 step 4's message never appears for a Dock drop.
- Where the sheet opens: on the window the source belongs to (the window a command, a picker or a drop came from, the Settings window included on Mac, as N11.4 does for the restore sheet). A Finder, Dock or Open in delivery goes to the frontmost main window; on Mac, with no main window open, one opens first.
- Until U162 lands, U166's minimal importer is the only intake and accepts only a collection package from Welcome's picker (R1591); every other source is not yet attached. Changed 2026-11-20 (text only; N17.2, N18.2): that picker is the collection-file picker of N17.2, limited to collection packages, which Welcome's Import a Collection File opens. No drop, Finder, Dock or Open in delivery reaches Meguri yet (not built).

## K2. File types

### K2.1 What Anki declares (Anki 26.9.3)

| Extension | Anki's description | Media types Anki declares |
|---|---|---|
| `.apkg` | "Anki deck package" | `application/x-apkg` (desktop document type); `application/vnd.anki` (also, in its Linux shared media-type database, with the same `*.apkg` pattern) |
| `.colpkg` | "Anki collection package" | `application/x-colpkg` |
| `.anki` (Anki 1.x decks) | none | `application/x-anki`, listed by the Linux desktop entry only, with no file pattern; Anki 26.9.3 does not import these files |
| `.ankiaddon` | "Anki add-on package" | `application/x-ankiaddon`; not a collection file |

- On macOS each of these document types is declared with the role Editor. Anki's source sets no handler rank and no type identifier: its packaging tool writes them (K9). AnkiMobile's declarations are not public (K9).

### K2.2 Meguri's declarations (Meguri design)

The same declarations in the macOS and the iOS/iPadOS Info.plist:

- **Imported type declarations** (`UTImportedTypeDeclarations`), never exported ones: the formats are Anki's, and an exported declaration would claim to define them. One per package kind:

  | Identifier | Tags | Conforms to | Description (Meguri's, R156) |
  |---|---|---|---|
  | `com.antoinefink.meguri.deck-package` | `public.filename-extension` `apkg`; `public.mime-type` `application/x-apkg` and `application/vnd.anki` | `public.data` | Deck Package |
  | `com.antoinefink.meguri.collection-package` (already declared, kept unchanged) | `public.filename-extension` `colpkg`; `public.mime-type` `application/x-colpkg` | `public.data` | Collection Package |

  The identifiers have a fixed prefix, not the build's bundle identifier, so the Debug and Release builds declare the same types. Once K9's question is settled, the identifiers Anki and AnkiMobile export are added to the document types' `LSItemContentTypes`; the declarations stay Meguri's.
- **Document types** (`CFBundleDocumentTypes`), three, each with `LSItemContentTypes` of one identifier: the deck package, the collection package and `public.zip-archive` (R1595: a `.zip` is read as a deck package, P18). `CFBundleTypeRole` `Viewer`: Meguri reads the file and never writes it back. `LSHandlerRank` `Default` for the two package types; `Alternate` for `public.zip-archive`, so Meguri never becomes the default app for zip files. A rank decides nothing against another app that also declares `Default`; Launch Services then chooses, and the user's own choice in Finder or Files always wins. Whether an installed Anki declares `Owner` is K9's question.
- iOS and iPadOS: `LSSupportsOpeningDocumentsInPlace` and `UIFileSharingEnabled` stay true (U7's baseline, so the Files app shows `Documents/Backups`: KTD12, R1629). Meguri opens an original in place for reading only (K3); a system copy is handled by K3 step 6.
- The file pickers of K1 offer the deck package, collection package and zip types, the three package extensions Anki's own picker offers; the collection-file picker before U162 (N17.2, Welcome's Import a Collection File included) offers the collection package type only. A file without one of these extensions cannot be picked; it reaches Meguri only as K1's second bullet says, and P18 step 2 reads its content.
- `.anki`, `.anki2` and `.ankiaddon` files are not declared. When one arrives anyway (a drop on a window), P18 classifies it `Unsupported` (K3).

## K3. Reading the file

The intake never copies the file. The flow's own source copy (`package/apkg-import#I2`, `package/container#P16.1`) is the only copy, with its preflight (`package/apkg-import#I15`), its progress (`extracting`, I14.5) and its Cancel. For each URL, in this order:

1. **Access.** A security-scoped URL is opened for access. The access stays open while the file is pending (K6) and until the flow's source copy exists, the flow ends or the file is refused; then it is closed.
2. **Coordinated read.** Classification and the flow's source copy each read under a coordinated read of the file (`NSFileCoordinator`, no changes), using the URL the coordinator returns. A file of a cloud provider that is not yet on the device is downloaded by that read; it runs off the main thread, and the sheet shows the wait with Cancel. A read never holds the coordination while the file is only pending.
3. **Refusals before classification.** A folder, a package that is a directory, a symbolic link that does not resolve to a regular file, a file that no longer exists and a file that cannot be read are refused with a short message naming the file; nothing else happens (Anki's drop is not accepted for folders either, K7).
4. **Classification.** P18 on the file, read only: `Apkg`, `Colpkg` or `Unsupported`. `Unsupported`: a short message names the accepted types, `.apkg`, `.colpkg` and `.zip` (R51); nothing else happens.
5. **Supported.** The URL goes to the flow K6 gives for the class and the collection's state, with its access still open (step 1).
6. **A system copy** (iOS and iPadOS). A file the system delivers as a copy for Meguri, which is any URL inside the app's `Documents/Inbox` (a Mail attachment, a share-sheet copy), is not the user's original. The intake removes it when its flow ends or the file is refused. At each launch, U162 removes every item `Documents/Inbox` still holds, except the URLs delivered to this launch. Because `UIFileSharingEnabled` shows `Documents` in the Files app, an item left there would be visible to the user.

The original file is never changed, moved or deleted, from any source. A file the user picks inside Meguri's own `Documents` (for example a backup in `Documents/Backups`) is an original too.

## K4. Several files at once (Meguri design)

A drop, an Open With of a Finder selection or a share of several files gives several URLs. Meguri runs one import flow at a time (each deck package import is one session and one undo step, `package/apkg-import#I14.3`; a collection package replaces the collection).

- The URLs are taken in the order the system gives them. K3 steps 3 and 4 run for each until one classifies `Apkg` or `Colpkg`; that file is the one imported, and the others are not read further. Their access is closed (and a system copy removed, K3 step 6).
- When no URL classifies as a package, K3 step 4's message is shown once.
- When more than one URL was given, the sheet of the chosen file says in one line that one file is imported at a time and names it (Meguri's wording).
- A file that arrives while an import flow of this intake is showing is refused with a message that an import is already open; it is not queued.

Anki 26.9.3 differs on every point (K7): its drop takes the first name that does not end in `.colpkg`, whatever its type, and an unsupported first file ends the drop with the warning; each file opened from Finder or the Dock starts its own import, so several can be open at once. These are UI behaviors; no golden sees them.

## K5. Whole-collection files

Every file P18 classifies `Colpkg` (`.colpkg`, `collection.apkg`, `backup-*.apkg`, any case), from every source of K1, a drop included, goes to the collection package flow with its replacement confirmation (R1596, R1626; `package/transfer-sheets#X3`). A drop never replaces the collection without that confirmation.

Anki 26.9.3 differs for drops only (K7): its drop leaves out names ending in `.colpkg` with that exact lowercase ending, so a dropped `x.colpkg` does nothing, while a dropped `X.COLPKG`, `collection.apkg` or `backup-*.apkg` still reaches its replacement question. Meguri follows R51 and R1596 instead: every source behaves the same. This is UI behavior; no golden sees it.

## K6. Arrival by collection state

The state is the lifecycle state of `engine/lifecycle#L1` when K3 step 5 hands the file on.

| State | Deck package (`Apkg`) | Collection package (`Colpkg`) |
|---|---|---|
| `Ready`, no review session open | the import sheet over the current screen (`package/apkg-import#I1`) | the collection package flow with its confirmation (X3) |
| `Ready`, a review session open | the import sheet over the review; after it closes, the review shows the same card on the same side (R52), unless the import's commit changed or removed that card: the review then follows `sched/review-session#C13` | refused at arrival with the session-active message (KTD17, U153); the review goes on and nothing is replaced. A session that opens while the confirmation shows makes the core refuse as the exclusive phase is entered (`package/container#P16.2`) |
| `Ready`, the long-operation slot held (`engine/lifecycle#L5`; for example Check Database, another deck import, or a restore or collection import before its exclusive phase) | the sheet opens and the inspect runs, since it takes no slot (`package/apkg-import#I3`); confirming the import while the slot is still held shows `Busy{running_op}` with the running operation named, and the sheet stays open | refused with `Busy{running_op}`, the running operation named |
| `ExclusiveBusy` (a restore, a collection import, a replacement from iCloud) | refused, naming the running operation | refused, naming the running operation |
| `NoCollection` (Welcome, R157) | K6.1 | the onboarding collection import with this file (`onboarding.collectionImport`; U162 path b) |
| `Failed{Corrupt}` (the damaged-collection screen) | refused: the import needs an open collection; the message points to the screen's remedies (R169) | refused: a `.colpkg` import is not among the ops `engine/lifecycle#L5` admits in this state (restore, Check Database and the iCloud replacement are the ones the screen offers, R169); K9 |
| `Failed` for another reason (the open failed) | refused, as for `Failed{Corrupt}` | the collection package flow: a replacement op may run here (`engine/lifecycle#L5`) |
| `Opening`, `Upgrading` | kept pending until the state settles, then handled by its row | the same |

- A refusal changes nothing; the intake closes the file's access (K3 step 1) and removes a system copy (K3 step 6). Its message is Meguri's wording, from the error kinds named here.
- An export, a backup or a Check Media run in progress does not hold the slot and does not stop a deck package import.

### K6.1 A deck package while there is no collection

U162 path c: a deck package needs a collection, so Start Empty's creation (R161, U166) runs first, then the import sheet opens over Home for the file.

- Not built (2026-11-20): no source of K1 can deliver a deck package while there is no collection yet. Welcome's Import a Collection File offers collection packages only (K2.2, N17.2), and drops, Finder, Dock and Open in are not attached (K1). The rules below stand for U162.
- From a picker of K1 that offers deck packages while there is no collection: the user chose to import, so creation follows at once.
- From any other source (a drop on Welcome's window, a Finder, Dock or Open in delivery): Welcome first asks whether to create a new collection on this device and import the file into it, naming the file, with Cancel and a default Import button (Meguri's wording). Import runs path c; Cancel drops the file, and Welcome stays as it was, its choices untouched (changed 2026-11-20: Welcome has no Connect to iCloud…, N18.2).
- Cancelling the import sheet after creation leaves the new collection as Start Empty leaves it (Home, local only). A creation failure shows its error on Welcome and drops the file.
- A second file arriving while Welcome's question shows replaces the first. Nothing pending survives a relaunch.

Anki keeps a file opened before a profile is open and imports it once one opens (K7); Meguri's Welcome is not a profile choice, so it asks instead.

## K7. How Anki 26.9.3's desktop receives files (comparison)

- Classification by name is case-insensitive (P18 step 1).
- The main window accepts a drop only on its deck list screen, and only when every dragged item is an existing file and not a folder. Of the dropped paths it leaves out those that end in `.colpkg`, compared case-sensitively, and passes the first of the rest to its import, whatever its type; the others are ignored without a message.
- A file opened from the system (Finder, the Dock) arrives as one message per file, and each starts its own import; nothing stops a second import while one is open.
  - While the profile window shows, the file is kept and imported once a profile opens, with a short message saying so.
  - While Anki is not on its deck list, deck overview or review screen, or a progress window runs, it is refused with a message asking to make sure a profile is open and Anki is not busy.
  - During startup it is tried again a second later.
  - A path that does not exist shows a message asking to use File › Import instead.
  - During a review the import goes ahead; a collection package reaches its replacement question.
- The picker's filter lists every extension Anki can import (packages, and the text and Mnemosyne formats, which are out of Meguri's scope).
- Any name no importer accepts, `.anki2` and `.anki` included, shows the same warning that the file type is not supported. Anki's translation files hold explanations for `.anki2` and `.anki` files, but 26.9.3 shows neither.

Meguri differs in K4 (several files), K5 (`.colpkg` drops), K6 (a collection package during a review is refused; a pending file at Welcome is asked about, K6.1) and K3 step 3 (a missing path is named in Meguri's own message).

## K8. Tests

UI and unit tests of U162 and U166 (`app/Packages/MeguriKit/Tests/SettingsTests/FileIntakeTests.swift`, `app/MeguriUITests/ImportExportTests.swift`):

- each K1 source delivers to the same intake, and the sheet opens on K1's window; a fake security-scoped URL has its access opened, kept until the flow's source copy exists, and closed after the flow and after a refusal; no copy is made before the flow's source copy;
- a folder, an unreadable file, a missing path and a broken link are refused with the original untouched;
- `notes.txt` → the accepted-types message; `deck.zip` and an extensionless package dropped on a window → the deck package sheet; `Deck.APKG` → the deck package sheet; `COLLECTION.APKG`, `X.COLPKG` and `backup-2026-01-01-10.00.00.apkg` dropped on the window → the collection package confirmation (K5);
- a drop of `notes.txt`, `a.apkg`, `b.apkg` → the sheet for `a.apkg` with the one-at-a-time line, `b.apkg` never read; a file arriving while that sheet shows → the already-open message;
- an iOS delivery inside `Documents/Inbox` → the item removed after the flow ends and after a refusal; a launch with a leftover item → removed, while a URL delivered to that launch is kept;
- every row of K6 with a FakeCore lifecycle, including the review session kept on the same card and side after an `.apkg` sheet closes, the C13 path when the import updated the current card's note, and the `.colpkg` refused during a review with the review continuing;
- K6.1 (once U162 attaches the sources; not built 2026-11-20): an `.apkg` chosen in a picker that offers deck packages with no collection → an empty collection created, then the import sheet over Home; an `.apkg` opened from Finder at Welcome → the question; Cancel → Welcome unchanged and no collection file; Import → the same as that picker's path;
- the built app's Info.plist on each platform holds K2.2's declarations exactly (identifiers, tags, roles, ranks, both iOS keys true): a plist check in `tools/mg lint-ui` or a unit test.

## K9. Open questions

- The type identifiers and handler ranks that Anki's macOS build and AnkiMobile declare for `.apkg` and `.colpkg` are not in Anki's source at the tag (its packaging tool writes them). If either exports an identifier, a file that Launch Services types with it may not match Meguri's imported identifier on a device where that app is installed. Needed: the identifiers and ranks read from an installed Anki 26.9.3 bundle and from AnkiMobile (the final-session manual checks); K2.2 then lists the identifiers as extra content types of each document type.
- K4: Meguri imports one file per intake, unlike Anki (K7). Whether Meguri should instead import several deck packages one after another is a product decision for Antoine.
- K6: whether a collection package should be importable from the damaged-collection screen, as a fourth remedy beside R169's three. Today `engine/lifecycle#L5` does not allow it.
