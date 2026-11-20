# Opening Browse and committing searches

Spec: browser/entry-points
Requirements: R1112, R1113, R1114; supports R48, R98, R216, R300, R319, R807, R815, R1092, R1102, R1106, R1109, R1110, R1116, R1153, R1322, R1417, R1615, R1728, R1733, R1782, R1786, R1792
Anki: 26.09.3 for the Anki rows of E2 to E9 (what Anki's desktop browser does when it opens, when it is handed a card or a search, when a search is committed, what it remembers and how long its field text can be); none for the rest (Meguri design)
Scenarios: conformance/scenarios/browser/entry/ (recorded, inactive: E11)
Status: Under review (R1232; round 8, decided 2026-11-20) for the lines of E2, E5 that begin "Changed 2026-11-20 (round 8)": do not implement them until approved. Every other section is in force and unchanged.

Which search Browse runs when it opens, what each entry point hands it, what a commit does to the field text and which text it runs, which searches the recent-search list records, and what Clear, Select All, Invert Selection and Select Notes do. The field-text rule is `search/join-and-replace` J2, the canonical text `search/canonical-form` (through `search/editing-services` D2), the built items `search/join-and-replace` J3 and J4, the runs, handles and selection mapping `browser/result-sets` H3, H11 and H12, the stored keys `browser/config-keys` K5, K6.1 and K6.4. The query pipeline, debounce and layout are U210's; the field, tokens and inline errors U214's; the suggestions U215's.

## E1. Terms

- **Open request.** What an entry point hands U210's `open(search:)` route on AppNavigation (the route the plan's U225 and `stats/browse-queries` Q5 call U150's `openBrowser(search:)`; it is one route): a **plain open** (no search), an **open with a search** (a text, or a built item of J3 already written by J4), or an **open from review** (the id of the card the review screen shows).
- **Field text.** The text in Browse's search field, always the search text (R1086). Text taken from the field gets J2 before anything reads it.
- **Commit.** A run that records a text in recent searches (E6). A commit shows its canonical text in the field and runs that text (E5), with one exception: Stats' joined text shows and runs as written, and its canonical form is what is recorded (E3 rule 5). A **live run** (R1106) runs the field text as typed, after J2, without changing the field and without recording; it is not a commit.
- **Shown search.** The text whose result the table shows: the text a commit ran, the default search as it ran (E2), Select Notes' text (E8), or a live run's text. "Results changed" Refresh (R1792) and the Cards/Notes toggle (R1116) run the shown search again, never the field text when it has changed since. Anki keeps its last run text the same way and reuses it when it refreshes or changes mode.
- **Recent searches.** The per-device list of committed texts (E6), which U215 offers in the empty field (R1102).
- **Result.** The handle Browse shows (`browser/result-sets` H6), with its mode, sort, selection, current row and scroll position.

## E2. Plain open (R1112)

Changed 2026-11-20 (round 8): Browse opens on the query it had while the app runs, and on first open on `defaultSearchText` read into tokens or the whole collection (`search/filter-tokens` T14); `deck:current` is not the default. Entry points with a search: `browser/workspace` V15.

1. **Browse already holds a result** of the open collection (shown before in this collection generation, `engine/lifecycle` L3): a plain open shows it as it was. Nothing runs; the search, mode, selection, current row and scroll position are kept. Selecting the Browse sidebar section or tab is a plain open, and so is ⌘F from Home or the deck page (R216), which also focuses the field and selects its text. Anki: opening its browser while the window exists only brings that window forward; Anki focuses its field on every open.
2. **Otherwise** Browse runs the default search:

   | `defaultSearchText` (`browser/config-keys` K6.4) | Runs | The field shows |
   |---|---|---|
   | set | the stored text, exactly as stored: untrimmed, not put in canonical form | the stored text |
   | not set | `deck:current` (the current-deck item of J3, `search/names-and-ids` N3.4) | nothing: the field is empty, with its placeholder |

3. The default search is not a commit: nothing is put in canonical form and nothing is recorded in recent searches (E6). Anki does the same.
4. The mode is the stored mode (`browserTableShowNotesMode`, K6.1); the sort is that mode's stored sort (K5.1).
5. The first row is selected and made current when the result has rows; nothing is selected when it has none (R1110's empty state). Anki selects the first row of a new result the same way (`browser/result-sets` H12's rule for no current index).
6. A stored default text that does not parse is shown in the field with its error inline as E5 step 4 shows a commit's; one whose run fails is shown with its run error inline as E5 step 3 shows a commit's (R1108). With no earlier result there is nothing to dim: the table area shows only the error. Nothing is recorded. Anki shows a warning and an empty table.
7. On `CollectionReplaced` Browse drops its result (U210); the next plain open is rule 2's.

## E3. Open with a search (R1114)

1. The text gets J2. A built item's text (J4) is already canonical and J2 changes nothing in it.
2. It is put in canonical form (`search/editing-services` D2).
3. **It parses.** The field shows the canonical text and that text runs as a commit (E5): it is recorded in recent searches (E6). When Browse holds a result, the selection and current row are mapped into the new one (R1154, `browser/result-sets` H12, same mode); otherwise E2 rule 5 applies. An entry point whose own spec sets the selection (the Selection column below) sets it instead, once the result is complete. The mode stays the stored mode, and the sort is that mode's stored sort, unless the entry point's spec writes another (`guiBrowse`'s `reorderCards`, `servers/ankiconnect-gui` G3). Anki records a search another window hands to its browser in its history as well, and keeps the browser's mode.
4. **It does not parse.** The field shows the text after J2, not in canonical form, with the inline error of E5 step 4; an earlier result stays, dimmed (R1108); nothing runs and nothing is recorded. Anki's callers hand its browser only valid searches: one that does not parse raises an error, Anki shows its generic error dialog with the parse message, and the field does not change (on a first open the browser window does not appear).
5. **Stats' joined text** (`stats/browse-queries` Q3, Q5) is shown in the field exactly as written and runs as written, so it is the shown search (E1). Decided 2026-10-27 (plan U210): it is still a commit, so recent searches record its canonical form, which U210 gets from `search/editing-services` D2 only for that purpose (E6). Anki shows and runs it as written and does not record it. A joined text that does not parse (none does: the Stats search has parsed before any item is clickable, and every joined text parses, `stats/browse-queries` Q2.3, Q6) is rule 4's case: it is shown as written with its error inline, nothing runs and nothing is recorded.
6. **AnkiConnect's `guiBrowse`** is checked in the core before Browse sees it (`servers/ankiconnect-gui` G2 rule 2, G3 check 7): a `query` that does not parse, or whose run fails, gets the error reply and Browse does not change. Without `query`, `guiBrowse` is a plain open (E2): Browse keeps the result it holds, or runs the default search when it holds none.

The entry points, what each hands Browse, and the spec that builds it:

| Entry point | Requirement | Search handed to Browse | Selection | Built by |
|---|---|---|---|---|
| a deck's ⋯ menu Browse (Home, deck page) | R48 | the deck-by-name item of the deck's full name; for a name J3.5 refuses, the deck-ids item of the deck and its descendants | rule 3 | `search/join-and-replace` J3, J3.5 |
| the session summary's Browse Missed Cards | R98 | the card-ids item of every card the summary lists as missed, in the summary's order (first miss first), deleted cards included (they match nothing); no item when the list is empty, so no link | rule 3 | J3 |
| the Home and deck-page heatmap, Show in Browser | R300, R1322 | the clicked day's search, unchanged (`prop:due=N`, `prop:rated=-N`) | rule 3 | `stats/heatmap-presentation` M14 |
| Stats' bars, cells and labels | R1417 | the joined text: rule 5 | rule 3 | `stats/browse-queries` Q3 |
| the editor's Show Duplicates | R807 | the note-ids item of the duplicates the last check found | rule 3 | `notes/field-check` K4 |
| a row of the Add history | R815 | the note-ids item of that note | rule 3 | J3 |
| an import result line's Show in Browse | R1615 | the note-ids item of the line's notes | rule 3 | `package/transfer-sheets` X2.5 |
| the Migaku Received page's Open in Browse | R1728 | the note-ids item of the row's live note | rule 3 | `servers/migaku-received-log` L15 |
| `meguri://browse?search=<search>` | R319 | the `search` query item's value, percent-decoded as UTF-8; `+` stays `+` | rule 3 | none: free text |
| Migaku `/create` `searchNote` | R1733 | `nid:` and the digits | the first row alone | `servers/ankiconnect-gui` G10 |
| AnkiConnect `guiBrowse` | R1782 | its `query` (rule 6) | rule 3 | `servers/ankiconnect-gui` G3 |
| AnkiConnect `guiEditNote` | R1786 | `nid:` and the note id | the first row alone, with the editor open on the note | `servers/ankiconnect-gui` G7 |
| the review screen | R1113 | none: E4 | E4 | |

- R48 quotes the deck search as `deck:"<full name>"`. Browse does not take that literal form: it opens with the deck-by-name item that J4 writes, and both deck menus (Home and the deck page) build the same item. J4 writes it with quotes only where `search/canonical-form` F4 needs them and with `\`, `*` and `_` escaped, so it finds that deck and its subdecks and no look-alike. Deck `Japanese Core` gives `"deck:Japanese Core"`, deck `a_b` gives `deck:a\_b`, deck `Lang::French` gives `deck:Lang::French`. The literal form would differ for such names: `deck:"a_b"` in canonical form is `deck:a_b`, whose `_` matches any one character.
- A `meguri://browse` link without a `search` item, or whose value is not valid percent-encoded UTF-8, is a plain open (E2). An empty value is the empty search: E5 step 3.

## E4. Open from review (R1113)

Anki: its browser, opened from the review screen, is handed the card the reviewer shows. It runs the default search (E2 rule 2), also when the browser was already open on another search, then selects that card's row alone (in notes mode, the row of the card's note), makes it current and scrolls to it. When the result does not hold the card, nothing is selected. Its search runs to the end before it looks, so nothing can happen in between.

Meguri:

1. Every open from the review screen while it shows a card (its question or its answer) is an open from review: ⌘F (R216), which also focuses the field and selects its text, the review ⋯ menu's Browse and any other entry from review.
2. Browse runs the default search as E2 rule 2, whether or not it holds a result.
3. It looks the card up with `index_of(handle, revision, card, id)` (`browser/result-sets` H12): in notes mode that gives the card's note row. While the search is in progress, a card not found in the final prefix is looked up again at each `Progress` and at `Complete`. Until it is found, nothing is selected and no row is current: E2 rule 5 does not apply.
4. **Found.** That row alone is selected, made current and scrolled into view.
5. **Not found once the result is complete** (deviation from Anki, R1113): Browse opens with the card-ids item of that card (`cid:<id>`) as E3 does, so the field shows `cid:<id>` and recent searches record it. Its one row is selected and made current. When the card was deleted meanwhile, the search finds nothing and Browse shows R1110's empty state.
6. **The default search fails.** When the stored default text does not parse, or its run fails (E2 rule 6), the result cannot hold the card: Browse goes straight to rule 5's `cid:<id>`, which replaces the error.
7. **The user acts first.** When, before rule 4 or rule 5 happens, the user changes the selection or the current row, edits the field, commits, toggles the mode or changes the sort, or another open request arrives, the lookup stops: rules 4 to 6 no longer apply, and the result follows what the user did. Scrolling does not stop it.
8. With no card shown (the congratulations screen, the session summary), an open from the review screen is a plain open (E2).

## E5. Commit (R1109)

Changed 2026-11-20 (round 8): a commit reads the text into tokens (`search/filter-tokens` T8); the field never shows canonical text.

What commits: Return in the field; accepting a suggestion (U215); a sidebar item or chip action (U216, U223); a token action (`search/editing-services` D4, U214); an open with a search (E3, E4 rule 5).

Before a commit changes the result, the inspector's pending save is flushed and waited for (`browser/inspector` I5). Anki saves its editor before every search it runs.

1. The field text gets J2 (a built text needs none).
2. It is put in canonical form (`search/editing-services` D2).
3. **It parses.** The field shows the canonical text, the canonical text runs and becomes the shown search (E1), and it is recorded (E6). Anki runs the canonical text, not the text as typed, and so does Meguri, byte for byte (R1076):
   - An empty field, or one of white space only, has the canonical text `deck:*` (`search/canonical-form` F2): the field then shows `deck:*`, `deck:*` runs and `deck:*` is recorded. `deck:*` is a deck term, true for every card (`search/names-and-ids` N3.2); it differs from the empty search only on a damaged collection, where in notes mode it leaves out notes without cards (`search/query-model` Q2, Q6). A live run of the empty field runs the empty search itself.
   - The shapes whose canonical text does not round-trip (`search/canonical-form` F7) run their canonical text too: committing `has\-cd:x` shows and runs `has-cd:x`, a custom-data term; a live run of `has\-cd:x` runs the field term.
   - A run that then fails with a run error (`search/errors` E3) keeps the canonical text in the field, shows the error inline (R1108) and leaves the text recorded.
4. **It does not parse.** Nothing runs and nothing is recorded. The field keeps the text as typed, the error's message shows inline with its range underlined, and the earlier result stays, dimmed, under "Results unchanged until the search is valid" (R1108, `search/errors` E5, E7). Anki keeps the text and shows its warning.
5. A commit keeps the mode; its selection and current row are mapped from the earlier result (R1154, `browser/result-sets` H12).

## E6. Recent searches (R1102, R1109)

1. The list is newest first and holds at most 30 texts.
2. Recording a text t removes every entry equal to t (code point for code point), puts t first and drops the entries after the 30th. Anki keeps its search history the same way (30 entries, newest first, an equal entry moved to the front).
3. What is recorded is the committed canonical text: E3 rule 3, E4 rule 5, E5 step 3; for Stats' joined text its canonical form (E3 rule 5). A Stats text that does not parse neither runs nor records anything (E3 rules 4 and 5).
4. Nothing else is recorded: not the default search (E2), not a live run, not Clear (E7), not Select Notes (E8), not a commit or open that does not parse, not a `guiBrowse` refused by E3 rule 6.
5. The list stays on the device in app-private storage, never in the collection and never synced (`browser/config-keys` K11). It survives relaunch and a change of collection. It is the only search history Browse keeps; the accepted-suggestion list of `search/editing-services` D6.1 (`recent`) is a separate device list that only ranks suggestions.
6. Anki records only Return in its field and searches other windows hand to its browser. Its sidebar clicks, its Select Notes, its default search, its Stats links and its duplicate finder's links run without being recorded. Meguri also records sidebar, chip, suggestion and token commits (R1109) and Stats' canonical text (E3 rule 5).

## E7. Clear and the empty state's action (R1092, R1110)

- The field's clear button, and Esc in the search field when no suggestion list is open (Esc first dismisses an open list), empty the field and run the default search as E2 rule 2 does (rule 1 never applies): with `defaultSearchText` set the field then shows the stored text, otherwise it stays empty with its placeholder and `deck:current` runs. Nothing is recorded. The mode is kept; the selection and current row are mapped from the earlier result (R1154).
- The empty state's action (R1110) does the same. R1110 quotes its text as "Clear search" and the screen contract writes "Clear Search"; which case stands is the open title-case question of `browser/browse-strings` W7, and until it is settled R1110's text stands (W6).
- Anki's field has no clear button, and Esc closes its browser window (R1092 deviation).

## E8. Select All, Invert Selection and Select Notes (R1153)

1. **Select All** selects every row of the result; the current row stays as it was (row 0 becomes current when there was none). In a result still in progress, rows that arrive later are selected too, until the user changes the selection. Anki selects every row and keeps its current row.
2. **Invert Selection** selects exactly the rows that were not selected; the current row stays as it was, also when it is now unselected. In a result still in progress it inverts over the rows received so far, and later rows stay unselected. Anki does the same over its whole result.
3. **Select Notes.** The note ids come from `resolve_selection(handle, revision, ranges, notes)` (`browser/result-sets` H12): in cards mode the distinct notes in the order of their first selected card, in notes mode the selected notes in index order.
4. Browse runs the note-ids item of those ids (`nid:`, J3) in the current mode, the field shows its text (J4), and that text is the shown search (E1).
5. Every row of the result is selected and the first row is current.
6. It is not recorded in recent searches. Anki runs the same search, selects every row, and does not record it in its history.
7. Nothing is selected: Select Notes is disabled (U218). An empty table: Select All and Invert Selection are disabled (`browser/command-map`).

## E9. The field's length

Anki's search field holds at most 2,000,000 UTF-16 code units: typing stops there, and a paste is cut to the room left, the limit less the length of the text it does not replace. A text Anki's browser writes into the field itself is cut at the limit too: its default search and Select Notes still run the whole text, while a search another window hands it runs what is left in the field. Meguri:

- Typing and pasting follow Anki's rule: at the limit typing adds nothing, and a paste is cut to the room left (nothing when there is none). A cut never splits a surrogate pair: the text is cut before the pair's first unit.
- A text Meguri writes into the field (a commit's canonical text, an entry point's search, Select Notes' `nid:` list, a cleared field's default text) is shown whole, even beyond the limit, and runs whole; the field is never cut under the user. Select Notes on 150,000 notes (R1153's scale) writes about 2.1 million units. Over the limit, typing adds nothing and a paste inserts nothing until deletions bring the text under it.

## E10. Differences from Anki

| Difference | Why | Where |
|---|---|---|
| A plain open of a Browse that holds a result keeps it; Anki's browser window simply comes forward. Same outcome | Browse is a section of the single window | E2 rule 1 |
| From review, a card the default search does not hold becomes a `cid:` search; Anki selects nothing | R1113 | E4 rules 5 and 6 |
| Sidebar, chip, suggestion and token commits are put in canonical form and recorded; Anki's sidebar shows the built text and does not record it | R1109 | E5, E6 |
| Stats' joined text is recorded in canonical form; Anki does not record it | Decided 2026-10-27 (U210) | E3 rule 5, E6 |
| A failed parse or run of the default search, or of a commit, shows inline, with the earlier result dimmed; Anki shows a modal warning | R1108 | E2 rule 6, E5 |
| A handed search that does not parse shows inline in the field; Anki's callers never hand one, and one would raise its error dialog with the field unchanged | R1108, R1114 | E3 rule 4 |
| Clear and the empty state's action run the default search; Esc never leaves Browse | R1092, R1110 | E7 |
| The recent-search list is per device and survives a change of collection; Anki's history belongs to its profile | KTD12, R1102 | E6 |
| A text Meguri writes into the field is never cut at the length limit; Anki cuts what it shows, and then runs either the whole text or the cut one | The field and the run never differ | E9 |

None of these is observable by the oracle, which runs Anki's collection library and not its browser (`conformance/AGENTS.md`), so none needs a `conformance/deviations.yaml` entry.

## E11. Tests and scenarios

The oracle sees the texts and the results, never the browser's choices. The scenarios of `conformance/scenarios/browser/entry/` are recorded (their goldens are under `conformance/goldens/browser/entry/`, so the expected texts and results are pinned) and inactive: not yet listed in `browser/area.yaml`'s `active` list. `browser/entry/texts` becomes active with U88's `search_build` adapter (`specs/conformance/search-ops.md` S6); `browser/entry/default-and-empty` with U82's deck terms (`deck:current`, `deck:*`), which Meguri's search plan does not yet match:

| Scenario | Covers | Observations |
|---|---|---|
| `browser/entry/texts` | E3's texts in canonical form (the deck item for R48 with a space, a `_` and a level, next to R48's literal quoted form; a missed-cards `cid:` list kept in its order; the heatmap's `prop:due=0` and `prop:rated=-3`; the note-ids items of the Add history, an import line, the Received page and Migaku; a deep link's free text), E5 step 3's `deck:*` for an empty and a white-space field and its `has-cd:x` for `has\-cd:x`, E4 rule 5's `cid:<id>` | `canonical_search`, `search_build` (`specs/conformance/search-ops.md` S2, S6) |
| `browser/entry/default-and-empty` | E2 rule 2 and E5 step 3: the default search `deck:current` finds the current deck and its subdecks, while the committed empty search `deck:*` and the empty text find every card, one whose deck no longer exists included; in notes mode the empty text also finds a note without cards and `deck:*` does not; a stored default text with spaces around it runs as stored and finds what the trimmed text finds | `search` (S1) |

Meguri's tests cover the rest from this text: U210's `BrowseEntryTests` (E2 to E4, E7, every row of E3's table, E4 rules 3, 6 and 7 with a streamed result), `QueryPipelineTests` (E5: the canonical text runs, Refresh and the toggle run the shown search) and `RecentSearchesStoreTests` (E6: the 30-entry cut, moving an equal entry to the front, nothing recorded for the default search, Clear, Select Notes, a live run or a parse error); U212's `SelectionModelTests` (E8); U214's `SearchFieldUITests` (E9's cuts and the whole text Meguri writes, E5 step 3's `deck:*`).
