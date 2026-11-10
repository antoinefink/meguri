# Browse strings (U210 to U223)

Spec-writer notes for `specs/browser/browse-strings.md`. Read at tag 26.09.3
(`refs/anki-26.09.3`); no code was copied. Written 2026-11-10 by checking every
Product Contract requirement of U210 to U223 for text that must be Anki's exact
English, then each candidate against Anki's English translation files. The
English values of W2 to W4's labels are Anki's interface wording; Anki's string
ids are not used as keys (the keys are Meguri's).

Meguri's own design, with no Anki source: the keys (W1), the error texts and
their selection rule (W2.1; the reason rule reuses `search/text-matching` T16),
the index of strings other files list (W5, which follows
`review/reviewer-strings` S5 for undo titles), the list of Meguri-worded or
requirement-fixed text (W6), the open title-case question (W7) and the tests
(W8). W2's "All fields" is R1200's text, not Anki's ("All Fields"); W6's
"Restore default name" is R899's.

| Spec section | Anki files read | Read for |
|---|---|---|
| browse-strings W2 | `qt/aqt/forms/findreplace.ui`, `qt/aqt/browser/find_and_replace.py`, `ftl/core/browsing.ftl`, `ftl/core/editing.ftl`, `ftl/core/findreplace.ftl` | the In menu's first two entries (Anki title-cases the first) and the three checkbox labels, their order in the dialog, the bold and colon form of Anki's control labels |
| browse-strings W2.1 | `qt/aqt/browser/find_and_replace.py` | that Anki shows only the backend's error text, after Apply, in a warning (no Meguri wording comes from Anki) |
| browse-strings W3 | `qt/aqt/forms/forget.ui`, `ftl/core/scheduling.ftl`, `ftl/core/actions.ftl` | the two Reset toggles' labels and order |
| browse-strings W4 | `ftl/core/studying.ftl`, `qt/aqt/browser/browser.py` (the Grade Now dialog) | the four rating names; the dialog's fixed order Again to Easy in one horizontal row, one button per rating with the profile's answer key for that rating as its shortcut and a shortcut-key tooltip, and a Cancel button (the tooltip and Cancel text are Meguri's wording in W4; "keys 1 to 4 always work" is U219's, not Anki's) |
| browse-strings W5 | `rslib/src/ops.rs` (operation names), `ftl/core/browsing.ftl`, `ftl/core/actions.ftl`, `ftl/core/card-stats.ftl` | that the undo names, column labels, flag names and Card Info labels already listed elsewhere are Anki's |
| browse-strings W6 | `ftl/core/browsing.ftl` (sidebar section names and items, compact row chips, result messages), `ftl/core/findreplace.ftl` (the selected-notes result), `ftl/core/preferences.ftl`, `ftl/core/scheduling.ftl` (Set Due Date prompt) | which requirement-quoted texts are also Anki's English (the sidebar section names, "Now", "N of M notes updated", "N notes updated" with Anki's period) and where the requirements' wording differs from Anki's (the Today items, "+N more tags", "N notes deleted") |
