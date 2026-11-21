# Browse and Note Types: implementation plan

Date: 2026-11-20. Author: spec-writer session `s-20261120T154704Z-1e3b6b1f` (branch `unit/browse-specs`).
Decisions: `docs/design/decisions.md`, the 2026-11-20 rows `browser`, `home.deckList` (sidebar symbols) and the two `noteTypes` rows (round 8: 1A 2A 3A 4B 5D 6A 7A 8A 9A 10A 11A; round 9: 1B 2B 3C 4B 5A 6B 7B 8B).
Specs: `search/filter-tokens`, `browser/workspace`, `notetypes/workspace`, `notetypes/draft-save`, `sync/meguri-provider` MP7, `notetypes/schema-changes` G4 (changed), the "Changed 2026-11-20" lines in `search/*`, `browser/*`, `notetypes/editor-actions`, `render/preview`, `stats/card-info`; layout `docs/design/native-interface-contract.md` N21 and N22.
Parent plan: `docs/plans/2026-10-12-1926-feat-meguri-anki-compatible-apple-app-plan.md` (its units U34, U35, U81–U92, U190–U203, U210–U223 are reused below; its superseded requirements carry notes).

**Gate, passed 2026-11-20.** Antoine approved every new spec line in chat on 2026-11-20, after an independent review and five reviewer rounds (`docs/process/review-log/browse-note-types-specs-20261120.md`); each file's Status line says so. Every unit of section 4 may implement its full scope, including the parts first held back: `browser/columns` C5 and C7.4, `browser/result-sets` H7's `fld:` keys, T3.1 in U88a, and the confirmed-losses precondition in U34 and U35.

**Round 10.** The decisions of round 10 (`decisions.md`, `home.today`, `sync.statusPopover`, 2026-11-20) change the sidebar and Today: Today becomes a summary where the sidebar is visible, a deck's sidebar number counts every waiting card, and double-click, Return or S on a deck row starts it. A round 10 unit will change the sidebar and Today; Browse's units build on its sidebar. With Browse selected, the sidebar's deck rows are filters (`search/filter-tokens` T9): a click and Return set the Deck token, S and double-click do nothing, and their numbers are Browse's filter counts (T12.4), not waiting counts.

## 1. What already exists (master, 2026-11-20)

From a read of `core/` and `app/` (no code changed by this plan):

- **Search core** (`meguri-search`): the full grammar parser with spans (`parser.rs`, `ast.rs`), the canonical writer (`canonical.rs`), errors, `SearchContext`, the plan skeleton (`plan/mod.rs`, `plan/sql.rs`) and the SQL functions (`sqlfns.rs`). **SQL is compiled only for the whole collection, `is:due` and `is:buried`;** every other term returns `NotYetSupported` (U81, U82 not built). No executor, sort, paging or streaming; no builder (`join-and-replace` J3), no suggestions, no editing services; **no search FFI at all**.
- **Browser core**: the config keys model and its write ops (`meguri-search/src/config.rs`, `meguri-engine/src/browser_config.rs`, not on FFI); the Deck and Question cells (`browser/deck_cell.rs`, `qa.rs`); card excerpts (engine, FFI, Swift wrapper); FFI sources for the deck tree, tag tree, flag names and note type names. Missing: result sets and sessions, the other cells, sort SQL.
- **Bulk ops**: bury, suspend, unsuspend, set due, reset, grade now (`sched/ops.rs`, FFI `start_sched_op`); flags (set, toggle, rename); tags (add, remove, rename, reparent, delete, clear unused); mark; delete notes (`notes/delete.rs`, FFI). Missing: **change deck of cards**, find and replace, find duplicates, change note type.
- **Card Info**: revlog reads and FSRS fields only; `meguri-stats` is a stub; no Card Info API, no curve.
- **Note types**: records, digest, add, add stock, clone, plain save, field rename, template formats, validation (V3 kinds), card requirements, schema reasons, `render_template` for a card with overridden texts; FFI for names, read, add, clone, rename, CSS, field options, template options. **`save_in` refuses every structure change** (U34 not built); no delete note type, no change note type, no restore, no empty-cards count; validation, rename and preview not on FFI; note type names carry no counts.
- **Undo**: the engine API, 38 Anki step names plus Meguri's, FFI, Swift Edit menu with "Undo <name>".
- **Meguri Sync**: bootstrap upload of the device collection (host, FFI, Swift setup views); `scm > ls` already surfaces as `FullSyncRequired` and nothing uploads by itself.
- **App**: `AppSection` has `browse` and `noteTypes`, but `Features/Browse`, `Features/NoteTypes` and `Features/CardInfo` are one-line placeholders and the shipped `availableSections` exclude them. Menus: File, Edit (Find… is the deck search), Deck, Card. No Swift Charts use yet (the heatmap is custom).

## 2. Conventions for every unit

- One implementer, one PR against `master`, opened early and ready for review; its own commits with the trailers of `docs/process/provenance.md` (`Role:`, `Spec:` with the spec ids it implements, `Session:`), the privacy and similarity checks, an independent review.
- Implementers build from the specs named here, never from the round 8 and 9 HTML pages; screen layout is N21 and N22, then the native build is looked at (`docs/design/workflow.md`).
- **Core integration tests**: each crate's integration tests are being consolidated into one test binary per crate (files stay in place, listed from a single `tests/main.rs`). A unit that adds a core integration test file adds one `mod` line to its crate's `tests/main.rs`.
- Verify with the smallest `tools/mg` jobs: core units `tools/mg core-test -- <test filter>` (nextest arguments) and `tools/mg core-lint`; Swift model units `tools/mg swift-test --package MeguriKit -- <filter>`; app units `tools/mg build-app --platform mac` and `--platform ios-sim`; UI suites and screenshots only in the macOS test VM (`tools/mg mac-ui`, `tools/mg screenshot`, `tools/mg a11y`), **one VM job at a time**; benchmarks `tools/mg bench --sizes 150k --bench <target>`. Hosted CI runs the Linux suites.
- At most three implementers at once. Core units run in Docker and can run three wide; Swift units that need the VM queue behind each other for their UI checks.

## 3. Speed budgets (the 150k fixture and larger)

The budgets are `browser/workspace` V17 (keystroke echo one frame; simple-token first page 100 ms p95; text, field and regex first page 300 ms then streamed; sort 200 ms; Cards/Notes 100 ms; scrolling below 5 ms/s of hitches; Select All 50 ms; bulk on 10,000 notes 2 s; Find and Replace on 150,000 notes 5 s; suggestions one frame; counts 200 ms; Note Types counts 300 ms, "no card" 1 s for 50,000 notes). How they are met:

- **Id lists, not rows.** A search returns only the ordered id list (8–16 bytes a row, `browser/result-sets` H6); a 500k-card handle is about 8 MB. Sorting runs in SQL on the id list; reversing flips it in memory (H8); Cards/Notes maps it without a new run (H11).
- **Lazy rows.** The table asks for the visible indexes plus 200 each side (H7, at most 1,000 a call) on the query lane; cells are cached by id and invalidated only for rows a change set touches (H10). Cells never render templates; Answer cells for the iPhone list come from `qa_cells` in the background.
- **Streaming** for slow terms (`search/query-model` Q10): the first page is final as soon as it fills.
- **Cancellation.** Each keystroke starts a request that supersedes the last and interrupts its reader (H3); suggestions read an in-memory name index (D6.7), never the database; counts arrive later from grouped reads cached by change seq (`search/filter-tokens` T12.4).
- **Selections are ranges** on the handle, resolved to ids in the core only when a command runs (H12).
- **Benchmarks** (U242's suites, the 150k fixture `tools/mg fixtures 150k`): every unit that adds a query adds its bench row; `tools/mg perf-gate` judges them.

## 4. Units in dependency order

Ids U81–U92, U34, U35 are the parent plan's (their specs are in force). U260 onwards are new. "Core" runs in Docker, "Swift" on the host with VM checks. ∥ marks what can run beside the previous units of its lane.

### Lane A: search and Browse core

| Unit | Scope | Spec sections | Side | Verify | Must not touch |
|---|---|---|---|---|---|
| **U82** | Names, ids, state, flag, property and history terms in SQL: `deck:`, `tag:`, `note:`, `card:`, `is:`, `flag:`, `prop:`, `added:`/`edited:`/`rated:`/`introduced:`/`resched:`, ids, `dupe:`; may land as up to three PRs by term family (names and ids; state and flags; properties and history) | `search/names-and-ids`, `state-and-flags`, `properties`, `history` (in force) | core | `core-test` search module; activate the search scenarios they unblock | browser, FFI |
| **U81** ∥ | Text matching: plain text, field terms, `re:`, `nc:`, `sc:`, `w:`, folding, excluded fields | `search/text-matching`, `fields`, `case-folding` (in force) | core | `core-test`; bench a plain-text search on 150k | names and ids terms |
| **U88a** ∥ | The builder: `build` of J3 items, J4, J5 (T3.1's two new field items are added once `search/filter-tokens` is approved, in U260 if U88a has landed); `classify_tail` (D5); the in-memory name index (D6.7) with its change-stream rebuild. Not `join`, `replace`, spans or edits | `search/join-and-replace` J1–J5, J8; `search/editing-services` D5, D6.7 | core | `core-test` builder module; property test J8 | search execution |
| **U84** | Rows, columns and sorting (19 columns) | `browser/columns` C1–C15, `browser/sorting` S1–S11 (in force) | core | `core-test`; activate `browser/sort/*`, `browser/columns/*` | sessions |
| **U85** | Search sessions, handles, paging, streaming, toggle, selection mapping | `browser/result-sets` H1–H16 (in force) | core | `core-test`; bench first page and resort on 150k | Swift |
| **U260** | Filter token model: kinds and values, run text, text rule, reading Anki text into a query, round-trip property tests | `search/filter-tokens` T1–T7 | core | `core-test filter_tokens`; T16's property test | sessions, FFI |
| **U261** | Field columns and their sort key; `rows` and `resort` accept `fld:` keys | `browser/workspace` V3, V4.2; `browser/result-sets` H7 note; `browser/sorting` S1 note | core | `core-test`; bench a field sort on 150k | Swift |
| **U262** ∥ | Column sets key `meguriBrowserColumnSets`: read, write, defaults, undo class | `browser/workspace` V2.3–V2.4, V4.3 | core | `core-test` | Anki browser keys (must stay byte-identical) |
| **U263** | Suggestions and filter counts | `search/filter-tokens` T12 (T12.4 counts) | core | `core-test`; bench 50 suggestions with 10,000 tags (< 4 ms) and counts on 150k (< 200 ms) | sessions |
| **U264** | Browse FFI, part 1: sessions, `start_search`, `rows`, `qa_cells`, resort, toggle, selection mapping and `resolve_selection`, with the CoreBridge wrapper | `browser/result-sets` H2–H12 over FFI | core + CoreBridge Swift | `core-test`; `swift-test --package MeguriCore` | MeguriKit UI |
| **U264a** | Browse FFI, part 2: the query model (T4, T7), suggestions, filter counts, column sets, and the selection summary (counts of suspended, buried, decks and note types of a resolved selection, for V7 and V8.1) | `search/filter-tokens` T4, T7, T12; `browser/workspace` V2.4, V7, V8.1's "shown when" | core + CoreBridge Swift | `core-test`; `swift-test --package MeguriCore` | MeguriKit UI |

### Lane B: Browse actions core

| Unit | Scope | Spec sections | Side | Verify | Must not touch |
|---|---|---|---|---|---|
| **U86** | Bulk framework (job, phases, cancel, progress, one undo step) and Find and Replace op | `browser/bulk-actions` intro, `browser/find-replace` F1–F11 (in force; F8 note) | core | `core-test`; bench R1832 | Swift |
| **U87** | Card and note bulk actions incl. the missing **Change Deck** op (B7.2); Browse targeting of Unsuspend and Unbury subsets; Flag set | `browser/bulk-actions` B1, B7, B9, B10, B11, B16 (+ notes); `browser/workspace` V8.1 | core + FFI | `core-test`; bench 10,000-note bulk ops | sidebar actions |
| **U265** | Find and Replace preview with the change list, All Found target, Whole Words | `browser/workspace` V9; `browser/find-replace` F10 note | core + FFI | `core-test`; bench preview on 10,000 notes | the F11 op's write path |
| **U266** ∥ | Find Duplicates | `browser/workspace` V10 | core + FFI | `core-test`; bench 150k notes < 2 s | |
| **U92** ∥ | Card Info snapshot incl. history and forgetting-curve points, over FFI | `stats/card-info` C1–C11 (in force) | core + FFI | `core-test`; `stats/card-info` scenarios | stats graphs |

### Lane C: Browse app

| Unit | Scope | Spec sections | Side | Verify | Must not touch |
|---|---|---|---|---|---|
| **U270** | Browse section shell (Mac, iPad regular): Today and Browse rows with symbols, ⇧⌘B, kept state, the filters sidebar (V6, T9 clicks, counts) | `browser/workspace` V1, V6; `search/filter-tokens` T9, T14; N21.1 | Swift | `swift-test --package MeguriKit`; `build-app --platform mac`; VM screenshot | editor, Note Types |
| **U270a** | The table: native `Table` with the standard set, the state column, deck and date cells, lazy rows, the status line, Cards / Notes | `browser/workspace` V2.1, V2.2, V2.5, V5, V11, V17's rows; N21.3 | Swift | model tests; VM screenshot of N21.3; `perf-ui` scroll test | sidebar |
| **U271** | Search field: tokens, live search, commit, token menu, keyboard, suggestions list, Save Search | `search/filter-tokens` T8, T10, T12 (UI), T13; N21.2 | Swift | model tests of T8–T10; VM UI test of the token flow | table internals |
| **U272** | Column sets UI: header menu, Fields submenu, own sets, widths, reorder, set menu in the status line | `browser/workspace` V2.2–V2.4, V3, V4 (UI) | Swift | model tests; VM screenshot | |
| **U273** | The inspector's Note tab (V12): card preview, Deck and Type rows, field editing, tags, formatting controls, saving | `browser/workspace` V12; N21.4; `editor/edit-flows` (Browse editor) | Swift (reuses the round 5 editor component) | model tests; VM UI edit-and-undo test | Add Note form behaviour |
| **U274** ∥ | Card Info view (Info tab, and the sheet hosts): stats rows, history, forgetting curve with Swift Charts under the history | `browser/workspace` V13; `cardInfo.card`; `stats/card-info` C7.5 | Swift | model tests; VM screenshot light and dark | |
| **U275** | Commands: Note menu, Card and Edit menu items, context menu, multi-row inspector, Delete question, undo titles, VoiceOver results; the immediate commands (Suspend, Unsuspend, Bury, Unbury, Flag, Delete) | `browser/workspace` V7, V8; N21.5, N21.8 | Swift | model tests of V8's titles and rules; VM UI | sheets |
| **U276** | Command sheets: Change Deck (deck search with create), Add Tags, Remove Tags, Set Due Date, Forget | `browser/workspace` V8.1, V8.2; N21.6 | Swift | model tests; VM UI | |
| **U277** | Find and Replace sheet with its preview; Find Duplicates sheet and its token | `browser/workspace` V9, V10; N21.6 | Swift | model tests; VM UI with a 3-note preview | |
| **U278** | iPad and the Duo opened: touch columns, slide-over sidebar, Select mode and its bar, the Duo's list and inspector halves, Filters sheet | `browser/workspace` V14.1, V14.2; N21.7 | Swift | `build-app --platform ios-sim`; one simulator job | Mac layout |
| **U279** | iPhone and the folded Duo: Browse tab, two-line rows, Filters sheet with Sort By, swipes, tap to Edit Note with More | `browser/workspace` V14.3; N21.10 | Swift | simulator job (iPhone, foldable) | |
| **U280** | Ways in: deck menu Browse Cards, Suspended Cards row, review Show in Browse (More and Card menu), heatmap day | `browser/workspace` V15; N21.9 | Swift | model tests; VM UI from review | review behaviour |

### Lane D: Note Types

| Unit | Scope | Spec sections | Side | Verify | Must not touch |
|---|---|---|---|---|---|
| **U34** | The structure draft apply op: fields, templates, sort field, removal, card generation, rewrite, schema reasons, backup; the confirmed-losses precondition of the draft save (S1.2), note type removal (S12) and Restore to Default (`restore-to-default` D6, D7, D7.1), including the `restore_summary` read (it needs the planner); the conformance executor change for the structure and restore scenarios (`conformance/notetype-ops` T9, `notetype-structure-ops` M1, M2: pass the counts read at the step). Test cases: a removed field gains content; a removed template gains a card; a field confirmed at 0 (no sheet) gains content; a count shrinks and the save commits; S12 with a note added and with a card added; a restore gains a field with content, gains template cards, and gains D4.1 kind-change cards; a restore after a digest change or a D6 replan returns `LossesChanged`; `Stale` wins over `LossesChanged`; a refusal records no undo step and the backup stays | `notetypes/structure-changes` S1–S13; `notetypes/restore-to-default` D1–D7.2; `notetypes/draft-save` D8; `conformance/notetype-ops` T9, `notetype-structure-ops` M1, M2 | core + FFI | `core-test`; `notetypes/structure/*` scenarios | UI |
| **U35** ∥ | Change Note Type op and its info query; the confirmed-losses precondition of M8 (callers pass what their M6 preview showed) and the conformance executor change for `change_notetype` (`conformance/change-notetype-ops` H2: a fresh M5/M6 at the step). Test cases: a discarded field gains content, counted by M5 (`<br>` included); a dropped template gains a card; an M9.1 card without a template at a renumbering target; M9.2 cloze-to-standard cards gain a cloze number; `schema_changed` wins over `LossesChanged`; a refusal records no undo step and the backup stays | `notetypes/change-notetype` M1–M11; `notetypes/draft-save` D8; `conformance/change-notetype-ops` H2 | core + FFI | `core-test`; scenarios | |
| **U267** | Note Types reads: `draft_summary`, `field_content_counts`, `notes_without_card`, near names; the list's counts, and the note type's note and card counts for W12's delete question (the restore loss read is U34's `restore_summary`) | `notetypes/draft-save` D2; `notetypes/workspace` W3.1, W7.1, W8; `notetypes/editor-actions` A2 | core + FFI | `core-test`; bench counts | the save op |
| **U268** ∥ | Card Text row CSS reading and single-value editing | `notetypes/workspace` W10.1, W10.2 | core (pure function) + FFI | `core-test` with byte-for-byte checks | templates |
| **U269** | Meguri Sync structure save job: round, counting the losses of all four ops again after the round (draft save, Change Note Type, Delete Note Type, Restore to Default), save, upload; the job ending (nothing saved or pending, handoff released) when the question shows again or the op returns `LossesChanged`; pending upload state and its needs-you line | `sync/meguri-provider` MP7; `notetypes/draft-save` D6, D7 | core host + FFI | `core-test`; `sync-server-test` with the fake server; `meguri-provider-real-server` once | iCloud paths |
| **U281** | Note Types section shell (Mac, iPad): sidebar row ⇧⌘T, list with counts, filter and Edited; Fields tab with the draft, crossing out, W4 options; Save without Meguri Sync and the Save sheet | `notetypes/workspace` W1–W4, W13 (draft undo); `notetypes/draft-save` D1–D5, D8; N22.1, N22.2, N22.5 | Swift | model tests of D3's table; VM screenshot | Browse |
| **U282** | Cards tab: card types list, code editor (colouring, completion, Insert), live issues with fixes | `notetypes/workspace` W5–W8; N22.3 | Swift | model tests; VM UI with a red and a yellow issue | |
| **U283** | Live preview and the Card Text row | `notetypes/workspace` W9, W10; N22.3, N22.4 | Swift | VM screenshots light and dark | |
| **U284** | New Note Type sheet, Duplicate, Rename, Delete, Restore (its question with `restore_summary`'s rows); Change Note Type sheet from Browse; showing each question again with the new counts on `LossesChanged` | `notetypes/workspace` W11, W12; N22.6, N22.8; `notetypes/change-notetype` | Swift | model tests; VM UI | |
| **U285** | Meguri Sync save path in the app: sheet paragraph, Save and Upload, progress, needs-you Upload… | `notetypes/draft-save` D4, D6, D7; MP7 | Swift | model tests; one cross-layer test with the local sync service (`tools/mg local-sync`) | |
| **U286** | iPad, Duo and iPhone Note Types layouts; read-only templates on iPhone; key row | `notetypes/workspace` W13; N22.7 | Swift | simulator jobs (iPad, foldable, iPhone) | |
| **U287** | Links: Edit Note Type… from Browse, the inspector, the forms and review; Back button; Show Notes to Browse | `notetypes/workspace` W14; `browser/workspace` V15 | Swift | VM UI round trip | |

### Dependencies and parallel work

- U82, U81 and U88a need nothing new: they start first, in parallel. U84 needs U82 (shared due expression); U85 needs U81, U82, U84. U260 needs U88a; U261 and U262 need U84; U263 needs U260 and U85; U264 needs U85, U260, U262, U263.
- Lane B: U86 needs U85; U87 needs U86; U265 needs U86; U266 needs U85; U92 is independent (start any time).
- Lane C: U264 needs U85; U264a needs U264, U260, U262, U263; U270 needs U264a; U270a needs U270; U271 needs U270 and U263; U272 needs U270a and U261; U273 needs U270a and U87 (its Deck row runs Change Deck; its Type menu's Change Note Type… stays disabled until U284); U274 needs U92; U275 needs U270a and U87; U276 needs U275; U277 needs U275, U265, U266, U260 and U271 (the Duplicates token); U278 and U279 need U271 and U275; U280 needs U270 and U263 (the Suspended Cards count is a filter count of Deck + Suspended).
- Lane D: U34 and U35 are independent of lanes A–C and start early; U267 needs U34 (its summary runs U34's planner); U268 needs nothing beyond what exists; U269 needs U34; U281 needs U267; U282 and U283 need U281 (U283 also U268); U284 needs U281 and U35; U285 needs U269 and U281; U286 needs U282; U287 needs U281 and U273.
- Up to three at a time, for example: {U82, U81, U88a} → {U84, U260, U34} → {U85, U262, U267} → {U86, U263, U268} → {U264, U87, U269} → {U264a, U265, U266} → then the Swift lanes two at a time (one VM job at a time), with U92 and U35 slotted in wherever a lane waits.

### Recommended first three

1. **U82**: deck, tag, state, flag and day terms are what every token and sidebar row runs.
2. **U88a**: the builder is what turns tokens into Anki text; it is small and unblocks U260.
3. **U34**: the note type structure op is on the critical path of all of Note Types and independent of search.

U81 is the fourth, as soon as one of these lands.

## 5. Requirements this plan supersedes

Each carries a note in the parent plan ("Superseded 2026-11-20 …"), with the decision row as authority: R1076, R1086, R1090, R1093 (canonical text, syntax mode, syntax reference; pasted Anki syntax is still read), R1131 (middle-cut deck names), R1168 (a sidebar click replaces only its own kind), R1193 and R1228 (two commands with round 7's keys), R1212 (Delete asks when more than one note goes or the one note has more than one card), R1234 (suggestions and a Filters sheet). Also noted, in part or as deferred: R1087, R1088, R1089, R1091, R1094, R1101, R1108, R1109, R1111, R1112, R1134, R1135, R1141, R1147, R1157, R1159, R1169, R1170, R1184, R1192, R1199, R1208 (with R1209–R1211), R1213, R1214, R1222, R1226, R1236, R1237, R282, R915, R916, R919, R920, R922, R925, R929, R931, R932, R940, R951. The specs' headers name the other requirements they change in part (R1095–R1100, R1103, R1166–R1180); their sections say how.

## 6. Decisions and defaults (each an assumption the specs make; the owner can change any)

1. **Question 8 (iPhone, folded Duo)** for Note Types: 8B, now confirmed by Antoine (decisions.md).
2. **Field options**: font and size not shown, values kept (decided). Also not shown in this version: "Use HTML editor by default" and "Exclude from unqualified searches"; stored values are kept, and the core keeps honouring a stored exclude-from-search in search (decided 2026-11-20 by Claude (coordinator) under Antoine's delegation of that day ("Carry on here and in general with your best jugment")). Shown: Name, Sort Field (table), Collapsed in Editor, Right-to-Left, Placeholder, each with a one-line purpose (`notetypes/workspace` W4). Verified in the specs: font, size, RTL, HTML-by-default, collapse and description are editing-form settings stored in the note type (`notetypes/records` N2, `editor/field-html` F1–F3); none changes the rendered card. Exclude-from-search is a search setting (`search/text-matching` T7).
3. **Leech** is a Card State value (`tag:leech`), so "Suspended or Leech" is one token; the sidebar reaches it through the built-in Leeches saved search.
4. **Day values** read "Today", "Last 7 Days", "Last 30 Days" (Anki's `added:7`, `added:30`), not "This Week" and "This Month", which would not be true; the built-in saved search is "Added in Last 7 Days". Due: Today, Overdue, Next 7 Days (tomorrow to seven days).
5. **Built-in saved searches** (Due Today, Added in Last 7 Days, Leeches) are not stored in the collection and cannot be renamed or deleted in this version (writing them on open would change the collection without a user action, R1138).
6. **Column sets and sort** live in one Meguri key `meguriBrowserColumnSets` (synced); Anki's `activeCols`, `activeNoteCols` and sort keys are left untouched, so other clients keep their own columns. The compact default applies when nothing is stored, whatever Anki stored.
7. **First open** of Browse runs `defaultSearchText` read into tokens when set, otherwise the whole collection (not `deck:current`); later opens restore the kept state.
8. **Unparsable pasted text** becomes literal words, never an error; text that holds Anki syntax converts to tokens on commit, and what no kind can show stays an advanced token.
9. **Card Info key** stays ⌥⌘I (R1226): the design's ⌘I is Italic while a field has focus.
10. **⇧⌘B**: View › Browse, and Card › Show in Browse while review presents a card (the two are never enabled together).
11. **No Flag** gets ⌘0; Flag sets (it does not toggle).
12. **Delete** follows the design: it asks when more than one note is selected, or when the one note has other cards that would also be deleted; otherwise no question, and Undo restores (decided 2026-11-20 by Claude (coordinator) under Antoine's delegation of that day ("Carry on here and in general with your best jugment")).
13. **The Save sheet** says "cards and their progress are deleted", not the design's "and their review history": the review log stays (`notetypes/structure-changes` S6).
14. **Undo after an upload**: a completed structure upload clears the undo history (an undo would leave the device behind the server).
15. **Not in the first version**, as the design's "later" list says: Grade Now, Reposition, Mark, Export Selected Notes, Create Copy, batch Edit Field, Clear Unused Tags, filtered decks from a search, sidebar renaming and dragging, Browser Appearance, Update to Current Search.
16. **Note Types sidebar row** gets a system symbol (`rectangle.stack`), like Today and Browse (decided 2026-11-20 by Claude (coordinator) under Antoine's delegation of that day ("Carry on here and in general with your best jugment")).
17. **Card Info rows** stay `stats/card-info` C4's (the design's sketch listed a "State" row, which Anki's Card Info does not have).
18. **Deck page Suspended Cards row**: kept; shown only while the deck has suspended cards; it opens Browse filtered to that deck and Suspended (decided 2026-11-20 by Claude (coordinator) under Antoine's delegation of that day ("Carry on here and in general with your best jugment")).
19. **"All bulk actions"** means the actions drawn in round 8's question 7 plus Find and Replace and Find Duplicates; the page's "Later" list stays later (`browser/workspace` V8.1; decided 2026-11-20 by Claude (coordinator) under Antoine's delegation of that day ("Carry on here and in general with your best jugment")).
20. **Column sets** record no undo step and keep the undo steps (the current-deck selection's class), so a column change never removes "Undo Suspend Cards".
21. **Status lines** name the requirement R1232 (Browse) or R915 (Note Types) with the round, as `specs/README-format.md` asks for a requirement id; the decision rows are the authority.
22. **Uploading after an offline structural save** reads the server's facts and uploads with no question; if the server changed since this device's last round, it shows the normal collection choice instead (MP7 step 5).
23. **Loss confirmation is a precondition** for every destructive note type operation (draft save, Delete Note Type, Restore to Default, Change Note Type): each op takes the losses the user confirmed and refuses with `LossesChanged` when live losses are larger, so content is never lost beyond what was confirmed (`notetypes/draft-save` D8; decided 2026-11-20 by Claude (coordinator) under Antoine's delegation).
24. **A heatmap day with no answers** is not a link (the card-ids item refuses an empty list).

## 7. Open points for the owner

- The five open points of the first version of this plan (decisions 2, 12, 16, 18 and 19) were decided by Claude (coordinator) under Antoine's delegation of 2026-11-20 and recorded in `decisions.md` (row `browser`, "Five points the specs raised"), as earlier delegated choices are (the rows that say "decided by Claude at Antoine's request").
- None open. The spec lines of section 8 were approved by Antoine on 2026-11-20.

## 8. Spec lines approved 2026-11-20

New files: `specs/search/filter-tokens.md`, `specs/browser/workspace.md`, `specs/notetypes/workspace.md`, `specs/notetypes/draft-save.md`. New sections: `sync/meguri-provider` MP7; `docs/design/native-interface-contract.md` N21, N22 and the note at the top of N11.10. Changed lines (each beginning "Changed 2026-11-20 (round 8)" or "(round 9)", each file's Status line naming them): `search/join-and-replace` J3, `search/editing-services` D2, D3, D6, `browser/sidebar` S2, S7, S9, `browser/bulk-actions` B3, B5, B11, B16, `browser/find-replace` F2, F8, F10, `browser/config-keys` K4, K6.2, `browser/columns` C5, C7.4, `browser/inspector` I8, `browser/entry-points` E2, E5, `browser/result-sets` H7, `browser/sorting` S1, `notetypes/editor-actions` A3, A6, A7, A10, A12, `notetypes/schema-changes` G4, `render/preview` V12, `stats/card-info` C7. The inventory entries `browse.*`, `cardInfo.card` and `noteTypes.*` point to these specs.

## 9. Progress (changed 2026-11-21, implementation review)

Merged into `master`: U82 (PR #25, names and ids; PR #26, state, flags, properties and history), U81 (PR #30), U88a (PR #23), U84 (PR #34), U34 part 1 (PR #28: the structure draft apply op, S1 to S11 and S13) and U34 part 2 (PR #31: the confirmed-losses precondition, S12 and Restore to Default).

Assigned to U260, in addition to its row in section 4: the builder follow-ups of PR #32 (`search/join-and-replace` J3.6's last bullet): refuse a negative note type id and a negative duplicates note type id (`negative_id`), rename `day_out_of_range` to `due_day_out_of_range`, add the refused item's path to `invalid_item`, and the matching builder tests and T16 row.

Code changes that the 2026-11-21 spec follow-ups need, for the coordinator to assign:

- U34 fixer: `notetypes/structure-changes` S1.2's whole order. In `core/crates/meguri-engine/src/notetypes/structure/plan.rs` `plan_draft`, move the two origin range checks after the planning snapshot's digest comparison, and move the sort-field position check up beside `check_unique`, before the note type is read, so that the draft-only checks come first. Test in `core/crates/meguri-engine/tests/notetype_structure.rs`: a draft loaded from a 3-field note type with origin 2, then a field removed by another write, returns `Stale` (not `InvalidArgument`), writes nothing and takes no backup; the existing stale-base out-of-range case changes to expect `Stale`; a fresh draft with an out-of-range origin still returns `InvalidArgument`; a draft whose note type was removed and whose sort field is past its field list returns `InvalidArgument`, not `Stale`.
- U81 follow-up: `search/text-matching` T7.1's line on a note whose note type is missing: when some note type has an excluded field, such a note matches no plain-text, `nc:`, `-sc:`, `re:` or `w:` term (today's plan tests it as a type without excluded fields; `sc:` already leaves it out).
- U85: build the search context's today and next-day-at with the day calculation alone (`sched/day-clock` D8.1); the conformance executor already does. Deck options' preset search and other searches inside an op or read that already took the day from the day clock keep that day (`search/query-model` Q8); no change there.

Tooling follow-up, queued: the copied-test-text check (`tools/mg similarity --search-data`) accepts only the two search roots; it should cover every conformance area.
