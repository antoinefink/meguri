# Opening Browse and committing searches (U210, U212, U214)

Spec-writer notes for `specs/browser/entry-points.md` (session
s-20261110T085429Z-165d83d3). Read at tag 26.09.3 (`refs/anki-26.09.3`); no
code was copied. The oracle was not run: it was busy, and every rule here
lives in Anki's desktop browser, which the oracle does not run
(`conformance/AGENTS.md`). The two draft scenarios of E11 observe only texts
and results, through the existing `canonical_search`, `search_build` and
`search` kinds.

| Spec section | Anki files read | Read for |
|---|---|---|
| E2 rules 1 to 5 | `qt/aqt/browser/browser.py`, `qt/aqt/__init__.py`, `qt/aqt/browser/table/table.py` | a second open of an existing browser window only raises it unless a card or a search is passed; the default search: the stored text when its trimmed form is not empty, shown in the field, else the current-deck search with an empty field and the placeholder; the default search runs without normalization and without a history entry; the first row selected after a search with no earlier selection |
| E1 (shown search) | `qt/aqt/browser/browser.py`, `qt/aqt/browser/table/table.py` | the browser keeps the last run text apart from the field and reuses it when it refreshes or changes mode |
| E2 rule 6 | `qt/aqt/browser/browser.py` | a failing run shows a warning |
| E3 rules 1 to 3 | `qt/aqt/browser/browser.py`, `qt/aqt/addcards_legacy.py`, `qt/aqt/mediasrv.py`, `qt/aqt/emptycards.py`, `qt/aqt/mediacheck.py`, `qt/aqt/filtered_deck.py` | a search handed by another window is normalized, put in the field and committed like Return, so it enters the history; the mode is unchanged; the selection is intersected with the earlier one |
| E3 rule 4 | `qt/aqt/browser/browser.py`, `qt/aqt/errors.py` | the window's constructor documents that callers must hand it a valid search; the first normalization of a handed search runs before the field is set and outside any error handling, so a parse error reaches the generic error dialog, the field is not set, and on a first open the window is not shown (it is shown only after the search is set up) |
| E3 rule 5 | `qt/aqt/stats.py` | Stats opens the browser and runs its text as given, without normalization or history |
| E3 table, R807 row | `qt/aqt/editor_legacy.py` | Anki opens a duplicate search on the raw first field (Meguri's row follows `notes/field-check` K4 instead) |
| E4 | `qt/aqt/main.py`, `qt/aqt/browser/browser.py`, `qt/aqt/browser/table/table.py`, `qt/aqt/browser/table/state.py`, `qt/aqt/reviewer.py` | the main window's Browse passes the reviewer's card (none outside review); the default search runs again on a second open with a card; the card's row (its note's row in notes mode) is selected alone and scrolled to; a card not in the result leaves nothing selected |
| E5 | `qt/aqt/browser/browser.py`, `rslib/src/search/writer.rs`, `rslib/src/search/parser.rs` | Return replaces white space, normalizes, puts the normalized text in the field, runs that normalized text and records it, also when the run then fails; a normalization error shows a warning, keeps the text and records nothing; the empty field normalizes to `deck:*`, which reads back as a deck term, not as the whole-collection term; the editor is saved before every search the browser runs |
| E6 | `qt/aqt/browser/browser.py`, `qt/aqt/browser/sidebar/tree.py`, `qt/aqt/browser/find_duplicates.py` | the history: newest first, an equal entry moved to the front, cut at 30, kept in the profile; sidebar clicks, the duplicate finder's links and Select Notes run without recording |
| E7 | `qt/aqt/browser/browser.py` | Esc closes the browser window; no clear button |
| E8 | `qt/aqt/browser/browser.py`, `qt/aqt/browser/table/table.py`, `qt/aqt/browser/table/state.py` | Select All selects every row and leaves the current row; Invert Selection selects all, then deselects the earlier selection, leaving the current row; Select Notes: the selected notes' id search, run without recording, then every row selected |
| E9 | `qt/aqt/browser/browser.py` | the search line's maximum length; text the browser sets is cut by the line edit's limit, while the default search and Select Notes keep the whole text as the run text and a handed search is read back from the cut field |
| E10 | `ftl/core/browsing.ftl` | the window title's selected-of-total strings and the placeholder hint, for comparison only (Meguri's wording is `browser/browse-strings` W6) |

Meguri's own design, with no Anki source: E2 rule 1's kept state inside the
single window, E3's table of Meguri entry points (deck menu, session summary,
heatmap, import result, Received page, deep link, Migaku, AnkiConnect) and its
Selection column, E3 rule 6, the deep link's decoding, E4 rules 1, 3 and 5 to
8 (Anki's search is synchronous, so the streaming cases have no Anki
counterpart), E6 rules 5 and 6's per-device list and Meguri's extra commits,
E7's clear button, E8's behavior on a result still in progress, E9's
surrogate-pair rule and its whole Meguri-written text.

Review pass (same session): the findings of the independent review were
checked against the files above at the tag; the Anki comparison of E3 rule 4
and the text a commit runs (E5 step 3) were corrected from that reading.
